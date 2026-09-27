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
//! | 投遞上限 | 預設 30，可選 10／30／50／100／0（不限） | 同 |
//! | 閒置檢查 | 預設 30 分鐘，可選 15／30／60／0（不檢查） | 同 |
//! | 上下列比例 | 0.15～0.85，預設 0.5 | 同（[`clamp_ratio`]） |

use std::collections::VecDeque;

use super::message::AgentMessage;

/// 投遞上限的預設值（照舊版 `DefaultMaxMessages`）。
pub const DEFAULT_MAX_MESSAGES: u32 = 30;
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
    /// 本次執行內的投遞序號（「訊息 #n」用，從 1 起）。
    pub delivery_seq: u32,
    /// 沙盒模式（新版才有；預設開）。
    pub sandbox: bool,
    /// 這個團隊的沙盒配置（`None`＝沒開或準備失敗）。**一個團隊一個**，所有 agent 共用。
    pub sandbox_cfg: Option<crate::sandbox::Sandbox>,
    /// agent 實際的工作目錄＝信箱所在的地方（有沙盒＝worktree，否則＝[`Self::dir`]）。
    pub work_dir: String,
    /// 信箱監看（開組時建立、關組時丟掉）。
    pub bus: Option<super::bus::SharedBus>,
    /// 最後點過的那一格（分頁 id）：點分頁列那一列時回到它。
    pub last_focused: Option<u32>,
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
            delivery_seq: 0,
            sandbox: true,
            sandbox_cfg: None,
            bus: None,
            last_focused: None,
        }
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
        assert_eq!(t.max_messages, 30);
        assert!(!t.limit_reached());
        t.message_count = 30;
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
}
