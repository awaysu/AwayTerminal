//! Rust 端的中／英字串表（搬移舊版 `Localization/Loc.cs` 的做法）。
//!
//! ## 為什麼是「Rust 端自己有一份表」而不是「回代碼給前端查表」
//!
//! TASK-015 B 要求兩種選一種、寫下理由。選這種，理由三條：
//!
//! 1. **漏掉的代價不一樣**。回代碼的話，漏改一處使用者會看到 `err.connNotFound`
//!    這種東西（等於壞掉）；這裡漏改一處只是那一句留在繁中（難看但看得懂）。
//! 2. **不必動協定**。錯誤字串是 `Result<_, String>` 的 `Err`、終端機畫面的訊息是
//!    直接寫進 pane 的位元組——改成代碼要動 `bridge.js`、對話框、`--verify` 的比對，
//!    改動面反而更大。
//! 3. **有些訊息本來就沒辦法代碼化**：它們帶作業系統給的原文
//!    （`開啟 COM5 失敗：系統找不到指定的檔案。`）。前端查表也翻不了後半段。
//!
//! 另外 [`crate::ttl::error`] 本來就有 `message()`（英文，照 `errdlg.cpp`）與
//! `message_zh()` 兩份，這裡只要照語言挑一個，不必搬 100 多條錯誤碼過去。
//!
//! ## 邊界：只翻「使用者看得到的」
//!
//! | 種類 | 翻不翻 | 為什麼 |
//! |---|---|---|
//! | `Err(...)` 回給前端的（對話框／提示） | ✅ | 使用者直接看到 |
//! | 寫進終端機畫面的（重連倒數、SSH 狀態、巨集結束） | ✅ | 同上 |
//! | 檔案選擇／存檔對話框的標題與篩選器 | ✅ | 同上 |
//! | `T{json}`（搜尋列、代理狀態標籤） | ✅ | 前端直接顯示 |
//! | `println!("[AwayTerminal] …")` | ❌ | **開發診斷**：打包後的 app 沒有 stdout；而且 `--verify` 與踩雷紀錄都在比對這些字串 |
//! | `--verify` 專用的訊息（`execverify.rs`） | ❌ | 只有開發時跑得到 |
//!
//! 清單（哪個檔有幾條）在 `docs/SETTINGS.md`，由
//! `scripts/i18n-audit.mjs` 重新產生，所以「有沒有漏」是可以重複驗的。
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

use std::sync::atomic::{AtomicU8, Ordering};

/// 目前語言。`0` ＝繁中，`1` ＝英文。
///
/// 放成 process 全域的原子變數，因為寫進終端機畫面的訊息是在**背景執行緒**
/// （PTY 讀取、重連、巨集）產生的，拿不到 tauri 的 `State`。
static LANG: AtomicU8 = AtomicU8::new(0);

/// 設定語言（`"zh"`／`"en"`；其他值一律當 `zh`）。啟動時與設定視窗按確定時呼叫。
pub fn set_lang(code: &str) {
    LANG.store(u8::from(code == "en"), Ordering::Relaxed);
}

/// 現在是英文嗎。
pub fn is_en() -> bool {
    LANG.load(Ordering::Relaxed) == 1
}

/// 查一個字串。**查不到就回 key 本身**（fail-soft；`table_is_sane` 測試會抓漏）。
pub fn t(key: &str) -> &'static str {
    match TABLE.iter().find(|(k, _, _)| *k == key) {
        Some((_, zh, en)) => {
            if is_en() {
                en
            } else {
                zh
            }
        }
        None => {
            debug_assert!(false, "i18n: 沒有這個 key：{key}");
            leak_key(key)
        }
    }
}

/// 查一個字串並填入參數（`{0}`／`{1}`…）。
pub fn tf(key: &str, args: &[&str]) -> String {
    fill(t(key), args)
}

/// `{0}`／`{1}`… 的取代（同舊版 `string.Format`、前端 `fmt()`）。
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = template.to_string();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

/// 查不到 key 時要回 `&'static str`，只能洩掉一份。**只會發生在寫錯 key 的時候**，
/// 而且 debug build 會先 panic，所以不會在正常使用中累積。
fn leak_key(key: &str) -> &'static str {
    Box::leak(key.to_string().into_boxed_str())
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
    ("err.unixPtyShort",     "unix pty (尚未實作)", "unix pty (not implemented yet)"),

    // ---------------- TTL 巨集（ttl/*.rs）----------------
    ("err.macroRunning",     "這個分頁已經在跑巨集了", "This tab is already running a macro"),
    ("err.macroReadFail",    "無法讀取巨集：{0}", "Could not read the macro: {0}"),
    ("err.macroThread",      "開不了巨集執行緒：{0}", "Could not start the macro thread: {0}"),
    ("err.noFileLoader",     "這個直譯器沒有檔案載入器，include 不能用：{0}",
                             "This interpreter has no file loader, so include is unavailable: {0}"),
    ("err.noSuchFile",       "沒有這個檔：{0}", "No such file: {0}"),
    ("term.macroInterrupted", "[巨集已中斷：{0}]", "[macro interrupted: {0}]"),
    ("term.macroDone",        "[巨集執行完畢：{0}]", "[macro finished: {0}]"),
    ("term.macroError",       "[巨集錯誤] {0} {1}:{2}", "[macro error] {0} {1}:{2}"),
    ("term.regexOptUnsupported", "[巨集] regexoption {0} 這個版本沒有支援（見 docs/TTL-REGEX.md）",
                                 "[macro] regexoption {0} is not supported in this version (see docs/TTL-REGEX.md)"),

    // ---------------- 更新檢查（update.rs；舊版 Loc 的 update.*）----------------
    ("update.checking",   "檢查中…", "Checking..."),
    ("update.latest",     "已是最新版本", "You are up to date"),
    ("update.failed",     "檢查失敗（請確認網路後再試）", "Check failed (check your connection and try again)"),
];

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

    /// 切語言真的會換，而且切回來也對。
    #[test]
    fn switches_language() {
        let _g = test_lock();
        set_lang("zh");
        assert_eq!(t("err.needHost"), "請輸入主機");
        set_lang("en");
        assert_eq!(t("err.needHost"), "Please enter a host");
        // 認不出來的語言代碼一律當繁中（同舊版 `Loc.SetLang`）
        set_lang("de");
        assert_eq!(t("err.needHost"), "請輸入主機");
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
