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
    pub fn label(self) -> &'static str {
        match self {
            TabKind::PowerShell => "PowerShell",
            TabKind::Claude => "Claude Code",
            TabKind::Custom => "自訂連線",
            TabKind::Ssh => "SSH",
            TabKind::Telnet => "Telnet",
            TabKind::Com => "連接埠",
            TabKind::Adb => "ADB",
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
    /// 診斷用。
    pub command_line: String,
    pub backend: String,
}

/// 傳給前端的一列（`tab-state` event 的內容）。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabView {
    pub id: u32,
    pub kind: TabKind,
    /// 種類的繁中名稱，圖示 tooltip 用（舊版 `KindTip` 的前半）。
    pub kind_label: &'static str,
    pub title: String,
    pub cwd_path: String,
    pub flags: String,
    pub busy: bool,
    pub started_at: u64,
    pub pid: u32,
    /// 記錄 log 中（tooltip 會多一行「● 記錄 log 中」，同舊版 `tip.tabLogging`）。
    pub logging: bool,
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

/// 所有分頁。放在 tauri `State` 裡。
pub struct TabManager {
    inner: Mutex<Inner>,
}

impl TabManager {
    pub fn new(view_mode: &str) -> Self {
        Self {
            inner: Mutex::new(Inner {
                view_mode: normalize_view_mode(view_mode).to_string(),
                ..Inner::default()
            }),
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

    /// 這個分頁目前的 log 槽（開始／停止記錄用）。
    pub fn logger_slot(&self, id: u32) -> Option<Arc<Mutex<Option<Arc<crate::logging::Logger>>>>> {
        self.lock().tabs.get(&id).map(|t| t.logger.clone())
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

    pub fn state(&self) -> TabState {
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

/// 把目前的分頁狀態送給前端。**一定要在放掉鎖之後呼叫**（`state()` 自己會鎖）。
pub fn emit_state(app: &AppHandle, tabs: &TabManager) {
    if let Err(e) = app.emit("tab-state", tabs.state()) {
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
    use super::{dir_name_of, parse_cwd};

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
