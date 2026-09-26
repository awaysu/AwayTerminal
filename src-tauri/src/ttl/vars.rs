//! TTL 的變數表——照 `ttpmacro/ttmparse.cpp` 的 `Variables[]` 那一組函式。
//!
//! | 原碼 | 這裡 |
//! |---|---|
//! | `CheckVar` | [`Vars::find`] |
//! | `NewIntVar` / `NewStrVar` | [`Vars::new_int`] / [`Vars::new_str`] |
//! | `NewIntAryVar` / `NewStrAryVar` | [`Vars::new_int_array`] / [`Vars::new_str_array`] |
//! | `NewLabVar` / `DelLabVar` / `CopyLabel` | [`Vars::new_label`] / [`Vars::drop_labels_of_level`] / [`Vars::label`] |
//! | `SetIntVal` / `CopyIntVal` / `SetStrVal` / `StrVarPtr` | 同名方法 |
//! | `TVariableType` | [`VarType`] |
//!
//! 三件照原碼的重點：
//!
//! 1. **名稱大小寫不敏感**（`_stricmp`）。
//! 2. **標籤和變數共用同一個名稱空間**：`:foo` 之後再 `foo = 1` 會是
//!    `Label already defined.`／型別不符，不是兩個不同的東西。
//! 3. 字串值是**位元組**（`char[512]`），不是 UTF-8 字串；長度上限 511。

use std::collections::HashMap;

use super::error::{Err, Result};
use super::lex::MAX_STR_LEN;

/// 變數型別（`TVariableType`，編號照原碼）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VarType {
    Unknown = 0,
    Integer = 1,
    String = 3,
    Label = 4,
    IntArray = 5,
    StrArray = 6,
}

/// 一個變數的值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Int(i32),
    /// 位元組（上限 511 + 結尾，同 `TStrVal`）。
    Str(Vec<u8>),
    /// 標籤：跳到哪一行（行號索引）＋屬於哪一層 include（原碼是 buffer 位置＋level）。
    Label { line: usize, level: usize },
    IntArray(Vec<i32>),
    StrArray(Vec<Vec<u8>>),
}

impl Value {
    pub fn var_type(&self) -> VarType {
        match self {
            Value::Int(_) => VarType::Integer,
            Value::Str(_) => VarType::String,
            Value::Label { .. } => VarType::Label,
            Value::IntArray(_) => VarType::IntArray,
            Value::StrArray(_) => VarType::StrArray,
        }
    }
}

/// 變數表。
#[derive(Debug, Default)]
pub struct Vars {
    /// key ＝小寫過的名稱（大小寫不敏感）；值裡保留使用者寫的原名，錯誤訊息才好看。
    map: HashMap<String, (String, Value)>,
    /// 插入順序（`ifdefined` 之類要穩定的行為時用得到，也方便測試）。
    order: Vec<String>,
}

/// 字串截到上限（`MaxStrLen-1`），同原碼的 `_TRUNCATE`。
pub fn truncate(mut s: Vec<u8>) -> Vec<u8> {
    s.truncate(MAX_STR_LEN - 1);
    s
}

impl Vars {
    pub fn new() -> Self {
        Self::default()
    }

    /// 建好系統變數（照 `ttl.cpp` 的 `InitVar` 順序）。
    ///
    /// `param1..N`／`paramcnt` 由呼叫端依實際參數補（第二批的執行入口才會用到）。
    pub fn with_system_vars() -> Self {
        let mut v = Self::new();
        v.new_int("result", 0);
        v.new_int("timeout", 0);
        v.new_int("mtimeout", 0);
        v.new_str("inputstr", b"");
        v.new_str("matchstr", b"");
        for i in 1..=9 {
            v.new_str(&format!("groupmatchstr{i}"), b"");
        }
        v
    }

    fn key(name: &str) -> String {
        name.to_ascii_lowercase()
    }

    /// `CheckVar`：有這個變數嗎。
    pub fn find(&self, name: &str) -> Option<&Value> {
        self.map.get(&Self::key(name)).map(|(_, v)| v)
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.map.get_mut(&Self::key(name)).map(|(_, v)| v)
    }

    pub fn var_type(&self, name: &str) -> VarType {
        self.find(name).map(|v| v.var_type()).unwrap_or(VarType::Unknown)
    }

    pub fn names(&self) -> &[String] {
        &self.order
    }

    fn insert(&mut self, name: &str, value: Value) {
        let k = Self::key(name);
        if !self.map.contains_key(&k) {
            self.order.push(k.clone());
        }
        self.map.insert(k, (name.to_string(), value));
    }

    pub fn new_int(&mut self, name: &str, val: i32) {
        self.insert(name, Value::Int(val));
    }

    pub fn new_str(&mut self, name: &str, val: &[u8]) {
        self.insert(name, Value::Str(truncate(val.to_vec())));
    }

    /// `NewIntAryVar`：大小 <= 0 是語法錯誤（原碼回 `ErrSyntax`）。
    pub fn new_int_array(&mut self, name: &str, size: i32) -> Result<()> {
        if size <= 0 {
            return Err(Err::Syntax);
        }
        self.insert(name, Value::IntArray(vec![0; size as usize]));
        Ok(())
    }

    pub fn new_str_array(&mut self, name: &str, size: i32) -> Result<()> {
        if size <= 0 {
            return Err(Err::Syntax);
        }
        self.insert(name, Value::StrArray(vec![Vec::new(); size as usize]));
        Ok(())
    }

    /// `NewLabVar`。重複定義由呼叫端先用 [`Vars::find`] 檢查（同原碼）。
    pub fn new_label(&mut self, name: &str, line: usize, level: usize) {
        self.insert(name, Value::Label { line, level });
    }

    /// `CopyLabel`。
    pub fn label(&self, name: &str) -> Option<(usize, usize)> {
        match self.find(name) {
            Some(Value::Label { line, level }) => Some((*line, *level)),
            _ => None,
        }
    }

    /// `DelLabVar`：某一層 include 收掉時，把那一層的標籤一起清掉。
    pub fn drop_labels_of_level(&mut self, level: usize) {
        let drop: Vec<String> = self
            .map
            .iter()
            .filter(|(_, (_, v))| matches!(v, Value::Label { level: l, .. } if *l >= level))
            .map(|(k, _)| k.clone())
            .collect();
        for k in drop {
            self.map.remove(&k);
            self.order.retain(|x| x != &k);
        }
    }

    // ---------------------------------------------------------------- 讀寫

    pub fn int_of(&self, name: &str) -> Option<i32> {
        match self.find(name) {
            Some(Value::Int(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn str_of(&self, name: &str) -> Option<&[u8]> {
        match self.find(name) {
            Some(Value::Str(v)) => Some(v),
            _ => None,
        }
    }

    pub fn set_int(&mut self, name: &str, val: i32) {
        match self.find_mut(name) {
            Some(Value::Int(v)) => *v = val,
            _ => self.new_int(name, val),
        }
    }

    pub fn set_str(&mut self, name: &str, val: &[u8]) {
        let val = truncate(val.to_vec());
        match self.find_mut(name) {
            Some(Value::Str(v)) => *v = val,
            _ => self.insert(name, Value::Str(val)),
        }
    }

    /// `SetResult`：只有在 `result` 存在且是整數時才寫（同原碼）。
    pub fn set_result(&mut self, val: i32) {
        if let Some(Value::Int(v)) = self.find_mut("result") {
            *v = val;
        }
    }

    /// `SetGroupMatchStr`。
    pub fn set_group_match(&mut self, no: usize, val: &[u8]) {
        let name = format!("groupmatchstr{no}");
        let val = truncate(val.to_vec());
        if let Some(Value::Str(v)) = self.find_mut(&name) {
            *v = val;
        }
    }

    /// 陣列元素（整數）。索引超出範圍 → `ErrOutOfRange`。
    pub fn int_at(&self, name: &str, index: i32) -> Result<i32> {
        match self.find(name) {
            Some(Value::IntArray(a)) => a
                .get(usize::try_from(index).map_err(|_| Err::OutOfRange)?)
                .copied()
                .ok_or(Err::OutOfRange),
            _ => Err(Err::TypeMismatch),
        }
    }

    pub fn set_int_at(&mut self, name: &str, index: i32, val: i32) -> Result<()> {
        match self.find_mut(name) {
            Some(Value::IntArray(a)) => {
                let i = usize::try_from(index).map_err(|_| Err::OutOfRange)?;
                *a.get_mut(i).ok_or(Err::OutOfRange)? = val;
                Ok(())
            }
            _ => Err(Err::TypeMismatch),
        }
    }

    pub fn str_at(&self, name: &str, index: i32) -> Result<&[u8]> {
        match self.find(name) {
            Some(Value::StrArray(a)) => a
                .get(usize::try_from(index).map_err(|_| Err::OutOfRange)?)
                .map(|v| v.as_slice())
                .ok_or(Err::OutOfRange),
            _ => Err(Err::TypeMismatch),
        }
    }

    pub fn set_str_at(&mut self, name: &str, index: i32, val: &[u8]) -> Result<()> {
        let val = truncate(val.to_vec());
        match self.find_mut(name) {
            Some(Value::StrArray(a)) => {
                let i = usize::try_from(index).map_err(|_| Err::OutOfRange)?;
                *a.get_mut(i).ok_or(Err::OutOfRange)? = val;
                Ok(())
            }
            _ => Err(Err::TypeMismatch),
        }
    }

    pub fn array_len(&self, name: &str) -> Option<usize> {
        match self.find(name) {
            Some(Value::IntArray(a)) => Some(a.len()),
            Some(Value::StrArray(a)) => Some(a.len()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 名稱大小寫不敏感（原碼 `_stricmp`）。
    #[test]
    fn names_are_case_insensitive() {
        let mut v = Vars::new();
        v.new_int("Foo", 1);
        assert_eq!(v.int_of("foo"), Some(1));
        assert_eq!(v.int_of("FOO"), Some(1));
        v.set_int("fOo", 2);
        assert_eq!(v.int_of("Foo"), Some(2));
        assert_eq!(v.names().len(), 1, "不可以變成兩個變數");
    }

    /// 系統變數清單照 `InitVar`。
    #[test]
    fn system_vars_match_init_var() {
        let v = Vars::with_system_vars();
        assert_eq!(v.var_type("result"), VarType::Integer);
        assert_eq!(v.var_type("timeout"), VarType::Integer);
        assert_eq!(v.var_type("mtimeout"), VarType::Integer);
        assert_eq!(v.var_type("inputstr"), VarType::String);
        assert_eq!(v.var_type("matchstr"), VarType::String);
        for i in 1..=9 {
            assert_eq!(v.var_type(&format!("groupmatchstr{i}")), VarType::String);
        }
        assert_eq!(v.var_type("nosuch"), VarType::Unknown);
    }

    /// 字串是位元組、上限 511。
    #[test]
    fn strings_are_bytes_and_truncated() {
        let mut v = Vars::new();
        v.new_str("s", "中文".as_bytes());
        assert_eq!(v.str_of("s").unwrap(), "中文".as_bytes());
        v.new_str("long", &vec![b'x'; 600]);
        assert_eq!(v.str_of("long").unwrap().len(), MAX_STR_LEN - 1);
    }

    /// 陣列：索引超範圍是 `Index out of range.`，不是 panic。
    #[test]
    fn array_index_out_of_range() {
        let mut v = Vars::new();
        v.new_int_array("a", 3).unwrap();
        assert_eq!(v.int_at("a", 0), Ok(0));
        v.set_int_at("a", 2, 7).unwrap();
        assert_eq!(v.int_at("a", 2), Ok(7));
        assert_eq!(v.int_at("a", 3), Err(Err::OutOfRange));
        assert_eq!(v.int_at("a", -1), Err(Err::OutOfRange));
        assert_eq!(v.set_int_at("a", 9, 1), Err(Err::OutOfRange));
    }

    /// `intdim 0` 是語法錯誤（原碼的 `NewIntAryVar` 對 size<=0 回錯）。
    #[test]
    fn zero_sized_array_is_syntax_error() {
        let mut v = Vars::new();
        assert_eq!(v.new_int_array("a", 0), Err(Err::Syntax));
        assert_eq!(v.new_str_array("b", -1), Err(Err::Syntax));
    }

    /// `set_result` 只在 `result` 是整數時才寫（沒有 result 就什麼都不做）。
    #[test]
    fn set_result_needs_result_var() {
        let mut v = Vars::new();
        v.set_result(5); // 沒有 result → 不建立
        assert_eq!(v.var_type("result"), VarType::Unknown);
        v.new_int("result", 0);
        v.set_result(5);
        assert_eq!(v.int_of("result"), Some(5));
    }

    /// 標籤和變數共用名稱空間（原碼是同一張表）。
    #[test]
    fn labels_share_the_namespace() {
        let mut v = Vars::new();
        v.new_label("top", 3, 0);
        assert_eq!(v.label("top"), Some((3, 0)));
        assert_eq!(v.var_type("top"), VarType::Label);
        assert!(v.find("top").is_some(), "同名變數會被視為已定義");
    }

    /// include 收掉那一層時，那一層的標籤要一起消失。
    #[test]
    fn labels_die_with_their_level() {
        let mut v = Vars::new();
        v.new_label("a", 1, 0);
        v.new_label("b", 2, 1);
        v.drop_labels_of_level(1);
        assert!(v.label("a").is_some());
        assert!(v.label("b").is_none());
    }
}
