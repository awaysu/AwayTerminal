//! 檢查更新（逐段照舊版 `Services/UpdateChecker.cs`）。
//!
//! ## 照抄的部分
//!
//! | 舊版 | 這裡 |
//! |---|---|
//! | `GET https://www.awaysu.cc/software/api.php?action=check_update&app=…&platform=windows&version=…` | 同（[`API`]／[`APP_SLUG`]） |
//! | `HttpClient { Timeout = 10s }`、`User-Agent: AwayTerminal/<版本>` | 同 |
//! | 回 `ok:false`／JSON 壞掉／沒網路 → **一律回 null，不吵使用者** | 同（回 `None`） |
//! | 版本比較交給伺服器的 `update_available`；沒帶才自己比 | 同（[`compare`]） |
//! | `page_url` 空的時候退回軟體頁 | 同（[`FALLBACK_PAGE`]） |
//! | 只有按下「檢查更新」才查（**啟動時不自動查**） | 同（只有 [`update_check`] 這個 command） |
//!
//! ## 刻意不同
//!
//! - `platform`：Windows 送 `windows`、mac 送 `macos`、Linux 送 `linux`（舊版只有 Windows）。
//! - **Tauri updater（自動下載安裝）不做**，那是階段 5。這裡只到「開下載頁」。

use std::time::Duration;

/// 軟體頁的公開 API（舊版同一支；不需密碼）。
pub const API: &str = "https://www.awaysu.cc/software/api.php";
/// 網站上的「參數代號」。**和舊版同一個 `awayterminal`**（2026-10-01 使用者定案：
/// 2.0 直接接手舊版的產品頁，舊版使用者按「檢查更新」就會看到 2.0）。
pub const APP_SLUG: &str = "awayterminal";
/// `page_url` 沒帶時的下載頁。
pub const FALLBACK_PAGE: &str = "https://www.awaysu.cc/software/awayterminal";
/// 舊版是 10 秒。
const TIMEOUT: Duration = Duration::from_secs(10);

/// 查詢結果（欄位名照舊版 `UpdateInfo`，給前端用 camelCase）。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub latest_version: String,
    pub update_available: bool,
    /// 最新版的更新說明（可能多行；沒有就是空字串）。
    pub release_notes: String,
    /// 下載頁（讓使用者自己挑安裝版／免安裝版，同舊版）。
    pub page_url: String,
}

/// 這個平台的 `platform` 參數。
fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// 組出查詢用的 URL（拆出來才能在測試裡檢查參數，也給 `--verify` 換 base）。
pub fn build_url(base: &str, current: &str) -> String {
    format!(
        "{base}?action=check_update&app={APP_SLUG}&platform={}&version={}",
        platform(),
        urlencode(current)
    )
}

/// 只有版本號要編碼，用不到通用的 URL 函式庫。
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 解析 API 的回覆。**任何不合預期都回 `None`**（同舊版的 `catch { return null; }`）。
pub fn parse(json: &str, current: &str) -> Option<UpdateInfo> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let obj = v.as_object()?;
    // 舊版：`ok` 不是 true 就當失敗
    if obj.get("ok") != Some(&serde_json::Value::Bool(true)) {
        return None;
    }
    let s = |k: &str| {
        obj.get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let latest = s("latest_version");
    if latest.trim().is_empty() {
        return None; // 舊版：沒有 latest_version 就當失敗
    }
    let mut page_url = s("page_url");
    if page_url.trim().is_empty() {
        page_url = FALLBACK_PAGE.to_string();
    }
    // `update_available` 由伺服器算好；沒帶（或是 null）就自己比一次
    let update_available = match obj.get("update_available") {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Null) | None => compare(&latest, current) > 0,
        Some(_) => false,
    };
    Some(UpdateInfo {
        latest_version: latest,
        update_available,
        release_notes: s("release_notes"),
        page_url,
    })
}

/// 版本比較（`a > b` 回正數），逐段比數字、段數不同補 0、`-beta` 之類視為比正式版小。
/// 逐行照舊版 `UpdateChecker.Compare`。
pub fn compare(a: &str, b: &str) -> i32 {
    fn parse_v(s: &str) -> (Vec<u32>, bool) {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let cut = s.find(['-', '+']);
        let pre = cut.is_some();
        let s = &s[..cut.unwrap_or(s.len())];
        (
            s.split('.').map(|x| x.parse().unwrap_or(0)).collect(),
            pre,
        )
    }
    let (na, pa) = parse_v(a);
    let (nb, pb) = parse_v(b);
    for i in 0..na.len().max(nb.len()) {
        let x = na.get(i).copied().unwrap_or(0);
        let y = nb.get(i).copied().unwrap_or(0);
        if x != y {
            return if x > y { 1 } else { -1 };
        }
    }
    match (pa, pb) {
        (true, false) => -1,
        (false, true) => 1,
        _ => 0,
    }
}

/// 真的去查一次（阻塞，所以 command 要 `spawn_blocking`）。
pub fn fetch(base: &str, current: &str) -> Option<UpdateInfo> {
    let url = build_url(base, current);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(format!("AwayTerminal/{current}"))
        .build()
        .into();
    let body = agent
        .get(&url)
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    parse(&body, current)
}

/// 「檢查更新」按鈕。回 `None`＝檢查失敗（前端顯示「檢查失敗」，**不跳錯誤視窗**）。
///
/// `base` 只給 `--verify` 用（指到本機的假伺服器）；正常呼叫不要帶。
#[tauri::command]
pub async fn update_check(current: String, base: Option<String>) -> Option<UpdateInfo> {
    let base = base.unwrap_or_else(|| API.to_string());
    // 網路 I/O 不能擋主執行緒（同 `exit_confirm` 那條隱含契約）
    tokio::task::spawn_blocking(move || {
        let r = fetch(&base, &current);
        match &r {
            Some(i) => println!(
                "[AwayTerminal] 檢查更新：最新 {} 有新版={}",
                i.latest_version, i.update_available
            ),
            None => println!("[AwayTerminal] 檢查更新：失敗（離線／伺服器沒回 ok）"),
        }
        r
    })
    .await
    .ok()
    .flatten()
}

/// **只給 `--verify` 用**：整條路（組 URL → HTTP GET → 解析）跑一次，
/// 外加「連不上」的那條路，**完全不碰真的網站**。
///
/// 做法：在 127.0.0.1 上開一個只回一次答案的假伺服器（`std::net`，不需要任何函式庫），
/// 讓 `fetch()` 去打它；再對一個**已經關掉的** port 打一次，證明失敗會安靜地回 `None`。
#[tauri::command]
pub async fn update_verify() -> Result<serde_json::Value, String> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    const BODY: &str = r#"{"ok":true,"latest_version":"9.9.9","release_notes":"verify 用的假回應",
                           "page_url":"https://127.0.0.1/fake","update_available":true}"#;

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("開不了假伺服器：{e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let server = std::thread::spawn(move || {
        // 只服務一個連線就結束（`fetch` 只打一次）
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let n = sock.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                BODY.len(),
                BODY
            );
            let _ = sock.write_all(resp.as_bytes());
            let _ = sock.flush();
            return req;
        }
        String::new()
    });

    let base = format!("http://127.0.0.1:{port}/api.php");
    let got = tokio::task::spawn_blocking(move || fetch(&base, "2.0.0"))
        .await
        .map_err(|e| e.to_string())?;
    let req = server.join().unwrap_or_default();

    // 關掉的 port（bind 之後馬上 drop → 沒人在聽）→ 應該安靜地回 None
    let dead_port = {
        let l = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        l.local_addr().map_err(|e| e.to_string())?.port()
    };
    let offline = tokio::task::spawn_blocking(move || {
        fetch(&format!("http://127.0.0.1:{dead_port}/api.php"), "2.0.0")
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "requestLine": req.lines().next().unwrap_or("").to_string(),
        "sentSlug": req.contains(&format!("app={APP_SLUG}")),
        "sentUserAgent": req.to_lowercase().contains("user-agent: awayterminal/2.0.0"),
        "parsed": got,
        "offlineIsNone": offline.is_none(),
    }))
}

/// 「關於」頁要顯示的東西。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    pub version: String,
    /// 編譯時間＝**這個 exe 的檔案寫入時間**（同舊版：build 當下寫檔，複製／安裝會保留）。
    pub build_time: String,
    /// 實際安裝的 xterm.js 版本（build 時從 `node_modules` 讀，見 `build.rs`）。
    pub xterm_version: String,
    pub tauri_version: String,
    pub download_url: String,
    pub source_url: String,
    /// 作者的 email 拆成三段：前端用 canvas 畫出來，畫面上沒有可以被爬的文字
    ///（同舊版 `RenderTextImage` 的用意）。
    pub author_parts: [String; 3],
}

/// 「關於」頁的資料。
#[tauri::command]
pub fn about_info() -> AboutInfo {
    let build_time = std::env::current_exe()
        .and_then(|p| p.metadata())
        .and_then(|m| m.modified())
        .map(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            dt.format("%Y/%-m/%-d %H:%M:%S").to_string()
        })
        .unwrap_or_else(|_| "-".to_string());
    AboutInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build_time,
        xterm_version: env!("XTERM_VERSION").to_string(),
        tauri_version: tauri::VERSION.to_string(),
        download_url: FALLBACK_PAGE.to_string(),
        source_url: "https://github.com/awaysu/AwayTerminal2".to_string(),
        author_parts: [
            "Awaysu (awaysu".to_string(),
            "@".to_string(),
            "gmail.com)".to_string(),
        ],
    }
}

/// `THIRD-PARTY-NOTICES.md` 的內容（「關於」頁展開顯示）。
///
/// **不複製一份**（PM 在 TASK-015 C1 明確要求）：開發時讀 repo 根目錄的那一份，
/// 安裝之後讀 exe 旁邊 `resources/` 的那一份（`tauri.conf.json` 會打包進去）。
#[tauri::command]
pub fn third_party_notices(app: tauri::AppHandle) -> Result<String, String> {
    use tauri::Manager;
    let mut tried: Vec<String> = Vec::new();
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        candidates.push(dir.join("THIRD-PARTY-NOTICES.md"));
        candidates.push(dir.join("resources/THIRD-PARTY-NOTICES.md"));
    }
    // dev：exe 在 target/debug/，repo 根目錄在上面第三層
    if let Ok(exe) = std::env::current_exe() {
        for up in 1..=4 {
            if let Some(d) = exe.ancestors().nth(up) {
                candidates.push(d.join("THIRD-PARTY-NOTICES.md"));
            }
        }
    }
    for p in candidates {
        match std::fs::read_to_string(&p) {
            Ok(s) => return Ok(s),
            Err(_) => tried.push(p.display().to_string()),
        }
    }
    Err(crate::i18n::t("about.noticesFail").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// URL 的參數與舊版一樣（順序也一樣，方便和舊版的 log 對照）。
    #[test]
    fn builds_the_same_url_as_v1() {
        let url = build_url(API, "2.0.0");
        assert!(url.starts_with("https://www.awaysu.cc/software/api.php?action=check_update"));
        assert!(url.contains("&app=awayterminal&"));
        assert!(url.contains("&version=2.0.0"));
        // 版本號有奇怪字元也要編碼
        assert!(build_url(API, "2.0.0 beta").contains("version=2.0.0%20beta"));
    }

    /// 正常回覆。
    #[test]
    fn parses_ok_response() {
        let json = r#"{"ok":true,"latest_version":"2.1.0","release_notes":"a\nb",
                       "page_url":"https://x/y","update_available":true}"#;
        let info = parse(json, "2.0.0").expect("應該解得出來");
        assert_eq!(info.latest_version, "2.1.0");
        assert_eq!(info.release_notes, "a\nb");
        assert_eq!(info.page_url, "https://x/y");
        assert!(info.update_available);
    }

    /// 伺服器沒帶 `update_available` → 自己比（舊版同）。
    #[test]
    fn computes_available_when_missing() {
        let json = r#"{"ok":true,"latest_version":"2.0.1"}"#;
        assert!(parse(json, "2.0.0").unwrap().update_available);
        let json = r#"{"ok":true,"latest_version":"2.0.0"}"#;
        assert!(!parse(json, "2.0.0").unwrap().update_available);
        // 沒帶 page_url → 退回軟體頁
        assert_eq!(parse(json, "2.0.0").unwrap().page_url, FALLBACK_PAGE);
    }

    /// 壞掉的回覆一律 `None`（**不吵使用者**，這是舊版刻意的行為）。
    #[test]
    fn bad_responses_are_none() {
        for json in [
            "",
            "not json",
            r#"{"ok":false,"error":"unknown app"}"#,
            r#"{"ok":true}"#,                        // 沒有 latest_version
            r#"{"ok":true,"latest_version":"  "}"#,  // 空白
            r#"[1,2,3]"#,                            // 不是物件
            r#"{"latest_version":"9.9.9"}"#,          // 沒有 ok
        ] {
            assert!(parse(json, "2.0.0").is_none(), "這個應該是 None：{json}");
        }
    }

    /// 版本比較：逐段、補 0、預發行版比正式版小（逐條照舊版 `Compare`）。
    #[test]
    fn compares_versions_like_v1() {
        assert!(compare("2.0.1", "2.0.0") > 0);
        assert!(compare("2.1", "2.0.9") > 0);
        assert!(compare("2.0", "2.0.0") == 0); // 段數不同補 0
        assert!(compare("v2.0.0", "2.0.0") == 0); // 前面的 v 不算
        assert!(compare("2.0.0-beta", "2.0.0") < 0); // 預發行版小
        assert!(compare("2.0.0", "2.0.0-beta") > 0);
        assert!(compare("2.0.0-beta", "2.0.0-rc") == 0); // 兩邊都是預發行 → 只比數字
        assert!(compare("10.0.0", "9.9.9") > 0); // 字串比較會錯的那個經典例子
        assert!(compare("", "2.0.0") < 0); // 壞掉的版本號不要當成有新版
    }

    /// 平台參數只有三種。
    #[test]
    fn platform_is_known() {
        assert!(["windows", "macos", "linux"].contains(&platform()));
    }
}
