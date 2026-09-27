//! 分頁模型與分頁清單。
//!
//! 對應舊版 `Models/TerminalTab.cs` 與 `MainWindow` 裡的 `Tabs` 集合。
//!
//! ## 為什麼分頁狀態不走舊字串協定
//! 舊版的分頁列是 **WPF**（`MainWindow.xaml` 的 `TabStrip`），不在 WebView2 裡，
//! 所以舊協定裡根本沒有「分頁列」相關的訊息——`n`/`s`/`t`/`x`/`K` 只是拿來同步
//! **分割模式的 pane**，狀態燈、tooltip、執行時間全都留在 C# 那邊。
//!
//! 新版分頁列改用 HTML 做，就需要一條把分頁狀態送進前端的路。這裡**不發明新的
//! 單字母協定**（那會讓 `docs/PROTOCOL.md` 的 31 條對照失真），改用一個獨立的
//! tauri event `tab-state`，payload 是 JSON。`terminal.js` 完全看不到它，
//! 舊協定也一個字都沒變。

use crate::i18n::{t};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

/// 連線種類。對應舊版 `TermKind`；這個階段只會建出前三種，
/// 其餘留在列舉裡讓之後的後端任務直接填。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TabKind {
    PowerShell,
    Claude,
    Custom,
    Ssh,
    Telnet,
    Com,
    Adb,
}

impl TabKind {
    /// 舊版 `TerminalTab` 建構子裡的 `KindKey` → 繁中顯示字（`Localization/Loc.cs`）。
    pub fn label(self) -> String {
        match self {
            TabKind::PowerShell => "PowerShell".to_string(),
            TabKind::Claude => "Claude Code".to_string(),
            TabKind::Custom => t("kind.custom"),
            TabKind::Ssh => "SSH".to_string(),
            TabKind::Telnet => "Telnet".to_string(),
            TabKind::Com => t("kind.com"),
            TabKind::Adb => "ADB".to_string(),
        }
    }

    /// 舊版 `UpdateStatuses` 依種類用不同的忙碌判斷；這裡只留下判斷所需的分類。
    pub fn is_local_shell(self) -> bool {
        matches!(self, TabKind::PowerShell)
    }

    /// 舊版 `TracksCwdTitle`：依提示行的目前目錄自動命名的種類
    /// （PowerShell / SSH / Telnet / 自訂 shell；Claude 直跑與 ADB 不算）。
    fn tracks_cwd_title(self) -> bool {
        matches!(
            self,
            TabKind::PowerShell | TabKind::Ssh | TabKind::Telnet | TabKind::Custom
        )
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 一個分頁。
pub struct Tab {
    pub id: u32,
    pub kind: TabKind,
    pub title: String,
    /// 使用者手動改過名 → 不再依目前目錄自動改名（舊版 `TitleLocked`）。
    pub title_locked: bool,
    /// 由提示字元行解析到的目前路徑（tooltip 第二行）。
    pub cwd_path: String,
    /// 舊版 `n` 協定第三欄。目前只有 `c`＝claude 分頁。
    pub flags: String,
    pub pid: u32,
    /// 分頁開啟時間（epoch ms）。tooltip 的「執行 日:時:分」從這裡算。
    pub started_at: u64,
    /// 最後一次收到輸出的時間（epoch ms）。狀態燈用。
    ///
    /// 用 `Arc<AtomicU64>` 而不是欄位直接存：輸出 callback 跑在 PTY 讀取執行緒上、
    /// 每個 chunk 都會更新一次，不能為了它去搶整個分頁清單的鎖。
    pub last_output: Arc<AtomicU64>,
    /// 最後一次**使用者輸入**的時間（epoch ms；0＝還沒打過）。
    ///
    /// 只有 `i` 協定那條路（`session_write_text`）會更新它，所以程式自己貼進去的字
    /// 也算——同舊版（`MainWindow.xaml.cs` 的 `case 'i'` 就是在那裡設 `LastInputUtc`）。
    /// 代理團隊的「這格現在可以打字給它嗎」要看它（[`crate::agent::deliver::agent_ready`]）。
    pub last_input: Arc<AtomicU64>,
    /// 最後一次**送出**（輸入裡含 CR／或我們自己補的 Enter）的時間（epoch ms）。
    /// 同舊版 `LastSubmitUtc`。
    pub last_submit: Arc<AtomicU64>,
    /// 狀態燈：忙碌（紅）／閒置（綠）。由 `status.rs` 的輪詢更新。
    pub busy: bool,
    /// 這個分頁的 log 記錄器（`None`＝沒在記錄）。
    ///
    /// 用 `Arc<Mutex<...>>` 是因為 PTY 的輸出 callback 在 spawn 當下就建好了，
    /// 而「開始記錄」是之後才按的——callback 需要一個可以事後填入的槽。
    pub logger: Arc<Mutex<Option<Arc<crate::logging::Logger>>>>,
    /// 逐分頁配色（`P` 協定）。`None`＝用設定的預設色。
    ///
    /// 只留在記憶體、不進 settings.json：分頁 id 跨重啟沒有意義，要持久化得等
    /// 「恢復分頁」（階段 3）。色票清單本身在 `settings.palette`。
    pub fg: Option<String>,
    pub bg: Option<String>,
    /// 這個分頁的輸出管線與 log 槽。斷線重連要沿用它們（同一條 channel）。
    pub out: Option<Arc<crate::output::OutputPump>>,
    /// TTL 巨集的輸入／輸出攔截槽（見 `src/tap.rs`）。
    pub tap: crate::tap::TapSlot,
    /// 正在跑的巨集（`None`＝沒有）。舊版是 `TerminalTab.IsMacroRunning`（只影響 tooltip），
    /// 我們多了檔名與目前行號，分頁列也看得見。
    pub macro_handle: Option<Arc<crate::ttl::runner::MacroHandle>>,
    /// 最後一次由前端回報的尺寸（重連時用，同舊版 `tab.Cols/Rows`）。
    pub cols: u16,
    pub rows: u16,
    /// 遠端連線參數（`None`＝本機分頁）。重連、我的最愛、恢復分頁都用這個結構。
    pub conn: Option<crate::reconnect::ConnParams>,
    /// 連續重連次數（退避用；一收到輸出就歸零，同舊版 `ReconnectAttempt`）。
    pub reconnect_attempt: u32,
    /// 目前這條重連鏈的世代。排程時記下，醒來對不上就放棄（同舊版「一個分頁一條鏈」）。
    pub reconnect_gen: u64,
    /// 沙盒模式的配置（`None`＝沒開沙盒）。
    pub sandbox: Option<crate::sandbox::Sandbox>,
    /// 這個分頁是哪一條自訂連線開的（右鍵切換沙盒、重新啟動分頁要用）。
    pub conn_name: Option<String>,
    /// 啟動時的工作目錄，**沙盒改寫之前**的那一個（恢復分頁要存這個，
    /// 不能存 worktree 路徑——否則下次會在沙盒裡再開一層沙盒）。
    pub work_dir: String,
    /// 診斷用。
    pub command_line: String,
    pub backend: String,
}

/// 重連要沿用的既有管線（見 `TabManager::session_parts_of`）。
pub struct SessionParts {
    pub pump: Arc<crate::output::OutputPump>,
    pub logger: Arc<Mutex<Option<Arc<crate::logging::Logger>>>>,
    pub last_output: Arc<AtomicU64>,
    /// TTL 巨集的攔截槽（現在一定是空的，見 `src/tap.rs`）。
    pub tap: crate::tap::TapSlot,
    pub cols: u16,
    pub rows: u16,
}

/// 一格 agent 的閒／忙時間戳（epoch ms；0＝還沒發生過）。
pub struct AgentSignals {
    pub busy: bool,
    pub last_output: u64,
    pub last_input: u64,
    pub last_submit: u64,
}

/// 傳給前端的一列（`tab-state` event 的內容）。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabView {
    pub id: u32,
    pub kind: TabKind,
    /// 種類的繁中名稱，圖示 tooltip 用（舊版 `KindTip` 的前半）。
    pub kind_label: String,
    pub title: String,
    pub cwd_path: String,
    pub flags: String,
    pub busy: bool,
    pub started_at: u64,
    pub pid: u32,
    /// 記錄 log 中（tooltip 會多一行「● 記錄 log 中」，同舊版 `tip.tabLogging`）。
    pub logging: bool,
    /// 沙盒模式的狀態（給 tooltip 與分頁列小標記）。`None`＝這個分頁沒有沙盒。
    pub sandbox: Option<crate::sandbox::Sandbox>,
    /// 這條自訂連線**設定上**有沒有開沙盒（右鍵選單的勾勾要顯示設定值，
    /// 不是目前分頁的狀態——改設定是下次啟動才生效）。
    pub conn_sandbox: Option<bool>,
    pub conn_name: Option<String>,
    /// 目前沒有連線、但這個分頁可以重連（SSH 分頁斷線後）。
    pub reconnectable: bool,
    /// 正在等自動重連（退避倒數中）。
    pub reconnect_attempt: u32,
    /// 正在跑的 TTL 巨集（`None`＝沒有）。**新增**：舊版只在 tooltip 提一句，
    /// 我們讓分頁列也看得到（檔名 + 目前行號）。
    pub macro_state: Option<crate::ttl::runner::MacroState>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabState {
    pub tabs: Vec<TabView>,
    pub active_id: Option<u32>,
    pub view_mode: String,
}

#[derive(Default)]
struct Inner {
    /// 分頁順序（分頁列由上而下；也是 `K`/`k` 協定的順序）。
    order: Vec<u32>,
    tabs: HashMap<u32, Tab>,
    active: Option<u32>,
    view_mode: String,
}

/// 一個分頁剛從忙轉閒（[`TabManager::busy_transitions`] 回的）。
///
/// 時間單位全是毫秒；要不要真的推播由 [`crate::status::should_push`] 決定（照舊版
/// `UpdateStatuses` 那一段的規則），這裡只負責把事實算出來。
#[derive(Debug, Clone, Copy)]
pub struct IdleEvent {
    pub id: u32,
    /// 這段忙碌的起點（epoch ms）。
    pub busy_since: u64,
    /// 這段忙碌持續了多久。
    pub busy_ms: u64,
    /// 最後一次送出的時間（epoch ms；0＝沒送過）。
    pub last_submit: u64,
    /// 距最後一次使用者輸入多久。
    pub since_input_ms: u64,
}

/// 所有分頁。放在 tauri `State` 裡。
pub struct TabManager {
    inner: Mutex<Inner>,
    /// 每個分頁「這一段忙碌是什麼時候開始的」（epoch ms）。
    ///
    /// 和 `Tab.busy` 分開放，因為它只有 Telegram 遠端的完成推播要用（舊版也是放在
    /// `MainWindow._busySince`，不在分頁物件上）。轉閒時移除。
    busy_since: Mutex<HashMap<u32, u64>>,
}

impl TabManager {
    pub fn new(view_mode: &str) -> Self {
        Self {
            inner: Mutex::new(Inner {
                view_mode: normalize_view_mode(view_mode).to_string(),
                ..Inner::default()
            }),
            busy_since: Mutex::new(HashMap::new()),
        }
    }

    pub fn insert(&self, tab: Tab) {
        let mut inner = self.lock();
        inner.order.push(tab.id);
        inner.tabs.insert(tab.id, tab);
    }

    /// 移除一個分頁，回傳「接著該選誰」（舊版 `RemoveTabSilently`：
    /// 關掉作用中那個就選原位置的分頁，沒有就選最後一個）。
    pub fn remove(&self, id: u32) -> Option<u32> {
        let mut inner = self.lock();
        let idx = inner.order.iter().position(|&x| x == id)?;
        inner.order.remove(idx);
        inner.tabs.remove(&id);
        if inner.active != Some(id) {
            return None;
        }
        inner.active = None;
        if inner.order.is_empty() {
            return None;
        }
        let next = inner.order[idx.min(inner.order.len() - 1)];
        inner.active = Some(next);
        Some(next)
    }

    pub fn contains(&self, id: u32) -> bool {
        self.lock().tabs.contains_key(&id)
    }

    pub fn set_active(&self, id: u32) -> bool {
        let mut inner = self.lock();
        if !inner.tabs.contains_key(&id) {
            return false;
        }
        inner.active = Some(id);
        true
    }

    pub fn active(&self) -> Option<u32> {
        self.lock().active
    }

    pub fn ids(&self) -> Vec<u32> {
        self.lock().order.clone()
    }

    pub fn view_mode(&self) -> String {
        self.lock().view_mode.clone()
    }

    pub fn set_view_mode(&self, mode: &str) -> String {
        let mode = normalize_view_mode(mode).to_string();
        self.lock().view_mode = mode.clone();
        mode
    }

    /// 三態循環：分頁 → 分割 → 分欄 → 分頁（舊版 `Split_Click`）。
    pub fn cycle_view_mode(&self) -> String {
        let mut inner = self.lock();
        inner.view_mode = match inner.view_mode.as_str() {
            "tab" => "split",
            "split" => "columns",
            _ => "tab",
        }
        .to_string();
        inner.view_mode.clone()
    }

    /// 改名。回傳 true＝真的變了（呼叫端才需要 emit `t`）。
    pub fn set_title(&self, id: u32, title: &str, lock_title: bool) -> bool {
        let mut inner = self.lock();
        let Some(tab) = inner.tabs.get_mut(&id) else {
            return false;
        };
        if lock_title {
            tab.title_locked = true;
        }
        if tab.title == title {
            return false;
        }
        tab.title = title.to_string();
        true
    }

    /// 提示字元行解析出來的路徑 → 更新 tooltip 的路徑並自動改名（舊版 `UpdateDirTitle`）。
    ///
    /// 回傳 `Some(新標題 or None)`＝路徑有變（tooltip 要重畫）；內層的 `Some` 代表
    /// 標題也跟著變了、呼叫端要 emit `t`。解析不到提示行就回 `None`，什麼都不動
    /// （claude 這類 TUI 沒有提示行，名稱會停在啟動當下的目錄——同舊版）。
    pub fn apply_cwd(&self, id: u32, prompt_line: &str) -> Option<Option<String>> {
        let path = parse_cwd(prompt_line)?;
        let mut inner = self.lock();
        let tab = inner.tabs.get_mut(&id)?;
        if !tab.kind.tracks_cwd_title() || tab.title_locked {
            return None;
        }
        if tab.cwd_path == path {
            return None;
        }
        tab.cwd_path = path.clone();
        let name = dir_name_of(&path);
        if name.is_empty() || tab.title == name {
            return Some(None);
        }
        // 「名稱(2)」＝同目錄的第二個分頁，保留（舊版同款判斷）
        if let Some(rest) = tab.title.strip_prefix(&name) {
            if rest.starts_with('(') && rest.ends_with(')') {
                return Some(None);
            }
        }
        tab.title = name.clone();
        Some(Some(name))
    }

    /// 依 id 清單重排（舊版 `ReorderTabs`：認得的往前放，認不得的略過）。
    pub fn reorder(&self, ids: &[u32]) {
        let mut inner = self.lock();
        let mut next: Vec<u32> = Vec::with_capacity(inner.order.len());
        for &id in ids {
            if inner.tabs.contains_key(&id) && !next.contains(&id) {
                next.push(id);
            }
        }
        // 沒被列到的（新開、或前端漏送）保持原相對順序接在後面
        for &id in &inner.order {
            if !next.contains(&id) {
                next.push(id);
            }
        }
        inner.order = next;
    }

    /// 狀態燈輪詢要的東西：(id, kind, pid, 最後輸出時間)。
    pub fn poll_snapshot(&self) -> Vec<(u32, TabKind, u32, u64)> {
        let inner = self.lock();
        inner
            .order
            .iter()
            .filter_map(|id| inner.tabs.get(id))
            .map(|t| (t.id, t.kind, t.pid, t.last_output.load(Ordering::Relaxed)))
            .collect()
    }

    /// 這一輪有哪些分頁剛從忙轉閒（Telegram 遠端的完成推播用）。
    ///
    /// **要在 [`Self::apply_busy`] 之前呼叫**——它靠「目前記著的 `busy`」和新算出來的
    /// 比對。順手維護 `busy_since`：轉忙時記下起點，轉閒時取出並移除。
    pub fn busy_transitions(&self, busy: &[(u32, bool)]) -> Vec<IdleEvent> {
        let now = now_ms();
        let inner = self.lock();
        let mut since = self.busy_since.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        for &(id, b) in busy {
            let Some(tab) = inner.tabs.get(&id) else { continue };
            if b && !tab.busy {
                since.insert(id, now);
            } else if !b && tab.busy {
                if let Some(start) = since.remove(&id) {
                    let last_input = tab.last_input.load(Ordering::Relaxed);
                    out.push(IdleEvent {
                        id,
                        busy_since: start,
                        busy_ms: now.saturating_sub(start),
                        last_submit: tab.last_submit.load(Ordering::Relaxed),
                        since_input_ms: now.saturating_sub(last_input),
                    });
                }
            }
        }
        out
    }

    /// 套用輪詢算出來的忙碌狀態，回傳 true＝有任何一個變了（才需要 emit）。
    pub fn apply_busy(&self, busy: &[(u32, bool)]) -> bool {
        let mut inner = self.lock();
        let mut changed = false;
        for &(id, b) in busy {
            if let Some(tab) = inner.tabs.get_mut(&id) {
                if tab.busy != b {
                    tab.busy = b;
                    changed = true;
                }
            }
        }
        changed
    }

    /// 哪些分頁要問 `q{id}US cwd`（舊版 `TracksCwdTitle`，外加作用中那個一定問）。
    pub fn cwd_query_ids(&self) -> Vec<u32> {
        let inner = self.lock();
        inner
            .order
            .iter()
            .filter_map(|id| inner.tabs.get(id))
            .filter(|t| t.kind.tracks_cwd_title() && !t.title_locked)
            .map(|t| t.id)
            .collect()
    }

    /// 同一個標題已經被用掉了嗎（`NextName` / `DirTabName` 用）。
    pub fn title_taken(&self, title: &str) -> bool {
        self.lock().tabs.values().any(|t| t.title == title)
    }

    /// 舊版 `NextName`：`PowerShell(1)`、`PowerShell(2)`…
    pub fn next_name(&self, prefix: &str) -> String {
        let prefix = if prefix.is_empty() { "Custom" } else { prefix };
        let mut n = 0;
        loop {
            n += 1;
            let name = format!("{prefix}({n})");
            if !self.title_taken(&name) {
                return name;
            }
        }
    }

    /// 舊版 `DirTabName`：用目錄名稱當分頁名，重複就補 `(2)`、`(3)`。
    pub fn dir_tab_name(&self, dir: &str, prefix: &str) -> String {
        let name = dir_name_of(dir);
        if name.is_empty() {
            return self.next_name(prefix);
        }
        if !self.title_taken(&name) {
            return name;
        }
        let mut n = 1;
        loop {
            n += 1;
            let dup = format!("{name}({n})");
            if !self.title_taken(&dup) {
                return dup;
            }
        }
    }

    /// 代理團隊的閒／忙判斷要的所有時間戳（見 `agent/deliver.rs`）。
    /// 分頁不在了＝`None`（那一格已經關掉）。
    pub fn agent_signals(&self, id: u32) -> Option<AgentSignals> {
        let inner = self.lock();
        let t = inner.tabs.get(&id)?;
        Some(AgentSignals {
            busy: t.busy,
            last_output: t.last_output.load(Ordering::Relaxed),
            last_input: t.last_input.load(Ordering::Relaxed),
            last_submit: t.last_submit.load(Ordering::Relaxed),
        })
    }

    /// 記一次使用者輸入（`i` 協定）。含 CR 就同時算一次「送出」。
    pub fn mark_input(&self, id: u32, submitted: bool) {
        let inner = self.lock();
        if let Some(t) = inner.tabs.get(&id) {
            let now = now_ms();
            t.last_input.store(now, Ordering::Relaxed);
            if submitted {
                t.last_submit.store(now, Ordering::Relaxed);
            }
        }
    }

    /// 記一次「我們自己送出了一行」（代理團隊投遞、遠端指令）。
    pub fn mark_submit(&self, id: u32) {
        let inner = self.lock();
        if let Some(t) = inner.tabs.get(&id) {
            t.last_submit.store(now_ms(), Ordering::Relaxed);
        }
    }

    /// 這個分頁目前的 log 槽（開始／停止記錄用）。
    pub fn logger_slot(&self, id: u32) -> Option<Arc<Mutex<Option<Arc<crate::logging::Logger>>>>> {
        self.lock().tabs.get(&id).map(|t| t.logger.clone())
    }

    /// 重連要沿用的東西（輸出管線、log 槽、最後回報的尺寸）。
    pub fn session_parts_of(&self, id: u32) -> Option<SessionParts> {
        let inner = self.lock();
        let t = inner.tabs.get(&id)?;
        Some(SessionParts {
            pump: t.out.clone()?,
            logger: t.logger.clone(),
            last_output: t.last_output.clone(),
            tap: t.tap.clone(),
            cols: t.cols,
            rows: t.rows,
        })
    }

    /// 正在跑的巨集（`None`＝沒有）。
    pub fn macro_of(&self, id: u32) -> Option<Arc<crate::ttl::runner::MacroHandle>> {
        self.lock().tabs.get(&id).and_then(|t| t.macro_handle.clone())
    }

    /// 掛上／拿掉巨集。
    pub fn set_macro(&self, id: u32, h: Option<Arc<crate::ttl::runner::MacroHandle>>) {
        if let Some(t) = self.lock().tabs.get_mut(&id) {
            t.macro_handle = h;
        }
    }

    /// 巨集的 `connect` 用：把這個分頁的連線參數換掉（原本沒有連線參數的分頁也可以）。
    pub fn set_conn(&self, id: u32, params: crate::reconnect::ConnParams) {
        if let Some(t) = self.lock().tabs.get_mut(&id) {
            t.conn = Some(params);
        }
    }

    /// 這個分頁的攔截槽（TTL 巨集用；分頁不存在時回 `None`）。
    pub fn tap_of(&self, id: u32) -> Option<crate::tap::TapSlot> {
        self.lock().tabs.get(&id).map(|t| t.tap.clone())
    }

    /// 這個分頁目前記住的尺寸。
    pub fn size_of(&self, id: u32) -> Option<(u16, u16)> {
        self.lock().tabs.get(&id).map(|t| (t.cols, t.rows))
    }

    /// 前端回報的尺寸（重連時要用最新的，不是當初開分頁的）。
    pub fn set_size(&self, id: u32, cols: u16, rows: u16) {
        if let Some(t) = self.lock().tabs.get_mut(&id) {
            if cols > 0 && rows > 0 {
                t.cols = cols;
                t.rows = rows;
            }
        }
    }

    /// 可以恢復的分頁（照分頁列的順序），回 (分頁 id, 存檔內容)。
    ///
    /// **代理團隊的格也在裡面**（它們是自訂連線開的分頁）：`restore::save` 之後會叫
    /// `agent::annotate_saved()` 把那幾筆改寫成 `kind = "agent"` 並補上組的資訊。
    /// 分兩步是因為分頁清單與團隊清單是兩個鎖，同時拿會有死鎖風險。
    ///
    /// 舊版的判斷是「`tab.Restore != null`」——也就是「知道怎麼重開它」。我們照同一個語意：
    /// PowerShell（記工作目錄）、自訂連線（記連線名稱＋目錄）、SSH／Telnet（記連線參數）。
    /// 自訂指令（`--cmd`、`?cmd=` 開的）不在其中——舊版也沒有那條路的重開資訊。
    pub fn restorable(&self) -> Vec<(u32, crate::restore::SavedTab)> {
        let inner = self.lock();
        let mut out = Vec::new();
        for id in &inner.order {
            let Some(t) = inner.tabs.get(id) else { continue };
            let base = crate::restore::SavedTab {
                title: t.title.clone(),
                opened_ms: t.started_at,
                ..Default::default()
            };
            let entry = match (&t.conn, t.kind) {
                // 遠端連線：參數就是重開所需的一切（不含密碼）
                (Some(conn), _) => crate::restore::SavedTab {
                    kind: match conn {
                        crate::reconnect::ConnParams::Ssh(_) => "ssh".to_string(),
                        crate::reconnect::ConnParams::Telnet(_) => "telnet".to_string(),
                        crate::reconnect::ConnParams::Com(_) => "com".to_string(),
                    },
                    conn: Some(conn.clone()),
                    ..base
                },
                // 自訂連線：名稱＋工作目錄（沙盒下次啟動時自己重新準備）
                (None, TabKind::Claude | TabKind::Custom) => match &t.conn_name {
                    Some(name) => crate::restore::SavedTab {
                        kind: "conn".to_string(),
                        conn_name: name.clone(),
                        dir: t.work_dir.clone(),
                        ..base
                    },
                    None => continue, // 自訂指令：舊版也沒有重開資訊
                },
                (None, TabKind::PowerShell) => crate::restore::SavedTab {
                    kind: "shell".to_string(),
                    dir: t.work_dir.clone(),
                    ..base
                },
                _ => continue,
            };
            out.push((*id, entry));
        }
        out
    }

    /// 恢復分頁時把最初的開啟時間填回去（tooltip 的執行時長不歸零，舊版 1.1.4）。
    pub fn set_started_at(&self, id: u32, ms: u64) {
        if ms > 0 {
            if let Some(t) = self.lock().tabs.get_mut(&id) {
                t.started_at = ms;
            }
        }
    }

    /// 這個分頁的遠端連線參數（`None`＝本機分頁，不能重連）。
    pub fn conn_params_of(&self, id: u32) -> Option<crate::reconnect::ConnParams> {
        self.lock().tabs.get(&id).and_then(|t| t.conn.clone())
    }

    /// 只要 SSH 那一種（我的最愛／對話框用）。
    pub fn ssh_params_of(&self, id: u32) -> Option<crate::ssh::conn::SshConnParams> {
        self.conn_params_of(id).and_then(|c| c.as_ssh().cloned())
    }

    /// 只要 Telnet 那一種。
    pub fn telnet_params_of(&self, id: u32) -> Option<crate::telnet::TelnetParams> {
        self.conn_params_of(id).and_then(|c| c.as_telnet().cloned())
    }

    /// 只要 COM 那一種。
    pub fn com_params_of(&self, id: u32) -> Option<crate::com::ComParams> {
        self.conn_params_of(id).and_then(|c| c.as_com().cloned())
    }

    /// 登入之後把帳號記進參數：重連就不必再問 `login as:`
    /// （同舊版把 `Restore.Host` 改成 `user@host`）。
    pub fn set_ssh_user(&self, id: u32, user: &str) {
        if let Some(t) = self.lock().tabs.get_mut(&id) {
            if let Some(crate::reconnect::ConnParams::Ssh(p)) = t.conn.as_mut() {
                p.user = user.to_string();
            }
        }
    }

    /// 退避次數 +1 並回傳新值。
    pub fn bump_reconnect_attempt(&self, id: u32) -> u32 {
        let mut inner = self.lock();
        match inner.tabs.get_mut(&id) {
            Some(t) => {
                t.reconnect_attempt += 1;
                t.reconnect_attempt
            }
            None => 1,
        }
    }

    /// 有輸出＝真的連上了 → 歸零（同舊版）。
    pub fn reset_reconnect_attempt(&self, id: u32) {
        if let Some(t) = self.lock().tabs.get_mut(&id) {
            t.reconnect_attempt = 0;
        }
    }

    pub fn set_reconnect_gen(&self, id: u32, gen: u64) {
        if let Some(t) = self.lock().tabs.get_mut(&id) {
            t.reconnect_gen = gen;
        }
    }

    pub fn reconnect_gen_of(&self, id: u32) -> Option<u64> {
        self.lock().tabs.get(&id).map(|t| t.reconnect_gen)
    }

    /// 這個分頁的沙盒配置。
    pub fn sandbox_of(&self, id: u32) -> Option<crate::sandbox::Sandbox> {
        self.lock().tabs.get(&id).and_then(|t| t.sandbox.clone())
    }

    /// 分頁標題（log 預設檔名、存檔預設檔名、關閉確認訊息用）。
    pub fn title_of(&self, id: u32) -> Option<String> {
        self.lock().tabs.get(&id).map(|t| t.title.clone())
    }

    /// 這個分頁的種類（清畫面要分「送 Esc+Ctrl+L」還是「送 `c` 清 xterm 緩衝」）。
    pub fn kind_of(&self, id: u32) -> Option<TabKind> {
        self.lock().tabs.get(&id).map(|t| t.kind)
    }

    /// 逐分頁配色（`P` 協定）。空字串＝清除覆寫、回到設定預設。
    pub fn set_colors(&self, id: u32, fg: &str, bg: &str) -> bool {
        let mut inner = self.lock();
        let Some(tab) = inner.tabs.get_mut(&id) else {
            return false;
        };
        tab.fg = (!fg.is_empty()).then(|| fg.to_string());
        tab.bg = (!bg.is_empty()).then(|| bg.to_string());
        true
    }

    /// `conns`＝目前的自訂連線清單（拿來填 `conn_sandbox`，也就是右鍵選單那個勾勾）。
    pub fn state_with(&self, conns: &[crate::settings::CustomConn]) -> TabState {
        let inner = self.lock();
        TabState {
            tabs: inner
                .order
                .iter()
                .filter_map(|id| inner.tabs.get(id))
                .map(|t| TabView {
                    id: t.id,
                    kind: t.kind,
                    kind_label: t.kind.label(),
                    title: t.title.clone(),
                    cwd_path: t.cwd_path.clone(),
                    flags: t.flags.clone(),
                    busy: t.busy,
                    started_at: t.started_at,
                    pid: t.pid,
                    logging: t
                        .logger
                        .lock()
                        .map(|g| g.is_some())
                        .unwrap_or(false),
                    sandbox: t.sandbox.clone(),
                    conn_sandbox: conn_sandbox_of(t.conn_name.as_deref(), conns),
                    conn_name: t.conn_name.clone(),
                    reconnectable: t.conn.is_some(),
                    reconnect_attempt: t.reconnect_attempt,
                    macro_state: t.macro_handle.as_ref().map(|h| {
                        crate::ttl::runner::MacroState {
                            file: h.file.clone(),
                            line: h.line.load(std::sync::atomic::Ordering::Relaxed),
                        }
                    }),
                })
                .collect(),
            active_id: inner.active,
            view_mode: inner.view_mode.clone(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 某條自訂連線**設定上**的沙盒開關（找不到連線就回 `None`）。
fn conn_sandbox_of(name: Option<&str>, conns: &[crate::settings::CustomConn]) -> Option<bool> {
    let name = name?;
    conns
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name))
        .map(|c| c.sandbox)
}

/// 把目前的分頁狀態送給前端。**一定要在放掉鎖之後呼叫**（`state_with()` 自己會鎖）。
///
/// 需要 app 的 `SettingsStore` 來填每個分頁「設定上」的沙盒開關；拿不到就送空清單
/// （右鍵選單的勾勾會暫時不顯示，但不會壞）。
pub fn emit_state(app: &AppHandle, tabs: &TabManager) {
    use tauri::Manager;
    let conns = app
        .try_state::<std::sync::Arc<crate::settings::SettingsStore>>()
        .map(|s| s.get().custom_conns)
        .unwrap_or_default();
    if let Err(e) = app.emit("tab-state", tabs.state_with(&conns)) {
        println!("[AwayTerminal] emit tab-state 失敗：{e}");
    }
}

fn normalize_view_mode(mode: &str) -> &str {
    match mode {
        "split" | "columns" => mode,
        _ => "tab",
    }
}

/// 路徑最後一段：`~/a/b/` → `b`、`/` → `/`、`C:\Users\me` → `me`、`C:\` → `C:`。
/// 逐字照抄舊版 `DirNameOf`。
pub fn dir_name_of(path: &str) -> String {
    let t = path.trim_end_matches(['/', '\\']);
    if t.is_empty() {
        return path.to_string();
    }
    match t.rfind(['/', '\\']) {
        Some(i) => {
            let last = &t[i + 1..];
            if last.is_empty() {
                path.to_string()
            } else {
                last.to_string()
            }
        }
        None => t.to_string(),
    }
}

/// 從提示字元行解析出目前目錄。
///
/// 舊版 `CwdRes` 是 8 條 regex；這裡不引入 regex crate（一個相依只為了這個不划算），
/// 改成等價的手寫解析，**規則與順序照舊版逐條對應**，註解裡標出是哪一條。
pub fn parse_cwd(line: &str) -> Option<String> {
    let line = line.trim_end();
    if line.is_empty() {
        return None;
    }

    // ① PowerShell：`PS C:\path>` / `PS /path>`
    if let Some(rest) = line.strip_prefix("PS ") {
        let rest = rest.trim_start();
        if let Some(end) = rest.rfind('>') {
            let p = rest[..end].trim();
            if is_pathish(p) {
                return Some(p.to_string());
            }
        }
    }

    // ② cmd：`C:\path>`
    if looks_like_drive(line) {
        if let Some(end) = line.find('>') {
            let p = line[..end].trim();
            if is_pathish(p) {
                return Some(p.to_string());
            }
        }
    }

    // ③ bash/zsh：`user@host:~/path$`（zsh 常用 `%`）
    if let Some((before, after)) = line.split_once(':') {
        if before.contains('@') && !before.contains(char::is_whitespace) {
            let p = after.trim_start();
            if let Some(end) = p.find([' ', '$', '#', '%']) {
                let p = &p[..end];
                if !p.is_empty() {
                    return Some(p.to_string());
                }
            }
        }
    }

    // ④ RHEL/CentOS：`[user@host ~]#`
    if let Some(rest) = line.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let inside = &rest[..end];
            if inside.contains('@') {
                if let Some((_, p)) = inside.rsplit_once(' ') {
                    if !p.is_empty() {
                        return Some(p.to_string());
                    }
                }
            }
        }
    }

    // ⑥ fish 等：`user@host /path>`；⑦⑧ macOS 預設提示（cwd 可能只是 basename）
    let mut parts = line.split_whitespace();
    if let (Some(head), Some(p)) = (parts.next(), parts.next()) {
        if head.contains('@') {
            let p = p.trim_end_matches(['>', '$', '#', '%']);
            if !p.is_empty() {
                return Some(p.to_string());
            }
        }
    }

    None
}

/// 看起來像個路徑（不是一整行英文句子）：以 `/`、`~` 或磁碟機代號開頭。
fn is_pathish(s: &str) -> bool {
    !s.is_empty() && (s.starts_with('/') || s.starts_with('~') || looks_like_drive(s))
}

fn looks_like_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

#[cfg(test)]
mod tests {
    use super::{dir_name_of, parse_cwd, Tab, TabKind, TabManager};

    fn tab(id: u32) -> Tab {
        Tab {
            id,
            kind: TabKind::Ssh,
            title: format!("t{id}"),
            title_locked: false,
            cwd_path: String::new(),
            flags: String::new(),
            pid: 0,
            started_at: 0,
            last_output: Default::default(),
            last_input: Default::default(),
            last_submit: Default::default(),
            busy: false,
            logger: Default::default(),
            fg: None,
            bg: None,
            out: None,
            tap: Default::default(),
            macro_handle: None,
            cols: 80,
            rows: 24,
            conn: Some(crate::reconnect::ConnParams::Ssh(Default::default())),
            reconnect_attempt: 0,
            reconnect_gen: 0,
            sandbox: None,
            conn_name: None,
            work_dir: String::new(),
            command_line: String::new(),
            backend: String::new(),
        }
    }

    /// 退避次數：每排一次 +1，一收到輸出就歸零（同舊版）。
    #[test]
    fn reconnect_attempt_bumps_and_resets() {
        let m = TabManager::new("tab");
        m.insert(tab(1));
        assert_eq!(m.bump_reconnect_attempt(1), 1);
        assert_eq!(m.bump_reconnect_attempt(1), 2);
        assert_eq!(m.bump_reconnect_attempt(1), 3);
        m.reset_reconnect_attempt(1);
        assert_eq!(m.bump_reconnect_attempt(1), 1, "歸零之後要從 1 重新算");
    }

    /// 一個分頁同時只有一條重連鏈：世代換過之後，舊的那條要認得出自己過期了。
    #[test]
    fn reconnect_generation_invalidates_old_chain() {
        let m = TabManager::new("tab");
        m.insert(tab(1));
        m.set_reconnect_gen(1, 7);
        assert_eq!(m.reconnect_gen_of(1), Some(7));
        m.set_reconnect_gen(1, 8); // 例如使用者按了 Enter
        assert_ne!(m.reconnect_gen_of(1), Some(7), "舊鏈醒來時要放棄");
        // 分頁關掉之後查不到世代 → 排程的執行緒也會放棄
        m.remove(1);
        assert_eq!(m.reconnect_gen_of(1), None);
    }

    /// 尺寸要記住最新的（重連用現在的大小，不是當初開分頁的）。
    #[test]
    fn size_is_remembered_for_reconnect() {
        let m = TabManager::new("tab");
        m.insert(tab(1));
        m.set_size(1, 120, 40);
        assert_eq!(m.size_of(1), Some((120, 40)));
        // 0 不可以蓋掉：分頁模式下隱藏的 pane 量不到尺寸會回報 0（舊版踩雷），
        // 那時要保留上一個有效值，不然重連會用 0×0 開 PTY
        m.set_size(1, 0, 0);
        assert_eq!(m.size_of(1), Some((120, 40)));
        // 沒有 out（還沒建 session）就拿不到重連用的 parts
        assert!(m.session_parts_of(1).is_none());
    }

    /// 登入後記住帳號：重連不必再問 `login as:`
    #[test]
    fn ssh_user_is_remembered_after_login() {
        let m = TabManager::new("tab");
        m.insert(tab(1));
        assert_eq!(m.ssh_params_of(1).map(|p| p.user), Some(String::new()));
        m.set_ssh_user(1, "root");
        assert_eq!(m.ssh_params_of(1).map(|p| p.user), Some("root".to_string()));
    }

    #[test]
    fn parses_prompt_lines() {
        assert_eq!(
            parse_cwd("PS C:\\Users\\me\\Desktop>").as_deref(),
            Some("C:\\Users\\me\\Desktop")
        );
        assert_eq!(parse_cwd("C:\\Windows\\System32>").as_deref(), Some("C:\\Windows\\System32"));
        assert_eq!(parse_cwd("me@box:~/work/proj$ ").as_deref(), Some("~/work/proj"));
        assert_eq!(parse_cwd("[me@box ~]# ").as_deref(), Some("~"));
        assert_eq!(parse_cwd("me@box /srv/app> ").as_deref(), Some("/srv/app"));
        // claude 之類的 TUI 沒有提示行 → 解析不到就別亂改名
        assert_eq!(parse_cwd("╭─ Welcome to Claude Code ─╮"), None);
        assert_eq!(parse_cwd(""), None);
    }

    #[test]
    fn takes_last_path_segment() {
        assert_eq!(dir_name_of("C:\\Users\\me\\AwayTerminal2"), "AwayTerminal2");
        assert_eq!(dir_name_of("~/a/b/"), "b");
        assert_eq!(dir_name_of("C:\\"), "C:");
        assert_eq!(dir_name_of("/"), "/");
    }
}
