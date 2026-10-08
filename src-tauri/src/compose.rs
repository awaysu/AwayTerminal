//! 「輸入文字」視窗的後端（搬移舊版 `Dialogs/ComposeDialog.xaml(.cs)` + `MainWindow.SendSnippet`）。
//!
//! 用途照舊版的註解：**先在一般輸入框把文字打好，按「送出」才整段貼進分頁**——
//! 繞過 claude 對逐鍵 IME 輸入的重複／亂碼問題（組字與提交都發生在輸入框，
//! xterm／ConPTY 只會收到最後一次整段貼上）。
//!
//! | 舊版行為 | 出處 | 這裡 |
//! |---|---|---|
//! | 載入文字檔：選檔、**上限 2MB**、內容取代文字框 | `LoadFile_Click` | [`compose_load_file`] |
//! | 解碼順序：BOM（UTF-8／UTF-16 LE／BE）→ 嚴格 UTF-8 → 系統 ANSI 字碼頁（繁中＝950／Big5） | `DecodeText` | [`decode_text`] |
//! | 換行統一成 `\r\n` | `LoadFile_Click` | 同 |
//! | 儲存：UTF-8 **無 BOM** | `Save_Click` | [`compose_save_file`] |
//! | 送出：走**貼上**那條路（`v` 協定 → `term.paste`），所以 claude 分頁照樣是 ESC+CR 軟換行 | `SendSnippet` → `PasteToTab` | [`compose_send`] |
//! | 「送出後送 Enter」：**貼完 200ms 再直接對 session 送 `\r`**（不可以併進貼上內容） | `SendSnippet` | 同 |
//! | 固定送到當初那個分頁（期間切分頁也不會送錯） | 同上 | 同（`id` 是參數） |
//! | 勾選狀態記在設定 | `AppSettings.ComposeSendEnter` | `settings.compose_send_enter` |
//! | （舊版沒有）記住上次載入的檔案，下次選檔從那裡開 | — | `settings.compose_last_file`（2.0.14，使用者要求） |
//! | （舊版沒有）儲存的對話框也開在同一個地方，存完也記成「上次的檔案」 | — | 同上（2.0.15，使用者要求「儲存路徑要和載入的一樣」） |

use crate::i18n::{t, tf};
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::settings::SettingsStore;
use crate::tabs::TabManager;

/// 載入文字檔的大小上限（舊版 `LoadMaxMB = 2`）。
pub const LOAD_MAX_MB: u64 = 2;

/// 讀進來的檔案內容。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedText {
    pub path: String,
    pub text: String,
    /// 用哪種編碼解出來的（顯示給使用者看：`UTF-8`／`UTF-16LE`／`Big5`…）。
    pub encoding: &'static str,
}

/// 文字檔解碼——**順序照舊版 `DecodeText`**：
///
/// 1. 有 BOM 就照 BOM（UTF-8／UTF-16 LE／UTF-16 BE）
/// 2. 沒有 BOM 先試**嚴格** UTF-8
/// 3. 不合法（例如舊的 Big5 記事本檔）才退回系統 ANSI 字碼頁
///
/// 舊版用 `CultureInfo.CurrentCulture.TextInfo.ANSICodePage`（繁中 Windows ＝ 950 ＝ Big5）。
/// 我們固定用 **Big5**：這個程式的使用者是繁中環境，而「猜錯編碼」的代價是中文變亂碼。
/// 真的需要其他字碼頁時再加選項（`docs/COMPOSE.md` 有寫）。
pub fn decode_text(bytes: &[u8]) -> (String, &'static str) {
    // 1. BOM
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (
            String::from_utf8_lossy(&bytes[3..]).into_owned(),
            "UTF-8 (BOM)",
        );
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (text, _, _) = encoding_rs::UTF_16LE.decode(&bytes[2..]);
        return (text.into_owned(), "UTF-16LE");
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (text, _, _) = encoding_rs::UTF_16BE.decode(&bytes[2..]);
        return (text.into_owned(), "UTF-16BE");
    }
    // 2. 嚴格 UTF-8
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_string(), "UTF-8");
    }
    // 3. 退回 Big5（繁中 Windows 的 ANSI 字碼頁 950）
    let (text, _, _) = encoding_rs::BIG5.decode(bytes);
    (text.into_owned(), "Big5")
}

/// 換行統一成 `\r\n`（舊版 `LoadFile_Click` 的那三個 Replace）。
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

/// 「載入文字檔」：跳檔案選擇 → 讀檔 → 解碼 → 統一換行。
///
/// 篩選器照舊版（`txt;md;log;json;csv;xml;yaml;yml` + 所有檔案）。
/// 超過 2MB 回 `Err`（舊版跳「檔案太大（上限 {0} MB），未載入。」）。
///
/// 2.0.14：對話框開在**上次載入的那個檔案**的資料夾、預選它的檔名；載入成功就記下這次的路徑。
/// 上次的資料夾已經不在了（搬走、隨身碟拔掉）就照系統預設開。
#[tauri::command]
pub async fn compose_load_file(
    app: AppHandle,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<Option<LoadedText>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    let dialog = at_last_file(app.dialog().file(), &settings, None);
    dialog
        .set_title(t("dlg.loadTextFile"))
        .add_filter(
            t("dlg.textFiles"),
            &["txt", "md", "log", "json", "csv", "xml", "yaml", "yml"],
        )
        .add_filter(t("dlg.allFiles"), &["*"])
        .pick_file(move |f| {
            let _ = tx.send(f);
        });
    let picked = tokio::task::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .map_err(|e| tf("err.filePickFailed", &[&e.to_string()]))?;
    let Some(path) = picked else { return Ok(None) };
    let path = path.to_string();
    let p = std::path::PathBuf::from(&path);

    let size = std::fs::metadata(&p).map_err(|e| tf("err.readFileFailed", &[&e.to_string()]))?.len();
    if size > LOAD_MAX_MB * 1024 * 1024 {
        return Err(tf("compose.loadTooBig", &[&LOAD_MAX_MB.to_string()]));
    }
    let bytes = std::fs::read(&p).map_err(|e| tf("err.readFileFailed", &[&e.to_string()]))?;
    let (text, encoding) = decode_text(&bytes);
    // 讀成功才記（太大、讀不到的檔不記，下次還是從上一個好的地方開）
    settings.update(|s| s.compose_last_file = path.clone());
    println!(
        "[AwayTerminal] 輸入文字：載入 {path}（{} bytes，{encoding}）",
        bytes.len()
    );
    Ok(Some(LoadedText {
        path,
        text: normalize_newlines(&text),
        encoding,
    }))
}

/// 檔案對話框開在「上次的檔案」（載入或儲存過的那一個）的資料夾、預選它的檔名。
/// 還沒有上次的檔案、或那個資料夾已經不在了：照系統預設開，檔名用 `fallback_name`（有給的話）。
fn at_last_file<R: tauri::Runtime>(
    mut dialog: tauri_plugin_dialog::FileDialogBuilder<R>,
    settings: &SettingsStore,
    fallback_name: Option<&str>,
) -> tauri_plugin_dialog::FileDialogBuilder<R> {
    let last = std::path::PathBuf::from(settings.get().compose_last_file);
    match last.parent().filter(|d| !d.as_os_str().is_empty() && d.is_dir()) {
        Some(dir) => {
            dialog = dialog.set_directory(dir);
            if let Some(name) = last.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
        }
        None => {
            if let Some(name) = fallback_name {
                dialog = dialog.set_file_name(name);
            }
        }
    }
    dialog
}

/// 「儲存」：跳存檔對話框 → 寫 UTF-8（**無 BOM**，同舊版 `new UTF8Encoding(false)`）。
///
/// 2.0.15：對話框和「載入」開在同一個地方（上次載入或儲存的那個檔案），存成功也記成上次的檔案。
#[tauri::command]
pub async fn compose_save_file(
    app: AppHandle,
    text: String,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    at_last_file(app.dialog().file(), &settings, Some("compose.txt"))
        .set_title(t("dlg.save"))
        .add_filter(t("dlg.textFiles"), &["txt"])
        .add_filter("Markdown", &["md"])
        .add_filter(t("dlg.allFiles"), &["*"])
        .save_file(move |f| {
            let _ = tx.send(f);
        });
    let picked = tokio::task::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .map_err(|e| tf("err.savePickFailed", &[&e.to_string()]))?;
    let Some(path) = picked else { return Ok(None) };
    let path = path.to_string();
    std::fs::write(&path, text.as_bytes()).map_err(|e| tf("err.saveFileFailed", &[&e.to_string()]))?;
    settings.update(|s| s.compose_last_file = path.clone());
    println!("[AwayTerminal] 輸入文字：存成 {path}");
    Ok(Some(path))
}

/// 送出（舊版 `SendSnippet`）。
///
/// 兩步：
/// 1. 走**貼上**那條路（`v` 協定 → `terminal.js` 的 `doPaste`），claude 分頁的換行
///    會變成 ESC+CR 軟換行、bracketed paste 也照原樣——和使用者按 Ctrl+V 完全一樣。
/// 2. 勾了「送出後送 Enter」：**等 200ms** 再直接對 session 送 `\r`。
///
/// ⚠️ 為什麼 Enter 不能併進貼上內容（舊版註解原話）：claude 分頁會把內容裡的換行轉成
/// ESC+CR 軟換行、bracketed paste 下換行也只是插入，**都不會送出**。
#[tauri::command]
pub async fn compose_send(
    app: AppHandle,
    id: u32,
    text: String,
    send_enter: bool,
    remember: Option<bool>,
    tabs_state: State<'_, Arc<TabManager>>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<(), String> {
    if text.is_empty() {
        return Ok(()); // 空白不送（舊版 `Send()` 也是直接 return）
    }
    if tabs_state.kind_of(id).is_none() {
        return Err(t("compose.noTab").to_string());
    }
    if remember.unwrap_or(true) {
        settings.update(|s| s.compose_send_enter = send_enter);
    }
    // 1. 貼上（和工具列「純文字貼上」同一條路）
    crate::host::emit_host(
        &app,
        format!("v{id}\x1f{}", crate::b64::encode(text.as_bytes())),
    );
    println!(
        "[AwayTerminal] 輸入文字：送出 {} 個字到分頁 {id}（送 Enter={send_enter}）",
        text.chars().count()
    );
    if !send_enter {
        return Ok(());
    }
    // 2. 200ms 之後補 CR（固定送到當初那個分頁）
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        if let Some(m) = app2.try_state::<crate::session::SessionManager>() {
            if let Some(s) = m.get(id) {
                s.write(b"\r");
            }
        }
    });
    Ok(())
}

/// **只給 `--verify` 用**：把一段中文寫成 **Big5** 檔（放 `%TEMP%`）再讀回來解碼，
/// 證明「Big5 檔 → 正確中文」這條路是通的（檔案選擇對話框沒辦法自動點）。
#[tauri::command]
pub fn compose_verify_roundtrip(text: String) -> Result<serde_json::Value, String> {
    let path = std::env::temp_dir().join("awayterm-verify-big5.txt");
    // 故意寫成 Big5（含 CR 當換行，一起驗換行統一）
    let source = format!("{text}\r第二行");
    let (bytes, _, had_errors) = encoding_rs::BIG5.encode(&source);
    if had_errors {
        return Err(t("err.notBig5").to_string());
    }
    std::fs::write(&path, &bytes).map_err(|e| tf("err.writeFailed", &[&e.to_string()]))?;

    let raw = std::fs::read(&path).map_err(|e| tf("err.readFailed", &[&e.to_string()]))?;
    let (decoded, encoding) = decode_text(&raw);
    let normalized = normalize_newlines(&decoded);
    let first_line = normalized.split("\r\n").next().unwrap_or("").to_string();
    // 換行全部是 CRLF＝再跑一次 normalize 不會變、而且 CR 與 LF 一樣多
    let crlf_ok = normalized.contains("\r\n")
        && normalize_newlines(&normalized) == normalized
        && normalized.matches('\r').count() == normalized.matches('\n').count();
    let _ = std::fs::remove_file(&path);
    Ok(serde_json::json!({
        "encoding": encoding,
        // 比對在這裡做：JS 那邊的字面（尤其空白）容易和這裡不一致（第一版就是這樣假失敗）
        "textOk": first_line == text,
        "text": first_line,
        "crlfOk": crlf_ok,
        "bytes": raw.len(),
    }))
}

// `try_state` 要這個 trait
use tauri::Manager;

#[cfg(test)]
mod tests {
    use super::*;

    /// BOM 的三種情形。
    #[test]
    fn decodes_by_bom() {
        let mut utf8 = vec![0xEF, 0xBB, 0xBF];
        utf8.extend_from_slice("中文".as_bytes());
        assert_eq!(decode_text(&utf8), ("中文".to_string(), "UTF-8 (BOM)"));

        let mut le = vec![0xFF, 0xFE];
        for u in "中文".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_text(&le), ("中文".to_string(), "UTF-16LE"));

        let mut be = vec![0xFE, 0xFF];
        for u in "中文".encode_utf16() {
            be.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(decode_text(&be), ("中文".to_string(), "UTF-16BE"));
    }

    /// 沒有 BOM：先試嚴格 UTF-8。
    #[test]
    fn prefers_strict_utf8() {
        assert_eq!(
            decode_text("中文 abc".as_bytes()),
            ("中文 abc".to_string(), "UTF-8")
        );
    }

    /// 不是合法 UTF-8 → 退回 Big5（舊版是系統 ANSI 字碼頁 950）。
    #[test]
    fn falls_back_to_big5() {
        // Big5：中＝A4 A4、文＝A4 E5
        let big5 = vec![0xA4, 0xA4, 0xA4, 0xE5];
        let (text, enc) = decode_text(&big5);
        assert_eq!(text, "中文");
        assert_eq!(enc, "Big5");
    }

    /// 純 ASCII 的 Big5 檔（沒有高位元組）會被當成 UTF-8——這是對的，結果一樣。
    #[test]
    fn ascii_is_utf8() {
        assert_eq!(decode_text(b"hello"), ("hello".to_string(), "UTF-8"));
    }

    /// 換行統一成 `\r\n`（三種輸入都一樣）。
    #[test]
    fn normalizes_newlines() {
        assert_eq!(normalize_newlines("a\nb"), "a\r\nb");
        assert_eq!(normalize_newlines("a\r\nb"), "a\r\nb");
        assert_eq!(normalize_newlines("a\rb"), "a\r\nb");
        assert_eq!(normalize_newlines("a\r\n\nb"), "a\r\n\r\nb");
    }
}
