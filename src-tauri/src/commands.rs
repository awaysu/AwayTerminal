//! 前端可呼叫的 tauri command。
//!
//! 語意對得上舊版的 C#↔JS 字串協定（建立 / 輸入 / 尺寸 / 輸出 / 結束），
//! 但傳輸改成 tauri command + `Channel`，輸出走原始位元組、不再經 base64。
//!
//! 分頁管理（TASK-004）分成兩組，**不要混用**：
//!   - `tab_*`：分頁列 UI 按下去的動作 → 改模型 **並** 發對應的舊協定給 `terminal.js`。
//!   - `pane_*`：`terminal.js` 那邊先發生的事（`p` / `k`）→ 只改模型，**不回送**，
//!     否則會和前端打乒乓（舊版 `case 'p'` 的註解就是「不回送避免迴圈」）。

use crate::i18n::{t, tf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, State};

use crate::host::emit_host;
use crate::output::OutputPump;
use crate::pty::{self, shell, SpawnOptions};
use crate::session::{ExitInfo, SessionManager};
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

/// 「經 PowerShell 啟動」的命令列：`<ps> -NoExit -EncodedCommand <base64>`。
///
/// PowerShell 那一段是 `& '<exe>' <args>`。**不可以**用 `-Command "…"` 包：
/// 使用者的參數與代理團隊 adapter 加的 `--append-system-prompt-file "C:\Users\Away Work\x.md"`
/// 含雙引號，會把外層的 `"…"` 提早結束，路徑有空白就被拆成兩個參數、角色檔沒注入（D7／G1）。
/// `-EncodedCommand` 是 UTF-16LE 的 base64，Windows 命令列的引號規則完全碰不到它，
/// `args` 原樣就是 PowerShell 語法（和使用者在 PowerShell 裡自己打的一樣）。
fn powershell_launch(ps_command_line: &str, exe: &str, args: &str) -> String {
    // PowerShell 單引號字串裡的單引號要寫成兩個
    let script = format!("& '{}'{}", exe.replace('\'', "''"), args);
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    format!(
        "{} -NoExit -EncodedCommand {}",
        ps_command_line,
        crate::b64::encode(&utf16)
    )
}

/// 連線名稱 → 可以當資料夾名的字串（Windows 不允許的字元換成 `_`）。
fn sandbox_dir_name(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let s = s.trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() {
        "conn".to_string()
    } else {
        s
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
    /// 斷線自動重連。省略＝用設定裡的值。
    pub auto_reconnect: Option<bool>,
    /// 要送給遠端的環境變數（SSH `env` request）。
    pub env: Option<Vec<(String, String)>>,
}

/// Telnet 連線的額外參數（`kind = "telnet"` 時才看）。
///
/// 舊版的連線視窗 SSH／Telnet 共用「保持連線」與「斷線自動重連」兩個欄位，這裡照同樣的做法。
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelnetArgs {
    pub host: String,
    /// 省略＝23（舊版切到 Telnet 就把 22 換成 23）。
    pub port: Option<u16>,
    /// 保持連線的間隔（分鐘）。省略＝用設定裡的值。Telnet 送的是 `IAC NOP`。
    pub keepalive_mins: Option<u32>,
    /// 斷線自動重連。省略＝用設定裡的值。
    pub auto_reconnect: Option<bool>,
}

/// ADB 的額外參數（`kind = "adb"` 時才看）。前端先呼叫 `adb_devices` 選好裝置。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbArgs {
    /// `adb.exe` 的路徑（前端先呼叫 `adb_devices` 拿到的那個；空＝再找一次）。
    pub path: Option<String>,
    /// 裝置序號。省略／空＝只有一台，開 `adb shell`（同舊版）。
    pub serial: Option<String>,
}

/// 代理團隊的額外參數（`kind = "agent"` 時才看）。
///
/// 前端先呼叫 `agent_team_create` 拿到計畫，再照計畫逐格開這一種。Rust 這邊自己去
/// 找這一格要跑哪一支 CLI、要加什麼參數（[`crate::agent::adapters`]），並沿用**團隊的**
/// 沙盒（一個團隊一棵 worktree、一個 Job Object）。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentArgs {
    /// 團隊 key（`agent_team_create` 回傳的）。
    pub team: String,
    /// 格號 1～4。
    pub index: u32,
}

/// COM（連接埠）的額外參數（`kind = "com"` 時才看）。
///
/// 欄位與字串值照舊版 `ComDialog` 與 `settings.json`（見 [`crate::com::ComParams`]）。
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComArgs {
    /// 埠名稱（`COM5`）。省略＝用設定裡上次用的。
    pub port: Option<String>,
    pub baud: Option<u32>,
    pub data_bits: Option<u8>,
    pub parity: Option<String>,
    pub stop_bits: Option<String>,
    pub flow: Option<String>,
    /// 斷線（拔線）自動重連。省略＝用設定裡的值。
    pub auto_reconnect: Option<bool>,
}

/// 開一條連線。
///
/// - `kind = "shell"`（舊稱 `powershell`）：pwsh 優先、否則 powershell。
/// - `kind = "custom"`：用 `command` 給的指令（分頁列「自訂指令…」與 `?cmd=` 這類 dev 入口）。
/// - `kind = "ssh"`：內建 SSH（`russh`），參數走 `ssh`（見 [`SshArgs`]）。
/// - `kind = "telnet"`：內建 Telnet，參數走 `telnet`（見 [`TelnetArgs`]）。
/// - `kind = "com"`：連接埠，參數走 `com`（見 [`ComArgs`]）。
///
/// `restore` 是恢復分頁用的索引（[`crate::restore::restore_list`] 回傳的第幾筆）：
/// 給了就在 `n` 之後、`s` 之前插一條 `b{id}US…`，把上次關閉前的畫面倒回去
/// （舊版 `AddTab` 的 `_restoreBufferForNextTab`；順序不能換，見 `docs/PROTOCOL.md`）。
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
    telnet: Option<TelnetArgs>,
    com: Option<ComArgs>,
    // `kind = "adb"`：adb.exe 路徑與（可省的）裝置序號。
    adb: Option<AdbArgs>,
    // `kind = "conn"`：要開哪一條自訂連線（依名稱）。
    conn: Option<String>,
    // `kind = "agent"`：代理團隊的哪一格。
    agent: Option<AgentArgs>,
    // `kind = "conn"`：要用哪個模型（接成 `--model <名稱>`；省略或空＝預設，不加參數）。
    // 只對 AI CLI 的連線有作用（Claude Code／Codex／OpenCode／Gemini，見 `agent/models.rs`）。
    model: Option<String>,
    // 恢復分頁：要倒回哪一筆的畫面（`restore_list` 的索引）。
    restore: Option<usize>,
    on_event: Channel<InvokeResponseBody>,
    manager: State<'_, SessionManager>,
    tabs_state: State<'_, Arc<TabManager>>,
    settings: State<'_, Arc<SettingsStore>>,
    teams: State<'_, Arc<crate::agent::TeamManager>>,
) -> Result<SessionInfo, String> {
    if kind == "ssh" || kind == "telnet" || kind == "com" {
        let params = if kind == "com" {
            let args = com.unwrap_or_default();
            let saved = settings.get();
            // 省略的欄位用設定裡上次用的（同舊版 ComDialog 開起來就是上次的值）
            crate::reconnect::ConnParams::Com(crate::com::ComParams {
                port: args
                    .port
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .unwrap_or(saved.com_port),
                baud: args.baud.unwrap_or(saved.com_baud),
                data_bits: args.data_bits.unwrap_or(saved.com_data_bits),
                parity: args.parity.unwrap_or(saved.com_parity),
                stop_bits: args.stop_bits.unwrap_or(saved.com_stop_bits),
                flow: args.flow.unwrap_or(saved.com_flow),
                auto_reconnect: args.auto_reconnect.unwrap_or(saved.auto_reconnect),
            })
        } else if kind == "ssh" {
            let args = ssh.ok_or_else(|| t("err.sshNeedsParams").to_string())?;
            let host = args.host.trim().to_string();
            if host.is_empty() {
                return Err(t("err.needHost").to_string());
            }
            crate::reconnect::ConnParams::Ssh(crate::ssh::conn::SshConnParams {
                host,
                port: args.port.unwrap_or(22),
                user: args.user.clone().unwrap_or_default(),
                key_path: args.key_path.clone().unwrap_or_default(),
                use_agent: args.use_agent.unwrap_or(true),
                keepalive_mins: args
                    .keepalive_mins
                    .unwrap_or(settings.get().keep_alive_mins),
                auto_reconnect: args.auto_reconnect.unwrap_or(settings.get().auto_reconnect),
                algos: args.algos.clone().unwrap_or_default(),
                env: args.env.clone().unwrap_or_default(),
            })
        } else {
            let args = telnet.ok_or_else(|| t("err.telnetNeedsParams").to_string())?;
            let host = args.host.trim().to_string();
            if host.is_empty() {
                return Err(t("err.needHost").to_string());
            }
            crate::reconnect::ConnParams::Telnet(crate::telnet::TelnetParams {
                host,
                port: args.port.unwrap_or(23),
                keepalive_mins: args
                    .keepalive_mins
                    .unwrap_or(settings.get().keep_alive_mins),
                auto_reconnect: args.auto_reconnect.unwrap_or(settings.get().auto_reconnect),
            })
        };
        return create_remote(
            app, params, title, cols, rows, restore, on_event, &manager, &tabs_state,
        );
    }

    // `kind = "agent"` ＝代理團隊的一格：連線、參數、沙盒、角色注入全部由後端決定。
    let mut agent_slot: Option<crate::agent::LaunchedSlot> = None;
    if kind == "agent" {
        let a = agent
            .as_ref()
            .ok_or_else(|| t("err.agentNeedsSlot").to_string())?;
        agent_slot = Some(crate::agent::plan_slot(&settings, &teams, &a.team, a.index)?);
    }

    // `kind = "conn"` ＝自訂連線（有名稱、有設定、可能有沙盒）。
    let mut conn_def = if kind == "conn" {
        let name = conn
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| t("err.connNeedsName").to_string())?;
        Some(
            crate::custom::find(&settings, name)
                .ok_or_else(|| tf("err.connNotFound", &[name]))?,
        )
    } else {
        // 代理團隊的一格：連線是後端查出來的（使用者的自訂連線優先）
        agent_slot.as_ref().map(|a| a.conn.clone())
    };

    // ---- 模型（2.0.2）：自訂連線＝前端帶來的；代理團隊的格＝`plan_slot` 已經接進參數了 ----
    let mut conn_model = String::new();
    let mut model_extra = String::new();
    if kind == "conn" {
        let want = model.as_deref().map(str::trim).unwrap_or("");
        if let (false, Some(c)) = (want.is_empty(), conn_def.as_mut()) {
            // 名稱會原樣接在命令列上 → 不合法的字元直接拒絕
            if !crate::agent::models::valid_model(want) {
                return Err(t("model.invalid").to_string());
            }
            // 不是 AI CLI 的連線（WSL、使用者自己的工具）沒有 `--model` 這回事 → 不加
            if crate::agent::adapters::backend_of(c).is_some() {
                // 使用者自己在「參數」欄寫的 `--model` 先拿掉（重複給 Codex 會報錯）
                c.args = crate::agent::models::strip_model_arg(&c.args);
                model_extra = crate::agent::models::model_arg(want);
                conn_model = want.to_string();
            }
        }
    } else if let Some(a) = &agent_slot {
        conn_model = a.model.clone();
    }

    let mut sh = match kind.as_str() {
        // "powershell" 是 TASK-003 的舊名，留著相容 `?cmd=` 之前的呼叫
        // Windows＝PowerShell；mac/Linux＝使用者的 $SHELL（見 `shell::local_shell`）
        "shell" | "powershell" => shell::local_shell().ok_or_else(|| {
            t("err.noPowerShell").to_string()
        })?,
        "custom" => {
            let cmd = command
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| t("err.customNeedsCommand").to_string())?;
            shell::custom(cmd).ok_or_else(|| tf("err.commandNotFound", &[cmd]))?
        }
        "conn" | "agent" => {
            let c = conn_def.as_ref().unwrap();
            let exe = std::path::PathBuf::from(&c.path);
            if !exe.is_file() {
                return Err(tf("err.exeMissing", &[&c.path]));
            }
            shell::Shell {
                command_line: String::new(), // 下面依沙盒與 via_powershell 組出來
                name: c.name.clone(),
                title: c.name.clone(),
                exe,
            }
        }
        // ADB：舊版 v1.0.18 起 ADB 是「自訂連線」，但**開的時候走裝置流程**
        //（`OpenAdbFlow` → `OpenAdbShell`），所以這裡是獨立的 kind。
        // 前端先呼叫 `adb_devices` 選好序號，再帶 `adb: { path, serial }` 過來。
        "adb" => {
            let args = adb.as_ref();
            let exe = crate::adb::resolve_path(args.and_then(|a| a.path.as_deref()))
                .ok_or_else(|| t("err.adbNotFound").to_string())?;
            let serial = args.and_then(|a| a.serial.as_deref()).unwrap_or("");
            shell::Shell {
                command_line: crate::adb::command_line(&exe, Some(serial)),
                name: "adb".to_string(),
                // 分頁名稱：有序號用序號、沒有用 ADB（同舊版 `OpenAdbShell` 的 title）
                title: if serial.trim().is_empty() {
                    "ADB".to_string()
                } else {
                    serial.to_string()
                },
                exe,
            }
        }
        other => return Err(tf("err.unsupportedKind", &[other])),
    };

    // ---- 沙盒（只有自訂連線與代理團隊有這個選項；`CLAUDE.md`「新增功能 → 沙盒模式」）----
    // 代理團隊：工作目錄由團隊決定（前端傳來的 cwd 不看）
    let cwd = match &agent_slot {
        Some(a) => Some(a.dir.clone()),
        None => cwd,
    };
    let mut work_dir = cwd.clone().or_else(default_cwd);
    // 恢復分頁要存「沙盒之前」的工作目錄，否則下次會在 worktree 裡再開一層沙盒
    let mut base_dir = work_dir.clone().unwrap_or_default();
    let mut sandbox = None;
    if let Some(a) = &agent_slot {
        // 沙盒是**團隊的**（`agent_team_create` 已經開好 worktree、寫好護欄）：
        // 這裡只沿用它的工作目錄與環境變數。
        if let Some(sb) = &a.sandbox {
            work_dir = Some(sb.work_dir.clone());
            sandbox = Some(sb.clone());
        }
    } else if let Some(c) = &conn_def {
        if c.sandbox {
            let mut base = std::path::PathBuf::from(work_dir.clone().unwrap_or_default());
            // 使用者沒選工作目錄（預設＝桌面）而且那裡不是 git repo：不要在桌面建
            // `.ai\sandbox\…`、也不要把護欄設定寫到桌面上（D14）→ 改用程式設定資料夾底下
            // 這條連線自己的目錄。使用者**自己選的**目錄照用（那是他的決定）。
            if cwd.is_none() && crate::sandbox::git_toplevel(&base).is_none() {
                let dir = settings
                    .dir()
                    .join("sandbox-work")
                    .join(sandbox_dir_name(&c.name));
                match std::fs::create_dir_all(&dir) {
                    Ok(()) => {
                        base = dir;
                        base_dir = base.to_string_lossy().to_string();
                        work_dir = Some(base_dir.clone());
                    }
                    Err(e) => println!(
                        "[AwayTerminal] 沙盒工作目錄建立失敗，沿用 {}：{e}",
                        base.display()
                    ),
                }
            }
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
        // 代理團隊的附加參數是 adapter 決定的（`--append-system-prompt-file` 之類），
        // 已經把沙盒的 `--sandbox` 接在後面了
        let extra = match &agent_slot {
            Some(a) => a.extra_args.as_str(),
            None => sandbox.as_ref().map(|s| s.extra_args.as_str()).unwrap_or(""),
        };
        let mut args = String::new();
        if !c.args.trim().is_empty() {
            args.push(' ');
            args.push_str(c.args.trim());
        }
        // 自訂連線選的模型（代理團隊的格已經含在 `extra` 裡，這裡是空的）
        args.push_str(&model_extra);
        args.push_str(extra);
        sh.command_line = if c.via_powershell {
            // 舊版是「先開互動 PowerShell，尺寸就緒後再把指令打進去」（避免以 80 欄啟動）。
            // 我們的 PTY 一開始就是前端回報的真實尺寸，所以直接用 -NoExit -EncodedCommand 起——
            // 結果一樣（工具跑完仍留在 shell 裡），少一套延後打字的機制。
            let ps = shell::powershell()
                .ok_or_else(|| t("err.noPowerShellVia").to_string())?;
            powershell_launch(&ps.command_line, &c.path, &args)
        } else {
            format!("\"{}\"{}", c.path, args)
        };
    }

    let is_claude = shell::is_claude_exe(&sh.exe);
    let tab_kind = match kind.as_str() {
        "shell" | "powershell" => TabKind::PowerShell,
        // ADB 是自己的種類（舊版 `TermKind.Adb`）：恢復分頁要認得它（C8），
        // 也不依提示行改名（`tracks_cwd_title` 不含 ADB）
        "adb" => TabKind::Adb,
        _ if is_claude => TabKind::Claude,
        _ => TabKind::Custom,
    };
    // 分頁名稱（舊版）：PowerShell 走 NextName("PowerShell(1)")；
    // claude 這類「以資料夾命名」的連線走 DirTabName；其餘 NextName(執行檔名)。
    // 代理團隊：分頁名稱＝Agent ID（舊版 `OpenCustom(conn, g.Dir, s.AgentId, …)`）；
    // 綁組時代表列會被改成組名。
    let title = match &agent_slot {
        Some(a) => Some(a.agent_id.clone()),
        None => title,
    };
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
    // 恢復分頁（1.0.45）：上次存的畫面要在 `n` 之後、`s` 之前倒回去（舊版 AddTab 的順序）
    crate::restore::emit_buffer(&app, id, restore);
    // 建完一定要再送 `s{id}`（舊版 MainWindow.xaml.cs 的 AddTab → SelectTab）。
    emit_host(&app, format!("s{id}"));

    // 輸出批次合併：讀取執行緒只把 bytes 丟進 pump，由 pump 執行緒合併後送一包。
    // 這同時避開 tauri 的門檻——`InvokeResponseBody::Raw` 小於 1024 bytes 會被序列化成
    // JSON 數字陣列用 eval 送，合併後的大包才走 fetch 自訂協定拿到真正的二進位。
    let pump = OutputPump::start(on_event.clone());

    // 狀態燈要知道「最後一次有輸出是什麼時候」。這條路一個 chunk 走一次，
    // 所以用 AtomicU64 直接寫，不去搶分頁清單的鎖（見 tabs.rs 的註解）。
    let last_output = Arc::new(std::sync::atomic::AtomicU64::new(tabs::now_ms()));

    // log 記錄的槽：spawn 當下先建好空的，使用者按「記錄 log…」時才填進 Logger
    // （輸出 callback 是在這裡就固定下來的，沒有槽就沒辦法事後掛上）。
    let logger: Arc<std::sync::Mutex<Option<Arc<crate::logging::Logger>>>> =
        Arc::new(std::sync::Mutex::new(None));

    // TTL 巨集的攔截槽（現在一定是空的，見 src/tap.rs）。要在 on_output 之前建好——
    // 那個 closure 在 spawn 當下就固定了，之後沒辦法再塞東西進去。
    let tap = crate::tap::TapSlot::new();
    let on_output = {
        let pump = pump.clone();
        let last_output = last_output.clone();
        let logger = logger.clone();
        let tap = tap.clone();
        Arc::new(move |bytes: &[u8]| {
            last_output.store(tabs::now_ms(), Ordering::Relaxed);
            tap.output(bytes);
            // log 先寫再餵畫面：舊版 OnSessionOutput 也是這個順序
            // 先把 Logger 複製出來、放掉槽的鎖再寫檔：磁碟慢／防毒卡住時不可以握著槽的鎖
            //（`state_with` 與關分頁都要鎖它，握著會讓整個 UI 一起凍住）
            let l = logger.lock().ok().and_then(|g| g.clone());
            if let Some(l) = l {
                l.write(bytes);
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
        tf("err.launchFailed", &[&sh.exe.display().to_string(), &e.to_string()])
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
        last_input: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        last_submit: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        busy: false,
        logger,
        fg: None,
        bg: None,
        out: Some(pump.clone()),
        tap: tap.clone(),
        macro_handle: None,
        cols,
        rows,
        conn: None,
        reconnect_attempt: 0,
        reconnect_gen: 0,
        sandbox: sandbox.clone(),
        conn_name: conn_def.as_ref().map(|c| c.name.clone()),
        model: conn_model,
        work_dir: base_dir,
        // 恢復分頁用：實際用的 adb.exe 與序號（下次不再跑 `adb devices`）
        adb: (kind == "adb").then(|| {
            (
                sh.exe.display().to_string(),
                adb.as_ref()
                    .and_then(|a| a.serial.clone())
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            )
        }),
        command_line: sh.command_line,
        backend: info.backend.clone(),
    });
    tabs_state.set_active(id);
    // 代理團隊：把分頁綁回那一格（啟動時間、角色注入方式都在這裡記）
    if let (Some(a), Some(plan)) = (&agent, &agent_slot) {
        crate::agent::slot_started(&teams, &a.team, a.index, id, plan);
    }
    manager.insert(id, session);
    tabs::emit_state(&app, &tabs_state);
    Ok(info)
}

/// 給**巨集的 `connect`** 用：在**既有的分頁**上開一條新連線。
///
/// 和 `create_remote` 的差別：分頁與輸出管線都已經存在（巨集正在那個分頁裡跑），
/// 所以這裡只換連線參數再叫 `reconnect::start`——那條路本來就是「沿用分頁的管線重連」。
/// 呼叫端（`ttl::runner::connect_from_macro`）已經確認分頁目前**沒有**連線。
pub fn macro_connect(
    app: &AppHandle,
    id: u32,
    kind: &str,
    host: &str,
    port: Option<u16>,
    user: &str,
    com: Option<u32>,
) -> bool {
    let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
        return false;
    };
    let Some(settings) = app.try_state::<Arc<SettingsStore>>() else {
        return false;
    };
    let s = settings.get();
    let params = match kind {
        "telnet" => crate::reconnect::ConnParams::Telnet(crate::telnet::TelnetParams {
            host: host.to_string(),
            port: port.unwrap_or(23),
            keepalive_mins: s.keep_alive_mins,
            auto_reconnect: false, // 巨集自己決定要不要重連
        }),
        "com" => crate::reconnect::ConnParams::Com(crate::com::ComParams {
            port: match com {
                Some(n) => format!("COM{n}"),
                None => s.com_port.clone(),
            },
            baud: s.com_baud,
            data_bits: s.com_data_bits,
            parity: s.com_parity.clone(),
            stop_bits: s.com_stop_bits.clone(),
            flow: s.com_flow.clone(),
            auto_reconnect: false,
        }),
        _ => crate::reconnect::ConnParams::Ssh(crate::ssh::conn::SshConnParams {
            host: host.to_string(),
            port: port.unwrap_or(22),
            user: user.to_string(),
            key_path: String::new(),
            use_agent: true,
            keepalive_mins: s.keep_alive_mins,
            auto_reconnect: false,
            algos: Default::default(),
            env: Vec::new(),
        }),
    };
    if host.is_empty() && !matches!(params, crate::reconnect::ConnParams::Com(_)) {
        return false;
    }
    tabs.set_conn(id, params.clone());
    let Some(parts) = tabs.session_parts_of(id) else {
        return false;
    };
    match crate::reconnect::start(app, id, &params, parts) {
        Ok(()) => {
            println!("[AwayTerminal] 巨集 connect：分頁 {id} → {}", params.target());
            tabs::emit_state(app, &tabs);
            true
        }
        Err(e) => {
            println!("[AwayTerminal] 巨集 connect 失敗：分頁 {id} → {e}");
            false
        }
    }
}

/// 開一條遠端連線（內建 SSH／Telnet）。
///
/// 與 shell 那條路的差別：**沒有本機子行程**（`pid` 是 0），連線是背景非同步進行的，
/// 過程中的 `login as:`／密碼提示／錯誤訊息都從同一條輸出 channel 出來，
/// 所以對 `terminal.js` 來說和本機 shell 沒有任何不同。
///
/// 兩種後端共用這條路（退避重連、提示訊息、我的最愛、恢復分頁都在
/// [`crate::reconnect`]），差別只有 `ConnParams` 裡面是哪一個變體。
#[allow(clippy::too_many_arguments)]
fn create_remote(
    app: AppHandle,
    params: crate::reconnect::ConnParams,
    title: Option<String>,
    cols: u16,
    rows: u16,
    restore: Option<usize>,
    on_event: Channel<InvokeResponseBody>,
    manager: &SessionManager,
    tabs_state: &Arc<TabManager>,
) -> Result<SessionInfo, String> {
    // (分頁種類, 預設標題, 後端名稱)
    let (tab_kind, default_title, backend) = match &params {
        crate::reconnect::ConnParams::Ssh(p) => (TabKind::Ssh, p.host.clone(), "russh"),
        // Telnet 照舊版是 `host:port`（`OpenTelnetDirect`）
        crate::reconnect::ConnParams::Telnet(p) => {
            (TabKind::Telnet, format!("{}:{}", p.host, p.port), "telnet")
        }
        // COM 照舊版是 `COM5 115200`（`OpenComDirect`）
        crate::reconnect::ConnParams::Com(p) => (TabKind::Com, p.title(), "serialport"),
    };
    let id = manager.next_id();
    // SSH 的標題先是 host，輸入帳號之後才變 user@host（見 on_user）
    let tab_title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or(default_title);

    emit_host(&app, format!("n{id}\x1f{tab_title}"));
    // 恢復分頁：畫面要在 `n` 之後、`s`／連線之前倒回去（舊版 AddTab 的順序）
    crate::restore::emit_buffer(&app, id, restore);
    emit_host(&app, format!("s{id}"));

    let pump = OutputPump::start(on_event);
    let last_output = Arc::new(std::sync::atomic::AtomicU64::new(tabs::now_ms()));
    let logger: Arc<std::sync::Mutex<Option<Arc<crate::logging::Logger>>>> =
        Arc::new(std::sync::Mutex::new(None));

    // 分頁先建好（`reconnect::start` 要從分頁拿輸出管線），再建 session
    tabs_state.insert(Tab {
        id,
        kind: tab_kind,
        title: tab_title.clone(),
        title_locked: false,
        cwd_path: String::new(),
        flags: String::new(),
        pid: 0,
        started_at: tabs::now_ms(),
        last_output,
        last_input: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        last_submit: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        busy: false,
        logger,
        fg: None,
        bg: None,
        out: Some(pump),
        tap: Default::default(),
        macro_handle: None,
        cols,
        rows,
        conn: Some(params.clone()),
        reconnect_attempt: 0,
        reconnect_gen: 0,
        sandbox: None, // 遠端連線沒有本機子行程可以隔離，不需要沙盒
        conn_name: None,
        model: String::new(),
        work_dir: String::new(),
        adb: None,
        command_line: params.target(),
        backend: backend.to_string(),
    });
    tabs_state.set_active(id);

    let parts = tabs_state
        .session_parts_of(id)
        .ok_or_else(|| t("err.tabCreateFailed").to_string())?;
    if let Err(e) = crate::reconnect::start(&app, id, &params, parts) {
        emit_host(&app, format!("x{id}"));
        // pump 也要收掉，否則每次開埠失敗就漏一條執行緒（C4／E5）
        if let Some(parts) = tabs_state.session_parts_of(id) {
            parts.pump.stop();
        }
        // 作用中分頁換回原位置的那一個並通知前端，否則 `active` 變 null、鍵盤沒有目標（C9；
        // 同 `close_tab` 的收尾）
        if let Some(next) = tabs_state.remove(id) {
            emit_host(&app, format!("s{next}"));
        }
        tabs::emit_state(&app, tabs_state);
        return Err(e);
    }
    println!(
        "[AwayTerminal] session {id} started: {} backend={backend}",
        params.target()
    );
    tabs::emit_state(&app, tabs_state);

    Ok(SessionInfo {
        id,
        pid: 0,
        command_line: params.target(),
        shell: match tab_kind {
            TabKind::Telnet => "telnet".to_string(),
            _ => "ssh".to_string(),
        },
        backend: backend.to_string(),
        flags: String::new(),
        title: tab_title,
    })
}

/// 寫入原始位元組（`term.onBinary` 用）。
#[tauri::command]
pub fn session_write(
    id: u32,
    data: Vec<u8>,
    manager: State<'_, SessionManager>,
    tabs_state: State<'_, Arc<TabManager>>,
) {
    // TTL 巨集執行中可以吃掉鍵盤（現在一定是空槽 → 一律放行，見 src/tap.rs）
    if let Some(tap) = tabs_state.tap_of(id) {
        if !tap.allow_input(&data) {
            return;
        }
    }
    if let Some(s) = manager.get(id) {
        s.write(&data);
    }
}

/// 寫入文字（`term.onData` 用；省掉上行的 JSON 數字陣列）。
#[tauri::command]
pub fn session_write_text(
    app: AppHandle,
    id: u32,
    text: String,
    manager: State<'_, SessionManager>,
    tabs_state: State<'_, Arc<TabManager>>,
) {
    if let Some(tap) = tabs_state.tap_of(id) {
        if !tap.allow_input(text.as_bytes()) {
            return;
        }
    }
    // 舊版是在 `case 'i'` 裡設 `LastInputUtc`／`LastSubmitUtc`：程式自己貼進去的字也算
    //（代理團隊的投遞就是走貼上，投遞完那 3 秒本來就不該再投下一封）。
    tabs_state.mark_input(id, text.contains(['\r', '\n']));
    if let Some(s) = manager.get(id) {
        s.write(text.as_bytes());
        return;
    }
    // 沒有連線的分頁：SSH／Telnet 斷線後按 Enter ＝在同一個分頁重連（舊版 `ManualReconnect`）。
    // 其餘的打字就安靜丟掉——舊版踩雷是「session 開失敗卻留在分頁上」會讓打字全被吞，
    // 我們這裡分頁根本沒有 session，所以不會有那種假活著的狀態。
    if tabs_state.conn_params_of(id).is_some() && text.contains(['\r', '\n']) {
        crate::reconnect::manual(&app, id);
    }
}

#[tauri::command]
pub fn session_resize(
    id: u32,
    cols: u16,
    rows: u16,
    manager: State<'_, SessionManager>,
    tabs_state: State<'_, Arc<TabManager>>,
) {
    // 記住最新尺寸：斷線重連要用現在的大小開新 session（同舊版 `tab.Cols/Rows`）
    tabs_state.set_size(id, cols, rows);
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
    close_tab(&app, id, &manager, &tabs_state);
}

/// [`tab_close`] 的本體，給背景執行緒用（Telegram 遠端的 `/close`）。
///
/// 前端不在迴圈裡：`x{id}` 一樣是 emit，所以從任何執行緒呼叫都可以。
pub fn close_tab(app: &AppHandle, id: u32, manager: &SessionManager, tabs_state: &Arc<TabManager>) {
    emit_host(app, format!("x{id}"));
    // 巨集要先叫停（舊版 `CloseTab` 也是先 `(tab.Macro as MacroRunner)?.Stop()`）
    crate::ttl::runner::stop_for_tab(app, id);
    // 關分頁要先收掉 log（舊版 RemoveTabSilently 的 `(tab.Logger as SessionLogger)?.Dispose()`）
    if let Some(slot) = tabs_state.logger_slot(id) {
        // 先拿出來、放掉槽的鎖再 close（close 要等正在寫的那一塊寫完）
        let taken = slot.lock().ok().and_then(|mut g| g.take());
        if let Some(l) = taken {
            l.close();
        }
    }
    // 輸出 pump 也要收掉：不收的話每關一個分頁就漏一條執行緒＋一條 Channel（C4／E5）。
    // ConPTY 的 close() 不會再發 on_exit、遠端的 on_exit 只 flush，所以只能在這裡停。
    if let Some(parts) = tabs_state.session_parts_of(id) {
        parts.pump.stop();
    }
    let next = tabs_state.remove(id);
    if let Some(s) = manager.remove(id) {
        // close() 會 sleep（優雅結束鍵 60ms），別卡在 IPC 執行緒上
        std::thread::spawn(move || s.close());
    }
    // 代理團隊的一格被關：組裡還有別格＝重綁（`g` 協定重排），沒有＝拆組
    //（舊版 `RemoveTabSilently` → `AfterAgentTabRemoved`）。前端不必知道這件事。
    if let Some(teams) = app.try_state::<Arc<crate::agent::TeamManager>>() {
        if teams.find_tab(id).is_some() {
            crate::agent::tab_removed(app, &teams, id);
        }
    }
    // Telegram 遠端：附著在這個分頁上的狀態（`/last` 基準等）一起清掉（H7）
    crate::telegram::remote::tab_closed(id);
    if let Some(next) = next {
        emit_host(app, format!("s{next}"));
    }
    tabs::emit_state(app, tabs_state);
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

/// 這個資料夾存在嗎（檔案總管右鍵開啟前先確認；舊版 `OpenDirFromShell` 也先檢查）。
#[tauri::command]
pub fn dir_exists(path: String) -> bool {
    !path.trim().is_empty() && std::path::Path::new(path.trim()).is_dir()
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
    // 代理團隊：記住最後點過的那一格（點分頁列那一列時回到它，舊版 `MarkActiveRow`）
    if let Some(teams) = app.try_state::<Arc<crate::agent::TeamManager>>() {
        crate::agent::note_focus(&teams, id);
    }
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
    // 關閉程式時存畫面（恢復分頁）：交給等在信箱那邊的 `restore::save`
    if kind == "save" {
        crate::restore::deliver(id, text);
        return;
    }
    // 遠端（Telegram）要「畫面上看得到的文字」：交給等在信箱那邊的 `telegram::screen`。
    // ⚠️ 這條路一定要走 `q…text`／`a…text`（xterm buffer 的 `translateToString`），
    // **不可以拿原始位元組流去 ANSI**——舊版 CLAUDE.md 的那條雷。
    if kind == "text" {
        crate::telegram::screen::deliver(id, text);
        return;
    }
    if kind == "shot" {
        crate::telegram::shot::deliver(id, text);
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::{powershell_launch, sandbox_dir_name};

    /// 把 `-EncodedCommand` 後面的 base64 解回 PowerShell 腳本（測試用的小解碼器）。
    fn decode_ps(cmd: &str) -> String {
        let b64 = cmd.rsplit(' ').next().unwrap();
        let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bits = 0u32;
        let mut n = 0;
        let mut bytes = Vec::new();
        for c in b64.bytes().filter(|&c| c != b'=') {
            let v = table.iter().position(|&t| t == c).unwrap() as u32;
            bits = (bits << 6) | v;
            n += 6;
            if n >= 8 {
                n -= 8;
                bytes.push((bits >> n) as u8);
                bits &= (1 << n) - 1;
            }
        }
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units).unwrap()
    }

    /// D7／G1：參數裡的雙引號（含空白的角色檔路徑）要原封不動到 PowerShell，
    /// 不可以再被外層 `-Command "…"` 的引號吃掉。
    #[test]
    fn via_powershell_keeps_quoted_args_intact() {
        let args = r#" --append-system-prompt-file "C:\Users\Away Work\x.md" --model "a b""#;
        let cmd = powershell_launch(r#""C:\pwsh.exe" -NoLogo"#, r"C:\npm\it's\claude.cmd", args);
        assert!(cmd.starts_with(r#""C:\pwsh.exe" -NoLogo -NoExit -EncodedCommand "#), "{cmd}");
        // 命令列本身不再含有會被 CommandLineToArgv 解讀的雙引號（除了 exe 路徑那一對）
        assert_eq!(cmd.matches('"').count(), 2, "{cmd}");
        assert_eq!(
            decode_ps(&cmd),
            format!(r"& 'C:\npm\it''s\claude.cmd'{args}")
        );
    }

    #[test]
    fn sandbox_dir_names_are_file_safe() {
        assert_eq!(sandbox_dir_name("Claude Code"), "Claude Code");
        assert_eq!(sandbox_dir_name(r#"a/b\c:d*e?"f<g>h|"#), "a_b_c_d_e__f_g_h_");
        assert_eq!(sandbox_dir_name("  ..  "), "conn");
        assert_eq!(sandbox_dir_name("x."), "x");
    }
}
