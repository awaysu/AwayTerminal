//! 主機金鑰對話框：Rust 的 SSH 任務 ↔ 前端。
//!
//! `russh` 的 `check_server_key` 是在連線交握中間被呼叫的，**必須當場給答案**，
//! 所以這裡的流程是：
//!
//! 1. SSH 任務呼叫 [`AppDecider::decide`]（同步），它配一個 request id、
//!    把回覆用的 sender 放進 [`PENDING`]，然後 emit tauri event `ssh-hostkey`。
//! 2. 前端跳對話框（文案照 PuTTY：第一次連線／金鑰變更兩種），使用者按下去之後
//!    呼叫 `ssh_hostkey_answer` 指令。
//! 3. 指令把答案送進 sender，`decide` 醒來回傳。
//!
//! **逾時（或前端根本沒回）一律當成取消**——主機金鑰沒確認就不連，這是安全預設。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use super::hostkey::{Fingerprints, Verdict};
use super::{HostKeyAnswer, HostKeyDecider};

/// 等使用者按按鈕最多等這麼久。比人的反應時間寬鬆很多，只是不要讓連線永遠掛著。
const ANSWER_TIMEOUT: Duration = Duration::from_secs(180);

type Pending = Mutex<HashMap<u64, std::sync::mpsc::Sender<HostKeyAnswer>>>;

fn pending() -> &'static Pending {
    static P: OnceLock<Pending> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
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
    fn decide(
        &self,
        host: &str,
        port: u16,
        verdict: &Verdict,
        fp: &Fingerprints,
    ) -> HostKeyAnswer {
        let id = next_id();
        let (tx, rx) = std::sync::mpsc::channel();
        pending()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);

        let req = HostKeyRequest {
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

        if self.app.emit("ssh-hostkey", req).is_err() {
            pending()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            return HostKeyAnswer::Reject;
        }

        let answer = rx.recv_timeout(ANSWER_TIMEOUT).unwrap_or_else(|_| {
            println!("[AwayTerminal] 主機金鑰確認逾時或視窗已關閉 → 當成取消");
            HostKeyAnswer::Reject
        });
        pending()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        answer
    }

    /// 協商到警告線以下的演算法。照 PuTTY：**同一台主機接受過就不再問**。
    fn accept_weak(&self, host: &str, port: u16, weak: &[(String, String)]) -> bool {
        let key = format!("{host}:{port}");
        if self.settings.get().ssh_weak_accepted.iter().any(|k| k == &key) {
            return true; // 之前接受過
        }

        let id = next_id();
        let (tx, rx) = std::sync::mpsc::channel();
        pending()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);

        let req = WeakAlgoRequest {
            id,
            tab_id: self.tab_id,
            host: host.to_string(),
            port,
            items: weak
                .iter()
                .map(|(k, n)| ((*k).to_string(), n.clone()))
                .collect(),
        };
        if self.app.emit("ssh-weak-algo", req).is_err() {
            pending()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            return false;
        }

        // 前端只有「繼續／取消」兩個選項，借用同一個回覆通道：
        // AcceptAndStore＝繼續並記住這台主機；Reject＝取消。
        let answer = rx.recv_timeout(ANSWER_TIMEOUT).unwrap_or_else(|_| {
            println!("[AwayTerminal] 弱演算法確認逾時 → 當成取消");
            HostKeyAnswer::Reject
        });
        pending()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);

        match answer {
            HostKeyAnswer::Reject => false,
            _ => {
                self.settings.update(|s| {
                    if !s.ssh_weak_accepted.iter().any(|k| k == &key) {
                        s.ssh_weak_accepted.push(key.clone());
                    }
                });
                true
            }
        }
    }
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
