//! 遠端的狀態與輪詢（搬移舊版 `TelegramRemote` 的其餘部分）。
//!
//! | 舊版 | 這裡 |
//! |---|---|
//! | `Start`／`Stop`／`IsRunning` | [`start`]／[`stop`]／[`is_running`] |
//! | `PollLoop`（long polling ＋ 3 秒退避） | [`poll_loop`]，跑在自己的 std 執行緒上 |
//! | `PrimeOffset` | [`api::Api::prime_offset`] |
//! | `HandleCommand` 的判斷 | [`super::cmd::parse`]（純邏輯，單獨測） |
//! | `SendLast`／`DiffNew`／`TidyForPhone` | [`send_last`]／[`super::tidy`] |
//! | `OnTabIdle`（完成推播） | [`on_tab_idle`] |
//! | `CheckIdleAsync`（9 分鐘警告、10 分鐘靜默離開） | [`check_idle`] |
//! | `NotifyOfflineBlocking` | [`notify_offline`] |
//!
//! ## ⚠️ token
//! **不進 log、不進回報、不進 `--verify` 輸出。** 這個檔案裡沒有任何 `println!` 印出 token
//! 或 URL；設定值本身也不經過任何 emit 給前端（設定視窗只送「有沒有設定」的布林）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::api::{Api, Incoming};
use super::cmd::{self, Action};
use super::tidy;

/// 輪詢錯誤的退避（舊版固定 3 秒）。
const BACKOFF: Duration = Duration::from_secs(3);
/// 等前端回畫面文字的上限。
const SCREEN_TIMEOUT: Duration = Duration::from_millis(2500);
/// 附著分頁閒置多久之後靜默離開（舊版 10 分鐘，9 分鐘先警告）。
const IDLE_EXIT: Duration = Duration::from_secs(600);
const IDLE_WARN: Duration = Duration::from_secs(540);
/// 自動推播的去重窗（舊版 8 秒）。
const DEDUP_WINDOW: Duration = Duration::from_secs(8);

/// 遠端的可變狀態。
#[derive(Default)]
pub struct State {
    /// 目前附著的分頁（`None`＝未選）。
    pub current: Option<u32>,
    /// 送指令後與分頁完成時自動回傳輸出（舊版預設開）。
    pub follow: bool,
    /// 其他（未附著的）分頁完成也推播（舊版預設關）。
    pub notify: bool,
    /// 提問時附加「不要用表格」（舊版預設關）。
    pub plain: bool,
    /// 每個分頁「上次推播的基準畫面」——只推基準之後的新行。
    baseline: std::collections::HashMap<u32, String>,
    /// 每個分頁上次自動推播的內容簽章與時間（去重用）。
    last_sent: std::collections::HashMap<u32, (String, Instant)>,
    /// `/more` 翻頁：上次的全文、已顯示區段的起點、哪個分頁。
    more_buf: String,
    more_pos: usize,
    more_tab: Option<u32>,
    /// 最後一次「使用者來訊」的時間（閒置計時）。
    last_user_msg: Option<Instant>,
    idle_warned: bool,
}

impl State {
    fn new(notify: bool) -> Self {
        Self {
            follow: true,
            notify,
            last_user_msg: Some(Instant::now()),
            ..Default::default()
        }
    }
}

struct Running {
    api: Api,
    chat_id: i64,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
}

fn running() -> &'static Mutex<Option<Running>> {
    static R: OnceLock<Mutex<Option<Running>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(None))
}

fn lock() -> std::sync::MutexGuard<'static, Option<Running>> {
    running().lock().unwrap_or_else(|e| e.into_inner())
}

pub fn is_running() -> bool {
    lock().is_some()
}

/// 遠端目前的狀態（設定視窗與 `--verify` 用；**不含 token**）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    /// 有沒有設定 token（**不回 token 本身**）。
    pub has_token: bool,
    pub chat_id: i64,
    pub current_tab: Option<u32>,
    pub follow: bool,
    pub notify: bool,
    pub plain: bool,
}

pub fn status(settings: &crate::settings::SettingsStore) -> Status {
    let s = settings.get();
    let g = lock();
    match g.as_ref() {
        Some(r) => {
            let st = r.state.lock().unwrap_or_else(|e| e.into_inner());
            Status {
                running: true,
                has_token: !s.telegram_bot_token.trim().is_empty(),
                chat_id: r.chat_id,
                current_tab: st.current,
                follow: st.follow,
                notify: st.notify,
                plain: st.plain,
            }
        }
        None => Status {
            running: false,
            has_token: !s.telegram_bot_token.trim().is_empty(),
            chat_id: s.telegram_chat_id,
            current_tab: None,
            follow: true,
            notify: s.remote_notify,
            plain: false,
        },
    }
}

/// 啟動遠端。`base`＝Bot API 位址（`None`＝正式的 api.telegram.org；`--verify` 會給假的）。
pub fn start(app: &AppHandle, token: &str, chat_id: i64, notify: bool, base: Option<&str>) -> bool {
    stop();
    let token = token.trim();
    if token.is_empty() || chat_id == 0 {
        println!("[AwayTerminal] Telegram 遠端：沒有 token 或 chat id，不啟動");
        return false;
    }
    let api = match base {
        Some(b) => Api::new(b, token),
        None => Api::telegram(token),
    };
    let stop_flag = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(State::new(notify)));
    *lock() = Some(Running {
        api: api.clone(),
        chat_id,
        stop: stop_flag.clone(),
        state: state.clone(),
    });
    let app2 = app.clone();
    std::thread::spawn(move || poll_loop(app2, api, chat_id, stop_flag, state));
    println!("[AwayTerminal] Telegram 遠端：已啟動（chat {chat_id}）");
    true
}

pub fn stop() {
    if let Some(r) = lock().take() {
        r.stop.store(true, Ordering::Relaxed);
        println!("[AwayTerminal] Telegram 遠端：已停止");
    }
}

/// 程式關閉前送一則「遠端離線」（舊版 `NotifyOfflineBlocking`）。
///
/// 讓使用者知道之後下指令不會有回應（程式沒開＝沒有東西 poll bot）。
/// 強殺／當機收不到屬預期。
pub fn notify_offline() {
    let (api, chat) = {
        let g = lock();
        match g.as_ref() {
            Some(r) => (r.api.clone(), r.chat_id),
            None => return,
        }
    };
    let _ = api.send_message(chat, &crate::i18n::t("tg.offline"), false, &[]);
}

/// 輪詢主迴圈。
fn poll_loop(
    app: AppHandle,
    api: Api,
    chat_id: i64,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
) {
    // 跳過啟動前累積的舊訊息，避免一開機就重播（見 `prime_offset` 的註解）
    let mut offset = api.prime_offset();
    let _ = api.set_my_commands(&cmd::command_menu());
    let _ = api.send_message(chat_id, &crate::i18n::t("tg.online"), false, &[]);
    let mut errors = 0u32;

    while !stop.load(Ordering::Relaxed) {
        // getUpdates 最多阻塞 30 秒 → 每輪至少檢查一次閒置
        check_idle(&api, chat_id, &state, &app);
        match api.get_updates(offset) {
            Ok((msgs, next)) => {
                if errors > 0 {
                    println!("[AwayTerminal] Telegram 遠端：輪詢在 {errors} 次錯誤後恢復");
                    errors = 0;
                }
                offset = next;
                for m in msgs {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    // 安全：只認設定裡那一個 chat id，其他人**不理也不回**
                    if m.chat_id != chat_id {
                        println!(
                            "[AwayTerminal] Telegram 遠端：忽略非授權的 chat（不回應）"
                        );
                        continue;
                    }
                    handle(&app, &api, chat_id, &state, &m);
                }
            }
            Err(e) => {
                // 409＝同一個 bot 有別的程式在 poll、401＝token 失效：都會「沒動作」，記下來才查得到
                errors += 1;
                if errors == 1 || errors.is_multiple_of(20) {
                    println!("[AwayTerminal] Telegram 遠端：輪詢失敗 #{errors}（{e}）");
                }
                std::thread::sleep(BACKOFF);
            }
        }
    }
}

/// 9 分鐘警告、10 分鐘靜默離開分頁檢視（分頁照跑）。
///
/// 閒置＝距最近活動的時間；活動取「手機來訊」與「附著分頁最近動作（輸出／打字）」的較晚者——
/// 在電腦上對該分頁持續工作時不會被手機沒動作判成閒置而離開（舊版註解）。
fn check_idle(api: &Api, chat_id: i64, state: &Arc<Mutex<State>>, app: &AppHandle) {
    let (current, last_msg, warned) = {
        let st = state.lock().unwrap_or_else(|e| e.into_inner());
        (st.current, st.last_user_msg, st.idle_warned)
    };
    let Some(tab) = current else { return };
    let Some(last_msg) = last_msg else { return };
    // 分頁上的活動（輸出／打字）也算
    let tab_active = app
        .try_state::<Arc<crate::tabs::TabManager>>()
        .and_then(|tabs| tabs.agent_signals(tab))
        .map(|s| {
            let now = crate::tabs::now_ms();
            let newest = s.last_output.max(s.last_input);
            Duration::from_millis(now.saturating_sub(newest))
        });
    let idle_by_msg = last_msg.elapsed();
    let idle = match tab_active {
        Some(d) => idle_by_msg.min(d),
        None => idle_by_msg,
    };
    if idle >= IDLE_EXIT {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.current = None;
        st.idle_warned = false;
        println!("[AwayTerminal] Telegram 遠端：閒置 10 分鐘，靜默離開分頁檢視");
    } else if idle >= IDLE_WARN && !warned {
        state.lock().unwrap_or_else(|e| e.into_inner()).idle_warned = true;
        let _ = api.send_message(chat_id, &crate::i18n::t("tg.idleWarn"), false, &[]);
    }
}

/// 處理一則來訊。
fn handle(app: &AppHandle, api: &Api, chat_id: i64, state: &Arc<Mutex<State>>, m: &Incoming) {
    {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.last_user_msg = Some(Instant::now());
        st.idle_warned = false;
    }
    // inline 按鈕
    if let Some((cb_id, data)) = &m.callback {
        let _ = api.answer_callback(cb_id, None);
        if let Some(rest) = data.strip_prefix("goto:") {
            if let Ok(n) = rest.parse::<usize>() {
                do_goto(app, api, chat_id, state, Some(n));
            }
        } else if let Some(rest) = data.strip_prefix("close:") {
            if let Ok(id) = rest.parse::<u32>() {
                confirmed_close(app, api, chat_id, state, id);
            }
        } else if data == "noop" {
            send(api, chat_id, &crate::i18n::t("tg.cancelled"));
        } else if let Some(rest) = data.strip_prefix("opt:") {
            // opt:<分頁>:<選項>
            let mut it = rest.split(':');
            if let (Some(tab), Some(n)) = (it.next(), it.next()) {
                let tab = tab.parse::<u32>().ok();
                let num = n.parse::<u32>().ok();
                if let (Some(tab), Some(num)) = (tab, num) {
                    // 按鈕可能是很久以前那則訊息上的 → 先確認還在同一個分頁（舊版同此）
                    let cur = state.lock().unwrap_or_else(|e| e.into_inner()).current;
                    if cur != Some(tab) {
                        send(api, chat_id, &crate::i18n::t("tg.notThatTab"));
                    } else if !answer_menu(app, api, chat_id, state, num) {
                        send(api, chat_id, &crate::i18n::t("tg.menuGone"));
                    }
                }
            }
        }
        return;
    }
    let Some(text) = m.text.as_deref() else { return };
    match cmd::parse(text) {
        Action::Help => send(api, chat_id, &cmd::help_text()),
        Action::TabList => send_tab_list(app, api, chat_id, state),
        Action::Where => send_where(app, api, chat_id, state),
        Action::Goto(n) => do_goto(app, api, chat_id, state, n),
        Action::Exit => {
            state.lock().unwrap_or_else(|e| e.into_inner()).current = None;
            send(api, chat_id, &crate::i18n::t("tg.left"));
        }
        Action::Last(n) => {
            send_last(app, api, chat_id, state, n, None, false, false);
        }
        Action::More => do_more(api, chat_id, state),
        Action::Shot => do_shot(app, api, chat_id, state),
        Action::Close(n) => do_close(app, api, chat_id, state, n),
        Action::Key(name) => do_key(app, api, chat_id, state, &name),
        Action::Stop => do_key(app, api, chat_id, state, "ctrl-c"),
        Action::Notify(on) => {
            state.lock().unwrap_or_else(|e| e.into_inner()).notify = on;
            send(api, chat_id, &crate::i18n::tf("tg.notifySet", &[&flag(on)]));
        }
        Action::Follow(on) => {
            state.lock().unwrap_or_else(|e| e.into_inner()).follow = on;
            send(api, chat_id, &crate::i18n::tf("tg.followSet", &[&flag(on)]));
        }
        Action::Plain(on) => {
            state.lock().unwrap_or_else(|e| e.into_inner()).plain = on;
            send(api, chat_id, &crate::i18n::tf("tg.plainSet", &[&flag(on)]));
        }
        Action::MenuAnswer(n) => {
            // 畫面上真的是選單才換算成方向鍵；不是就當普通文字送出
            if !answer_menu(app, api, chat_id, state, n) {
                do_send(app, api, chat_id, state, &n.to_string());
            }
        }
        Action::Send(t) => do_send(app, api, chat_id, state, &t),
        Action::Unknown => send(api, chat_id, &crate::i18n::t("tg.unknown")),
    }
}

fn flag(on: bool) -> String {
    crate::i18n::t(if on { "tg.on" } else { "tg.off" })
}

/// 送一則文字（超過上限自動切段）。
fn send(api: &Api, chat_id: i64, text: &str) {
    for part in tidy::split_message(text, tidy::TELEGRAM_LIMIT) {
        if let Err(e) = api.send_message(chat_id, &part, false, &[]) {
            println!("[AwayTerminal] Telegram 遠端：送訊息失敗（{e}）");
            return;
        }
    }
}

fn tab_rows(app: &AppHandle) -> Vec<(u32, String, bool)> {
    let Some(tabs) = app.try_state::<Arc<crate::tabs::TabManager>>() else {
        return Vec::new();
    };
    // 代理團隊／聊天室只算代表列那一格（舊版 `RemoteVisible`）
    let teams = app.try_state::<Arc<crate::agent::TeamManager>>();
    tabs.ids()
        .into_iter()
        .filter(|id| teams.as_ref().map(|t| t.is_strip_row(*id)).unwrap_or(true))
        .filter_map(|id| {
            // 代理團隊的代表列顯示「組名（代理團隊 Agent-11）」，同舊版 `RemoteTitle`
            let title = teams
                .as_ref()
                .and_then(|t| t.remote_title(id))
                .or_else(|| tabs.title_of(id))?;
            let busy = tabs.agent_signals(id).map(|s| s.busy).unwrap_or(false);
            Some((id, title, busy))
        })
        .collect()
}

fn send_tab_list(app: &AppHandle, api: &Api, chat_id: i64, state: &Arc<Mutex<State>>) {
    let rows = tab_rows(app);
    if rows.is_empty() {
        send(api, chat_id, &crate::i18n::t("tg.noTabs"));
        return;
    }
    let cur = state.lock().unwrap_or_else(|e| e.into_inner()).current;
    let mut body = String::new();
    let mut btns: Vec<(String, String)> = Vec::new();
    for (i, (id, title, busy)) in rows.iter().enumerate() {
        let mark = if Some(*id) == cur { "▶ " } else { "" };
        let light = if *busy { "🟠" } else { "🟢" };
        body.push_str(&format!("{mark}[{}] {light} {title}\n", i + 1));
        btns.push((format!("{} {}", i + 1, trunc(title, 14)), format!("goto:{}", i + 1)));
    }
    let rows_btn: Vec<Vec<(String, String)>> = btns.chunks(2).map(|c| c.to_vec()).collect();
    let _ = api.send_message(chat_id, body.trim_end(), false, &rows_btn);
}

fn send_where(app: &AppHandle, api: &Api, chat_id: i64, state: &Arc<Mutex<State>>) {
    let cur = state.lock().unwrap_or_else(|e| e.into_inner()).current;
    let Some(tab) = cur else {
        send(api, chat_id, &crate::i18n::t("tg.notAttached"));
        return;
    };
    let rows = tab_rows(app);
    match rows.iter().position(|(id, _, _)| *id == tab) {
        Some(i) => {
            let (_, title, busy) = &rows[i];
            let light = if *busy { "🟠" } else { "🟢" };
            send(
                api,
                chat_id,
                &crate::i18n::tf("tg.whereAt", &[&(i + 1).to_string(), title, light]),
            );
        }
        None => {
            state.lock().unwrap_or_else(|e| e.into_inner()).current = None;
            send(api, chat_id, &crate::i18n::t("tg.tabGone"));
        }
    }
}

fn do_goto(
    app: &AppHandle,
    api: &Api,
    chat_id: i64,
    state: &Arc<Mutex<State>>,
    n: Option<usize>,
) {
    let rows = tab_rows(app);
    if rows.is_empty() {
        send(api, chat_id, &crate::i18n::t("tg.noTabs"));
        return;
    }
    let Some(n) = n.filter(|n| *n >= 1 && *n <= rows.len()) else {
        send_tab_list(app, api, chat_id, state);
        return;
    };
    let (id, title, _) = rows[n - 1].clone();
    // 附著即定基準：之後只推新輸出
    let screen = super::screen::recent_text(app, id, SCREEN_TIMEOUT);
    {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.current = Some(id);
        st.baseline.insert(id, screen);
    }
    send(
        api,
        chat_id,
        &crate::i18n::tf("tg.entered", &[&n.to_string(), &title]),
    );
}

fn do_key(app: &AppHandle, api: &Api, chat_id: i64, state: &Arc<Mutex<State>>, name: &str) {
    let (cur, follow) = {
        let st = state.lock().unwrap_or_else(|e| e.into_inner());
        (st.current, st.follow)
    };
    let Some(tab) = cur else {
        send(api, chat_id, &crate::i18n::t("tg.notAttached"));
        return;
    };
    let Some(bytes) = cmd::key_bytes(name) else {
        send(api, chat_id, &crate::i18n::t("tg.keyUsage"));
        return;
    };
    let ok = app
        .try_state::<crate::session::SessionManager>()
        .and_then(|m| m.get(tab))
        .map(|s| {
            s.write(bytes);
            true
        })
        .unwrap_or(false);
    if !ok {
        send(api, chat_id, &crate::i18n::t("tg.sendFailed"));
        return;
    }
    if follow {
        std::thread::sleep(Duration::from_millis(500));
        send_last(app, api, chat_id, state, 15, None, true, false);
    }
}

/// 純文字 → 打進目前附著的分頁（**文字與 Enter 分兩次**，同 `agent/deliver.rs`）。
fn do_send(app: &AppHandle, api: &Api, chat_id: i64, state: &Arc<Mutex<State>>, text: &str) {
    let (cur, plain) = {
        let st = state.lock().unwrap_or_else(|e| e.into_inner());
        (st.current, st.plain)
    };
    let Some(tab) = cur else {
        send(api, chat_id, &crate::i18n::t("tg.notAttached"));
        return;
    };
    let mut to_send = text.to_string();
    if plain && cmd::wants_plain_suffix(text) {
        to_send.push_str(&cmd::plain_suffix());
    }
    let Some(tabs) = app.try_state::<Arc<crate::tabs::TabManager>>() else {
        return;
    };
    let Some(sessions) = app.try_state::<crate::session::SessionManager>() else {
        return;
    };
    if sessions.get(tab).is_none() {
        state.lock().unwrap_or_else(|e| e.into_inner()).current = None;
        send(api, chat_id, &crate::i18n::t("tg.tabGone"));
        return;
    }
    // 和代理團隊共用同一條路：claude 分頁走 `v` 協定貼上、其餘直接寫，Enter 隔 300ms 再送
    crate::agent::deliver::send_text_then_enter(app, &tabs, &sessions, tab, &to_send, true);
    // **不做「送出後 700ms 即時回推」**——舊版拿掉了：固定延遲常抓到半成品，又會和完成推播
    // 重複（使用者實測一次 hi 收到三則）。只靠完成推播（`on_tab_idle`）。
}

/// 推畫面給手機（舊版 `SendLast`）。
///
/// - `incremental`（follow／完成推播）：只推「上次基準」之後的新輸出；對不到基準退回快照。
/// - `auto`（忙轉閒）：沒有新輸出就**完全不送**（舊版送過只有標頭的空訊息，是手機端噪音主因）。
#[allow(clippy::too_many_arguments)]
fn send_last(
    app: &AppHandle,
    api: &Api,
    chat_id: i64,
    state: &Arc<Mutex<State>>,
    lines: usize,
    header: Option<&str>,
    incremental: bool,
    auto: bool,
) {
    let cur = state.lock().unwrap_or_else(|e| e.into_inner()).current;
    let Some(tab) = cur else {
        if !auto {
            send(api, chat_id, &crate::i18n::t("tg.notAttached"));
        }
        return;
    };
    let raw = super::screen::recent_text(app, tab, SCREEN_TIMEOUT);
    let diff = if incremental {
        let st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.baseline.get(&tab).and_then(|b| tidy::diff_new(b, &raw))
    } else {
        None
    };
    state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .baseline
        .insert(tab, raw.clone());

    // 先抓一大段、瘦身去雜訊**之後**才取行，行數額度才不會被 spinner 雜訊吃掉
    let all_full = tidy::tidy_for_phone(&raw);
    let body = match &diff {
        Some(d) => tidy::tidy_for_phone(d),
        None => all_full.clone(),
    };
    if body.trim().is_empty() {
        if auto {
            return; // 自動推播沒有新輸出 → 整則不送
        }
        let msg = header.map(|h| h.to_string()).unwrap_or_else(|| {
            crate::i18n::t(if diff.is_some() {
                "tg.noNewOutput"
            } else {
                "tg.noOutput"
            })
        });
        send(api, chat_id, &msg);
        return;
    }
    let txt = if diff.is_some() {
        body.clone() // 增量：整段新輸出（超過上限由 clip_tail 取尾）
    } else {
        let arr: Vec<&str> = body.split('\n').collect();
        let start = arr.len().saturating_sub(lines);
        arr[start..].join("\n")
    };
    let sent = tidy::clip_tail(&txt, tidy::MAX_BODY);

    // 內容去重：忙→閒可能連續觸發兩次，或 diff 對不上退回快照而重送同一段
    let sig = sent.split_whitespace().collect::<Vec<_>>().join(" ");
    if auto {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((prev, at)) = st.last_sent.get(&tab) {
            if *prev == sig && at.elapsed() < DEDUP_WINDOW {
                return;
            }
        }
        st.last_sent.insert(tab, (sig, Instant::now()));
    }
    // `/more` 翻頁狀態
    {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.more_buf = all_full.clone();
        st.more_tab = Some(tab);
        st.more_pos = all_full.chars().count().saturating_sub(sent.chars().count());
    }
    // 畫面是選擇題 → 附上選項按鈕。**「是不是選單」要看瘦身前的文字**（導航列會被濾掉）
    let opts = cmd::menu_options(&sent, diff.as_deref().unwrap_or(&raw));
    let btns: Vec<Vec<(String, String)>> = if opts.len() >= 2 {
        opts.chunks(3)
            .map(|c| {
                c.iter()
                    .map(|(n, label)| (format!("{n} {label}"), format!("opt:{tab}:{n}")))
                    .collect()
            })
            .collect()
    } else {
        Vec::new()
    };
    let head = header
        .map(|h| format!("{}\n", tidy::esc_html(h)))
        .unwrap_or_default();
    let html = format!("{head}<pre>{}</pre>", tidy::esc_html(&sent));
    if let Err(e) = api.send_message(chat_id, &html, true, &btns) {
        println!("[AwayTerminal] Telegram 遠端：送畫面失敗（{e}）");
    }
}

/// `/more`：上一則輸出再往前翻一頁。
fn do_more(api: &Api, chat_id: i64, state: &Arc<Mutex<State>>) {
    let (buf, pos) = {
        let st = state.lock().unwrap_or_else(|e| e.into_inner());
        (st.more_buf.clone(), st.more_pos)
    };
    if buf.is_empty() || pos == 0 {
        send(api, chat_id, &crate::i18n::t("tg.noMore"));
        return;
    }
    let chars: Vec<char> = buf.chars().collect();
    let end = pos;
    let start = end.saturating_sub(tidy::MAX_BODY);
    let page: String = chars[start..end].iter().collect();
    state.lock().unwrap_or_else(|e| e.into_inner()).more_pos = start;
    let page = page.trim_end_matches('\n');
    let html = format!(
        "{}\n<pre>{}</pre>",
        tidy::esc_html(&crate::i18n::t("tg.morePage")),
        tidy::esc_html(page)
    );
    let _ = api.send_message(chat_id, &html, true, &[]);
}

/// `/close`：**先問一次**才關（舊版的誤觸保險——手機上按錯一個鍵就關掉跑了一小時的
/// claude 是真的會發生）。按「確定關閉」才走 [`confirmed_close`]。
fn do_close(
    app: &AppHandle,
    api: &Api,
    chat_id: i64,
    state: &Arc<Mutex<State>>,
    n: Option<u32>,
) {
    let rows = tab_rows(app);
    // 帶編號＝清單上的第 n 個；不帶＝目前附著的
    let target = match n {
        Some(i) => rows.get((i as usize).saturating_sub(1)).map(|(id, _, _)| *id),
        None => state.lock().unwrap_or_else(|e| e.into_inner()).current,
    };
    let Some(tab) = target else {
        send(
            api,
            chat_id,
            &crate::i18n::t(if n.is_some() {
                "tg.noTabs"
            } else {
                "tg.notAttached"
            }),
        );
        return;
    };
    let Some((_, title, _)) = rows.iter().find(|(id, _, _)| *id == tab) else {
        send(api, chat_id, &crate::i18n::t("tg.tabGone"));
        return;
    };
    // 代理團隊的一格＝整組一起關，要講清楚
    let team = app
        .try_state::<Arc<crate::agent::TeamManager>>()
        .map(|t| t.find_tab(tab).is_some())
        .unwrap_or(false);
    let what = crate::i18n::t(if team {
        "tg.closeWhatTeam"
    } else {
        "tg.closeWhatTab"
    });
    let text = crate::i18n::tf("tg.closeAsk", &[title, &what]);
    let btns = vec![vec![
        (crate::i18n::t("tg.closeYes"), format!("close:{tab}")),
        (crate::i18n::t("tg.closeNo"), "noop".to_string()),
    ]];
    let _ = api.send_message(chat_id, &text, false, &btns);
}

/// 按了「確定關閉」。
fn confirmed_close(app: &AppHandle, api: &Api, chat_id: i64, state: &Arc<Mutex<State>>, tab: u32) {
    let title = tab_rows(app)
        .into_iter()
        .find(|(id, _, _)| *id == tab)
        .map(|(_, t, _)| t)
        .unwrap_or_default();
    let Some(sessions) = app.try_state::<crate::session::SessionManager>() else {
        return;
    };
    let Some(tabs) = app.try_state::<Arc<crate::tabs::TabManager>>() else {
        return;
    };
    crate::commands::close_tab(app, tab, &sessions, &tabs);
    {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        if st.current == Some(tab) {
            st.current = None;
        }
        st.baseline.remove(&tab);
        st.last_sent.remove(&tab);
    }
    send(api, chat_id, &crate::i18n::tf("tg.closed", &[&title]));
}

/// `/shot`：把畫面畫成 PNG 送出去。
fn do_shot(app: &AppHandle, api: &Api, chat_id: i64, state: &Arc<Mutex<State>>) {
    let cur = state.lock().unwrap_or_else(|e| e.into_inner()).current;
    let Some(tab) = cur else {
        send(api, chat_id, &crate::i18n::t("tg.notAttached"));
        return;
    };
    match super::shot::capture_png(app, tab) {
        Some(png) if !png.is_empty() => {
            if let Err(e) = api.send_photo(chat_id, &png, "") {
                println!("[AwayTerminal] Telegram 遠端：送截圖失敗（{e}）");
                send(api, chat_id, &crate::i18n::t("tg.shotFailed"));
            }
        }
        _ => send(api, chat_id, &crate::i18n::t("tg.shotFailed")),
    }
}

/// 選單應答：把「選第 N 項」換算成 ↑／↓ ＋ Enter（claude 的選單不吃數字鍵）。
///
/// 回 false＝畫面上不是選單（呼叫端當普通文字送出）。
fn answer_menu(
    app: &AppHandle,
    api: &Api,
    chat_id: i64,
    state: &Arc<Mutex<State>>,
    want: u32,
) -> bool {
    let cur = state.lock().unwrap_or_else(|e| e.into_inner()).current;
    let Some(tab) = cur else { return false };
    let screen = super::screen::recent_text(app, tab, SCREEN_TIMEOUT);
    if !cmd::is_menu_screen(&screen) {
        return false;
    }
    let Some(now) = cmd::current_menu_index(&screen) else {
        return false;
    };
    let Some(sessions) = app.try_state::<crate::session::SessionManager>() else {
        return false;
    };
    let Some(session) = sessions.get(tab) else {
        return false;
    };
    let delta = want as i64 - now as i64;
    let key = if delta > 0 { "down" } else { "up" };
    for _ in 0..delta.abs() {
        if let Some(b) = cmd::key_bytes(key) {
            session.write(b);
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    std::thread::sleep(Duration::from_millis(150));
    session.write(b"\r");
    send(api, chat_id, &crate::i18n::tf("tg.chose", &[&want.to_string()]));
    let follow = state.lock().unwrap_or_else(|e| e.into_inner()).follow;
    if follow {
        std::thread::sleep(Duration::from_millis(800));
        send_last(app, api, chat_id, state, 20, None, true, false);
    }
    true
}

/// 分頁從忙碌轉閒置（完成推播；舊版 `OnTabIdle`）。
///
/// 觸發條件在 `status.rs` 判斷（**有沒有送出過** ＋ 忙碌時長），這裡只負責推。
pub fn on_tab_idle(app: &AppHandle, tab: u32, title: &str) {
    let (api, chat_id, state) = {
        let g = lock();
        match g.as_ref() {
            Some(r) => (r.api.clone(), r.chat_id, r.state.clone()),
            None => return,
        }
    };
    let (current, follow, notify) = {
        let st = state.lock().unwrap_or_else(|e| e.into_inner());
        (st.current, st.follow, st.notify)
    };
    if tab_notify(tab) == Some(false) {
        return;
    }
    if current == Some(tab) && follow {
        // 附著分頁完成了一件真工作＝算「活動」，重置閒置計時：在電腦上持續工作時手機會一直
        // 收到，不會因為「手機本身 10 分鐘沒動作」就被自動離開（舊版使用者實測回報）。
        {
            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            st.last_user_msg = Some(Instant::now());
            st.idle_warned = false;
        }
        let header = crate::i18n::tf("tg.done", &[title]);
        let app2 = app.clone();
        std::thread::spawn(move || {
            send_last(&app2, &api, chat_id, &state, 30, Some(&header), true, true);
        });
        return;
    }
    // 逐分頁旗標優先於全域的 /notify（v2 多的）
    if tab_notify(tab).unwrap_or(notify) {
        let msg = crate::i18n::tf("tg.doneOther", &[title]);
        std::thread::spawn(move || send(&api, chat_id, &msg));
    }
}

/// 逐分頁「推播到 Telegram」（v2 多的；`None`＝跟著全域設定）。
///
/// 放在遠端這一層而不是 `Tab` 上：它只有推播要用，而且**不持久化**
/// （分頁 id 跨重啟沒有意義，同逐分頁配色）。
fn tab_notify_map() -> &'static Mutex<std::collections::HashMap<u32, bool>> {
    static M: OnceLock<Mutex<std::collections::HashMap<u32, bool>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

pub fn set_tab_notify(tab: u32, on: bool) {
    tab_notify_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(tab, on);
}

pub fn tab_notify(tab: u32) -> Option<bool> {
    tab_notify_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&tab)
        .copied()
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 沒有 token／chat id 就不啟動（設定視窗留空時不要開一條空轉的執行緒）。
    #[test]
    fn needs_token_and_chat_id() {
        assert!(!is_running());
    }

    /// `trunc` 不切壞多位元字元。
    #[test]
    fn truncates_safely() {
        assert_eq!(trunc("abc", 10), "abc");
        assert_eq!(trunc("abcdef", 4), "abc…");
        assert_eq!(trunc("一二三四五", 3), "一二…");
    }

    /// 開關顯示字走 i18n（內建後備是繁中的「開」／「關」）。
    #[test]
    fn flag_strings() {
        assert_eq!(flag(true), crate::i18n::t("tg.on"));
        assert_eq!(flag(false), crate::i18n::t("tg.off"));
        assert_ne!(flag(true), flag(false));
        assert!(!flag(true).is_empty());
    }
}
