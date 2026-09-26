//! TTL 直譯器的執行模型——照 `ttpmacro/ttl.cpp` 的 `Exec`／`ExecCmnd` 與
//! `ttpmacro/ttmbuff.c` 的 buffer／stack。
//!
//! ## 為什麼是「一行一步」的狀態機
//!
//! 原碼不是先建 AST 再走一遍，而是**每次只讀一行、執行一行**，流程控制靠幾個
//! 「跳過」旗標 + 一個小堆疊：
//!
//! | 原碼的全域 | 意思 | 這裡 |
//! |---|---|---|
//! | `IfNest` | 目前在幾層 `if … endif` 裡 | [`Interp::if_nest`] |
//! | `ElseFlag` | 正在往前找 `else`／`elseif`／`endif` | [`Interp::else_flag`] |
//! | `EndIfFlag` | 正在往前找 `endif` | [`Interp::end_if_flag`] |
//! | `EndWhileFlag` | 正在往前找迴圈結尾（`while` 條件不成立時） | [`Interp::end_while_flag`] |
//! | `BreakFlag`／`ContinueFlag` | `break`／`continue` 正在往前找迴圈結尾 | 同名欄位 |
//! | `NextFlag` | `next` 跳回 `for` 那一行時的「這是續跑」標記 | [`Interp::next_flag`] |
//! | `ParseAgain` | 這一行還沒讀完（單行 `if`／`execcmnd`） | [`Interp::parse_again`] |
//! | `PtrStack`／`LevelStack`／`TypeStack`（`MAXSP` 10） | call／for／while 的堆疊 | [`Interp::stack`] |
//! | `Buff[]`／`BuffPtr[]`（`MAXNESTLEVEL` 10） | include 的每一層來源與位置 | [`Interp::buffers`] |
//!
//! **照抄這個模型有一個現在就用得到的好處**：一步＝一行，所以第二批的 `wait`／`pause`
//! 只要讓 [`Interp::step`] 回 [`Step::Waiting`] 就能掛起，不必把直譯器改成另一種寫法。
//!
//! ## 刻意的簡化（不影響行為）
//!
//! 原碼的跳躍位置是「檔案內的位元組偏移」；我們用**行索引**。因為 `GetRawLine` 永遠從
//! 行開頭開始讀，兩者等價，但行索引不必重算行號。

use std::collections::HashMap;

use super::error::{Err, Result, TtlError};
use super::expr::{self, Eval, Val};
use super::lex::Lexer;
use super::vars::{VarType, Vars};
use super::words::Word;

/// `MAXNESTLEVEL`：include 的深度上限。
pub const MAX_NEST_LEVEL: usize = 10;
/// `MAXSP`：call／for／while 的堆疊上限。
pub const MAX_SP: usize = 10;

/// `include` 時去要檔案的 callback。抽成型別別名讓 clippy 開心，也比較好讀。
pub type Loader = Box<dyn FnMut(&str) -> std::result::Result<Source, String> + Send>;

/// 一個來源檔（include 一層一個）。
#[derive(Clone, Debug)]
pub struct Source {
    /// 顯示用的檔名（錯誤訊息要用）。
    pub name: String,
    /// 每一行的原始位元組（不含換行）。
    pub lines: Vec<Vec<u8>>,
}

impl Source {
    /// 從檔案內容切行。`\r\n`／`\n`／`\r` 都算換行——原碼是「遇到 < 0x20 的字元就結束這一行，
    /// 接著把連續的控制字元跳掉」，效果一樣。
    pub fn new(name: impl Into<String>, text: &[u8]) -> Self {
        let mut lines = Vec::new();
        let mut cur = Vec::new();
        for &b in text {
            if b < 0x20 && b != b'\t' {
                if !cur.is_empty() {
                    lines.push(std::mem::take(&mut cur));
                } else {
                    lines.push(Vec::new());
                }
            } else {
                cur.push(b);
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        Self {
            name: name.into(),
            lines,
        }
    }
}

/// include 的一層。
struct Buffer {
    src: Source,
    /// 下一行要讀的位置。
    line: usize,
}

/// 堆疊上的一個框（`TypeStack` 的三種：`CtlCall`／`CtlFor`／`CtlWhile`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ctl {
    /// `call`：回來時要跳回這裡。
    Call { level: usize, line: usize },
    /// `for`：迴圈頭那一行。`valid = false` ＝已經是最後一圈（原碼的 `INVALIDPTR`）。
    For {
        level: usize,
        line: usize,
        valid: bool,
    },
    /// `while`／`do`：迴圈頭那一行。
    While { level: usize, line: usize },
}

/// 一次 [`Interp::step`] 的結果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// 執行了一行，還有得跑。
    Ran,
    /// 巨集結束（跑到檔尾、`end`、或 `exit`）。
    Finished,
}

/// 直譯器。
pub struct Interp {
    buffers: Vec<Buffer>,
    lex: Lexer,
    pub vars: Vars,
    stack: Vec<Ctl>,

    if_nest: u32,
    else_flag: u32,
    end_if_flag: u32,
    end_while_flag: u32,
    break_flag: u32,
    continue_flag: bool,
    next_flag: bool,
    parse_again: bool,
    finished: bool,

    /// `setexitcode` 設定的值。
    pub exit_code: i32,
    /// 目前這一行的行號（1 起算）。
    line_no: usize,
    /// `include` 要用的檔案讀取器。抽成 callback 是為了讓測試不必真的碰檔案系統。
    loader: Loader,
    /// 執行過幾行（防呆：`ttl_probe` 用來擋住寫壞的無窮迴圈）。
    pub steps: u64,
    /// 會碰外界的指令（`send`／`wait`／對話框…）要用的介面。
    /// `None` ＝只能跑不碰 I/O 的指令（單元測試預設就是這樣）。
    host: Option<std::sync::Arc<dyn super::host::MacroHost>>,
    /// 巨集的「目前目錄」（原碼的 `CurrentDir`）：相對路徑相對於它。
    /// **不是行程的工作目錄**——`setdir`／`changedir` 只改這個。
    current_dir: std::path::PathBuf,
    /// 開著的檔案（`fileopen` 給的整數就是這裡的索引，同原碼的 handle 表）。
    files: std::collections::HashMap<i32, super::files::OpenFile>,
    /// `findfirst` 還沒吐完的檔名。
    finds: std::collections::HashMap<i32, Vec<String>>,
    /// 下一個控制代碼。
    next_handle: i32,
}

impl Interp {
    /// 從一份來源建直譯器。`include` 會用 `loader` 去要檔案。
    pub fn new(
        src: Source,
        loader: Loader,
    ) -> std::result::Result<Self, TtlError> {
        let mut it = Self {
            buffers: Vec::new(),
            lex: Lexer::new(b""),
            vars: Vars::with_system_vars(),
            stack: Vec::new(),
            if_nest: 0,
            else_flag: 0,
            end_if_flag: 0,
            end_while_flag: 0,
            break_flag: 0,
            continue_flag: false,
            next_flag: false,
            parse_again: false,
            finished: false,
            exit_code: 0,
            line_no: 0,
            loader,
            steps: 0,
            host: None,
            current_dir: std::env::current_dir().unwrap_or_default(),
            files: std::collections::HashMap::new(),
            finds: std::collections::HashMap::new(),
            next_handle: 0,
        };
        it.push_buffer(src)?;
        Ok(it)
    }

    /// 只從一段文字建（測試與 `ttl_probe` 用；`include` 會失敗）。
    pub fn from_text(name: &str, text: &str) -> std::result::Result<Self, TtlError> {
        Self::new(
            Source::new(name, text.as_bytes()),
            Box::new(|f| Err(format!("這個直譯器沒有檔案載入器，include 不能用：{f}"))),
        )
    }

    /// 從檔案建（`include` 會相對於主檔所在的資料夾找）。
    pub fn from_file(path: &std::path::Path) -> std::result::Result<Self, TtlError> {
        let text = std::fs::read(path).map_err(|_| TtlError {
            err: Err::CantOpen,
            line_no: 0,
            line: String::new(),
            file: path.display().to_string(),
            start: 0,
            end: 0,
        })?;
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let base = dir.clone();
        let mut it = Self::new(
            Source::new(name, &text),
            Box::new(move |f| {
                let p = dir.join(f);
                let text = std::fs::read(&p).map_err(|e| format!("{}：{e}", p.display()))?;
                Ok(Source::new(
                    p.file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    &text,
                ))
            }),
        )?;
        // 巨集的「目前目錄」預設是巨集檔所在的資料夾（同原碼啟動時的 CurrentDir）
        if !base.as_os_str().is_empty() {
            it.set_current_dir(base);
        }
        Ok(it)
    }

    /// 跑到結束（或超過 `max_steps` 行——防呆用）。
    pub fn run(&mut self, max_steps: u64) -> std::result::Result<(), TtlError> {
        while !self.finished {
            if self.steps >= max_steps {
                return Err(self.error(Err::StackOver));
            }
            if self.step()? == Step::Finished {
                break;
            }
        }
        Ok(())
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn line_no(&self) -> usize {
        self.line_no
    }

    /// 目前的檔名（`include` 進去時是被 include 的那個檔）。
    pub fn file_name(&self) -> &str {
        self.buffers.last().map(|b| b.src.name.as_str()).unwrap_or("")
    }

    /// 執行一行（原碼的 `Exec()`）。
    pub fn step(&mut self) -> std::result::Result<Step, TtlError> {
        if self.finished {
            return Ok(Step::Finished);
        }
        self.steps += 1;
        if !self.parse_again && !self.next_line() {
            self.finished = true;
            return Ok(Step::Finished);
        }
        self.parse_again = false;
        match self.exec_cmnd() {
            Ok(()) => {
                if self.finished {
                    Ok(Step::Finished)
                } else {
                    Ok(Step::Ran)
                }
            }
            Err(e) => Err(self.error(e)),
        }
    }

    /// 把錯誤碼包成帶位置的錯誤（原碼 `DispErr` 取的那些欄位）。
    fn error(&self, err: Err) -> TtlError {
        let start = self.lex.parse_ptr();
        let end = if start == self.lex.ptr() {
            self.lex.line().len()
        } else {
            self.lex.ptr()
        };
        TtlError {
            err,
            line_no: self.line_no,
            line: String::from_utf8_lossy(self.lex.line()).into_owned(),
            file: self.file_name().to_string(),
            start,
            end,
        }
    }

    // ------------------------------------------------------------ buffer / 行

    fn push_buffer(&mut self, src: Source) -> std::result::Result<(), TtlError> {
        if self.buffers.len() >= MAX_NEST_LEVEL {
            return Err(self.error(Err::StackOver));
        }
        let level = self.buffers.len();
        self.buffers.push(Buffer { src, line: 0 });
        self.register_labels(level)?;
        Ok(())
    }

    /// `RegisterLabels`：先掃一遍把 `:label` 記下來（**跳到未來的標籤要靠這個**）。
    fn register_labels(&mut self, level: usize) -> std::result::Result<(), TtlError> {
        let lines = self.buffers[level].src.lines.clone();
        for (i, line) in lines.iter().enumerate() {
            let mut lex = Lexer::new(line);
            if lex.first_char() != b':' {
                continue;
            }
            let Some(name) = lex.label_name() else {
                continue;
            };
            if lex.first_char() != 0 {
                // 標籤後面還有東西 → 語法錯誤（同原碼）
                self.line_no = i + 1;
                self.lex.reset(line);
                return Err(self.error(Err::Syntax));
            }
            if self.vars.find(&name).is_some() {
                self.line_no = i + 1;
                self.lex.reset(line);
                return Err(self.error(Err::LabelAlreadyDef));
            }
            // 原碼記的是「標籤那一行之後」的位置
            self.vars.new_label(&name, i + 1, level);
        }
        Ok(())
    }

    /// `GetNewLine`：讀下一行「有東西」的行；讀完就往上一層 include 退。
    fn next_line(&mut self) -> bool {
        loop {
            // 這一層讀完了 → 退一層（最外層讀完＝結束）
            while self
                .buffers
                .last()
                .map(|b| b.line >= b.src.lines.len())
                .unwrap_or(true)
            {
                if self.buffers.len() <= 1 {
                    return false;
                }
                let level = self.buffers.len() - 1;
                self.buffers.pop();
                self.vars.drop_labels_of_level(level);
                // 堆疊上屬於那一層的框也要丟掉（同 `CloseBuff`）
                self.stack.retain(|c| ctl_level(c) < level);
            }
            let b = self.buffers.last_mut().unwrap();
            let line = b.src.lines[b.line].clone();
            self.line_no = b.line + 1;
            b.line += 1;
            self.lex.reset(&line);
            // 空行與只有標籤的行跳過（同原碼）
            let c = self.lex.first_char();
            if c == 0 || c == b':' {
                continue;
            }
            self.lex.set_ptr(self.lex.ptr() - 1);
            return true;
        }
    }

    fn level(&self) -> usize {
        self.buffers.len() - 1
    }

    /// 跳到某一行（同一層）。
    fn jump_to(&mut self, line: usize) {
        if let Some(b) = self.buffers.last_mut() {
            b.line = line;
        }
    }

    /// 退回某一層（`JumpToLabel`／`NextLoop` 都會做這件事）。
    fn unwind_to_level(&mut self, level: usize) {
        while self.buffers.len() > level + 1 {
            let l = self.buffers.len() - 1;
            self.buffers.pop();
            self.vars.drop_labels_of_level(l);
            self.stack.retain(|c| ctl_level(c) < l);
        }
    }

    // ------------------------------------------------------------ 主分派

    /// `ExecCmnd()`：跳過旗標的階梯 → 指令分派 → 賦值。
    fn exec_cmnd(&mut self) -> Result<()> {
        self.lex.mark_token_start();
        let word = self.lex.reserved_word();

        // ---- 1. 正在找迴圈結尾（while 條件不成立）----
        if self.end_while_flag > 0 {
            if let Some(w) = word {
                match w {
                    Word::While | Word::Until | Word::Do => self.end_while_flag += 1,
                    Word::EndWhile | Word::EndUntil | Word::Loop => self.end_while_flag -= 1,
                    _ => {}
                }
            }
            return Ok(());
        }

        // ---- 2. break / continue 正在往前找迴圈結尾 ----
        if self.break_flag > 0 {
            if let Some(w) = word {
                match w {
                    Word::If => {
                        if self.check_then()? {
                            self.if_nest += 1;
                        }
                    }
                    Word::EndIf => {
                        if self.if_nest < 1 {
                            return Err(Err::InvalidCtl);
                        }
                        self.if_nest -= 1;
                    }
                    Word::For | Word::While | Word::Until | Word::Do => self.break_flag += 1,
                    Word::Next | Word::EndWhile | Word::EndUntil | Word::Loop => {
                        self.break_flag -= 1
                    }
                    _ => {}
                }
            }
            if self.break_flag > 0 || !self.continue_flag {
                return Ok(());
            }
            // continue：找到迴圈結尾了 → 讓它照常執行（`next`／`endwhile` 會跳回去）
            self.continue_flag = false;
        }

        // ---- 3. 正在找 endif ----
        if self.end_if_flag > 0 {
            if let Some(w) = word {
                if w == Word::If && self.check_then()? {
                    self.end_if_flag += 1;
                } else if w == Word::EndIf {
                    self.end_if_flag -= 1;
                }
            }
            return Ok(());
        }

        // ---- 4. 正在找 else / elseif / endif ----
        if self.else_flag > 0 {
            if let Some(w) = word {
                match w {
                    Word::If if self.check_then()? => self.end_if_flag += 1,
                    Word::Else => self.else_flag -= 1,
                    Word::ElseIf => {
                        if self.check_else_if()? != 0 {
                            self.else_flag -= 1;
                        }
                    }
                    Word::EndIf => {
                        self.else_flag -= 1;
                        if self.else_flag == 0 {
                            self.if_nest -= 1;
                        }
                    }
                    _ => {}
                }
            }
            return Ok(());
        }

        // ---- 5. 真的執行 ----
        match word {
            Some(w) => self.dispatch(w),
            None => self.assignment(),
        }
    }

    /// 保留字 → 指令。沒實作的保留字回 `Unknown command.`（同原碼對不認識指令的處理）。
    fn dispatch(&mut self, w: Word) -> Result<()> {
        match w {
            // ---- 流程控制 ----
            Word::If => self.cmd_if(),
            Word::Then => Err(Err::Syntax), // `then` 只能跟在 if／elseif 後面
            Word::Else => self.cmd_else(),
            Word::ElseIf => self.cmd_else_if(),
            Word::EndIf => self.cmd_end_if(),
            Word::For => self.cmd_for(),
            Word::Next => self.cmd_next(),
            Word::While => self.cmd_while(true),
            Word::Until => self.cmd_while(false),
            Word::EndWhile => self.cmd_end_while(true),
            Word::EndUntil => self.cmd_end_while(false),
            Word::Do => self.cmd_do(),
            Word::Loop => self.cmd_loop(),
            Word::Break => self.cmd_break(false),
            Word::Continue => self.cmd_break(true),
            Word::Goto => self.cmd_goto(),
            Word::Call => self.cmd_call(),
            Word::Return => self.cmd_return(),
            Word::End | Word::Exit => self.cmd_end(),
            Word::Include => self.cmd_include(),
            Word::IfDefined => self.cmd_if_defined(),
            // ---- 其餘（字串／整數／時間…）在 cmds.rs ----
            other => self.dispatch_cmds(other),
        }
    }

    // ------------------------------------------------------------ 參數輔助

    /// `GetIntVal`。
    pub(super) fn int_val(&mut self) -> Result<i32> {
        expr::int_val(&mut self.lex, &self.vars)
    }

    /// `GetStrVal`。
    pub(super) fn str_val(&mut self) -> Result<Vec<u8>> {
        expr::str_val(&mut self.lex, &self.vars)
    }

    /// 參數列尾端檢查：後面不可以還有東西（原碼每個指令最後都做這件事）。
    pub(super) fn end_of_args(&mut self) -> Result<()> {
        if self.lex.first_char() != 0 {
            Err(Err::Syntax)
        } else {
            Ok(())
        }
    }

    /// 巨集的目前目錄（相對路徑的基準）。
    pub fn current_dir(&self) -> &std::path::Path {
        &self.current_dir
    }

    pub fn set_current_dir(&mut self, dir: std::path::PathBuf) {
        self.current_dir = dir;
    }

    /// 放一個開著的檔案進表裡，回傳控制代碼。
    pub(super) fn files_put(&mut self, f: super::files::OpenFile) -> i32 {
        let h = self.next_handle;
        self.next_handle += 1;
        self.files.insert(h, f);
        h
    }

    /// 拿走（關閉）一個檔案。
    pub(super) fn files_take(&mut self, h: i32) -> Option<super::files::OpenFile> {
        self.files.remove(&h)
    }

    /// 對某個開著的檔案做一件事。控制代碼不對就回 `None`（同原碼：無效的 handle 不是錯誤）。
    pub(super) fn files_with<T>(
        &mut self,
        h: i32,
        f: impl FnOnce(&mut super::files::OpenFile) -> T,
    ) -> Option<T> {
        self.files.get_mut(&h).map(f)
    }

    pub(super) fn finds_put(&mut self, names: Vec<String>) -> i32 {
        let h = self.next_handle;
        self.next_handle += 1;
        self.finds.insert(h, names);
        h
    }

    pub(super) fn finds_next(&mut self, h: i32) -> Option<String> {
        let list = self.finds.get_mut(&h)?;
        if list.is_empty() {
            None
        } else {
            Some(list.remove(0))
        }
    }

    pub(super) fn finds_take(&mut self, h: i32) {
        self.finds.remove(&h);
    }

    /// 掛上（或卸下）外界介面。執行器在開始跑之前呼叫一次。
    pub fn set_host(&mut self, host: Option<std::sync::Arc<dyn super::host::MacroHost>>) {
        self.host = host;
    }

    pub(super) fn host(&self) -> Option<std::sync::Arc<dyn super::host::MacroHost>> {
        self.host.clone()
    }

    /// 讀一個字串常值（`send` 的參數列要「字串或運算式」輪流試）。
    pub(super) fn lex_string(&mut self) -> Result<Option<Vec<u8>>> {
        self.lex.mark_token_start();
        self.lex.string()
    }

    /// 讀一個運算式；這裡沒有東西就回 `None`（不是錯誤）。
    pub(super) fn try_expression(&mut self) -> Result<Option<super::expr::Val>> {
        self.lex.mark_token_start();
        super::expr::Eval::new(&mut self.lex, &self.vars).expression()
    }

    /// 直接讀一個識別字（`intdim` 這種「名稱不是變數參照」的指令用）。
    pub(super) fn lex_identifier(&mut self) -> Option<String> {
        self.lex.mark_token_start();
        self.lex.identifier()
    }

    /// 後面還有東西嗎（原碼 `GetFirstChar()!=0` 的那個檢查）。
    pub(super) fn lex_first_char_nonzero(&mut self) -> bool {
        self.lex.first_char() != 0
    }

    pub(super) fn parameter_given(&mut self) -> bool {
        self.lex.parameter_given()
    }

    /// `GetIntVar`／`GetStrVar`：目標變數。**不存在就自動建**（原碼就是這樣，
    /// 所以 `int2str istr i` 不用先宣告 `istr`）。
    pub(super) fn var_ref(&mut self, want: VarType) -> Result<VarRef> {
        self.lex.mark_token_start();
        let Some(name) = self.lex.identifier() else {
            return Err(Err::Syntax);
        };
        let index = Eval::new(&mut self.lex, &self.vars).index()?;
        match self.vars.var_type(&name) {
            VarType::Unknown => {
                // 自動建立（陣列索引寫在沒宣告的名字上是型別不符）
                if index.is_some() {
                    return Err(Err::TypeMismatch);
                }
                match want {
                    VarType::Integer => self.vars.new_int(&name, 0),
                    VarType::String => self.vars.new_str(&name, b""),
                    _ => return Err(Err::TypeMismatch),
                }
                Ok(VarRef { name, index: None })
            }
            t if t == want && index.is_none() => Ok(VarRef { name, index: None }),
            VarType::IntArray if want == VarType::Integer => match index {
                Some(i) => Ok(VarRef {
                    name,
                    index: Some(i),
                }),
                None => Err(Err::TypeMismatch),
            },
            VarType::StrArray if want == VarType::String => match index {
                Some(i) => Ok(VarRef {
                    name,
                    index: Some(i),
                }),
                None => Err(Err::TypeMismatch),
            },
            _ => Err(Err::TypeMismatch),
        }
    }

    pub(super) fn set_int_ref(&mut self, r: &VarRef, val: i32) -> Result<()> {
        match r.index {
            Some(i) => self.vars.set_int_at(&r.name, i, val),
            None => {
                self.vars.set_int(&r.name, val);
                Ok(())
            }
        }
    }

    pub(super) fn set_str_ref(&mut self, r: &VarRef, val: &[u8]) -> Result<()> {
        match r.index {
            Some(i) => self.vars.set_str_at(&r.name, i, val),
            None => {
                self.vars.set_str(&r.name, val);
                Ok(())
            }
        }
    }

    pub(super) fn get_str_ref(&self, r: &VarRef) -> Result<Vec<u8>> {
        match r.index {
            Some(i) => Ok(self.vars.str_at(&r.name, i)?.to_vec()),
            None => Ok(self.vars.str_of(&r.name).unwrap_or(b"").to_vec()),
        }
    }

    // ------------------------------------------------------------ 流程控制指令

    /// `CheckThen`：這一行後面有沒有 `then`（原碼會跳過非字母的東西一直找）。
    fn check_then(&mut self) -> Result<bool> {
        loop {
            let mut b;
            loop {
                b = self.lex.first_char();
                if b == 0 {
                    return Ok(false);
                }
                if b.is_ascii_alphabetic() || b == b'_' {
                    break;
                }
            }
            self.lex.set_ptr(self.lex.ptr() - 1);
            let Some(word) = self.lex.identifier() else {
                return Ok(false);
            };
            if word.eq_ignore_ascii_case("then") {
                if self.lex.first_char() != 0 {
                    return Err(Err::Syntax);
                }
                return Ok(true);
            }
        }
    }

    /// `CheckElseIf`：`elseif <運算式> then`。
    fn check_else_if(&mut self) -> Result<i32> {
        let val = self.int_val()?;
        match self.lex.reserved_word() {
            Some(Word::Then) if self.lex.first_char() == 0 => Ok(val),
            _ => Err(Err::Syntax),
        }
    }

    /// `TTLIf`：區塊式（`if … then`）與單行式（`if <cond> <指令>`）。
    fn cmd_if(&mut self) -> Result<()> {
        let val = match Eval::new(&mut self.lex, &self.vars).expression()? {
            Some(Val::Int(v)) => v,
            Some(Val::Str(_)) => return Err(Err::TypeMismatch),
            None => return Err(Err::Syntax),
        };
        let tmp = self.lex.ptr();
        if self.lex.reserved_word() == Some(Word::Then) {
            // 區塊式
            if self.lex.first_char() != 0 {
                return Err(Err::Syntax);
            }
            self.if_nest += 1;
            if val == 0 {
                self.else_flag = 1; // 往前找 else／elseif／endif
            }
            Ok(())
        } else {
            // 單行式：條件成立就執行同一行後面那個指令
            self.lex.set_ptr(tmp);
            if !self.lex.parameter_given() {
                return Err(Err::Syntax);
            }
            if val == 0 {
                return Ok(());
            }
            self.exec_cmnd()
        }
    }

    fn cmd_else(&mut self) -> Result<()> {
        self.end_of_args()?;
        if self.if_nest < 1 {
            return Err(Err::InvalidCtl);
        }
        // true 分支跑完落到 else → 往前找 endif
        self.if_nest -= 1;
        self.end_if_flag = 1;
        Ok(())
    }

    fn cmd_else_if(&mut self) -> Result<()> {
        self.check_else_if()?;
        if self.if_nest < 1 {
            return Err(Err::InvalidCtl);
        }
        self.if_nest -= 1;
        self.end_if_flag = 1;
        Ok(())
    }

    fn cmd_end_if(&mut self) -> Result<()> {
        self.end_of_args()?;
        if self.if_nest < 1 {
            return Err(Err::InvalidCtl);
        }
        self.if_nest -= 1;
        Ok(())
    }

    /// `TTLFor`：`for <整數變數> <起> <迄>`。
    ///
    /// 第一次進來時把變數設成起始值並壓一個框；`next` 會跳回這一行，
    /// 那時 `next_flag` 是 true ＝「續跑」，把變數往目標值走一步。
    /// **起＝迄時只跑一圈**，而且變數可以遞減（原碼用 `i<ValEnd` / `i>ValEnd` 判斷方向）。
    fn cmd_for(&mut self) -> Result<()> {
        let var = self.var_ref(VarType::Integer)?;
        let start = self.int_val()?;
        let end = self.int_val()?;
        self.end_of_args()?;

        if !self.take_next_flag() {
            // 第一次
            if self.stack.len() >= MAX_SP {
                return Err(Err::StackOver);
            }
            let line = self.line_no - 1; // 這一行（`for` 自己）
            self.stack.push(Ctl::For {
                level: self.level(),
                line,
                valid: true,
            });
            self.set_int_ref(&var, start)?;
            if start == end {
                self.last_for_loop();
            }
        } else {
            let mut i = match var.index {
                Some(idx) => self.vars.int_at(&var.name, idx)?,
                None => self.vars.int_of(&var.name).unwrap_or(0),
            };
            if i < end {
                i += 1;
            } else if i > end {
                i -= 1;
            }
            self.set_int_ref(&var, i)?;
            if i == end {
                self.last_for_loop();
            }
        }
        Ok(())
    }

    fn take_next_flag(&mut self) -> bool {
        let f = self.next_flag;
        self.next_flag = false;
        f
    }

    /// `LastForLoop`：把最上面的 for 框標成「最後一圈」。
    fn last_for_loop(&mut self) {
        if let Some(Ctl::For { valid, .. }) = self.stack.last_mut() {
            *valid = false;
        }
    }

    /// `TTLNext` → `NextLoop`。
    fn cmd_next(&mut self) -> Result<()> {
        self.end_of_args()?;
        let Some(&Ctl::For { level, line, valid }) = self.stack.last() else {
            return Err(Err::InvalidCtl);
        };
        self.next_flag = valid;
        if !valid {
            self.stack.pop();
            return Ok(());
        }
        self.unwind_to_level(level);
        self.jump_to(line);
        Ok(())
    }

    /// `TTLWhile`（`mode=true`）／`TTLUntil`（`mode=false`）。
    fn cmd_while(&mut self, mode: bool) -> Result<()> {
        let val = self.int_val()?;
        self.end_of_args()?;
        if (val != 0) == mode {
            if self.stack.len() >= MAX_SP {
                return Err(Err::StackOver);
            }
            self.stack.push(Ctl::While {
                level: self.level(),
                line: self.line_no - 1,
            });
        } else {
            self.end_while_flag = 1; // 往前找 endwhile／enduntil
        }
        Ok(())
    }

    /// `TTLEndWhile`／`TTLEndUntil` → `BackToWhile`。
    ///
    /// 可以帶條件（`endwhile <運算式>`）：條件與 mode 一致才跳回去。
    fn cmd_end_while(&mut self, mode: bool) -> Result<()> {
        let mut val = i32::from(mode);
        if self.parameter_given() {
            val = self.int_val()?;
        }
        self.end_of_args()?;
        self.back_to_while((val != 0) == mode)
    }

    fn back_to_while(&mut self, jump: bool) -> Result<()> {
        let Some(&Ctl::While { level, line }) = self.stack.last() else {
            return Err(Err::InvalidCtl);
        };
        self.stack.pop();
        self.unwind_to_level(level);
        if jump {
            self.jump_to(line);
        }
        Ok(())
    }

    /// `TTLDo`：`do` / `do while <cond>` / `do until <cond>`。
    fn cmd_do(&mut self) -> Result<()> {
        let mut val = 1;
        if self.parameter_given() {
            match self.lex.reserved_word() {
                Some(Word::While) => val = self.int_val()?,
                Some(Word::Until) => val = i32::from(self.int_val()? == 0),
                _ => return Err(Err::Syntax),
            }
            self.end_of_args()?;
        }
        if val != 0 {
            if self.stack.len() >= MAX_SP {
                return Err(Err::StackOver);
            }
            self.stack.push(Ctl::While {
                level: self.level(),
                line: self.line_no - 1,
            });
        } else {
            self.end_while_flag = 1;
        }
        Ok(())
    }

    /// `TTLLoop`：`loop` / `loop while <cond>` / `loop until <cond>`。
    fn cmd_loop(&mut self) -> Result<()> {
        let mut val = 1;
        if self.parameter_given() {
            match self.lex.reserved_word() {
                Some(Word::While) => val = self.int_val()?,
                Some(Word::Until) => val = i32::from(self.int_val()? == 0),
                _ => return Err(Err::Syntax),
            }
            self.end_of_args()?;
        }
        self.back_to_while(val != 0)
    }

    /// `TTLBreak`／`TTLContinue` → `BreakLoop`。
    fn cmd_break(&mut self, is_continue: bool) -> Result<()> {
        self.end_of_args()?;
        let Some(top) = self.stack.last().copied() else {
            return Err(Err::InvalidCtl);
        };
        match top {
            Ctl::For { level, .. } | Ctl::While { level, .. } => {
                if is_continue {
                    self.continue_flag = true;
                } else {
                    self.stack.pop();
                    self.unwind_to_level(level);
                }
                self.break_flag = 1;
                Ok(())
            }
            Ctl::Call { .. } => Err(Err::InvalidCtl),
        }
    }

    /// `TTLGoto` → `JumpToLabel`。
    fn cmd_goto(&mut self) -> Result<()> {
        let Some(name) = self.lex.label_name() else {
            return Err(Err::Syntax);
        };
        if self.lex.first_char() != 0 {
            return Err(Err::Syntax);
        }
        let Some((line, level)) = self.vars.label(&name) else {
            return Err(Err::LabelReq);
        };
        if level < self.level() {
            self.unwind_to_level(level);
        }
        self.jump_to(line);
        Ok(())
    }

    /// `TTLCall` → `CallToLabel`。**只能呼叫同一層的標籤**（原碼：不同層回 `ErrCantCall`）。
    fn cmd_call(&mut self) -> Result<()> {
        let Some(name) = self.lex.label_name() else {
            return Err(Err::Syntax);
        };
        if self.lex.first_char() != 0 {
            return Err(Err::Syntax);
        }
        let Some((line, level)) = self.vars.label(&name) else {
            return Err(Err::LabelReq);
        };
        if level != self.level() {
            return Err(Err::CantCall);
        }
        if self.stack.len() >= MAX_SP {
            return Err(Err::StackOver);
        }
        let back = self.buffers.last().map(|b| b.line).unwrap_or(0);
        self.stack.push(Ctl::Call {
            level: self.level(),
            line: back,
        });
        self.jump_to(line);
        Ok(())
    }

    /// `TTLReturn` → `ReturnFromSub`。
    fn cmd_return(&mut self) -> Result<()> {
        self.end_of_args()?;
        let Some(&Ctl::Call { level, line }) = self.stack.last() else {
            return Err(Err::InvalidCtl);
        };
        self.stack.pop();
        self.unwind_to_level(level);
        self.jump_to(line);
        Ok(())
    }

    /// `TTLEnd`／`TTLExit`。
    fn cmd_end(&mut self) -> Result<()> {
        self.end_of_args()?;
        self.finished = true;
        Ok(())
    }

    /// `TTLInclude` → `BuffInclude`：把另一個檔疊在上面，讀完自動退回來。
    fn cmd_include(&mut self) -> Result<()> {
        let path = self.str_val()?;
        self.end_of_args()?;
        let name = String::from_utf8_lossy(&path).into_owned();
        if self.buffers.len() >= MAX_NEST_LEVEL {
            return Err(Err::CantOpen);
        }
        let src = match (self.loader)(&name) {
            Ok(s) => s,
            Err(_) => return Err(Err::CantOpen),
        };
        let level = self.buffers.len();
        self.buffers.push(Buffer { src, line: 0 });
        // 標籤註冊失敗（重複定義）要把那一層收掉再回報
        let lines = self.buffers[level].src.lines.clone();
        for (i, line) in lines.iter().enumerate() {
            let mut lex = Lexer::new(line);
            if lex.first_char() != b':' {
                continue;
            }
            let Some(lbl) = lex.label_name() else { continue };
            if lex.first_char() != 0 {
                self.buffers.pop();
                return Err(Err::Syntax);
            }
            if self.vars.find(&lbl).is_some() {
                self.buffers.pop();
                return Err(Err::LabelAlreadyDef);
            }
            self.vars.new_label(&lbl, i + 1, level);
        }
        Ok(())
    }

    /// `TTLIfDefined`：把變數型別放進 `result`（0＝沒有、1＝整數、3＝字串、5／6＝陣列）。
    fn cmd_if_defined(&mut self) -> Result<()> {
        // 原碼的 GetVarType：保留字與不存在的名字都算 TypUnknown，而且**不回報錯誤**
        let p = self.lex.ptr();
        let t = match self.lex.identifier() {
            None => VarType::Unknown,
            Some(name) => {
                if super::words::check_reserved_word(&name).is_some() {
                    VarType::Unknown
                } else {
                    match self.vars.var_type(&name) {
                        VarType::IntArray => {
                            match Eval::new(&mut self.lex, &self.vars).index()? {
                                Some(i) => {
                                    if self.vars.int_at(&name, i).is_ok() {
                                        VarType::Integer
                                    } else {
                                        VarType::Unknown
                                    }
                                }
                                None => VarType::IntArray,
                            }
                        }
                        VarType::StrArray => {
                            match Eval::new(&mut self.lex, &self.vars).index()? {
                                Some(i) => {
                                    if self.vars.str_at(&name, i).is_ok() {
                                        VarType::String
                                    } else {
                                        VarType::Unknown
                                    }
                                }
                                None => VarType::StrArray,
                            }
                        }
                        other => other,
                    }
                }
            }
        };
        let _ = p;
        self.vars.set_result(t as i32);
        Ok(())
    }

    /// 賦值：`名稱 [ 索引 ] = 字串常值 | 運算式`（`ExecCmnd` 尾端那一段）。
    fn assignment(&mut self) -> Result<()> {
        self.lex.mark_token_start();
        let Some(name) = self.lex.identifier() else {
            return Err(Err::Syntax);
        };
        let index = Eval::new(&mut self.lex, &self.vars).index()?;
        if self.lex.first_char() != b'=' {
            return Err(Err::Syntax);
        }
        // 先試字串常值，不是的話當運算式（同原碼的順序）
        let val = match self.lex.string()? {
            Some(s) => Val::Str(s),
            None => match Eval::new(&mut self.lex, &self.vars).expression()? {
                Some(v) => v,
                None => return Err(Err::Syntax),
            },
        };

        let existing = self.vars.var_type(&name);
        match existing {
            VarType::Unknown => {
                if index.is_some() {
                    return Err(Err::Syntax); // 沒宣告的陣列
                }
                match &val {
                    Val::Int(v) => self.vars.new_int(&name, *v),
                    Val::Str(s) => self.vars.new_str(&name, s),
                }
            }
            VarType::Integer if index.is_none() => match &val {
                Val::Int(v) => self.vars.set_int(&name, *v),
                Val::Str(_) => return Err(Err::TypeMismatch),
            },
            VarType::String if index.is_none() => match &val {
                Val::Str(s) => self.vars.set_str(&name, s),
                Val::Int(_) => return Err(Err::TypeMismatch),
            },
            VarType::IntArray => match (index, &val) {
                (Some(i), Val::Int(v)) => self.vars.set_int_at(&name, i, *v)?,
                (Some(_), Val::Str(_)) => return Err(Err::TypeMismatch),
                (None, _) => return Err(Err::Syntax),
            },
            VarType::StrArray => match (index, &val) {
                (Some(i), Val::Str(s)) => self.vars.set_str_at(&name, i, s)?,
                (Some(_), Val::Int(_)) => return Err(Err::TypeMismatch),
                (None, _) => return Err(Err::Syntax),
            },
            _ => return Err(Err::TypeMismatch),
        }
        self.end_of_args()
    }
}

/// 目標變數（可能是陣列的一格）。
pub(super) struct VarRef {
    pub name: String,
    pub index: Option<i32>,
}

fn ctl_level(c: &Ctl) -> usize {
    match c {
        Ctl::Call { level, .. } | Ctl::For { level, .. } | Ctl::While { level, .. } => *level,
    }
}

/// 方便測試：跑完一段巨集，回傳最後的變數表。
pub fn run_text(src: &str) -> std::result::Result<Vars, TtlError> {
    let mut it = Interp::from_text("test.ttl", src)?;
    it.run(100_000)?;
    Ok(std::mem::take(&mut it.vars))
}

/// 方便測試：跑一段巨集並把 `include` 用記憶體裡的檔案表提供。
pub fn run_text_with_includes(
    src: &str,
    files: HashMap<String, String>,
) -> std::result::Result<Vars, TtlError> {
    let mut it = Interp::new(
        Source::new("main.ttl", src.as_bytes()),
        Box::new(move |name| match files.get(name) {
            Some(text) => Ok(Source::new(name, text.as_bytes())),
            None => Err(format!("沒有這個檔：{name}")),
        }),
    )?;
    it.run(100_000)?;
    Ok(std::mem::take(&mut it.vars))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> Vars {
        run_text(src).expect("巨集應該跑得完")
    }

    fn err_of(src: &str) -> Err {
        run_text(src).expect_err("應該要出錯").err
    }

    #[test]
    fn assignment_and_types() {
        let v = run("a = 1\nb = a + 2\ns = 'hi'\nt = s");
        assert_eq!(v.int_of("a"), Some(1));
        assert_eq!(v.int_of("b"), Some(3));
        assert_eq!(v.str_of("s").unwrap(), b"hi");
        assert_eq!(v.str_of("t").unwrap(), b"hi");
    }

    /// 型別不能換（整數變數不能塞字串）。
    #[test]
    fn type_cannot_change() {
        assert_eq!(err_of("a = 1\na = 'x'"), Err::TypeMismatch);
        assert_eq!(err_of("s = 'x'\ns = 1"), Err::TypeMismatch);
    }

    /// 註解、空行、標籤行都不算指令。
    #[test]
    fn comments_and_labels_are_skipped() {
        let v = run("; 註解\n\n:top\na = 1 ; 行尾註解\n/* 區塊 */ b = 2");
        assert_eq!(v.int_of("a"), Some(1));
        assert_eq!(v.int_of("b"), Some(2));
    }

    /// 區塊式 if／else／elseif／endif。
    #[test]
    fn if_block() {
        let v = run("a = 0\nif 1 then\n a = 1\nelse\n a = 2\nendif");
        assert_eq!(v.int_of("a"), Some(1));
        let v = run("a = 0\nif 0 then\n a = 1\nelse\n a = 2\nendif");
        assert_eq!(v.int_of("a"), Some(2));
        let v = run("a = 0\nif 0 then\n a = 1\nelseif 1 then\n a = 2\nelse\n a = 3\nendif");
        assert_eq!(v.int_of("a"), Some(2));
        let v = run("a = 0\nif 0 then\n a = 1\nelseif 0 then\n a = 2\nelse\n a = 3\nendif");
        assert_eq!(v.int_of("a"), Some(3));
    }

    /// 巢狀 if：跳過的時候也要數層數，不然會配錯 endif。
    #[test]
    fn nested_if() {
        let v = run(
            "a = 0\nif 0 then\n if 1 then\n  a = 1\n endif\n a = 2\nelse\n a = 3\nendif\nb = 9",
        );
        assert_eq!(v.int_of("a"), Some(3));
        assert_eq!(v.int_of("b"), Some(9), "endif 之後要繼續跑");
    }

    /// 單行式 if。
    #[test]
    fn single_line_if() {
        let v = run("a = 0\nif 1 a = 5");
        assert_eq!(v.int_of("a"), Some(5));
        let v = run("a = 0\nif 0 a = 5");
        assert_eq!(v.int_of("a"), Some(0));
        // 單行式後面接的也可以是流程指令
        let v = run("a = 0\nfor i 1 3\n if i = 2 continue\n a = a + i\nnext");
        assert_eq!(v.int_of("a"), Some(4), "1+3（i=2 被 continue 跳掉）");
    }

    /// if 配對錯誤。
    #[test]
    fn unbalanced_if() {
        assert_eq!(err_of("endif"), Err::InvalidCtl);
        assert_eq!(err_of("else"), Err::InvalidCtl);
    }

    #[test]
    fn for_loop() {
        let v = run("s = 0\nfor i 1 5\n s = s + i\nnext");
        assert_eq!(v.int_of("s"), Some(15));
        assert_eq!(v.int_of("i"), Some(5), "迴圈變數留在最後的值");
    }

    /// 起＝迄只跑一圈；起 > 迄是**遞減**（原碼用 i>ValEnd 判斷方向）。
    #[test]
    fn for_loop_directions() {
        let v = run("n = 0\nfor i 3 3\n n = n + 1\nnext");
        assert_eq!(v.int_of("n"), Some(1));
        let v = run("s = 0\nfor i 3 1\n s = s * 10 + i\nnext");
        assert_eq!(v.int_of("s"), Some(321), "3,2,1");
    }

    #[test]
    fn while_loop() {
        let v = run("i = 0\nwhile i < 3\n i = i + 1\nendwhile");
        assert_eq!(v.int_of("i"), Some(3));
        // 一次都不跑
        let v = run("i = 9\nwhile 0\n i = 1\nendwhile");
        assert_eq!(v.int_of("i"), Some(9));
    }

    /// `until … enduntil`：條件成立就結束（和 while 相反）。
    #[test]
    fn until_loop() {
        let v = run("i = 0\nuntil i >= 3\n i = i + 1\nenduntil");
        assert_eq!(v.int_of("i"), Some(3));
    }

    /// `do … loop`／`do while`／`loop until`。
    #[test]
    fn do_loop() {
        let v = run("i = 0\ndo\n i = i + 1\nloop while i < 3");
        assert_eq!(v.int_of("i"), Some(3));
        let v = run("i = 0\ndo while i < 2\n i = i + 1\nloop");
        assert_eq!(v.int_of("i"), Some(2));
        let v = run("i = 0\ndo\n i = i + 1\nloop until i = 4");
        assert_eq!(v.int_of("i"), Some(4));
    }

    /// break 與 continue（含巢狀）。
    #[test]
    fn break_and_continue() {
        let v = run("s = 0\nfor i 1 10\n if i > 3 then\n  break\n endif\n s = s + i\nnext");
        assert_eq!(v.int_of("s"), Some(6), "1+2+3");
        let v = run("s = 0\nfor i 1 5\n if i = 3 then\n  continue\n endif\n s = s + i\nnext");
        assert_eq!(v.int_of("s"), Some(12), "1+2+4+5");
        // 巢狀：break 只跳出內層
        let v = run(
            "s = 0\nfor i 1 2\n for j 1 5\n  if j > 2 then\n   break\n  endif\n  s = s + 1\n next\nnext",
        );
        assert_eq!(v.int_of("s"), Some(4), "外層 2 圈 × 內層 2 圈");
        // 迴圈外 break
        assert_eq!(err_of("break"), Err::InvalidCtl);
    }

    /// goto 往前、往後都要能跳。
    #[test]
    fn goto_both_directions() {
        let v = run("a = 0\ngoto skip\na = 1\n:skip\nb = 2");
        assert_eq!(v.int_of("a"), Some(0));
        assert_eq!(v.int_of("b"), Some(2));
        let v = run("i = 0\n:top\ni = i + 1\nif i < 3 goto top\nd = i");
        assert_eq!(v.int_of("d"), Some(3));
    }

    /// call／return。
    #[test]
    fn call_and_return() {
        let v = run("a = 0\ncall sub\nb = 1\nend\n:sub\na = 7\nreturn");
        assert_eq!(v.int_of("a"), Some(7));
        assert_eq!(v.int_of("b"), Some(1), "return 之後要回到 call 的下一行");
        assert_eq!(err_of("return"), Err::InvalidCtl);
        assert_eq!(err_of("call nosuch"), Err::LabelReq);
    }

    /// `end`／`exit` 立刻結束。
    #[test]
    fn end_stops_execution() {
        let v = run("a = 1\nend\na = 2");
        assert_eq!(v.int_of("a"), Some(1));
        let v = run("a = 1\nexit\na = 2");
        assert_eq!(v.int_of("a"), Some(1));
    }

    /// 標籤重複定義。
    #[test]
    fn duplicate_label() {
        assert_eq!(err_of(":a\n:a\n"), Err::LabelAlreadyDef);
    }

    /// 不認識的指令 → `Unknown command.`；還沒實作的保留字也是同一個錯。
    #[test]
    fn unknown_and_unimplemented() {
        // 沒有 `=` 的識別字＝語法錯誤（原碼的 assignment 路徑）
        assert_eq!(err_of("foobar 1"), Err::Syntax);
        // 保留字但還沒實作（正規表示式是 TASK-014、xmodem 那組不做）
        assert_eq!(err_of("waitregex 'x'"), Err::NotSupported);
        assert_eq!(err_of("xmodemrecv 'f' 1 0"), Err::NotSupported);
    }

    /// `ifdefined` 回報型別（0／1／3／5／6）。
    #[test]
    fn if_defined_reports_type() {
        let v = run("i = 1\nifdefined i\nr1 = result\nifdefined nosuch\nr2 = result\ns = 'x'\nifdefined s\nr3 = result");
        assert_eq!(v.int_of("r1"), Some(1), "整數");
        assert_eq!(v.int_of("r2"), Some(0), "沒有這個變數");
        assert_eq!(v.int_of("r3"), Some(3), "字串");
    }

    /// include：另一個檔的變數與標籤都看得到，讀完自動回來。
    #[test]
    fn include_files() {
        let mut files = HashMap::new();
        files.insert("lib.ttl".to_string(), "x = 42\n".to_string());
        let v = run_text_with_includes("include 'lib.ttl'\ny = x + 1", files).unwrap();
        assert_eq!(v.int_of("x"), Some(42));
        assert_eq!(v.int_of("y"), Some(43));
    }

    /// include 找不到檔 → `Can't open file.`
    #[test]
    fn include_missing_file() {
        let e = run_text_with_includes("include 'nope.ttl'", HashMap::new()).unwrap_err();
        assert_eq!(e.err, Err::CantOpen);
    }

    /// 錯誤要帶行號與檔名。
    #[test]
    fn errors_carry_position() {
        let e = run_text("a = 1\nb = 1/0\n").unwrap_err();
        assert_eq!(e.err, Err::DivByZero);
        assert_eq!(e.line_no, 2);
        assert_eq!(e.file, "test.ttl");
        assert!(e.to_string().contains("Divide by zero."));
        assert!(e.to_string().contains("test.ttl:2"));
    }

    /// 堆疊上限（`MAXSP` 10）：11 層 call 要回 `Stack overflow.`
    #[test]
    fn stack_overflow() {
        let mut src = String::new();
        for i in 0..12 {
            src.push_str(&format!("call l{i}\nend\n:l{i}\n"));
        }
        src.push_str("return\n");
        assert_eq!(run_text(&src).unwrap_err().err, Err::StackOver);
    }

    /// 一步＝一行：第二批的 `wait`／`pause` 要靠這個掛起。
    #[test]
    fn step_runs_one_line_at_a_time() {
        let mut it = Interp::from_text("t.ttl", "a = 1\nb = 2\nc = 3").unwrap();
        assert_eq!(it.step().unwrap(), Step::Ran);
        assert_eq!(it.vars.int_of("a"), Some(1));
        assert_eq!(it.vars.int_of("b"), None, "第二行還沒跑");
        assert_eq!(it.step().unwrap(), Step::Ran);
        assert_eq!(it.step().unwrap(), Step::Ran);
        assert_eq!(it.step().unwrap(), Step::Finished);
        assert!(it.finished());
    }
}
