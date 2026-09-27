//! 巨集與「外界」之間的介面：連線、畫面、對話框、log。
//!
//! 直譯器本身（`exec.rs`／`expr.rs`／`cmds.rs`）完全不知道 tauri 或 session 的存在；
//! 會碰外界的指令（`send`／`wait`／`messagebox`…）都經過這個 trait。好處有三個：
//!
//! 1. `ttl_probe` 可以用**程式內的測試 server + 假 host** 驗完整條 `connect`→`wait`→`sendln`，
//!    不需要真的連線或視窗；
//! 2. app 端的實作（`runner.rs`）可以自己決定「等」的時候怎麼掛住（我們用專用執行緒）；
//! 3. 第二批要接的東西都集中在一個檔案裡，不會散進直譯器。
//!
//! ## 對應原碼
//!
//! 原碼是 `ttpmacro`（巨集）與 `ttermpro`（終端機）**兩個行程用 DDE 通訊**：
//! `DDEOut`／`DDESend` 把要送的字丟過去、`Read1Byte` 從收到的資料裡一個一個讀。
//! `CLAUDE.md` 定的做法是「DDE 改為程式內直接呼叫連線後端」，所以這裡就是那個界面：
//!
//! | 原碼 | 這裡 |
//! |---|---|
//! | `DDEOut`／`DDEOut1Byte`／`DDESend` | [`MacroHost::send`] |
//! | `Read1Byte`（從 DDE 收到的緩衝讀一個位元組） | [`MacroHost::read_byte`]（實作見 [`RecvBuffer`]） |
//! | `SetWait`／`CmpWait`／`Wait`（10 個候選、逐位元組比對） | [`WaitMatcher`] |
//! | `ClearBuff`（`flushrecv`） | [`MacroHost::flush_recv`] |
//! | `ttmdlg.cpp`／`msgdlg.cpp`／`inpdlg.cpp`／`ListDlg.cpp`／`statdlg.cpp` | [`MacroHost::dialog`] |
//! | `SendCmnd(CmdConnect)`（透過 DDE 叫 ttermpro 連線） | [`MacroHost::connect`] |

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// 對話框的請求（`messagebox`／`yesnobox`／`inputbox`／`passwordbox`／`listbox`…）。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogRequest {
    /// `message`｜`yesno`｜`input`｜`password`｜`status`｜`closestatus`｜`list`｜`filename`｜`dirname`
    pub kind: String,
    pub title: String,
    pub message: String,
    /// `inputbox` 的預設值。
    pub default: String,
    /// `listbox` 的選項。
    pub items: Vec<String>,
}

/// 對話框的回覆。
#[derive(Clone, Debug, Default)]
pub struct DialogAnswer {
    /// `yesnobox`：1＝是、0＝否（原碼是 `IDYES`／`IDNO` → `result`）。
    /// `listbox`：選到第幾項（0 起算），取消是 -1。
    pub number: i32,
    /// `inputbox`／`passwordbox`／`filenamebox` 的文字。
    pub text: String,
    /// 使用者按取消（`inputbox` 取消時原碼把 `result` 設 0、`inputstr` 不變）。
    pub cancelled: bool,
}

/// 巨集能對外界做的事。**會等的方法自己負責檢查中斷旗標**（見 [`MacroHost::stopped`]）。
pub trait MacroHost: Send + Sync {
    /// 送位元組給連線（`send`／`sendln`）。
    fn send(&self, data: &[u8]);

    /// 從接收緩衝讀一個位元組（同原碼的 `Read1Byte`）。沒有資料就回 `None`。
    fn read_byte(&self) -> Option<u8>;

    /// 清掉接收緩衝（`flushrecv`）。
    fn flush_recv(&self);

    /// 巨集被要求停止了嗎（使用者按中斷、分頁關閉、連線斷掉）。
    fn stopped(&self) -> bool;

    /// 連線還活著嗎（原碼的 `Linked`／`ComReady`；不活的話 `send`／`wait` 要回
    /// `Link macro first. Use 'connect' macro.`）。
    fn connected(&self) -> bool;

    /// 把一行字印在終端機畫面上（`dispstr`、以及巨集自己的錯誤訊息）。
    fn echo(&self, text: &str);

    /// 跳對話框並**等使用者回答**。
    fn dialog(&self, req: DialogRequest) -> DialogAnswer;

    /// 睡一下（`pause`／`mpause`／等資料時的間隔）。實作要能被中斷。
    fn sleep(&self, ms: u64);

    /// 現在的時間（毫秒）——逾時計算用。抽成方法是為了讓測試能控制時間。
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    // ---- 以下有預設實作＝「這個 host 不支援」，probe 的假 host 不用全部實作 ----

    /// **這條連線**的主機名稱（`gethostname`）。
    ///
    /// ⚠️ 原碼的 `gethostname` **不是本機的 hostname**：它走 DDE
    /// （`GetTTParam(CmdGetHostname)`）問 ttermpro「你現在連到哪」，而且會先檢查
    /// `Linked`（沒連線就回 `Link macro first.`）。照抄成 `hostname()` 會是**錯的**。
    ///
    /// SSH／Telnet 回主機名稱、COM 回埠名（同舊版「連到什麼」的語意），
    /// 本機 shell 沒有連線對象 → `None`（呼叫端寫空字串）。
    fn conn_host(&self) -> Option<String> {
        None
    }

    /// 清畫面（`clearscreen`）。
    fn clear_screen(&self) {}

    /// 響一聲（`beep`）。
    fn beep(&self) {}

    /// 改分頁標題（`settitle`）。
    fn set_title(&self, _title: &str) {}

    /// 目前的分頁標題（`gettitle`）。
    fn title(&self) -> String {
        String::new()
    }

    /// log：開始／寫一行／關閉（`logopen`／`logwrite`／`logclose`）。
    /// 回 `false` ＝做不到（`result` 會設成錯誤值）。
    fn log_open(&self, _path: &str, _append: bool) -> bool {
        false
    }
    fn log_write(&self, _text: &[u8]) -> bool {
        false
    }
    fn log_close(&self) {}

    /// `exec`：跑一個外部程式。回傳值照原碼：**-1 開不起來、0 開起來了（沒等）、
    /// 有等的話是 exit code**。
    ///
    /// ⚠️ **沙盒分頁的規則**（PM 在 TASK-014 定的）：巨集是使用者自己寫的、不是 AI agent，
    /// 所以不套 Claude Code 那種 hook 護欄；但 `exec` 出來的子行程一律
    /// **進巨集自己的 Job Object（kill-on-close）＋帶沙盒的環境變數**，
    /// 這樣巨集不會變成沙盒的後門（關分頁或巨集結束就一起收）。
    fn spawn_process(
        &self,
        _cmdline: &str,
        _cwd: Option<&str>,
        _hide: bool,
        _wait: bool,
    ) -> i32 {
        -1
    }

    /// 連線（`connect`）／斷線（`disconnect`）。`connect` 回 `false` ＝連不上。
    fn connect(&self, _params: &str) -> bool {
        false
    }
    fn disconnect(&self) {}
}

/// 接收緩衝：把連線的輸出收起來給 `wait`／`recvln` 用。
///
/// 兩件和原碼不同、但**照舊版 AwayTerminal**（舊版是行為下限）的事：
///
/// 1. **去掉 ANSI escape 序列**再比對。原碼是把 DDE 收到的原始位元組直接比，所以遇到
///    有顏色的提示字元（`\x1b[32m$\x1b[0m`）就比不到；舊版 C# 版會先去掉 ANSI，
///    使用者的巨集是照那個行為寫的，所以**照舊版**。
/// 2. 緩衝上限 400KB，滿了砍成 200KB（和舊版同一組數字）——巨集在等一個永遠不來的字串時
///    不可以把記憶體吃光。
///
/// UTF-8 跨 chunk 不裂：我們只在位元組層面拆 ANSI（同 `logging.rs` 的做法），
/// 中文的位元組原樣留著，所以 `wait '完成'` 跨封包也比對得到。
pub struct RecvBuffer {
    inner: Mutex<RecvInner>,
}

struct RecvInner {
    buf: VecDeque<u8>,
    /// 沒收完的 ESC 序列（跨 chunk 保留，同 `logging::Logger`）。
    carry: Vec<u8>,
}

/// 上限與砍到的大小（同舊版 `MacroRunner`）。
const MAX_BUF: usize = 400_000;
const TRIM_TO: usize = 200_000;

impl Default for RecvBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl RecvBuffer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(RecvInner {
                buf: VecDeque::new(),
                carry: Vec::new(),
            }),
        }
    }

    /// 連線來的原始位元組（含 ANSI）。
    pub fn push(&self, bytes: &[u8]) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = std::mem::take(&mut g.carry);
        data.extend_from_slice(bytes);
        let (clean, carry) = strip_ansi(&data);
        g.carry = carry;
        g.buf.extend(clean);
        if g.buf.len() > MAX_BUF {
            let cut = g.buf.len() - TRIM_TO;
            g.buf.drain(..cut);
        }
    }

    pub fn read_byte(&self) -> Option<u8> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .buf
            .pop_front()
    }

    pub fn clear(&self) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.buf.clear();
        g.carry.clear();
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 去掉 ANSI／OSC 序列，回傳 (乾淨的位元組, 沒收完的尾巴)。
///
/// 三種形狀和 `logging.rs` 的掃描器一致（那邊是給 log 用的，同一組規則）：
/// `ESC ] … BEL|ESC \`（OSC）、`ESC [ … 結尾字元`（CSI）、`ESC 單一字元`。
fn strip_ansi(data: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] != 0x1b {
            out.push(data[i]);
            i += 1;
            continue;
        }
        // ESC 開頭：看下一個字元決定形狀
        if i + 1 >= data.len() {
            return (out, data[i..].to_vec()); // 只有 ESC，等下一批
        }
        match data[i + 1] {
            b']' => {
                // OSC：到 BEL 或 ESC \ 為止
                let mut j = i + 2;
                loop {
                    if j >= data.len() {
                        return (out, data[i..].to_vec());
                    }
                    if data[j] == 0x07 {
                        j += 1;
                        break;
                    }
                    if data[j] == 0x1b && j + 1 < data.len() && data[j + 1] == b'\\' {
                        j += 2;
                        break;
                    }
                    if data[j] == 0x1b && j + 1 >= data.len() {
                        return (out, data[i..].to_vec());
                    }
                    j += 1;
                }
                i = j;
            }
            b'[' => {
                // CSI：參數位元組 0x30–0x3F、中間位元組 0x20–0x2F、結尾 0x40–0x7E
                let mut j = i + 2;
                while j < data.len() && (0x30..=0x3f).contains(&data[j]) {
                    j += 1;
                }
                while j < data.len() && (0x20..=0x2f).contains(&data[j]) {
                    j += 1;
                }
                if j >= data.len() {
                    return (out, data[i..].to_vec());
                }
                i = j + 1; // 吃掉結尾字元
            }
            _ => {
                i += 2; // `ESC 單一字元`
            }
        }
    }
    (out, Vec::new())
}

/// `wait` 的比對器——**逐位元組、10 個候選**，演算法照 `ttmdde.c` 的 `Wait()`。
///
/// 每個候選各有一個「已經對到幾個字」的計數器；對不上時會往回找最長的可用前綴
/// （原碼那段 `for (j=WaitCount[i]-1; ...)` 就是在做這件事，等於 KMP 的退回）。
/// **多個候選同一個位元組同時完成時，索引小的贏**（原碼的迴圈是 `for (i=9; i>=0; i--)`，
/// 所以 i=0 最後寫入 `Found`）。
pub struct WaitMatcher {
    /// 候選字串（最多 10 個）。
    pats: Vec<Vec<u8>>,
    /// 每個候選已經對到幾個位元組。
    count: Vec<usize>,
    /// 收到的這一行（`waitln`／`recvln` 的 `inputstr`；同原碼的 `RecvLnBuff`）。
    line: Vec<u8>,
    /// 上一個位元組（原碼 `RecvLnLast`：LF 之後的下一個位元組才清行緩衝）。
    last: u8,
}

impl WaitMatcher {
    /// 最多 10 個候選（原碼 `PWaitStr[10]`）。
    pub const MAX_PATTERNS: usize = 10;

    pub fn new(pats: Vec<Vec<u8>>) -> Self {
        let n = pats.len();
        Self {
            pats,
            count: vec![0; n],
            line: Vec::new(),
            last: 0,
        }
    }

    /// 吃一個位元組。回 `Some(index+1)` ＝第幾個候選命中（1 起算，同 `result`）。
    pub fn feed(&mut self, b: u8) -> Option<usize> {
        // 「這一行」的累積規則照原碼 `PutRecvLnBuff`：**換行也存進去**，
        // 而且是在「上一個位元組是 LF」的下一個位元組才把前一行清掉（延後清）。
        if self.last == 0x0a {
            self.line.clear();
        }
        self.line.push(b);
        self.last = b;

        let mut found = None;
        // ⚠️ 要從後往前跑：這樣索引小的候選最後寫入，「同時命中時小的贏」才成立
        for i in (0..self.pats.len()).rev() {
            let pat = &self.pats[i];
            if pat.is_empty() {
                continue;
            }
            if pat[self.count[i]] == b {
                self.count[i] += 1;
            } else if self.count[i] > 0 {
                // 往回找還能用的前綴（原碼那段 j 迴圈）
                let mut j = self.count[i] as isize - 1;
                while j >= 0 {
                    let ju = j as usize;
                    if pat[ju] == b
                        && (ju == 0 || pat[..ju] == pat[self.count[i] - ju..self.count[i]])
                    {
                        break;
                    }
                    j -= 1;
                }
                self.count[i] = if j >= 0 { j as usize + 1 } else { 0 };
            }
            if self.count[i] == pat.len() {
                found = Some(i + 1);
                // 命中之後把計數歸零：原碼在命中時會 `ClearWait()`，所以不會再拿舊的
                // 計數去索引；我們允許繼續餵（`waitln` 會換條件繼續等），所以自己歸零。
                self.count[i] = 0;
            }
        }
        found
    }

    /// 換一組候選，但**保留「這一行」的內容**。
    ///
    /// `waitln` 需要這個：先等使用者給的字串，命中之後改等換行，而 `inputstr` 要的是
    /// **整行**（包含命中之前就收到的部分）。原碼是同一個 `RecvLnBuff` 一路累積，
    /// 只是把 `PWaitStr` 換掉（`ClearWait` + `SetWait(1, LF)`），語意一樣。
    pub fn switch_to(&mut self, pats: Vec<Vec<u8>>) {
        self.count = vec![0; pats.len()];
        self.pats = pats;
    }

    /// 收到的那一行，**去掉尾端的 LF 與 CR**（原碼 `GetRecvLnBuff` 就是這樣處理的）。
    /// `waitln`／`recvln` 的 `inputstr` 用這個。
    pub fn line_before_newline(&self) -> &[u8] {
        let mut end = self.line.len();
        if end > 0 && self.line[end - 1] == 0x0a {
            end -= 1;
            if end > 0 && self.line[end - 1] == 0x0d {
                end -= 1;
            }
        }
        &self.line[..end]
    }

    /// 收到的那一行（原樣，含 CR／LF）。
    pub fn line(&self) -> &[u8] {
        &self.line
    }
}

/// 一個「什麼都不能做」的 host（單元測試用：只讓不碰 I/O 的指令跑）。
pub struct NullHost {
    pub sent: Mutex<Vec<u8>>,
    pub echoed: Mutex<Vec<String>>,
    pub recv: RecvBuffer,
    pub stop: std::sync::atomic::AtomicBool,
    /// 對話框一律這樣回（測試用）。
    pub answer: Mutex<DialogAnswer>,
    /// 收到的對話框請求（測試用）。
    pub asked: Mutex<Vec<DialogRequest>>,
    pub link: bool,
    /// 測試用：`exec` 收到的請求，以及要回什麼。
    pub execs: Mutex<Vec<ExecRequest>>,
    pub exec_result: i32,
}

impl Default for NullHost {
    fn default() -> Self {
        Self {
            sent: Mutex::new(Vec::new()),
            echoed: Mutex::new(Vec::new()),
            recv: RecvBuffer::new(),
            stop: std::sync::atomic::AtomicBool::new(false),
            answer: Mutex::new(DialogAnswer::default()),
            asked: Mutex::new(Vec::new()),
            link: true,
            execs: Mutex::new(Vec::new()),
            exec_result: 0,
        }
    }
}

impl NullHost {
    pub fn shared() -> Arc<NullHost> {
        Arc::new(Self::default())
    }

    /// 測試用：假裝連線送來了這些位元組。
    pub fn feed(&self, bytes: &[u8]) {
        self.recv.push(bytes);
    }

    pub fn sent_text(&self) -> String {
        String::from_utf8_lossy(&self.sent.lock().unwrap()).into_owned()
    }
}

impl MacroHost for NullHost {
    fn send(&self, data: &[u8]) {
        self.sent.lock().unwrap().extend_from_slice(data);
    }
    fn read_byte(&self) -> Option<u8> {
        self.recv.read_byte()
    }
    fn flush_recv(&self) {
        self.recv.clear();
    }
    fn stopped(&self) -> bool {
        self.stop.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn connected(&self) -> bool {
        self.link
    }
    fn echo(&self, text: &str) {
        self.echoed.lock().unwrap().push(text.to_string());
    }
    fn dialog(&self, req: DialogRequest) -> DialogAnswer {
        self.asked.lock().unwrap().push(req);
        self.answer.lock().unwrap().clone()
    }
    fn sleep(&self, ms: u64) {
        // 測試裡不要真的睡太久
        std::thread::sleep(std::time::Duration::from_millis(ms.min(5)));
    }
    fn spawn_process(&self, cmdline: &str, cwd: Option<&str>, hide: bool, wait: bool) -> i32 {
        self.execs.lock().unwrap().push(ExecRequest {
            cmdline: cmdline.to_string(),
            cwd: cwd.map(|s| s.to_string()),
            hide,
            wait,
        });
        self.exec_result
    }
}

/// 測試用：`exec` 收到的請求。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecRequest {
    pub cmdline: String,
    pub cwd: Option<String>,
    pub hide: bool,
    pub wait: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 基本比對與 `result` 的編號（1 起算）。
    #[test]
    fn matches_and_returns_one_based_index() {
        let mut m = WaitMatcher::new(vec![b"abc".to_vec(), b"xy".to_vec()]);
        assert_eq!(m.feed(b'a'), None);
        assert_eq!(m.feed(b'b'), None);
        assert_eq!(m.feed(b'c'), Some(1));
        let mut m = WaitMatcher::new(vec![b"abc".to_vec(), b"xy".to_vec()]);
        assert_eq!(m.feed(b'x'), None);
        assert_eq!(m.feed(b'y'), Some(2));
    }

    /// 同一個位元組讓兩個候選同時完成 → **索引小的贏**（照原碼的迴圈方向）。
    #[test]
    fn lower_index_wins_on_tie() {
        let mut m = WaitMatcher::new(vec![b"ok".to_vec(), b"k".to_vec()]);
        assert_eq!(m.feed(b'o'), None);
        assert_eq!(m.feed(b'k'), Some(1), "`ok` 和 `k` 同時完成，要回 1");
    }

    /// 對不上時要能退回可用的前綴（`aab` 對 `aaab`）。
    #[test]
    fn partial_match_backtracks() {
        let mut m = WaitMatcher::new(vec![b"aab".to_vec()]);
        assert_eq!(m.feed(b'a'), None);
        assert_eq!(m.feed(b'a'), None);
        assert_eq!(m.feed(b'a'), None, "第三個 a 之後還是對到兩個");
        assert_eq!(m.feed(b'b'), Some(1));
    }

    /// 跨 chunk 的中文（`wait '完成'`）——我們只在位元組層面比，所以天然沒問題。
    #[test]
    fn matches_utf8_across_chunks() {
        let pat = "完成".as_bytes().to_vec();
        let mut m = WaitMatcher::new(vec![pat]);
        let text = "處理完成了".as_bytes();
        let mut hit = None;
        for &b in text {
            if let Some(i) = m.feed(b) {
                hit = Some(i);
            }
        }
        assert_eq!(hit, Some(1));
    }

    /// `recvln`／`waitln` 用的「這一行」：含 CR／LF 存進去，讀的時候才去尾；
    /// 下一行的第一個位元組才會把前一行清掉（原碼 `PutRecvLnBuff` 的延後清）。
    #[test]
    fn line_buffer_resets_on_newline() {
        let mut m = WaitMatcher::new(vec![b"\n".to_vec()]);
        for &b in b"abc\r\n" {
            m.feed(b);
        }
        assert_eq!(m.line_before_newline(), b"abc", "去掉尾端的 CR LF");
        let mut m = WaitMatcher::new(vec![b"zz".to_vec()]);
        for &b in b"hello" {
            m.feed(b);
        }
        assert_eq!(m.line_before_newline(), b"hello");
    }

    /// 前一行是在**下一行的第一個位元組**才被清掉（原碼的延後清）。
    #[test]
    fn line_buffer_clears_lazily() {
        let mut m = WaitMatcher::new(vec![b"zz".to_vec()]);
        for &b in b"one\r\n" {
            m.feed(b);
        }
        assert_eq!(m.line_before_newline(), b"one");
        m.feed(b'x');
        assert_eq!(m.line_before_newline(), b"x", "新的一行開始了");
    }

    /// 接收緩衝會去掉 ANSI（照舊版；原碼是直接比原始位元組）。
    #[test]
    fn recv_buffer_strips_ansi() {
        let buf = RecvBuffer::new();
        buf.push(b"\x1b[32mPS C:\\>\x1b[0m ");
        let mut got = Vec::new();
        while let Some(b) = buf.read_byte() {
            got.push(b);
        }
        assert_eq!(got, b"PS C:\\> ".to_vec());
    }

    /// ESC 序列被切在兩批之間也要處理（跨 chunk 的 carry）。
    #[test]
    fn recv_buffer_handles_split_escape() {
        let buf = RecvBuffer::new();
        buf.push(b"a\x1b[3");
        buf.push(b"2mb");
        let mut got = Vec::new();
        while let Some(b) = buf.read_byte() {
            got.push(b);
        }
        assert_eq!(got, b"ab".to_vec());
    }

    /// OSC（`ESC ] … BEL`）也要去掉，而且不可以把後面的字吃掉。
    #[test]
    fn recv_buffer_strips_osc() {
        let buf = RecvBuffer::new();
        buf.push(b"x\x1b]0;title\x07y");
        let mut got = Vec::new();
        while let Some(b) = buf.read_byte() {
            got.push(b);
        }
        assert_eq!(got, b"xy".to_vec());
    }

    /// 中文位元組原樣留著（不會被當成 escape 的一部分）。
    #[test]
    fn recv_buffer_keeps_utf8() {
        let buf = RecvBuffer::new();
        buf.push("中文\x1b[0m完成".as_bytes());
        let mut got = Vec::new();
        while let Some(b) = buf.read_byte() {
            got.push(b);
        }
        assert_eq!(String::from_utf8(got).unwrap(), "中文完成");
    }

    /// 緩衝上限：滿了要砍掉前面，不可以無限長大。
    #[test]
    fn recv_buffer_is_bounded() {
        let buf = RecvBuffer::new();
        let chunk = vec![b'x'; 100_000];
        for _ in 0..6 {
            buf.push(&chunk);
        }
        assert!(buf.len() <= MAX_BUF, "len={}", buf.len());
    }
}
