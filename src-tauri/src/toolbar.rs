//! 工具列與右鍵選單的動作（TASK-005）。
//!
//! 全部照舊版 `MainWindow.xaml.cs` 的 handler 逐一對應，函式名後面註明來源：
//!
//! | 新版 command | 舊版 | 送出的協定 |
//! |---|---|---|
//! | `toolbar_copy` | `Copy_Click` | `q{id}US sel` |
//! | `toolbar_copy_all` | `CopyAll_Click` | `q{id}US all` |
//! | `toolbar_copy_all_file` | 右鍵「複製全部存至檔案」 | `q{id}US file` |
//! | `toolbar_copy_paste` | 右鍵「複製且貼上」 | `q{id}US selpaste` |
//! | `toolbar_paste` | `Paste_Click` → `PasteToActive` → `PasteToTab` | `v{id}US{base64}` |
//! | `toolbar_clear` | `Clear_Click` | PowerShell/SSH＝Esc → 60ms → Ctrl+L；其餘＝`c{id}` |
//! | `toolbar_scroll` | `Page_Click` → `ScrollActive` | `S{id}US{up\|down\|top\|bottom}` |
//! | `toolbar_search` | 右鍵「搜尋」 | `F` |
//! | `tab_colors` | `MenuColor_Click` | `P{id}US{fg}US{bg}` |
//! | `save_text_to_file` | `SaveBufferToFile` | （無，直接寫檔） |
//! | `pick_work_dir` | `PickWorkDir` | （無） |
//! | `log_start` / `log_stop` | `LogAction` / `StopLogging` | （無） |
//! | `open_url` | `ShowUrlMenu` → `OpenUrlExternal` | （無） |
//!
//! **確認對話框與 toast 都在前端**（舊版是 WPF `MessageBox` / `Popup`）；這裡只負責動作。

use crate::i18n::{t, tf};
use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::host::emit_host;
use crate::logging::{self, Logger};
use crate::session::SessionManager;
use crate::settings::SettingsStore;
use crate::tabs::{self, TabKind, TabManager};

// ------------------------------------------------------------ 複製 / 查詢

/// 複製選取內容（舊版 `Copy_Click`）。回覆走 `a{id}US sel US{text}`，由前端寫剪貼簿。
#[tauri::command]
pub fn toolbar_copy(app: AppHandle, id: u32) {
    emit_host(&app, format!("q{id}\x1fsel"));
}

/// 複製全部緩衝文字（舊版 `CopyAll_Click`）。注意 `all` 拿的是**含顏色**的序列化文字
/// （`ser.serialize()`），與 `file`／`text` 的純文字不同——這是舊版的行為。
#[tauri::command]
pub fn toolbar_copy_all(app: AppHandle, id: u32) {
    emit_host(&app, format!("q{id}\x1fall"));
}

/// 複製全部存至檔案（舊版右鍵 `ctx.copyAllFile`）。回覆走 `a…file` → [`save_text_to_file`]。
#[tauri::command]
pub fn toolbar_copy_all_file(app: AppHandle, id: u32) {
    emit_host(&app, format!("q{id}\x1ffile"));
}

/// 複製且貼上（舊版右鍵 `ctx.copyPaste`）：回覆同 `sel`，前端寫完剪貼簿再貼回同一個分頁。
#[tauri::command]
pub fn toolbar_copy_paste(app: AppHandle, id: u32) {
    emit_host(&app, format!("q{id}\x1fselpaste"));
}

/// 純文字貼上（舊版 `Paste_Click` → `PasteToTab`）。
///
/// **一定要走 `v` 協定**（`xterm.paste()`），不可以直接寫 session：
/// 直接寫會讓每個換行被當成 Enter 送出，只剩最後一行留在輸入框（舊版 v1.0.7 踩雷）。
/// claude 分頁由 `terminal.js` 的 `doPaste` 自己換成 ESC+CR 軟換行。
#[tauri::command]
pub fn toolbar_paste(app: AppHandle, id: u32, text: String) {
    if text.is_empty() {
        return;
    }
    emit_host(
        &app,
        format!("v{id}\x1f{}", crate::b64::encode(text.as_bytes())),
    );
}

/// 清畫面（舊版 `Clear_Click`，**確認對話框在前端**）。
///
/// 兩條路的代價不同：PowerShell / SSH 只是叫 shell 重畫，xterm 的 scrollback 還在；
/// Telnet / COM 走 `term.clear()` 會把 scrollback 洗掉、救不回來。
/// Esc 與 Ctrl+L **不能黏著送**，PSReadLine 會當成 escape 序列而兩者都失效（舊版註解）。
#[tauri::command]
pub fn toolbar_clear(
    app: AppHandle,
    id: u32,
    manager: State<'_, SessionManager>,
    tabs_state: State<'_, Arc<TabManager>>,
) {
    let kind = tabs_state.kind_of(id);
    let shell_like = matches!(kind, Some(TabKind::PowerShell) | Some(TabKind::Ssh));
    if !shell_like {
        emit_host(&app, format!("c{id}"));
        return;
    }
    let Some(session) = manager.get(id) else { return };
    // 60ms 的等待不能卡 IPC 執行緒
    std::thread::spawn(move || {
        session.write(&[0x1B]); // Esc：清掉還沒送出的輸入
        std::thread::sleep(std::time::Duration::from_millis(60));
        session.write(&[0x0C]); // Ctrl+L：清畫面
    });
}

/// 翻頁（舊版 `ScrollActive`）。只動視窗、不送任何輸入。
#[tauri::command]
pub fn toolbar_scroll(app: AppHandle, id: u32, action: String) {
    if !matches!(action.as_str(), "up" | "down" | "top" | "bottom") {
        return;
    }
    emit_host(&app, format!("S{id}\x1f{action}"));
}

/// 全選（`A{id}`）。
///
/// ⚠️ **舊版 1.2.x 沒有這個功能**：`Localization/Loc.cs` 留著 `ctx.selectAll` 字串，
/// 但整份 `MainWindow.xaml.cs` 沒有任何 `PostToWeb("A…")`，`A` 是死協定。
/// TASK-006 由 PM 決定當作**新功能**補上（`terminal.js` 的 `A` 分支本來就能用）。
#[tauri::command]
pub fn toolbar_select_all(app: AppHandle, id: u32) {
    emit_host(&app, format!("A{id}"));
}

/// 開搜尋列（舊版右鍵 `ctx.search`）。`F` 沒有 id 欄位，`terminal.js` 對作用中那個 pane 開。
#[tauri::command]
pub fn toolbar_search(app: AppHandle) {
    emit_host(&app, "F");
}

/// 逐分頁配色（舊版 `MenuColor_Click`）。`fg`/`bg` 皆空＝清除覆寫、回到設定預設。
#[tauri::command]
pub fn tab_colors(
    app: AppHandle,
    id: u32,
    fg: String,
    bg: String,
    tabs_state: State<'_, Arc<TabManager>>,
) {
    if !tabs_state.set_colors(id, &fg, &bg) {
        return;
    }
    emit_host(&app, format!("P{id}\x1f{fg}\x1f{bg}"));
}

// ------------------------------------------------------------------ 檔案

/// 「複製全部存至檔案」的落地（舊版 `SaveBufferToFile`）。
///
/// 預設檔名 `{分頁名稱}-{yyyyMMdd-HHmmss}.txt`；編碼與舊版 `File.WriteAllText(…, Encoding.UTF8)`
/// 一致＝**UTF-8 with BOM**（.NET 的 `Encoding.UTF8` 靜態屬性會輸出 BOM）。
/// 回傳實際存到哪裡；使用者取消回 `None`。
#[tauri::command]
pub fn save_text_to_file(
    app: AppHandle,
    id: u32,
    text: String,
    tabs_state: State<'_, Arc<TabManager>>,
) -> Result<Option<String>, String> {
    let title = tabs_state.title_of(id).unwrap_or_default();
    let name = logging::default_save_name(&title);
    let picked = app
        .dialog()
        .file()
        .set_file_name(&name)
        .add_filter("Text file", &["txt"])
        .add_filter("All files", &["*"])
        .blocking_save_file();
    let Some(path) = picked.and_then(to_path) else {
        return Ok(None);
    };
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(text.as_bytes());
    std::fs::write(&path, bytes).map_err(|e| tf("err.saveFailed", &[&e.to_string()]))?;
    println!("[AwayTerminal] 已存檔 {} ({} bytes)", path.display(), text.len());
    Ok(Some(path.to_string_lossy().to_string()))
}

/// 選工作目錄（舊版 `PickWorkDir`）。預選上次選過的資料夾並記住新的選擇。
///
/// 舊版對 `LastDir` 做了「背景執行緒探測、最多等 300ms」的保護（休眠的硬碟／斷線的網路磁碟
/// 會讓 `Directory.Exists` 卡住好幾秒 → 對話框遲遲不出現）。這裡同樣不在主執行緒上檢查存在性：
/// 直接把路徑交給對話框，開不開得起來由系統決定。
#[tauri::command]
pub fn pick_work_dir(
    app: AppHandle,
    title: String,
    settings: State<'_, Arc<SettingsStore>>,
) -> Option<String> {
    let last = settings.get().last_dir;
    let mut builder = app.dialog().file().set_title(&title);
    if !last.trim().is_empty() {
        builder = builder.set_directory(&last);
    }
    let path = builder.blocking_pick_folder().and_then(to_path)?;
    let dir = path.to_string_lossy().to_string();
    settings.update(|s| s.last_dir = dir.clone());
    Some(dir)
}

/// 網址選單的「從瀏覽器開啟」（舊版 `OpenUrlExternal`）。
///
/// **只放行 http/https**：擋掉 `file:`、`javascript:` 等，避免點到終端機輸出的怪字串
/// 就觸發本機動作（舊版 1.1.6 的註解）。
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(tf("err.onlyHttp", &[url]));
    }
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

// ------------------------------------------------------------------- log

/// log 對話框要用的預設值（舊版 `LogDialog` 的建構子）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogDefaults {
    /// 完整預設路徑＝`{LogDir}\{分頁名稱}_{yyyyMMdd_HHmmss}.log`
    pub path: String,
    pub timestamp: bool,
    pub append: bool,
}

#[tauri::command]
pub fn log_defaults(
    id: u32,
    tabs_state: State<'_, Arc<TabManager>>,
    settings: State<'_, Arc<SettingsStore>>,
) -> LogDefaults {
    let s = settings.get();
    let title = tabs_state.title_of(id).unwrap_or_default();
    let path = std::path::Path::new(&s.log_dir).join(logging::default_log_name(&title));
    LogDefaults {
        path: path.to_string_lossy().to_string(),
        timestamp: s.log_timestamp,
        append: s.log_append,
    }
}

/// 讓使用者挑 log 檔位置（舊版 `LogDialog` 的「瀏覽…」）。
#[tauri::command]
pub fn log_pick_path(app: AppHandle, current: String) -> Option<String> {
    let p = std::path::Path::new(&current);
    let mut builder = app
        .dialog()
        .file()
        .add_filter("Log file", &["log"])
        .add_filter("All files", &["*"]);
    if let Some(name) = p.file_name() {
        builder = builder.set_file_name(name.to_string_lossy().as_ref());
    }
    if let Some(dir) = p.parent() {
        if dir.is_dir() {
            builder = builder.set_directory(dir);
        }
    }
    builder
        .blocking_save_file()
        .and_then(to_path)
        .map(|x| x.to_string_lossy().to_string())
}

/// 開始記錄（舊版 `LogAction` 的成功分支）。也把路徑／選項存回設定，同舊版 `LogDialog.Ok_Click`。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn log_start(
    app: AppHandle,
    id: u32,
    path: String,
    timestamp: bool,
    append: bool,
    // `remember = false` ＝不要把這次的位置／選項寫回設定（`--verify` 用，
    // 免得驗證改掉使用者的 log 資料夾）。省略＝寫回（正常按「開始記錄」的行為）。
    remember: Option<bool>,
    tabs_state: State<'_, Arc<TabManager>>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(t("err.needLogPath").to_string());
    }
    let slot = tabs_state
        .logger_slot(id)
        .ok_or_else(|| tf("err.tabNotFound", &[&id.to_string()]))?;
    // 開檔有逾時保護：這台機器實測「我的文件」被防毒擋住時會**永遠不返回**，
    // 而這個 command 跑在 IPC 執行緒上（見 Logger::open_with_timeout 的說明）。
    let logger = Logger::open_with_timeout(
        std::path::Path::new(path),
        timestamp,
        append,
        std::time::Duration::from_secs(3),
    )
    .map_err(|e| tf("err.logStartFailed", &[&e.to_string()]))?;

    if remember.unwrap_or(true) {
        settings.update(|s| {
            if let Some(dir) = std::path::Path::new(path).parent() {
                if !dir.as_os_str().is_empty() {
                    s.log_dir = dir.to_string_lossy().to_string();
                }
            }
            s.log_timestamp = timestamp;
            s.log_append = append;
        });
    }

    let real = logger.path().to_string_lossy().to_string();
    {
        let mut g = slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = g.take() {
            old.close();
        }
        *g = Some(Arc::new(logger));
    }
    println!("[AwayTerminal] log 開始：分頁 {id} → {real}（時間戳={timestamp} append={append}）");
    tabs::emit_state(&app, &tabs_state);
    Ok(real)
}

/// 給**巨集**（`logopen`）用的版本：不碰設定、不需要 tauri 的 `State`。
///
/// 和 `log_start` 共用 `Logger::open_with_timeout`（開檔有 3 秒逾時保護，見 logging.rs）。
pub fn log_start_inner(
    app: &AppHandle,
    tabs_state: &Arc<TabManager>,
    id: u32,
    path: &str,
    timestamp: bool,
    append: bool,
) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(t("err.macroLogNoName").to_string());
    }
    let slot = tabs_state
        .logger_slot(id)
        .ok_or_else(|| tf("err.tabNotFound", &[&id.to_string()]))?;
    let logger = Logger::open_with_timeout(
        std::path::Path::new(path),
        timestamp,
        append,
        std::time::Duration::from_secs(3),
    )
    .map_err(|e| tf("err.logStartFailed", &[&e.to_string()]))?;
    let real = logger.path().to_string_lossy().to_string();
    {
        let mut g = slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = g.take() {
            old.close();
        }
        *g = Some(Arc::new(logger));
    }
    println!("[AwayTerminal] log 開始（巨集）：分頁 {id} → {real}");
    tabs::emit_state(app, tabs_state);
    Ok(real)
}

/// 給**巨集**（`logclose`）用的版本。
pub fn log_stop_inner(tabs_state: &Arc<TabManager>, id: u32) {
    if let Some(slot) = tabs_state.logger_slot(id) {
        let logger = {
            let mut g = slot.lock().unwrap_or_else(|e| e.into_inner());
            g.take()
        };
        if let Some(l) = logger {
            l.close();
            println!(
                "[AwayTerminal] log 停止（巨集）：分頁 {id} → {}",
                l.path().display()
            );
        }
    }
}

/// 停止記錄（舊版 `StopLogging`）。回傳剛才寫到哪個檔案，前端可以據此開資料夾。
#[tauri::command]
pub fn log_stop(
    app: AppHandle,
    id: u32,
    tabs_state: State<'_, Arc<TabManager>>,
) -> Option<String> {
    let slot = tabs_state.logger_slot(id)?;
    let logger = {
        let mut g = slot.lock().unwrap_or_else(|e| e.into_inner());
        g.take()
    }?;
    logger.close();
    let path = logger.path().to_string_lossy().to_string();
    println!("[AwayTerminal] log 停止：分頁 {id} → {path}");
    tabs::emit_state(&app, &tabs_state);
    Some(path)
}

/// 在檔案總管裡選取那個 log 檔（舊版 `StopLogging` 的 `explorer /select,`）。
#[tauri::command]
pub fn reveal_path(app: AppHandle, path: String) -> Result<(), String> {
    tauri_plugin_opener::OpenerExt::opener(&app)
        .reveal_item_in_dir(std::path::PathBuf::from(path))
        .map_err(|e| e.to_string())
}

/// **只給 `--verify` 用**：把文字寫到指定路徑（驗證要先產一支小巨集）。
///
/// 和 `save_text_to_file` 的差別：那個會跳存檔對話框（要使用者選位置），
/// 這個直接寫——所以**只接受系統暫存資料夾底下的路徑**，免得被當成任意寫檔的後門。
#[tauri::command]
pub fn save_text_to_file_at(path: String, text: String) -> Result<String, String> {
    let p = std::path::PathBuf::from(&path);
    let temp = std::env::temp_dir();
    if !p.starts_with(&temp) {
        return Err(tf("err.tempOnlyPath", &[&temp.display().to_string()]));
    }
    std::fs::write(&p, text.as_bytes()).map_err(|e| tf("err.writeFailed", &[&e.to_string()]))?;
    Ok(p.to_string_lossy().to_string())
}

/// 選一支 `.ttl` 巨集（分頁右鍵「執行巨集…」）。
///
/// 篩選器照舊版 `MacroAction`：「TeraTerm 巨集 (*.ttl)」＋「所有檔案」。
#[tauri::command]
pub async fn macro_pick_file(app: AppHandle, title: Option<String>) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .set_title(title.unwrap_or_else(|| t("dlg.pickMacro")))
        .add_filter(t("dlg.teratermMacro"), &["ttl"])
        .add_filter(t("dlg.allFiles"), &["*"])
        .pick_file(move |f| {
            let _ = tx.send(f);
        });
    let picked = tokio::task::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .ok()
        .flatten()?;
    Some(picked.to_string())
}

/// 系統暫存資料夾。
///
/// 兩個用途：`--verify` 的自動驗證要一個「一定寫得進去」的位置（預設的
/// 「我的文件」在這台機器被防毒擋住，見 `Logger::open_with_timeout`），
/// 以及之後 log／存檔失敗時可以拿它當後備位置建議給使用者。
#[tauri::command]
pub fn temp_dir() -> String {
    std::env::temp_dir().to_string_lossy().to_string()
}

fn to_path(p: FilePath) -> Option<std::path::PathBuf> {
    p.into_path().ok()
}
