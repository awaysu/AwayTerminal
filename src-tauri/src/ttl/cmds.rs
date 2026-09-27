//! 不碰 I/O 的 TTL 指令——逐個照 `ttpmacro/ttl.cpp` 的 `TTL*` 函式。
//!
//! 每個指令的註解都寫了原碼的函式名，對照原碼時照著找就好。
//! 參數的讀法、`result` 的值、邊界條件（1 起算的位置、負數、超出範圍…）都照原碼。
//!
//! 這一批**不含**：連線（`send`／`wait`／`connect`…）、對話框、檔案存取、
//! 正規表示式（`strmatch`／`strreplace`／`waitregex`）、剪貼簿、`exec`。
//! 那些是 TASK-013，原因寫在 `docs/TTL.md`。

use super::cksum;
use super::error::{Err, Result};
use super::exec::Interp;
use super::vars::VarType;
use super::words::Word;

impl Interp {
    /// `ExecCmnd` 裡「不是流程控制」的那一大段 switch。
    pub(super) fn dispatch_cmds(&mut self, w: Word) -> Result<()> {
        match w {
            // ---- 陣列宣告 ----
            Word::IntDim => self.cmd_dim(true),
            Word::StrDim => self.cmd_dim(false),

            // ---- 字串 ----
            Word::StrLen => self.cmd_strlen(),
            Word::StrCompare => self.cmd_strcompare(),
            Word::StrConcat => self.cmd_strconcat(),
            Word::StrCopy => self.cmd_strcopy(),
            Word::StrScan => self.cmd_strscan(),
            Word::StrInsert => self.cmd_strinsert(),
            Word::StrRemove => self.cmd_strremove(),
            Word::StrTrim => self.cmd_strtrim(),
            Word::StrSplit => self.cmd_strsplit(),
            Word::StrJoin => self.cmd_strjoin(),
            Word::StrSpecial => self.cmd_strspecial(),
            Word::ToLower => self.cmd_case(false),
            Word::ToUpper => self.cmd_case(true),

            // ---- 整數 ↔ 字串 ----
            Word::Int2Str => self.cmd_int2str(),
            Word::Str2Int => self.cmd_str2int(),
            Word::Code2Str => self.cmd_code2str(),
            Word::Str2Code => self.cmd_str2code(),
            Word::Sprintf => self.cmd_sprintf(false),
            Word::Sprintf2 => self.cmd_sprintf(true),

            // ---- 數值 ----
            Word::Random => self.cmd_random(),

            // ---- CRC / checksum（`cksum.rs`，逐段照 ttl.cpp）----
            Word::Checksum8 => self.cmd_cksum(cksum::Kind::Sum8, false),
            Word::Checksum8File => self.cmd_cksum(cksum::Kind::Sum8, true),
            Word::Checksum16 => self.cmd_cksum(cksum::Kind::Sum16, false),
            Word::Checksum16File => self.cmd_cksum(cksum::Kind::Sum16, true),
            Word::Checksum32 => self.cmd_cksum(cksum::Kind::Sum32, false),
            Word::Checksum32File => self.cmd_cksum(cksum::Kind::Sum32, true),
            Word::Crc16 => self.cmd_cksum(cksum::Kind::Crc16, false),
            Word::Crc16File => self.cmd_cksum(cksum::Kind::Crc16, true),
            Word::Crc32 => self.cmd_cksum(cksum::Kind::Crc32, false),
            Word::Crc32File => self.cmd_cksum(cksum::Kind::Crc32, true),
            Word::Uptime => self.cmd_uptime(),
            Word::GetHostname => self.cmd_gethostname(),
            Word::RotateL => self.cmd_rotate(true),
            Word::RotateR => self.cmd_rotate(false),

            // ---- 時間 ----
            Word::GetTime => self.cmd_get_time(true),
            Word::GetDate => self.cmd_get_time(false),

            // ---- 環境變數與路徑 ----
            Word::GetEnv => self.cmd_getenv(),
            Word::SetEnv => self.cmd_setenv(),
            Word::ExpandEnv => self.cmd_expandenv(),
            Word::Basename => self.cmd_basename(),
            Word::Dirname => self.cmd_dirname(),
            Word::MakePath => self.cmd_makepath(),

            // ---- 其他 ----
            Word::SetExitCode => self.cmd_set_exit_code(),
            Word::GetVer => self.cmd_get_ver(),

            // 認得是保留字但還沒實作
            Word::Unsupported(_) => Err(Err::NotSupported),
            // 流程控制在 exec.rs、字詞運算子不能當指令用；其餘（會碰外界的）在 io.rs
            other => self.dispatch_io(other),
        }
    }

    // ---------------------------------------------------------------- 陣列

    /// `TTLDim`：`intdim <名稱> <大小>` / `strdim <名稱> <大小>`。
    /// 名稱是保留字、或已經存在 → 語法錯誤（原碼就是這樣，不是「重新配置」）。
    fn cmd_dim(&mut self, int_array: bool) -> Result<()> {
        let Some(name) = self.lex_identifier() else {
            return Err(Err::Syntax);
        };
        if super::words::check_reserved_word(&name).is_some() {
            return Err(Err::Syntax);
        }
        if self.vars.find(&name).is_some() {
            return Err(Err::Syntax);
        }
        let size = self.int_val()?;
        if int_array {
            self.vars.new_int_array(&name, size)
        } else {
            self.vars.new_str_array(&name, size)
        }
    }

    // ---------------------------------------------------------------- 字串

    /// `TTLStrLen`：`strlen <字串>` → `result` ＝**位元組長度**（中文一個字是 3 個）。
    fn cmd_strlen(&mut self) -> Result<()> {
        let s = self.str_val()?;
        self.end_of_args()?;
        self.vars.set_result(s.len() as i32);
        Ok(())
    }

    /// `TTLStrCompare`：`strcompare <a> <b>` → `result` ＝ -1／0／1（`strcmp` 的位元組比較）。
    fn cmd_strcompare(&mut self) -> Result<()> {
        let a = self.str_val()?;
        let b = self.str_val()?;
        self.end_of_args()?;
        let r = match a.cmp(&b) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
        self.vars.set_result(r);
        Ok(())
    }

    /// `TTLStrConcat`：`strconcat <字串變數> <字串>`（接在後面，超過 511 就截掉）。
    fn cmd_strconcat(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let add = self.str_val()?;
        self.end_of_args()?;
        let mut cur = self.get_str_ref(&target)?;
        cur.extend_from_slice(&add);
        self.set_str_ref(&target, &cur)
    }

    /// `TTLStrCopy`：`strcopy <來源> <從第幾個字（1 起算）> <幾個字> <目標變數>`。
    /// `from < 1` 當 1；長度超過剩餘長度就截短；負數長度當 0。
    fn cmd_strcopy(&mut self) -> Result<()> {
        let src = self.str_val()?;
        let mut from = self.int_val()?;
        let mut len = self.int_val()?;
        let target = self.var_ref(VarType::String)?;
        self.end_of_args()?;

        if from < 1 {
            from = 1;
        }
        let src_len = src.len() as i32 - from + 1;
        if len > src_len {
            len = src_len;
        }
        if len < 0 {
            len = 0;
        }
        let start = (from - 1).min(src.len() as i32).max(0) as usize;
        let end = (start + len.max(0) as usize).min(src.len());
        let out = src[start..end].to_vec();
        self.set_str_ref(&target, &out)
    }

    /// `TTLStrScan`：`strscan <被找的> <要找的>` → `result` ＝位置（**1 起算**），找不到是 0。
    /// 任一邊是空字串就是 0。
    fn cmd_strscan(&mut self) -> Result<()> {
        let hay = self.str_val()?;
        let needle = self.str_val()?;
        self.end_of_args()?;
        if hay.is_empty() || needle.is_empty() {
            self.vars.set_result(0);
            return Ok(());
        }
        let pos = hay
            .windows(needle.len())
            .position(|w| w == needle.as_slice())
            .map(|i| i as i32 + 1)
            .unwrap_or(0);
        self.vars.set_result(pos);
        Ok(())
    }

    /// `TTLStrInsert`：`strinsert <字串變數> <位置（1 起算）> <要插入的>`。
    /// 位置不在 1..=長度+1，或插完超過上限 → 語法錯誤。
    fn cmd_strinsert(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let index = self.int_val()?;
        let add = self.str_val()?;
        self.end_of_args()?;

        let cur = self.get_str_ref(&target)?;
        if index <= 0 || index > cur.len() as i32 + 1 {
            return Err(Err::Syntax);
        }
        if cur.len() + add.len() + 1 > super::lex::MAX_STR_LEN {
            return Err(Err::Syntax);
        }
        let at = (index - 1) as usize;
        let mut out = cur[..at].to_vec();
        out.extend_from_slice(&add);
        out.extend_from_slice(&cur[at..]);
        self.set_str_ref(&target, &out)
    }

    /// `TTLStrRemove`：`strremove <字串變數> <位置（1 起算）> <幾個字>`。
    /// 長度 <= 0、位置 <= 0、或超出字串範圍 → 語法錯誤。
    fn cmd_strremove(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let index = self.int_val()?;
        let len = self.int_val()?;
        self.end_of_args()?;

        let cur = self.get_str_ref(&target)?;
        if len <= 0 || index <= 0 || (index - 1 + len) > cur.len() as i32 {
            return Err(Err::Syntax);
        }
        let at = (index - 1) as usize;
        let mut out = cur[..at].to_vec();
        out.extend_from_slice(&cur[at + len as usize..]);
        self.set_str_ref(&target, &out)
    }

    /// `TTLStrTrim`：`strtrim <字串變數> <要去掉的字元們>`（頭尾各去，中間不動）。
    fn cmd_strtrim(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let chars = self.str_val()?;
        self.end_of_args()?;

        let cur = self.get_str_ref(&target)?;
        let mut table = [false; 256];
        for &c in &chars {
            table[c as usize] = true;
        }
        let start = cur.iter().position(|&c| !table[c as usize]).unwrap_or(cur.len());
        let end = cur
            .iter()
            .rposition(|&c| !table[c as usize])
            .map(|i| i + 1)
            .unwrap_or(start);
        let out = cur[start..end.max(start)].to_vec();
        self.set_str_ref(&target, &out)
    }

    /// `TTLStrSplit`：`strsplit <來源> <分隔字元> [最多幾段]`。
    ///
    /// 結果放進 `groupmatchstr1..9`，`result` ＝實際切出幾段。
    /// **分隔字元只能是一個字元**（原碼：`len != 1` 就語法錯誤）；`最多幾段` 只能是 1～9。
    /// 省略時是 9，而且超過的部分**丟掉**（原碼多走一格就是為了這個）。
    fn cmd_strsplit(&mut self) -> Result<()> {
        let src = self.str_val()?;
        let delim = self.str_val()?;
        let mut maxvar = 9;
        let mut omit = true;
        if self.parameter_given() {
            maxvar = self.int_val()?;
            omit = false;
        }
        self.end_of_args()?;
        if !(1..=9).contains(&maxvar) {
            return Err(Err::Syntax);
        }
        if delim.len() != 1 {
            return Err(Err::Syntax);
        }
        let d = delim[0];

        // 原碼：從頭掃，遇到分隔字元就切一段；count 省略時多走一格把超過的丟掉
        let limit = maxvar + i32::from(omit);
        let mut parts: Vec<Vec<u8>> = vec![Vec::new()];
        let mut count = 1;
        for &b in src.iter() {
            if count >= limit {
                break;
            }
            if b == d {
                count += 1;
                if count <= 9 {
                    parts.push(Vec::new());
                }
            } else if let Some(last) = parts.last_mut() {
                last.push(b);
            }
        }
        // 省略「最多幾段」時，最後多出來的那一段是「丟掉」的，不算在 count 裡
        let real = count.min(maxvar);
        for i in 1..=9 {
            let v = if i <= real {
                parts.get(i as usize - 1).cloned().unwrap_or_default()
            } else {
                Vec::new()
            };
            self.vars.set_group_match(i as usize, &v);
        }
        self.vars.set_result(count);
        Ok(())
    }

    /// `TTLStrJoin`：`strjoin <字串變數> <分隔字串> [幾段]`——把 `groupmatchstr1..N` 接起來。
    fn cmd_strjoin(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let delim = self.str_val()?;
        let mut maxvar = 9;
        if self.parameter_given() {
            maxvar = self.int_val()?;
        }
        self.end_of_args()?;
        if !(1..=9).contains(&maxvar) {
            return Err(Err::Syntax);
        }
        let mut out: Vec<u8> = Vec::new();
        for i in 1..=maxvar {
            let name = format!("groupmatchstr{i}");
            if let Some(s) = self.vars.str_of(&name) {
                out.extend_from_slice(s);
                if i < maxvar {
                    out.extend_from_slice(&delim);
                }
            }
        }
        self.set_str_ref(&target, &out)
    }

    /// `TTLStrSpecial` → `RestoreNewLine`：把 `\n` `\t` `\0` `\\` 換成真正的字元，
    /// **其餘的反斜線原樣留著**（`\q` 還是 `\q`）。
    fn cmd_strspecial(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let src = if self.parameter_given() {
            let s = self.str_val()?;
            self.end_of_args()?;
            s
        } else {
            self.end_of_args()?;
            self.get_str_ref(&target)?
        };
        let mut out = Vec::with_capacity(src.len());
        let mut i = 0;
        while i < src.len() {
            if src[i] == b'\\' && i + 1 < src.len() {
                match src[i + 1] {
                    b'\\' => {
                        out.push(b'\\');
                        i += 2;
                    }
                    b'n' => {
                        out.push(b'\n');
                        i += 2;
                    }
                    b't' => {
                        out.push(b'\t');
                        i += 2;
                    }
                    b'0' => {
                        out.push(0);
                        i += 2;
                    }
                    _ => {
                        out.push(b'\\');
                        i += 1;
                    }
                }
            } else {
                out.push(src[i]);
                i += 1;
            }
        }
        self.set_str_ref(&target, &out)
    }

    /// `TTLToLower`／`TTLToUpper`：`tolower <目標> <來源>`。**只動 ASCII A-Z／a-z**
    /// （原碼是手寫的 ±0x20，不碰其他位元組——中文不會被弄壞）。
    fn cmd_case(&mut self, upper: bool) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let src = self.str_val()?;
        self.end_of_args()?;
        let out: Vec<u8> = src
            .iter()
            .map(|&c| {
                if upper {
                    if c.is_ascii_lowercase() {
                        c - 0x20
                    } else {
                        c
                    }
                } else if c.is_ascii_uppercase() {
                    c + 0x20
                } else {
                    c
                }
            })
            .collect();
        self.set_str_ref(&target, &out)
    }

    // ---------------------------------------------------------------- 整數 ↔ 字串

    /// `TTLInt2Str`：`int2str <字串變數> <整數>`（十進位，`%d`）。
    fn cmd_int2str(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let num = self.int_val()?;
        self.end_of_args()?;
        self.set_str_ref(&target, num.to_string().as_bytes())
    }

    /// `TTLStr2Int`：`str2int <整數變數> <字串>`。
    ///
    /// 十進位，或 `$`／`0x` 開頭的十六進位（原碼把 `$` 換成 `0x` 再走 `%i`）。
    /// `result` ＝ 1 成功／0 失敗，失敗時變數設 0。
    fn cmd_str2int(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::Integer)?;
        let s = self.str_val()?;
        self.end_of_args()?;

        let text = String::from_utf8_lossy(&s).to_string();
        let t = text.trim_start();
        let (radix, digits) = if let Some(rest) = t.strip_prefix('$') {
            (16, rest)
        } else if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
            (16, rest)
        } else {
            (10, t)
        };
        // C 的 sscanf("%d"/"%i")：吃到不是數字就停，前面有東西才算成功
        let (sign, digits) = match digits.strip_prefix('-') {
            Some(r) if radix == 10 => (-1i64, r),
            _ => (1i64, digits.strip_prefix('+').unwrap_or(digits)),
        };
        let taken: String = digits
            .chars()
            .take_while(|c| c.is_digit(radix))
            .collect();
        if taken.is_empty() {
            self.set_int_ref(&target, 0)?;
            self.vars.set_result(0);
            return Ok(());
        }
        let v = i64::from_str_radix(&taken, radix).unwrap_or(0) * sign;
        self.set_int_ref(&target, v as i32)?;
        self.vars.set_result(1);
        Ok(())
    }

    /// `TTLCode2Str`：`code2str <字串變數> <整數>`——把 32-bit 值當 big-endian 的
    /// 4 個位元組，**跳過前面的 0**。例：`code2str s $41` → `"A"`、`$4142` → `"AB"`。
    fn cmd_code2str(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let num = self.int_val()? as u32;
        self.end_of_args()?;
        let mut out = Vec::with_capacity(4);
        for c in 0..4 {
            let d = ((num >> ((3 - c) * 8)) & 0xff) as u8;
            if !out.is_empty() || d != 0 {
                out.push(d);
            }
        }
        self.set_str_ref(&target, &out)
    }

    /// `TTLStr2Code`：`str2code <整數變數> <字串>`——取**最後** 4 個位元組當 big-endian。
    fn cmd_str2code(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::Integer)?;
        let s = self.str_val()?;
        self.end_of_args()?;
        let take = s.len().min(4);
        let mut num: u32 = 0;
        for &b in &s[s.len() - take..] {
            num = num.wrapping_mul(256).wrapping_add(b as u32);
        }
        self.set_int_ref(&target, num as i32)
    }

    /// `TTLSprintf`（`sprintf`）／`TTLSprintf(1)`（`sprintf2`）。
    ///
    /// `sprintf <格式> <參數…>` → 結果放進 `inputstr`；
    /// `sprintf2 <字串變數> <格式> <參數…>` → 放進指定的變數。
    ///
    /// 支援 `%[flags][width][.precision]conv`，flags 是 `- + 0 空白 #`，
    /// width／precision 可以是 `*`（從參數取）。conv 支援 `c d i o u x X s %`。
    /// ⚠️ **浮點（`e E f g G a A`）沒實作**：TTL 本身沒有浮點型別，原碼是直接丟給 C 的
    /// `snprintf`；我們回 `result=2` + 語法錯誤（同「格式不合法」那條路）。見 docs/TTL.md。
    fn cmd_sprintf(&mut self, to_var: bool) -> Result<()> {
        let target = if to_var {
            match self.var_ref(VarType::String) {
                Ok(t) => Some(t),
                Err(e) => {
                    self.vars.set_result(4);
                    return Err(e);
                }
            }
        } else {
            None
        };
        let fmt = match self.str_val() {
            Ok(f) => f,
            Err(e) => {
                self.vars.set_result(1);
                return Err(e);
            }
        };

        let mut out: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < fmt.len() {
            if fmt[i] != b'%' {
                out.push(fmt[i]);
                i += 1;
                continue;
            }
            // 收集 %…conv
            let mut j = i + 1;
            if j < fmt.len() && fmt[j] == b'%' {
                out.push(b'%');
                i = j + 1;
                continue;
            }
            let mut flags = Flags::default();
            while j < fmt.len() {
                match fmt[j] {
                    b'-' => flags.left = true,
                    b'+' => flags.plus = true,
                    b'0' => flags.zero = true,
                    b' ' => flags.space = true,
                    b'#' => flags.alt = true,
                    _ => break,
                }
                j += 1;
            }
            // width
            let mut width: Option<usize> = None;
            if j < fmt.len() && fmt[j] == b'*' {
                let w = self.int_val()?;
                width = Some(w.max(0) as usize);
                if w < 0 {
                    flags.left = true; // C 的規則：負寬度＝靠左
                }
                j += 1;
            } else {
                let mut n = 0usize;
                let mut any = false;
                while j < fmt.len() && fmt[j].is_ascii_digit() {
                    n = n * 10 + (fmt[j] - b'0') as usize;
                    j += 1;
                    any = true;
                }
                if any {
                    width = Some(n);
                }
            }
            // precision
            let mut prec: Option<usize> = None;
            if j < fmt.len() && fmt[j] == b'.' {
                j += 1;
                if j < fmt.len() && fmt[j] == b'*' {
                    let p = self.int_val()?;
                    prec = Some(p.max(0) as usize);
                    j += 1;
                } else {
                    let mut n = 0usize;
                    while j < fmt.len() && fmt[j].is_ascii_digit() {
                        n = n * 10 + (fmt[j] - b'0') as usize;
                        j += 1;
                    }
                    prec = Some(n);
                }
            }
            if j >= fmt.len() {
                // `%` 之後什麼都沒有：原碼把剩下的原樣接上去
                out.extend_from_slice(&fmt[i..]);
                break;
            }
            let conv = fmt[j];
            let piece = match conv {
                b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'c' => {
                    let v = self.int_val()?;
                    format_int(conv, v, &flags, width, prec)
                }
                b's' => {
                    let s = self.str_val()?;
                    format_str(&s, &flags, width, prec)
                }
                _ => {
                    // 浮點與不認識的轉換
                    self.vars.set_result(2);
                    return Err(Err::Syntax);
                }
            };
            out.extend_from_slice(&piece);
            i = j + 1;
        }

        match target {
            Some(t) => self.set_str_ref(&t, &out)?,
            None => self.vars.set_str("inputstr", &out),
        }
        self.vars.set_result(0);
        Ok(())
    }

    // ---------------------------------------------------------------- 數值

    /// `TTLRandom`：`random <整數變數> <上限>` → 0～上限（**含上限**）。上限 <= 0 是語法錯誤。
    ///
    /// 原碼用 SFMT 亂數並做「去偏」；我們用系統時間＋xorshift 的小亂數器
    /// （巨集用不著密碼學等級的亂數，也不想為此多一個依賴）。範圍與含端點的行為照原碼。
    fn cmd_random(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::Integer)?;
        let max = self.int_val()?;
        if self.lex_first_char_nonzero() || max <= 0 {
            return Err(Err::Syntax);
        }
        let span = (max as u32).wrapping_add(1);
        let v = (next_rand() % span) as i32;
        self.set_int_ref(&target, v)
    }

    /// `TTLDoChecksum`／`TTLDoChecksumFile`：
    /// `crc16 <整數變數> <字串>`、`crc16file <整數變數> <檔名>`（其餘四種同形）。
    ///
    /// 照原碼的兩個細節：
    ///   * **空字串／空檔名直接 return，不寫變數**（`if (Str[0]==0) return Err;`）
    ///   * `*file` 開不了檔時 `result` ＝ **-1**，而且**也不寫變數**
    ///
    /// 算法本身在 [`cksum`]（含標準檢查向量的測試）。
    fn cmd_cksum(&mut self, kind: cksum::Kind, from_file: bool) -> Result<()> {
        let target = self.var_ref(VarType::Integer)?;
        let arg = self.str_val()?;
        self.end_of_args()?;
        if arg.is_empty() {
            return Ok(()); // 同原碼：什麼都不做
        }
        let data = if from_file {
            let path = String::from_utf8_lossy(&arg).to_string();
            match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    // 原碼：result = -1，變數不動
                    self.vars.set_result(-1);
                    return Ok(());
                }
            }
        } else {
            arg
        };
        let v = kind.apply(&data);
        // TTL 的整數是 32 位元有號（`docs/TTL.md`）：CRC32 超過 i32::MAX 時照原碼的
        // 位元樣式存進去（原碼是 `SetIntVal(VarId, cksum)`，DWORD → int 的位元重解讀）。
        self.set_int_ref(&target, v as i32)
    }

    /// `TTLUptime`：`uptime <整數變數>` ＝ 作業系統開機到現在的**毫秒**數。
    ///
    /// 原碼用 `GetTickCount()`（32 位元，**49 天會繞回 0**，原碼註解明講而且說
    /// 「TeraTerm 不支援 64 位元變數所以用 GetTickCount64 沒有意義」）。
    /// 我們照它繞回——`docs/TTL.md` 有記這個相容行為。
    ///
    /// 跨平台的取值在 `awayterm_platform::uptime`（Windows `GetTickCount64`、
    /// Linux `/proc/uptime`、mac `sysctl kern.boottime`），一律截成 32 位元。
    fn cmd_uptime(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::Integer)?;
        self.end_of_args()?;
        let ms = awayterm_platform::uptime::uptime_ms_u32();
        self.set_int_ref(&target, ms as i32)
    }

    /// `TTLGetHostname`：`gethostname <字串變數>` ＝ **這條連線**連到的主機。
    ///
    /// ⚠️ **不是本機的 hostname。** 原碼走 DDE 問 ttermpro「你現在連到哪」，
    /// 而且會先檢查 `Linked`——沒連線時回 `Link macro first. Use 'connect' macro.`
    /// （＝`Err::LinkFirst`），這一點照抄。
    ///
    /// 這是 TASK-023 查原碼才發現的：照名字實作成本機 hostname 會是**靜悄悄的錯**。
    fn cmd_gethostname(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        self.end_of_args()?;
        // `need_link()` 是 io.rs 已經有的那一條（沒有 host 或連線不在 → `LinkFirst`），
        // 和 `send`／`wait` 走同一個判斷。
        self.need_link()?;
        let host = self
            .host()
            .and_then(|h| h.conn_host())
            .unwrap_or_default();
        self.set_str_ref(&target, host.as_bytes())
    }

    /// `BitRotate`：`rotateleft <整數變數> <值> <位數>`（`rotateright` 是負的位數）。
    fn cmd_rotate(&mut self, left: bool) -> Result<()> {
        let target = self.var_ref(VarType::Integer)?;
        let x = self.int_val()?;
        let mut n = self.int_val()?;
        self.end_of_args()?;
        if !left {
            n = n.wrapping_neg();
        }
        n %= 32;
        if n < 0 {
            n += 32;
        }
        let v = if n == 0 {
            x
        } else {
            (x.wrapping_shl(n as u32)) | (((x as u32) >> (32 - n)) as i32)
        };
        self.set_int_ref(&target, v)
    }

    // ---------------------------------------------------------------- 時間

    /// `TTLGetTime`：`gettime <字串變數> [格式] [時區]`／`getdate` 同款。
    ///
    /// 沒給格式時：`getdate` ＝ `%Y-%m-%d`、`gettime` ＝ `%H:%M:%S`（原碼的預設）。
    /// 給了格式才會設 `result`（0 成功／1 失敗／2 格式裡有不能用的 `%x`）。
    ///
    /// 可用的 `%` 代號**照原碼的白名單**（`IsValidStrftimeCode`）：
    /// `a A b B c d H I j m M p S U w W x X y Y z Z %`，前面可以加 `#`（我們接受但忽略）。
    /// ⚠️ 第三個參數（時區）**沒實作**——原碼是改 `TZ` 環境變數再 `tzset()`，
    /// 那會影響整個行程（我們是多分頁的 app，不能這樣做）。給了就回 `result=2`。
    fn cmd_get_time(&mut self, time_mode: bool) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let mut set_result = false;
        let fmt: Vec<u8> = if self.parameter_given() {
            set_result = true;
            let f = self.str_val()?;
            if self.parameter_given() {
                // 時區：不支援（見上面的說明）
                let _ = self.str_val()?;
                self.end_of_args()?;
                self.vars.set_result(2);
                return Ok(());
            }
            f
        } else if time_mode {
            b"%H:%M:%S".to_vec()
        } else {
            b"%Y-%m-%d".to_vec()
        };
        self.end_of_args()?;

        match strftime(&fmt) {
            Some(s) => {
                self.set_str_ref(&target, s.as_bytes())?;
                if set_result {
                    self.vars.set_result(0);
                }
            }
            None => {
                if set_result {
                    self.vars.set_result(2);
                }
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------- 環境與路徑

    /// `TTLGetEnv`：`getenv <名稱> <字串變數>`（沒有那個環境變數就是空字串）。
    fn cmd_getenv(&mut self) -> Result<()> {
        let name = self.str_val()?;
        let target = self.var_ref(VarType::String)?;
        self.end_of_args()?;
        let val = std::env::var(String::from_utf8_lossy(&name).as_ref()).unwrap_or_default();
        self.set_str_ref(&target, val.as_bytes())
    }

    /// `TTLSetEnv`：`setenv <名稱> <值>`。
    ///
    /// ⚠️ 只影響**這個行程**（原碼也是 `_putenv_s`）。我們是多分頁的 app，
    /// 所以它會影響之後開的所有分頁——文件裡要提醒。
    fn cmd_setenv(&mut self) -> Result<()> {
        let name = self.str_val()?;
        let val = self.str_val()?;
        self.end_of_args()?;
        let name = String::from_utf8_lossy(&name).into_owned();
        let val = String::from_utf8_lossy(&val).into_owned();
        // SAFETY: 單執行緒的巨集執行期間設環境變數；同原碼的 `_putenv_s`。
        unsafe { std::env::set_var(name, val) };
        Ok(())
    }

    /// `TTLExpandEnv`：`expandenv <字串變數> [字串]`——把 `%NAME%` 換成環境變數的值。
    fn cmd_expandenv(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let src = if self.parameter_given() {
            let s = self.str_val()?;
            self.end_of_args()?;
            s
        } else {
            self.end_of_args()?;
            self.get_str_ref(&target)?
        };
        let text = String::from_utf8_lossy(&src).into_owned();
        let mut out = String::new();
        let mut rest = text.as_str();
        while let Some(start) = rest.find('%') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            match after.find('%') {
                Some(end) => {
                    let name = &after[..end];
                    match std::env::var(name) {
                        Ok(v) => out.push_str(&v),
                        // 找不到就把 `%NAME%` 原樣留著（Windows 的 ExpandEnvironmentStrings 也是）
                        Err(_) => {
                            out.push('%');
                            out.push_str(name);
                            out.push('%');
                        }
                    }
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('%');
                    out.push_str(after);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        self.set_str_ref(&target, out.as_bytes())
    }

    /// `TTLBasename`：`basename <字串變數> <路徑>`。
    fn cmd_basename(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let path = self.str_val()?;
        self.end_of_args()?;
        let s = String::from_utf8_lossy(&path).into_owned();
        let base = s
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&s)
            .to_string();
        self.set_str_ref(&target, base.as_bytes())
    }

    /// `TTLDirname`：`dirname <字串變數> <路徑>`。
    fn cmd_dirname(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let path = self.str_val()?;
        self.end_of_args()?;
        let s = String::from_utf8_lossy(&path).into_owned();
        let dir = match s.rfind(['\\', '/']) {
            Some(i) => s[..i].to_string(),
            None => String::new(),
        };
        self.set_str_ref(&target, dir.as_bytes())
    }

    /// `TTLMakePath`：`makepath <字串變數> <資料夾> <檔名>`（資料夾尾端沒有 `\` 就補一個）。
    fn cmd_makepath(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        let dir = self.str_val()?;
        let name = self.str_val()?;
        self.end_of_args()?;
        let mut out = dir.clone();
        if !out.is_empty() && out.last() != Some(&b'\\') && out.last() != Some(&b'/') {
            out.push(b'\\');
        }
        out.extend_from_slice(&name);
        self.set_str_ref(&target, &out)
    }

    // ---------------------------------------------------------------- 其他

    /// `TTLSetExitCode`：`setexitcode <整數>`。
    fn cmd_set_exit_code(&mut self) -> Result<()> {
        let v = self.int_val()?;
        self.end_of_args()?;
        self.exit_code = v;
        Ok(())
    }

    /// `TTLGetVer`：`getver <字串變數>`——回本程式的版本（原碼回 Tera Term 的版本）。
    fn cmd_get_ver(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        self.end_of_args()?;
        let v = format!("AwayTerminal {}", env!("CARGO_PKG_VERSION"));
        self.set_str_ref(&target, v.as_bytes())
    }
}

#[derive(Default)]
struct Flags {
    left: bool,
    plus: bool,
    zero: bool,
    space: bool,
    alt: bool,
}

/// `%d` `%i` `%u` `%o` `%x` `%X` `%c` 的格式化（C 的規則）。
fn format_int(conv: u8, v: i32, f: &Flags, width: Option<usize>, prec: Option<usize>) -> Vec<u8> {
    if conv == b'c' {
        let body = vec![(v as u32 & 0xff) as u8];
        return pad(body, f, width, false);
    }
    let (mut body, neg) = match conv {
        b'd' | b'i' => {
            let neg = v < 0;
            ((v as i64).unsigned_abs().to_string(), neg)
        }
        b'u' => ((v as u32).to_string(), false),
        b'o' => (format!("{:o}", v as u32), false),
        b'x' => (format!("{:x}", v as u32), false),
        _ => (format!("{:X}", v as u32), false),
    };
    // precision ＝最少幾位數字
    if let Some(p) = prec {
        while body.len() < p {
            body.insert(0, '0');
        }
    }
    if f.alt {
        match conv {
            b'o' if !body.starts_with('0') => body.insert(0, '0'),
            b'x' => body.insert_str(0, "0x"),
            b'X' => body.insert_str(0, "0X"),
            _ => {}
        }
    }
    let sign = if neg {
        "-"
    } else if matches!(conv, b'd' | b'i') {
        if f.plus {
            "+"
        } else if f.space {
            " "
        } else {
            ""
        }
    } else {
        ""
    };
    // `0` 旗標：補在正負號之後
    if f.zero && !f.left && prec.is_none() {
        if let Some(w) = width {
            while body.len() + sign.len() < w {
                body.insert(0, '0');
            }
        }
    }
    let full = format!("{sign}{body}").into_bytes();
    pad(full, f, width, false)
}

/// `%s` 的格式化（precision ＝最多幾個位元組）。
fn format_str(s: &[u8], f: &Flags, width: Option<usize>, prec: Option<usize>) -> Vec<u8> {
    let mut body = s.to_vec();
    if let Some(p) = prec {
        body.truncate(p);
    }
    pad(body, f, width, true)
}

fn pad(body: Vec<u8>, f: &Flags, width: Option<usize>, _is_str: bool) -> Vec<u8> {
    let Some(w) = width else { return body };
    if body.len() >= w {
        return body;
    }
    let fill = w - body.len();
    let mut out = Vec::with_capacity(w);
    if f.left {
        out.extend_from_slice(&body);
        out.extend(std::iter::repeat_n(b' ', fill));
    } else {
        out.extend(std::iter::repeat_n(b' ', fill));
        out.extend_from_slice(&body);
    }
    out
}

/// `strftime` 的子集——**只認原碼白名單裡的代號**
/// （`IsValidStrftimeCode`：`a A b B c d H I j m M p S U w W x X y Y z Z %`）。
/// 遇到白名單以外的 `%x` 回 `None`（＝原碼的 `isInvalidStrftimeChar` → `result=2`）。
fn strftime(fmt: &[u8]) -> Option<String> {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    const WD_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const WD_LONG: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    const MON_SHORT: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const MON_LONG: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let wd = now.weekday().num_days_from_sunday() as usize;
    let mon0 = now.month0() as usize;
    let mut out = String::new();
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] != b'%' {
            out.push(fmt[i] as char);
            i += 1;
            continue;
        }
        if i + 1 >= fmt.len() {
            return None; // 以 `%` 結尾＝不合法
        }
        let mut j = i + 1;
        if fmt[j] == b'#' && j + 1 < fmt.len() {
            j += 1; // MSVC 的 `#` 修飾詞：接受但忽略
        }
        let piece = match fmt[j] {
            b'a' => WD_SHORT[wd].to_string(),
            b'A' => WD_LONG[wd].to_string(),
            b'b' => MON_SHORT[mon0].to_string(),
            b'B' => MON_LONG[mon0].to_string(),
            b'c' => format!(
                "{} {} {:2} {:02}:{:02}:{:02} {}",
                WD_SHORT[wd],
                MON_SHORT[mon0],
                now.day(),
                now.hour(),
                now.minute(),
                now.second(),
                now.year()
            ),
            b'd' => format!("{:02}", now.day()),
            b'H' => format!("{:02}", now.hour()),
            b'I' => format!("{:02}", if now.hour12().1 == 0 { 12 } else { now.hour12().1 }),
            b'j' => format!("{:03}", now.ordinal()),
            b'm' => format!("{:02}", now.month()),
            b'M' => format!("{:02}", now.minute()),
            b'p' => (if now.hour12().0 { "PM" } else { "AM" }).to_string(),
            b'S' => format!("{:02}", now.second()),
            b'U' => format!("{:02}", (now.ordinal() + 6 - wd as u32) / 7),
            b'w' => wd.to_string(),
            b'W' => {
                let shifted = (wd + 6) % 7; // 週一當第一天
                format!("{:02}", (now.ordinal() + 6 - shifted as u32) / 7)
            }
            b'x' => format!(
                "{:02}/{:02}/{:02}",
                now.month(),
                now.day(),
                now.year() % 100
            ),
            b'X' => format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second()),
            b'y' => format!("{:02}", now.year() % 100),
            b'Y' => now.year().to_string(),
            b'z' => now.format("%z").to_string(),
            // C 的 `%Z` 是時區名稱；跨平台拿不到一致的名字 → 用偏移（文件有寫）
            b'Z' => now.format("%z").to_string(),
            b'%' => "%".to_string(),
            _ => return None,
        };
        out.push_str(&piece);
        i = j + 1;
    }
    Some(out)
}

/// 小亂數器（xorshift64*，種子取自系統時間）。巨集的 `random` 不需要更好的。
fn next_rand() -> u32 {
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};
    thread_local! {
        static STATE: Cell<u64> = const { Cell::new(0) };
    }
    STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x2545F4914F6CDD1D)
                | 1;
        }
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        s.set(x);
        (x.wrapping_mul(0x2545F491_4F6CDD1D) >> 32) as u32
    })
}

#[cfg(test)]
mod tests {
    use super::super::exec::run_text;
    use super::*;

    fn run(src: &str) -> super::super::vars::Vars {
        run_text(src).expect("巨集應該跑得完")
    }

    fn err_of(src: &str) -> Err {
        run_text(src).expect_err("應該要出錯").err
    }

    /// `strlen` 算的是**位元組**（中文一個字 3 個）。
    #[test]
    fn strlen_counts_bytes() {
        let v = run("strlen 'abc'\na = result");
        assert_eq!(v.int_of("a"), Some(3));
        let v = run("strlen '中文'\na = result");
        assert_eq!(v.int_of("a"), Some(6), "UTF-8 的中文一個字 3 個位元組");
    }

    #[test]
    fn strcompare_returns_minus_one_zero_one() {
        let v = run("strcompare 'a' 'b'\nx = result\nstrcompare 'b' 'a'\ny = result\nstrcompare 'a' 'a'\nz = result");
        assert_eq!(v.int_of("x"), Some(-1));
        assert_eq!(v.int_of("y"), Some(1));
        assert_eq!(v.int_of("z"), Some(0));
    }

    #[test]
    fn strconcat_and_strcopy() {
        let v = run("s = 'ab'\nstrconcat s 'cd'");
        assert_eq!(v.str_of("s").unwrap(), b"abcd");
        // strcopy 是 1 起算
        let v = run("strcopy 'abcdef' 2 3 t");
        assert_eq!(v.str_of("t").unwrap(), b"bcd");
        // 位置 < 1 當 1；長度超過就截短
        let v = run("strcopy 'abc' 0 99 t");
        assert_eq!(v.str_of("t").unwrap(), b"abc");
        let v = run("strcopy 'abc' 3 99 t");
        assert_eq!(v.str_of("t").unwrap(), b"c");
        // 負長度當 0。⚠️ 要加括號：參數是**運算式**，`1 -5` 會被讀成 `1-5` ＝ -4
        // （原碼的 `GetIntVal` 也是讀整個運算式，這是寫 TTL 常踩的雷，見 docs/TTL.md）
        let v = run("strcopy 'abc' 1 (-5) t");
        assert_eq!(v.str_of("t").unwrap(), b"");
    }

    #[test]
    fn strscan_is_one_based() {
        let v = run("strscan 'hello' 'll'\na = result\nstrscan 'hello' 'zz'\nb = result\nstrscan '' 'x'\nc = result");
        assert_eq!(v.int_of("a"), Some(3));
        assert_eq!(v.int_of("b"), Some(0));
        assert_eq!(v.int_of("c"), Some(0));
    }

    #[test]
    fn strinsert_and_strremove() {
        let v = run("s = 'abc'\nstrinsert s 2 'XY'");
        assert_eq!(v.str_of("s").unwrap(), b"aXYbc");
        let v = run("s = 'abc'\nstrinsert s 4 'Z'");
        assert_eq!(v.str_of("s").unwrap(), b"abcZ", "位置＝長度+1 是合法的");
        assert_eq!(err_of("s = 'abc'\nstrinsert s 5 'Z'"), Err::Syntax);
        assert_eq!(err_of("s = 'abc'\nstrinsert s 0 'Z'"), Err::Syntax);

        let v = run("s = 'abcdef'\nstrremove s 2 3");
        assert_eq!(v.str_of("s").unwrap(), b"aef");
        assert_eq!(err_of("s = 'abc'\nstrremove s 2 9"), Err::Syntax);
        assert_eq!(err_of("s = 'abc'\nstrremove s 1 0"), Err::Syntax);
    }

    /// `strtrim` 只去頭尾，中間的不動。
    #[test]
    fn strtrim_removes_only_ends() {
        let v = run("s = 'xxaxbxx'\nstrtrim s 'x'");
        assert_eq!(v.str_of("s").unwrap(), b"axb");
        let v = run("s = '  hi  '\nstrtrim s ' '");
        assert_eq!(v.str_of("s").unwrap(), b"hi");
        // 全部都要去掉 → 空字串
        let v = run("s = 'xxx'\nstrtrim s 'x'");
        assert_eq!(v.str_of("s").unwrap(), b"");
    }

    /// `strsplit` → `groupmatchstr1..9` + `result`。
    #[test]
    fn strsplit_fills_group_match() {
        let v = run("strsplit 'a,b,c' ','\nn = result");
        assert_eq!(v.int_of("n"), Some(3));
        assert_eq!(v.str_of("groupmatchstr1").unwrap(), b"a");
        assert_eq!(v.str_of("groupmatchstr2").unwrap(), b"b");
        assert_eq!(v.str_of("groupmatchstr3").unwrap(), b"c");
        assert_eq!(v.str_of("groupmatchstr4").unwrap(), b"", "多的要清空");
        // 連續的分隔字元會切出空字串（原碼刻意不合併）
        let v = run("strsplit 'a,,b' ','\nn = result");
        assert_eq!(v.int_of("n"), Some(3));
        assert_eq!(v.str_of("groupmatchstr2").unwrap(), b"");
        // 分隔字元只能一個字
        assert_eq!(err_of("strsplit 'a,b' ',,'"), Err::Syntax);
        assert_eq!(err_of("strsplit 'a' ',' 0"), Err::Syntax);
        assert_eq!(err_of("strsplit 'a' ',' 10"), Err::Syntax);
    }

    /// `strjoin` 把 `groupmatchstr1..N` 接起來。
    #[test]
    fn strjoin_uses_group_match() {
        let v = run("strsplit 'a,b,c' ','\nstrjoin s '-' 3");
        assert_eq!(v.str_of("s").unwrap(), b"a-b-c");
    }

    /// `strspecial`：`\n` `\t` `\0` `\\` 會換掉，其他反斜線留著。
    #[test]
    fn strspecial_escapes() {
        let v = run(r"s = 'a\nb\tc\\d\qe'");
        assert_eq!(v.str_of("s").unwrap(), br"a\nb\tc\\d\qe".to_vec());
        let v = run("s = 'a\\nb'\nstrspecial s");
        assert_eq!(v.str_of("s").unwrap(), b"a\nb");
        let v = run("strspecial t 'x\\ty'");
        assert_eq!(v.str_of("t").unwrap(), b"x\ty");
        let v = run("strspecial t 'x\\qy'");
        assert_eq!(v.str_of("t").unwrap(), br"x\qy".to_vec(), "不認識的轉義原樣留著");
    }

    /// `tolower`／`toupper` 只動 ASCII（中文不會被弄壞）。
    #[test]
    fn case_conversion_is_ascii_only() {
        let v = run("tolower a 'AbC中文'");
        assert_eq!(v.str_of("a").unwrap(), "abc中文".as_bytes());
        let v = run("toupper b 'AbC中文'");
        assert_eq!(v.str_of("b").unwrap(), "ABC中文".as_bytes());
    }

    #[test]
    fn int2str_and_str2int() {
        let v = run("int2str s 42");
        assert_eq!(v.str_of("s").unwrap(), b"42");
        let v = run("int2str s -7");
        assert_eq!(v.str_of("s").unwrap(), b"-7");

        let v = run("str2int n '123'\nr = result");
        assert_eq!(v.int_of("n"), Some(123));
        assert_eq!(v.int_of("r"), Some(1));
        let v = run("str2int n '$ff'\nr = result");
        assert_eq!(v.int_of("n"), Some(255));
        let v = run("str2int n '0x10'\nr = result");
        assert_eq!(v.int_of("n"), Some(16));
        let v = run("str2int n 'abc'\nr = result");
        assert_eq!(v.int_of("n"), Some(0));
        assert_eq!(v.int_of("r"), Some(0), "失敗時 result＝0");
        let v = run("str2int n '12abc'\nr = result");
        assert_eq!(v.int_of("n"), Some(12), "C 的 sscanf 吃到不是數字就停");
    }

    #[test]
    fn code2str_and_str2code() {
        let v = run("code2str s $41");
        assert_eq!(v.str_of("s").unwrap(), b"A");
        let v = run("code2str s $4142");
        assert_eq!(v.str_of("s").unwrap(), b"AB");
        let v = run("str2code n 'A'");
        assert_eq!(v.int_of("n"), Some(0x41));
        let v = run("str2code n 'AB'");
        assert_eq!(v.int_of("n"), Some(0x4142));
        let v = run("str2code n 'ABCDE'");
        assert_eq!(v.int_of("n"), Some(0x42434445), "只取最後 4 個位元組");
    }

    /// `sprintf` 寫進 `inputstr`、`sprintf2` 寫進指定變數。
    #[test]
    fn sprintf_targets() {
        let v = run("sprintf '%d-%s' 7 'x'");
        assert_eq!(v.str_of("inputstr").unwrap(), b"7-x");
        assert_eq!(v.int_of("result"), Some(0));
        let v = run("sprintf2 s '%d' 42");
        assert_eq!(v.str_of("s").unwrap(), b"42");
    }

    /// 旗標／寬度／精度／`*`／各種轉換。
    #[test]
    fn sprintf_formats() {
        let cases: &[(&str, &str)] = &[
            ("sprintf2 s '%d' 42", "42"),
            ("sprintf2 s '%5d' 42", "   42"),
            ("sprintf2 s '%-5d|' 42", "42   |"),
            ("sprintf2 s '%05d' 42", "00042"),
            ("sprintf2 s '%+d' 42", "+42"),
            ("sprintf2 s '% d' 42", " 42"),
            ("sprintf2 s '%d' -42", "-42"),
            ("sprintf2 s '%05d' -42", "-0042"),
            ("sprintf2 s '%x' 255", "ff"),
            ("sprintf2 s '%X' 255", "FF"),
            ("sprintf2 s '%#x' 255", "0xff"),
            ("sprintf2 s '%o' 8", "10"),
            ("sprintf2 s '%u' -1", "4294967295"),
            ("sprintf2 s '%c' 65", "A"),
            ("sprintf2 s '%s' 'hi'", "hi"),
            ("sprintf2 s '%.2s' 'hello'", "he"),
            ("sprintf2 s '%6.2s|' 'hello'", "    he|"),
            ("sprintf2 s '%*d' 5 42", "   42"),
            ("sprintf2 s '%.*d' 4 42", "0042"),
            ("sprintf2 s '100%%'", "100%"),
            ("sprintf2 s 'a%db' 1", "a1b"),
        ];
        for (src, want) in cases {
            let v = run(src);
            assert_eq!(
                String::from_utf8_lossy(v.str_of("s").unwrap()),
                *want,
                "{src}"
            );
        }
    }

    /// 浮點轉換這一批沒實作 → `result=2` + 語法錯誤（見 docs/TTL.md）。
    #[test]
    fn sprintf_float_is_not_implemented() {
        assert_eq!(err_of("sprintf2 s '%f' 1"), Err::Syntax);
    }

    /// `intdim`／`strdim` 與陣列讀寫。
    #[test]
    fn arrays() {
        let v = run("intdim a 3\na[0] = 10\na[2] = a[0] + 5\nx = a[2]");
        assert_eq!(v.int_of("x"), Some(15));
        let v = run("strdim s 2\ns[0] = 'hi'\nstrconcat s[0] '!'\nt = s[0]");
        assert_eq!(v.str_of("t").unwrap(), b"hi!");
        // 重複宣告、保留字名稱都是語法錯誤
        assert_eq!(err_of("intdim a 3\nintdim a 3"), Err::Syntax);
        assert_eq!(err_of("intdim if 3"), Err::Syntax);
        assert_eq!(err_of("intdim a 0"), Err::Syntax);
        // 索引超範圍
        assert_eq!(err_of("intdim a 2\nx = a[5]"), Err::OutOfRange);
    }

    /// `random` 的範圍是 0～上限（含）。
    #[test]
    fn random_range() {
        for _ in 0..50 {
            let v = run("random n 3");
            let n = v.int_of("n").unwrap();
            assert!((0..=3).contains(&n), "n={n}");
        }
        // 上限 <= 0 是語法錯誤（原碼的 `MaxNum <= 0`）
        assert_eq!(err_of("random n 0"), Err::Syntax);
        assert_eq!(err_of("random n -1"), Err::Syntax);
    }

    /// `rotateleft`／`rotateright`（32-bit 旋轉）。
    #[test]
    fn rotate() {
        let v = run("rotateleft n 1 1");
        assert_eq!(v.int_of("n"), Some(2));
        let v = run("rotateleft n $80000000 1");
        assert_eq!(v.int_of("n"), Some(1), "最高位轉到最低位");
        let v = run("rotateright n 1 1");
        assert_eq!(v.int_of("n"), Some(i32::MIN));
        let v = run("rotateleft n 5 32");
        assert_eq!(v.int_of("n"), Some(5), "轉 32 位＝不動");
    }

    /// `gettime`／`getdate` 的預設格式。
    #[test]
    fn get_time_defaults() {
        let v = run("getdate d\ngettime t");
        let d = String::from_utf8_lossy(v.str_of("d").unwrap()).into_owned();
        let t = String::from_utf8_lossy(v.str_of("t").unwrap()).into_owned();
        assert_eq!(d.len(), 10, "%Y-%m-%d：{d}");
        assert_eq!(d.as_bytes()[4], b'-');
        assert_eq!(t.len(), 8, "%H:%M:%S：{t}");
        assert_eq!(t.as_bytes()[2], b':');
    }

    /// 自訂格式；白名單外的代號 → `result=2`（原碼的 `isInvalidStrftimeChar`）。
    #[test]
    fn get_time_format_whitelist() {
        let v = run("gettime s '%Y/%m/%d'\nr = result");
        let s = String::from_utf8_lossy(v.str_of("s").unwrap()).into_owned();
        assert_eq!(s.len(), 10, "{s}");
        assert_eq!(v.int_of("r"), Some(0));
        // `%F` 不在原碼的白名單裡
        let v = run("gettime s '%F'\nr = result");
        assert_eq!(v.int_of("r"), Some(2));
        // 以 `%` 結尾也不合法
        let v = run("gettime s 'abc%'\nr = result");
        assert_eq!(v.int_of("r"), Some(2));
        // 第三個參數（時區）沒實作 → result=2
        let v = run("gettime s '%H' 'JST-9'\nr = result");
        assert_eq!(v.int_of("r"), Some(2));
    }

    /// 環境變數與路徑。
    #[test]
    fn env_and_paths() {
        let v = run("setenv 'AWAY_TTL_TEST' 'hello'\ngetenv 'AWAY_TTL_TEST' s");
        assert_eq!(v.str_of("s").unwrap(), b"hello");
        let v = run("setenv 'AWAY_TTL_TEST2' 'world'\nexpandenv s 'a %AWAY_TTL_TEST2% b'");
        assert_eq!(v.str_of("s").unwrap(), b"a world b");
        let v = run("expandenv s 'x %NO_SUCH_ENV_VAR_XYZ% y'");
        assert_eq!(
            v.str_of("s").unwrap(),
            b"x %NO_SUCH_ENV_VAR_XYZ% y",
            "找不到就原樣留著"
        );

        let v = run(r"basename b 'C:\dir\file.txt'");
        assert_eq!(v.str_of("b").unwrap(), b"file.txt");
        let v = run(r"dirname d 'C:\dir\file.txt'");
        assert_eq!(v.str_of("d").unwrap(), br"C:\dir".to_vec());
        let v = run(r"makepath p 'C:\dir' 'file.txt'");
        assert_eq!(v.str_of("p").unwrap(), br"C:\dir\file.txt".to_vec());
        let v = run(r"makepath p 'C:\dir\' 'file.txt'");
        assert_eq!(
            v.str_of("p").unwrap(),
            br"C:\dir\file.txt".to_vec(),
            "已經有反斜線就不要再加"
        );
    }

    /// `setexitcode`／`getver`。
    #[test]
    fn misc_commands() {
        let mut it = super::super::exec::Interp::from_text("t.ttl", "setexitcode 3").unwrap();
        it.run(100).unwrap();
        assert_eq!(it.exit_code, 3);
        let v = run("getver s");
        assert!(
            String::from_utf8_lossy(v.str_of("s").unwrap()).starts_with("AwayTerminal"),
            "版本字串"
        );
    }

    /// 目標變數**不存在時會自動建**（原碼的 `GetStrVar`／`GetIntVar` 就是這樣，
    /// 所以 `int2str istr i` 不必先宣告 `istr`）。
    #[test]
    fn target_vars_are_auto_created() {
        let v = run("int2str brandnew 5");
        assert_eq!(v.str_of("brandnew").unwrap(), b"5");
        let v = run("str2int alsonew '7'");
        assert_eq!(v.int_of("alsonew"), Some(7));
    }

    /// 型別不符：字串變數不能當整數目標。
    #[test]
    fn target_type_is_checked() {
        assert_eq!(err_of("s = 'x'\nstr2int s '1'"), Err::TypeMismatch);
        assert_eq!(err_of("n = 1\nint2str n 5"), Err::TypeMismatch);
    }
}
