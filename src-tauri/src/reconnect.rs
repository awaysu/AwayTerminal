//! 分頁層的「遠端連線」：連線參數、建立 session、斷線自動重連。
//!
//! TASK-009 只有 SSH，TASK-010 把它**泛化**成 [`ConnParams`]（SSH／Telnet 共用一套退避與提示），
//! 因為舊版的連線視窗本來就是兩種共用同一組欄位（保持連線、斷線自動重連），
//! `OnSessionExited` / `ScheduleReconnect` / `ManualReconnect` 也沒有分連線種類。
//!
//! 行為**逐項照舊版** `MainWindow.xaml.cs`（對照表在 `docs/SSH.md` 第 5 節）：
//!
//! | 舊版 | 這裡 |
//! |---|---|
//! | 退避 `min(30, 3 × 次數)` 秒：3,6,9…30 | [`schedule`] |
//! | 畫面上印「[連線中斷，N 秒後自動重連…（關閉分頁可停止）]」 | [`schedule`] |
//! | 沒勾自動重連 → 印「[連線已結束]」＋「[按 Enter 在此分頁重新連線]」 | [`on_exit`] |
//! | 等待中按 Enter ＝不等退避、立刻連 | [`manual`] |
//! | 重連前先用 `b` 協定把舊畫面推進 scrollback | [`reconnect_now`] |
//! | 一個分頁同時只有一條重連鏈 | `reconnect_gen`（排程時記下世代，醒來發現變了就放棄） |
//! | 分頁已關閉／已經重連好了 → 放棄 | 同上 |
//!
//! ## ⚠️「連上了」怎麼判斷（各後端不同）
//!
//! 舊版是「一收到輸出就把退避次數歸零」。那條規則建立在「輸出全部來自 `ssh.exe`」這個副作用上，
//! 內建實作**不成立**：我們自己的狀態訊息也走同一條輸出 callback。所以每個後端各自給一個
//! 明確的里程碑（`OnConnected`）：
//!
//! | 後端 | 「連上了」＝ |
//! |---|---|
//! | SSH | `request_shell` 成功（shell channel 開起來） |
//! | Telnet | **從 socket 讀到第一批位元組**（沒有 shell channel 可用；我們自己的訊息不經過 socket） |
//! | COM | **開埠成功**（序列裝置可能永遠不主動說話，等輸出會讓退避永遠不歸零） |

use crate::i18n::{t, tf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::host::emit_host;
use crate::output::OutputPump;
use crate::session::{ExitInfo, OnExit, OnOutput, SessionManager};
use crate::com::ComParams;
use crate::ssh::conn::SshConnParams;
use crate::tabs::{self, TabManager};
use crate::telnet::TelnetParams;

/// 一條遠端連線的完整參數（分頁層記住它，用來重連、存我的最愛、恢復分頁）。
///
/// **沒有任何密碼欄位**——舊版的 `SavedTab` 也沒有，密碼一律當場問。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ConnParams {
    Ssh(SshConnParams),
    Telnet(TelnetParams),
    Com(ComParams),
}

impl ConnParams {
    pub fn auto_reconnect(&self) -> bool {
        match self {
            Self::Ssh(p) => p.auto_reconnect,
            Self::Telnet(p) => p.auto_reconnect,
            Self::Com(p) => p.auto_reconnect,
        }
    }

    /// 診斷用（後端 log）。
    pub fn target(&self) -> String {
        match self {
            Self::Ssh(p) => format!("ssh://{}:{}", p.host, p.port),
            Self::Telnet(p) => format!("telnet://{}:{}", p.host, p.port),
            Self::Com(p) => format!("com://{}@{}", p.port, p.baud),
        }
    }

    pub fn as_ssh(&self) -> Option<&SshConnParams> {
        match self {
            Self::Ssh(p) => Some(p),
            _ => None,
        }
    }

    pub fn as_telnet(&self) -> Option<&TelnetParams> {
        match self {
            Self::Telnet(p) => Some(p),
            _ => None,
        }
    }

    pub fn as_com(&self) -> Option<&ComParams> {
        match self {
            Self::Com(p) => Some(p),
            _ => None,
        }
    }
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

/// 後端回報「真的連上了」→ 退避次數歸零。各後端的判斷見本檔開頭的表。
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
    let Some(params) = tabs.conn_params_of(id) else {
        return;
    };

    // 這條已經死掉的 session 還掛在 SessionManager 上（是 `start` 放進去的），
    // **一定要先取下來**：否則下面的「已經連上了嗎」永遠成立，重連根本不會排。
    // 取下來之後在背景 drop（drop 會關 socket／送優雅結束鍵，別卡住這條 task）。
    if let Some(manager) = app.try_state::<SessionManager>() {
        if let Some(dead) = manager.remove(id) {
            std::thread::spawn(move || drop(dead));
        }
    }

    // 使用者自己取消的（主機金鑰／弱演算法按「取消」、登入提示按 Ctrl+C）不可以自動重連：
    // 否則 3 秒後又跳同一個問題，無限循環（E3）。改成跟「沒勾自動重連」一樣，等使用者按 Enter。
    if params.auto_reconnect() && !info.user_cancelled {
        schedule(app, id);
    } else {
        // 舊版：灰字「[連線已結束]」＋黃字「[按 Enter 在此分頁重新連線]」
        echo(
            app,
            id,
            &format!(
                "\r\n\x1b[90m{}\x1b[0m \x1b[33m{}\x1b[0m\r\n",
                t("term.connEnded"),
                t("term.pressEnter")
            ),
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
            "\r\n\x1b[33m{}\x1b[0m\r\n",
            tf("term.reconnectIn", &[&delay.to_string()])
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
    let Some(params) = tabs.conn_params_of(id) else {
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
            if params.auto_reconnect() {
                schedule(app, id);
            } else {
                echo(
                    app,
                    id,
                    &format!("\x1b[33m{}\x1b[0m\r\n", t("term.pressEnter")),
                );
            }
        }
    }
}

/// 這個分頁的輸出／結束管線（兩種後端共用）。
///
/// `parts` 是分頁**既有**的輸出管線與 log 槽——重連要沿用它們，
/// 否則輸出會流不到原本那條 channel（畫面就死了）。
fn pipeline(app: &AppHandle, id: u32, parts: &tabs::SessionParts) -> (OnOutput, OnExit) {
    let on_output = {
        let pump = parts.pump.clone();
        let last_output = parts.last_output.clone();
        let logger = parts.logger.clone();
        let tap = parts.tap.clone();
        Arc::new(move |bytes: &[u8]| {
            last_output.store(tabs::now_ms(), Ordering::Relaxed);
            // TTL 巨集的 `wait` 要看得到輸出（現在一定是空槽，見 src/tap.rs）
            tap.output(bytes);
            // log 先寫再餵畫面：舊版 OnSessionOutput 也是這個順序
            // 先把 Logger 複製出來、放掉槽的鎖再寫檔：磁碟慢／防毒卡住時不可以握著槽的鎖
            //（`state_with` 與關分頁都要鎖它，握著會讓整個 UI 一起凍住）
            let l = logger.lock().ok().and_then(|g| g.clone());
            if let Some(l) = l {
                l.write(bytes);
            }
            pump.push(bytes);
        }) as OnOutput
    };

    let on_exit = {
        let app = app.clone();
        let pump: Arc<OutputPump> = parts.pump.clone();
        Arc::new(move |info: ExitInfo| {
            // 排空還沒送出的輸出，但**不要** stop——重連要繼續用同一條 pump
            pump.flush();
            on_exit(&app, id, info);
        }) as OnExit
    };

    (on_output, on_exit)
}

/// 建立（或重建）一條遠端 session 並掛進 `SessionManager`。
pub fn start(
    app: &AppHandle,
    id: u32,
    params: &ConnParams,
    parts: tabs::SessionParts,
) -> Result<(), String> {
    let manager = app
        .try_state::<SessionManager>()
        .ok_or_else(|| t("err.connListNotReady").to_string())?;
    // 分頁原本的 pump 可能已經停了（PTY 分頁的行程結束後巨集 `connect`）→ 用同一條
    // channel 重開，否則新連線的輸出全被丟掉（C4）。還在跑就什麼都不做。
    parts.pump.ensure_running();
    let (on_output, on_exit) = pipeline(app, id, &parts);
    // 連線瞬間失敗時（Telnet／COM），後端的 on_exit 可能在下面 `manager.insert` **之前**
    // 就跑完了：那時 `remove` 拿到 None，之後才把死 session 登記上去 → 分頁永遠卡在
    // 「已連上」（C3）。所以 on_exit 先立旗標，insert 之後再看一次。
    let exited = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let on_exit = {
        let exited = exited.clone();
        Arc::new(move |info: ExitInfo| {
            exited.store(true, Ordering::SeqCst);
            on_exit(info);
        }) as OnExit
    };
    let on_connected = {
        let app = app.clone();
        Arc::new(move || note_connected(&app, id))
    };

    let session: Arc<dyn crate::session::TerminalSession> = match params {
        ConnParams::Ssh(p) => {
            // 連線視窗填的密碼只在分頁的記憶體裡（重連沿用；沒填＝在終端機問）
            let password = app
                .try_state::<Arc<TabManager>>()
                .and_then(|tabs| tabs.ssh_password_of(id));
            crate::ssh::conn::start(app, id, p, password, &parts, on_output, on_exit, on_connected)?
        }
        ConnParams::Com(p) => {
            // 開埠是同步的：失敗就回 Err（呼叫端會印紅字／收掉分頁，同舊版 SerialPort.Open()）
            let (session, warnings) = crate::com::spawn(p, on_output, on_exit, Some(on_connected))?;
            // 參數被 crate 的限制降級時要**說出來**（1.5 停止位元、Mark/Space 同位…）
            for w in warnings {
                echo(app, id, &format!("\r\n\x1b[33m[{w}]\x1b[0m\r\n"));
            }
            session
        }
        ConnParams::Telnet(p) => crate::telnet::spawn(
            crate::telnet::TelnetOptions {
                host: p.host.clone(),
                port: p.port,
                cols: parts.cols,
                rows: parts.rows,
                keepalive_mins: p.keepalive_mins,
            },
            on_output,
            on_exit,
            Some(on_connected),
        ),
    };
    manager.insert(id, session.clone());
    if exited.load(Ordering::SeqCst) {
        // on_exit 已經在 insert 之前跑完（它 remove 不到、該排的重連也排了）：
        // 把剛登記的死 session 收回來。只收**自己這一條**，不會誤收之後重連上的新 session。
        if let Some(dead) = manager.remove_if_same(id, &session) {
            // drop 會關 socket／送優雅結束鍵，別卡在呼叫端（可能是 IPC 執行緒）
            std::thread::spawn(move || {
                drop(dead);
                drop(session);
            });
        }
    }
    Ok(())
}

pub(crate) fn echo(app: &AppHandle, id: u32, text: &str) {
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

    /// 兩種後端共用同一條退避規則（舊版的 `ScheduleReconnect` 也不分種類）。
    #[test]
    fn both_kinds_share_the_same_switches() {
        let ssh = ConnParams::Ssh(SshConnParams {
            auto_reconnect: true,
            ..Default::default()
        });
        let telnet = ConnParams::Telnet(TelnetParams {
            auto_reconnect: true,
            ..Default::default()
        });
        assert!(ssh.auto_reconnect() && telnet.auto_reconnect());
        assert!(ssh.as_ssh().is_some() && ssh.as_telnet().is_none());
        assert!(telnet.as_telnet().is_some() && telnet.as_ssh().is_none());
    }

    /// 連線參數**不含密碼**——我的最愛與恢復分頁也是存這個結構。
    #[test]
    fn conn_params_have_no_password_field() {
        for p in [
            ConnParams::Ssh(SshConnParams::default()),
            ConnParams::Telnet(TelnetParams::default()),
        ] {
            let json = serde_json::to_string(&p).unwrap();
            for bad in ["password", "passwd", "passphrase", "secret"] {
                assert!(!json.contains(bad), "連線參數不可以有 {bad} 欄位：{json}");
            }
        }
    }

    /// enum 的 JSON 形狀（設定檔與我的最愛存的就是這個，改了會讓舊設定讀不回來）。
    #[test]
    fn json_shape_is_tagged_by_kind() {
        let json = serde_json::to_string(&ConnParams::Telnet(TelnetParams {
            host: "10.0.0.5".into(),
            port: 2323,
            keepalive_mins: 10,
            auto_reconnect: true,
        }))
        .unwrap();
        assert!(json.contains("\"kind\":\"telnet\""), "{json}");
        let back: ConnParams = serde_json::from_str(&json).unwrap();
        assert_eq!(back.as_telnet().unwrap().port, 2323);
    }
}
