//! 匯入舊版 AwayTerminal 的設定（`CLAUDE.md` 的「匯入舊版 `%LOCALAPPDATA%\AwayTerminal\settings.json`」）。
//!
//! ## 原則
//!
//! 1. **絕對不動舊檔**（只讀）。使用者可能還在用舊版——這個團隊本身就跑在舊版底下。
//! 2. **只匯入對得上的欄位**，對不上的列進報告（`docs/MIGRATION.md` 有完整對照表）。
//! 3. **不匯入「工作階段狀態」**：`SavedTabs`／`History`（上次開的分頁、紀錄）——
//!    那是「上次關程式時的樣子」，匯進來會在第一次啟動就莫名開一堆分頁。
//! 4. 舊檔的 key 是 **PascalCase**（`FontFamily`），新版是 camelCase（`fontFamily`）；
//!    這裡逐欄對映，不做自動改名（自動改名會把 `ComPort` 變成 `comPort` 但 `AdbPath` 也會被帶進來）。
//!
//! ## 什麼時候問
//!
//! - **第一次啟動**（新版還沒有 `settings.json`）而舊檔存在 → 問一次。
//! - 設定視窗也有「匯入舊版設定…」（可以之後再做，也可以選別台電腦拷過來的檔）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::State;

use crate::settings::{AppSettings, SettingsStore};

/// 舊版的設定檔位置（`%LOCALAPPDATA%\AwayTerminal\settings.json`）。
pub fn old_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA").ok()?;
        Some(PathBuf::from(base).join("AwayTerminal").join("settings.json"))
    }
    #[cfg(not(windows))]
    {
        None // 舊版只有 Windows
    }
}

/// 匯入的結果（給畫面上的摘要用）。
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// 讀到的舊檔路徑。
    pub path: String,
    /// 舊檔有幾個 key。
    pub old_keys: usize,
    /// 真的套用了幾個欄位。
    pub applied: usize,
    /// 匯入了幾條自訂連線。
    pub conns: usize,
    /// 匯入了幾筆我的最愛。
    pub favorites: usize,
    /// 刻意沒匯入的**欄位名稱**（`SavedTabs`…）。原因不放這裡：
    /// 那是長篇說明，進後端 log 與 `docs/MIGRATION.md`（畫面上只給數量與名稱）。
    pub skipped: Vec<String>,
    /// 跳過的我的最愛**名稱**（代理團隊的設定、還沒支援的種類）。
    pub skipped_favorites: Vec<String>,
    /// 匯入時的提醒（例如 COM 的 Mark 同位在新版不支援）。
    pub warnings: Vec<String>,
}

/// 取一個字串欄位（舊檔是 PascalCase）。
fn s(old: &serde_json::Value, key: &str) -> Option<String> {
    old.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .filter(|v| !v.trim().is_empty())
}

fn n(old: &serde_json::Value, key: &str) -> Option<u64> {
    old.get(key).and_then(serde_json::Value::as_u64)
}

fn b(old: &serde_json::Value, key: &str) -> Option<bool> {
    old.get(key).and_then(serde_json::Value::as_bool)
}

/// 舊版的語言碼 → 新版（`zh` 是舊版只有中英兩種時的寫法）。
fn map_language(old: &str) -> Option<String> {
    match old.trim() {
        "zh" | "zh-TW" => Some("zh-TW".to_string()),
        "en" => Some("en".to_string()),
        other if crate::i18n::is_supported_lang(other) => Some(other.to_string()),
        _ => None,
    }
}

/// 把舊版的 JSON 套到新版的設定上。**回報哪些套了、哪些沒套。**
pub fn apply(old: &serde_json::Value, to: &mut AppSettings) -> ImportReport {
    let mut r = ImportReport {
        old_keys: old.as_object().map(serde_json::Map::len).unwrap_or(0),
        ..Default::default()
    };
    let mut applied = 0usize;

    // ---- 字體與顏色（欄位名一樣，只是大小寫）----
    if let Some(v) = s(old, "FontFamily") {
        to.font_family = v;
        applied += 1;
    }
    if let Some(v) = n(old, "FontSize").and_then(|v| AppSettings::clamp_font_size(v as u32)) {
        to.font_size = v;
        applied += 1;
    }
    if let Some(v) = s(old, "Foreground") {
        to.foreground = crate::prefs::valid_color(&v, &to.foreground);
        applied += 1;
    }
    if let Some(v) = s(old, "Background") {
        to.background = crate::prefs::valid_color(&v, &to.background);
        applied += 1;
    }
    if let Some(v) = n(old, "ImeQuietMs") {
        to.ime_quiet_ms = (v as u32).min(150);
        applied += 1;
    }

    // ---- 語言 ----
    if let Some(v) = s(old, "Language").and_then(|v| map_language(&v)) {
        to.language = v;
        applied += 1;
    }

    // ---- log ----
    if let Some(v) = s(old, "LogDir") {
        to.log_dir = v;
        applied += 1;
    }
    if let Some(v) = b(old, "LogTimestamp") {
        to.log_timestamp = v;
        applied += 1;
    }
    if let Some(v) = b(old, "LogAppend") {
        to.log_append = v;
        applied += 1;
    }

    // ---- 連線的「上次用的值」與行為 ----
    if let Some(v) = b(old, "AutoReconnect") {
        to.auto_reconnect = v;
        applied += 1;
    }
    if let Some(v) = n(old, "KeepAliveMins") {
        to.keep_alive_mins = (v as u32).min(1440);
        applied += 1;
    }
    if let Some(v) = s(old, "LastDir") {
        to.last_dir = v;
        applied += 1;
    }

    // ---- 連接埠（COM）----
    if let Some(v) = s(old, "ComPort") {
        to.com_port = v;
        applied += 1;
    }
    if let Some(v) = n(old, "ComBaud") {
        to.com_baud = v as u32;
        applied += 1;
    }
    if let Some(v) = n(old, "ComDataBits") {
        to.com_data_bits = v as u8;
        applied += 1;
    }
    if let Some(v) = s(old, "ComParity") {
        if matches!(v.as_str(), "Mark" | "Space") {
            r.warnings
                .push(crate::i18n::tf("migrate.warnParity", &[&v]));
            to.com_parity = "None".to_string();
        } else {
            to.com_parity = v;
        }
        applied += 1;
    }
    if let Some(v) = s(old, "ComStopBits") {
        if v == "OnePointFive" {
            r.warnings.push(crate::i18n::t("migrate.warnStopBits"));
            to.com_stop_bits = "One".to_string();
        } else {
            to.com_stop_bits = v;
        }
        applied += 1;
    }
    if let Some(v) = s(old, "ComFlow") {
        if v == "RequestToSendXOnXOff" {
            r.warnings.push(crate::i18n::t("migrate.warnFlow"));
            to.com_flow = "RequestToSend".to_string();
        } else {
            to.com_flow = v;
        }
        applied += 1;
    }

    // ---- 介面狀態 ----
    if let Some(v) = b(old, "TabPanelVisible") {
        to.tab_panel_visible = v;
        applied += 1;
    }
    if let Some(v) = old.get("TabPanelWidth").and_then(serde_json::Value::as_f64) {
        to.tab_panel_width = v;
        applied += 1;
    }
    if let Some(v) = b(old, "ExitRestoreTabs") {
        to.exit_restore_tabs = v;
        applied += 1;
    }
    if let Some(v) = b(old, "ComposeSendEnter") {
        to.compose_send_enter = v;
        applied += 1;
    }
    if let Some(v) = n(old, "RestoreBufferLines") {
        to.restore_buffer_lines = (v as u32).min(100_000);
        applied += 1;
    }

    // ---- Telegram 遠端（功能是階段 4；PM 要求「存但功能未做」）----
    if let Some(v) = b(old, "RemoteEnabled") {
        to.remote_enabled = v;
        applied += 1;
    }
    if let Some(v) = s(old, "TelegramBotToken") {
        to.telegram_bot_token = v;
        applied += 1;
    }
    if let Some(v) = old.get("TelegramChatId").and_then(serde_json::Value::as_i64) {
        to.telegram_chat_id = v;
        applied += 1;
    }
    if let Some(v) = b(old, "RemoteNotify") {
        to.remote_notify = v;
        applied += 1;
    }

    // ---- 自訂連線 ----
    if let Some(list) = old.get("CustomConns").and_then(serde_json::Value::as_array) {
        for c in list {
            let Some(name) = s(c, "Name") else { continue };
            let path = s(c, "Path").unwrap_or_default();
            let conn = crate::settings::CustomConn {
                name: name.clone(),
                path,
                args: s(c, "Args").unwrap_or_default(),
                icon: s(c, "Icon").unwrap_or_else(|| "run".to_string()),
                close_key: s(c, "CloseKey").unwrap_or_else(|| "ctrl-c".to_string()),
                close_count: n(c, "CloseCount").unwrap_or(3) as u32,
                pick_dir: b(c, "PickDir").unwrap_or(false),
                hidden: b(c, "Hidden").unwrap_or(false),
                via_powershell: b(c, "ViaPowerShell").unwrap_or(false),
                // 舊版沒有沙盒 → 用新版的規則決定（AI agent 開、WSL／ADB 關）
                sandbox: to.sandbox_default && !matches!(name.as_str(), "WSL" | "ADB"),
            };
            // 同名的不重複加（使用者可能已經自己加過）
            if !to
                .custom_conns
                .iter()
                .any(|x| x.name.eq_ignore_ascii_case(&conn.name))
            {
                to.custom_conns.push(conn);
                r.conns += 1;
            }
        }
        applied += 1;
    }

    // ---- 我的最愛 ----
    if let Some(list) = old.get("Favorites").and_then(serde_json::Value::as_array) {
        for f in list {
            let Some(name) = s(f, "Name") else { continue };
            // 代理團隊的最愛（`TeamSetup` 非空）＝階段 4 的功能 → 先不匯入
            if s(f, "TeamSetup").is_some() {
                // 代理團隊的最愛＝階段 4 的功能（原因見 docs/MIGRATION.md）
                println!("[AwayTerminal] 匯入：跳過我的最愛「{name}」（代理團隊的設定，階段 4 才有）");
                r.skipped_favorites.push(name);
                continue;
            }
            let tab = f.get("Tab").cloned().unwrap_or(serde_json::Value::Null);
            let old_type = s(&tab, "Type").unwrap_or_else(|| "ps".to_string());
            let dir = s(&tab, "Dir").unwrap_or_default();
            let item = match old_type.as_str() {
                // `ps`＝PowerShell。`claude`／`custom`／`adb` 在舊版都是「自訂連線」那一類
                "ps" => crate::favorites::FavoriteItem {
                    name,
                    kind: "shell".to_string(),
                    dir,
                    ..Default::default()
                },
                "claude" | "custom" => crate::favorites::FavoriteItem {
                    name,
                    kind: "conn".to_string(),
                    dir,
                    conn_name: s(&tab, "Name").unwrap_or_default(),
                    ..Default::default()
                },
                "ssh" => crate::favorites::FavoriteItem {
                    name,
                    kind: "ssh".to_string(),
                    dir,
                    ssh: Some(crate::ssh::conn::SshConnParams {
                        host: s(&tab, "Host").unwrap_or_default(),
                        port: n(&tab, "Port").unwrap_or(22) as u16,
                        user: s(&tab, "User").unwrap_or_default(),
                        keepalive_mins: to.keep_alive_mins,
                        auto_reconnect: to.auto_reconnect,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                "telnet" => crate::favorites::FavoriteItem {
                    name,
                    kind: "telnet".to_string(),
                    dir,
                    telnet: Some(crate::telnet::TelnetParams {
                        host: s(&tab, "Host").unwrap_or_default(),
                        port: n(&tab, "Port").unwrap_or(23) as u16,
                        keepalive_mins: to.keep_alive_mins,
                        auto_reconnect: to.auto_reconnect,
                    }),
                    ..Default::default()
                },
                "com" => crate::favorites::FavoriteItem {
                    name,
                    kind: "com".to_string(),
                    dir,
                    com: Some(crate::com::ComParams {
                        port: s(&tab, "ComPort").unwrap_or_else(|| to.com_port.clone()),
                        baud: n(&tab, "Baud").unwrap_or(to.com_baud as u64) as u32,
                        data_bits: n(&tab, "DataBits").unwrap_or(8) as u8,
                        parity: s(&tab, "Parity").unwrap_or_else(|| "None".to_string()),
                        stop_bits: s(&tab, "StopBits").unwrap_or_else(|| "One".to_string()),
                        flow: s(&tab, "Flow").unwrap_or_else(|| "None".to_string()),
                        auto_reconnect: to.auto_reconnect,
                    }),
                    ..Default::default()
                },
                other => {
                    // `adb` 在新版是 `kind="adb"`，但我的最愛還沒支援 adb → 記下來
                    println!(
                        "[AwayTerminal] 匯入：跳過我的最愛「{name}」（種類 {other} 新版的我的最愛還沒支援）"
                    );
                    r.skipped_favorites.push(name);
                    continue;
                }
            };
            if !to
                .favorites
                .iter()
                .any(|x| x.name.eq_ignore_ascii_case(&item.name))
            {
                to.favorites.push(item);
                r.favorites += 1;
            }
        }
        applied += 1;
    }

    // ---- 刻意不匯入的 ----
    //
    // 報告只帶**欄位名稱**；為什麼不匯入是長篇說明 → 進後端 log ＋ `docs/MIGRATION.md`
    // 的對照表（那張表是給人看的，翻成八種語言沒有意義，而且會馬上和文件不同步）。
    // i18n-audit:log-only-begin 這張原因表只進後端 log 與 docs/MIGRATION.md（不是介面文字）
    for (key, why) in [
        ("SavedTabs", "上次關程式時開著的分頁（工作階段狀態，匯進來會莫名開一堆分頁）"),
        ("History", "舊版的「紀錄」清單（新版改用我的最愛）"),
        ("ExplorerMenu", "新版直接看登錄檔的實際狀態，不存這個旗標"),
        ("ClaudePath", "舊版 v1.0.18 起 Claude 已經是「自訂連線」，會跟著 CustomConns 進來"),
        ("ClaudeArgs", "同上"),
        ("ClaudeEnabled", "同上"),
        ("ClaudeCommand", "舊版自己就標成停用欄位"),
        ("AdbPath", "同 Claude：ADB 也是自訂連線"),
        ("AdbEnabled", "同上"),
        ("AgentChatFolder", "代理團隊（階段 4）"),
        ("DirBookmarks", "新版還沒有資料夾書籤"),
        ("HostHistory", "新版用我的最愛取代主機歷史（TASK-009 已決定）"),
        ("SeededSamples", "舊版「範例是否放過」的旗標，新版沒有範例"),
        ("ClaudeMigratedToCustom", "舊版自己的遷移旗標"),
        ("ExitUpdateMd", "離開時更新 CLAUDE.md＝代理團隊（階段 4）"),
        ("LastConnType", "「上次連的」由連線對話框自己記"),
        ("LastUser", "同上"),
        ("LastHost", "同上（而且不想在第一次啟動就預填內網 IP）"),
        ("LastSshPort", "同上"),
        ("LastTelnetPort", "同上"),
        ("ExtraFields", "舊版自己的「未知欄位保留區」"),
    ] {
        if old.get(key).is_some() {
            println!("[AwayTerminal] 匯入：不匯入 {key}（{why}）");
            r.skipped.push(key.to_string());
        }
    }
    // i18n-audit:log-only-end
    // Agent* 一族（數量多，合成一條）
    if old
        .as_object()
        .is_some_and(|m| m.keys().any(|k| k.starts_with("Agent")))
    {
        println!("[AwayTerminal] 匯入：不匯入 Agent*（代理團隊的設定，階段 4 才有對應功能）");
        r.skipped.push("Agent*".to_string());
    }

    r.applied = applied;
    r
}

/// 讀舊檔（**唯讀**）。
pub fn read_old(path: &Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| crate::i18n::tf("err.readFileFailed", &[&e.to_string()]))?;
    // 舊版是 .NET 寫的，可能帶 UTF-8 BOM
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_json::from_str(text).map_err(|e| crate::i18n::tf("err.oldSettingsBadJson", &[&e.to_string()]))
}

// ------------------------------------------------------------------ commands

/// 這次啟動時，新版的 `settings.json` 還不存在嗎（＝第一次啟動）。
///
/// 在 `lib.rs` 的 `setup` 最開頭就記下來：autosave 很快就會把檔案寫出來，
/// 之後再看檔案存不存在永遠是「存在」。
pub struct FirstRun(pub bool);

/// 有沒有舊版設定可以匯入（第一次啟動時問一次的依據）。
#[tauri::command]
pub fn migrate_probe(
    _settings: State<'_, Arc<SettingsStore>>,
    first_run: State<'_, FirstRun>,
) -> serde_json::Value {
    let old = old_path();
    let exists = old.as_deref().is_some_and(Path::is_file);
    // 第一次啟動＝程式剛起來時還沒有 settings.json（見 `FirstRun`）
    let first = first_run.0;
    let mut conns = 0usize;
    let mut favs = 0usize;
    if let (true, Some(p)) = (exists, old.as_deref()) {
        if let Ok(v) = read_old(p) {
            conns = v
                .get("CustomConns")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            favs = v
                .get("Favorites")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
        }
    }
    serde_json::json!({
        "oldPath": old.map(|p| p.display().to_string()).unwrap_or_default(),
        "oldExists": exists,
        "firstRun": first,
        "conns": conns,
        "favorites": favs,
    })
}

/// 選一個舊版的 `settings.json`（設定視窗的「匯入舊版設定…」；也能選從別台拷過來的檔）。
#[tauri::command]
pub async fn migrate_pick_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    let start = old_path().and_then(|p| p.parent().map(Path::to_path_buf));
    let mut dlg = app
        .dialog()
        .file()
        .set_title(crate::i18n::t("migrate.pick"))
        .add_filter("settings.json", &["json"]);
    if let Some(dir) = start {
        dlg = dlg.set_directory(dir);
    }
    dlg.pick_file(move |f| {
        let _ = tx.send(f.and_then(|p| p.into_path().ok()));
    });
    // 等使用者選（同其他檔案對話框：不能擋主執行緒，所以這個 command 是 async）
    let picked = tokio::task::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .map_err(|e| e.to_string())?;
    Ok(picked.map(|p| p.display().to_string()))
}

/// 匯入（`path` 省略＝用舊版的預設位置）。**不動舊檔。**
#[tauri::command]
pub fn migrate_import(
    app: tauri::AppHandle,
    path: Option<String>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<ImportReport, String> {
    let p = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => old_path().ok_or_else(|| crate::i18n::t("err.noOldSettings"))?,
    };
    let old = read_old(&p)?;
    // 這次啟動的 settings.json 讀不進來 → 整個工作階段都不寫檔；照樣匯入的話只會改在記憶體裡、
    // 關掉就沒了，卻回報成功（BUG D6）。直接告訴使用者。
    if let Some(why) = settings.readonly_reason() {
        return Err(why);
    }
    let mut report = ImportReport::default();
    let after = settings.update(|s| {
        report = apply(&old, s);
    });
    report.path = p.display().to_string();

    // 語言與主題要立刻生效（同設定視窗按確定）
    crate::i18n::set_lang(&after.language);
    crate::host::emit_host(&app, format!("T{}", after.theme_json()));
    println!(
        "[AwayTerminal] 匯入舊版設定：{} → 套用 {} 個欄位、{} 條自訂連線、{} 筆我的最愛（跳過 {} 項）",
        report.path, report.applied, report.conns, report.favorites, report.skipped.len()
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一份「像真的」舊版設定（含所有欄位、中文名稱、Big5 時代的路徑、Mark 同位）。
    fn old_json() -> serde_json::Value {
        serde_json::json!({
            "FontFamily": "Consolas",
            "FontSize": 18,
            "Foreground": "#00FF00",
            "Background": "#101010",
            "ImeQuietMs": 35,
            "Language": "zh",
            "LogDir": "D:\\紀錄\\AwayTerminal",
            "LogTimestamp": false,
            "LogAppend": false,
            "AutoReconnect": true,
            "KeepAliveMins": 7,
            "LastDir": "C:\\Users\\Awaysu\\桌面\\專案",
            "ComPort": "COM7",
            "ComBaud": 9600,
            "ComDataBits": 7,
            "ComParity": "Mark",
            "ComStopBits": "OnePointFive",
            "ComFlow": "RequestToSendXOnXOff",
            "TabPanelVisible": false,
            "TabPanelWidth": 300.0,
            "ExitRestoreTabs": false,
            "ComposeSendEnter": false,
            "RestoreBufferLines": 500,
            "RemoteEnabled": true,
            "TelegramBotToken": "123:abc",
            "TelegramChatId": -1001234567890i64,
            "RemoteNotify": true,
            "CustomConns": [
                { "Name": "ClaudeCode", "Path": "C:\\claude.exe", "Args": "--x", "Icon": "claude",
                  "CloseKey": "ctrl-d", "CloseCount": 2, "PickDir": true, "Hidden": false, "ViaPowerShell": false },
                { "Name": "WSL", "Path": "C:\\Windows\\System32\\wsl.exe", "Args": "", "Icon": "wsl",
                  "CloseKey": "ctrl-c", "CloseCount": 3, "PickDir": false, "Hidden": false, "ViaPowerShell": false }
            ],
            "Favorites": [
                { "Name": "我的伺服器", "Tab": { "Type": "ssh", "Host": "192.168.1.9", "Port": 2222, "User": "root" }, "TeamSetup": "" },
                { "Name": "桌面", "Tab": { "Type": "ps", "Dir": "C:\\Users\\Awaysu\\Desktop" }, "TeamSetup": "" },
                { "Name": "序列埠", "Tab": { "Type": "com", "ComPort": "COM3", "Baud": 19200 }, "TeamSetup": "" },
                { "Name": "團隊", "Tab": { "Type": "ps" }, "TeamSetup": "{\"folder\":\"x\"}" },
                { "Name": "手機", "Tab": { "Type": "adb", "AdbSerial": "R5C" }, "TeamSetup": "" }
            ],
            "SavedTabs": [ { "Type": "ps" } ],
            "History": [ { "Type": "ps" } ],
            "ExplorerMenu": true,
            "ClaudePath": "C:\\claude.exe",
            "AdbPath": "C:\\adb.exe",
            "AgentMaxMessages": 30,
            "HostHistory": ["a"],
            "DirBookmarks": ["b"]
        })
    }

    /// 對得上的欄位都要套過去（含 Big5 時代的中文路徑）。
    #[test]
    fn imports_matching_fields() {
        let mut s = AppSettings::default();
        let r = apply(&old_json(), &mut s);
        assert_eq!(s.font_family, "Consolas");
        assert_eq!(s.font_size, 18);
        assert_eq!(s.foreground, "#00FF00");
        assert_eq!(s.background, "#101010");
        assert_eq!(s.ime_quiet_ms, 35);
        assert_eq!(s.language, "zh-TW", "舊版的 zh 要變成 zh-TW");
        assert_eq!(s.log_dir, "D:\\紀錄\\AwayTerminal");
        assert!(!s.log_timestamp);
        assert!(!s.log_append);
        assert!(s.auto_reconnect);
        assert_eq!(s.keep_alive_mins, 7);
        assert_eq!(s.last_dir, "C:\\Users\\Awaysu\\桌面\\專案");
        assert_eq!(s.com_port, "COM7");
        assert_eq!(s.com_baud, 9600);
        assert!(!s.tab_panel_visible);
        assert_eq!(s.tab_panel_width, 300.0);
        assert!(!s.exit_restore_tabs);
        assert!(!s.compose_send_enter);
        assert_eq!(s.restore_buffer_lines, 500);
        assert!(r.applied > 20, "套用的欄位數太少：{}", r.applied);
    }

    /// `serialport` 不支援的 COM 值要降級**並且提醒**（不可以安靜改掉）。
    #[test]
    fn degrades_unsupported_com_values_with_warnings() {
        let mut s = AppSettings::default();
        let r = apply(&old_json(), &mut s);
        assert_eq!(s.com_parity, "None");
        assert_eq!(s.com_stop_bits, "One");
        assert_eq!(s.com_flow, "RequestToSend");
        assert_eq!(r.warnings.len(), 3, "三個降級都要提醒：{:?}", r.warnings);
    }

    /// 自訂連線：全部進來，沙盒依新版規則（agent 開、WSL 關）。
    #[test]
    fn imports_custom_connections() {
        let mut s = AppSettings::default();
        let r = apply(&old_json(), &mut s);
        assert_eq!(r.conns, 2);
        let claude = s.custom_conns.iter().find(|c| c.name == "ClaudeCode").unwrap();
        assert_eq!(claude.close_key, "ctrl-d");
        assert_eq!(claude.close_count, 2);
        assert!(claude.pick_dir);
        assert!(claude.sandbox, "agent 預設開沙盒");
        let wsl = s.custom_conns.iter().find(|c| c.name == "WSL").unwrap();
        assert!(!wsl.sandbox, "WSL 預設不開沙盒");
    }

    /// 我的最愛：ps／ssh／com 進來，代理團隊與 adb 跳過並記原因。
    #[test]
    fn imports_favorites_and_reports_skips() {
        let mut s = AppSettings::default();
        let r = apply(&old_json(), &mut s);
        assert_eq!(r.favorites, 3, "ps／ssh／com 三筆：{:?}", s.favorites);
        let ssh = s.favorites.iter().find(|f| f.name == "我的伺服器").unwrap();
        assert_eq!(ssh.kind, "ssh");
        let p = ssh.ssh.as_ref().unwrap();
        assert_eq!(p.host, "192.168.1.9");
        assert_eq!(p.port, 2222);
        assert_eq!(p.user, "root");
        // 跳過的最愛只帶名稱（原因進 log ＋ docs/MIGRATION.md）
        assert!(
            r.skipped_favorites.iter().any(|x| x == "團隊"),
            "代理團隊的最愛要列進 skipped_favorites：{:?}",
            r.skipped_favorites
        );
        assert!(
            r.skipped_favorites.iter().any(|x| x == "手機"),
            "adb 的最愛要列進 skipped_favorites：{:?}",
            r.skipped_favorites
        );
    }

    /// 工作階段狀態與已被取代的欄位都要列進「沒匯入」。
    #[test]
    fn lists_intentional_skips() {
        let mut s = AppSettings::default();
        let r = apply(&old_json(), &mut s);
        for key in ["SavedTabs", "History", "ExplorerMenu", "ClaudePath", "AdbPath", "HostHistory"] {
            assert!(
                r.skipped.iter().any(|x| x.starts_with(key)),
                "{key} 應該列進 skipped：{:?}",
                r.skipped
            );
        }
        assert!(r.skipped.iter().any(|x| x.starts_with("Agent*")));
    }

    /// Telegram 的欄位要存下來（功能是階段 4，但不能弄丟使用者的 token）。
    #[test]
    fn keeps_telegram_settings() {
        let mut s = AppSettings::default();
        apply(&old_json(), &mut s);
        assert!(s.remote_enabled);
        assert_eq!(s.telegram_bot_token, "123:abc");
        assert_eq!(s.telegram_chat_id, -1001234567890);
        assert!(s.remote_notify);
    }

    /// 同名的不重複加（使用者已經自己加過 → 不覆蓋）。
    #[test]
    fn does_not_duplicate_existing_entries() {
        let mut s = AppSettings::default();
        s.custom_conns.push(crate::settings::CustomConn {
            name: "WSL".to_string(),
            path: "已經有的".to_string(),
            ..Default::default()
        });
        let r = apply(&old_json(), &mut s);
        assert_eq!(r.conns, 1, "只該加 ClaudeCode");
        assert_eq!(
            s.custom_conns.iter().filter(|c| c.name == "WSL").count(),
            1,
            "WSL 不該變兩條"
        );
        assert_eq!(
            s.custom_conns.iter().find(|c| c.name == "WSL").unwrap().path,
            "已經有的",
            "已經存在的不該被覆蓋"
        );
    }

    /// 壞掉的舊檔不該讓匯入把設定改壞。
    #[test]
    fn empty_or_garbage_json_changes_nothing() {
        let mut s = AppSettings::default();
        let before = (s.font_family.clone(), s.font_size);
        let r = apply(&serde_json::json!({}), &mut s);
        assert_eq!((s.font_family.clone(), s.font_size), before);
        assert_eq!(r.applied, 0);
        // 型別不對的欄位一律忽略（舊檔被手改壞過）
        let weird = serde_json::json!({ "FontSize": "十八", "Foreground": 123, "CustomConns": "x" });
        let r2 = apply(&weird, &mut s);
        assert_eq!((s.font_family.clone(), s.font_size), before);
        assert_eq!(r2.applied, 0);
    }

    /// BOM 開頭的檔案讀得進來（舊版是 .NET 寫的）。
    #[test]
    fn reads_bom_prefixed_file() {
        let p = std::env::temp_dir().join("awayterm-migrate-test.json");
        std::fs::write(&p, "\u{feff}{\"FontSize\": 16}").unwrap();
        let v = read_old(&p).expect("BOM 檔要讀得進來");
        assert_eq!(v.get("FontSize").unwrap().as_u64(), Some(16));
        let _ = std::fs::remove_file(&p);
    }
}
