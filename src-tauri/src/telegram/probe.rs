//! `--verify` 的 Telegram 區段：**假的 Bot API**（127.0.0.1）＋整條路走一遍。
//!
//! ## 絕對不打真的 Telegram
//! [`FakeBot`] 起一個只聽 `127.0.0.1` 的 `TcpListener`，[`remote::start`] 用它當 `base`。
//! 沒有任何連線出去這台機器（PM 的要求：「驗證不打真的 Telegram」）。
//! token 是寫死的 `verify-not-a-real-token`，本來就不是真的；即使如此，
//! 檢查項目裡有一條專門確認**它不會出現在任何一行輸出裡**。
//!
//! ## 為什麼是 command 而不是 `examples/` 的 probe
//! 遠端的一半行為要有真的分頁才驗得出來（把字打進 PTY、要畫面文字、要截圖、
//! 忙→閒推播）。那些只有在 app 跑起來、pane 存在的時候才有。純邏輯的部分
//! （指令解析、雜訊過濾、切段、base64）已經是 `cargo test` 的 48 個單元測試。
//!
//! 每一條檢查都印一行 `PASS`／`FAIL`，最後回給前端印在 pane 上。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::remote;

/// 驗證用的假 token。**不是真的**，而且有一條檢查確認它不會被印出去。
const FAKE_TOKEN: &str = "verify-not-a-real-token";
/// 授權的 chat id。
const CHAT: i64 = 424_242;
/// 沒授權的 chat id（要被安靜忽略）。
const OTHER_CHAT: i64 = 999_111;

/// 假的 Bot API 記下來的一次呼叫。
#[derive(Clone, Debug)]
struct Call {
    method: String,
    body: String,
}

#[derive(Default)]
struct Shared {
    /// 還沒被 `getUpdates` 領走的更新。
    queue: Vec<String>,
    /// 程式打出去的呼叫（`getUpdates` 不記，太吵）。
    calls: Vec<Call>,
    /// 下 n 次 `getUpdates` 回 [`Shared::fail_status`]（驗退避與恢復）。
    fail_updates: u32,
    /// 上面那 n 次要回的狀態碼。0 ＝ 500（預設）。
    /// TASK-036 用 409 驗「暫時性錯誤要重試而且退避遞增」。
    fail_status: u16,
    /// 不是 0 就讓 `getUpdates` **一直**回這個狀態（驗 401 這種致命錯誤要停掉輪詢）。
    fatal_status: u16,
    /// 每一次 `getUpdates` 進來的時間，用來量退避有沒有遞增。
    poll_at: Vec<Instant>,
    /// 發過幾次 `getUpdates`。
    polls: u32,
    /// `prime_offset` 用的 `offset=-1` 來過幾次。
    primes: u32,
}

struct FakeBot {
    port: u16,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    next_update: AtomicU32,
}

impl FakeBot {
    fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = shared.clone();
        let stop2 = stop.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                match conn {
                    Ok(c) => {
                        let s3 = s2.clone();
                        std::thread::spawn(move || {
                            let _ = serve(c, &s3);
                        });
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            port,
            shared,
            stop,
            next_update: AtomicU32::new(1),
        })
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 排一則使用者來訊。
    fn say(&self, chat: i64, text: &str) {
        let id = self.next_update.fetch_add(1, Ordering::Relaxed);
        let json = serde_json::json!({
            "update_id": id,
            "message": { "chat": { "id": chat }, "text": text }
        });
        self.lock().queue.push(json.to_string());
    }

    /// 排一則按鈕回呼。
    fn tap(&self, chat: i64, data: &str) {
        let id = self.next_update.fetch_add(1, Ordering::Relaxed);
        let json = serde_json::json!({
            "update_id": id,
            "callback_query": { "id": "cb1", "data": data, "message": { "chat": { "id": chat } } }
        });
        self.lock().queue.push(json.to_string());
    }

    /// 現在記錄了幾次呼叫。**每個檢查前都先拿一次**，當成 [`Self::wait`] 的起點。
    fn mark(&self) -> usize {
        self.lock().calls.len()
    }

    /// 等到**第 `from` 次之後**有一次呼叫符合條件（回傳它），或逾時回 `None`。
    ///
    /// ⚠️ `from` 不可以省。第一版從 0 開始掃，於是會match到**很久以前**的訊息：
    /// `/new 1` 那條檢查等的是含 `/last` 的回覆，結果立刻match到前面 `goto:1` 送的
    /// 「已進入 …/last 看輸出」，當場回傳、分頁還沒建好 → 假失敗（而換個順序就會是**假通過**）。
    fn wait<F: Fn(&Call) -> bool>(&self, from: usize, secs: u64, pred: F) -> Option<Call> {
        let until = Instant::now() + Duration::from_secs(secs);
        let mut from = from;
        loop {
            {
                let g = self.lock();
                for c in g.calls.iter().skip(from) {
                    if pred(c) {
                        return Some(c.clone());
                    }
                }
                from = g.calls.len();
            }
            if Instant::now() >= until {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// 發過幾次 `getUpdates`。
    ///
    /// ⚠️ 這幾個 getter 存在的理由：`bot.lock()` 在**同一個運算式裡出現兩次就會鎖死自己**
    /// （`std::sync::Mutex` 不可重入，前一個 guard 活到整條敘述結束）。第一次跑
    /// `--verify` 就是這樣卡住的：probe 抓著鎖不放 → 假 Bot API 的每個連線都卡在
    /// `shared.lock()` → 每次 `getUpdates` 都逾時、整段驗證卡到逾時才收工。
    fn polls(&self) -> u32 {
        self.lock().polls
    }

    /// `offset=-1`（prime offset）來過幾次。
    fn primes(&self) -> u32 {
        self.lock().primes
    }

    /// 目前為止的 `sendMessage`／`sendPhoto` 幾次。
    fn count(&self, method: &str) -> usize {
        self.lock().calls.iter().filter(|c| c.method == method).count()
    }
}

impl Drop for FakeBot {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // 讓 accept 醒過來收攤
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// 一條連線：讀一個 HTTP 請求、依方法回答。只認得我們自己會發的那幾種。
fn serve(mut stream: TcpStream, shared: &Arc<Mutex<Shared>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let mut parts = line.split_whitespace();
    let _verb = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("").to_string();
    // 標頭：只要 Content-Length
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 {
            break;
        }
        if h.trim().is_empty() {
            break;
        }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut body)?;
    }
    let body = String::from_utf8_lossy(&body).to_string();

    // /bot<token>/<method>[?query]
    let method = path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .to_string();

    let reply = if method == "getUpdates" {
        let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
        if path.contains("offset=-1") {
            g.primes += 1;
            // prime：回「最後一則」的 update_id，模擬關機期間累積的舊訊息
            drop(g);
            r#"{"ok":true,"result":[{"update_id":5000,"message":{"chat":{"id":0},"text":"舊的"}}]}"#
                .to_string()
        } else {
            g.polls += 1;
            g.poll_at.push(Instant::now());
            // 致命狀態（401／404）：一直回，程式應該要**停掉輪詢**而不是一直重試
            if g.fatal_status != 0 {
                let code = g.fatal_status;
                drop(g);
                return write_http(&mut stream, code, "{\"ok\":false,\"description\":\"Unauthorized\"}");
            }
            if g.fail_updates > 0 {
                g.fail_updates -= 1;
                let code = if g.fail_status == 0 { 500 } else { g.fail_status };
                drop(g);
                return write_http(&mut stream, code, "{\"ok\":false}");
            }
            let mut items = std::mem::take(&mut g.queue);
            drop(g);
            // **一定要真的 long poll**：馬上回空的話輪詢迴圈會每秒打幾千次、
            // 每次一條執行緒 → 執行緒爆掉、連線開始逾時（第一次跑實際踩到，
            // 整段 `--verify` 卡到 420 秒逾時）。真的 Telegram 是 30 秒，
            // 這裡 2 秒就夠（有訊息就立刻回）。
            if items.is_empty() {
                let until = Instant::now() + Duration::from_secs(2);
                while Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(40));
                    let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
                    if !g.queue.is_empty() {
                        items = std::mem::take(&mut g.queue);
                        break;
                    }
                }
            }
            format!("{{\"ok\":true,\"result\":[{}]}}", items.join(","))
        }
    } else {
        shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .calls
            .push(Call {
                method: method.clone(),
                body: body.clone(),
            });
        r#"{"ok":true,"result":{"message_id":1}}"#.to_string()
    };
    write_http(&mut stream, 200, &reply)
}

fn write_http(stream: &mut TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

// --------------------------------------------------------------------- 檢查

struct Report {
    lines: Vec<String>,
    ok: bool,
}

impl Report {
    fn check(&mut self, name: &str, pass: bool, detail: &str) {
        self.ok &= pass;
        let mark = if pass { "PASS" } else { "FAIL" };
        if detail.is_empty() {
            self.lines.push(format!("[verify] {mark} {name}"));
        } else {
            self.lines.push(format!("[verify] {mark} {name}　{detail}"));
        }
    }
}

/// 目前分頁列有幾列（`/new` 開完／關掉之後比對用）。
fn tab_rows_len(app: &AppHandle) -> usize {
    app.try_state::<Arc<crate::tabs::TabManager>>()
        .map(|t| t.ids().len())
        .unwrap_or(0)
}

/// 從最後一則帶 `close:` 按鈕的訊息裡挖出分頁 id。
fn last_close_id(bot: &FakeBot) -> Option<u32> {
    let g = bot.lock();
    let c = g.calls.iter().rev().find(|c| c.body.contains("close:"))?;
    let at = c.body.rfind("close:")? + "close:".len();
    let rest = &c.body[at..];
    let end = rest.find(|ch: char| !ch.is_ascii_digit())?;
    rest[..end].parse().ok()
}

/// 從 `sendMessage` 的 JSON 裡挖出 `text`。
fn text_of(c: &Call) -> String {
    serde_json::from_str::<serde_json::Value>(&c.body)
        .ok()
        .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

/// 跑 Telegram 遠端的驗證。`tab`＝可以拿來打字／截圖的分頁（`None`＝跳過要分頁的項目）。
///
/// **不會動到使用者的設定**：`remote::start` 直接吃參數，不經過 `SettingsStore`。
#[tauri::command]
pub async fn telegram_probe(app: AppHandle, tab: Option<u32>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || run(&app, tab))
        .await
        .map_err(|e| format!("telegram_probe 沒跑完：{e}"))?
}

fn run(app: &AppHandle, tab: Option<u32>) -> Result<Vec<String>, String> {
    let bot = FakeBot::start().map_err(|e| format!("假 Bot API 起不來：{e}"))?;
    let mut r = Report {
        lines: vec![format!(
            "[verify] Telegram 遠端（假 Bot API 在 127.0.0.1:{}，不連外）",
            bot.port
        )],
        ok: true,
    };

    // 遠端本來就沒在跑（使用者沒開，或前一段驗完收乾淨了）
    r.check("開始前遠端沒在跑", !remote::is_running(), "");

    let started = remote::start(app, FAKE_TOKEN, CHAT, false, Some(&bot.base()));
    r.check("啟動", started && remote::is_running(), "");

    // 1) 啟動三件事：prime offset（offset=-1）、註冊指令選單、上線通知
    // 起點 0 是對的：`bot` 是這一段剛建的，前面沒有任何呼叫可以誤 match。
    let online = bot.wait(0, 10, |c| c.method == "sendMessage");
    let primes = bot.primes();
    r.check(
        "啟動先 prime offset（offset=-1，不重播關機期間的舊訊息）",
        primes >= 1,
        &format!("primes={primes}"),
    );
    r.check(
        "註冊指令選單 setMyCommands",
        bot.wait(0, 5, |c| c.method == "setMyCommands").is_some(),
        "",
    );
    r.check(
        "送上線通知",
        online.as_ref().map(|c| text_of(c).contains("AwayTerminal")).unwrap_or(false),
        "",
    );

    // 2) 非授權的 chat：不理也不回
    let before = bot.count("sendMessage");
    bot.say(OTHER_CHAT, "/help");
    std::thread::sleep(Duration::from_millis(900));
    r.check(
        "非授權 chat 的指令不理也不回",
        bot.count("sendMessage") == before,
        &format!("sendMessage 沒有增加（{before}）"),
    );

    // 3) /help
    let m2 = bot.mark();
    bot.say(CHAT, "/help");
    let help = bot.wait(m2, 5, |c| c.method == "sendMessage" && text_of(c).contains("/goto"));
    r.check("/help 回指令一覽", help.is_some(), "");

    // 4) 沒選分頁就 /last
    let m3 = bot.mark();
    bot.say(CHAT, "/last");
    let na = bot.wait(m3, 5, |c| c.method == "sendMessage" && text_of(c).contains("/goto"));
    r.check("沒附著分頁時 /last 提示先 /goto", na.is_some(), "");

    // 5) /goto（不帶編號）列分頁，附 inline 按鈕
    let m4 = bot.mark();
    bot.say(CHAT, "/goto");
    let list = bot.wait(m4, 5, |c| c.method == "sendMessage" && c.body.contains("goto:"));
    r.check("/goto 列分頁並附按鈕", list.is_some(), "");

    // 6) 開關類指令
    let m5 = bot.mark();
    bot.say(CHAT, "/notify on");
    r.check(
        "/notify on 有回覆",
        bot.wait(m5, 5, |c| c.method == "sendMessage" && !text_of(c).is_empty())
            .is_some(),
        "",
    );

    // 7) 未知指令
    let m6 = bot.mark();
    bot.say(CHAT, "/nosuchthing");
    r.check(
        "未知指令回提示",
        bot.wait(m6, 5, |c| c.method == "sendMessage" && text_of(c).contains("/help"))
            .is_some(),
        "",
    );

    // 8) /new：列可開的連線（至少有 PowerShell（桌面）與 ADB）
    let m7 = bot.mark();
    bot.say(CHAT, "/new");
    let newlist = bot.wait(m7, 6, |c| c.method == "sendMessage" && c.body.contains("new:"));
    r.check("/new 列可開的連線並附按鈕", newlist.is_some(), "");
    r.check(
        "/new 的清單含 PowerShell（桌面）",
        newlist
            .as_ref()
            .map(|c| text_of(c).contains("PowerShell"))
            .unwrap_or(false),
        "",
    );

    // 9) /history：列最近連線（v2 ＝我的最愛，見 remote::do_history 的註解）
    let m8 = bot.mark();
    bot.say(CHAT, "/history");
    r.check(
        "/history 列清單並附按鈕",
        bot.wait(m8, 6, |c| c.method == "sendMessage" && c.body.contains("hist:"))
            .is_some(),
        "",
    );

    // 10) 編號超出範圍要回「1~n」，不可以真的去開東西
    let opens_before = bot.count("sendMessage");
    let m9 = bot.mark();
    bot.say(CHAT, "/new 999");
    let ranged = bot.wait(m9, 6, |c| c.method == "sendMessage" && text_of(c).contains('~'));
    r.check(
        "/new 編號超出範圍只回提示（不開分頁）",
        ranged.is_some() && bot.count("sendMessage") > opens_before,
        "",
    );

    // 11) `/ssh`／`/telnet` 不帶參數：**只有在使用者沒有那一型的我的最愛時才敢測**。
    //
    // ⚠️ 有最愛的話這兩個指令會真的去連那台主機——`--verify` 絕不可以連外（PM 的規則）。
    // 解析層的行為（含 `[user@]主機[:埠]`、IPv6 被切壞）已經有單元測試，這裡只測
    // 「沒有最愛時回用法」這一條，其餘跳過並印出原因。
    let favs = app
        .try_state::<Arc<crate::settings::SettingsStore>>()
        .map(|st| {
            let s = st.get();
            (
                s.favorites.iter().any(|f| f.kind == "ssh"),
                s.favorites.iter().any(|f| f.kind == "telnet"),
            )
        })
        .unwrap_or((true, true));
    if !favs.0 {
        let m10 = bot.mark();
        bot.say(CHAT, "/ssh");
        r.check(
            "/ssh 不帶參數、沒有 SSH 最愛 → 回用法（不連外）",
            bot.wait(m10, 6, |c| c.method == "sendMessage" && text_of(c).contains("/ssh"))
                .is_some(),
            "",
        );
    } else {
        r.lines.push(
            "[verify] SKIP /ssh 不帶參數（使用者有 SSH 最愛，測它會真的連外）".to_string(),
        );
    }
    if !favs.1 {
        let m11 = bot.mark();
        bot.say(CHAT, "/telnet");
        r.check(
            "/telnet 不帶參數、沒有 Telnet 最愛 → 回用法（不連外）",
            bot.wait(m11, 6, |c| c.method == "sendMessage" && text_of(c).contains("/telnet"))
                .is_some(),
            "",
        );
    } else {
        r.lines.push(
            "[verify] SKIP /telnet 不帶參數（使用者有 Telnet 最愛，測它會真的連外）".to_string(),
        );
    }

    // 11b) 真的開一個分頁：走 `/new 1` ＝清單第一項「PowerShell（桌面）」。
    // **只開本機 shell**，不碰任何網路；開完自己關掉，不留東西給使用者。
    let tabs_before = tab_rows_len(app);
    let m12 = bot.mark();
    bot.say(CHAT, "/new 1");
    let opened = bot.wait(m12, 12, |c| c.method == "sendMessage" && text_of(c).contains("/last"));
    let tabs_after = tab_rows_len(app);
    r.check(
        "/new 1 真的開了分頁並自動附著（telegram-open → telegram_opened）",
        opened.is_some() && tabs_after == tabs_before + 1,
        &format!("分頁 {tabs_before} → {tabs_after}"),
    );
    if tabs_after > tabs_before {
        // 收乾淨：用遠端自己的 `/close` ＋確認按鈕，順便驗確認流程真的會關
        let m13 = bot.mark();
        bot.say(CHAT, "/close");
        if bot
            .wait(m13, 6, |c| c.method == "sendMessage" && c.body.contains("close:"))
            .is_some()
        {
            // 從那則訊息挖出 `close:{id}` 再按下去
            if let Some(id) = last_close_id(&bot) {
                bot.tap(CHAT, &format!("close:{id}"));
                std::thread::sleep(Duration::from_millis(1200));
            }
        }
        r.check(
            "按「確定關閉」之後那個分頁收掉了",
            tab_rows_len(app) == tabs_before,
            &format!("分頁回到 {tabs_before}"),
        );
    }

    // 12) 輪詢錯誤 → 退避 3 秒後恢復（不會就此停掉，舊版 1.1.9 的教訓）
    let polls_before = bot.polls();
    bot.lock().fail_updates = 2;
    let until = Instant::now() + Duration::from_secs(12);
    let mut recovered = false;
    while Instant::now() < until {
        let (fails, polls) = {
            let g = bot.lock();
            (g.fail_updates, g.polls)
        };
        if fails == 0 && polls > polls_before + 2 {
            recovered = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    r.check(
        "輪詢失敗後退避並恢復（不會永久停掉）",
        recovered,
        &format!("polls {polls_before} → {}", bot.polls()),
    );

    // 9) 需要真的分頁的部分
    match tab {
        Some(id) => {
            // /goto <n>：用按鈕回呼，編號從 /goto 的清單來——這裡直接用 1（驗證環境第一個分頁）
            let m15 = bot.mark();
            bot.tap(CHAT, "goto:1");
            let entered = bot.wait(m15, 6, |c| c.method == "sendMessage" && text_of(c).contains("/exit"));
            r.check("按按鈕進分頁（callback goto:1）", entered.is_some(), "");

            // 打字進分頁：看得到就算成功（前端的 `q…text` 回得來）
            let before = bot.count("sendMessage");
            bot.say(CHAT, "echo verify-telegram-marker");
            std::thread::sleep(Duration::from_millis(1500));
            let screen = super::screen::recent_text(app, id, Duration::from_secs(2));
            r.check(
                "純文字打進分頁（畫面上找得到）",
                screen.contains("verify-telegram-marker"),
                &format!("畫面 {} 字", screen.chars().count()),
            );
            // 這裡等的是「忙→閒」的完成推播（`status.rs` 的 600ms 輪詢算出來的）。
            // 重點不是「有沒有推」而是**只推一則**：舊版曾經「送出後固定延遲回推」＋
            // 完成推播兩條路都送，使用者實測一次 `hi` 收到三則。
            std::thread::sleep(Duration::from_millis(2500));
            let pushes = bot.count("sendMessage") - before;
            r.check(
                "送出一行之後最多回一則（不會一問三答）",
                pushes <= 1,
                &format!("{pushes} 則"),
            );

            // /last：要拿到畫面文字
            let m17 = bot.mark();
            bot.say(CHAT, "/last 10");
            let last = bot.wait(m17, 6, |c| c.method == "sendMessage" && c.body.contains("<pre>"));
            r.check("/last 回畫面文字（HTML <pre>）", last.is_some(), "");

            // /shot：要是有效的 PNG
            let png = super::shot::capture_png(app, id);
            let good = png.as_ref().map(|p| super::shot::is_png(p) && p.len() > 1000);
            r.check(
                "截圖是有效的 PNG",
                good == Some(true),
                &format!("{} 位元組", png.as_ref().map(|p| p.len()).unwrap_or(0)),
            );
            let m18 = bot.mark();
            bot.say(CHAT, "/shot");
            r.check(
                "/shot 走 sendPhoto",
                bot.wait(m18, 8, |c| c.method == "sendPhoto").is_some(),
                "",
            );

            // 長訊息切段：4096 是 Telegram 的硬上限
            let long = "一二三四五六七八九十".repeat(500); // 5000 字
            let parts = super::tidy::split_message(&long, super::tidy::TELEGRAM_LIMIT);
            let all_fit = parts
                .iter()
                .all(|p| p.chars().count() <= super::tidy::TELEGRAM_LIMIT);
            r.check(
                "超長訊息切成多段、每段不超過 4096",
                parts.len() >= 2 && all_fit,
                &format!("{} 段", parts.len()),
            );

            // 完成推播：忙→閒只推一則（8 秒內同內容去重）
            let before = bot.count("sendMessage");
            let title = "verify";
            remote::on_tab_idle(app, id, title);
            std::thread::sleep(Duration::from_millis(1200));
            let after_one = bot.count("sendMessage");
            remote::on_tab_idle(app, id, title);
            std::thread::sleep(Duration::from_millis(1200));
            let after_two = bot.count("sendMessage");
            r.check(
                "完成推播只推一則（同內容 8 秒內去重）",
                after_two == after_one && after_one >= before,
                &format!("{before} → {after_one} → {after_two}"),
            );

            // /close 要先問過才關（誤觸保險）——只驗「有問」，不真的按確定
            let m19 = bot.mark();
            bot.say(CHAT, "/close");
            let ask = bot.wait(m19, 6, |c| c.method == "sendMessage" && c.body.contains("close:"));
            r.check("/close 先出確認按鈕（不直接關）", ask.is_some(), "");
            bot.tap(CHAT, "noop");
            std::thread::sleep(Duration::from_millis(700));
            r.check(
                "按取消之後分頁還在",
                app.try_state::<crate::session::SessionManager>()
                    .and_then(|m| m.get(id))
                    .is_some(),
                "",
            );

            bot.say(CHAT, "/exit");
            std::thread::sleep(Duration::from_millis(600));
        }
        None => r
            .lines
            .push("[verify] SKIP 需要分頁的項目（沒有可用的分頁）".to_string()),
    }

    // 10) token 不進任何輸出
    let leaked = r.lines.iter().any(|l| l.contains(FAKE_TOKEN));
    r.check("token 不出現在任何一行輸出裡", !leaked, "");

    // 12b) 409（別的程式在 poll）＝暫時性 → 要重試，而且**退避要遞增**（TASK-036）
    {
        bot.lock().poll_at.clear();
        bot.lock().fail_status = 409;
        // **剛好 3 次**：失敗 3 次（退避 3→6→12 秒）之後第 4 次會成功，
        // 所以 `poll_at` 會有 4 筆 → 量得到 3 個間隔，而且第 4 筆本身就證明恢復了。
        // 設 4 次的話最後要再等 24 秒的退避，整段 `--verify` 平白多半分鐘。
        bot.lock().fail_updates = 3;
        let until = Instant::now() + Duration::from_secs(40);
        while Instant::now() < until && bot.lock().poll_at.len() < 4 {
            std::thread::sleep(Duration::from_millis(200));
        }
        // 收集相鄰兩次 getUpdates 的間隔；退避是 3 → 6 → 12 秒，所以間隔要一路變大
        let gaps: Vec<u64> = {
            let g = bot.lock();
            g.poll_at.windows(2).map(|w| w[1].duration_since(w[0]).as_millis() as u64).collect()
        };
        let grew = gaps.len() >= 3 && gaps[1] > gaps[0] + 1000 && gaps[2] > gaps[1] + 1000;
        r.check(
            "409 會重試而且退避遞增（3→6→12 秒）",
            grew,
            &format!("間隔(ms)={gaps:?}"),
        );
        bot.lock().fail_status = 0;
        // 恢復之後退避要歸零：第 4 次（成功那次）之後下一次應該**馬上**來
        //（long poll 2 秒），不是再等 24 秒
        let n_before = bot.lock().poll_at.len();
        let until = Instant::now() + Duration::from_secs(15);
        while Instant::now() < until && bot.lock().poll_at.len() <= n_before {
            std::thread::sleep(Duration::from_millis(200));
        }
        let after = bot.lock().poll_at.len();
        let last_gap = {
            let g = bot.lock();
            g.poll_at
                .windows(2)
                .last()
                .map(|w| w[1].duration_since(w[0]).as_millis() as u64)
                .unwrap_or(u64::MAX)
        };
        r.check(
            "409 結束後輪詢恢復，而且退避歸零（下一次很快就來）",
            after > n_before && last_gap < 8_000,
            &format!("最後一個間隔 {last_gap} ms（歸零的話應該只有 long poll 的 2 秒多）"),
        );
    }

    // 12c) 401＝token 失效，**重試永遠不會成功** → 要停掉輪詢、關掉設定、通知一次（TASK-036）
    {
        // ⚠️ 這一段會把設定裡的 `remoteEnabled` 關掉（那正是要驗的行為），
        // 所以先記下使用者原本的值，最後一定要放回去。
        let enabled_before = {
            use tauri::Manager;
            app.state::<std::sync::Arc<crate::settings::SettingsStore>>().get().remote_enabled
        };
        bot.lock().fatal_status = 401;
        let polls_at_start = bot.polls();
        // 等它自己停掉（輪詢逾時 2 秒 + 退避，給 25 秒）
        let until = Instant::now() + Duration::from_secs(25);
        while Instant::now() < until && remote::is_running() {
            std::thread::sleep(Duration::from_millis(200));
        }
        r.check("401 之後遠端自己停了（不再重試）", !remote::is_running(), "");

        // 停了就不可以再打 getUpdates
        let polls_when_stopped = bot.polls();
        std::thread::sleep(Duration::from_secs(5));
        r.check(
            "停掉之後真的沒有再打 getUpdates",
            bot.polls() == polls_when_stopped,
            &format!("polls {polls_at_start} → {polls_when_stopped} → {}", bot.polls()),
        );
        // 打的次數要是「個位數」——舊版 12 分鐘打 240 次就是這一條要擋的
        r.check(
            "401 之後只打了幾次就放棄（舊版是無限重試）",
            polls_when_stopped - polls_at_start <= 5,
            &format!("這段期間打了 {} 次", polls_when_stopped - polls_at_start),
        );

        let reason = remote::stopped_reason();
        r.check(
            "記下停止原因（遠端設定視窗要顯示）",
            reason.as_deref().map(|t| t.contains("401")).unwrap_or(false),
            &format!("{reason:?}"),
        );
        let enabled_now = {
            use tauri::Manager;
            app.state::<std::sync::Arc<crate::settings::SettingsStore>>().get().remote_enabled
        };
        r.check("設定裡的 remoteEnabled 被關掉（下次啟動不會再重試）", !enabled_now, "");

        // 還原使用者的設定與旗標
        bot.lock().fatal_status = 0;
        {
            use tauri::Manager;
            app.state::<std::sync::Arc<crate::settings::SettingsStore>>()
                .update(|s| s.remote_enabled = enabled_before);
        }
        r.lines.push(format!(
            "[verify] 已還原使用者的 remoteEnabled＝{enabled_before}（驗證不留痕跡）"
        ));
    }

    remote::stop();
    std::thread::sleep(Duration::from_millis(200));
    r.check("停止之後遠端不再跑", !remote::is_running(), "");

    // 11) 遠端設定視窗的「取得 chat id」（TASK-029）。
    // **一定要在 `remote::stop()` 之後**：輪詢那條連線會把更新領走，還在跑的話這裡會拿到空的。
    bot.say(CHAT, "hello from phone");
    let got = super::api::Api::new(&bot.base(), FAKE_TOKEN).latest_chat_id();
    r.check(
        "取得 chat id 抓到最近一則訊息的 chat",
        matches!(got, Ok(Some(id)) if id == CHAT),
        &format!("回傳 {got:?}（要 Ok(Some({CHAT}))）"),
    );

    r.lines.push(format!(
        "[verify] Telegram 遠端：{}",
        if r.ok { "全部通過" } else { "有項目失敗" }
    ));
    Ok(r.lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假 Bot API 真的會回答（不然整段驗證都是假的通過）。
    #[test]
    fn fake_bot_answers() {
        let bot = FakeBot::start().expect("起不來");
        let api = super::super::api::Api::new(&bot.base(), FAKE_TOKEN);
        // prime 回的是「最後一則」→ offset 要是 5001，不是 0
        assert_eq!(api.prime_offset(), 5001);
        bot.say(CHAT, "hello");
        let (msgs, next) = api.get_updates(0).expect("getUpdates 失敗");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text.as_deref(), Some("hello"));
        assert!(next > 0);
        api.send_message(CHAT, "hi", false, &[]).expect("sendMessage 失敗");
        assert_eq!(bot.count("sendMessage"), 1);
    }

    /// `getUpdates` 失敗計數會被吃掉（驗退避那一段靠它）。
    #[test]
    fn fake_bot_can_fail_on_purpose() {
        let bot = FakeBot::start().expect("起不來");
        let api = super::super::api::Api::new(&bot.base(), FAKE_TOKEN);
        bot.lock().fail_updates = 1;
        assert!(api.get_updates(0).is_err(), "第一次應該失敗");
        assert!(api.get_updates(0).is_ok(), "第二次應該成功");
    }
}
