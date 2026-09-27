//! SSH 演算法順序（B4，`CLAUDE.md` 風險 3 的核心）。
//!
//! **順序照 PuTTY**：先強後弱，而且 PuTTY 有一條「warn below this line」——
//! 線以下的演算法仍然在清單裡（舊設備連得上），但實際協商到它們時會跳警告。
//! 我們照同一個語意：[`is_weak`] 判斷線以下，`ssh/mod.rs` 的 `kex_done` 負責跳對話框。
//!
//! ## 為什麼不是直接用 russh 的預設
//! `Preferred::DEFAULT` 是「安全的那一組」，**完全沒有** SHA-1 系列與 CBC，
//! 所以舊設備會在交握就失敗。這正是風險 3 要解的。
//!
//! ## russh 缺哪些 PuTTY 有的演算法
//! 這些**不是被我們拿掉的，是 russh 沒有實作**（要在文件裡講清楚，不能靜默跳過）：
//!
//! | 類別 | PuTTY 有、russh 沒有 | 影響 |
//! |---|---|---|
//! | kex | NTRU Prime hybrid（`ntru-curve25519-sha512@tinyssh.org`） | 沒差，我們有 `mlkem768x25519-sha256`（後量子的標準版） |
//! | hostkey | **Ed448**、**`ssh-dss`（DSA）** | DSA 是很舊的設備才會只支援它 → **這種設備會連不上**。russh 有 `dsa` feature，要開才有（見下） |
//! | cipher | Blowfish、單 DES（`des-cbc`）、Arcfour | 只有 20 年前的設備才需要，PuTTY 也把它們放在警告線下 |
//! | MAC | `hmac-md5`、`hmac-sha1-96` | 少數舊設備會只有這兩個 → 可能連不上 |
//!
//! `ssh-dss` 的取捨：russh 的 `dsa` feature 可以開，但 DSA（1024-bit）已經被
//! OpenSSH 8.8 完全移除。**先不開**，等使用者的設備清單真的出現只支援 DSA 的機器再說
//! （只要在 `Cargo.toml` 的 russh features 加 `"dsa"`，再把 `Algorithm::Dsa` 排進最後）。

use crate::i18n::{t};
use std::borrow::Cow;

use russh::keys::{Algorithm, EcdsaCurve, HashAlg};
use russh::{cipher, kex, mac, Preferred};

/// 使用者對某條連線的演算法覆寫。空的（或整個 `None`）＝用預設順序。
///
/// 存在連線設定裡（`settings.json`），所以是字串而不是 russh 的型別。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AlgoOverride {
    pub kex: Vec<String>,
    pub host_key: Vec<String>,
    pub cipher: Vec<String>,
    pub mac: Vec<String>,
}

impl AlgoOverride {
    pub fn is_empty(&self) -> bool {
        self.kex.is_empty()
            && self.host_key.is_empty()
            && self.cipher.is_empty()
            && self.mac.is_empty()
    }
}

// ------------------------------------------------------------------ 預設順序

/// kex，強→弱。最後三個在 PuTTY 的警告線以下。
pub const KEX_ORDER: &[kex::Name] = &[
    kex::MLKEM768X25519_SHA256, // 後量子 hybrid（PuTTY 用 NTRU，語意相同）
    kex::CURVE25519,
    kex::CURVE25519_PRE_RFC_8731,
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
    kex::DH_G18_SHA512,
    kex::DH_G17_SHA512,
    kex::DH_G16_SHA512,
    kex::DH_G15_SHA512,
    kex::DH_GEX_SHA256,
    kex::DH_G14_SHA256,
    // ---------------- 以下是 PuTTY 的「warn below here」----------------
    kex::DH_G14_SHA1,
    kex::DH_GEX_SHA1,
    kex::DH_G1_SHA1,
];

/// 主機金鑰演算法，強→弱。最後一個（`ssh-rsa`＝RSA+SHA-1）在警告線以下。
pub fn host_key_order() -> Vec<Algorithm> {
    vec![
        Algorithm::Ed25519,
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP256,
        },
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP384,
        },
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP521,
        },
        Algorithm::Rsa {
            hash: Some(HashAlg::Sha512),
        },
        Algorithm::Rsa {
            hash: Some(HashAlg::Sha256),
        },
        // ---------------- warn below here ----------------
        // `ssh-rsa`：RSA 搭 SHA-1 簽章。很多舊網路設備只有這個。
        Algorithm::Rsa { hash: None },
    ]
}

/// cipher，強→弱。CBC 與 3DES 在警告線以下。
///
/// ⚠️ **與 PuTTY 的差異**：PuTTY 的預設清單把 3DES 放在警告線**之上**（歷史原因）。
/// 我們把所有 CBC 與 3DES 都放在線下——CBC 模式在 SSH 上有已知的攻擊面，
/// 而 3DES 的 64-bit 區塊也早就不該當預設。舊設備照樣連得上，只是會看到一次警告。
pub const CIPHER_ORDER: &[cipher::Name] = &[
    cipher::CHACHA20_POLY1305,
    cipher::AES_256_GCM,
    cipher::AES_128_GCM,
    cipher::AES_256_CTR,
    cipher::AES_192_CTR,
    cipher::AES_128_CTR,
    // ---------------- warn below here ----------------
    cipher::AES_256_CBC,
    cipher::AES_192_CBC,
    cipher::AES_128_CBC,
    cipher::TRIPLE_DES_CBC,
];

/// MAC，強→弱。SHA-1 系列在警告線以下。
///
/// ETM（encrypt-then-MAC）排在同演算法的非 ETM 之前，同 PuTTY 與 OpenSSH。
pub const MAC_ORDER: &[mac::Name] = &[
    mac::HMAC_SHA256_ETM,
    mac::HMAC_SHA512_ETM,
    mac::HMAC_SHA256,
    mac::HMAC_SHA512,
    // ---------------- warn below here ----------------
    mac::HMAC_SHA1_ETM,
    mac::HMAC_SHA1,
];

/// 警告線以下的名稱（協商到這些就跳 PuTTY 式的警告）。
const WEAK_KEX: &[&str] = &[
    "diffie-hellman-group14-sha1",
    "diffie-hellman-group-exchange-sha1",
    "diffie-hellman-group1-sha1",
];
const WEAK_HOST_KEY: &[&str] = &["ssh-rsa", "ssh-dss"];
const WEAK_CIPHER: &[&str] = &[
    "aes256-cbc",
    "aes192-cbc",
    "aes128-cbc",
    "3des-cbc",
    "des-cbc",
    "blowfish-cbc",
    "arcfour",
    "arcfour128",
    "arcfour256",
];
const WEAK_MAC: &[&str] = &["hmac-sha1", "hmac-sha1-etm@openssh.com", "hmac-sha1-96", "hmac-md5"];

/// 這四個名稱裡有沒有在警告線以下的？回傳「哪幾個」（給對話框列出來）。
pub fn weak_ones(kex: &str, host_key: &str, cipher: &str, mac: &str) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if WEAK_KEX.contains(&kex) {
        out.push((t("algo.kex"), kex.to_string()));
    }
    if WEAK_HOST_KEY.contains(&host_key) {
        out.push((t("algo.hostkey"), host_key.to_string()));
    }
    if WEAK_CIPHER.contains(&cipher) {
        out.push((t("algo.cipher"), cipher.to_string()));
    }
    if WEAK_MAC.contains(&mac) {
        out.push((t("algo.mac"), mac.to_string()));
    }
    out
}

/// 依使用者的覆寫組出 `Preferred`。覆寫是空的就用上面的預設順序。
///
/// 覆寫的名稱**認不出來就跳過**（設定檔可能是手改的、或從新版降級回來），
/// 跳過的名稱會回在第二個回傳值裡，呼叫端可以印出來。
pub fn preferred(ov: &AlgoOverride) -> (Preferred, Vec<String>) {
    let mut unknown = Vec::new();

    let kex: Vec<kex::Name> = if ov.kex.is_empty() {
        KEX_ORDER.to_vec()
    } else {
        pick(&ov.kex, KEX_ORDER, |n| n.as_ref(), &mut unknown)
    };
    let cipher: Vec<cipher::Name> = if ov.cipher.is_empty() {
        CIPHER_ORDER.to_vec()
    } else {
        pick(&ov.cipher, CIPHER_ORDER, |n| n.as_ref(), &mut unknown)
    };
    let mac: Vec<mac::Name> = if ov.mac.is_empty() {
        MAC_ORDER.to_vec()
    } else {
        pick(&ov.mac, MAC_ORDER, |n| n.as_ref(), &mut unknown)
    };
    let all_keys = host_key_order();
    let key: Vec<Algorithm> = if ov.host_key.is_empty() {
        all_keys
    } else {
        let mut out = Vec::new();
        for want in &ov.host_key {
            match all_keys.iter().find(|a| a.as_str() == want.as_str()) {
                Some(a) => out.push(a.clone()),
                None => unknown.push(want.clone()),
            }
        }
        if out.is_empty() {
            host_key_order()
        } else {
            out
        }
    };

    (
        Preferred {
            kex: Cow::Owned(kex),
            key: Cow::Owned(key),
            cipher: Cow::Owned(cipher),
            mac: Cow::Owned(mac),
            ..Preferred::DEFAULT
        },
        unknown,
    )
}

/// 從 `known` 裡挑出 `want` 列到的（保留 `want` 的順序）；認不出來的記進 `unknown`。
/// 一個都沒挑到就退回完整的 `known`——空清單會讓交握直接失敗，那不是使用者想要的。
fn pick<T: Clone>(
    want: &[String],
    known: &[T],
    name_of: impl Fn(&T) -> &str,
    unknown: &mut Vec<String>,
) -> Vec<T> {
    let mut out = Vec::new();
    for w in want {
        match known.iter().find(|k| name_of(k) == w.as_str()) {
            Some(k) => out.push(k.clone()),
            None => unknown.push(w.clone()),
        }
    }
    if out.is_empty() {
        known.to_vec()
    } else {
        out
    }
}

/// 給「進階」對話框列出可選的名稱（四組，順序就是預設順序；`weak` 標出警告線以下）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlgoCatalog {
    pub kex: Vec<AlgoItem>,
    pub host_key: Vec<AlgoItem>,
    pub cipher: Vec<AlgoItem>,
    pub mac: Vec<AlgoItem>,
}

#[derive(serde::Serialize)]
pub struct AlgoItem {
    pub name: String,
    pub weak: bool,
}

/// 給 B6 的「進階」對話框用：四組可選名稱 + 哪些在警告線下。
#[tauri::command]
pub fn algo_catalog() -> AlgoCatalog {
    catalog()
}

pub fn catalog() -> AlgoCatalog {
    fn items<'a>(names: impl Iterator<Item = &'a str>, weak: &[&str]) -> Vec<AlgoItem> {
        names
            .map(|n| AlgoItem {
                name: n.to_string(),
                weak: weak.contains(&n),
            })
            .collect()
    }
    AlgoCatalog {
        kex: items(KEX_ORDER.iter().map(|n| n.as_ref()), WEAK_KEX),
        host_key: items(
            host_key_order().iter().map(|a| a.as_str()).collect::<Vec<_>>().into_iter(),
            WEAK_HOST_KEY,
        ),
        cipher: items(CIPHER_ORDER.iter().map(|n| n.as_ref()), WEAK_CIPHER),
        mac: items(MAC_ORDER.iter().map(|n| n.as_ref()), WEAK_MAC),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 清單裡的名稱都必須是 russh 真的認得的，否則交握時會靜默少一個演算法。
    #[test]
    fn every_name_is_known_to_russh() {
        for n in KEX_ORDER {
            assert!(
                kex::ALL_KEX_ALGORITHMS.contains(&n),
                "russh 不認得 kex {n:?}"
            );
        }
        for n in CIPHER_ORDER {
            assert!(
                cipher::ALL_CIPHERS.contains(&n),
                "russh 不認得 cipher {n:?}"
            );
        }
        for n in MAC_ORDER {
            assert!(mac::ALL_MAC_ALGORITHMS.contains(&n), "russh 不認得 mac {n:?}");
        }
    }

    /// 舊設備要的那幾個一定要在清單裡（`CLAUDE.md` 風險 3 的 S2～S5）。
    #[test]
    fn legacy_algorithms_are_present_but_last() {
        let kex: Vec<&str> = KEX_ORDER.iter().map(|n| n.as_ref()).collect();
        for want in ["diffie-hellman-group14-sha1", "diffie-hellman-group-exchange-sha1", "diffie-hellman-group1-sha1"] {
            assert!(kex.contains(&want), "缺 {want}");
        }
        // 三個 SHA-1 kex 一定在最後三名
        assert_eq!(&kex[kex.len() - 3..], &["diffie-hellman-group14-sha1", "diffie-hellman-group-exchange-sha1", "diffie-hellman-group1-sha1"]);

        let ciphers: Vec<&str> = CIPHER_ORDER.iter().map(|n| n.as_ref()).collect();
        for want in ["aes128-cbc", "aes256-cbc", "3des-cbc"] {
            assert!(ciphers.contains(&want), "缺 {want}");
        }
        assert_eq!(ciphers.last(), Some(&"3des-cbc"));

        let macs: Vec<&str> = MAC_ORDER.iter().map(|n| n.as_ref()).collect();
        assert!(macs.contains(&"hmac-sha1"));
        assert_eq!(macs.last(), Some(&"hmac-sha1"));

        // ssh-rsa（RSA + SHA-1）要在，而且是最後一個
        let keys: Vec<String> = host_key_order().iter().map(|a| a.as_str().to_string()).collect();
        assert!(keys.contains(&"ssh-rsa".to_string()));
        assert_eq!(keys.last().map(|s| s.as_str()), Some("ssh-rsa"));
    }

    #[test]
    fn strong_algorithms_are_not_flagged_weak() {
        assert!(weak_ones("curve25519-sha256", "ssh-ed25519", "chacha20-poly1305@openssh.com", "hmac-sha2-256").is_empty());
    }

    #[test]
    fn legacy_combo_is_flagged_weak() {
        let w = weak_ones("diffie-hellman-group14-sha1", "ssh-rsa", "aes128-cbc", "hmac-sha1");
        assert_eq!(w.len(), 4, "四項都該被標出來：{w:?}");
        let kinds: Vec<&str> = w.iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, vec![t("algo.kex"), t("algo.hostkey"), t("algo.cipher"), t("algo.mac")]);
    }

    #[test]
    fn override_keeps_user_order_and_reports_unknown() {
        let ov = AlgoOverride {
            cipher: vec!["aes128-ctr".into(), "no-such-cipher".into()],
            ..Default::default()
        };
        let (p, unknown) = preferred(&ov);
        assert_eq!(p.cipher.len(), 1);
        assert_eq!(p.cipher[0].as_ref(), "aes128-ctr");
        assert_eq!(unknown, vec!["no-such-cipher".to_string()]);
        // 沒被覆寫的那幾組維持預設順序
        assert_eq!(p.kex.len(), KEX_ORDER.len());
    }

    #[test]
    fn override_that_matches_nothing_falls_back_to_default() {
        let ov = AlgoOverride {
            kex: vec!["totally-made-up".into()],
            ..Default::default()
        };
        let (p, unknown) = preferred(&ov);
        // 空清單會讓交握直接失敗，所以要退回預設
        assert_eq!(p.kex.len(), KEX_ORDER.len());
        assert_eq!(unknown.len(), 1);
    }

    #[test]
    fn catalog_marks_the_warn_line() {
        let c = catalog();
        assert!(c.kex.iter().any(|i| i.weak));
        assert!(c.kex.first().is_some_and(|i| !i.weak));
        assert!(c.kex.last().is_some_and(|i| i.weak));
        assert!(c.host_key.last().is_some_and(|i| i.weak && i.name == "ssh-rsa"));
    }
}
