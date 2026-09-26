// AwayTerminal2 — Rust 後端

pub mod bench;
pub mod commands;
pub mod host;
pub mod output;
pub mod pty;
pub mod session;
pub mod startup;

use session::SessionManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 一定要在建立任何執行緒 / 子行程之前（見 startup.rs 的說明）
    startup::prepare_process_environment();
    println!("[AwayTerminal] ConPTY backend: {}", pty::backend_name());

    tauri::Builder::default()
        .manage(SessionManager::new())
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::report_renderer,
            commands::log_line,
            commands::conpty_backend,
            commands::session_create,
            commands::session_write,
            commands::session_write_text,
            commands::session_resize,
            commands::session_close,
            commands::session_list,
            host::host_ready,
            host::host_message,
            bench::bench_raw,
            bench::bench_vec,
            bench::bench_base64,
            bench::bench_channel,
        ])
        .build(tauri::generate_context!())
        .expect("error while building AwayTerminal")
        .run(|app, event| {
            // 關閉程式時把所有 session 收乾淨：正常走 ClosePseudoConsole 才不會留殭屍
            // conhost / OpenConsole 鎖住資料夾（舊版踩雷）。
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager;
                app.state::<SessionManager>().close_all();
            }
        });
}
