//! `settings.json`：讀寫與套用。
//!
//! 對應舊版 `Services/AppSettings.cs`，預設值照抄。差別：
//!   - 路徑用 Tauri 的 app config dir（Windows＝`%APPDATA%\com.awaysu.awayterminal`），
//!     **不是**舊版的 `%LOCALAPPDATA%\AwayTerminal`。舊設定的匯入是階段 5 的獨立任務，
//!     現在兩邊完全分開，不會互相踩到（舊版踩雷：新舊版共用 settings.json 會「剝欄位」）。
//!   - 欄位只有這個階段用得到的那些；之後每做一個功能就往下加。
//!
//! 寫檔照舊版 `AppSettings.Save()`：先寫 `.tmp` 再原子替換，中途被強制結束不會留半截 JSON。
//! 存檔有防抖（見 [`SettingsStore::mark_dirty`]）——Ctrl+滾輪一路縮放不會每格寫一次檔。

use crate::i18n::{t};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 防抖間隔：最後一次變更之後這麼久沒有新變更才真的寫檔。
const SAVE_DEBOUNCE: Duration = Duration::from_millis(600);

/// 視窗大小／位置（舊版沒存，這次加上；`maximized` 時 x/y/w/h 記的是還原後的大小）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowBounds {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl Default for WindowBounds {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            width: 1200,
            height: 800,
            maximized: false,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    // ---- 字型與顏色（舊版 AppSettings.FontFamily / FontSize / Foreground / Background）----
    pub font_family: String,
    /// 終端機渲染器：`auto`（啟動時實測決定）｜`webgl`｜`canvas`｜`dom`。
    ///
    /// `CLAUDE.md` 風險 2 解法 c：Linux 的 WebKitGTK 上 WebGL 可能被停用或很慢，
    /// 所以要能退回、而且要能手動選（自動選錯時使用者有辦法救自己）。
    #[serde(default = "default_renderer")]
    pub renderer: String,
    pub font_size: u32,
    pub foreground: String,
    pub background: String,

    /// claude 分頁的靜止閘門門檻，0＝關閉（舊版 ImeQuietMs）。
    pub ime_quiet_ms: u32,
    /// 關閉程式時每個分頁保留的 scrollback 行數（舊版 RestoreBufferLines；恢復分頁還沒做，先存著）。
    pub restore_buffer_lines: u32,

    // ---- 分頁列（舊版 TabPanelVisible / TabPanelWidth）----
    pub tab_panel_visible: bool,
    pub tab_panel_width: f64,

    /// 檢視三態：`tab` | `split` | `columns`（舊版 `_viewMode`，只在記憶體裡；這次存檔）。
    pub view_mode: String,

    pub window: WindowBounds,

    // ---- log 記錄（舊版 LogDir / LogTimestamp / LogAppend）----
    /// log 預設資料夾。空字串＝啟動時填成「我的文件\AwayTerminalLogs」（同舊版 `Load()`）。
    pub log_dir: String,
    pub log_timestamp: bool,
    pub log_append: bool,

    /// 上次選過的工作目錄（舊版 `LastDir`）：資料夾選擇視窗會預選它。
    pub last_dir: String,

    // ---- SSH（舊版 KeepAliveMins / AutoReconnect）----
    /// 保持連線的間隔（分鐘），0＝關閉。舊版預設 10。
    pub keep_alive_mins: u32,
    /// 斷線自動重連（舊版連線視窗的勾選）。
    pub auto_reconnect: bool,
    /// 已經接受過「弱演算法」警告的主機（`host:port`）。照 PuTTY：接受過就不再問。
    pub ssh_weak_accepted: Vec<String>,

    // ---- 連接埠（舊版 ComPort / ComBaud / ComDataBits / ComParity / ComStopBits / ComFlow）----
    // 舊版的 COM 對話框開起來就是「上次用的值」，所以這六個欄位要存檔。
    // 字串值照舊版（`None`／`Odd`／`Even`／`Mark`／`Space`、`One`／`Two`／`OnePointFive`、
    // `None`／`XOnXOff`／`RequestToSend`／`RequestToSendXOnXOff`），舊 settings.json 才讀得回來。
    pub com_port: String,
    pub com_baud: u32,
    pub com_data_bits: u8,
    pub com_parity: String,
    pub com_stop_bits: String,
    pub com_flow: String,

    /// 我的最愛（舊版 `AppSettings.Favorites`）。**不存密碼**。
    pub favorites: Vec<crate::favorites::FavoriteItem>,

    // ---- 恢復分頁（舊版 SavedTabs / ExitRestoreTabs）----
    /// 上次關閉時存下來的分頁（下次啟動照這個恢復）。畫面內容另外存在
    /// `{app config dir}/restore/tab{n}.txt`——同舊版的檔案分法。**不含密碼**。
    pub saved_tabs: Vec<crate::restore::SavedTab>,
    /// 離開對話框「下次開啟恢復目前分頁」的勾選狀態（舊版 `ExitRestoreTabs`，預設開）。
    pub exit_restore_tabs: bool,
    /// 離開對話框的「Claude Code 離開前更新 CLAUDE.md」勾選（舊版 `ExitUpdateMd`）。
    /// 預設**不**勾（舊版的 `IsChecked="False"`）。
    #[serde(default)]
    pub exit_update_md: bool,
    /// Telegram 遠端（舊版 `RemoteEnabled`／`TelegramBotToken`／`TelegramChatId`／`RemoteNotify`）。
    ///
    /// **功能是階段 4**；這裡先存著，因為匯入舊版設定時不該把使用者的 token 弄丟
    /// （PM 在 TASK-016 D 指定）。⚠️ token 是機密：不進 log、不進 tooltip、不進恢復分頁。
    pub remote_enabled: bool,
    pub telegram_bot_token: String,
    pub telegram_chat_id: i64,
    pub remote_notify: bool,
    /// **新版多的**：新加入的自訂連線預設要不要開沙盒（設定視窗可改；**預設關**，
    /// 2026-10-02 使用者改的，原本是開）。已存在的連線不受影響。
    /// ⚠️ WSL／ADB 這類「拿來操作機器」的工具即使這裡是開，自動偵測仍然預設關
    ///（見 `custom::default_sandbox`）。
    pub sandbox_default: bool,

    /// 「輸入文字」視窗的「送出後送 Enter」勾選（舊版 `ComposeSendEnter`，預設開）。
    pub compose_send_enter: bool,

    /// 各家 AI CLI **上次選的模型**（key＝`claude-code`／`codex`／`opencode`／`geminicli`；
    /// 空字串＝選了「預設」）。選模型的視窗用它當預選值（2.0.2 新增，見 `agent/models.rs`）。
    pub last_models: std::collections::BTreeMap<String, String>,
    /// 設定視窗的「開啟時選模型」（2.0.3；**預設關**）。開著＝開 AI CLI 的自訂連線時跳
    /// 「選擇模型」、代理團隊設定視窗每一格有模型欄位；關著＝完全不問，照 CLI 自己的預設
    ///（同 2.0.1 以前），恢復分頁與我的最愛也不檢查模型還在不在。
    pub ask_model_on_open: bool,

    /// 自訂連線清單（舊版 `AppSettings.CustomConns`）。
    ///
    /// 舊版 v1.0.18 起**不自動建立任何自訂連線**：全新安裝是空的，使用者自己按
    /// 「自動偵測」一鍵加入想要的工具。照抄這個行為——「我明明全部刪掉了」的清單
    /// 不應該又冒出東西。
    pub custom_conns: Vec<CustomConn>,

    /// 逐分頁配色的候選色票（舊版寫死在 `MainWindow.xaml` 的右鍵選單裡，這次搬進設定讓使用者能改）。
    ///
    /// **「per-tab 存哪一層」的決定**：色票清單存在這裡（全域、可編輯），
    /// 但「哪個分頁選了哪一組」只留在記憶體。理由＝分頁 id 跨重啟沒有意義，
    /// 要持久化得等「恢復分頁」把分頁本身存下來（階段 3）。見 docs/REGRESSION-CHECKLIST.md。
    pub palette: Vec<ColorPair>,

    /// 介面語言：`zh-TW` | `en` | `zh-CN` | `ja` | `ko` | `es` | `de` | `fr`。
    ///
    /// **空字串＝使用者還沒選過** → 第一次啟動時由前端用系統語言對一個（`strings.js`
    /// 的 `matchLang`）並存回來。舊版預設一律繁中、沒有系統語言偵測（見 docs/SETTINGS.md）。
    /// ⚠️ 舊設定檔裡是 `zh`（只有中英兩種的時候）→ 讀進來時當成 `zh-TW`。
    pub language: String,

    /// **不認識的欄位原樣保留。** 對應舊版的 `[JsonExtensionData] ExtraFields`。
    ///
    /// 為什麼（舊版踩雷紀錄第 52 條，2026-07-27 中招、遠端靜默 3 小時才發現）：
    /// 舊的 exe 一存檔就把它不認識的新欄位整組洗掉——那次被洗掉的正是
    /// Telegram 的 token／chatId／RemoteEnabled。會發生的情境：
    ///   * 使用者降版（新版寫過 settings.json，再開舊版）
    ///   * 安裝版與開發版共用同一個設定檔（我們自己測試時最容易中）
    ///
    /// `#[serde(flatten)]` ＋ `serde_json::Map` ＝ 反序列化時把剩下的 key 全部收進來、
    /// 序列化時原樣寫回去。**不要**給它 `skip_serializing_if`：那會讓空的 map 不寫，
    /// 但也會讓「本來有、這次沒讀到」的情況變成刪除。
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// 一條自訂連線。欄位照舊版 `Services/AppSettings.cs` 的 `CustomConn`，
/// 外加 TASK-007 的 `sandbox`（舊版沒有這個欄位）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomConn {
    pub name: String,
    /// 執行檔完整路徑。
    pub path: String,
    pub args: String,
    /// 圖示 key（前端把它對到一個 inline SVG；`run` 是通用的）。
    pub icon: String,
    /// 關閉分頁送的鍵：`ctrl-c`(0x03) / `ctrl-d`(0x04) / `none`。
    pub close_key: String,
    /// 關閉鍵送幾次（1~5）。
    pub close_count: u32,
    /// 啟動前先選工作目錄。
    pub pick_dir: bool,
    /// 隱藏（不列在「新分頁」下拉）。
    pub hidden: bool,
    /// 透過 PowerShell 執行（`.cmd`／需要 shell 時用）。
    pub via_powershell: bool,
    /// **沙盒模式**（TASK-007 新增，`CLAUDE.md`「新增功能」一節）。
    ///
    /// **預設關**（2026-10-02 使用者改的，原本預設開）：設定檔裡沒有這個欄位的連線
    ///（例如從舊版匯入的）一律當成沒開沙盒。
    pub sandbox: bool,
}

impl Default for CustomConn {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: String::new(),
            args: String::new(),
            icon: "run".to_string(),
            close_key: "ctrl-c".to_string(),
            close_count: 3,
            pick_dir: false,
            hidden: false,
            via_powershell: false,
            sandbox: false,
        }
    }
}

impl CustomConn {
    /// 關閉分頁時送的位元組（舊版 `OpenCustom` 的 `closeBytes`）。
    pub fn close_bytes(&self) -> Vec<u8> {
        if self.close_key == "none" {
            return Vec::new();
        }
        let byte = if self.close_key == "ctrl-d" { 0x04 } else { 0x03 };
        let count = if (1..=5).contains(&self.close_count) {
            self.close_count
        } else {
            3
        };
        vec![byte; count as usize]
    }
}

/// 一組前景／背景色（逐分頁配色的色票）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ColorPair {
    pub fg: String,
    pub bg: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            font_family: default_font_family().to_string(),
            renderer: default_renderer(),
            font_size: 14,
            foreground: "#E0E0E0".to_string(),
            background: "#1E1E1E".to_string(),
            ime_quiet_ms: 20,
            restore_buffer_lines: 2000,
            tab_panel_visible: true,
            tab_panel_width: 220.0,
            view_mode: "tab".to_string(),
            window: WindowBounds::default(),
            log_dir: String::new(),
            log_timestamp: true,
            log_append: true,
            last_dir: String::new(),
            keep_alive_mins: 10,
            auto_reconnect: false,
            ssh_weak_accepted: Vec::new(),
            // 舊版 AppSettings 的預設：COM5 / 115200 / 8 / None / One / None
            com_port: "COM5".to_string(),
            com_baud: 115_200,
            com_data_bits: 8,
            com_parity: "None".to_string(),
            com_stop_bits: "One".to_string(),
            com_flow: "None".to_string(),
            favorites: Vec::new(),
            saved_tabs: Vec::new(),
            exit_restore_tabs: true,
            exit_update_md: false,
            remote_enabled: false,
            telegram_bot_token: String::new(),
            telegram_chat_id: 0,
            remote_notify: false,
            sandbox_default: false,
            compose_send_enter: true,
            last_models: std::collections::BTreeMap::new(),
            ask_model_on_open: false,
            custom_conns: Vec::new(),
            // 舊版 MainWindow.xaml 的「配色」子選單那五組，順序照抄
            palette: vec![
                ColorPair { fg: "#FFFF00".into(), bg: "#000000".into() },
                ColorPair { fg: "#F8F8F2".into(), bg: "#282A36".into() },
                ColorPair { fg: "#C9D1D9".into(), bg: "#0D1117".into() },
                ColorPair { fg: "#C0CAF5".into(), bg: "#1A1B26".into() },
                ColorPair { fg: "#EBDBB2".into(), bg: "#282828".into() },
            ],
            language: String::new(),
            extra: serde_json::Map::new(),
        }
    }
}

/// 渲染器的預設值：`auto` ＝啟動時實測決定（WebGL → canvas → DOM）。
fn default_renderer() -> String {
    "auto".to_string()
}

/// 使用者選的字型後面接的 fallback 清單，依平台。
///
/// | 平台 | 等寬 | 中文 |
/// |---|---|---|
/// | Windows | Cascadia Mono → Consolas | Microsoft JhengHei（微軟正黑體） |
/// | macOS | Menlo → SF Mono | PingFang TC → Heiti TC |
/// | Linux | DejaVu Sans Mono → Liberation Mono | Noto Sans Mono CJK TC → Noto Sans CJK TC |
///
/// 最後一定要有 `monospace`——前面全都沒裝時至少還是等寬（不等寬的話 xterm 的
/// 欄位對位會整片跑掉）。
///
/// 舊版那一組是 `Consolas, "Microsoft JhengHei", "微軟正黑體", monospace`，
/// Windows 這一欄逐字沿用（多了 Cascadia Mono 當第一順位，那是新版的預設字型）。
pub fn font_fallback() -> &'static str {
    // ⚠️ **自帶的兩套排在系統字型前面**（TASK-033）：`Sarasa Mono TC` 是我們打包進去的
    // 中英文等寬中文字型（中文剛好兩個英文字寬），`Cascadia Mono` 是自帶的英文等寬。
    // 它們一定在（跟著安裝檔走），所以「新機器上中文變成不等寬」這件事不會再發生。
    #[cfg(target_os = "macos")]
    {
        "\"Cascadia Mono\", \"Sarasa Mono TC\", Menlo, \"SF Mono\", \"PingFang TC\", \"Heiti TC\", monospace"
    }
    #[cfg(target_os = "linux")]
    {
        "\"Cascadia Mono\", \"Sarasa Mono TC\", \"DejaVu Sans Mono\", \"Liberation Mono\", \"Noto Sans Mono CJK TC\", \"Noto Sans CJK TC\", monospace"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "\"Cascadia Mono\", \"Sarasa Mono TC\", Consolas, \"Microsoft JhengHei\", \"微軟正黑體\", monospace"
    }
}

/// 預設字型（**設定檔第一次建立時**才會用到）。
///
/// TASK-033 起是**自帶的 `JetBrains Mono`**，三個平台一樣——以前是依平台猜一個系統字型
/// （Windows `Cascadia Mono`／mac `Menlo`／Linux `DejaVu Sans Mono`），
/// 三台機器長得不一樣，而且 Linux 上那套不一定裝了。
///
/// ⚠️ **已經有 settings.json 的使用者不會被改到**：`font_family` 是從檔案讀出來的，
/// 這個函式只在「檔案不存在」或「舊檔案裡根本沒有這個欄位」時才會被問到。
pub fn default_font_family() -> &'static str {
    "JetBrains Mono"
}

impl AppSettings {
    /// 舊版 `PostTheme()` 的 `T{json}` 內容。欄位名是 `terminal.js` 認的那些，別改。
    pub fn theme_json(&self) -> serde_json::Value {
        serde_json::json!({
            // 使用者選的字型 ＋ **依平台的 fallback**（`CLAUDE.md` 平台差異表的
            // 「中文字型 fallback」）。舊版只有 Windows 那一組。
            "fontFamily": format!("\"{}\", {}", self.font_family, font_fallback()),
            "fontSize": self.font_size,
            // 前端照這個決定要掛哪個 renderer addon（`auto` ＝自己實測）
            "renderer": self.renderer,
            "foreground": self.foreground,
            "background": self.background,
            "imeQuietMs": self.ime_quiet_ms,
            "restoreLines": self.restore_buffer_lines,
            // 這幾個字前端直接顯示 → 隨語言（舊版 `PostTheme()` 也是 `Loc.T` 進 JSON）
            "agentStates": [
                t("ma.stateIdle"),
                t("ma.stateBusy"),
                t("ma.stateQueued"),
                t("ma.stateExited"),
                t("ma.stateBusyQueued"),
            ],
            "search": {
                "placeholder": t("search.placeholder"),
                "prev": t("search.prev"),
                "next": t("search.next"),
                "close": t("search.close")
            }
        })
    }

    /// 字級的合法範圍照舊版 `z` 協定處理（6~40）。
    pub fn clamp_font_size(size: u32) -> Option<u32> {
        (6..=40).contains(&size).then_some(size)
    }
}

/// 設定檔的持有者。放在 tauri `State` 裡。
pub struct SettingsStore {
    path: PathBuf,
    inner: Mutex<AppSettings>,
    dirty: Arc<AtomicBool>,
    /// 讀檔失敗那一次不要寫回去（同舊版 `_suppressSave`）：否則一個暫時的讀取錯誤
    /// 會把使用者的設定整份換成預設值。
    writable: bool,
}

impl SettingsStore {
    /// 讀設定檔；檔案不存在＝全新安裝，用預設值（並允許寫回）。
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("settings.json");
        let (settings, writable) = match std::fs::read_to_string(&path) {
            // 帶 UTF-8 BOM 的檔（PowerShell 5.1 `Out-File` 存的就有）serde_json 讀不進來 →
            // 以前整份設定靜默丟掉、這個工作階段什麼都存不了（BUG D6）。先剝掉 BOM。
            Ok(text) => match serde_json::from_str::<AppSettings>(
                text.strip_prefix('\u{feff}').unwrap_or(&text),
            ) {
                Ok(s) => (s, true),
                Err(e) => {
                    println!("[AwayTerminal] settings.json 解析失敗，這次不寫回：{e}");
                    (AppSettings::default(), false)
                }
            },
            // 全新安裝：用預設值，並排一次存檔——讓 settings.json 一開始就存在，
            // 使用者（和我們驗證時）看得到它、也知道可以改哪些欄位
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (AppSettings::default(), true),
            Err(e) => {
                println!("[AwayTerminal] settings.json 讀取失敗，這次不寫回：{e}");
                (AppSettings::default(), false)
            }
        };
        println!("[AwayTerminal] settings: {}", path.display());
        let fresh = writable && !path.exists();
        Self {
            path,
            inner: Mutex::new(settings),
            dirty: Arc::new(AtomicBool::new(fresh)),
            writable,
        }
    }

    /// 這次啟動的設定**存不回去**嗎（讀檔或解析失敗 → 為了不蓋掉使用者的檔，整個工作階段
    /// 都不寫）。回傳已翻譯、給使用者看的說明；可以存時回 `None`。
    ///
    /// 會改設定的指令（匯入舊版設定、設定視窗按確定）要用它告訴使用者「改了也不會留下來」，
    /// 不然就是靜默丟失（BUG D6）。
    pub fn readonly_reason(&self) -> Option<String> {
        (!self.writable)
            .then(|| crate::i18n::tf("err.settingsReadOnly", &[&self.path.display().to_string()]))
    }

    /// 設定檔所在的資料夾（`known_hosts` 之類的東西也放這裡）。
    pub fn dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn get(&self) -> AppSettings {
        self.lock().clone()
    }

    /// 空的 `log_dir` 填成「我的文件\AwayTerminalLogs」（同舊版 `AppSettings.Load()`）。
    /// 路徑由 Tauri 的 path resolver 給，所以 mac/Linux 也會落在各自的文件資料夾。
    pub fn fill_log_dir(&self, documents: &Path) {
        let mut inner = self.lock();
        if inner.log_dir.trim().is_empty() {
            inner.log_dir = documents
                .join("AwayTerminalLogs")
                .to_string_lossy()
                .to_string();
            drop(inner);
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    /// 改設定並排一次防抖存檔。
    pub fn update(&self, f: impl FnOnce(&mut AppSettings)) -> AppSettings {
        let copy = {
            let mut s = self.lock();
            f(&mut s);
            s.clone()
        };
        self.dirty.store(true, Ordering::Relaxed);
        copy
    }

    /// 立刻寫檔（程式結束時用）。
    pub fn flush(&self) {
        if !self.writable {
            return;
        }
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return;
        }
        let snapshot = self.lock().clone();
        self.write(&snapshot);
    }

    fn write(&self, settings: &AppSettings) {
        let Some(dir) = self.path.parent() else { return };
        if let Err(e) = std::fs::create_dir_all(dir) {
            println!("[AwayTerminal] 建立設定資料夾失敗：{e}");
            return;
        }
        let json = match serde_json::to_string_pretty(settings) {
            Ok(j) => j,
            Err(e) => {
                println!("[AwayTerminal] 設定序列化失敗：{e}");
                return;
            }
        };
        // 先寫暫存檔再原子替換（同舊版）：中途被強制結束不會留下半截 JSON
        let tmp = self.path.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp, json) {
            println!("[AwayTerminal] 設定寫入失敗：{e}");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &self.path) {
            println!("[AwayTerminal] 設定替換失敗：{e}");
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, AppSettings> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 防抖存檔執行緒：`dirty` 被設起來之後靜置 [`SAVE_DEBOUNCE`] 才真的寫。
///
/// Ctrl+滾輪縮放一路轉會連續送十幾個 `z`，逐次寫檔既慢又會磨 SSD；
/// 這裡讓連續變更只落地一次。
pub fn spawn_autosave(store: Arc<SettingsStore>) {
    std::thread::spawn(move || {
        let mut quiet = Duration::ZERO;
        loop {
            std::thread::sleep(Duration::from_millis(200));
            if store.dirty.load(Ordering::Relaxed) {
                quiet += Duration::from_millis(200);
                if quiet >= SAVE_DEBOUNCE {
                    quiet = Duration::ZERO;
                    store.flush();
                }
            } else {
                quiet = Duration::ZERO;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **不認識的欄位要原樣保留**（舊版踩雷第 52 條：舊 exe 一存檔就把新欄位洗掉，
    /// 那次被洗掉的是 Telegram 的 token）。
    ///
    /// 會發生的情境：使用者降版、或安裝版與開發版共用同一個 settings.json。
    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let json = r#"{
            "fontSize": 18,
            "someFutureFeature": { "a": 1, "b": ["x"] },
            "telegramBotToken": "keep-me"
        }"#;
        let s: AppSettings = serde_json::from_str(json).expect("要讀得進來");
        assert_eq!(s.font_size, 18, "認識的欄位照舊");
        assert_eq!(s.telegram_bot_token, "keep-me");
        assert!(s.extra.contains_key("someFutureFeature"), "不認識的欄位被吃掉了");

        let out = serde_json::to_string(&s).expect("要寫得出來");
        let back: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            back["someFutureFeature"]["a"], 1,
            "寫回去的時候不認識的欄位不見了——這就是第 52 條的資料遺失"
        );
        assert_eq!(back["fontSize"], 18);
    }

    /// 認識的欄位**不可以**跑進 `extra`（不然會寫出重複的 key）。
    #[test]
    fn known_fields_do_not_leak_into_extra() {
        let json = r#"{ "fontSize": 20, "language": "ja" }"#;
        let s: AppSettings = serde_json::from_str(json).unwrap();
        assert!(s.extra.is_empty(), "extra 撿到了認識的欄位：{:?}", s.extra);
    }

    /// 預設值寫出來的 JSON 不含 `extra` 這個 key（`flatten` 是攤平，不是巢狀）。
    #[test]
    fn flatten_does_not_add_a_nested_key() {
        let out = serde_json::to_string(&AppSettings::default()).unwrap();
        assert!(!out.contains("\"extra\""), "flatten 沒生效：{out}");
    }

    /// BUG D6：帶 BOM 的設定檔要讀得進來，而且之後照常可以存。
    #[test]
    fn a_bom_file_is_read_and_stays_writable() {
        let dir = std::env::temp_dir().join(format!("awayterm-settings-bom-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::write(&path, "\u{feff}{\"fontSize\": 21}").unwrap();

        let store = SettingsStore::load(&dir);
        assert_eq!(store.get().font_size, 21, "BOM 檔的內容要讀得到");
        assert!(store.readonly_reason().is_none(), "BOM 檔不可以被當成壞檔");
        store.update(|s| s.font_size = 22);
        store.flush();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("22"), "要存得回去：{after}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 解析失敗時**不可以**寫回去（不然使用者的檔案會被預設值蓋掉）。
    ///
    /// v2 的做法比舊版更保守：舊版是先備份 `settings.json.bad` 再退預設，
    /// 我們是**整個不寫**（`writable = false`），原檔完全不動。
    #[test]
    fn a_broken_file_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("awayterm-settings-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        let broken = "{ this is not json";
        std::fs::write(&path, broken).unwrap();

        let store = SettingsStore::load(&dir);
        store.update(|s| s.font_size = 99);
        store.flush();

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, broken, "壞掉的設定檔被覆寫了");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
