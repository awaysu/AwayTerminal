//! 主機金鑰的記錄與確認（PuTTY 的「host key cache」）。
//!
//! ## PuTTY 怎麼做 / 我們怎麼做
//!
//! | | PuTTY | 這裡 |
//! |---|---|---|
//! | 存放位置 | Windows 登錄檔 `HKCU\Software\SimonTatham\PuTTY\SshHostKeys` | **檔案** `{app config dir}/known_hosts`（跨平台，人看得懂、能直接刪一行） |
//! | 格式 | `rsa2@22:host` = 金鑰參數 | **OpenSSH `known_hosts` 格式**（`host keytype base64` 或 `[host]:port …`） |
//! | 第一次連線 | 「The server's host key is not cached」+ 指紋 + 接受並儲存／只這次／取消 | 同語意（見 `HostKeyPrompt`） |
//! | 金鑰變更 | **更嚴重的警告**「POSSIBLE SECURITY BREACH!」 | 同語意，前端用不同樣式與更強的措辭 |
//! | 指紋 | SHA256（新版）／MD5（舊版） | **兩個都顯示** |
//!
//! 為什麼不自己定格式：OpenSSH 的 `known_hosts` 已經是「一行一台主機、可人工檢視與刪除」，
//! 而且 `russh` 自帶讀寫與「金鑰換了」的判斷（`check_known_hosts_path` 會回
//! `Error::KeyChanged { line }`），自創格式只會多一份要維護的解析器。
//!
//! ⚠️ 我們**不讀** `~/.ssh/known_hosts`：那是 OpenSSH 的檔，程式不該偷偷往裡面寫。
//! 檔案位置印在啟動 log 裡，使用者要清掉某一台就自己刪那一行。

use crate::i18n::{tf};
use std::path::{Path, PathBuf};

use russh::keys::ssh_key::{self, HashAlg};

/// 主機金鑰比對結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 記錄過而且一樣 → 直接連，不打擾使用者（PuTTY 同樣不問）。
    Known,
    /// 沒有記錄過（第一次連這台）。
    Unknown,
    /// **記錄過但金鑰不同** → PuTTY 的「POSSIBLE SECURITY BREACH」。
    Changed { line: usize },
}

/// 給使用者看的指紋（兩種都給，同 PuTTY 新舊版）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprints {
    /// 例 `ssh-ed25519`
    pub algorithm: String,
    /// 位元數（RSA 之類看得出強度）。取不到時為 0。
    pub bits: u32,
    /// 例 `SHA256:abc…`
    pub sha256: String,
    /// 例 `aa:bb:cc:…`（MD5，PuTTY 舊版與 OpenSSH `-E md5` 的格式）
    pub md5: String,
}

pub fn fingerprints(key: &ssh_key::PublicKey) -> Fingerprints {
    let sha256 = key.fingerprint(HashAlg::Sha256).to_string();
    let md5 = match key.to_bytes() {
        Ok(blob) => md5_hex(&blob),
        Err(_) => "?".to_string(),
    };
    Fingerprints {
        algorithm: key.algorithm().as_str().to_string(),
        bits: key_bits(key),
        sha256,
        md5,
    }
}

/// MD5 指紋：`aa:bb:cc:…`（對整個 SSH 線上格式的公鑰 blob 取 MD5，同 OpenSSH `-E md5`）。
fn md5_hex(blob: &[u8]) -> String {
    let digest = md5::compute(blob);
    digest
        .0
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn key_bits(key: &ssh_key::PublicKey) -> u32 {
    use ssh_key::public::KeyData;
    match key.key_data() {
        KeyData::Ed25519(_) => 256,
        // 用 ssh-key 算好的模數位元數（稽核 E11：`n().as_bytes()` 含 mpint 的前導 0x00，
        // 2048 位元會多報成 2056）
        KeyData::Rsa(rsa) => rsa.key_size(),
        KeyData::Ecdsa(e) => match e.curve() {
            ssh_key::EcdsaCurve::NistP256 => 256,
            ssh_key::EcdsaCurve::NistP384 => 384,
            ssh_key::EcdsaCurve::NistP521 => 521,
        },
        _ => 0,
    }
}

/// `known_hosts` 檔。
pub struct HostKeyStore {
    path: PathBuf,
}

impl HostKeyStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 比對一把主機金鑰。讀不到檔案（還沒有任何記錄）視為 `Unknown`。
    pub fn check(&self, host: &str, port: u16, key: &ssh_key::PublicKey) -> Verdict {
        if !self.path.exists() {
            return Verdict::Unknown;
        }
        match russh::keys::check_known_hosts_path(host, port, key, &self.path) {
            Ok(true) => Verdict::Known,
            Ok(false) => Verdict::Unknown,
            Err(russh::keys::Error::KeyChanged { line }) => Verdict::Changed { line },
            Err(e) => {
                // 檔案壞掉時**不可以**當成 Unknown 然後覆寫——那等於把警告吞掉。
                // 回 Changed 讓使用者看到最嚴重的那個對話框並自己去看檔案。
                println!("[AwayTerminal] known_hosts 讀取失敗（當成金鑰不符處理）：{e}");
                Verdict::Changed { line: 0 }
            }
        }
    }

    /// 記下一把主機金鑰。
    ///
    /// **同一台主機、同型別的舊記錄會先拿掉**（稽核 E2）：russh 的 `check_known_hosts_path`
    /// 只要有任何一行「同主機同型別但金鑰不同」就回 `KeyChanged`，第一版只 append，
    /// 金鑰變更後按「接受並儲存」等於沒存，每次連線（含自動重連）都再跳紅框。
    ///
    /// 寫法是「讀整個檔 → 濾掉舊行 → 加新行 → 寫暫存檔 → rename 蓋過去」，
    /// 寫到一半當掉也不會留下半個檔（原子寫入）。
    pub fn learn(&self, host: &str, port: u16, key: &ssh_key::PublicKey) -> Result<(), String> {
        // 兩個分頁同時按「接受並儲存」時不要互相蓋掉（也共用同一個暫存檔名）
        static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = WRITING.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| tf("err.mkdirFailed", &[&e.to_string()]))?;
        }
        // 格式就是 OpenSSH 的：非預設埠用 `[host]:port`（同 russh 讀取端的慣例）。
        let pattern = if port == 22 {
            host.to_string()
        } else {
            format!("[{host}]:{port}")
        };
        let line = key
            .to_openssh()
            .map_err(|e| tf("err.keySerialize", &[&e.to_string()]))?;

        let old = match std::fs::read(&self.path) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(tf("err.knownHostsOpen", &[&e.to_string()])),
        };

        // 哪幾行要拿掉：用 russh 自己的比對（含 `|1|` 雜湊主機名、逗號分隔的多主機），
        // 才能保證拿掉的正好是它會判成 KeyChanged 的那幾行。
        // ⚠️ russh 的行號**不數 `#` 開頭的註解行**（見 known_hosts.rs），下面照同一套規則數。
        let stale: Vec<usize> = if old.is_empty() {
            Vec::new()
        } else {
            russh::keys::known_hosts::known_host_keys_path(host, port, &self.path)
                .map_err(|e| tf("err.knownHostsOpen", &[&e.to_string()]))?
                .into_iter()
                .filter(|(_, recorded)| recorded.algorithm() == key.algorithm())
                .map(|(n, _)| n)
                .collect()
        };

        let mut out = String::with_capacity(old.len() + line.len() + pattern.len() + 2);
        let mut n = 1usize;
        for l in old.split_inclusive('\n') {
            if l.starts_with('#') {
                out.push_str(l);
                continue;
            }
            // 一行列了好幾台主機（`a,b ssh-ed25519 …`）時整行拿掉——那一行的金鑰已經不對了
            if !stale.contains(&n) {
                out.push_str(l);
            }
            n += 1;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("{pattern} {line}\n"));

        let tmp = self.path.with_extension(format!("tmp{}", std::process::id()));
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(out.as_bytes())?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp, &self.path)
        };
        write().map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            tf("err.knownHostsWrite", &[&e.to_string()])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 用固定的公鑰字串，不在測試裡產生金鑰：
    //   - `check`／`learn` 只需要公鑰，不需要私鑰；
    //   - `ssh-key` 的 `PrivateKey::random` 綁在它自己的 `rand_core` 版本上，
    //     外面塞任何 `rand` 都會對不上型別（實際踩過）。
    // 這兩把是 ssh-key 測試向量裡的 ed25519 公鑰，內容不同、長度相同。
    const KEY_A: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const KEY_B: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF";

    fn parse(text: &str) -> ssh_key::PublicKey {
        ssh_key::PublicKey::from_openssh(text).unwrap()
    }

    #[test]
    fn unknown_then_known_then_changed() {
        let dir = std::env::temp_dir().join(format!("awayterm-hostkey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = HostKeyStore::new(dir.join("known_hosts"));

        let ka = parse(KEY_A);
        let kb = parse(KEY_B);

        // 第一次：沒記錄
        assert_eq!(store.check("example.test", 22, &ka), Verdict::Unknown);

        // 記下來之後：一樣就通過
        store.learn("example.test", 22, &ka).unwrap();
        assert_eq!(store.check("example.test", 22, &ka), Verdict::Known);

        // 換一把同型別的金鑰：一定要被擋下來
        match store.check("example.test", 22, &kb) {
            Verdict::Changed { .. } => {}
            other => panic!("金鑰換了卻沒被擋下：{other:?}"),
        }

        // 非預設埠是獨立記錄（OpenSSH 的 [host]:port 慣例）
        assert_eq!(store.check("example.test", 2222, &ka), Verdict::Unknown);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 稽核 E2：金鑰變更後「接受並儲存」要真的生效——舊的同型別那行要拿掉，
    /// 別台主機、別的埠、註解行都要原樣留著。
    #[test]
    fn learn_replaces_changed_key() {
        let dir = std::env::temp_dir().join(format!("awayterm-hostkey-e2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("known_hosts");
        let store = HostKeyStore::new(path.clone());
        let ka = parse(KEY_A);
        let kb = parse(KEY_B);

        std::fs::write(
            &path,
            format!("# 使用者自己寫的註解\nother.test {KEY_A}\n[example.test]:2222 {KEY_A}\n"),
        )
        .unwrap();
        store.learn("example.test", 22, &ka).unwrap();
        assert_eq!(store.check("example.test", 22, &kb), Verdict::Changed { line: 3 });

        // 使用者在紅框按「接受並儲存」
        store.learn("example.test", 22, &kb).unwrap();
        assert_eq!(store.check("example.test", 22, &kb), Verdict::Known, "換過之後要直接通過");
        assert!(matches!(store.check("example.test", 22, &ka), Verdict::Changed { .. }));

        // 其他記錄不能被波及
        assert_eq!(store.check("other.test", 22, &ka), Verdict::Known);
        assert_eq!(store.check("example.test", 2222, &ka), Verdict::Known);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# 使用者自己寫的註解\n"), "{text}");
        assert_eq!(text.lines().filter(|l| l.starts_with("example.test ")).count(), 1, "{text}");
        // 暫存檔不能留下來
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .filter(|n| n != "known_hosts")
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 稽核 E11：2048 位元的 RSA 要顯示 2048（第一版多算 mpint 前導的 0x00 → 2056）。
    #[test]
    fn rsa_bits_are_exact() {
        // 最高位元是 1 的 2048 位元模數，mpint 編碼會多一個前導 0x00
        let mut n = vec![0xC5u8; 256];
        n[255] = 0x01;
        let rsa = ssh_key::public::RsaPublicKey::new(
            ssh_key::Mpint::from_positive_bytes(&[0x01, 0x00, 0x01]),
            ssh_key::Mpint::from_positive_bytes(&n),
        )
        .unwrap();
        assert_eq!(rsa.n().as_bytes().len(), 257, "前提：mpint 有前導 0x00");
        let key = ssh_key::PublicKey::from(rsa);
        assert_eq!(fingerprints(&key).bits, 2048);
    }

    #[test]
    fn fingerprints_have_both_forms() {
        let f = fingerprints(&parse(KEY_A));
        assert!(f.sha256.starts_with("SHA256:"), "{}", f.sha256);
        assert_eq!(f.md5.split(':').count(), 16, "MD5 應該是 16 組十六進位：{}", f.md5);
        assert_eq!(f.algorithm, "ssh-ed25519");
        assert_eq!(f.bits, 256);
    }
}
