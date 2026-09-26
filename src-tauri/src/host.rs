//! host 端（Rust）對前端的舊字串協定介面。
//!
//! 舊版是 WPF 用 `PostWebMessageAsString` 送 `n…`／`T…` 這類字串給 WebView2。
//! 新版用 tauri event `host-msg`（payload 就是同一個字串），`src/bridge.js` 收到後
//! 轉交給搬過來的 `terminal.js`。協定字串本身沒有改。

use tauri::{AppHandle, Emitter};

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
/// 值照舊版 `Services/AppSettings.cs` 的預設值寫死；設定檔是之後的任務。
#[tauri::command]
pub fn host_ready(app: AppHandle) {
    let theme = serde_json::json!({
        // 舊版：$"\"{s.FontFamily}\", Consolas, \"Microsoft JhengHei\", \"微軟正黑體\", monospace"
        "fontFamily": "\"Cascadia Mono\", Consolas, \"Microsoft JhengHei\", \"微軟正黑體\", monospace",
        "fontSize": 14,          // AppSettings.FontSize
        "foreground": "#E0E0E0", // AppSettings.Foreground
        "background": "#1E1E1E", // AppSettings.Background
        "imeQuietMs": 20,        // AppSettings.ImeQuietMs（claude 分頁靜止閘門；0=關閉）
        "restoreLines": 2000,    // AppSettings.RestoreBufferLines
        "agentStates": ["閒置", "忙碌", "有信待送", "已結束", "忙碌 · 有信待送"],
        "search": {
            "placeholder": "搜尋",
            "prev": "上一個 (Shift+Enter)",
            "next": "下一個 (Enter)",
            "close": "關閉 (Esc)"
        }
    });
    emit_host(&app, format!("T{theme}"));
}

/// 尚未接上的 JS→host 訊息（`p` / `k` / `z` / `G` / `a` / `m` / `U` …）。
///
/// 故意不靜靜丟掉：記進 log，這樣「還有哪些協定沒接」在 dev log 裡看得見。
/// 每一種對應的功能屬於之後的 UI 任務，見 docs/PROTOCOL.md。
#[tauri::command]
pub fn host_message(msg: String) {
    let kind = msg.chars().next().unwrap_or('?');
    let what = match kind {
        'p' => "選取 pane（分頁列尚未實作）",
        'k' => "拖曳後的新順序（分割/分欄 UI 尚未實作）",
        'z' => "Ctrl+滾輪字級（設定檔尚未實作，不會記住）",
        'G' => "Multi-Agent 分隔線比例（尚未實作）",
        'a' => "查詢回覆（q 協定尚未由 host 發出）",
        'm' => "程式接管滑鼠提示（尚未實作）",
        'U' => "點了終端機裡的連結（開啟選單尚未實作）",
        _ => "未知",
    };
    println!("[AwayTerminal] [host_message 未接] {kind} = {what} :: {}", head(&msg));
}
