//! 自訂連線（搬移舊版）。
//!
//! 舊版在 `Dialogs/CustomConnDialog.xaml.cs`（清單 UI ＋ 自動偵測）與
//! `MainWindow.xaml.cs` 的 `OpenCustom`（啟動）。這裡搬的是**資料與規則**，
//! UI 改成頁內對話框（在 `src/tabbar.js`）。
//!
//! | 舊版 | 這裡 |
//! |---|---|
//! | `AppSettings.CustomConns` | `settings.custom_conns`（多一個 `sandbox` 欄位） |
//! | `CustomConnDialog.KnownTools` | [`KNOWN_TOOLS`]，**順序照抄**（四個 AI CLI 在前、WSL 在 GeminiCLI 之後）；2.0.6 多了 Antigravity（在 QwenCode 之後）、2.0.10 多了 Grok（在 Antigravity 之後） |
//! | `CustomConnDialog.ResolveTool` | [`resolve_tool`]，找的目錄照抄 |
//! | `AutoDetect_Click` | [`auto_detect`]：已存在（同名或**同路徑**）就跳過 |
//! | `OpenCustom` 的 `closeBytes` | `CustomConn::close_bytes()` |
//! | v1.0.18 起不自動建立任何自訂連線 | 同（`custom_conns` 預設是空的） |

use crate::i18n::{t, tf};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::State;

use crate::settings::{CustomConn, SettingsStore};

/// 自動偵測會找的已知工具。**順序就是加入清單的順序**（舊版使用者 2026-09-15 指定）。
pub struct KnownTool {
    pub name: &'static str,
    /// 依序嘗試的執行檔名（npm 裝的通常是 `.cmd`）。
    pub exe_names: &'static [&'static str],
    pub args: &'static str,
    pub icon: &'static str,
    pub pick_dir: bool,
}

pub const KNOWN_TOOLS: &[KnownTool] = &[
    KnownTool {
        name: "ClaudeCode",
        exe_names: &["claude.exe", "claude.cmd", "claude"],
        args: "--dangerously-skip-permissions",
        icon: "claude-code",
        pick_dir: true,
    },
    // Codex CLI：桌面版把 CLI 裝在 %LOCALAPPDATA%\Programs\OpenAI\Codex\bin 並加進使用者 PATH；
    // npm 版是 %APPDATA%\npm\codex.cmd。參數刻意留空（不預設跳過核准，需要的人自己加）。
    KnownTool {
        name: "Codex",
        exe_names: &["codex.exe", "codex.cmd", "codex"],
        args: "",
        icon: "codex",
        pick_dir: true,
    },
    // OpenCode 預設帶 --auto（舊版使用者要求 2026-09-15）
    KnownTool {
        name: "OpenCode",
        exe_names: &["opencode.exe", "opencode.cmd", "opencode"],
        args: "--auto",
        icon: "opencode",
        pick_dir: true,
    },
    KnownTool {
        name: "GeminiCLI",
        exe_names: &["gemini.exe", "gemini.cmd", "gemini"],
        args: "",
        icon: "geminicli",
        pick_dir: true,
    },
    // Qwen Code：參數留空（要跳過核准的人自己加 --yolo）
    KnownTool {
        name: "QwenCode",
        exe_names: &["qwen.exe", "qwen.cmd", "qwen"],
        args: "",
        icon: "qwen",
        pick_dir: true,
    },
    // Antigravity CLI（Google，2026-05 推出；指令叫 `agy`，Go 單一執行檔，Windows 裝在
    // %LOCALAPPDATA%\agy\bin，mac／Linux 在 ~/.local/bin）。2.0.6 只做到「自訂連線＋選模型＋沙盒」
    // （使用者 2026-10-05 定的第 1 層），代理團隊還不能選它。參數留空（要跳過核准的人自己加
    // --dangerously-skip-permissions）
    KnownTool {
        name: "Antigravity",
        exe_names: &["agy.exe", "agy"],
        args: "",
        icon: "antigravity",
        pick_dir: true,
    },
    // Grok CLI（xAI 的 Grok Build，2026-05 推出、08-07 出 1.0；指令叫 `grok`；
    // 官方安裝程式放在 ~/.grok/bin（Windows 是 %USERPROFILE%\.grok\bin）並加進使用者 PATH）。
    // 2.0.10 只做到第 1 層（自訂連線＋選模型＋沙盒），代理團隊還不能選它。參數留空
    // （要跳過核准的人自己加 --yolo）
    KnownTool {
        name: "Grok",
        exe_names: &["grok.exe", "grok"],
        args: "",
        icon: "grok",
        pick_dir: true,
    },
    KnownTool {
        name: "WSL",
        exe_names: &["wsl.exe"],
        args: "",
        icon: "wsl",
        pick_dir: false,
    },
    KnownTool {
        name: "Aider",
        exe_names: &["aider.exe", "aider.cmd", "aider"],
        args: "",
        icon: "run",
        pick_dir: true,
    },
    // ADB：舊版 v1.0.18 起不再是內建選單項目，改成一般自訂連線。參數固定 shell。
    KnownTool {
        name: "ADB",
        exe_names: &["adb.exe", "adb"],
        args: "shell",
        icon: "adb",
        pick_dir: false,
    },
];

/// 這個工具要不要沙盒？
///
/// `global`＝設定視窗的「新增的自訂連線預設開啟沙盒」（`settings.sandbox_default`；
/// **出廠值是關**，2026-10-02 使用者改的）。使用者把它打開之後，自動偵測加進來的
/// AI coding agent 才預設開；WSL／ADB 這種「使用者拿來操作機器」的工具永遠預設**關**——
/// 對它們開沙盒只會讓使用者莫名其妙進到一個 worktree 裡。
fn default_sandbox(name: &str, global: bool) -> bool {
    global && !matches!(name, "WSL" | "ADB")
}

/// 在 PATH 與常見安裝位置尋找工具，回傳第一個存在的完整路徑。
///
/// 額外目錄照抄舊版 `ResolveTool`：
/// `~/.local/bin`、`%APPDATA%\npm`、`%LOCALAPPDATA%\Programs`、
/// 以及 Codex 桌面版的 `…\Programs\OpenAI\Codex\bin`（它的 PATH 是事後才加的，
/// 比它早啟動的行程看不到 → 直接找目錄）。
pub fn resolve_tool(exe_names: &[&str]) -> Option<PathBuf> {
    for exe in exe_names {
        if let Some(p) = crate::pty::shell::which(exe) {
            return Some(p);
        }
        for dir in extra_dirs() {
            let full = dir.join(exe);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

fn extra_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home_dir() {
        dirs.push(home.join(".local").join("bin"));
        // OpenCode 官方安裝程式的位置（它只把這裡寫進 shell 的 rc 檔，比它早啟動的行程看不到）
        dirs.push(home.join(".opencode").join("bin"));
        // Grok CLI 官方安裝程式的位置（PATH 是事後才加的，比它早啟動的行程看不到）
        dirs.push(home.join(".grok").join("bin"));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        dirs.push(Path::new(&appdata).join("npm"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let programs = Path::new(&local).join("Programs");
        dirs.push(programs.join("OpenAI").join("Codex").join("bin"));
        dirs.push(programs);
        // Antigravity CLI 的安裝程式把 agy.exe 放這裡並加進使用者 PATH（同 Codex：比它早啟動的行程看不到）
        dirs.push(Path::new(&local).join("agy").join("bin"));
    }
    dirs
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(PathBuf::from)
}

/// 自動偵測的結果裡的一項（每個已知工具一項；2.0.13 起偵測完跳視窗列出來，使用者要求）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectItem {
    pub name: String,
    /// `added`（這次新加入）／`existing`（清單裡已經有）／`missing`（這台電腦找不到）。
    pub status: &'static str,
    /// 找到的（或清單裡那一條的）執行檔路徑；`missing` 是空的。
    pub path: String,
}

/// 自動偵測：把找到、而且清單裡還沒有的工具加進去。回傳這次加了哪些名稱。
///
/// 「已存在」的判斷照舊版：**同名**或**同路徑**都算（後者是為了 1.1.10 以前叫
/// 「Gemini」的舊項目，避免重複加入）。
pub fn auto_detect(existing: &[CustomConn], sandbox_global: bool) -> Vec<CustomConn> {
    detect_report(existing, sandbox_global).0
}

/// [`auto_detect`] ＋每個已知工具的結果（順序＝[`KNOWN_TOOLS`]）。
pub fn detect_report(existing: &[CustomConn], sandbox_global: bool) -> (Vec<CustomConn>, Vec<DetectItem>) {
    let mut added = Vec::new();
    let mut report = Vec::new();
    let item = |name: &str, status, path: &str| DetectItem {
        name: name.to_string(),
        status,
        path: path.to_string(),
    };
    for tool in KNOWN_TOOLS {
        if let Some(c) = existing
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(tool.name))
        {
            report.push(item(tool.name, "existing", &c.path));
            continue;
        }
        let Some(path) = resolve_tool(tool.exe_names) else {
            report.push(item(tool.name, "missing", ""));
            continue;
        };
        let path_str = path.to_string_lossy().to_string();
        if existing
            .iter()
            .chain(added.iter())
            .any(|c: &CustomConn| c.path.eq_ignore_ascii_case(&path_str))
        {
            report.push(item(tool.name, "existing", &path_str));
            continue;
        }
        report.push(item(tool.name, "added", &path_str));
        // `.cmd` / `.bat` 要透過 shell 跑（舊版同款判斷）
        let lower = path_str.to_ascii_lowercase();
        let via_ps = lower.ends_with(".cmd") || lower.ends_with(".bat");
        added.push(CustomConn {
            name: tool.name.to_string(),
            path: path_str,
            args: tool.args.to_string(),
            icon: tool.icon.to_string(),
            pick_dir: tool.pick_dir,
            via_powershell: via_ps,
            sandbox: default_sandbox(tool.name, sandbox_global),
            ..CustomConn::default()
        });
    }
    (added, report)
}

// ------------------------------------------------------------------ commands

/// 自訂連線清單（前端「新分頁 ▾」與設定對話框用）。
#[tauri::command]
pub fn custom_list(settings: State<'_, Arc<SettingsStore>>) -> Vec<CustomConn> {
    settings.get().custom_conns
}

/// 執行一次自動偵測並存檔。回傳每個已知工具的結果（前端跳視窗列出「新加入／已經有／沒找到」）。
#[tauri::command]
pub fn custom_detect(settings: State<'_, Arc<SettingsStore>>) -> Vec<DetectItem> {
    let cfg = settings.get();
    let (added, report) = detect_report(&cfg.custom_conns, cfg.sandbox_default);
    if !added.is_empty() {
        let names: Vec<String> = added.iter().map(|c| c.name.clone()).collect();
        settings.update(|s| s.custom_conns.extend(added));
        println!("[AwayTerminal] 自動偵測加入：{}", names.join("、"));
    }
    report
}

/// 「回到預設」（2.0.8，使用者要求）：**清掉整份自訂連線清單**，清完就是空的。
/// 使用者自己加的、改過的都會不見（前端先確認過才會呼叫）。回傳清掉幾條。
///
/// **不會**接著自動偵測（2026-10-05 使用者改的；2.0.8 原本會）：偵測到的工具馬上又加回來，
/// 看起來就像「按了沒刪」。要加回來由使用者自己按「自動偵測」。
#[tauri::command]
pub fn custom_reset(settings: State<'_, Arc<SettingsStore>>) -> usize {
    let removed = settings.get().custom_conns.len();
    settings.update(|s| s.custom_conns.clear());
    println!("[AwayTerminal] 自訂連線回到預設：清掉 {removed} 條");
    removed
}

/// 新增或更新一條自訂連線（依 `name` 比對；`original_name` 是改名時的舊名）。
#[tauri::command]
pub fn custom_save(
    conn: CustomConn,
    original_name: Option<String>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<(), String> {
    let name = conn.name.trim().to_string();
    if name.is_empty() {
        return Err(t("err.needName").to_string());
    }
    if conn.path.trim().is_empty() {
        return Err(t("err.needExePath").to_string());
    }
    let key = original_name.unwrap_or_else(|| name.clone());
    // 改名成另一條既有連線的名字 → 會變成兩條同名（`find` 永遠取第一條、`delete` 兩條一起刪；
    // BUG D12）。不是改名（key 就是 name）時是「更新同名那條」，不算衝突。
    if !key.trim().eq_ignore_ascii_case(&name)
        && settings
            .get()
            .custom_conns
            .iter()
            .any(|c| c.name.trim().eq_ignore_ascii_case(&name))
    {
        return Err(tf("err.connNameTaken", &[&name]));
    }
    settings.update(|s| {
        match s
            .custom_conns
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(&key))
        {
            Some(slot) => *slot = conn,
            None => s.custom_conns.push(conn),
        }
    });
    Ok(())
}

#[tauri::command]
pub fn custom_delete(name: String, settings: State<'_, Arc<SettingsStore>>) {
    settings.update(|s| s.custom_conns.retain(|c| !c.name.eq_ignore_ascii_case(&name)));
}

/// 切換某條自訂連線的沙盒開關（分頁右鍵「沙盒模式」）。
///
/// **改的是設定，不是目前這個分頁**——`CLAUDE.md` 明寫「改變在下次啟動該分頁時生效」。
/// 前端會提示，並提供「立即重新啟動分頁」。
#[tauri::command]
pub fn conn_set_sandbox(
    app: tauri::AppHandle,
    name: String,
    sandbox: bool,
    settings: State<'_, Arc<SettingsStore>>,
    tabs_state: State<'_, Arc<crate::tabs::TabManager>>,
) -> Result<(), String> {
    let mut found = false;
    settings.update(|s| {
        if let Some(c) = s
            .custom_conns
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(&name))
        {
            c.sandbox = sandbox;
            found = true;
        }
    });
    if !found {
        return Err(tf("err.connNotFound", &[&name]));
    }
    println!("[AwayTerminal] 自訂連線「{name}」沙盒模式 → {sandbox}（下次啟動生效）");
    crate::tabs::emit_state(&app, &tabs_state);
    Ok(())
}

/// 清除某個分頁的沙盒 worktree（分頁右鍵「清除沙盒…」）。
///
/// **分支保留**——裡面可能有還沒合併回去的成果（`docs/AGENT-SANDBOX.md` 說明怎麼合併）。
#[tauri::command]
pub fn sandbox_clear(
    id: u32,
    tabs_state: State<'_, Arc<crate::tabs::TabManager>>,
) -> Result<String, String> {
    let sb = tabs_state
        .sandbox_of(id)
        .ok_or_else(|| t("err.tabNoSandbox").to_string())?;
    if !sb.has_worktree {
        return Err(t("err.sandboxNoWorktree").to_string());
    }
    // BUG D1：還有行程在 worktree 裡跑就不清——Windows 刪不掉使用中的目錄，
    // git 會清掉一半內容再把 worktree 除名。代理團隊的每一格共用同一棵 worktree，
    // 所以用 work_dir 比對**所有**分頁，不是只看這一格。
    let in_use = tabs_state.poll_snapshot().into_iter().any(|(tid, _, pid, _)| {
        pid != 0
            && tabs_state
                .sandbox_of(tid)
                .is_some_and(|o| o.work_dir == sb.work_dir)
            && crate::status::pid_exists(pid)
    });
    if in_use {
        return Err(t("err.sandboxInUse").to_string());
    }
    crate::sandbox::remove_worktree(&sb.work_dir)?;
    Ok(sb.branch)
}

/// 依名稱取一條自訂連線。
pub fn find(settings: &SettingsStore, name: &str) -> Option<CustomConn> {
    settings
        .get()
        .custom_conns
        .into_iter()
        .find(|c| c.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_detect_skips_same_name_and_same_path() {
        // 名稱已存在 → 跳過
        let existing = vec![CustomConn {
            name: "ClaudeCode".into(),
            path: "C:\\whatever\\claude.cmd".into(),
            ..CustomConn::default()
        }];
        let added = auto_detect(&existing, true);
        assert!(!added.iter().any(|c| c.name == "ClaudeCode"));

        // 路徑已存在但名稱不同（舊版叫 Gemini 的那種）→ 也要跳過。
        // 用實際偵測到的路徑來組測試資料，機器上沒裝任何工具時自動跳過這段。
        let all = auto_detect(&[], true);
        if let Some(first) = all.first() {
            let existing = vec![CustomConn {
                name: "SomethingElse".into(),
                path: first.path.clone(),
                ..CustomConn::default()
            }];
            let added = auto_detect(&existing, true);
            assert!(
                !added.iter().any(|c| c.path == first.path),
                "同一支執行檔不該被加第二次"
            );
        }
    }

    /// 偵測結果（2.0.13）：每個已知工具剛好一項、順序照 KNOWN_TOOLS；「新加入」的和真的加進去的一致。
    #[test]
    fn detect_report_lists_every_known_tool() {
        let existing = vec![CustomConn {
            name: "ClaudeCode".into(),
            path: "C:\\whatever\\claude.cmd".into(),
            ..CustomConn::default()
        }];
        let (added, report) = detect_report(&existing, false);
        let names: Vec<&str> = report.iter().map(|r| r.name.as_str()).collect();
        let known: Vec<&str> = KNOWN_TOOLS.iter().map(|t| t.name).collect();
        assert_eq!(names, known);
        assert_eq!(report[0].status, "existing");
        assert_eq!(report[0].path, "C:\\whatever\\claude.cmd");
        let added_names: Vec<&str> = report.iter().filter(|r| r.status == "added").map(|r| r.name.as_str()).collect();
        assert_eq!(added_names, added.iter().map(|c| c.name.as_str()).collect::<Vec<_>>());
        assert!(report.iter().filter(|r| r.status == "missing").all(|r| r.path.is_empty()));
    }

    #[test]
    fn sandbox_follows_the_global_default_except_for_shells() {
        // 設定裡把「新增的自訂連線預設開啟沙盒」打開 → agent 開、WSL／ADB 仍然關
        assert!(default_sandbox("ClaudeCode", true));
        assert!(default_sandbox("Codex", true));
        assert!(default_sandbox("Aider", true));
        assert!(!default_sandbox("WSL", true));
        assert!(!default_sandbox("ADB", true));
        // 全域預設是關的（**出廠值**，2026-10-02 起）→ 連 agent 也不開（TASK-015 A4）
        assert!(!default_sandbox("ClaudeCode", false));
        assert!(!crate::settings::AppSettings::default().sandbox_default);
        // 新建一條（使用者自己加的）預設也是關的
        assert!(!CustomConn::default().sandbox);
    }

    #[test]
    fn close_bytes_follow_old_rules() {
        let c = CustomConn::default(); // ctrl-c ×3
        assert_eq!(c.close_bytes(), vec![0x03, 0x03, 0x03]);
        let d = CustomConn {
            close_key: "ctrl-d".into(),
            close_count: 2,
            ..CustomConn::default()
        };
        assert_eq!(d.close_bytes(), vec![0x04, 0x04]);
        let n = CustomConn {
            close_key: "none".into(),
            ..CustomConn::default()
        };
        assert!(n.close_bytes().is_empty());
        // 超出 1~5 退回 3（舊版同款夾範圍）
        let bad = CustomConn {
            close_count: 99,
            ..CustomConn::default()
        };
        assert_eq!(bad.close_bytes().len(), 3);
    }
}
