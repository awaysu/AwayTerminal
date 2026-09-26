//! 沙盒模式的端到端驗證，**不需要開 GUI**。
//!
//! 1. 在 `%TEMP%` 建一個乾淨的 git repo（一個 commit）
//! 2. 呼叫 `sandbox::prepare()`：開 worktree、設環境變數、產生護欄設定
//! 3. 檢查產出（worktree／分支／`.git/info/exclude`／`.claude/*`／JSON 合法）
//! 4. 加參數 `--live` 時**真的跑一次 Claude Code**，確認 `PreToolUse` hook 擋得下
//!    `taskkill /IM`（這會花一次 API 呼叫）
//!
//! 用法：
//! ```text
//! cargo run --example sandbox_probe            # 不呼叫 API
//! cargo run --example sandbox_probe -- --live  # 連 Claude Code 一起驗
//! ```
//!
//! ⚠️ 巢狀執行：Claude Code 看到 `CLAUDECODE=1` 會拒絕在自己裡面再開一個，
//! 所以 `--live` 會把那一族環境變數清掉（見 [`clear_nested_claude_env`]）。

use std::path::{Path, PathBuf};
use std::process::Command;

use awayterminal_lib::sandbox;

fn main() {
    let live = std::env::args().any(|a| a == "--live");
    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut report = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("PASS  {name}：{detail}");
        } else {
            fail += 1;
            println!("FAIL  {name}：{detail}");
        }
    };

    println!("== AwayTerminal2 sandbox_probe ==");

    // ------------------------------------------------ 1. 乾淨的測試 repo
    let dir = std::env::temp_dir().join(format!("awayterm-sandbox-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建立測試目錄");
    if let Err(e) = make_repo(&dir) {
        println!("FAIL  建立測試 repo：{e}");
        std::process::exit(1);
    }
    println!("測試 repo：{}", dir.display());

    // Claude Code 的路徑：PATH 找不到時補 `~/.local/bin`（這台機器就裝在那裡）
    let claude = find_claude();
    let tool = claude
        .clone()
        .unwrap_or_else(|| PathBuf::from("claude.exe"));

    // ------------------------------------------------ 2. prepare
    let sb = match sandbox::prepare(&dir, "ClaudeCode", &tool.to_string_lossy()) {
        Ok(sb) => sb,
        Err(e) => {
            println!("FAIL  sandbox::prepare：{e}");
            std::process::exit(1);
        }
    };
    println!("沙盒根：{}", sb.root);
    println!("工作目錄：{}", sb.work_dir);

    // ------------------------------------------------ 3. 檢查產出
    report(
        "worktree 開起來了",
        sb.has_worktree && Path::new(&sb.work_dir).is_dir(),
        format!("has_worktree={} 分支={}", sb.has_worktree, sb.branch),
    );
    report(
        "分支名格式 sandbox/<名>-<時間>",
        sb.branch.starts_with("sandbox/ClaudeCode-"),
        sb.branch.clone(),
    );
    let branch_exists = git(&dir, &["rev-parse", "--verify", &sb.branch]).is_ok();
    report("分支真的存在", branch_exists, sb.branch.clone());

    let status = git(&dir, &["status", "--porcelain"]).unwrap_or_default();
    report(
        ".ai/sandbox 被忽略（git status 乾淨）",
        !status.contains(".ai/sandbox"),
        if status.trim().is_empty() {
            "git status 空的".to_string()
        } else {
            format!("git status:\n{status}")
        },
    );
    let exclude = std::fs::read_to_string(dir.join(".git").join("info").join("exclude"))
        .unwrap_or_default();
    report(
        "忽略寫在 .git/info/exclude（不是 .gitignore）",
        exclude.contains("/.ai/sandbox/") && !dir.join(".gitignore").exists(),
        "使用者的 .gitignore 沒有被建立或修改".to_string(),
    );

    let env: std::collections::HashMap<_, _> = sb.env.iter().cloned().collect();
    report(
        "TEMP／TMP 導到沙盒",
        env.get("TEMP").is_some_and(|v| v.starts_with(&sb.root))
            && env.get("TMP").is_some_and(|v| v.starts_with(&sb.root)),
        format!("TEMP={}", env.get("TEMP").cloned().unwrap_or_default()),
    );
    report(
        "沒有動 HOME／APPDATA／USERPROFILE",
        !["HOME", "APPDATA", "LOCALAPPDATA", "USERPROFILE"]
            .iter()
            .any(|k| env.contains_key(*k)),
        "這幾個一改 agent 就會掉登入".to_string(),
    );

    let settings_path = Path::new(&sb.work_dir)
        .join(".claude")
        .join("settings.local.json");
    let guard_path = Path::new(&sb.work_dir)
        .join(".claude")
        .join("awayterm-sandbox-guard.mjs");
    report(
        "產生護欄檔案",
        settings_path.is_file() && guard_path.is_file(),
        format!("{:?}", sb.guardrails),
    );
    match std::fs::read_to_string(&settings_path) {
        Ok(text) => {
            let v: Result<serde_json::Value, _> = serde_json::from_str(&text);
            let ok = v
                .as_ref()
                .map(|v| v["hooks"]["PreToolUse"][0]["matcher"] == "Bash")
                .unwrap_or(false);
            report(
                "settings.local.json 是合法 JSON 且掛上 PreToolUse(Bash)",
                ok,
                text.lines().take(3).collect::<Vec<_>>().join(" / "),
            );
        }
        Err(e) => report("讀 settings.local.json", false, e.to_string()),
    }

    // ------------------------------------------------ 4. 真的跑一次 Claude Code
    if !live {
        println!("SKIP  Claude Code 實機驗證：加 `-- --live` 才跑（會花一次 API 呼叫）");
    } else if let Some(claude) = claude {
        println!("Claude Code：{}", claude.display());
        match run_claude_denied(&claude, &sb) {
            Ok(out) => {
                // 護欄的拒絕理由裡一定有這句（見 resources/sandbox-guard.mjs 的 SUFFIX）
                let blocked = out.contains("沙盒模式擋下") || out.contains("按名稱砍行程");
                report(
                    "Claude Code 的 PreToolUse hook 擋下 taskkill /IM",
                    blocked,
                    String::new(),
                );
                println!("---- claude 的輸出（前 1500 字）----");
                println!("{}", head(&out, 1500));
                println!("---- 輸出結束 ----");
            }
            Err(e) => report("跑 Claude Code", false, e),
        }
    } else {
        report("跑 Claude Code", false, "找不到 claude 執行檔".to_string());
    }

    // ------------------------------------------------ 收尾
    let _ = git(&dir, &["worktree", "remove", "--force", &sb.work_dir]);
    let _ = std::fs::remove_dir_all(&dir);
    println!();
    println!("RESULT: {pass} PASS / {fail} FAIL");
    if fail > 0 {
        std::process::exit(1);
    }
}

fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn find_claude() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CLAUDE_CODE_EXECPATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    for name in ["claude.exe", "claude.cmd", "claude"] {
        if let Some(p) = awayterminal_lib::pty::shell::which(name) {
            return Some(p);
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    let p = Path::new(&home).join(".local").join("bin").join("claude.exe");
    p.is_file().then_some(p)
}

/// 建一個有一個 commit 的 repo（worktree 要從 HEAD 開，所以不能是空 repo）。
fn make_repo(dir: &Path) -> Result<(), String> {
    git(dir, &["init", "-q"])?;
    // 測試 repo 要能 commit：使用者的全域設定可能沒有 user.name
    git(dir, &["config", "user.name", "awayterm-probe"])?;
    git(dir, &["config", "user.email", "probe@example.invalid"])?;
    std::fs::write(dir.join("README.md"), "sandbox probe\n").map_err(|e| e.to_string())?;
    git(dir, &["add", "README.md"])?;
    git(dir, &["commit", "-q", "-m", "init"])?;
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git {}：{e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Claude Code 在自己裡面再開一個會被拒絕（「Raw mode is not supported」／巢狀偵測），
/// 所以要把它用來認出「我在 Claude Code 裡」的環境變數清掉。
fn clear_nested_claude_env(cmd: &mut Command) {
    for key in [
        "CLAUDECODE",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_CODE_SESSION_ATTENDED",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_MESSAGING_SOCKET",
        "CLAUDE_CODE_MESSAGING_TOKEN",
        "CLAUDE_CODE_EXECPATH",
    ] {
        cmd.env_remove(key);
    }
}

/// 在沙盒工作區跑一次 claude，叫它執行一個會被護欄擋下的指令。
fn run_claude_denied(claude: &Path, sb: &sandbox::Sandbox) -> Result<String, String> {
    let mut cmd = Command::new(claude);
    cmd.current_dir(&sb.work_dir)
        .arg("-p")
        .arg("請用 Bash 工具執行這個指令：taskkill /IM notepad.exe　。如果被拒絕，請把拒絕的理由原文完整貼出來。")
        .arg("--dangerously-skip-permissions");
    clear_nested_claude_env(&mut cmd);
    // 沙盒的環境變數（護欄腳本要 AWAYTERM_SANDBOX_ROOT 才知道哪些路徑算沙盒內）
    for (k, v) in &sb.env {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|e| format!("啟動 claude 失敗：{e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        text.push_str("\n[stderr] ");
        text.push_str(&err);
    }
    Ok(text)
}
