// AwayTerminal2 — Rust 後端（階段 1 骨架，尚未接 PTY）

/// 最小 IPC 驗證用指令：前端 invoke('ping') 會拿到版本字串。
#[tauri::command]
fn ping() -> String {
    format!(
        "pong from AwayTerminal {} (tauri {}, {})",
        env!("CARGO_PKG_VERSION"),
        tauri::VERSION,
        std::env::consts::OS
    )
}

/// 前端把實際使用的渲染器（WebGL / DOM）回報到啟動 log，
/// 讓不開 devtools 也能確認 WebGL addon 有沒有成功啟用。
#[tauri::command]
fn report_renderer(renderer: String) {
    println!("[AwayTerminal] renderer = {renderer}");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ping, report_renderer])
        .run(tauri::generate_context!())
        .expect("error while running AwayTerminal");
}
