// AwayTerminal2 — Rust 後端

pub mod b64;
pub mod bench;
pub mod cli;
pub mod com;
pub mod compose;
pub mod commands;
pub mod custom;
pub mod favorites;
pub mod host;
pub mod i18n;
pub mod logging;
pub mod output;
pub mod prefs;
pub mod pty;
pub mod reconnect;
pub mod restore;
pub mod sandbox;
pub mod session;
pub mod settings;
pub mod ssh;
pub mod startup;
pub mod status;
pub mod tabs;
pub mod update;
pub mod tap;
pub mod telnet;
pub mod toolbar;
pub mod ttl;

use std::sync::Arc;

use tauri::{Emitter, Manager, WindowEvent};

use cli::LaunchArgs;
use session::SessionManager;
use settings::SettingsStore;
use tabs::TabManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 一定要在建立任何執行緒 / 子行程之前（見 startup.rs 的說明）
    startup::prepare_process_environment();
    println!("[AwayTerminal] ConPTY backend: {}", pty::backend_name());

    let args = LaunchArgs::from_env();
    if args.cmd.is_some() || args.verify > 0 || args.bench {
        println!("[AwayTerminal] 啟動參數：{args:?}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(SessionManager::new())
        .manage(args)
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
            prefs::settings_apply,
            prefs::ssh_weak_clear,
            prefs::font_list,
            update::update_check,
            update::about_info,
            update::update_verify,
            update::third_party_notices,
            commands::pane_selected,
            commands::pane_reordered,
            commands::pane_font_size,
            commands::pane_answer,
            cli::launch_args,
            toolbar::toolbar_copy,
            toolbar::toolbar_copy_all,
            toolbar::toolbar_copy_all_file,
            toolbar::toolbar_copy_paste,
            toolbar::toolbar_paste,
            toolbar::toolbar_clear,
            toolbar::toolbar_scroll,
            toolbar::toolbar_search,
            toolbar::toolbar_select_all,
            toolbar::tab_colors,
            toolbar::save_text_to_file,
            toolbar::pick_work_dir,
            toolbar::open_url,
            toolbar::log_defaults,
            toolbar::log_pick_path,
            toolbar::log_start,
            toolbar::log_stop,
            toolbar::reveal_path,
            toolbar::temp_dir,
            toolbar::macro_pick_file,
            toolbar::save_text_to_file_at,
            compose::compose_load_file,
            compose::compose_save_file,
            compose::compose_send,
            compose::compose_verify_roundtrip,
            ssh::prompt::ssh_hostkey_answer,
            ssh::algos::algo_catalog,
            custom::custom_list,
            custom::custom_detect,
            custom::custom_save,
            custom::custom_delete,
            custom::conn_set_sandbox,
            custom::sandbox_clear,
            favorites::fav_list,
            favorites::fav_candidate,
            favorites::fav_add,
            favorites::fav_delete,
            favorites::fav_rename,
            favorites::fav_move,
            com::com_ports,
            ttl::runner::macro_run,
            ttl::runner::macro_stop,
            ttl::runner::macro_answer,
            ttl::runner::macro_verify,
            ttl::execverify::exec_verify,
            restore::restore_list,
            restore::exit_confirm,
            restore::exit_cancel,
            restore::restore_verify_save,
            restore::restore_verify_clear,
            sandbox::sandbox_probe,
            sandbox::sandbox_verify_cleanup,
            sandbox::pid_alive,
            sandbox::sandbox_verify,
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
            // 空的 log_dir 補成「我的文件\AwayTerminalLogs」（同舊版 AppSettings.Load）
            if let Ok(docs) = app.path().document_dir() {
                store.fill_log_dir(&docs);
            }
            // 介面語言要在任何訊息產生之前設好（背景執行緒寫進畫面的字也吃這個）
            i18n::set_lang(&store.get().language);
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
                WindowEvent::CloseRequested { api, .. } => {
                    remember_window_bounds(window, &store);
                    store.flush();
                    // 舊版 `OnClosingAsk`：先攔下來，跳自訂的離開對話框
                    // （勾「下次開啟恢復目前分頁」）。確認後前端呼叫 `exit_confirm`。
                    use std::sync::atomic::Ordering;
                    if !restore::asked().swap(true, Ordering::SeqCst) {
                        api.prevent_close();
                        let app = window.app_handle().clone();
                        let restore_default = store.get().exit_restore_tabs;
                        if let Err(e) = app.emit("exit-request", restore_default) {
                            // 前端收不到就沒人會回答 → 別把視窗鎖死，直接讓它關
                            println!("[AwayTerminal] 離開對話框發不出去（{e}），直接關閉");
                            restore::asked().store(false, Ordering::SeqCst);
                            app.exit(0);
                        }
                    }
                    // 第二次按 X（前端壞掉、對話框沒出來）→ 不再攔，照關
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
