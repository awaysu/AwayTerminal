//! 前端可呼叫的 tauri command。
//!
//! 語意對得上舊版的 C#↔JS 字串協定（建立 / 輸入 / 尺寸 / 輸出 / 結束），
//! 但傳輸改成 tauri command + `Channel`，輸出走原始位元組、不再經 base64。

use std::sync::Arc;

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

use crate::output::OutputPump;
use crate::pty::{self, shell, SpawnOptions};
use crate::session::{ExitInfo, SessionManager};

/// `session_create` 的回覆。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: u32,
    pub pid: u32,
    /// 實際啟動的命令列（診斷用）。
    pub command_line: String,
    /// `pwsh` / `powershell`。
    pub shell: String,
    /// `conpty.dll (OpenConsole)` 或 `inbox conhost`。
    pub backend: String,
}

/// 連線結束通知。走同一條 channel，但以 JSON 送出——前端看
/// 「收到的是 ArrayBuffer 還是物件」來區分輸出與事件。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ExitEvent {
    kind: &'static str,
    id: u32,
    exit_code: Option<i32>,
}

/// 最小 IPC 驗證用指令。
#[tauri::command]
pub fn ping() -> String {
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
pub fn report_renderer(renderer: String) {
    println!("[AwayTerminal] renderer = {renderer}");
}

/// 前端把一行診斷訊息印到後端 stdout（IPC bench 結果等），
/// 這樣不必開 devtools、也不必看視窗就能從啟動 log 拿到數據。
#[tauri::command]
pub fn log_line(msg: String) {
    println!("[AwayTerminal] {msg}");
}

/// 目前使用的 ConPTY 主機（前端可印在終端第一行）。
#[tauri::command]
pub fn conpty_backend() -> String {
    pty::backend_name()
}

/// 開一條連線。本階段只支援 `"powershell"`。
#[tauri::command]
pub fn session_create(
    kind: String,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    on_event: Channel<InvokeResponseBody>,
    manager: State<'_, SessionManager>,
) -> Result<SessionInfo, String> {
    if kind != "powershell" {
        return Err(format!("尚未支援的連線種類：{kind}"));
    }

    let sh = shell::powershell().ok_or_else(|| {
        "找不到 pwsh.exe 或 powershell.exe（PATH 與 System32 都沒有）".to_string()
    })?;

    let id = manager.next_id();

    // 輸出批次合併：讀取執行緒只把 bytes 丟進 pump，由 pump 執行緒合併後送一包。
    // 這同時避開 tauri 的門檻——`InvokeResponseBody::Raw` 小於 1024 bytes 會被序列化成
    // JSON 數字陣列用 eval 送，合併後的大包才走 fetch 自訂協定拿到真正的二進位。
    let pump = Arc::new(OutputPump::start(on_event.clone()));

    let on_output = {
        let pump = pump.clone();
        Arc::new(move |bytes: &[u8]| pump.push(bytes)) as crate::session::OnOutput
    };
    let on_exit = {
        let pump = pump.clone();
        let channel = on_event.clone();
        Arc::new(move |info: ExitInfo| {
            // 先把還沒送出的輸出排空，再送結束事件，順序才不會顛倒
            pump.flush_and_stop();
            let payload = ExitEvent {
                kind: "exit",
                id,
                exit_code: info.exit_code,
            };
            if let Ok(json) = serde_json::to_string(&payload) {
                let _ = channel.send(InvokeResponseBody::Json(json));
            }
        }) as crate::session::OnExit
    };

    let session = pty::spawn(
        SpawnOptions {
            command_line: sh.command_line.clone(),
            cols,
            rows,
            cwd,
            graceful_exit_bytes: SpawnOptions::default_graceful_exit_bytes(),
        },
        on_output,
        on_exit,
    )
    .map_err(|e| format!("啟動 {} 失敗：{e}", sh.exe.display()))?;

    let info = SessionInfo {
        id,
        pid: session.pid(),
        command_line: sh.command_line,
        shell: sh.name.to_string(),
        backend: session.backend_name().to_string(),
    };
    println!(
        "[AwayTerminal] session {} started: pid={} shell={} backend={}",
        info.id, info.pid, info.shell, info.backend
    );
    manager.insert(id, session);
    Ok(info)
}

/// 寫入原始位元組（`term.onBinary` 用）。
#[tauri::command]
pub fn session_write(id: u32, data: Vec<u8>, manager: State<'_, SessionManager>) {
    if let Some(s) = manager.get(id) {
        s.write(&data);
    }
}

/// 寫入文字（`term.onData` 用；省掉上行的 JSON 數字陣列）。
#[tauri::command]
pub fn session_write_text(id: u32, text: String, manager: State<'_, SessionManager>) {
    if let Some(s) = manager.get(id) {
        s.write(text.as_bytes());
    }
}

#[tauri::command]
pub fn session_resize(id: u32, cols: u16, rows: u16, manager: State<'_, SessionManager>) {
    if let Some(s) = manager.get(id) {
        s.resize(cols, rows);
    }
}

#[tauri::command]
pub fn session_close(id: u32, manager: State<'_, SessionManager>) {
    if let Some(s) = manager.remove(id) {
        // close() 會 sleep（優雅結束鍵 60ms），別卡在 IPC 執行緒上
        std::thread::spawn(move || s.close());
    }
}

#[tauri::command]
pub fn session_list(manager: State<'_, SessionManager>) -> Vec<u32> {
    manager.ids()
}
