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
        KeyData::Rsa(rsa) => (rsa.n().as_bytes().len() as u32) * 8,
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

    /// 記下一把主機金鑰（append 一行）。
    pub fn learn(&self, host: &str, port: u16, key: &ssh_key::PublicKey) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| tf("err.mkdirFailed", &[&e.to_string()]))?;
        }
        // `russh` 的 `learn_known_hosts_path` 沒有被 `pub use` 出來，所以自己寫這一行。
        // 格式就是 OpenSSH 的：非預設埠用 `[host]:port`（同 russh 讀取端的慣例）。
        let pattern = if port == 22 {
            host.to_string()
        } else {
            format!("[{host}]:{port}")
        };
        let line = key
            .to_openssh()
            .map_err(|e| tf("err.keySerialize", &[&e.to_string()]))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| tf("err.knownHostsOpen", &[&e.to_string()]))?;
        use std::io::Write;
        writeln!(file, "{pattern} {line}").map_err(|e| tf("err.knownHostsWrite", &[&e.to_string()]))
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

    #[test]
    fn fingerprints_have_both_forms() {
        let f = fingerprints(&parse(KEY_A));
        assert!(f.sha256.starts_with("SHA256:"), "{}", f.sha256);
        assert_eq!(f.md5.split(':').count(), 16, "MD5 應該是 16 組十六進位：{}", f.md5);
        assert_eq!(f.algorithm, "ssh-ed25519");
        assert_eq!(f.bits, 256);
    }
}
