//! Telegram Bot API 的最小用戶端（搬移舊版自寫的 `HttpClient` 那幾個呼叫）。
//!
//! 舊版刻意不用第三方 Telegram 套件——只需要 `getUpdates`／`sendMessage`／`sendPhoto`／
//! `answerCallbackQuery`／`setMyCommands` 五個端點。這裡照樣自己寫，用已經在依賴樹裡的
//! `ureq`（阻塞式）＋ 一個專用執行緒，**不多拉任何相依**。
//!
//! ## 為什麼是阻塞式 ＋ 自己的執行緒
//! long polling 一次要掛 30 秒。舊版是 async，但它踩過一個雷（1.1.10 實錄）：
//! `Start` 在 UI 執行緒被呼叫，`_ = PollLoop()` 讓整串 await 都接回 UI 執行緒 →
//! 指令處理時在 UI 執行緒上等 WebView2 回覆，而回覆也要 UI 執行緒處理 → 必定逾時。
//! 我們用一條自己的 std 執行緒，根本不會碰到那種「等自己」的情況。
//!
//! ## ⚠️ token
//! **token 絕不進 log、不進錯誤訊息、不進 `--verify` 輸出。**
//! 這個模組的每個 `println!` 都只印端點名稱與狀態碼，URL 一律不印
//! （URL 裡就有 token：`https://api.telegram.org/bot<TOKEN>/…`）。

use std::time::Duration;

/// Bot API 的基底位址。`--verify` 與 `telegram_probe` 會換成 `http://127.0.0.1:<port>`。
#[derive(Clone, Debug)]
pub struct Api {
    base: String,
    token: String,
    /// long polling 的 `timeout` 參數（秒）。
    pub poll_timeout: u32,
}

/// 一則收到的訊息（只取我們用得到的欄位）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    pub update_id: i64,
    pub chat_id: i64,
    /// 文字訊息的內容（inline 按鈕的點擊是 `callback`）。
    pub text: Option<String>,
    /// inline 按鈕的 `callback_data` 與它的 id。
    pub callback: Option<(String, String)>,
}

impl Api {
    pub fn new(base: &str, token: &str) -> Self {
        let base = base.trim_end_matches('/').to_string();
        // 對著 127.0.0.1 就是 `--verify` 的假 Bot API（[`crate::telegram::probe`]）：
        // long poll 縮成 2 秒，整段驗證才不用等 30 秒一輪，逾時也不會拖 50 秒。
        let local = base.contains("127.0.0.1") || base.contains("localhost");
        Self {
            base,
            token: token.trim().to_string(),
            poll_timeout: if local { 2 } else { 30 },
        }
    }

    /// 正式用的位址。
    pub fn telegram(token: &str) -> Self {
        Self::new("https://api.telegram.org", token)
    }

    /// 組 URL。**只在這裡拼 token，不要把回傳值印出來。**
    fn url(&self, method: &str) -> String {
        format!("{}/bot{}/{method}", self.base, self.token)
    }

    fn agent(&self, secs: u64) -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(secs)))
            .build()
            .into()
    }

    /// `getUpdates`。回傳 (訊息, 下一個 offset)。
    ///
    /// `offset = -1` ＝只要最後一則（啟動時用它把 offset 推到最新，見 [`Self::prime_offset`]）。
    pub fn get_updates(&self, offset: i64) -> Result<(Vec<Incoming>, i64), String> {
        // long poll 要等 `poll_timeout` 秒，HTTP 逾時要比它長一點（舊版是 30 / 50）
        let agent = self.agent(self.poll_timeout as u64 + 20);
        let url = format!(
            "{}?timeout={}&offset={offset}",
            self.url("getUpdates"),
            self.poll_timeout
        );
        let mut resp = agent
            .get(&url)
            .call()
            .map_err(|e| describe(&e))?;
        let status = resp.status().as_u16();
        if status != 200 {
            // 和 `describe()` 用同一個寫法，`http_status()` 才認得（見那兩個函式的註解）
            return Err(format!("HTTP {status}"));
        }
        // i18n-audit:log-only-begin 錯誤字串只會進 println! 的診斷行，使用者看不到
        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("getUpdates 讀取失敗：{e}"))?;
        // i18n-audit:log-only-end
        parse_updates(&body, offset)
    }

    /// 啟動時把 offset 推到最新，避免一開機就重播關機期間累積的舊指令。
    ///
    /// ⚠️ 一定要用 `offset=-1`（只要最後一則）：預設一次最多回 100 則，關著超過 100 則訊息時
    /// offset 會停在第 101 則，第一次輪詢就把 101～N 當成現在的指令全部重播
    /// （打進附著的分頁、`/close`…）——舊版的註解就是這個教訓。
    pub fn prime_offset(&self) -> i64 {
        let agent = self.agent(10);
        let url = format!("{}?timeout=0&offset=-1", self.url("getUpdates"));
        match agent.get(&url).call() {
            Ok(mut resp) => {
                let body = resp.body_mut().read_to_string().unwrap_or_default();
                parse_updates(&body, 0).map(|(_, next)| next).unwrap_or(0)
            }
            Err(e) => {
                println!("[AwayTerminal] Telegram：prime offset 失敗（{}）", describe(&e));
                0
            }
        }
    }

    /// 「取得 chat id」：抓最近一則**訊息**的 chat id（舊版 `TelegramRemote.TryGetLatestChatId`）。
    ///
    /// 逐項照舊版：`getUpdates`（不帶 offset）→ 掃 `result` → 回**最後一則**訊息的
    /// `message.chat.id`；沒有訊息就回 `None`。inline 按鈕的點擊不算（舊版只看 `message`）。
    /// 逾時 10 秒、`timeout=0`（不 long poll）——這是使用者按按鈕在等的，不能卡 30 秒。
    ///
    /// ⚠️ 遠端正在跑的時候按這顆，這次查詢會和輪詢那條連線搶同一批 update
    /// （Telegram 對同一個 token 只給一條 getUpdates）。舊版也是這樣，行為照舊。
    pub fn latest_chat_id(&self) -> Result<Option<i64>, String> {
        let agent = self.agent(10);
        let url = format!("{}?timeout=0", self.url("getUpdates"));
        let mut resp = agent.get(&url).call().map_err(|e| describe(&e))?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(format!("getUpdates HTTP {status}"));
        }
        // i18n-audit:log-only-begin 錯誤字串只進診斷行
        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("getUpdates 讀取失敗：{e}"))?;
        // i18n-audit:log-only-end
        let (msgs, _) = parse_updates(&body, 0)?;
        Ok(msgs
            .iter()
            .rfind(|m| m.callback.is_none() && m.chat_id != 0)
            .map(|m| m.chat_id))
    }

    /// `sendMessage`。`html`＝用 HTML parse mode；`buttons`＝inline 鍵盤。
    pub fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        html: bool,
        buttons: &[Vec<(String, String)>],
    ) -> Result<(), String> {
        let mut payload = serde_json::json!({ "chat_id": chat_id, "text": text });
        if html {
            payload["parse_mode"] = serde_json::Value::String("HTML".into());
        }
        if !buttons.is_empty() {
            let rows: Vec<Vec<serde_json::Value>> = buttons
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|(t, d)| serde_json::json!({ "text": t, "callback_data": d }))
                        .collect()
                })
                .collect();
            payload["reply_markup"] = serde_json::json!({ "inline_keyboard": rows });
        }
        self.post_json("sendMessage", &payload)
    }

    /// `answerCallbackQuery`（讓按鈕停止轉圈）。
    pub fn answer_callback(&self, callback_id: &str, text: Option<&str>) -> Result<(), String> {
        let mut payload = serde_json::json!({ "callback_query_id": callback_id });
        if let Some(t) = text {
            payload["text"] = serde_json::Value::String(t.to_string());
        }
        self.post_json("answerCallbackQuery", &payload)
    }

    /// `setMyCommands`（註冊原生 `/` 指令選單）。
    pub fn set_my_commands(&self, cmds: &[(&str, String)]) -> Result<(), String> {
        let list: Vec<serde_json::Value> = cmds
            .iter()
            .map(|(c, d)| serde_json::json!({ "command": c, "description": d }))
            .collect();
        self.post_json("setMyCommands", &serde_json::json!({ "commands": list }))
    }

    /// `sendPhoto`（multipart）。`png`＝圖片位元組。
    pub fn send_photo(&self, chat_id: i64, png: &[u8], caption: &str) -> Result<(), String> {
        let boundary = format!("----AwayTerminal{}", crate::tabs::now_ms());
        let mut body: Vec<u8> = Vec::with_capacity(png.len() + 512);
        let mut field = |name: &str, value: &str| {
            body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            );
            body.extend_from_slice(value.as_bytes());
            body.extend_from_slice(b"\r\n");
        };
        field("chat_id", &chat_id.to_string());
        if !caption.is_empty() {
            field("caption", caption);
        }
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"photo\"; filename=\"screen.png\"\r\n",
        );
        body.extend_from_slice(b"Content-Type: image/png\r\n\r\n");
        body.extend_from_slice(png);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let agent = self.agent(60);
        let resp = agent
            .post(self.url("sendPhoto"))
            .header("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
            .send(&body[..])
            .map_err(|e| describe(&e))?;
        let status = resp.status().as_u16();
        if status == 200 {
            Ok(())
        } else {
            Err(format!("sendPhoto HTTP {status}"))
        }
    }

    fn post_json(&self, method: &str, payload: &serde_json::Value) -> Result<(), String> {
        let agent = self.agent(30);
        let resp = agent
            .post(self.url(method))
            .header("Content-Type", "application/json")
            .send(payload.to_string().as_bytes())
            .map_err(|e| describe(&e))?;
        let status = resp.status().as_u16();
        if status == 200 {
            Ok(())
        } else {
            Err(format!("{method} HTTP {status}"))
        }
    }
}

/// 錯誤訊息**只留型別與狀態**，不要把 URL（含 token）帶進去。
///
/// 回傳值有兩個用途：`println!` 的診斷行，以及 [`http_status`] 的分類
/// （401 要當成致命錯誤停掉輪詢，見 `remote::poll_loop`）。
/// ⚠️ **`HTTP <code>` 這個寫法是契約**：改了要一起改 [`http_status`] 與它的測試。
// i18n-audit:log-only-begin 只給 println! 的診斷用
fn describe(e: &ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(code) => format!("HTTP {code}"),
        ureq::Error::Timeout(_) => "逾時".to_string(),
        other => {
            // `other.to_string()` 在某些變體會含 URL → 只取型別名
            let s = format!("{other:?}");
            s.split(['(', ' ']).next().unwrap_or("未知錯誤").to_string()
        }
    }
}
// i18n-audit:log-only-end

/// 從錯誤訊息裡取出 HTTP 狀態碼（沒有就是 `None`＝不是伺服器回的錯，例如逾時／斷線）。
///
/// [`describe`] 與 [`Api::get_updates`] 的非 200 分支都把狀態寫成 `HTTP <code>`，
/// 這裡就認那個寫法。**呼叫端要拿它分「重試有沒有意義」**：
///
/// | 狀態 | 意思 | 該怎麼辦 |
/// |---|---|---|
/// | 401 | token 無效或被撤銷 | **致命**，重試永遠不會成功 |
/// | 404 | bot 不存在（token 格式對但查無此 bot） | **致命**，同上 |
/// | 409 | 同一個 bot 有別的程式在 poll | 暫時性，重試 |
/// | 5xx／逾時／斷線 | Telegram 那邊或網路 | 暫時性，重試 |
pub fn http_status(err: &str) -> Option<u16> {
    let rest = err.strip_prefix("HTTP ")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// 這個錯誤重試也不會成功嗎（401／404）。
pub fn is_fatal(err: &str) -> bool {
    matches!(http_status(err), Some(401) | Some(404))
}

/// 解析 `getUpdates` 的回覆。
pub fn parse_updates(body: &str, offset: i64) -> Result<(Vec<Incoming>, i64), String> {
    // i18n-audit:log-only-begin 同上，只會出現在診斷行
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("getUpdates JSON 壞掉：{e}"))?;
    // i18n-audit:log-only-end
    let Some(arr) = v.get("result").and_then(|r| r.as_array()) else {
        // `ok: false` 也走這裡（例：token 失效時 Telegram 回 401 ＋ description）
        return Ok((Vec::new(), offset));
    };
    let mut out = Vec::new();
    let mut next = offset;
    for upd in arr {
        let update_id = upd.get("update_id").and_then(|x| x.as_i64()).unwrap_or(0);
        if update_id >= next {
            next = update_id + 1;
        }
        // inline 按鈕點擊
        if let Some(cq) = upd.get("callback_query") {
            let chat_id = cq
                .pointer("/message/chat/id")
                .and_then(|x| x.as_i64())
                .unwrap_or(0);
            let id = cq.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let data = cq.get("data").and_then(|x| x.as_str()).unwrap_or("").to_string();
            out.push(Incoming {
                update_id,
                chat_id,
                text: None,
                callback: Some((id, data)),
            });
            continue;
        }
        // 一般訊息（`message` 或編輯過的 `edited_message`）
        let msg = upd.get("message").or_else(|| upd.get("edited_message"));
        let Some(msg) = msg else { continue };
        let chat_id = msg
            .pointer("/chat/id")
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        let text = msg.get("text").and_then(|x| x.as_str()).map(|s| s.to_string());
        out.push(Incoming {
            update_id,
            chat_id,
            text,
            callback: None,
        });
    }
    Ok((out, next))
}

#[cfg(test)]
mod tests {
    /// `HTTP <code>` 這個寫法是 `describe()` 與 `get_updates()` 的契約。
    #[test]
    fn http_status_parses_the_canonical_form() {
        use super::{http_status, is_fatal};
        assert_eq!(http_status("HTTP 401"), Some(401));
        assert_eq!(http_status("HTTP 409"), Some(409));
        assert_eq!(http_status("HTTP 500"), Some(500));
        // 不是伺服器回的錯
        assert_eq!(http_status("逾時"), None);
        assert_eq!(http_status("Transport"), None);
        assert_eq!(http_status(""), None);
        // 只有 401／404 是致命的
        assert!(is_fatal("HTTP 401"));
        assert!(is_fatal("HTTP 404"));
        assert!(!is_fatal("HTTP 409"));
        assert!(!is_fatal("HTTP 500"));
        assert!(!is_fatal("逾時"));
    }

    use super::*;

    #[test]
    fn parses_text_messages() {
        let body = r#"{"ok":true,"result":[
            {"update_id":10,"message":{"chat":{"id":123},"text":"/help"}},
            {"update_id":11,"message":{"chat":{"id":123},"text":"hello"}}
        ]}"#;
        let (msgs, next) = parse_updates(body, 0).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text.as_deref(), Some("/help"));
        assert_eq!(msgs[0].chat_id, 123);
        assert_eq!(next, 12, "offset ＝最後一個 update_id + 1");
    }

    #[test]
    fn parses_callback_queries() {
        let body = r#"{"ok":true,"result":[
            {"update_id":5,"callback_query":{"id":"cb1","data":"opt:3:2",
             "message":{"chat":{"id":99}}}}
        ]}"#;
        let (msgs, next) = parse_updates(body, 0).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].callback, Some(("cb1".into(), "opt:3:2".into())));
        assert_eq!(msgs[0].chat_id, 99);
        assert_eq!(next, 6);
    }

    /// 編輯過的訊息也算（使用者在手機上改字重送）。
    #[test]
    fn parses_edited_messages() {
        let body = r#"{"ok":true,"result":[
            {"update_id":7,"edited_message":{"chat":{"id":1},"text":"改過的"}}]}"#;
        let (msgs, _) = parse_updates(body, 0).unwrap();
        assert_eq!(msgs[0].text.as_deref(), Some("改過的"));
    }

    /// `ok: false`（token 失效）不要當成錯誤炸掉輪詢，offset 保持不變。
    #[test]
    fn tolerates_error_responses() {
        let body = r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#;
        let (msgs, next) = parse_updates(body, 42).unwrap();
        assert!(msgs.is_empty());
        assert_eq!(next, 42);
    }

    /// 壞掉的 JSON 要回 Err（輪詢會退避重試）。
    #[test]
    fn rejects_broken_json() {
        assert!(parse_updates("not json", 0).is_err());
    }

    /// **URL 不會出現在錯誤訊息裡**（裡面有 token）。
    #[test]
    fn errors_never_leak_the_url() {
        let api = Api::new("http://127.0.0.1:1", "SECRET-TOKEN-123");
        let err = api.send_message(1, "x", false, &[]).unwrap_err();
        assert!(!err.contains("SECRET-TOKEN-123"), "token 洩漏了：{err}");
        assert!(!err.contains("127.0.0.1"), "URL 洩漏了：{err}");
        let err2 = api.get_updates(0).unwrap_err();
        assert!(!err2.contains("SECRET-TOKEN-123"), "token 洩漏了：{err2}");
    }
}
