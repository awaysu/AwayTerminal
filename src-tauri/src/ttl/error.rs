//! TTL 的錯誤碼與訊息。
//!
//! 逐條照 `ttpmacro/ttmparse.h`（`Err*` 的編號）與 `ttpmacro/errdlg.cpp` 的 `DispErr`
//! （英文訊息**原字**，連 `Label requiered.` 的拼字錯誤一起保留——那是使用者搜過的字串，
//! 我們不該「順手修好」讓它對不上）。繁中訊息是新版多的，給對話框用。

/// 錯誤碼。編號**就是** `ttmparse.h` 的 `Err*`，方便逐項對照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Err {
    /// `")" expected.`
    CloseParent = 1,
    /// `Can't call sub.`
    CantCall = 2,
    /// `Can't link macro.`
    CantConnect = 3,
    /// `Can't open file.`
    CantOpen = 4,
    /// `Divide by zero.`
    DivByZero = 5,
    /// `Invalid control.`（if／迴圈的配對不對）
    InvalidCtl = 6,
    /// `Label already defined.`
    LabelAlreadyDef = 7,
    /// `Label requiered.`（原碼的拼字錯誤，照抄）
    LabelReq = 8,
    /// `Link macro first. Use 'connect' macro.`
    LinkFirst = 9,
    /// `Stack overflow.`
    StackOver = 10,
    /// `Syntax error.`
    Syntax = 11,
    /// `Too many labels.`
    TooManyLabels = 12,
    /// `Too many variables.`
    TooManyVar = 13,
    /// `Type mismatch.`
    TypeMismatch = 14,
    /// `Variable not initialized.`
    VarNotInit = 15,
    /// `"*/" expected.`
    CloseComment = 16,
    /// `Index out of range.`
    OutOfRange = 17,
    /// `"]" expected.`
    CloseBracket = 18,
    /// `Can't allocate memory.`
    FewMemory = 19,
    /// `Unknown command.`（認得是保留字但這個版本沒實作，也用這個）
    NotSupported = 20,
    /// `Can't execute command.`
    CantExec = 21,
    /// **新增（原碼沒有）**：使用者中斷、分頁關閉或連線斷掉。
    ///
    /// 原碼的中斷是把 `TTLStatus` 直接設成結束、不算錯誤；我們需要一個碼讓
    /// 正在等的指令（`wait`／`pause`）把控制權交回去，執行器再把它當「正常中斷」處理
    /// （不跳錯誤對話框）。編號從 100 起跳，不會和原碼的 1～21 撞。
    Interrupted = 100,
}

impl Err {
    /// `errdlg.cpp` 的英文訊息，**一字不改**。
    pub fn message(self) -> &'static str {
        match self {
            Err::CloseParent => "\")\" expected.",
            Err::CantCall => "Can't call sub.",
            Err::CantConnect => "Can't link macro.",
            Err::CantOpen => "Can't open file.",
            Err::DivByZero => "Divide by zero.",
            Err::InvalidCtl => "Invalid control.",
            Err::LabelAlreadyDef => "Label already defined.",
            // 原碼就是這樣拼的（requiered）
            Err::LabelReq => "Label requiered.",
            Err::LinkFirst => "Link macro first. Use 'connect' macro.",
            Err::StackOver => "Stack overflow.",
            Err::Syntax => "Syntax error.",
            Err::TooManyLabels => "Too many labels.",
            Err::TooManyVar => "Too many variables.",
            Err::TypeMismatch => "Type mismatch.",
            Err::VarNotInit => "Variable not initialized.",
            Err::CloseComment => "\"*/\" expected.",
            Err::OutOfRange => "Index out of range.",
            Err::CloseBracket => "\"]\" expected.",
            Err::FewMemory => "Can't allocate memory.",
            Err::NotSupported => "Unknown command.",
            Err::CantExec => "Can't execute command.",
            Err::Interrupted => "Macro interrupted.",
        }
    }

    /// 繁中訊息（新版多的；對話框用）。
    pub fn message_zh(self) -> &'static str {
        match self {
            Err::CloseParent => "少了對應的「)」。",
            Err::CantCall => "無法呼叫副程式。",
            Err::CantConnect => "無法連結巨集。",
            Err::CantOpen => "開不了檔案。",
            Err::DivByZero => "除以零。",
            Err::InvalidCtl => "流程控制的配對不正確（if／for／while 少了結尾，或結尾多餘）。",
            Err::LabelAlreadyDef => "標籤重複定義。",
            Err::LabelReq => "這裡需要標籤名稱。",
            Err::LinkFirst => "請先用 connect 建立連線。",
            Err::StackOver => "堆疊溢位（call／for／while 疊太深）。",
            Err::Syntax => "語法錯誤。",
            Err::TooManyLabels => "標籤太多。",
            Err::TooManyVar => "變數太多。",
            Err::TypeMismatch => "型別不符（整數與字串不能混用）。",
            Err::VarNotInit => "變數尚未初始化。",
            Err::CloseComment => "少了對應的「*/」。",
            Err::OutOfRange => "索引超出陣列範圍。",
            Err::CloseBracket => "少了對應的「]」。",
            Err::FewMemory => "記憶體不足。",
            Err::NotSupported => "不認識的指令（或這個版本還沒實作）。",
            Err::CantExec => "無法執行指令。",
            Err::Interrupted => "巨集已中斷。",
        }
    }

    /// 照**目前的介面語言**挑一個（英文那份就是原碼 `errdlg.cpp` 的字）。
    ///
    /// 這就是 TASK-015 選「Rust 端自己有一份表」的理由之一：這兩份訊息本來就都在，
    /// 不必為了 i18n 把 22 個錯誤碼送到前端再查一次表。
    pub fn message_for_lang(self) -> &'static str {
        if crate::i18n::is_en() {
            self.message()
        } else {
            self.message_zh()
        }
    }

    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 一個帶位置的錯誤。位置資訊照 `DispErr`：行號、行內容、以及這一段 token 的範圍
/// （`LineParsePtr`～`LinePtr`，對話框會把那一段標起來）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TtlError {
    pub err: Err,
    /// 1 起算的行號（同 `GetLineNo`）。
    pub line_no: usize,
    /// 出錯的那一行原文。
    pub line: String,
    /// 檔名（`include` 進來的檔案會是它自己的名字，同 `GetMacroFileName`）。
    pub file: String,
    /// token 在行內的起迄（位元組位置，0 起算）。
    pub start: usize,
    pub end: usize,
}

impl std::fmt::Display for TtlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 格式參考舊版與 errdlg：訊息 + 檔名:行號 + 該行內容
        write!(
            f,
            "{} ({}:{}) {}",
            self.err.message(),
            if self.file.is_empty() { "macro" } else { &self.file },
            self.line_no,
            self.line.trim_end()
        )
    }
}

impl std::error::Error for TtlError {}

pub type Result<T> = std::result::Result<T, Err>;

#[cfg(test)]
mod tests {
    use super::*;

    /// 錯誤碼的編號要和 `ttmparse.h` 一致（別人照原碼查表時對得上）。
    #[test]
    fn codes_match_ttmparse_h() {
        assert_eq!(Err::CloseParent.code(), 1);
        assert_eq!(Err::DivByZero.code(), 5);
        assert_eq!(Err::Syntax.code(), 11);
        assert_eq!(Err::TypeMismatch.code(), 14);
        assert_eq!(Err::VarNotInit.code(), 15);
        assert_eq!(Err::OutOfRange.code(), 17);
        assert_eq!(Err::NotSupported.code(), 20);
        assert_eq!(Err::CantExec.code(), 21);
    }

    /// 訊息一字不改——包含原碼的拼字錯誤。
    #[test]
    fn messages_are_verbatim() {
        assert_eq!(Err::Syntax.message(), "Syntax error.");
        assert_eq!(Err::CloseParent.message(), "\")\" expected.");
        assert_eq!(
            Err::LabelReq.message(),
            "Label requiered.",
            "原碼就是拼成 requiered，不要修"
        );
        assert_eq!(
            Err::LinkFirst.message(),
            "Link macro first. Use 'connect' macro."
        );
    }
}
