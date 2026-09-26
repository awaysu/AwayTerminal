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

    /// 逐分頁配色的候選色票（舊版寫死在 `MainWindow.xaml` 的右鍵選單裡，這次搬進設定讓使用者能改）。
    ///
    /// **「per-tab 存哪一層」的決定**：色票清單存在這裡（全域、可編輯），
    /// 但「哪個分頁選了哪一組」只留在記憶體。理由＝分頁 id 跨重啟沒有意義，
    /// 要持久化得等「恢復分頁」把分頁本身存下來（階段 3）。見 docs/REGRESSION-CHECKLIST.md。
    pub palette: Vec<ColorPair>,

    /// `zh` | `en`（中英切換是之後的任務，先存著）。
    pub language: String,
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
            font_family: "Cascadia Mono".to_string(),
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
            // 舊版 MainWindow.xaml 的「配色」子選單那五組，順序照抄
            palette: vec![
                ColorPair { fg: "#FFFF00".into(), bg: "#000000".into() },
                ColorPair { fg: "#F8F8F2".into(), bg: "#282A36".into() },
                ColorPair { fg: "#C9D1D9".into(), bg: "#0D1117".into() },
                ColorPair { fg: "#C0CAF5".into(), bg: "#1A1B26".into() },
                ColorPair { fg: "#EBDBB2".into(), bg: "#282828".into() },
            ],
            language: "zh".to_string(),
        }
    }
}

impl AppSettings {
    /// 舊版 `PostTheme()` 的 `T{json}` 內容。欄位名是 `terminal.js` 認的那些，別改。
    pub fn theme_json(&self) -> serde_json::Value {
        serde_json::json!({
            // 舊版：$"\"{s.FontFamily}\", Consolas, \"Microsoft JhengHei\", \"微軟正黑體\", monospace"
            "fontFamily": format!(
                "\"{}\", Consolas, \"Microsoft JhengHei\", \"微軟正黑體\", monospace",
                self.font_family
            ),
            "fontSize": self.font_size,
            "foreground": self.foreground,
            "background": self.background,
            "imeQuietMs": self.ime_quiet_ms,
            "restoreLines": self.restore_buffer_lines,
            "agentStates": ["閒置", "忙碌", "有信待送", "已結束", "忙碌 · 有信待送"],
            "search": {
                "placeholder": "搜尋",
                "prev": "上一個 (Shift+Enter)",
                "next": "下一個 (Enter)",
                "close": "關閉 (Esc)"
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
            Ok(text) => match serde_json::from_str::<AppSettings>(&text) {
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
