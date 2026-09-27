//! 沙盒模式（新功能，規格＝`CLAUDE.md`「新增功能 → 沙盒模式」）。
//!
//! 目的：**AI agent 測試時不要影響使用者正在用的電腦**。分三層：
//!
//! | 層 | 做什麼 | 這個檔 |
//! |---|---|---|
//! | 1 工作區隔離 | git repo 就開 `git worktree`，`TEMP`／`CARGO_TARGET_DIR` 導到沙盒 | [`prepare`] |
//! | 2 行程與指令護欄 | 行程樹進 Job Object（`pty/job.rs`）＋在沙盒裡產生各工具的護欄設定 | [`write_guardrails`] / [`extra_args`] |
//! | 3 桌面隔離 | Windows Sandbox／Hyper-V／WSL2 —— **只寫文件**（`docs/AGENT-SANDBOX.md`） | — |
//!
//! ## ⚠️ 前兩層是防呆，不是防壞
//! agent 與使用者在**同一個 Windows 登入工作階段**，繞過 hook（例如自己寫一支
//! `.ps1` 再執行）仍然碰得到桌面、視窗與其他行程。這一點 `docs/AGENT-SANDBOX.md`
//! 有明講，程式裡也不要假裝它是安全邊界。
//!
//! ## 絕對不動的環境變數
//! `HOME`／`APPDATA`／`LOCALAPPDATA`／`USERPROFILE` **一律不改**——
//! Claude Code、Codex 的登入狀態存在那裡，改了會讓 agent 掉登入（`CLAUDE.md` 明寫）。

use crate::i18n::{t, tf};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 沙盒目錄放在工作區的這個位置底下（`CLAUDE.md`：`.ai/sandbox/<tab>/`）。
/// 兩段分開存，`Path::join` 才不會做出 `C:\x\.ai/sandbox\y` 這種混合分隔符的路徑。
const SANDBOX_DIRS: [&str; 2] = [".ai", "sandbox"];

/// 護欄腳本。用 **Node** 寫成一支跨平台腳本，理由見 `docs/AGENT-SANDBOX.md`：
/// Claude Code 自己就要 Node，所以「有 Claude Code 的地方一定有 node」；
/// hook 的輸入是 stdin 上的 JSON，`sh` 沒有 jq 解不動、PowerShell 7 在 mac/Linux 不保證有。
const GUARD_SCRIPT: &str = include_str!("../resources/sandbox-guard.mjs");

/// 一條連線的沙盒配置結果。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sandbox {
    /// 沙盒根目錄（`<repo>/.ai/sandbox/<name>` 或非 repo 時的 `<workdir>/.ai/sandbox/<name>`）。
    pub root: String,
    /// 實際的工作目錄：有 worktree 就是 worktree 路徑，否則是原本的工作目錄。
    pub work_dir: String,
    /// worktree 的分支名（沒有 worktree 時是空的）。
    pub branch: String,
    /// 有沒有開成 worktree（不是 git repo 就沒有）。
    pub has_worktree: bool,
    /// 要加進子行程的環境變數。
    pub env: Vec<(String, String)>,
    /// 產生了哪些護欄檔案（顯示給使用者看的相對路徑）。
    pub guardrails: Vec<String>,
    /// 給這個工具追加的啟動參數（Codex／Gemini 的 `--sandbox`）。
    pub extra_args: String,
}

/// 準備沙盒。`name` 是分頁或團隊的識別字（會出現在路徑與分支名裡）。
///
/// 失敗時**不要**讓整條連線開不起來——呼叫端應該退回「沒有沙盒」並在畫面上說明。
pub fn prepare(work_dir: &Path, name: &str, tool_path: &str) -> Result<Sandbox, String> {
    let safe = sanitize(name);
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M").to_string();

    // ---- 1. 是 git repo 嗎？（子目錄也算，所以問 git 而不是找 .git）----
    let repo_root = git_toplevel(work_dir);
    let (root, work, branch, has_worktree) = match &repo_root {
        Some(repo) => {
            let root = repo.join(SANDBOX_DIRS[0]).join(SANDBOX_DIRS[1]).join(&safe);
            let branch = format!("sandbox/{safe}-{stamp}");
            match add_worktree(repo, &root, &branch) {
                Ok(()) => {
                    ensure_ignored(repo);
                    (root.clone(), root, branch, true)
                }
                Err(e) => {
                    // worktree 開不起來（例如分支已存在、repo 沒有任何 commit）→
                    // 退成「無 worktree 沙盒」，其餘兩層照做
                    println!("[AwayTerminal] 沙盒 worktree 失敗，退成無 worktree：{e}");
                    let root = repo.join(SANDBOX_DIRS[0]).join(SANDBOX_DIRS[1]).join(&safe);
                    let _ = std::fs::create_dir_all(&root);
                    ensure_ignored(repo);
                    (root, work_dir.to_path_buf(), String::new(), false)
                }
            }
        }
        None => {
            let root = work_dir.join(SANDBOX_DIRS[0]).join(SANDBOX_DIRS[1]).join(&safe);
            std::fs::create_dir_all(&root).map_err(|e| tf("err.sandboxDirFailed", &[&e.to_string()]))?;
            (root, work_dir.to_path_buf(), String::new(), false)
        }
    };

    // ---- 2. 環境變數（只導可以安全隔離的）----
    let tmp = root.join(".tmp");
    let _ = std::fs::create_dir_all(&tmp);
    let mut env = vec![
        ("TEMP".to_string(), tmp.to_string_lossy().to_string()),
        ("TMP".to_string(), tmp.to_string_lossy().to_string()),
        // 讓 hook 腳本知道自己在哪個沙盒裡（deny 訊息與路徑判斷要用）
        (
            "AWAYTERM_SANDBOX_ROOT".to_string(),
            work.to_string_lossy().to_string(),
        ),
    ];
    // Rust 專案才設 CARGO_TARGET_DIR：非 Rust 專案設了只是多一個沒人看的變數
    if work.join("Cargo.toml").is_file() {
        env.push((
            "CARGO_TARGET_DIR".to_string(),
            root.join(".target").to_string_lossy().to_string(),
        ));
    }

    // ---- 3. 護欄設定 ----
    let guardrails = write_guardrails(&work, tool_path);

    Ok(Sandbox {
        root: root.to_string_lossy().to_string(),
        work_dir: work.to_string_lossy().to_string(),
        branch,
        has_worktree,
        env,
        guardrails,
        extra_args: extra_args(tool_path).to_string(),
    })
}

/// 工具本身支援的沙盒參數（`CLAUDE.md`：Codex `--sandbox workspace-write`、Gemini `--sandbox`）。
///
/// ⚠️ 參數名是照 `CLAUDE.md` 寫的，**還沒有在本機實際跑過那兩個工具驗證**
/// （這台機器上沒裝）。列在 TASK_RESULT 的「需要使用者確認」。
pub fn extra_args(tool_path: &str) -> &'static str {
    match tool_kind(tool_path) {
        ToolKind::Codex => " --sandbox workspace-write",
        ToolKind::Gemini => " --sandbox",
        _ => "",
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum ToolKind {
    Claude,
    Codex,
    Gemini,
    Other,
}

/// 依執行檔名判斷是哪個工具（同舊版 `IsClaudeExe`／`UsesDirTitle` 的作法：看檔名）。
pub fn tool_kind(path: &str) -> ToolKind {
    let stem = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if stem.contains("claude") {
        ToolKind::Claude
    } else if stem.contains("codex") {
        ToolKind::Codex
    } else if stem.contains("gemini") {
        ToolKind::Gemini
    } else {
        ToolKind::Other
    }
}

/// 在沙盒工作區裡產生護欄設定。**只寫沙盒目錄裡的檔，不碰使用者原本的設定。**
///
/// 目前只有 Claude Code 有 hook 機制（`.claude/settings.local.json` 的 `PreToolUse`）。
/// 已經有那個檔時**合併 hooks 而不是蓋掉**（worktree 是從 HEAD 開的，通常沒有，
/// 但使用者可能 commit 過一份）。
pub fn write_guardrails(work: &Path, tool_path: &str) -> Vec<String> {
    let mut written = Vec::new();
    if tool_kind(tool_path) != ToolKind::Claude {
        return written;
    }

    let dir = work.join(".claude");
    if std::fs::create_dir_all(&dir).is_err() {
        return written;
    }

    // 1. hook 腳本本體
    let script = dir.join("awayterm-sandbox-guard.mjs");
    if std::fs::write(&script, GUARD_SCRIPT).is_ok() {
        written.push(".claude/awayterm-sandbox-guard.mjs".to_string());
    }

    // 2. settings.local.json 的 PreToolUse hook（合併）
    let settings_path = dir.join("settings.local.json");
    let mut root: serde_json::Value = std::fs::read_to_string(&settings_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }

    let entry = serde_json::json!({
        "matcher": "Bash",
        "hooks": [{
            "type": "command",
            "command": format!("node \"{}\"", script.to_string_lossy().replace('\\', "\\\\")),
        }]
    });

    let hooks = root
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        *hooks = serde_json::json!({});
    }
    let pre = hooks
        .as_object_mut()
        .unwrap()
        .entry("PreToolUse")
        .or_insert_with(|| serde_json::json!([]));
    if !pre.is_array() {
        *pre = serde_json::json!([]);
    }
    let arr = pre.as_array_mut().unwrap();
    // 同一支腳本只掛一次（重開分頁會重跑這段）
    let already = arr.iter().any(|e| {
        e.to_string()
            .contains("awayterm-sandbox-guard.mjs")
    });
    if !already {
        arr.push(entry);
    }

    if let Ok(text) = serde_json::to_string_pretty(&root) {
        if std::fs::write(&settings_path, text).is_ok() {
            written.push(".claude/settings.local.json".to_string());
        }
    }
    written
}

// ------------------------------------------------------------------ git 部分

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| tf("err.gitFailed", &[&args.join(" "), &e.to_string()]))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 這個目錄在哪個 git repo 裡（子目錄也算）。不是 repo 就回 `None`。
pub fn git_toplevel(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    git(dir, &["rev-parse", "--show-toplevel"])
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// `git worktree add -b <branch> <path>`（從目前 HEAD 開）。
fn add_worktree(repo: &Path, path: &Path, branch: &str) -> Result<(), String> {
    if path.exists() {
        // 同名沙盒已經存在（同一條連線重開）→ 直接用它，不要重複 add
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| tf("err.sandboxDirFailed", &[&e.to_string()]))?;
    }
    let path_str = path.to_string_lossy().to_string();
    git(repo, &["worktree", "add", "-b", branch, &path_str])?;
    Ok(())
}

/// 讓 `.ai/sandbox/` 不要出現在 `git status`。
///
/// **不改使用者的 `.gitignore`**（那是他的檔、會進 commit）——寫進
/// `.git/info/exclude`，那是本機、不進版本控制的忽略清單。
fn ensure_ignored(repo: &Path) {
    let exclude = repo.join(".git").join("info").join("exclude");
    let line = format!("/{}/{}/", SANDBOX_DIRS[0], SANDBOX_DIRS[1]);
    let current = std::fs::read_to_string(&exclude).unwrap_or_default();
    if current.lines().any(|l| l.trim() == line) {
        return;
    }
    if let Some(dir) = exclude.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut text = current;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("# AwayTerminal 沙盒模式（自動加入；刪掉這行沙盒目錄就會出現在 git status）\n");
    text.push_str(&line);
    text.push('\n');
    if std::fs::write(&exclude, text).is_err() {
        println!("[AwayTerminal] 寫不進 .git/info/exclude，沙盒目錄會出現在 git status");
    }
}

/// 移除一個沙盒 worktree。**分支保留**（裡面可能有還沒合併的成果）。
pub fn remove_worktree(work_dir: &str) -> Result<(), String> {
    let path = PathBuf::from(work_dir);
    let repo = git_toplevel(&path).ok_or_else(|| t("err.notGitRepo").to_string())?;
    let path_str = path.to_string_lossy().to_string();
    git(&repo, &["worktree", "remove", "--force", &path_str])?;
    println!("[AwayTerminal] 已移除沙盒 worktree：{path_str}（分支保留）");
    Ok(())
}

/// 路徑與分支名都要吃得下去：只留英數、`-`、`_`、`.`，**再接一段名稱的短 hash**。
///
/// 為什麼要 hash（TASK-007 Issue 3）：非 ASCII 會整段變成 `-` 再被 trim 掉，
/// 所以「代理團隊」「我的專案」這種全中文名稱 sanitize 之後都是空的——
/// 沒有 hash 的話它們會共用同一個沙盒目錄。接上名稱的 8 碼 hash 就不會撞，
/// 而且同一個名稱永遠得到同一個目錄（重開分頁要接回原本的沙盒）。
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // 連續的 `-` 併成一個，看起來才不會是 `-----2`
    let mut squeezed = String::with_capacity(cleaned.len());
    for c in cleaned.chars() {
        if c == '-' && squeezed.ends_with('-') {
            continue;
        }
        squeezed.push(c);
    }
    let base = squeezed.trim_matches('-');
    let hash = short_hash(name);
    if base.is_empty() {
        hash
    } else {
        format!("{base}-{hash}")
    }
}

/// 名稱的 8 碼十六進位 hash（FNV-1a，取 64 bit 的高 32 位）。
///
/// 只是要「不同名稱不要撞到同一個資料夾」，不是密碼學用途，所以不引入 hash crate。
fn short_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", (h >> 32) as u32)
}

// --------------------------------------------------- 驗證用（`--verify` 專用）
//
// 這幾個指令只給 `--verify` 的自動驗證用，不是產品功能。刻意做得很窄：
// `pid_alive` **只查存活、不砍任何行程**（`.ai/bus/0014`：這個團隊自己就跑在舊版
// AwayTerminal 底下，按名稱砍行程等於自殺；連「砍自己記下的 PID」都由 Job Object 做，
// 不由這裡做）。

/// `--verify` 用：pwsh 的路徑 ＋ 一個**在 git repo 裡**的工作目錄。
///
/// 驗證需要 repo 才驗得到 worktree；程式自己的 cwd（`cargo run` 是 `src-tauri/`）
/// 就在本專案的 repo 裡，所以直接用它。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxProbe {
    pub pwsh: Option<String>,
    pub repo_dir: Option<String>,
}

/// `--verify` 開始前先清掉上一次的殘留。
///
/// `--verify` 會在專案 repo 裡建一個 `__awayterm_verify_*` 的沙盒 worktree 與分支，
/// 正常跑完會自己清；但如果那次 dev 被 timeout 砍掉（我們驗證時常這樣收尾）就會留著。
/// 這個指令只動名稱以 `__awayterm_verify` 開頭的東西，不會碰使用者自己的沙盒。
#[tauri::command]
pub fn sandbox_verify_cleanup() -> Vec<String> {
    let mut removed = Vec::new();
    let Some(repo) = std::env::current_dir().ok().and_then(|d| git_toplevel(&d)) else {
        return removed;
    };

    // 1. worktree：從 `git worktree list --porcelain` 找路徑含 __awayterm_verify 的
    if let Ok(list) = git(&repo, &["worktree", "list", "--porcelain"]) {
        for line in list.lines() {
            let Some(path) = line.strip_prefix("worktree ") else {
                continue;
            };
            if !path.contains("__awayterm_verify") {
                continue;
            }
            if git(&repo, &["worktree", "remove", "--force", path]).is_ok() {
                removed.push(format!("worktree {path}"));
            }
        }
    }
    let _ = git(&repo, &["worktree", "prune"]);

    // 2. 分支：sandbox/__awayterm_verify…
    if let Ok(list) = git(&repo, &["branch", "--list", "sandbox/__awayterm_verify*"]) {
        for line in list.lines() {
            let branch = line.trim_start_matches('*').trim();
            if branch.is_empty() {
                continue;
            }
            if git(&repo, &["branch", "-D", branch]).is_ok() {
                removed.push(format!("branch {branch}"));
            }
        }
    }
    if !removed.is_empty() {
        println!("[AwayTerminal] --verify 清掉上次的殘留：{removed:?}");
    }
    removed
}

#[tauri::command]
pub fn sandbox_probe() -> SandboxProbe {
    let pwsh = crate::pty::shell::which("pwsh.exe")
        .or_else(|| crate::pty::shell::which("pwsh"))
        .map(|p| p.to_string_lossy().to_string());
    let repo_dir = std::env::current_dir()
        .ok()
        .and_then(|d| git_toplevel(&d))
        .map(|p| p.to_string_lossy().to_string());
    SandboxProbe { pwsh, repo_dir }
}

/// 一個 PID 還活著嗎。**唯讀**：不砍、不改，只回答存活。
#[tauri::command]
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        // 用 Toolhelp 掃一遍（和狀態燈同一套唯讀機制），不開行程 handle
        crate::status::pid_exists(pid)
    }
    #[cfg(not(windows))]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
}

/// `--verify` 用的沙盒檢查結果。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxChecks {
    /// `.ai/sandbox/` 有沒有真的被 git 忽略（`git status --porcelain` 裡看不到它）。
    pub ignored: bool,
    pub worktree_exists: bool,
    pub branch_exists: bool,
}

#[tauri::command]
pub fn sandbox_verify(
    id: u32,
    tabs_state: tauri::State<'_, std::sync::Arc<crate::tabs::TabManager>>,
) -> Result<SandboxChecks, String> {
    let sb = tabs_state
        .sandbox_of(id)
        .ok_or_else(|| t("err.tabNoSandbox").to_string())?;
    let work = PathBuf::from(&sb.work_dir);
    let repo = git_toplevel(&work);

    let ignored = match &repo {
        Some(repo) => {
            // 從主 worktree 看：status 裡不該出現 .ai/sandbox
            let out = git(repo, &["status", "--porcelain"]).unwrap_or_default();
            !out.lines().any(|l| l.contains(".ai/sandbox"))
        }
        None => true, // 不是 repo 就沒有 git status 的問題
    };
    let branch_exists = if sb.branch.is_empty() {
        false
    } else {
        match &repo {
            Some(repo) => git(repo, &["rev-parse", "--verify", &sb.branch]).is_ok(),
            None => false,
        }
    };
    Ok(SandboxChecks {
        ignored,
        worktree_exists: work.is_dir(),
        branch_exists,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_paths_safe() {
        // 看得懂的前綴 + 短 hash（前綴讓人在檔案總管裡認得出來）
        assert!(sanitize("ClaudeCode").starts_with("ClaudeCode-"));
        assert!(sanitize("Claude Code 2").starts_with("Claude-Code-2-"));
        assert!(sanitize("a/b\\c:d").starts_with("a-b-c-d-"));
        // 連續的 `-` 併成一個（不要做出 `-----2-xxxxxxxx`）
        assert!(sanitize("我的 分頁 (2)").starts_with("2-"));
        // 路徑與 git 分支名不可以出現其他字元
        for name in ["ClaudeCode", "我的 分頁 (2)", "a/b\\c:d", "", "---"] {
            let s = sanitize(name);
            assert!(!s.is_empty(), "{name:?} → 空字串");
            assert!(
                s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
                "{s} 含有不能進路徑的字元"
            );
        }
    }

    #[test]
    fn sanitize_does_not_collide_for_cjk_names() {
        // TASK-007 Issue 3：全中文名稱 sanitize 之後都是空的，沒有 hash 就會撞在一起
        let a = sanitize("代理團隊");
        let b = sanitize("我的專案");
        assert_ne!(a, b, "不同的中文名稱不可以共用同一個沙盒目錄");
        assert_eq!(a.len(), 8, "sanitize 為空時只剩 8 碼 hash：{a}");
        // 同一個名稱一定要得到同一個目錄（重開分頁要接回原本的沙盒）
        assert_eq!(a, sanitize("代理團隊"));
    }

    #[test]
    fn tool_kind_from_exe_name() {
        assert_eq!(tool_kind("C:\\x\\claude.cmd"), ToolKind::Claude);
        assert_eq!(tool_kind("/usr/bin/codex"), ToolKind::Codex);
        assert_eq!(tool_kind("gemini.exe"), ToolKind::Gemini);
        assert_eq!(tool_kind("pwsh.exe"), ToolKind::Other);
    }

    #[test]
    fn extra_args_match_claude_md() {
        assert_eq!(extra_args("codex.cmd"), " --sandbox workspace-write");
        assert_eq!(extra_args("gemini.cmd"), " --sandbox");
        assert_eq!(extra_args("claude.cmd"), ""); // claude 走 hook，不是參數
        assert_eq!(extra_args("pwsh.exe"), "");
    }

    #[test]
    fn guardrails_merge_instead_of_overwrite() {
        let dir = std::env::temp_dir().join(format!("awayterm-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".claude")).unwrap();

        // 使用者已經有一份設定，裡面有別的 hook 與別的欄位
        let existing = r#"{
          "permissions": {"allow": ["Bash(ls)"]},
          "hooks": {"PreToolUse": [{"matcher": "Write", "hooks": [{"type":"command","command":"echo hi"}]}]}
        }"#;
        std::fs::write(dir.join(".claude").join("settings.local.json"), existing).unwrap();

        let written = write_guardrails(&dir, "claude.cmd");
        assert!(written.iter().any(|w| w.ends_with("settings.local.json")));
        assert!(written.iter().any(|w| w.ends_with("awayterm-sandbox-guard.mjs")));

        let text = std::fs::read_to_string(dir.join(".claude").join("settings.local.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).expect("要是合法 JSON");
        // 原本的欄位還在
        assert!(v["permissions"]["allow"][0].as_str() == Some("Bash(ls)"));
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2, "原本那個 hook 要留著，我們的要接在後面");
        assert_eq!(pre[0]["matcher"], "Write");
        assert_eq!(pre[1]["matcher"], "Bash");

        // 再跑一次不可以重複加
        write_guardrails(&dir, "claude.cmd");
        let text = std::fs::read_to_string(dir.join(".claude").join("settings.local.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guardrails_skipped_for_non_claude_tools() {
        let dir = std::env::temp_dir().join(format!("awayterm-guard2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(write_guardrails(&dir, "pwsh.exe").is_empty());
        assert!(!dir.join(".claude").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
