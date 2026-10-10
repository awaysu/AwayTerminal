//! 團隊與格的狀態（搬移舊版 `Models/AgentGroup.cs` ＋ `AgentSlot.cs`）。
//!
//! 一個團隊 2～4 個 agent，每個 agent 是**一個獨立的分頁**（session／xterm／log／恢復畫面
//! 都照一般分頁走）；綁組只是薄薄一層。右側分頁列只顯示代表列那一列。
//!
//! | 項目 | 舊版 | 這裡 |
//! |---|---|---|
//! | Agent ID | `Agent-{組號}{格號}`（組號 1～9、格號 1～4） | 同 |
//! | 組號取法 | 目前開著的組裡最小的空號；恢復時優先沿用上次的 | 同（[`next_free_number`]） |
//! | 外框顏色 | 格 1 淡紅 `#EF9A9A`、2 淡藍 `#90CAF9`、3 淡綠 `#A5D6A7`、4 淡紫 `#CE93D8` | 同 |
//! | pane 標題 | `Agent-12 · Software Engineer · Codex` | 同（[`Slot::label`]） |
//! | 投遞上限 | 預設 30，可選 10／30／50／100／0（不限） | 選項同；**預設 50**（2026-10-01 使用者要求：30 則跑到一半就停） |
//! | 閒置檢查 | 預設 30 分鐘，可選 15／30／60／0（不檢查） | 同 |
//! | 上下列比例 | 0.15～0.85，預設 0.5 | 同（[`clamp_ratio`]） |

use std::collections::VecDeque;

use super::message::AgentMessage;

/// 投遞上限的預設值。舊版 `DefaultMaxMessages` 是 30；2026-10-01 使用者改成 50。
/// 只影響**新開的**團隊——恢復的團隊沿用它存下來的上限。
pub const DEFAULT_MAX_MESSAGES: u32 = 50;
/// 設定視窗與右鍵選單可選的上限（0＝不限）。
pub const LIMIT_CHOICES: &[u32] = &[10, 30, 50, 100, 0];
/// 閒置檢查的預設分鐘數（照舊版 `DefaultIdleCheckMinutes`）。
pub const DEFAULT_IDLE_CHECK_MINUTES: u32 = 30;
/// 可選的閒置檢查分鐘數（0＝不檢查）。
pub const IDLE_CHECK_CHOICES: &[u32] = &[15, 30, 60, 0];
/// 一個團隊最多幾格。
pub const MAX_SLOTS: usize = 4;
/// 最多同時開幾組。
pub const MAX_TEAMS: u32 = 9;

/// 這一組的用途：代理團隊（信箱分工）或 AI 聊天室（輪流討論）。
/// 舊版 `Models/AgentGroup.cs` 的 `GroupMode`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupKind {
    #[default]
    Team,
    Chat,
}

/// AI 聊天室的進行階段（舊版 `ChatPhase`）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatPhase {
    /// 等使用者給主題。
    #[default]
    NeedTopic,
    /// 輪流發言中。
    Discussing,
    /// 已請主持人寫結論。
    Concluding,
    /// 結論寫完了。
    Done,
}

/// 聊天室的預設回合數（舊版 1.2.8 從 5 改成 3，使用者要求）。
pub const DEFAULT_ROUNDS: u32 = 3;
/// 設定視窗可選的回合數。
pub const ROUND_CHOICES: &[u32] = &[3, 5, 8, 10];
/// 某一位超過這麼多分鐘沒發言就跳過他這一回合（在紀錄註明）。
pub const TURN_TIMEOUT_MINUTES: u128 = 5;
/// 討論紀錄資料夾（相對專案資料夾）。
pub const CHAT_REL_DIR: &str = ".ai/chat";

/// 一格（一個 agent）。
#[derive(Clone, Debug, Default)]
pub struct Slot {
    /// 1～4。
    pub index: u32,
    /// 組號（算 Agent ID 用）。
    pub team_number: u32,
    /// 角色檔名（`roles/*.md` 去副檔名，例 `software-engineer`）；空＝None。
    pub role: String,
    /// 角色標題（角色檔第一個 `#` 標題；None＝`None`）。
    pub role_title: String,
    /// CLI 種類（`claude-code`／`codex`／`opencode`／`geminicli`）。
    pub backend: String,
    /// 這一格用的模型（傳給 CLI 的 `--model`；空＝預設，不加參數）。**啟動時才生效**，
    /// 所以改模型和改 CLI／角色一樣要重開那一格。
    pub model: String,
    /// 設定視窗勾了「啟用」。
    pub enabled: bool,
    /// 這格的分頁 id（還沒啟動＝`None`）。
    pub tab: Option<u32>,
    /// 這次啟動的時間（epoch ms）。**不能用分頁的開啟時間**——恢復分頁時那是原始時間。
    pub launched_ms: u128,
    /// 這格是經 PowerShell 啟動的（npm 版的 `.cmd`）。
    ///
    /// 舊版看的是 `tab.Kind == TermKind.PowerShell`；我們的自訂連線分頁不會是那個種類，
    /// 所以啟動時直接把連線的 `via_powershell` 記在這裡（[`super::deliver::agent_ready`]
    /// 要靠它決定多等一點）。
    pub via_ps: bool,
    /// 角色已經交給這個 CLI（啟動參數注入＝一開始就 true；OpenCode／Gemini 要等打完第一句）。
    pub role_injected: bool,
    /// 還沒打給 CLI 的「請先讀角色檔」那一句。
    pub pending_first_message: Option<String>,
    /// 組好的角色檔路徑。
    pub role_file: String,
    /// 恢復分頁時要沿用的執行檔／參數（舊版 `LaunchSlot` 的 `saved` 分支：
    /// 「每格照上次的執行檔／參數」）。`None`＝重新解析一次。
    pub saved_conn: Option<crate::settings::CustomConn>,
    /// 待投遞的信（FIFO）。
    pub queue: VecDeque<AgentMessage>,
    /// 上一次打字給這格的時間（epoch ms；0＝還沒打過）。
    pub last_delivered_ms: u128,
    /// 這次投遞後是否已經檢查過「Enter 有沒有被吞」（只補送一次）。
    pub delivery_checked: bool,
    /// 上次送給前端的狀態標籤（`E` 協定；`None`＝要重送）。
    pub posted_state: Option<u32>,
}

impl Slot {
    pub fn new(team_number: u32, index: u32) -> Self {
        Self {
            index,
            team_number,
            role_title: "None".to_string(),
            delivery_checked: true,
            ..Default::default()
        }
    }

    /// `Agent-{組號}{格號}`。
    pub fn agent_id(&self) -> String {
        format!("Agent-{}{}", self.team_number, self.index)
    }

    /// 外框顏色（Material 200 級，和分頁列狀態圖示同一系列）。
    pub fn color(&self) -> &'static str {
        match self.index {
            1 => "#EF9A9A",
            2 => "#90CAF9",
            3 => "#A5D6A7",
            _ => "#CE93D8",
        }
    }

    /// CLI 的顯示名稱（認不出來就原樣回傳，同舊版 `BackendName`）。
    pub fn backend_name(&self) -> String {
        super::adapters::display_name_of(&self.backend)
    }

    /// pane 標題：`Agent-12 · Software Engineer · Codex`。
    pub fn label(&self) -> String {
        format!(
            "{} · {} · {}",
            self.agent_id(),
            self.role_title,
            self.backend_name()
        )
    }

    /// 角色縮寫（視窗標題與分頁 tooltip 用）。
    pub fn short_label(&self) -> String {
        format!("{} {}", self.agent_id(), self.role_title)
    }

    /// 套用設定時「這一格要怎麼處理」（舊版 `MultiAgentDialog.ActionOf`）。
    ///
    /// | 目前 | 想要 | 結果 |
    /// |---|---|---|
    /// | 沒有分頁 | 啟用 | `Start` |
    /// | 沒有分頁 | 不啟用 | `None` |
    /// | 有分頁 | 不啟用 | `Close` |
    /// | 有分頁 | 啟用、CLI／角色都沒變、沒按重新啟動 | `None` |
    /// | 有分頁 | 啟用、CLI 或角色變了，或按了重新啟動 | `Restart` |
    ///
    /// 「改角色也要重開」是因為**角色是啟動時注入的**（`--append-system-prompt-file`／
    /// `developer_instructions`），不重開它讀到的還是舊角色。
    pub fn action_for(&self, want_enabled: bool, backend: &str, role: &str, want_restart: bool) -> SlotAction {
        // 格 1 永遠啟用（設定視窗的勾選是停用的）
        let want_enabled = want_enabled || self.index == 1;
        if self.tab.is_none() {
            return if want_enabled { SlotAction::Start } else { SlotAction::None };
        }
        if !want_enabled {
            return SlotAction::Close;
        }
        let same = self.backend.eq_ignore_ascii_case(backend) && self.role.eq_ignore_ascii_case(role);
        if same && !want_restart {
            SlotAction::None
        } else {
            SlotAction::Restart
        }
    }
}

/// 套用設定後這一格會發生什麼事。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SlotAction {
    None,
    Start,
    Restart,
    Close,
}

/// 一個團隊。
/// 沒有 `Clone`／`Debug`：裡面有信箱監看（`Arc<MessageBus>`）與排隊中的信，
/// 複製一份沒有意義——要讀狀態就在 [`super::TeamManager`] 的鎖裡讀。
pub struct Team {
    /// 穩定代號（恢復分頁時靠它把各格綁回來）。
    pub key: String,
    /// 組號 1～9。
    pub number: u32,
    /// 專案資料夾（所有 agent 共用）。
    pub dir: String,
    /// 分頁列那一列的標題（預設資料夾名）。
    pub title: String,
    pub slots: Vec<Slot>,
    /// 上列占的高度比例。
    pub ratio: f64,
    /// 暫停投遞（照收信、照排隊，只是不打字）。
    pub paused: bool,
    /// 這次暫停是投遞到上限造成的（上限調高時自動解除；使用者自己按的不解除）。
    pub paused_by_limit: bool,
    /// 本輪已投遞幾則。
    pub message_count: u32,
    /// 投遞上限（0＝不限）。
    pub max_messages: u32,
    /// 整組閒置這麼多分鐘就請 Agent-x1 問大家狀況（0＝不檢查）。
    pub idle_check_minutes: u32,
    /// 整組從什麼時候開始全部閒置（epoch ms；0＝現在不是全閒置）。
    pub all_idle_since_ms: u128,
    /// 上一次閒置提問送出的時間（epoch ms；0＝還沒問過／已有新動靜可以再問）。
    /// 問過一次之後，沒有新動靜（使用者打字、投遞、遠端指令）就不再重複問，
    /// 免得工作都做完了還每隔幾十分鐘戳一次 Agent-x1（使用者回報 2026-10-10）。
    pub idle_check_sent_ms: u128,
    /// 上次閒置提問送出時 Agent-x1 的輸出書籤（`OutputTail::position`）：只在這之後找完成標記。
    pub idle_check_tail_pos: u64,
    /// 這一段連續輸出從什麼時候開始（epoch ms；0＝現在沒有）。閒置檢查用它分辨
    /// 「真的在工作」和「閒置中偶爾重畫一下畫面」，見 `deliver::check_team_idle`。
    pub busy_since_ms: u128,
    /// 這一段連續輸出最後一次看到有人忙的時間（epoch ms）。
    pub last_busy_ms: u128,
    /// 本次執行內的投遞序號（「訊息 #n」用，從 1 起）。
    pub delivery_seq: u32,
    /// 沙盒模式（新版才有；預設關）。
    pub sandbox: bool,
    /// 這個團隊的沙盒配置（`None`＝沒開或準備失敗）。**一個團隊一個**，所有 agent 共用。
    pub sandbox_cfg: Option<crate::sandbox::Sandbox>,
    /// agent 實際的工作目錄＝信箱所在的地方（有沙盒＝worktree，否則＝[`Self::dir`]）。
    pub work_dir: String,
    /// 信箱監看（開組時建立、關組時丟掉）。
    pub bus: Option<super::bus::SharedBus>,
    /// 最後點過的那一格（分頁 id）：點分頁列那一列時回到它。
    pub last_focused: Option<u32>,
    /// 套用設定中：`tab_close` 不要逐格重排或拆組（做完再一起處理）。
    /// 舊版是 `MainWindow._suspendRelink`。
    pub suspend_relink: bool,
    /// 這一輪套用設定「剛啟動」的格號（通知 PM 時要排除它們——它們的角色檔是新讀的）。
    pub fresh: Vec<u32>,

    // ---- AI 聊天室（TASK-019；沿用同一個 Team／pane 排版，只是不走信箱投遞，
    //      改由 AwayTerminal 主持輪流發言，見 `super::chat`）----
    /// 這一組是代理團隊還是聊天室。
    pub kind: GroupKind,
    /// 討論回合（一回合＝每個人各發言一次）。
    pub rounds: u32,
    /// 這場討論的資料夾名（例 `20260927-1152`）。
    pub chat_folder: String,
    /// 使用者給的主題（還沒給＝空）。
    pub topic: String,
    /// 目前第幾回合（1 起）。
    pub round: u32,
    /// 這一回合輪到參加者清單裡的第幾位（0 起）。
    pub speaker: usize,
    pub phase: ChatPhase,
    /// 目前這一輪是什麼時候請他發言的（epoch ms；0＝還沒請）。
    pub turn_asked_ms: u128,
    /// 目前這一輪請的是哪一位（Agent ID）：等他發言期間名單若變了，靠這個找回他而不是靠索引。
    pub asked_agent_id: String,
    /// 這一輪什麼時候輪到他的（還沒開口問就開始算）：一直忙碌問不到也要逾時跳過。
    pub turn_started_ms: u128,
    /// 使用者按了「結束討論」：這一輪結束後就去寫結論。
    pub end_requested: bool,
}

impl Team {
    pub fn new(key: impl Into<String>, number: u32, dir: impl Into<String>) -> Self {
        let dir = dir.into();
        Self {
            key: key.into(),
            number,
            work_dir: dir.clone(),
            dir,
            title: String::new(),
            slots: (1..=MAX_SLOTS as u32).map(|i| Slot::new(number, i)).collect(),
            ratio: 0.5,
            paused: false,
            paused_by_limit: false,
            message_count: 0,
            max_messages: DEFAULT_MAX_MESSAGES,
            idle_check_minutes: DEFAULT_IDLE_CHECK_MINUTES,
            all_idle_since_ms: 0,
            idle_check_sent_ms: 0,
            idle_check_tail_pos: 0,
            busy_since_ms: 0,
            last_busy_ms: 0,
            delivery_seq: 0,
            sandbox: false,
            sandbox_cfg: None,
            bus: None,
            last_focused: None,
            suspend_relink: false,
            fresh: Vec::new(),
            kind: GroupKind::Team,
            rounds: DEFAULT_ROUNDS,
            chat_folder: String::new(),
            topic: String::new(),
            round: 1,
            speaker: 0,
            phase: ChatPhase::NeedTopic,
            turn_asked_ms: 0,
            asked_agent_id: String::new(),
            turn_started_ms: 0,
            end_requested: false,
        }
    }

    pub fn is_chat(&self) -> bool {
        self.kind == GroupKind::Chat
    }

    /// 聊天室的主持人＝**格 1**（角色固定主持人），而且有分頁。
    ///
    /// 不能用「格號最小、還開著的那格」：主持人分頁關掉後會變成第 2 位（普通參加者）
    /// 被當主持人去寫結論。格 1 不在＝沒有主持人（`None`）。
    pub fn host(&self) -> Option<&Slot> {
        self.running().find(|s| s.index == 1)
    }

    /// 已啟動（有分頁）的格，依格號排序。
    pub fn running(&self) -> impl Iterator<Item = &Slot> {
        self.slots.iter().filter(|s| s.tab.is_some())
    }

    pub fn running_mut(&mut self) -> impl Iterator<Item = &mut Slot> {
        self.slots.iter_mut().filter(|s| s.tab.is_some())
    }

    /// 代表列＝格號最小、有分頁的那格（通常是格 1）。
    pub fn row_tab(&self) -> Option<u32> {
        self.running().next().and_then(|s| s.tab)
    }

    /// 依 Agent ID 找格。
    pub fn slot_by_id(&self, agent_id: &str) -> Option<&Slot> {
        self.slots
            .iter()
            .find(|s| s.agent_id().eq_ignore_ascii_case(agent_id))
    }

    pub fn slot_by_id_mut(&mut self, agent_id: &str) -> Option<&mut Slot> {
        self.slots
            .iter_mut()
            .find(|s| s.agent_id().eq_ignore_ascii_case(agent_id))
    }

    /// 依分頁 id 找格。
    pub fn slot_by_tab(&self, tab: u32) -> Option<&Slot> {
        self.slots.iter().find(|s| s.tab == Some(tab))
    }

    /// 已經投遞到上限（不限＝永遠 false）。
    pub fn limit_reached(&self) -> bool {
        self.max_messages > 0 && self.message_count >= self.max_messages
    }

    /// 上限的顯示文字（數字或 ∞）。
    pub fn limit_text(&self) -> String {
        if self.max_messages > 0 {
            self.max_messages.to_string()
        } else {
            "∞".to_string()
        }
    }

    /// 還沒送出的信。
    pub fn pending_count(&self) -> usize {
        self.slots.iter().map(|s| s.queue.len()).sum()
    }

    /// 這個 ID 屬於本組嗎（`Agent-{組號}{格號}`，照舊版 `Mine`：前綴＋剛好一位）。
    pub fn owns_id(&self, id: &str) -> bool {
        let prefix = format!("Agent-{}", self.number);
        id.len() == prefix.len() + 1 && id.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase())
    }

    /// 啟用中的格（設定視窗勾了的）。
    pub fn enabled(&self) -> impl Iterator<Item = &Slot> {
        self.slots.iter().filter(|s| s.enabled)
    }
}

/// 上下列比例的合法範圍（照舊版 `ClampRatio`）。
pub fn clamp_ratio(r: f64) -> f64 {
    if r.is_nan() {
        0.5
    } else {
        r.clamp(0.15, 0.85)
    }
}

/// 目前開著的組沒用到的最小組號（1～9；全滿回 0）。`preferred` 沒被占用就優先用它。
pub fn next_free_number(open: &[u32], preferred: u32) -> u32 {
    if (1..=MAX_TEAMS).contains(&preferred) && !open.contains(&preferred) {
        return preferred;
    }
    (1..=MAX_TEAMS).find(|n| !open.contains(n)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_ids_and_colors_follow_v1() {
        let t = Team::new("k", 1, "C:\\p");
        assert_eq!(t.slots[0].agent_id(), "Agent-11");
        assert_eq!(t.slots[3].agent_id(), "Agent-14");
        assert_eq!(t.slots[0].color(), "#EF9A9A");
        assert_eq!(t.slots[1].color(), "#90CAF9");
        assert_eq!(t.slots[2].color(), "#A5D6A7");
        assert_eq!(t.slots[3].color(), "#CE93D8");
        let mut s = Slot::new(1, 2);
        s.role_title = "Software Engineer".to_string();
        s.backend = "codex".to_string();
        assert_eq!(s.label(), "Agent-12 · Software Engineer · Codex");
        assert_eq!(s.short_label(), "Agent-12 Software Engineer");
    }

    /// 組號：最小空號；`preferred` 沒被占用就用它；全滿回 0。
    #[test]
    fn picks_free_team_numbers() {
        assert_eq!(next_free_number(&[], 0), 1);
        assert_eq!(next_free_number(&[1, 2], 0), 3);
        assert_eq!(next_free_number(&[1, 3], 0), 2);
        assert_eq!(next_free_number(&[1, 2], 5), 5, "preferred 沒被占用就用它");
        assert_eq!(next_free_number(&[1, 2, 5], 5), 3, "preferred 被占用 → 最小空號");
        assert_eq!(next_free_number(&[1, 2, 3, 4, 5, 6, 7, 8, 9], 0), 0, "全滿");
    }

    /// 「這個 ID 是本組的嗎」：`Agent-11`／`Agent-12` 是組 1 的，`Agent-21` 不是。
    #[test]
    fn recognises_own_agent_ids() {
        let t = Team::new("k", 1, "C:\\p");
        assert!(t.owns_id("Agent-11"));
        assert!(t.owns_id("agent-14"));
        assert!(!t.owns_id("Agent-21"));
        assert!(!t.owns_id("Agent-1"), "少一位");
        assert!(!t.owns_id("Agent-111"), "多一位");
        assert!(!t.owns_id("AwayTerminal"));
    }

    /// 上限與暫停的判斷。
    #[test]
    fn tracks_the_delivery_limit() {
        let mut t = Team::new("k", 1, "C:\\p");
        assert_eq!(t.max_messages, 50);
        t.message_count = 49;
        assert!(!t.limit_reached());
        t.message_count = 50;
        assert!(t.limit_reached());
        t.max_messages = 0;
        assert!(!t.limit_reached(), "0＝不限");
        assert_eq!(t.limit_text(), "∞");
    }

    #[test]
    fn clamps_ratio() {
        assert_eq!(clamp_ratio(0.5), 0.5);
        assert_eq!(clamp_ratio(0.0), 0.15);
        assert_eq!(clamp_ratio(1.0), 0.85);
        assert_eq!(clamp_ratio(f64::NAN), 0.5);
    }

    /// 套用設定的四種結果（舊版 `ActionOf` 的真值表）。
    #[test]
    fn decides_what_to_do_with_each_slot() {
        let mut s = Slot::new(1, 2);
        s.backend = "claude-code".to_string();
        s.role = "software-engineer".to_string();

        // 還沒啟動
        assert_eq!(s.action_for(true, "claude-code", "software-engineer", false), SlotAction::Start);
        assert_eq!(s.action_for(false, "claude-code", "software-engineer", false), SlotAction::None);

        // 已經在跑
        s.tab = Some(7);
        assert_eq!(s.action_for(true, "claude-code", "software-engineer", false), SlotAction::None);
        assert_eq!(s.action_for(false, "", "", false), SlotAction::Close);
        // 換 CLI 或換角色都要重開（角色是啟動時注入的）
        assert_eq!(s.action_for(true, "codex", "software-engineer", false), SlotAction::Restart);
        assert_eq!(s.action_for(true, "claude-code", "qa-engineer", false), SlotAction::Restart);
        // 什麼都沒變但按了「重新啟動」
        assert_eq!(s.action_for(true, "claude-code", "software-engineer", true), SlotAction::Restart);
        // 大小寫不算變
        assert_eq!(s.action_for(true, "Claude-Code", "Software-Engineer", false), SlotAction::None);
    }

    /// 格 1 不能關（勾選是停用的，所以「不啟用」也當成啟用）。
    #[test]
    fn slot_one_can_never_be_closed() {
        let mut s = Slot::new(1, 1);
        s.backend = "claude-code".to_string();
        s.tab = Some(7);
        assert_eq!(s.action_for(false, "claude-code", "", false), SlotAction::None);
        s.tab = None;
        assert_eq!(s.action_for(false, "claude-code", "", false), SlotAction::Start);
    }

    /// 恢復時的組號：上次的沒被占用就沿用（Agent ID 才不會變）。
    #[test]
    fn restore_prefers_the_previous_group_number() {
        assert_eq!(next_free_number(&[1, 3], 3), 2, "上次的 3 被占用了 → 最小空號");
        assert_eq!(next_free_number(&[2], 3), 3, "上次的 3 沒被占用 → 沿用");
        assert_eq!(next_free_number(&[], 7), 7);
    }

    /// 代表列＝最小格號、有分頁的那格。
    #[test]
    fn row_tab_is_the_lowest_running_slot() {
        let mut t = Team::new("k", 2, "C:\\p");
        assert_eq!(t.row_tab(), None);
        t.slots[1].tab = Some(7);
        t.slots[2].tab = Some(8);
        assert_eq!(t.row_tab(), Some(7));
        t.slots[0].tab = Some(9);
        assert_eq!(t.row_tab(), Some(9), "格 1 啟動之後它才是代表列");
        assert_eq!(t.slot_by_tab(8).unwrap().agent_id(), "Agent-23");
    }

    /// 主持人固定是格 1：格 1 的分頁關掉後不能換成第 2 位（G4）。
    #[test]
    fn host_is_always_slot_one() {
        let mut t = Team::new("k", 1, "C:\\p");
        t.slots[0].tab = Some(5);
        t.slots[1].tab = Some(6);
        assert_eq!(t.host().map(|s| s.index), Some(1));
        t.slots[0].tab = None;
        assert!(t.host().is_none(), "格 1 不在＝沒有主持人，不是由格 2 頂上");
    }
}
