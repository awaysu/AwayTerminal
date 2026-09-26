//! Telnet 後端的端到端驗證，**完全不靠外部網路**：測試伺服器在同一支程式裡，
//! 綁 `127.0.0.1` 的臨時埠。
//!
//! 驗的項目（PM 在 TASK-010 指定的那幾條 + 幾條舊版註解裡提過的雷）：
//!
//! 1. 連上、收到資料、「真的連上了」只回報一次
//! 2. 協商序列：`DO NAWS` → `WILL NAWS` + `SB NAWS 尺寸`
//! 3. `resize` 後重送 NAWS；**尺寸沒變不重送**
//! 4. `WILL ECHO` → `DO ECHO`、`WILL TTYPE` → `DONT TTYPE`（照舊版只認 ECHO／SGA）
//! 5. 協商序列被切在封包邊界上照樣正確（舊版踩過的雷）
//! 6. 送出的 `0xFF` 轉義成 `FF FF`；收到的 `IAC IAC` 還原成一個 `0xFF`
//! 7. Enter 送**單一 CR**（照舊版，不自己加 LF）
//! 8. 中文 UTF-8 跨封包不裂
//! 9. 伺服器斷線 → session 正常結束；重開後連得回去
//! 10. 連不上 → 紅字錯誤 + 結束事件（自動重連才接得上）
//!
//! 用法：`cargo run --example telnet_probe`

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use awayterminal_lib::session::{ExitInfo, OnExit, OnOutput, TerminalSession};
use awayterminal_lib::telnet::{self, TelnetOptions};

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_NAWS: u8 = 31;
const OPT_TTYPE: u8 = 24;
const SB_CMD: u8 = 250;
const SE_CMD: u8 = 240;
const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;

/// 一台最小 telnet 伺服器：收到的位元組全部記下來，並可以主動送任意位元組。
struct Server {
    port: u16,
    /// 收到的所有位元組（含協商）。
    got: Arc<Mutex<Vec<u8>>>,
    /// 連上的那條連線（送東西用）。
    peer: Arc<Mutex<Option<TcpStream>>>,
    listener: Arc<Mutex<Option<TcpListener>>>,
}

impl Server {
    /// `port = 0` ＝隨機挑一個臨時埠。
    fn start(port: u16) -> Server {
        let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind 127.0.0.1");
        let port = listener.local_addr().unwrap().port();
        let got = Arc::new(Mutex::new(Vec::new()));
        let peer = Arc::new(Mutex::new(None));
        let keep = Arc::new(Mutex::new(Some(listener.try_clone().unwrap())));

        {
            let got = got.clone();
            let peer = peer.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    let _ = stream.set_nodelay(true);
                    *peer.lock().unwrap() = Some(stream.try_clone().unwrap());
                    let got = got.clone();
                    std::thread::spawn(move || {
                        let mut buf = [0u8; 4096];
                        loop {
                            match stream.read(&mut buf) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => got.lock().unwrap().extend_from_slice(&buf[..n]),
                            }
                        }
                    });
                }
            });
        }
        Server {
            port,
            got,
            peer,
            listener: keep,
        }
    }

    fn send(&self, bytes: &[u8]) {
        let mut g = self.peer.lock().unwrap();
        if let Some(s) = g.as_mut() {
            let _ = s.write_all(bytes);
            let _ = s.flush();
        }
    }

    fn received(&self) -> Vec<u8> {
        self.got.lock().unwrap().clone()
    }

    /// 等到收到的位元組裡出現某個序列（或逾時）。
    fn wait_for(&self, needle: &[u8], timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if find(&self.received(), needle).is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn close_peer(&self) {
        if let Some(s) = self.peer.lock().unwrap().take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }

    /// 連 listener 一起收掉（測「伺服器不在了」）。
    fn shutdown(&self) {
        self.close_peer();
        drop(self.listener.lock().unwrap().take());
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// 收集一條 session 的輸出／結束事件。
struct Sink {
    out: Arc<Mutex<Vec<u8>>>,
    exited: Arc<AtomicBool>,
    exits: Arc<AtomicUsize>,
    connected: Arc<AtomicUsize>,
}

impl Sink {
    fn new() -> Sink {
        Sink {
            out: Arc::new(Mutex::new(Vec::new())),
            exited: Arc::new(AtomicBool::new(false)),
            exits: Arc::new(AtomicUsize::new(0)),
            connected: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn callbacks(&self) -> (OnOutput, OnExit, telnet::OnConnected) {
        let out = self.out.clone();
        let on_output: OnOutput = Arc::new(move |b: &[u8]| out.lock().unwrap().extend_from_slice(b));
        let exited = self.exited.clone();
        let exits = self.exits.clone();
        let on_exit: OnExit = Arc::new(move |_: ExitInfo| {
            exits.fetch_add(1, Ordering::SeqCst);
            exited.store(true, Ordering::SeqCst);
        });
        let connected = self.connected.clone();
        let on_connected: telnet::OnConnected =
            Arc::new(move || { connected.fetch_add(1, Ordering::SeqCst); });
        (on_output, on_exit, on_connected)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.out.lock().unwrap()).to_string()
    }

    fn bytes(&self) -> Vec<u8> {
        self.out.lock().unwrap().clone()
    }

    fn wait_text(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.text().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn wait_exit(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.exited.load(Ordering::SeqCst) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

fn naws_sb(cols: u16, rows: u16) -> Vec<u8> {
    telnet::naws_sb(cols, rows)
}

fn main() {
    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut report = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("PASS  {name}：{detail}");
        } else {
            fail += 1;
            println!("FAIL  {name}：{detail}");
        }
    };

    println!("== AwayTerminal2 telnet_probe ==");

    // ---------------------------------------------------------------- 1. 基本連線
    let server = Server::start(0);
    let sink = Sink::new();
    let (on_output, on_exit, on_connected) = sink.callbacks();
    let session = telnet::spawn(
        TelnetOptions {
            host: "127.0.0.1".to_string(),
            port: server.port,
            cols: 80,
            rows: 24,
            keepalive_mins: 0,
        },
        on_output,
        on_exit,
        Some(on_connected),
    );

    // 連上就會自己送 `IAC WILL NAWS`（多數伺服器不會主動問，PuTTY 也是連上就送）
    let offered = server.wait_for(&[IAC, WILL, OPT_NAWS], Duration::from_secs(5));
    report(
        "連上後主動提供 NAWS",
        offered,
        format!("收到 IAC WILL NAWS = {offered}"),
    );

    server.send(b"Welcome to test telnet\r\n");
    let got = sink.wait_text("Welcome", Duration::from_secs(5));
    report(
        "收得到伺服器的資料",
        got,
        format!("畫面上有 Welcome = {got}"),
    );
    report(
        "「真的連上了」只回報一次",
        sink.connected.load(Ordering::SeqCst) == 1,
        format!("次數={}（要 1；這是重連退避歸零的依據）", sink.connected.load(Ordering::SeqCst)),
    );

    // ---------------------------------------------------------------- 2. NAWS 協商
    let before = server.received().len();
    server.send(&[IAC, DO, OPT_NAWS]);
    let want = {
        let mut v = vec![IAC, WILL, OPT_NAWS];
        v.extend_from_slice(&naws_sb(80, 24));
        v
    };
    let ok = server.wait_for(&want, Duration::from_secs(5));
    report(
        "DO NAWS → WILL NAWS + 尺寸子協商",
        ok,
        format!("收到 WILL NAWS + SB NAWS 80x24 = {ok}（先前已收 {before} bytes）"),
    );

    // ---------------------------------------------------------------- 3. resize 重送
    session.resize(120, 40);
    let ok = server.wait_for(&naws_sb(120, 40), Duration::from_secs(5));
    report(
        "resize 之後重送 NAWS",
        ok,
        format!("收到 SB NAWS 120x40 = {ok}（舊版這裡是空的，NAWS 是新加的）"),
    );

    let count_before = count_occurrences(&server.received(), &naws_sb(120, 40));
    session.resize(120, 40); // 同尺寸
    std::thread::sleep(Duration::from_millis(300));
    let count_after = count_occurrences(&server.received(), &naws_sb(120, 40));
    report(
        "尺寸沒變不重送 NAWS",
        count_before == count_after,
        format!("送出次數 {count_before} → {count_after}（前端每次 fit 都會呼叫 resize）"),
    );

    // ---------------------------------------------------------------- 4. 選項回應照舊版
    server.send(&[IAC, WILL, OPT_ECHO]);
    let echo_ok = server.wait_for(&[IAC, DO, OPT_ECHO], Duration::from_secs(5));
    server.send(&[IAC, WILL, OPT_TTYPE]);
    let ttype_ok = server.wait_for(&[IAC, DONT, OPT_TTYPE], Duration::from_secs(5));
    report(
        "WILL ECHO → DO ECHO、WILL TTYPE → DONT TTYPE",
        echo_ok && ttype_ok,
        format!("ECHO={echo_ok} TTYPE={ttype_ok}（舊版只認 ECHO / SGA）"),
    );

    server.send(&[IAC, DO, OPT_SGA]);
    let sga_ok = server.wait_for(&[IAC, WILL, OPT_SGA], Duration::from_secs(5));
    server.send(&[IAC, DO, 42]);
    let other_ok = server.wait_for(&[IAC, WONT, 42], Duration::from_secs(5));
    report(
        "DO SGA → WILL SGA、其餘 DO → WONT",
        sga_ok && other_ok,
        format!("SGA={sga_ok} 其他={other_ok}"),
    );

    // -------------------------------------------- 4b. TTYPE（TASK-011，PM 決定要做）
    let offered_ttype = find(&server.received(), &[IAC, WILL, OPT_TTYPE]).is_some();
    report(
        "連上後也主動提供 TTYPE",
        offered_ttype,
        format!("收到 IAC WILL TTYPE = {offered_ttype}（同 PuTTY：連上就一起送）"),
    );

    server.send(&[IAC, DO, OPT_TTYPE]);
    let will_ttype = count_occurrences(&server.received(), &[IAC, WILL, OPT_TTYPE]) >= 1;
    server.send(&[IAC, SB_CMD, OPT_TTYPE, TTYPE_SEND, IAC, SE_CMD]);
    let mut want = vec![IAC, SB_CMD, OPT_TTYPE, TTYPE_IS];
    want.extend_from_slice(b"xterm");
    want.extend_from_slice(&[IAC, SE_CMD]);
    let is_ok = server.wait_for(&want, Duration::from_secs(5));
    report(
        "SB TTYPE SEND → SB TTYPE IS xterm",
        will_ttype && is_ok,
        format!("有 WILL TTYPE={will_ttype}、回了 IS xterm={is_ok}（舊版不認 TTYPE）"),
    );

    // -------------------------------------------- 4c. WONT / DONT（TASK-011，PM 決定要做）
    // 前面已經談成 DO ECHO；對方收回 → 這是狀態改變 → 要回 DONT ECHO
    server.send(&[IAC, WONT, OPT_ECHO]);
    let dont_echo = server.wait_for(&[IAC, DONT, OPT_ECHO], Duration::from_secs(5));
    // 沒談成過的東西被拒絕 → **不回**（不然兩邊有來有往就變無限乒乓）
    let before = server.received().len();
    server.send(&[IAC, WONT, 77, IAC, DONT, 78]);
    std::thread::sleep(Duration::from_millis(300));
    let quiet = server.received().len() == before;
    report(
        "WONT/DONT 只在狀態改變時回答",
        dont_echo && quiet,
        format!("收回 ECHO 有回 DONT={dont_echo}、沒談過的拒絕不回={quiet}（RFC 854）"),
    );

    // ---------------------------------------------------------------- 5. 協商切在封包邊界
    let mark_before = sink.bytes().len();
    server.send(&[b'A', IAC]);
    std::thread::sleep(Duration::from_millis(150));
    server.send(&[WILL, OPT_SGA, b'B']);
    let sga2 = server.wait_for(&[IAC, DO, OPT_SGA], Duration::from_secs(5));
    let shown = String::from_utf8_lossy(&sink.bytes()[mark_before..]).to_string();
    let clean = shown == "AB";
    report(
        "協商序列被切成兩個封包",
        sga2 && clean,
        format!("有回 DO SGA={sga2}、畫面只多了 {shown:?}（要 \"AB\"，不可以出現 0xFB 之類的亂碼）"),
    );

    // ---------------------------------------------------------------- 6. IAC 轉義
    session.write(&[b'x', 0xff, b'y']);
    let ok = server.wait_for(&[b'x', IAC, IAC, b'y'], Duration::from_secs(5));
    report(
        "送出的 0xFF 轉義成 FF FF",
        ok,
        format!("伺服器收到 x FF FF y = {ok}"),
    );

    let mark_before = sink.bytes().len();
    server.send(&[b'p', IAC, IAC, b'q']);
    std::thread::sleep(Duration::from_millis(300));
    let shown = sink.bytes()[mark_before..].to_vec();
    report(
        "收到的 IAC IAC 還原成一個 0xFF",
        shown == vec![b'p', 0xff, b'q'],
        format!("畫面收到 {shown:?}（要 [112, 255, 113]）"),
    );

    // ---------------------------------------------------------------- 7. CR 處理
    let mark = server.received().len();
    session.write(b"\r");
    std::thread::sleep(Duration::from_millis(300));
    let after = server.received()[mark..].to_vec();
    report(
        "Enter 送單一 CR（照舊版，不自己加 LF）",
        after == vec![b'\r'],
        format!("伺服器收到 {after:?}（舊版 TelnetSession.Write 原樣送，沒有 CR LF / CR NUL 轉換）"),
    );

    // ---------------------------------------------------------------- 8. 中文跨封包
    let mark_before = sink.bytes().len();
    let text = "中文測試".as_bytes();
    let (a, b) = text.split_at(5); // 切在某個字的中間
    server.send(a);
    std::thread::sleep(Duration::from_millis(200));
    server.send(b);
    let ok = sink.wait_text("中文測試", Duration::from_secs(5));
    let shown = sink.bytes()[mark_before..].to_vec();
    report(
        "中文 UTF-8 跨封包不裂",
        ok && shown == text,
        format!("畫面上有完整的「中文測試」= {ok}，位元組原樣={}", shown == text),
    );

    // ---------------------------------------------------------------- 9. 伺服器斷線
    server.close_peer();
    let exited = sink.wait_exit(Duration::from_secs(5));
    report(
        "伺服器斷線 → session 正常結束",
        exited && sink.exits.load(Ordering::SeqCst) == 1,
        format!(
            "結束事件={exited}、次數={}（要正好 1 次，重連才不會排兩條）",
            sink.exits.load(Ordering::SeqCst)
        ),
    );
    drop(session);

    // 重開一條連到同一台（模擬自動重連真的連回去）
    let sink2 = Sink::new();
    let (o2, e2, c2) = sink2.callbacks();
    let session2 = telnet::spawn(
        TelnetOptions {
            host: "127.0.0.1".to_string(),
            port: server.port,
            cols: 100,
            rows: 30,
            keepalive_mins: 0,
        },
        o2,
        e2,
        Some(c2),
    );
    // ⚠️ 要等伺服器**真的 accept 了新連線**才送資料，否則會送到剛剛關掉的那條上
    // （第一版 probe 就是這樣假失敗的）。客戶端一連上就會送 `IAC WILL NAWS`，拿它當信號。
    let offers_before = count_occurrences(&server.received(), &[IAC, WILL, OPT_NAWS]);
    let accepted = {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut ok = false;
        while Instant::now() < deadline {
            if count_occurrences(&server.received(), &[IAC, WILL, OPT_NAWS]) > offers_before {
                ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        ok
    };
    server.send(b"second\r\n");
    let back = accepted && sink2.wait_text("second", Duration::from_secs(5));
    report(
        "重連到同一台",
        back,
        format!("第二條連線被 accept={accepted}、收得到資料={back}"),
    );
    // 重連後的尺寸要是新的（NAWS 用連線當下的 cols/rows）
    let mark = server.received().len();
    server.send(&[IAC, DO, OPT_NAWS]);
    let ok = server.wait_for(&naws_sb(100, 30), Duration::from_secs(5));
    report(
        "重連後的 NAWS 用新的尺寸",
        ok,
        format!("收到 SB NAWS 100x30 = {ok}（先前 {mark} bytes）"),
    );
    drop(session2);

    // ---------------------------------------------------------------- 10. 連不上
    server.shutdown();
    std::thread::sleep(Duration::from_millis(200));
    let sink3 = Sink::new();
    let (o3, e3, c3) = sink3.callbacks();
    let dead = telnet::spawn(
        TelnetOptions {
            host: "127.0.0.1".to_string(),
            // 1 號埠不會有人在聽（同 ssh_probe 的做法）
            port: 1,
            cols: 80,
            rows: 24,
            keepalive_mins: 0,
        },
        o3,
        e3,
        Some(c3),
    );
    let exited = sink3.wait_exit(Duration::from_secs(25));
    let text = sink3.text();
    let red = text.contains("\x1b[31m");
    report(
        "連不上 → 紅字錯誤 + 結束事件",
        exited && red,
        format!(
            "結束={exited}、有紅字={red}、「連上了」次數={}（要 0），訊息={:?}",
            sink3.connected.load(Ordering::SeqCst),
            text.trim()
        ),
    );
    drop(dead);

    println!("SKIP  keepalive（IAC NOP）：最短間隔是 1 分鐘，probe 不等它；單元測試蓋掉組包，實機由使用者確認");

    println!();
    println!("RESULT: {pass} PASS / {fail} FAIL");
    if fail > 0 {
        std::process::exit(1);
    }
}

fn count_occurrences(hay: &[u8], needle: &[u8]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while let Some(pos) = find(&hay[i..], needle) {
        n += 1;
        i += pos + needle.len();
    }
    n
}
