//! PTY 層驗證（不開視窗、不需要焦點）。
//!
//! 跑法：`cargo run --example pty_probe`
//!
//! 測三件事：
//! 1. PowerShell：`echo AWAY_OK; exit 7` → 輸出含 AWAY_OK、exit code 7、5 秒內結束。
//! 2. cmd：`cmd /c "echo x"` → exit code 0 偵測得到。
//! 3. resize：開一條互動 PowerShell，resize 後問 `$Host.UI.RawUI.WindowSize` 看有沒有跟上。
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
fn test_powershell_exit_code() -> Result<String, String> {
    let sh = shell::powershell().ok_or("找不到 PowerShell")?;
    let cmdline = shell::quote_command(
        &sh.exe,
        &["-NoLogo", "-NoProfile", "-Command", "echo AWAY_OK; exit 7"],
    );

    let c = Collected::new();
    let (on_out, on_exit) = c.callbacks();
    let session = pty::spawn(
        SpawnOptions {
            command_line: cmdline.clone(),
            cols: 80,
            rows: 24,
            cwd: None,
            graceful_exit_bytes: Vec::new(), // 一次性指令不需要送 Ctrl+C
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
        sh.name, elapsed
    ))
}

/// 2. cmd 的結束偵測（exit code 0）。
fn test_cmd_exit_detection() -> Result<String, String> {
    let cmd = shell::cmd().ok_or("找不到 cmd.exe")?;
    let cmdline = format!("\"{}\" /c \"echo x\"", cmd.display());

    let c = Collected::new();
    let (on_out, on_exit) = c.callbacks();
    let session = pty::spawn(
        SpawnOptions {
            command_line: cmdline,
            cols: 80,
            rows: 24,
            cwd: None,
            graceful_exit_bytes: Vec::new(),
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
fn test_resize() -> Result<String, String> {
    let sh = shell::powershell().ok_or("找不到 PowerShell")?;
    let cmdline = shell::quote_command(&sh.exe, &["-NoLogo", "-NoProfile"]);

    let c = Collected::new();
    let (on_out, on_exit) = c.callbacks();
    let session = pty::spawn(
        SpawnOptions {
            command_line: cmdline,
            cols: 80,
            rows: 24,
            cwd: None,
            graceful_exit_bytes: SpawnOptions::default_graceful_exit_bytes(),
        },
        on_out,
        on_exit,
    )
    .map_err(|e| format!("resize: spawn 失敗 {e}"))?;

    // 等提示字元出現
    if !wait_for(&c, "PS ", Duration::from_secs(15)) {
        session.close();
        return Err("resize: 15 秒內沒有看到 PowerShell 提示字元".to_string());
    }

    session.resize(120, 40);
    std::thread::sleep(Duration::from_millis(300));

    // 印成單行好比對：WIDTH=120
    session.write(b"$w=$Host.UI.RawUI.WindowSize; \"AWAY_SIZE=$($w.Width)x$($w.Height)\"\r");

    let ok = wait_for(&c, "AWAY_SIZE=120x40", Duration::from_secs(10));
    let text = c.text();
    session.close();

    if !ok {
        let tail: String = text.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!(
            "resize: 沒有看到 AWAY_SIZE=120x40（畫面尾端：{tail:?}）"
        ));
    }
    Ok("resize(120,40) → $Host.UI.RawUI.WindowSize = 120x40".to_string())
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
