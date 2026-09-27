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
pub mod deliver;
pub mod message;
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

    /// 點分頁列那一列要切到哪個分頁：代理團隊＝最後點過的那一格（還在組裡的話）。
    pub fn focus_target(&self, tab: u32) -> u32 {
        let list = self.lock();
        let Some(team) = list.iter().find(|t| t.slot_by_tab(tab).is_some()) else {
            return tab;
        };
        team.last_focused
            .filter(|id| team.slot_by_tab(*id).is_some())
            .unwrap_or(tab)
    }
}

/// 建團隊視窗送過來的一格。
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotSetup {
    pub enabled: bool,
    /// CLI 種類（`claude-code`／`codex`／`opencode`／`geminicli`）。
    pub backend: String,
    /// 角色檔名（空＝None）。
    pub role: String,
}

/// 建團隊視窗的全部欄位（舊版 `MultiAgentSetup`，多一個 `sandbox`）。
#[derive(Clone, Debug, serde::Deserialize)]
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
    /// **沙盒模式**（新增；預設開，`CLAUDE.md`「新增功能」一節）。
    #[serde(default = "default_sandbox")]
    pub sandbox: bool,
}

fn default_max() -> u32 {
    team::DEFAULT_MAX_MESSAGES
}
fn default_idle() -> u32 {
    team::DEFAULT_IDLE_CHECK_MINUTES
}
fn default_sandbox() -> bool {
    true
}

/// 一格的啟動計畫（前端照它呼叫 `session_create`）。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotPlan {
    pub index: u32,
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
    for m in msgs {
        crate::host::emit_host(app, m);
    }
    crate::host::emit_host(app, g);
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
    let conn = adapters::resolve(settings, backend).ok_or_else(|| {
        crate::i18n::tf(
            "ma.backendMissing",
            &[backend.display_name(), &slot.agent_id()],
        )
    })?;
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
        extra_args: format!("{}{sb_extra}", launch.extra_args),
        dir: team.dir.clone(),
        work_dir: team.work_dir.clone(),
        sandbox: team.sandbox_cfg.clone(),
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
}

#[tauri::command]
pub fn agent_setup_options(
    settings: State<'_, Arc<SettingsStore>>,
    teams: State<'_, Arc<TeamManager>>,
) -> SetupOptions {
    let data_dir = roles::data_dir_or_verify(settings.dir());
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
            })
        })
        .collect();
    SetupOptions {
        backends,
        roles: roles::list_roles(&data_dir),
        default_roles: roles::DEFAULT_SLOT_ROLES
            .iter()
            .map(|s| s.to_string())
            .collect(),
        limit_choices: team::LIMIT_CHOICES.to_vec(),
        idle_choices: team::IDLE_CHECK_CHOICES.to_vec(),
        default_max_messages: team::DEFAULT_MAX_MESSAGES,
        default_idle_check: team::DEFAULT_IDLE_CHECK_MINUTES,
        open_teams: teams.lock().len() as u32,
    }
}

/// 「還原角色檔預設」。
#[tauri::command]
pub fn agent_roles_restore(settings: State<'_, Arc<SettingsStore>>) -> Vec<roles::RoleInfo> {
    let dir = roles::data_dir_or_verify(settings.dir());
    roles::restore_defaults(&dir);
    println!("[AwayTerminal] 代理團隊：角色檔已還原成預設");
    roles::list_roles(&dir)
}

/// 「開啟角色檔資料夾」要開哪裡。
#[tauri::command]
pub fn agent_roles_dir(settings: State<'_, Arc<SettingsStore>>) -> String {
    let dir = roles::data_dir_or_verify(settings.dir());
    roles::ensure_defaults(&dir);
    roles::roles_dir(&dir).to_string_lossy().to_string()
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
    let dir = setup.dir.trim();
    if dir.is_empty() || !std::path::Path::new(dir).is_dir() {
        return Err(crate::i18n::tf("ma.dlgFolderMissing", &[dir]));
    }
    let open: Vec<u32> = teams.lock().iter().map(|t| t.number).collect();
    let number = team::next_free_number(&open, 0);
    if number == 0 {
        return Err(crate::i18n::t("ma.tooMany"));
    }
    let data_dir = roles::data_dir_or_verify(settings.dir());
    let key = format!("{number}-{}", crate::tabs::now_ms());
    let mut t = Team::new(key.clone(), number, dir);
    t.max_messages = setup.max_messages;
    t.idle_check_minutes = setup.idle_check_minutes;
    t.sandbox = setup.sandbox;
    t.title = if !setup.title.trim().is_empty() && !tabs.title_taken(setup.title.trim()) {
        setup.title.trim().to_string()
    } else {
        tabs.dir_tab_name(dir, &crate::i18n::t("ma.title"))
    };
    for (i, s) in t.slots.iter_mut().enumerate() {
        let ss = setup.slots.get(i).cloned().unwrap_or_default();
        // 新開的組格 1 一定啟用（使用者就是要跟它說話）
        s.enabled = ss.enabled || i == 0;
        s.backend = ss.backend;
        s.role = ss.role;
        s.role_title = roles::title_of(&data_dir, &s.role);
    }
    for s in t.slots.iter().filter(|s| s.enabled) {
        if adapters::Backend::by_key(&s.backend).is_none() {
            return Err(crate::i18n::tf("ma.dlgNeedBackend", &[&s.agent_id()]));
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
            if let Some(conn) = adapters::resolve(&settings, b) {
                let files = crate::sandbox::write_guardrails(&work, &conn.path);
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

    // 信箱在 agent 真正工作的資料夾裡（有沙盒＝worktree）
    bus::ensure_gitignore(std::path::Path::new(&t.work_dir));
    roles::clear_session(&data_dir, number);
    // 名單要完整才組得對（隊友清單是「已啟用」的格）→ 先全部填好再一次組
    for i in 0..t.slots.len() {
        if !t.slots[i].enabled {
            continue;
        }
        match roles::compose(&data_dir, &t, t.slots[i].index) {
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
        "[AwayTerminal] 代理團隊 {number} 建立：目錄={} 工作區={} agents={}",
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
        let _ = roles::compose(&data_dir, t, i);
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
        if let Some(t) = list.iter_mut().find(|t| t.key == key) {
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
    println!("[AwayTerminal] 代理團隊 {key}：{any} 個 agent 就緒，開始監看 {work_dir}\\.ai\\bus");
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

// ---------------------------------------------------------------- --verify
// i18n-audit:log-only-begin 這一段只在 `--verify` 跑（驗證輸出），使用者不會看到；不進八語表


/// `--verify` 的準備：用**假 agent**（`examples/fake_agent.rs` 編出來的執行檔）當 provider，
/// 並在 `%TEMP%` 開一個空專案。回傳那個專案資料夾。
///
/// ⚠️ 這條路**不動使用者的任何東西**：不寫 settings.json、不碰使用者的 `.ai/`、
/// 不啟動真的 claude／codex。覆寫只活在記憶體裡，[`agent_verify_end`] 會清掉。
#[tauri::command]
pub fn agent_verify_begin() -> Result<String, String> {
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
    let dir = std::env::temp_dir().join(format!("awayterm-verify-team-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    adapters::set_verify_exe(Some(exe.to_string_lossy().to_string()));
    // 角色檔也寫在 %TEMP%，不碰使用者真的 multiagent 資料夾
    roles::set_verify_data_dir(Some(dir.join("appdata")));
    println!(
        "[AwayTerminal] --verify 代理團隊：假 agent={} 專案={}",
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
    }
}

/// `--verify` 收尾：清掉假 agent 的覆寫，並刪掉 `%TEMP%` 的那個專案資料夾。
#[tauri::command]
pub fn agent_verify_end(dir: String) -> String {
    adapters::set_verify_exe(None);
    roles::set_verify_data_dir(None);
    let p = std::path::PathBuf::from(&dir);
    // 只刪自己在 %TEMP% 底下建的那一個（名字要對得上，免得刪錯東西）
    let name_ok = p
        .file_name()
        .map(|n| n.to_string_lossy().starts_with("awayterm-verify-team-"))
        .unwrap_or(false);
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
        // 點分頁列那一列 → 最後點過的那一格
        assert_eq!(m.focus_target(5), 5);
        m.lock()[0].last_focused = Some(6);
        assert_eq!(m.focus_target(5), 6);
        // 那一格已經關掉了 → 回代表列自己
        m.lock()[0].slots[1].tab = None;
        assert_eq!(m.focus_target(5), 5);
    }
}
