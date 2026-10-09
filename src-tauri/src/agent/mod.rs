//! 代理團隊（Multi-Agent）：2～4 個互動 AI CLI 各帶角色分工，用專案裡的 `.ai/bus/`
//! 資料夾互寄信（搬移舊版 `Services/MultiAgent/`、`Models/Agent*`、`MainWindow.MultiAgent.cs`）。
//!
//! | 檔案 | 舊版 | 做什麼 |
//! |---|---|---|
//! | [`message`] | `Models/AgentMessage.cs` | 信件格式（檔名編號、front matter、寬鬆解析） |
//! | [`bus`] | `Services/MultiAgent/MessageBus.cs` | 監看信箱、`.delivered` 紀錄、AwayTerminal 自己寄信 |
//! | [`roles`] | `Services/MultiAgent/RoleLibrary.cs` | 三層角色檔＋**執行期脈絡**（逐字照舊版模板） |
//! | [`adapters`] | `Services/MultiAgent/*Adapter.cs` | 四家 CLI 的啟動參數差異 |
//! | [`team`] | `Models/AgentGroup.cs`／`AgentSlot.cs` | 組／格的狀態 |
//! | [`deliver`] | `MainWindow.MultiAgent.cs` | 600ms tick：閒置判定、投遞、節流、停止 |
//! | 這裡 | 同上 | [`TeamManager`]（tauri `State`）與所有 command |
//!
//! ## 一個團隊＝N 個獨立分頁
//! 刻意沿用舊版的做法：**不另做「一個分頁多個 session」的模型**。每個 agent 就是一個一般
//! 分頁（session／xterm／log／恢復畫面都照一般分頁走），綁組只是薄薄一層——前端用 `g`
//! 協定把它們的 pane 排成「下一上 N−1」，分頁列只顯示代表列那一列。
//!
//! ## 為什麼建團隊要前端配合
//! 每個 agent 的 PTY 輸出走各自的 tauri `Channel`，而 `Channel` 只能由前端建。所以流程是：
//! `team_create`（Rust 決定組號、沙盒、角色檔、每格的連線與參數）→ 前端依回傳的計畫逐格
//! `session_create(kind="agent")` → `team_ready`（Rust 綁組、送 `g`、開始監看信箱）。

pub mod adapters;
pub mod bus;
pub mod chat;
pub mod deliver;
pub mod message;
pub mod models;
pub mod roles;
pub mod team;

use std::sync::{Arc, Mutex, MutexGuard};

use tauri::{AppHandle, Manager, State};

use crate::settings::SettingsStore;
use crate::tabs::TabManager;
use team::{Slot, Team};

/// 所有開著的團隊。放在 tauri `State` 裡。
#[derive(Default)]
pub struct TeamManager {
    inner: Mutex<Vec<Team>>,
    /// 上一次送出的 `agent-state`（JSON）：內容沒變就不送（G7）。
    /// 600ms tick 每次都叫 `post_state`，無條件送的話分頁列每 600ms 整個重建——
    /// 拖曳中的列失效、tooltip 一直重置。前端開起來時自己 `agent_teams` 拿一次，不靠這個事件。
    last_posted: Mutex<Option<String>>,
}

impl TeamManager {
    pub fn lock(&self) -> MutexGuard<'_, Vec<Team>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// 這個分頁屬於哪個團隊的哪一格（`(團隊 key, 格號)`）。
    pub fn find_tab(&self, tab: u32) -> Option<(String, u32)> {
        self.lock().iter().find_map(|t| {
            t.slot_by_tab(tab)
                .map(|s| (t.key.clone(), s.index))
        })
    }

    /// 分頁列要不要列這個分頁：一般分頁都列；代理團隊只列那一組的代表列
    /// （舊版 `IsStripRow`）。
    pub fn is_strip_row(&self, tab: u32) -> bool {
        match self.lock().iter().find(|t| t.slot_by_tab(tab).is_some()) {
            Some(team) => team.row_tab() == Some(tab),
            None => true,
        }
    }

    /// Telegram 遠端列這個分頁時要顯示的名字（舊版 `RemoteTitle`）：
    /// 代理團隊的一格＝`組名（代理團隊 Agent-12）`，一般分頁回 `None`（用分頁自己的標題）。
    pub fn remote_title(&self, tab: u32) -> Option<String> {
        self.lock().iter().find_map(|t| {
            let slot = t.slot_by_tab(tab)?;
            let kind = crate::i18n::t(if t.is_chat() { "chat.title" } else { "ma.title" });
            Some(format!("{}（{} {}）", t.title, kind, slot.agent_id()))
        })
    }

    /// 點分頁列那一列、套用設定收尾時要切到哪個分頁：代理團隊／聊天室＝**一律回第 1 格**
    ///（Agent-x1，代表列；2.1.2 使用者指定）。以前是回「最後點過的那一格」（舊版
    /// `FocusTargetOf`），結果常常以為在對 Agent-11 下指令，焦點其實留在 Agent-12。
    pub fn focus_target(&self, tab: u32) -> u32 {
        let list = self.lock();
        list.iter()
            .find(|t| t.slot_by_tab(tab).is_some())
            .and_then(|t| t.row_tab())
            .unwrap_or(tab)
    }
}

/// 建團隊視窗送過來的一格。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotSetup {
    pub enabled: bool,
    /// CLI 種類（`claude-code`／`codex`／`opencode`／`geminicli`）。
    pub backend: String,
    /// 角色檔名（空＝None）。
    pub role: String,
    /// 這一格用的模型（空＝預設，不加 `--model`）。2.0.2 新增。
    #[serde(default)]
    pub model: String,
    /// 使用者按了「重新啟動」（設定沒變也要重開；舊版 `WantRestart`）。只有既有團隊用得到。
    #[serde(default)]
    pub restart: bool,
}

/// 建團隊視窗的全部欄位（舊版 `MultiAgentSetup`，多一個 `sandbox`）。
///
/// 「我的最愛」存的也是這一份（舊版 `FavoriteItem.TeamSetup`），所以要能序列化。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSetup {
    /// 專案資料夾（所有 agent 共用）。
    pub dir: String,
    /// 分頁列那一列的標題（空＝用資料夾名）。
    #[serde(default)]
    pub title: String,
    pub slots: Vec<SlotSetup>,
    /// 投遞上限（0＝不限）。
    #[serde(default = "default_max")]
    pub max_messages: u32,
    /// 閒置檢查分鐘數（0＝不檢查）。
    #[serde(default = "default_idle")]
    pub idle_check_minutes: u32,
    /// **沙盒模式**（新增，`CLAUDE.md`「新增功能」一節；**預設關**，2026-10-02 使用者改的）。
    #[serde(default)]
    pub sandbox: bool,
    /// 這一組是代理團隊還是 AI 聊天室（省略＝代理團隊）。
    #[serde(default)]
    pub kind: team::GroupKind,
    /// 聊天室的討論回合（只有 `kind = "chat"` 看它）。
    #[serde(default = "default_rounds")]
    pub rounds: u32,
}

fn default_rounds() -> u32 {
    team::DEFAULT_ROUNDS
}

fn default_max() -> u32 {
    team::DEFAULT_MAX_MESSAGES
}
fn default_idle() -> u32 {
    team::DEFAULT_IDLE_CHECK_MINUTES
}

/// 一格的啟動計畫（前端照它呼叫 `session_create`）。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotPlan {
    pub index: u32,
    /// 恢復分頁時要倒回哪一筆的畫面（`restore_list` 的索引）。新開的團隊是 `None`。
    pub restore: Option<usize>,
    pub agent_id: String,
    pub role_title: String,
    pub backend: String,
    pub backend_name: String,
    /// pane 標題（`g` 協定用的那個）。
    pub label: String,
    pub color: String,
}

/// `team_create` 的回覆。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamPlan {
    pub key: String,
    pub number: u32,
    pub title: String,
    /// agent 實際的工作目錄（有沙盒＝worktree）。
    pub work_dir: String,
    pub slots: Vec<SlotPlan>,
    pub sandbox: Option<crate::sandbox::Sandbox>,
}

/// 一個團隊的狀態（給前端畫分頁列那一列與右鍵選單）。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamView {
    pub key: String,
    pub number: u32,
    pub title: String,
    pub dir: String,
    pub row_tab: Option<u32>,
    pub ratio: f64,
    pub paused: bool,
    pub paused_by_limit: bool,
    pub message_count: u32,
    pub max_messages: u32,
    pub idle_check_minutes: u32,
    pub pending: u32,
    pub sandbox: bool,
    /// 最後點過的那一格（分頁列點這一列時回到它）。
    pub last_focused: Option<u32>,
    /// 這一組是代理團隊還是 AI 聊天室。
    pub kind: team::GroupKind,
    // ---- 只有聊天室看得到的欄位 ----
    pub rounds: u32,
    pub round: u32,
    pub phase: team::ChatPhase,
    pub topic: String,
    pub chat_folder: String,
    /// 分頁列那一列的狀態文字（等主題／討論中第 n/N 回合／寫結論／已結束）。
    pub chat_status: String,
    pub agents: Vec<AgentView>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub index: u32,
    pub agent_id: String,
    pub role_title: String,
    pub backend: String,
    pub backend_name: String,
    /// 這一格用的模型（空＝預設）。
    pub model: String,
    pub tab: Option<u32>,
    pub queued: u32,
    pub color: String,
}

fn view_of(t: &Team) -> TeamView {
    TeamView {
        key: t.key.clone(),
        number: t.number,
        title: t.title.clone(),
        dir: t.dir.clone(),
        row_tab: t.row_tab(),
        ratio: t.ratio,
        paused: t.paused,
        paused_by_limit: t.paused_by_limit,
        message_count: t.message_count,
        max_messages: t.max_messages,
        idle_check_minutes: t.idle_check_minutes,
        pending: t.pending_count() as u32,
        sandbox: t.sandbox_cfg.is_some(),
        last_focused: t.last_focused.filter(|id| t.slot_by_tab(*id).is_some()),
        kind: t.kind,
        rounds: t.rounds,
        round: t.round,
        phase: t.phase,
        topic: t.topic.clone(),
        chat_folder: t.chat_folder.clone(),
        chat_status: if t.is_chat() {
            chat::status_text(t)
        } else {
            String::new()
        },
        agents: t
            .slots
            .iter()
            .filter(|s| s.enabled || s.tab.is_some())
            .map(|s| AgentView {
                index: s.index,
                agent_id: s.agent_id(),
                role_title: s.role_title.clone(),
                backend: s.backend.clone(),
                backend_name: s.backend_name(),
                model: s.model.clone(),
                tab: s.tab,
                queued: s.queue.len() as u32,
                color: s.color().to_string(),
            })
            .collect(),
    }
}

/// 送 pane 的狀態標籤（`E` 協定）並把團隊狀態送給前端（`agent-state` event）。
///
/// 標籤：0 閒置、1 忙碌、2 有信待投遞、3 已結束、4 忙碌且有信待投遞。
/// **4 是舊版使用者回報後補的**：信只在閒置時投遞，原本忙碌會蓋掉「有信待送」，
/// PM 寄了暫停信、對方沒停也看不出信還在排隊。
pub fn post_state(app: &AppHandle, teams: &Arc<TeamManager>) {
    let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
        return;
    };
    let sessions = app.try_state::<crate::session::SessionManager>();
    let mut posts: Vec<String> = Vec::new();
    let views: Vec<TeamView> = {
        let mut list = teams.lock();
        for team in list.iter_mut() {
            for s in team.slots.iter_mut() {
                let Some(id) = s.tab else { continue };
                let queued = !s.queue.is_empty();
                let alive = sessions.as_ref().is_some_and(|m| m.get(id).is_some());
                let busy = tabs.agent_signals(id).map(|g| g.busy).unwrap_or(false);
                let st = if !alive {
                    3
                } else if busy {
                    if queued { 4 } else { 1 }
                } else if queued {
                    2
                } else {
                    0
                };
                if s.posted_state == Some(st) {
                    continue;
                }
                s.posted_state = Some(st);
                posts.push(format!("E{id}\x1f{st}"));
            }
        }
        list.iter().map(view_of).collect()
    };
    for p in posts {
        crate::host::emit_host(app, p);
    }
    // 內容沒變就不送（G7）
    let Ok(json) = serde_json::to_string(&views) else {
        return;
    };
    {
        let mut last = teams.last_posted.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_deref() == Some(json.as_str()) {
            return;
        }
        *last = Some(json);
    }
    use tauri::Emitter;
    let _ = app.emit("agent-state", &views);
}

/// 綁組／重綁（舊版 `LinkAgentGroup` ＋ `PostAgentGroup`）：標題（代表列＝組名、其餘＝Agent ID）、
/// 前端排版（`g` 協定）。
///
/// `g{下方 pane id}US{上列比例}US{上列 id,…}US{標籤|…}US{外框顏色,…}`
/// （標籤與顏色的順序＝下方、上列由左到右）
fn link(app: &AppHandle, teams: &Arc<TeamManager>, key: &str) {
    let (msgs, g) = {
        let list = teams.lock();
        let Some(team) = list.iter().find(|t| t.key == key) else {
            return;
        };
        let run: Vec<&Slot> = team.running().collect();
        if run.is_empty() {
            return;
        }
        let mut msgs = Vec::new();
        if let Some(tabs) = app.try_state::<Arc<TabManager>>() {
            for (i, s) in run.iter().enumerate() {
                let want = if i == 0 { team.title.clone() } else { s.agent_id() };
                let id = s.tab.unwrap();
                if tabs.set_title(id, &want, false) {
                    msgs.push(format!("t{id}\x1f{want}"));
                }
            }
        }
        let g = format!(
            "g{}\x1f{}\x1f{}\x1f{}\x1f{}",
            run[0].tab.unwrap(),
            format_ratio(team.ratio),
            run.iter()
                .skip(1)
                .map(|s| s.tab.unwrap().to_string())
                .collect::<Vec<_>>()
                .join(","),
            run.iter()
                .map(|s| s.label().replace('|', "/").replace('\x1f', " "))
                .collect::<Vec<_>>()
                .join("|"),
            run.iter().map(|s| s.color()).collect::<Vec<_>>().join(","),
        );
        (msgs, g)
    };
    let renamed = !msgs.is_empty();
    for m in msgs {
        crate::host::emit_host(app, m);
    }
    crate::host::emit_host(app, g);
    // 標題變了要讓**分頁列**也重畫：`t{id}` 只更新 terminal.js 的 pane 標題，
    // 我們的分頁列讀的是 `tab-state` event（改名之後沒送＝那一列還是舊名字，--verify 抓到）
    if renamed {
        if let Some(tabs) = app.try_state::<Arc<TabManager>>() {
            crate::tabs::emit_state(app, &tabs);
        }
    }
    // 重排後 pane 的狀態標籤要重送
    {
        let mut list = teams.lock();
        if let Some(team) = list.iter_mut().find(|t| t.key == key) {
            for s in team.slots.iter_mut() {
                s.posted_state = None;
            }
        }
    }
    post_state(app, teams);
}

/// 舊版 `Ratio.ToString("0.###", InvariantCulture)`。
fn format_ratio(r: f64) -> String {
    let s = format!("{:.3}", r);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() { "0".to_string() } else { s.to_string() }
}

/// 一格要怎麼啟動（`session_create(kind="agent")` 用）。
pub struct LaunchedSlot {
    pub agent_id: String,
    /// 要跑哪一支 CLI（使用者的自訂連線優先）。
    pub conn: crate::settings::CustomConn,
    /// 附加在連線參數後面的（adapter 的 ＋ 沙盒的 `--sandbox`）。
    pub extra_args: String,
    /// 團隊的專案資料夾（**沙盒之前**的那一個；恢復分頁要存這個）。
    pub dir: String,
    /// 實際的工作目錄（有沙盒＝worktree）。
    pub work_dir: String,
    /// 團隊的沙盒配置（環境變數與 Job Object 照它走）。
    pub sandbox: Option<crate::sandbox::Sandbox>,
    /// 角色要靠打字注入的那一句（`None`＝走啟動參數，一開始就算注入完成）。
    pub first_message: Option<String>,
    pub via_ps: bool,
    /// 這一格用的模型（空＝預設）。記在分頁上，恢復分頁與我的最愛要存。
    pub model: String,
}

/// 查一格的啟動方式。找不到 CLI 就回錯誤（呼叫端會 `agent_slot_failed`）。
pub fn plan_slot(
    settings: &Arc<SettingsStore>,
    teams: &Arc<TeamManager>,
    key: &str,
    index: u32,
) -> Result<LaunchedSlot, String> {
    let list = teams.lock();
    let team = list
        .iter()
        .find(|t| t.key == key)
        .ok_or_else(|| crate::i18n::t("ma.openFail"))?;
    let slot = team
        .slots
        .iter()
        .find(|s| s.index == index)
        .ok_or_else(|| crate::i18n::t("ma.openFail"))?;
    let backend = adapters::Backend::by_key(&slot.backend)
        .ok_or_else(|| crate::i18n::tf("ma.dlgNeedBackend", &[&slot.agent_id()]))?;
    // 恢復分頁：優先沿用上次那一格的執行檔／參數（舊版 `LaunchSlot` 的 `saved` 分支）。
    // 舊版的條件照抄：**絕對路徑一律要存在**（npm 版的 .cmd 也一樣——CLI 移除了還開
    // PowerShell 分頁跑不存在的 .cmd，那一格會顯示「執行中」、信打進 shell），
    // 只有「靠 PATH 找」的裸名才交給 PowerShell 解析。
    let saved = slot.saved_conn.as_ref().filter(|c| {
        let p = std::path::Path::new(&c.path);
        if c.path.trim().is_empty() {
            false
        } else if p.is_absolute() {
            p.is_file()
        } else {
            c.via_powershell
        }
    });
    let mut conn = match saved {
        Some(c) => c.clone(),
        None => adapters::resolve(settings, backend).ok_or_else(|| {
            crate::i18n::tf(
                "ma.backendMissing",
                &[backend.display_name(), &slot.agent_id()],
            )
        })?,
    };
    // 這一格選了模型：連線參數裡使用者自己寫的 `--model` 先拿掉（重複給 Codex 會報錯），
    // 再把選的接在最後面。選「預設」（空）就完全不動連線的參數。
    let model = slot.model.trim().to_string();
    let model_extra = models::model_arg(&model);
    if !model_extra.is_empty() {
        conn.args = models::strip_model_arg(&conn.args);
    }
    let role_text = std::fs::read_to_string(&slot.role_file).unwrap_or_default();
    let launch = adapters::build_launch(
        backend,
        &conn,
        &slot.agent_id(),
        &slot.role_title,
        &slot.role_file,
        &role_text,
    );
    // 沙盒的 `--sandbox`（Codex／Gemini）接在 adapter 的參數後面
    let sb_extra = team
        .sandbox_cfg
        .as_ref()
        .map(|_| crate::sandbox::extra_args(&conn.path))
        .unwrap_or("");
    let via_ps = conn.via_powershell
        || conn.path.to_ascii_lowercase().ends_with(".cmd")
        || conn.path.to_ascii_lowercase().ends_with(".bat");
    Ok(LaunchedSlot {
        agent_id: slot.agent_id(),
        extra_args: format!("{}{model_extra}{sb_extra}", launch.extra_args),
        model: if model_extra.is_empty() { String::new() } else { model },
        dir: team.dir.clone(),
        work_dir: team.work_dir.clone(),
        // 團隊的沙盒是用空的 tool_path 準備的（一組多家 CLI），`guard_warning` 永遠是空的 →
        // 依**這一格**實際跑的 CLI 補上（Claude Code 找不到 node＝護欄 hook 不會生效；D15），
        // 和單一連線走 `sandbox::prepare(…, tool_path)` 的結果一致
        sandbox: team.sandbox_cfg.clone().map(|mut sb| {
            sb.guard_warning = crate::sandbox::guard_warning(&conn.path);
            sb
        }),
        first_message: launch.first_message,
        via_ps,
        conn,
    })
}

/// 這一格的分頁開起來了（`session_create` 成功之後叫）。
pub fn slot_started(
    teams: &Arc<TeamManager>,
    key: &str,
    index: u32,
    tab: u32,
    plan: &LaunchedSlot,
) {
    let mut list = teams.lock();
    let Some(team) = list.iter_mut().find(|t| t.key == key) else {
        return;
    };
    let Some(s) = team.slots.iter_mut().find(|s| s.index == index) else {
        return;
    };
    s.tab = Some(tab);
    // **這次啟動**的時間。不能用分頁的開啟時間——恢復分頁時那是原始時間（舊版註解）
    s.launched_ms = crate::tabs::now_ms() as u128;
    s.via_ps = plan.via_ps;
    s.pending_first_message = plan.first_message.clone();
    s.role_injected = plan.first_message.is_none();
    s.last_delivered_ms = 0;
    s.delivery_checked = true;
    s.posted_state = None;
    println!(
        "[AwayTerminal] 代理團隊 {}：{} 啟動（分頁 {tab}、{}、角色注入={}）",
        team.number,
        s.agent_id(),
        plan.conn.path,
        if plan.first_message.is_none() { "旗標" } else { "打字" }
    );
}

// ---------------------------------------------------------------- command

/// 這一組用哪一套角色庫（代理團隊／AI 聊天室）。
fn library_for(kind: team::GroupKind) -> &'static roles::Library {
    match kind {
        team::GroupKind::Chat => &roles::CHAT,
        team::GroupKind::Team => &roles::TEAM,
    }
}

/// 設定視窗要的清單：有裝哪幾家 CLI、有哪些角色。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupOptions {
    pub backends: Vec<BackendOption>,
    pub roles: Vec<roles::RoleInfo>,
    /// 格 1～4 的預設角色。
    pub default_roles: Vec<String>,
    pub limit_choices: Vec<u32>,
    pub idle_choices: Vec<u32>,
    pub default_max_messages: u32,
    pub default_idle_check: u32,
    /// 聊天室的討論回合選項與預設值。
    pub round_choices: Vec<u32>,
    pub default_rounds: u32,
    /// 聊天室第 1 位固定的角色（設定視窗要把那一格的角色下拉停用）。
    pub host_role: String,
    /// 已經開了幾組（9 組滿了前端要擋）。
    pub open_teams: u32,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendOption {
    pub key: String,
    pub name: String,
    /// 找到的執行檔（前端只顯示，讓使用者知道會跑哪一支）。
    pub path: String,
    pub via_powershell: bool,
    /// 這家 CLI 上次選的模型（新開的格預選它；空＝預設）。
    pub last_model: String,
}

#[tauri::command]
pub fn agent_setup_options(
    kind: Option<team::GroupKind>,
    settings: State<'_, Arc<SettingsStore>>,
    teams: State<'_, Arc<TeamManager>>,
) -> SetupOptions {
    let kind = kind.unwrap_or_default();
    let lib = library_for(kind);
    let data_dir = roles::data_dir_or_verify(settings.dir());
    let last_models = settings.get().last_models;
    let backends = adapters::ALL_KEYS
        .iter()
        .filter_map(|k| {
            let b = adapters::Backend::by_key(k)?;
            let conn = adapters::resolve(&settings, b)?;
            Some(BackendOption {
                key: b.key().to_string(),
                name: b.display_name().to_string(),
                path: conn.path,
                via_powershell: conn.via_powershell,
                last_model: last_models.get(b.key()).cloned().unwrap_or_default(),
            })
        })
        .collect();
    SetupOptions {
        backends,
        roles: roles::list_roles(lib, &data_dir),
        default_roles: lib.default_slots.iter().map(|s| s.to_string()).collect(),
        round_choices: team::ROUND_CHOICES.to_vec(),
        default_rounds: team::DEFAULT_ROUNDS,
        host_role: roles::CHAT_HOST_ROLE.to_string(),
        limit_choices: team::LIMIT_CHOICES.to_vec(),
        idle_choices: team::IDLE_CHECK_CHOICES.to_vec(),
        default_max_messages: team::DEFAULT_MAX_MESSAGES,
        default_idle_check: team::DEFAULT_IDLE_CHECK_MINUTES,
        open_teams: teams.lock().len() as u32,
    }
}

/// 這台機器上找得到任何一家代理團隊用的 CLI 嗎（「新分頁 ▾」要決定「代理團隊…」「AI聊天室…」
/// 能不能選；2.0.7，使用者要求）。比 [`agent_setup_options`] 輕：不讀角色檔，找到第一家就停。
#[tauri::command]
pub fn agent_backends_any(settings: State<'_, Arc<SettingsStore>>) -> bool {
    adapters::ALL_KEYS
        .iter()
        .filter_map(|k| adapters::Backend::by_key(k))
        .any(|b| adapters::resolve(&settings, b).is_some())
}

/// 「還原角色檔預設」。
#[tauri::command]
pub fn agent_roles_restore(
    kind: Option<team::GroupKind>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Vec<roles::RoleInfo> {
    let lib = library_for(kind.unwrap_or_default());
    let dir = roles::data_dir_or_verify(settings.dir());
    roles::restore_defaults(lib, &dir);
    println!("[AwayTerminal] {}：角色檔已還原成預設", lib.dir_name);
    roles::list_roles(lib, &dir)
}

/// 「開啟角色檔資料夾」要開哪裡。
#[tauri::command]
pub fn agent_roles_dir(
    kind: Option<team::GroupKind>,
    settings: State<'_, Arc<SettingsStore>>,
) -> String {
    let lib = library_for(kind.unwrap_or_default());
    let dir = roles::data_dir_or_verify(settings.dir());
    roles::ensure_defaults(lib, &dir);
    roles::roles_dir(lib, &dir).to_string_lossy().to_string()
}

/// 「開啟訊息資料夾」要開哪裡（有沙盒就是 worktree 裡那一個）。
#[tauri::command]
pub fn agent_bus_dir(key: String, teams: State<'_, Arc<TeamManager>>) -> Option<String> {
    let list = teams.lock();
    let team = list.iter().find(|t| t.key == key)?;
    let dir = std::path::Path::new(&team.work_dir).join(".ai").join("bus");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.to_string_lossy().to_string())
}

/// [`create_team`] 的差異參數（新開的團隊全部用預設）。
#[derive(Default)]
pub struct CreateOpts {
    /// 沿用上次的代號（恢復分頁用）。
    pub key: Option<String>,
    /// 優先用這個組號（沒被占用才會用到；0＝不指定）。
    pub preferred_number: u32,
    pub ratio: f64,
    /// 標題已經被別的分頁用著也照用（恢復分頁時那個分頁就是自己）。
    pub title_may_exist: bool,
}

/// 建一個團隊：組號 → 沙盒 → `.gitignore` → 組合每格角色檔 → 回啟動計畫。
///
/// **這裡不啟動任何分頁**（PTY 的輸出 channel 只能由前端建）：前端拿到計畫後逐格呼叫
/// `session_create(kind="agent", agent={team,index})`，最後呼叫 [`agent_team_ready`]。
#[tauri::command]
pub fn agent_team_create(
    setup: TeamSetup,
    settings: State<'_, Arc<SettingsStore>>,
    teams: State<'_, Arc<TeamManager>>,
    tabs: State<'_, Arc<TabManager>>,
) -> Result<TeamPlan, String> {
    create_team(
        &settings,
        &teams,
        &tabs,
        setup,
        CreateOpts {
            ratio: 0.5,
            ..CreateOpts::default()
        },
    )
}

/// [`agent_team_create`] 與 [`agent_team_restore`] 的共同本體。
pub fn create_team(
    settings: &Arc<SettingsStore>,
    teams: &Arc<TeamManager>,
    tabs: &Arc<TabManager>,
    setup: TeamSetup,
    opts: CreateOpts,
) -> Result<TeamPlan, String> {
    let dir = setup.dir.trim();
    if dir.is_empty() || !std::path::Path::new(dir).is_dir() {
        return Err(crate::i18n::tf("ma.dlgFolderMissing", &[dir]));
    }
    let open: Vec<u32> = teams.lock().iter().map(|t| t.number).collect();
    let number = team::next_free_number(&open, opts.preferred_number);
    if number == 0 {
        return Err(crate::i18n::t("ma.tooMany"));
    }
    let data_dir = roles::data_dir_or_verify(settings.dir());
    let lib = library_for(setup.kind);
    let key = opts
        .key
        .clone()
        .unwrap_or_else(|| format!("{number}-{}", crate::tabs::now_ms()));
    let mut t = Team::new(key.clone(), number, dir);
    t.kind = setup.kind;
    t.rounds = setup.rounds.max(1);
    t.max_messages = setup.max_messages;
    t.idle_check_minutes = setup.idle_check_minutes;
    t.sandbox = setup.sandbox;
    t.ratio = team::clamp_ratio(if opts.ratio > 0.0 { opts.ratio } else { 0.5 });
    let want_title = setup.title.trim();
    t.title = if !want_title.is_empty() && (opts.title_may_exist || !tabs.title_taken(want_title)) {
        want_title.to_string()
    } else {
        tabs.dir_tab_name(
            dir,
            &crate::i18n::t(if t.is_chat() { "chat.title" } else { "ma.title" }),
        )
    };
    let is_chat = t.is_chat();
    for (i, s) in t.slots.iter_mut().enumerate() {
        let ss = setup.slots.get(i).cloned().unwrap_or_default();
        // 新開的組格 1 一定啟用（使用者就是要跟它說話；聊天室＝主持人）
        s.enabled = ss.enabled || i == 0;
        s.backend = ss.backend;
        s.model = ss.model.trim().to_string();
        // 聊天室的第 1 位固定主持人（舊版設定視窗的角色下拉是停用的）
        s.role = if is_chat && i == 0 {
            roles::CHAT_HOST_ROLE.to_string()
        } else {
            ss.role
        };
        s.role_title = roles::title_of(lib, &data_dir, &s.role);
    }
    // 聊天室至少要兩位參加者才討論得起來（舊版 `chat.dlgNeedTwo`）
    if is_chat && t.slots.iter().filter(|s| s.enabled).count() < 2 {
        return Err(crate::i18n::t("chat.dlgNeedTwo"));
    }
    for s in t.slots.iter().filter(|s| s.enabled) {
        if adapters::Backend::by_key(&s.backend).is_none() {
            return Err(crate::i18n::tf("ma.dlgNeedBackend", &[&s.agent_id()]));
        }
        // 模型名稱會原樣接在命令列上 → 不合法的字元在這裡就擋下來
        if !s.model.is_empty() && !models::valid_model(&s.model) {
            return Err(crate::i18n::t("model.invalid"));
        }
    }

    // ---- 沙盒：**一個團隊一個**（所有 agent 同一棵 worktree，信箱就在裡面）----
    if setup.sandbox {
        let base = std::path::PathBuf::from(dir);
        // 沙盒名用組名（中文 → `<sanitized>-<hash>`，由 sandbox::prepare 處理）
        match crate::sandbox::prepare(&base, &format!("team-{}", t.title), "") {
            Ok(sb) => {
                t.work_dir = sb.work_dir.clone();
                println!(
                    "[AwayTerminal] 代理團隊 {number} 沙盒：{} worktree={} 分支={}",
                    sb.root,
                    sb.has_worktree,
                    if sb.branch.is_empty() { "-" } else { &sb.branch }
                );
                t.sandbox_cfg = Some(sb);
            }
            Err(e) => println!("[AwayTerminal] 代理團隊沙盒準備失敗，這一組不進沙盒：{e}"),
        }
        // 護欄設定：每一家 CLI 各自的（`write_guardrails` 只對 Claude Code 有動作），
        // 在**團隊的 worktree 裡產生一次**
        let work = std::path::PathBuf::from(&t.work_dir);
        let mut done: Vec<String> = Vec::new();
        for s in t.slots.iter().filter(|s| s.enabled) {
            let Some(b) = adapters::Backend::by_key(&s.backend) else { continue };
            if done.contains(&b.key().to_string()) {
                continue;
            }
            done.push(b.key().to_string());
            if let Some(conn) = adapters::resolve(settings, b) {
                let files = crate::sandbox::write_guardrails(&work, &conn.path);
                // 護欄寫了但不會生效（Claude Code 沒有 node；D15）：記一筆，分頁 tooltip 由
                // `plan_slot` 帶的 `guard_warning` 顯示
                let warn = crate::sandbox::guard_warning(&conn.path);
                if !warn.is_empty() {
                    println!("[AwayTerminal] 代理團隊 {number} 護欄未生效（{}）：{warn}", b.display_name());
                }
                if !files.is_empty() {
                    println!(
                        "[AwayTerminal] 代理團隊 {number} 護欄（{}）：{}",
                        b.display_name(),
                        files.join(", ")
                    );
                }
            }
        }
    }

    // `.ai/` 底下同時放信箱與討論紀錄，所以兩種都要加進 .gitignore
    bus::ensure_gitignore(std::path::Path::new(&t.work_dir));
    roles::clear_session(lib, &data_dir, number);
    // 名單要完整才組得對（隊友清單是「已啟用」的格）→ 先全部填好再一次組
    for i in 0..t.slots.len() {
        if !t.slots[i].enabled {
            continue;
        }
        match roles::compose(lib, &data_dir, &t, t.slots[i].index) {
            Ok(p) => t.slots[i].role_file = p.to_string_lossy().to_string(),
            Err(e) => {
                return Err(crate::i18n::tf(
                    "ma.roleComposeFailed",
                    &[&t.slots[i].agent_id(), &e.to_string()],
                ))
            }
        }
    }

    let plan = TeamPlan {
        key: key.clone(),
        number,
        title: t.title.clone(),
        work_dir: t.work_dir.clone(),
        slots: t
            .slots
            .iter()
            .filter(|s| s.enabled)
            .map(|s| SlotPlan {
                index: s.index,
                restore: None,
                agent_id: s.agent_id(),
                role_title: s.role_title.clone(),
                backend: s.backend.clone(),
                backend_name: s.backend_name(),
                label: s.label(),
                color: s.color().to_string(),
            })
            .collect(),
        sandbox: t.sandbox_cfg.clone(),
    };
    println!(
        "[AwayTerminal] {} {number} 建立：目錄={} 工作區={} agents={}",
        if t.is_chat() { "聊天室" } else { "代理團隊" },
        t.dir,
        t.work_dir,
        plan.slots
            .iter()
            .map(|s| format!("{}:{}:{}", s.agent_id, s.backend, s.role_title))
            .collect::<Vec<_>>()
            .join(",")
    );
    teams.lock().push(t);
    Ok(plan)
}

/// 一格啟動失敗（CLI 找不到／被移除）：不能留在名單裡——角色檔、設定視窗都會以為他在。
/// 舊版 `LaunchSlot` 回 null 那條路。
#[tauri::command]
pub fn agent_slot_failed(
    key: String,
    index: u32,
    settings: State<'_, Arc<SettingsStore>>,
    teams: State<'_, Arc<TeamManager>>,
) {
    let data_dir = roles::data_dir_or_verify(settings.dir());
    let mut list = teams.lock();
    let Some(t) = list.iter_mut().find(|t| t.key == key) else {
        return;
    };
    if let Some(s) = t.slots.iter_mut().find(|s| s.index == index) {
        println!("[AwayTerminal] 代理團隊 {}：{} 啟動失敗", t.number, s.agent_id());
        s.enabled = false;
        s.tab = None;
    }
    // 隊友名單重組（打字注入的還沒讀、旗標注入的下次重啟會讀到）
    let indices: Vec<u32> = t.slots.iter().filter(|s| s.enabled).map(|s| s.index).collect();
    for i in indices {
        let _ = roles::compose(library_for(t.kind), &data_dir, t, i);
    }
}

/// 全部格都啟動完了：綁組、送 `g`、開始監看信箱（舊版 `LinkAgentGroup` ＋ `StartBus`）。
#[tauri::command]
pub fn agent_team_ready(
    app: AppHandle,
    key: String,
    teams: State<'_, Arc<TeamManager>>,
) -> Result<u32, String> {
    let (any, work_dir) = {
        let list = teams.lock();
        let Some(t) = list.iter().find(|t| t.key == key) else {
            return Err(crate::i18n::t("ma.openFail"));
        };
        (t.running().count(), t.work_dir.clone())
    };
    if any == 0 {
        teams.lock().retain(|t| t.key != key);
        return Err(crate::i18n::t("ma.openFail"));
    }
    {
        let mut list = teams.lock();
        // 聊天室不走信箱（由 AwayTerminal 主持輪流發言，見 `chat.rs`）：建了監看反而會把
        // agent 誤寫進 `.ai/bus/` 的檔排進一個永遠不投遞的佇列（G9）
        if let Some(t) = list.iter_mut().find(|t| t.key == key && !t.is_chat()) {
            let b: bus::SharedBus = Arc::new(bus::MessageBus::new(&work_dir));
            b.start();
            t.bus = Some(b);
        }
    }
    let arc = (*teams).clone();
    link(&app, &arc, &key);
    let row = {
        let list = teams.lock();
        list.iter().find(|t| t.key == key).and_then(|t| t.row_tab())
    };
    if let Some(id) = row {
        crate::host::emit_host(&app, format!("s{id}"));
    }
    let sep = std::path::MAIN_SEPARATOR;
    println!("[AwayTerminal] 代理團隊 {key}：{any} 個 agent 就緒，開始監看 {work_dir}{sep}.ai{sep}bus");
    Ok(any as u32)
}

/// 右鍵「投遞」→ 10／30／50／100／不限／暫停（舊版 `AgentDelivery_Click`）。
///
/// 選次數＝改成這個上限**並繼續投遞**（暫停中就解除、本輪計數歸零，補送暫停期間收到的信）；
/// 沒有暫停時只改上限、計數照舊。
#[tauri::command]
pub fn agent_delivery_set(
    app: AppHandle,
    key: String,
    limit: Option<u32>,
    teams: State<'_, Arc<TeamManager>>,
) {
    {
        let mut list = teams.lock();
        let Some(t) = list.iter_mut().find(|t| t.key == key) else {
            return;
        };
        match limit {
            None => {
                if t.paused {
                    return;
                }
                t.paused = true;
                t.paused_by_limit = false;
                println!(
                    "[AwayTerminal] 代理團隊 {}：使用者暫停投遞，待投遞 {}",
                    t.number,
                    t.pending_count()
                );
            }
            Some(max) => {
                t.max_messages = max;
                if t.paused {
                    t.paused = false;
                    t.paused_by_limit = false;
                    t.message_count = 0;
                }
                println!(
                    "[AwayTerminal] 代理團隊 {}：投遞上限={} 計數={} 待投遞={}",
                    t.number,
                    t.limit_text(),
                    t.message_count,
                    t.pending_count()
                );
            }
        }
    }
    let arc = (*teams).clone();
    post_state(&app, &arc);
}

/// 右鍵「停止任務」：Esc → 1 秒後 Ctrl+U → 1.5 秒時打停止句＋Enter。
#[tauri::command]
pub fn agent_stop(app: AppHandle, key: String, teams: State<'_, Arc<TeamManager>>) -> Vec<String> {
    let arc = (*teams).clone();
    deliver::stop_team(&app, &arc, &key)
}

/// 上下分隔線拖完的新比例（`G` 協定）。
#[tauri::command]
pub fn agent_ratio(bottom_tab: u32, ratio: f64, teams: State<'_, Arc<TeamManager>>) {
    let mut list = teams.lock();
    if let Some(t) = list
        .iter_mut()
        .find(|t| t.row_tab() == Some(bottom_tab) || t.slot_by_tab(bottom_tab).is_some())
    {
        t.ratio = team::clamp_ratio(ratio);
    }
}

/// 點了某一格（前端也可以直接叫；平常是 `pane_selected` 自己記）。
#[tauri::command]
pub fn agent_focused(tab: u32, teams: State<'_, Arc<TeamManager>>) {
    let arc = (*teams).clone();
    note_focus(&arc, tab);
}

/// 點了某一格（`pane_selected` 會叫）。舊版 `MarkActiveRow` 裡的那一行。
pub fn note_focus(teams: &Arc<TeamManager>, tab: u32) {
    let mut list = teams.lock();
    if let Some(t) = list.iter_mut().find(|t| t.slot_by_tab(tab).is_some()) {
        t.last_focused = Some(tab);
    }
}

/// 一格的分頁被關掉了（`tab_close` 會叫）：組裡還有別格＝重綁，沒有＝拆組。
pub fn tab_removed(app: &AppHandle, teams: &Arc<TeamManager>, tab: u32) {
    let key = {
        let mut list = teams.lock();
        let Some(t) = list.iter_mut().find(|x| x.slot_by_tab(tab).is_some()) else {
            return;
        };
        // 套用設定中：這一輪會關好幾格再開好幾格，中途不重排也不拆組
        //（舊版 `_suspendRelink`）。`agent_team_apply_done` 做完才一次處理。
        if t.suspend_relink {
            if let Some(s) = t.slots.iter_mut().find(|s| s.tab == Some(tab)) {
                s.tab = None;
                s.posted_state = None;
            }
            if t.last_focused == Some(tab) {
                t.last_focused = None;
            }
            return;
        }
        if let Some(s) = t.slots.iter_mut().find(|s| s.tab == Some(tab)) {
            s.tab = None;
            s.queue.clear();
            s.posted_state = None;
        }
        if t.last_focused == Some(tab) {
            t.last_focused = None;
        }
        t.key.clone()
    };
    let gone = {
        let list = teams.lock();
        list.iter()
            .find(|t| t.key == key)
            .map(|t| t.running().count() == 0)
            .unwrap_or(true)
    };
    if gone {
        disband(app, teams, &key);
    } else {
        link(app, teams, &key);
    }
}

/// 目前所有團隊的狀態（前端初始化時問一次）。
#[tauri::command]
pub fn agent_teams(teams: State<'_, Arc<TeamManager>>) -> Vec<TeamView> {
    teams.lock().iter().map(view_of).collect()
}

/// 關閉整組：要關哪些分頁（前端逐個呼叫 `tab_close`，關完呼叫 [`agent_team_gone`]）。
#[tauri::command]
pub fn agent_team_tabs(key: String, teams: State<'_, Arc<TeamManager>>) -> Vec<u32> {
    let list = teams.lock();
    match list.iter().find(|t| t.key == key) {
        Some(t) => t.running().filter_map(|s| s.tab).collect(),
        None => Vec::new(),
    }
}

/// 一格的分頁關掉了（`tab_close` 已經自己處理；這個留給前端的例外路徑）。
#[tauri::command]
pub fn agent_tab_closed(app: AppHandle, tab: u32, teams: State<'_, Arc<TeamManager>>) {
    let arc = (*teams).clone();
    tab_removed(&app, &arc, tab);
}

/// 整組解散：停止監看信箱、移出清單（舊版 `DisbandAgentGroup`）。
///
/// **`u` 協定的唯一呼叫者**：解散時如果還有分頁活著（例如整組關閉的流程中間出錯），
/// 要叫前端把外框拆掉、各格變回一般分頁——否則那些 pane 會卡在一個沒有團隊的
/// `.agents` 外框裡，還掛著 agent 標籤與狀態小標。
/// 舊版 JS 有 `ungroupAll()` 但 C# 端**從來沒有呼叫者**（和 `A` 全選一樣的情況，
/// 見 `docs/PROTOCOL.md`），這裡補上。正常關閉整組時每一格都已經 `x{id}` 掉了，
/// 走不到這條路。
fn disband(app: &AppHandle, teams: &Arc<TeamManager>, key: &str) {
    let mut list = teams.lock();
    if let Some(i) = list.iter().position(|t| t.key == key) {
        let t = list.remove(i);
        let orphan = t.running().filter_map(|s| s.tab).next();
        println!(
            "[AwayTerminal] 代理團隊 {} 關閉{}",
            t.number,
            match orphan {
                Some(id) => format!("（還有分頁活著 → 送 u{id} 拆掉外框）"),
                None => String::new(),
            }
        );
        if let Some(id) = orphan {
            drop(list);
            crate::host::emit_host(app, format!("u{id}"));
            post_state(app, teams);
            return;
        }
    }
    drop(list);
    post_state(app, teams);
}

/// 前端關完整組之後叫一次（分頁都已經 `tab_close` 掉了）。
#[tauri::command]
pub fn agent_team_gone(app: AppHandle, key: String, teams: State<'_, Arc<TeamManager>>) {
    let arc = (*teams).clone();
    disband(&app, &arc, &key);
}

/// 對**既有團隊**套用設定的結果（舊版 `ApplyAgentSetup`）。
///
/// 這裡不啟動任何分頁（PTY 的 channel 只能由前端建），所以回傳一份「要做什麼」的清單：
/// 前端先關 `close_tabs`、再依 `launch` 逐格 `session_create`、最後呼叫
/// [`agent_team_apply_done`]。整個過程中 `suspend_relink` 是 true，`tab_close` 不會重排。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPlan {
    /// 要關掉的分頁（由後往前關）。
    pub close_tabs: Vec<u32>,
    /// 要啟動的格。
    pub launch: Vec<SlotPlan>,
    /// 名單變了（做完要通知 PM 重讀角色檔）。
    pub roster_changed: bool,
    /// 有沒有任何實際變動（沒有＝前端可以什麼都不做）。
    pub changed: bool,
}

/// [`agent_team_apply`] 會改到的欄位的快照：中途失敗時整個還原（G6）。
/// `Team` 沒有 `Clone`（裡面有信箱監看），所以只存會被改的那幾個欄位。
struct ApplySnapshot {
    slots: Vec<Slot>,
    max_messages: u32,
    paused: bool,
    paused_by_limit: bool,
    idle_check_minutes: u32,
    all_idle_since_ms: u128,
    rounds: u32,
}

impl ApplySnapshot {
    fn of(t: &Team) -> Self {
        Self {
            slots: t.slots.clone(),
            max_messages: t.max_messages,
            paused: t.paused,
            paused_by_limit: t.paused_by_limit,
            idle_check_minutes: t.idle_check_minutes,
            all_idle_since_ms: t.all_idle_since_ms,
            rounds: t.rounds,
        }
    }

    fn restore(self, t: &mut Team) {
        t.slots = self.slots;
        t.max_messages = self.max_messages;
        t.paused = self.paused;
        t.paused_by_limit = self.paused_by_limit;
        t.idle_check_minutes = self.idle_check_minutes;
        t.all_idle_since_ms = self.all_idle_since_ms;
        t.rounds = self.rounds;
    }
}

/// 「代理團隊設定…」按了套用。
///
/// 逐項照舊版 `ApplyAgentSetup` 的順序與規則：
///
/// 1. **投遞上限**改了 → 換掉；如果目前是「因為到上限而暫停」而且新上限還沒到，
///    就自動解除暫停（**計數照舊、不歸零**——歸零只發生在右鍵選次數那條路）。
/// 2. **閒置檢查**改了 → 換掉並把「整組從什麼時候開始閒置」歸零。
/// 3. **要關的格**：關掉分頁（執行中＝結束那個 CLI）、`enabled = false`、清佇列。
/// 4. **要開／重開的格**：已經有分頁的先關掉（**角色是啟動時注入的，改角色也得重開**），
///    再套用新的 CLI／角色、清佇列。
/// 5. 名單可能變了 → **每一格**的角色檔都重組（隊友清單要對）。
/// 6. （前端啟動各格之後）沒有任何格在跑＝拆組；否則重綁。
/// 7. 名單變了 → 寄一封 INFO 給 PM，要它重讀 Runtime Context。
#[tauri::command]
pub fn agent_team_apply(
    key: String,
    setup: TeamSetup,
    settings: State<'_, Arc<SettingsStore>>,
    teams: State<'_, Arc<TeamManager>>,
) -> Result<ApplyPlan, String> {
    let data_dir = roles::data_dir_or_verify(settings.dir());
    let mut list = teams.lock();
    let Some(t) = list.iter_mut().find(|t| t.key == key) else {
        return Err(crate::i18n::t("ma.openFail"));
    };

    // 0. **先驗證、再動狀態**（G6）：以前是邊改邊驗，第 3 格的 CLI 不對而回 Err 時，
    //    第 2 格的分頁已經 `take()` 掉——前端拿到 Err 不會關它，它就成了不屬於任何格的孤兒。
    for i in 0..t.slots.len() {
        let want = setup.slots.get(i).cloned().unwrap_or_default();
        let s = &t.slots[i];
        if !(want.enabled || s.index == 1) {
            continue;
        }
        let running = s.tab.is_some();
        let same_setup = s.backend.eq_ignore_ascii_case(&want.backend)
            && s.role.eq_ignore_ascii_case(&want.role)
            && s.model == want.model.trim();
        let will_launch = !running || !same_setup || want.restart;
        if will_launch && adapters::Backend::by_key(&want.backend).is_none() {
            return Err(crate::i18n::tf("ma.dlgNeedBackend", &[&s.agent_id()]));
        }
        let want_model = want.model.trim();
        if will_launch && !want_model.is_empty() && !models::valid_model(want_model) {
            return Err(crate::i18n::t("model.invalid"));
        }
    }
    // 組角色檔（寫檔）還是可能失敗 → 留一份快照，失敗時整個還原（分頁 id 也還回各格）
    let snapshot = ApplySnapshot::of(t);

    let mut changed = false;

    // 聊天室：第一列是「討論回合」，沒有投遞上限／閒置檢查（前端送的是固定值，不能蓋掉）
    if t.is_chat() {
        let rounds = setup.rounds.max(1);
        if rounds != t.rounds {
            t.rounds = rounds;
            println!("[AwayTerminal] 聊天室 {}：討論回合={rounds}", t.number);
            changed = true;
        }
    }

    // 1. 投遞上限
    let max = setup.max_messages;
    if !t.is_chat() && max != t.max_messages {
        t.max_messages = max;
        // 因為到上限而暫停、上限調高了＝接著送（計數照舊）
        if t.paused && t.paused_by_limit && !t.limit_reached() {
            t.paused = false;
            t.paused_by_limit = false;
        }
        println!(
            "[AwayTerminal] 代理團隊 {}：投遞上限={} 計數={} 暫停={}",
            t.number,
            t.limit_text(),
            t.message_count,
            t.paused
        );
        changed = true;
    }

    // 2. 閒置檢查
    if !t.is_chat() && setup.idle_check_minutes != t.idle_check_minutes {
        t.idle_check_minutes = setup.idle_check_minutes;
        t.all_idle_since_ms = 0;
        println!(
            "[AwayTerminal] 代理團隊 {}：閒置檢查={}",
            t.number,
            if t.idle_check_minutes > 0 {
                format!("{} 分鐘", t.idle_check_minutes)
            } else {
                "不檢查".to_string()
            }
        );
        changed = true;
    }

    // 3／4. 哪些要關、哪些要開（格 1 不能關，同舊版的勾選是停用的）
    let mut close_tabs = Vec::new();
    let mut launch_idx: Vec<u32> = Vec::new();
    let mut roster_changed = false;
    let mut closed: Vec<String> = Vec::new();
    for i in 0..t.slots.len() {
        let want = setup.slots.get(i).cloned().unwrap_or_default();
        let idx = t.slots[i].index;
        let running = t.slots[i].tab.is_some();
        let enabled = t.slots[i].enabled;
        // 模型和 CLI／角色一樣是啟動時才生效的 → 改了也要重開那一格
        let same_setup = t.slots[i].backend.eq_ignore_ascii_case(&want.backend)
            && t.slots[i].role.eq_ignore_ascii_case(&want.role)
            && t.slots[i].model == want.model.trim();
        // 格 1 永遠啟用
        let want_enabled = want.enabled || idx == 1;

        if !want_enabled {
            if running || enabled {
                if let Some(tab) = t.slots[i].tab.take() {
                    close_tabs.push(tab);
                }
                roster_changed |= enabled;
                t.slots[i].enabled = false;
                t.slots[i].queue.clear();
                closed.push(t.slots[i].agent_id());
                changed = true;
            }
            continue;
        }

        // 啟用中：沒在跑＝要啟動；在跑但設定變了（或使用者按了「重新啟動」）＝關掉重開
        let restart = running && (!same_setup || want.restart);
        if !running || restart {
            if let Some(tab) = t.slots[i].tab.take() {
                close_tabs.push(tab);
                // 只換模型不算名單變動（Agent ID／角色／CLI 都沒變，不必請 PM 重讀名單）
                roster_changed |= !(t.slots[i].backend.eq_ignore_ascii_case(&want.backend)
                    && t.slots[i].role.eq_ignore_ascii_case(&want.role));
            } else {
                roster_changed = true;
            }
            // CLI 種類與模型名稱在第 0 步已經驗過
            t.slots[i].enabled = true;
            t.slots[i].backend = want.backend.clone();
            t.slots[i].model = want.model.trim().to_string();
            t.slots[i].role = want.role.clone();
            t.slots[i].role_title = roles::title_of(library_for(t.kind), &data_dir, &want.role);
            t.slots[i].queue.clear();
            launch_idx.push(idx);
            changed = true;
        }
    }

    if !changed {
        return Ok(ApplyPlan {
            close_tabs: Vec::new(),
            launch: Vec::new(),
            roster_changed: false,
            changed: false,
        });
    }

    // 5. 名單可能變了 → 每格的角色檔都重組（新開的格要用新檔啟動）
    let indices: Vec<u32> = t.slots.iter().filter(|s| s.enabled).map(|s| s.index).collect();
    for i in &indices {
        match roles::compose(library_for(t.kind), &data_dir, t, *i) {
            Ok(p) => {
                if let Some(s) = t.slots.iter_mut().find(|s| s.index == *i) {
                    s.role_file = p.to_string_lossy().to_string();
                }
            }
            Err(e) => {
                let who = t
                    .slots
                    .iter()
                    .find(|s| s.index == *i)
                    .map(|s| s.agent_id())
                    .unwrap_or_default();
                // 前端拿到 Err 什麼都不會做 → 把狀態整個還原（`take()` 掉的分頁 id 還回各格），
                // 已經重寫的角色檔也照原名單再組一次，免得還在跑的人讀到沒生效的名單（G6）
                snapshot.restore(t);
                let lib = library_for(t.kind);
                let enabled: Vec<u32> = t.slots.iter().filter(|s| s.enabled).map(|s| s.index).collect();
                for j in enabled {
                    let _ = roles::compose(lib, &data_dir, t, j);
                }
                return Err(crate::i18n::tf("ma.roleComposeFailed", &[&who, &e.to_string()]));
            }
        }
    }

    // 這一輪的關／開中途不要重排
    t.suspend_relink = true;
    t.fresh = launch_idx.clone();
    let launch: Vec<SlotPlan> = launch_idx
        .iter()
        .filter_map(|idx| t.slots.iter().find(|s| s.index == *idx))
        .map(|s| SlotPlan {
            index: s.index,
            restore: None,
            agent_id: s.agent_id(),
            role_title: s.role_title.clone(),
            backend: s.backend.clone(),
            backend_name: s.backend_name(),
            label: s.label(),
            color: s.color().to_string(),
        })
        .collect();
    println!(
        "[AwayTerminal] 代理團隊 {}：套用設定 → 啟動 {} 關閉 {}",
        t.number,
        launch.iter().map(|s| s.agent_id.clone()).collect::<Vec<_>>().join(","),
        if closed.is_empty() { "-".to_string() } else { closed.join(",") }
    );
    Ok(ApplyPlan {
        close_tabs,
        launch,
        roster_changed,
        changed: true,
    })
}

/// 套用設定的收尾：重綁（或拆組）、把作用中分頁拉回這一組、名單變了就通知 PM。
#[tauri::command]
pub fn agent_team_apply_done(
    app: AppHandle,
    key: String,
    roster_changed: bool,
    teams: State<'_, Arc<TeamManager>>,
) -> Option<u32> {
    let (running, fresh) = {
        let mut list = teams.lock();
        let t = list.iter_mut().find(|t| t.key == key)?;
        t.suspend_relink = false;
        let fresh = std::mem::take(&mut t.fresh);
        (t.running().count(), fresh)
    };
    let arc = (*teams).clone();
    if running == 0 {
        disband(&app, &arc, &key);
        return None;
    }
    link(&app, &arc, &key);

    // 名單變了 → 通知 PM（沒有 PM 角色就是代表列那一格）：角色檔已重新產生，要它重讀
    if roster_changed {
        let msg = {
            let list = teams.lock();
            list.iter().find(|t| t.key == key).and_then(|t| {
                let pm = t
                    .running()
                    .find(|s| s.role == "product-manager" && !fresh.contains(&s.index))
                    .or_else(|| t.running().find(|s| !fresh.contains(&s.index)))?;
                let roster = t
                    .enabled()
                    .map(|s| format!("{} {} ({})", s.agent_id(), s.role_title, s.backend_name()))
                    .collect::<Vec<_>>()
                    .join(", ");
                Some((
                    t.bus.clone()?,
                    pm.agent_id(),
                    pm.role_file.clone(),
                    roster,
                ))
            })
        };
        if let Some((bus, pm_id, role_file, roster)) = msg {
            bus.write_message(
                "AwayTerminal",
                &pm_id,
                "INFO",
                "",
                &format!(
                    "The team roster changed. Enabled agents now: {roster}.\n\n\
Your role file {role_file} has been regenerated. Re-read its Runtime Context section before assigning more work."
                ),
            );
        }
    }
    let row = {
        let list = teams.lock();
        list.iter().find(|t| t.key == key).and_then(|t| t.row_tab())
    };
    // 關掉的那格若是作用中分頁，tab_close 會跳到別的分頁 → 拉回這一組
    if let Some(id) = row {
        let target = teams.focus_target(id);
        crate::host::emit_host(&app, format!("s{target}"));
        return Some(target);
    }
    None
}

/// 「代理團隊設定…」要填進視窗的目前狀態（舊版 `ApplyInitial`）。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSetupState {
    pub key: String,
    /// 組號（確認對話框的 `Agent-{組號}{格號}` 用——`key` 的前綴是**建組當時**的組號，
    /// 恢復後組號可能換了，不能拿來算；G8）。
    pub number: u32,
    pub dir: String,
    pub title: String,
    /// 代理團隊還是 AI 聊天室（前端靠它決定開哪一種設定視窗、載哪一套角色庫；B1）。
    pub kind: team::GroupKind,
    /// 聊天室的討論回合（聊天室設定視窗第一列的值；B1）。
    pub rounds: u32,
    pub max_messages: u32,
    pub idle_check_minutes: u32,
    pub sandbox: bool,
    pub slots: Vec<SlotState>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotState {
    pub index: u32,
    pub enabled: bool,
    pub backend: String,
    pub role: String,
    /// 這一格用的模型（空＝預設）。
    pub model: String,
    /// `running`（有連線）／`exited`（分頁在但連線結束）／`notRunning`（沒有分頁）。
    pub state: String,
}

#[tauri::command]
pub fn agent_team_state(
    app: AppHandle,
    key: String,
    teams: State<'_, Arc<TeamManager>>,
) -> Option<TeamSetupState> {
    let sessions = app.try_state::<crate::session::SessionManager>();
    let list = teams.lock();
    let t = list.iter().find(|t| t.key == key)?;
    Some(TeamSetupState {
        key: t.key.clone(),
        number: t.number,
        dir: t.dir.clone(),
        title: t.title.clone(),
        kind: t.kind,
        rounds: t.rounds,
        max_messages: t.max_messages,
        idle_check_minutes: t.idle_check_minutes,
        sandbox: t.sandbox_cfg.is_some(),
        slots: t
            .slots
            .iter()
            .map(|s| SlotState {
                index: s.index,
                enabled: s.enabled,
                backend: s.backend.clone(),
                role: s.role.clone(),
                model: s.model.clone(),
                state: match s.tab {
                    Some(id) => {
                        let alive = sessions.as_ref().is_some_and(|m| m.get(id).is_some());
                        if alive { "running" } else { "exited" }.to_string()
                    }
                    None => "notRunning".to_string(),
                },
            })
            .collect(),
    })
}

/// 這個分頁所在的團隊存成「我的最愛」要記的東西（舊版 `FavoriteFromTab` 的團隊分支：
/// 記整組設定，重開不跳設定視窗）。不是團隊的分頁回 `None`。
///
/// 回 `(組名, 設定)`。只記**有在用**的格（啟用而且選了 CLI），同舊版
/// `Enabled = s.Enabled && !string.IsNullOrEmpty(s.Backend)`。
pub fn favorite_setup(teams: &TeamManager, tab: u32) -> Option<(String, TeamSetup)> {
    let list = teams.lock();
    let t = list.iter().find(|t| t.slot_by_tab(tab).is_some())?;
    let setup = TeamSetup {
        dir: t.dir.clone(),
        title: String::new(),
        slots: t
            .slots
            .iter()
            .map(|s| SlotSetup {
                enabled: s.enabled && !s.backend.is_empty(),
                backend: s.backend.clone(),
                role: s.role.clone(),
                model: s.model.clone(),
                restart: false,
            })
            .collect(),
        max_messages: t.max_messages,
        idle_check_minutes: t.idle_check_minutes,
        sandbox: t.sandbox_cfg.is_some(),
        kind: t.kind,
        rounds: t.rounds,
    };
    Some((t.title.clone(), setup))
}

/// 存檔時把代理團隊的格改寫成 `kind = "agent"` 並補上組的資訊（`restore::save` 呼叫）。
///
/// 一組的每一格都存一份組的設定（上限／閒置檢查／比例／組號／沙盒），恢復時取第一格的——
/// 和舊版 `SavedTab` 的做法一樣（那些欄位在舊版也是每一筆都有）。
pub fn annotate_saved(teams: &Arc<TeamManager>, entries: &mut [(u32, crate::restore::SavedTab)]) {
    let list = teams.lock();
    for (tab, entry) in entries.iter_mut() {
        let Some(team) = list.iter().find(|t| t.slot_by_tab(*tab).is_some()) else {
            continue;
        };
        let Some(slot) = team.slot_by_tab(*tab) else { continue };
        entry.kind = "agent".to_string();
        entry.dir = team.dir.clone();
        entry.agent_key = team.key.clone();
        entry.agent_index = slot.index;
        entry.agent_group_number = team.number;
        entry.agent_backend = slot.backend.clone();
        entry.agent_role = slot.role.clone();
        // 這一格的模型以團隊記的為準（分頁上那一份是啟動當下的）
        entry.model = slot.model.clone();
        entry.agent_ratio = team.ratio;
        entry.agent_max_messages = team.max_messages;
        entry.agent_idle_check = team.idle_check_minutes;
        entry.agent_sandbox = team.sandbox_cfg.is_some();
        entry.agent_kind = team.kind;
        entry.agent_rounds = team.rounds;
        entry.agent_chat_folder = team.chat_folder.clone();
        // `conn_name`（上次跑的是哪一條連線）由 `restorable()` 填好了，恢復時照它查路徑
    }
}

/// 恢復一組代理團隊（舊版 `RestoreAgentGroup`）。
///
/// `indices` ＝ `restore_list()` 裡屬於同一個 `agentKey` 的那幾筆（順序不重要，
/// 這裡自己依格號排）。行為照舊版：
///
/// - 同資料夾；資料夾不見了就**不恢復**（log 一行）
/// - **同組號**（沒被占用才沿用，否則取最小空號）→ Agent ID 盡量不變
/// - 同比例、同投遞上限、同閒置檢查、同沙盒設定
/// - 每格照上次的執行檔／參數（見 [`plan_slot`] 的 `saved_conn`）
/// - **角色檔以目前的 `roles/` 重新組合**（CLI 是新 session；OpenCode／Gemini 會重打第一句）
/// - 沙盒團隊：`prepare()` 看到 worktree 還在就沿用
///
/// `models`＝格號 → 模型的覆寫（前端發現上次的模型已經不在清單裡、請使用者重選之後帶過來）；
/// 沒有覆寫的格用存檔裡的模型。
#[tauri::command]
pub fn agent_team_restore(
    indices: Vec<usize>,
    models: Option<std::collections::HashMap<u32, String>>,
    settings: State<'_, Arc<SettingsStore>>,
    teams: State<'_, Arc<TeamManager>>,
    tabs: State<'_, Arc<TabManager>>,
) -> Result<TeamPlan, String> {
    let mut saved: Vec<(usize, crate::restore::SavedTab)> = indices
        .into_iter()
        .filter_map(|i| crate::restore::saved_at(i).map(|e| (i, e)))
        .filter(|(_, e)| e.agent_index >= 1 && e.agent_index <= 4)
        .collect();
    saved.sort_by_key(|(_, e)| e.agent_index);
    // 同一格存了兩筆（不該發生）→ 只留第一筆，同舊版 `bySlot.ContainsKey` 的判斷
    saved.dedup_by_key(|(_, e)| e.agent_index);
    let first = saved
        .first()
        .map(|(_, e)| e.clone())
        .ok_or_else(|| crate::i18n::t("ma.openFail"))?;
    if first.dir.trim().is_empty() || !std::path::Path::new(&first.dir).is_dir() {
        let msg = crate::i18n::tf("ma.dlgFolderMissing", &[&first.dir]);
        println!("[AwayTerminal] 代理團隊不恢復：{msg}");
        return Err(msg);
    }

    let setup = TeamSetup {
        dir: first.dir.clone(),
        title: first.title.clone(),
        slots: (1..=4)
            .map(|i| match saved.iter().find(|(_, e)| e.agent_index == i) {
                Some((_, e)) => SlotSetup {
                    enabled: true,
                    backend: e.agent_backend.clone(),
                    role: e.agent_role.clone(),
                    model: models
                        .as_ref()
                        .and_then(|m| m.get(&i).cloned())
                        .unwrap_or_else(|| e.model.clone()),
                    restart: false,
                },
                None => SlotSetup::default(),
            })
            .collect(),
        max_messages: first.agent_max_messages,
        idle_check_minutes: first.agent_idle_check,
        sandbox: first.agent_sandbox,
        kind: first.agent_kind,
        rounds: if first.agent_rounds > 0 {
            first.agent_rounds
        } else {
            team::DEFAULT_ROUNDS
        },
    };
    let saved_conns: Vec<(u32, crate::settings::CustomConn, usize)> = saved
        .iter()
        .map(|(i, e)| {
            (
                e.agent_index,
                crate::settings::CustomConn {
                    name: e.conn_name.clone(),
                    path: e.conn_name.clone(), // 佔位，下面用 settings 查真正的路徑
                    ..crate::settings::CustomConn::default()
                },
                *i,
            )
        })
        .collect();

    let mut plan = create_team(
        &settings,
        &teams,
        &tabs,
        setup,
        CreateOpts {
            key: Some(first.agent_key.clone()).filter(|k| !k.is_empty()),
            preferred_number: first.agent_group_number,
            ratio: if first.agent_ratio > 0.0 { first.agent_ratio } else { 0.5 },
            title_may_exist: true,
        },
    )?;
    // 每格：上次的連線（依名稱從使用者的自訂連線清單查；查不到就讓 plan_slot 自動偵測）
    // ＋ 要倒回哪一筆畫面
    {
        let mut list = teams.lock();
        if let Some(t) = list.iter_mut().find(|t| t.key == plan.key) {
            // 聊天室：沿用上次那場的紀錄資料夾指標（右鍵「開啟討論紀錄資料夾」開得到上一場），
            // 但**進度不回來**——停在「等主題」，下次開始討論會開新的資料夾（舊版行為）
            t.chat_folder = first.agent_chat_folder.clone();
            for (idx, placeholder, restore_index) in &saved_conns {
                let _ = restore_index;
                if let Some(s) = t.slots.iter_mut().find(|s| s.index == *idx) {
                    s.saved_conn = crate::custom::find(&settings, &placeholder.name);
                }
            }
        }
    }
    for p in plan.slots.iter_mut() {
        p.restore = saved
            .iter()
            .find(|(_, e)| e.agent_index == p.index)
            .map(|(i, _)| *i);
    }
    println!(
        "[AwayTerminal] 代理團隊恢復：組號={}（上次 {}）目錄={} 格={} 比例={}",
        plan.number,
        first.agent_group_number,
        plan.work_dir,
        plan.slots.len(),
        first.agent_ratio
    );
    Ok(plan)
}

// ---------------------------------------------------------------- AI 聊天室

/// 「開始討論／換主題…」：給主題並開始第 1 回合（舊版 `AskChatTopic` → `StartChatDiscussion`）。
///
/// 換主題時如果這個資料夾已經有一場討論，會**開新的討論紀錄資料夾**（舊的發言檔／結論
/// 不會被當成這一場的），所以角色檔要重組——裡面寫著資料夾路徑。
#[tauri::command]
pub fn chat_start(
    app: AppHandle,
    key: String,
    topic: String,
    settings: State<'_, Arc<SettingsStore>>,
    teams: State<'_, Arc<TeamManager>>,
) -> Result<String, String> {
    let topic = topic.trim().to_string();
    if topic.is_empty() {
        return Err(crate::i18n::t("chat.topicEmpty"));
    }
    let data_dir = roles::data_dir_or_verify(settings.dir());
    let header = {
        let mut list = teams.lock();
        let t = list
            .iter_mut()
            .find(|t| t.key == key && t.is_chat())
            .ok_or_else(|| crate::i18n::t("ma.openFail"))?;
        chat::start_discussion(t, &topic);
        // 角色檔重組（新的資料夾路徑 ＋ 這次的主題都寫在執行期脈絡裡）
        let indices: Vec<u32> = t.slots.iter().filter(|s| s.enabled).map(|s| s.index).collect();
        for i in &indices {
            match roles::compose(&roles::CHAT, &data_dir, t, *i) {
                Ok(pth) => {
                    if let Some(sl) = t.slots.iter_mut().find(|s| s.index == *i) {
                        sl.role_file = pth.to_string_lossy().to_string();
                    }
                }
                Err(e) => println!("[AwayTerminal] 聊天室角色檔組合失敗：{e}"),
            }
        }
        let header = chat::transcript_header(t);
        chat::write_transcript(t, &header);
        println!(
            "[AwayTerminal] 聊天室 CHAT-{}：開始討論，{} 回合，紀錄 {}",
            t.number, t.rounds, t.chat_folder
        );
        t.chat_folder.clone()
    };
    let arc = (*teams).clone();
    post_state(&app, &arc);
    Ok(header)
}

/// 「插話…」：把使用者的一段話接進討論紀錄，下一位發言的人就看得到。
///
/// 只有**討論中或寫結論中**才能插話（舊版註解：還沒有主題時插話會先建出 `transcript.md`，
/// 之後「開始討論」看到檔案就換新資料夾＝那句話誰也看不到；結束後插話也沒人會讀）。
#[tauri::command]
pub fn chat_say(key: String, text: String, teams: State<'_, Arc<TeamManager>>) -> Result<(), String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }
    let list = teams.lock();
    let t = list
        .iter()
        .find(|t| t.key == key && t.is_chat())
        .ok_or_else(|| crate::i18n::t("ma.openFail"))?;
    if !matches!(
        t.phase,
        team::ChatPhase::Discussing | team::ChatPhase::Concluding
    ) {
        return Err(crate::i18n::t("chat.sayNotNow"));
    }
    chat::user_said(t, &text);
    Ok(())
}

/// 「結束討論」：這一輪結束後就請主持人寫結論（舊版 `ChatEnd_Click`）。
#[tauri::command]
pub fn chat_end(app: AppHandle, key: String, teams: State<'_, Arc<TeamManager>>) -> bool {
    let ok = {
        let mut list = teams.lock();
        match list.iter_mut().find(|t| t.key == key && t.is_chat()) {
            Some(t) if t.phase == team::ChatPhase::Discussing => {
                t.end_requested = true;
                let text = crate::i18n::t("chat.trUserEnd");
                chat::write_transcript(t, &text);
                println!(
                    "[AwayTerminal] 聊天室 CHAT-{}：使用者要求在第 {} 回合結束",
                    t.number, t.round
                );
                true
            }
            _ => false,
        }
    };
    if ok {
        let arc = (*teams).clone();
        post_state(&app, &arc);
    }
    ok
}

/// 「開啟討論紀錄資料夾」要開哪裡。
#[tauri::command]
pub fn chat_folder(key: String, teams: State<'_, Arc<TeamManager>>) -> Option<String> {
    let list = teams.lock();
    let t = list.iter().find(|t| t.key == key && t.is_chat())?;
    let dir = chat::chat_dir(t);
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.to_string_lossy().to_string())
}

/// 改這一組的名稱（分頁列那一列的標題）。
///
/// 為什麼要專門一個 command：分頁右鍵「更改名稱」改的是**分頁**的標題，而代表列的標題
/// 每次重綁（`link`）都會被組名蓋回去——所以要改的是 `Team::title`。代理團隊與聊天室共用。
#[tauri::command]
pub fn agent_team_rename(
    app: AppHandle,
    key: String,
    title: String,
    teams: State<'_, Arc<TeamManager>>,
) -> Result<String, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(crate::i18n::t("err.nameEmpty"));
    }
    {
        let mut list = teams.lock();
        let t = list
            .iter_mut()
            .find(|t| t.key == key)
            .ok_or_else(|| crate::i18n::t("ma.openFail"))?;
        println!(
            "[AwayTerminal] {} {}：改名「{}」→「{title}」",
            if t.is_chat() { "聊天室" } else { "代理團隊" },
            t.number,
            t.title
        );
        t.title = title.clone();
    }
    // 重綁就會把代表列的標題換成新組名（`link` 會送 `t{id}` 與 `g`）
    let arc = (*teams).clone();
    link(&app, &arc, &key);
    Ok(title)
}

// ---------------------------------------------------------------- --verify
// i18n-audit:log-only-begin 這一段只在 `--verify` 跑（驗證輸出），使用者不會看到；不進八語表


/// `--verify` 的準備：用**假 agent**（`examples/fake_agent.rs` 編出來的執行檔）當 provider，
/// 並在 `%TEMP%` 開一個空專案。回傳那個專案資料夾。
///
/// ⚠️ 這條路**不動使用者的任何東西**：不寫 settings.json、不碰使用者的 `.ai/`、
/// 不啟動真的 claude／codex。覆寫只活在記憶體裡，[`agent_verify_end`] 會清掉。
#[tauri::command]
pub fn agent_verify_begin(section: Option<String>) -> Result<String, String> {
    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name(if cfg!(windows) { "fake_agent.exe" } else { "fake_agent" });
    // dev 時 awayterminal.exe 在 target\debug\，假 agent 在 target\debug\examples\
    let exe = if exe.is_file() {
        exe
    } else {
        exe.parent()
            .map(|d| d.join("examples").join(exe.file_name().unwrap_or_default()))
            .filter(|p| p.is_file())
            .ok_or_else(|| {
                "找不到 fake_agent（先跑 `cargo build --example fake_agent`）".to_string()
            })?
    };
    // **每一段一個子資料夾**（TASK-018 Issue 2）：本來是照行程 id 取名，所以同一次 `--verify`
    // 的幾段共用同一個資料夾——前一段結束時刪掉、後一段再建回來。目前的順序安全，但多加一段
    // 或改成交錯執行就會互相踩。
    let section = section
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or("team")
        .to_string();
    let dir = std::env::temp_dir()
        .join(format!("awayterm-verify-team-{}", std::process::id()))
        .join(&section);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    adapters::set_verify_exe(Some(exe.to_string_lossy().to_string()));
    // 角色檔也寫在 %TEMP%，不碰使用者真的 multiagent 資料夾
    roles::set_verify_data_dir(Some(dir.join("appdata")));
    println!(
        "[AwayTerminal] --verify {section}：假 agent={} 專案={}",
        exe.display(),
        dir.display()
    );
    Ok(dir.to_string_lossy().to_string())
}

/// `--verify`：代替 agent 寫一封信進團隊的信箱（回傳檔名）。
#[tauri::command]
pub fn agent_verify_send(
    key: String,
    from: String,
    to: String,
    teams: State<'_, Arc<TeamManager>>,
) -> Option<String> {
    let bus = {
        let list = teams.lock();
        list.iter().find(|t| t.key == key)?.bus.clone()?
    };
    bus.write_message(&from, &to, "TASK", "TASK-V1", "verify：請回一封 TASK_RESULT。")
}

/// `--verify`：看一眼團隊目前的狀態（投遞計數、待投遞、信箱裡有哪些檔）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyState {
    pub found: bool,
    pub number: u32,
    pub running: u32,
    pub message_count: u32,
    pub pending: u32,
    pub paused: bool,
    pub work_dir: String,
    pub sandbox: bool,
    /// 信箱裡的檔名（排序過）。
    pub bus_files: Vec<String>,
    pub delivered: Vec<String>,
    pub tabs: Vec<u32>,
    pub ratio: f64,
    /// 每一格**組好的角色檔**的摘要（依格號排序）。
    ///
    /// 為什麼要它：`--verify` 不能靠 pane 上的新輸出判斷角色檔——跑到後面 pane 是 0×0，
    /// `b` 協定的 `held` 要等 pane fit 到最終寬度才把新輸出寫出來，所以 CLI 印的那一行
    /// 可能還沒進 buffer（同一段第二次跑 true、第三次跑 false，實際踩到）。讀檔沒這個問題。
    pub roles: Vec<RoleFileInfo>,
}

/// 一格角色檔的摘要（`--verify` 用）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleFileInfo {
    /// 這一格是誰（程式裡的值，用來和檔案內容對照）。
    pub slot_id: String,
    /// 檔案裡的 `Agent ID:`。
    pub agent_id: String,
    /// 檔案裡的 `Role:`。
    pub role: String,
    /// 有 `# Runtime Context (generated by AwayTerminal)` 那一段。
    pub has_runtime_context: bool,
    /// 三層都在（common rules 的標題 ＋ 執行期脈絡）。
    pub three_layers: bool,
    pub bytes: u64,
}

#[tauri::command]
pub fn agent_verify_state(key: String, teams: State<'_, Arc<TeamManager>>) -> VerifyState {
    let list = teams.lock();
    let Some(t) = list.iter().find(|t| t.key == key) else {
        return VerifyState {
            found: false,
            number: 0,
            running: 0,
            message_count: 0,
            pending: 0,
            paused: false,
            work_dir: String::new(),
            sandbox: false,
            bus_files: Vec::new(),
            delivered: Vec::new(),
            tabs: Vec::new(),
            ratio: 0.0,
            roles: Vec::new(),
        };
    };
    let bus_dir = std::path::Path::new(&t.work_dir).join(".ai").join("bus");
    let mut bus_files: Vec<String> = std::fs::read_dir(&bus_dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| message::is_message_name(n))
                .collect()
        })
        .unwrap_or_default();
    bus_files.sort();
    let mut delivered: Vec<String> = std::fs::read_to_string(bus_dir.join(bus::DELIVERED_NAME))
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();
    delivered.sort();
    VerifyState {
        found: true,
        number: t.number,
        running: t.running().count() as u32,
        message_count: t.message_count,
        pending: t.pending_count() as u32,
        paused: t.paused,
        work_dir: t.work_dir.clone(),
        sandbox: t.sandbox_cfg.is_some(),
        bus_files,
        delivered,
        tabs: t.running().filter_map(|s| s.tab).collect(),
        ratio: t.ratio,
        roles: t
            .running()
            .map(|s| {
                let text = std::fs::read_to_string(&s.role_file).unwrap_or_default();
                // 代理團隊的角色檔是 `Agent ID:`／`Role:`，聊天室是 `你的代號：`／`你的角色：`
                //（執行期脈絡兩邊完全不同）→ 兩種都要認得
                let field = |keys: &[&str]| {
                    text.lines()
                        .find_map(|l| {
                            keys.iter()
                                .find_map(|k| l.strip_prefix(*k).map(|v| v.trim().to_string()))
                        })
                        .unwrap_or_default()
                };
                RoleFileInfo {
                    slot_id: s.agent_id(),
                    agent_id: field(&["Agent ID:", "你的代號："]),
                    role: field(&["Role:", "你的角色："]),
                    // 兩種角色檔的標題不一樣：代理團隊是英文
                    //（`# AwayTerminal Multi-Agent Common Rules`／`# Runtime Context (generated…)`），
                    // 聊天室是中文（`# AwayTerminal AI 聊天室 共同規則`／`# Runtime Context（AwayTerminal 產生）`）
                    has_runtime_context: text.contains("# Runtime Context"),
                    three_layers: (text.contains("# AwayTerminal Multi-Agent Common Rules")
                        || text.contains("# AwayTerminal AI 聊天室 共同規則"))
                        && text.contains("# Runtime Context"),
                    bytes: text.len() as u64,
                }
            })
            .collect(),
    }
}

/// `--verify`：討論紀錄的統計（聊天室那一段用）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatVerify {
    pub bytes: u64,
    /// `## 第 N 回合 · Agent-xx` 有幾則。
    pub turns: u32,
    /// 使用者插話幾則。
    pub user_said: u32,
    /// 結論幾則。
    pub conclusions: u32,
    /// 這場討論資料夾裡的檔名（排序過）。
    pub files: Vec<String>,
}

#[tauri::command]
pub fn chat_verify_transcript(key: String, teams: State<'_, Arc<TeamManager>>) -> ChatVerify {
    let list = teams.lock();
    let Some(t) = list.iter().find(|t| t.key == key) else {
        return ChatVerify {
            bytes: 0,
            turns: 0,
            user_said: 0,
            conclusions: 0,
            files: Vec::new(),
        };
    };
    let text = std::fs::read_to_string(chat::chat_path(t, "transcript.md")).unwrap_or_default();
    // 標題行的樣子隨語言，所以數的是「## 」開頭那幾種前綴
    let turn_head = crate::i18n::tf("chat.trTurn", &["", "", ""]);
    let turn_key = turn_head.split_whitespace().next().unwrap_or("").to_string();
    let user_key = crate::i18n::t("chat.trUserSaid");
    let concl_key = crate::i18n::t("chat.trConclusion");
    let mut turns = 0;
    let mut user_said = 0;
    let mut conclusions = 0;
    for line in text.lines().filter(|l| l.starts_with("## ")) {
        let h = &line[3..];
        if !concl_key.is_empty() && h.starts_with(&concl_key) {
            conclusions += 1;
        } else if !user_key.is_empty() && h.starts_with(&user_key) {
            user_said += 1;
        } else if turn_key.is_empty() || h.starts_with(&turn_key) {
            turns += 1;
        }
    }
    let mut files: Vec<String> = std::fs::read_dir(chat::chat_dir(t))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    ChatVerify {
        bytes: text.len() as u64,
        turns,
        user_said,
        conclusions,
        files,
    }
}

/// `--verify` 收尾：清掉假 agent 的覆寫，並刪掉 `%TEMP%` 的那個專案資料夾。
#[tauri::command]
pub fn agent_verify_end(dir: String) -> String {
    adapters::set_verify_exe(None);
    roles::set_verify_data_dir(None);
    let p = std::path::PathBuf::from(&dir);
    // 只刪自己在 %TEMP% 底下建的那一個（名字要對得上，免得刪錯東西）
    // 路徑長相：`%TEMP%\awayterm-verify-team-<pid>\<段名>`（每一段一個子資料夾）；
    // 也接受沒有子資料夾的舊寫法。
    let name_ok = {
        let is_root = |q: &std::path::Path| {
            q.file_name()
                .map(|n| n.to_string_lossy().starts_with("awayterm-verify-team-"))
                .unwrap_or(false)
        };
        is_root(&p) || p.parent().map(is_root).unwrap_or(false)
    };
    if !name_ok || !p.starts_with(std::env::temp_dir()) {
        return format!("拒絕刪除（不是 %TEMP% 底下的驗證資料夾）：{dir}");
    }
    // 剛關掉的 PTY 還在收尾（優雅結束鍵 60ms ＋ 收行程的執行緒），資料夾會被占著 →
    // 重試幾次（--verify 實際踩到 os error 32）
    let mut last = String::new();
    for i in 0..10 {
        match std::fs::remove_dir_all(&p) {
            Ok(()) => return format!("已清掉 {dir}（第 {} 次嘗試）", i + 1),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return format!("已清掉 {dir}")
            }
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        }
    }
    format!("清不掉 {dir}（{last}）")
}

// i18n-audit:log-only-end

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_the_ratio_like_dotnet() {
        assert_eq!(format_ratio(0.5), "0.5");
        assert_eq!(format_ratio(0.333_333), "0.333");
        assert_eq!(format_ratio(0.15), "0.15");
        assert_eq!(format_ratio(1.0), "1");
    }

    #[test]
    fn finds_tabs_and_strip_rows() {
        let m = TeamManager::default();
        {
            let mut t = Team::new("k", 1, "C:\\p");
            t.slots[0].enabled = true;
            t.slots[0].tab = Some(5);
            t.slots[1].enabled = true;
            t.slots[1].tab = Some(6);
            m.lock().push(t);
        }
        assert_eq!(m.find_tab(6), Some(("k".to_string(), 2)));
        assert_eq!(m.find_tab(99), None);
        assert!(m.is_strip_row(5), "格 1 是代表列");
        assert!(!m.is_strip_row(6), "其餘格不列在分頁列");
        assert!(m.is_strip_row(99), "不屬於任何團隊的分頁照常列");
        // 點分頁列那一列 → 一律第 1 格（代表列），不管最後點過哪一格（2.1.2）
        assert_eq!(m.focus_target(5), 5);
        m.lock()[0].last_focused = Some(6);
        assert_eq!(m.focus_target(5), 5);
        assert_eq!(m.focus_target(6), 5, "從組裡任一格問都回代表列");
        m.lock()[0].slots[1].tab = None;
        assert_eq!(m.focus_target(5), 5);
    }
}
