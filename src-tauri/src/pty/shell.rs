//! 找出要開的 shell。
//!
//! 照 CLAUDE.md：Windows 用 PowerShell；有裝 `pwsh` 優先（PowerShell 7+），
//! 沒有才用內建的 `powershell.exe`（Windows PowerShell 5.1）。

use std::path::PathBuf;

/// 選到的 shell。
pub struct Shell {
    pub exe: PathBuf,
    /// 完整命令列（已加引號）。
    pub command_line: String,
    /// 診斷用：`pwsh` 或 `powershell`。
    pub name: &'static str,
}

/// PowerShell：`pwsh.exe` 優先，否則 `powershell.exe`。
#[cfg(windows)]
pub fn powershell() -> Option<Shell> {
    if let Some(exe) = which("pwsh.exe") {
        return Some(Shell {
            command_line: quote_command(&exe, &["-NoLogo"]),
            exe,
            name: "pwsh",
        });
    }

    // System32 的絕對路徑優先於 PATH：PATH 被改過的機器上還是要開到真的那支
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let inbox = PathBuf::from(system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let exe = if inbox.is_file() {
        inbox
    } else {
        which("powershell.exe")?
    };
    Some(Shell {
        command_line: quote_command(&exe, &["-NoLogo"]),
        exe,
        name: "powershell",
    })
}

#[cfg(not(windows))]
pub fn powershell() -> Option<Shell> {
    which("pwsh").map(|exe| Shell {
        command_line: quote_command(&exe, &["-NoLogo"]),
        exe,
        name: "pwsh",
    })
}

/// `cmd.exe`（pty_probe 的 exit code 測試用）。
#[cfg(windows)]
pub fn cmd() -> Option<PathBuf> {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let p = PathBuf::from(system_root).join("System32").join("cmd.exe");
    if p.is_file() {
        Some(p)
    } else {
        which("cmd.exe")
    }
}

/// 把 exe 與參數組成 `CreateProcess` 用的命令列。
pub fn quote_command(exe: &std::path::Path, args: &[&str]) -> String {
    let mut s = format!("\"{}\"", exe.display());
    for a in args {
        s.push(' ');
        if a.contains(' ') && !a.starts_with('"') {
            s.push('"');
            s.push_str(a);
            s.push('"');
        } else {
            s.push_str(a);
        }
    }
    s
}

/// 在 PATH 裡找一個執行檔（不用外部指令）。
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
