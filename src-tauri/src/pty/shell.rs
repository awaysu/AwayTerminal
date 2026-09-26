//! 找出要開的 shell / 指令。
//!
//! 照 CLAUDE.md：Windows 用 PowerShell；有裝 `pwsh` 優先（PowerShell 7+），
//! 沒有才用內建的 `powershell.exe`（Windows PowerShell 5.1）。

use std::path::{Path, PathBuf};

/// 選到的執行目標。
pub struct Shell {
    pub exe: PathBuf,
    /// 完整命令列（已加引號）。
    pub command_line: String,
    /// 診斷用：`pwsh` / `powershell` / 自訂指令的執行檔名。
    pub name: String,
    /// 分頁標題（舊版 `n` 協定第二欄）。
    pub title: String,
}

/// PowerShell：`pwsh.exe` 優先，否則 `powershell.exe`。
#[cfg(windows)]
pub fn powershell() -> Option<Shell> {
    if let Some(exe) = which("pwsh.exe") {
        return Some(Shell {
            command_line: quote_command(&exe, &["-NoLogo"]),
            exe,
            name: "pwsh".to_string(),
            title: "PowerShell".to_string(),
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
        name: "powershell".to_string(),
        title: "PowerShell".to_string(),
    })
}

#[cfg(not(windows))]
pub fn powershell() -> Option<Shell> {
    which("pwsh").map(|exe| Shell {
        command_line: quote_command(&exe, &["-NoLogo"]),
        exe,
        name: "pwsh".to_string(),
        title: "PowerShell".to_string(),
    })
}

/// 自訂指令（`?cmd=claude`、`?cmd=codex` 這類 dev 測試入口）。
///
/// `spec` 可以是單一指令名（在 PATH 找，Windows 會試 `.exe` / `.cmd` / `.bat`，
/// npm 安裝的 CLI 通常是 `.cmd`），也可以是「指令 + 參數」。
/// 找不到就回 None——不要把找不到的東西丟給 `CreateProcess`，那會得到看不懂的 Win32 錯誤。
pub fn custom(spec: &str) -> Option<Shell> {
    let (head, args) = split_first_token(spec);
    let exe = resolve_program(&head)?;
    let mut command_line = format!("\"{}\"", exe.display());
    if !args.is_empty() {
        command_line.push(' ');
        command_line.push_str(args);
    }
    let name = exe
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| head.clone());
    let title = exe
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| head.clone());
    Some(Shell {
        exe,
        command_line,
        name,
        title,
    })
}

/// 舊版 `IsClaudeExe`：檔名含 `claude` ⇒ 貼上走 ESC+CR（`n` 協定 flags `c`）。
pub fn is_claude_exe(exe: &Path) -> bool {
    exe.file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase().contains("claude"))
        .unwrap_or(false)
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
pub fn quote_command(exe: &Path, args: &[&str]) -> String {
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

/// 取出第一個 token（支援開頭加引號的路徑），其餘原樣回傳當參數。
fn split_first_token(spec: &str) -> (String, &str) {
    let spec = spec.trim();
    if let Some(after_quote) = spec.strip_prefix('"') {
        if let Some(end) = after_quote.find('"') {
            return (
                after_quote[..end].to_string(),
                after_quote[end + 1..].trim_start(),
            );
        }
    }
    match spec.find(char::is_whitespace) {
        Some(i) => (spec[..i].to_string(), spec[i..].trim_start()),
        None => (spec.to_string(), ""),
    }
}

/// 解析程式路徑：絕對／相對路徑直接檢查，否則在 PATH 找（Windows 補常見副檔名）。
fn resolve_program(head: &str) -> Option<PathBuf> {
    let p = PathBuf::from(head);
    if p.components().count() > 1 || p.is_absolute() {
        return if p.is_file() { Some(p) } else { None };
    }

    #[cfg(windows)]
    {
        // 已經有副檔名就只試它，否則照 PATHEXT 的常見順序；npm 裝的 CLI 多半是 .cmd
        if p.extension().is_some() {
            return which(head);
        }
        for ext in ["exe", "cmd", "bat", "com"] {
            if let Some(found) = which(&format!("{head}.{ext}")) {
                return Some(found);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        which(head)
    }
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

#[cfg(test)]
mod tests {
    use super::{is_claude_exe, split_first_token};
    use std::path::Path;

    #[test]
    fn splits_first_token() {
        assert_eq!(split_first_token("claude"), ("claude".into(), ""));
        assert_eq!(
            split_first_token("codex -c tui.whimsy=false"),
            ("codex".into(), "-c tui.whimsy=false")
        );
        assert_eq!(
            split_first_token("\"C:\\Program Files\\x\\a b.exe\" --flag"),
            ("C:\\Program Files\\x\\a b.exe".into(), "--flag")
        );
        assert_eq!(split_first_token("  spaced  "), ("spaced".into(), ""));
    }

    #[test]
    fn detects_claude_exe() {
        assert!(is_claude_exe(Path::new("C:\\x\\claude.cmd")));
        assert!(is_claude_exe(Path::new("/usr/bin/Claude")));
        assert!(is_claude_exe(Path::new("claude-code.exe")));
        assert!(!is_claude_exe(Path::new("pwsh.exe")));
        assert!(!is_claude_exe(Path::new("codex.cmd")));
    }
}
