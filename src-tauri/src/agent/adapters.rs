//! 四家 Coding Agent CLI 的差異（搬移舊版 `Services/MultiAgent/*Adapter.cs`）。
//!
//! 上層只做兩件會碰到 CLI 的事：**啟動它**（自訂連線＋附加參數，走既有的 `session_create`
//! `kind = "conn"` 那條路）、**往它的終端機打一行字**（[`super::deliver::send_text_then_enter`]），
//! 所以這個介面刻意很小。
//!
//! | CLI | 角色怎麼交給它 | 舊版註解裡的理由 |
//! |---|---|---|
//! | ClaudeCode | `--append-system-prompt-file "<角色檔>"` | 附加在預設系統提示後（舊版 1.1.11 實跑驗證） |
//! | Codex | `-c tui.whimsy=false -c "developer_instructions='…'"` | 只有字串版、沒有檔案版（官方 issue #12926 not-planned） |
//! | OpenCode | `--auto` ＋ 第一次閒置時打「請先讀角色檔，讀完回 READY」 | 沒有每 session 附加系統提示的參數 |
//! | GeminiCLI | 第一次閒置時打同一句 | `GEMINI_SYSTEM_MD` 會**整份取代**內建提示，不適合注入角色 |

use crate::settings::{CustomConn, SettingsStore};

/// 四家的 key（＝圖示 key）。順序＝設定視窗下拉的順序。
pub const ALL_KEYS: &[&str] = &["claude-code", "codex", "opencode", "geminicli"];

/// Codex 的 `developer_instructions` 直接內嵌的字數上限（超過就改帶短指引）。
const MAX_INLINE: usize = 8000;

/// 一家 CLI。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    ClaudeCode,
    Codex,
    OpenCode,
    GeminiCli,
}

impl Backend {
    pub fn by_key(key: &str) -> Option<Self> {
        Some(match key.to_ascii_lowercase().as_str() {
            "claude-code" => Self::ClaudeCode,
            "codex" => Self::Codex,
            "opencode" => Self::OpenCode,
            "geminicli" => Self::GeminiCli,
            _ => return None,
        })
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::GeminiCli => "geminicli",
        }
    }

    /// 設定視窗與 pane 標題顯示的名字（舊版 `DisplayName`）。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "ClaudeCode",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::GeminiCli => "GeminiCLI",
        }
    }

    /// 執行檔名含這個字就算同一種（使用者換過圖示或手動新增的自訂連線）。
    fn exe_word(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::GeminiCli => "gemini",
        }
    }
}

/// `backend` 字串 → 顯示名稱（認不出來就原樣回傳，同舊版 `BackendName`）。
pub fn display_name_of(key: &str) -> String {
    Backend::by_key(key)
        .map(|b| b.display_name().to_string())
        .unwrap_or_else(|| key.to_string())
}

/// 這家 CLI 的啟動方式。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    /// 附加在自訂連線參數後面（**前面自帶一個空白**，同舊版 `ExtraArgs`）。
    pub extra_args: String,
    /// 非 `None`＝這家 CLI 沒有可靠的「每 session 系統提示」管道，
    /// 第一次閒置時由 AwayTerminal 打這一句（保底注入）。
    pub first_message: Option<String>,
}

/// **只給 `--verify` 用**的執行檔覆寫（假 agent 的路徑）。
///
/// 為什麼要它：`--verify` 必須跑完整條路（建團隊 → 啟動 → 投遞），但**絕不可以啟動真的
/// claude／codex**，也**絕不可以動使用者的自訂連線清單**（那是「測試不碰使用者正在用的東西」
/// 那條規則）。所以覆寫只活在記憶體裡，`agent_verify_end` 會清掉，永遠不進 settings.json。
static VERIFY_EXE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// 設定／清掉覆寫（`None`＝清掉）。
pub fn set_verify_exe(path: Option<String>) {
    if let Ok(mut g) = VERIFY_EXE.lock() {
        *g = path;
    }
}

fn verify_exe() -> Option<String> {
    VERIFY_EXE.lock().ok().and_then(|g| g.clone())
}

/// 要啟動哪一支：**優先沿用使用者「自訂連線」清單裡同圖示（或同執行檔名）的那筆**
/// （路徑／參數／PowerShell／關閉鍵都照使用者設的），沒有才自動偵測。
/// 找不到＝`None`（設定視窗不列這一家）。
pub fn resolve(settings: &SettingsStore, backend: Backend) -> Option<CustomConn> {
    // `--verify`：一律用假 agent（不看設定、不找真的 CLI）
    if let Some(path) = verify_exe() {
        return Some(CustomConn {
            name: format!("verify-{}", backend.key()),
            path,
            args: String::new(),
            icon: backend.key().to_string(),
            ..CustomConn::default()
        });
    }
    let all = settings.get().custom_conns;
    let mut conns: Vec<&CustomConn> = all.iter().filter(|c| !c.path.trim().is_empty()).collect();
    // 沒隱藏的優先（舊版 `OrderBy(c => c.Hidden)`）
    conns.sort_by_key(|c| c.hidden);

    let usable = |c: &CustomConn| c.via_powershell || std::path::Path::new(&c.path).is_file();
    let hit = conns
        .iter()
        .find(|c| c.icon.eq_ignore_ascii_case(backend.key()) && usable(c))
        .or_else(|| {
            conns.iter().find(|c| {
                !ALL_KEYS.iter().any(|k| k.eq_ignore_ascii_case(&c.icon))
                    && exe_matches(&c.path, backend.exe_word())
                    && usable(c)
            })
        });
    if let Some(hit) = hit {
        return Some(CustomConn {
            name: hit.name.clone(),
            path: hit.path.clone(),
            args: hit.args.clone(),
            icon: backend.key().to_string(),
            close_key: hit.close_key.clone(),
            close_count: hit.close_count,
            via_powershell: hit.via_powershell,
            pick_dir: false,
            hidden: hit.hidden,
            sandbox: hit.sandbox,
        });
    }

    // 自動偵測（和「自訂… → 自動偵測」同一張表：預設參數也一樣，
    // 例 ClaudeCode 的 --dangerously-skip-permissions）
    let tool = crate::custom::KNOWN_TOOLS
        .iter()
        .find(|t| t.icon.eq_ignore_ascii_case(backend.key()))?;
    let path = crate::custom::resolve_tool(tool.exe_names)?;
    let path = path.to_string_lossy().to_string();
    let lower = path.to_ascii_lowercase();
    Some(CustomConn {
        name: tool.name.to_string(),
        path,
        args: tool.args.to_string(),
        icon: backend.key().to_string(),
        via_powershell: lower.ends_with(".cmd") || lower.ends_with(".bat"),
        ..CustomConn::default()
    })
}

/// 這條自訂連線是哪一家 AI CLI（[`resolve`] 的反方向）：圖示就是那一家的，或執行檔名含
/// 那一家的字。都不是（WSL、ADB、使用者自己的工具）回 `None`。
pub fn backend_of(conn: &CustomConn) -> Option<Backend> {
    if let Some(b) = Backend::by_key(&conn.icon) {
        return Some(b);
    }
    ALL_KEYS
        .iter()
        .filter_map(|k| Backend::by_key(k))
        .find(|b| exe_matches(&conn.path, b.exe_word()))
}

fn exe_matches(path: &str, word: &str) -> bool {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase().contains(word))
        .unwrap_or(false)
}

/// 舊版 `RunsViaPowerShell`：`.cmd`／`.bat` 也算（cmd.exe 的命令列上限是 8191）。
fn runs_via_powershell(conn: &CustomConn) -> bool {
    let lower = conn.path.to_ascii_lowercase();
    conn.via_powershell || lower.ends_with(".cmd") || lower.ends_with(".bat")
}

/// 角色指引（英文、單行、不含引號與 PowerShell／cmd 特殊字元——要能安全地放進命令列，
/// 也要能直接打進 TUI）。舊版 `CliAdapterBase.Pointer`，**逐字**。
pub fn pointer(agent_id: &str, role_title: &str, role_file: &str) -> String {
    format!(
        "You are {agent_id} ({role_title}) in an AwayTerminal Multi-Agent team. \
Before doing anything else, read the file {role_file} completely \
(it is UTF-8; in Windows PowerShell use Get-Content -Raw -Encoding UTF8). \
It defines your role, your teammates and how to send and receive messages. Follow it for the whole session. \
Your teammates are separate terminals, not sub-agents: never use spawn_agent or any other sub-agent tool to reach them."
    )
}

/// 多行文字壓成一行（TOML literal string 不能換行、不能含單引號；命令列不能含雙引號）。
/// 舊版 `OneLine`：引號換成**全形**的，不是刪掉。
fn one_line(s: &str) -> String {
    s.replace('\r', "")
        .replace('\n', " ")
        .replace('\'', "\u{2019}")
        .replace('"', "\u{201d}")
        .trim()
        .to_string()
}

/// 組這一格的啟動方式。`role_text`＝組好的角色檔內容（Codex 會把它塞進命令列）。
pub fn build_launch(
    backend: Backend,
    conn: &CustomConn,
    agent_id: &str,
    role_title: &str,
    role_file: &str,
    role_text: &str,
) -> Launch {
    match backend {
        Backend::ClaudeCode => Launch {
            extra_args: format!(" --append-system-prompt-file \"{role_file}\""),
            first_message: None,
        },
        Backend::Codex => {
            let full = one_line(role_text);
            // 整份角色檔太長，或經 PowerShell 啟動（npm 版 codex.cmd）→ 改帶一句「先讀角色檔」
            // 的短指引。仍是原生 developer instructions，不必等 CLI 閒置再打字
            //（Codex 第一次進資料夾會先問要不要信任，那時打字會打進信任選單）。
            let text = if !runs_via_powershell(conn) && full.chars().count() <= MAX_INLINE {
                full
            } else {
                one_line(&pointer(agent_id, role_title, role_file))
            };
            // tui.whimsy=false：舊版 probe 實錄——gpt-6-astra 閒置時輸入框背景有「星星閃爍」
            // 動畫，每秒重畫 6～7 次、約 8KB/s → 畫面永遠不會靜止 2 秒，`agent_ready` 永遠 false、
            // 信一直卡在佇列。只關裝飾動畫；`tui.animations=false` 可能連「Working」這類忙碌
            // 指示一起關，忙閒判斷會失準，所以不用它。
            Launch {
                extra_args: format!(" -c tui.whimsy=false -c \"developer_instructions='{text}'\""),
                first_message: None,
            }
        }
        Backend::OpenCode => {
            // 代理團隊裡一律帶 --auto（舊版使用者要求 2026-09-15）：團隊裡的 agent 一跳權限
            // 詢問就停在那等人按，後面的信也送不出去。連線參數裡已經有就不重複加。
            let has_auto = conn
                .args
                .split_whitespace()
                .any(|w| w.eq_ignore_ascii_case("--auto"));
            Launch {
                extra_args: if has_auto { String::new() } else { " --auto".to_string() },
                first_message: Some(format!(
                    "{} After reading it, reply only with READY.",
                    pointer(agent_id, role_title, role_file)
                )),
            }
        }
        Backend::GeminiCli => Launch {
            extra_args: String::new(),
            first_message: Some(format!(
                "{} After reading it, reply only with READY.",
                pointer(agent_id, role_title, role_file)
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(path: &str, args: &str, via_ps: bool) -> CustomConn {
        CustomConn {
            name: "x".to_string(),
            path: path.to_string(),
            args: args.to_string(),
            via_powershell: via_ps,
            ..CustomConn::default()
        }
    }

    #[test]
    fn maps_keys_and_names() {
        assert_eq!(Backend::by_key("CLAUDE-CODE"), Some(Backend::ClaudeCode));
        assert_eq!(Backend::by_key("geminicli"), Some(Backend::GeminiCli));
        assert_eq!(Backend::by_key("aider"), None);
        assert_eq!(display_name_of("codex"), "Codex");
        assert_eq!(display_name_of("nope"), "nope", "認不出來就原樣回傳");
    }

    /// 自訂連線 → 是哪一家 AI CLI：圖示優先，其次看執行檔名；WSL／ADB 這類不是。
    #[test]
    fn recognises_which_cli_a_connection_is() {
        let mut c = conn("C:\\x\\whatever.exe", "", false);
        c.icon = "codex".to_string();
        assert_eq!(backend_of(&c), Some(Backend::Codex), "圖示是 codex");
        assert_eq!(backend_of(&conn("C:\\npm\\opencode.cmd", "", true)), Some(Backend::OpenCode));
        assert_eq!(backend_of(&conn("C:\\bin\\claude.exe", "", false)), Some(Backend::ClaudeCode));
        assert_eq!(backend_of(&conn("/usr/bin/gemini", "", false)), Some(Backend::GeminiCli));
        assert_eq!(backend_of(&conn("C:\\Windows\\System32\\wsl.exe", "", false)), None);
        assert_eq!(backend_of(&conn("C:\\adb\\adb.exe", "shell", false)), None);
    }

    /// Claude Code：角色走旗標，不用打字。
    #[test]
    fn claude_injects_the_role_file_as_a_flag() {
        let l = build_launch(
            Backend::ClaudeCode,
            &conn("C:\\c\\claude.exe", "", false),
            "Agent-12",
            "Software Engineer",
            "C:\\d\\Agent-12.md",
            "roletext",
        );
        assert_eq!(
            l.extra_args,
            " --append-system-prompt-file \"C:\\d\\Agent-12.md\""
        );
        assert_eq!(l.first_message, None);
    }

    /// Codex：短的角色檔整份內嵌；`tui.whimsy=false` 一定在。
    #[test]
    fn codex_inlines_short_role_text() {
        let l = build_launch(
            Backend::Codex,
            &conn("C:\\c\\codex.exe", "", false),
            "Agent-12",
            "Software Engineer",
            "C:\\d\\Agent-12.md",
            "line one\nline 'two' and \"three\"",
        );
        assert!(l.extra_args.starts_with(" -c tui.whimsy=false -c \"developer_instructions='"));
        assert!(l.extra_args.contains("line one line \u{2019}two\u{2019} and \u{201d}three\u{201d}"));
        assert!(!l.extra_args.contains('\n'));
        assert_eq!(l.first_message, None);
    }

    /// Codex：太長 → 改帶短指引（仍是啟動參數，不是打字）。
    #[test]
    fn codex_falls_back_to_a_pointer_when_too_long() {
        let long = "x".repeat(MAX_INLINE + 1);
        let l = build_launch(
            Backend::Codex,
            &conn("C:\\c\\codex.exe", "", false),
            "Agent-12",
            "Software Engineer",
            "C:\\d\\Agent-12.md",
            &long,
        );
        assert!(l.extra_args.contains("Before doing anything else, read the file C:\\d\\Agent-12.md"));
        assert!(!l.extra_args.contains(&long));
    }

    /// Codex：經 PowerShell 啟動（.cmd）→ 不管長短都用短指引（cmd.exe 命令列上限）。
    #[test]
    fn codex_uses_a_pointer_via_powershell() {
        let l = build_launch(
            Backend::Codex,
            &conn("C:\\npm\\codex.cmd", "", false),
            "Agent-12",
            "Software Engineer",
            "C:\\d\\Agent-12.md",
            "short",
        );
        assert!(l.extra_args.contains("read the file C:\\d\\Agent-12.md"));
        assert!(!l.extra_args.contains("'short'"));
    }

    /// OpenCode：補 `--auto`（已經有就不重複），角色靠第一句打進去。
    #[test]
    fn opencode_adds_auto_once() {
        let l = build_launch(
            Backend::OpenCode,
            &conn("C:\\o\\opencode.exe", "", false),
            "Agent-13",
            "QA Engineer",
            "C:\\d\\Agent-13.md",
            "",
        );
        assert_eq!(l.extra_args, " --auto");
        let msg = l.first_message.unwrap();
        assert!(msg.starts_with("You are Agent-13 (QA Engineer) in an AwayTerminal Multi-Agent team."));
        assert!(msg.ends_with("After reading it, reply only with READY."));

        let l2 = build_launch(
            Backend::OpenCode,
            &conn("C:\\o\\opencode.exe", "--auto --foo", false),
            "Agent-13",
            "QA Engineer",
            "C:\\d\\Agent-13.md",
            "",
        );
        assert_eq!(l2.extra_args, "");
    }

    /// Gemini：完全不加參數，只靠第一句。
    #[test]
    fn gemini_only_types_the_first_message() {
        let l = build_launch(
            Backend::GeminiCli,
            &conn("C:\\g\\gemini.cmd", "", false),
            "Agent-14",
            "None",
            "C:\\d\\Agent-14.md",
            "",
        );
        assert_eq!(l.extra_args, "");
        assert!(l.first_message.is_some());
    }

    /// 打進 TUI 的指引不能含引號與換行（命令列與 TOML 都要安全）。
    #[test]
    fn the_pointer_is_shell_safe() {
        let p = pointer("Agent-12", "Software Engineer", "C:\\d\\Agent-12.md");
        assert!(!p.contains('\n') && !p.contains('"') && !p.contains('\''));
    }
}
