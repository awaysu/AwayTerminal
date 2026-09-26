//! TTL 的運算式——**11 層優先權逐段照 `ttpmacro/ttmparse.cpp`**。
//!
//! | 原碼 | 優先權 | 運算子 | 這裡 |
//! |---|---|---|---|
//! | `GetFactor` | 1 | 常值、變數、`(…)`、單元 `+ - ~ ! not` | [`Eval::factor`] |
//! | `EvalMultiplication` | 2 | `* / %` | [`Eval::multiplication`] |
//! | `EvalAddition` | 3 | `+ -` | [`Eval::addition`] |
//! | `EvalBitShift` | 4 | `<< >> >>>` | [`Eval::bit_shift`] |
//! | `EvalBitAnd` | 5 | `&` / `and` | [`Eval::bit_and`] |
//! | `EvalBitXor` | 6 | `^` / `xor` | [`Eval::bit_xor`] |
//! | `EvalBitOr` | 7 | `\|` / `or` | [`Eval::bit_or`] |
//! | `EvalGreater` | 8 | `< > <= >=` | [`Eval::greater`] |
//! | `EvalEqual` | 9 | `= == <> !=` | [`Eval::equal`] |
//! | `EvalLogicalAnd` | 10 | `&&` | [`Eval::logical_and`] |
//! | `GetExpression` | 11 | `\|\|`（外加原碼寫了但沒有 token 的邏輯 xor） | [`Eval::expression`] |
//!
//! ## 三個容易搞錯的地方（都照原碼）
//!
//! 1. **位元運算比比較運算「緊」**（第 5～7 層 vs 第 8～9 層）——和 C 相反！
//!    所以 `a = 1 and b = 1` 的意思是 `a = (1 and b) = 1`，不是 C 的那種讀法。
//! 2. **`and`／`or`／`xor`／`not` 是位元運算**，邏輯運算要寫 `&& || !`。
//! 3. **整數是 32-bit 有號、溢位環繞**（C 的 `int`）。除以 0 與取模 0 都是 `Divide by zero.`。
//!
//! 另外：字串在運算式裡**只能是單獨一個因子**（原碼在每一層都有
//! `if (Type!=TypInteger) return TRUE;`）。也就是說 `'a' + 'b'` 不是字串相接，
//! 而是「字串後面接了看不懂的東西」——要接字串得用 `strconcat`。

use super::error::{Err, Result};
use super::lex::{Lexer, Op};
use super::vars::{VarType, Vars};
use super::words::Word;

/// 運算式的結果。字串在原碼裡是「變數 id」，我們直接帶值（語意一樣，少一層間接）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Val {
    Int(i32),
    Str(Vec<u8>),
}

impl Val {
    pub fn var_type(&self) -> VarType {
        match self {
            Val::Int(_) => VarType::Integer,
            Val::Str(_) => VarType::String,
        }
    }

    pub fn as_int(&self) -> Result<i32> {
        match self {
            Val::Int(v) => Ok(*v),
            Val::Str(_) => Err(Err::TypeMismatch),
        }
    }

    pub fn as_str(&self) -> Result<&[u8]> {
        match self {
            Val::Str(v) => Ok(v),
            Val::Int(_) => Err(Err::TypeMismatch),
        }
    }
}

/// 32-bit：移位的邊界行為照原碼（`INT_BIT`）。
const INT_BIT: i32 = 32;

pub struct Eval<'a> {
    pub lex: &'a mut Lexer,
    pub vars: &'a Vars,
}

impl<'a> Eval<'a> {
    pub fn new(lex: &'a mut Lexer, vars: &'a Vars) -> Self {
        Self { lex, vars }
    }

    /// `GetExpression`（優先權 11）。回 `Ok(None)` ＝這裡根本沒有運算式。
    pub fn expression(&mut self) -> Result<Option<Val>> {
        let start = self.lex.ptr();
        let first = self.logical_and();
        if first.is_err() {
            self.lex.set_ptr(start); // 原碼：出錯就把 LinePtr 還原到運算式開頭
        }
        let Some(mut left) = first? else {
            self.lex.set_ptr(start);
            return Ok(None);
        };
        // 字串到這裡就結束（原碼：型別不是整數就直接回）
        let mut l = match left {
            Val::Int(v) => v,
            Val::Str(_) => return Ok(Some(left)),
        };
        loop {
            let p = self.lex.ptr();
            let Some(op) = self.lex.operator() else {
                return Ok(Some(Val::Int(l)));
            };
            if op != Op::LOr {
                self.lex.set_ptr(p);
                return Ok(Some(Val::Int(l)));
            }
            let r = self.need_int(|s| s.logical_and(), start)?;
            l = i32::from((l != 0) || (r != 0));
            left = Val::Int(l);
            let _ = &left;
        }
    }

    /// `EvalLogicalAnd`（10）：`&&`
    fn logical_and(&mut self) -> Result<Option<Val>> {
        let Some(first) = self.equal()? else {
            return Ok(None);
        };
        let mut l = match first {
            Val::Int(v) => v,
            Val::Str(_) => return Ok(Some(first)),
        };
        loop {
            let p = self.lex.ptr();
            let Some(op) = self.lex.operator() else {
                return Ok(Some(Val::Int(l)));
            };
            if op != Op::LAnd {
                self.lex.set_ptr(p);
                return Ok(Some(Val::Int(l)));
            }
            let r = self.need_int(|s| s.equal(), p)?;
            l = i32::from((l != 0) && (r != 0));
        }
    }

    /// `EvalEqual`（9）：`=` `==` `<>` `!=`
    fn equal(&mut self) -> Result<Option<Val>> {
        self.binary_level(
            |s| s.greater(),
            &[Op::Eq, Op::Ne],
            |op, a, b| {
                Ok(match op {
                    Op::Eq => i32::from(a == b),
                    _ => i32::from(a != b),
                })
            },
        )
    }

    /// `EvalGreater`（8）：`<` `>` `<=` `>=`
    fn greater(&mut self) -> Result<Option<Val>> {
        self.binary_level(
            |s| s.bit_or(),
            &[Op::Lt, Op::Gt, Op::Le, Op::Ge],
            |op, a, b| {
                Ok(match op {
                    Op::Lt => i32::from(a < b),
                    Op::Gt => i32::from(a > b),
                    Op::Le => i32::from(a <= b),
                    _ => i32::from(a >= b),
                })
            },
        )
    }

    /// `EvalBitOr`（7）：`|` / `or`
    fn bit_or(&mut self) -> Result<Option<Val>> {
        self.binary_level(|s| s.bit_xor(), &[Op::BOr], |_, a, b| Ok(a | b))
    }

    /// `EvalBitXor`（6）：`^` / `xor`
    fn bit_xor(&mut self) -> Result<Option<Val>> {
        self.binary_level(|s| s.bit_and(), &[Op::BXor], |_, a, b| Ok(a ^ b))
    }

    /// `EvalBitAnd`（5）：`&` / `and`
    fn bit_and(&mut self) -> Result<Option<Val>> {
        self.binary_level(|s| s.bit_shift(), &[Op::BAnd], |_, a, b| Ok(a & b))
    }

    /// `EvalBitShift`（4）：`<<` `>>` `>>>`
    ///
    /// 邊界行為照原碼：負的位移量會反向、`>= 32` 會飽和成 0（算術右移負數是 `~0`）。
    fn bit_shift(&mut self) -> Result<Option<Val>> {
        self.binary_level(
            |s| s.addition(),
            &[Op::Shl, Op::Shr, Op::Lshr],
            |op, a, b| {
                let mut n = b;
                if op == Op::Shl {
                    n = n.wrapping_neg(); // 原碼：左移時把位移量取負，之後走同一條梯子
                }
                Ok(if n <= -INT_BIT {
                    0
                } else if n < 0 {
                    a.wrapping_shl((-n) as u32)
                } else if n == 0 {
                    a
                } else if n < INT_BIT {
                    if op == Op::Lshr {
                        ((a as u32) >> n) as i32
                    } else {
                        a >> n
                    }
                } else if a > 0 || op == Op::Lshr {
                    0
                } else {
                    !0
                })
            },
        )
    }

    /// `EvalAddition`（3）：`+` `-`
    fn addition(&mut self) -> Result<Option<Val>> {
        self.binary_level(
            |s| s.multiplication(),
            &[Op::Plus, Op::Minus],
            |op, a, b| {
                Ok(match op {
                    Op::Plus => a.wrapping_add(b),
                    _ => a.wrapping_sub(b),
                })
            },
        )
    }

    /// `EvalMultiplication`（2）：`*` `/` `%`
    ///
    /// ⚠️ 原碼的除零檢查是 `if (Val2 == 0 && WId != RsvMul)`——所以 `%` 也算除零。
    fn multiplication(&mut self) -> Result<Option<Val>> {
        self.binary_level(
            |s| s.factor(),
            &[Op::Mul, Op::Div, Op::Mod],
            |op, a, b| {
                if b == 0 && op != Op::Mul {
                    return Err(Err::DivByZero);
                }
                Ok(match op {
                    Op::Mul => a.wrapping_mul(b),
                    Op::Div => a.wrapping_div(b),
                    _ => a.wrapping_rem(b),
                })
            },
        )
    }

    /// 一層左結合的二元運算（原碼每一層長得一模一樣，抽成一個函式）。
    fn binary_level(
        &mut self,
        mut next: impl FnMut(&mut Self) -> Result<Option<Val>>,
        ops: &[Op],
        apply: impl Fn(Op, i32, i32) -> Result<i32>,
    ) -> Result<Option<Val>> {
        let Some(first) = next(self)? else {
            return Ok(None);
        };
        let mut l = match first {
            Val::Int(v) => v,
            // 原碼：型別不是整數就直接把它回上去，不再看運算子
            Val::Str(_) => return Ok(Some(first)),
        };
        loop {
            let p = self.lex.ptr();
            let Some(op) = self.lex.operator() else {
                return Ok(Some(Val::Int(l)));
            };
            if !ops.contains(&op) {
                self.lex.set_ptr(p);
                return Ok(Some(Val::Int(l)));
            }
            let r = match next(self)? {
                Some(Val::Int(v)) => v,
                Some(Val::Str(_)) => return Err(Err::TypeMismatch),
                None => return Err(Err::Syntax),
            };
            l = apply(op, l, r)?;
        }
    }

    /// 取下一層並要求是整數（給 `&&`／`||` 那兩層用：它們的錯誤處理和別層略有不同）。
    fn need_int(
        &mut self,
        mut next: impl FnMut(&mut Self) -> Result<Option<Val>>,
        restore: usize,
    ) -> Result<i32> {
        match next(self) {
            Ok(Some(Val::Int(v))) => Ok(v),
            Ok(Some(Val::Str(_))) => {
                self.lex.set_ptr(restore);
                Err(Err::TypeMismatch)
            }
            Ok(None) => {
                self.lex.set_ptr(restore);
                Err(Err::Syntax)
            }
            Err(e) => {
                self.lex.set_ptr(restore);
                Err(e)
            }
        }
    }

    /// `GetFactor`（1）：識別字（變數／單元運算子的字詞形式）、數字、字串、`(…)`、單元運算子。
    fn factor(&mut self) -> Result<Option<Val>> {
        let p = self.lex.ptr();
        let r = self.factor_inner();
        if r.is_err() {
            self.lex.set_ptr(p); // 原碼：出錯就把 LinePtr 還原
        }
        r
    }

    fn factor_inner(&mut self) -> Result<Option<Val>> {
        // 1) 識別字：保留字（只能是單元運算子）或變數
        let p = self.lex.ptr();
        if let Some(name) = self.lex.identifier() {
            if let Some(w) = super::words::check_reserved_word(&name) {
                // `not x`：原碼只認 BNot／LNot，其餘保留字在運算式裡是語法錯誤
                let inner = match self.factor()? {
                    Some(v) => v.as_int()?,
                    None => return Err(Err::Syntax),
                };
                return match w {
                    Word::BNot => Ok(Some(Val::Int(!inner))),
                    _ => Err(Err::Syntax),
                };
            }
            // 變數
            return match self.vars.find(&name) {
                None => Err(Err::VarNotInit),
                Some(v) => match v.var_type() {
                    VarType::Integer => Ok(Some(Val::Int(match v {
                        super::vars::Value::Int(i) => *i,
                        _ => unreachable!(),
                    }))),
                    VarType::String => Ok(Some(Val::Str(match v {
                        super::vars::Value::Str(s) => s.clone(),
                        _ => unreachable!(),
                    }))),
                    VarType::IntArray => match self.index()? {
                        Some(i) => Ok(Some(Val::Int(self.vars.int_at(&name, i)?))),
                        // 原碼：陣列沒帶 [] 時把「變數 id」當值——我們沒有 id 的概念，
                        // 這種寫法只在 strsplit 這類「傳陣列」的指令才有意義（那是第二批）。
                        None => Err(Err::TypeMismatch),
                    },
                    VarType::StrArray => match self.index()? {
                        Some(i) => Ok(Some(Val::Str(self.vars.str_at(&name, i)?.to_vec()))),
                        None => Err(Err::TypeMismatch),
                    },
                    VarType::Label | VarType::Unknown => Err(Err::TypeMismatch),
                },
            };
        }
        self.lex.set_ptr(p);

        // 2) 數字
        if let Some(n) = self.lex.number() {
            return Ok(Some(Val::Int(n)));
        }

        // 3) 字串常值（原碼的 GetFactor 沒有這條——字串是由 GetStrVal 處理的。
        //    但 `GetVarType`／賦值那條路會先試 GetString，所以這裡也接受，
        //    語意等於「字串因子」，之後任何運算子都會讓它變成型別不符。）
        if let Some(s) = self.lex.string()? {
            return Ok(Some(Val::Str(s)));
        }

        // 4) 單元運算子
        let p = self.lex.ptr();
        if let Some(op) = self.lex.operator() {
            let inner = match self.factor()? {
                Some(v) => v.as_int()?,
                None => return Err(Err::Syntax),
            };
            return match op {
                Op::Plus => Ok(Some(Val::Int(inner))),
                Op::Minus => Ok(Some(Val::Int(inner.wrapping_neg()))),
                Op::BNot => Ok(Some(Val::Int(!inner))),
                Op::LNot => Ok(Some(Val::Int(i32::from(inner == 0)))),
                _ => Err(Err::Syntax),
            };
        }
        self.lex.set_ptr(p);

        // 5) 括號
        if self.lex.first_char() == b'(' {
            let v = match self.expression()? {
                Some(v) => v,
                None => return Err(Err::Syntax),
            };
            if self.lex.first_char() != b')' {
                return Err(Err::CloseParent);
            }
            return Ok(Some(v));
        }
        self.lex.set_ptr(p);

        Ok(None)
    }

    /// `GetIndex`：`[ 運算式 ]`。沒有 `[` 就回 `None`。
    pub fn index(&mut self) -> Result<Option<i32>> {
        let p = self.lex.ptr();
        if self.lex.first_char() != b'[' {
            self.lex.set_ptr(p);
            return Ok(None);
        }
        let v = match self.expression()? {
            Some(v) => v.as_int()?,
            None => return Err(Err::Syntax),
        };
        if self.lex.first_char() != b']' {
            return Err(Err::CloseBracket);
        }
        Ok(Some(v))
    }
}

/// 方便的入口：算一個整數運算式（原碼 `GetIntVal`）。
pub fn int_val(lex: &mut Lexer, vars: &Vars) -> Result<i32> {
    lex.mark_token_start();
    match Eval::new(lex, vars).expression()? {
        Some(v) => v.as_int(),
        None => Err(Err::Syntax),
    }
}

/// 算一個字串值（原碼 `GetStrVal`：字串常值或字串變數）。
pub fn str_val(lex: &mut Lexer, vars: &Vars) -> Result<Vec<u8>> {
    lex.mark_token_start();
    match Eval::new(lex, vars).expression()? {
        Some(Val::Str(s)) => Ok(s),
        Some(Val::Int(_)) => Err(Err::TypeMismatch),
        None => Err(Err::Syntax),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> Result<i32> {
        let mut vars = Vars::with_system_vars();
        vars.new_int("a", 2);
        vars.new_int("b", 3);
        vars.new_str("s", b"hi");
        let mut lex = Lexer::new(src.as_bytes());
        int_val(&mut lex, &vars)
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(eval("1+2*3"), Ok(7));
        assert_eq!(eval("(1+2)*3"), Ok(9));
        assert_eq!(eval("7/2"), Ok(3), "整數除法");
        assert_eq!(eval("-7/2"), Ok(-3), "C 的除法是往零取整");
        assert_eq!(eval("7%3"), Ok(1));
        assert_eq!(eval("-7%3"), Ok(-1), "C 的 % 跟著被除數的號");
        assert_eq!(eval("a+b*2"), Ok(8));
    }

    /// 除以 0 與取模 0 都是 `Divide by zero.`（原碼的條件是 `Val2==0 && op!=Mul`）。
    #[test]
    fn divide_by_zero() {
        assert_eq!(eval("1/0"), Err(Err::DivByZero));
        assert_eq!(eval("1%0"), Err(Err::DivByZero));
        assert_eq!(eval("1*0"), Ok(0), "乘 0 當然沒事");
    }

    /// ⚠️ **位元運算比比較運算緊**——和 C 相反。
    #[test]
    fn bitwise_binds_tighter_than_comparison() {
        // C 的讀法是 (1) = (1 & 1) → 1；TTL 的讀法是 (1 = 1) & 1 嗎？不是！
        // TTL：`&` 在第 5 層、`=` 在第 9 層 → 先算 `1 & 1`，再比 `1 = 1` → 1
        assert_eq!(eval("1 = 1 & 1"), Ok(1));
        // 這個例子才看得出差別：`2 & 1` ＝ 0，所以 `0 = 0` ＝ 1
        assert_eq!(eval("0 = 2 & 1"), Ok(1));
        // 如果 `=` 比 `&` 緊（C 的規則）就會是 `(0=2) & 1` ＝ 0
    }

    /// 字詞運算子是**位元**運算，不是邏輯運算。
    #[test]
    fn word_operators_are_bitwise() {
        assert_eq!(eval("2 and 1"), Ok(0), "位元 and：2&1＝0");
        assert_eq!(eval("2 && 1"), Ok(1), "邏輯 and：兩個都非 0");
        assert_eq!(eval("2 or 1"), Ok(3), "位元 or");
        assert_eq!(eval("2 || 1"), Ok(1), "邏輯 or");
        assert_eq!(eval("3 xor 1"), Ok(2));
        assert_eq!(eval("not 0"), Ok(-1), "位元 not：~0＝-1");
        assert_eq!(eval("!0"), Ok(1), "邏輯 not");
    }

    /// 比較運算的結果是 0／1。
    #[test]
    fn comparisons() {
        assert_eq!(eval("1<2"), Ok(1));
        assert_eq!(eval("2<=2"), Ok(1));
        assert_eq!(eval("3>4"), Ok(0));
        assert_eq!(eval("3>=4"), Ok(0));
        assert_eq!(eval("1=1"), Ok(1));
        assert_eq!(eval("1==1"), Ok(1));
        assert_eq!(eval("1<>2"), Ok(1));
        assert_eq!(eval("1!=2"), Ok(1));
    }

    /// 移位的邊界行為（逐條照原碼那個梯子）。
    #[test]
    fn shifts_match_the_original_ladder() {
        assert_eq!(eval("1<<4"), Ok(16));
        assert_eq!(eval("256>>4"), Ok(16));
        assert_eq!(eval("-16>>2"), Ok(-4), "算術右移保留符號");
        assert_eq!(eval("-16>>>28"), Ok(15), "邏輯右移把符號位當資料");
        assert_eq!(eval("1<<32"), Ok(0), ">=32 飽和成 0");
        assert_eq!(eval("-1>>32"), Ok(-1), "算術右移負數飽和成 ~0");
        assert_eq!(eval("-1>>>32"), Ok(0));
        assert_eq!(eval("1<<-1"), Ok(0), "負的位移量＝反向（1>>1）");
        assert_eq!(eval("8>>-1"), Ok(16), "負的右移＝左移");
    }

    /// 32-bit 環繞（C 的 int）。
    #[test]
    fn integers_wrap_at_32_bits() {
        assert_eq!(eval("2147483647+1"), Ok(i32::MIN));
        assert_eq!(eval("$FFFFFFFF"), Ok(-1));
    }

    /// 單元運算子。
    #[test]
    fn unary_operators() {
        assert_eq!(eval("-5"), Ok(-5));
        assert_eq!(eval("+5"), Ok(5));
        assert_eq!(eval("~0"), Ok(-1));
        assert_eq!(eval("!5"), Ok(0));
        assert_eq!(eval("- -5"), Ok(5));
    }

    /// 沒初始化的變數 → `Variable not initialized.`
    #[test]
    fn undefined_variable() {
        assert_eq!(eval("nosuchvar+1"), Err(Err::VarNotInit));
    }

    /// 字串不能參與整數運算。
    #[test]
    fn strings_are_not_integers() {
        assert_eq!(eval("s"), Err(Err::TypeMismatch));
        assert_eq!(eval("s+1"), Err(Err::TypeMismatch));
        assert_eq!(eval("1+s"), Err(Err::TypeMismatch));
    }

    /// 括號沒收尾 → `")" expected.`
    #[test]
    fn missing_close_paren() {
        assert_eq!(eval("(1+2"), Err(Err::CloseParent));
    }

    /// 字串值：常值、相鄰片段、字串變數。
    #[test]
    fn string_values() {
        let mut vars = Vars::with_system_vars();
        vars.new_str("s", b"hi");
        // ⚠️ 片段要**緊貼**才會相接（原碼 GetString 看的是收尾引號的下一個字元）
        let mut lex = Lexer::new(b"'ab'#33");
        assert_eq!(str_val(&mut lex, &vars).unwrap(), b"ab!");
        let mut lex = Lexer::new(b"'ab' #33");
        assert_eq!(
            str_val(&mut lex, &vars).unwrap(),
            b"ab",
            "中間有空白就不是同一個字串常值（#33 會留給下一個參數）"
        );
        let mut lex = Lexer::new(b"s");
        assert_eq!(str_val(&mut lex, &vars).unwrap(), b"hi");
        let mut lex = Lexer::new(b"1");
        assert_eq!(str_val(&mut lex, &vars), Err(Err::TypeMismatch));
    }

    /// 陣列元素。
    #[test]
    fn array_elements() {
        let mut vars = Vars::with_system_vars();
        vars.new_int_array("a", 3).unwrap();
        vars.set_int_at("a", 1, 42).unwrap();
        vars.new_str_array("s", 2).unwrap();
        vars.set_str_at("s", 0, b"zero").unwrap();

        let mut lex = Lexer::new(b"a[1]+1");
        assert_eq!(int_val(&mut lex, &vars), Ok(43));
        let mut lex = Lexer::new(b"a[3]");
        assert_eq!(int_val(&mut lex, &vars), Err(Err::OutOfRange));
        let mut lex = Lexer::new(b"s[0]");
        assert_eq!(str_val(&mut lex, &vars).unwrap(), b"zero");
        // 索引也可以是運算式
        let mut lex = Lexer::new(b"a[0+1]");
        assert_eq!(int_val(&mut lex, &vars), Ok(42));
        // `]` 忘了
        let mut lex = Lexer::new(b"a[1");
        assert_eq!(int_val(&mut lex, &vars), Err(Err::CloseBracket));
    }
}
