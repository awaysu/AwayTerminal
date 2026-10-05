//! 巨集 `exec` 的兩件事，**不需要開 GUI**：
//!
//! 1. 子行程（和它開出來的孫行程）真的帶到我們設的 `TEMP`
//! 2. 關掉 Job Object 的 handle 時，**孫行程**也一起被收掉（kill-on-close）
//!
//! 用法：
//! ```text
//! cargo run --example job_probe            # 用 powershell.exe（System32 的真檔案）
//! cargo run --example job_probe -- --pwsh  # 用 pwsh（這台是 Store 的別名，會逃走）
//! ```
//!
//! ## 這支 probe 抓到的事（2026-09-27）
//!
//! 這台機器的 `pwsh` 是 **Microsoft Store 的 app execution alias**
//! （`%LOCALAPPDATA%\Microsoft\WindowsApps\pwsh.exe`）：真正的行程是 **AppX 啟動服務**
//! 開的，不是我們的子行程開的 → **不在我們的 Job Object 裡，關 job 收不到它**。
//! `powershell.exe`（System32 的真檔案）就正常被收。
//! 這是 Store 別名的性質，不是 job 的程式有問題——寫在
//! `docs/AGENT-SANDBOX.md` 與 `docs/REGRESSION-CHECKLIST.md`。
//!
//! ⚠️ 只查自己這次開出來的 PID，不按名稱砍任何東西。

// Job Object 只有 Windows 有；Unix 的對應是行程群組（`platform/src/pgroup.rs`），
// 由 `sandbox_probe`／分頁關閉流程驗。這裡留一個 main 讓 `cargo clippy --all-targets` 在 Unix 也編得過。
#[cfg(not(windows))]
pub fn main() {
    println!("job_probe 只有 Windows（Job Object）。Unix 的對應是行程群組，見 platform/src/pgroup.rs。");
}

#[cfg(windows)]
fn main() {
    win::main();
}

#[cfg(windows)]
mod win {

use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use awayterminal_lib::pty::job::JobObject;
use awayterminal_lib::status::pid_exists;

const CREATE_SUSPENDED: u32 = 0x0000_0004;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn main() {
    // `--pwsh`＝故意用 Store 別名（預期逃走，只印不當失敗）
    let store_alias = std::env::args().any(|a| a == "--pwsh");
    let shell = if store_alias { "pwsh" } else { "powershell" };

    let root = std::env::temp_dir().join("awayterm-job-probe");
    let tmp = root.join(".tmp");
    let _ = std::fs::create_dir_all(&tmp);
    let pid_out = root.join("pid.txt");
    let temp_out = root.join("temp.txt");
    let _ = std::fs::remove_file(&pid_out);
    let _ = std::fs::remove_file(&temp_out);

    let ps1 = root.join("body.ps1");
    let mut f = std::fs::File::create(&ps1).unwrap();
    writeln!(f, "$PID | Out-File -Encoding ascii '{}'", pid_out.display()).unwrap();
    writeln!(
        f,
        "$env:TEMP | Out-File -Encoding ascii '{}'",
        temp_out.display()
    )
    .unwrap();
    writeln!(f, "Start-Sleep 60").unwrap();
    drop(f);

    let cmdline = format!(
        "{shell} -NoLogo -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
        ps1.display()
    );
    println!("命令列              = {cmdline}");

    let mut cmd = Command::new("cmd");
    cmd.arg("/C").raw_arg(&cmdline);
    cmd.env("TEMP", &tmp).env("TMP", &tmp);
    cmd.current_dir(&root);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);

    let job = JobObject::create().expect("建 job 失敗");
    let child = cmd.spawn().expect("spawn 失敗");
    let shell_pid = child.id();
    unsafe { job.assign(child.as_raw_handle() as _) }.expect("assign 失敗");
    resume(shell_pid);

    // 等孫行程把 PID 與 TEMP 寫出來
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < 30 && !pid_out.is_file() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let grand_pid: u32 = std::fs::read_to_string(&pid_out)
        .unwrap_or_default()
        .trim()
        .parse()
        .unwrap_or(0);
    let child_temp = std::fs::read_to_string(&temp_out).unwrap_or_default().trim().to_string();

    println!("cmd 的 PID          = {shell_pid}");
    println!("pwsh（孫）的 PID    = {grand_pid}");
    println!("孫行程看到的 TEMP   = {child_temp}");
    println!("我們設的 TEMP       = {}", tmp.display());
    println!("TEMP 有導過去       = {}", child_temp.eq_ignore_ascii_case(&tmp.to_string_lossy()));
    println!("關 job 前孫行程在   = {}", pid_exists(grand_pid));

    // 這裡就是「巨集結束」：handle 一關，job 裡剩下的行程要一起走
    std::mem::forget(child); // 同 runner.rs 的 `exec … 0`（不等）
    drop(job);
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let shell_alive = pid_exists(shell_pid);
    let grand_alive = pid_exists(grand_pid);
    println!("關 job 後 cmd 在    = {shell_alive}（要 false）");
    println!("關 job 後孫行程在   = {grand_alive}（要 false）");

    if grand_pid == 0 {
        println!("PROBE FAIL：孫行程沒寫出 PID（腳本沒跑起來？）");
        std::process::exit(1);
    }
    if grand_alive {
        // 收尾：只砍我們自己這次記下的那個 PID（絕不按名稱）
        let _ = Command::new("taskkill")
            .args(["/PID", &grand_pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if store_alias {
            println!(
                "PROBE NOTE：Store 別名（{shell}）的行程不在 job 裡，如預期逃走了（已用 PID 收掉）"
            );
            return;
        }
        println!("PROBE FAIL：孫行程沒被 job 收掉，PID {grand_pid} 還活著（已用 PID 收掉）");
        std::process::exit(1);
    }
    println!("PROBE PASS");
}

fn resume(pid: u32) {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap.is_null() {
            return;
        }
        let mut te: THREADENTRY32 = std::mem::zeroed();
        te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        if Thread32First(snap, &mut te) != 0 {
            loop {
                if te.th32OwnerProcessID == pid {
                    let th = OpenThread(THREAD_SUSPEND_RESUME, 0, te.th32ThreadID);
                    if !th.is_null() {
                        ResumeThread(th);
                        windows_sys::Win32::Foundation::CloseHandle(th);
                    }
                }
                te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
                if Thread32Next(snap, &mut te) == 0 {
                    break;
                }
            }
        }
        windows_sys::Win32::Foundation::CloseHandle(snap);
    }
}
}
