//! Rust 端使用者可見字串的取得方式。
//!
//! ## 一份翻譯，不是兩份
//!
//! 八種語言的字串在**前端**（`src/lang/<代碼>.js`）。Rust 端**不存八種語言**：
//! 前端啟動時與切語言時把「Rust 會用到的那幾十條」推過來（[`i18n_push`]），
//! 這裡存成一張 runtime 的表；推之前（啟動最早期）用下面內建的繁中／英文當後備。
//!
//! ### 為什麼不是「Rust 回代碼、前端查表」（PM 在 TASK-015 修訂版要求的做法）
//!
//! 一部分做得到、一部分做不到，所以整個改成「推字串進來」：
//!
//! 1. **做不到的**：**直接寫進終端機畫面的訊息**（重連倒數、SSH 的「連線到 …」、
//!    巨集結束提示、COM 降級警告、恢復分頁的分隔行）。那些是背景執行緒把**位元組**
//!    寫進 pane，和 PTY 的輸出走同一條路——前端拿到的是終端機內容，沒有機會查表。
//! 2. **回代碼的風險**：漏改一處，使用者就看到 `err.connNotFound` 這種字（等於壞掉）；
//!    這個做法漏改一處只是那一句退回內建的繁中／英文（難看但看得懂）。
//! 3. **有些訊息帶作業系統的原文**（`開啟 COM5 失敗：系統找不到指定的檔案。`），
//!    前端查表也翻不了後半段——參數化之後仍然是原文。
//!
//! 兩種做法的共同目標「翻譯只有一份」都達到了：`i18n_keys()` 回報 Rust 需要哪些 key，
//! `scripts/test-i18n.mjs` 會檢查前端的表裡都有，所以不會漏。
//!
//! 另外 [`crate::ttl::error`] 本來就有 `message()`（英文，照 `errdlg.cpp`）與
//! `message_zh()` 兩份，TTL 的 22 條錯誤訊息照語言挑一個就好（`message_for_lang`）。
//!
//! ## 邊界：只處理「使用者看得到的」
//!
//! | 種類 | 處理 | 為什麼 |
//! |---|---|---|
//! | `Err(...)` 回給前端的（對話框／提示） | ✅ 查表 | 使用者直接看到 |
//! | 寫進終端機畫面的（重連倒數、SSH 狀態、巨集結束） | ✅ 查表 | 同上 |
//! | 檔案選擇／存檔對話框的標題與篩選器 | ✅ 查表 | 同上 |
//! | `T{json}`（搜尋列、代理狀態標籤） | ✅ 查表 | 前端直接顯示 |
//! | `println!("[AwayTerminal] …")` | ❌ 不翻 | **開發診斷**：打包後的 app 沒有 stdout；`--verify` 與踩雷紀錄都在比對這些字串 |
//! | `--verify` 專用的訊息 | ❌ 不翻 | 只有開發時跑得到 |
//!
//! 清單（哪個檔有幾條）在 `docs/SETTINGS.md`，由 `scripts/i18n-audit.mjs` 重新產生。
//!
//! ## 用法
//!
//! ```ignore
//! use crate::i18n::{t, tf};
//! return Err(t("err.needHost").to_string());
//! return Err(tf("err.connNotFound", &[name]));
//! ```
//!
//! 參數用 `{0}`／`{1}`（同舊版 `string.Format`，也同前端 `strings.js` 的 `fmt()`）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use tauri::Manager;

/// 目前語言（只分「內建後備要用哪一份」）。`0` ＝繁中，`1` ＝英文。
///
/// 放成 process 全域的原子變數，因為寫進終端機畫面的訊息是在**背景執行緒**
/// （PTY 讀取、重連、巨集）產生的，拿不到 tauri 的 `State`。
static LANG: AtomicU8 = AtomicU8::new(0);

/// 前端推過來的字串（key → 已經是目前語言的字）。
fn pushed() -> &'static Mutex<HashMap<String, String>> {
    static P: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 設定內建後備要用哪一種語言。
///
/// `zh-TW`／`zh` 以外的中文（`zh-CN`…）也走繁中那一份——只是後備，
/// 真正的簡中字串由前端推過來。非中文一律用英文那一份。
pub fn set_lang(code: &str) {
    let zh = code.starts_with("zh");
    LANG.store(u8::from(!zh), Ordering::Relaxed);
}

/// 內建後備現在是英文嗎（`ttl::error::message_for_lang` 也用這個）。
pub fn is_en() -> bool {
    LANG.load(Ordering::Relaxed) == 1
}

/// 查一個字串：**前端推過來的優先**，沒有就用內建的繁中／英文，都沒有就回 key 本身。
///
/// 回 `String` 而不是 `&'static str`：推過來的字是 runtime 才有的。
pub fn t(key: &str) -> String {
    if let Some(v) = pushed().lock().ok().and_then(|m| m.get(key).cloned()) {
        if !v.is_empty() {
            return v;
        }
    }
    match TABLE.iter().find(|(k, _, _)| *k == key) {
        Some((_, zh, en)) => (if is_en() { *en } else { *zh }).to_string(),
        None => {
            debug_assert!(false, "i18n: 沒有這個 key：{key}");
            key.to_string()
        }
    }
}

/// 查一個字串並填入參數（`{0}`／`{1}`…）。
pub fn tf(key: &str, args: &[&str]) -> String {
    fill(&t(key), args)
}

/// `{0}`／`{1}`… 的取代（同舊版 `string.Format`、前端 `fmt()`）。
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = template.to_string();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

/// 支援的介面語言（順序＝設定視窗下拉的順序，和前端 `strings.js` 的 `LANGS` 一致）。
///
/// ⚠️ 加語言時**兩邊都要加**：這裡與 `src/lang/`＋`strings.js` 的 `LANGS`。
/// `scripts/test-i18n.mjs` 會比對兩邊（前端表缺 key 會叫）。
pub const LANGS: &[&str] = &["zh-TW", "en", "zh-CN", "ja", "ko", "es", "de", "fr"];

/// 這是我們支援的語言代碼嗎（`zh` 是舊設定檔的寫法，當成 `zh-TW`）。
pub fn is_supported_lang(code: &str) -> bool {
    code == "zh" || LANGS.contains(&code)
}

/// 系統語言（例如 `zh-Hant-TW`、`ja-JP`）。
///
/// **只在第一次啟動時用**：前端把它對到支援的八種語言（`strings.js` 的 `matchLang`），
/// 對不到就用 `en`。使用者在設定視窗改過之後就固定，不再看系統。
/// 舊版沒有這個行為（舊版預設一律繁中）→ `docs/SETTINGS.md` 標為**新增**。
#[tauri::command]
pub fn system_locale() -> String {
    sys_locale::get_locale().unwrap_or_default()
}

/// Rust 端會用到的 key（前端照這份清單推字串過來）。
#[tauri::command]
pub fn i18n_keys() -> Vec<&'static str> {
    TABLE.iter().map(|(k, _, _)| *k).collect()
}

/// 前端把「已經翻好的字」推過來（啟動時與切語言時各一次）。
///
/// `lang` 只用來決定內建後備要用哪一份；`strings` 是 key → 目前語言的字。
///
/// ⚠️ 推完要**重送 `T{json}`**：那包 JSON 裡有搜尋列與代理狀態的字，
/// 它們是 `theme_json()` 用 `t()` 取的 → 不重送的話搜尋列會留在上一個語言
///（`--verify` 抓到過：切成英文之後搜尋列還是「搜尋」）。
#[tauri::command]
pub fn i18n_push(
    app: tauri::AppHandle,
    lang: String,
    strings: HashMap<String, String>,
) -> usize {
    let n = set_pushed(&lang, strings);
    if let Some(store) = app.try_state::<std::sync::Arc<crate::settings::SettingsStore>>() {
        let theme = store.get().theme_json();
        crate::host::emit_host(&app, format!("T{theme}"));
    }
    println!("[AwayTerminal] i18n：語言={lang}，前端推了 {n} 條字串過來（已重送 T{{json}}）");
    n
}

/// [`i18n_push`] 的本體（不含 tauri 的部分，測試用這個）。
pub fn set_pushed(lang: &str, strings: HashMap<String, String>) -> usize {
    set_lang(lang);
    let n = strings.len();
    if let Ok(mut m) = pushed().lock() {
        *m = strings;
    }
    n
}

/// `(key, 繁中, English)`。
///
/// 英文**優先照舊版 `Localization/Loc.cs` 的同名 key**（連標點與大小寫）；
/// 舊版沒有的（內建 SSH／Telnet／COM／TTL／沙盒都是新版才有的東西）才自己寫。
#[rustfmt::skip]
static TABLE: &[(&str, &str, &str)] = &[
    // ---------------- 連線建立（commands.rs）----------------
    ("err.sshNeedsParams",     "kind=ssh 需要 ssh 參數", "kind=ssh requires the ssh parameters"),
    ("err.telnetNeedsParams",  "kind=telnet 需要 telnet 參數", "kind=telnet requires the telnet parameters"),
    ("err.connNeedsName",      "kind=conn 需要 conn（連線名稱）", "kind=conn requires conn (the connection name)"),
    // 字型（TASK-033）：下載／匯入／移除。這些字也會出現在設定視窗的提示列上。
    ("font.noDir",             "找不到字型資料夾", "Font folder not found"),
    ("font.pickTitle",         "選擇字型檔", "Choose font files"),
    ("font.pickFilter",        "字型檔 (*.ttf, *.otf, *.ttc, *.otc)", "Font files (*.ttf, *.otf, *.ttc, *.otc)"),
    ("font.notOurs",           "只能讀程式自己的字型資料夾", "Only AwayTerminal's own font folders can be read"),
    ("font.notHttps",          "下載網址不是 https，已拒絕", "Download URL is not https; refused"),
    ("font.busy",              "這套字型正在下載中", "This font is already downloading"),
    ("font.cancelled",         "已取消下載", "Download cancelled"),
    ("font.unknownId",         "清單裡沒有這套字型：{0}", "No such font in the catalog: {0}"),
    ("font.badExt",            "不是字型檔（只收 .ttf／.otf／.ttc／.otc）：{0}", "Not a font file (.ttf/.otf/.ttc/.otc only): {0}"),
    ("font.notAFont",          "讀不出字型家族名，可能不是字型或是符號字型：{0}", "No font family name found; not a font, or a symbol font: {0}"),
    ("font.wrongFont",         "下載到的檔案不是 {0}，已丟棄", "The downloaded file is not {0}; discarded"),
    ("err.customNeedsCommand", "kind=custom 需要 command", "kind=custom requires command"),
    ("err.needHost",           "請輸入主機", "Please enter a host"),
    ("err.connNotFound",       "找不到自訂連線：{0}", "Custom connection not found: {0}"),
    ("err.commandNotFound",    "找不到指令：{0}", "Command not found: {0}"),
    ("err.exeMissing",         "執行檔不存在：{0}", "Executable does not exist: {0}"),
    ("err.unsupportedKind",    "尚未支援的連線種類：{0}", "Connection type not supported yet: {0}"),
    ("err.noPowerShell",       "找不到 pwsh.exe 或 powershell.exe（PATH 與 System32 都沒有）",
                               "Neither pwsh.exe nor powershell.exe was found (not on PATH, not in System32)"),
    ("err.noPowerShellVia",    "找不到 PowerShell（via_powershell 需要它）",
                               "PowerShell not found (via_powershell needs it)"),
    ("err.launchFailed",       "啟動 {0} 失敗：{1}", "Failed to start {0}: {1}"),
    ("err.tabCreateFailed",    "分頁建立失敗", "Could not create the tab"),
    ("err.tabNotFound",        "找不到分頁 {0}", "Tab {0} not found"),
    ("err.adbNotFound",        "找不到 adb", "adb was not found"),
    ("err.agentNeedsSlot",     "kind=agent 需要 agent（團隊與格號）", "kind=agent requires agent (the team and slot)"),
    ("err.tabNotFoundPlain",   "找不到分頁", "Tab not found"),
    ("err.tabsNotReady",       "分頁清單還沒準備好", "The tab list is not ready yet"),
    ("err.settingsNotReady",   "設定還沒準備好", "Settings are not ready yet"),
    ("err.connListNotReady",   "連線清單還沒準備好", "The connection list is not ready yet"),

    // ---------------- 分頁種類（tabs.rs）----------------
    ("kind.custom", "自訂連線", "Custom connection"),
    ("kind.com",    "連接埠", "Serial port"),

    // ---------------- 未接的舊協定（host.rs）----------------
    ("host.linkClicked",   "點了終端機裡的連結（開啟選單尚未實作）",
                           "A link in the terminal was clicked (the open menu is not implemented yet)"),
    ("host.mouseTakeover", "程式接管滑鼠提示（複製／選取功能尚未實作）",
                           "The app took over the mouse (copy/selection is not implemented yet)"),
    ("host.agentRatio",    "Multi-Agent 分隔線比例（代理團隊尚未實作）",
                           "Multi-Agent splitter ratio (agent teams are not implemented yet)"),
    ("host.unknown",       "未知", "unknown"),

    // ---------------- 檔案對話框（compose.rs／toolbar.rs）----------------
    ("dlg.loadTextFile",   "載入文字檔", "Load a text file"),
    ("dlg.save",           "儲存", "Save"),
    ("dlg.textFiles",      "文字檔", "Text files"),
    ("dlg.allFiles",       "所有檔案", "All files"),
    ("migrate.warnParity",     "同位檢查 {0} 在新版不支援（serialport 只有 None／Odd／Even），已改成 None",
                               "Parity {0} is not supported in v2 (serialport has only None/Odd/Even); changed to None"),
    ("migrate.warnStopBits",   "停止位元 1.5 在新版不支援，已改成 1",
                               "1.5 stop bits are not supported in v2; changed to 1"),
    ("migrate.warnFlow",       "流量控制 RTS/CTS+XON/XOFF 在新版不支援，已改成 RTS/CTS",
                               "Flow control RTS/CTS+XON/XOFF is not supported in v2; changed to RTS/CTS"),
    ("err.noExePath",          "拿不到執行檔路徑", "Could not determine the executable path"),
    ("err.registryWrite",      "寫登錄檔失敗（{0}）：{1}", "Could not write the registry ({0}): {1}"),
    ("err.registryDelete",     "刪登錄檔失敗（{0}）：{1}", "Could not delete the registry key ({0}): {1}"),
    ("err.oldSettingsBadJson", "舊版設定檔不是合法的 JSON：{0}", "The old settings file is not valid JSON: {0}"),
    ("err.noOldSettings",      "這個平台沒有舊版設定檔", "There is no v1 settings file on this platform"),
    ("migrate.pick",           "選舊版的 settings.json", "Choose the old settings.json"),
    ("dlg.pickMacro",      "選擇 TTL 巨集", "Choose a TTL macro"),
    ("dlg.teratermMacro",  "TeraTerm 巨集", "TeraTerm macro"),
    ("err.filePickFailed", "檔案選擇失敗：{0}", "The file dialog failed: {0}"),
    ("err.savePickFailed", "存檔對話框失敗：{0}", "The save dialog failed: {0}"),
    ("err.readFileFailed", "讀取檔案失敗：{0}", "Could not read the file: {0}"),
    ("err.saveFileFailed", "儲存檔案失敗：{0}", "Could not save the file: {0}"),
    ("err.writeFailed",    "寫入失敗：{0}", "Write failed: {0}"),
    ("err.readFailed",     "讀檔失敗：{0}", "Read failed: {0}"),

    // ---------------- 輸入文字（compose.rs）----------------
    // 上限的數字用參數帶進來（同舊版 `compose.loadTooBig` 的 {0}）
    ("compose.loadTooBig", "檔案太大（上限 {0} MB），未載入。",
                           "The file is too large (limit {0} MB); nothing was loaded."),
    ("compose.noTab",      "沒有分頁可送", "No tab to send to"),
    ("err.notBig5",        "這段文字沒辦法用 Big5 表示", "This text cannot be represented in Big5"),

    // ---------------- 我的最愛（favorites.rs）----------------
    ("fav.title",         "我的最愛", "Favorites"),
    ("fav.exists",        "已經在我的最愛裡了：{0}", "Already in favorites: {0}"),
    ("fav.nameTaken",     "已經有一筆叫「{0}」了", "There is already an entry named \"{0}\""),
    ("err.needName",      "請輸入名稱", "Please enter a name"),
    ("err.needExePath",   "請輸入執行檔路徑", "Please enter the path to the executable"),

    // ---------------- log（logging.rs／toolbar.rs）----------------
    ("err.needLogPath",   "請輸入 log 存檔位置。", "Please choose where to save the log."),
    ("err.logStartFailed", "無法開始記錄：{0}", "Could not start logging: {0}"),
    ("err.macroLogNoName", "巨集的 logopen 沒有給檔名", "The macro's logopen did not give a file name"),
    ("err.logOpenTimeout",
     "開啟 {0} 逾時（{1} 秒沒有反應）。這台機器的防毒／資料夾保護可能擋住了寫入，請把 AwayTerminal 加進例外，或把 log 位置換到別的資料夾。",
     "Opening {0} timed out ({1} s with no response). Antivirus or folder protection on this machine may be blocking the write - add AwayTerminal to its exclusions, or choose a different folder for logs."),

    // ---------------- 工具列（toolbar.rs）----------------
    ("err.saveFailed",    "存檔失敗：{0}", "Saving failed: {0}"),
    ("err.onlyHttp",      "只允許 http/https：{0}", "Only http/https is allowed: {0}"),
    ("err.tempOnlyPath",  "只接受暫存資料夾底下的路徑（{0}）",
                          "Only paths inside the temporary folder are accepted ({0})"),

    // ---------------- 搜尋列與代理狀態（settings.rs 的 `T{json}`）----------------
    ("search.placeholder", "搜尋", "Search"),
    ("search.prev",        "上一個 (Shift+Enter)", "Previous (Shift+Enter)"),
    ("search.next",        "下一個 (Enter)", "Next (Enter)"),
    ("search.close",       "關閉 (Esc)", "Close (Esc)"),
    ("ma.stateIdle",       "閒置", "idle"),
    ("ma.stateBusy",       "忙碌", "busy"),
    ("ma.stateQueued",     "有信待送", "mail queued"),
    ("ma.stateExited",     "已結束", "ended"),
    ("ma.stateBusyQueued", "忙碌 · 有信待送", "busy · mail queued"),

    // ---------------- 恢復分頁（restore.rs）----------------
    ("term.restoreSeparator", "──── 以上為上次關閉前的紀錄（{0}）────",
                              "──── above is the record from before the last exit ({0}) ────"),

    // ---------------- 斷線重連（reconnect.rs）----------------
    ("term.connEnded",     "[連線已結束]", "[connection closed]"),
    ("term.pressEnter",    "[按 Enter 在此分頁重新連線]", "[press Enter to reconnect in this tab]"),
    ("term.reconnectIn",   "[連線中斷，{0} 秒後自動重連…（關閉分頁可停止）]",
                           "[disconnected; reconnecting in {0} s... (close the tab to stop)]"),

    // ---------------- 沙盒（sandbox.rs／custom.rs）----------------
    ("err.sandboxDirFailed",  "建立沙盒目錄失敗：{0}", "Could not create the sandbox folder: {0}"),
    ("err.gitFailed",         "git 執行失敗（{0}）：{1}", "git failed ({0}): {1}"),
    ("err.notGitRepo",        "不在 git repo 裡", "Not inside a git repository"),
    ("err.tabNoSandbox",      "這個分頁沒有沙盒", "This tab has no sandbox"),
    ("err.sandboxNoWorktree", "這個沙盒沒有 worktree（不是 git repo），沒有東西要移除",
                              "This sandbox has no worktree (not a git repository); there is nothing to remove"),
    ("err.sandboxInUse",      "這個沙盒還有分頁在執行（代理團隊是整組共用）。請先結束分頁裡的程式（例如輸入 exit），再清除沙盒。",
                              "This sandbox is still in use by a running tab (agent teams share one). Exit the program in the tab first (e.g. type exit), then clear the sandbox."),
    ("err.sandboxLeftover",   "worktree 已從 git 移除，但資料夾刪不掉（可能還有程式開著裡面的檔案）：{0}\n{1}",
                              "The worktree was removed from git, but its folder could not be deleted (a program may still have files open in it): {0}\n{1}"),
    ("sb.guardNoNode",        "護欄未啟用：找不到 node，Claude Code 的指令護欄 hook 不會執行（沙盒的其他部分照常）",
                              "Guardrails inactive: node was not found, so the Claude Code command-guard hook will not run (the rest of the sandbox still works)"),
    ("err.connNameTaken",     "已經有一條叫「{0}」的自訂連線，請換一個名稱",
                              "A custom connection named \"{0}\" already exists; please choose another name"),

    // ---------------- 設定檔（settings.rs／migrate.rs）----------------
    ("err.settingsReadOnly",  "這次啟動時設定檔讀不進來，為了不蓋掉它，這次的變更都不會存檔：{0}\n請修好或移走這個檔案後重新啟動。",
                              "The settings file could not be read at startup, so to avoid overwriting it no changes will be saved this session: {0}\nFix or move the file away, then restart."),

    // ---------------- 離開時更新 CLAUDE.md（claudemd.rs）／關於（update.rs）----------------
    // BUG D3：這兩條以前不在表裡 → release 把字面 key 打進 Claude Code，debug 直接 panic
    ("exit.mdPrompt",         "請更新 CLAUDE.md，把這次工作的重點與變更記錄進去。",
                              "Please update CLAUDE.md to record this session's key changes."),
    ("about.noticesFail",     "讀不到 THIRD-PARTY-NOTICES.md", "Could not read THIRD-PARTY-NOTICES.md"),

    // ---------------- 字型下載（fontstore.rs）----------------
    ("font.tooLarge",         "下載的檔案超過 {0} MB，不像是字型，已中止",
                              "The download is larger than {0} MB and does not look like a font; aborted"),

    // ---------------- SSH（ssh/*.rs）----------------
    ("algo.kex",      "金鑰交換", "Key exchange"),
    ("algo.hostkey",  "主機金鑰", "Host key"),
    ("algo.cipher",   "加密", "Cipher"),
    ("algo.mac",      "訊息驗證", "MAC"),
    ("err.sshRuntime",        "建立 SSH runtime 失敗", "Could not create the SSH runtime"),
    ("term.sshCertUnsupported", "這台主機用 OpenSSH 憑證當主機金鑰，目前還不支援。",
                                "This host uses an OpenSSH certificate as its host key, which is not supported yet."),
    ("term.sshConnecting",    "連線到 {0}:{1} …", "Connecting to {0}:{1} ..."),
    ("term.sshEnvFailed",     "（環境變數 {0} 送不出去：{1}）",
                              "(could not send the environment variable {0}: {1})"),
    ("term.sshKeyRejected",   "金鑰被拒絕，改用其他方式。", "The key was rejected; trying another method."),
    ("term.sshKeyAuthFailed", "金鑰驗證失敗（{0}）。", "Key authentication failed ({0})."),
    ("err.sshConnectFailed",  "連線失敗：{0}", "Connection failed: {0}"),
    ("err.sshConnectTimeout", "{0} 秒內沒有回應", "no response within {0} seconds"),
    ("err.sshNoUser",         "沒有輸入帳號，連線取消。", "No user name was entered; the connection was cancelled."),
    ("err.sshSessionFailed",  "開啟 session 失敗：{0}", "Could not open the session: {0}"),
    ("err.sshPtyFailed",      "請求 PTY 失敗：{0}", "The PTY request failed: {0}"),
    ("err.sshShellFailed",    "開啟 shell 失敗：{0}", "Could not open the shell: {0}"),
    ("err.sshAuthFailed",     "驗證失敗：{0}", "Authentication failed: {0}"),
    ("err.sshAuthFailedPlain", "驗證失敗。", "Authentication failed."),
    ("err.sshBadPassword3",   "密碼錯誤三次，連線結束。", "Wrong password three times; the connection was closed."),
    ("err.sshKeyRead",        "讀不到金鑰檔 {0}：{1}", "Could not read the key file {0}: {1}"),
    ("err.sshKeyDecrypt",     "金鑰解密失敗：{0}", "Could not decrypt the key: {0}"),
    ("err.sshKeyLoad",        "金鑰讀取失敗：{0}", "Could not load the key: {0}"),
    ("err.pageantMissing",    "找不到 Pageant：{0}", "Pageant not found: {0}"),
    ("err.noAuthSock",        "沒有 SSH_AUTH_SOCK", "SSH_AUTH_SOCK is not set"),
    ("err.agentConnect",      "連不上 ssh-agent：{0}", "Could not connect to ssh-agent: {0}"),
    ("err.agentIdentities",   "agent 沒有回應身分清單：{0}", "The agent did not return an identity list: {0}"),
    ("err.cancelled",         "已取消。", "Cancelled."),
    ("err.connCancelled",     "連線已取消。", "The connection was cancelled."),
    ("err.mkdirFailed",       "建立資料夾失敗：{0}", "Could not create the folder: {0}"),
    ("err.keySerialize",      "金鑰序列化失敗：{0}", "Could not serialise the key: {0}"),
    ("err.knownHostsOpen",    "開啟 known_hosts 失敗：{0}", "Could not open known_hosts: {0}"),
    ("err.knownHostsWrite",   "寫入 known_hosts 失敗：{0}", "Could not write known_hosts: {0}"),

    // ---------------- Telnet（telnet/mod.rs）----------------
    ("err.hostNotFoundWhy",  "找不到主機 {0}：{1}", "Host {0} not found: {1}"),
    ("err.hostNotFound",     "找不到主機 {0}", "Host {0} not found"),
    ("err.connectFailed",    "連線 {0} 失敗：{1}", "Could not connect to {0}: {1}"),

    // ---------------- 連接埠（com/mod.rs）----------------
    ("com.parityUnsupported",   "同位檢查 {0} 這個函式庫不支援（只有 None／Odd／Even），已改用 None",
                                "Parity {0} is not supported by the library (only None/Odd/Even); using None instead"),
    ("com.stopBitsUnsupported", "停止位元 {0} 這個函式庫不支援（只有 1 與 2），已改用 1",
                                "Stop bits {0} are not supported by the library (only 1 and 2); using 1 instead"),
    ("com.flowRtsXonUnsupported", "流量控制 RTS/CTS+XON/XOFF 這個函式庫不支援，已改用 RTS/CTS",
                                  "Flow control RTS/CTS+XON/XOFF is not supported by the library; using RTS/CTS instead"),
    ("com.flowUnknown",         "流量控制 {0} 認不出來，已改用 None",
                                "Flow control {0} was not recognised; using None instead"),
    ("com.dataBitsUnsupported", "資料位元 {0} 不支援，已改用 8", "Data bits {0} are not supported; using 8 instead"),
    ("err.comOpenFailed",       "開啟 {0} 失敗：{1}", "Could not open {0}: {1}"),
    ("err.comDtrFailed",        "設定 DTR 失敗：{0}", "Could not set DTR: {0}"),
    ("err.comRtsFailed",        "設定 RTS 失敗：{0}", "Could not set RTS: {0}"),
    ("err.comHandleFailed",     "{0} 無法複製 handle：{1}", "{0}: could not duplicate the handle: {1}"),

    // ---------------- PTY（pty/*.rs）----------------
    ("err.conptyCreate",     "ConptyCreatePseudoConsole 失敗 (HRESULT 0x{0})",
                             "ConptyCreatePseudoConsole failed (HRESULT 0x{0})"),
    ("err.createPseudoCon",  "CreatePseudoConsole 失敗 (HRESULT 0x{0})",
                             "CreatePseudoConsole failed (HRESULT 0x{0})"),
    ("err.attrListSizeZero", "InitializeProcThreadAttributeList 回報大小 0",
                             "InitializeProcThreadAttributeList reported a size of 0"),
    ("err.unixPtyTodo",      "此平台的 PTY 後端尚未實作（Windows ConPTY 已完成，forkpty 待後續任務）",
                             "The PTY backend for this platform is not implemented yet (Windows ConPTY is done; forkpty is a later task)"),
    ("err.unixPtyShort",     "unix pty (openpty)", "unix pty (openpty)"),

    // ---------------- TTL 巨集（ttl/*.rs）----------------
    ("err.macroRunning",     "這個分頁已經在跑巨集了", "This tab is already running a macro"),
    ("err.macroReadFail",    "無法讀取巨集：{0}", "Could not read the macro: {0}"),
    ("err.macroThread",      "開不了巨集執行緒：{0}", "Could not start the macro thread: {0}"),
    ("err.noFileLoader",     "這個直譯器沒有檔案載入器，include 不能用：{0}",
                             "This interpreter has no file loader, so include is unavailable: {0}"),
    ("err.noSuchFile",       "沒有這個檔：{0}", "No such file: {0}"),
    ("err.noSuchDir",        "沒有這個資料夾：{0}", "No such folder: {0}"),
    ("term.macroInterrupted", "[巨集已中斷：{0}]", "[macro interrupted: {0}]"),
    ("term.macroDone",        "[巨集執行完畢：{0}]", "[macro finished: {0}]"),
    ("term.macroError",       "[巨集錯誤] {0} {1}:{2}", "[macro error] {0} {1}:{2}"),
    ("term.regexOptUnsupported", "[巨集] regexoption {0} 這個版本沒有支援（見 docs/TTL-REGEX.md）",
                                 "[macro] regexoption {0} is not supported in this version (see docs/TTL-REGEX.md)"),

    // ---------------- 更新檢查（update.rs；舊版 Loc 的 update.*）----------------
    ("update.checking",   "檢查中…", "Checking..."),
    ("update.latest",     "已是最新版本", "You are up to date"),
    ("update.failed",     "檢查失敗（請確認網路後再試）", "Check failed (check your connection and try again)"),
    // ---------------- 代理團隊（agent/*；舊版 Loc 的 ma.*，中英文逐字照舊版）----------------
    ("ma.title",          "代理團隊", "Multi-Agent"),
    ("ma.menuStop",       "停止任務", "Stop tasks"),
    ("ma.stopPrompt",     "先停一下然後記錄目前狀態", "Stop for now and record the current state."),
    ("ma.tooMany",        "代理團隊最多同時開 9 組。", "At most 9 Multi-Agent teams can be open at the same time."),
    ("ma.openFail",       "代理團隊沒有任何 agent 啟動成功。", "No agent of this Multi-Agent team could be started."),
    ("ma.backendMissing", "找不到 {0}，{1} 沒有啟動。\n請先安裝，或到「新連接 → 自訂…」設定路徑。",
                          "{0} was not found, so {1} was not started.\nInstall it, or set its path in New \u{2192} Custom\u{2026}."),
    ("ma.dlgFolderMissing", "資料夾不存在：\n{0}", "Folder not found:\n{0}"),
    ("ma.dlgNeedBackend",   "{0} 沒有選代理人類型。", "{0} has no Agent Type selected."),
    // 2.0.2：模型名稱會原樣接在命令列上，不合法的字元直接拒絕
    ("model.invalid", "模型名稱只能用英文、數字和 . _ - / : @ 這些符號。",
                      "A model name may only contain letters, digits and . _ - / : @"),
    // 新版才有的（舊版組角色檔失敗只記 log；我們讓建團隊直接失敗，否則 agent 會拿到空角色）
    ("ma.roleComposeFailed", "{0} 的角色檔組合失敗：{1}", "Could not compose the role file for {0}: {1}"),
    ("ma.idleCheckPrompt",
     "[AwayTerminal] 團隊目前全部閒置。請逐一問每個 agent 現在是否還有任務在進行、卡在哪裡，需要的話重新指派或回報給我。",
     "[AwayTerminal] The whole team is idle. Ask each agent whether it still has a task running and where it is stuck, then reassign or report back as needed."),
    // 投遞時打進收件人終端機的那一行：{0}＝序號、{1}＝寄件人、{2}＝task、{3}＝type、{4}＝信件路徑
    ("ma.deliverOne",
     "[AwayTerminal] 訊息 #{0} from {1} ({2}, {3})：請讀 {4}，依你的角色處理，完成後回信給 {1}。",
     "[AwayTerminal] Message #{0} from {1} ({2}, {3}): read {4}, handle it according to your role, then reply to {1}."),
    ("ma.deliverMany",
     "[AwayTerminal] 你有 {0} 則新訊息：請依序讀 {1}，各自依你的角色處理並回信給寄件人。",
     "[AwayTerminal] You have {0} new messages: read {1} in order, handle each according to your role and reply to its sender."),
    ("ma.deliverInfo",
     "[AwayTerminal] 通知 #{0}：請讀 {1}（AwayTerminal 的系統通知，不需要回信）。",
     "[AwayTerminal] Notice #{0}: read {1} (a system notice from AwayTerminal; no reply needed)."),
    // ---------------- AI 聊天室（agent/chat.rs；舊版 Loc 的 chat.*，中英文逐字照舊版）----------------
    ("chat.title",        "AI聊天室", "AI Chat Room"),
    ("chat.dlgNeedTwo",   "AI 聊天室至少要兩位參加者。", "An AI Chat Room needs at least two participants."),
    ("chat.sayNotNow",    "討論還沒開始（或已經結束）。先右鍵「開始討論／換主題…」給主題，討論中再插話。",
                          "The discussion hasn't started (or has already finished). Start a topic first, then add your comment during the discussion."),
    // 新版才有：主題不能是空的（舊版的輸入框按確定時就擋掉了）
    ("chat.topicEmpty",   "請先給討論主題。", "Please give a topic first."),
    // 打進參加者畫面的話（{0}…＝回合數／檔案路徑）
    ("chat.turnPrompt",
     "[AwayTerminal] 第 {0}/{1} 回合，輪到你（{2}）發言：請先讀 {3} 看大家說了什麼，再把你的發言寫進 {4}（300 字以內，UTF-8），寫完就好，不用等別人。",
     "[AwayTerminal] Round {0}/{1}, your turn ({2}): read {3} to see what everyone said, then write your reply into {4} (300 characters max, UTF-8). Just write the file; don't wait for the others."),
    ("chat.conclusionPrompt",
     "[AwayTerminal] 討論結束（共 {0} 回合）。請讀 {1}，整理成結論寫進 {2}（600 字以內，UTF-8），並在你的畫面上把結論顯示給使用者看。",
     "[AwayTerminal] The discussion is over ({0} rounds). Read {1}, write the conclusion into {2} (600 characters max, UTF-8), and also show it on your own screen for the user."),
    // 討論紀錄 transcript.md 裡的標題行
    ("chat.trTopic",      "主題：{0}", "Topic: {0}"),
    ("chat.trRounds",     "討論回合：{0}", "Rounds: {0}"),
    ("chat.trTurn",       "第 {0} 回合 · {1}（{2}）", "Round {0} · {1} ({2})"),
    ("chat.trSkipped",    "## 第 {0} 回合 · {1}\n\n（超過 {2} 分鐘沒有回應，跳過）\n",
                          "## Round {0} · {1}\n\n(No reply after {2} minutes — skipped.)\n"),
    ("chat.trEnded",      "## 第 {0} 回合 · {1}\n\n（這一格已結束，跳過）\n",
                          "## Round {0} · {1}\n\n(This participant has exited — skipped.)\n"),
    ("chat.trHostGone",   "## 結論\n\n（主持人 {0} 已結束，沒有結論）\n",
                          "## Conclusion\n\n(The host {0} has exited — no conclusion.)\n"),
    ("chat.trConclusion", "結論", "Conclusion"),
    ("chat.trUserEnd",    "## 使用者\n\n（要求結束討論）\n", "## User\n\n(Asked to end the discussion.)\n"),
    ("chat.trUserSaid",   "使用者", "User"),
    // pane 狀態標籤與分頁 tooltip
    ("chat.tipNeedTopic", "等你給討論主題（右鍵「開始討論…」）", "Waiting for a topic (right-click \u{2192} Start discussion\u{2026})"),
    ("chat.tipRound",     "討論中 第 {0}/{1} 回合", "Discussing — round {0}/{1}"),
    ("chat.tipConcluding", "主持人正在寫結論", "The host is writing the conclusion"),
    ("chat.tipDone",      "討論已結束（右鍵可開啟討論紀錄資料夾）", "Discussion finished (right-click to open the transcript folder)"),
    // 改團隊／聊天室名稱
    ("err.nameEmpty",     "名稱不能是空的。", "The name cannot be empty."),
    // ---------------- Telegram 遠端（telegram/*；舊版字串是寫死的繁中，這裡照抄）----------------
    ("tg.help",
     "AwayTerminal 遠端指令\n/goto [n]  進入第 n 個分頁；不帶編號＝列出分頁點按進入（🟢閒 🟠忙）\n/new  開新連線（每種連線列一個，回覆數字開啟；SSH 開啟後回覆帳號、密碼登入）\n/ssh [user@]主機[:埠]  開 SSH（不帶參數用我的最愛第一條；帶 user@ 直接連、免回帳號）\n/telnet [主機[:埠]]  開 Telnet（不帶參數用我的最愛第一條）\n/history [編號]  最近連線；帶編號＝用該筆開新連線\n/shot  終端機畫面截圖\n(直接打字)  送出該行 + Enter；畫面是選擇題時回數字或點訊息附的按鈕＝選該選項\n/key <名稱>  送控制鍵 ctrl-c/ctrl-d/esc/tab/enter/up/down/left/right\n/stop  送 Ctrl+C\n/last [n]  最後 n 行輸出（預設 20）\n/more  上一則輸出再往前翻一頁\n/close [n]  真正關閉分頁（無參數＝關目前附著的；/goto 看編號）\n/where  我在哪個分頁\n/follow on|off  進入分頁後、完成時自動回傳輸出（預設開）\n/notify on|off  其他（未進入的）分頁完成也推播通知（預設關）\n/plain on|off  提問時請 AI 用純文字回答、不要表格（預設關；表格本來就會自動攤平）\n/exit  離開分頁檢視（不關分頁；附著後閒置 10 分鐘會靜默自動離開）",
     "AwayTerminal remote commands\n/goto [n]  attach to tab n; without a number it lists the tabs to tap (🟢 idle 🟠 busy)\n/shot  screenshot of the terminal\n(plain text)  sends that line + Enter; on a menu screen reply with the number or tap a button\n/key <name>  send a control key: ctrl-c/ctrl-d/esc/tab/enter/up/down/left/right\n/stop  send Ctrl+C\n/last [n]  the last n lines of output (default 20)\n/more  page further back through the previous output\n/close [n]  really close a tab (no argument = the attached one; /goto shows the numbers)\n/where  which tab am I in\n/follow on|off  send output back automatically once the tab finishes (default on)\n/notify on|off  also notify when other (not attached) tabs finish (default off)\n/plain on|off  ask the AI to answer in plain text, no tables (default off; tables are flattened anyway)\n/exit  leave the tab view (the tab keeps running; 10 minutes idle leaves it silently)"),
    ("tg.cmdGoto",
     "進入分頁／列出分頁",
     "Attach to a tab / list tabs"),
    ("tg.cmdLast",
     "最後幾行輸出",
     "The last lines of output"),
    ("tg.cmdMore",
     "再往前翻一頁",
     "Page further back"),
    ("tg.cmdShot",
     "畫面截圖",
     "Screenshot of the screen"),
    ("tg.cmdKey",
     "送控制鍵",
     "Send a control key"),
    ("tg.cmdStop",
     "送 Ctrl+C",
     "Send Ctrl+C"),
    ("tg.cmdWhere",
     "我在哪個分頁",
     "Which tab am I in"),
    ("tg.cmdClose",
     "關閉分頁",
     "Close a tab"),
    ("tg.cmdFollow",
     "完成時自動回傳輸出",
     "Send output back when finished"),
    ("tg.cmdNotify",
     "其他分頁完成也通知",
     "Notify when other tabs finish too"),
    ("tg.cmdPlain",
     "請 AI 用純文字回答",
     "Ask the AI for plain text"),
    ("tg.cmdExit",
     "離開分頁檢視",
     "Leave the tab view"),
    ("tg.cmdHelp",
     "指令一覽",
     "List of commands"),
    ("tg.fatalToken",
     "Bot Token 無效或已被撤銷（HTTP {0}），遠端控制已自動停止。請到「遠端設定」重新填入 Bot Token。",
     "The bot token is invalid or has been revoked (HTTP {0}). The remote has been turned off. Open the Remote settings dialog and enter a new bot token."),
    ("tg.online",
     "🟢 AwayTerminal 已開啟，遠端上線——可以開始下指令了。/goto 選分頁、/help 看指令。",
     "🟢 AwayTerminal is open and the remote is online — you can start sending commands. /goto to pick a tab, /help for the commands."),
    ("tg.offline",
     "🔴 AwayTerminal 已關閉，遠端離線中——指令暫時不會有回應，重新開啟程式後恢復。",
     "🔴 AwayTerminal has closed and the remote is offline — commands will not get a reply until you start the program again."),
    ("tg.noTabs",
     "目前沒有開啟的分頁。",
     "There are no open tabs."),
    ("tg.notAttached",
     "尚未選擇分頁。先 /goto。",
     "No tab selected yet. Use /goto first."),
    ("tg.tabGone",
     "分頁已關閉或尚未就緒，請重新 /goto。",
     "That tab is closed or not ready — use /goto again."),
    ("tg.entered",
     "已進入 [{0}] {1}。直接打字送指令，/last 看輸出，/exit 離開。",
     "Attached to [{0}] {1}. Just type to send a command, /last to see the output, /exit to leave."),
    ("tg.left",
     "已離開分頁檢視（分頁仍在執行）。",
     "Left the tab view (the tab keeps running)."),
    ("tg.whereAt",
     "目前在 [{0}] {1} {2}",
     "You are in [{0}] {1} {2}"),
    ("tg.noOutput",
     "畫面上沒有輸出。",
     "There is no output on the screen."),
    ("tg.noNewOutput",
     "（尚無新輸出）",
     "(no new output yet)"),
    ("tg.noMore",
     "先 /last 拿到最新輸出，再用 /more 往前翻。",
     "Use /last to get the latest output first, then /more to page back."),
    ("tg.keyUsage",
     "用法：/key <ctrl-c|ctrl-d|esc|tab|enter|up|down|left|right>",
     "Usage: /key <ctrl-c|ctrl-d|esc|tab|enter|up|down|left|right>"),
    ("tg.sendFailed",
     "送鍵失敗（分頁關閉或按鍵名稱錯誤）。",
     "Could not send the key (the tab is closed or the key name is wrong)."),
    ("tg.shotFailed",
     "截圖失敗（畫面尚未就緒）。",
     "The screenshot failed (the screen is not ready)."),
    ("tg.unknown",
     "未知指令。/help 看全部。",
     "Unknown command. /help lists them all."),
    ("tg.notifySet",
     "其他分頁完成通知：{0}",
     "Notify when other tabs finish: {0}"),
    ("tg.followSet",
     "自動回傳輸出：{0}",
     "Send output back automatically: {0}"),
    ("tg.plainSet",
     "提問附加「不要用表格」：{0}",
     "Append “no tables, please” to questions: {0}"),
    ("tg.plainSuffix",
     "（請用純文字條列回答，不要用表格。）",
     " (Please answer in plain text with bullet points, not tables.)"),
    ("tg.done",
     "🟢 {0} 完成",
     "🟢 {0} finished"),
    ("tg.doneOther",
     "🟢 {0} 閒置（完成）",
     "🟢 {0} is idle (finished)"),
    ("tg.idleWarn",
     "⏳ 已閒置 9 分鐘，再 1 分鐘沒動作將自動離開分頁檢視（分頁不會被關閉）。",
     "⏳ Idle for 9 minutes — one more minute without activity and the tab view is left automatically (the tab is not closed)."),
    ("tg.closeAsk",
     "確定要關閉 [{0}] 嗎？{1}",
     "Really close [{0}]?{1}"),
    ("tg.closeWhatTab",
     "（會結束該分頁執行中的程式）",
     " (this ends the program running in that tab)"),
    ("tg.closeWhatTeam",
     "（整個代理團隊的所有 agent 會一起關閉）",
     " (every agent of the whole Multi-Agent team is closed as well)"),
    ("tg.closeYes",
     "✅ 確定關閉",
     "✅ Close it"),
    ("tg.closeNo",
     "✖ 取消",
     "✖ Cancel"),
    ("tg.closed",
     "已關閉 [{0}]。",
     "Closed [{0}]."),
    ("tg.cancelled",
     "已取消。",
     "Cancelled."),
    ("tg.menuGone",
     "畫面上已沒有選單（可能已經回答過了）。",
     "There is no menu on the screen any more (it may already have been answered)."),
    ("tg.notThatTab",
     "已不在該分頁，先 /goto 回去再選。",
     "You are not in that tab any more — /goto back into it first."),
    ("tg.chose",
     "已選擇 {0}。",
     "Chose {0}."),
    ("tg.morePage",
     "…往前一頁",
     "…one page further back"),
    ("tg.on",
     "開",
     "on"),
    ("tg.off",
     "關",
     "off"),
    // ---------------- TASK-021：/new /ssh /telnet /history ----------------
    ("tg.cmdNew",
     "開新連線（每種列一個，選數字）",
     "Open a new connection (one per kind, reply with a number)"),
    ("tg.cmdHistory",
     "最近連線紀錄（/history n 開新連線）",
     "Recent connections (/history n opens one)"),
    ("tg.cmdSsh",
     "開 SSH（可帶 user@主機:埠）",
     "Open SSH (optionally user@host:port)"),
    ("tg.cmdTelnet",
     "開 Telnet（可帶 主機:埠）",
     "Open Telnet (optionally host:port)"),
    ("tg.connShell",
     "PowerShell（桌面）",
     "PowerShell (Desktop)"),
    ("tg.noConns",
     "沒有可用的連線。",
     "There are no connections available."),
    ("tg.pickConn",
     "選擇要開啟的連線（點按鈕，或回覆數字）：",
     "Pick the connection to open (tap a button or reply with the number):"),
    ("tg.recentConns",
     "最近連線（點按鈕或 /history <編號> 用該筆開新連線）：",
     "Recent connections (tap a button, or /history <number> to open one):"),
    ("tg.noHistory",
     "還沒有連線紀錄。",
     "There are no connections recorded yet."),
    ("tg.rangeIs",
     "編號要在 1~{0} 之間。/goto 看編號。",
     "The number must be between 1 and {0}. /goto shows them."),
    ("tg.openFailed",
     "開啟失敗。",
     "Could not open it."),
    ("tg.opened",
     "已開啟並進入 [{0}]。直接打字送指令，/last 看輸出。",
     "Opened and attached to [{0}]. Just type to send a command, /last to see the output."),
    ("tg.openedSsh",
     "已開啟並進入 [{0}]。login as: → 直接回覆帳號，接著照畫面提示回覆密碼。",
     "Opened and attached to [{0}]. At login as: reply with the account name, then follow the screen for the password."),
    ("tg.sshUsage",
     "用法：/ssh [user@]主機[:埠]（不帶參數時用我的最愛裡第一條 SSH）",
     "Usage: /ssh [user@]host[:port] (without arguments the first SSH favourite is used)"),
    ("tg.telnetUsage",
     "用法：/telnet 主機[:埠]（不帶參數時用我的最愛裡第一條 Telnet）",
     "Usage: /telnet host[:port] (without arguments the first Telnet favourite is used)"),
    // ---------------- TASK-022：Linux 的平台提示 ----------------
    ("err.comDialoutGroup",
     "沒有權限開這個連接埠。Linux 要加入 dialout 群組：{0}（執行後要重新登入才生效）。",
     "No permission to open this serial port. On Linux you need to be in the dialout group: {0} (log in again afterwards)."),
    ("warn.gtkImModule",
     "沒有設定 GTK_IM_MODULE，中文輸入法可能不能用。裝 fcitx5 的話設 GTK_IM_MODULE=fcitx，裝 ibus 的話設 GTK_IM_MODULE=ibus，再重新登入。",
     "GTK_IM_MODULE is not set, so a Chinese input method may not work. Set GTK_IM_MODULE=fcitx for fcitx5 or GTK_IM_MODULE=ibus for ibus, then log in again."),
];

/// 測試用：把前端推過來的字串清掉（各測試之間不要互相影響）。
#[cfg(test)]
pub fn clear_pushed_for_test() {
    if let Ok(mut m) = pushed().lock() {
        m.clear();
    }
}

/// 測試用的鎖：`LANG` 是 process 全域的，`cargo test` 會平行跑，
/// 所以**任何會改語言的測試**（含別的模組的）都要先拿這個鎖，
/// 不然會偶發地讀到另一個測試設的語言。
#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表本身要合理：key 不重複、三個欄位都不空、參數編號兩邊一致。
    #[test]
    fn table_is_sane() {
        let mut seen = std::collections::HashSet::new();
        for (k, zh, en) in TABLE {
            assert!(seen.insert(*k), "key 重複：{k}");
            assert!(!k.is_empty() && !zh.is_empty() && !en.is_empty(), "{k} 有空欄位");
            // `{0}`…`{3}` 在兩種語言裡都要出現同樣的次數，不然換語言就會掉參數
            for i in 0..4 {
                let p = format!("{{{i}}}");
                assert_eq!(
                    zh.contains(&p),
                    en.contains(&p),
                    "{k} 的 {p} 只在一種語言裡出現"
                );
            }
        }
    }

    /// 內建**後備**的語言切換（前端還沒推字串過來時用的那一份）。
    ///
    /// 只有兩份：中文與英文。`zh-CN` 這種也走中文那一份（只是後備，
    /// 真正的簡中字串由前端推過來）；非中文一律走英文那一份。
    #[test]
    fn switches_fallback_language() {
        let _g = test_lock();
        clear_pushed_for_test();
        set_lang("zh-TW");
        assert_eq!(t("err.needHost"), "請輸入主機");
        set_lang("zh-CN");
        assert_eq!(t("err.needHost"), "請輸入主機", "簡中的後備用中文那一份");
        set_lang("en");
        assert_eq!(t("err.needHost"), "Please enter a host");
        set_lang("de");
        assert_eq!(t("err.needHost"), "Please enter a host", "非中文的後備用英文");
    }

    /// 前端推過來的字串**優先於**內建後備，而且空字串不算（會退回後備）。
    #[test]
    fn pushed_strings_win() {
        let _g = test_lock();
        clear_pushed_for_test();
        set_lang("de");
        let mut m = std::collections::HashMap::new();
        m.insert("err.needHost".to_string(), "Bitte einen Host angeben.".to_string());
        m.insert("err.needName".to_string(), String::new()); // 空的 → 退回後備
        set_pushed("de", m);
        assert_eq!(t("err.needHost"), "Bitte einen Host angeben.");
        assert_eq!(t("err.needName"), "Please enter a name");
        // 帶參數的也走同一條路
        let mut m2 = std::collections::HashMap::new();
        m2.insert("err.connNotFound".to_string(), "Nicht gefunden: {0}".to_string());
        set_pushed("de", m2);
        assert_eq!(tf("err.connNotFound", &["claude"]), "Nicht gefunden: claude");
        clear_pushed_for_test();
    }

    /// `i18n_keys()` 是前端要推哪些 key 的依據，不能是空的，也不能有重複。
    #[test]
    fn keys_are_reported_for_the_frontend() {
        let keys = i18n_keys();
        assert!(keys.len() > 100, "至少有 100 條，實際 {}", keys.len());
        let uniq: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(uniq.len(), keys.len(), "i18n_keys() 有重複");
    }

    /// 參數取代（多位數也要對：`{1}` 不能被 `{0}` 吃掉）。
    #[test]
    fn fills_arguments() {
        let _g = test_lock();
        set_lang("zh");
        assert_eq!(tf("err.connNotFound", &["claude"]), "找不到自訂連線：claude");
        assert_eq!(tf("err.gitFailed", &["worktree add", "boom"]), "git 執行失敗（worktree add）：boom");
        // 參數比佔位符少 → 沒填的原樣留著（總比 panic 好）
        assert_eq!(fill("{0} / {1}", &["a"]), "a / {1}");
    }

    /// 這幾個 key 是別的模組**一定**會用到的，改名就會在這裡叫。
    #[test]
    fn keys_used_elsewhere_exist() {
        for k in [
            "err.needHost",
            "err.tabNotFound",
            "term.macroDone",
            "search.placeholder",
            "ma.stateIdle",
            "com.parityUnsupported",
        ] {
            assert!(TABLE.iter().any(|(key, _, _)| key == &k), "少了 {k}");
        }
    }
}
