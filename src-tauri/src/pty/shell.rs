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

/// 「本機 shell」分頁要開什麼。
///
/// | 平台 | 開什麼 |
/// |---|---|
/// | Windows | PowerShell（`pwsh.exe` 優先，否則內建的 `powershell.exe`） |
/// | macOS | 使用者的 `$SHELL`（沒設就 `/bin/zsh`） |
/// | Linux | 使用者的 `$SHELL`（沒設就 `/bin/bash`） |
///
/// 照 `CLAUDE.md` 的平台差異表：「mac/Linux 預設開使用者的 `$SHELL`，
/// **有裝 `pwsh` 才開 PowerShell**」。所以 Unix 上「PowerShell」是另一個選項
/// （[`powershell`]），不是預設的那一個。
///
/// 分頁標題沿用舊版：Windows 是 `PowerShell`，Unix 用 shell 的檔名（`zsh`／`bash`），
/// 之後提示字元出來會被改成工作目錄名（`TracksCwdTitle`）。
#[cfg(windows)]
pub fn local_shell() -> Option<Shell> {
    powershell()
}

#[cfg(not(windows))]
pub fn local_shell() -> Option<Shell> {
    let argv = awayterm_platform::pty::default_shell();
    let exe = PathBuf::from(argv.first()?);
    let name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("shell")
        .to_string();
    Some(Shell {
        command_line: quote_command(&exe, &[]),
        exe,
        title: name.clone(),
        name,
    })
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

/// mac／Linux：**有裝 `pwsh` 才提供 PowerShell**（`CLAUDE.md` 的平台差異表）。
/// 預設的本機 shell 是使用者的 `$SHELL`，見 [`default_shell`]。
#[cfg(not(windows))]
pub fn powershell() -> Option<Shell> {
    awayterm_platform::which::pwsh().map(|exe| Shell {
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
/// 找 PATH 上的某個執行檔。
///
/// ⚠️ **`WindowsApps` 底下的 Microsoft Store「app execution alias」排到最後**
/// （2026-09-27 實測，TASK-014）：用別名啟動的行程真正是由 **AppX 啟動服務**建立的，
/// **不會進我們的 Job Object**，所以沙盒分頁關掉時收不掉那一棵
/// （見 `examples/job_probe.rs` 與 `docs/AGENT-SANDBOX.md`）。
/// 這台機器的 `pwsh` 就是別名，而 `C:\Program Files\PowerShell\7\pwsh.exe` 是真檔案。
/// 只有「完全找不到真檔案」時才回別名（能跑總比找不到好）。
#[cfg(windows)]
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let mut alias: Option<PathBuf> = None;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if !candidate.is_file() {
            continue;
        }
        if is_store_alias(&candidate) {
            if alias.is_none() {
                alias = Some(candidate);
            }
            continue;
        }
        return Some(candidate);
    }
    alias
}

/// mac／Linux：`PATH` ＋ 幾個 GUI 程式的 PATH 常常沒有的目錄
/// （`~/.local/bin`、Homebrew、npm 全域、nvm…），並且要檢查**執行權限位元**。
///
/// ⚠️ **mac 的 GUI 程式拿不到使用者的 PATH**：從 Finder／Dock 啟動繼承的是 `launchd`
/// 的環境，不會跑 `.zshrc`／`.zprofile`，所以 `PATH` 常常只有 `/usr/bin:/bin:…`，
/// Homebrew 裝的 `node`／`pwsh`／`adb` 全都找不到。Store 別名那條規則**只有 Windows**。
///
/// 實作與順序在 `awayterm_platform::which`（那一層在 Windows 上也被三個 target 的
/// 編譯器檢查過，而且它的測試在這台機器上就跑得到）。
#[cfg(not(windows))]
pub fn which(name: &str) -> Option<PathBuf> {
    awayterm_platform::which::which(name)
}

/// 這個路徑是 Store 的 app execution alias 嗎（`…\WindowsApps\x.exe`）。
///
/// **只有 Windows 有這種東西**；Unix 那邊 `which` 走 `awayterm_platform::which`。
/// 測試在 Windows 以外的平台也要跑得到（規則是純字串比對），所以留著不加 `cfg`，
/// 只在 `which` 的呼叫點分平台。
fn is_store_alias(p: &Path) -> bool {
    p.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("WindowsApps"))
    })
}

#[cfg(test)]
mod tests {
    use super::{is_claude_exe, is_store_alias, split_first_token};
    use std::path::Path;

    /// Store 的 app execution alias 要認得出來（那種行程進不了 Job Object，
    /// 見 `which` 的註解與 `examples/job_probe.rs`）。
    #[test]
    fn spots_store_aliases() {
        assert!(is_store_alias(Path::new(
            "C:\\Users\\x\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe"
        )));
        // 大小寫不該有差（Windows 的路徑不分大小寫）
        assert!(is_store_alias(Path::new("C:\\x\\windowsapps\\python.exe")));
        assert!(!is_store_alias(Path::new(
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe"
        )));
        // 只是名字裡有 WindowsApps 的一段，不是整段相等 → 不算
        assert!(!is_store_alias(Path::new("C:\\MyWindowsAppsTools\\a.exe")));
    }

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
