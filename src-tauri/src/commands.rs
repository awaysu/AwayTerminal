//! 前端可呼叫的 tauri command。
//!
//! 語意對得上舊版的 C#↔JS 字串協定（建立 / 輸入 / 尺寸 / 輸出 / 結束），
//! 但傳輸改成 tauri command + `Channel`，輸出走原始位元組、不再經 base64。
//!
//! 分頁管理（TASK-004）分成兩組，**不要混用**：
//!   - `tab_*`：分頁列 UI 按下去的動作 → 改模型 **並** 發對應的舊協定給 `terminal.js`。
//!   - `pane_*`：`terminal.js` 那邊先發生的事（`p` / `k`）→ 只改模型，**不回送**，
//!     否則會和前端打乒乓（舊版 `case 'p'` 的註解就是「不回送避免迴圈」）。

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, State};

use crate::host::emit_host;
use crate::output::OutputPump;
use crate::pty::{self, shell, SpawnOptions};
use crate::session::{ExitInfo, SessionManager, TerminalSession};
use crate::settings::{AppSettings, SettingsStore};
use crate::tabs::{self, Tab, TabKind, TabManager};

/// `session_create` 的回覆。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: u32,
    pub pid: u32,
    /// 實際啟動的命令列（診斷用）。
    pub command_line: String,
    /// `pwsh` / `powershell`，自訂指令則是執行檔名。
    pub shell: String,
    /// `conpty.dll (OpenConsole)` 或 `inbox conhost`。
    pub backend: String,
    /// 舊版 `n` 協定第三欄的 flags。目前只有 `c`＝claude 分頁（貼上走 ESC+CR）。
    pub flags: String,
    /// 分頁標題（`n` 協定第二欄）。
    pub title: String,
}

/// 連線結束通知。走同一條 channel，但以 JSON 送出——前端看
/// 「收到的是 ArrayBuffer 還是物件」來區分輸出與事件。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ExitEvent {
    kind: &'static str,
    id: u32,
    exit_code: Option<i32>,
}

/// 最小 IPC 驗證用指令。
#[tauri::command]
pub fn ping() -> String {
    format!(
        "pong from AwayTerminal {} (tauri {}, {})",
        env!("CARGO_PKG_VERSION"),
        tauri::VERSION,
        std::env::consts::OS
    )
}

/// 前端把實際使用的渲染器（WebGL / DOM）回報到啟動 log，
/// 讓不開 devtools 也能確認 WebGL addon 有沒有成功啟用。
#[tauri::command]
pub fn report_renderer(renderer: String) {
    println!("[AwayTerminal] renderer = {renderer}");
}

/// 前端把一行診斷訊息印到後端 stdout（IPC bench 結果等），
/// 這樣不必開 devtools、也不必看視窗就能從啟動 log 拿到數據。
#[tauri::command]
pub fn log_line(msg: String) {
    println!("[AwayTerminal] {msg}");
}

/// 目前使用的 ConPTY 主機（前端可印在終端第一行）。
#[tauri::command]
pub fn conpty_backend() -> String {
    pty::backend_name()
}

// ------------------------------------------------------------------ 連線

/// 新分頁的預設工作目錄：桌面（同舊版 `ReopenHistory` 的 `deskDir()`）。
///
/// 舊版從工具列開 PowerShell 會先跳資料夾選擇視窗；那個對話框屬於之後的任務，
/// 在它做出來之前用桌面當預設，跟舊版的後備路徑一致。
fn default_cwd() -> Option<String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    let desktop = std::path::Path::new(&home).join("Desktop");
    if desktop.is_dir() {
        Some(desktop.to_string_lossy().to_string())
    } else {
        Some(home)
    }
}

/// SSH 連線的額外參數（`kind = "ssh"` 時才看）。
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshArgs {
    pub host: String,
    pub port: Option<u16>,
    /// 給了就不問 `login as:`（我的最愛／恢復分頁之後會用到）。
    pub user: Option<String>,
    /// 私鑰檔（OpenSSH 或 `.ppk`）。
    pub key_path: Option<String>,
    /// 要不要試 Pageant／ssh-agent。預設試（找不到就安靜跳過）。
    pub use_agent: Option<bool>,
    /// 這條連線的演算法覆寫（B4 的「進階」區）。省略＝用 PuTTY 式的預設順序。
    pub algos: Option<crate::ssh::algos::AlgoOverride>,
    /// 保持連線的間隔（分鐘）。省略＝用設定裡的值。
    pub keepalive_mins: Option<u32>,
}

/// 開一條連線。
///
/// - `kind = "shell"`（舊稱 `powershell`）：pwsh 優先、否則 powershell。
/// - `kind = "custom"`：用 `command` 給的指令（分頁列「自訂指令…」與 `?cmd=` 這類 dev 入口）。
/// - `kind = "ssh"`：內建 SSH（`russh`），參數走 `ssh`（見 [`SshArgs`]）。
///
/// 建好之後依序 emit 舊協定的 `n{id}US{title}[US{flags}]` 與 `s{id}`——
/// **`s` 不能漏**：少了它 `terminal.js` 的 `active` 會留在 null、`refit()` 直接 return，
/// pane 永遠不 fit 也不回報尺寸（TASK-003 實際踩過）。
// tauri command 的參數是前端傳來的具名欄位，攤平是這個框架的慣例；
// 包成 struct 會讓 JS 那邊變成 `{ args: {...} }`，反而難讀。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn session_create(
    app: AppHandle,
    kind: String,
    command: Option<String>,
    title: Option<String>,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    ssh: Option<SshArgs>,
    // `kind = "conn"`：要開哪一條自訂連線（依名稱）。
    conn: Option<String>,
    on_event: Channel<InvokeResponseBody>,
    manager: State<'_, SessionManager>,
    tabs_state: State<'_, Arc<TabManager>>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<SessionInfo, String> {
    if kind == "ssh" {
        let args = ssh.ok_or_else(|| "kind=ssh 需要 ssh 參數".to_string())?;
        return create_ssh(
            app,
            args,
            title,
            cols,
            rows,
            on_event,
            &manager,
            &tabs_state,
            &settings,
        );
    }

    // `kind = "conn"` ＝自訂連線（有名稱、有設定、可能有沙盒）。
    let conn_def = if kind == "conn" {
        let name = conn
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "kind=conn 需要 conn（連線名稱）".to_string())?;
        Some(
            crate::custom::find(&settings, name)
                .ok_or_else(|| format!("找不到自訂連線：{name}"))?,
        )
    } else {
        None
    };

    let mut sh = match kind.as_str() {
        // "powershell" 是 TASK-003 的舊名，留著相容 `?cmd=` 之前的呼叫
        "shell" | "powershell" => shell::powershell().ok_or_else(|| {
            "找不到 pwsh.exe 或 powershell.exe（PATH 與 System32 都沒有）".to_string()
        })?,
        "custom" => {
            let cmd = command
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "kind=custom 需要 command".to_string())?;
            shell::custom(cmd).ok_or_else(|| format!("找不到指令：{cmd}"))?
        }
        "conn" => {
            let c = conn_def.as_ref().unwrap();
            let exe = std::path::PathBuf::from(&c.path);
            if !exe.is_file() {
                return Err(format!("執行檔不存在：{}", c.path));
            }
            shell::Shell {
                command_line: String::new(), // 下面依沙盒與 via_powershell 組出來
                name: c.name.clone(),
                title: c.name.clone(),
                exe,
            }
        }
        other => return Err(format!("尚未支援的連線種類：{other}")),
    };

    // ---- 沙盒（只有自訂連線有這個選項；`CLAUDE.md`「新增功能 → 沙盒模式」）----
    let mut work_dir = cwd.clone().or_else(default_cwd);
    let mut sandbox = None;
    if let Some(c) = &conn_def {
        if c.sandbox {
            let base = std::path::PathBuf::from(work_dir.clone().unwrap_or_default());
            match crate::sandbox::prepare(&base, &c.name, &c.path) {
                Ok(sb) => {
                    work_dir = Some(sb.work_dir.clone());
                    println!(
                        "[AwayTerminal] 沙盒：{} worktree={} 分支={} 護欄={:?}",
                        sb.root,
                        sb.has_worktree,
                        if sb.branch.is_empty() { "-" } else { &sb.branch },
                        sb.guardrails
                    );
                    sandbox = Some(sb);
                }
                Err(e) => {
                    // 沙盒開不起來不該讓連線開不了：退成沒有沙盒並在 log 講清楚
                    println!("[AwayTerminal] 沙盒準備失敗，這條連線不進沙盒：{e}");
                }
            }
        }
    }

    // ---- 自訂連線的命令列（要等沙盒決定完 extra_args 才組得出來）----
    if let Some(c) = &conn_def {
        let extra = sandbox.as_ref().map(|s| s.extra_args.as_str()).unwrap_or("");
        let mut args = String::new();
        if !c.args.trim().is_empty() {
            args.push(' ');
            args.push_str(c.args.trim());
        }
        args.push_str(extra);
        sh.command_line = if c.via_powershell {
            // 舊版是「先開互動 PowerShell，尺寸就緒後再把指令打進去」（避免以 80 欄啟動）。
            // 我們的 PTY 一開始就是前端回報的真實尺寸，所以直接用 -NoExit -Command 起——
            // 結果一樣（工具跑完仍留在 shell 裡），少一套延後打字的機制。
            let ps = shell::powershell()
                .ok_or_else(|| "找不到 PowerShell（via_powershell 需要它）".to_string())?;
            format!(
                "{} -NoExit -Command \"& '{}'{}\"",
                ps.command_line,
                // PowerShell 單引號字串裡的單引號要寫成兩個
                c.path.replace('\'', "''"),
                args
            )
        } else {
            format!("\"{}\"{}", c.path, args)
        };
    }

    let is_claude = shell::is_claude_exe(&sh.exe);
    let tab_kind = match kind.as_str() {
        "shell" | "powershell" => TabKind::PowerShell,
        _ if is_claude => TabKind::Claude,
        _ => TabKind::Custom,
    };
    // 分頁名稱（舊版）：PowerShell 走 NextName("PowerShell(1)")；
    // claude 這類「以資料夾命名」的連線走 DirTabName；其餘 NextName(執行檔名)。
    let tab_title = match title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        Some(t) => t,
        None => match tab_kind {
            TabKind::PowerShell => tabs_state.next_name("PowerShell"),
            TabKind::Claude => {
                tabs_state.dir_tab_name(work_dir.as_deref().unwrap_or(""), &sh.title)
            }
            _ => tabs_state.next_name(&sh.title),
        },
    };

    let id = manager.next_id();

    // 舊版 n 協定第三欄 flags：`c`＝claude 分頁（貼上走 ESC+CR，見 terminal.js doPaste）。
    // 舊版是 C# 的 IsClaudeExe（檔名含 claude）判斷，這裡照同一個規則。
    let flags = if is_claude { "c" } else { "" };
    let n_msg = if flags.is_empty() {
        format!("n{id}\x1f{tab_title}")
    } else {
        format!("n{id}\x1f{tab_title}\x1f{flags}")
    };
    emit_host(&app, n_msg);
    // 建完一定要再送 `s{id}`（舊版 MainWindow.xaml.cs 的 AddTab → SelectTab）。
    emit_host(&app, format!("s{id}"));

    // 輸出批次合併：讀取執行緒只把 bytes 丟進 pump，由 pump 執行緒合併後送一包。
    // 這同時避開 tauri 的門檻——`InvokeResponseBody::Raw` 小於 1024 bytes 會被序列化成
    // JSON 數字陣列用 eval 送，合併後的大包才走 fetch 自訂協定拿到真正的二進位。
    let pump = Arc::new(OutputPump::start(on_event.clone()));

    // 狀態燈要知道「最後一次有輸出是什麼時候」。這條路一個 chunk 走一次，
    // 所以用 AtomicU64 直接寫，不去搶分頁清單的鎖（見 tabs.rs 的註解）。
    let last_output = Arc::new(std::sync::atomic::AtomicU64::new(tabs::now_ms()));

    // log 記錄的槽：spawn 當下先建好空的，使用者按「記錄 log…」時才填進 Logger
    // （輸出 callback 是在這裡就固定下來的，沒有槽就沒辦法事後掛上）。
    let logger: Arc<std::sync::Mutex<Option<Arc<crate::logging::Logger>>>> =
        Arc::new(std::sync::Mutex::new(None));

    let on_output = {
        let pump = pump.clone();
        let last_output = last_output.clone();
        let logger = logger.clone();
        Arc::new(move |bytes: &[u8]| {
            last_output.store(tabs::now_ms(), Ordering::Relaxed);
            // log 先寫再餵畫面：舊版 OnSessionOutput 也是這個順序
            if let Ok(g) = logger.lock() {
                if let Some(l) = g.as_ref() {
                    l.write(bytes);
                }
            }
            pump.push(bytes);
        }) as crate::session::OnOutput
    };
    let on_exit = {
        let pump = pump.clone();
        let channel = on_event.clone();
        Arc::new(move |info: ExitInfo| {
            // 先把還沒送出的輸出排空，再送結束事件，順序才不會顛倒
            pump.flush_and_stop();
            let payload = ExitEvent {
                kind: "exit",
                id,
                exit_code: info.exit_code,
            };
            if let Ok(json) = serde_json::to_string(&payload) {
                let _ = channel.send(InvokeResponseBody::Json(json));
            }
        }) as crate::session::OnExit
    };

    let session = pty::spawn(
        SpawnOptions {
            command_line: sh.command_line.clone(),
            cols,
            rows,
            cwd: work_dir.clone(),
            // 自訂連線有自己的關閉鍵設定（ctrl-c / ctrl-d / none × 次數，同舊版）
            graceful_exit_bytes: match &conn_def {
                Some(c) => c.close_bytes(),
                None => SpawnOptions::default_graceful_exit_bytes(),
            },
            env: sandbox
                .as_ref()
                .map(|s| s.env.clone())
                .unwrap_or_default(),
            // 沙盒第 2 層：整棵行程樹進 kill-on-close 的 Job Object
            kill_on_close: sandbox.is_some(),
        },
        on_output,
        on_exit,
    )
    .map_err(|e| {
        // 建不起來就把已經送出的 `n` 收回去，否則前端會留一個沒有連線的空 pane
        emit_host(&app, format!("x{id}"));
        format!("啟動 {} 失敗：{e}", sh.exe.display())
    })?;

    let info = SessionInfo {
        id,
        pid: session.pid(),
        command_line: sh.command_line.clone(),
        shell: sh.name.clone(),
        backend: session.backend_name().to_string(),
        flags: flags.to_string(),
        title: tab_title.clone(),
    };
    println!(
        "[AwayTerminal] session {} started: pid={} shell={} flags={} backend={} title={}",
        info.id,
        info.pid,
        info.shell,
        if info.flags.is_empty() { "-" } else { &info.flags },
        info.backend,
        info.title,
    );

    tabs_state.insert(Tab {
        id,
        kind: tab_kind,
        title: tab_title,
        title_locked: false,
        cwd_path: String::new(),
        flags: flags.to_string(),
        pid: info.pid,
        started_at: tabs::now_ms(),
        last_output,
        busy: false,
        logger,
        fg: None,
        bg: None,
        sandbox: sandbox.clone(),
        conn_name: conn_def.as_ref().map(|c| c.name.clone()),
        command_line: sh.command_line,
        backend: info.backend.clone(),
    });
    tabs_state.set_active(id);
    manager.insert(id, session);
    tabs::emit_state(&app, &tabs_state);
    Ok(info)
}

/// 開一條內建 SSH 連線（`russh`）。
///
/// 與 shell 那條路的差別：**沒有本機子行程**（`pid` 是 0），連線與驗證是背景非同步進行的，
/// 過程中的 `login as:` / 密碼提示都從同一條輸出 channel 出來，所以對 `terminal.js` 來說
/// 和本機 shell 沒有任何不同。
#[allow(clippy::too_many_arguments)]
fn create_ssh(
    app: AppHandle,
    args: SshArgs,
    title: Option<String>,
    cols: u16,
    rows: u16,
    on_event: Channel<InvokeResponseBody>,
    manager: &SessionManager,
    tabs_state: &Arc<TabManager>,
    settings: &Arc<SettingsStore>,
) -> Result<SessionInfo, String> {
    let host = args.host.trim().to_string();
    if host.is_empty() {
        return Err("請輸入主機".to_string());
    }
    let port = args.port.unwrap_or(22);
    let id = manager.next_id();
    // 舊版：分頁標題先是 host，輸入帳號之後才變成 user@host（見 on_user）
    let tab_title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| host.clone());

    emit_host(&app, format!("n{id}\x1f{tab_title}"));
    emit_host(&app, format!("s{id}"));

    let pump = Arc::new(OutputPump::start(on_event));
    let last_output = Arc::new(std::sync::atomic::AtomicU64::new(tabs::now_ms()));
    let logger: Arc<std::sync::Mutex<Option<Arc<crate::logging::Logger>>>> =
        Arc::new(std::sync::Mutex::new(None));

    let on_output = {
        let pump = pump.clone();
        let last_output = last_output.clone();
        let logger = logger.clone();
        Arc::new(move |bytes: &[u8]| {
            last_output.store(tabs::now_ms(), Ordering::Relaxed);
            if let Ok(g) = logger.lock() {
                if let Some(l) = g.as_ref() {
                    l.write(bytes);
                }
            }
            pump.push(bytes);
        }) as crate::session::OnOutput
    };
    let on_exit = {
        let pump = pump.clone();
        Arc::new(move |_info: ExitInfo| {
            pump.flush_and_stop();
            // 遠端連線結束時舊版會在畫面上留一行灰字提示（`term.exited`），
            // 這裡由 ssh 模組自己印（它知道是「連不上」還是「登出」），這邊不重複。
        }) as crate::session::OnExit
    };
    let on_user: crate::ssh::OnUser = {
        let app = app.clone();
        let tabs_state = tabs_state.clone();
        let host = host.clone();
        Arc::new(move |user: &str| {
            let target = format!("{user}@{host}");
            if tabs_state.set_title(id, &target, false) {
                emit_host(&app, format!("t{id}\x1f{target}"));
                tabs::emit_state(&app, &tabs_state);
            }
        })
    };

    let store = Arc::new(crate::ssh::hostkey::HostKeyStore::new(
        settings.dir().join("known_hosts"),
    ));
    println!("[AwayTerminal] known_hosts: {}", store.path().display());
    let decider = Arc::new(crate::ssh::prompt::AppDecider::new(
        app.clone(),
        id,
        store.path().to_string_lossy().to_string(),
        (*settings).clone(),
    ));

    let session = crate::ssh::spawn(
        crate::ssh::SshOptions {
            host: host.clone(),
            port,
            user: args.user.clone(),
            cols,
            rows,
            auth: crate::ssh::SshAuth {
                key_path: args.key_path.clone(),
                key_passphrase: None, // 有密碼的金鑰在終端機裡問（同 PuTTY）
                use_agent: args.use_agent.unwrap_or(true),
            },
            algos: args.algos.clone().unwrap_or_default(),
            keepalive_mins: args.keepalive_mins.unwrap_or(settings.get().keep_alive_mins),
        },
        store,
        decider,
        on_output,
        on_exit,
        Some(on_user),
    );

    let info = SessionInfo {
        id,
        pid: 0,
        command_line: format!("ssh://{host}:{port}"),
        shell: "ssh".to_string(),
        backend: session.backend_name().to_string(),
        flags: String::new(),
        title: tab_title.clone(),
    };
    println!("[AwayTerminal] session {id} started: ssh {host}:{port} backend=russh");

    tabs_state.insert(Tab {
        id,
        kind: TabKind::Ssh,
        title: tab_title,
        title_locked: false,
        cwd_path: String::new(),
        flags: String::new(),
        pid: 0,
        started_at: tabs::now_ms(),
        last_output,
        busy: false,
        logger,
        fg: None,
        bg: None,
        sandbox: None, // SSH 不需要沙盒（沒有本機子行程可以隔離）
        conn_name: None,
        command_line: info.command_line.clone(),
        backend: info.backend.clone(),
    });
    tabs_state.set_active(id);
    manager.insert(id, session);
    tabs::emit_state(&app, tabs_state);
    Ok(info)
}

/// 寫入原始位元組（`term.onBinary` 用）。
#[tauri::command]
pub fn session_write(id: u32, data: Vec<u8>, manager: State<'_, SessionManager>) {
    if let Some(s) = manager.get(id) {
        s.write(&data);
    }
}

/// 寫入文字（`term.onData` 用；省掉上行的 JSON 數字陣列）。
#[tauri::command]
pub fn session_write_text(id: u32, text: String, manager: State<'_, SessionManager>) {
    if let Some(s) = manager.get(id) {
        s.write(text.as_bytes());
    }
}

#[tauri::command]
pub fn session_resize(id: u32, cols: u16, rows: u16, manager: State<'_, SessionManager>) {
    if let Some(s) = manager.get(id) {
        s.resize(cols, rows);
    }
}

#[tauri::command]
pub fn session_list(manager: State<'_, SessionManager>) -> Vec<u32> {
    manager.ids()
}

// --------------------------------------------------------------- 分頁 UI

/// 關分頁。順序照舊版 `RemoveTabSilently`：先 `x{id}` 讓前端拆掉 pane，
/// 再在背景關 session（送優雅結束鍵 Ctrl+C ×3、等 60ms、才強制收尾），
/// 最後把作用中換到原位置的分頁並送 `s`。
///
/// 關閉確認（舊版的 Yes/No MessageBox）在前端做，這裡只負責關。
#[tauri::command]
pub fn tab_close(
    app: AppHandle,
    id: u32,
    manager: State<'_, SessionManager>,
    tabs_state: State<'_, Arc<TabManager>>,
) {
    emit_host(&app, format!("x{id}"));
    // 關分頁要先收掉 log（舊版 RemoveTabSilently 的 `(tab.Logger as SessionLogger)?.Dispose()`）
    if let Some(slot) = tabs_state.logger_slot(id) {
        if let Ok(mut g) = slot.lock() {
            if let Some(l) = g.take() {
                l.close();
            }
        }
    }
    let next = tabs_state.remove(id);
    if let Some(s) = manager.remove(id) {
        // close() 會 sleep（優雅結束鍵 60ms），別卡在 IPC 執行緒上
        std::thread::spawn(move || s.close());
    }
    if let Some(next) = next {
        emit_host(&app, format!("s{next}"));
    }
    tabs::emit_state(&app, &tabs_state);
}

/// 分頁列點一列 → 設為作用中並通知前端（`s{id}`）。
#[tauri::command]
pub fn tab_select(app: AppHandle, id: u32, tabs_state: State<'_, Arc<TabManager>>) {
    if !tabs_state.set_active(id) {
        return;
    }
    emit_host(&app, format!("s{id}"));
    tabs::emit_state(&app, &tabs_state);
}

/// 改名（舊版右鍵「更改名稱」）：鎖住標題不再依目前目錄自動改，並送 `t` 同步 pane 標題。
#[tauri::command]
pub fn tab_rename(app: AppHandle, id: u32, title: String, tabs_state: State<'_, Arc<TabManager>>) {
    let title = title.trim().to_string();
    if title.is_empty() {
        return;
    }
    if tabs_state.set_title(id, &title, true) {
        emit_host(&app, format!("t{id}\x1f{title}"));
    }
    tabs::emit_state(&app, &tabs_state);
}

/// 分頁列拖曳排序 → 存新順序並用 `K` 同步分割／分欄模式的 pane 順序（舊版 `Tab_DragDrop`）。
#[tauri::command]
pub fn tabs_reorder(app: AppHandle, ids: Vec<u32>, tabs_state: State<'_, Arc<TabManager>>) {
    tabs_state.reorder(&ids);
    let order = tabs_state.ids();
    let list = order
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    emit_host(&app, format!("K{list}"));
    tabs::emit_state(&app, &tabs_state);
}

/// 檢視三態循環：分頁 → 分割 → 分欄 → 分頁（舊版 `Split_Click`）。回傳新模式。
#[tauri::command]
pub fn view_mode_cycle(
    app: AppHandle,
    tabs_state: State<'_, Arc<TabManager>>,
    settings: State<'_, Arc<SettingsStore>>,
) -> String {
    let mode = tabs_state.cycle_view_mode();
    emit_host(&app, format!("L{mode}"));
    settings.update(|s| s.view_mode = mode.clone());
    tabs::emit_state(&app, &tabs_state);
    mode
}

/// 設定檔目前的內容（前端啟動時讀一次，套用分頁列寬度／顯示等）。
#[tauri::command]
pub fn settings_get(settings: State<'_, Arc<SettingsStore>>) -> AppSettings {
    settings.get()
}

/// 分頁列寬度／顯示狀態（舊版 `TabSplitter_DragCompleted` / `TabPanelToggle_Click`）。
#[tauri::command]
pub fn tab_panel_set(
    visible: Option<bool>,
    width: Option<f64>,
    settings: State<'_, Arc<SettingsStore>>,
) {
    settings.update(|s| {
        if let Some(v) = visible {
            s.tab_panel_visible = v;
        }
        if let Some(w) = width {
            s.tab_panel_width = w.round().max(120.0);
        }
    });
}

// ------------------------------------------------- terminal.js 先發生的事

/// `p{id}`：使用者在分割模式點了某個 pane。只改模型，**不回送 `s`**（避免迴圈）。
#[tauri::command]
pub fn pane_selected(app: AppHandle, id: u32, tabs_state: State<'_, Arc<TabManager>>) {
    if tabs_state.set_active(id) {
        tabs::emit_state(&app, &tabs_state);
    }
}

/// `k{ids}`：pane 拖曳後的新順序。只改模型，**不回送 `K`**。
#[tauri::command]
pub fn pane_reordered(app: AppHandle, ids: Vec<u32>, tabs_state: State<'_, Arc<TabManager>>) {
    tabs_state.reorder(&ids);
    tabs::emit_state(&app, &tabs_state);
}

/// `z{size}`：Ctrl+滾輪縮放後的字級。照舊版只接受 6~40 並存進設定
/// （存檔有防抖，見 settings.rs——一路滾不會每格寫一次檔）。
#[tauri::command]
pub fn pane_font_size(size: u32, settings: State<'_, Arc<SettingsStore>>) {
    if let Some(size) = AppSettings::clamp_font_size(size) {
        settings.update(|s| s.font_size = size);
    }
}

/// `a{id}US{kind}US{text}`：前端對 `q` 的回覆。
///
/// 這裡只處理 `cwd`（提示字元行 → shell 分頁自動改名成目前目錄名稱，舊版 1.1.2）。
///
/// `sel` / `selpaste` / `all` 由**前端**直接寫剪貼簿（webview 自己有 clipboard API，
/// 不必為此多裝一個 plugin）；`file` 由前端轉呼叫 `save_text_to_file`。
/// 剩下的 `save`（關閉程式時序列化 scrollback）與 `text`（Telegram 遠端查詢）
/// 對應的功能還沒做，先記 log。
#[tauri::command]
pub fn pane_answer(
    app: AppHandle,
    id: u32,
    kind: String,
    text: String,
    tabs_state: State<'_, Arc<TabManager>>,
) {
    if kind != "cwd" {
        println!("[AwayTerminal] [a 未接] kind={kind} id={id} len={}", text.len());
        return;
    }
    if let Some(new_title) = tabs_state.apply_cwd(id, &text) {
        if let Some(title) = new_title {
            emit_host(&app, format!("t{id}\x1f{title}"));
        }
        tabs::emit_state(&app, &tabs_state);
    }
}
