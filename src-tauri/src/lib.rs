// AwayTerminal2 — Rust 後端

pub mod bench;
pub mod commands;
pub mod host;
pub mod output;
pub mod pty;
pub mod session;
pub mod settings;
pub mod startup;
pub mod status;
pub mod tabs;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};

use session::SessionManager;
use settings::SettingsStore;
use tabs::TabManager;

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
            commands::session_list,
            commands::tab_close,
            commands::tab_select,
            commands::tab_rename,
            commands::tabs_reorder,
            commands::view_mode_cycle,
            commands::tab_panel_set,
            commands::settings_get,
            commands::pane_selected,
            commands::pane_reordered,
            commands::pane_font_size,
            commands::pane_answer,
            host::host_ready,
            host::host_message,
            bench::bench_raw,
            bench::bench_vec,
            bench::bench_base64,
            bench::bench_channel,
        ])
        .setup(|app| {
            // 設定檔要在任何 command 跑起來之前備好（`host_ready` 的 T{json} 直接讀它）
            let dir = app.path().app_config_dir()?;
            let store = Arc::new(SettingsStore::load(&dir));
            settings::spawn_autosave(store.clone());

            let view_mode = store.get().view_mode;
            let tabs = Arc::new(TabManager::new(&view_mode));

            apply_window_bounds(app.handle(), &store);
            status::spawn(app.handle().clone(), tabs.clone());

            app.manage(store);
            app.manage(tabs);
            Ok(())
        })
        .on_window_event(|window, event| {
            // 視窗大小／位置存進設定（寫檔本身有防抖，拖曳中不會一直寫）
            let Some(store) = window.try_state::<Arc<SettingsStore>>() else {
                return;
            };
            match event {
                WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
                    remember_window_bounds(window, &store);
                }
                WindowEvent::CloseRequested { .. } => {
                    remember_window_bounds(window, &store);
                    store.flush();
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building AwayTerminal")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // 關閉程式時把所有 session 收乾淨：正常走 ClosePseudoConsole 才不會留殭屍
                // conhost / OpenConsole 鎖住資料夾（舊版踩雷）。
                app.state::<SessionManager>().close_all();
                if let Some(store) = app.try_state::<Arc<SettingsStore>>() {
                    store.flush();
                }
            }
        });
}

/// 啟動時把視窗擺回上次的大小／位置。
fn apply_window_bounds(app: &tauri::AppHandle, store: &SettingsStore) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let b = store.get().window;
    if b.maximized {
        let _ = win.maximize();
        return;
    }
    let _ = win.set_size(tauri::LogicalSize::new(b.width as f64, b.height as f64));
    if let (Some(x), Some(y)) = (b.x, b.y) {
        let _ = win.set_position(tauri::LogicalPosition::new(x as f64, y as f64));
    }
}

/// 最大化時只記 `maximized`，不覆寫還原後的大小（否則還原回來會變成整個螢幕）。
fn remember_window_bounds(window: &tauri::Window, store: &SettingsStore) {
    let maximized = window.is_maximized().unwrap_or(false);
    let scale = window.scale_factor().unwrap_or(1.0);
    let size = window.inner_size().ok().map(|s| s.to_logical::<f64>(scale));
    let pos = window.outer_position().ok().map(|p| p.to_logical::<f64>(scale));
    store.update(|s| {
        s.window.maximized = maximized;
        if maximized {
            return;
        }
        if let Some(size) = size {
            if size.width >= 200.0 && size.height >= 150.0 {
                s.window.width = size.width.round() as u32;
                s.window.height = size.height.round() as u32;
            }
        }
        if let Some(pos) = pos {
            s.window.x = Some(pos.x.round() as i32);
            s.window.y = Some(pos.y.round() as i32);
        }
    });
}
