//! `/shot`：把分頁畫面變成一張 PNG。
//!
//! ## 這一版怎麼做（B(a)）
//! 前端（`bridge.js` 的 `shotPng`）把畫面文字畫進一張**新的** `<canvas>` 再 `toDataURL`，
//! 我們這邊只負責要圖、等圖、解 base64。**不抓 xterm 自己的 canvas**：WebGL 的 drawing
//! buffer 畫完就可以丟（`preserveDrawingBuffer` 預設 false，`toDataURL` 多半全黑），
//! 打開它又會拖慢渲染——而渲染速度正是這一版的重點。
//!
//! 代價：**單色**（前景／背景），舊版 WPF 是整塊 render、每個字有顏色。
//! 寫進 `docs/REGRESSION-CHECKLIST.md` 的「刻意與舊版不同」。
//!
//! ## 之後要接平台 API（B(b)）
//! 真的要「和螢幕上一模一樣」就得截視窗，那是各平台各一套：
//! Windows `PrintWindow`、macOS `CGWindowListCreateImage`（要螢幕錄製權限）、
//! Linux 得看 X11／Wayland（Wayland 下一般程式根本不能截別人的視窗）。
//! 介面已經留好——[`capture_png`] 先問 [`platform::capture`]，回 `None` 才退回前端那條路。
//! 三個平台目前都是 `None`（還沒做），所以行為上就是「一律用前端畫的」。
//!
//! ⚠️ 平台截圖需要視窗在前景／有焦點，而**這個團隊的規則是不准碰前景視窗**，
//! 所以 B(b) 真的要做的時候只能在使用者自己的機器上驗，不能進 `--verify`。

use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::AppHandle;

/// 等前端回 PNG 的上限。畫 400 列的 canvas 在慢機器上也是幾十毫秒的事，2 秒很寬鬆。
const TIMEOUT: Duration = Duration::from_secs(2);

type Mailbox = Mutex<Vec<(u32, Sender<String>)>>;

fn mailbox() -> &'static Mailbox {
    static M: OnceLock<Mailbox> = OnceLock::new();
    M.get_or_init(|| Mutex::new(Vec::new()))
}

/// `pane_answer` 收到 `shot` 的回覆時呼叫（內容是 base64 的 PNG）。
pub fn deliver(id: u32, base64_png: String) {
    let mut g = mailbox().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pos) = g.iter().position(|(i, _)| *i == id) {
        let (_, tx) = g.remove(pos);
        let _ = tx.send(base64_png);
    }
}

/// 拿這個分頁的 PNG。**不要在 tauri 的 IPC 執行緒上呼叫**（會等前端回覆）。
pub fn capture_png(app: &AppHandle, id: u32) -> Option<Vec<u8>> {
    if let Some(png) = platform::capture(app, id) {
        return Some(png);
    }
    let (tx, rx) = mpsc::channel();
    {
        let mut g = mailbox().lock().unwrap_or_else(|e| e.into_inner());
        g.retain(|(i, _)| *i != id);
        g.push((id, tx));
    }
    crate::host::emit_host(app, format!("q{id}\x1fshot"));
    let b64 = match rx.recv_timeout(TIMEOUT) {
        Ok(s) => s,
        Err(_) => {
            let mut g = mailbox().lock().unwrap_or_else(|e| e.into_inner());
            g.retain(|(i, _)| *i != id);
            println!("[AwayTerminal] Telegram：分頁 {id} 的截圖等不到（前端沒回 a…shot）");
            return None;
        }
    };
    if b64.is_empty() {
        return None;
    }
    match decode_base64(&b64) {
        Some(png) if is_png(&png) => Some(png),
        _ => {
            println!("[AwayTerminal] Telegram：截圖不是有效的 PNG（{} 位元組）", b64.len());
            None
        }
    }
}

/// PNG 的檔頭（`sendPhoto` 送錯格式 Telegram 只會回一句 400，很難查）。
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
}

/// base64 解碼。自己寫是因為專案裡沒有 base64 crate，而這是唯一需要解的地方
/// （`v` 協定那邊是**編碼**，在前端做）。
pub fn decode_base64(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 26,
            b'0'..=b'9' => (c - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        let v = val(c)?;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// 平台截圖（B(b)）。三個平台都還沒做，一律回 `None` → 退回前端畫的那張。
mod platform {
    use tauri::AppHandle;

    #[cfg(windows)]
    pub fn capture(_app: &AppHandle, _id: u32) -> Option<Vec<u8>> {
        // TODO(B(b))：`PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT)` → BMP → PNG。
        // 要把 pane 在視窗裡的位置換算出來（分割／分欄模式下不是整個 client 區）。
        None
    }

    #[cfg(target_os = "macos")]
    pub fn capture(_app: &AppHandle, _id: u32) -> Option<Vec<u8>> {
        // TODO(B(b))：`CGWindowListCreateImage`。需要「螢幕錄製」權限，第一次會跳系統對話框
        // → 只能在使用者自己按過之後才會有圖，不能當預設路徑。
        None
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    pub fn capture(_app: &AppHandle, _id: u32) -> Option<Vec<u8>> {
        // TODO(B(b))：X11 可以 `XGetImage`；Wayland 下一般程式不能截別人的視窗
        // （要走 xdg-desktop-portal，會跳使用者確認）→ Linux 大概會一直用前端那條路。
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PNG 檔頭判斷。
    #[test]
    fn detects_png_header() {
        assert!(is_png(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0]));
        assert!(!is_png(b"GIF89a"));
        assert!(!is_png(b""));
        assert!(!is_png(&[0x89, b'P', b'N']));
    }

    /// base64：對照已知答案（含 1 個與 2 個 `=` 的補位）。
    #[test]
    fn decodes_base64() {
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(decode_base64("TWFu").unwrap(), b"Man");
        assert_eq!(decode_base64("YQ==").unwrap(), b"a");
        assert_eq!(decode_base64("YWI=").unwrap(), b"ab");
        assert_eq!(decode_base64("YWJj").unwrap(), b"abc");
    }

    /// 前端送的 dataURL 會被切掉前綴，但可能帶換行（`toDataURL` 不會，保險）。
    #[test]
    fn ignores_whitespace() {
        assert_eq!(decode_base64("TW Fu\n").unwrap(), b"Man");
    }

    /// 不是 base64 的字元 → `None`（不要吐出半截垃圾去當 PNG 送）。
    #[test]
    fn rejects_garbage() {
        assert!(decode_base64("!!!!").is_none());
        assert!(decode_base64("<html>").is_none());
    }

    /// 真的 PNG 走一遍 base64 → 解回來還是 PNG。
    #[test]
    fn round_trips_a_tiny_png() {
        // 1×1 透明 PNG 的標準 base64
        let b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z\
                   8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";
        let png = decode_base64(b64).unwrap();
        assert!(is_png(&png), "解回來的不是 PNG");
        assert!(png.len() > 60, "長度不對：{}", png.len());
    }

    /// 沒有人在等的回覆安靜丟掉。
    #[test]
    fn ignores_answers_nobody_waits_for() {
        deliver(999_998, "x".to_string());
    }
}
