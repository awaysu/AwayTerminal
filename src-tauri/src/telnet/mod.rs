//! 內建 Telnet（TASK-010）。
//!
//! 搬移舊版 `Sessions/TelnetSession.cs`，**行為逐項照舊版**，只加 `CLAUDE.md`
//! 已經定案的 NAWS（視窗大小通知）。行為表在 `docs/TELNET.md`。
//!
//! | 舊版 | 這裡 |
//! |---|---|
//! | 連線放背景（同步 connect 在主機不通時會卡到 TCP 逾時約 20 秒） | 連線在自己的執行緒上做 |
//! | 連不上：紅字印錯誤 + 觸發 `Exited`（自動重連會接手） | 同 |
//! | `WILL ECHO`／`WILL SGA` → `DO`；其餘 `WILL` → `DONT` | [`Iac::respond`] |
//! | `DO SGA` → `WILL`；其餘 `DO` → `WONT` | 同（多了 `DO NAWS` → `WILL`＋尺寸） |
//! | `WONT`／`DONT` **不回應** | 同（PuTTY 會回，我們照舊版；見 `docs/TELNET.md`） |
//! | 子協商（`IAC SB … IAC SE`）內容一律略過 | [`Iac`] 的 `Sb`／`SbIac` 狀態 |
//! | IAC 解析狀態要跨讀取邊界保留 | `Iac` 是有狀態的，read 之間不重設（舊版踩過這個雷） |
//! | 送出時 `0xFF` 轉義成 `FF FF`，一次寫出（不逐 byte） | [`escape`] + 單次 `write_all` |
//! | keepalive＝每 N 分鐘一個 `IAC NOP`（0＝關），直接寫串流不轉義 | [`keepalive_thread`] |
//! | 關閉分頁**不送優雅結束鍵**（只關 socket） | [`TelnetSession::close`] |
//! | `ProcessId` ＝ 0 | [`TelnetSession::pid`] |
//! | `Resize` 是空的（`// NAWS 可選，暫略`） | **改成真的送 NAWS**（`CLAUDE.md`：「Telnet 自己實作（加 NAWS）」） |

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::session::{ExitInfo, OnExit, OnOutput, TerminalSession};

/// Telnet 指令（RFC 854）。
const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const NOP: u8 = 241;

/// 選項編號。`ECHO`／`SGA` 是舊版就處理的兩個；`NAWS` 是新加的。
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_NAWS: u8 = 31;

/// 一條 Telnet 連線的參數。
///
/// 和 [`crate::ssh::conn::SshConnParams`] 一樣，這個結構同時是「分頁層重連要記住的東西」、
/// 「連線對話框的欄位」與「我的最愛存的內容」。Telnet 沒有帳號密碼的概念（要登入是遠端自己問的），
/// 所以本來就沒有祕密可存。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TelnetParams {
    pub host: String,
    pub port: u16,
    /// 保持連線的間隔（分鐘），0＝關。舊版的連線視窗這個欄位 SSH／Telnet 共用。
    pub keepalive_mins: u32,
    /// 斷線自動重連（同上，舊版 SSH／Telnet 共用一個勾選）。
    pub auto_reconnect: bool,
}

impl Default for TelnetParams {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 23,
            keepalive_mins: 0,
            auto_reconnect: false,
        }
    }
}

/// 建立連線要的東西（參數 + 初始尺寸）。
pub struct TelnetOptions {
    pub host: String,
    pub port: u16,
    pub cols: u16,
    pub rows: u16,
    pub keepalive_mins: u32,
}

/// IAC 解析狀態機。
///
/// 抽成獨立結構有兩個原因：① 狀態**必須**跨 `read` 保留（舊版註解寫得很清楚：協商序列落在
/// 讀取邊界上時，舊寫法會把後半當資料印到畫面、協商也沒回應）；② 這樣可以完全不碰 socket 做單元測試。
#[derive(Debug, Default)]
pub struct Iac {
    state: State,
    cmd: u8,
    /// 對方同意我們送視窗大小了嗎（收到 `DO NAWS`）。
    pub naws_ok: bool,
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
enum State {
    #[default]
    Data,
    Iac,
    Option,
    Sb,
    SbIac,
}

impl Iac {
    /// 吃一段收到的位元組，回傳 (要寫進終端機的資料, 要回給對方的協商位元組)。
    pub fn process(&mut self, buf: &[u8], cols: u16, rows: u16) -> (Vec<u8>, Vec<u8>) {
        let mut data = Vec::with_capacity(buf.len());
        let mut reply = Vec::new();
        for &b in buf {
            match self.state {
                State::Data => {
                    if b == IAC {
                        self.state = State::Iac;
                    } else {
                        data.push(b);
                    }
                }
                State::Iac => {
                    if b == IAC {
                        data.push(IAC); // 轉義的 0xFF＝資料
                        self.state = State::Data;
                    } else if matches!(b, WILL | WONT | DO | DONT) {
                        self.cmd = b;
                        self.state = State::Option;
                    } else if b == SB {
                        self.state = State::Sb;
                    } else {
                        // NOP／GA／AYT… 兩位元組指令：丟掉（同舊版）
                        self.state = State::Data;
                    }
                }
                State::Option => {
                    self.respond(self.cmd, b, cols, rows, &mut reply);
                    self.state = State::Data;
                }
                State::Sb => {
                    // 子協商內容一律略過（同舊版）
                    if b == IAC {
                        self.state = State::SbIac;
                    }
                }
                State::SbIac => {
                    // IAC SE 結束；IAC IAC ＝子協商資料裡的 0xFF，還在子協商裡
                    self.state = if b == SE { State::Data } else { State::Sb };
                }
            }
        }
        (data, reply)
    }

    /// 回應一個選項協商。**規則照舊版 `RespondOption`**，只多了 `DO NAWS`。
    fn respond(&mut self, cmd: u8, opt: u8, cols: u16, rows: u16, out: &mut Vec<u8>) {
        match cmd {
            WILL => {
                let reply = if opt == OPT_ECHO || opt == OPT_SGA { DO } else { DONT };
                out.extend_from_slice(&[IAC, reply, opt]);
            }
            DO => {
                if opt == OPT_NAWS {
                    // 新增：答應送視窗大小，並立刻送一次目前尺寸
                    self.naws_ok = true;
                    out.extend_from_slice(&[IAC, WILL, OPT_NAWS]);
                    out.extend_from_slice(&naws_sb(cols, rows));
                } else {
                    let reply = if opt == OPT_SGA { WILL } else { WONT };
                    out.extend_from_slice(&[IAC, reply, opt]);
                }
            }
            // WONT／DONT：舊版不回應（PuTTY 會回，差異寫在 docs/TELNET.md）
            _ => {}
        }
    }
}

/// `IAC SB NAWS <寬 16-bit> <高 16-bit> IAC SE`（RFC 1073）。
/// 尺寸位元組剛好是 `0xFF` 時要轉義成 `FF FF`，否則會被當成 IAC。
pub fn naws_sb(cols: u16, rows: u16) -> Vec<u8> {
    let mut v = vec![IAC, SB, OPT_NAWS];
    for b in [
        (cols >> 8) as u8,
        (cols & 0xff) as u8,
        (rows >> 8) as u8,
        (rows & 0xff) as u8,
    ] {
        v.push(b);
        if b == IAC {
            v.push(IAC);
        }
    }
    v.extend_from_slice(&[IAC, SE]);
    v
}

/// 鍵盤輸入的轉義：`0xFF` → `FF FF`（同舊版 `Write`）。
pub fn escape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 8);
    for &b in data {
        if b == IAC {
            out.push(IAC);
        }
        out.push(b);
    }
    out
}

/// 一條 Telnet 連線。
pub struct TelnetSession {
    /// 連上之後才有；寫入從這裡拿（同舊版的 `volatile NetworkStream? _stream`）。
    stream: Mutex<Option<TcpStream>>,
    /// 目前的終端機尺寸（NAWS 用）。
    size: Mutex<(u16, u16)>,
    /// 對方同意我們送 NAWS 了嗎（`DO NAWS` 收到才是 true）。
    naws_ok: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    /// 診斷用：連上之後填入遠端位址。
    peer: Mutex<String>,
    keepalive_mins: u32,
    /// 0＝遠端連線沒有本機子行程（同舊版 `ProcessId => 0`）。
    pid: AtomicU32,
}

impl TelnetSession {
    fn write_raw(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut g = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = g.as_mut() {
            let _ = s.write_all(bytes);
            let _ = s.flush();
        }
    }

    pub fn peer(&self) -> String {
        self.peer.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl TerminalSession for TelnetSession {
    fn write(&self, data: &[u8]) {
        // 先轉義再一次送出：`NoDelay` 下逐 byte 寫＝每個字一個 TCP 封包，
        // 貼 4KB 會變上千個封包，小型 telnet 裝置會掉（舊版註解）。
        self.write_raw(&escape(data));
    }

    fn resize(&self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }
        {
            let mut g = self.size.lock().unwrap_or_else(|e| e.into_inner());
            if *g == (cols, rows) {
                return; // 尺寸沒變就不要一直送（舊版的 r 協定每次 fit 都會來）
            }
            *g = (cols, rows);
        }
        // 對方沒答應 NAWS 就不能送子協商（RFC 1073：要先 DO NAWS）
        if self.naws_ok.load(Ordering::Relaxed) {
            self.write_raw(&naws_sb(cols, rows));
        }
    }

    fn pid(&self) -> u32 {
        self.pid.load(Ordering::Relaxed)
    }

    fn backend_name(&self) -> &'static str {
        "telnet"
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // 舊版 Dispose：沒有優雅結束鍵，直接關 socket（讀取執行緒的 read 會回 0 →
        // 走 finally → Exited）。
        let g = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = g.as_ref() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl Drop for TelnetSession {
    fn drop(&mut self) {
        self.close();
    }
}

/// 連上而且**第一次收到資料**時呼叫一次。
///
/// 這是 Telnet 版的「真的連上了」——Telnet 沒有 SSH 的 shell channel 可以當里程碑。
/// ⚠️ 不可以用「有輸出」當條件（那是舊版 SSH 的做法）：我們自己的狀態訊息也走輸出 callback，
/// 會被誤判。這裡用的是**從 socket 讀到位元組**，我們自己的訊息不會經過那裡。
/// 詳見 `docs/REGRESSION-CHECKLIST.md`「隱含契約」那條總則。
pub type OnConnected = Arc<dyn Fn() + Send + Sync>;

/// 開一條 Telnet 連線。連線本身在背景執行緒上做（同舊版：同步 connect 會卡 UI）。
pub fn spawn(
    opts: TelnetOptions,
    on_output: OnOutput,
    on_exit: OnExit,
    on_connected: Option<OnConnected>,
) -> Arc<TelnetSession> {
    let session = Arc::new(TelnetSession {
        stream: Mutex::new(None),
        size: Mutex::new((opts.cols, opts.rows)),
        naws_ok: Arc::new(AtomicBool::new(false)),
        closed: Arc::new(AtomicBool::new(false)),
        peer: Mutex::new(String::new()),
        keepalive_mins: opts.keepalive_mins,
        pid: AtomicU32::new(0),
    });

    let s = session.clone();
    std::thread::Builder::new()
        .name(format!("telnet-{}:{}", opts.host, opts.port))
        .spawn(move || connect_and_read(s, opts, on_output, on_exit, on_connected))
        .expect("spawn telnet thread");

    session
}

fn connect_and_read(
    session: Arc<TelnetSession>,
    opts: TelnetOptions,
    on_output: OnOutput,
    on_exit: OnExit,
    on_connected: Option<OnConnected>,
) {
    let target = format!("{}:{}", opts.host, opts.port);
    let stream = match connect(&target) {
        Ok(s) => s,
        Err(e) => {
            if !session.closed.load(Ordering::Relaxed) {
                // 舊版：連不上就紅字印錯誤並觸發 Exited（勾了自動重連的話由它接手退避重試）
                on_output(format!("\r\n\x1b[31m{e}\x1b[0m\r\n").as_bytes());
                on_exit(ExitInfo { exit_code: None });
            }
            return;
        }
    };
    if let Ok(peer) = stream.peer_addr() {
        *session.peer.lock().unwrap_or_else(|e| e.into_inner()) = peer.to_string();
    }
    let reader = match stream.try_clone() {
        Ok(r) => r,
        Err(e) => {
            on_output(format!("\r\n\x1b[31m{e}\x1b[0m\r\n").as_bytes());
            on_exit(ExitInfo { exit_code: None });
            return;
        }
    };
    *session.stream.lock().unwrap_or_else(|e| e.into_inner()) = Some(stream);

    // 使用者在連線還沒完成時就關了分頁
    if session.closed.load(Ordering::Relaxed) {
        session.close();
        on_exit(ExitInfo { exit_code: None });
        return;
    }

    // NAWS 必須自己先開口：多數 telnet 伺服器不會主動問（PuTTY 也是連上就送 WILL NAWS）。
    // 對方回 `DO NAWS` 才會真的送尺寸（見 Iac::respond）。
    session.write_raw(&[IAC, WILL, OPT_NAWS]);

    if session.keepalive_mins > 0 {
        keepalive_thread(session.clone());
    }

    read_loop(&session, reader, &on_output, on_connected);

    if !session.closed.load(Ordering::Relaxed) {
        session.close();
    }
    on_exit(ExitInfo { exit_code: None });
}

/// 解析主機名並連線。`ToSocketAddrs` 會把每個解析結果都試一次（IPv6 優先的主機不會直接失敗）。
fn connect(target: &str) -> Result<TcpStream, String> {
    let addrs: Vec<_> = target
        .to_socket_addrs()
        .map_err(|e| format!("找不到主機 {target}：{e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("找不到主機 {target}"));
    }
    let mut last = String::new();
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(20)) {
            Ok(s) => {
                let _ = s.set_nodelay(true); // 同舊版 `new TcpClient { NoDelay = true }`
                return Ok(s);
            }
            Err(e) => last = format!("連線 {addr} 失敗：{e}"),
        }
    }
    Err(last)
}

fn read_loop(
    session: &Arc<TelnetSession>,
    mut reader: TcpStream,
    on_output: &OnOutput,
    on_connected: Option<OnConnected>,
) {
    let mut iac = Iac::default();
    let mut buf = [0u8; 8192];
    let mut first = true;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break, // 關 socket 也會走到這裡
        };
        if first {
            first = false;
            // 「真的連上了」＝從 socket 讀到第一批位元組（連協商也算：對方在跟我們說話）
            if let Some(cb) = &on_connected {
                cb();
            }
        }
        let (cols, rows) = *session.size.lock().unwrap_or_else(|e| e.into_inner());
        let (data, reply) = iac.process(&buf[..n], cols, rows);
        if iac.naws_ok {
            session.naws_ok.store(true, Ordering::Relaxed);
        }
        if !reply.is_empty() {
            session.write_raw(&reply);
        }
        if !data.is_empty() {
            on_output(&data);
        }
    }
}

/// 每 N 分鐘一個 `IAC NOP`（同舊版）。
///
/// **不能走 [`TerminalSession::write`]**：那條路會把 `0xFF` 轉義成資料位元組
/// （舊版 `SendNop` 的註解特別講了這件事）。
fn keepalive_thread(session: Arc<TelnetSession>) {
    let mins = session.keepalive_mins.max(1) as u64;
    std::thread::Builder::new()
        .name("telnet-keepalive".to_string())
        .spawn(move || {
            loop {
                // 一秒一格地睡，關分頁時才能很快收掉這條執行緒
                for _ in 0..(mins * 60) {
                    if session.closed.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
                if session.closed.load(Ordering::Relaxed) {
                    return;
                }
                session.write_raw(&[IAC, NOP]);
            }
        })
        .expect("spawn telnet keepalive thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `WILL ECHO` → `DO ECHO`、`WILL TTYPE` → `DONT TTYPE`（逐條照舊版 `RespondOption`）。
    #[test]
    fn responds_like_old_version() {
        let mut iac = Iac::default();
        let (data, reply) = iac.process(&[IAC, WILL, OPT_ECHO], 80, 24);
        assert!(data.is_empty());
        assert_eq!(reply, vec![IAC, DO, OPT_ECHO]);

        let (_, reply) = iac.process(&[IAC, WILL, 24], 80, 24); // 24 = TTYPE
        assert_eq!(reply, vec![IAC, DONT, 24], "舊版只認 ECHO / SGA");

        let (_, reply) = iac.process(&[IAC, DO, OPT_SGA], 80, 24);
        assert_eq!(reply, vec![IAC, WILL, OPT_SGA]);

        let (_, reply) = iac.process(&[IAC, DO, 24], 80, 24);
        assert_eq!(reply, vec![IAC, WONT, 24]);

        // WONT / DONT：舊版不回應
        let (_, reply) = iac.process(&[IAC, WONT, OPT_ECHO, IAC, DONT, OPT_SGA], 80, 24);
        assert!(reply.is_empty(), "舊版對 WONT/DONT 不回應");
    }

    /// `DO NAWS` → `WILL NAWS` + 尺寸子協商（新增功能，`CLAUDE.md` 定案）。
    #[test]
    fn naws_answers_with_size() {
        let mut iac = Iac::default();
        let (_, reply) = iac.process(&[IAC, DO, OPT_NAWS], 120, 40);
        assert!(iac.naws_ok);
        assert_eq!(
            reply,
            vec![IAC, WILL, OPT_NAWS, IAC, SB, OPT_NAWS, 0, 120, 0, 40, IAC, SE]
        );
    }

    /// 尺寸剛好含 `0xFF` 時要轉義（否則會被當成 IAC）。
    #[test]
    fn naws_escapes_ff_in_size() {
        assert_eq!(
            naws_sb(255, 24),
            vec![IAC, SB, OPT_NAWS, 0, 255, 255, 0, 24, IAC, SE]
        );
    }

    /// **狀態要跨 chunk 保留**：協商序列被切成兩段時，後半不可以變成畫面上的亂碼。
    /// 這是舊版實際踩過的雷（`TelnetSession.cs` 的 `_iac` 註解）。
    #[test]
    fn keeps_state_across_chunks() {
        let mut iac = Iac::default();
        let (data, reply) = iac.process(&[b'h', b'i', IAC], 80, 24);
        assert_eq!(data, b"hi");
        assert!(reply.is_empty());
        let (data, reply) = iac.process(&[WILL], 80, 24);
        assert!(data.is_empty() && reply.is_empty(), "還在等選項編號");
        let (data, reply) = iac.process(&[OPT_SGA, b'!'], 80, 24);
        assert_eq!(data, b"!", "協商之後的資料要正常吐出來");
        assert_eq!(reply, vec![IAC, DO, OPT_SGA]);
    }

    /// 子協商切在邊界上也不能漏（`IAC SB … IAC SE` 中間的內容一律丟掉）。
    #[test]
    fn subnegotiation_split_across_chunks() {
        let mut iac = Iac::default();
        let (data, _) = iac.process(&[IAC, SB, 24, 0, b'x'], 80, 24);
        assert!(data.is_empty());
        let (data, _) = iac.process(&[b'y', IAC], 80, 24);
        assert!(data.is_empty(), "還在子協商裡");
        let (data, _) = iac.process(&[SE, b'A'], 80, 24);
        assert_eq!(data, b"A", "IAC SE 之後才回到資料");
    }

    /// 子協商內容裡的 `IAC IAC` 不是結束。
    #[test]
    fn iac_iac_inside_subnegotiation_is_data() {
        let mut iac = Iac::default();
        let (data, _) = iac.process(&[IAC, SB, 24, IAC, IAC, 1, IAC, SE, b'Z'], 80, 24);
        assert_eq!(data, b"Z");
    }

    /// 收到的 `IAC IAC` 是資料裡的 `0xFF`（不是指令）。
    #[test]
    fn incoming_iac_iac_is_one_ff_byte() {
        let mut iac = Iac::default();
        let (data, reply) = iac.process(&[b'a', IAC, IAC, b'b'], 80, 24);
        assert_eq!(data, vec![b'a', 0xff, b'b']);
        assert!(reply.is_empty());
    }

    /// `IAC NOP`／`IAC GA` 這類兩位元組指令丟掉、不進畫面（同舊版）。
    #[test]
    fn two_byte_commands_are_dropped() {
        let mut iac = Iac::default();
        let (data, reply) = iac.process(&[b'a', IAC, NOP, b'b', IAC, 249, b'c'], 80, 24);
        assert_eq!(data, b"abc");
        assert!(reply.is_empty());
    }

    /// 送出時 `0xFF` 要變成兩個（否則遠端會把它當指令開頭）。
    #[test]
    fn outgoing_ff_is_escaped() {
        assert_eq!(escape(&[b'a', 0xff, b'b']), vec![b'a', 0xff, 0xff, b'b']);
        assert_eq!(escape(b"hello\r"), b"hello\r".to_vec(), "其餘位元組原樣送");
    }

    /// UTF-8 中文被切在兩個封包之間時，我們只做 IAC 拆解、**不碰位元組**，
    /// 兩段接起來仍是完整的字（解碼是 xterm 的事，同舊版）。
    #[test]
    fn utf8_split_across_chunks_is_untouched() {
        let text = "中文測試".as_bytes();
        let (a, b) = text.split_at(5); // 切在某個字的中間
        let mut iac = Iac::default();
        let (mut out, _) = iac.process(a, 80, 24);
        let (rest, _) = iac.process(b, 80, 24);
        out.extend_from_slice(&rest);
        assert_eq!(out, text);
        assert_eq!(String::from_utf8(out).unwrap(), "中文測試");
    }

    /// 連線參數裡沒有任何祕密欄位（我的最愛也存這個結構）。
    #[test]
    fn params_have_no_secret_field() {
        let json = serde_json::to_string(&TelnetParams::default()).unwrap();
        for bad in ["password", "passwd", "passphrase", "secret"] {
            assert!(!json.contains(bad), "Telnet 參數不可以有 {bad}：{json}");
        }
    }

    /// 預設埠 23（舊版連線視窗切到 Telnet 就把 22 換成 23）。
    #[test]
    fn default_port_is_23() {
        assert_eq!(TelnetParams::default().port, 23);
    }
}
