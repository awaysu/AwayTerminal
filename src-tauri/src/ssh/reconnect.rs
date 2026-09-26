//! SSH 分頁層：連線參數、建立 session、斷線自動重連（B5）。
//!
//! 行為**逐項照舊版** `MainWindow.xaml.cs` 的 `OnSessionExited` / `ScheduleReconnect`
//! / `ManualReconnect` / `TryReconnect`（對照表在 `docs/SSH.md` 第 5 節）：
//!
//! | 舊版 | 這裡 |
//! |---|---|
//! | 退避 `min(30, 3 × 次數)` 秒：3,6,9…30 | [`schedule`] |
//! | 一收到輸出就把次數歸零 | [`note_output`]（由輸出 callback 呼叫） |
//! | 畫面上印「[連線中斷，N 秒後自動重連…（關閉分頁可停止）]」 | [`schedule`] |
//! | 沒勾自動重連 → 印「[連線已結束]」＋「[按 Enter 在此分頁重新連線]」 | [`on_exit`] |
//! | 等待中按 Enter ＝不等退避、立刻連 | [`manual`] |
//! | 重連前先用 `b` 協定把舊畫面推進 scrollback（ConPTY 的 `ESC[2J` 會吃掉最後一頁） | [`reconnect_now`] |
//! | 一個分頁同時只有一條重連鏈 | `reconnect_gen`（排程時記下世代，醒來發現變了就放棄） |
//! | 分頁已關閉／已經重連好了 → 放棄 | 同上 |
//!
//! **重連沿用已接受的主機金鑰與弱演算法決定**（兩者都記在檔案／設定裡，不是記在 session 上），
//! 但金鑰真的變了照樣會擋——因為每次重連都會重跑 `check_server_key`。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::host::emit_host;
use crate::output::OutputPump;
use crate::session::{ExitInfo, SessionManager};
use crate::settings::SettingsStore;
use crate::tabs::{self, TabManager};

/// 一條 SSH 連線的完整參數。
///
/// 這個結構同時是：分頁層重連要記住的東西、B6 對話框的欄位、以及**我的最愛**存的內容。
/// **刻意沒有密碼欄位**——舊版的 `SavedTab` 也沒有（我的最愛不存密碼），密碼一律當場問。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SshConnParams {
    pub host: String,
    pub port: u16,
    /// 空＝連上後在終端機問 `login as:`（PuTTY 式）。
    pub user: String,
    /// 私鑰檔（OpenSSH 或 `.ppk`）。
    pub key_path: String,
    /// 要不要試 Pageant／ssh-agent。
    pub use_agent: bool,
    /// 保持連線的間隔（分鐘），0＝關。
    pub keepalive_mins: u32,
    /// 斷線自動重連（舊版連線視窗的勾選）。
    pub auto_reconnect: bool,
    pub algos: super::algos::AlgoOverride,
    /// 要送給遠端的環境變數（SSH `env` request）。
    /// 舊版是 `ssh.exe` 的 `-o SendEnv=…`；伺服器拒絕時只印一行灰字，不擋連線。
    pub env: Vec<(String, String)>,
}

/// 退避上限與係數（舊版 `Math.Min(30, 3 * attempt)`）。
const BACKOFF_STEP_SECS: u64 = 3;
const BACKOFF_MAX_SECS: u64 = 30;

/// 第 `attempt` 次重連要等幾秒（舊版 `Math.Min(30, 3 * attempt)`）。
pub fn backoff_secs(attempt: u32) -> u64 {
    (BACKOFF_STEP_SECS * attempt as u64).min(BACKOFF_MAX_SECS)
}

/// 重連鏈的世代編號。排程時記下，醒來時對不上就放棄（同舊版「一條鏈」的保證）。
static GEN: AtomicU64 = AtomicU64::new(1);

fn next_gen() -> u64 {
    GEN.fetch_add(1, Ordering::Relaxed) + 1
}

/// shell 真的開起來了 → 退避次數歸零。
///
/// ⚠️ 舊版是「一收到輸出就歸零」。**我們不能照那樣做**：自己的狀態訊息
/// （「連線到 host:port …」、`login as:`、錯誤訊息）也走同一條輸出 callback，
/// 會被誤認成「連上了」→ 退避永遠停在第 1 次的 3 秒（實測踩到，`--verify` 抓出來的）。
/// 改成由 `OnConnected` 呼叫，語意是「shell channel 開成功」。
pub fn note_connected(app: &AppHandle, id: u32) {
    if let Some(tabs) = app.try_state::<Arc<TabManager>>() {
        tabs.reset_reconnect_attempt(id);
    }
}

/// session 結束時由 `on_exit` 呼叫。決定「排重連」還是「印提示等 Enter」。
pub fn on_exit(app: &AppHandle, id: u32, info: ExitInfo) {
    let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
        return;
    };
    // 分頁已經被關掉（使用者按 ✕ / 程式結束）→ 什麼都不做
    let Some(params) = tabs.ssh_params_of(id) else {
        return;
    };
    let _ = info;

    // 這條已經死掉的 session 還掛在 SessionManager 上（是 `start` 放進去的），
    // **一定要先取下來**：否則下面的「已經連上了嗎」永遠成立，重連根本不會排。
    // 取下來之後在背景 drop（`SshSession::drop` 會送優雅結束鍵並小睡，別卡住這條 task）。
    if let Some(manager) = app.try_state::<SessionManager>() {
        if let Some(dead) = manager.remove(id) {
            std::thread::spawn(move || drop(dead));
        }
    }

    if params.auto_reconnect {
        schedule(app, id);
    } else {
        // 舊版：灰字「[連線已結束]」＋黃字「[按 Enter 在此分頁重新連線]」
        echo(
            app,
            id,
            "\r\n\x1b[90m[連線已結束]\x1b[0m \x1b[33m[按 Enter 在此分頁重新連線]\x1b[0m\r\n",
        );
        tabs::emit_state(app, &tabs);
    }
}

/// 排一次自動重連（退避 3,6,9…最多 30 秒）。
pub fn schedule(app: &AppHandle, id: u32) {
    let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
        return;
    };
    let attempt = tabs.bump_reconnect_attempt(id);
    let delay = backoff_secs(attempt);
    let gen = next_gen();
    tabs.set_reconnect_gen(id, gen);

    echo(
        app,
        id,
        &format!(
            "\r\n\x1b[33m[連線中斷，{delay} 秒後自動重連…（關閉分頁可停止）]\x1b[0m\r\n"
        ),
    );
    tabs::emit_state(app, &tabs);

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(delay));
        let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
            return;
        };
        // 這條鏈還是最新的嗎？（期間按過 Enter、或分頁關掉了 → 放棄）
        if tabs.reconnect_gen_of(id) != Some(gen) {
            return;
        }
        if app
            .try_state::<SessionManager>()
            .is_some_and(|m| m.get(id).is_some())
        {
            return; // 已經連上了
        }
        reconnect_now(&app, id);
    });
}

/// 使用者在已結束的分頁按 Enter：不等退避、立刻連（舊版 `ManualReconnect`）。
pub fn manual(app: &AppHandle, id: u32) {
    let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
        return;
    };
    tabs.reset_reconnect_attempt(id);
    // 作廢還在倒數的那條鏈
    tabs.set_reconnect_gen(id, next_gen());
    reconnect_now(app, id);
}

/// 真的重連。沿用同一個分頁、同一條輸出 channel。
fn reconnect_now(app: &AppHandle, id: u32) {
    let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
        return;
    };
    let Some(params) = tabs.ssh_params_of(id) else {
        return;
    };
    let Some(parts) = tabs.session_parts_of(id) else {
        return;
    };

    // 1.0.45：先把舊畫面整頁推進 scrollback、游標歸位（`b` 協定，兩欄皆空＝只推）。
    // 不先推的話，新 session 的第一幀清可視區會讓「最後一頁」舊訊息憑空消失。
    emit_host(app, format!("b{id}\x1f\x1f"));

    match start(app, id, &params, parts) {
        Ok(()) => {
            tabs::emit_state(app, &tabs);
        }
        Err(e) => {
            echo(app, id, &format!("\r\n\x1b[31m{e}\x1b[0m\r\n"));
            if params.auto_reconnect {
                schedule(app, id);
            } else {
                echo(app, id, "\x1b[33m[按 Enter 在此分頁重新連線]\x1b[0m\r\n");
            }
        }
    }
}

/// 建立（或重建）一條 SSH session 並掛進 `SessionManager`。
///
/// `parts` 是這個分頁**既有**的輸出管線與 log 槽——重連要沿用它們，
/// 否則輸出會流不到原本那條 channel（畫面就死了）。
pub fn start(
    app: &AppHandle,
    id: u32,
    params: &SshConnParams,
    parts: tabs::SessionParts,
) -> Result<(), String> {
    let settings = app
        .try_state::<Arc<SettingsStore>>()
        .ok_or_else(|| "設定還沒準備好".to_string())?;
    let tabs = app
        .try_state::<Arc<TabManager>>()
        .ok_or_else(|| "分頁清單還沒準備好".to_string())?;
    let manager = app
        .try_state::<SessionManager>()
        .ok_or_else(|| "連線清單還沒準備好".to_string())?;

    let tabs::SessionParts {
        pump,
        logger,
        last_output,
        cols,
        rows,
    } = parts;

    let on_output = {
        let pump = pump.clone();
        let last_output = last_output.clone();
        let logger = logger.clone();
        Arc::new(move |bytes: &[u8]| {
            last_output.store(tabs::now_ms(), Ordering::Relaxed);
            if let Ok(g) = logger.lock() {
                if let Some(l) = g.as_ref() {
                    l.write(bytes);
                }
            }
            pump.push(bytes);
        }) as crate::session::OnOutput
    };

    let on_exit = {
        let app = app.clone();
        let pump: Arc<OutputPump> = pump.clone();
        Arc::new(move |info: ExitInfo| {
            // 排空還沒送出的輸出，但**不要** stop——重連要繼續用同一條 pump
            pump.flush();
            on_exit_inner(&app, id, info);
        }) as crate::session::OnExit
    };

    let on_user: super::OnUser = {
        let app = app.clone();
        let tabs = tabs.inner().clone();
        let host = params.host.clone();
        Arc::new(move |user: &str| {
            let target = format!("{user}@{host}");
            if tabs.set_title(id, &target, false) {
                emit_host(&app, format!("t{id}\x1f{target}"));
                tabs::emit_state(&app, &tabs);
            }
            // 記住帳號：重連就不必再問 login as:（同舊版把 Restore.Host 改成 user@host）
            tabs.set_ssh_user(id, user);
        })
    };

    let store = Arc::new(super::hostkey::HostKeyStore::new(
        settings.dir().join("known_hosts"),
    ));
    let decider = Arc::new(super::prompt::AppDecider::new(
        app.clone(),
        id,
        store.path().to_string_lossy().to_string(),
        (*settings).clone(),
    ));

    let session = super::spawn(
        super::SshOptions {
            host: params.host.clone(),
            port: params.port,
            user: (!params.user.trim().is_empty()).then(|| params.user.clone()),
            cols,
            rows,
            auth: super::SshAuth {
                key_path: (!params.key_path.trim().is_empty()).then(|| params.key_path.clone()),
                key_passphrase: None, // 有密碼的金鑰當場問（同 PuTTY）
                use_agent: params.use_agent,
            },
            algos: params.algos.clone(),
            keepalive_mins: params.keepalive_mins,
            env: params.env.clone(),
        },
        store,
        decider,
        on_output,
        on_exit,
        Some(on_user),
        Some({
            let app = app.clone();
            Arc::new(move || note_connected(&app, id)) as super::OnConnected
        }),
    );
    manager.insert(id, session);
    Ok(())
}

/// `on_exit` 的內部版本（`start` 的 callback 用；分出來避免遞迴型別問題）。
fn on_exit_inner(app: &AppHandle, id: u32, info: ExitInfo) {
    on_exit(app, id, info);
}

fn echo(app: &AppHandle, id: u32, text: &str) {
    if let Some(tabs) = app.try_state::<Arc<TabManager>>() {
        if let Some(parts) = tabs.session_parts_of(id) {
            parts.pump.push(text.as_bytes());
            parts.pump.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 退避序列逐字對舊版：3,6,9,…,30，之後一直是 30。
    #[test]
    fn backoff_matches_old_version() {
        assert_eq!(backoff_secs(1), 3);
        assert_eq!(backoff_secs(2), 6);
        assert_eq!(backoff_secs(3), 9);
        assert_eq!(backoff_secs(9), 27);
        assert_eq!(backoff_secs(10), 30);
        assert_eq!(backoff_secs(11), 30, "上限 30 秒");
        assert_eq!(backoff_secs(1000), 30);
    }

    /// 連線參數**不含密碼**——我的最愛也是存這個結構，舊版的 `SavedTab` 同樣沒有密碼欄位。
    #[test]
    fn conn_params_have_no_password_field() {
        let json = serde_json::to_string(&SshConnParams::default()).unwrap();
        for bad in ["password", "passwd", "passphrase", "secret"] {
            assert!(!json.contains(bad), "連線參數不可以有 {bad} 欄位：{json}");
        }
    }
}
