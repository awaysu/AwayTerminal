//! 會碰外界的 TTL 指令：連線輸出入、等待、暫停、對話框、log、連線控制。
//!
//! 全部走 [`crate::ttl::host::MacroHost`]，所以直譯器本身不認識 tauri／session。
//! 對照的原碼在 `ttl.cpp` 的 `TTLSend`／`TTLWait`／`TTLPause`／`TTLMessageBox`… 與
//! `ttmmain.cpp` 的 `IdTTLWait*` 狀態機。
//!
//! ## 「等」怎麼做
//!
//! 原碼是 Windows 訊息迴圈：`TTLStatus = IdTTLWait` 之後回到訊息迴圈，每次 idle 檢查一次。
//! 我們是**每個分頁一條巨集執行緒**，所以「等」就在指令裡等（每 10ms 檢查一次中斷旗標
//! 與逾時）。`Interp::step()` 仍然是「一行一步」——只是那一步可能花掉 `timeout` 秒。
//! 主執行緒完全不受影響（巨集不在上面跑）。

use super::error::{Err, Result};
use super::exec::Interp;
use super::host::{DialogRequest, WaitMatcher};
use super::vars::VarType;
use super::words::Word;

/// 等資料時的輪詢間隔。
const POLL_MS: u64 = 10;

impl Interp {
    /// `dispatch_cmds` 裡「要碰外界」的那些。
    pub(super) fn dispatch_io(&mut self, w: Word) -> Result<()> {
        match w {
            // ---- 連線輸出 ----
            Word::Send => self.cmd_send(false),
            Word::SendLn => self.cmd_send(true),
            Word::DispStr => self.cmd_dispstr(),
            // ---- 等待 ----
            Word::Wait => self.cmd_wait(false, false),
            Word::WaitLn => self.cmd_wait(true, false),
            // `waitregex` 和 `wait` 共用等待的機制，只是比對改成正規表示式（逐行）
            Word::WaitRegex => self.cmd_wait(false, true),
            Word::WaitN => self.cmd_waitn(),
            Word::WaitRecv => self.cmd_waitrecv(),
            Word::RecvLn => self.cmd_recvln(),
            Word::FlushRecv => self.cmd_flushrecv(),
            Word::Pause => self.cmd_pause(1000),
            Word::MilliPause => self.cmd_pause(1),
            // ---- 終端機 ----
            Word::Beep => self.cmd_beep(),
            Word::ClearScreen => self.cmd_clearscreen(),
            Word::SetTitle => self.cmd_settitle(),
            Word::GetTitle => self.cmd_gettitle(),
            // ---- log ----
            Word::LogOpen => self.cmd_logopen(),
            Word::LogWrite => self.cmd_logwrite(),
            Word::LogClose => self.cmd_logclose(),
            // ---- 連線控制 ----
            Word::Connect => self.cmd_connect(),
            Word::Disconnect => self.cmd_disconnect(),
            Word::TestLink => self.cmd_testlink(),
            // ---- 對話框 ----
            Word::MessageBox => self.cmd_messagebox(),
            Word::YesNoBox => self.cmd_yesnobox(),
            Word::InputBox => self.cmd_inputbox(false),
            Word::PasswordBox => self.cmd_inputbox(true),
            Word::StatusBox => self.cmd_statusbox(),
            Word::CloseSBox => self.cmd_closesbox(),
            Word::ListBox => self.cmd_listbox(),
            Word::FilenameBox => self.cmd_pathbox(false),
            Word::DirnameBox => self.cmd_pathbox(true),
            Word::SetDlgPos => self.cmd_setdlgpos(),
            // ---- 外部程式與「執行一行 TTL」----
            Word::Exec => self.cmd_exec(),
            Word::ExecCmnd => self.cmd_execcmnd(),
            // 正規表示式在 regex.rs、檔案類在 files.rs
            Word::StrMatch | Word::StrReplace | Word::RegexOption => self.dispatch_regex(w),
            other => self.dispatch_files(other),
        }
    }

    /// 需要連線才能做的指令：沒有 host 或連線不在 → `Link macro first. Use 'connect' macro.`
    /// （原碼的 `if (! Linked) return ErrLinkFirst;`）
    pub(super) fn need_link(&self) -> Result<()> {
        match self.host() {
            Some(h) if h.connected() => Ok(()),
            _ => Err(Err::LinkFirst),
        }
    }

    // ---------------------------------------------------------------- 輸出

    /// `TTLSend`／`TTLSendLn` → `GetParamStrings`：一連串參數，
    /// **字串照送、整數送一個位元組**（原碼 `DDEOut1Byte(LOBYTE(Val))`）。
    /// `sendln` 最後再送一個 CR（**只有 CR，沒有 LF**）。
    fn cmd_send(&mut self, newline: bool) -> Result<()> {
        self.need_link()?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            if let Some(s) = self.lex_string()? {
                out.extend_from_slice(&s);
                continue;
            }
            match self.try_expression()? {
                Some(super::expr::Val::Str(s)) => out.extend_from_slice(&s),
                Some(super::expr::Val::Int(v)) => out.push((v & 0xff) as u8),
                None => break,
            }
        }
        if newline {
            out.push(0x0d);
        }
        if let Some(h) = self.host() {
            h.send(&out);
        }
        Ok(())
    }

    /// `TTLDispStr`：把字印在畫面上（不送給遠端）。
    fn cmd_dispstr(&mut self) -> Result<()> {
        let mut out: Vec<u8> = Vec::new();
        loop {
            if let Some(s) = self.lex_string()? {
                out.extend_from_slice(&s);
                continue;
            }
            match self.try_expression()? {
                Some(super::expr::Val::Str(s)) => out.extend_from_slice(&s),
                Some(super::expr::Val::Int(v)) => out.push((v & 0xff) as u8),
                None => break,
            }
        }
        if let Some(h) = self.host() {
            h.echo(&String::from_utf8_lossy(&out));
        }
        Ok(())
    }

    // ---------------------------------------------------------------- 等待

    /// 逾時（毫秒）＝ `timeout` × 1000 + `mtimeout`（原碼 `TTLWait` 的算法）。0＝永遠等。
    fn timeout_ms(&self) -> u64 {
        let sec = self.vars.int_of("timeout").unwrap_or(0).max(0) as u64;
        let ms = self.vars.int_of("mtimeout").unwrap_or(0).max(0) as u64;
        sec * 1000 + ms
    }

    /// `TTLWait(FALSE)` ＝ `wait`、`TTLWait(TRUE)` ＝ `waitln`、`regex=true` ＝ `waitregex`。
    ///
    /// 最多 10 個候選（字串常值或字串變數）。`result` ＝第幾個命中（1 起算），逾時是 0。
    /// `waitln` 命中之後還要等到換行，並把那一行放進 `inputstr`。
    ///
    /// `waitregex` 的比對是**逐行**做的（原碼 `Wait()` 只在收到 LF 時呼叫
    /// `FindRegexString()`，資料燒完之後再試一次），命中時 `inputstr` ＝那一行，
    /// 並且會設 `matchstr`／`groupmatchstr1..9`。
    fn cmd_wait(&mut self, wait_line: bool, regex: bool) -> Result<()> {
        let mut pats: Vec<Vec<u8>> = Vec::new();
        while pats.len() < WaitMatcher::MAX_PATTERNS {
            if let Some(s) = self.lex_string()? {
                pats.push(s);
                continue;
            }
            match self.try_expression()? {
                Some(super::expr::Val::Str(s)) => pats.push(s),
                Some(super::expr::Val::Int(_)) => return Err(Err::TypeMismatch),
                None => break,
            }
        }
        self.end_of_args()?;
        self.need_link()?;
        if pats.is_empty() {
            return Err(Err::Syntax);
        }

        let timeout = self.timeout_ms();
        if regex {
            return self.wait_regex(pats, timeout);
        }

        // 截止時間只算一次：`waitln` 的第二階段（等換行）和第一階段共用同一個
        // （原碼的逾時計時器在 TTLWait 設一次，進 IdTTLWaitNL 也不重設）
        let deadline = self.deadline(timeout);
        let mut m = WaitMatcher::new(pats);
        let hit = self.pump_until(deadline, &mut m)?;
        match hit {
            None => {
                self.vars.set_result(0); // 逾時
                Ok(())
            }
            Some(idx) => {
                self.vars.set_result(idx as i32);
                if !wait_line {
                    return Ok(());
                }
                // 命中的候選本身就是換行 → 這一行已經收完了（原碼 `CmpWait(ResultCode, "\n")`）
                if m.pattern(idx) == Some(b"\n".as_slice()) {
                    let line = self.recv_line.take();
                    self.vars.set_str("inputstr", &line);
                    return Ok(());
                }
                // `waitln`：命中之後改等換行，行緩衝繼續累積
                // （`inputstr` 要的是整行，含命中之前收到的部分；同原碼的 RecvLnBuff）
                m.switch_to(vec![vec![0x0a]]);
                let got = self.pump_until(deadline, &mut m)?;
                if got.is_none() {
                    // 等不到換行就逾時：`result` ＝ 0，`inputstr` 給目前收到的那一段
                    self.vars.set_result(0);
                }
                let line = self.recv_line.take();
                self.vars.set_str("inputstr", &line);
                Ok(())
            }
        }
    }

    /// `waitregex`：一行一行讀，每收滿一行就用每個 pattern 試一次（同原碼 `FindRegexString`）。
    fn wait_regex(&mut self, pats: Vec<Vec<u8>>, timeout_ms: u64) -> Result<()> {
        let deadline = self.deadline(timeout_ms);
        let opts = self.regex_options().clone();
        let patterns: Vec<String> = pats
            .iter()
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
        let mut line: Vec<u8> = Vec::new();
        loop {
            match self.host_read_byte() {
                Some(b) => {
                    // 共用的行緩衝也要跟著收（`wait` 系列都會 `PutRecvLnBuff`）；
                    // LF 照原碼在比對之後才放
                    if b != 0x0a {
                        self.recv_line.put(b);
                    }
                    if b == 0x0a {
                        // 一整行收滿了 → 試每個 pattern（索引小的先）
                        let text = String::from_utf8_lossy(&trim_eol(&line)).into_owned();
                        for (i, pat) in patterns.iter().enumerate() {
                            // pattern 壞掉時照原碼繼續試下一個（`FindRegexString` 只看 > 0）
                            if let Ok(Some(m)) = super::regex::find_one(&opts, pat, &text) {
                                self.clear_groups();
                                self.apply_match(&m);
                                self.vars.set_str("inputstr", text.as_bytes());
                                self.vars.set_result(i as i32 + 1);
                                // 原碼 `SetInputStr(GetRecvLnBuff())`：拿走＝清空
                                self.recv_line.clear();
                                return Ok(());
                            }
                        }
                        self.recv_line.put(b);
                        line.clear();
                    } else {
                        line.push(b);
                    }
                }
                None => {
                    // 沒有更多資料了：原碼在這時也會拿「還沒換行的那一段」試一次
                    if !line.is_empty() {
                        let text = String::from_utf8_lossy(&trim_eol(&line)).into_owned();
                        for (i, pat) in patterns.iter().enumerate() {
                            if let Ok(Some(m)) = super::regex::find_one(&opts, pat, &text) {
                                self.clear_groups();
                                self.apply_match(&m);
                                self.vars.set_str("inputstr", text.as_bytes());
                                self.vars.set_result(i as i32 + 1);
                                self.recv_line.clear();
                                return Ok(());
                            }
                        }
                    }
                    if self.wait_tick(deadline)? {
                        self.vars.set_result(0);
                        return Ok(());
                    }
                }
            }
        }
    }

    /// `TTLWaitN`：等行緩衝累積到 n 個位元組（原碼 `WaitN()`／`IdTTLWaitN`）。
    ///
    /// - 期間換行**也累積**（`SetWaitN` 把 `RecvLnClear` 關掉），而且之前 `wait` 留在
    ///   行緩衝裡的位元組也算數（原碼比的是 `RecvLnPtr`）；
    /// - 收滿：`result = 1`、`inputstr` ＝整段（只去掉最後的 CR LF，同 `GetRecvLnBuff`）；
    /// - 逾時：`inputstr` ＝收到的那一段，有收到東西 `result = -1`、什麼都沒有 `0`。
    fn cmd_waitn(&mut self) -> Result<()> {
        let n = self.int_val()?;
        self.end_of_args()?;
        self.need_link()?;
        let timeout = self.timeout_ms();
        let deadline = self.deadline(timeout);
        self.recv_line.set_clear_on_lf(false);
        let want = n.max(0) as usize;
        let r = loop {
            if self.recv_line.len() >= want {
                break Ok(true);
            }
            match self.host_read_byte() {
                Some(b) => self.recv_line.put(b),
                None => match self.wait_tick(deadline) {
                    Ok(false) => {}
                    Ok(true) => break Ok(false),
                    Err(e) => break Err(e),
                },
            }
        };
        // 原碼成功時 `ClearWaitN()` 把它開回來；逾時那條沒開（原碼的疏漏，之後的
        // `wait` 行緩衝會跨行累積）——我們兩條都開回來
        self.recv_line.set_clear_on_lf(true);
        let full = r?;
        let line = self.recv_line.take();
        let result = if full {
            1
        } else if line.is_empty() {
            0
        } else {
            -1
        };
        self.vars.set_result(result);
        self.vars.set_str("inputstr", &line);
        Ok(())
    }

    /// `TTLWaitRecv`：`waitrecv <子字串> <長度> <位置>`（原碼 `SetWait2`／`Wait2()`／`IdTTLWait2`）。
    ///
    /// 收到的資料滑過一個 `長度` 位元組的視窗；**視窗第 `位置` 個位元組起**等於子字串
    /// 之後，再等視窗填滿 `長度` 個位元組才結束。
    ///
    /// - 成功：`result = 1`、`inputstr` ＝視窗內容；
    /// - 逾時：子字串已經出現過 → `result = -1`、`inputstr` ＝視窗內容；沒出現 → `0`、空字串。
    /// - `長度` 夾在 子字串長度～511、`位置` 夾在 1～`長度 - 子字串長度 + 1`（同原碼）。
    fn cmd_waitrecv(&mut self) -> Result<()> {
        let sub = self.str_val()?;
        let len = self.int_val()?;
        let pos = self.int_val()?;
        self.end_of_args()?;
        self.need_link()?;
        self.vars.set_str("inputstr", b"");

        // ---- SetWait2 ----
        let max = super::lex::MAX_STR_LEN - 1;
        let sub: Vec<u8> = sub.into_iter().take(max).collect();
        let sub_len = sub.len();
        let mut wlen = if len < 1 { 0 } else { (len as usize).min(max) };
        if wlen < sub_len {
            wlen = sub_len;
        }
        let hi = (wlen - sub_len + 1) as i64;
        let sub_pos = (i64::from(pos)).clamp(1, hi.max(1)) as usize;
        let mut win: Vec<u8> = Vec::with_capacity(wlen);
        let mut found = sub.is_empty();

        let timeout = self.timeout_ms();
        let deadline = self.deadline(timeout);
        // ---- Wait2() ----
        let done = loop {
            if found && win.len() == wlen {
                break Ok(true);
            }
            match self.host_read_byte() {
                Some(b) => {
                    // （`wlen == 0` 時子字串一定是空的 → 上面第一圈就結束了，走不到這裡）
                    if !win.is_empty() && win.len() >= wlen {
                        win.remove(0);
                    }
                    win.push(b);
                    if !found && win.len() >= sub_pos + sub_len - 1 {
                        let at = sub_pos - 1;
                        found = win.get(at..at + sub_len) == Some(sub.as_slice());
                    }
                }
                None => match self.wait_tick(deadline) {
                    Ok(false) => {}
                    Ok(true) => break Ok(false),
                    Err(e) => break Err(e),
                },
            }
        };
        if done? {
            self.vars.set_str("inputstr", &win);
            self.vars.set_result(1);
        } else if found {
            self.vars.set_str("inputstr", &win);
            self.vars.set_result(-1);
        } else {
            self.vars.set_str("inputstr", b"");
            self.vars.set_result(0);
        }
        Ok(())
    }

    /// `TTLRecvLn`：等一整行（`inputstr` ＝那一行，`result` ＝ 1；逾時 0）。
    ///
    /// 照原碼：一開始就把 `inputstr` 清空、`result` 設 1；逾時時 `result = 0`，
    /// `inputstr` ＝收到的那半行。行緩衝是共用的，所以前一個 `wait` 在行中間命中後
    /// 接 `recvln`，拿到的是整行。
    fn cmd_recvln(&mut self) -> Result<()> {
        self.end_of_args()?;
        self.need_link()?;
        self.vars.set_str("inputstr", b"");
        self.vars.set_result(1);
        let timeout = self.timeout_ms();
        let deadline = self.deadline(timeout);
        let mut m = WaitMatcher::new(vec![vec![0x0a]]);
        let hit = self.pump_until(deadline, &mut m)?;
        if hit.is_none() {
            self.vars.set_result(0);
        }
        let line = self.recv_line.take();
        self.vars.set_str("inputstr", &line);
        Ok(())
    }

    /// `TTLFlushRecv`：清掉還沒比對的接收資料（原碼 `FlushRecv` 連行緩衝一起清）。
    fn cmd_flushrecv(&mut self) -> Result<()> {
        self.end_of_args()?;
        if let Some(h) = self.host() {
            h.flush_recv();
        }
        self.recv_line.clear();
        Ok(())
    }

    /// `TTLPause`（秒）／`TTLMilliPause`（毫秒）。中斷時立刻回來。
    fn cmd_pause(&mut self, scale: u64) -> Result<()> {
        let v = self.int_val()?;
        self.end_of_args()?;
        if v <= 0 {
            return Ok(());
        }
        let total = (v as u64).saturating_mul(scale);
        let Some(h) = self.host() else {
            return Ok(());
        };
        let end = h.now_ms().saturating_add(total);
        loop {
            let now = h.now_ms();
            if now >= end {
                break;
            }
            if h.stopped() {
                return Err(Err::Interrupted);
            }
            // 用同一個 `now` 算剩多少：以前 `while` 判斷完再呼叫一次 `now_ms()`，
            // 兩次之間跨過 `end` 就是 u64 減法溢位（debug 建置 panic）
            h.sleep(POLL_MS.min(end.saturating_sub(now)));
        }
        Ok(())
    }

    // ---------------------------------------------------------------- 終端機

    fn cmd_beep(&mut self) -> Result<()> {
        // 原碼的 beep 可以帶參數（音效類型），我們忽略參數只響一聲
        if self.parameter_given() {
            let _ = self.int_val()?;
        }
        self.end_of_args()?;
        if let Some(h) = self.host() {
            h.beep();
        }
        Ok(())
    }

    fn cmd_clearscreen(&mut self) -> Result<()> {
        // 原碼：`clearscreen <0|1|2>`（0＝畫面、1＝畫面+捲動緩衝…）；我們只做「清畫面」
        if self.parameter_given() {
            let _ = self.int_val()?;
        }
        self.end_of_args()?;
        if let Some(h) = self.host() {
            h.clear_screen();
        }
        Ok(())
    }

    fn cmd_settitle(&mut self) -> Result<()> {
        let title = self.str_val()?;
        self.end_of_args()?;
        if let Some(h) = self.host() {
            h.set_title(&String::from_utf8_lossy(&title));
        }
        Ok(())
    }

    fn cmd_gettitle(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        self.end_of_args()?;
        let title = self.host().map(|h| h.title()).unwrap_or_default();
        self.set_str_ref(&target, title.as_bytes())
    }

    // ---------------------------------------------------------------- log

    /// `TTLLogOpen`：`logopen <檔名> <binary> <append>`（原碼還有第 4、5 個參數，我們忽略並說明）。
    /// `result` ＝ 0 成功、1 失敗（原碼用 `result` 回報成功與否）。
    fn cmd_logopen(&mut self) -> Result<()> {
        let path = self.str_val()?;
        // binary 旗標：我們的 log 一律是「去 ANSI 的文字」，所以讀進來但不用（文件有寫）
        let _binary = if self.parameter_given() { self.int_val()? } else { 0 };
        let append = if self.parameter_given() { self.int_val()? } else { 0 };
        // 原碼之後還有 `plainText`／`timestamp`／`timestamptype`；讀掉不用
        while self.parameter_given() {
            let _ = self.int_val()?;
        }
        self.end_of_args()?;
        let ok = self
            .host()
            .map(|h| h.log_open(&String::from_utf8_lossy(&path), append != 0))
            .unwrap_or(false);
        self.vars.set_result(i32::from(!ok));
        Ok(())
    }

    fn cmd_logwrite(&mut self) -> Result<()> {
        let text = self.str_val()?;
        self.end_of_args()?;
        let ok = self.host().map(|h| h.log_write(&text)).unwrap_or(false);
        self.vars.set_result(i32::from(!ok));
        Ok(())
    }

    fn cmd_logclose(&mut self) -> Result<()> {
        self.end_of_args()?;
        if let Some(h) = self.host() {
            h.log_close();
        }
        Ok(())
    }

    // ---------------------------------------------------------------- 連線控制

    /// `TTLConnect`：`connect '<參數>'`。
    ///
    /// 參數格式照 TeraTerm 的命令列（`docs/TTL.md` 有對照表）：
    /// `host:port /ssh /user=x /passwd=y`、`/telnet`、`/C=3`（COM3）。
    /// **已經連著的時候 `result=2`**（同原碼），連上 0、連不上 1。
    fn cmd_connect(&mut self) -> Result<()> {
        let params = self.str_val()?;
        self.end_of_args()?;
        let Some(h) = self.host() else {
            return Err(Err::LinkFirst);
        };
        if h.connected() {
            self.vars.set_result(2);
            return Ok(());
        }
        let ok = h.connect(&String::from_utf8_lossy(&params));
        self.vars.set_result(i32::from(!ok));
        Ok(())
    }

    fn cmd_disconnect(&mut self) -> Result<()> {
        // 原碼可以帶一個參數（要不要關視窗）；讀掉不用
        if self.parameter_given() {
            let _ = self.int_val()?;
        }
        self.end_of_args()?;
        if let Some(h) = self.host() {
            h.disconnect();
        }
        Ok(())
    }

    /// `TTLTestLink`：`result` ＝ 2 連線中、1 有連線層但沒連上、0 沒有。
    /// 我們只有「連著／沒連著」兩種狀態 → 2 或 0（文件有寫）。
    fn cmd_testlink(&mut self) -> Result<()> {
        self.end_of_args()?;
        let v = match self.host() {
            Some(h) if h.connected() => 2,
            _ => 0,
        };
        self.vars.set_result(v);
        Ok(())
    }

    // ---------------------------------------------------------------- 對話框

    /// `messagebox <訊息> [標題]`。
    fn cmd_messagebox(&mut self) -> Result<()> {
        let msg = self.str_val()?;
        let title = if self.parameter_given() {
            self.str_val()?
        } else {
            b"Macro".to_vec()
        };
        // 原碼還可以帶旗標（圖示／按鈕），讀掉不用
        while self.parameter_given() {
            let _ = self.int_val()?;
        }
        self.end_of_args()?;
        self.ask_dialog("message", &title, &msg, b"", &[]);
        Ok(())
    }

    /// `yesnobox <訊息> [標題]` → `result` ＝ 1 是、0 否。
    fn cmd_yesnobox(&mut self) -> Result<()> {
        let msg = self.str_val()?;
        let title = if self.parameter_given() {
            self.str_val()?
        } else {
            b"Macro".to_vec()
        };
        self.end_of_args()?;
        let a = self.ask_dialog("yesno", &title, &msg, b"", &[]);
        // 按 Esc／關掉視窗＝「否」（原碼 `IDOK` 才是 1，其餘 0）。前端取消時以前送 -1，
        // 直接拿來當 `result` 的話 `if result then` 會走「是」的分支
        self.vars.set_result(if a.cancelled { 0 } else { a.number });
        Ok(())
    }

    /// `inputbox <訊息> [標題] [預設值]`／`passwordbox <訊息> [標題]`。
    ///
    /// 輸入的文字放進 `inputstr`；**按取消時 `result=0` 且 `inputstr` 是空的**（同原碼）。
    fn cmd_inputbox(&mut self, password: bool) -> Result<()> {
        let msg = self.str_val()?;
        let title = if self.parameter_given() {
            self.str_val()?
        } else {
            b"Macro".to_vec()
        };
        let default = if !password && self.parameter_given() {
            self.str_val()?
        } else {
            Vec::new()
        };
        self.end_of_args()?;
        let kind = if password { "password" } else { "input" };
        let a = self.ask_dialog(kind, &title, &msg, &default, &[]);
        if a.cancelled {
            self.vars.set_result(0);
            self.vars.set_str("inputstr", b"");
        } else {
            self.vars.set_result(1);
            self.vars.set_str("inputstr", a.text.as_bytes());
        }
        Ok(())
    }

    /// `statusbox <訊息> [標題]`：不會等使用者（原碼是一個常駐的小視窗）。
    fn cmd_statusbox(&mut self) -> Result<()> {
        let msg = self.str_val()?;
        let title = if self.parameter_given() {
            self.str_val()?
        } else {
            b"Macro".to_vec()
        };
        self.end_of_args()?;
        self.ask_dialog("status", &title, &msg, b"", &[]);
        Ok(())
    }

    fn cmd_closesbox(&mut self) -> Result<()> {
        self.end_of_args()?;
        self.ask_dialog("closestatus", b"", b"", b"", &[]);
        Ok(())
    }

    /// `listbox <訊息> <標題> <字串陣列>` → `result` ＝選了第幾項（0 起算），取消是 -1。
    ///
    /// 原碼的第三個參數是字串陣列變數；我們照做。
    fn cmd_listbox(&mut self) -> Result<()> {
        let msg = self.str_val()?;
        let title = self.str_val()?;
        let Some(name) = self.lex_identifier() else {
            return Err(Err::Syntax);
        };
        // 原碼之後還有「預設選第幾項」與旗標，讀掉不用
        while self.parameter_given() {
            let _ = self.int_val()?;
        }
        self.end_of_args()?;
        let items: Vec<String> = match self.vars.find(&name) {
            Some(super::vars::Value::StrArray(a)) => a
                .iter()
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect(),
            _ => return Err(Err::TypeMismatch),
        };
        let a = self.ask_dialog("list", &title, &msg, b"", &items);
        self.vars.set_result(a.number);
        Ok(())
    }

    /// `filenamebox <訊息> [標題]`／`dirnamebox <訊息> [標題]`：
    /// 選到的路徑放進 `inputstr`，`result` ＝ 1 選了、0 取消（原碼同款）。
    fn cmd_pathbox(&mut self, dir: bool) -> Result<()> {
        let msg = self.str_val()?;
        let title = if self.parameter_given() {
            self.str_val()?
        } else {
            b"Macro".to_vec()
        };
        while self.parameter_given() {
            let _ = self.int_val()?;
        }
        self.end_of_args()?;
        let kind = if dir { "dirname" } else { "filename" };
        let a = self.ask_dialog(kind, &title, &msg, b"", &[]);
        if a.cancelled || a.text.is_empty() {
            self.vars.set_result(0);
        } else {
            self.vars.set_result(1);
            self.vars.set_str("inputstr", a.text.as_bytes());
        }
        Ok(())
    }

    /// `setdlgpos <x> <y>`：**座標忽略**（我們的對話框是頁內置中的，見 docs/TTL.md）。
    fn cmd_setdlgpos(&mut self) -> Result<()> {
        let _x = self.int_val()?;
        let _y = self.int_val()?;
        self.end_of_args()?;
        Ok(())
    }

    fn ask_dialog(
        &mut self,
        kind: &str,
        title: &[u8],
        message: &[u8],
        default: &[u8],
        items: &[String],
    ) -> super::host::DialogAnswer {
        let req = DialogRequest {
            kind: kind.to_string(),
            title: String::from_utf8_lossy(title).into_owned(),
            message: String::from_utf8_lossy(message).into_owned(),
            default: String::from_utf8_lossy(default).into_owned(),
            items: items.to_vec(),
        };
        match self.host() {
            Some(h) => h.dialog(req),
            None => super::host::DialogAnswer::default(),
        }
    }

    // ---------------------------------------------------------------- 外部程式

    /// `TTLExec`：`exec <命令列> [show|hide|minimize|maximize] [wait] [工作目錄]`。
    ///
    /// `result`：**-1 開不起來、0 開起來了（沒等）、有等的話是 exit code**（照原碼）。
    /// 視窗模式：`hide` 真的隱藏；`minimize`／`maximize` **接受但照 show 處理**
    /// （見 `docs/TTL.md` 的偏差表）。
    ///
    /// 沙盒分頁裡開出來的子行程會進巨集自己的 Job Object ＋帶沙盒環境變數，見 `host.rs`。
    fn cmd_exec(&mut self) -> Result<()> {
        let cmdline = self.str_val()?;
        let mut hide = false;
        let mut wait = 0;
        let mut cwd: Option<Vec<u8>> = None;
        if self.parameter_given() {
            let mode = self.str_val()?;
            let mode = String::from_utf8_lossy(&mode).to_ascii_lowercase();
            match mode.as_str() {
                "hide" => hide = true,
                // 原碼是 SW_MINIMIZE／SW_MAXIMIZE；我們照 show 處理（文件有寫）
                "show" | "minimize" | "maximize" => {}
                _ => return Err(Err::Syntax),
            }
            if self.parameter_given() {
                wait = self.int_val()?;
                if self.parameter_given() {
                    cwd = Some(self.str_val()?);
                }
            }
        }
        self.end_of_args()?;
        if cmdline.is_empty() {
            return Err(Err::Syntax);
        }
        let cmdline = String::from_utf8_lossy(&cmdline).into_owned();
        let cwd = cwd.map(|c| String::from_utf8_lossy(&c).into_owned());
        let r = match self.host() {
            Some(h) => h.spawn_process(&cmdline, cwd.as_deref(), hide, wait != 0),
            None => -1,
        };
        self.vars.set_result(r);
        Ok(())
    }

    /// `TTLExecCmnd`：**執行一行 TTL 指令字串**（不是外部程式）。
    ///
    /// 原碼是把 `LineBuff` 換成那個字串、`LinePtr` 歸零、設 `ParseAgain`，
    /// 讓主迴圈重新解析同一「行」。我們照做（`exec_line_now`）。
    fn cmd_execcmnd(&mut self) -> Result<()> {
        let line = self.str_val()?;
        self.end_of_args()?;
        self.exec_line_now(&line)
    }

    // ---------------------------------------------------------------- 等待的機制

    /// 逾時的截止時間（`None` ＝永遠等）。
    fn deadline(&self, timeout_ms: u64) -> Option<u64> {
        if timeout_ms == 0 {
            None
        } else {
            self.host().map(|h| h.now_ms() + timeout_ms)
        }
    }

    fn host_read_byte(&self) -> Option<u8> {
        self.host().and_then(|h| h.read_byte())
    }

    /// 沒資料時睡一下。回 `true` ＝逾時了；中斷時回 `Err(Interrupted)`。
    fn wait_tick(&self, deadline: Option<u64>) -> Result<bool> {
        let Some(h) = self.host() else {
            return Ok(true);
        };
        if h.stopped() {
            return Err(Err::Interrupted);
        }
        // 連線斷掉就不要再等了（同原碼：`Linked` 變 false 時 wait 會結束）
        if !h.connected() {
            return Ok(true);
        }
        if let Some(d) = deadline {
            if h.now_ms() >= d {
                return Ok(true);
            }
        }
        h.sleep(POLL_MS);
        Ok(false)
    }

    /// 一直讀位元組餵給比對器，直到命中、逾時（`deadline`，`None` ＝永遠等）或被中斷。
    /// 每個位元組先放進共用的行緩衝（原碼 `Wait()` 裡的 `PutRecvLnBuff`）再比對。
    fn pump_until(&mut self, deadline: Option<u64>, m: &mut WaitMatcher) -> Result<Option<usize>> {
        loop {
            match self.host_read_byte() {
                Some(b) => {
                    self.recv_line.put(b);
                    if let Some(idx) = m.feed(b) {
                        return Ok(Some(idx));
                    }
                }
                None => {
                    if self.wait_tick(deadline)? {
                        return Ok(None);
                    }
                }
            }
        }
    }
}

/// 去掉尾端的 CR（LF 已經在呼叫端處理掉了）。
fn trim_eol(line: &[u8]) -> Vec<u8> {
    let mut v = line.to_vec();
    while v.last() == Some(&0x0d) {
        v.pop();
    }
    v
}

#[cfg(test)]
mod tests {
    use super::super::exec::Interp;
    use super::super::host::{DialogAnswer, NullHost};
    use super::*;
    use std::sync::Arc;

    /// 跑一段巨集，回傳 (變數表, host)。
    fn run_with_host(src: &str, host: Arc<NullHost>) -> (super::super::vars::Vars, Arc<NullHost>) {
        let mut it = Interp::from_text("t.ttl", src).unwrap();
        it.set_host(Some(host.clone()));
        it.run(100_000).expect("巨集應該跑得完");
        (std::mem::take(&mut it.vars), host)
    }

    fn run(src: &str) -> (super::super::vars::Vars, Arc<NullHost>) {
        run_with_host(src, NullHost::shared())
    }

    /// `send`／`sendln`：字串照送、整數送一個位元組、`sendln` 補 CR。
    #[test]
    fn send_and_sendln() {
        let (_, h) = run("send 'abc'");
        assert_eq!(h.sent_text(), "abc");
        let (_, h) = run("sendln 'abc'");
        assert_eq!(h.sent.lock().unwrap().as_slice(), b"abc\r");
        // 整數＝一個位元組（原碼 LOBYTE）
        let (_, h) = run("send 65 66");
        assert_eq!(h.sent_text(), "AB");
        // 字串變數與常值混用
        let (_, h) = run("s = 'x'\nsendln 'a' s #33");
        assert_eq!(h.sent.lock().unwrap().as_slice(), b"ax!\r");
    }

    /// 沒有連線時 `send`／`wait` 要回 `Link macro first.`
    #[test]
    fn commands_need_a_link() {
        let host = Arc::new(NullHost {
            link: false,
            ..Default::default()
        });
        let mut it = Interp::from_text("t.ttl", "sendln 'x'").unwrap();
        it.set_host(Some(host));
        assert_eq!(it.run(100).unwrap_err().err, Err::LinkFirst);
    }

    /// `wait` 命中：`result` ＝第幾個候選（1 起算）。
    #[test]
    fn wait_sets_result_index() {
        let host = NullHost::shared();
        host.feed(b"hello world\r\n");
        let (v, _) = run_with_host("timeout = 1\nwait 'zzz' 'world'\nr = result", host);
        assert_eq!(v.int_of("r"), Some(2));
    }

    /// `wait` 逾時 → `result` ＝ 0（而且不會卡住）。
    #[test]
    fn wait_timeout_gives_zero() {
        let host = NullHost::shared();
        host.feed(b"nothing useful");
        let (v, _) = run_with_host("mtimeout = 30\nwait 'never'\nr = result", host);
        assert_eq!(v.int_of("r"), Some(0));
    }

    /// `waitln`：命中之後還要等換行，`inputstr` ＝那一行。
    #[test]
    fn waitln_captures_the_line() {
        let host = NullHost::shared();
        host.feed(b"prefix OK suffix\r\nnext line\r\n");
        let (v, _) = run_with_host("timeout = 1\nwaitln 'OK'\nr = result\ns = inputstr", host);
        assert_eq!(v.int_of("r"), Some(1));
        assert_eq!(v.str_of("s").unwrap(), b"prefix OK suffix");
    }

    /// `recvln`：拿一整行。
    #[test]
    fn recvln_reads_a_line() {
        let host = NullHost::shared();
        host.feed(b"first\r\nsecond\r\n");
        let (v, _) = run_with_host("timeout = 1\nrecvln\ns1 = inputstr\nrecvln\ns2 = inputstr", host);
        assert_eq!(v.str_of("s1").unwrap(), b"first");
        assert_eq!(v.str_of("s2").unwrap(), b"second");
    }

    /// `waitn`：等 n 個位元組。
    #[test]
    fn waitn_counts_bytes() {
        let host = NullHost::shared();
        host.feed(b"12345");
        let (v, _) = run_with_host("timeout = 1\nwaitn 3\nr = result\ns = inputstr", host);
        assert_eq!(v.int_of("r"), Some(1));
        assert_eq!(v.str_of("s").unwrap(), b"123");
        // 不夠的話逾時：有收到東西 → result = -1、inputstr ＝收到的那段（原碼 IdTTLWaitN 逾時）
        let host = NullHost::shared();
        host.feed(b"12");
        let (v, _) = run_with_host("mtimeout = 30\nwaitn 5\nr = result\ns = inputstr", host);
        assert_eq!(v.int_of("r"), Some(-1));
        assert_eq!(v.str_of("s").unwrap(), b"12");
        // 什麼都沒收到 → 0
        let (v, _) = run_with_host("mtimeout = 30\nwaitn 5\nr = result", NullHost::shared());
        assert_eq!(v.int_of("r"), Some(0));
    }

    /// `waitn` 保留中間的 CR／LF（期間換行不清行緩衝），只去掉最後的 CR LF。
    #[test]
    fn waitn_keeps_newlines() {
        let host = NullHost::shared();
        host.feed(b"a\r\nb\r\n");
        let (v, _) = run_with_host("timeout = 1\nwaitn 6\ns = inputstr", host);
        assert_eq!(v.str_of("s").unwrap(), b"a\r\nb");
        let host = NullHost::shared();
        host.feed(b"a\r\nbcd");
        let (v, _) = run_with_host("timeout = 1\nwaitn 5\ns = inputstr", host);
        assert_eq!(v.str_of("s").unwrap(), b"a\r\nbc");
    }

    /// 行緩衝跨指令保留：`wait` 在行中間命中後接 `recvln`，`inputstr` 是**整行**。
    #[test]
    fn recvln_after_wait_gets_whole_line() {
        let host = NullHost::shared();
        host.feed(b"login: ok done\r\nnext\r\n");
        let (v, _) = run_with_host(
            "timeout = 1\nwait 'login:'\nrecvln\ns1 = inputstr\nrecvln\ns2 = inputstr",
            host,
        );
        assert_eq!(v.str_of("s1").unwrap(), b"login: ok done");
        assert_eq!(v.str_of("s2").unwrap(), b"next");
    }

    /// `recvln` 逾時：`result = 0`、`inputstr` ＝收到的半行（開始時先清空）。
    #[test]
    fn recvln_timeout_gives_partial_line() {
        let host = NullHost::shared();
        host.feed(b"half");
        let (v, _) = run_with_host("inputstr = 'old'\nmtimeout = 30\nrecvln\nr = result\ns = inputstr", host);
        assert_eq!(v.int_of("r"), Some(0));
        assert_eq!(v.str_of("s").unwrap(), b"half");
        let (v, _) = run_with_host("inputstr = 'old'\nmtimeout = 30\nrecvln\ns = inputstr", NullHost::shared());
        assert_eq!(v.str_of("s").unwrap(), b"", "開始時就清空");
    }

    /// `flushrecv` 連行緩衝一起清。
    #[test]
    fn flushrecv_clears_line_buffer() {
        // 同一支巨集裡：wait 留下的前半行被 flushrecv 清掉
        let host = NullHost::shared();
        host.feed(b"garbage OK");
        let mut it = Interp::from_text("t.ttl", "timeout = 1\nwait 'OK'\nflushrecv\nrecvln\ns = inputstr").unwrap();
        it.set_host(Some(host.clone()));
        // 跑到 recvln 之前再餵新的一行
        for _ in 0..3 {
            it.step().unwrap();
        }
        host.feed(b"clean\r\n");
        it.run(100).unwrap();
        assert_eq!(it.vars.str_of("s").unwrap(), b"clean");
    }

    /// `waitln` 第二階段（等換行）和第一階段共用同一個截止時間（以前重新起算，最多 2 倍）。
    #[test]
    fn waitln_shares_one_deadline() {
        let host = NullHost::shared();
        host.feed(b"xx OK but no newline");
        let t0 = std::time::Instant::now();
        let (v, _) = run_with_host("mtimeout = 300\nwaitln 'OK'\nr = result\ns = inputstr", host);
        let ms = t0.elapsed().as_millis();
        assert_eq!(v.int_of("r"), Some(0));
        assert_eq!(v.str_of("s").unwrap(), b"xx OK but no newline");
        assert!(ms < 550, "只能等一次 timeout（300ms），實際 {ms}ms");
    }

    /// `waitln` 的候選本身是換行：命中就是整行，不再等下一個換行。
    #[test]
    fn waitln_with_newline_pattern() {
        let host = NullHost::shared();
        host.feed(b"abc\r\nzzz");
        let (v, _) = run_with_host("mtimeout = 100\nwaitln #10\nr = result\ns = inputstr", host);
        assert_eq!(v.int_of("r"), Some(1));
        assert_eq!(v.str_of("s").unwrap(), b"abc");
    }

    /// `waitrecv`：滑動視窗、子字串在指定位置、視窗填滿才結束（原碼 `Wait2()`）。
    #[test]
    fn waitrecv_window() {
        let host = NullHost::shared();
        host.feed(b"abcXYZdef");
        let (v, _) = run_with_host("timeout = 1\nwaitrecv 'XYZ' 6 4\nr = result\ns = inputstr", host);
        assert_eq!(v.int_of("r"), Some(1));
        assert_eq!(v.str_of("s").unwrap(), b"abcXYZ");
        let host = NullHost::shared();
        host.feed(b"abcXYZdef");
        let (v, _) = run_with_host("timeout = 1\nwaitrecv 'XYZ' 6 1\ns = inputstr", host);
        assert_eq!(v.str_of("s").unwrap(), b"XYZdef");
        // 子字串出現了但視窗沒填滿就逾時 → -1
        let host = NullHost::shared();
        host.feed(b"XYZ");
        let (v, _) = run_with_host("mtimeout = 30\nwaitrecv 'XYZ' 6 1\nr = result\ns = inputstr", host);
        assert_eq!(v.int_of("r"), Some(-1));
        assert_eq!(v.str_of("s").unwrap(), b"XYZ");
        // 沒出現 → 0、inputstr 空
        let host = NullHost::shared();
        host.feed(b"nothing");
        let (v, _) = run_with_host("mtimeout = 30\nwaitrecv 'XYZ' 6 1\nr = result\ns = inputstr", host);
        assert_eq!(v.int_of("r"), Some(0));
        assert_eq!(v.str_of("s").unwrap(), b"");
        // 長度／位置超出範圍會被夾住（不 panic）
        let host = NullHost::shared();
        host.feed(b"XYZ");
        let (v, _) = run_with_host("timeout = 1\nwaitrecv 'XYZ' 0 99\nr = result\ns = inputstr", host);
        assert_eq!(v.int_of("r"), Some(1));
        assert_eq!(v.str_of("s").unwrap(), b"XYZ");
    }

    /// `yesnobox` 按 Esc／取消 → `result = 0`（前端送的是 -1；`if result then` 不可以走「是」）。
    #[test]
    fn yesnobox_cancel_is_no() {
        let host = Arc::new(NullHost::default());
        *host.answer.lock().unwrap() = DialogAnswer {
            number: -1,
            cancelled: true,
            ..Default::default()
        };
        let (v, _) = run_with_host("yesnobox '要嗎' 'T'\nr = result", host);
        assert_eq!(v.int_of("r"), Some(0));
    }

    /// `flushrecv` 之後 `wait` 不該再看到舊資料。
    #[test]
    fn flushrecv_drops_pending_data() {
        let host = NullHost::shared();
        host.feed(b"OK");
        let (v, _) = run_with_host("mtimeout = 30\nflushrecv\nwait 'OK'\nr = result", host);
        assert_eq!(v.int_of("r"), Some(0));
    }

    /// `wait` 比對的是**去掉 ANSI 之後**的文字（照舊版；見 host.rs 的說明）。
    #[test]
    fn wait_ignores_ansi_colours() {
        let host = NullHost::shared();
        host.feed(b"\x1b[32mPS C:\\>\x1b[0m ");
        let (v, _) = run_with_host("timeout = 1\nwait 'PS C:'\nr = result", host);
        assert_eq!(v.int_of("r"), Some(1));
    }

    /// 中文跨 chunk 也比對得到。
    #[test]
    fn wait_matches_chinese_across_chunks() {
        let host = NullHost::shared();
        let bytes = "處理完成了".as_bytes();
        host.feed(&bytes[..4]);
        host.feed(&bytes[4..]);
        let (v, _) = run_with_host("timeout = 1\nwait '完成'\nr = result", host);
        assert_eq!(v.int_of("r"), Some(1));
    }

    /// `pause` 會真的等（但被中斷時立刻回來）。
    #[test]
    fn pause_waits_and_can_be_interrupted() {
        let host = NullHost::shared();
        let t0 = std::time::Instant::now();
        let (_, _) = run_with_host("mpause 40", host);
        assert!(t0.elapsed().as_millis() >= 20, "至少等了一下");

        // 中斷：巨集要以 Interrupted 結束
        let host = NullHost::shared();
        host.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let mut it = Interp::from_text("t.ttl", "mpause 5000").unwrap();
        it.set_host(Some(host));
        assert_eq!(it.run(100).unwrap_err().err, Err::Interrupted);
    }

    /// 對話框：`messagebox` 只是通知，`yesnobox`／`inputbox` 有回傳值。
    #[test]
    fn dialogs_round_trip() {
        let host = NullHost::shared();
        let (_, h) = run_with_host("messagebox '嗨' '標題'", host);
        let asked = h.asked.lock().unwrap();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].kind, "message");
        assert_eq!(asked[0].message, "嗨");
        assert_eq!(asked[0].title, "標題");
        drop(asked);

        let host = Arc::new(NullHost::default());
        *host.answer.lock().unwrap() = DialogAnswer {
            number: 1,
            ..Default::default()
        };
        let (v, _) = run_with_host("yesnobox '要嗎' 'T'\nr = result", host);
        assert_eq!(v.int_of("r"), Some(1));

        let host = Arc::new(NullHost::default());
        *host.answer.lock().unwrap() = DialogAnswer {
            number: 1,
            text: "打的字".into(),
            cancelled: false,
        };
        let (v, _) = run_with_host("inputbox '請輸入' 'T' '預設'\ns = inputstr\nr = result", host);
        assert_eq!(v.str_of("s").unwrap(), "打的字".as_bytes());
        assert_eq!(v.int_of("r"), Some(1));

        // 取消 → result=0、inputstr 清空
        let host = Arc::new(NullHost::default());
        *host.answer.lock().unwrap() = DialogAnswer {
            cancelled: true,
            ..Default::default()
        };
        let (v, _) = run_with_host("inputbox '請輸入'\nr = result\ns = inputstr", host);
        assert_eq!(v.int_of("r"), Some(0));
        assert_eq!(v.str_of("s").unwrap(), b"");
    }

    /// `listbox` 的選項來自字串陣列，`result` ＝選了第幾項（0 起算）。
    #[test]
    fn listbox_reads_string_array() {
        let host = Arc::new(NullHost::default());
        *host.answer.lock().unwrap() = DialogAnswer {
            number: 2,
            ..Default::default()
        };
        let (v, h) = run_with_host(
            "strdim items 3\nitems[0] = 'a'\nitems[1] = 'b'\nitems[2] = 'c'\nlistbox '選一個' 'T' items\nr = result",
            host,
        );
        assert_eq!(v.int_of("r"), Some(2));
        let asked = h.asked.lock().unwrap();
        assert_eq!(asked[0].items, vec!["a", "b", "c"]);
    }

    /// `testlink`：連著是 2、沒連著是 0。
    #[test]
    fn testlink_reports_state() {
        let (v, _) = run("testlink\nr = result");
        assert_eq!(v.int_of("r"), Some(2));
        let host = Arc::new(NullHost {
            link: false,
            ..Default::default()
        });
        let (v, _) = run_with_host("testlink\nr = result", host);
        assert_eq!(v.int_of("r"), Some(0));
    }

    /// `connect`：已經連著就是 `result=2`（同原碼）。
    #[test]
    fn connect_when_already_linked() {
        let (v, _) = run("connect '127.0.0.1:23 /telnet'\nr = result");
        assert_eq!(v.int_of("r"), Some(2));
    }

    /// `dispstr` 印在畫面上、不送給遠端。
    #[test]
    fn dispstr_goes_to_screen() {
        let (_, h) = run("dispstr '畫面上' #13 #10");
        assert!(h.sent.lock().unwrap().is_empty(), "不可以送給遠端");
        assert!(h.echoed.lock().unwrap()[0].contains("畫面上"));
    }
}

