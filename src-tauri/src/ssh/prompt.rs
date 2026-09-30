//! 主機金鑰對話框：Rust 的 SSH 任務 ↔ 前端。
//!
//! `russh` 的 `check_server_key` 是在連線交握中間被呼叫的，**必須當場給答案**，
//! 所以這裡的流程是：
//!
//! 1. SSH 任務呼叫 [`AppDecider::decide`]（回傳 future），它配一個 request id、
//!    把回覆用的 `oneshot` sender 放進 [`pending`]，然後 emit tauri event `ssh-hostkey`。
//! 2. 前端跳對話框（文案照 PuTTY：第一次連線／金鑰變更兩種），使用者按下去之後
//!    呼叫 `ssh_hostkey_answer` 指令。
//! 3. 指令把答案送進 sender，`decide` 的 future 醒來回傳。
//!
//! **逾時（或前端根本沒回）一律當成取消**——主機金鑰沒確認就不連，這是安全預設。
//!
//! ⚠️ **等答案的時候不可以佔住 tokio worker**（稽核 E1）：SSH runtime 只有 2 條 worker，
//! 第一版用 `std::sync::mpsc::recv_timeout` 同步等，兩個對話框同時開著（或對話框沒答就關分頁）
//! 就把兩條 worker 都卡住 → **所有**既有 SSH 分頁最多 3 分鐘沒有輸入輸出、keepalive 也不送。
//! 現在改成 `tokio::sync::oneshot` + `tokio::time::timeout`，等待期間 worker 照常跑別的連線；
//! 分頁關掉時 SSH 任務那邊會把這個 future 丟掉（見 `ssh::Handler` 的取消），
//! [`Ticket`] 的 `Drop` 順手把登記拿掉。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use super::hostkey::{Fingerprints, Verdict};
use super::{Asking, HostKeyAnswer, HostKeyDecider, WeakAnswer};

/// 等使用者按按鈕最多等這麼久。比人的反應時間寬鬆很多，只是不要讓連線永遠掛著。
const ANSWER_TIMEOUT: Duration = Duration::from_secs(180);

type Pending = Mutex<HashMap<u64, oneshot::Sender<HostKeyAnswer>>>;

fn pending() -> &'static Pending {
    static P: OnceLock<Pending> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// 一個等待中的詢問。**drop 時把登記拿掉**——不論是答了、逾時，還是 SSH 任務因為
/// 分頁關閉而把整個 future 丟掉，表裡都不會留下永遠沒人收的 sender。
struct Ticket {
    id: u64,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        pending()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

/// 登記 → emit → 等答案（不佔 worker）。emit 失敗、逾時、sender 被丟掉都回 `None`（＝取消）。
async fn ask<T: serde::Serialize + Clone>(
    app: &AppHandle,
    event: &str,
    make: impl FnOnce(u64) -> T,
) -> Option<HostKeyAnswer> {
    let id = next_id();
    let (tx, rx) = oneshot::channel();
    pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, tx);
    let _ticket = Ticket { id };

    if app.emit(event, make(id)).is_err() {
        return None;
    }
    match tokio::time::timeout(ANSWER_TIMEOUT, rx).await {
        Ok(Ok(answer)) => Some(answer),
        _ => None,
    }
}

/// 送給前端的內容。
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HostKeyRequest {
    id: u64,
    /// 哪一個分頁問的（前端要把對話框和分頁對起來）。
    tab_id: u32,
    host: String,
    port: u16,
    /// `unknown`＝第一次連線；`changed`＝**金鑰變了**（PuTTY 的 POSSIBLE SECURITY BREACH）。
    kind: &'static str,
    /// `changed` 時舊記錄在 known_hosts 的第幾行（0＝檔案讀不到）。
    line: usize,
    fingerprints: Fingerprints,
    /// known_hosts 檔案位置，讓對話框可以告訴使用者去哪裡刪。
    store_path: String,
}

pub struct AppDecider {
    app: AppHandle,
    tab_id: u32,
    store_path: String,
    settings: std::sync::Arc<crate::settings::SettingsStore>,
}

impl AppDecider {
    pub fn new(
        app: AppHandle,
        tab_id: u32,
        store_path: String,
        settings: std::sync::Arc<crate::settings::SettingsStore>,
    ) -> Self {
        Self {
            app,
            tab_id,
            store_path,
            settings,
        }
    }
}

impl HostKeyDecider for AppDecider {
    fn decide<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        verdict: &'a Verdict,
        fp: &'a Fingerprints,
    ) -> Asking<'a, HostKeyAnswer> {
        Box::pin(async move {
            let make = |id| HostKeyRequest {
                id,
                tab_id: self.tab_id,
                host: host.to_string(),
                port,
                kind: match verdict {
                    Verdict::Changed { .. } => "changed",
                    _ => "unknown",
                },
                line: match verdict {
                    Verdict::Changed { line } => *line,
                    _ => 0,
                },
                fingerprints: fp.clone(),
                store_path: self.store_path.clone(),
            };
            ask(&self.app, "ssh-hostkey", make).await.unwrap_or_else(|| {
                println!("[AwayTerminal] 主機金鑰確認逾時或視窗已關閉 → 當成取消");
                HostKeyAnswer::Reject
            })
        })
    }

    /// 協商到警告線以下的演算法。照 PuTTY：**同一台主機、同一組演算法接受過就不再問**。
    ///
    /// ⚠️ 這裡**不寫設定**（稽核 E9）：`kex_done` 比 `check_server_key` 早被呼叫，
    /// 這時還不知道對方是不是真的那台主機。要記住的話回 [`WeakAnswer::AcceptAndRemember`]，
    /// 由 SSH 任務在主機金鑰驗證通過後呼叫 [`HostKeyDecider::remember_weak`]。
    fn accept_weak<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        weak: &'a [(String, String)],
    ) -> Asking<'a, WeakAnswer> {
        Box::pin(async move {
            if weak_covered(&self.settings.get().ssh_weak_accepted, host, port, weak) {
                return WeakAnswer::Accept; // 之前接受過同一組（或更多）
            }

            let make = |id| WeakAlgoRequest {
                id,
                tab_id: self.tab_id,
                host: host.to_string(),
                port,
                items: weak.to_vec(),
            };
            // 前端只有「繼續／取消」兩個選項，借用同一個回覆通道：
            // AcceptAndStore＝繼續並記住；AcceptOnce＝只這次；Reject＝取消。
            match ask(&self.app, "ssh-weak-algo", make).await {
                Some(HostKeyAnswer::AcceptAndStore) => WeakAnswer::AcceptAndRemember,
                Some(HostKeyAnswer::AcceptOnce) => WeakAnswer::Accept,
                Some(HostKeyAnswer::Reject) => WeakAnswer::Reject,
                None => {
                    println!("[AwayTerminal] 弱演算法確認逾時或視窗已關閉 → 當成取消");
                    WeakAnswer::Reject
                }
            }
        })
    }

    fn remember_weak(&self, host: &str, port: u16, weak: &[(String, String)]) {
        self.settings
            .update(|s| remember_in(&mut s.ssh_weak_accepted, host, port, weak));
    }
}

// ------------------------------------------------------- 弱演算法「已接受」的記錄
//
// 稽核 E9：第一版只記 `host:port`，之後那台主機**換成更弱的演算法**（降級）也不會再問。
// 現在一筆記錄是 `host:port 演算法1,演算法2,…`（名稱排序、去重）：
// 這次協商到的弱演算法**全部**在某一筆記錄裡才算接受過。
// 舊格式（只有 `host:port`）對不到任何演算法 → 會再問一次，答了就換成新格式。

fn weak_prefix(host: &str, port: u16) -> String {
    format!("{host}:{port} ")
}

/// 這次的弱演算法是不是都被某一筆記錄涵蓋。
fn weak_covered(accepted: &[String], host: &str, port: u16, weak: &[(String, String)]) -> bool {
    let prefix = weak_prefix(host, port);
    accepted.iter().any(|k| {
        k.strip_prefix(&prefix).is_some_and(|rest| {
            let set: Vec<&str> = rest.split(',').collect();
            weak.iter().all(|(_, n)| set.contains(&n.as_str()))
        })
    })
}

/// 把這次接受的演算法併進那台主機的記錄（同一台主機只留一筆）。
fn remember_in(accepted: &mut Vec<String>, host: &str, port: u16, weak: &[(String, String)]) {
    let prefix = weak_prefix(host, port);
    let mut names: Vec<String> = weak.iter().map(|(_, n)| n.clone()).collect();
    accepted.retain(|k| match k.strip_prefix(&prefix) {
        Some(rest) => {
            names.extend(rest.split(',').filter(|s| !s.is_empty()).map(str::to_string));
            false
        }
        // 舊格式的 `host:port`（沒有演算法）也一併換掉
        None => k != prefix.trim_end(),
    });
    names.sort();
    names.dedup();
    accepted.push(format!("{prefix}{}", names.join(",")));
}

/// 弱演算法警告要送給前端的內容（PuTTY 的 warn-below-this-line）。
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WeakAlgoRequest {
    id: u64,
    tab_id: u32,
    host: String,
    port: u16,
    /// 每一項是（類別, 演算法名稱），例 ("加密", "aes128-cbc")。
    items: Vec<(String, String)>,
}

/// 前端按下三個按鈕之一時呼叫。
#[tauri::command]
pub fn ssh_hostkey_answer(id: u64, answer: HostKeyAnswer) {
    let tx = pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    if let Some(tx) = tx {
        let _ = tx.send(answer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(names: &[&str]) -> Vec<(String, String)> {
        names.iter().map(|n| ("x".to_string(), n.to_string())).collect()
    }

    #[test]
    fn weak_record_includes_algorithms() {
        let mut acc = Vec::new();
        remember_in(&mut acc, "h", 22, &w(&["hmac-sha1", "aes128-cbc"]));
        assert_eq!(acc, vec!["h:22 aes128-cbc,hmac-sha1".to_string()]);

        // 同一組或子集合：算接受過
        assert!(weak_covered(&acc, "h", 22, &w(&["hmac-sha1"])));
        assert!(weak_covered(&acc, "h", 22, &w(&["aes128-cbc", "hmac-sha1"])));
        // 降級到沒接受過的演算法：要再問（E9）
        assert!(!weak_covered(&acc, "h", 22, &w(&["hmac-sha1", "3des-cbc"])));
        // 別台主機／別的埠不算
        assert!(!weak_covered(&acc, "h", 2222, &w(&["hmac-sha1"])));
        assert!(!weak_covered(&acc, "hh", 22, &w(&["hmac-sha1"])));
    }

    #[test]
    fn weak_record_merges_and_replaces_old_format() {
        let mut acc = vec!["h:22".to_string(), "other:22 ssh-rsa".to_string()];
        // 舊格式不涵蓋任何演算法 → 要再問
        assert!(!weak_covered(&acc, "h", 22, &w(&["ssh-rsa"])));
        remember_in(&mut acc, "h", 22, &w(&["ssh-rsa"]));
        remember_in(&mut acc, "h", 22, &w(&["3des-cbc"]));
        assert_eq!(
            acc,
            vec!["other:22 ssh-rsa".to_string(), "h:22 3des-cbc,ssh-rsa".to_string()]
        );
        assert!(weak_covered(&acc, "h", 22, &w(&["ssh-rsa", "3des-cbc"])));
    }
}
