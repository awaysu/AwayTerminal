//! TTL 的正規表示式：`strmatch`／`strreplace`／`waitregex`／`regexoption`。
//!
//! 引擎是 **`fancy-regex`**（決定過程與完整差異表在 `docs/TTL-REGEX.md`）。
//! 對照的原碼：`ttl.cpp` 的 `TTLStrMatch`／`TTLStrReplace`／`TTLWaitRegex`／`TTLRegexOption`
//! 與 `ttmdde.c` 的 `FindRegexStringOne`／`FindRegexString`。
//!
//! ## 三件照原碼的重點
//!
//! 1. **位置是 1 起算的位元組位移**（`FindRegexStringOne` 回 `r + 1`）。
//! 2. **比對前先把 `groupmatchstr1..9` 清空**，然後 `matchstr` ＝整個命中、
//!    `groupmatchstr{i}` ＝第 i 組（原碼那段 `for (i = 1; i <= 9; i++) SetGroupMatchStr(i, "")`）。
//! 3. pattern 壞掉時：`strmatch` 回 `result=0`、`strreplace` 回 `result=-1`（原碼就是不一致的）。
//!
//! ## 一個刻意的補償
//!
//! Oniguruma 的 Ruby 語法裡 `^`／`$` 是**行**錨點；Rust 的預設是整段文字的錨點。
//! 所以我們**預設開 Rust 的 `m` 旗標**，`regexoption 'SINGLELINE'` 才關掉它
//! （那正是 Oniguruma `SINGLELINE` 的意思：`^`→`\A`、`$`→`\Z`）。

use fancy_regex::Regex;

use super::error::{Err, Result};
use super::exec::Interp;
use super::vars::VarType;
use super::words::Word;

/// `regexoption` 設定的狀態（原碼的 `RegexOpt`／`RegexEnc`／`RegexSyntax`）。
#[derive(Clone, Debug)]
pub struct RegexOptions {
    /// 忽略大小寫（Oniguruma `IGNORECASE` → Rust `i`）。
    pub ignore_case: bool,
    /// 忽略空白與 `#` 註解（`EXTEND` → Rust `x`）。
    pub extend: bool,
    /// `.` 也吃換行。⚠️ Oniguruma 把這個叫 **MULTILINE**（不是 Perl 的 `/m`），對應 Rust 的 `s`。
    pub dot_matches_newline: bool,
    /// `^`／`$` 是行錨點。**預設 true**（Ruby 語法的行為）；
    /// `SINGLELINE` 會設成 false（`^`→`\A`、`$`→`\Z`）。
    pub line_anchors: bool,
    /// 不要回空比對（`FIND_NOT_EMPTY`）。
    pub find_not_empty: bool,
}

impl Default for RegexOptions {
    fn default() -> Self {
        Self {
            ignore_case: false,
            extend: false,
            dot_matches_newline: false,
            // 見檔頭：Ruby 語法的 `^`／`$` 是行錨點
            line_anchors: true,
            find_not_empty: false,
        }
    }
}

impl RegexOptions {
    /// 把選項變成 Rust 的行內旗標前綴。
    fn flags(&self) -> String {
        let mut f = String::new();
        if self.ignore_case {
            f.push('i');
        }
        if self.extend {
            f.push('x');
        }
        if self.dot_matches_newline {
            f.push('s');
        }
        if self.line_anchors {
            f.push('m');
        }
        if f.is_empty() {
            String::new()
        } else {
            format!("(?{f})")
        }
    }

    /// 編譯一個 pattern（把旗標加在前面）。
    pub fn compile(&self, pattern: &str) -> std::result::Result<Regex, fancy_regex::Error> {
        Regex::new(&format!("{}{}", self.flags(), pattern))
    }
}

/// pattern 編譯或執行失敗（原碼 `FindRegexStringOne` 回 -1 的那個情形）。
///
/// 刻意不帶訊息：呼叫端只需要知道「壞了」，而 `strmatch` 與 `strreplace`
/// 對這件事的 `result` 不一樣（0 vs -1），訊息也沒有地方顯示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadPattern;

/// 一次比對的結果。
pub struct MatchResult {
    /// 命中的起點（**位元組位移，0 起算**；`result` 要 +1）。
    pub start: usize,
    /// 整個命中的字串（`matchstr`）。
    pub whole: String,
    /// 第 1～9 組（沒有的組是空字串）。
    pub groups: Vec<String>,
}

/// 找第一個命中（原碼 `FindRegexStringOne`）。
///
/// 回 `Ok(None)` ＝沒命中、`Err(())` ＝pattern 壞掉（呼叫端決定要回 0 還是 -1）。
pub fn find_one(
    opts: &RegexOptions,
    pattern: &str,
    target: &str,
) -> std::result::Result<Option<MatchResult>, BadPattern> {
    let re = opts.compile(pattern).map_err(|_| BadPattern)?;
    let mut from = 0usize;
    loop {
        let caps = match re.captures_from_pos(target, from) {
            Ok(Some(c)) => c,
            Ok(None) => return Ok(None),
            Err(_) => return Err(BadPattern), // 回溯爆掉／執行期錯誤
        };
        let m = match caps.get(0) {
            Some(m) => m,
            None => return Ok(None),
        };
        // `FIND_NOT_EMPTY`：命中空字串就往後再找
        if opts.find_not_empty && m.start() == m.end() {
            if m.end() >= target.len() {
                return Ok(None);
            }
            // 往下一個字元邊界移動（不可以切在多位元組字的中間）
            from = next_char_boundary(target, m.end());
            continue;
        }
        let mut groups = vec![String::new(); 9];
        for i in 1..=9 {
            if let Some(g) = caps.get(i) {
                groups[i - 1] = g.as_str().to_string();
            }
        }
        return Ok(Some(MatchResult {
            start: m.start(),
            whole: m.as_str().to_string(),
            groups,
        }));
    }
}

fn next_char_boundary(s: &str, mut i: usize) -> usize {
    i += 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

impl Interp {
    /// 正規表示式那四個指令的分派。
    pub(super) fn dispatch_regex(&mut self, w: Word) -> Result<()> {
        match w {
            Word::StrMatch => self.cmd_strmatch(),
            Word::StrReplace => self.cmd_strreplace(),
            Word::RegexOption => self.cmd_regexoption(),
            // `waitregex`／`waitregexln` 在 io.rs（和 `wait` 共用等待的機制）
            _ => Err(Err::NotSupported),
        }
    }

    /// 把比對結果寫進 `matchstr`／`groupmatchstr1..9`（原碼在比對成功時做的事）。
    pub(super) fn apply_match(&mut self, m: &MatchResult) {
        self.vars.set_str("matchstr", m.whole.as_bytes());
        for (i, g) in m.groups.iter().enumerate() {
            self.vars.set_group_match(i + 1, g.as_bytes());
        }
    }

    /// 比對前先把 9 個群組清空（原碼 `FindRegexStringOne` 開頭那個迴圈）。
    pub(super) fn clear_groups(&mut self) {
        for i in 1..=9 {
            self.vars.set_group_match(i, b"");
        }
    }

    /// `TTLStrMatch`：`strmatch <字串> <pattern>` → `result` ＝ 1 起算的位元組位移。
    fn cmd_strmatch(&mut self) -> Result<()> {
        let target = self.str_val()?;
        let pattern = self.str_val()?;
        self.end_of_args()?;

        let target = String::from_utf8_lossy(&target).into_owned();
        let pattern = String::from_utf8_lossy(&pattern).into_owned();
        let opts = self.regex_options().clone();
        match find_one(&opts, &pattern, &target) {
            Ok(Some(m)) => {
                self.clear_groups();
                let start = m.start;
                self.apply_match(&m);
                self.vars.set_result(start as i32 + 1);
            }
            // 沒命中與 pattern 壞掉都是 0（原碼：`ret > 0` 才算命中）
            Ok(None) | Err(BadPattern) => self.vars.set_result(0),
        }
        Ok(())
    }

    /// `TTLStrReplace`：`strreplace <字串變數> <從第幾個字> <pattern> <新字串>`。
    ///
    /// ⚠️ **新字串是原樣插入的**（原碼沒有 `\1`／`$1` 展開）。要用群組請先讀 `groupmatchstr*`。
    fn cmd_strreplace(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let pos = self.int_val()?;
        let pattern = self.str_val()?;
        let newstr = self.str_val()?;
        self.end_of_args()?;

        let cur = self.get_str_ref(&target)?;
        let src_len = cur.len() as i32;
        if pos > src_len || pos <= 0 {
            self.vars.set_result(0);
            return Ok(());
        }
        let from = (pos - 1) as usize;
        let text = String::from_utf8_lossy(&cur).into_owned();
        // 原碼是從 `pos` 之後的那一段裡找（`p = tmpstr + pos`）
        let (head, tail) = text.split_at(from.min(text.len()));
        let pattern = String::from_utf8_lossy(&pattern).into_owned();
        let opts = self.regex_options().clone();
        match find_one(&opts, &pattern, tail) {
            Ok(Some(m)) => {
                self.clear_groups();
                let start = m.start;
                let len = m.whole.len();
                self.apply_match(&m);
                let mut out = head.as_bytes().to_vec();
                out.extend_from_slice(&tail.as_bytes()[..start]);
                out.extend_from_slice(&newstr);
                out.extend_from_slice(&tail.as_bytes()[start + len..]);
                self.set_str_ref(&target, &out)?;
                self.vars.set_result(1);
            }
            Ok(None) => self.vars.set_result(0),
            // 原碼這裡和 strmatch 不一樣：pattern 壞掉是 -1
            Err(BadPattern) => self.vars.set_result(-1),
        }
        Ok(())
    }

    /// `TTLRegexOption`：一串關鍵字。
    ///
    /// 不支援的值**接受但印一行黃字**（舊巨集裡的一行不該讓整支停掉，但也不能安靜忽略）；
    /// **認不出來**的關鍵字照原碼回語法錯誤。對照表在 `docs/TTL-REGEX.md` 第 3 節。
    fn cmd_regexoption(&mut self) -> Result<()> {
        let mut opts = self.regex_options().clone();
        let mut unsupported: Vec<String> = Vec::new();
        while self.parameter_given() {
            let raw = self.str_val()?;
            let key = String::from_utf8_lossy(&raw).to_ascii_uppercase();
            let key = key.strip_prefix("OPTION_").unwrap_or(&key).to_string();
            match key.as_str() {
                "NONE" => opts = RegexOptions::default(),
                "IGNORECASE" => opts.ignore_case = true,
                "EXTEND" => opts.extend = true,
                // ⚠️ Oniguruma 的 MULTILINE ＝「`.` 吃換行」
                "MULTILINE" => opts.dot_matches_newline = true,
                // SINGLELINE ＝關掉行錨點（`^`→`\A`、`$`→`\Z`）
                "SINGLELINE" => opts.line_anchors = false,
                "NEGATE_SINGLELINE" => opts.line_anchors = true,
                "FIND_NOT_EMPTY" => opts.find_not_empty = true,
                // 接受但沒有效果的（見 docs/TTL-REGEX.md）
                "FIND_LONGEST" | "DONT_CAPTURE_GROUP" | "CAPTURE_GROUP" => {
                    unsupported.push(key)
                }
                other => {
                    let full = String::from_utf8_lossy(&raw).to_ascii_uppercase();
                    if full.starts_with("SYNTAX_")
                        || full.starts_with("ENCODING_")
                        || is_syntax_name(&full)
                        || is_encoding_name(&full)
                    {
                        // 語法／編碼：只有 Ruby 那一套與 UTF-8
                        if full == "ENCODING_UTF8"
                            || full == "UTF8"
                            || full == "ENCODING_ASCII"
                            || full == "ASCII"
                            || full == "SYNTAX_RUBY"
                            || full == "RUBY"
                            || full == "SYNTAX_PERL"
                            || full == "PERL"
                            || full == "SYNTAX_PERL_NG"
                            || full == "PERL_NG"
                            || full == "SYNTAX_DEFAULT"
                        {
                            // 就是我們用的，不必做事
                        } else {
                            unsupported.push(full);
                        }
                    } else {
                        let _ = other;
                        return Err(Err::Syntax); // 認不出來 → 照原碼
                    }
                }
            }
        }
        self.end_of_args()?;
        for k in &unsupported {
            let msg = format!(
                "\r\n\x1b[33m[巨集] regexoption {k} 這個版本沒有支援（見 docs/TTL-REGEX.md）\x1b[0m\r\n"
            );
            if let Some(h) = self.host() {
                h.echo(&msg);
            }
            println!("[AwayTerminal] 巨集：regexoption {k} 未支援");
        }
        self.set_regex_options(opts);
        Ok(())
    }
}

/// Oniguruma 的語法名稱（`regexoption` 可以不帶 `SYNTAX_` 前綴）。
fn is_syntax_name(s: &str) -> bool {
    matches!(
        s,
        "ASIS"
            | "POSIX_BASIC"
            | "POSIX_EXTENDED"
            | "EMACS"
            | "GREP"
            | "GNU_REGEX"
            | "JAVA"
            | "PERL"
            | "PERL_NG"
            | "RUBY"
    )
}

/// Oniguruma 的編碼名稱（同樣可以不帶前綴）。
fn is_encoding_name(s: &str) -> bool {
    s.starts_with("ISO_8859_")
        || s.starts_with("UTF16")
        || s.starts_with("UTF32")
        || s.starts_with("EUC_")
        || matches!(
            s,
            "ASCII" | "UTF8" | "SJIS" | "CP932" | "KOI8_R" | "CP1251" | "BIG5" | "GB18030"
        )
}

#[cfg(test)]
mod tests {
    use super::super::exec::run_text;
    use super::*;

    fn run(src: &str) -> super::super::vars::Vars {
        run_text(src).expect("巨集應該跑得完")
    }

    /// **`docs/TTL-REGEX.md` 第 2 節那張表**：換引擎或升版時這條會先叫。
    #[test]
    fn oniguruma_feature_matrix() {
        let opts = RegexOptions::default();
        // (語法, pattern, 樣本, 要不要命中)
        let supported: &[(&str, &str, &str)] = &[
            ("群組", r"(\d+)-(\d+)", "12-34"),
            ("懶惰量詞", r"a.*?b", "axxbxxb"),
            ("POSIX 類", r"[[:alpha:]]+", "abc123"),
            ("速記類", r"\d\w\s", "1a "),
            ("Oniguruma \\h", r"\h+", "1aG"),
            ("\\R", r"a\Rb", "a\r\nb"),
            ("錨點", r"\Aabc\z", "abc"),
            ("\\Z 在結尾換行前", r"abc\Z", "abc\n"),
            ("具名群組 <>", r"(?<num>\d+)", "x42"),
            ("具名群組 P<>", r"(?P<num>\d+)", "x42"),
            ("行內旗標", r"(?i)abc", "ABC"),
            ("後向參照", r"(ab)\1", "abab"),
            ("具名後向參照", r"(?<x>ab)\k<x>", "abab"),
            ("前瞻", r"foo(?=bar)", "foobar"),
            ("負前瞻", r"foo(?!bar)", "foobaz"),
            ("後顧", r"(?<=foo)bar", "foobar"),
            ("負後顧", r"(?<!foo)bar", "bazbar"),
            ("變長後顧", r"(?<=a+)b", "aaab"),
            ("原子群組", r"(?>ab|a)b", "abb"),
            ("佔有量詞", r"a++b", "aaab"),
            ("\\G", r"\Gabc", "abc"),
            ("\\K", r"foo\Kbar", "foobar"),
            ("條件式", r"(a)?(?(1)b|c)", "ab"),
            ("遞迴", r"a\g<0>?", "aaa"),
            ("缺席運算子", r"(?~abc)", "xyz"),
            ("Unicode 屬性", r"\p{Han}+", "中文x"),
            ("註解", r"a(?#c)b", "ab"),
            ("十六進位轉義", r"\x41", "A"),
        ];
        for (name, pat, subject) in supported {
            let got = find_one(&opts, pat, subject);
            assert!(
                matches!(got, Ok(Some(_))),
                "{name}（{pat}）應該支援且命中，實際 {:?}",
                got.map(|o| o.map(|m| m.whole))
            );
        }

        // 這兩個 fancy-regex 不支援（差異表第 2 節最後兩列）
        assert!(
            find_one(&opts, r"\101", "A").is_err(),
            "八進位轉義：fancy-regex 會當成後向參照"
        );
        assert!(find_one(&opts, r"\cA", "\u{1}").is_err(), "控制字元轉義");
    }

    /// `^`／`$` 是**行**錨點（Ruby 語法；我們預設開 `m` 補回來）。
    #[test]
    fn line_anchors_are_on_by_default() {
        let opts = RegexOptions::default();
        assert!(
            matches!(find_one(&opts, "^b", "a\nb"), Ok(Some(_))),
            "`^` 要是行錨點"
        );
        assert!(
            matches!(find_one(&opts, "c$", "abc\ndef"), Ok(Some(_))),
            "`$` 要是行錨點"
        );
        // SINGLELINE 關掉（＝原碼 `^`→`\A`、`$`→`\Z`）
        let single = RegexOptions {
            line_anchors: false,
            ..Default::default()
        };
        assert!(
            matches!(find_one(&single, "^b", "a\nb"), Ok(None)),
            "SINGLELINE 之後 `^` 只在開頭"
        );
    }

    /// Oniguruma 的 `MULTILINE` ＝「`.` 吃換行」（不是 Perl 的 `/m`）。
    #[test]
    fn oniguruma_multiline_is_dot_all() {
        let opts = RegexOptions::default();
        assert!(matches!(find_one(&opts, "a.b", "a\nb"), Ok(None)));
        let multi = RegexOptions {
            dot_matches_newline: true,
            ..Default::default()
        };
        assert!(matches!(find_one(&multi, "a.b", "a\nb"), Ok(Some(_))));
    }

    /// `FIND_NOT_EMPTY`：不回空比對。
    #[test]
    fn find_not_empty() {
        let opts = RegexOptions::default();
        let m = find_one(&opts, "a*", "bba").unwrap().unwrap();
        assert_eq!(m.whole, "", "預設會回空比對（Oniguruma 也是）");
        let ne = RegexOptions {
            find_not_empty: true,
            ..Default::default()
        };
        let m = find_one(&ne, "a*", "bba").unwrap().unwrap();
        assert_eq!(m.whole, "a");
        assert_eq!(m.start, 2);
    }

    /// `strmatch`：位置是 **1 起算的位元組位移**；群組進 `groupmatchstr*`。
    #[test]
    fn strmatch_position_and_groups() {
        let v = run("strmatch 'user=bob id=7' '(\\w+)=(\\d+)'\nr = result\nm = matchstr\ng1 = groupmatchstr1\ng2 = groupmatchstr2");
        assert_eq!(v.int_of("r"), Some(10), "`id=7` 從第 10 個位元組開始（1 起算）");
        assert_eq!(v.str_of("m").unwrap(), b"id=7");
        assert_eq!(v.str_of("g1").unwrap(), b"id");
        assert_eq!(v.str_of("g2").unwrap(), b"7");
        // 沒命中
        let v = run("strmatch 'abc' 'zzz'\nr = result");
        assert_eq!(v.int_of("r"), Some(0));
        // pattern 壞掉 → 0（和 strreplace 不一樣，照原碼）
        let v = run("strmatch 'abc' '(unclosed'\nr = result");
        assert_eq!(v.int_of("r"), Some(0));
    }

    /// 中文（UTF-8）的位元組位移。
    #[test]
    fn strmatch_byte_offsets_with_chinese() {
        let v = run("strmatch '中文abc' 'abc'\nr = result");
        assert_eq!(v.int_of("r"), Some(7), "兩個中文＝6 個位元組，所以 abc 從第 7 個開始");
    }

    /// 比對前要把舊的群組清掉（原碼那個迴圈）。
    #[test]
    fn groups_are_cleared_before_matching() {
        let v = run(
            "strmatch 'a1' '(a)(1)'\nstrmatch 'zz' '(z)'\ng1 = groupmatchstr1\ng2 = groupmatchstr2",
        );
        assert_eq!(v.str_of("g1").unwrap(), b"z");
        assert_eq!(v.str_of("g2").unwrap(), b"", "上一次的第 2 組要被清掉");
    }

    /// `strreplace`：`result` 1／0／-1，新字串**原樣插入**。
    #[test]
    fn strreplace_behaviour() {
        let v = run("s = 'hello world'\nstrreplace s 1 'w\\w+' 'there'\nr = result");
        assert_eq!(v.str_of("s").unwrap(), b"hello there");
        assert_eq!(v.int_of("r"), Some(1));

        // 從第 pos 個位元組之後才找
        let v = run("s = 'aXaY'\nstrreplace s 3 'a' 'b'\nr = result");
        assert_eq!(v.str_of("s").unwrap(), b"aXbY");

        // 沒找到 → 0，字串不動
        let v = run("s = 'abc'\nstrreplace s 1 'zzz' 'x'\nr = result");
        assert_eq!(v.str_of("s").unwrap(), b"abc");
        assert_eq!(v.int_of("r"), Some(0));

        // pos 超範圍 → 0
        let v = run("s = 'abc'\nstrreplace s 9 'a' 'x'\nr = result");
        assert_eq!(v.int_of("r"), Some(0));

        // pattern 壞掉 → **-1**（和 strmatch 不同，照原碼）
        let v = run("s = 'abc'\nstrreplace s 1 '(unclosed' 'x'\nr = result");
        assert_eq!(v.int_of("r"), Some(-1));

        // `\1` 不會展開（原碼沒有這個功能）
        let v = run("s = 'ab'\nstrreplace s 1 '(a)(b)' '[\\1]'\nr = result");
        assert_eq!(
            v.str_of("s").unwrap(),
            br"[\1]".to_vec(),
            "新字串是原樣插入的"
        );
    }

    /// `regexoption`：認得的設定會生效，不支援的不會讓巨集掛掉，亂寫的是語法錯誤。
    #[test]
    fn regexoption_mapping() {
        // IGNORECASE
        let v = run("regexoption 'IGNORECASE'\nstrmatch 'ABC' 'abc'\nr = result");
        assert_eq!(v.int_of("r"), Some(1));
        // OPTION_ 前綴也可以
        let v = run("regexoption 'OPTION_IGNORECASE'\nstrmatch 'ABC' 'abc'\nr = result");
        assert_eq!(v.int_of("r"), Some(1));
        // SINGLELINE 關掉行錨點
        let v = run("regexoption 'SINGLELINE'\nstrmatch 'a#b' '^b'\nr = result");
        assert_eq!(v.int_of("r"), Some(0));
        // NONE 回到預設
        let v = run("regexoption 'IGNORECASE'\nregexoption 'OPTION_NONE'\nstrmatch 'ABC' 'abc'\nr = result");
        assert_eq!(v.int_of("r"), Some(0));
        // 不支援但認得的關鍵字：不報錯
        let v = run("regexoption 'FIND_LONGEST'\nregexoption 'BIG5'\nregexoption 'SYNTAX_EMACS'\nr = 1");
        assert_eq!(v.int_of("r"), Some(1));
        // 亂寫 → 語法錯誤（照原碼）
        assert_eq!(
            run_text("regexoption 'NOT_A_REAL_OPTION'").unwrap_err().err,
            Err::Syntax
        );
    }

    /// 非 UTF-8 的位元組：解碼時變成 U+FFFD，所以比對不到原本的字（差異表第 4 節）。
    #[test]
    fn invalid_utf8_is_lossy() {
        // Big5 的「中」是 0xA4 0xA4
        let v = run("s = #$A4#$A4\nstrmatch s '.'\nr = result");
        // 還是會命中「某個字元」（U+FFFD），但不是原本的中文
        assert_eq!(v.int_of("r"), Some(1));
        let v = run("s = #$A4#$A4\nstrmatch s '中'\nr = result");
        assert_eq!(v.int_of("r"), Some(0), "Big5 的位元組比不到 UTF-8 的『中』");
    }
}
