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

use std::sync::mpsc::{self, Sender};

use tauri::{AppHandle, Emitter, Manager};

use super::api::{Api, Incoming};
use super::cmd::{self, Action};
use super::tidy;

/// 輪詢錯誤的退避起點（舊版固定 3 秒，沒有上限也不遞增）。
///
/// TASK-036 改成**遞增**：每失敗一次乘二，最多 [`BACKOFF_MAX`]，成功就歸零。
/// 舊做法在 token 失效時 12 分鐘打了 240 次，既吵又沒有用。
const BACKOFF: Duration = Duration::from_secs(3);
/// 退避上限。再久使用者會覺得「怎麼都沒反應」，再短就變成洗版。
const BACKOFF_MAX: Duration = Duration::from_secs(60);
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

    /// 把一個已經不在的分頁的東西都丟掉（附著、基準畫面、去重、翻頁）。
    fn forget_tab(&mut self, tab: u32) {
        if self.current == Some(tab) {
            self.current = None;
            self.idle_warned = false;
        }
        self.baseline.remove(&tab);
        self.last_sent.remove(&tab);
        if self.more_tab == Some(tab) {
            self.more_tab = None;
            self.more_buf.clear();
            self.more_pos = 0;
        }
    }
}

/// 分頁關掉了（`commands::close_tab` 呼叫；UI 關、遠端 `/close` 關都會走到）。
///
/// BUG H7：以前遠端狀態不清——附著中的分頁從電腦上關掉後，`/last` 要等 2.5 秒才回
/// 「沒有輸出」而不是「分頁已關閉」；每個分頁的基準畫面（最多 400 行）永遠不釋放。
/// 遠端沒在跑也要清逐分頁推播設定（那張表不跟著遠端的啟停）。
pub fn tab_closed(id: u32) {
    tab_notify_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    let state = lock().as_ref().map(|r| r.state.clone());
    if let Some(state) = state {
        state.lock().unwrap_or_else(|e| e.into_inner()).forget_tab(id);
    }
}

/// 附著的分頁還在嗎（`/last`、`/shot`、選單應答用）。不在就清掉它的狀態、回「分頁已關閉」。
///
/// 看的是**分頁**在不在，不是連線：斷線等重連的 SSH 分頁畫面還在，`/last` 照樣要能看。
fn tab_gone(app: &AppHandle, api: &Api, chat_id: i64, state: &Arc<Mutex<State>>, tab: u32) -> bool {
    let gone = app
        .try_state::<Arc<crate::tabs::TabManager>>()
        .map(|t| !t.contains(tab))
        .unwrap_or(false);
    if gone {
        state.lock().unwrap_or_else(|e| e.into_inner()).forget_tab(tab);
        send(api, chat_id, &crate::i18n::t("tg.tabGone"));
    }
    gone
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
    /// 上一次**不是使用者按的**停止原因（目前只有 401／404），沒有就是 `None`。
    /// 遠端設定視窗打開時顯示它，不然使用者只會看到「已停止」而不知道為什麼。
    pub stopped_reason: Option<String>,
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
                stopped_reason: None, // 正在跑就沒有「為什麼停了」
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
            stopped_reason: stopped_reason(),
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
    clear_stopped_reason(); // 使用者自己停的，不是錯誤
    if let Some(r) = lock().take() {
        r.stop.store(true, Ordering::Relaxed);
        println!("[AwayTerminal] Telegram 遠端：已停止");
    }
}

/// 「上一次**不是使用者按的**停止原因」。只活在這一次執行期間
/// （重開程式之後 `remoteEnabled` 已經是 false，不會再去打 Telegram，所以不必持久化）。
fn reason_slot() -> &'static Mutex<Option<String>> {
    static R: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(None))
}

/// 遠端設定視窗要顯示的停止原因（沒有就是 `None`）。
pub fn stopped_reason() -> Option<String> {
    reason_slot().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn clear_stopped_reason() {
    *reason_slot().lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// **致命錯誤**（token 無效／bot 不存在）：停掉輪詢、把設定裡的開關關掉、通知前端一次。
///
/// 為什麼要把 `remote_enabled` 設回 false：不然下次啟動又會拿同一個壞掉的 token
/// 再打 240 次。使用者到「遠端設定」重新填 token 時會自己再打開。
///
/// 通知走 `telegram-fatal` 事件，前端顯示一次。**這個函式一輪只會被呼叫一次**
/// （呼叫完 `poll_loop` 就 return 了），所以不會洗版。
///
/// `own_stop` 是呼叫它的那條輪詢執行緒自己的停止旗標（BUG H2）：使用者重存設定時，
/// 舊執行緒已經被 `stop()`、新的 `Running` 已經登記——舊執行緒這時才拿到 401 的話，
/// 以前會 `take()` 走**新的** `Running`、把開關關回去。現在只在「正在跑的就是自己」時才動。
///
/// 對著本機假 Bot API（`--verify`）時不 emit `telegram-fatal`（BUG H9：不然驗證途中
/// 前端會跳「Bot Token 無效」對話框）。停止原因與設定照改——那正是 `--verify` 要驗的，
/// 它驗完會把 `remote_enabled` 放回去。
fn fatal_stop(app: &AppHandle, api: &Api, own_stop: &Arc<AtomicBool>, err: &str, status: u16) {
    if own_stop.load(Ordering::Relaxed) {
        // 自己早就被停掉了（使用者按了停止或重存）→ 這個錯誤跟現在的設定無關
        println!("[AwayTerminal] Telegram 遠端：已停止的輪詢收到 {err}，忽略");
        return;
    }
    let reason = crate::i18n::tf("tg.fatalToken", &[&status.to_string()]);
    println!("[AwayTerminal] Telegram 遠端：{err} → 停止輪詢（token 無效，不再重試）");
    // 先把自己從「正在跑」名單拿掉（`stop()` 會清掉原因，所以這裡自己來）
    {
        let mut g = lock();
        let mine = g.as_ref().is_some_and(|r| Arc::ptr_eq(&r.stop, own_stop));
        own_stop.store(true, Ordering::Relaxed);
        if !mine {
            // 名單上已經是新的一條了 → 不碰它、也不改設定
            return;
        }
        g.take();
    }
    *reason_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(reason.clone());
    // 設定裡的開關也關掉並存檔（`--verify` 會自己記下原值、驗完放回去）
    if let Some(store) = app.try_state::<Arc<crate::settings::SettingsStore>>() {
        store.update(|s| s.remote_enabled = false);
    }
    if api.is_local() {
        println!("[AwayTerminal] Telegram 遠端：假 Bot API（--verify），不通知前端");
        return;
    }
    if let Err(e) = app.emit("telegram-fatal", reason) {
        println!("[AwayTerminal] Telegram 遠端：通知前端失敗：{e}");
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
    // 跳過啟動前累積的舊訊息，避免一開機就重播（見 `prime_offset` 的註解）。
    // **一定要成功才開始輪詢**（BUG H1）：開機時網路還沒好就退回 offset 0 的話，
    // 第一次成功輪詢會把關機期間的 `/goto`、`/new`、文字整批執行。失敗就退避重試。
    let mut backoff = BACKOFF;
    let mut prime_errors = 0u32;
    let mut offset = loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match api.prime_offset() {
            Ok(o) => break o,
            Err(e) => {
                if super::api::is_fatal(&e) {
                    let status = super::api::http_status(&e).unwrap_or(0);
                    fatal_stop(&app, &api, &stop, &e, status);
                    return;
                }
                prime_errors += 1;
                if prime_errors == 1 || prime_errors.is_multiple_of(20) {
                    println!(
                        "[AwayTerminal] Telegram 遠端：prime offset 失敗 #{prime_errors}（{e}），{} 秒後重試",
                        backoff.as_secs()
                    );
                }
                sleep_unless_stopped(backoff, &stop);
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    };
    backoff = BACKOFF;
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
                    backoff = BACKOFF; // 恢復了就把退避歸零，下次出問題重新從 3 秒開始
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
                // 這一輪輪詢期間自己已經被停掉了 → 錯誤與現在的設定無關（BUG H2）
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                // **401／404 ＝ 重試永遠不會成功**（token 無效或 bot 不存在）→ 停掉，別再打。
                // 舊做法一視同仁重試，實測 12 分鐘打了 240 次還是 401（TASK-036）。
                if super::api::is_fatal(&e) {
                    let status = super::api::http_status(&e).unwrap_or(0);
                    fatal_stop(&app, &api, &stop, &e, status);
                    return;
                }
                // 其餘（409＝別的程式在 poll、5xx、逾時、斷線）都是暫時性 → 重試，但退避遞增
                errors += 1;
                if errors == 1 || errors.is_multiple_of(20) {
                    println!(
                        "[AwayTerminal] Telegram 遠端：輪詢失敗 #{errors}（{e}），{} 秒後重試",
                        backoff.as_secs()
                    );
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }
}

/// 睡 `d`，但每 200ms 看一次停止旗標（退避最長 60 秒，使用者按停止不該等那麼久才真的停）。
fn sleep_unless_stopped(d: Duration, stop: &AtomicBool) {
    let until = Instant::now() + d;
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= until {
            break;
        }
        std::thread::sleep((until - now).min(Duration::from_millis(200)));
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
        } else if let Some(rest) = data.strip_prefix("new:") {
            if let Ok(n) = rest.parse::<usize>() {
                do_new(app, api, chat_id, state, Some(n));
            }
        } else if let Some(rest) = data.strip_prefix("hist:") {
            if let Ok(n) = rest.parse::<usize>() {
                do_history(app, api, chat_id, state, Some(n));
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
        Action::New(n) => do_new(app, api, chat_id, state, n),
        Action::Ssh(arg) => do_open_remote(app, api, chat_id, state, &arg, true),
        Action::Telnet(arg) => do_open_remote(app, api, chat_id, state, &arg, false),
        Action::History(n) => do_history(app, api, chat_id, state, n),
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
        // 等不到畫面就不定基準（BUG H4：空字串當基準會讓下一次推播整張重送）
        match screen {
            Some(s) => {
                st.baseline.insert(id, s);
            }
            None => {
                st.baseline.remove(&id);
            }
        }
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
    // `/plain` 的提示只加在會跟 AI 對話的分頁上（見 `cmd::wants_plain_suffix`：
    // 加在 SSH 的密碼上會讓登入失敗而且看不出原因）
    let talks_to_ai = app
        .try_state::<Arc<crate::tabs::TabManager>>()
        .and_then(|t| t.kind_of(tab))
        .map(|k| matches!(k, crate::tabs::TabKind::Claude | crate::tabs::TabKind::Custom))
        .unwrap_or(false);
    if plain && cmd::wants_plain_suffix(text, talks_to_ai) {
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
    if auto {
        // 自動推播不回訊息，只清狀態
        let gone = app
            .try_state::<Arc<crate::tabs::TabManager>>()
            .map(|t| !t.contains(tab))
            .unwrap_or(false);
        if gone {
            state.lock().unwrap_or_else(|e| e.into_inner()).forget_tab(tab);
            return;
        }
    } else if tab_gone(app, api, chat_id, state, tab) {
        return;
    }
    // 等不到前端回畫面（逾時）＝`None`：**不可以**拿空字串當基準（BUG H4），
    // 否則下一次完成推播對空基準做 diff，整張快照重送一次
    let fetched = super::screen::recent_text(app, tab, SCREEN_TIMEOUT);
    let raw = fetched.clone().unwrap_or_default();
    let diff = if incremental {
        let st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.baseline.get(&tab).and_then(|b| tidy::diff_new(b, &raw))
    } else {
        None
    };
    if let Some(screen) = fetched {
        state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .baseline
            .insert(tab, screen);
    }

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
        // `/close 0` 不可以變成第 1 個（BUG H6：以前 saturating_sub）；也不能在 parse 時
        // 濾成 None——那會變成「關掉目前附著的」。0 就是清單上沒有的編號。
        Some(i) => (i as usize)
            .checked_sub(1)
            .and_then(|k| rows.get(k))
            .map(|(id, _, _)| *id),
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
    if tab_gone(app, api, chat_id, state, tab) {
        return;
    }
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
    if tab_gone(app, api, chat_id, state, tab) {
        return true; // 已經回了「分頁已關閉」，別再當文字送
    }
    let screen = super::screen::recent_text(app, tab, SCREEN_TIMEOUT).unwrap_or_default();
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

// ---------------------------------------------------------------- 開新連線

/// 要請前端開哪一種分頁。序列化成 JSON 給 `telegram-open` event，欄位名對齊
/// `session_create`（前端只是把它交給 `createSession`）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSpec {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telnet: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub com: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adb: Option<serde_json::Value>,
}

impl OpenSpec {
    fn of(kind: &str) -> Self {
        Self {
            kind: kind.to_string(),
            cwd: None,
            conn: None,
            ssh: None,
            telnet: None,
            com: None,
            adb: None,
        }
    }
}

/// 等前端回「開好的分頁 id」的信箱（只有一個遠端，所以單格就夠）。
type OpenBox = Mutex<Option<Sender<Option<u32>>>>;

fn open_box() -> &'static OpenBox {
    static M: OnceLock<OpenBox> = OnceLock::new();
    M.get_or_init(|| Mutex::new(None))
}

/// 前端開完分頁（或開失敗）之後呼叫。
pub fn opened(id: Option<u32>) {
    if let Some(tx) = open_box().lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = tx.send(id);
    }
}

/// 請前端開一個分頁，回傳 `(分頁 id, 標題)`。
///
/// ## 為什麼要繞前端
/// `session_create` 需要一個輸出用的 `Channel`，那是前端 invoke 才有的東西——Rust 這邊
/// 生不出來。所以照 `ssh-hostkey`／`macha-dialog` 的既有作法：emit 一個事件、等一個
/// 回覆 command（[`opened`]）。**隱含契約**：前端收到 `telegram-open` 一定要回
/// `telegram_opened`（成功給 id、失敗給 null），不回就等到逾時。
///
/// **不要在 tauri 的 IPC 執行緒上呼叫**（會等前端）。
fn open_tab(app: &AppHandle, spec: &OpenSpec) -> Option<(u32, String)> {
    let (tx, rx) = mpsc::channel();
    *open_box().lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    if app.emit("telegram-open", spec).is_err() {
        open_box().lock().unwrap_or_else(|e| e.into_inner()).take();
        return None;
    }
    // SSH／Telnet 的 `session_create` 在**連上之前**就回來（交握在背景），所以 8 秒很夠
    let id = match rx.recv_timeout(Duration::from_secs(8)) {
        Ok(Some(id)) => id,
        Ok(None) => return None,
        Err(_) => {
            open_box().lock().unwrap_or_else(|e| e.into_inner()).take();
            println!("[AwayTerminal] Telegram：開分頁等不到前端回覆（telegram_opened）");
            return None;
        }
    };
    let title = app
        .try_state::<Arc<crate::tabs::TabManager>>()
        .and_then(|t| t.title_of(id))
        .unwrap_or_default();
    Some((id, title))
}

/// 開完連線的共同收尾（舊版 `AttachAndReport`）：附著、SSH 提示回帳號、其餘推開場畫面。
fn attach_and_report(
    app: &AppHandle,
    api: &Api,
    chat_id: i64,
    state: &Arc<Mutex<State>>,
    id: u32,
    title: &str,
    ssh: bool,
) {
    {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.current = Some(id);
        st.baseline.remove(&id); // 新分頁：沒有基準，第一次推整個畫面
    }
    if ssh {
        // `login as:` 階段：提示直接回覆帳號（之後照畫面提示回覆密碼）
        send(api, chat_id, &crate::i18n::tf("tg.openedSsh", &[title]));
        return;
    }
    send(api, chat_id, &crate::i18n::tf("tg.opened", &[title]));
    let follow = state.lock().unwrap_or_else(|e| e.into_inner()).follow;
    if follow {
        // 推開場畫面（telnet／COM 的登入提示、shell 的提示列）
        std::thread::sleep(Duration::from_millis(1500));
        send_last(app, api, chat_id, state, 12, None, true, false);
    }
}

/// `/new` 的可開清單（舊版 `ListConnections`）。
///
/// 舊版的 SSH／Telnet 兩項來自 `LastHost`；**v2 沒有 `LastHost`**（TASK-009 決定用我的最愛
/// 取代主機歷史，`docs/MIGRATION.md` 有記），所以這裡改成列我的最愛裡的連線。
/// 其餘照舊版：PowerShell（桌面）、連接埠、ADB、自訂連線（不含隱藏的）。
fn conn_list(app: &AppHandle) -> Vec<(String, OpenSpec)> {
    let Some(store) = app.try_state::<Arc<crate::settings::SettingsStore>>() else {
        return Vec::new();
    };
    let s = store.get();
    let desk = dirs_desktop();
    let mut out: Vec<(String, OpenSpec)> = Vec::new();

    let mut ps = OpenSpec::of("shell");
    ps.cwd = desk.clone();
    // 本機 shell 的名字看作業系統：Windows 是 PowerShell，mac／Linux 是使用者的 `$SHELL` → 叫 Terminal
    let shell_label = if cfg!(windows) { "tg.connShell" } else { "tg.connTerminal" };
    out.push((crate::i18n::t(shell_label), ps));

    // 我的最愛（v2 的「主機紀錄」）
    for f in &s.favorites {
        let mut spec = OpenSpec::of(&f.kind);
        let label = match f.kind.as_str() {
            "ssh" => {
                spec.ssh = serde_json::to_value(&f.ssh).ok();
                format!("SSH {}", f.name)
            }
            "telnet" => {
                spec.telnet = serde_json::to_value(&f.telnet).ok();
                format!("Telnet {}", f.name)
            }
            "com" => {
                spec.com = serde_json::to_value(&f.com).ok();
                f.name.clone()
            }
            "conn" => {
                spec.kind = "conn".to_string();
                spec.conn = Some(f.conn_name.clone());
                // 需要選資料夾的自訂連線：遠端不能跳資料夾框 → 以桌面開啟（同舊版）
                spec.cwd = if f.dir.is_empty() { desk.clone() } else { Some(f.dir.clone()) };
                f.name.clone()
            }
            _ => {
                spec.kind = "shell".to_string();
                spec.cwd = if f.dir.is_empty() { desk.clone() } else { Some(f.dir.clone()) };
                f.name.clone()
            }
        };
        out.push((label, spec));
    }

    // 連接埠（設定裡上次用的那個）
    if !s.com_port.trim().is_empty() {
        let mut spec = OpenSpec::of("com");
        // 省略欄位時 `session_create` 會用設定裡上次的值（同舊版 ComDialog）
        spec.com = Some(serde_json::json!({ "port": s.com_port }));
        out.push((format!("{} {}", s.com_port, s.com_baud), spec));
    }

    out.push(("ADB".to_string(), OpenSpec::of("adb")));

    for c in s.custom_conns.iter().filter(|c| !c.hidden && !c.name.trim().is_empty()) {
        let mut spec = OpenSpec::of("conn");
        spec.conn = Some(c.name.clone());
        if c.pick_dir {
            spec.cwd = desk.clone(); // 遠端不能跳資料夾框
        }
        out.push((c.name.clone(), spec));
    }
    out
}

fn dirs_desktop() -> Option<String> {
    std::env::var("USERPROFILE")
        .ok()
        .map(|h| format!("{h}\\Desktop"))
        .filter(|p| std::path::Path::new(p).is_dir())
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/Desktop")))
        .filter(|p| std::path::Path::new(p).is_dir())
}

/// `/new [n]`：不帶編號＝列清單＋按鈕；帶編號＝開那一條並附著。
fn do_new(
    app: &AppHandle,
    api: &Api,
    chat_id: i64,
    state: &Arc<Mutex<State>>,
    n: Option<usize>,
) {
    let list = conn_list(app);
    if list.is_empty() {
        send(api, chat_id, &crate::i18n::t("tg.noConns"));
        return;
    }
    let Some(n) = n else {
        let mut body = format!("{}\n", crate::i18n::t("tg.pickConn"));
        let mut btns = Vec::new();
        for (i, (label, _)) in list.iter().enumerate() {
            body.push_str(&format!("{}: {label}\n", i + 1));
            btns.push((format!("{} {}", i + 1, trunc(label, 14)), format!("new:{}", i + 1)));
        }
        let rows: Vec<Vec<(String, String)>> = btns.chunks(2).map(|c| c.to_vec()).collect();
        let _ = api.send_message(chat_id, body.trim_end(), false, &rows);
        return;
    };
    if n < 1 || n > list.len() {
        send(
            api,
            chat_id,
            &crate::i18n::tf("tg.rangeIs", &[&list.len().to_string()]),
        );
        return;
    }
    let (_, spec) = &list[n - 1];
    match open_tab(app, spec) {
        Some((id, title)) => {
            attach_and_report(app, api, chat_id, state, id, &title, spec.kind == "ssh")
        }
        None => send(api, chat_id, &crate::i18n::t("tg.openFailed")),
    }
}

/// `/ssh [user@]主機[:埠]` 與 `/telnet [主機[:埠]]`。
///
/// 不帶參數時舊版用 `LastHost`；v2 沒有那個欄位 → 用**我的最愛裡第一條同型態的**
/// （TASK-009 用我的最愛取代主機歷史）。都沒有就回用法。
fn do_open_remote(
    app: &AppHandle,
    api: &Api,
    chat_id: i64,
    state: &Arc<Mutex<State>>,
    arg: &str,
    ssh: bool,
) {
    let (mut host, mut port) = cmd::parse_host_port(arg);
    let fav = first_favorite(app, if ssh { "ssh" } else { "telnet" });
    if host.trim().is_empty() {
        // 我的最愛那一條整條拿來用（帳號、金鑰、演算法都在裡面），不只是主機名
        if let Some(spec) = fav.clone() {
            match open_tab(app, &spec) {
                Some((id, title)) => {
                    attach_and_report(app, api, chat_id, state, id, &title, ssh);
                    return;
                }
                None => {
                    send(api, chat_id, &crate::i18n::t("tg.openFailed"));
                    return;
                }
            }
        }
        send(
            api,
            chat_id,
            &crate::i18n::t(if ssh { "tg.sshUsage" } else { "tg.telnetUsage" }),
        );
        return;
    }
    if port == 0 {
        port = if ssh { 22 } else { 23 };
    }
    // `user@主機` ＝帳號已知，不必在畫面上問（舊版 OpenSshUserAtHost）
    let mut user = String::new();
    if let Some((u, h)) = host.split_once('@') {
        user = u.to_string();
        host = h.to_string();
    }
    let mut spec = OpenSpec::of(if ssh { "ssh" } else { "telnet" });
    if ssh {
        spec.ssh = Some(serde_json::json!({ "host": host, "port": port, "user": user }));
    } else {
        spec.telnet = Some(serde_json::json!({ "host": host, "port": port }));
    }
    match open_tab(app, &spec) {
        // 帶了 `user@` 就不必提示回帳號，直接照畫面提示回密碼
        Some((id, title)) => {
            attach_and_report(app, api, chat_id, state, id, &title, ssh && user.is_empty())
        }
        None => send(api, chat_id, &crate::i18n::t("tg.openFailed")),
    }
}

/// 我的最愛裡第一條指定型態的，整條變成 `OpenSpec`。
fn first_favorite(app: &AppHandle, kind: &str) -> Option<OpenSpec> {
    let store = app.try_state::<Arc<crate::settings::SettingsStore>>()?;
    let s = store.get();
    let f = s.favorites.iter().find(|f| f.kind == kind)?;
    let mut spec = OpenSpec::of(kind);
    match kind {
        "ssh" => spec.ssh = serde_json::to_value(&f.ssh).ok(),
        "telnet" => spec.telnet = serde_json::to_value(&f.telnet).ok(),
        _ => return None,
    }
    Some(spec)
}

/// `/history [n]`：列最近可重開的連線；帶編號才開。
///
/// ⚠️ **刻意不做「回覆數字選取」**（舊版註解）：純數字要留給終端機輸入與選單應答。
///
/// 舊版列的是 `AppSettings.History`（最多 10 筆）。**v2 沒有 History 清單**
/// （TASK-009 用我的最愛取代，`docs/MIGRATION.md` 的跳過表有記），所以這裡列我的最愛。
/// 代理團隊／AI 聊天室一律不列——重開要跳設定視窗，從手機觸發沒人按。
fn do_history(
    app: &AppHandle,
    api: &Api,
    chat_id: i64,
    state: &Arc<Mutex<State>>,
    n: Option<usize>,
) {
    let list: Vec<(String, OpenSpec)> = conn_list(app)
        .into_iter()
        .filter(|(_, spec)| spec.kind != "agent")
        .take(10)
        .collect();
    if list.is_empty() {
        send(api, chat_id, &crate::i18n::t("tg.noHistory"));
        return;
    }
    let Some(n) = n else {
        let mut body = format!("{}\n", crate::i18n::t("tg.recentConns"));
        let mut btns = Vec::new();
        for (i, (label, _)) in list.iter().enumerate() {
            body.push_str(&format!("{}: {label}\n", i + 1));
            btns.push((format!("{} {}", i + 1, trunc(label, 14)), format!("hist:{}", i + 1)));
        }
        let rows: Vec<Vec<(String, String)>> = btns.chunks(2).map(|c| c.to_vec()).collect();
        let _ = api.send_message(chat_id, body.trim_end(), false, &rows);
        return;
    };
    if n < 1 || n > list.len() {
        send(
            api,
            chat_id,
            &crate::i18n::tf("tg.rangeIs", &[&list.len().to_string()]),
        );
        return;
    }
    let (_, spec) = &list[n - 1];
    match open_tab(app, spec) {
        Some((id, title)) => {
            attach_and_report(app, api, chat_id, state, id, &title, spec.kind == "ssh")
        }
        None => send(api, chat_id, &crate::i18n::t("tg.openFailed")),
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
