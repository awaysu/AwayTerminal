//! TTL 的詞法分析——逐段照 `ttpmacro/ttmparse.cpp`。
//!
//! | 原碼 | 這裡 |
//! |---|---|
//! | `GetFirstChar` | [`Lexer::first_char`] |
//! | `CheckParameterGiven` | [`Lexer::parameter_given`] |
//! | `GetIdentifier` | [`Lexer::identifier`] |
//! | `GetReservedWord` | [`Lexer::reserved_word`] |
//! | `GetOperator` | [`Lexer::operator`] |
//! | `GetLabelName` | [`Lexer::label_name`] |
//! | `GetString` / `GetQuotedStr` / `GetCharByCode` | [`Lexer::string`] |
//! | `GetNumber` | [`Lexer::number`] |
//! | `LinePtr` / `LineLen` / `LineParsePtr` | [`Lexer::ptr`] / `line.len()` / [`Lexer::parse_ptr`] |
//!
//! **一行就是一串位元組**（原碼的 `LineBuff` 是 `char[1024]`）。字串值同樣是位元組
//! （`TStrVal` 是 `char[512]`），所以 `#$41`、`strcmp` 的位元組序、以及「中文被當成兩個
//! 位元組」這些行為都和原碼一致。要顯示時才轉成 UTF-8 字串。

use super::error::{Err, Result};
use super::words::{check_reserved_word, Word};

/// `ttmdef.h` 的三個上限，照抄。
pub const MAX_NAME_LEN: usize = 32;
pub const MAX_STR_LEN: usize = 512;
pub const MAX_LINE_LEN: usize = 1024;

/// 一行的詞法分析器。狀態只有「讀到哪裡」，和原碼的 `LinePtr` 一樣。
pub struct Lexer {
    line: Vec<u8>,
    ptr: usize,
    /// 這個 token 開始的位置（原碼 `LineParsePtr`；錯誤對話框要把那一段標起來）。
    parse_ptr: usize,
    /// `/* */` 註解跨行時的狀態（原碼的 `commenting`）。
    commenting: bool,
}

impl Lexer {
    pub fn new(line: &[u8]) -> Self {
        // 原碼：超過 MaxLineLen-1 的部分**丟掉**（不是切成下一行），見 GetRawLine 的註解
        let mut buf = line.to_vec();
        buf.truncate(MAX_LINE_LEN - 1);
        Self {
            line: buf,
            ptr: 0,
            parse_ptr: 0,
            commenting: false,
        }
    }

    /// 換一行（沿用跨行的註解狀態，同原碼的全域 `commenting`）。
    pub fn reset(&mut self, line: &[u8]) {
        let mut buf = line.to_vec();
        buf.truncate(MAX_LINE_LEN - 1);
        self.line = buf;
        self.ptr = 0;
        self.parse_ptr = 0;
    }

    pub fn line(&self) -> &[u8] {
        &self.line
    }

    pub fn ptr(&self) -> usize {
        self.ptr
    }

    pub fn set_ptr(&mut self, p: usize) {
        self.ptr = p.min(self.line.len());
    }

    pub fn parse_ptr(&self) -> usize {
        self.parse_ptr
    }

    /// 原碼的 `UpdateLineParsePtr`：記下「這個 token 從哪裡開始」。
    pub fn mark_token_start(&mut self) {
        self.parse_ptr = self.ptr;
    }

    /// `/* */` 有沒有正常收尾（原碼 `IsCommentClosed`）。
    /// 呼叫之後狀態會清掉——同原碼（「讓註解前的指令還能執行」）。
    pub fn comment_closed(&mut self) -> bool {
        let ret = !self.commenting;
        self.commenting = false;
        ret
    }

    fn peek(&self) -> u8 {
        if self.ptr < self.line.len() {
            self.line[self.ptr]
        } else {
            0
        }
    }

    fn peek_at(&self, off: usize) -> u8 {
        let i = self.ptr + off;
        if i < self.line.len() {
            self.line[i]
        } else {
            0
        }
    }

    fn skip_blanks(&mut self) {
        while self.ptr < self.line.len() {
            let b = self.line[self.ptr];
            if b == b' ' || b == b'\t' {
                self.ptr += 1;
            } else {
                break;
            }
        }
    }

    /// `GetFirstChar`：跳過空白與註解，回傳下一個「有意義」的字元並吃掉它。
    /// 回 0 ＝這一行沒有東西了（`;` 之後也算沒有東西）。
    pub fn first_char(&mut self) -> u8 {
        if self.ptr >= self.line.len() {
            return 0;
        }
        self.skip_blanks();

        // 跨行的 /* ... */ 還沒收尾：找到 */ 為止
        if self.commenting {
            while self.ptr < self.line.len() {
                if self.peek() == b'*' && self.peek_at(1) == b'/' {
                    self.commenting = false;
                    self.ptr += 2;
                    break;
                }
                self.ptr += 1;
            }
            if self.commenting {
                return 0; // 整行都在註解裡
            }
            self.skip_blanks();
        }

        // 這一行開始的 /* ... */（可以有好幾段，同原碼的 do/while）
        loop {
            if self.peek() == b'/' && self.peek_at(1) == b'*' {
                let mut closed = false;
                self.ptr += 2;
                while self.ptr < self.line.len() {
                    if self.peek() == b'*' && self.peek_at(1) == b'/' {
                        self.ptr += 2;
                        closed = true;
                        break;
                    }
                    self.ptr += 1;
                }
                self.skip_blanks();
                if !closed {
                    // 這一行沒收尾 → 記著，下一行繼續找
                    self.commenting = true;
                }
                if self.peek() != b'/' {
                    break;
                }
            } else {
                break;
            }
        }

        let b = self.peek();
        // 原碼：`b > ' '` 且不是 `;`（分號到行尾都是註解）
        if b > b' ' && b != b';' {
            self.ptr += 1;
            return b;
        }
        0
    }

    /// `CheckParameterGiven`：後面還有參數嗎（不吃掉）。
    pub fn parameter_given(&mut self) -> bool {
        let p = self.ptr;
        let got = self.first_char() != 0;
        self.ptr = p;
        got
    }

    /// `GetIdentifier`：`[A-Za-z_][A-Za-z0-9_]*`，超過 31 個字的部分丟掉。
    pub fn identifier(&mut self) -> Option<String> {
        let b = self.first_char();
        if b == 0 {
            return None;
        }
        if !is_csymf(b) {
            self.ptr -= 1;
            return None;
        }
        let mut name = Vec::with_capacity(MAX_NAME_LEN);
        name.push(b);
        while self.ptr < self.line.len() && is_csym(self.peek()) {
            if name.len() < MAX_NAME_LEN - 1 {
                name.push(self.peek());
            }
            self.ptr += 1;
        }
        Some(String::from_utf8_lossy(&name).into_owned())
    }

    /// `GetReservedWord`：是保留字就吃掉並回傳，不是就把位置還原。
    pub fn reserved_word(&mut self) -> Option<Word> {
        let p = self.ptr;
        let name = self.identifier()?;
        match check_reserved_word(&name) {
            Some(w) => Some(w),
            None => {
                self.ptr = p;
                None
            }
        }
    }

    /// `GetLabelName`：標籤名稱。**第一個字元不檢查**（原碼就是這樣：`:1abc` 也可以）。
    pub fn label_name(&mut self) -> Option<String> {
        let b = self.first_char();
        if b == 0 {
            return None;
        }
        let mut name = vec![b];
        while self.ptr < self.line.len() && is_csym(self.peek()) {
            if name.len() < MAX_NAME_LEN - 1 {
                name.push(self.peek());
            }
            self.ptr += 1;
        }
        Some(String::from_utf8_lossy(&name).into_owned())
    }

    /// `GetOperator`：符號運算子，或字詞形式（`and`／`or`／`xor`／`not`）。
    pub fn operator(&mut self) -> Option<Op> {
        let p = self.ptr;
        let b = self.first_char();
        let op = match b {
            0 => return None,
            b'*' => Op::Mul,
            b'+' => Op::Plus,
            b'-' => Op::Minus,
            b'/' => Op::Div,
            b'%' => Op::Mod,
            b'=' => {
                // `=` 與 `==` 都是相等比較
                if self.peek() == b'=' {
                    self.ptr += 1;
                }
                Op::Eq
            }
            b'<' => match self.peek() {
                b'=' => {
                    self.ptr += 1;
                    Op::Le
                }
                b'>' => {
                    self.ptr += 1;
                    Op::Ne
                }
                b'<' => {
                    self.ptr += 1;
                    Op::Shl
                }
                _ => Op::Lt,
            },
            b'>' => match self.peek() {
                b'=' => {
                    self.ptr += 1;
                    Op::Ge
                }
                b'>' => {
                    self.ptr += 1;
                    if self.peek() == b'>' {
                        self.ptr += 1;
                        Op::Lshr // `>>>` 邏輯右移
                    } else {
                        Op::Shr
                    }
                }
                _ => Op::Gt,
            },
            b'&' => {
                if self.peek() == b'&' {
                    self.ptr += 1;
                    Op::LAnd
                } else {
                    Op::BAnd
                }
            }
            b'|' => {
                if self.peek() == b'|' {
                    self.ptr += 1;
                    Op::LOr
                } else {
                    Op::BOr
                }
            }
            b'^' => Op::BXor,
            b'~' => Op::BNot,
            b'!' => {
                if self.peek() == b'=' {
                    self.ptr += 1;
                    Op::Ne
                } else {
                    Op::LNot
                }
            }
            _ => {
                // 字詞形式：退一格讓 reserved_word 重讀
                self.ptr -= 1;
                match self.reserved_word() {
                    Some(w) if w.is_operator() => match w {
                        Word::BAnd => Op::BAnd,
                        Word::BOr => Op::BOr,
                        Word::BXor => Op::BXor,
                        Word::BNot => Op::BNot,
                        _ => {
                            self.ptr = p;
                            return None;
                        }
                    },
                    _ => {
                        self.ptr = p;
                        return None;
                    }
                }
            }
        };
        Some(op)
    }

    /// `GetNumber`：十進位，或 `$` 開頭的十六進位。**沒有二進位、也沒有 `0x`**。
    ///
    /// 溢位照 C 的 `int`（環繞），所以用 `wrapping_*`。
    pub fn number(&mut self) -> Option<i32> {
        let b = self.first_char();
        if b == 0 {
            return None;
        }
        if b.is_ascii_digit() {
            let mut n = (b - b'0') as i32;
            while self.ptr < self.line.len() && self.peek().is_ascii_digit() {
                n = n.wrapping_mul(10).wrapping_add((self.peek() - b'0') as i32);
                self.ptr += 1;
            }
            Some(n)
        } else if b == b'$' {
            let mut n: i32 = 0;
            while self.ptr < self.line.len() && self.peek().is_ascii_hexdigit() {
                let d = hex_val(self.peek());
                n = n.wrapping_mul(16).wrapping_add(d as i32);
                self.ptr += 1;
            }
            // 原碼：`$` 後面沒有十六進位數字時就是 0（不是錯誤）
            Some(n)
        } else {
            self.ptr -= 1;
            None
        }
    }

    /// `GetString`：字串常值。可以是好幾段**直接相鄰**的片段接起來：
    ///
    /// - `"..."` / `'...'`：引號內的位元組（`>= ' '` 或 tab），**沒有反斜線轉義**
    /// - `#65` / `#$41`：字元碼（1～255，超出就是語法錯誤）
    ///
    /// 例：`'ab' #13 #10 "cd"` ＝ `ab\r\ncd`。不是字串開頭就回 `Ok(None)`。
    pub fn string(&mut self) -> Result<Option<Vec<u8>>> {
        let mut q = self.first_char();
        if q == 0 {
            return Ok(None);
        }
        self.ptr -= 1;
        if q != b'"' && q != b'\'' && q != b'#' {
            return Ok(None);
        }
        let mut out: Vec<u8> = Vec::new();
        while q == b'"' || q == b'\'' || q == b'#' {
            self.ptr += 1;
            match q {
                b'"' | b'\'' => self.quoted_str(q, &mut out)?,
                _ => self.char_by_code(&mut out)?,
            }
            q = self.peek();
        }
        Ok(Some(out))
    }

    /// `GetQuotedStr`。
    fn quoted_str(&mut self, q: u8, out: &mut Vec<u8>) -> Result<()> {
        let mut b = self.peek();
        while self.ptr < self.line.len() && (b >= b' ' || b == b'\t') && b != q {
            if out.len() < MAX_STR_LEN - 1 {
                out.push(b);
            }
            self.ptr += 1;
            b = self.peek();
        }
        if b == q {
            if self.ptr < self.line.len() {
                self.ptr += 1;
            }
            Ok(())
        } else {
            Err(Err::Syntax) // 引號沒有收尾
        }
    }

    /// `GetCharByCode`：`#` 之後的十進位或 `$` 十六進位，值必須是 1～255。
    fn char_by_code(&mut self, out: &mut Vec<u8>) -> Result<()> {
        let mut b = self.peek();
        if !b.is_ascii_digit() && b != b'$' {
            return Err(Err::Syntax);
        }
        // 原碼用 WORD（16-bit）累加，所以超大的數字會環繞之後才檢查範圍
        let mut n: u16 = 0;
        if b != b'$' {
            while self.ptr < self.line.len() && b.is_ascii_digit() {
                n = n.wrapping_mul(10).wrapping_add((b - b'0') as u16);
                self.ptr += 1;
                b = self.peek();
            }
        } else {
            self.ptr += 1;
            b = self.peek();
            while self.ptr < self.line.len() && b.is_ascii_hexdigit() {
                n = n.wrapping_mul(16).wrapping_add(hex_val(b) as u16);
                self.ptr += 1;
                b = self.peek();
            }
        }
        if n == 0 || n > 255 {
            return Err(Err::Syntax);
        }
        if out.len() < MAX_STR_LEN - 1 {
            out.push(n as u8);
        }
        Ok(())
    }
}

/// 運算子。編號對應 `ttmparse.h` 的 `Rsv*`（註解裡標出來，方便對照）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// RsvBNot `~` / `not`
    BNot,
    /// RsvBAnd `&` / `and`
    BAnd,
    /// RsvBOr `|` / `or`
    BOr,
    /// RsvBXor `^` / `xor`
    BXor,
    /// RsvMul `*`
    Mul,
    /// RsvPlus `+`
    Plus,
    /// RsvMinus `-`
    Minus,
    /// RsvDiv `/`
    Div,
    /// RsvMod `%`
    Mod,
    /// RsvLT `<`
    Lt,
    /// RsvEQ `=` / `==`
    Eq,
    /// RsvGT `>`
    Gt,
    /// RsvLE `<=`
    Le,
    /// RsvNE `<>` / `!=`
    Ne,
    /// RsvGE `>=`
    Ge,
    /// RsvLNot `!`
    LNot,
    /// RsvLAnd `&&`
    LAnd,
    /// RsvLOr `||`
    LOr,
    /// RsvARShift `>>`（算術右移）
    Shr,
    /// RsvALShift `<<`
    Shl,
    /// RsvLRShift `>>>`（邏輯右移）
    Lshr,
}

fn is_csymf(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic()
}

fn is_csym(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

fn hex_val(b: u8) -> u8 {
    if b.is_ascii_alphabetic() {
        (b | 0x20) - b'a' + 10
    } else {
        b - b'0'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(s: &str) -> Lexer {
        Lexer::new(s.as_bytes())
    }

    /// 空白／tab 跳過；`;` 之後整段是註解。
    #[test]
    fn first_char_skips_blanks_and_comments() {
        let mut l = lex("  \t a ; bcd");
        assert_eq!(l.first_char(), b'a');
        assert_eq!(l.first_char(), 0, "分號之後就是行尾");
    }

    /// `/* */` 註解（原碼有開 SUPPORT_C_STYLE_COMMENT），同一行可以有好幾段。
    #[test]
    fn c_style_comments() {
        let mut l = lex("a /* x */ /* y */ b");
        assert_eq!(l.first_char(), b'a');
        assert_eq!(l.first_char(), b'b');

        // 跨行：第一行沒收尾 → 整行後半被吃掉，下一行找到 */ 才恢復
        let mut l = lex("a /* x");
        assert_eq!(l.first_char(), b'a');
        assert_eq!(l.first_char(), 0);
        assert!(!l.comment_closed(), "沒收尾");
    }

    #[test]
    fn identifiers_and_reserved_words() {
        let mut l = lex("foo_1 if");
        assert_eq!(l.identifier().as_deref(), Some("foo_1"));
        assert_eq!(l.reserved_word(), Some(Word::If));

        // 數字開頭不是識別字，而且位置要還原
        let mut l = lex("1abc");
        assert!(l.identifier().is_none());
        assert_eq!(l.number(), Some(1));

        // 超過 31 個字的部分丟掉（MaxNameLen）
        let long = "a".repeat(40);
        let mut l = lex(&long);
        assert_eq!(l.identifier().unwrap().len(), MAX_NAME_LEN - 1);
    }

    /// 不是保留字的話位置要還原（原碼 `GetReservedWord` 的 `LinePtr = P`）。
    #[test]
    fn reserved_word_restores_position() {
        let mut l = lex("myvar = 1");
        assert!(l.reserved_word().is_none());
        assert_eq!(l.identifier().as_deref(), Some("myvar"));
    }

    #[test]
    fn numbers_decimal_and_hex() {
        assert_eq!(lex("123").number(), Some(123));
        assert_eq!(lex("$ff").number(), Some(255));
        assert_eq!(lex("$FF").number(), Some(255));
        assert_eq!(lex("$").number(), Some(0), "原碼：$ 後面沒數字＝0");
        // 沒有 0x 這種寫法：`0x10` 會被讀成 0，然後 x10 留在後面
        let mut l = lex("0x10");
        assert_eq!(l.number(), Some(0));
        assert_eq!(l.identifier().as_deref(), Some("x10"));
    }

    /// 運算子的每一種拼法。
    #[test]
    fn operators() {
        let cases: &[(&str, Op)] = &[
            ("*", Op::Mul),
            ("+", Op::Plus),
            ("-", Op::Minus),
            ("/", Op::Div),
            ("%", Op::Mod),
            ("=", Op::Eq),
            ("==", Op::Eq),
            ("<", Op::Lt),
            ("<=", Op::Le),
            ("<>", Op::Ne),
            ("!=", Op::Ne),
            ("<<", Op::Shl),
            (">", Op::Gt),
            (">=", Op::Ge),
            (">>", Op::Shr),
            (">>>", Op::Lshr),
            ("&", Op::BAnd),
            ("&&", Op::LAnd),
            ("|", Op::BOr),
            ("||", Op::LOr),
            ("^", Op::BXor),
            ("~", Op::BNot),
            ("!", Op::LNot),
            ("and", Op::BAnd),
            ("or", Op::BOr),
            ("xor", Op::BXor),
            ("not", Op::BNot),
        ];
        for (s, want) in cases {
            assert_eq!(lex(s).operator(), Some(*want), "{s}");
        }
        assert!(lex("foo").operator().is_none(), "不是運算子要還原位置");
    }

    /// 字串：引號內原樣、**沒有反斜線轉義**、相鄰片段接起來。
    #[test]
    fn strings_concatenate_pieces() {
        assert_eq!(lex("'abc'").string().unwrap().unwrap(), b"abc");
        assert_eq!(lex("\"a'b\"").string().unwrap().unwrap(), b"a'b");
        assert_eq!(
            lex(r"'a\nb'").string().unwrap().unwrap(),
            br"a\nb".to_vec(),
            "TTL 的字串沒有 \\n 轉義，反斜線就是反斜線"
        );
        assert_eq!(lex("#65").string().unwrap().unwrap(), b"A");
        assert_eq!(lex("#$41").string().unwrap().unwrap(), b"A");
        assert_eq!(
            lex("'ab'#13#10\"cd\"").string().unwrap().unwrap(),
            b"ab\r\ncd".to_vec()
        );
        // 不是字串開頭
        assert!(lex("abc").string().unwrap().is_none());
    }

    /// 字元碼必須是 1～255；引號沒收尾是語法錯誤。
    #[test]
    fn string_errors() {
        assert_eq!(lex("#0").string(), Err(Err::Syntax));
        assert_eq!(lex("#256").string(), Err(Err::Syntax));
        assert_eq!(lex("'abc").string(), Err(Err::Syntax));
        assert_eq!(lex("#x").string(), Err(Err::Syntax));
    }

    /// 字串長度上限 511（MaxStrLen-1），多的丟掉。
    #[test]
    fn strings_are_truncated_at_max_len() {
        let src = format!("'{}'", "x".repeat(600));
        let got = Lexer::new(src.as_bytes()).string().unwrap().unwrap();
        assert_eq!(got.len(), MAX_STR_LEN - 1);
    }

    /// 中文在字串裡就是位元組（同原碼；顯示時才解成 UTF-8）。
    #[test]
    fn chinese_is_bytes() {
        let got = lex("'中文'").string().unwrap().unwrap();
        assert_eq!(got, "中文".as_bytes());
    }
}
