//! AI CLI 的額度顯示（工具列右上角；2.0.11 新增，舊版沒有）。
//!
//! 使用者 2026-10-06 定的格式（一家一行，沒有資料的那家不顯示）：
//!
//! ```text
//! Claude:5h 41%|7d 12%|reset 1h26m
//! Codex:5h 22%|7d 38%|reset 3h46m
//! ```
//!
//! | CLI | 資料從哪裡來 |
//! |---|---|
//! | Codex | 它自己的 session 紀錄（`$CODEX_HOME/sessions/YYYY/MM/DD/*.jsonl`）：`token_count` 事件的 `rate_limits`（primary＝5 小時、secondary＝7 天，看 `window_minutes`），`turn_context` 的 `model`／`effort`；顯示名稱查 `models_cache.json`。**只讀檔，不動 Codex** |
//! | Claude Code | Claude Code 每次更新狀態列時把 `rate_limits`／`model` 交給狀態列指令。AwayTerminal 開 Claude Code 時多帶 `--settings <檔>`，把狀態列指令換成**自己的 exe**（[`STATUSLINE_ARG`]）：記下數字，再把同一份輸入交給使用者原本的狀態列指令（畫面不變） |
//!
//! 兩家都是「CLI 最近一次回報的數字」：CLI 沒在用的時候不會變，所以帶 `updatedAt` 讓前端標示多久以前。
//! Claude 只有從 AwayTerminal 開的 Claude Code 才收得到（別的終端機開的不會經過這裡）。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// 以這個參數啟動 AwayTerminal＝當 Claude Code 的狀態列指令（`main.rs` 在建立視窗前就分流）。
pub const STATUSLINE_ARG: &str = "--claude-statusline";
/// 狀態列指令要把資料寫到哪個資料夾（AwayTerminal 開 Claude Code 時放進環境變數）。
pub const ENV_DIR: &str = "AWAYTERM_QUOTA_DIR";
/// 使用者原本的狀態列指令（空＝沒有，什麼都不印）。
pub const ENV_CHAIN: &str = "AWAYTERM_SL_CHAIN";

/// 一個額度區間（5 小時或 7 天）。
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// 已用百分比（0～100）。
    pub used_pct: f64,
    /// 重置時間（Unix 秒；0＝不知道）。
    pub resets_at: i64,
}

/// 一家 CLI 的額度。
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Quota {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    /// 顯示用的模型名稱（`Opus 5.5`、`GPT-6.1-Sol`）。
    pub model: String,
    /// 推理強度（Codex 的 `medium`；Claude 沒有）。
    pub effort: String,
    /// 額度數字是什麼時候回報的（Unix 秒）。
    pub updated_at: i64,
}

/// `quota_get` 的回覆：沒有資料的那家是 `None`。
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaReport {
    pub claude: Option<Quota>,
    pub codex: Option<Quota>,
    /// 現在時間（Unix 秒）：前端用它算倒數，不必相信 webview 的時鐘和這邊一致。
    pub now: i64,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
}

/// 額度資料的資料夾（設定資料夾底下的 `quota`）。
pub fn quota_dir(settings_dir: &Path) -> PathBuf {
    settings_dir.join("quota")
}

// ---------------------------------------------------------------- Claude Code

/// Claude Code 交給狀態列的 JSON → 這次要記的東西。`prev` 是上次記的（這次沒有 `rate_limits`
/// 時沿用它的數字：剛開的 session 還沒打過 API，Claude Code 不會給額度）。
pub fn claude_from_statusline(v: &Value, prev: Option<&Quota>, now: i64) -> Quota {
    let window = |key: &str| -> Option<Window> {
        let w = v.get("rate_limits")?.get(key)?;
        Some(Window {
            used_pct: w.get("used_percentage")?.as_f64()?,
            resets_at: w.get("resets_at").and_then(Value::as_i64).unwrap_or(0),
        })
    };
    let five = window("five_hour");
    let seven = window("seven_day");
    let model = v
        .get("model")
        .and_then(|m| m.get("display_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    let fresh = five.is_some() || seven.is_some();
    let prev = prev.cloned().unwrap_or_default();
    Quota {
        five_hour: if fresh { five } else { prev.five_hour },
        seven_day: if fresh { seven } else { prev.seven_day },
        model: if model.is_empty() { prev.model } else { model },
        effort: String::new(),
        updated_at: if fresh { now } else { prev.updated_at },
    }
}

fn read_quota(path: &Path) -> Option<Quota> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// 先寫暫存檔再改名：讀的人（`quota_get`）不會讀到寫一半的檔。
fn write_quota(path: &Path, q: &Quota) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(q).unwrap_or_default())?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// `AwayTerminal --claude-statusline`：Claude Code 每次更新狀態列都會跑一次（要快）。
///
/// 1. 讀完 stdin（Claude Code 給的 JSON），有額度就記到 `$AWAYTERM_QUOTA_DIR/claude.json`；
/// 2. 有 `$AWAYTERM_SL_CHAIN`（使用者原本的狀態列指令）就把**同一份輸入**交給它，輸出原樣轉印。
///
/// 任何一步失敗都不可以影響 Claude Code：記不了就略過，原本的指令照跑。
pub fn run_statusline() -> i32 {
    let mut input = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut input);
    if let Ok(dir) = std::env::var(ENV_DIR) {
        if let Ok(v) = serde_json::from_slice::<Value>(&input) {
            let dir = PathBuf::from(dir);
            let path = dir.join("claude.json");
            let q = claude_from_statusline(&v, read_quota(&path).as_ref(), now_secs());
            let _ = std::fs::create_dir_all(&dir);
            let _ = write_quota(&path, &q);
        }
    }
    let chain = std::env::var(ENV_CHAIN).unwrap_or_default();
    if chain.trim().is_empty() {
        return 0;
    }
    match run_chain(&chain, &input) {
        Ok((out, code)) => {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(&out);
            let _ = stdout.flush();
            code
        }
        Err(_) => 0,
    }
}

/// 用 Claude Code 跑狀態列的同一種 shell 跑使用者原本的指令。
///
/// Claude Code 在 Windows 上用 Git Bash 跑狀態列（找不到才是 cmd），mac／Linux 用 `sh`。
/// 使用者的指令是照那個 shell 寫的（例如 `~/.claude/x.sh`），所以這裡要用同一種。
fn run_chain(command: &str, input: &[u8]) -> std::io::Result<(Vec<u8>, i32)> {
    use std::process::{Command, Stdio};
    let mut cmd = match chain_shell() {
        Some(bash) => {
            let mut c = Command::new(bash);
            c.arg("-c").arg(command);
            c
        }
        None => {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let mut c = Command::new("cmd.exe");
                c.arg("/D").arg("/S").arg("/C");
                // `/S /C "…"`：整串原樣交給 cmd，不讓 Rust 再加一層引號
                c.raw_arg(format!("\"{command}\""));
                c
            }
            #[cfg(not(windows))]
            {
                let mut c = Command::new("/bin/sh");
                c.arg("-c").arg(command);
                c
            }
        }
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 這個 exe 是 GUI 程式、沒有主控台：不加這個旗標的話每次都會閃一個黑窗
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }
    let out = child.wait_with_output()?;
    Ok((out.stdout, out.status.code().unwrap_or(0)))
}

/// Windows：Claude Code 用的 Git Bash（`CLAUDE_CODE_GIT_BASH_PATH` → 裝 Git 的標準位置 → PATH 上的 bash）。
#[cfg(windows)]
fn chain_shell() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CLAUDE_CODE_GIT_BASH_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    // Git for Windows 的標準位置（全機安裝／只裝給自己）
    let candidates = [
        ("ProgramFiles", "Git\\bin\\bash.exe"),
        ("ProgramW6432", "Git\\bin\\bash.exe"),
        ("LOCALAPPDATA", "Programs\\Git\\bin\\bash.exe"),
    ];
    for (var, rel) in candidates {
        if let Ok(base) = std::env::var(var) {
            let p = PathBuf::from(base).join(rel);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    // PATH 上的 bash.exe，但不要 System32 那個（那是 WSL 的入口，不是 Git Bash）
    crate::pty::shell::which("bash.exe").filter(|p| {
        !p.to_string_lossy()
            .to_ascii_lowercase()
            .contains("\\windows\\system32\\")
    })
}

#[cfg(not(windows))]
fn chain_shell() -> Option<PathBuf> {
    None
}

/// 使用者原本的狀態列設定（`command`, `padding`）：照 Claude Code 的優先順序找
/// （專案的 `settings.local.json` → 專案的 `settings.json` → 使用者的 `settings.json`）。
/// 我們的 `--settings` 比這些都優先，所以要自己找出原本那一個接在後面。
pub fn original_statusline(work_dir: Option<&Path>) -> Option<(String, Option<i64>)> {
    let mut files = Vec::new();
    if let Some(w) = work_dir {
        files.push(w.join(".claude").join("settings.local.json"));
        files.push(w.join(".claude").join("settings.json"));
    }
    let config = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".claude")));
    if let Some(c) = config {
        files.push(c.join("settings.json"));
    }
    files.iter().find_map(|f| {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(f).ok()?).ok()?;
        statusline_of(&v)
    })
}

fn statusline_of(v: &Value) -> Option<(String, Option<i64>)> {
    let sl = v.get("statusLine")?;
    let cmd = sl.get("command")?.as_str()?.trim();
    if cmd.is_empty() || cmd.contains(STATUSLINE_ARG) {
        return None;
    }
    Some((cmd.to_string(), sl.get("padding").and_then(Value::as_i64)))
}

/// 開 Claude Code 時要多加的參數與環境變數。
///
/// 回 `None`＝不接（使用者自己在參數裡寫了 `--settings`，兩份會互相蓋掉，不碰他的）。
pub fn claude_launch(
    settings_dir: &Path,
    user_args: &str,
    work_dir: Option<&Path>,
) -> Option<(String, Vec<(String, String)>)> {
    if user_args.split_whitespace().any(|a| a == "--settings" || a.starts_with("--settings=")) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let dir = quota_dir(settings_dir);
    std::fs::create_dir_all(&dir).ok()?;
    let original = original_statusline(work_dir);
    // 狀態列指令用正斜線：Git Bash 與 cmd 都認得，反斜線在 bash 的雙引號裡可能被吃掉
    let exe_str = exe.to_string_lossy().replace('\\', "/");
    let settings = serde_json::json!({
        "statusLine": {
            "type": "command",
            "command": format!("\"{exe_str}\" {STATUSLINE_ARG}"),
            "padding": original.as_ref().and_then(|o| o.1).unwrap_or(0),
        }
    });
    let file = dir.join("claude-statusline.json");
    std::fs::write(&file, serde_json::to_vec_pretty(&settings).ok()?).ok()?;
    let env = vec![
        (ENV_DIR.to_string(), dir.to_string_lossy().to_string()),
        (
            ENV_CHAIN.to_string(),
            original.map(|o| o.0).unwrap_or_default(),
        ),
    ];
    Some((format!(" --settings \"{}\"", file.display()), env))
}

// ---------------------------------------------------------------- Codex

fn codex_home() -> Option<PathBuf> {
    if let Ok(h) = std::env::var("CODEX_HOME") {
        if !h.trim().is_empty() {
            return Some(PathBuf::from(h));
        }
    }
    home_dir().map(|h| h.join(".codex"))
}

/// 名稱是數字的子資料夾，由大到小（`sessions/2026/10/06` 那種）。
fn numbered_dirs_desc(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| Some((e.file_name().to_str()?.parse().ok()?, e.path())))
        .collect();
    v.sort_by_key(|&(n, _)| std::cmp::Reverse(n));
    v.into_iter().map(|(_, p)| p).collect()
}

/// 最近有寫入的 session 紀錄：看最新的兩天（跨夜的長 session 在前一天的資料夾裡），取修改時間最新的。
fn latest_codex_session(sessions: &Path) -> Option<PathBuf> {
    let mut days = Vec::new();
    'outer: for y in numbered_dirs_desc(sessions) {
        for m in numbered_dirs_desc(&y) {
            for d in numbered_dirs_desc(&m) {
                days.push(d);
                if days.len() >= 2 {
                    break 'outer;
                }
            }
        }
    }
    days.iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

/// 讀檔案最後 `max` bytes（session 紀錄可能幾十 MB，只需要最後的幾筆）。
fn read_tail(path: &Path, max: u64) -> Option<String> {
    use std::io::{Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    // 從中間開始讀的話第一行是半行 → 丟掉
    Some(if start > 0 {
        text.split_once('\n').map(|(_, r)| r.to_string()).unwrap_or_default()
    } else {
        text
    })
}

/// Codex 的 session 紀錄（尾端幾行）→ 額度與模型。`slug → 顯示名稱` 由呼叫端給。
pub fn parse_codex_session(text: &str, display_name: impl Fn(&str) -> Option<String>) -> Option<Quota> {
    let mut q = Quota::default();
    let mut have_limits = false;
    let mut have_model = false;
    for line in text.lines().rev() {
        if have_limits && have_model {
            break;
        }
        let want_limits = !have_limits && line.contains("\"rate_limits\"");
        let want_model = !have_model && line.contains("\"turn_context\"");
        if !want_limits && !want_model {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(p) = v.get("payload") else { continue };
        if want_limits {
            if let Some(rl) = p.get("rate_limits").filter(|r| r.is_object()) {
                for key in ["primary", "secondary"] {
                    let Some(w) = rl.get(key).filter(|w| w.is_object()) else { continue };
                    let Some(used) = w.get("used_percent").and_then(Value::as_f64) else { continue };
                    let win = Window {
                        used_pct: used,
                        resets_at: w.get("resets_at").and_then(Value::as_i64).unwrap_or(0),
                    };
                    // 看區間長度決定是 5 小時還是 7 天（不要假設 primary 一定是 5 小時）
                    match w.get("window_minutes").and_then(Value::as_i64) {
                        Some(m) if m >= 24 * 60 => q.seven_day = Some(win),
                        Some(_) => q.five_hour = Some(win),
                        None if key == "primary" => q.five_hour = Some(win),
                        None => q.seven_day = Some(win),
                    }
                }
                q.updated_at = v
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_rfc3339)
                    .unwrap_or(0);
                have_limits = true;
            }
        }
        if want_model && v.get("type").and_then(Value::as_str) == Some("turn_context") {
            let slug = p.get("model").and_then(Value::as_str).unwrap_or("").trim();
            if !slug.is_empty() {
                q.model = display_name(slug).unwrap_or_else(|| slug.to_string());
                q.effort = p
                    .get("effort")
                    .or_else(|| p.get("reasoning_effort"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                have_model = true;
            }
        }
    }
    (have_limits || have_model).then_some(q)
}

/// `2026-10-06T04:59:41.391Z` → Unix 秒（只認 UTC 的 `Z` 結尾，Codex 寫的就是這種）。
fn parse_rfc3339(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let time = time.split('.').next()?;
    let mut t = time.split(':').map(|x| x.parse::<i64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next()??);
    // 公曆日期 → 從 1970-01-01 起的天數（Howard Hinnant 的 days_from_civil）
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

fn codex_display_name(home: &Path, slug: &str) -> Option<String> {
    let text = std::fs::read_to_string(home.join("models_cache.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("models")?.as_array()?.iter().find_map(|m| {
        (m.get("slug")?.as_str()? == slug)
            .then(|| m.get("display_name")?.as_str().map(str::to_string))
            .flatten()
    })
}

pub fn codex_quota() -> Option<Quota> {
    let home = codex_home()?;
    let file = latest_codex_session(&home.join("sessions"))?;
    let text = read_tail(&file, 2 * 1024 * 1024)?;
    parse_codex_session(&text, |slug| codex_display_name(&home, slug))
}

// ---------------------------------------------------------------- command

/// 右上角的額度顯示（前端定時呼叫）。
#[tauri::command]
pub async fn quota_get(
    settings: tauri::State<'_, std::sync::Arc<crate::settings::SettingsStore>>,
) -> Result<QuotaReport, String> {
    let dir = quota_dir(&settings.dir());
    tokio::task::spawn_blocking(move || QuotaReport {
        claude: read_quota(&dir.join("claude.json")),
        codex: codex_quota(),
        now: now_secs(),
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_statusline_keeps_previous_limits_when_missing() {
        let v: Value = serde_json::from_str(
            r#"{"model":{"id":"claude-opus-5-5","display_name":"Opus 5.5"},
                "rate_limits":{"five_hour":{"used_percentage":41.2,"resets_at":1791269305},
                               "seven_day":{"used_percentage":12,"resets_at":1791580267}}}"#,
        )
        .unwrap();
        let q = claude_from_statusline(&v, None, 100);
        assert_eq!(q.model, "Opus 5.5");
        assert_eq!(q.five_hour.as_ref().unwrap().used_pct, 41.2);
        assert_eq!(q.seven_day.as_ref().unwrap().resets_at, 1_791_580_267);
        assert_eq!(q.updated_at, 100);

        // 新 session 還沒打過 API：沒有 rate_limits → 數字與時間沿用上次，模型換新的
        let v2: Value = serde_json::from_str(r#"{"model":{"display_name":"Sonnet 4.6"}}"#).unwrap();
        let q2 = claude_from_statusline(&v2, Some(&q), 200);
        assert_eq!(q2.model, "Sonnet 4.6");
        assert_eq!(q2.five_hour, q.five_hour);
        assert_eq!(q2.updated_at, 100);
    }

    #[test]
    fn codex_session_tail() {
        let text = concat!(
            r#"{"timestamp":"2026-10-06T03:24:00.302Z","type":"turn_context","payload":{"model":"gpt-6.1-sol","effort":"medium"}}"#,
            "\n",
            r#"{"timestamp":"2026-10-06T04:59:41.391Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":24.0,"window_minutes":300,"resets_at":1791269305},"secondary":{"used_percent":38.0,"window_minutes":10080,"resets_at":1791580267}}}}"#,
            "\n",
            // 最後一筆 token_count 沒有 rate_limits（null）→ 往前找
            r#"{"timestamp":"2026-10-06T05:00:00.000Z","type":"event_msg","payload":{"type":"token_count","rate_limits":null}}"#,
            "\n"
        );
        let q = parse_codex_session(text, |s| (s == "gpt-6.1-sol").then(|| "GPT-6.1-Sol".to_string())).unwrap();
        assert_eq!(q.model, "GPT-6.1-Sol");
        assert_eq!(q.effort, "medium");
        assert_eq!(q.five_hour.unwrap().used_pct, 24.0);
        assert_eq!(q.seven_day.unwrap().used_pct, 38.0);
        assert_eq!(q.updated_at, 1_791_262_781);
    }

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-10-06T04:59:41.391Z"), Some(1_791_262_781));
        assert_eq!(parse_rfc3339("2026-10-06T04:59:41+08:00"), None);
    }

    #[test]
    fn statusline_of_skips_our_own_command() {
        let ours: Value = serde_json::from_str(
            r#"{"statusLine":{"type":"command","command":"\"C:/x/AwayTerminal.exe\" --claude-statusline"}}"#,
        )
        .unwrap();
        assert_eq!(statusline_of(&ours), None, "不可以接回自己（無限遞迴）");
        let user: Value = serde_json::from_str(
            r#"{"statusLine":{"type":"command","command":"powershell -File x.ps1","padding":2}}"#,
        )
        .unwrap();
        assert_eq!(statusline_of(&user), Some(("powershell -File x.ps1".to_string(), Some(2))));
    }
}

#[cfg(test)]
mod live {
    /// 這台機器上的 Codex 實際讀得到什麼（`cargo test --lib quota::live -- --ignored --nocapture`）。
    #[test]
    #[ignore]
    fn codex_on_this_machine() {
        println!("{:?}", super::codex_quota());
        println!("{:?}", super::original_statusline(None));
    }
}
