//! 設定視窗的後端（搬移舊版 `Dialogs/SettingsDialog` 的「按確定才套用」那一段）。
//!
//! | 舊版 | 出處 | 這裡 |
//! |---|---|---|
//! | 按「確定」→ 寫進 `AppSettings` → `Save()` | `Ok_Click` | [`settings_apply`]（一次收全部欄位，走既有的防抖寫檔） |
//! | 按確定後 `PostTheme()` 重送 `T{json}` | `Settings_Click` | 同（[`settings_apply`] 自己 emit） |
//! | 按確定後 `ApplyWebDefaultBg()` | 同上 | 前端自己設 `body` 背景（我們沒有 WebView2 的 `DefaultBackgroundColor`） |
//! | `Loc.SetLang(lang)` | 同上 | [`crate::i18n::set_lang`] ＋前端 `setLang()`，**不必重啟** |
//! | 按「取消」什麼都不動 | `IsCancel` | 前端不呼叫這個 command 就好 |
//!
//! 舊版對話框只有 5 個欄位（語言／字型／字級／前景／背景／`imeQuietMs`／檔案總管選單）。
//! PM 在 TASK-015 要求「`settings.json` 已有的欄位全部要能從這裡改」，所以多收了
//! `restoreBufferLines`／`keepAliveMins`／`autoReconnect`／log 三項／`exitRestoreTabs`／
//! 沙盒預設。哪些是舊版就有的、哪些是新版多的，對照表在 `docs/SETTINGS.md`。

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::settings::{AppSettings, SettingsStore};

/// 設定視窗按「確定」時送回來的東西。**每個欄位都是 `Option`**：
/// 沒帶的就不動（前端只送它真的有 UI 的欄位，之後加欄位不必改這個結構）。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefsPatch {
    pub language: Option<String>,
    pub font_family: Option<String>,
    pub font_size: Option<u32>,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub ime_quiet_ms: Option<u32>,
    pub restore_buffer_lines: Option<u32>,
    pub keep_alive_mins: Option<u32>,
    pub auto_reconnect: Option<bool>,
    pub log_dir: Option<String>,
    pub log_timestamp: Option<bool>,
    pub log_append: Option<bool>,
    pub exit_restore_tabs: Option<bool>,
    pub sandbox_default: Option<bool>,
}

/// 顏色字串的驗證（舊版 `ValidColor`：認不出來就退回預設）。
///
/// 只接受 `#RGB`／`#RRGGBB`——xterm.js 的 `theme` 也只吃這種，
/// 舊版 WPF 的 `ColorConverter` 還吃 `Red` 這類名稱，但那些在 CSS 與 xterm 之間
/// 行為不一致（`Red` 在 xterm 6 是可以的，但 `#` 開頭才保證一致），所以收窄。
pub fn valid_color(text: &str, fallback: &str) -> String {
    let t = text.trim();
    let hex = t.strip_prefix('#').unwrap_or("");
    if (hex.len() == 3 || hex.len() == 6) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        t.to_string()
    } else {
        fallback.to_string()
    }
}

/// 套用設定（舊版 `Ok_Click` ＋ `Settings_Click` 的後半段）。
///
/// 回傳套用後的完整設定，前端拿去重畫（也用來驗證 clamp 的結果）。
#[tauri::command]
pub fn settings_apply(
    app: AppHandle,
    patch: PrefsPatch,
    settings: State<'_, Arc<SettingsStore>>,
) -> AppSettings {
    let after = settings.update(|s| {
        if let Some(lang) = &patch.language {
            // 認得的八種才收（前端的 `LANGS`）；認不出來就不動，避免把設定寫壞
            if crate::i18n::is_supported_lang(lang) {
                s.language = lang.clone();
            }
        }
        if let Some(f) = &patch.font_family {
            // 舊版：空白就退回 Cascadia Mono
            let f = f.trim();
            s.font_family = if f.is_empty() {
                AppSettings::default().font_family
            } else {
                f.to_string()
            };
        }
        if let Some(size) = patch.font_size {
            // 舊版收 6~72 再由 xterm 自己處理；我們照 `z` 協定的 6~40（見 clamp_font_size）
            s.font_size = AppSettings::clamp_font_size(size).unwrap_or(s.font_size);
        }
        if let Some(c) = &patch.foreground {
            s.foreground = valid_color(c, &AppSettings::default().foreground);
        }
        if let Some(c) = &patch.background {
            s.background = valid_color(c, &AppSettings::default().background);
        }
        if let Some(ms) = patch.ime_quiet_ms {
            s.ime_quiet_ms = ms.min(150); // 舊版 Math.Clamp(q, 0, 150)
        }
        if let Some(n) = patch.restore_buffer_lines {
            s.restore_buffer_lines = n.min(100_000);
        }
        if let Some(m) = patch.keep_alive_mins {
            s.keep_alive_mins = m.min(1440);
        }
        if let Some(b) = patch.auto_reconnect {
            s.auto_reconnect = b;
        }
        if let Some(d) = &patch.log_dir {
            s.log_dir = d.trim().to_string();
        }
        if let Some(b) = patch.log_timestamp {
            s.log_timestamp = b;
        }
        if let Some(b) = patch.log_append {
            s.log_append = b;
        }
        if let Some(b) = patch.exit_restore_tabs {
            s.exit_restore_tabs = b;
        }
        if let Some(b) = patch.sandbox_default {
            s.sandbox_default = b;
        }
    });

    // 語言要在重送 `T{json}` 之前設好（那包 JSON 裡有搜尋列與代理狀態的字）
    crate::i18n::set_lang(&after.language);
    crate::host::emit_host(&app, format!("T{}", after.theme_json()));
    println!(
        "[AwayTerminal] 設定已套用：語言={} 字型={} {}px 前景={} 背景={} imeQuiet={}ms",
        after.language,
        after.font_family,
        after.font_size,
        after.foreground,
        after.background,
        after.ime_quiet_ms
    );
    after
}

/// 「清除已接受的弱演算法記錄」（設定視窗的按鈕）。回傳清掉幾筆。
///
/// 這類「記錄型」的欄位不給一般的編輯 UI（PM 在 TASK-015 A2 定的），只給一個清除鈕。
#[tauri::command]
pub fn ssh_weak_clear(settings: State<'_, Arc<SettingsStore>>) -> usize {
    let n = settings.get().ssh_weak_accepted.len();
    settings.update(|s| s.ssh_weak_accepted.clear());
    println!("[AwayTerminal] 已清除弱演算法記錄：{n} 筆");
    n
}

/// 系統上的等寬字型清單（設定視窗的字型下拉）。
///
/// 舊版用 `Fonts.SystemFontFamilies`（WPF）。Tauri 沒有這個 API，瀏覽器也沒有
/// 「列出所有字型」的標準做法（`queryLocalFonts` 要權限、WebView2 上不一定有）。
/// 所以這裡回**一份候選清單**，只留這台機器上真的有的：
/// Windows 從 `%WINDIR%\Fonts` 的檔名判斷，其他平台之後再補。
/// 字型下拉是 `<input list=…>`，所以清單不完整也能自己打。
#[tauri::command]
pub fn font_list() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    #[cfg(windows)]
    {
        // (顯示名稱, 字型檔名的開頭)
        const CANDIDATES: &[(&str, &str)] = &[
            ("Cascadia Mono", "CascadiaMono"),
            ("Cascadia Code", "CascadiaCode"),
            ("Consolas", "consola"),
            ("Courier New", "cour"),
            ("Lucida Console", "lucon"),
            ("MS Gothic", "msgothic"),
            ("NSimSun", "simsun"),
            ("DejaVu Sans Mono", "DejaVuSansMono"),
            ("Microsoft JhengHei", "msjh"),
            ("Microsoft YaHei", "msyh"),
        ];
        let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()) + "\\Fonts";
        let names: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().to_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        for (show, file) in CANDIDATES {
            let f = file.to_lowercase();
            if names.iter().any(|n| n.starts_with(&f)) {
                out.push((*show).to_string());
            }
        }
    }
    if out.is_empty() {
        // 找不到就給一份合理的（使用者仍可自己打）
        out = ["Cascadia Mono", "Consolas", "Courier New", "monospace"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 顏色驗證：認得的照用，認不出來的退回預設（同舊版 `ValidColor`）。
    #[test]
    fn validates_colors() {
        assert_eq!(valid_color("#E0E0E0", "#fallback"), "#E0E0E0");
        assert_eq!(valid_color("  #1e1e1e  ", "#fallback"), "#1e1e1e");
        assert_eq!(valid_color("#abc", "#fallback"), "#abc");
        // 名稱式顏色**刻意不收**（見 `valid_color` 的註解）
        assert_eq!(valid_color("Red", "#fallback"), "#fallback");
        assert_eq!(valid_color("#12345", "#fallback"), "#fallback");
        assert_eq!(valid_color("", "#fallback"), "#fallback");
        assert_eq!(valid_color("#GGGGGG", "#fallback"), "#fallback");
    }

    /// 字型清單不會是空的（不然下拉會是空白，使用者以為壞了）。
    #[test]
    fn font_list_is_never_empty() {
        assert!(!font_list().is_empty());
    }
}
