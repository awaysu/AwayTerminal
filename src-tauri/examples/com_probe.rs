//! 連接埠（COM）的端到端驗證，**不需要任何硬體**。
//!
//! 做法：`com::spawn_with_link` 吃的是一組 std 的 `Read` + `Write`（[`com::Link`]），
//! 所以這裡用**同程式內的兩條管線**當假裝置：
//!
//! ```text
//!   假裝置 ──(管線 A)──▶ reader ─▶ session ─▶ on_output   （裝置送給我們）
//!   假裝置 ◀─(管線 B)──  writer ◀─ session ◀─ write()     （我們送給裝置）
//! ```
//!
//! 驗的項目（PM 在 TASK-011 指定的那幾條）：
//!
//! 1. 「連上了」只在**開埠時**回報一次（序列裝置可能永遠不說話 → 不能等輸出）
//! 2. 資料原樣兩向（不做任何轉換）
//! 3. CR 照原樣送（沒有 CR LF 轉換）
//! 4. 大塊寫入**一次寫出**（不是逐 byte）
//! 5. 中文 UTF-8 跨「封包」不裂
//! 6. 對端關閉 → 結束事件**正好一次**
//! 7. 使用者關分頁 → 結束事件也正好一次，而且不送任何優雅結束鍵
//! 8. 參數對映：舊版有、`serialport` 沒有的值要降級**並且說出來**
//! 9. 這台機器的埠列舉（可能是空的 → `SKIP`）
//!
//! 用法：`cargo run --example com_probe`

use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use awayterminal_lib::com::{self, ComParams, Link};
use awayterminal_lib::session::{ExitInfo, OnExit, OnOutput, TerminalSession};

/// 假裝置的另一端。
struct Device {
    /// 寫進去 → session 會讀到。
    to_session: Option<Box<dyn Write + Send>>,
    /// 收集 session 寫給我們的位元組，以及**每次 `read` 拿到多少**（驗「一次寫出」）。
    got: Arc<Mutex<Vec<u8>>>,
    chunks: Arc<Mutex<Vec<usize>>>,
}

impl Device {
    fn send(&mut self, bytes: &[u8]) {
        if let Some(w) = self.to_session.as_mut() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// 關掉「裝置→session」那一頭：session 的 read 會回 0（＝拔線）。
    fn unplug(&mut self) {
        self.to_session = None;
    }

    fn received(&self) -> Vec<u8> {
        self.got.lock().unwrap().clone()
    }

    fn chunk_sizes(&self) -> Vec<usize> {
        self.chunks.lock().unwrap().clone()
    }

    fn wait_bytes(&self, n: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.received().len() >= n {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

/// 建一組「假序列埠 + 裝置端」。
fn fake_port() -> (Link, Device) {
    // 裝置 → session
    let (dev_out_r, dev_out_w) = std::io::pipe().expect("pipe");
    // session → 裝置
    let (sess_out_r, sess_out_w) = std::io::pipe().expect("pipe");

    let got = Arc::new(Mutex::new(Vec::new()));
    let chunks = Arc::new(Mutex::new(Vec::new()));
    {
        // 裝置端持續讀，記下每次讀到多少（驗「一次寫出」）
        let got = got.clone();
        let chunks = chunks.clone();
        let mut r = sess_out_r;
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        got.lock().unwrap().extend_from_slice(&buf[..n]);
                        chunks.lock().unwrap().push(n);
                    }
                }
            }
        });
    }

    let link = Link {
        reader: Box::new(dev_out_r),
        writer: Box::new(sess_out_w),
        name: "fake".to_string(),
    };
    let device = Device {
        to_session: Some(Box::new(dev_out_w)),
        got,
        chunks,
    };
    (link, device)
}

struct Sink {
    out: Arc<Mutex<Vec<u8>>>,
    exits: Arc<AtomicUsize>,
    connected: Arc<AtomicUsize>,
}

impl Sink {
    fn new() -> Sink {
        Sink {
            out: Arc::new(Mutex::new(Vec::new())),
            exits: Arc::new(AtomicUsize::new(0)),
            connected: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn callbacks(&self) -> (OnOutput, OnExit, com::OnConnected) {
        let out = self.out.clone();
        let on_output: OnOutput =
            Arc::new(move |b: &[u8]| out.lock().unwrap().extend_from_slice(b));
        let exits = self.exits.clone();
        let on_exit: OnExit = Arc::new(move |_: ExitInfo| {
            exits.fetch_add(1, Ordering::SeqCst);
        });
        let connected = self.connected.clone();
        let on_connected: com::OnConnected = Arc::new(move || {
            connected.fetch_add(1, Ordering::SeqCst);
        });
        (on_output, on_exit, on_connected)
    }

    fn bytes(&self) -> Vec<u8> {
        self.out.lock().unwrap().clone()
    }

    fn wait_bytes(&self, n: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.bytes().len() >= n {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn wait_exit(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.exits.load(Ordering::SeqCst) > 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
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

    println!("== AwayTerminal com_probe ==");

    // ---------------------------------------------------------------- 1. 開埠＝連上了
    let (link, mut device) = fake_port();
    let sink = Sink::new();
    let (on_output, on_exit, on_connected) = sink.callbacks();
    let session = com::spawn_with_link(link, on_output, on_exit, Some(on_connected));
    report(
        "開埠就算「連上了」（不等輸出）",
        sink.connected.load(Ordering::SeqCst) == 1,
        format!(
            "次數={}（要 1；序列裝置可能永遠不主動說話，等輸出會讓重連退避永遠不歸零）",
            sink.connected.load(Ordering::SeqCst)
        ),
    );
    report(
        "後端名稱與 PID",
        session.backend_name() == "serialport" && session.pid() == 0,
        format!("backend={} pid={}（PID 要 0）", session.backend_name(), session.pid()),
    );

    // ---------------------------------------------------------------- 2. 裝置 → 畫面
    device.send(b"READY>");
    let ok = sink.wait_bytes(6, Duration::from_secs(5));
    report(
        "裝置送的資料原樣進畫面",
        ok && sink.bytes() == b"READY>".to_vec(),
        format!("收到 {:?}", String::from_utf8_lossy(&sink.bytes())),
    );

    // ---------------------------------------------------------------- 3. 畫面 → 裝置（含 CR）
    session.write(b"AT\r");
    let ok = device.wait_bytes(3, Duration::from_secs(5));
    report(
        "打字原樣送出、CR 不轉換",
        ok && device.received() == b"AT\r".to_vec(),
        format!(
            "裝置收到 {:?}（要 [65, 84, 13]，不可以變成 CR LF）",
            device.received()
        ),
    );

    // ---------------------------------------------------------------- 4. 大塊一次寫出
    let big = vec![b'x'; 4096];
    let before = device.chunk_sizes().len();
    session.write(&big);
    let ok = device.wait_bytes(3 + 4096, Duration::from_secs(5));
    let sizes = device.chunk_sizes();
    let new_chunks = &sizes[before.min(sizes.len())..];
    // 管線可能自己切，但**絕不該**是 4096 次 1 byte（那就是逐 byte 寫）
    let biggest = new_chunks.iter().copied().max().unwrap_or(0);
    report(
        "貼上 4KB 是一次寫出（不是逐 byte）",
        ok && new_chunks.len() < 64 && biggest > 1,
        format!(
            "裝置端讀了 {} 次、最大一次 {biggest} bytes（逐 byte 會是 4096 次 × 1）",
            new_chunks.len()
        ),
    );

    // ---------------------------------------------------------------- 5. 中文跨「封包」
    let text = "中文測試".as_bytes();
    let base = sink.bytes().len();
    let (a, b) = text.split_at(5); // 切在某個字的中間
    device.send(a);
    std::thread::sleep(Duration::from_millis(150));
    device.send(b);
    let ok = sink.wait_bytes(base + text.len(), Duration::from_secs(5));
    let tail = sink.bytes()[base..].to_vec();
    report(
        "中文 UTF-8 跨封包不裂",
        ok && tail == text,
        format!(
            "收到 {:?}（位元組原樣={}）",
            String::from_utf8_lossy(&tail),
            tail == text
        ),
    );

    // ---------------------------------------------------------------- 6. 拔線
    device.unplug();
    let exited = sink.wait_exit(Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(200));
    report(
        "拔線（對端關閉）→ 結束事件正好一次",
        exited && sink.exits.load(Ordering::SeqCst) == 1,
        format!(
            "結束={exited}、次數={}（要 1；多一次就會排兩條重連）",
            sink.exits.load(Ordering::SeqCst)
        ),
    );
    drop(session);

    // ---------------------------------------------------------------- 7. 使用者關分頁
    let (link2, mut device2) = fake_port();
    let sink2 = Sink::new();
    let (o2, e2, c2) = sink2.callbacks();
    let session2 = com::spawn_with_link(link2, o2, e2, Some(c2));
    device2.send(b"hi");
    sink2.wait_bytes(2, Duration::from_secs(5));
    let before_close = device2.received().len();
    session2.close();
    let exited = sink2.wait_exit(Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(300));
    report(
        "關分頁 → 結束事件正好一次",
        exited && sink2.exits.load(Ordering::SeqCst) == 1,
        format!("結束={exited}、次數={}", sink2.exits.load(Ordering::SeqCst)),
    );
    report(
        "關分頁不送任何優雅結束鍵",
        device2.received().len() == before_close,
        format!(
            "關閉前後裝置收到的位元組數 {before_close} → {}（舊版 Dispose 只關 port，不送鍵）",
            device2.received().len()
        ),
    );
    // 關掉之後再寫不該有東西出去、也不該爆
    session2.write(b"ZZZ");
    std::thread::sleep(Duration::from_millis(200));
    report(
        "關閉後再打字：安靜丟掉",
        device2.received().len() == before_close,
        format!("裝置端仍是 {} bytes", device2.received().len()),
    );
    drop(session2);

    // ---------------------------------------------------------------- 8. 參數對映
    let clean = ["None", "Odd", "Even"]
        .iter()
        .all(|n| com::map_parity(n).warning.is_none())
        && ["One", "Two"].iter().all(|n| com::map_stop(n).warning.is_none())
        && ["None", "XOnXOff", "RequestToSend"]
            .iter()
            .all(|n| com::map_flow(n).warning.is_none());
    let warns = [
        com::map_parity("Mark").warning,
        com::map_stop("OnePointFive").warning,
        com::map_flow("RequestToSendXOnXOff").warning,
    ];
    let all_warn = warns.iter().all(|w| w.is_some());
    report(
        "支援的參數不出警告、不支援的**有說出來**",
        clean && all_warn,
        format!(
            "支援的乾淨={clean}；降級訊息={:?}",
            warns
                .iter()
                .map(|w| w.clone().unwrap_or_default())
                .collect::<Vec<_>>()
        ),
    );

    // ---------------------------------------------------------------- 9. 這台機器的埠
    let ports = com::list_ports();
    if ports.is_empty() {
        println!("SKIP  埠列舉：這台機器目前沒有序列埠（插上 USB 轉序列線再跑一次就會列出來）");
    } else {
        println!("      埠列舉（這台機器實際結果）：");
        for p in &ports {
            println!("      | {}  →  {}", p.name, p.label);
        }
        report(
            "埠列舉",
            true,
            format!("{} 個（名稱＋USB 描述都拿到了）", ports.len()),
        );
    }

    // 預設值（舊版 AppSettings）
    let d = ComParams::default();
    report(
        "預設值照舊版",
        d.port == "COM5" && d.baud == 115_200 && d.data_bits == 8 && d.title() == "COM5 115200",
        format!("{} {} {}／標題「{}」", d.port, d.baud, d.data_bits, d.title()),
    );

    println!("SKIP  真裝置（鮑率／流量控制／拔線重連）：需要 USB 轉序列線，見 docs/COM.md 的 C1～C14");

    println!();
    println!("RESULT: {pass} PASS / {fail} FAIL");
    if fail > 0 {
        std::process::exit(1);
    }
}
