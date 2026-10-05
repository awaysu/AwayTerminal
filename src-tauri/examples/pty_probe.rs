//! PTY 層驗證（不開視窗、不需要焦點）。
//!
//! 跑法：`cargo run --example pty_probe`
//!
//! 測三件事（Windows／Unix 各一組實作，項目對應）：
//!
//! | # | Windows | macOS／Linux |
//! |---|---|---|
//! | 1 | PowerShell：`echo AWAY_OK; exit 7` → 輸出含 AWAY_OK、exit code 7、5 秒內結束 | `$SHELL -c` 同一句 |
//! | 2 | cmd：`cmd /c "echo x"` → exit code 0 偵測得到 | `/bin/sh -c "echo x"` |
//! | 3 | resize：互動 PowerShell，resize 後問 `$Host.UI.RawUI.WindowSize` | 互動 `$SHELL`，resize 後問 `stty size` |
//!
//! Unix 那組走 `openpty` ＋ fork（`platform/src/pty.rs`），這支 probe 是它在真機上的第一道驗證
//! （docs/PLATFORM-UNIX.md 第 2 節 B）。
//!
//! Unix 上三項都改用 `/bin/sh`（`sh -c`、`stty size`）——PowerShell／cmd 在那邊不是預設。
//!
//! 離開時 exit code 0＝全過、1＝有項目失敗。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use awayterminal_lib::pty::{self, shell, SpawnOptions};
use awayterminal_lib::session::ExitInfo;

struct Collected {
    out: Arc<Mutex<Vec<u8>>>,
    exited: Arc<Mutex<Option<ExitInfo>>>,
    done: Arc<AtomicBool>,
}

impl Collected {
    fn new() -> Self {
        Self {
            out: Arc::new(Mutex::new(Vec::new())),
            exited: Arc::new(Mutex::new(None)),
            done: Arc::new(AtomicBool::new(false)),
        }
    }

    fn callbacks(&self) -> (awayterminal_lib::session::OnOutput, awayterminal_lib::session::OnExit) {
        let out = self.out.clone();
        let exited = self.exited.clone();
        let done = self.done.clone();
        (
            Arc::new(move |b: &[u8]| out.lock().unwrap().extend_from_slice(b)),
            Arc::new(move |info: ExitInfo| {
                *exited.lock().unwrap() = Some(info);
                done.store(true, Ordering::SeqCst);
            }),
        )
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.out.lock().unwrap()).to_string()
    }

    /// 等結束事件；回傳是否在時限內結束。
    fn wait_exit(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.done.load(Ordering::SeqCst) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn exit_code(&self) -> Option<i32> {
        self.exited.lock().unwrap().and_then(|i| i.exit_code)
    }
}

fn main() {
    // 和正式啟動一樣先整理環境（std handle 歸零在 debug build 是 no-op，
    // 所以這個 probe 從有重導 stdout 的工具環境跑也還能印東西）
    awayterminal_lib::startup::prepare_process_environment();

    println!("== AwayTerminal2 pty_probe ==");
    println!("ConPTY backend: {}", pty::backend_name());

    let mut failures = 0;
    failures += run(test_powershell_exit_code);
    failures += run(test_cmd_exit_detection);
    failures += run(test_resize);

    println!();
    if failures == 0 {
        println!("RESULT: all PASS");
    } else {
        println!("RESULT: {failures} FAILED");
        std::process::exit(1);
    }
}

fn run(f: fn() -> Result<String, String>) -> i32 {
    match f() {
        Ok(msg) => {
            println!("PASS  {msg}");
            0
        }
        Err(msg) => {
            println!("FAIL  {msg}");
            1
        }
    }
}

/// 1. PowerShell 一次性指令：輸出含 AWAY_OK、exit code 7、5 秒內結束。
#[cfg(windows)]
fn test_powershell_exit_code() -> Result<String, String> {
    #[cfg(windows)]
    let (name, cmdline) = {
        let sh = shell::powershell().ok_or("找不到 PowerShell")?;
        let cmdline = shell::quote_command(
            &sh.exe,
            &["-NoLogo", "-NoProfile", "-Command", "echo AWAY_OK; exit 7"],
        );
        (sh.name, cmdline)
    };
    // Unix：PowerShell 不是預設（有裝 `pwsh` 才提供），改用 `/bin/sh` 測同一件事
    #[cfg(not(windows))]
    let (name, cmdline) = (
        "sh".to_string(),
        shell::quote_command(std::path::Path::new("/bin/sh"), &["-c", "echo AWAY_OK; exit 7"]),
    );

    let c = Collected::new();
    let (on_out, on_exit) = c.callbacks();
    let session = pty::spawn(
        SpawnOptions {
            command_line: cmdline.clone(),
            cols: 80,
            rows: 24,
            cwd: None,
            graceful_exit_bytes: Vec::new(),
            env: Vec::new(),
            kill_on_close: false, // 一次性指令不需要送 Ctrl+C
        },
        on_out,
        on_exit,
    )
    .map_err(|e| format!("powershell exit code: spawn 失敗 {e}"))?;

    let started = Instant::now();
    let exited = c.wait_exit(Duration::from_secs(5));
    let elapsed = started.elapsed();
    let text = c.text();
    let code = c.exit_code();
    session.close();

    if !exited {
        return Err(format!(
            "powershell exit code: 5 秒內沒有收到結束事件（輸出 {} bytes）",
            text.len()
        ));
    }
    if !text.contains("AWAY_OK") {
        return Err(format!(
            "powershell exit code: 輸出沒有 AWAY_OK（實際 {:?}）",
            text.chars().take(200).collect::<String>()
        ));
    }
    if code != Some(7) {
        return Err(format!("powershell exit code: 期望 7、實際 {code:?}"));
    }
    Ok(format!(
        "powershell ({}) 輸出含 AWAY_OK、exit code 7、{:?} 結束",
        name, elapsed
    ))
}

/// 2. cmd 的結束偵測（exit code 0）。
#[cfg(windows)]
fn test_cmd_exit_detection() -> Result<String, String> {
    #[cfg(windows)]
    let cmdline = {
        let cmd = shell::cmd().ok_or("找不到 cmd.exe")?;
        format!("\"{}\" /c \"echo x\"", cmd.display())
    };
    #[cfg(not(windows))]
    let cmdline = shell::quote_command(std::path::Path::new("/bin/sh"), &["-c", "echo x"]);

    let c = Collected::new();
    let (on_out, on_exit) = c.callbacks();
    let session = pty::spawn(
        SpawnOptions {
            command_line: cmdline,
            cols: 80,
            rows: 24,
            cwd: None,
            graceful_exit_bytes: Vec::new(),
            env: Vec::new(),
            kill_on_close: false,
        },
        on_out,
        on_exit,
    )
    .map_err(|e| format!("cmd exit: spawn 失敗 {e}"))?;

    let exited = c.wait_exit(Duration::from_secs(5));
    let text = c.text();
    let code = c.exit_code();
    session.close();

    if !exited {
        return Err("cmd exit: 5 秒內沒有收到結束事件".to_string());
    }
    if code != Some(0) {
        return Err(format!("cmd exit: 期望 exit code 0、實際 {code:?}"));
    }
    if !text.contains('x') {
        return Err("cmd exit: 輸出沒有 x".to_string());
    }
    Ok(format!("cmd /c echo x → exit code 0（輸出 {} bytes）", text.len()))
}

/// 3. resize：互動 PowerShell 下 resize 後 `$Host.UI.RawUI.WindowSize` 要跟上。
#[cfg(windows)]
fn test_resize() -> Result<String, String> {
    #[cfg(windows)]
    let (cmdline, prompt, ask, want) = {
        let sh = shell::powershell().ok_or("找不到 PowerShell")?;
        (
            shell::quote_command(&sh.exe, &["-NoLogo", "-NoProfile"]),
            "PS ",
            &b"$w=$Host.UI.RawUI.WindowSize; \"AWAY_SIZE=$($w.Width)x$($w.Height)\"\r"[..],
            "AWAY_SIZE=120x40",
        )
    };
    // Unix：互動 `sh`，用 `stty size`（印「列 欄」）確認 TIOCSWINSZ 有跟上。
    // 提示字元先改成固定字串，才不受使用者 PS1 影響
    #[cfg(not(windows))]
    let (cmdline, prompt, ask, want) = (
        shell::quote_command(std::path::Path::new("/bin/sh"), &["-i"]),
        "$ ",
        &b"echo AWAY_SIZE=$(stty size | tr ' ' x)\r"[..],
        "AWAY_SIZE=40x120",
    );

    let c = Collected::new();
    let (on_out, on_exit) = c.callbacks();
    let session = pty::spawn(
        SpawnOptions {
            command_line: cmdline,
            cols: 80,
            rows: 24,
            cwd: None,
            graceful_exit_bytes: SpawnOptions::default_graceful_exit_bytes(),
            env: Vec::new(),
            kill_on_close: false,
        },
        on_out,
        on_exit,
    )
    .map_err(|e| format!("resize: spawn 失敗 {e}"))?;

    // 等提示字元出現
    if !wait_for(&c, prompt, Duration::from_secs(15)) {
        session.close();
        return Err("resize: 15 秒內沒有看到 PowerShell 提示字元".to_string());
    }

    session.resize(120, 40);
    std::thread::sleep(Duration::from_millis(300));

    // 印成單行好比對：WIDTH=120
    session.write(ask);

    let ok = wait_for(&c, want, Duration::from_secs(10));
    let text = c.text();
    session.close();

    if !ok {
        let tail: String = text.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!(
            "resize: 沒有看到 {want}（畫面尾端：{tail:?}）"
        ));
    }
    Ok(format!("resize(120,40) → {want}"))
}

// ---------------------------------------------------------------------------
// macOS／Linux：同樣三項，用 `$SHELL`／`/bin/sh`
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
fn spawn_unix(cmdline: String, graceful: Vec<u8>) -> Result<(Collected, Arc<dyn awayterminal_lib::session::TerminalSession>), String> {
    let c = Collected::new();
    let (on_out, on_exit) = c.callbacks();
    let session = pty::spawn(
        SpawnOptions {
            command_line: cmdline,
            cols: 80,
            rows: 24,
            cwd: None,
            graceful_exit_bytes: graceful,
            env: Vec::new(),
            kill_on_close: false,
        },
        on_out,
        on_exit,
    )
    .map_err(|e| format!("spawn 失敗 {e}"))?;
    Ok((c, session))
}

/// 1. `$SHELL -c 'echo AWAY_OK; exit 7'`：輸出含 AWAY_OK、exit code 7、5 秒內結束。
#[cfg(not(windows))]
fn test_powershell_exit_code() -> Result<String, String> {
    let sh = shell::local_shell().ok_or("找不到本機 shell（$SHELL）")?;
    let cmdline = shell::quote_command(&sh.exe, &["-c", "echo AWAY_OK; exit 7"]);
    let (c, session) = spawn_unix(cmdline, Vec::new()).map_err(|e| format!("shell exit code: {e}"))?;

    let started = Instant::now();
    let exited = c.wait_exit(Duration::from_secs(5));
    let elapsed = started.elapsed();
    let text = c.text();
    let code = c.exit_code();
    session.close();

    if !exited {
        return Err(format!(
            "shell exit code: 5 秒內沒有收到結束事件（輸出 {} bytes）",
            text.len()
        ));
    }
    if !text.contains("AWAY_OK") {
        return Err(format!(
            "shell exit code: 輸出沒有 AWAY_OK（實際 {:?}）",
            text.chars().take(200).collect::<String>()
        ));
    }
    if code != Some(7) {
        return Err(format!("shell exit code: 期望 7、實際 {code:?}"));
    }
    Ok(format!(
        "{} ({}) 輸出含 AWAY_OK、exit code 7、{:?} 結束",
        sh.name,
        sh.exe.display(),
        elapsed
    ))
}

/// 2. `/bin/sh -c "echo x"` 的結束偵測（exit code 0）。
#[cfg(not(windows))]
fn test_cmd_exit_detection() -> Result<String, String> {
    let cmdline = shell::quote_command(std::path::Path::new("/bin/sh"), &["-c", "echo x"]);
    let (c, session) = spawn_unix(cmdline, Vec::new()).map_err(|e| format!("sh exit: {e}"))?;

    let exited = c.wait_exit(Duration::from_secs(5));
    let text = c.text();
    let code = c.exit_code();
    session.close();

    if !exited {
        return Err("sh exit: 5 秒內沒有收到結束事件".to_string());
    }
    if code != Some(0) {
        return Err(format!("sh exit: 期望 exit code 0、實際 {code:?}"));
    }
    if !text.contains('x') {
        return Err("sh exit: 輸出沒有 x".to_string());
    }
    Ok(format!("sh -c \"echo x\" → exit code 0（輸出 {} bytes）", text.len()))
}

/// 3. resize：互動 `$SHELL` 下 resize 後 `stty size` 要回 `40 120`。
#[cfg(not(windows))]
fn test_resize() -> Result<String, String> {
    let sh = shell::local_shell().ok_or("找不到本機 shell（$SHELL）")?;
    let (c, session) = spawn_unix(sh.command_line.clone(), SpawnOptions::default_graceful_exit_bytes())
        .map_err(|e| format!("resize: {e}"))?;

    // 提示字元長相不固定（使用者的 zshrc），所以只等「有任何輸出」
    if !wait_for_any_output(&c, Duration::from_secs(15)) {
        session.close();
        return Err("resize: 15 秒內 shell 沒有任何輸出".to_string());
    }

    session.resize(120, 40);
    std::thread::sleep(Duration::from_millis(300));

    // 打進去的那一行會被 echo，但 echo 裡是 `$(…)` 不是展開後的值，所以不會誤判
    session.write(b"echo AWAY_SIZE=$(stty size | awk '{print $2 \"x\" $1}')\r");

    let ok = wait_for(&c, "AWAY_SIZE=120x40", Duration::from_secs(10));
    let text = c.text();
    session.write(b"exit\r");
    session.close();

    if !ok {
        let tail: String = text.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!("resize: 沒有看到 AWAY_SIZE=120x40（畫面尾端：{tail:?}）"));
    }
    Ok(format!("{}：resize(120,40) → stty size = 40 120", sh.name))
}

#[cfg(not(windows))]
fn wait_for_any_output(c: &Collected, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !c.out.lock().unwrap().is_empty() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn wait_for(c: &Collected, needle: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if c.text().contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}
