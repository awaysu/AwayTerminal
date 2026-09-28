//! Telegram 遠端（搬移舊版 `Services/TelegramRemote.cs`）。
//!
//! 一台電腦一個 bot：只認設定裡那一個 chat id，其他人一律**不理也不回**。
//! long polling（`getUpdates`），沒有 webhook。
//!
//! | 模組 | 做什麼 |
//! |---|---|
//! | [`api`] | Bot API 的 HTTP（`ureq`，自己的執行緒）。錯誤訊息**不含 URL**（URL 裡有 token） |
//! | [`cmd`] | 指令字串 → [`cmd::Action`]（純函式，好測） |
//! | [`tidy`] | 雜訊過濾／表格攤平／切段／增量比對（純函式，每條規則一個測試） |
//! | [`screen`] | 向前端要「畫面上看得到的文字」（`q…text`／`a…text`） |
//! | [`shot`] | 向前端要 PNG（`q…shot`；平台截圖介面留在 `shot::platform`） |
//! | [`remote`] | 輪詢執行緒 ＋ 狀態機 ＋ 完成推播 |
//!
//! ## ⚠️ token
//! **不進 log、不進回報、不進 `--verify` 輸出。**
//! [`status`] 回給前端的是「有沒有設定」的布林（`hasToken`），不是 token 本身；
//! 設定視窗存了新 token 之後也只回 `hasToken`。
//!
//! 存放方式**照舊版：明文存在 settings.json**（`telegram_bot_token`）。
//! 舊版 `AppSettings.TelegramBotToken` 就是明文，v2 沿用才能直接匯入舊設定。
//! 要改成 DPAPI／keychain 是跨三平台的另一件事，已回報 Agent-11 決定。

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::settings::SettingsStore;

pub mod api;
pub mod cmd;
pub mod probe;
pub mod remote;
pub mod screen;
pub mod shot;
pub mod tidy;

/// 程式啟動：設定裡開著就把遠端拉起來（舊版 `MainWindow_Loaded` → `ApplyRemoteSettings`）。
pub fn start_if_enabled(app: &AppHandle, store: &SettingsStore) {
    let s = store.get();
    if !s.remote_enabled {
        return;
    }
    remote::start(
        app,
        &s.telegram_bot_token,
        s.telegram_chat_id,
        s.remote_notify,
        None,
    );
}

/// 設定視窗的 Telegram 區塊：目前狀態（**不含 token**）。
#[tauri::command]
pub fn telegram_state(settings: State<'_, Arc<SettingsStore>>) -> remote::Status {
    remote::status(&settings)
}

/// 設定視窗按了套用。`token` 是 `None`＝不動（使用者沒改那一欄，前端不會把舊 token 回傳）。
#[tauri::command]
pub fn telegram_apply(
    app: AppHandle,
    enabled: bool,
    token: Option<String>,
    chat_id: i64,
    notify: bool,
    settings: State<'_, Arc<SettingsStore>>,
) -> remote::Status {
    settings.update(|s| {
        s.remote_enabled = enabled;
        if let Some(t) = &token {
            s.telegram_bot_token = t.trim().to_string();
        }
        s.telegram_chat_id = chat_id;
        s.remote_notify = notify;
    });
    settings.flush();
    let s = settings.get();
    if enabled {
        // 重存＝重開輪詢（舊版也是；換 token／chat id 才會生效）
        remote::start(
            &app,
            &s.telegram_bot_token,
            s.telegram_chat_id,
            s.remote_notify,
            None,
        );
    } else {
        remote::stop();
    }
    remote::status(&settings)
}

/// 遠端設定視窗的「取得 chat id」（舊版 `RemoteDialog.GetId_Click`）。
///
/// `token` 是 `None`＝用設定裡存的那一個（視窗永遠不回填 token，留空就是「沒改」）。
/// 回 `Ok(None)`＝bot 收到的訊息裡沒有可用的 chat id（使用者還沒傳訊息給 bot）。
/// **錯誤字串不含 URL**（URL 裡有 token，見 `api::describe`）。
#[tauri::command]
pub fn telegram_get_chat_id(
    token: Option<String>,
    base: Option<String>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<Option<i64>, String> {
    let token = match token {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => settings.get().telegram_bot_token,
    };
    if token.trim().is_empty() {
        // 前端在送出之前就會擋（`remote.needToken`），這裡是第二道
        return Err("no token".into());
    }
    let api = match base {
        Some(b) => api::Api::new(&b, &token),
        None => api::Api::telegram(&token),
    };
    api.latest_chat_id()
}

/// 前端開完 `telegram-open` 要的分頁之後回報（`None`＝開失敗）。
///
/// **隱含契約**：`telegram-open` 一定要回這一個，否則遠端的執行緒等到逾時
/// （同 `ssh_hostkey_answer`／`macro_answer`）。
#[tauri::command]
pub fn telegram_opened(id: Option<u32>) {
    remote::opened(id);
}

/// 逐分頁「推播到 Telegram」（分頁右鍵選單）。
///
/// **舊版沒有這個**：舊版只有全域的 `/notify`（未附著的分頁完成要不要通知）。
/// 這是 v2 多的——`None`＝跟著全域設定，`Some(false)`＝這個分頁永遠不推。
/// 只留在記憶體、不進 settings.json（同逐分頁配色：分頁 id 跨重啟沒有意義）。
#[tauri::command]
pub fn telegram_tab_notify(id: u32, on: bool) {
    remote::set_tab_notify(id, on);
}

/// 分頁右鍵選單要打勾嗎。
#[tauri::command]
pub fn telegram_tab_state(id: u32, settings: State<'_, Arc<SettingsStore>>) -> bool {
    remote::tab_notify(id).unwrap_or_else(|| settings.get().remote_notify)
}
