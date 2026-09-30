//! log 記錄：把分頁輸出寫成純文字檔。
//!
//! 逐條照抄舊版 `Logging/SessionLogger.cs`。**格式細節就是這個功能的全部**，
//! 差一個位元組就不算搬完，所以下面每一條都標出舊版對應的地方：
//!
//! | 項目 | 舊版行為 | 這裡 |
//! |---|---|---|
//! | 編碼 | `new UTF8Encoding(true)` ⇒ **UTF-8 with BOM** | [`LogWriter::open`] 在新檔／truncate／空檔時寫 `EF BB BF` |
//! | 換行 | `text.Replace("\r\n", "\n")`，之後 `StreamWriter.Write(string)` 不做轉換 ⇒ 檔案裡是 **LF** | [`strip_for_log`] |
//! | 去 ANSI | regex `\x1b\][\s\S]*?(?:\x07\|\x1b\\)\|\x1b[@-Z\\-_]\|\x1b\[[0-?]*[ -/]*[@-~]` | [`strip_ansi_into`]（手寫掃描，同三個分支） |
//! | 殘餘控制碼 | regex `[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]` 刪掉（保留 `\n` `\t`） | [`strip_for_log`] |
//! | 跨 chunk 的半截 ESC | `SplitIncompleteEscape`：最後一個 ESC 之後 ≤4096 且序列不完整就留到下一塊；OSC 要看到 BEL／ESC `\` 才算完整 | [`split_incomplete_escape`] |
//! | 跨 chunk 的半截 UTF-8 | `Encoding.UTF8.GetDecoder()` 保留狀態 | [`Logger::write`] 自己留 `utf8_carry` |
//! | 時間戳 | 每行開頭 `[yy-MM-dd HH:mm:ss] `（**本地時間**），整個 chunk 組好再寫一次 | [`Logger::write`] |
//! | flush | 每個 chunk 一次 `Flush()`（不是 AutoFlush） | [`Logger::write`] |
//!
//! 舊版的註解也一起搬：`ESC[3` / `2m` 被讀取邊界切開時 regex 兩半都對不到，log 裡就會留下
//! `[32m` 這種碎片；claude 串流每秒幾十 KB，逐字寫＝每秒上萬次 WriteFile，所以要整塊組好再寫。

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 半截 ESC 序列最多留這麼多位元組；超過就當作不是序列、照寫（同舊版 4096）。
const MAX_ESC_CARRY: usize = 4096;

pub struct Logger {
    path: PathBuf,
    timestamp: bool,
    inner: Mutex<Inner>,
}

struct Inner {
    writer: BufWriter<File>,
    at_line_start: bool,
    /// 還沒收完的 ESC 序列（跨 chunk）。
    esc_carry: String,
    /// 還沒收完的 UTF-8 位元組（跨 chunk）。
    utf8_carry: Vec<u8>,
    closed: bool,
}

impl Logger {
    /// 開始記錄。`append = false` 時截斷既有檔案（同舊版 `StreamWriter(path, append, …)`）。
    pub fn open(path: &Path, timestamp: bool, append: bool) -> std::io::Result<Self> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .append(append)
            .truncate(!append)
            .open(path)?;
        // .NET 的 UTF8Encoding(true) 只在「檔案從頭開始寫」時輸出 BOM：
        // append 到一個已經有內容的檔案不會再插一個 BOM。
        //
        // ⚠️ 這裡要看**檔案長度**，不是 `stream_position()`：append 模式剛開檔時
        // 位置回報 0（寫入才跳到尾端），用位置判斷會在每次 append 都插一個 BOM。
        let at_start = file.metadata()?.len() == 0;
        if at_start {
            file.write_all(&[0xEF, 0xBB, 0xBF])?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            timestamp,
            inner: Mutex::new(Inner {
                writer: BufWriter::new(file),
                at_line_start: true,
                esc_carry: String::new(),
                utf8_carry: Vec::new(),
                closed: false,
            }),
        })
    }

    /// 和 [`Logger::open`] 一樣，但**開檔動作丟到背景執行緒、最多等 `timeout`**。
    ///
    /// 為什麼需要：2026-09-26 在這台機器實測，同一份程式碼寫
    /// `%TEMP%` 只要 1ms，寫「我的文件\AwayTerminalLogs」卻**永遠不返回**——
    /// 防毒（PC-cillin 的資料夾保護）把剛建置、沒簽章的 exe 擋住，而且不是回一個錯誤、
    /// 是直接卡住。`log_start` 跑在 IPC 執行緒上，卡住就等於整個程式的 IPC 全停。
    ///
    /// 舊版對同一類問題也是這樣防的（`PickWorkDir` 把 `Directory.Exists` 丟背景執行緒、
    /// 最多等 300ms，因為休眠的硬碟／斷線的網路磁碟會讓它卡好幾秒）。
    ///
    /// 逾時的話那條背景執行緒會留著（它卡在系統呼叫裡，叫不回來），但只有一條，
    /// 比讓整個程式凍住好。
    pub fn open_with_timeout(
        path: &Path,
        timestamp: bool,
        append: bool,
        timeout: std::time::Duration,
    ) -> Result<Self, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let p = path.to_path_buf();
        std::thread::spawn(move || {
            let r = Logger::open(&p, timestamp, append).map_err(|e| e.to_string());
            let _ = tx.send(r);
        });
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(_) => Err(crate::i18n::tf(
                "err.logOpenTimeout",
                &[&path.display().to_string(), &timeout.as_secs().to_string()],
            )),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 寫入一塊原始輸出。在 PTY 讀取執行緒上被呼叫，所以不可以太慢。
    pub fn write(&self, data: &[u8]) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.closed {
            return;
        }

        // ---- 1. UTF-8 解碼（保留跨 chunk 狀態）----
        // 中文字被讀取邊界切開時，直接 from_utf8_lossy 會寫出 U+FFFD（舊版註解）
        let mut bytes = std::mem::take(&mut inner.utf8_carry);
        bytes.extend_from_slice(data);
        let (text, rest) = decode_utf8_prefix(&bytes);
        inner.utf8_carry = rest;

        // ---- 2. 半截 ESC 序列 ----
        let mut text = format!("{}{}", inner.esc_carry, text);
        inner.esc_carry = split_incomplete_escape(&mut text);

        // ---- 3. 去 ANSI + 去殘餘控制碼 + CRLF→LF ----
        let text = strip_for_log(&text);
        if text.is_empty() {
            return;
        }

        // ---- 4. 時間戳（整塊組好再寫一次）----
        let out = if !self.timestamp {
            text
        } else {
            let stamp = format!("[{}] ", chrono::Local::now().format("%y-%m-%d %H:%M:%S"));
            let mut sb = String::with_capacity(text.len() + 64);
            for c in text.chars() {
                if inner.at_line_start {
                    sb.push_str(&stamp);
                    inner.at_line_start = false;
                }
                sb.push(c);
                if c == '\n' {
                    inner.at_line_start = true;
                }
            }
            sb
        };

        let _ = inner.writer.write_all(out.as_bytes());
        let _ = inner.writer.flush();
    }

    pub fn close(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.closed {
            return;
        }
        inner.closed = true;
        let _ = inner.writer.flush();
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        self.close();
    }
}

/// 把 `bytes` 前面「完整的 UTF-8 部分」解碼出來，回傳 (文字, 剩下的半截位元組)。
///
/// 真的壞掉的位元組（不是「還沒收完」）照 lossy 換成 U+FFFD 往前走——
/// 不然一個壞位元組會讓後面整條 log 卡住。
///
/// ⚠️ 一定要用迴圈、不可以遞迴：Big5 BBS 畫面或 `cat` 二進位檔一塊就有幾千個壞位元組，
/// 每個壞位元組遞迴一層會撐爆讀取執行緒的堆疊（堆疊溢位不是 panic，整個 app 直接死掉）。
fn decode_utf8_prefix(bytes: &[u8]) -> (String, Vec<u8>) {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                return (out, Vec::new());
            }
            Err(e) => {
                let good = e.valid_up_to();
                // `..good` 保證是合法 UTF-8
                out.push_str(std::str::from_utf8(&rest[..good]).unwrap_or_default());
                match e.error_len() {
                    // 只是還沒收完（尾端不完整）→ 留給下一塊
                    None => return (out, rest[good..].to_vec()),
                    // 真的不合法 → 換成 U+FFFD，往後繼續
                    Some(bad) => {
                        out.push('\u{FFFD}');
                        rest = &rest[good + bad..];
                    }
                }
            }
        }
    }
}

/// 把尾端「還沒收完」的 ESC 序列切下來留到下一個 chunk。
/// 邏輯與回傳意義同舊版 `SessionLogger.SplitIncompleteEscape`。
pub fn split_incomplete_escape(text: &mut String) -> String {
    let Some(esc) = text.rfind('\u{1b}') else {
        return String::new();
    };
    if text.len() - esc > MAX_ESC_CARRY {
        return String::new();
    }
    let tail_str = &text[esc..];
    // OSC（ESC ] … BEL／ESC \）要看到終止符才算收完：`ESC ]` 本身也符合兩字元 Fe 分支，
    // 會被誤判成完整、把 `]0;title` 留在 log 裡（舊版註解）
    let is_osc = tail_str.as_bytes().get(1) == Some(&b']');
    let osc_open =
        is_osc && !tail_str.contains('\u{07}') && !tail_str.contains("\u{1b}\\");
    if !osc_open && matches_escape_at_start(tail_str).is_some() {
        return String::new(); // 最後一個 ESC 序列是完整的
    }
    let tail = tail_str.to_string();
    text.truncate(esc);
    tail
}

/// `s` 開頭是不是一個**完整**的 ANSI 序列；是就回傳它的位元組長度。
///
/// 三個分支與舊版 regex 一一對應：
///   1. `\x1b\][\s\S]*?(?:\x07|\x1b\\)`  OSC，終止符是 BEL 或 ESC `\`
///   2. `\x1b\[[0-?]*[ -/]*[@-~]`        CSI
///   3. `\x1b[@-Z\\-_]`                  兩字元 Fe 序列
///
/// 順序也照 regex 的交替順序（OSC 先於 Fe，否則 `ESC ]` 會被 Fe 吃掉）。
fn matches_escape_at_start(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.first() != Some(&0x1b) {
        return None;
    }
    match b.get(1) {
        // ① OSC
        Some(b']') => {
            let mut i = 2;
            while i < b.len() {
                if b[i] == 0x07 {
                    return Some(i + 1);
                }
                if b[i] == 0x1b && b.get(i + 1) == Some(&b'\\') {
                    return Some(i + 2);
                }
                i += 1;
            }
            None
        }
        // ② CSI：參數位元組 [0-?]*、中間位元組 [ -/]*、結束位元組 [@-~]
        Some(b'[') => {
            let mut i = 2;
            while i < b.len() && (0x30..=0x3f).contains(&b[i]) {
                i += 1;
            }
            while i < b.len() && (0x20..=0x2f).contains(&b[i]) {
                i += 1;
            }
            match b.get(i) {
                Some(&c) if (0x40..=0x7e).contains(&c) => Some(i + 1),
                _ => None,
            }
        }
        // ③ 兩字元 Fe：regex 的 [@-Z\-_] ＝ 0x40..=0x5a 與 0x5c..=0x5f（`[`=0x5b 已被②接走，
        //    `]`=0x5d 已被①接走，順序與 regex 的交替順序一致）
        Some(&c) if (0x40..=0x5a).contains(&c) || (0x5c..=0x5f).contains(&c) => Some(2),
        _ => None,
    }
}

/// 去 ANSI → CRLF 轉 LF → 去殘餘控制碼（保留 `\n` `\t`）。順序同舊版。
pub fn strip_for_log(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    strip_ansi_into(text, &mut out);
    let out = out.replace("\r\n", "\n");
    out.chars()
        .filter(|&c| {
            let n = c as u32;
            // [\x00-\x08\x0b\x0c\x0e-\x1f\x7f]（\t=0x09、\n=0x0a 保留）
            !(n <= 0x08 || n == 0x0b || n == 0x0c || (0x0e..=0x1f).contains(&n) || n == 0x7f)
        })
        .collect()
}

/// 刪掉所有完整的 ANSI 序列；對不上的 ESC 原樣留著（regex 也是這樣）。
fn strip_ansi_into(text: &str, out: &mut String) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if let Some(len) = matches_escape_at_start(&text[i..]) {
                i += len;
                continue;
            }
        }
        // 逐「字元」前進，不能逐位元組（中文字會被切斷）
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
}

/// log 預設檔名：`{分頁名稱}_{yyyyMMdd_HHmmss}.log`，非法字元換成 `_`（同舊版 `LogDialog`）。
pub fn default_log_name(tab_title: &str) -> String {
    let safe: String = tab_title
        .chars()
        .map(|c| if is_invalid_file_char(c) { '_' } else { c })
        .collect();
    let safe = if safe.trim().is_empty() {
        "log".to_string()
    } else {
        safe
    };
    format!("{safe}_{}.log", chrono::Local::now().format("%Y%m%d_%H%M%S"))
}

/// 「複製全部存至檔案」的預設檔名：`{分頁名稱}-{yyyyMMdd-HHmmss}.txt`（同舊版 `SaveBufferToFile`）。
pub fn default_save_name(tab_title: &str) -> String {
    let safe: String = tab_title
        .chars()
        .map(|c| if is_invalid_file_char(c) { '_' } else { c })
        .collect();
    let safe = if safe.trim().is_empty() {
        "AwayTerminal".to_string()
    } else {
        safe
    };
    format!("{safe}-{}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S"))
}

/// 約等於 .NET `Path.GetInvalidFileNameChars()`。
fn is_invalid_file_char(c: char) -> bool {
    matches!(c, '"' | '<' | '>' | '|' | ':' | '*' | '?' | '\\' | '/') || (c as u32) < 0x20
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_osc_and_fe() {
        assert_eq!(strip_for_log("\x1b[32mgreen\x1b[0m"), "green");
        assert_eq!(strip_for_log("\x1b]0;title\x07body"), "body");
        assert_eq!(strip_for_log("\x1b]0;title\x1b\\body"), "body");
        assert_eq!(strip_for_log("a\x1bDb"), "ab"); // 兩字元 Fe（D=0x44 在 [@-Z] 內）
        // `\x1b=`（DECKPAM）不在舊版 regex 的 Fe 範圍 [@-Z\\-_] 裡 → 只有 ESC 被後面的
        // 控制碼過濾刪掉，`=` 會留在 log 裡。這是舊版的實際行為，照抄。
        assert_eq!(strip_for_log("a\x1b=b"), "a=b");
        assert_eq!(strip_for_log("\x1b[?25lhi\x1b[?25h"), "hi");
    }

    #[test]
    fn normalizes_newlines_and_drops_control_chars() {
        assert_eq!(strip_for_log("a\r\nb\r\n"), "a\nb\n");
        assert_eq!(strip_for_log("a\x07b\x08c"), "abc"); // BEL / BS 刪掉
        assert_eq!(strip_for_log("a\tb\nc"), "a\tb\nc"); // TAB / LF 保留
    }

    #[test]
    fn keeps_cjk_intact() {
        assert_eq!(strip_for_log("\x1b[36m中文測試\x1b[0m\n"), "中文測試\n");
    }

    #[test]
    fn carries_incomplete_escape_to_next_chunk() {
        // ESC[3 | 2m 被切開：舊版的 log 碎片問題（`[32m` 留在檔案裡）
        let mut a = "hello\x1b[3".to_string();
        let carry = split_incomplete_escape(&mut a);
        assert_eq!(a, "hello");
        assert_eq!(carry, "\x1b[3");
        let mut b = format!("{carry}2mworld");
        assert_eq!(split_incomplete_escape(&mut b), "");
        assert_eq!(strip_for_log(&b), "world");
    }

    #[test]
    fn carries_unterminated_osc() {
        // `ESC ]` 也符合兩字元 Fe 分支 → 一定要先判 OSC 有沒有終止符，否則 `]0;t` 會留在 log 裡
        let mut a = "x\x1b]0;ti".to_string();
        assert_eq!(split_incomplete_escape(&mut a), "\x1b]0;ti");
        assert_eq!(a, "x");
    }

    #[test]
    fn complete_escape_is_not_carried() {
        let mut a = "x\x1b[0m".to_string();
        assert_eq!(split_incomplete_escape(&mut a), "");
        assert_eq!(a, "x\x1b[0m");
    }

    #[test]
    fn decodes_split_utf8() {
        let full = "中文".as_bytes().to_vec();
        let (head, carry) = decode_utf8_prefix(&full[..4]); // 「中」+「文」的第一個位元組
        assert_eq!(head, "中");
        assert_eq!(carry, vec![full[3]]);
        let mut rest = carry;
        rest.extend_from_slice(&full[4..]);
        let (tail, carry2) = decode_utf8_prefix(&rest);
        assert_eq!(tail, "文");
        assert!(carry2.is_empty());
    }

    /// 大塊非 UTF-8（Big5 畫面、二進位檔）不可以撐爆堆疊（C1）。
    #[test]
    fn decodes_huge_invalid_block_without_recursion() {
        let mut bytes = vec![0xFFu8; 1_000_000];
        bytes.extend_from_slice("中".as_bytes());
        bytes.push(0xE4); // 半截的下一個字
        let (text, carry) = decode_utf8_prefix(&bytes);
        assert_eq!(text.chars().filter(|&c| c == '\u{FFFD}').count(), 1_000_000);
        assert!(text.ends_with('中'));
        assert_eq!(carry, vec![0xE4]);
    }

    #[test]
    fn sanitizes_file_names() {
        assert!(default_log_name("a/b:c").starts_with("a_b_c_"));
        assert!(default_log_name("   ").starts_with("log_"));
        assert!(default_save_name("tab").ends_with(".txt"));
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn writes_bom_lf_and_timestamps() {
        let dir = std::env::temp_dir().join("awayterm-log-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("a.log");

        let lg = Logger::open(&path, true, false).expect("open");
        lg.write(b"\x1b[32m\xe4\xb8\xad\xe6\x96\x87 ok\x1b[0m\r\n");
        lg.write(b"second\r\n");
        lg.close();

        let bytes = std::fs::read(&path).expect("read");
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "UTF-8 BOM");
        let text = String::from_utf8(bytes[3..].to_vec()).expect("utf8");
        assert!(!text.contains('\r'), "換行必須是 LF：{text:?}");
        assert!(!text.contains('\x1b'), "不可殘留 ANSI：{text:?}");
        let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 2);
        for l in &lines {
            // `[yy-MM-dd HH:mm:ss] ` ＝ 1 + 17 + 1 + 1 ＝ 20 個位元組
            let b = l.as_bytes();
            assert!(b.len() > 20, "時間戳長度：{l:?}");
            assert_eq!(b[0], b'[', "時間戳開頭：{l:?}");
            assert_eq!(b[18], b']', "時間戳結尾：{l:?}");
            assert_eq!(b[19], b' ', "時間戳後面要有一個空白：{l:?}");
        }
        assert!(lines[0].ends_with("中文 ok"));
        assert!(lines[1].ends_with("second"));

        // append：BOM 不會再插一次
        let lg2 = Logger::open(&path, false, true).expect("reopen");
        lg2.write(b"third\n");
        lg2.close();
        let again = std::fs::read(&path).expect("read2");
        assert_eq!(again.iter().filter(|&&b| b == 0xEF).count(), 1);
        assert!(String::from_utf8_lossy(&again).ends_with("third\n"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
