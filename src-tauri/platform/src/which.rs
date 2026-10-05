//! Unix 的執行檔搜尋（`src/pty/shell.rs` 的 `which()` 對應）。
//!
//! Windows 那一邊要處理 `PATHEXT`（`.exe`／`.cmd`／`.bat`）與 **Microsoft Store 的
//! app execution alias**（那種行程會脫離 Job Object，所以排最後）。Unix 完全不同：
//! 沒有副檔名、判斷「可不可以執行」要看權限位元，但**多了幾個 PATH 常常沒有的地方**。
//!
//! | 位置 | 為什麼要找 |
//! |---|---|
//! | `PATH` | 正常情況 |
//! | `~/.local/bin` | `pip install --user`、Claude Code 的安裝位置（這台 Windows 機器上也是 `~/.local/bin`） |
//! | `/opt/homebrew/bin` | Apple Silicon 的 Homebrew（**GUI 程式的 PATH 沒有它**，見下） |
//! | `/usr/local/bin` | Intel mac 的 Homebrew、Linux 手動安裝 |
//! | `~/.npm-global/bin`、`~/.nvm/versions/node/*/bin` | npm 全域裝的 CLI（codex／gemini 之類） |
//! | `~/.cargo/bin`、`~/go/bin` | 語言自己的 bin |
//! | `~/.opencode/bin`、`~/.bun/bin` | OpenCode 官方安裝程式（`curl … \| bash`）、bun 全域裝的 CLI |
//!
//! # ⚠️ mac 的 GUI 程式拿不到使用者的 PATH
//!
//! 從 Finder／Dock 啟動的程式繼承的是 `launchd` 的環境，**不會跑 `.zshrc`／`.zprofile`**，
//! 所以 `PATH` 常常只有 `/usr/bin:/bin:/usr/sbin:/sbin`。Homebrew 裝的 `node`、`pwsh`、
//! `adb` 就全都找不到。這就是為什麼一定要**自己補上那幾個目錄**，不能只信 `PATH`。
//! （Windows 端沒這個問題，所以那邊只查 PATH。）
//!
//! 待真機驗證：`~/.nvm` 的 glob 展開、以及使用者自訂 prefix 的 npm（`npm config get prefix`）。

use std::path::{Path, PathBuf};

/// 除了 `PATH` 之外還要找的地方（順序就是優先順序）。
///
/// 回傳絕對路徑；不存在的目錄也留著（呼叫端只是逐一試，不存在就跳過）。
pub fn extra_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);

    if let Some(h) = &home {
        out.push(h.join(".local/bin"));
    }
    if cfg!(target_os = "macos") {
        // Apple Silicon 的 Homebrew 在 /opt，Intel 的在 /usr/local
        out.push(PathBuf::from("/opt/homebrew/bin"));
        out.push(PathBuf::from("/opt/homebrew/sbin"));
    }
    out.push(PathBuf::from("/usr/local/bin"));
    if let Some(h) = &home {
        out.push(h.join(".cargo/bin"));
        out.push(h.join(".npm-global/bin"));
        out.push(h.join("go/bin"));
        // 各工具自己的安裝程式放的地方：它們只把這個目錄寫進 `.zshrc`／`.bashrc`，
        // 所以從 Finder／Dock 啟動、或比安裝早開的行程都看不到（2026-10-05 真機：自動偵測找不到 OpenCode）
        out.push(h.join(".opencode/bin"));
        out.push(h.join(".bun/bin"));
        // nvm：`~/.nvm/versions/node/<版本>/bin`，版本是變的 → 逐個列出來（新的在前）
        out.extend(nvm_bins(h));
    }
    out
}

/// `~/.nvm/versions/node/*/bin`，版本號字典序**由大到小**（新的先試）。
fn nvm_bins(home: &Path) -> Vec<PathBuf> {
    let base = home.join(".nvm/versions/node");
    let Ok(dir) = std::fs::read_dir(&base) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    // 字典序反向≈版本新的在前。**不是**語意化版本排序（`v9` 會排在 `v10` 前面），
    // 但只是搜尋順序、不影響正確性，不值得為它引一個 semver 依賴。
    v.sort();
    v.reverse();
    v.into_iter().map(|p| p.join("bin")).collect()
}

/// 這個路徑是「可以執行的檔案」嗎。
///
/// Unix 要看權限位元——`is_file()` 不夠（目錄也可能有 x、而沒有 x 的檔案不能執行）。
#[cfg(unix)]
pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(m) => m.is_file() && m.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Windows 上沒有權限位元的概念，退回「是不是檔案」（這個 crate 在 Windows 只是為了
/// 能編譯與跑純邏輯測試，真正的 Windows 搜尋在主 crate 的 `pty/shell.rs`）。
#[cfg(not(unix))]
pub fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// 在 `PATH` ＋ [`extra_dirs`] 裡找一個執行檔。
///
/// `name` 含路徑分隔符就直接檢查那個路徑（同 `which(1)`）。
pub fn which(name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if name.contains('/') {
        let p = PathBuf::from(name);
        return if is_executable(&p) { Some(p) } else { None };
    }
    for dir in path_dirs().into_iter().chain(extra_dirs()) {
        let cand = dir.join(name);
        if is_executable(&cand) {
            return Some(cand);
        }
    }
    None
}

/// `PATH` 切成目錄清單（Unix 用 `:`）。
fn path_dirs() -> Vec<PathBuf> {
    match std::env::var_os("PATH") {
        Some(p) => std::env::split_paths(&p).collect(),
        None => Vec::new(),
    }
}

/// 有裝 PowerShell 嗎（`CLAUDE.md`：「有裝 `pwsh` 才開 PowerShell」）。
pub fn pwsh() -> Option<PathBuf> {
    which("pwsh")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空字串與明顯不存在的東西回 None，不 panic。
    #[test]
    fn missing_programs_return_none() {
        assert!(which("").is_none());
        assert!(which("definitely-not-a-real-program-xyzzy").is_none());
        assert!(which("/definitely/not/here/xyzzy").is_none());
    }

    /// 額外目錄的順序：`~/.local/bin` 最前面，mac 上 Homebrew 在 `/usr/local` 之前。
    #[test]
    fn extra_dirs_are_in_priority_order() {
        let dirs = extra_dirs();
        let s: Vec<String> = dirs.iter().map(|p| p.display().to_string()).collect();
        if std::env::var_os("HOME").is_some() {
            let local = s.iter().position(|x| x.ends_with(".local/bin") || x.ends_with(".local\\bin"));
            assert!(local.is_some(), "沒有 ~/.local/bin：{s:?}");
        }
        if cfg!(target_os = "macos") {
            let brew = s.iter().position(|x| x == "/opt/homebrew/bin").unwrap();
            let usr = s.iter().position(|x| x == "/usr/local/bin").unwrap();
            assert!(brew < usr, "Apple Silicon 的 Homebrew 要排在 /usr/local 前面");
        }
        // 每個平台都至少有 /usr/local/bin
        assert!(s.iter().any(|x| x == "/usr/local/bin"));
    }

    /// 工具自己的安裝目錄也要在清單裡（OpenCode 的安裝程式只把它寫進 shell 的 rc 檔）。
    #[test]
    fn extra_dirs_include_tool_installer_dirs() {
        if std::env::var_os("HOME").is_none() {
            return;
        }
        let dirs = extra_dirs();
        for want in [".opencode/bin", ".bun/bin"] {
            assert!(dirs.iter().any(|p| p.ends_with(want)), "沒有 ~/{want}：{dirs:?}");
        }
    }

    /// 含斜線的名字直接當路徑處理（不去 PATH 找）。
    #[test]
    fn a_name_with_a_slash_is_treated_as_a_path() {
        assert!(which("./nope-xyzzy").is_none());
    }

    /// Unix 上「存在但沒有執行權限」不算找到。
    #[cfg(unix)]
    #[test]
    fn a_non_executable_file_is_not_a_program() {
        let dir = std::env::temp_dir().join(format!("awayterm-which-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("plain.txt");
        std::fs::write(&f, b"x").unwrap();
        assert!(!is_executable(&f));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
