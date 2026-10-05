//! 投遞：信箱 → 收件人的終端機（搬移舊版 `MainWindow.MultiAgent.cs` 的投遞那半段）。
//!
//! 每 600 毫秒跑一次（舊版是狀態燈的 `DispatcherTimer`，同一個 tick）。一輪做四件事：
//!
//! 1. [`resend_enter_if_swallowed`]：上一行的 Enter 被吞掉就補送一個。
//! 2. 這格閒下來了（[`agent_ready`]）就先打「請先讀角色檔」（OpenCode／Gemini 的保底注入）。
//! 3. 佇列裡有信、沒暫停、角色已注入 → [`deliver_queued`]。
//! 4. [`check_team_idle`]：整組閒太久就請 Agent-x1 問大家狀況。
//!
//! ## 為什麼一定要等「閒置」才打字
//! 這些 CLI 是 TUI，沒有 hook、沒有 HTTP 入口，唯一的送信方式就是**把字打進它的輸入框**。
//! 它正在工作時輸入框可能不接受、或把字接在它自己正在組的訊息後面。所以投遞只在
//! 收件人閒置時發生——這也是為什麼「寄一封暫停信」攔不住正在工作的 agent
//! （執行期脈絡的 `### Delivery timing` 那段就是在講這件事）。

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::session::SessionManager;
use crate::tabs::{TabKind, TabManager};

use super::message::AgentMessage;
use super::team::{Slot, Team};
use super::TeamManager;

/// 輪詢週期（舊版的狀態燈 timer 就是 600ms，代理團隊搭在同一個 tick 上）。
const TICK: Duration = Duration::from_millis(600);

/// 打完一行到補送 Enter 之間的間隔（舊版 `RemoteEnterDelayMs`）。
pub const ENTER_DELAY_MS: u64 = 300;

/// 「停止任務」的三段時距（舊版 `StopClearDelayMs` / `StopPromptDelayMs`）。
pub const STOP_CLEAR_DELAY_MS: u64 = 1000;
pub const STOP_PROMPT_DELAY_MS: u64 = 1500;

/// 打完一行之後多久檢查「Enter 是不是被吞了」（舊版寫死 10 秒）。
const ENTER_CHECK_AFTER_SEC: u64 = 10;
/// 檢查時「這段時間內沒有新輸出」就算被吞（舊版 2 秒）。
const ENTER_SWALLOWED_IF_QUIET_SEC: u64 = 2;

/// 剛啟動的格在這段時間內就算 session 還查不到也當成「在跑」：`slot_started` 先把分頁 id
/// 記進格裡、`manager.insert` 才登記 session，中間這一瞬間收到的信不能被當成「收件人沒在跑」。
const STARTUP_GRACE_MS: u128 = 5_000;

/// 這一格的 CLI 還活著嗎：有分頁**而且** session 還在（G2）。
///
/// 只看 `tab.is_some()` 不夠——CLI 結束後分頁還開著（顯示「已結束」），信排進佇列卻永遠
/// 送不出去；重啟那一格時 `queue.clear()` 又把它們丟掉，寄件人永遠不知道。
fn slot_alive(s: &Slot, is_live: impl Fn(u32) -> bool, now: u128) -> bool {
    let Some(id) = s.tab else { return false };
    is_live(id) || now.saturating_sub(s.launched_ms) < STARTUP_GRACE_MS
}

/// 「收件人沒在跑，這封沒投遞」的通知內文（給寄件人的 INFO，英文照舊版——agent 讀的）。
fn undeliverable_body(m: &AgentMessage, roster: &str) -> String {
    format!(
        "Your message {} was not delivered: {} is not running in this team.\n\n\
Running agents: {roster}.\n\
Send it to one of them instead, or tell the user that this agent needs to be enabled.",
        m.rel_path(),
        m.to
    )
}

/// 在跑的格的名單（`Agent-12 Software Engineer (Codex), …`）。
fn running_roster(team: &Team, is_live: &dyn Fn(u32) -> bool, now: u128) -> String {
    team.slots
        .iter()
        .filter(|s| slot_alive(s, is_live, now))
        .map(|s| format!("{} {} ({})", s.agent_id(), s.role_title, s.backend_name()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 一格現在的時間戳（從分頁抓出來的），[`agent_ready`] 的輸入。
#[derive(Clone, Copy, Debug)]
pub struct Signals {
    pub now_ms: u64,
    /// 這格這次啟動的時間。
    pub launched_ms: u64,
    pub last_output_ms: u64,
    pub last_input_ms: u64,
    pub last_submit_ms: u64,
    pub last_delivered_ms: u64,
    /// 經 PowerShell 啟動（要多等一點）。
    pub via_ps: bool,
}

/// 這一格現在可以打字給它嗎。**逐條照舊版 `AgentReady`**：
///
/// | 條件 | 直接跑 exe | 經 PowerShell |
/// |---|---|---|
/// | 啟動後至少過了 | 5 秒 | 10 秒 |
/// | CLI 啟動後**有畫過東西** | 必須 | 必須 |
/// | 最後一次輸出之後靜止 | 2000 ms | 3000 ms |
/// | 距上次投遞 | ≥3 秒 | ≥3 秒 |
/// | 距使用者最後一次打字 | ≥3 秒 | ≥3 秒 |
/// | 距最後一次送出 | ≥3 秒 | ≥3 秒 |
///
/// 「經 PowerShell 的多等一點」的理由（舊版註解）：PowerShell 提示行出來之後 node 還要
/// 載入 CLI，那段安靜期打的字會被吃掉。
///
/// 舊版還有一條 `tab.PendingCommand == null`（先開互動 PowerShell、尺寸就緒再把指令打進去
/// 的那條路）。我們的 PTY 一開始就是前端回報的真實尺寸、沒有那個延後打字的機制，
/// 所以沒有對應的欄位（見 `commands.rs` 裡 `via_powershell` 的註解）。
pub fn agent_ready(s: &Signals) -> bool {
    let (warmup_sec, quiet_ms) = if s.via_ps { (10, 3000) } else { (5, 2000) };
    let since = |t: u64| s.now_ms.saturating_sub(t);
    s.launched_ms > 0
        && since(s.launched_ms) >= warmup_sec * 1000
        // CLI 啟動後有畫過東西（開場畫完了）
        && s.last_output_ms > s.launched_ms
        && since(s.last_output_ms) >= quiet_ms
        && since(s.last_delivered_ms) >= 3000
        && since(s.last_input_ms) >= 3000
        // 遠端（Telegram）剛送出訊息給 Agent-x1：別在 Enter 送達前把信打進去
        && since(s.last_submit_ms) >= 3000
}

/// Enter 被吞了嗎（舊版 `ResendEnterIfSwallowed` 的判斷部分）。
///
/// 打完那一行 10 秒後，對方除了打字回顯之外沒有再輸出＝Enter 可能沒送出去
/// （claude 把整塊當貼上、CR 變換行，舊版 1.1.10 實測過）。
pub fn enter_swallowed(now_ms: u64, last_delivered_ms: u64, last_output_ms: u64) -> Option<bool> {
    if last_delivered_ms == 0 || now_ms.saturating_sub(last_delivered_ms) < ENTER_CHECK_AFTER_SEC * 1000 {
        return None; // 還沒到檢查時間
    }
    Some(
        last_output_ms.saturating_sub(last_delivered_ms)
            < ENTER_SWALLOWED_IF_QUIET_SEC * 1000,
    )
}

/// 投遞時要打進終端機的那一行（舊版 `DeliverQueued` 組字串的部分）。
///
/// - 一封、寄件人是 AwayTerminal → `ma.deliverInfo`
/// - 一封、寄件人是 agent → `ma.deliverOne`
/// - 多封 → `ma.deliverMany`（分隔符：英文 `, `、其餘語言 `、`）
pub fn delivery_line(batch: &[AgentMessage], first_seq: u32) -> String {
    if batch.len() == 1 {
        let m = &batch[0];
        let seq = first_seq.to_string();
        if m.from.eq_ignore_ascii_case("AwayTerminal") {
            return crate::i18n::tf("ma.deliverInfo", &[&seq, &m.rel_path()]);
        }
        let task = if m.task.trim().is_empty() { "—" } else { m.task.trim() };
        return crate::i18n::tf("ma.deliverOne", &[&seq, &m.from, task, &m.kind, &m.rel_path()]);
    }
    // 舊版：`Loc.Lang == "en" ? ", " : "、"`
    let sep = if crate::i18n::is_en() { ", " } else { "、" };
    let list = batch
        .iter()
        .map(|m| m.rel_path())
        .collect::<Vec<_>>()
        .join(sep);
    crate::i18n::tf("ma.deliverMany", &[&batch.len().to_string(), &list])
}

/// 這一輪可以一次送幾封（舊版 `take`：剩下的額度裡能拿多少就拿多少）。
pub fn batch_size(queued: usize, max_messages: u32, message_count: u32) -> usize {
    if max_messages == 0 {
        return queued;
    }
    let left = max_messages.saturating_sub(message_count) as usize;
    queued.min(left)
}

// ---------------------------------------------------------------- 打字進終端機

/// 送一行文字進分頁、隔 [`ENTER_DELAY_MS`] 再**單獨**送 Enter（舊版 `SendTextThenEnter`）。
///
/// ⚠️ 文字＋CR 一次寫入時 claude 會當成貼上、CR 變成軟換行而**不送出**（舊版 1.1.10 實測），
/// 所以一律分兩次。claude 分頁的文字走前端的 `v` 協定（＝和使用者按 Ctrl+V 完全一樣，
/// bracketed paste、ESC+CR 軟換行都照原樣），其餘直接寫進 PTY。
pub fn send_text_then_enter(
    app: &AppHandle,
    tabs: &Arc<TabManager>,
    sessions: &SessionManager,
    id: u32,
    text: &str,
    enter: bool,
) {
    if sessions.get(id).is_none() {
        return;
    }
    let claude_paste = tabs.kind_of(id) == Some(TabKind::Claude);
    if claude_paste {
        crate::host::emit_host(
            app,
            format!("v{id}\x1f{}", crate::b64::encode(text.as_bytes())),
        );
    } else if !text.is_empty() {
        if let Some(s) = sessions.get(id) {
            s.write(text.as_bytes());
        }
    }
    if !enter {
        return;
    }
    tabs.mark_submit(id);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(ENTER_DELAY_MS)).await;
        if let Some(m) = app.try_state::<SessionManager>() {
            if let Some(s) = m.get(id) {
                s.write(b"\r");
            }
        }
    });
}

/// 「停止任務」：整組每一格送 Esc 中斷 → 1 秒後 Ctrl+U 清輸入框 → 1.5 秒時打停止句＋Enter。
///
/// 舊版註解：信只在收件人閒置時投遞，PM 寄暫停信攔不住正在工作的 agent，所以要有直接中斷
/// 的入口。**Ctrl+U 是 probe 實測補的**——claude 還在思考、沒輸出就被 Esc 中斷時，會把剛才
/// 那則訊息放回輸入框，不清掉的話停止句會接在後面、合成一則重新送出（＝它繼續做原本的事）。
/// 閒置的格收到 Esc 也無害（claude／codex 閒置時實測照樣收下停止句並回報狀態）。
pub fn stop_team(app: &AppHandle, teams: &Arc<TeamManager>, key: &str) -> Vec<String> {
    let (targets, ids) = {
        let mut list = teams.lock();
        let Some(team) = list.iter_mut().find(|t| t.key == key) else {
            return Vec::new();
        };
        let sessions = app.state::<SessionManager>();
        let now = crate::tabs::now_ms();
        let mut targets = Vec::new();
        let mut ids = Vec::new();
        for s in team.slots.iter_mut() {
            let Some(tab) = s.tab else { continue };
            if sessions.get(tab).is_none() {
                continue;
            }
            if let Some(sess) = sessions.get(tab) {
                sess.write(b"\x1b");
            }
            // 停止流程跑完前不投遞佇列裡的信（`agent_ready` 要距上次打字 ≥3 秒）
            mark_typed(s, now);
            targets.push(tab);
            ids.push(s.agent_id());
        }
        (targets, ids)
    };
    if targets.is_empty() {
        return Vec::new();
    }
    println!(
        "[AwayTerminal] 代理團隊 {key}：停止任務 → {}",
        ids.join(",")
    );
    let app2 = app.clone();
    let teams2 = teams.clone();
    let key2 = key.to_string();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(STOP_CLEAR_DELAY_MS)).await;
        if let Some(m) = app2.try_state::<SessionManager>() {
            for id in &targets {
                if let Some(s) = m.get(*id) {
                    s.write(b"\x15"); // Ctrl+U
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(
            STOP_PROMPT_DELAY_MS - STOP_CLEAR_DELAY_MS,
        ))
        .await;
        let prompt = crate::i18n::t("ma.stopPrompt");
        let Some(tabs) = app2.try_state::<Arc<TabManager>>() else {
            return;
        };
        let Some(sessions) = app2.try_state::<SessionManager>() else {
            return;
        };
        for id in &targets {
            // 這 1.5 秒內那格被重開或結束
            if sessions.get(*id).is_none() {
                continue;
            }
            send_text_then_enter(&app2, &tabs, &sessions, *id, &prompt, true);
        }
        let now = crate::tabs::now_ms();
        let mut list = teams2.lock();
        if let Some(team) = list.iter_mut().find(|t| t.key == key2) {
            for s in team.slots.iter_mut() {
                if s.tab.is_some_and(|t| targets.contains(&t)) {
                    mark_typed(s, now);
                }
            }
        }
    });
    ids
}

/// 打完字之後記一筆（舊版 `MarkTyped`）。
pub fn mark_typed(s: &mut Slot, now_ms: u64) {
    s.last_delivered_ms = now_ms as u128;
    s.delivery_checked = false;
}

// ---------------------------------------------------------------- 每 600ms 的 tick

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(TICK);
        tick(&app);
    });
}

fn tick(app: &AppHandle) {
    let Some(teams) = app.try_state::<Arc<TeamManager>>() else {
        return;
    };
    if teams.is_empty() {
        return;
    }
    let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
        return;
    };
    let Some(sessions) = app.try_state::<SessionManager>() else {
        return;
    };
    let now = crate::tabs::now_ms();

    // 1. 信箱：有新信就排進收件人的佇列（`poll` 只回穩定超過 1.5 秒的檔）。
    //    聊天室沒有 `bus`（它不走信箱，見 `agent/chat.rs`），所以這一圈自動跳過它。
    let keys: Vec<String> = teams.lock().iter().map(|t| t.key.clone()).collect();
    for key in &keys {
        let (bus, new_msgs) = {
            let list = teams.lock();
            match list.iter().find(|t| &t.key == key).and_then(|t| t.bus.clone()) {
                Some(bus) => {
                    let msgs = bus.poll();
                    (bus, msgs)
                }
                None => continue,
            }
        };
        for m in new_msgs {
            on_message(app, &teams, key, &bus, m);
        }
    }

    // 2. 每一格：補 Enter → 角色注入 → 投遞
    let mut to_type: Vec<(u32, String, bool)> = Vec::new(); // (分頁 id, 文字, 要不要 Enter)
    let mut enter_only: Vec<u32> = Vec::new();
    // 收件人已結束、不投遞的信要回給寄件人的 INFO：(信箱, 寄件人, task, 內文)。放掉鎖之後再寫。
    let mut notices: Vec<(super::bus::SharedBus, String, String, String)> = Vec::new();
    {
        let mut list = teams.lock();
        for team in list.iter_mut() {
            // 分頁已經不在了（被關掉）＝那一格結束
            for s in team.slots.iter_mut() {
                if let Some(id) = s.tab {
                    if !tabs.contains(id) {
                        s.tab = None;
                        s.queue.clear();
                    }
                }
            }
            // 收件人 CLI 已結束但分頁還開著（G2）：排著的信永遠送不出去，重啟那一格時
            // `queue.clear()` 又會丟掉 → 現在就記「已投遞」並回 INFO 給寄件人（和收信時
            // 收件人沒在跑同一條路）
            if !team.is_chat() {
                let is_live = |id: u32| sessions.get(id).is_some();
                let now128 = now as u128;
                let mut dropped: Vec<AgentMessage> = Vec::new();
                for s in team.slots.iter_mut() {
                    if s.tab.is_none() || s.queue.is_empty() || slot_alive(s, is_live, now128) {
                        continue;
                    }
                    println!(
                        "[AwayTerminal] 代理團隊 {}：{} 已結束，{} 封排隊中的信不投遞",
                        team.number,
                        s.agent_id(),
                        s.queue.len()
                    );
                    dropped.extend(s.queue.drain(..));
                }
                if let (false, Some(bus)) = (dropped.is_empty(), team.bus.clone()) {
                    let roster = running_roster(team, &is_live, now128);
                    for m in dropped {
                        // 廣播信：還有別人排著就等最後一個人拿到再記（同投遞那段的規則）
                        let still_queued = m.is_broadcast()
                            && team
                                .slots
                                .iter()
                                .any(|x| x.queue.iter().any(|q| q.file_name == m.file_name));
                        if !still_queued {
                            bus.mark_delivered(&m.file_name);
                        }
                        let sender_alive = team
                            .slot_by_id(&m.from)
                            .is_some_and(|x| slot_alive(x, is_live, now128));
                        if !m.is_broadcast() && sender_alive {
                            notices.push((bus.clone(), m.from.clone(), m.task.clone(), undeliverable_body(&m, &roster)));
                        }
                    }
                }
            }
            // ---- AI 聊天室：不投遞信，改由 AwayTerminal 主持輪流發言 ----
            //（角色注入那一段照樣要跑：OpenCode／Gemini 的角色是第一次閒置時打進去的）
            if team.is_chat() {
                for i in 0..team.slots.len() {
                    let Some(id) = team.slots[i].tab else { continue };
                    let Some(sig) = tabs.agent_signals(id) else { continue };
                    if sessions.get(id).is_none() {
                        continue;
                    }
                    let s = &mut team.slots[i];
                    // 發言提示那一行的 Enter 被吞＝等 5 分鐘才跳過，補送比較划算（舊版註解）
                    if !s.delivery_checked {
                        if let Some(swallowed) =
                            enter_swallowed(now, s.last_delivered_ms as u64, sig.last_output)
                        {
                            s.delivery_checked = true;
                            if swallowed {
                                enter_only.push(id);
                                println!("[AwayTerminal] 聊天室 {}：補送 Enter", s.agent_id());
                            }
                        }
                    }
                    let is_ready = agent_ready(&Signals {
                        now_ms: now,
                        launched_ms: s.launched_ms as u64,
                        last_output_ms: sig.last_output,
                        last_input_ms: sig.last_input,
                        last_submit_ms: sig.last_submit,
                        last_delivered_ms: s.last_delivered_ms as u64,
                        via_ps: s.via_ps,
                    });
                    if let Some(msg) = s.pending_first_message.clone() {
                        if is_ready {
                            s.pending_first_message = None;
                            s.role_injected = true;
                            mark_typed(s, now);
                            s.delivery_checked = true;
                            let who = s.agent_id();
                            to_type.push((id, msg, true));
                            println!("[AwayTerminal] 聊天室 {who}：角色以打字注入");
                        }
                    }
                }
                // 還有人的角色還沒注入就先不開始輪流（它會把提示打成 CLI 的第一句）
                let injecting = team
                    .slots
                    .iter()
                    .any(|s| s.tab.is_some() && s.pending_first_message.is_some());
                if !injecting {
                    let alive = |id: u32| sessions.get(id).is_some();
                    let ready = |s: &super::team::Slot| {
                        let Some(id) = s.tab else { return false };
                        let Some(sig) = tabs.agent_signals(id) else {
                            return false;
                        };
                        agent_ready(&Signals {
                            now_ms: now,
                            launched_ms: s.launched_ms as u64,
                            last_output_ms: sig.last_output,
                            last_input_ms: sig.last_input,
                            last_submit_ms: sig.last_submit,
                            last_delivered_ms: s.last_delivered_ms as u64,
                            via_ps: s.via_ps,
                        })
                    };
                    if let super::chat::ChatAction::Type { tab, text } =
                        super::chat::tick(team, now as u128, &alive, &ready)
                    {
                        to_type.push((tab, text, true));
                    }
                }
                continue;
            }

            let (max, count, paused) = (team.max_messages, team.message_count, team.paused);
            let mut delivered_files: Vec<(String, bool)> = Vec::new();
            let mut new_count = count;
            let mut seq = team.delivery_seq;
            let mut hit_limit = false;
            let slot_count = team.slots.len();
            for i in 0..slot_count {
                let Some(id) = team.slots[i].tab else { continue };
                let Some(sig) = tabs.agent_signals(id) else { continue };
                if sessions.get(id).is_none() {
                    continue;
                }
                let s = &mut team.slots[i];

                // (a) Enter 補送
                if !s.delivery_checked {
                    if let Some(swallowed) =
                        enter_swallowed(now, s.last_delivered_ms as u64, sig.last_output)
                    {
                        s.delivery_checked = true;
                        if swallowed {
                            enter_only.push(id);
                            println!("[AwayTerminal] 代理團隊 {}：補送 Enter", s.agent_id());
                        }
                    }
                }

                let ready = agent_ready(&Signals {
                    now_ms: now,
                    launched_ms: s.launched_ms as u64,
                    last_output_ms: sig.last_output,
                    last_input_ms: sig.last_input,
                    last_submit_ms: sig.last_submit,
                    last_delivered_ms: s.last_delivered_ms as u64,
                    via_ps: s.via_ps,
                });
                if !ready {
                    continue;
                }

                // (b) OpenCode／Gemini：角色靠第一句打進去（保底注入）
                if let Some(msg) = s.pending_first_message.take() {
                    to_type.push((id, msg, true));
                    s.role_injected = true;
                    mark_typed(s, now);
                    // 預期它只回一句 READY、輸出很短——不做「Enter 沒送出」補送（舊版實測會誤判）
                    s.delivery_checked = true;
                    println!("[AwayTerminal] 代理團隊 {}：角色以打字注入", s.agent_id());
                    continue;
                }

                // (c) 投遞
                if s.queue.is_empty() || paused || !s.role_injected || hit_limit {
                    continue;
                }
                let take = batch_size(s.queue.len(), max, new_count);
                if take == 0 {
                    hit_limit = true;
                    continue;
                }
                let batch: Vec<AgentMessage> = s.queue.drain(..take).collect();
                seq += batch.len() as u32;
                let first_seq = seq - batch.len() as u32 + 1;
                let line = delivery_line(&batch, first_seq);
                to_type.push((id, line, true));
                mark_typed(s, now);
                new_count += batch.len() as u32;
                let who = s.agent_id();
                let names: Vec<String> = batch.iter().map(|m| m.file_name.clone()).collect();
                for m in &batch {
                    delivered_files.push((m.file_name.clone(), m.is_broadcast()));
                }
                println!(
                    "[AwayTerminal] 代理團隊投遞 #{first_seq} → {who}（{}）計數 {new_count}/{}",
                    names.join(", "),
                    if max == 0 { "∞".to_string() } else { max.to_string() }
                );
                if max > 0 && new_count >= max {
                    hit_limit = true;
                }
            }
            team.delivery_seq = seq;
            team.message_count = new_count;
            // `to: all` 的信同一封排在每個收件人的佇列裡：要等**最後一個**收件人也拿到才記
            // 「已投遞」，否則中途關程式（`.delivered` 已寫）重開後還沒收到的人永遠收不到。
            if let Some(bus) = &team.bus {
                for (name, broadcast) in delivered_files {
                    let still_queued = broadcast
                        && team
                            .slots
                            .iter()
                            .any(|x| x.queue.iter().any(|m| m.file_name == name));
                    if !still_queued {
                        bus.mark_delivered(&name);
                    }
                }
            }
            if max > 0 && team.message_count >= max && !team.paused {
                team.paused = true;
                team.paused_by_limit = true;
                println!(
                    "[AwayTerminal] 代理團隊 {} 投遞到上限（{}）→ 暫停；待投遞 {}",
                    team.number,
                    team.message_count,
                    team.pending_count()
                );
            }

            // 3. 閒置檢查
            if let Some((id, prompt)) = check_team_idle(team, now, &tabs, &sessions) {
                to_type.push((id, prompt, true));
            }
        }
    }

    for (bus, sender, task, body) in notices {
        bus.write_message("AwayTerminal", &sender, "INFO", &task, &body);
    }

    // 4. 真的打字（鎖已經放掉——`send_text_then_enter` 會 emit、也會排 300ms 的 Enter）
    for id in enter_only {
        if let Some(s) = sessions.get(id) {
            s.write(b"\r");
        }
    }
    for (id, text, enter) in to_type {
        send_text_then_enter(app, &tabs, &sessions, id, &text, enter);
    }

    super::post_state(app, &teams);
}

/// 閒置計時把「連續輸出不到這麼久」當成畫面重畫，不當成有人在工作（見 [`idle_tracking`]）。
const IDLE_WORK_BURST_MS: u128 = 10_000;
/// 兩次輸出之間隔這麼久以內算同一段連續輸出。
const IDLE_BURST_GAP_MS: u128 = 5_000;

/// 這一輪閒置計時該怎麼走（[`idle_tracking`] 的結果）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdleStep {
    /// 有人真的在工作 → 「從什麼時候開始閒置」歸零。
    Reset,
    /// 有人正在輸出，但還短得像重畫 → 計時照走，這一輪先不插話。
    Hold,
    /// 整組安靜 → 計時照走，時間到了就可以問。
    Quiet,
}

/// 分辨「真的在工作」和「閒置中偶爾重畫一下畫面」（2.0.9）。
///
/// 舊版只要任何一格被判成忙碌就把閒置計時歸零。但閒置的 CLI 也會自己輸出——Codex 背景
/// 更新外掛目錄／模型清單時會重畫畫面，間隔比 30 分鐘短，計時就永遠湊不滿（2026-10-05
/// 3kingdoms：OpenCode 被 API 錯誤中斷後整組停了一個多小時，提醒一次都沒出現）。
///
/// 現在只有兩種情況算工作：
/// - 有人**送出了一行**（使用者按 Enter、投遞、遠端指令）；
/// - 連續輸出滿 [`IDLE_WORK_BURST_MS`]（中間安靜不到 [`IDLE_BURST_GAP_MS`] 算同一段）。
fn idle_tracking(team: &mut Team, now: u128, any_busy: bool, submitted: bool) -> IdleStep {
    // 上一段已經安靜超過間隔＝結束了（下一次輸出是新的一段，不能和它加起來）
    if team.busy_since_ms != 0 && now.saturating_sub(team.last_busy_ms) > IDLE_BURST_GAP_MS {
        team.busy_since_ms = 0;
    }
    if any_busy {
        if team.busy_since_ms == 0 {
            team.busy_since_ms = now;
        }
        team.last_busy_ms = now;
    }
    let working = team.busy_since_ms != 0
        && team.last_busy_ms.saturating_sub(team.busy_since_ms) >= IDLE_WORK_BURST_MS;
    if submitted || working {
        IdleStep::Reset
    } else if any_busy {
        IdleStep::Hold
    } else {
        IdleStep::Quiet
    }
}

/// 整組閒置太久 → 請 Agent-x1 問大家狀況（舊版 `CheckTeamIdle`，使用者要求 2026-09-16）。
///
/// 條件：≥2 格在跑、沒人在工作（[`idle_tracking`]）、沒有信在排隊、沒有待注入的角色、沒暫停，
/// 連續閒置 `idle_check_minutes` 分鐘；而且格 1 自己也 [`agent_ready`]（使用者剛在那格打字就等下一輪）。
/// 送出後重新計時；**不算進投遞則數**（這是 AwayTerminal 自己問的，不是 agent 之間的信）。
fn check_team_idle(
    team: &mut Team,
    now: u64,
    tabs: &Arc<TabManager>,
    sessions: &SessionManager,
) -> Option<(u32, String)> {
    if team.idle_check_minutes == 0 || team.paused {
        team.all_idle_since_ms = 0;
        team.busy_since_ms = 0;
        return None;
    }
    let live: Vec<usize> = (0..team.slots.len())
        .filter(|&i| {
            team.slots[i]
                .tab
                .is_some_and(|id| sessions.get(id).is_some())
        })
        .collect();
    let lead = live.iter().copied().find(|&i| team.slots[i].index == 1);
    let nothing_pending = lead.is_some()
        && live.len() >= 2
        && live.iter().all(|&i| {
            let s = &team.slots[i];
            s.queue.is_empty() && s.pending_first_message.is_none()
        });
    if !nothing_pending {
        team.all_idle_since_ms = 0;
        team.busy_since_ms = 0;
        return None;
    }
    let since = team.all_idle_since_ms;
    let mut any_busy = false;
    let mut submitted = false;
    for &i in &live {
        let s = &team.slots[i];
        let Some(g) = s.tab.and_then(|id| tabs.agent_signals(id)) else {
            continue;
        };
        any_busy |= g.busy;
        submitted |= since != 0 && (g.last_submit as u128 > since || s.last_delivered_ms > since);
    }
    match idle_tracking(team, now as u128, any_busy, submitted) {
        IdleStep::Reset => {
            team.all_idle_since_ms = 0;
            return None;
        }
        IdleStep::Hold => return None,
        IdleStep::Quiet => {}
    }
    if team.all_idle_since_ms == 0 {
        team.all_idle_since_ms = now as u128;
        return None;
    }
    let idle_ms = (now as u128).saturating_sub(team.all_idle_since_ms);
    if idle_ms < team.idle_check_minutes as u128 * 60_000 {
        return None;
    }
    let i = lead?;
    let id = team.slots[i].tab?;
    let sig = tabs.agent_signals(id)?;
    let s = &mut team.slots[i];
    if !agent_ready(&Signals {
        now_ms: now,
        launched_ms: s.launched_ms as u64,
        last_output_ms: sig.last_output,
        last_input_ms: sig.last_input,
        last_submit_ms: sig.last_submit,
        last_delivered_ms: s.last_delivered_ms as u64,
        via_ps: s.via_ps,
    }) {
        return None;
    }
    mark_typed(s, now);
    let who = s.agent_id();
    team.all_idle_since_ms = now as u128;
    println!(
        "[AwayTerminal] 代理團隊 {}：閒置 {} 分鐘 → 請 {who} 問大家狀況",
        team.number, team.idle_check_minutes
    );
    Some((id, crate::i18n::t("ma.idleCheckPrompt")))
}

/// 信箱發現一封新信（舊版 `OnAgentMessage`）。
///
/// 只收「收件人是本組」的（`all`＝寄件人是本組）；同一個資料夾的其他組各自處理自己的。
fn on_message(
    app: &AppHandle,
    teams: &Arc<TeamManager>,
    key: &str,
    bus: &super::bus::SharedBus,
    m: AgentMessage,
) {
    if bus.is_delivered(&m.file_name) {
        return;
    }
    let mut undeliverable: Option<(String, String)> = None; // (寄件人, 名單)
    // 「在跑」＝有分頁而且 CLI 還活著（G2）；拿不到 SessionManager（不會發生）就退回只看分頁
    let sessions = app.try_state::<SessionManager>();
    let is_live = |id: u32| sessions.as_ref().is_none_or(|m| m.get(id).is_some());
    let now = crate::tabs::now_ms() as u128;
    {
        let mut list = teams.lock();
        let Some(team) = list.iter_mut().find(|t| t.key == key) else {
            return;
        };
        let mine = if m.is_broadcast() { &m.from } else { &m.to };
        if !team.owns_id(mine) {
            println!(
                "[AwayTerminal] 代理團隊 {}：略過 {}（from={} to={} 不是本組）",
                team.number, m.file_name, m.from, m.to
            );
            return;
        }
        println!(
            "[AwayTerminal] 代理團隊 {}：收到 {} from={} to={} type={} task={}{}",
            team.number,
            m.file_name,
            m.from,
            m.to,
            m.kind,
            if m.task.is_empty() { "—" } else { &m.task },
            if m.header_warning { "（front matter 有問題，照樣投遞）" } else { "" }
        );

        if m.is_broadcast() {
            let targets: Vec<usize> = (0..team.slots.len())
                .filter(|&i| {
                    slot_alive(&team.slots[i], is_live, now)
                        && !team.slots[i].agent_id().eq_ignore_ascii_case(&m.from)
                })
                .collect();
            if targets.is_empty() {
                bus.mark_delivered(&m.file_name);
            }
            for i in targets {
                team.slots[i].queue.push_back(m.clone());
            }
        } else {
            match team.slot_by_id_mut(&m.to) {
                Some(slot) if slot.enabled && slot_alive(slot, is_live, now) => {
                    slot.queue.push_back(m.clone());
                }
                _ => {
                    bus.mark_delivered(&m.file_name);
                    println!(
                        "[AwayTerminal] 代理團隊 {}：{} 沒有在跑，{} 不投遞",
                        team.number, m.to, m.file_name
                    );
                    // 寄件人是本組 agent 才回通知（AwayTerminal 自己的信不回，免得打轉）
                    if team
                        .slot_by_id(&m.from)
                        .is_some_and(|s| slot_alive(s, is_live, now))
                    {
                        undeliverable = Some((m.from.clone(), running_roster(team, &is_live, now)));
                    }
                }
            }
        }
    }
    if let Some((sender, roster)) = undeliverable {
        bus.write_message("AwayTerminal", &sender, "INFO", &m.task, &undeliverable_body(&m, &roster));
    }
    super::post_state(app, teams);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(via_ps: bool) -> Signals {
        // 一個「什麼條件都滿足」的基準：啟動 60 秒前、5 秒前有輸出、其餘都很久以前
        Signals {
            now_ms: 100_000,
            launched_ms: 40_000,
            last_output_ms: 95_000,
            last_input_ms: 50_000,
            last_submit_ms: 50_000,
            last_delivered_ms: 50_000,
            via_ps,
        }
    }

    /// 基準狀態＝可以打字。
    #[test]
    fn ready_when_everything_settled() {
        assert!(agent_ready(&sig(false)));
        assert!(agent_ready(&sig(true)));
    }

    /// 剛啟動還不能打（直接跑 5 秒、經 PowerShell 10 秒）。
    #[test]
    fn waits_for_the_cli_to_start() {
        let mut s = sig(false);
        s.launched_ms = s.now_ms - 4_999;
        s.last_output_ms = s.now_ms - 2_500;
        assert!(!agent_ready(&s), "不到 5 秒不能打");
        s.launched_ms = s.now_ms - 5_001;
        assert!(agent_ready(&s));

        let mut p = sig(true);
        p.launched_ms = p.now_ms - 9_000;
        p.last_output_ms = p.now_ms - 3_500;
        assert!(!agent_ready(&p), "經 PowerShell 要等 10 秒");
        p.launched_ms = p.now_ms - 10_001;
        assert!(agent_ready(&p));
    }

    /// CLI 啟動後還沒畫過東西＝還在載入，不能打。
    #[test]
    fn needs_output_after_launch() {
        let mut s = sig(false);
        s.last_output_ms = s.launched_ms - 1; // 上一個 session 的輸出
        assert!(!agent_ready(&s));
        s.last_output_ms = s.now_ms - 1_000;
        assert!(!agent_ready(&s), "剛畫完還沒靜止 2 秒");
        s.last_output_ms = s.now_ms - 2_000;
        assert!(agent_ready(&s));
    }

    /// 靜止門檻：直接跑 2000ms、經 PowerShell 3000ms。
    #[test]
    fn needs_a_quiet_screen() {
        let mut s = sig(false);
        s.last_output_ms = s.now_ms - 1_999;
        assert!(!agent_ready(&s));
        s.last_output_ms = s.now_ms - 2_000;
        assert!(agent_ready(&s));

        let mut p = sig(true);
        p.last_output_ms = p.now_ms - 2_999;
        assert!(!agent_ready(&p));
        p.last_output_ms = p.now_ms - 3_000;
        assert!(agent_ready(&p));
    }

    /// 三個「3 秒」的閘門：剛投遞過、使用者剛打字、剛送出過。
    #[test]
    fn three_second_gates() {
        for field in 0..3 {
            let mut s = sig(false);
            let v = s.now_ms - 2_999;
            match field {
                0 => s.last_delivered_ms = v,
                1 => s.last_input_ms = v,
                _ => s.last_submit_ms = v,
            }
            assert!(!agent_ready(&s), "第 {field} 個閘門沒攔住");
            let v = s.now_ms - 3_000;
            match field {
                0 => s.last_delivered_ms = v,
                1 => s.last_input_ms = v,
                _ => s.last_submit_ms = v,
            }
            assert!(agent_ready(&s), "第 {field} 個閘門過頭了");
        }
    }

    /// 「在跑」＝有分頁而且 session 還在；剛啟動 5 秒內 session 還沒登記也算（G2）。
    #[test]
    fn slot_alive_needs_a_live_session() {
        let mut s = Slot::new(1, 2);
        let live = |id: u32| id == 7;
        assert!(!slot_alive(&s, live, 100_000), "沒有分頁");
        s.tab = Some(7);
        s.launched_ms = 10_000;
        assert!(slot_alive(&s, live, 100_000));
        s.tab = Some(8); // 分頁還開著，但 CLI 已結束
        assert!(!slot_alive(&s, live, 100_000), "CLI 結束了就不算在跑");
        s.launched_ms = 98_000;
        assert!(slot_alive(&s, live, 100_000), "剛啟動、session 還沒登記");
    }

    /// 閒置計時：閒置中偶爾重畫一下畫面不算工作，連續輸出或送出一行才算（2.0.9）。
    #[test]
    fn idle_timer_ignores_short_repaints() {
        let mut t = Team::new("k", 1, "d");
        // 重畫：忙 1.8 秒就停 → 這一輪不插話，但計時不歸零
        assert_eq!(idle_tracking(&mut t, 100_000, true, false), IdleStep::Hold);
        assert_eq!(idle_tracking(&mut t, 101_800, true, false), IdleStep::Hold);
        assert_eq!(idle_tracking(&mut t, 102_400, false, false), IdleStep::Quiet);
        // 隔了很久再重畫一次＝新的一段，不會和上一段加起來
        assert_eq!(idle_tracking(&mut t, 1_900_000, true, false), IdleStep::Hold);
        assert_eq!(idle_tracking(&mut t, 1_910_000, false, false), IdleStep::Quiet);

        // 真的在工作：連續輸出滿 10 秒（中間停 3 秒算同一段）→ 歸零
        let mut w = Team::new("k", 1, "d");
        assert_eq!(idle_tracking(&mut w, 100_000, true, false), IdleStep::Hold);
        assert_eq!(idle_tracking(&mut w, 104_000, true, false), IdleStep::Hold);
        assert_eq!(idle_tracking(&mut w, 107_000, false, false), IdleStep::Quiet);
        assert_eq!(idle_tracking(&mut w, 108_000, true, false), IdleStep::Hold);
        assert_eq!(idle_tracking(&mut w, 110_000, true, false), IdleStep::Reset);
        // 停下來之後，那一段結束前（5 秒內）仍算工作，之後才回到安靜
        assert_eq!(idle_tracking(&mut w, 113_000, false, false), IdleStep::Reset);
        assert_eq!(idle_tracking(&mut w, 116_000, false, false), IdleStep::Quiet);

        // 有人送出了一行（Enter／投遞）＝一定算工作，就算畫面沒在動
        let mut s = Team::new("k", 1, "d");
        assert_eq!(idle_tracking(&mut s, 100_000, false, true), IdleStep::Reset);
    }

    /// 還沒啟動（`launched_ms == 0`）一定不能打。
    #[test]
    fn never_ready_before_launch() {
        let mut s = sig(false);
        s.launched_ms = 0;
        assert!(!agent_ready(&s));
    }

    /// Enter 補送：10 秒後才檢查；有輸出就不補。
    #[test]
    fn resends_enter_only_when_quiet() {
        assert_eq!(enter_swallowed(10_000, 0, 9_000), None, "還沒投遞過");
        assert_eq!(enter_swallowed(19_999, 10_000, 10_000), None, "不到 10 秒");
        // 投遞後 2 秒內沒有新輸出 → 被吞了
        assert_eq!(enter_swallowed(20_000, 10_000, 11_999), Some(true));
        // 投遞後 2 秒內有輸出 → 沒被吞
        assert_eq!(enter_swallowed(20_000, 10_000, 12_000), Some(false));
    }

    /// 一次能送幾封：不限＝全部；有上限＝只送剩下的額度。
    #[test]
    fn batches_up_to_the_limit() {
        assert_eq!(batch_size(5, 0, 100), 5, "0＝不限");
        assert_eq!(batch_size(5, 30, 0), 5);
        assert_eq!(batch_size(5, 30, 28), 2);
        assert_eq!(batch_size(5, 30, 30), 0);
        assert_eq!(batch_size(0, 30, 0), 0);
    }

    fn msg(name: &str, from: &str, to: &str, kind: &str, task: &str) -> AgentMessage {
        AgentMessage {
            file_name: name.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            task: task.to_string(),
            ..Default::default()
        }
    }

    /// 投遞那一行：一封 agent 的信 → `ma.deliverOne`，**和舊版打進來的那一行逐字一樣**。
    #[test]
    fn delivery_line_for_one_message() {
        let _g = crate::i18n::test_lock();
        crate::i18n::set_lang("zh-TW");
        let line = delivery_line(
            &[msg("0037-Agent-11-to-Agent-12.md", "Agent-11", "Agent-12", "TASK", "TASK-017")],
            34,
        );
        assert_eq!(
            line,
            "[AwayTerminal] 訊息 #34 from Agent-11 (TASK-017, TASK)：請讀 .ai/bus/0037-Agent-11-to-Agent-12.md，依你的角色處理，完成後回信給 Agent-11。"
        );
    }

    /// 沒有 task 欄位＝顯示破折號（舊版就是 `"—"`）。
    #[test]
    fn delivery_line_without_a_task() {
        let _g = crate::i18n::test_lock();
        crate::i18n::set_lang("zh-TW");
        let line = delivery_line(&[msg("0002-Agent-11-to-Agent-12.md", "Agent-11", "Agent-12", "QUESTION", "  ")], 2);
        assert!(line.contains("(—, QUESTION)"), "{line}");
    }

    /// AwayTerminal 自己的通知走 `ma.deliverInfo`（「不需要回信」）。
    #[test]
    fn delivery_line_for_a_system_notice() {
        let _g = crate::i18n::test_lock();
        crate::i18n::set_lang("zh-TW");
        let line = delivery_line(&[msg("0009-AwayTerminal-to-Agent-12.md", "AwayTerminal", "Agent-12", "INFO", "")], 9);
        assert_eq!(
            line,
            "[AwayTerminal] 通知 #9：請讀 .ai/bus/0009-AwayTerminal-to-Agent-12.md（AwayTerminal 的系統通知，不需要回信）。"
        );
    }

    /// 多封一次送：分隔符中文用「、」、英文用 ", "。
    #[test]
    fn delivery_line_for_a_batch() {
        let _g = crate::i18n::test_lock();
        crate::i18n::set_lang("zh-TW");
        let batch = [
            msg("0032-Agent-11-to-Agent-12.md", "Agent-11", "Agent-12", "TASK", "A"),
            msg("0034-Agent-11-to-Agent-12.md", "Agent-11", "Agent-12", "TASK", "B"),
            msg("0036-Agent-11-to-Agent-12.md", "Agent-11", "Agent-12", "TASK", "C"),
        ];
        let line = delivery_line(&batch, 10);
        assert_eq!(
            line,
            "[AwayTerminal] 你有 3 則新訊息：請依序讀 .ai/bus/0032-Agent-11-to-Agent-12.md、.ai/bus/0034-Agent-11-to-Agent-12.md、.ai/bus/0036-Agent-11-to-Agent-12.md，各自依你的角色處理並回信給寄件人。"
        );
        crate::i18n::set_lang("en");
        assert!(delivery_line(&batch, 10).contains(".md, .ai/bus/0034"));
        crate::i18n::set_lang("zh-TW");
    }
}
