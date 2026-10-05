// AwayTerminal2 — Rust 後端

pub mod adb;
pub mod agent;
pub mod b64;
pub mod bench;
pub mod claudemd;
pub mod cli;
pub mod com;
pub mod compose;
pub mod commands;
pub mod custom;
pub mod favorites;
pub mod fonts;
pub mod fontstore;
pub mod host;
pub mod i18n;
pub mod logging;
pub mod migrate;
pub mod output;
pub mod prefs;
pub mod pty;
pub mod reconnect;
pub mod restore;
pub mod sandbox;
pub mod session;
pub mod settings;
#[cfg(windows)]
pub mod shellmenu;
pub mod ssh;
pub mod startup;
pub mod status;
pub mod tabs;
pub mod update;
pub mod tap;
pub mod taskbar;
pub mod telegram;
pub mod telnet;
pub mod toolbar;
pub mod ttl;
pub mod winicon;
pub mod winpos;

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
    // 工作列身分：**一定要在建立視窗之前**（Windows 只認第一次設定的值）。
    // 不設的話 Windows 拿 exe 路徑去猜，開發版／安裝版／1.x 可能被歸成同一堆（TASK-035）。
    winicon::set_app_user_model_id();
    println!("[AwayTerminal] ConPTY backend: {}", pty::backend_name());

    let args = LaunchArgs::from_env();
    // `args` 等一下會被 `.manage()` 吃掉，這個旗標要先抄一份給 `setup` 用
    let verifying = args.verify > 0;
    if args.cmd.is_some() || args.verify > 0 || args.bench {
        println!("[AwayTerminal] 啟動參數：{args:?}");
    }

    let mut builder = tauri::Builder::default();
    // Tauri updater（階段 5）：**只有設了公鑰才掛**。
    //
    // 沒設公鑰時這個功能整個不存在，「關於 → 檢查更新」照 TASK-015 那條路
    // （`update.rs`，問 awaysu.cc 的 api.php，只告知有新版、不自動下載）。
    // 掛了 plugin 卻沒有公鑰＝任何人都能餵一包假的更新給使用者，所以寧可不掛。
    // 產生金鑰的步驟與發佈流程在 `docs/RELEASE.md`；**私鑰絕不進 repo**。
    if updater_pubkey_configured() {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
        println!("[AwayTerminal] updater：已設公鑰，自動更新可用");
    }
    builder
        // 單一執行個體（TASK-016 C）：檔案總管右鍵會再啟動一個 exe，
        // 它把 `--open-dir <路徑>` 交給**已經在跑的**那個視窗，然後自己結束
        //（同舊版 `IpcPipe`，只是底層從 Named Pipe 換成 plugin 的 mutex + 訊息）。
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let dir = crate::cli::LaunchArgs::parse(argv.into_iter().skip(1)).open_dir;
            println!("[AwayTerminal] 單一執行個體：第二個實例來了，--open-dir={dir:?}");
            // 把視窗拉到前面（使用者按了右鍵選單，期待看到視窗）
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
            if let Some(dir) = dir {
                let _ = app.emit("open-dir", dir);
            }
        }))
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
            commands::tab_adb,
            commands::view_mode_cycle,
            commands::tab_panel_set,
            adb::adb_devices,
            #[cfg(windows)]
            shellmenu::shell_menu_apply,
            #[cfg(windows)]
            shellmenu::shell_menu_state,
            commands::dir_exists,
            migrate::migrate_probe,
            migrate::migrate_pick_file,
            migrate::migrate_import,
            commands::settings_get,
            prefs::settings_apply,
            prefs::settings_readonly_reason,
            prefs::ssh_weak_clear,
            prefs::font_list,
            winicon::window_icon_probe,
            fontstore::font_catalog,
            fontstore::font_faces,
            fontstore::font_face_bytes,
            fontstore::font_import,
            fontstore::font_pick_files,
            fontstore::font_download,
            fontstore::font_download_cancel,
            fontstore::font_remove,
            i18n::i18n_keys,
            i18n::system_locale,
            i18n::i18n_push,
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
            toolbar::open_dir,
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
            agent::agent_setup_options,
            agent::agent_backends_any,
            agent::agent_roles_restore,
            agent::agent_roles_dir,
            agent::agent_bus_dir,
            agent::agent_team_create,
            agent::agent_slot_failed,
            agent::agent_team_ready,
            agent::agent_team_apply,
            agent::agent_team_apply_done,
            agent::agent_team_state,
            agent::agent_team_restore,
            agent::agent_team_rename,
            agent::chat_start,
            agent::chat_say,
            agent::chat_end,
            agent::chat_folder,
            agent::agent_delivery_set,
            agent::agent_stop,
            agent::agent_ratio,
            agent::agent_focused,
            agent::agent_teams,
            agent::agent_team_tabs,
            agent::agent_tab_closed,
            // 前端關完整組後通知（B4：之前沒登記，六處呼叫全被 `.catch` 吞掉，團隊留在 TeamManager）
            agent::agent_team_gone,
            agent::agent_verify_begin,
            agent::agent_verify_send,
            agent::agent_verify_state,
            agent::agent_verify_end,
            agent::chat_verify_transcript,
            // 模型清單與「上次選的模型」（2.0.2；單一連線與代理團隊共用）
            agent::models::cli_models,
            agent::models::cli_models_refresh_all,
            agent::models::conn_models,
            agent::models::model_remember,
            telegram::telegram_state,
            telegram::telegram_apply,
            telegram::telegram_get_chat_id,
            telegram::telegram_opened,
            telegram::telegram_tab_notify,
            telegram::telegram_tab_state,
            telegram::probe::telegram_probe,
            claudemd::claude_md_available,
            claudemd::claude_md_update,
        ])
        .setup(move |app| {
            // 設定檔要在任何 command 跑起來之前備好（`host_ready` 的 T{json} 直接讀它）
            let dir = app.path().app_config_dir()?;
            // 「這次是不是第一次啟動」要在**任何寫檔之前**問（autosave 很快就會把檔案寫出來，
            // 之後再問就永遠是 false）。匯入舊版設定的提示靠這個旗標。
            let first_run = !dir.join("settings.json").is_file();
            app.manage(crate::migrate::FirstRun(first_run));
            let store = Arc::new(SettingsStore::load(&dir));
            // 空的 log_dir 補成「我的文件\AwayTerminalLogs」（同舊版 AppSettings.Load）
            if let Ok(docs) = app.path().document_dir() {
                store.fill_log_dir(&docs);
            }
            // 內建後備的語言要在任何訊息產生之前設好（背景執行緒寫進畫面的字也吃這個）。
            // 空的＝使用者還沒選過 → 保持預設（繁中）；前端啟動時會用系統語言決定並推字串過來。
            let lang = store.get().language;
            if !lang.is_empty() {
                i18n::set_lang(&lang);
            }
            settings::spawn_autosave(store.clone());
            // 檔案總管右鍵選單：已登錄但指向別的 exe（搬家／升級）→ 重新指到這一支（D4）。
            // 背景做，登錄檔慢也不拖啟動；只在 release 建置做（見 `refresh_if_moved`）。
            #[cfg(windows)]
            std::thread::spawn(|| match shellmenu::refresh_if_moved() {
                Ok(true) => println!("[AwayTerminal] 檔案總管右鍵選單：exe 路徑變了，已重新登錄"),
                Ok(false) => {}
                Err(e) => println!("[AwayTerminal] 檔案總管右鍵選單重新登錄失敗：{e}"),
            });

            let view_mode = store.get().view_mode;
            let tabs = Arc::new(TabManager::new(&view_mode));

            apply_window_bounds(app.handle(), &store);
            // 工作列圖示（TASK-035）。實測 `WM_GETICON` 回 ICON_BIG=0、類別 HICON 也是 0 →
            // Windows 只好畫預設的空白圖示。Tauri 的 default_window_icon 只掛上了小圖示。
            // 這裡直接用 exe 自己的 .ico 資源補上大小兩份（dev 與 release 同一條路）。
            if let Some(win) = app.get_webview_window("main") {
                let (big, small) = winicon::apply_window_icon(&win);
                if !big || !small {
                    println!("[AwayTerminal] 視窗圖示：大圖示={big} 小圖示={small}（有 false 就是載不到 exe 的 .ico 資源）");
                }
            }
            status::spawn(app.handle().clone(), tabs.clone());
            // 代理團隊的投遞 tick（600ms，和狀態燈同一個節奏；沒有團隊時直接 return）
            agent::deliver::spawn(app.handle().clone());
            // 字型：先把「自帶」與「下載／匯入」兩個資料夾告訴 fonts 模組，再背景掃描。
            // 自帶的在 Tauri resources 底下；dev 沒有 resources，往上找 repo 的 src-tauri/fonts
            //（同 `third_party_notices` 與 conpty 的找法）。
            fonts::init(builtin_font_dir(app.handle()), dir.join("fonts"));
            // 字型清單先在背景掃好（幾百 MB 的字型資料夾要半秒多）：
            // 使用者按「其他設定」時下拉要立刻有東西，不能當場才掃（TASK-031）
            fonts::warm_up();

            // Telegram 遠端：設定裡開著就拉起輪詢（要在 manage 之前拿 store 的值）。
            //
            // ⚠️ **`--verify` 時不要拉起來**（TASK-035 發現）：那會用**使用者真正的 token**
            // 連上真正的 Telegram，只為了跑自動驗證——而且 `telegram_probe` 的第一項
            // 斷言就是「開始前遠端沒在跑」，使用者把遠端打開之後那一項必然 FAIL
            //（看起來像程式壞了，其實是驗證自己把它拉起來的）。驗證要的那條路
            // 由 `telegram_probe` 用**只聽 127.0.0.1 的假 Bot API** 完整跑過。
            if !verifying {
                telegram::start_if_enabled(app.handle(), &store);
            } else {
                println!("[AwayTerminal] --verify：不啟動真的 Telegram 遠端（改由 telegram_probe 用假 Bot API 驗）");
            }

            // 模型清單的「自動更新」（2.0.7）：每分鐘看一次時間，到了選的整點就重新問一次
            agent::models::spawn_auto_refresh(store.clone());

            app.manage(store);
            app.manage(tabs);
            app.manage(Arc::new(agent::TeamManager::default()));
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
                        // payload 是兩個勾選的上次狀態（舊版 `ExitDialog` 開起來就是上次的值）
                        let cur = store.get();
                        let payload = serde_json::json!({
                            "restore": cur.exit_restore_tabs,
                            "updateMd": cur.exit_update_md,
                        });
                        if let Err(e) = app.emit("exit-request", payload) {
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
///
/// 位置**要先驗證**還落在某個螢幕上（TASK-026）：外接螢幕拔掉、解析度改小、
/// 或設定檔裡留著最小化時存進去的假座標，照套就會開在看不到的地方——
/// 工作列有圖示、點了沒畫面，而且重開也救不回來。驗不過就不套位置、直接置中
/// （舊版 WPF 根本不記位置，`MainWindow.xaml` 是 `WindowStartupLocation="CenterScreen"`
/// ——「一定看得到」是舊版的既有行為，不能退步）。
/// 自帶字型的資料夾。
///
/// 安裝之後在 Tauri resources 底下的 `fonts/`；**開發時沒有 resources**，
/// 所以往上找 repo 的 `src-tauri/fonts`（同 `third_party_notices` 與 conpty 的找法）。
/// 兩個都沒有就回第一個候選——`fonts::scan` 讀不到就當作沒有自帶字型，不會爆。
fn builtin_font_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        candidates.push(dir.join("fonts"));
        candidates.push(dir.join("resources/fonts"));
    }
    if let Ok(exe) = std::env::current_exe() {
        for up in 1..=4 {
            if let Some(d) = exe.ancestors().nth(up) {
                candidates.push(d.join("fonts"));
                candidates.push(d.join("src-tauri/fonts"));
            }
        }
    }
    for c in &candidates {
        if c.is_dir() {
            return c.clone();
        }
    }
    candidates.into_iter().next().unwrap_or_default()
}

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
    let (Some(x), Some(y)) = (b.x, b.y) else {
        return;
    };
    let monitors = logical_monitors(&win);
    let rect = winpos::Rect::new(x, y, b.width as i32, b.height as i32);
    if monitors.is_empty() {
        println!("[AwayTerminal] 列不出螢幕清單，記住的視窗位置不套用，改為置中");
        let _ = win.center();
        return;
    }
    if !winpos::is_visible_on(rect, &monitors) {
        println!("[AwayTerminal] 記住的視窗位置 ({x},{y}) 不在任何螢幕上，改為置中");
        let _ = win.center();
        return;
    }
    let _ = win.set_position(tauri::LogicalPosition::new(x as f64, y as f64));
}

/// 所有螢幕的矩形，換算成**主視窗當下 scale factor 下的邏輯像素**。
///
/// 一定要用視窗的 scale factor，不能用各螢幕自己的：`set_position(LogicalPosition)`
/// 就是拿視窗的 scale factor 換回實體座標的，兩邊不一致的話混用 DPI 時會對不上。
fn logical_monitors(win: &tauri::WebviewWindow) -> Vec<winpos::Rect> {
    let scale = win.scale_factor().unwrap_or(1.0);
    let Ok(monitors) = win.available_monitors() else {
        return Vec::new();
    };
    monitors
        .iter()
        .map(|m| {
            let p = m.position().to_logical::<f64>(scale);
            let s = m.size().to_logical::<f64>(scale);
            winpos::Rect::new(
                p.x.round() as i32,
                p.y.round() as i32,
                s.width.round() as i32,
                s.height.round() as i32,
            )
        })
        .collect()
}

/// 最大化時只記 `maximized`，不覆寫還原後的大小（否則還原回來會變成整個螢幕）。
///
/// **最小化時整個不記**（TASK-026）：Windows 會把最小化的視窗移到實體座標
/// `(-32000,-32000)` 並照樣發 `Moved`／`Resized`；`maximized` 這時候也讀不準
/// （最小化前是不是最大化，`is_maximized()` 回答不了），所以連它一起不動。
fn remember_window_bounds(window: &tauri::Window, store: &SettingsStore) {
    if window.is_minimized().unwrap_or(false) {
        return;
    }
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
            let (x, y) = (pos.x.round() as i32, pos.y.round() as i32);
            // 第二道保險：萬一某個平台在 `is_minimized()` 變 true 之前就先送 `Moved`，
            // 這條會擋掉 −32000 那類不可能出現在真桌面上的座標。
            if winpos::plausible_position(x, y) {
                s.window.x = Some(x);
                s.window.y = Some(y);
            }
        }
    });
}

/// `tauri.conf.json` 的 `plugins.updater.pubkey` 有沒有填。
///
/// 讀的是**編譯時嵌進來的那份設定**（`include_str!`），不是執行時的檔案——設定檔就在
/// exe 裡，不會被使用者改掉，也不必等 `app.config()`（這個判斷要在 Builder 建起來之前）。
fn updater_pubkey_configured() -> bool {
    let conf = include_str!("../tauri.conf.json");
    serde_json::from_str::<serde_json::Value>(conf)
        .ok()
        .and_then(|v| {
            v.get("plugins")?
                .get("updater")?
                .get("pubkey")?
                .as_str()
                .map(|k| !k.trim().is_empty())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod version_tests {
    /// 版本號有三份來源，必須一致。
    ///
    /// * `Cargo.toml` 的 `version`（＝`CARGO_PKG_VERSION`，關於頁與更新檢查用）
    /// * `tauri.conf.json` 的 `version`（安裝檔檔名、MSI ProductVersion、updater 比對用）
    /// * `package.json` 的 `version`
    ///
    /// 漏改一處的後果是安靜的：`npm run tauri build` 照樣過，但安裝檔的版本與程式自己
    /// 報的版本不一樣 → 更新檢查會一直說有新版（或一直說已是最新）。
    #[test]
    fn the_three_version_sources_agree() {
        let cargo = env!("CARGO_PKG_VERSION");

        let conf = include_str!("../tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(conf).expect("tauri.conf.json 不是合法 JSON");
        let conf_ver = conf["version"].as_str().expect("tauri.conf.json 沒有 version");

        let pkg = include_str!("../../package.json");
        let pkg: serde_json::Value = serde_json::from_str(pkg).expect("package.json 不是合法 JSON");
        let pkg_ver = pkg["version"].as_str().expect("package.json 沒有 version");

        assert_eq!(cargo, conf_ver, "Cargo.toml 與 tauri.conf.json 的版本不一致");
        assert_eq!(cargo, pkg_ver, "Cargo.toml 與 package.json 的版本不一致");
    }

    /// 安裝檔要帶的東西不可以被改掉（授權宣告與 conpty 都是執行時真的需要的檔案）。
    ///
    /// 角色範本／護欄腳本是 `include_str!` 嵌在 exe 裡，不在這份清單——只有真的要
    /// 「以檔案形式存在」的才列進 `resources`。
    #[test]
    fn the_bundle_still_ships_the_notices_and_conpty() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let res = &conf["bundle"]["resources"];
        assert!(
            res.get("../THIRD-PARTY-NOTICES.md").is_some(),
            "安裝檔沒帶 THIRD-PARTY-NOTICES.md（russh 是 Apache-2.0、serialport 是 MPL-2.0、\
             TeraTerm 是 BSD-3，散布時必須附授權全文）"
        );
        assert!(res.get("resources/conpty/*").is_some(), "安裝檔沒帶 conpty");
        assert_eq!(
            conf["bundle"]["licenseFile"].as_str(),
            Some("../LICENSE"),
            "安裝檔的授權頁沒有指到 MIT 全文"
        );
    }

    /// updater 的設定骨架在、但**公鑰還沒填**（填了就代表要發自動更新了，
    /// 那時候這條測試要改成檢查格式，並確認 `docs/RELEASE.md` 的流程跑過一次）。
    #[test]
    fn the_updater_is_configured_but_disabled_until_a_key_exists() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let up = &conf["plugins"]["updater"];
        assert!(up.is_object(), "updater 的設定骨架不見了");
        assert!(
            up["endpoints"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false),
            "updater 沒有 endpoint"
        );
        assert_eq!(
            up["pubkey"].as_str(),
            Some(""),
            "公鑰被填進 repo 了？金鑰對由使用者自己產生與保管（docs/RELEASE.md），             而且**私鑰絕不進 repo**。真的要發自動更新時請改這條測試。"
        );
        assert!(
            !super::updater_pubkey_configured(),
            "沒有公鑰的時候不可以掛 updater plugin"
        );
    }

    /// 介面有八語，安裝檔的語言清單也要八種（`scripts/test-i18n.mjs` 管介面那一邊）。
    #[test]
    fn the_installer_offers_all_eight_languages() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let langs = conf["bundle"]["windows"]["nsis"]["languages"]
            .as_array()
            .expect("nsis.languages 不見了");
        // NSIS 自己的語言檔名稱（不是我們的語言代碼）
        for want in [
            "TradChinese",
            "English",
            "SimpChinese",
            "Japanese",
            "Korean",
            "Spanish",
            "German",
            "French",
        ] {
            assert!(
                langs.iter().any(|l| l.as_str() == Some(want)),
                "安裝檔少了 {want}"
            );
        }
        assert_eq!(langs.len(), 8);
    }
}
