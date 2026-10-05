//! 角色檔：三層組合（搬移舊版 `Services/MultiAgent/RoleLibrary.cs`）。
//!
//! 每個 agent 拿到**一份檔**：
//! ① `common.md`（所有人都有）
//! ② `roles/<角色>.md`（依角色；None＝略過）
//! ③ [`runtime_context`]（AwayTerminal 產生：Agent ID、隊友名單、信箱格式…）
//!
//! | 舊版 | 這裡 |
//! |---|---|
//! | 範本嵌在 exe（csproj EmbeddedResource） | [`TEMPLATES`]（`include_str!`），同樣是嵌在執行檔裡 |
//! | `%LOCALAPPDATA%\AwayTerminal\multiagent\` | `settings.dir()/multiagent/`（讓使用者自己改） |
//! | `.defaults.json` 雜湊記錄 | 同（去 BOM、CRLF→LF 之後的 SHA-256） |
//! | `PreviousDefaults`（以前出過的範本雜湊） | [`PREVIOUS_DEFAULTS`]，**目前是空的**——v2 還沒出過任何版本的範本 |
//! | `EnsureDefaults` / `RestoreDefaults` | [`ensure_defaults`] / [`restore_defaults`] |
//! | `ListRoles` / `TitleOf` / `Compose` / `ClearSession` | 同名函式 |
//!
//! ⚠️ 角色檔與 `common.md` 的**語言不套八語**（TASK-017 A10）：那是給 agent 讀的，
//! 不是 UI。原樣照舊版。
//!
//! ## 兩套角色庫（TASK-019）
//! 舊版的 `RoleLibrary`（代理團隊）與 `ChatRoleLibrary`（AI 聊天室）是兩個幾乎一樣的類別——
//! 雜湊自動更新、三層組合、標題取法全部重複一遍。這裡只有**一份實作**，差異放進
//! [`Library`]：資料夾名、內嵌範本、內建角色順序、預設角色、沒有角色時的標題、
//! 執行期脈絡產生器。[`TEAM`] 與 [`CHAT`] 兩個常數就是舊版那兩個類別。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::message::BUS_REL_DIR;
use super::team::{Slot, Team};

/// 一套角色庫（舊版的 `RoleLibrary` ／ `ChatRoleLibrary`）。
///
/// 兩套共用同一份實作，差別只有這幾個欄位。
pub struct Library {
    /// 資料目錄底下的資料夾名（`multiagent` ／ `chatroom`）。
    pub dir_name: &'static str,
    /// 內嵌的範本（第一次用到時複製出去讓使用者改）。
    pub templates: &'static [(&'static str, &'static str)],
    /// 下拉選單的固定順序（其餘依檔名排在後面）。
    pub built_in: &'static [&'static str],
    /// 設定視窗格 1～4 的預設角色。
    pub default_slots: &'static [&'static str],
    /// 以前出過的範本雜湊（`.defaults.json` 不見時的後備判斷）。
    pub previous: &'static [(&'static str, &'static [&'static str])],
    /// 沒有選角色時顯示什麼（代理團隊＝`None`；聊天室＝`-`，照舊版）。
    pub title_none: &'static str,
    /// 這是聊天室那一套嗎。
    ///
    /// ⚠️ **不要用 `std::ptr::eq(lib, &CHAT)` 判斷**：`const` 在 Rust 是**每個使用點各自
    /// inline 一份**，`&CHAT` 在不同地方可能是不同位址，比對會隨機失敗（實際踩到：
    /// 聊天室的角色檔被組成代理團隊的執行期脈絡）。所以用一個明確的旗標。
    pub chat: bool,
}

/// 代理團隊（舊版 `RoleLibrary`）。
pub const TEAM: Library = Library {
    dir_name: "multiagent",
    templates: TEAM_TEMPLATES,
    built_in: BUILT_IN_ROLES,
    default_slots: DEFAULT_SLOT_ROLES,
    previous: PREVIOUS_DEFAULTS,
    title_none: "None",
    chat: false,
};

/// AI 聊天室（舊版 `ChatRoleLibrary`）。
pub const CHAT: Library = Library {
    dir_name: "chatroom",
    templates: CHAT_TEMPLATES,
    built_in: CHAT_ROLES,
    default_slots: CHAT_DEFAULT_SLOT_ROLES,
    // 聊天室的範本這一版才第一次出現，所以還沒有「以前那一版」
    previous: &[],
    title_none: "-",
    chat: true,
};

/// 內嵌的範本：(相對路徑, 內容)。路徑一律用 `/`（寫檔時才換成平台分隔符）。
pub const TEAM_TEMPLATES: &[(&str, &str)] = &[
    ("common.md", include_str!("../../resources/multiagent/common.md")),
    (
        "roles/product-manager.md",
        include_str!("../../resources/multiagent/roles/product-manager.md"),
    ),
    (
        "roles/software-engineer.md",
        include_str!("../../resources/multiagent/roles/software-engineer.md"),
    ),
    (
        "roles/software-architect.md",
        include_str!("../../resources/multiagent/roles/software-architect.md"),
    ),
    (
        "roles/ui-ux-designer.md",
        include_str!("../../resources/multiagent/roles/ui-ux-designer.md"),
    ),
    (
        "roles/qa-engineer.md",
        include_str!("../../resources/multiagent/roles/qa-engineer.md"),
    ),
];

/// 聊天室的 21 個角色（原樣照舊版 `Resources/ChatRoom/`）。
pub const CHAT_TEMPLATES: &[(&str, &str)] = &[
    ("common.md", include_str!("../../resources/chatroom/common.md")),
    (
        "roles/career-advisor.md",
        include_str!("../../resources/chatroom/roles/career-advisor.md"),
    ),
    (
        "roles/cloud-engineer.md",
        include_str!("../../resources/chatroom/roles/cloud-engineer.md"),
    ),
    (
        "roles/creative-designer.md",
        include_str!("../../resources/chatroom/roles/creative-designer.md"),
    ),
    (
        "roles/devils-advocate.md",
        include_str!("../../resources/chatroom/roles/devils-advocate.md"),
    ),
    (
        "roles/education-advisor.md",
        include_str!("../../resources/chatroom/roles/education-advisor.md"),
    ),
    (
        "roles/embedded-engineer.md",
        include_str!("../../resources/chatroom/roles/embedded-engineer.md"),
    ),
    (
        "roles/hardware-engineer.md",
        include_str!("../../resources/chatroom/roles/hardware-engineer.md"),
    ),
    (
        "roles/health-advisor.md",
        include_str!("../../resources/chatroom/roles/health-advisor.md"),
    ),
    (
        "roles/host.md",
        include_str!("../../resources/chatroom/roles/host.md"),
    ),
    (
        "roles/investment-analyst.md",
        include_str!("../../resources/chatroom/roles/investment-analyst.md"),
    ),
    (
        "roles/legal-advisor.md",
        include_str!("../../resources/chatroom/roles/legal-advisor.md"),
    ),
    (
        "roles/marketing-expert.md",
        include_str!("../../resources/chatroom/roles/marketing-expert.md"),
    ),
    (
        "roles/network-engineer.md",
        include_str!("../../resources/chatroom/roles/network-engineer.md"),
    ),
    (
        "roles/psychology-advisor.md",
        include_str!("../../resources/chatroom/roles/psychology-advisor.md"),
    ),
    (
        "roles/relationship-advisor.md",
        include_str!("../../resources/chatroom/roles/relationship-advisor.md"),
    ),
    (
        "roles/researcher.md",
        include_str!("../../resources/chatroom/roles/researcher.md"),
    ),
    (
        "roles/rf-engineer.md",
        include_str!("../../resources/chatroom/roles/rf-engineer.md"),
    ),
    (
        "roles/security-expert.md",
        include_str!("../../resources/chatroom/roles/security-expert.md"),
    ),
    (
        "roles/software-engineer.md",
        include_str!("../../resources/chatroom/roles/software-engineer.md"),
    ),
    (
        "roles/travel-planner.md",
        include_str!("../../resources/chatroom/roles/travel-planner.md"),
    ),
    (
        "roles/wireless-expert.md",
        include_str!("../../resources/chatroom/roles/wireless-expert.md"),
    ),
];

/// 聊天室角色的下拉順序（照舊版 `ChatRoleLibrary.BuiltInRoles`：
/// 主持人、反方辯論者、情報研究員在前，其餘依使用者給的清單）。
pub const CHAT_ROLES: &[&str] = &[
    "host",
    "devils-advocate",
    "researcher",
    "software-engineer",
    "embedded-engineer",
    "rf-engineer",
    "wireless-expert",
    "hardware-engineer",
    "network-engineer",
    "cloud-engineer",
    "security-expert",
    "legal-advisor",
    "investment-analyst",
    "marketing-expert",
    "health-advisor",
    "education-advisor",
    "career-advisor",
    "psychology-advisor",
    "travel-planner",
    "creative-designer",
    "relationship-advisor",
];

/// 聊天室第 1～4 位的預設角色（**第 1 位固定主持人**）。
pub const CHAT_DEFAULT_SLOT_ROLES: &[&str] =
    &["host", "devils-advocate", "researcher", "software-engineer"];

/// 聊天室的主持人角色檔名（第 1 位固定它，設定視窗的角色下拉會停用）。
pub const CHAT_HOST_ROLE: &str = "host";

/// 內建角色在下拉裡的固定順序（其餘使用者自訂的依檔名排在後面）。
/// 舊版使用者指定 UI/UX Designer 排在 QA Engineer 上面。
pub const BUILT_IN_ROLES: &[&str] = &[
    "product-manager",
    "software-engineer",
    "software-architect",
    "ui-ux-designer",
    "qa-engineer",
];

/// 設定視窗格 1～4 的預設角色（和下拉順序分開：格 4 仍預設 QA）。
pub const DEFAULT_SLOT_ROLES: &[&str] = &[
    "product-manager",
    "software-engineer",
    "software-architect",
    "qa-engineer",
];

/// 以前各版內建範本的正規化 SHA-256（改範本時把「改之前」那版的雜湊加進來）。
///
/// 正常情況用不到它：v2 每次寫檔都會更新 `.defaults.json`，所以判斷「使用者改過沒有」
/// 靠記錄就夠。這張表是給**記錄不見了**的資料目錄用的（使用者自己刪掉 `.defaults.json`，
/// 或從別台複製 `multiagent/` 過來）。查不到就一律當成「使用者的版本」不動——
/// 和舊版 `PreviousDefaults` 的行為一致。
pub const PREVIOUS_DEFAULTS: &[(&str, &[&str])] = &[
    // TASK-018 加了「## Sandbox Mode」那一段之前的版本（commit 58849e3）
    ("common.md", &["a7a3fc9b00fe105d5bb19e75d8df8063d876b49f2a741dea7c7d7091c2ba6144"]),
];

const MANIFEST_FILE: &str = ".defaults.json";

/// **只給 `--verify` 用**的資料目錄覆寫。
///
/// 為什麼要它：`--verify` 建的是一個真的團隊，會在資料目錄底下 `clear_session()` ＋寫角色檔。
/// 那是使用者真的資料目錄（`%APPDATA%\\com.awaysu.awayterminal\\multiagent\\`）——如果使用者
/// 此刻正開著組號 1 的團隊，`clear_session(1)` 會把它的角色檔刪掉。
/// 「測試與 `--verify` 不可以動使用者真的在用的系統狀態」是這個專案最重要的規則之一，
/// 所以驗證時把整個資料目錄改到 `%TEMP%`。覆寫只活在記憶體裡，`agent_verify_end` 會清掉。
static VERIFY_DATA_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

pub fn set_verify_data_dir(dir: Option<PathBuf>) {
    if let Ok(mut g) = VERIFY_DATA_DIR.lock() {
        *g = dir;
    }
}

/// 實際要用的資料目錄（`--verify` 時是 `%TEMP%` 底下那個）。
pub fn data_dir_or_verify(settings_dir: PathBuf) -> PathBuf {
    VERIFY_DATA_DIR
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or(settings_dir)
}

/// 這套角色庫的根（範本、角色檔、`sessions/`）。
pub fn root(lib: &Library, data_dir: &Path) -> PathBuf {
    data_dir.join(lib.dir_name)
}

pub fn roles_dir(lib: &Library, data_dir: &Path) -> PathBuf {
    root(lib, data_dir).join("roles")
}

pub fn common_path(lib: &Library, data_dir: &Path) -> PathBuf {
    root(lib, data_dir).join("common.md")
}

/// 一個組號的成品資料夾（組好的角色檔放這裡）。
pub fn session_dir(lib: &Library, data_dir: &Path, team_number: u32) -> PathBuf {
    root(lib, data_dir)
        .join("sessions")
        .join(team_number.to_string())
}

/// 去 UTF-8 BOM、CRLF→LF 之後的 SHA-256（小寫十六進位）。
///
/// 為什麼要正規化：範本檔在 git 工作目錄裡可能是 LF 或 CRLF，嵌進執行檔的位元組
/// 不一定和使用者資料目錄裡那一份一樣（舊版註解）。
fn normalized_hash(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let text = text.replace("\r\n", "\n");
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

/// 讀 `.defaults.json`；沒有＝空（沒記錄的檔改用 [`PREVIOUS_DEFAULTS`] 判斷）；
/// 壞掉＝`None`（同樣用 `PREVIOUS_DEFAULTS`，而且不覆寫壞檔）。
fn load_manifest(lib: &Library, data_dir: &Path) -> Option<HashMap<String, String>> {
    let path = root(lib, data_dir).join(MANIFEST_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(HashMap::new()),
        Err(_) => None,
    }
}

fn save_manifest(lib: &Library, data_dir: &Path, manifest: &HashMap<String, String>) {
    let dir = root(lib, data_dir);
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_string_pretty(manifest) {
        if let Err(e) = std::fs::write(dir.join(MANIFEST_FILE), json) {
            println!("[AwayTerminal] 代理團隊角色檔記錄寫入失敗：{e}");
        }
    }
}

/// 缺檔就從內嵌範本補；使用者沒改過的舊版範本換成新版（改過的不動）。
pub fn ensure_defaults(lib: &Library, data_dir: &Path) {
    write_defaults(lib, data_dir, false);
}

/// 內建範本全部覆寫回預設（使用者自己新增的角色檔不受影響）。
pub fn restore_defaults(lib: &Library, data_dir: &Path) {
    write_defaults(lib, data_dir, true);
}

/// 舊版 `WriteDefaults`。
///
/// 為什麼不是「檔案存在就不動」：舊版實錄——加 UI/UX Designer 時改了內嵌的
/// `product-manager.md`，但使用者資料目錄裡複製出來的舊版（從沒改過）一直沒換，
/// PM 不知道有設計師。所以要分得出「使用者改過」和「只是舊版」。
fn write_defaults(lib: &Library, data_dir: &Path, overwrite: bool) {
    let mut manifest = load_manifest(lib, data_dir);
    let mut changed = false;
    for (rel, body) in lib.templates {
        let target = root(lib, data_dir).join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let new_hash = normalized_hash(body.as_bytes());
        let existing = std::fs::read(&target).ok();
        match (&existing, overwrite) {
            (Some(cur_bytes), false) => {
                let cur = normalized_hash(cur_bytes);
                if cur == new_hash {
                    // 已經是這一版：只補記錄
                } else {
                    let untouched = match manifest.as_ref().and_then(|m| m.get(*rel)) {
                        Some(written) => *written == cur,
                        None => lib
                            .previous
                            .iter()
                            .find(|(k, _)| k == rel)
                            .is_some_and(|(_, olds)| olds.contains(&cur.as_str())),
                    };
                    if !untouched {
                        continue; // 使用者改過 → 不動，也不改記錄
                    }
                    if let Err(e) = std::fs::write(&target, body) {
                        println!("[AwayTerminal] 代理團隊角色檔 {rel} 更新失敗：{e}");
                        continue;
                    }
                    println!("[AwayTerminal] 代理團隊角色檔已更新成新版預設：{rel}");
                }
            }
            _ => {
                if let Some(parent) = target.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&target, body) {
                    println!("[AwayTerminal] 代理團隊角色檔 {rel} 寫入失敗：{e}");
                    continue;
                }
            }
        }
        if let Some(m) = manifest.as_mut() {
            if m.get(*rel) != Some(&new_hash) {
                m.insert(rel.to_string(), new_hash);
                changed = true;
            }
        }
    }
    if changed {
        if let Some(m) = &manifest {
            save_manifest(lib, data_dir, m);
        }
    }
}

/// 一個角色（下拉選單一列）。
#[derive(Clone, Debug, serde::Serialize)]
pub struct RoleInfo {
    pub key: String,
    pub title: String,
}

/// `roles/*.md` → (key, 標題)。內建的在前、其餘依檔名。
pub fn list_roles(lib: &Library, data_dir: &Path) -> Vec<RoleInfo> {
    ensure_defaults(lib, data_dir);
    let mut keys: Vec<String> = match std::fs::read_dir(roles_dir(lib, data_dir)) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md")))
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .collect(),
        Err(e) => {
            println!("[AwayTerminal] 代理團隊角色清單讀取失敗：{e}");
            Vec::new()
        }
    };
    keys.sort_by(|a, b| {
        let rank = |k: &str| lib.built_in.iter().position(|b| *b == k).unwrap_or(100);
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
    });
    keys.into_iter()
        .map(|k| RoleInfo {
            title: title_of(lib, data_dir, &k),
            key: k,
        })
        .collect()
}

/// 角色標題＝角色檔第一個「# 」標題；沒有就把檔名轉成 Title Case
/// （`software-engineer` → `Software Engineer`）。空 key＝`None`。
pub fn title_of(lib: &Library, data_dir: &Path, key: &str) -> String {
    if key.trim().is_empty() {
        return lib.title_none.to_string();
    }
    // 第一次用（範本還沒複製出來）時不能退回檔名轉換——「qa-engineer」會變「Qa Engineer」
    ensure_defaults(lib, data_dir);
    let path = roles_dir(lib, data_dir).join(format!("{key}.md"));
    // 使用者用記事本存過的角色檔可能帶 BOM（第一行變成「\u{feff}# …」，`trim` 去不掉）
    // 或是 Big5／UTF-16：一律經 `read_text` 解碼並去 BOM，否則標題退成檔名（G10）
    if let Ok((text, _)) = super::message::read_text(&path) {
        for line in text.lines() {
            let t = line.trim();
            if let Some(title) = t.strip_prefix("# ") {
                return title.trim().replace('|', "/");
            }
            if !t.is_empty() {
                break;
            }
        }
    }
    // 找不到角色檔：代理團隊把檔名轉成 Title Case，聊天室照舊版回檔名本身
    if lib.chat {
        key.to_string()
    } else {
        title_case(key)
    }
}

/// `software-engineer` → `Software Engineer`。
fn title_case(key: &str) -> String {
    key.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(c) => c.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 開組時清空這個組號的成品資料夾（上次同組號留下的舊檔）。
pub fn clear_session(lib: &Library, data_dir: &Path, team_number: u32) {
    let dir = session_dir(lib, data_dir, team_number);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// 讀 `common.md`／角色檔。不是 UTF-8（使用者用 Big5／UTF-16 存）時照樣解碼，
/// 不能像 `read_to_string` 那樣整層靜靜消失；BOM 也要去掉，否則組好的檔中間夾一個 BOM（G10）。
fn read_or_empty(path: &Path) -> String {
    match super::message::read_text(path) {
        Ok((text, enc)) => {
            if enc != "UTF-8" && enc != "UTF-8 (BOM)" {
                println!(
                    "[AwayTerminal] 角色檔 {} 不是 UTF-8，以 {enc} 解碼",
                    path.display()
                );
            }
            text
        }
        Err(_) => String::new(),
    }
}

/// 組合一個 agent 的角色檔（UTF-8 無 BOM）→ 回傳絕對路徑。
/// 隊友名單＝組內「已啟用」的格，所以呼叫端要先把名單填好再一次全部組。
pub fn compose(
    lib: &Library,
    data_dir: &Path,
    team: &Team,
    slot_index: u32,
) -> std::io::Result<PathBuf> {
    ensure_defaults(lib, data_dir);
    let me = team
        .slots
        .iter()
        .find(|s| s.index == slot_index)
        .ok_or_else(|| std::io::Error::other("slot not found"))?;

    let mut out = String::new();
    out.push_str(read_or_empty(&common_path(lib, data_dir)).trim_end());
    out.push_str("\n\n");
    if !me.role.trim().is_empty() {
        let role_text = read_or_empty(&roles_dir(lib, data_dir).join(format!("{}.md", me.role)));
        let role_text = role_text.trim_end();
        if !role_text.is_empty() {
            out.push_str("---\n\n");
            out.push_str(role_text);
            out.push_str("\n\n");
        }
    }
    out.push_str("---\n\n");
    // 第三層：代理團隊與聊天室各有自己的執行期脈絡（內容完全不同）
    out.push_str(&if lib.chat {
        super::chat::runtime_context(team, me)
    } else {
        runtime_context(team, me)
    });

    let dir = session_dir(lib, data_dir, team.number);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.md", me.agent_id()));
    std::fs::write(&path, out)?;
    Ok(path)
}

/// 第三層：執行期脈絡。
///
/// **逐字照舊版 `RoleLibrary.RuntimeContext`**，只換執行時的值；這份文字是四家 CLI
/// 共同的行為規格，改一個字都可能讓 agent 走鐘（每一段下面的舊版註解說明它是哪一次
/// 實錄加上去的）。英文照舊版——這是給 agent 讀的，不走八語 UI。
///
/// 投遞那一行的範例故意用**目前介面語言**的 `ma.deliverOne`，和真的打進終端機的一致。
pub fn runtime_context(team: &Team, me: &Slot) -> String {
    let enabled: Vec<&Slot> = team.slots.iter().filter(|s| s.enabled).collect();
    let has_worker = enabled.iter().any(|x| x.index != me.index);
    let w1 = enabled
        .iter()
        .map(|x| x.agent_id().chars().count())
        .max()
        .unwrap_or(0);
    let w2 = enabled
        .iter()
        .map(|x| x.role_title.chars().count())
        .max()
        .unwrap_or(0);

    let mut sb = String::new();
    sb.push_str("# Runtime Context (generated by AwayTerminal)\n\n");
    sb.push_str(&format!("Agent ID: {}\n", me.agent_id()));
    sb.push_str(&format!(
        "Role: {}\n",
        if me.role.trim().is_empty() {
            "None (general assistant)".to_string()
        } else {
            me.role_title.clone()
        }
    ));
    sb.push_str(&format!("Provider: {}\n", me.backend_name()));
    sb.push_str(&format!("Team session: MAS-{}\n", team.number));
    // 有沙盒時信箱在 worktree 裡（`bus::MessageBus::new(&work_dir)`）：這裡寫原始 repo，
    // agent 照字面組路徑就會把信寫進原始 repo 的 `.ai/bus/`，永遠收不到（G5）。
    // 沒沙盒時 work_dir＝dir，和舊版逐字相同。
    sb.push_str(&format!("Project directory: {}\n", team.work_dir));
    sb.push_str("Enabled agents:\n");
    for x in &enabled {
        sb.push_str(&format!(
            "  - {}  {}  ({}){}\n",
            pad(&x.agent_id(), w1),
            pad(&x.role_title, w2),
            x.backend_name(),
            if x.index == me.index { "   <- you" } else { "" }
        ));
    }
    if !has_worker {
        sb.push_str("You are the only enabled agent (Solo Mode).\n");
    }
    sb.push_str(&format!(
        "Mailbox: {BUS_REL_DIR}/  (relative to the project directory)\n\n"
    ));

    // 舊版 1.2.0 實測：Codex（gpt-5.6-sol）內建「你是 /root，可以 spawn_agent 開子代理」的
    // 提示詞，使用者說「讓 Agent-12 做…」時它直接開了名叫 /root/agent_12 的子代理，信箱完全
    // 沒用到；--disable multi_agent、agents.max_depth 等設定都拿不掉那些工具 → 只能在這裡講清楚。
    // Claude Code 的 Task 工具同理。
    sb.push_str("## Your teammates are separate terminals, not sub-agents\n\n");
    sb.push_str("Every other agent listed above is an independent coding-agent session running in its own AwayTerminal pane.\n");
    sb.push_str("They are NOT your sub-agents and you cannot reach them with any built-in tool.\n\n");
    sb.push_str("- \"Have Agent-xx do something\" / \"let Agent-xx handle it\" always means: write a mailbox message to that agent (below).\n");
    sb.push_str("- Do not use built-in sub-agent or collaboration tools in this team session at all: no spawn_agent, followup_task,\n");
    sb.push_str("  send_message, wait_agent, list_agents, interrupt_agent, no Task/sub-agent tool, and never create a sub-agent named after a teammate.\n");
    sb.push_str("- Do not wait, sleep or poll for replies. End your turn; AwayTerminal types a notice into your terminal when a reply arrives.\n\n");

    sb.push_str("## How to send a message\n\n");
    sb.push_str(&format!(
        "Write ONE new file {BUS_REL_DIR}/NNNN-{}-to-<recipient id>.md where NNNN is\n",
        me.agent_id()
    ));
    sb.push_str("(the highest existing NNNN in that folder) + 1, zero-padded to 4 digits. Start the file with a\n");
    sb.push_str("YAML front matter block, then write the body in Markdown:\n\n");
    sb.push_str("```\n---\n");
    sb.push_str(&format!("from: {}\n", me.agent_id()));
    let sample_to = enabled
        .iter()
        .find(|x| x.index != me.index)
        .map(|x| x.agent_id())
        .unwrap_or_else(|| me.agent_id());
    sb.push_str(&format!("to: {sample_to}\n"));
    sb.push_str("type: TASK_RESULT        # TASK | TASK_RESULT | QUESTION | ANSWER | REVIEW_REQUEST | REVIEW_RESULT | BLOCKED | INFO\n");
    sb.push_str("task: TASK-001\n");
    sb.push_str("status: completed        # completed | failed | blocked | stopped | paused | pass | fail  (only for results)\n");
    sb.push_str("files_changed:\n  - path/to/file\n");
    sb.push_str("---\n(body)\n```\n\n");
    sb.push_str(&format!(
        "Never edit or delete an existing message file. Do not write anything else into {BUS_REL_DIR}/.\n"
    ));
    sb.push_str("Writing the file is the whole act of sending: after writing it, end your turn; AwayTerminal delivers it.\n\n");

    // 舊版使用者回報（2026-09-15）：PM 寄了暫停信就跟使用者說「已通知暫停」，
    // 但收件人正在工作、信在排隊，根本沒停。
    sb.push_str("### Delivery timing\n\n");
    sb.push_str("AwayTerminal types a message into the recipient's terminal only when the recipient is idle. While the recipient is\n");
    sb.push_str("working, the message waits in a queue and is delivered after it finishes its current work. A message therefore\n");
    sb.push_str("cannot interrupt or stop an agent that is working.\n\n");
    sb.push_str(&format!(
        "- If the user wants agents to stop right away, tell the user to right-click the team tab and choose \"{}\"\n",
        crate::i18n::t("ma.menuStop")
    ));
    sb.push_str("  (it interrupts every agent), or to press Esc in that agent's pane. Do not claim that a message has stopped anyone.\n");
    sb.push_str("- When you report a message you sent, say it was sent and will be read when the recipient is idle. Do not say the\n");
    sb.push_str("  recipient has received it, been notified, or acted on it until its reply arrives.\n\n");

    sb.push_str("## How you receive messages\n\n");
    sb.push_str("AwayTerminal types a line like this into your terminal:\n\n");
    let example_from = enabled
        .iter()
        .find(|x| x.index != me.index)
        .map(|x| x.agent_id())
        .unwrap_or_else(|| format!("Agent-{}1", team.number));
    sb.push_str("    ");
    sb.push_str(&crate::i18n::tf(
        "ma.deliverOne",
        &[
            "7",
            &example_from,
            "TASK-001",
            "TASK",
            &format!("{BUS_REL_DIR}/0007-{example_from}-to-{}.md", me.agent_id()),
        ],
    ));
    sb.push_str("\n\n");
    sb.push_str("Read that file, act according to your role, and reply by writing a new message file.\n");
    sb.push_str(&format!(
        "You may read any file in {BUS_REL_DIR}/ for context.\n"
    ));
    // 舊版 D（使用者選，2026-09-15 實錄）：派 TASK-003 的信因為收件人一直忙、排了 23 分鐘才送，
    // 工程師早就自己讀信箱做完了，晚到的投遞又讓它多花一輪重看。
    sb.push_str("A message can be delivered long after it was written. If AwayTerminal delivers a message you have already read and\n");
    sb.push_str("handled (for example you found it in the mailbox yourself), say so in one line and continue. Do not redo the work and do\n");
    sb.push_str("not reply to it again.\n");
    // 舊版 1.2.0 實測：PowerShell 5.1 的 Get-Content 預設用系統字碼頁讀，中文訊息變亂碼
    //（Codex 在 Windows 預設 shell 就是它）。
    sb.push_str("Message files are UTF-8. Read and write them as UTF-8 (in Windows PowerShell use Get-Content -Raw -Encoding UTF8 <file>).\n\n");

    // 舊版使用者要求（2026-09-15）：格 2～4 的任務不管是完成、失敗、被停止、被暫停，
    // 一律回報給格 1（下方全寬、使用者對話的那一格）。「停止任務」打進去的那一句不是從信箱
    // 來的，也要回報——否則 Agent-x1 不知道各格停在哪。
    let lead = &team.slots[0];
    if me.index != lead.index && lead.enabled {
        sb.push_str(&format!("## Always report to {}\n\n", lead.agent_id()));
        sb.push_str(&format!(
            "{} ({}) coordinates this team and is the agent the user talks to.\n",
            lead.agent_id(),
            lead.role_title
        ));
        sb.push_str("Whenever a task you are working on ends for any reason - completed, failed, blocked, stopped, interrupted or paused,\n");
        sb.push_str("including when the user or AwayTerminal tells you to stop or pause, and even if the task did not come from the mailbox -\n");
        sb.push_str(&format!(
            "write a message to {} with what happened, the current state and what is left to do.\n",
            lead.agent_id()
        ));
        sb.push_str("Use type TASK_RESULT (status completed or failed) for finished work, BLOCKED when you cannot continue,\n");
        sb.push_str("and INFO with status stopped or paused when you were told to stop or pause.\n\n");
    } else if has_worker {
        sb.push_str("## Reports from your teammates\n\n");
        sb.push_str("The other agents report to you whenever one of their tasks ends, fails, is blocked, or is stopped or paused.\n");
        sb.push_str("Use those reports to keep track of where each agent is.\n\n");
    }

    // 舊版使用者要求（2026-09-15）：有開 UI/UX Designer 時，有畫面的任務一律先找設計師。
    // 實錄：使用者對 PM 說「用 C# 寫一個貪食蛇遊戲」，PM 直接派給工程師（它的角色檔是舊版、
    // 沒提設計師）。寫在執行期脈絡而不是只改 product-manager.md：資料目錄裡的舊角色檔不一定
    // 會更新，而且只有真的有開設計師才講。
    let designer = enabled
        .iter()
        .find(|x| x.index != me.index && x.role == "ui-ux-designer");
    if let Some(d) = designer {
        if me.role == "product-manager" || me.index == lead.index {
            sb.push_str("## Design before UI implementation\n\n");
            sb.push_str(&format!(
                "{} (UI/UX Designer) is enabled. Every task that has a user interface - new or changed windows, screens,\n",
                d.agent_id()
            ));
            sb.push_str(&format!(
                "layouts, controls or visual style - first gets a design from {}: assign the design task, wait for the result,\n",
                d.agent_id()
            ));
            sb.push_str("then give the design to the agent that implements it. This applies to small tasks too.\n");
            sb.push_str("Skip the design step only when the change has no visible UI or the user asks to skip it.\n\n");
        }
    }

    // 舊版 B、C（使用者選，2026-09-15 實錄：貪食蛇小遊戲三個 agent 花 45 分鐘以上——工程師的
    // GUI 驅動測試一直被別的視窗搶走焦點、失焦自動暫停，14 分鐘都在修測試腳本；三個終端機
    // ＋使用者共用同一個桌面，這種測試在多代理下注定不穩）。
    let ask_user = if me.index == lead.index || !lead.enabled {
        "ask the user".to_string()
    } else {
        format!("ask {} to ask the user", lead.agent_id())
    };
    sb.push_str("## Shared desktop\n\n");
    sb.push_str(&format!(
        "All agents and the user share one {} desktop, so any window can lose focus at any moment.\n",
        desktop_name()
    ));
    sb.push_str("Do not run checks that need the foreground window or keyboard focus: no GUI automation that sends keystrokes or clicks\n");
    sb.push_str("to windows, no bringing windows to the front, no capturing live application windows, no tests that depend on a window\n");
    sb.push_str("keeping focus. Prefer command-line and headless checks.\n");
    sb.push_str(&format!(
        "If something can only be verified by looking at a window, {ask_user} to check it.\n\n"
    ));

    let report_stuck = if me.index == lead.index || !lead.enabled {
        "tell the user what you tried, what you found and what you need".to_string()
    } else {
        format!(
            "report BLOCKED to {} with what you tried, what you found and what you need",
            lead.agent_id()
        )
    };
    sb.push_str("## When you are stuck\n\n");
    sb.push_str("If the same problem is still unsolved after two different attempts, or after about 10 minutes, stop working on it.\n");
    sb.push_str(&format!(
        "Do not build more tools, scripts or test harnesses around it; {report_stuck}.\n\n"
    ));

    sb.push_str("## Talking to the user\n\n");
    // 舊版 1.2.0 實測：「叫 Agent-12 顯示 123」→ worker 只把 123 寫進回信，自己的畫面沒顯示，
    // 使用者在那格看不到。
    sb.push_str(&format!(
        "Only {} takes requests from the user and asks the user questions; the other agents report to {}\n",
        lead.agent_id(),
        lead.agent_id()
    ));
    sb.push_str("with a message file. The user can still see every agent's terminal, so a worker also shows its work in its own terminal:\n");
    sb.push_str("when a task asks you to show, print or display something, output it in your terminal reply as well as in your result message.\n");
    sb.push_str("Write terminal replies in the user's language.\n");
    sb
}

/// 舊版是 WPF、只有 Windows，那一句寫死「one Windows desktop」。跨平台版照平台換字。
fn desktop_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}

/// `String::PadRight`（依**字元數**補空白，不是位元組）。
fn pad(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 測試用的 `%TEMP%` 資料夾，**drop 就刪掉**。
    ///
    /// 為什麼要 Drop 而不是在測試最後一行刪：assert 失敗時那一行跑不到，
    /// 資料夾就留在 `%TEMP%` 裡（TASK-017 留下兩個 `awayterm-roles-compose-*`，
    /// PM 在收尾清單裡抓到）。
    struct TempDir(PathBuf);

    impl std::ops::Deref for TempDir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "awayterm-roles-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn team_of(number: u32, dir: &str) -> Team {
        let mut t = Team::new("k", number, dir);
        t.slots[0].enabled = true;
        t.slots[0].role = "product-manager".to_string();
        t.slots[0].role_title = "Product Manager".to_string();
        t.slots[0].backend = "claude-code".to_string();
        t.slots[1].enabled = true;
        t.slots[1].role = "software-engineer".to_string();
        t.slots[1].role_title = "Software Engineer".to_string();
        t.slots[1].backend = "claude-code".to_string();
        t
    }

    /// 範本補齊：六個檔＋`.defaults.json`。
    #[test]
    fn writes_the_built_in_templates() {
        let dir = temp_dir("ensure");
        ensure_defaults(&TEAM, &dir);
        assert!(common_path(&TEAM, &dir).is_file());
        for r in BUILT_IN_ROLES {
            assert!(
                roles_dir(&TEAM, &dir).join(format!("{r}.md")).is_file(),
                "缺 {r}.md"
            );
        }
        let manifest = load_manifest(&TEAM, &dir).unwrap();
        assert_eq!(manifest.len(), TEAM_TEMPLATES.len());
    }

    /// 使用者改過的檔不動；`restore_defaults` 才覆寫回去。
    #[test]
    fn keeps_user_edits_until_restore() {
        let dir = temp_dir("edits");
        ensure_defaults(&TEAM, &dir);
        let mine = roles_dir(&TEAM, &dir).join("qa-engineer.md");
        std::fs::write(&mine, "# My QA\n\n我自己改的\n").unwrap();
        ensure_defaults(&TEAM, &dir);
        assert!(
            std::fs::read_to_string(&mine).unwrap().contains("我自己改的"),
            "使用者改過的角色檔被蓋掉了"
        );
        assert_eq!(title_of(&TEAM, &dir, "qa-engineer"), "My QA");
        restore_defaults(&TEAM, &dir);
        assert!(!std::fs::read_to_string(&mine).unwrap().contains("我自己改的"));
    }

    /// 「只是舊版、使用者沒改過」→ 換成新版（舊版 `.defaults.json` 的用途）。
    #[test]
    fn updates_untouched_old_templates() {
        let dir = temp_dir("stale");
        ensure_defaults(&TEAM, &dir);
        // 假裝上一版的內容：檔案改掉，同時把記錄也改成「AwayTerminal 寫的就是這一份」
        let target = roles_dir(&TEAM, &dir).join("qa-engineer.md");
        let old_body = "# QA Engineer\n\n上一版\n";
        std::fs::write(&target, old_body).unwrap();
        let mut m = load_manifest(&TEAM, &dir).unwrap();
        m.insert(
            "roles/qa-engineer.md".to_string(),
            normalized_hash(old_body.as_bytes()),
        );
        save_manifest(&TEAM, &dir, &m);

        ensure_defaults(&TEAM, &dir);
        assert!(
            !std::fs::read_to_string(&target).unwrap().contains("上一版"),
            "沒改過的舊版範本應該被換成新版"
        );
    }

    /// CRLF 只差在換行不算「使用者改過」（正規化雜湊）。
    #[test]
    fn crlf_only_difference_is_not_an_edit() {
        let body = "# A\n\nline\n";
        assert_eq!(
            normalized_hash(body.as_bytes()),
            normalized_hash(body.replace('\n', "\r\n").as_bytes())
        );
        assert_eq!(
            normalized_hash(body.as_bytes()),
            normalized_hash(format!("\u{feff}{body}").as_bytes()),
            "BOM 也要先去掉"
        );
    }

    /// 角色標題取第一個 `# ` 標題；沒有角色＝`None`。
    #[test]
    fn reads_role_titles() {
        let dir = temp_dir("titles");
        assert_eq!(title_of(&TEAM, &dir, "software-engineer"), "Software Engineer");
        assert_eq!(title_of(&TEAM, &dir, "qa-engineer"), "QA Engineer");
        assert_eq!(title_of(&TEAM, &dir, "ui-ux-designer"), "UI/UX Designer");
        assert_eq!(title_of(&TEAM, &dir, ""), "None");
        // 沒有這個檔＝檔名轉 Title Case
        assert_eq!(title_of(&TEAM, &dir, "my-own-role"), "My Own Role");
    }

    /// 下拉順序：內建五個照舊版順序（設計師在 QA 前），自訂的排後面。
    #[test]
    fn lists_built_in_roles_first() {
        let dir = temp_dir("list");
        ensure_defaults(&TEAM, &dir);
        std::fs::write(roles_dir(&TEAM, &dir).join("aaa-custom.md"), "# Custom\n").unwrap();
        let keys: Vec<String> = list_roles(&TEAM, &dir).into_iter().map(|r| r.key).collect();
        assert_eq!(
            keys,
            vec![
                "product-manager",
                "software-engineer",
                "software-architect",
                "ui-ux-designer",
                "qa-engineer",
                "aaa-custom"
            ]
        );
    }

    /// 三層組合：common → 角色 → 執行期脈絡，各層之間一條 `---`。
    #[test]
    fn composes_three_layers() {
        let dir = temp_dir("compose");
        let team = team_of(3, "C:\\proj");
        let path = compose(&TEAM, &dir, &team, 2).unwrap();
        assert!(path.ends_with("Agent-32.md"));
        // 範本檔本身是 CRLF（照舊版原樣寫出去，不動它）→ 比對前統一換行
        let text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        assert!(text.starts_with("# AwayTerminal Multi-Agent Common Rules"));
        assert!(text.contains("\n---\n\n# Software Engineer\n"));
        assert!(text.contains("\n---\n\n# Runtime Context (generated by AwayTerminal)\n"));
        assert!(text.contains("Agent ID: Agent-32"));
        // 成品資料夾照組號分開
        assert!(path.parent().unwrap().ends_with("3"));
    }

    /// `clear_session` 只清自己組號那一個資料夾。
    #[test]
    fn clears_only_its_own_session_dir() {
        let dir = temp_dir("clear");
        let t3 = team_of(3, "C:\\p");
        let mut t4 = team_of(4, "C:\\p");
        t4.number = 4;
        for s in t4.slots.iter_mut() {
            s.team_number = 4;
        }
        compose(&TEAM, &dir, &t3, 1).unwrap();
        let keep = compose(&TEAM, &dir, &t4, 1).unwrap();
        clear_session(&TEAM, &dir, 3);
        assert!(!session_dir(&TEAM, &dir, 3).join("Agent-31.md").exists());
        assert!(keep.is_file(), "別組的成品不能被清掉");
    }

    /// 執行期脈絡：**和舊版模板逐字比對**（fixture 在 `resources/multiagent/runtime-context.txt`）。
    #[test]
    fn runtime_context_matches_the_v1_template() {
        let expected = include_str!("../../resources/multiagent/runtime-context.txt");
        // fixture 是從**舊版真的產生出來的**那一份抓出來的（v1.2.8 執行中的 Agent-12.md），
        // 只把裡面的專案路徑換成中性路徑——那是執行時的值，兩邊用同一個字串就不影響
        // 「逐字比對」的意義（TASK-018：原本的 fixture 含使用者名稱）。
        let team = team_of(1, "C:\\Projects\\Example");
        let me = &team.slots[1];
        let got = runtime_context(&team, me);
        // fixture 是 LF，比對前把換行統一；fixture 來自只有 Windows 的舊版，
        // 「one Windows desktop」那一句在別的平台會換成 `desktop_name()`
        let expected = expected.replace("one Windows desktop", &format!("one {} desktop", desktop_name()));
        assert_eq!(
            got.replace("\r\n", "\n"),
            expected.replace("\r\n", "\n"),
            "執行期脈絡和舊版模板不一樣了"
        );
    }

    /// Solo Mode：只有一格啟用時多一行，而且不會有「Always report to」那段。
    #[test]
    fn solo_mode_says_so() {
        let mut team = team_of(1, "C:\\p");
        team.slots[1].enabled = false;
        let text = runtime_context(&team, &team.slots[0]);
        assert!(text.contains("You are the only enabled agent (Solo Mode).\n"));
        assert!(!text.contains("## Always report to"));
        assert!(!text.contains("## Reports from your teammates"));
        assert!(text.contains("ask the user to check it."));
        assert!(text.contains("tell the user what you tried"));
    }

    /// 有開設計師時，PM／格 1 多「Design before UI implementation」那段（設計師自己沒有）。
    #[test]
    fn mentions_the_designer_only_to_the_lead() {
        let mut team = team_of(1, "C:\\p");
        team.slots[2].enabled = true;
        team.slots[2].role = "ui-ux-designer".to_string();
        team.slots[2].role_title = "UI/UX Designer".to_string();
        team.slots[2].backend = "codex".to_string();
        let pm = runtime_context(&team, &team.slots[0]);
        assert!(pm.contains("## Design before UI implementation"));
        assert!(pm.contains("Agent-13 (UI/UX Designer) is enabled."));
        let se = runtime_context(&team, &team.slots[1]);
        assert!(!se.contains("## Design before UI implementation"));
        let designer = runtime_context(&team, &team.slots[2]);
        assert!(!designer.contains("## Design before UI implementation"));
    }

    /// 名單欄位對齊照 `PadRight`（依字元數）。
    #[test]
    fn pads_the_roster_columns() {
        let team = team_of(1, "C:\\p");
        let text = runtime_context(&team, &team.slots[0]);
        assert!(text.contains("  - Agent-11  Product Manager    (ClaudeCode)   <- you\n"));
        assert!(text.contains("  - Agent-12  Software Engineer  (ClaudeCode)\n"));
    }

    /// 角色檔帶 BOM 或不是 UTF-8：標題照樣讀到、組檔時角色層不消失、沒有夾 BOM（G10）。
    #[test]
    fn reads_role_files_with_bom_or_legacy_encoding() {
        let dir = temp_dir("bom");
        ensure_defaults(&TEAM, &dir);
        let roles = roles_dir(&TEAM, &dir);
        std::fs::write(roles.join("bom-role.md"), "\u{feff}# 帶 BOM 的角色\n\n內容 A\n").unwrap();
        assert_eq!(title_of(&TEAM, &dir, "bom-role"), "帶 BOM 的角色");

        // UTF-16 LE（記事本「Unicode」）
        let mut le = vec![0xFF, 0xFE];
        le.extend("# UTF16 Role\n\nbody\n".encode_utf16().flat_map(|u| u.to_le_bytes()));
        std::fs::write(roles.join("utf16-role.md"), &le).unwrap();
        assert_eq!(title_of(&TEAM, &dir, "utf16-role"), "UTF16 Role");

        // Big5：組檔時角色層要在（以前 read_to_string 失敗＝整層靜靜消失）
        let mut big5 = b"# Big5 Role\n\n".to_vec();
        big5.extend([0xA7, 0xB9, 0xA6, 0xA8, 0xA4, 0x46]); // 完成了
        std::fs::write(roles.join("big5-role.md"), &big5).unwrap();
        let mut team = team_of(7, "C:\\p");
        team.slots[1].role = "big5-role".to_string();
        let p = compose(&TEAM, &dir, &team, 2).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# Big5 Role"));
        assert!(text.contains("完成了"));
        assert!(!text.contains('\u{feff}'));
    }

    /// 有沙盒時執行期脈絡寫 worktree（信箱真正所在的地方），不是原始 repo（G5）。
    #[test]
    fn runtime_context_points_at_the_work_dir() {
        let mut team = team_of(1, "C:\\repo");
        team.work_dir = "C:\\repo\\.ai\\sandbox\\x".to_string();
        let text = runtime_context(&team, &team.slots[0]);
        assert!(text.contains("Project directory: C:\\repo\\.ai\\sandbox\\x\n"));
    }
}
