//! AI 聊天室（搬移舊版 `MainWindow.ChatRoom.cs` ＋ `Services/ChatRoom/ChatRoleLibrary.cs`）。
//!
//! 2～4 個 AI 各帶一個角色，針對使用者給的主題**輪流**討論。畫面與分頁完全沿用代理團隊那一套
//! （[`Team`]、下一上 N−1 的 pane、分頁列一組一列、沙盒、恢復分頁、停止任務）；
//! 差別只有一件事：**不走 `.ai/bus/` 信箱，改由 AwayTerminal 主持**——
//! 輪到誰就在那一格打一行「第 N 回合輪到你」，它把發言寫成檔案，AwayTerminal 接進共用的
//! `transcript.md`，再換下一位。
//!
//! ## 檔案佈局（照舊版）
//! ```text
//! <專案>/.ai/chat/<yyyyMMdd-HHmm>[-n]/
//!     transcript.md        ← 共用紀錄，**只有 AwayTerminal 寫**（agent 不准改）
//!     r{回合}-Agent-xx.md  ← 每個人自己這一輪的發言
//!     conclusion.md        ← 主持人最後寫的結論
//! ```
//!
//! ## 為什麼沒有 watcher
//! 代理團隊要「發現任何人隨時寫的信」，所以需要掃目錄；聊天室是**我們先開口問**，
//! 然後只等那一個檔案，所以每 600ms 的 tick 直接看那個檔的 mtime 就夠
//! （[`read_finished`]）。舊版也是這樣做的。
//!
//! ## 一個時序陷阱（舊版註解）
//! 讀「寫完了」的檔案一定要確認它是**我們開口問之後**才寫的：恢復分頁、或同一個資料夾再開
//! 一場時，上一場留下的 `r1-Agent-11.md` 會被當成這一場的發言。而且要等 mtime ≥1 秒前
//! 才算寫完，否則會讀到一半。

use std::path::{Path, PathBuf};

use tauri::AppHandle;

use super::team::{ChatPhase, Slot, Team, CHAT_REL_DIR, TURN_TIMEOUT_MINUTES};

/// 這場討論的資料夾（絕對路徑）。
pub fn chat_dir(team: &Team) -> PathBuf {
    Path::new(&team.work_dir)
        .join(".ai")
        .join("chat")
        .join(&team.chat_folder)
}

/// 這場討論裡某個檔的絕對路徑。
pub fn chat_path(team: &Team, file: &str) -> PathBuf {
    chat_dir(team).join(file)
}

/// 打進畫面那一行裡用的相對路徑（agent 的工作目錄就是 `work_dir`）。
pub fn chat_rel_path(team: &Team, file: &str) -> String {
    format!("{CHAT_REL_DIR}/{}/{file}", team.chat_folder)
}

/// 這一輪某位要寫的檔名。
pub fn turn_file(round: u32, agent_id: &str) -> String {
    format!("r{round}-{agent_id}.md")
}

/// 這場討論的資料夾名＝現在的時間；同一分鐘內已經有一場（同資料夾開兩間、連續換主題）
/// 就補 `-2`、`-3`，**不共用**（舊版 `NewChatFolder`）。
pub fn new_chat_folder(work_dir: &str) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M").to_string();
    let base = Path::new(work_dir).join(".ai").join("chat");
    let mut name = stamp.clone();
    let mut n = 2;
    while base.join(&name).is_dir() {
        name = format!("{stamp}-{n}");
        n += 1;
        if n > 99 {
            break;
        }
    }
    name
}

/// 往 `transcript.md` 追加一段（UTF-8 無 BOM，段落之間空一行）。
pub fn write_transcript(team: &Team, text: &str) {
    let path = chat_path(team, "transcript.md");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body = format!("{}\n\n", text.trim_end());
    use std::io::Write;
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(body.as_bytes()) {
                println!("[AwayTerminal] 聊天室討論紀錄寫入失敗：{e}");
            }
        }
        Err(e) => println!("[AwayTerminal] 聊天室討論紀錄開檔失敗：{e}"),
    }
}

/// 讀「寫完了」的檔案（舊版 `ReadFinished`）。`None`＝還沒有／還在寫／是**上一場**留下的舊檔。
///
/// 兩個條件（都照舊版）：
/// 1. mtime **不能早於**我們開口問的時間 −2 秒（FAT／網路磁碟的 mtime 只有 2 秒解析度，留餘裕）
/// 2. mtime 至少 1 秒前（還在寫的檔不要讀）
pub fn read_finished(path: &Path, asked_ms: u128, now_ms: u128) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let written = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    if written + 2000 < asked_ms {
        return None; // 舊檔
    }
    if now_ms.saturating_sub(written) < 1000 {
        return None; // 還在寫
    }
    // agent 可能用 Windows PowerShell 5.1 寫檔（UTF-16／Big5）：照 BOM／內容解碼，
    // 不是 UTF-8 也要讀得到，否則這一位每回合都被當成逾時（G3 同一類）
    let (text, _) = super::message::read_text(path).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

// ---------------------------------------------------------------- 執行期脈絡

/// 聊天室的第三層（舊版 `ChatRoleLibrary.RuntimeContext`）。
///
/// **和代理團隊的完全不同，而且是中文**——照舊版原樣。
// i18n-audit:log-only-begin 這一段是**角色檔的內容**（agent 讀的，不是 UI）。
// 舊版 `ChatRoleLibrary.RuntimeContext` 就是中文，照 TASK-017 A10 的規則不套八語。
pub fn runtime_context(team: &Team, me: &Slot) -> String {
    let joined: Vec<&Slot> = team.slots.iter().filter(|s| s.enabled).collect();
    let w1 = joined
        .iter()
        .map(|x| x.agent_id().chars().count())
        .max()
        .unwrap_or(0);
    let w2 = joined
        .iter()
        .map(|x| x.role_title.chars().count())
        .max()
        .unwrap_or(0);
    let host = joined.first().copied();

    let mut sb = String::new();
    sb.push_str("# Runtime Context（AwayTerminal 產生）\n\n");
    sb.push_str(&format!("你的代號：{}\n", me.agent_id()));
    sb.push_str(&format!("你的角色：{}\n", me.role_title));
    sb.push_str(&format!("你用的 AI：{}\n", me.backend_name()));
    sb.push_str(&format!("聊天室編號：CHAT-{}\n", team.number));
    // 有沙盒時 agent 的工作目錄是 worktree，討論紀錄也在那裡——寫原始 repo 的路徑，
    // agent 照字面組路徑就會寫到原始 repo 去（G5）。沒沙盒時 work_dir＝dir，文字不變。
    sb.push_str(&format!("專案資料夾：{}\n", team.work_dir));
    sb.push_str(&format!(
        "討論回合：{} 回合（一回合＝每個人各發言一次；使用者可以提前結束）\n",
        team.rounds
    ));
    sb.push_str("參加者：\n");
    for x in &joined {
        sb.push_str(&format!(
            "  - {}  {}  ({}){}\n",
            pad(&x.agent_id(), w1),
            pad(&x.role_title, w2),
            x.backend_name(),
            if x.index == me.index { "   ← 你" } else { "" }
        ));
    }
    if let Some(h) = host {
        sb.push_str(&format!("主持人：{}（{}）\n\n", h.agent_id(), h.role_title));
    } else {
        sb.push('\n');
    }

    sb.push_str("## 討論紀錄與你的發言\n\n");
    sb.push_str(&format!(
        "- 共用討論紀錄：{CHAT_REL_DIR}/{}/transcript.md（所有人的發言，AwayTerminal 依序接進去）\n",
        team.chat_folder
    ));
    sb.push_str(&format!(
        "- 你的發言檔：{CHAT_REL_DIR}/{}/r{{回合數}}-{}.md（例：r1-{}.md）\n",
        team.chat_folder,
        me.agent_id(),
        me.agent_id()
    ));
    sb.push_str("- 路徑都是相對於專案資料夾。檔案一律 UTF-8（Windows PowerShell 讀檔請用 Get-Content -Raw -Encoding UTF8）。\n\n");
    sb.push_str("輪到你時，AwayTerminal 會在你的畫面打一行字，告訴你第幾回合、要讀哪一份紀錄、發言要寫到哪個檔案。\n");
    sb.push_str("**檔案路徑一律以那一行給的為準**（使用者換主題時，討論紀錄會換到新的資料夾，上面寫的路徑就過期了）。照著做：\n");
    sb.push_str("讀紀錄 → 想清楚你要回應誰 → 把發言寫進上面那個檔案（300 字以內）→ 結束這一輪。\n");
    sb.push_str("不要修改別人的發言檔，也不要改 transcript.md。\n\n");

    if host.is_some_and(|h| h.index == me.index) {
        sb.push_str("## 你是主持人\n\n");
        sb.push_str(&format!(
            "- 討論結束時 AwayTerminal 會叫你寫結論，寫到 {CHAT_REL_DIR}/{}/conclusion.md（600 字以內），\n",
            team.chat_folder
        ));
        sb.push_str("  並在你自己的畫面上顯示摘要給使用者看。\n");
        sb.push_str("- 使用者只跟你說話；其他參加者只透過討論紀錄互動。\n\n");
    }

    if !team.topic.trim().is_empty() {
        sb.push_str(&format!("## 這次的主題\n\n{}\n", team.topic));
    }
    sb
}

// i18n-audit:log-only-end

/// `String::PadRight`（依**字元數**補空白）。
fn pad(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

// ---------------------------------------------------------------- 主持（600ms tick）

/// tick 要做的事（打字的動作交給呼叫端，因為那要放掉鎖）。
pub enum ChatAction {
    /// 什麼都不用做。
    None,
    /// 在這個分頁打一行字（＋Enter）。
    Type { tab: u32, text: String },
}

/// 聊天室的一次 tick（舊版 `ChatRoomTick`）。**在 `TeamManager` 的鎖裡呼叫。**
///
/// 回傳「要打什麼字」讓呼叫端在放掉鎖之後做（`send_text_then_enter` 會 emit、也會排 300ms 的 Enter）。
/// `ready` 是那一格的 `agent_ready`（呼叫端算好傳進來，因為它要看分頁的時間戳）。
pub fn tick(
    team: &mut Team,
    now: u128,
    alive: &dyn Fn(u32) -> bool,
    ready: &dyn Fn(&Slot) -> bool,
) -> ChatAction {
    if matches!(team.phase, ChatPhase::NeedTopic | ChatPhase::Done) || team.paused {
        return ChatAction::None;
    }
    // 參加者＝已啟動的格（依格號）。**已結束的格也留在名單裡**，索引才不會跳動；
    // 輪到他時直接跳過、不等 5 分鐘（舊版註解）。
    let speakers: Vec<(usize, u32, String, String)> = team
        .slots
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.tab.map(|tab| (i, tab, s.agent_id(), s.role_title.clone())))
        .collect();
    if speakers.is_empty() {
        return ChatAction::None;
    }

    // ---- 寫結論 ----
    if team.phase == ChatPhase::Concluding {
        // 主持人固定是**格 1**（角色下拉停用的那一格）。不能拿 `speakers[0]`——格 1 的分頁
        // 關掉後那是第 2 位，會叫一個普通參加者去寫結論（G4）。格 1 不在＝主持人已結束。
        let Some((hi, htab, hid, _)) = speakers
            .iter()
            .find(|(i, _, _, _)| team.slots[*i].index == 1)
            .cloned()
        else {
            let hid = format!("Agent-{}1", team.number);
            let text = crate::i18n::tf("chat.trHostGone", &[&hid]);
            write_transcript(team, &text);
            finish(team, true);
            return ChatAction::None;
        };
        if !alive(htab) {
            // 主持人已結束：沒人能寫結論，收場並在紀錄註明
            let text = crate::i18n::tf("chat.trHostGone", &[&hid]);
            write_transcript(team, &text);
            finish(team, true);
            return ChatAction::None;
        }
        if team.turn_asked_ms == 0 {
            if !ready(&team.slots[hi]) {
                // 主持人一直忙到問不了：等滿兩倍逾時就收場（同結論逾時）
                if team.turn_started_ms != 0
                    && now.saturating_sub(team.turn_started_ms) >= TURN_TIMEOUT_MINUTES * 2 * 60_000
                {
                    finish(team, true);
                }
                return ChatAction::None;
            }
            let text = crate::i18n::tf(
                "chat.conclusionPrompt",
                &[
                    &(team.round.saturating_sub(1)).to_string(),
                    &chat_rel_path(team, "transcript.md"),
                    &chat_rel_path(team, "conclusion.md"),
                ],
            );
            team.turn_asked_ms = now;
            team.asked_agent_id = hid.clone();
            super::deliver::mark_typed(&mut team.slots[hi], now as u64);
            println!("[AwayTerminal] 聊天室 CHAT-{}：請 {hid} 寫結論", team.number);
            return ChatAction::Type { tab: htab, text };
        }
        let path = chat_path(team, "conclusion.md");
        match read_finished(&path, team.turn_asked_ms, now) {
            Some(conclusion) => {
                let title = crate::i18n::t("chat.trConclusion");
                let role = team.slots[hi].role_title.clone();
                write_transcript(team, &format!("## {title}（{hid} {role}）\n\n{conclusion}"));
                finish(team, false);
            }
            None => {
                if now.saturating_sub(team.turn_asked_ms) >= TURN_TIMEOUT_MINUTES * 2 * 60_000 {
                    finish(team, true);
                }
            }
        }
        return ChatAction::None;
    }

    // ---- 討論中 ----
    // 使用者按了「結束討論」而下一位還沒被問到 → 不再多問一個人，直接去寫結論
    if team.turn_asked_ms == 0 && team.end_requested {
        conclude(team, "user");
        return ChatAction::None;
    }

    let (si, tab, agent_id, role_title) = if team.turn_asked_ms != 0 {
        // 問過某位、等他發言：**一律先用 Agent ID 找回他**（期間名單變了索引會跑掉——
        // 這要在「索引超出範圍」的判斷之前做，否則最後一位在等發言時前面有人被關掉，
        // 他的那一輪會被整個略過）；他自己被關掉了就換下一位。
        match speakers.iter().position(|(_, _, id, _)| *id == team.asked_agent_id) {
            Some(pos) => {
                team.speaker = pos;
                speakers[pos].clone()
            }
            None => {
                advance_turn(team, speakers.len());
                return ChatAction::None;
            }
        }
    } else {
        if team.speaker >= speakers.len() {
            advance_turn(team, speakers.len());
            return ChatAction::None;
        }
        speakers[team.speaker].clone()
    };

    let file = turn_file(team.round, &agent_id);
    if !alive(tab) {
        // 這一格已結束（問之前或問之後都一樣）：不等 5 分鐘，跳過並在紀錄註明
        let text = crate::i18n::tf("chat.trEnded", &[&team.round.to_string(), &agent_id]);
        write_transcript(team, &text);
        println!(
            "[AwayTerminal] 聊天室 CHAT-{}：{agent_id} 已結束，第 {} 回合跳過",
            team.number, team.round
        );
        advance_turn(team, speakers.len());
        return ChatAction::None;
    }

    if team.turn_asked_ms == 0 {
        if !ready(&team.slots[si]) {
            // 一直忙碌（畫面不停重繪之類）連問都問不到：等滿逾時一樣跳過並註明，
            // 5 分鐘逾時才不會只保護「問了以後」
            if team.turn_started_ms != 0
                && now.saturating_sub(team.turn_started_ms) >= TURN_TIMEOUT_MINUTES * 60_000
            {
                let text = crate::i18n::tf(
                    "chat.trSkipped",
                    &[
                        &team.round.to_string(),
                        &agent_id,
                        &TURN_TIMEOUT_MINUTES.to_string(),
                    ],
                );
                write_transcript(team, &text);
                println!(
                    "[AwayTerminal] 聊天室 CHAT-{}：{agent_id} 第 {} 回合一直忙碌，跳過",
                    team.number, team.round
                );
                advance_turn(team, speakers.len());
            }
            return ChatAction::None;
        }
        let text = crate::i18n::tf(
            "chat.turnPrompt",
            &[
                &team.round.to_string(),
                &team.rounds.to_string(),
                &role_title,
                &chat_rel_path(team, "transcript.md"),
                &chat_rel_path(team, &file),
            ],
        );
        team.turn_asked_ms = now;
        team.asked_agent_id = agent_id.clone();
        super::deliver::mark_typed(&mut team.slots[si], now as u64);
        println!(
            "[AwayTerminal] 聊天室 CHAT-{}：第 {}/{} 回合 → {agent_id}（{role_title}）",
            team.number, team.round, team.rounds
        );
        return ChatAction::Type { tab, text };
    }

    let path = chat_path(team, &file);
    if let Some(said) = read_finished(&path, team.turn_asked_ms, now) {
        let head = crate::i18n::tf(
            "chat.trTurn",
            &[&team.round.to_string(), &agent_id, &role_title],
        );
        write_transcript(team, &format!("## {head}\n\n{said}"));
        advance_turn(team, speakers.len());
        return ChatAction::None;
    }
    if now.saturating_sub(team.turn_asked_ms) >= TURN_TIMEOUT_MINUTES * 60_000 {
        let text = crate::i18n::tf(
            "chat.trSkipped",
            &[
                &team.round.to_string(),
                &agent_id,
                &TURN_TIMEOUT_MINUTES.to_string(),
            ],
        );
        write_transcript(team, &text);
        println!(
            "[AwayTerminal] 聊天室 CHAT-{}：{agent_id} 第 {} 回合逾時",
            team.number, team.round
        );
        advance_turn(team, speakers.len());
    }
    ChatAction::None
}

/// 換下一位；一回合走完換下一回合；回合數跑完（或使用者按了結束）就去寫結論。
/// 舊版 `AdvanceChatTurn`。
pub fn advance_turn(team: &mut Team, speaker_count: usize) {
    team.turn_asked_ms = 0;
    team.asked_agent_id.clear();
    team.turn_started_ms = crate::tabs::now_ms() as u128;
    team.speaker += 1;
    if team.speaker < speaker_count && !team.end_requested {
        return;
    }
    team.speaker = 0;
    team.round += 1;
    if team.end_requested || team.round > team.rounds {
        let why = if team.end_requested { "user" } else { "rounds" };
        conclude(team, why);
    }
}

/// 進入「寫結論」階段（舊版 `ConcludeChat`）。
///
/// `round` 在這之後＝「跑完的回合數＋1」（結論提示用 `round − 1` 說共幾回合）。
pub fn conclude(team: &mut Team, why: &str) {
    team.turn_asked_ms = 0;
    team.asked_agent_id.clear();
    team.turn_started_ms = crate::tabs::now_ms() as u128;
    // 這一回合已經有人講過＝算一回合（從 `advance_turn` 來的已經加過了）
    if team.phase == ChatPhase::Discussing && team.speaker > 0 {
        team.round += 1;
    }
    team.speaker = 0;
    team.phase = ChatPhase::Concluding;
    println!(
        "[AwayTerminal] 聊天室 CHAT-{}：討論結束（{why}）→ 寫結論",
        team.number
    );
}

pub fn finish(team: &mut Team, timed_out: bool) {
    team.phase = ChatPhase::Done;
    team.turn_asked_ms = 0;
    team.asked_agent_id.clear();
    println!(
        "[AwayTerminal] 聊天室 CHAT-{}：結束{}",
        team.number,
        if timed_out { "（沒有結論）" } else { "" }
    );
}

/// 開始一場討論（舊版 `StartChatDiscussion`）。角色檔要由呼叫端重組（它有 data_dir）。
///
/// 回傳 true＝換了新的討論紀錄資料夾（呼叫端要重組角色檔，裡面寫著資料夾路徑）。
pub fn start_discussion(team: &mut Team, topic: &str) -> bool {
    // 這個資料夾已經有一場討論（換主題、或恢復分頁接回來的那場）→ 開新的資料夾，
    // 舊的發言檔／結論不會被當成這一場的
    let mut new_folder = false;
    if team.chat_folder.is_empty() || chat_path(team, "transcript.md").is_file() {
        team.chat_folder = new_chat_folder(&team.work_dir);
        new_folder = true;
    }
    team.topic = topic.trim().to_string();
    team.round = 1;
    team.speaker = 0;
    team.end_requested = false;
    team.turn_asked_ms = 0;
    team.asked_agent_id.clear();
    team.turn_started_ms = crate::tabs::now_ms() as u128;
    team.phase = ChatPhase::Discussing;
    new_folder
}

/// 討論紀錄的開頭（`start_discussion` 之後、角色檔重組之後寫）。
pub fn transcript_header(team: &Team) -> String {
    let mut sb = String::new();
    sb.push_str(&format!(
        "# {} CHAT-{}\n",
        crate::i18n::t("chat.title"),
        team.number
    ));
    sb.push_str(&crate::i18n::tf("chat.trTopic", &[&team.topic]));
    sb.push('\n');
    sb.push_str(&crate::i18n::tf("chat.trRounds", &[&team.rounds.to_string()]));
    sb.push('\n');
    for s in team.running() {
        sb.push_str(&format!(
            "- {}  {}  ({})\n",
            s.agent_id(),
            s.role_title,
            s.backend_name()
        ));
    }
    sb
}

/// pane 狀態標籤與分頁 tooltip 用的一句話（舊版 `chat.tip*`）。
pub fn status_text(team: &Team) -> String {
    match team.phase {
        ChatPhase::NeedTopic => crate::i18n::t("chat.tipNeedTopic"),
        ChatPhase::Discussing => crate::i18n::tf(
            "chat.tipRound",
            &[&team.round.to_string(), &team.rounds.to_string()],
        ),
        ChatPhase::Concluding => crate::i18n::t("chat.tipConcluding"),
        ChatPhase::Done => crate::i18n::t("chat.tipDone"),
    }
}

/// 把使用者的一段話接進討論紀錄（舊版 `ChatSay_Click`）。
pub fn user_said(team: &Team, text: &str) {
    let head = crate::i18n::t("chat.trUserSaid");
    write_transcript(team, &format!("## {head}\n\n{}", text.trim()));
    println!("[AwayTerminal] 聊天室 CHAT-{}：使用者插話", team.number);
}

/// 只在需要時 emit：聊天室的狀態變了（前端的分頁列那一列要重畫）。
pub fn post_state(app: &AppHandle, teams: &std::sync::Arc<super::TeamManager>) {
    super::post_state(app, teams);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::team::GroupKind;

    fn chat_team(dir: &str) -> Team {
        let mut t = Team::new("k", 1, dir);
        t.kind = GroupKind::Chat;
        t.work_dir = dir.to_string();
        t.chat_folder = "20260927-1200".to_string();
        t.rounds = 2;
        for (i, (role, title)) in [("host", "主持人"), ("devils-advocate", "反方辯論者")]
            .iter()
            .enumerate()
        {
            let s = &mut t.slots[i];
            s.enabled = true;
            s.role = role.to_string();
            s.role_title = title.to_string();
            s.backend = "claude-code".to_string();
            s.tab = Some(10 + i as u32);
            s.launched_ms = 1;
        }
        t
    }

    /// 發言檔名與相對路徑照舊版。
    #[test]
    fn file_names_follow_v1() {
        let t = chat_team("C:\\p");
        assert_eq!(turn_file(1, "Agent-11"), "r1-Agent-11.md");
        assert_eq!(turn_file(12, "Agent-34"), "r12-Agent-34.md");
        assert_eq!(
            chat_rel_path(&t, "transcript.md"),
            ".ai/chat/20260927-1200/transcript.md"
        );
        assert_eq!(
            chat_rel_path(&t, &turn_file(2, "Agent-12")),
            ".ai/chat/20260927-1200/r2-Agent-12.md"
        );
    }

    /// 輪替：兩個人 × 2 回合，之後進入寫結論。
    #[test]
    fn round_robin_then_conclude() {
        let mut t = chat_team("C:\\p");
        t.phase = ChatPhase::Discussing;
        assert_eq!((t.round, t.speaker), (1, 0));
        advance_turn(&mut t, 2); // 第 1 回合第 1 位講完
        assert_eq!((t.round, t.speaker), (1, 1));
        advance_turn(&mut t, 2); // 第 1 回合結束
        assert_eq!((t.round, t.speaker), (2, 0));
        assert_eq!(t.phase, ChatPhase::Discussing);
        advance_turn(&mut t, 2);
        advance_turn(&mut t, 2); // 第 2 回合結束＝回合數跑完
        assert_eq!(t.phase, ChatPhase::Concluding, "回合數跑完要去寫結論");
        assert_eq!(t.round, 3, "結論提示用 round-1 說共幾回合");
    }

    /// 使用者按「結束討論」：這一輪結束後就去寫結論，不再多問一個人。
    #[test]
    fn user_end_stops_after_this_turn() {
        let mut t = chat_team("C:\\p");
        t.phase = ChatPhase::Discussing;
        t.end_requested = true;
        advance_turn(&mut t, 4);
        assert_eq!(t.phase, ChatPhase::Concluding);
    }

    /// `conclude` 從「這一回合已經有人講過」進來要多算一回合。
    #[test]
    fn conclude_counts_a_partial_round() {
        let mut t = chat_team("C:\\p");
        t.phase = ChatPhase::Discussing;
        t.round = 2;
        t.speaker = 1; // 第 2 回合已經有人講過
        conclude(&mut t, "user");
        assert_eq!(t.round, 3);
        assert_eq!(t.speaker, 0);

        let mut t2 = chat_team("C:\\p");
        t2.phase = ChatPhase::Discussing;
        t2.round = 2;
        t2.speaker = 0; // 還沒有人講
        conclude(&mut t2, "user");
        assert_eq!(t2.round, 2, "沒人講過就不多算");
    }

    /// `read_finished`：舊檔不算、還在寫不算、寫完才算。
    #[test]
    fn read_finished_rejects_stale_and_fresh_files() {
        let dir = std::env::temp_dir().join(format!("awayterm-chat-rf-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("r1-Agent-11.md");
        std::fs::write(&path, "我的發言").unwrap();
        let written = std::fs::metadata(&path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        // 我們「之後」才開口問 → 這是上一場的舊檔
        assert_eq!(read_finished(&path, written + 5000, written + 9000), None);
        // 剛寫完 1 秒內 → 可能還在寫
        assert_eq!(read_finished(&path, written - 1000, written + 500), None);
        // 問了之後寫、而且已經穩定 → 算
        assert_eq!(
            read_finished(&path, written - 1000, written + 2000).as_deref(),
            Some("我的發言")
        );
        // 空檔不算
        std::fs::write(&path, "   \n").unwrap();
        assert_eq!(read_finished(&path, 0, written + 9000), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 資料夾名：同一分鐘第二場補 `-2`。
    #[test]
    fn folder_names_do_not_collide() {
        let dir = std::env::temp_dir().join(format!("awayterm-chat-nf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = new_chat_folder(&dir.to_string_lossy());
        std::fs::create_dir_all(dir.join(".ai").join("chat").join(&first)).unwrap();
        let second = new_chat_folder(&dir.to_string_lossy());
        assert_ne!(first, second);
        assert_eq!(second, format!("{first}-2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `start_discussion`：已經有 transcript 就換新資料夾（換主題不會污染上一場）。
    #[test]
    fn starting_again_uses_a_new_folder() {
        let dir = std::env::temp_dir().join(format!("awayterm-chat-sd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut t = chat_team(&dir.to_string_lossy());
        t.chat_folder = String::new();
        assert!(start_discussion(&mut t, "第一個主題"));
        let first = t.chat_folder.clone();
        assert!(!first.is_empty());
        assert_eq!(t.phase, ChatPhase::Discussing);
        // 還沒有 transcript → 同一個資料夾
        assert!(!start_discussion(&mut t, "還沒寫過紀錄"));
        assert_eq!(t.chat_folder, first);
        // 有 transcript 了 → 換新資料夾
        write_transcript(&t, "## 第 1 回合");
        assert!(start_discussion(&mut t, "換主題"));
        assert_ne!(t.chat_folder, first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 執行期脈絡：主持人多一段，非主持人沒有；主題寫在最後。
    #[test]
    fn runtime_context_marks_the_host() {
        let mut t = chat_team("C:\\Projects\\Example");
        t.topic = "要不要自己寫 SSH".to_string();
        let host = runtime_context(&t, &t.slots[0]);
        assert!(host.starts_with("# Runtime Context（AwayTerminal 產生）"));
        assert!(host.contains("你的代號：Agent-11"));
        assert!(host.contains("聊天室編號：CHAT-1"));
        assert!(host.contains("## 你是主持人"));
        assert!(host.contains("## 這次的主題\n\n要不要自己寫 SSH"));
        // 欄位對齊：角色標題補到最長那個的字元數（反方辯論者＝5 字）再加兩個空白
        assert!(host.contains("  - Agent-11  主持人    (ClaudeCode)   ← 你"));
        assert!(host.contains("  - Agent-12  反方辯論者  (ClaudeCode)\n"));
        let other = runtime_context(&t, &t.slots[1]);
        assert!(!other.contains("## 你是主持人"));
        assert!(other.contains("主持人：Agent-11（主持人）"));
        assert!(other.contains("r{回合數}-Agent-12.md"));
    }
}
