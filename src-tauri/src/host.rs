//! host 端（Rust）對前端的舊字串協定介面。
//!
//! 舊版是 WPF 用 `PostWebMessageAsString` 送 `n…`／`T…` 這類字串給 WebView2。
//! 新版用 tauri event `host-msg`（payload 就是同一個字串），`src/bridge.js` 收到後
//! 轉交給搬過來的 `terminal.js`。協定字串本身沒有改。
//!
//! 對照表在 `docs/PROTOCOL.md`。

use crate::i18n::{t};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::settings::SettingsStore;

/// host → JS：發一個舊協定字串。
pub fn emit_host(app: &AppHandle, msg: impl Into<String>) {
    let msg = msg.into();
    if let Err(e) = app.emit("host-msg", msg.clone()) {
        println!("[AwayTerminal] emit host-msg 失敗（{e}）：{}", head(&msg));
    }
}

fn head(s: &str) -> String {
    s.chars().take(80).collect()
}

/// `terminal.js` 載完會送 `ready`。這裡回舊版 `PostTheme()` 的 `T{json}`。
///
/// TASK-004 起內容來自 `settings.json`（之前是 Rust 常數）。
#[tauri::command]
pub fn host_ready(app: AppHandle, settings: State<'_, Arc<SettingsStore>>) {
    let theme = settings.get().theme_json();
    emit_host(&app, format!("T{theme}"));
}

/// 尚未接上的 JS→host 訊息。
///
/// 故意不靜靜丟掉：記進 log，這樣「還有哪些協定沒接」在 dev log 裡看得見。
/// 每一種對應的功能屬於之後的任務，見 `docs/PROTOCOL.md`。
#[tauri::command]
pub fn host_message(msg: String) {
    let kind = msg.chars().next().unwrap_or('?');
    let what = match kind {
        'U' => t("host.linkClicked"),
        'm' => t("host.mouseTakeover"),
        'G' => t("host.agentRatio"),
        _ => t("host.unknown"),
    };
    println!("[AwayTerminal] [host_message 未接] {kind} = {what} :: {}", head(&msg));
}
