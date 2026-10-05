//! 各家 AI CLI「現在有哪些模型可以選」＋啟動時怎麼指定模型（2.0.2 新增，舊版沒有）。
//!
//! 使用者的流程（2026-10-02 定案）：選完目錄 → **先問 CLI 目前有哪些模型** → 讓使用者選 →
//! 用 `--model` 啟動。恢復分頁與我的最愛記住選的模型；原本的模型**不在清單裡**時才再問一次。
//!
//! | CLI | 清單怎麼來 | 可信度 |
//! |---|---|---|
//! | Codex | `codex debug models`（JSON；只列 `visibility = "list"` 的）。失敗就讀 `~/.codex/models_cache.json` | 權威 |
//! | OpenCode | `opencode models`（一行一個 `供應商/模型`） | 權威 |
//! | Claude Code | **沒有列清單的指令** → 內建別名（`--help` 寫的那幾個）＋從它的程式本體（`claude.exe`／`cli.js`）找出來的每個版本，`~/.claude.json` 補 `[1m]` 的寫法（2.0.3） | 不權威 |
//! | Gemini CLI | 沒有列清單的指令 → 別名（auto／pro／flash／flash-lite）＋從它的程式本體找出來的每個版本（2.0.3）；找不到程式本體就只能自己輸入 | 不權威 |
//! | Antigravity CLI | 沒有列清單的指令，而且它的清單在伺服器上（程式本體只看得到 `GetCascadeModelConfigs` 這種 RPC 名）→ **內建的靜態清單**（[`ANTIGRAVITY_MODELS`]，從 1.2.16 的程式本體字串整理的；2.0.6） | 不權威 |
//!
//! 「權威」＝清單是 CLI 自己回報的，所以「上次用的模型不在裡面」是真的不見了，要請使用者重選。
//! 不權威的清單只是方便挑，使用者自己打的名稱不在裡面很正常，**不會**被當成不見了。
//!
//! 五家指定模型的參數剛好都是 `--model <名稱>`（Antigravity 的 `-m` 也是 `--model` 的縮寫）。
//! 使用者在自訂連線的「參數」欄自己寫了 `--model`／`-m` 時，選了模型就把它拿掉再接上新的
//! （Codex 重複給會直接報錯）。
//!
//! Antigravity CLI 只做到這一層（使用者 2026-10-05 定的「第 1 層」：自訂連線＋選模型＋沙盒），
//! **代理團隊不能選它**——所以它不是 [`adapters::Backend`]，這裡另外用 [`ModelCli`] 把五家包起來。

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

/// Antigravity CLI 的模型（`(id, 顯示名)`）。
///
/// 它沒有列清單的指令，清單是伺服器給的（`AGY_LLM_GATEWAY_MODELS`／`GetCascadeModelConfigs`），
/// 所以這份是**靜態的**：2026-10-05 從 agy 1.2.16 的 Windows 執行檔字串整理出來的（Go 的字串表
/// 是黏在一起的，沒辦法像 Claude Code 那樣在執行時掃）。它會自己在背景更新，這份清單可能落後；
/// 哪些你的帳號能用也要看它自己的 `/usage`。順序：Gemini 由新到舊，再來 Claude，最後開源模型。
pub const ANTIGRAVITY_MODELS: &[(&str, &str)] = &[
    ("gemini-3.8-flash", "Gemini 3.8 Flash"),
    ("gemini-3.7-flash", "Gemini 3.7 Flash"),
    ("gemini-3.6-flash", "Gemini 3.6 Flash"),
    ("gemini-3.5-flash", "Gemini 3.5 Flash"),
    ("gemini-3.1-pro", "Gemini 3.1 Pro"),
    ("claude-opus-5-5", "Claude Opus 5.5"),
    ("claude-opus-4-8", "Claude Opus 4.8"),
    ("claude-opus-4-6", "Claude Opus 4.6"),
    ("claude-sonnet-4-6", "Claude Sonnet 4.6"),
    ("claude-sonnet-4-5", "Claude Sonnet 4.5"),
    ("gpt-oss-120b", "GPT-OSS 120B"),
];

/// 模型清單認得的 CLI：代理團隊那四家（[`Backend`]），加上只做到「自訂連線＋選模型」的 Antigravity。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelCli {
    Agent(Backend),
    Antigravity,
}

impl ModelCli {
    /// Antigravity 的 key（＝圖示 key、`last_models` 的 key）。
    pub const ANTIGRAVITY_KEY: &'static str = "antigravity";

    pub fn by_key(key: &str) -> Option<Self> {
        if key.eq_ignore_ascii_case(Self::ANTIGRAVITY_KEY) {
            return Some(Self::Antigravity);
        }
        Backend::by_key(key).map(Self::Agent)
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Agent(b) => b.key(),
            Self::Antigravity => Self::ANTIGRAVITY_KEY,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Agent(b) => b.display_name(),
            Self::Antigravity => "Antigravity",
        }
    }

    /// 這條自訂連線是哪一家（同 [`adapters::backend_of`] 的想法：圖示，或執行檔名）。
    /// 都不是（WSL、ADB、使用者自己的工具）回 `None`。
    pub fn of_conn(conn: &CustomConn) -> Option<Self> {
        if let Some(b) = adapters::backend_of(conn) {
            return Some(Self::Agent(b));
        }
        if conn.icon.eq_ignore_ascii_case(Self::ANTIGRAVITY_KEY)
            || crate::sandbox::tool_kind(&conn.path) == crate::sandbox::ToolKind::Antigravity
        {
            return Some(Self::Antigravity);
        }
        None
    }
}

/// Antigravity CLI 的內建清單（見 [`ANTIGRAVITY_MODELS`]）。
pub fn antigravity_models() -> Vec<ModelInfo> {
    ANTIGRAVITY_MODELS
        .iter()
        .map(|(id, label)| ModelInfo {
            id: (*id).to_string(),
            label: (*label).to_string(),
        })
        .collect()
}

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

/// Claude Code 的完整模型名稱 → `(家族在 CLAUDE_ALIASES 裡的位置, 主版號, 次版號)`。
///
/// `claude-opus-5-5` → opus 5.5；`claude-fable-5` → fable 5.0；`claude-opus-5-5[1m]` 同 5.5；
/// `claude-haiku-4-5-20251001` → haiku 4.5（最後那段八位數是日期，不是版號）。
/// 舊式命名（`claude-3-5-sonnet-…`）與不認得的家族回 `None`。
fn parse_claude_id(id: &str) -> Option<(usize, u32, u32)> {
    claude_id_parts(id).map(|p| (p.family, p.major, p.minor))
}

/// 一個 Claude 模型名稱拆開之後的樣子。
struct ClaudeId {
    /// 家族在 [`CLAUDE_ALIASES`] 裡的位置。
    family: usize,
    major: u32,
    minor: u32,
    /// 名稱帶日期（`-20251001`）＝固定快照。
    dated: bool,
    /// 結尾是 `[1m]`（100 萬 token 的版本）。
    one_m: bool,
}

/// `claude-<家族>-<主版號>[-<次版號>][-<八位數日期>][[1m]]`，整串都要對得上。
fn claude_id_parts(id: &str) -> Option<ClaudeId> {
    let (base, one_m) = match id.strip_suffix("[1m]") {
        Some(b) => (b, true),
        None => (id, false),
    };
    let mut parts = base.strip_prefix("claude-")?.split('-');
    let name = parts.next()?;
    let family = CLAUDE_ALIASES.iter().position(|a| *a == name)?;
    let digits = |p: &str, max: usize| !p.is_empty() && p.len() <= max && p.bytes().all(|b| b.is_ascii_digit());
    let major = parts.next().filter(|p| digits(p, 2))?.parse().ok()?;
    let mut minor = None;
    let mut dated = false;
    for p in parts {
        // 次版號最多兩位數；八位數的是日期；其餘（或順序不對）＝不是模型名稱
        if digits(p, 2) && !dated && minor.is_none() {
            minor = Some(p.parse().ok()?);
        } else if p.len() == 8 && digits(p, 8) && !dated {
            dated = true;
        } else {
            return None;
        }
    }
    let minor = minor.unwrap_or(0);
    Some(ClaudeId {
        family,
        major,
        minor,
        dated,
        one_m,
    })
}

/// 掃 Claude Code 的程式本體時，一次讀多少（檔案有兩百多 MB，不整個讀進記憶體）。
const SCAN_CHUNK: usize = 4 * 1024 * 1024;
/// 相鄰兩塊重疊的長度（比最長的模型名稱長就好），跨塊的名稱才不會被切斷。
const SCAN_OVERLAP: usize = 64;
/// 一個版本在程式裡至少被提到這麼多次才列出來（只出現一兩次的是文件／對照表裡順帶提到的
/// 老模型，例 `claude-haiku-3-5`）。
const MIN_MENTIONS: u32 = 5;

/// 要在程式本體裡找哪一家的模型名稱。
struct NameScan {
    /// 名稱的開頭（`claude-`／`gemini-`）。
    prefix: &'static [u8],
    /// 名稱裡會不會有小數點（Gemini 的版本寫成 `2.5`；Claude 用 `-`）。
    dots: bool,
    /// 整串是不是這一家的模型名稱。
    valid: fn(&str) -> bool,
}

const CLAUDE_SCAN: NameScan = NameScan {
    prefix: b"claude-",
    dots: false,
    valid: |id| claude_id_parts(id).is_some(),
};

const GEMINI_SCAN: NameScan = NameScan {
    prefix: b"gemini-",
    dots: true,
    valid: |id| gemini_id_parts(id).is_some(),
};

/// 從一塊位元組裡找出模型名稱並累計次數。只算**起點在 `limit` 之前**的（後面那段留給下一塊）。
/// `before`＝這一塊前面那個位元組（用來判斷名稱前面是不是連著別的字）。
fn scan_bytes(scan: &NameScan, data: &[u8], limit: usize, before: u8, counts: &mut HashMap<String, u32>) {
    let pat = scan.prefix;
    let is_word = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'/');
    let mut i = 0;
    while i < limit && i + pat.len() <= data.len() {
        if data[i] != pat[0] || !data[i..].starts_with(pat) {
            i += 1;
            continue;
        }
        // 名稱的字元：小寫英數、`-`（Gemini 多一個 `.`），最後可以接 `[1m]`
        let mut end = i + pat.len();
        while end < data.len()
            && (data[end].is_ascii_lowercase()
                || data[end].is_ascii_digit()
                || data[end] == b'-'
                || (scan.dots && data[end] == b'.'))
        {
            end += 1;
        }
        if data[end..].starts_with(b"[1m]") {
            end += 4;
        }
        let after = end;
        // 句尾的句點（`…use gemini-2.5-pro.`）不是名稱的一部分
        while scan.dots && end > i && matches!(data[end - 1], b'.' | b'-') {
            end -= 1;
        }
        let prev = if i == 0 { before } else { data[i - 1] };
        // 後面連著別的字的不算。可以有小數點的名稱已經把句點吃掉了，所以那種只看英數／底線／斜線
        let next_ok = after >= data.len()
            || if scan.dots {
                !(data[after].is_ascii_alphanumeric() || matches!(data[after], b'_' | b'/'))
            } else {
                !is_word(data[after])
            };
        // 前面連著別的字（`us.anthropic.claude-…`、`/claude-…`）的不算
        if !is_word(prev) && next_ok {
            if let Ok(id) = std::str::from_utf8(&data[i..end]) {
                if (scan.valid)(id) {
                    *counts.entry(id.to_string()).or_insert(0) += 1;
                }
            }
        }
        i = after.max(i + 1);
    }
}

/// 這個檔案（Claude Code 的執行檔或 `cli.js`）裡提到哪些模型名稱、各幾次。
///
/// Claude Code 沒有列出模型的指令，但它 `/model` 選單裡的東西都寫在程式本體裡——
/// 所以**它一更新，這份清單就跟著更新**。只讀、分塊讀，不執行任何東西。
pub fn scan_claude_file(path: &Path) -> std::io::Result<HashMap<String, u32>> {
    let mut counts = HashMap::new();
    scan_file(&CLAUDE_SCAN, path, &mut counts)?;
    Ok(counts)
}

/// Gemini CLI 的程式本體（`…/@google/gemini-cli/bundle/` 底下那一堆 `.js`）提到哪些模型名稱。
/// 同 [`scan_claude_file`]：它也沒有列出模型的指令，名稱都寫在程式裡。
pub fn scan_gemini_dir(dir: &Path) -> HashMap<String, u32> {
    let mut counts = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return counts;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "js" || x == "mjs") && p.is_file() {
            let _ = scan_file(&GEMINI_SCAN, &p, &mut counts);
        }
    }
    counts
}

/// 分塊讀一個檔案，把找到的模型名稱累計進 `counts`。
fn scan_file(scan: &NameScan, path: &Path, counts: &mut HashMap<String, u32>) -> std::io::Result<()> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; SCAN_CHUNK + SCAN_OVERLAP];
    let mut carried = 0usize;
    let mut before = b' ';
    loop {
        // 把緩衝區填滿（`read` 一次不一定讀滿）
        let mut len = carried;
        while len < buf.len() {
            let n = file.read(&mut buf[len..])?;
            if n == 0 {
                break;
            }
            len += n;
        }
        let last = len < buf.len();
        let limit = if last { len } else { len - SCAN_OVERLAP };
        scan_bytes(scan, &buf[..len], limit, before, counts);
        if last {
            break;
        }
        before = buf[limit - 1];
        buf.copy_within(limit..len, 0);
        carried = len - limit;
    }
    Ok(())
}

/// [`gemini_id_parts`] 拆出來的 `(主版號, 次版號, 等級, 是不是 preview)`。
type GeminiParts = (u32, u32, u8, bool);

/// Gemini 的模型名稱拆開：`gemini-<版本>-<pro|flash|flash-lite>[-preview]`。
/// 回 `(主版號, 次版號, 等級, 是不是 preview)`；等級 0＝pro、1＝flash、2＝flash-lite。
///
/// 只收**聊天用**的那幾種：`…-image`、`…-live-…`、`…-base`、`…-customtools`、`…-001`、
/// embedding、computer-use 這些不是給 `--model` 選的，不收。
fn gemini_id_parts(id: &str) -> Option<GeminiParts> {
    let rest = id.strip_prefix("gemini-")?;
    let (ver, tier) = rest.split_once('-')?;
    let num = |p: &str| -> Option<u32> {
        (!p.is_empty() && p.len() <= 2 && p.bytes().all(|b| b.is_ascii_digit())).then(|| p.parse().ok())?
    };
    let (major, minor) = match ver.split_once('.') {
        Some((a, b)) => (num(a)?, num(b)?),
        None => (num(ver)?, 0),
    };
    let (tier, preview) = match tier.strip_suffix("-preview") {
        Some(t) => (t, true),
        None => (tier, false),
    };
    let rank = match tier {
        "pro" => 0,
        "flash" => 1,
        "flash-lite" => 2,
        _ => return None,
    };
    Some((major, minor, rank, preview))
}

/// Gemini CLI 的別名（它的程式裡的 `GEMINI_MODEL_ALIAS_*`）：`auto`＝讓 CLI 自己挑。
const GEMINI_ALIASES: &[&str] = &["auto", "pro", "flash", "flash-lite"];

/// Gemini CLI 的清單：四個別名，後面是它認得的每個版本，由新到舊
///（同一版裡 pro → flash → flash-lite，正式版排在 preview 前面）。
/// 名稱本身就帶版本（`gemini-2.5-pro`），所以顯示的就是名稱。
pub fn gemini_models(known: &HashMap<String, u32>) -> Vec<ModelInfo> {
    let mut ids: Vec<(&String, GeminiParts)> = known
        .iter()
        .filter(|(_, n)| **n >= MIN_MENTIONS)
        .filter_map(|(id, _)| gemini_id_parts(id).map(|p| (id, p)))
        .collect();
    if ids.is_empty() {
        return Vec::new();
    }
    ids.sort_by_key(|(id, p)| (std::cmp::Reverse((p.0, p.1)), p.2, p.3, (*id).clone()));
    GEMINI_ALIASES
        .iter()
        .map(|a| a.to_string())
        .chain(ids.into_iter().map(|(id, _)| id.clone()))
        .map(|id| ModelInfo { label: id.clone(), id })
        .collect()
}

/// Gemini CLI 的程式本體可能在哪幾個資料夾（照連線指到的執行檔推）：
/// npm 全域安裝在 Windows 是 `gemini.cmd` 旁邊的 `node_modules/…/bundle`；mac／Linux 的
/// `bin/gemini` 是指到 `…/bundle/gemini.js` 的連結，解開連結就是那個資料夾。
fn gemini_program_dirs(exe: &Path) -> Vec<PathBuf> {
    let pkg = Path::new("node_modules").join("@google").join("gemini-cli").join("bundle");
    let mut out = Vec::new();
    if let Some(dir) = exe.parent() {
        out.push(dir.join(&pkg));
        out.push(dir.join("..").join("lib").join(&pkg));
    }
    if let Ok(real) = std::fs::canonicalize(exe) {
        if let Some(dir) = real.parent() {
            out.push(dir.to_path_buf());
        }
    }
    out
}

/// 要去哪些檔案找：連線指到的執行檔本身（原生安裝的 `claude.exe`）；npm 版的 `claude.cmd`
/// 只是個啟動器，真正的程式在旁邊的 `node_modules/@anthropic-ai/claude-code/cli.js`。
fn claude_program_files(exe: &Path) -> Vec<PathBuf> {
    let mut out = vec![exe.to_path_buf()];
    if let Some(dir) = exe.parent() {
        out.push(
            dir.join("node_modules")
                .join("@anthropic-ai")
                .join("claude-code")
                .join("cli.js"),
        );
    }
    out
}

/// `~/.claude.json` 裡看得到的完整模型名稱：每個專案的 `lastModelUsage`（實際用過的）
/// ＋ `additionalModelOptionsCache`（Claude Code 自己的 `/model` 選單多出來的選項）。
///
/// **只讀模型名稱**，不碰其他內容；這個檔是 Claude Code 自己的設定，不是我們的。
pub fn claude_seen(json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    let mut push = |id: &str| {
        let id = id.trim();
        if valid_model(id) && parse_claude_id(id).is_some() && !out.iter().any(|x| x == id) {
            out.push(id.to_string());
        }
    };
    if let Some(projects) = v.get("projects").and_then(|p| p.as_object()) {
        for p in projects.values() {
            if let Some(usage) = p.get("lastModelUsage").and_then(|u| u.as_object()) {
                for id in usage.keys() {
                    push(id);
                }
            }
        }
    }
    if let Some(extra) = v.get("additionalModelOptionsCache").and_then(|a| a.as_array()) {
        for o in extra {
            if let Some(id) = o.get("value").and_then(|x| x.as_str()) {
                push(id);
            }
        }
    }
    out
}

/// 同一個版本（例 Opus 4.5）在程式裡的各種寫法。
#[derive(Default)]
struct ClaudeVersion {
    /// 一般的寫法：`(名稱, 次數, 有沒有帶日期)`。
    plain: Vec<(String, u32, bool)>,
    /// `[1m]` 的寫法。
    one_m: Vec<(String, u32, bool)>,
    total: u32,
}

/// 幾種寫法裡挑一個給使用者選：優先不帶日期的（會跟著小改版走），其次被提到最多次的。
fn pick_claude_id(ids: &[(String, u32, bool)]) -> Option<&str> {
    ids.iter()
        .max_by_key(|(id, count, dated)| (!*dated, *count, std::cmp::Reverse(id.len())))
        .map(|(id, _, _)| id.as_str())
}

/// `Opus 5.5`／`Fable 5`（次版號是 0 就不寫）。
fn claude_version_name(family: usize, major: u32, minor: u32) -> String {
    let alias = CLAUDE_ALIASES[family];
    let name = format!("{}{}", alias[..1].to_ascii_uppercase(), &alias[1..]);
    if minor == 0 {
        format!("{name} {major}")
    } else {
        format!("{name} {major}.{minor}")
    }
}

/// Claude Code 的清單（照它自己的 `/model` 選單的樣子）：
///
/// 1. 四個別名，標上目前的最新版：`Fable 5.1 (fable)`、`Opus 5.5 (opus)`…
/// 2. 它認得的**每一個版本**，由新到舊：`Opus 5.5`、`Sonnet 5.5`、`Fable 5.1`、`Opus 5`、
///    `Opus 4.8`…；有 100 萬 token 版本的多一筆 `· 1M`。
///
/// `known`＝從 Claude Code 程式本體掃出來的名稱與次數（[`scan_claude_file`]）。
/// `seen`＝這台機器實際用過的（[`claude_seen`]）：補上 `[1m]` 的寫法；掃不到程式本體時
///（裝法我們不認得）就只靠它，那時清單只有用過的版本。
///
/// 太舊的版本帳號不一定還能用——這裡不知道，也不該假裝知道，選了不能用 CLI 自己會講。
pub fn claude_models(known: &HashMap<String, u32>, seen: &[String]) -> Vec<ModelInfo> {
    let mut versions: std::collections::BTreeMap<(u32, u32, usize), ClaudeVersion> = Default::default();
    let mut add = |id: &str, count: u32| {
        let Some(p) = claude_id_parts(id) else { return };
        // key＝(主版號, 次版號, 家族)：之後倒著走就是由新到舊
        let v = versions.entry((p.major, p.minor, p.family)).or_default();
        v.total += count;
        let list = if p.one_m { &mut v.one_m } else { &mut v.plain };
        match list.iter_mut().find(|x| x.0 == id) {
            Some(x) => x.1 += count,
            None => list.push((id.to_string(), count, p.dated)),
        }
    };
    for (id, count) in known {
        add(id, *count);
    }
    // 用過的：有掃到程式本體時只算一次（補 `[1m]` 的寫法用，不足以讓一個沒掃到的版本上榜）；
    // 沒掃到時它就是唯一的來源，直接給足門檻
    let weight = if known.is_empty() { MIN_MENTIONS } else { 1 };
    for id in seen {
        add(id, weight);
    }
    versions.retain(|_, v| v.total >= MIN_MENTIONS);

    let mut out: Vec<ModelInfo> = CLAUDE_ALIASES
        .iter()
        .enumerate()
        .map(|(fi, alias)| ModelInfo {
            id: alias.to_string(),
            label: versions
                .keys()
                .rev()
                .find(|k| k.2 == fi)
                .map(|k| claude_version_name(fi, k.0, k.1))
                .unwrap_or_else(|| alias.to_string()),
        })
        .collect();

    // 同一個版本號裡的家族順序照別名的順序（fable、opus、sonnet、haiku）
    let mut keys: Vec<&(u32, u32, usize)> = versions.keys().collect();
    keys.sort_by_key(|k| (std::cmp::Reverse((k.0, k.1)), k.2));
    for key in keys {
        let v = &versions[key];
        let name = claude_version_name(key.2, key.0, key.1);
        let one_m = pick_claude_id(&v.one_m);
        // 只看過 `[1m]` 的寫法（例 `claude-fable-5-1[1m]`）→ 一般的寫法就是去掉 `[1m]`
        let plain = pick_claude_id(&v.plain)
            .map(str::to_string)
            .or_else(|| one_m.and_then(|id| id.strip_suffix("[1m]")).map(str::to_string));
        if let Some(id) = plain {
            out.push(ModelInfo { id, label: name.clone() });
        }
        if let Some(id) = one_m {
            out.push(ModelInfo {
                id: id.to_string(),
                label: format!("{name} \u{b7} 1M"),
            });
        }
    }
    out
}

/// Claude Code 的設定檔（`$CLAUDE_CONFIG_DIR/.claude.json`，預設 `~/.claude.json`）。
fn claude_config_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !dir.trim().is_empty() {
            return Some(PathBuf::from(dir).join(".claude.json"));
        }
    }
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok()?;
    Some(PathBuf::from(home).join(".claude.json"))
}

/// 跑一個指令、收它的 stdout（阻塞；有逾時）。
///
/// stdout 另開一條執行緒讀：`codex debug models` 的 JSON 有幾百 KB，等行程結束再讀的話
/// pipe 會先塞滿、行程卡在寫出，永遠等不到結束。逾時只砍**自己開的這個子行程**。
fn run(exe: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
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
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("timeout after {}s", timeout.as_secs()));
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

/// 更新 OpenCode 自己的模型快取要連網，最多等這麼久。
const REFRESH_TIMEOUT: Duration = Duration::from_secs(30);

/// `opencode models`；`refresh` 時先試 `--refresh`，那一條失敗（沒網路、舊版沒有這個參數）
/// 就退回不帶參數的——寧可拿到舊一點的清單，也不要因為更新不了而變成沒有清單。
fn opencode_models(exe: &Path, refresh: bool) -> Result<Vec<ModelInfo>, String> {
    if refresh {
        if let Ok(models) = run(exe, &["models", "--refresh"], REFRESH_TIMEOUT).map(|t| parse_opencode(&t)) {
            if !models.is_empty() {
                return Ok(models);
            }
        }
    }
    run(exe, &["models"], LIST_TIMEOUT).map(|t| parse_opencode(&t))
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
///
/// `refresh`＝使用者按了設定裡的「更新模型清單」：OpenCode 多帶 `--refresh`（重新向
/// models.dev 抓它自己的模型快取，要連網，所以等久一點）。其餘三家本來每次都是當場問／當場掃。
fn fetch(cli: ModelCli, conn: &CustomConn, refresh: bool) -> ModelList {
    let exe = Path::new(&conn.path);
    let exe_name = exe
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut list = ModelList {
        backend: cli.key().to_string(),
        backend_name: cli.display_name().to_string(),
        source: "none".to_string(),
        ..ModelList::default()
    };
    let backend = match cli {
        ModelCli::Agent(b) => b,
        ModelCli::Antigravity => {
            // 清單在它的伺服器上，程式本體找不到 → 內建的靜態清單（`static`：提示列會講清楚）
            list.models = antigravity_models();
            list.source = "static".to_string();
            return list;
        }
    };
    match backend {
        Backend::Codex => match run(exe, &["debug", "models"], LIST_TIMEOUT).map(|t| parse_codex(&t)) {
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
        Backend::OpenCode => match opencode_models(exe, refresh) {
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
            // 沒有列清單的指令：別名是內建的；有哪些版本去它的程式本體裡找（它一更新就跟著變），
            // 再用這台機器用過的紀錄補上 `[1m]` 的寫法
            let known = claude_program_files(exe)
                .iter()
                .filter_map(|p| scan_claude_file(p).ok())
                .find(|m| !m.is_empty())
                .unwrap_or_default();
            let seen = claude_config_path()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|t| claude_seen(&t))
                .unwrap_or_default();
            list.models = claude_models(&known, &seen);
            list.source = "builtin".to_string();
        }
        Backend::GeminiCli => {
            // 也沒有列清單的指令：去它的程式本體裡找（同 Claude Code）。找不到（裝法不認得）
            // 就維持沒有清單，使用者自己輸入
            let models = gemini_program_dirs(exe)
                .iter()
                .map(|d| gemini_models(&scan_gemini_dir(d)))
                .find(|m| !m.is_empty())
                .unwrap_or_default();
            if !models.is_empty() {
                list.models = models;
                list.source = "builtin".to_string();
            }
        }
    }
    list
}

type Cache = Mutex<HashMap<String, (Instant, ModelList)>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 這家 CLI 的模型清單（有快取；`refresh`＝不看快取）。`last` 每次都重新填。
pub fn list_for(settings: &SettingsStore, cli: ModelCli, conn: &CustomConn, refresh: bool) -> ModelList {
    let key = format!("{}|{}", cli.key(), conn.path.to_ascii_lowercase());
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
            let l = fetch(cli, conn, refresh);
            println!(
                "[AwayTerminal] 模型清單 {}：{} 個（來源={}{}）",
                cli.display_name(),
                l.models.len(),
                l.source,
                if l.note.is_empty() { String::new() } else { format!("，{}", l.note) }
            );
            // 失敗的結果不快取：使用者修好（例如重新登入）之後下一次就要拿得到
            if l.source != "none" || cli == ModelCli::Agent(Backend::GeminiCli) {
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
        .get(cli.key())
        .cloned()
        .unwrap_or_default();
    list
}

/// 把這台機器上找得到的每一家 CLI 的清單都重新問一次（設定視窗的「手動更新」與每天的
/// 「自動更新」共用）。回傳每一家的結果；找不到任何一家＝空的。
pub fn refresh_all(settings: &SettingsStore) -> Vec<ModelList> {
    let jobs: Vec<(ModelCli, CustomConn)> = adapters::ALL_KEYS
        .iter()
        .filter_map(|k| Backend::by_key(k))
        .filter_map(|b| adapters::resolve(settings, b).map(|c| (ModelCli::Agent(b), c)))
        .collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|(b, c)| scope.spawn(move || list_for(settings, *b, c, true)))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

/// 模型清單的「自動更新」（2.0.7）：設定裡有勾的話，每天在選的整點（01／03／…／23）
/// 重新向每一家 AI CLI 問一次。程式要開著；那個小時裡只做一次，錯過那個小時就等明天
/// （不補做——使用者選的就是那個時間）。
pub fn spawn_auto_refresh(settings: Arc<SettingsStore>) {
    std::thread::Builder::new()
        .name("model-auto-refresh".into())
        .spawn(move || {
            use chrono::{Datelike, Timelike};
            // 上一次做的是哪一天（本地日期的序號）；啟動那一小時若剛好是選的整點也會做
            let mut done_day: Option<i32> = None;
            loop {
                std::thread::sleep(Duration::from_secs(60));
                let s = settings.get();
                if !s.model_auto_refresh || !s.ask_model_on_open {
                    continue;
                }
                let now = chrono::Local::now();
                if now.hour() != u32::from(s.model_auto_refresh_hour) {
                    continue;
                }
                let today = now.num_days_from_ce();
                if done_day == Some(today) {
                    continue;
                }
                done_day = Some(today);
                let lists = refresh_all(&settings);
                println!(
                    "[AwayTerminal] 模型清單自動更新（{:02}:00）：{}",
                    s.model_auto_refresh_hour,
                    if lists.is_empty() {
                        "這台電腦沒有找到 AI CLI".to_string()
                    } else {
                        lists
                            .iter()
                            .map(|l| format!("{} {}", l.backend_name, l.models.len()))
                            .collect::<Vec<_>>()
                            .join("、")
                    }
                );
            }
        })
        .expect("spawn model-auto-refresh thread");
}

/// 設定視窗的「手動更新」：所有找得到的 CLI 立刻重新問一次（和自動更新做的事一樣）。
#[tauri::command]
pub async fn cli_models_refresh_all(settings: State<'_, Arc<SettingsStore>>) -> Result<Vec<ModelList>, String> {
    let settings = settings.inner().clone();
    tokio::task::spawn_blocking(move || refresh_all(&settings))
        .await
        .map_err(|e| e.to_string())
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
        // 代理團隊的設定視窗只會問四家；Antigravity 不在團隊裡，這裡不認它（它的清單走 `conn_models`）
        let jobs: Vec<(ModelCli, CustomConn)> = backends
            .iter()
            .filter_map(|k| Backend::by_key(k))
            .filter_map(|b| adapters::resolve(&settings, b).map(|c| (ModelCli::Agent(b), c)))
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
        let cli = ModelCli::of_conn(&conn)?;
        Some(list_for(&settings, cli, &conn, refresh))
    })
    .await
    .map_err(|e| e.to_string())
}

/// 記住這家 CLI 這次選的模型（下次開的時候預選它）。空字串＝選了「預設」。
#[tauri::command]
pub fn model_remember(backend: String, model: String, settings: State<'_, Arc<SettingsStore>>) {
    let Some(b) = ModelCli::by_key(&backend) else { return };
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

    #[test]
    fn reads_claude_versions_from_its_config() {
        assert_eq!(parse_claude_id("claude-opus-5-5"), Some((1, 5, 5)));
        assert_eq!(parse_claude_id("claude-opus-5-5[1m]"), Some((1, 5, 5)));
        assert_eq!(parse_claude_id("claude-fable-5"), Some((0, 5, 0)));
        assert_eq!(parse_claude_id("claude-haiku-4-5-20251001"), Some((3, 4, 5)), "八位數是日期");
        assert_eq!(parse_claude_id("claude-3-5-sonnet-20241022"), None, "舊式命名不認");
        assert_eq!(parse_claude_id("gpt-6-sol"), None);

        // 重複的 key（不同大小寫的專案路徑）與不相干的欄位都不可以讓它壞掉
        let json = r#"{
            "numStartups": 3,
            "projects": {
                "D:/a": {"lastModelUsage": {"claude-opus-4-8[1m]": {}, "claude-haiku-4-5-20251001": {}}},
                "D:/b": {"lastModelUsage": {"claude-opus-5-5": {}, "claude-opus-5-5[1m]": {}, "claude-sonnet-5-5": {}}},
                "D:/c": {}
            },
            "additionalModelOptionsCache": [{"value": "claude-fable-5-1[1m]", "label": "Fable", "description": "Fable 5.1"}]
        }"#;
        let seen = claude_seen(json);
        assert!(seen.contains(&"claude-fable-5-1[1m]".to_string()));
        assert!(claude_seen("not json").is_empty());

        // 掃不到程式本體（裝法不認得）→ 只靠用過的紀錄：別名標上用過的最新版，後面是用過的每一版
        let m = claude_models(&HashMap::new(), &seen);
        let pairs: Vec<(&str, &str)> = m.iter().map(|x| (x.id.as_str(), x.label.as_str())).collect();
        assert_eq!(
            pairs,
            [
                ("fable", "Fable 5.1"),
                ("opus", "Opus 5.5"),
                ("sonnet", "Sonnet 5.5"),
                ("haiku", "Haiku 4.5"),
                ("claude-opus-5-5", "Opus 5.5"),
                ("claude-opus-5-5[1m]", "Opus 5.5 \u{b7} 1M"),
                ("claude-sonnet-5-5", "Sonnet 5.5"),
                // 只看過 [1m] 的寫法 → 一般的寫法就是去掉 [1m]
                ("claude-fable-5-1", "Fable 5.1"),
                ("claude-fable-5-1[1m]", "Fable 5.1 \u{b7} 1M"),
                ("claude-opus-4-8", "Opus 4.8"),
                ("claude-opus-4-8[1m]", "Opus 4.8 \u{b7} 1M"),
                ("claude-haiku-4-5-20251001", "Haiku 4.5"),
            ]
        );

        // 沒有任何紀錄：只有別名、不標版本
        let bare = claude_models(&HashMap::new(), &[]);
        assert_eq!(bare.len(), 4);
        assert!(bare.iter().all(|x| x.id == x.label));
    }

    /// 模型名稱的各種寫法都拆得對，不是模型名稱的不認。
    #[test]
    fn splits_claude_model_names() {
        let p = claude_id_parts("claude-sonnet-4-5-20250929[1m]").unwrap();
        assert_eq!((p.family, p.major, p.minor, p.dated, p.one_m), (2, 4, 5, true, true));
        let p = claude_id_parts("claude-opus-4-20250514").unwrap();
        assert_eq!((p.major, p.minor, p.dated), (4, 0, true), "主版號後面直接接日期");
        let p = claude_id_parts("claude-opus-4-0").unwrap();
        assert_eq!((p.major, p.minor, p.dated, p.one_m), (4, 0, false, false));
        for bad in ["claude-opus", "claude-opus-latest", "claude-opus-5-5-preview", "claude-code-5", "claude-opus-123", "claude-opus-5-5-5"] {
            assert!(claude_id_parts(bad).is_none(), "{bad} 不是模型名稱");
        }
    }

    /// 從程式本體掃模型名稱：前後連著別的字的不算、跨塊的不漏不重複。
    #[test]
    fn scans_model_names_out_of_a_program_file() {
        let text = br#"x="claude-opus-5-5",y='claude-opus-5-5[1m]';us.anthropic.claude-opus-5-5 /claude-sonnet-5
            claude-opus-5-5-preview claude-fable-5-1) claude-haiku-4-5-20251001, claude-opus-5-5X claude-code-5"#;
        let mut counts = HashMap::new();
        scan_bytes(&CLAUDE_SCAN, text, text.len(), b' ', &mut counts);
        let mut got: Vec<(&str, u32)> = counts.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        got.sort();
        assert_eq!(
            got,
            [
                ("claude-fable-5-1", 1),
                ("claude-haiku-4-5-20251001", 1),
                ("claude-opus-5-5", 1),
                ("claude-opus-5-5[1m]", 1),
            ],
            "us.anthropic.…、/…、-preview、後面連著字母的、不是模型家族的都不算"
        );

        // 真的走一次檔案：名稱剛好落在兩塊的交界上
        let dir = std::env::temp_dir().join(format!("awayterm-scan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("prog.bin");
        let mut data = vec![b'.'; SCAN_CHUNK - 10];
        data.extend_from_slice(b" claude-opus-4-8 ");
        data.extend(std::iter::repeat_n(b'.', SCAN_CHUNK));
        data.extend_from_slice(b"\"claude-sonnet-5\"");
        std::fs::write(&file, &data).unwrap();
        let found = scan_claude_file(&file).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found.get("claude-opus-4-8"), Some(&1), "跨塊的名稱要找得到、而且只算一次");
        assert_eq!(found.get("claude-sonnet-5"), Some(&1), "檔案結尾的也要找得到");
        assert_eq!(found.len(), 2);
    }

    /// Gemini CLI：只收聊天用的模型名稱；句尾的句點不算名稱的一部分；清單由新到舊。
    #[test]
    fn lists_gemini_models_from_its_program() {
        assert_eq!(gemini_id_parts("gemini-2.5-pro"), Some((2, 5, 0, false)));
        assert_eq!(gemini_id_parts("gemini-3-flash-preview"), Some((3, 0, 1, true)));
        assert_eq!(gemini_id_parts("gemini-3.5-flash-lite"), Some((3, 5, 2, false)));
        for bad in [
            "gemini-3", "gemini-2.5", "gemini-2.5-flash-image", "gemini-3-flash-base", "gemini-2.0-flash-001",
            "gemini-3.1-pro-preview-customtools", "gemini-2.0-flash-live-preview-04-09", "gemini-embedding-001",
            "gemini-9001-super-duper", "gemini-2.5-computer-use-preview-10-2025",
        ] {
            assert!(gemini_id_parts(bad).is_none(), "{bad} 不是給 --model 選的");
        }

        let text = br#"var A="gemini-2.5-pro";use gemini-2.5-pro. or "gemini-3-flash-preview", models/gemini-2.5-flash
            x="gemini-2.5-flash-image" y='gemini-3.8-flash' gemini-2.5-proX"#;
        let mut counts = HashMap::new();
        scan_bytes(&GEMINI_SCAN, text, text.len(), b' ', &mut counts);
        let mut got: Vec<(&str, u32)> = counts.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        got.sort();
        assert_eq!(
            got,
            [("gemini-2.5-pro", 2), ("gemini-3-flash-preview", 1), ("gemini-3.8-flash", 1)],
            "句尾的句點不算；models/… 與 …-image、後面連著字母的不收"
        );

        // 實際套件（0.62.0）掃出來的次數
        let known: HashMap<String, u32> = [
            ("gemini-2.5-pro", 54), ("gemini-3-flash-preview", 51), ("gemini-3-pro-preview", 48),
            ("gemini-2.0-flash", 36), ("gemini-3.1-pro-preview", 33), ("gemini-2.5-flash", 24),
            ("gemini-3.5-flash", 15), ("gemini-3.1-flash-lite", 15), ("gemini-2.5-flash-lite", 15),
            ("gemini-3.8-flash", 12), ("gemini-3.5-flash-lite", 12), ("gemini-3.1-flash-lite-preview", 6),
            ("gemini-3-flash", 3), ("gemini-3-pro", 3), ("gemini-3.1-pro", 3),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let ids: Vec<String> = gemini_models(&known).into_iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            [
                "auto", "pro", "flash", "flash-lite",
                "gemini-3.8-flash",
                "gemini-3.5-flash", "gemini-3.5-flash-lite",
                "gemini-3.1-pro-preview", "gemini-3.1-flash-lite", "gemini-3.1-flash-lite-preview",
                "gemini-3-pro-preview", "gemini-3-flash-preview",
                "gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite",
                "gemini-2.0-flash",
            ],
            "別名在前；只被提到三次的（gemini-3-flash…）不列"
        );
        assert!(gemini_models(&HashMap::new()).is_empty(), "找不到程式本體＝沒有清單（連別名也不列）");
    }

    /// 掃得到程式本體時的清單：別名標最新版；每個版本由新到舊；只被提到一兩次的老模型不列；
    /// 同一版有好幾種寫法時挑不帶日期、被提到最多次的。
    #[test]
    fn lists_every_version_the_program_knows() {
        let known: HashMap<String, u32> = [
            ("claude-opus-5-5", 46), ("claude-opus-5-5[1m]", 2), ("claude-opus-5", 79),
            ("claude-opus-4-8", 55), ("claude-opus-4", 10), ("claude-opus-4-0", 22), ("claude-opus-4-20250514", 6),
            ("claude-sonnet-5-5", 29), ("claude-sonnet-5", 53), ("claude-sonnet-3-7", 2),
            ("claude-fable-5-1", 33), ("claude-fable-5", 35),
            ("claude-haiku-4-5", 30), ("claude-haiku-4-5-20251001", 16), ("claude-haiku-3-5", 2),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        // 用過的紀錄只補 [1m] 的寫法；程式本體不認得的版本（這裡的 9.9）不會因為用過就上榜
        let seen = vec!["claude-fable-5-1[1m]".to_string(), "claude-opus-9-9".to_string()];
        let m = claude_models(&known, &seen);
        let pairs: Vec<(&str, &str)> = m.iter().map(|x| (x.id.as_str(), x.label.as_str())).collect();
        assert_eq!(
            pairs,
            [
                ("fable", "Fable 5.1"),
                ("opus", "Opus 5.5"),
                ("sonnet", "Sonnet 5.5"),
                ("haiku", "Haiku 4.5"),
                ("claude-opus-5-5", "Opus 5.5"),
                ("claude-opus-5-5[1m]", "Opus 5.5 \u{b7} 1M"),
                ("claude-sonnet-5-5", "Sonnet 5.5"),
                ("claude-fable-5-1", "Fable 5.1"),
                ("claude-fable-5-1[1m]", "Fable 5.1 \u{b7} 1M"),
                ("claude-fable-5", "Fable 5"),
                ("claude-opus-5", "Opus 5"),
                ("claude-sonnet-5", "Sonnet 5"),
                ("claude-opus-4-8", "Opus 4.8"),
                ("claude-haiku-4-5", "Haiku 4.5"),
                ("claude-opus-4-0", "Opus 4"),
            ]
        );
    }

    /// **手動**（`cargo test -- --ignored real_cli`）：真的去問這台機器上裝的 CLI。
    /// 用環境變數給路徑：`AT_CODEX`、`AT_OPENCODE`（沒給的那一家跳過）。
    /// 重點是驗 [`run`]：`.cmd` 起得來、幾百 KB 的輸出不會卡住、不會跳黑窗。
    #[test]
    #[ignore]
    fn real_cli_lists_models() {
        // Claude Code：掃這台機器真的 `claude.exe`（兩百多 MB）＋讀真的 `~/.claude.json`
        //（`AT_CLAUDE` 沒給就只靠用過的紀錄）
        let conn = CustomConn { path: std::env::var("AT_CLAUDE").unwrap_or_default(), ..CustomConn::default() };
        let started = Instant::now();
        let claude = fetch(ModelCli::Agent(Backend::ClaudeCode), &conn, false);
        println!("ClaudeCode（{} ms）:", started.elapsed().as_millis());
        for m in &claude.models {
            println!("  {:<28} {}", m.label, m.id);
        }
        assert_eq!(claude.source, "builtin");
        assert!(claude.models.len() >= CLAUDE_ALIASES.len());
        // Gemini CLI：`AT_GEMINI_BUNDLE`＝它的 `bundle` 資料夾（這台機器沒裝，用下載下來的套件驗）
        if let Ok(dir) = std::env::var("AT_GEMINI_BUNDLE") {
            let started = Instant::now();
            let list = gemini_models(&scan_gemini_dir(Path::new(&dir)));
            println!("GeminiCLI（{} ms）: {:?}", started.elapsed().as_millis(), list.iter().map(|m| m.id.as_str()).collect::<Vec<_>>());
            assert!(list.len() > GEMINI_ALIASES.len());
        }
        for (var, backend) in [("AT_CODEX", Backend::Codex), ("AT_OPENCODE", Backend::OpenCode)] {
            let Ok(path) = std::env::var(var) else { continue };
            let conn = CustomConn { path, ..CustomConn::default() };
            let started = Instant::now();
            let list = fetch(ModelCli::Agent(backend), &conn, false);
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

    /// Antigravity CLI（2.0.6，只做到選模型）：圖示或執行檔名 `agy` 都認得；不是代理團隊的 Backend。
    #[test]
    fn antigravity_is_a_model_cli_but_not_a_team_backend() {
        let conn = |path: &str, icon: &str| CustomConn {
            name: "x".into(),
            path: path.into(),
            icon: icon.into(),
            ..CustomConn::default()
        };
        assert_eq!(
            ModelCli::of_conn(&conn("C:\\Users\\x\\AppData\\Local\\agy\\bin\\agy.exe", "run")),
            Some(ModelCli::Antigravity)
        );
        assert_eq!(ModelCli::of_conn(&conn("D:\\tools\\mything.exe", "antigravity")), Some(ModelCli::Antigravity));
        assert_eq!(ModelCli::of_conn(&conn("C:\\npm\\claude.cmd", "run")), Some(ModelCli::Agent(Backend::ClaudeCode)));
        assert_eq!(ModelCli::of_conn(&conn("C:\\x\\strategy.exe", "run")), None);
        assert_eq!(ModelCli::by_key("ANTIGRAVITY"), Some(ModelCli::Antigravity));
        assert_eq!(ModelCli::by_key("codex"), Some(ModelCli::Agent(Backend::Codex)));
        assert_eq!(Backend::by_key("antigravity"), None, "代理團隊不能選它（第 1 層）");
        assert_eq!(ModelCli::Antigravity.key(), "antigravity");

        // 靜態清單：id 都合法、不重複、Gemini 在前
        let list = antigravity_models();
        assert!(list.len() >= 5);
        assert!(list.iter().all(|m| valid_model(&m.id)));
        let mut ids: Vec<&str> = list.iter().map(|m| m.id.as_str()).collect();
        ids.dedup();
        assert_eq!(ids.len(), list.len());
        assert!(ids[0].starts_with("gemini-"));
    }
}
