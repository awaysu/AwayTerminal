//! 各家 AI CLI「現在有哪些模型可以選」＋啟動時怎麼指定模型（2.0.2 新增，舊版沒有）。
//!
//! 使用者的流程（2026-10-02 定案）：選完目錄 → **先問 CLI 目前有哪些模型** → 讓使用者選 →
//! 用 `--model` 啟動。恢復分頁與我的最愛記住選的模型；原本的模型**不在清單裡**時才再問一次。
//!
//! | CLI | 清單怎麼來 | 可信度 |
//! |---|---|---|
//! | Codex | `codex debug models`（JSON；只列 `visibility = "list"` 的）。失敗就讀 `~/.codex/models_cache.json` | 權威 |
//! | OpenCode | `opencode models`（一行一個 `供應商/模型`） | 權威 |
//! | Claude Code | **沒有列清單的指令** → 內建別名（`--help` 寫的那幾個） | 不權威 |
//! | Gemini CLI | 沒有列清單的指令 → 只能自己輸入 | 不權威 |
//!
//! 「權威」＝清單是 CLI 自己回報的，所以「上次用的模型不在裡面」是真的不見了，要請使用者重選。
//! 不權威的清單只是方便挑，使用者自己打的名稱不在裡面很正常，**不會**被當成不見了。
//!
//! 四家指定模型的參數剛好都是 `--model <名稱>`。使用者在自訂連線的「參數」欄自己寫了
//! `--model`／`-m` 時，選了模型就把它拿掉再接上新的（Codex 重複給會直接報錯）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::State;

use super::adapters::{self, Backend};
use crate::settings::{CustomConn, SettingsStore};

/// 問 CLI 要清單最多等多久（`opencode models` 實測約 2 秒）。
const LIST_TIMEOUT: Duration = Duration::from_secs(10);
/// 清單快取多久（同一次設定視窗裡換來換去不要每次都重跑 CLI）。
const CACHE_TTL: Duration = Duration::from_secs(600);
/// 模型名稱的長度上限（防呆）。
const MAX_MODEL_LEN: usize = 120;

/// Claude Code 的模型別名（`claude --help`：「an alias for the latest model (e.g. 'fable',
/// 'opus', or 'sonnet') or a model's full name」）。
const CLAUDE_ALIASES: &[&str] = &["fable", "opus", "sonnet", "haiku"];

/// 一個可選的模型。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// 傳給 `--model` 的字。
    pub id: String,
    /// 顯示用（Codex 有 `display_name`；其餘和 id 一樣）。
    pub label: String,
}

/// 一家 CLI 的模型清單。
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelList {
    /// `claude-code`／`codex`／`opencode`／`geminicli`。
    pub backend: String,
    pub backend_name: String,
    pub models: Vec<ModelInfo>,
    /// `cli`（CLI 自己回報的）／`cache`（Codex 的快取檔）／`builtin`（內建別名）／`none`。
    pub source: String,
    /// 清單是 CLI 自己回報的（「不在清單裡」才有意義）。
    pub authoritative: bool,
    /// `cli`＝跑了哪個指令；失敗＝原因（顯示在提示列）。
    pub note: String,
    /// 這家 CLI 上次選的模型（空＝預設）。
    pub last: String,
}

/// 模型名稱可以直接接在命令列上嗎：只收英數與 `. _ - / : @ [ ]`，不能以 `-` 開頭。
///
/// 名稱會原樣接在啟動命令列後面（經 PowerShell 啟動時還會過一層 shell），
/// 所以空白、引號、`;`、`|`、`&` 這些一律不收。
pub fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= MAX_MODEL_LEN
        && !model.starts_with('-')
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':' | '@' | '[' | ']'))
}

/// 接在啟動參數後面的那一段（**前面自帶一個空白**；空的或不合法的名稱＝不加）。
pub fn model_arg(model: &str) -> String {
    let m = model.trim();
    if valid_model(m) {
        format!(" --model {m}")
    } else {
        String::new()
    }
}

/// 把參數字串裡既有的 `--model X`／`--model=X`／`-m X`／`-m=X` 拿掉。
///
/// 只在使用者**選了模型**時才呼叫：選了「預設」就原樣保留他自己寫的。
pub fn strip_model_arg(args: &str) -> String {
    let tokens = split_args(args);
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if t == "--model" || t == "-m" {
            i += 2; // 連後面的值一起拿掉
            continue;
        }
        if t.starts_with("--model=") || t.starts_with("-m=") {
            i += 1;
            continue;
        }
        out.push(t);
        i += 1;
    }
    out.join(" ")
}

/// 以空白切參數，雙引號裡的空白不切（引號留在 token 裡，接回去時原樣）。
fn split_args(args: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut quoted = false;
    for (i, c) in args.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if c.is_whitespace() && !quoted {
            if let Some(s) = start.take() {
                out.push(&args[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&args[s..]);
    }
    out
}

/// `codex debug models` 的輸出（也是 `models_cache.json` 的格式）：
/// `{"models":[{"slug":"gpt-…","display_name":"GPT-…","visibility":"list"}, …]}`。
pub fn parse_codex(json: &str) -> Vec<ModelInfo> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(list) = v.get("models").and_then(|m| m.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter(|m| m.get("visibility").and_then(|x| x.as_str()).unwrap_or("list") == "list")
        .filter_map(|m| {
            let id = m.get("slug")?.as_str()?.trim();
            if !valid_model(id) {
                return None;
            }
            let label = m
                .get("display_name")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(id);
            Some(ModelInfo {
                id: id.to_string(),
                label: label.to_string(),
            })
        })
        .collect()
}

/// `opencode models` 的輸出：一行一個 `供應商/模型`，其餘的行（警告、空行）跳過。
pub fn parse_opencode(text: &str) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = Vec::new();
    for line in text.lines() {
        let id = line.trim();
        if !id.contains('/') || !valid_model(id) || out.iter().any(|m| m.id == id) {
            continue;
        }
        out.push(ModelInfo {
            id: id.to_string(),
            label: id.to_string(),
        });
    }
    out
}

/// 跑一個指令、收它的 stdout（阻塞；有逾時）。
///
/// stdout 另開一條執行緒讀：`codex debug models` 的 JSON 有幾百 KB，等行程結束再讀的話
/// pipe 會先塞滿、行程卡在寫出，永遠等不到結束。逾時只砍**自己開的這個子行程**。
fn run(exe: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW); // 不要閃一個黑窗
    }
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let reader = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = Vec::new();
            let _ = out.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).into_owned()
        })
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Ok(st),
            Ok(None) if start.elapsed() < LIST_TIMEOUT => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("timeout after {}s", LIST_TIMEOUT.as_secs()));
            }
            Err(e) => break Err(e.to_string()),
        }
    };
    let text = reader.and_then(|h| h.join().ok()).unwrap_or_default();
    let status = status?;
    if !status.success() && text.trim().is_empty() {
        return Err(format!("exit code {}", status.code().unwrap_or(-1)));
    }
    Ok(text)
}

/// Codex 自己的模型快取（`$CODEX_HOME/models_cache.json`，預設 `~/.codex`）。
fn codex_cache_path() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("CODEX_HOME") {
        if !home.trim().is_empty() {
            return Some(PathBuf::from(home).join("models_cache.json"));
        }
    }
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok()?;
    Some(PathBuf::from(home).join(".codex").join("models_cache.json"))
}

/// 問這家 CLI 目前有哪些模型（阻塞，最多 [`LIST_TIMEOUT`]）。不看快取。
fn fetch(backend: Backend, conn: &CustomConn) -> ModelList {
    let exe = Path::new(&conn.path);
    let exe_name = exe
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut list = ModelList {
        backend: backend.key().to_string(),
        backend_name: backend.display_name().to_string(),
        source: "none".to_string(),
        ..ModelList::default()
    };
    match backend {
        Backend::Codex => match run(exe, &["debug", "models"]).map(|t| parse_codex(&t)) {
            Ok(models) if !models.is_empty() => {
                list.models = models;
                list.source = "cli".to_string();
                list.authoritative = true;
                list.note = format!("{exe_name} debug models");
            }
            other => {
                let why = match other {
                    Err(e) => e,
                    Ok(_) => "empty list".to_string(),
                };
                // 指令跑不起來（舊版 Codex 沒有 `debug models`）→ 退回它自己的快取檔
                let cached = codex_cache_path()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .map(|t| parse_codex(&t))
                    .unwrap_or_default();
                if cached.is_empty() {
                    list.note = why;
                } else {
                    list.models = cached;
                    list.source = "cache".to_string();
                    list.authoritative = true;
                    list.note = "models_cache.json".to_string();
                }
            }
        },
        Backend::OpenCode => match run(exe, &["models"]).map(|t| parse_opencode(&t)) {
            Ok(models) if !models.is_empty() => {
                list.models = models;
                list.source = "cli".to_string();
                list.authoritative = true;
                list.note = format!("{exe_name} models");
            }
            Ok(_) => list.note = "empty list".to_string(),
            Err(e) => list.note = e,
        },
        Backend::ClaudeCode => {
            list.models = CLAUDE_ALIASES
                .iter()
                .map(|a| ModelInfo {
                    id: a.to_string(),
                    label: a.to_string(),
                })
                .collect();
            list.source = "builtin".to_string();
        }
        Backend::GeminiCli => {}
    }
    list
}

type Cache = Mutex<HashMap<String, (Instant, ModelList)>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 這家 CLI 的模型清單（有快取；`refresh`＝不看快取）。`last` 每次都重新填。
pub fn list_for(settings: &SettingsStore, backend: Backend, conn: &CustomConn, refresh: bool) -> ModelList {
    let key = format!("{}|{}", backend.key(), conn.path.to_ascii_lowercase());
    let cached = if refresh {
        None
    } else {
        cache()
            .lock()
            .ok()
            .and_then(|c| c.get(&key).cloned())
            .filter(|(at, _)| at.elapsed() < CACHE_TTL)
            .map(|(_, l)| l)
    };
    let mut list = match cached {
        Some(l) => l,
        None => {
            let l = fetch(backend, conn);
            println!(
                "[AwayTerminal] 模型清單 {}：{} 個（來源={}{}）",
                backend.display_name(),
                l.models.len(),
                l.source,
                if l.note.is_empty() { String::new() } else { format!("，{}", l.note) }
            );
            // 失敗的結果不快取：使用者修好（例如重新登入）之後下一次就要拿得到
            if l.source != "none" || backend == Backend::GeminiCli {
                if let Ok(mut c) = cache().lock() {
                    c.insert(key, (Instant::now(), l.clone()));
                }
            }
            l
        }
    };
    list.last = settings
        .get()
        .last_models
        .get(backend.key())
        .cloned()
        .unwrap_or_default();
    list
}

/// 代理團隊／AI 聊天室的設定視窗：這幾家 CLI 各有哪些模型（平行問，整體最多等一個逾時）。
#[tauri::command]
pub async fn cli_models(
    backends: Vec<String>,
    refresh: Option<bool>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<Vec<ModelList>, String> {
    let settings = settings.inner().clone();
    let refresh = refresh.unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        let jobs: Vec<(Backend, CustomConn)> = backends
            .iter()
            .filter_map(|k| Backend::by_key(k))
            .filter_map(|b| adapters::resolve(&settings, b).map(|c| (b, c)))
            .collect();
        std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .iter()
                .map(|(b, c)| {
                    let settings = &settings;
                    scope.spawn(move || list_for(settings, *b, c, refresh))
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        })
    })
    .await
    .map_err(|e| e.to_string())
}

/// 單一自訂連線：這條連線是哪一家 AI CLI、有哪些模型。不是 AI CLI（WSL、ADB…）回 `None`
///（＝不用問模型，直接開）。
#[tauri::command]
pub async fn conn_models(
    name: String,
    refresh: Option<bool>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<Option<ModelList>, String> {
    let settings = settings.inner().clone();
    let refresh = refresh.unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        let conn = crate::custom::find(&settings, &name)?;
        let backend = adapters::backend_of(&conn)?;
        Some(list_for(&settings, backend, &conn, refresh))
    })
    .await
    .map_err(|e| e.to_string())
}

/// 記住這家 CLI 這次選的模型（下次開的時候預選它）。空字串＝選了「預設」。
#[tauri::command]
pub fn model_remember(backend: String, model: String, settings: State<'_, Arc<SettingsStore>>) {
    let Some(b) = Backend::by_key(&backend) else { return };
    let model = model.trim().to_string();
    if !model.is_empty() && !valid_model(&model) {
        return;
    }
    if settings.get().last_models.get(b.key()).map(String::as_str) == Some(model.as_str()) {
        return;
    }
    settings.update(|s| {
        s.last_models.insert(b.key().to_string(), model.clone());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_shell_safe_model_names_are_accepted() {
        for ok in ["opus", "gpt-6.1-sol", "anthropic/claude-sonnet-5-5", "claude-opus-5-5[1m]", "us.anthropic.claude:0", "a@b"] {
            assert!(valid_model(ok), "{ok} 應該合法");
        }
        for bad in ["", "a b", "a;b", "a\"b", "a'b", "a|b", "a&b", "$(x)", "`x`", "-m", "--model", "a\nb"] {
            assert!(!valid_model(bad), "{bad:?} 不應該合法");
        }
        assert!(!valid_model(&"x".repeat(MAX_MODEL_LEN + 1)));
    }

    #[test]
    fn model_arg_is_empty_for_default_or_bad_names() {
        assert_eq!(model_arg("opus"), " --model opus");
        assert_eq!(model_arg("  gpt-6-sol "), " --model gpt-6-sol");
        assert_eq!(model_arg(""), "", "預設＝不加參數");
        assert_eq!(model_arg("a; rm -rf /"), "", "不合法的名稱不可以進命令列");
    }

    /// 使用者自己在「參數」欄寫的 `--model` 要拿掉（Codex 重複給會報錯），其餘原樣。
    #[test]
    fn strips_an_existing_model_flag() {
        assert_eq!(strip_model_arg("--dangerously-skip-permissions --model opus"), "--dangerously-skip-permissions");
        assert_eq!(strip_model_arg("-m gpt-6-sol --auto"), "--auto");
        assert_eq!(strip_model_arg("--model=opus --auto"), "--auto");
        assert_eq!(strip_model_arg("--auto -m=x"), "--auto");
        assert_eq!(strip_model_arg("--auto"), "--auto");
        assert_eq!(strip_model_arg(""), "");
        // 引號裡的空白不切、也不會被誤認成 --model
        assert_eq!(
            strip_model_arg("-c \"a --model b\" --model opus"),
            "-c \"a --model b\""
        );
        // 旗標在最後、後面沒有值
        assert_eq!(strip_model_arg("--auto --model"), "--auto");
    }

    #[test]
    fn parses_the_codex_catalog() {
        let json = r#"{"fetched_at":"x","models":[
            {"slug":"gpt-6-sol","display_name":"GPT-6-Sol","visibility":"list"},
            {"slug":"gpt-reserve","display_name":"GPT-Reserve","visibility":"hide"},
            {"slug":"gpt-5.5","visibility":"list"},
            {"slug":"bad name","display_name":"x","visibility":"list"}
        ]}"#;
        let m = parse_codex(json);
        assert_eq!(m.len(), 2, "hide 的與名稱不合法的不列");
        assert_eq!(m[0], ModelInfo { id: "gpt-6-sol".into(), label: "GPT-6-Sol".into() });
        assert_eq!(m[1].label, "gpt-5.5", "沒有 display_name 就用 slug");
        assert!(parse_codex("not json").is_empty());
        assert!(parse_codex("{}").is_empty());
    }

    /// **手動**（`cargo test -- --ignored real_cli`）：真的去問這台機器上裝的 CLI。
    /// 用環境變數給路徑：`AT_CODEX`、`AT_OPENCODE`（沒給的那一家跳過）。
    /// 重點是驗 [`run`]：`.cmd` 起得來、幾百 KB 的輸出不會卡住、不會跳黑窗。
    #[test]
    #[ignore]
    fn real_cli_lists_models() {
        for (var, backend) in [("AT_CODEX", Backend::Codex), ("AT_OPENCODE", Backend::OpenCode)] {
            let Ok(path) = std::env::var(var) else { continue };
            let conn = CustomConn { path, ..CustomConn::default() };
            let started = Instant::now();
            let list = fetch(backend, &conn);
            println!(
                "{}: {} 個（來源={}，{}，{} ms）{:?}",
                backend.display_name(),
                list.models.len(),
                list.source,
                list.note,
                started.elapsed().as_millis(),
                list.models.iter().take(4).map(|m| m.id.as_str()).collect::<Vec<_>>()
            );
            assert_eq!(list.source, "cli", "{}", list.note);
            assert!(list.authoritative && !list.models.is_empty());
        }
    }

    #[test]
    fn parses_the_opencode_list() {
        let text = "opencode/big-pickle\r\nanthropic/claude-sonnet-5-5\n\nWARN something happened\nopencode/big-pickle\n";
        let m = parse_opencode(text);
        assert_eq!(
            m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            ["opencode/big-pickle", "anthropic/claude-sonnet-5-5"],
            "不是 供應商/模型 的行跳過、重複的只留一個"
        );
    }
}
