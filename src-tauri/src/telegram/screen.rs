//! 「拿那個分頁畫面上看得到的文字」——遠端的每個指令都要它。
//!
//! ## ⚠️ 舊版 `CLAUDE.md` 那條雷
//! > 遠端 `/last` **絕不能用原始位元組流去 ANSI**。
//!
//! 位元組流裡是 claude／codex 逐格重繪的控制序列，把它去掉 ANSI 得到的是一團重複的垃圾
//! （同一行出現十幾次不同寬度的版本）。要拿的是 **xterm 畫面上真正看得到的那些字**——
//! 也就是 `terminal.js` 的 `lastPlainText()`（`buffer.getLine().translateToString()`，
//! 自動接回 `isWrapped` 的折行）。
//!
//! 那條路是舊協定的 `q{id}US text` → `a{id}US text US<內容>`，**`terminal.js` 早就有了**
//! （那個檔和舊版逐字一樣）。這裡補的只是 Rust 這一端的信箱：
//! 送出 `q`、等 `a` 回來、逾時就放棄。
//!
//! 逾時放棄之後**不做任何「退回位元組流」的備援**——舊版有，而且它正是那條雷的來源
//! （UI 執行緒卡住時退回劣化的位元組流，選單偵測對不到、使用者看到亂碼）。
//! 我們的輪詢在自己的執行緒上，不會卡住 webview，所以逾時代表前端真的有問題，
//! 這時候寧可回空字串讓指令回一句「畫面尚未就緒」。

use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::AppHandle;

/// 等 `a{id}US text US…` 回來的信箱（同 `restore.rs` 的作法）。
type Mailbox = Mutex<Vec<(u32, Sender<String>)>>;

fn mailbox() -> &'static Mailbox {
    static M: OnceLock<Mailbox> = OnceLock::new();
    M.get_or_init(|| Mutex::new(Vec::new()))
}

/// `pane_answer` 收到 `text` 的回覆時呼叫。
pub fn deliver(id: u32, text: String) {
    let mut g = mailbox().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pos) = g.iter().position(|(i, _)| *i == id) {
        let (_, tx) = g.remove(pos);
        let _ = tx.send(text);
    }
}

/// 向前端要這個分頁畫面上的文字（最多 400 行，同舊版）。
///
/// **不要在 tauri 的 IPC 執行緒上呼叫**——它會等前端回覆。遠端是自己的執行緒，沒問題。
///
/// 等不到回覆回 `None`（BUG H4）：和「畫面真的是空的」（`Some("")`）要分得開，
/// 呼叫端才不會拿逾時的空字串去當推播基準。
pub fn recent_text(app: &AppHandle, id: u32, timeout: Duration) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    {
        let mut g = mailbox().lock().unwrap_or_else(|e| e.into_inner());
        g.retain(|(i, _)| *i != id); // 同一個分頁只留最後一個等待者
        g.push((id, tx));
    }
    crate::host::emit_host(app, format!("q{id}\x1ftext"));
    match rx.recv_timeout(timeout) {
        Ok(text) => Some(text),
        Err(_) => {
            let mut g = mailbox().lock().unwrap_or_else(|e| e.into_inner());
            g.retain(|(i, _)| *i != id);
            println!("[AwayTerminal] Telegram：分頁 {id} 的畫面文字等不到（前端沒回 a…text）");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 信箱：送進來的文字交給等待者。
    #[test]
    fn delivers_to_the_waiter() {
        let (tx, rx) = mpsc::channel();
        mailbox()
            .lock()
            .unwrap()
            .push((7, tx));
        deliver(7, "畫面內容".to_string());
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "畫面內容");
    }

    /// 沒有人在等的回覆安靜丟掉（不能 panic）。
    #[test]
    fn ignores_answers_nobody_waits_for() {
        deliver(999_999, "x".to_string());
    }

    /// 同一個分頁再要一次 → 只留最後一個等待者（前一個會拿不到，但不會卡住）。
    #[test]
    fn only_the_latest_request_waits() {
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();
        {
            let mut g = mailbox().lock().unwrap();
            g.retain(|(i, _)| *i != 8);
            g.push((8, tx1));
        }
        {
            let mut g = mailbox().lock().unwrap();
            g.retain(|(i, _)| *i != 8);
            g.push((8, tx2));
        }
        deliver(8, "second".to_string());
        assert!(rx1.recv_timeout(Duration::from_millis(50)).is_err());
        assert_eq!(rx2.recv_timeout(Duration::from_secs(1)).unwrap(), "second");
    }
}
