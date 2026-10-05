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
    /// 護欄沒有真的生效的原因（空字串＝沒問題）。目前只有「Claude Code 的 hook 要 node，
    /// 但 PATH 找不到 node」這一種（BUG D15：原生 `claude.exe` 不帶 node）。
    /// 已翻譯好的字，前端 tooltip 直接顯示。
    pub guard_warning: String,
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
                // 重用既有 worktree 時回的是它**實際**的分支（BUG D5），不是上面新組的名字
                Ok(branch) => {
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
    // mac／Linux 的程式看的是 TMPDIR，不是 TEMP／TMP（BUG D13）
    if !cfg!(windows) {
        env.push(("TMPDIR".to_string(), tmp.to_string_lossy().to_string()));
    }
    // Rust 專案才設 CARGO_TARGET_DIR：非 Rust 專案設了只是多一個沒人看的變數
    if work.join("Cargo.toml").is_file() {
        env.push((
            "CARGO_TARGET_DIR".to_string(),
            root.join(".target").to_string_lossy().to_string(),
        ));
    }

    // ---- 3. 護欄設定 ----
    let guardrails = write_guardrails(&work, tool_path);
    let guard_warning = guard_warning(tool_path);
    if !guard_warning.is_empty() {
        println!("[AwayTerminal] 沙盒護欄未生效：{guard_warning}");
    }

    Ok(Sandbox {
        root: root.to_string_lossy().to_string(),
        work_dir: work.to_string_lossy().to_string(),
        branch,
        has_worktree,
        env,
        guardrails,
        extra_args: extra_args(tool_path).to_string(),
        guard_warning,
    })
}

/// 護欄有沒有真的會生效（BUG D15）。回傳已翻譯的警告，沒問題回空字串。
///
/// Claude Code 的 hook 是 `node "<腳本>"`：npm 裝的 claude 一定有 node，但**原生安裝的
/// `claude.exe` 不需要 node**，這時 hook 每次都執行失敗（Claude Code 把它當非阻擋錯誤，
/// 指令照樣放行）→ 護欄等於沒有，卻看起來開著。找不到 node 就明講。
/// 代理團隊的護欄也是 `write_guardrails` 寫的，要顯示同一句可以呼叫這裡。
pub fn guard_warning(tool_path: &str) -> String {
    if tool_kind(tool_path) != ToolKind::Claude || node_available() {
        return String::new();
    }
    t("sb.guardNoNode")
}

fn node_available() -> bool {
    crate::pty::shell::which("node.exe")
        .or_else(|| crate::pty::shell::which("node"))
        .is_some()
}

/// 工具本身支援的沙盒參數（`CLAUDE.md`：Codex `--sandbox workspace-write`、Gemini `--sandbox`）。
///
/// ⚠️ 參數名是照 `CLAUDE.md` 寫的，**還沒有在本機實際跑過那兩個工具驗證**
/// （這台機器上沒裝）。列在 TASK_RESULT 的「需要使用者確認」。
pub fn extra_args(tool_path: &str) -> &'static str {
    match tool_kind(tool_path) {
        ToolKind::Codex => " --sandbox workspace-write",
        // Antigravity CLI 的 `--sandbox`＝「Run in a sandbox with terminal restrictions enabled」
        // （照它的 --help，2.0.6；同樣沒在本機跑過）
        ToolKind::Gemini | ToolKind::Antigravity => " --sandbox",
        _ => "",
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum ToolKind {
    Claude,
    Codex,
    Gemini,
    /// Antigravity CLI（執行檔叫 `agy`）。
    Antigravity,
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
    } else if stem == "agy" || stem.contains("antigravity") {
        // `agy` 要整個檔名相等：三個字母太短，`contains` 會把 strategy.exe 之類的也算進去
        ToolKind::Antigravity
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

/// `git worktree add -b <branch> <path>`（從目前 HEAD 開）。回傳這個 worktree **實際**的分支。
///
/// 目錄已經存在時（同一條連線重開）：
/// - **確實是這個 repo 登記中的 worktree** → 直接用它，分支從 worktree 讀回來（BUG D5：
///   以前回的是這次新組、根本不存在的分支名）。
/// - 否則（BUG D2：舊版 `sandbox_clear` 留下的空殼、`worktree add` 失敗時 fallback 建的
///   一般目錄）→ 以前直接當 worktree 用，agent 的 git 往上找到主 repo，隔離整個反過來。
///   現在先 prune、把目錄清掉（只剩我們自己的 `.tmp`／`.target` 才刪，有別的東西就改名
///   留著不丟）再重新 add。
fn add_worktree(repo: &Path, path: &Path, branch: &str) -> Result<String, String> {
    if path.exists() {
        if path.join(".git").is_file() && is_registered_worktree(repo, path) {
            let actual = git(path, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
            // detached HEAD 會回 `HEAD`，那不是分支名
            return Ok(if actual == "HEAD" { String::new() } else { actual });
        }
        println!(
            "[AwayTerminal] 沙盒目錄存在但不是有效的 worktree，清掉重開：{}",
            path.display()
        );
        let _ = git(repo, &["worktree", "prune"]);
        discard_stale_dir(path)?;
    }
    // 已登記但目錄已經不見的 worktree 要先除名，否則 `worktree add` 會拒絕同一個路徑
    let _ = git(repo, &["worktree", "prune"]);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| tf("err.sandboxDirFailed", &[&e.to_string()]))?;
    }
    let path_str = path.to_string_lossy().to_string();
    git(repo, &["worktree", "add", "-b", branch, &path_str])?;
    Ok(branch.to_string())
}

/// 把「不是 worktree 的沙盒目錄」讓開。
///
/// 裡面只有我們自己產生的東西（`.tmp`／`.target`／`.claude`）→ 直接刪；
/// 有別的東西（可能是使用者或 agent 的成果）→ **改名留著**（`<名稱>.stale-<時間>`），不丟資料。
fn discard_stale_dir(path: &Path) -> Result<(), String> {
    const OURS: [&str; 3] = [".tmp", ".target", ".claude"];
    let only_ours = std::fs::read_dir(path)
        .map(|rd| {
            rd.flatten()
                .all(|e| OURS.iter().any(|o| e.file_name().to_string_lossy() == *o))
        })
        .unwrap_or(false);
    if only_ours {
        return std::fs::remove_dir_all(path).map_err(|e| tf("err.sandboxDirFailed", &[&e.to_string()]));
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut aside = path.as_os_str().to_owned();
    aside.push(format!(".stale-{stamp}"));
    std::fs::rename(path, &aside).map_err(|e| tf("err.sandboxDirFailed", &[&e.to_string()]))?;
    println!(
        "[AwayTerminal] 舊的沙盒目錄裡有東西，改名留著：{}",
        PathBuf::from(aside).display()
    );
    Ok(())
}

/// 兩個路徑是不是同一個地方（git 在 Windows 回 `C:/x/y`，我們手上是 `C:\x\y`；
/// 還可能一邊是 8.3 短檔名 → 能 canonicalize 就先 canonicalize）。
fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let c = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let s = c.to_string_lossy().replace('\\', "/");
        let s = s.trim_start_matches("//?/").trim_end_matches('/').to_string();
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    norm(a) == norm(b)
}

/// `path` 是不是 `repo` 目前登記中的 worktree（`git worktree list --porcelain`）。
fn is_registered_worktree(repo: &Path, path: &Path) -> bool {
    git(repo, &["worktree", "list", "--porcelain"])
        .map(|list| {
            list.lines()
                .filter_map(|l| l.strip_prefix("worktree "))
                .any(|w| same_path(Path::new(w), path))
        })
        .unwrap_or(false)
}

/// 主 repo（主工作樹）的根目錄。在連結 worktree 裡問 `--show-toplevel` 回的是 worktree
/// 自己，所以改問 `--git-common-dir`（所有 worktree 共用的 `<repo>/.git`）再取上一層。
fn main_repo_of(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    let common = git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .ok()
        .map(PathBuf::from)
        // git < 2.31 沒有 --path-format：回的可能是相對路徑
        .or_else(|| git(dir, &["rev-parse", "--git-common-dir"]).ok().map(|s| dir.join(s)))?;
    // bare repo 沒有工作樹，沙盒也不會開在那種地方
    if common.file_name().map(|n| n == ".git").unwrap_or(false) {
        common.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

/// 目錄已經不在（或 git 認不得）時，照 [`prepare`] 的配置倒推主 repo：
/// `<repo>/.ai/sandbox/<name>`。
fn repo_from_layout(path: &Path) -> Option<PathBuf> {
    let sandbox = path.parent()?;
    let ai = sandbox.parent()?;
    if sandbox.file_name()? != SANDBOX_DIRS[1] || ai.file_name()? != SANDBOX_DIRS[0] {
        return None;
    }
    let repo = ai.parent()?;
    repo.join(".git").exists().then(|| repo.to_path_buf())
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
///
/// BUG D1：以前用 `git_toplevel(worktree)`（回的是 worktree 自己）當 cwd 跑
/// `git worktree remove` → Windows 不能刪行程的 cwd → git 清完內容、刪不掉目錄、
/// 卻照樣把 worktree 除名，留下空殼（再按一次得到 `not a working tree`）。
/// 現在一律在**主 repo** 跑；失敗時 prune，已除名但目錄還在就自己刪。
///
/// ⚠️ 呼叫端要先確定沒有分頁還在用這個 worktree（`custom::sandbox_clear` 會檢查）。
pub fn remove_worktree(work_dir: &str) -> Result<(), String> {
    let path = PathBuf::from(work_dir);
    let repo = main_repo_of(&path)
        .or_else(|| repo_from_layout(&path))
        .ok_or_else(|| t("err.notGitRepo").to_string())?;
    if same_path(&repo, &path) {
        // 保險：絕不對主工作樹本身做 remove
        return Err(t("err.sandboxNoWorktree").to_string());
    }
    let path_str = path.to_string_lossy().to_string();
    if let Err(e) = git(&repo, &["worktree", "remove", "--force", &path_str]) {
        let _ = git(&repo, &["worktree", "prune"]);
        if is_registered_worktree(&repo, &path) {
            // 還登記著＝真的沒移掉（例如檔案被鎖），照實回報
            return Err(tf("err.gitFailed", &["worktree remove", &e]));
        }
        println!("[AwayTerminal] worktree remove 失敗但已除名，自己清殘留目錄：{e}");
    }
    // git 成功也可能因為檔案被鎖留下目錄；已經除名的殘骸不留（否則下次重開會被當成 worktree）
    if path.exists() {
        std::fs::remove_dir_all(&path)
            .map_err(|e| tf("err.sandboxLeftover", &[&path_str, &e.to_string()]))?;
    }
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
        // `kill(pid, 0)` 而不是看 `/proc/<pid>`：mac 沒有 `/proc`，而且這個做法
        // 在兩個 Unix 上都對（唯讀，只檢查存在與權限、不送訊號）。
        awayterm_platform::proctree::pid_exists(pid)
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
        assert_eq!(tool_kind("C:\\Users\\x\\AppData\\Local\\agy\\bin\\agy.exe"), ToolKind::Antigravity);
        assert_eq!(tool_kind("/home/x/.local/bin/agy"), ToolKind::Antigravity);
        assert_eq!(tool_kind("strategy.exe"), ToolKind::Other); // 不能用 contains("agy")
    }

    #[test]
    fn extra_args_match_claude_md() {
        assert_eq!(extra_args("codex.cmd"), " --sandbox workspace-write");
        assert_eq!(extra_args("gemini.cmd"), " --sandbox");
        assert_eq!(extra_args("agy.exe"), " --sandbox");
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

    /// 在暫存資料夾建一個有一個 commit 的 repo。機器上沒有 git 就回 None（測試跳過）。
    fn temp_repo(tag: &str) -> Option<PathBuf> {
        let dir = std::env::temp_dir().join(format!("awayterm-wt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        git(&dir, &["init", "-q"]).ok()?;
        git(
            &dir,
            &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"],
        )
        .ok()?;
        Some(dir)
    }

    /// D1／D5：清除沙盒要真的把 worktree 拿掉（目錄不留、git 也除名）；
    /// 重用既有 worktree 時分支要讀回實際的名字。
    #[test]
    fn worktree_reuse_and_remove() {
        let Some(repo) = temp_repo("rm") else { return };
        let wt = repo.join(".ai").join("sandbox").join("t1");
        let b1 = add_worktree(&repo, &wt, "sandbox/t1-first").unwrap();
        assert_eq!(b1, "sandbox/t1-first");
        // 重開：不可以回新組的名字（那個分支不存在）
        let b2 = add_worktree(&repo, &wt, "sandbox/t1-second").unwrap();
        assert_eq!(b2, "sandbox/t1-first", "重用時要回實際分支");
        std::fs::write(wt.join("work.txt"), "x").unwrap();

        remove_worktree(&wt.to_string_lossy()).unwrap();
        assert!(!wt.exists(), "worktree 目錄要不見");
        assert!(!is_registered_worktree(&repo, &wt));
        // 分支保留
        assert!(git(&repo, &["rev-parse", "--verify", "sandbox/t1-first"]).is_ok());
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// D2：同路徑是一般目錄（舊殘骸／fallback 建的）→ 不可以當成 worktree，要清掉重開。
    #[test]
    fn stale_plain_dir_is_replaced() {
        let Some(repo) = temp_repo("stale") else { return };
        let wt = repo.join(".ai").join("sandbox").join("t2");
        std::fs::create_dir_all(wt.join(".tmp")).unwrap();
        let b = add_worktree(&repo, &wt, "sandbox/t2-x").unwrap();
        assert_eq!(b, "sandbox/t2-x");
        assert!(wt.join(".git").is_file(), "要是真的 worktree");
        assert!(is_registered_worktree(&repo, &wt));

        // 有別人的東西的殘骸 → 改名留著，不刪
        let wt3 = repo.join(".ai").join("sandbox").join("t3");
        std::fs::create_dir_all(&wt3).unwrap();
        std::fs::write(wt3.join("keep.txt"), "important").unwrap();
        add_worktree(&repo, &wt3, "sandbox/t3-x").unwrap();
        let kept = std::fs::read_dir(wt3.parent().unwrap())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("t3.stale-") && e.path().join("keep.txt").is_file());
        assert!(kept, "有內容的舊目錄要改名留著");
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn repo_from_layout_needs_sandbox_layout() {
        assert!(repo_from_layout(Path::new("C:/x/y")).is_none());
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
