//! TTL 的保留字表。
//!
//! **整張表逐字照 `ttpmacro/ttmparse.cpp` 的 `CheckReservedWord`**（211 個名稱，
//! 由原碼直接抽出來生成，避免手抄錯字）。比對是**大小寫不敏感**的（原碼用 `_stricmp`）。
//!
//! 「認得但這一批還沒實作」的指令對映到 [`Word::Unsupported`]：這樣它們仍然是保留字
//! （不會被當成變數名），錯誤是 `Unknown command.`（`ErrNotSupported`）而不是語法錯誤——
//! 和原碼對不認識指令的行為一致。
//!
//! 第二批（TASK-013）要接的是 `send`／`wait`／`connect`／對話框／檔案／正規表示式那些。

/// 一個保留字。變體只列**這一批實作的**，其餘一律 [`Word::Unsupported`]（但仍是保留字）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    BAnd,
    BNot,
    BOr,
    BXor,
    Basename,
    Break,
    Call,
    Code2Str,
    Continue,
    Dirname,
    Do,
    Else,
    ElseIf,
    End,
    EndIf,
    EndUntil,
    EndWhile,
    Exit,
    ExpandEnv,
    For,
    GetDate,
    GetEnv,
    GetTime,
    GetVer,
    Goto,
    If,
    IfDefined,
    Include,
    Int2Str,
    IntDim,
    Loop,
    MakePath,
    Next,
    Random,
    Return,
    RotateL,
    RotateR,
    SetEnv,
    SetExitCode,
    Sprintf,
    Sprintf2,
    Str2Code,
    Str2Int,
    StrCompare,
    StrConcat,
    StrCopy,
    StrDim,
    StrInsert,
    StrJoin,
    StrLen,
    StrRemove,
    StrScan,
    StrSpecial,
    StrSplit,
    StrTrim,
    Then,
    ToLower,
    ToUpper,
    Until,
    While,
    /// 認得是保留字，但這個版本還沒實作（`ErrNotSupported`）。名稱留著給錯誤訊息用。
    Unsupported(&'static str),
}

impl Word {
    /// 這個字是不是「運算子」（原碼：`WordId >= RsvOperator`，1000 以上）。
    /// `GetOperator` 的字詞形式只接受這一類。
    pub fn is_operator(self) -> bool {
        matches!(self, Word::BAnd | Word::BOr | Word::BXor | Word::BNot)
    }

    /// 給錯誤訊息用的名稱。
    pub fn name(self) -> &'static str {
        match self {
            Word::Unsupported(n) => n,
            _ => LOOKUP
                .iter()
                .find(|(_, w)| *w == self)
                .map(|(n, _)| *n)
                .unwrap_or("?"),
        }
    }
}

/// 名稱 → 保留字。**順序照原碼抽出的順序**，方便逐項對照。
pub static LOOKUP: &[(&str, Word)] = &[
    ("and", Word::BAnd),
    ("beep", Word::Unsupported("beep")),
    ("bplusrecv", Word::Unsupported("bplusrecv")),
    ("bplussend", Word::Unsupported("bplussend")),
    ("break", Word::Break),
    ("bringupbox", Word::Unsupported("bringupbox")),
    ("basename", Word::Basename),
    ("call", Word::Call),
    ("callmenu", Word::Unsupported("callmenu")),
    ("changedir", Word::Unsupported("changedir")),
    ("checksum8", Word::Unsupported("checksum8")),
    ("checksum8file", Word::Unsupported("checksum8file")),
    ("checksum16", Word::Unsupported("checksum16")),
    ("checksum16file", Word::Unsupported("checksum16file")),
    ("checksum32", Word::Unsupported("checksum32")),
    ("checksum32file", Word::Unsupported("checksum32file")),
    ("clearscreen", Word::Unsupported("clearscreen")),
    ("clipb2var", Word::Unsupported("clipb2var")),
    ("closesbox", Word::Unsupported("closesbox")),
    ("closett", Word::Unsupported("closett")),
    ("code2str", Word::Code2Str),
    ("connect", Word::Unsupported("connect")),
    ("continue", Word::Continue),
    ("crc16", Word::Unsupported("crc16")),
    ("crc16file", Word::Unsupported("crc16file")),
    ("crc32", Word::Unsupported("crc32")),
    ("crc32file", Word::Unsupported("crc32file")),
    ("cygconnect", Word::Unsupported("cygconnect")),
    ("delpassword", Word::Unsupported("delpassword")),
    ("delpassword2", Word::Unsupported("delpassword2")),
    ("disconnect", Word::Unsupported("disconnect")),
    ("dispstr", Word::Unsupported("dispstr")),
    ("do", Word::Do),
    ("dirname", Word::Dirname),
    ("else", Word::Else),
    ("elseif", Word::ElseIf),
    ("enablekeyb", Word::Unsupported("enablekeyb")),
    ("end", Word::End),
    ("endif", Word::EndIf),
    ("enduntil", Word::EndUntil),
    ("endwhile", Word::EndWhile),
    ("exec", Word::Unsupported("exec")),
    ("execcmnd", Word::Unsupported("execcmnd")),
    ("exit", Word::Exit),
    ("expandenv", Word::ExpandEnv),
    ("fileclose", Word::Unsupported("fileclose")),
    ("fileconcat", Word::Unsupported("fileconcat")),
    ("filecopy", Word::Unsupported("filecopy")),
    ("filecreate", Word::Unsupported("filecreate")),
    ("filedelete", Word::Unsupported("filedelete")),
    ("filelock", Word::Unsupported("filelock")),
    ("filemarkptr", Word::Unsupported("filemarkptr")),
    ("fileopen", Word::Unsupported("fileopen")),
    ("filereadln", Word::Unsupported("filereadln")),
    ("fileread", Word::Unsupported("fileread")),
    ("filerename", Word::Unsupported("filerename")),
    ("filesearch", Word::Unsupported("filesearch")),
    ("fileseek", Word::Unsupported("fileseek")),
    ("fileseekback", Word::Unsupported("fileseekback")),
    ("filestat", Word::Unsupported("filestat")),
    ("filestrseek", Word::Unsupported("filestrseek")),
    ("filestrseek2", Word::Unsupported("filestrseek2")),
    ("filetruncate", Word::Unsupported("filetruncate")),
    ("fileunlock", Word::Unsupported("fileunlock")),
    ("filewrite", Word::Unsupported("filewrite")),
    ("filewriteln", Word::Unsupported("filewriteln")),
    ("findclose", Word::Unsupported("findclose")),
    ("findfirst", Word::Unsupported("findfirst")),
    ("findnext", Word::Unsupported("findnext")),
    ("flushrecv", Word::Unsupported("flushrecv")),
    ("foldercreate", Word::Unsupported("foldercreate")),
    ("folderdelete", Word::Unsupported("folderdelete")),
    ("foldersearch", Word::Unsupported("foldersearch")),
    ("for", Word::For),
    ("getdate", Word::GetDate),
    ("getdir", Word::Unsupported("getdir")),
    ("getenv", Word::GetEnv),
    ("getfileattr", Word::Unsupported("getfileattr")),
    ("gethostname", Word::Unsupported("gethostname")),
    ("getipv4addr", Word::Unsupported("getipv4addr")),
    ("getipv6addr", Word::Unsupported("getipv6addr")),
    ("getpassword", Word::Unsupported("getpassword")),
    ("getpassword2", Word::Unsupported("getpassword2")),
    ("getspecialfolder", Word::Unsupported("getspecialfolder")),
    ("gettime", Word::GetTime),
    ("gettitle", Word::Unsupported("gettitle")),
    ("getttdir", Word::Unsupported("getttdir")),
    ("getttpos", Word::Unsupported("getttpos")),
    ("getver", Word::GetVer),
    ("goto", Word::Goto),
    ("if", Word::If),
    ("ifdefined", Word::IfDefined),
    ("include", Word::Include),
    ("inputbox", Word::Unsupported("inputbox")),
    ("int2str", Word::Int2Str),
    ("intdim", Word::IntDim),
    ("ispassword", Word::Unsupported("ispassword")),
    ("ispassword2", Word::Unsupported("ispassword2")),
    ("kmtfinish", Word::Unsupported("kmtfinish")),
    ("kmtget", Word::Unsupported("kmtget")),
    ("kmtrecv", Word::Unsupported("kmtrecv")),
    ("kmtsend", Word::Unsupported("kmtsend")),
    ("listbox", Word::Unsupported("listbox")),
    ("loadkeymap", Word::Unsupported("loadkeymap")),
    ("logautoclosemode", Word::Unsupported("logautoclosemode")),
    ("logclose", Word::Unsupported("logclose")),
    ("loginfo", Word::Unsupported("loginfo")),
    ("logopen", Word::Unsupported("logopen")),
    ("logpause", Word::Unsupported("logpause")),
    ("logrotate", Word::Unsupported("logrotate")),
    ("logstart", Word::Unsupported("logstart")),
    ("logwrite", Word::Unsupported("logwrite")),
    ("loop", Word::Loop),
    ("makepath", Word::MakePath),
    ("messagebox", Word::Unsupported("messagebox")),
    ("mpause", Word::Unsupported("mpause")),
    ("next", Word::Next),
    ("not", Word::BNot),
    ("or", Word::BOr),
    ("outputdebugstring", Word::Unsupported("outputdebugstring")),
    ("passwordbox", Word::Unsupported("passwordbox")),
    ("pause", Word::Unsupported("pause")),
    ("quickvanrecv", Word::Unsupported("quickvanrecv")),
    ("quickvansend", Word::Unsupported("quickvansend")),
    ("random", Word::Random),
    ("recvln", Word::Unsupported("recvln")),
    ("recvfile", Word::Unsupported("recvfile")),
    ("regexoption", Word::Unsupported("regexoption")),
    ("restoresetup", Word::Unsupported("restoresetup")),
    ("return", Word::Return),
    ("rotateleft", Word::RotateL),
    ("rotateright", Word::RotateR),
    ("scprecv", Word::Unsupported("scprecv")),
    ("scpsend", Word::Unsupported("scpsend")),
    ("send", Word::Unsupported("send")),
    ("sendbreak", Word::Unsupported("sendbreak")),
    ("sendbroadcast", Word::Unsupported("sendbroadcast")),
    ("sendlnbroadcast", Word::Unsupported("sendlnbroadcast")),
    ("sendlnmulticast", Word::Unsupported("sendlnmulticast")),
    ("sendmulticast", Word::Unsupported("sendmulticast")),
    ("sendtext", Word::Unsupported("sendtext")),
    ("sendbinary", Word::Unsupported("sendbinary")),
    ("setfileattr", Word::Unsupported("setfileattr")),
    ("setmulticastname", Word::Unsupported("setmulticastname")),
    ("sendfile", Word::Unsupported("sendfile")),
    ("sendkcode", Word::Unsupported("sendkcode")),
    ("sendln", Word::Unsupported("sendln")),
    ("setbaud", Word::Unsupported("setbaud")),
    ("setdate", Word::Unsupported("setdate")),
    ("setdebug", Word::Unsupported("setdebug")),
    ("setdir", Word::Unsupported("setdir")),
    ("setdlgpos", Word::Unsupported("setdlgpos")),
    ("setdtr", Word::Unsupported("setdtr")),
    ("setecho", Word::Unsupported("setecho")),
    ("setenv", Word::SetEnv),
    ("setexitcode", Word::SetExitCode),
    ("setflowctrl", Word::Unsupported("setflowctrl")),
    ("setpassword", Word::Unsupported("setpassword")),
    ("setpassword2", Word::Unsupported("setpassword2")),
    ("setrts", Word::Unsupported("setrts")),
    ("setserialdelaychar", Word::Unsupported("setserialdelaychar")),
    ("setserialdelayline", Word::Unsupported("setserialdelayline")),
    ("setspeed", Word::Unsupported("setspeed")),
    ("setsync", Word::Unsupported("setsync")),
    ("settime", Word::Unsupported("settime")),
    ("settitle", Word::Unsupported("settitle")),
    ("show", Word::Unsupported("show")),
    ("showtt", Word::Unsupported("showtt")),
    ("sprintf", Word::Sprintf),
    ("sprintf2", Word::Sprintf2),
    ("statusbox", Word::Unsupported("statusbox")),
    ("str2code", Word::Str2Code),
    ("str2int", Word::Str2Int),
    ("strcompare", Word::StrCompare),
    ("strconcat", Word::StrConcat),
    ("strcopy", Word::StrCopy),
    ("strdim", Word::StrDim),
    ("strinsert", Word::StrInsert),
    ("strjoin", Word::StrJoin),
    ("strlen", Word::StrLen),
    ("strmatch", Word::Unsupported("strmatch")),
    ("strremove", Word::StrRemove),
    ("strreplace", Word::Unsupported("strreplace")),
    ("strscan", Word::StrScan),
    ("strspecial", Word::StrSpecial),
    ("strsplit", Word::StrSplit),
    ("strtrim", Word::StrTrim),
    ("testlink", Word::Unsupported("testlink")),
    ("then", Word::Then),
    ("tolower", Word::ToLower),
    ("toupper", Word::ToUpper),
    ("unlink", Word::Unsupported("unlink")),
    ("until", Word::Until),
    ("uptime", Word::Unsupported("uptime")),
    ("var2clipb", Word::Unsupported("var2clipb")),
    ("waitregex", Word::Unsupported("waitregex")),
    ("wait", Word::Unsupported("wait")),
    ("wait4all", Word::Unsupported("wait4all")),
    ("waitevent", Word::Unsupported("waitevent")),
    ("waitln", Word::Unsupported("waitln")),
    ("waitn", Word::Unsupported("waitn")),
    ("waitrecv", Word::Unsupported("waitrecv")),
    ("while", Word::While),
    ("xmodemrecv", Word::Unsupported("xmodemrecv")),
    ("xmodemsend", Word::Unsupported("xmodemsend")),
    ("xor", Word::BXor),
    ("yesnobox", Word::Unsupported("yesnobox")),
    ("ymodemrecv", Word::Unsupported("ymodemrecv")),
    ("ymodemsend", Word::Unsupported("ymodemsend")),
    ("zmodemrecv", Word::Unsupported("zmodemrecv")),
    ("zmodemsend", Word::Unsupported("zmodemsend")),
];

/// 查表（大小寫不敏感，同原碼的 `_stricmp`）。
pub fn check_reserved_word(name: &str) -> Option<Word> {
    LOOKUP
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, w)| *w)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表的大小＝原碼的 211 筆（含 `setspeed` 這個 `setbaud` 的別名）。
    #[test]
    fn table_size_matches_source() {
        assert_eq!(LOOKUP.len(), 211, "CheckReservedWord 有 211 個名稱");
    }

    /// 大小寫不敏感（原碼用 `_stricmp`）。
    #[test]
    fn lookup_is_case_insensitive() {
        assert_eq!(check_reserved_word("IF"), Some(Word::If));
        assert_eq!(check_reserved_word("EndIf"), Some(Word::EndIf));
        assert_eq!(check_reserved_word("STRLEN"), Some(Word::StrLen));
        assert_eq!(check_reserved_word("notaword"), None);
    }

    /// ⚠️ `and`／`or`／`xor`／`not` 在 TTL 是**位元**運算（原碼對映到 RsvBAnd／RsvBOr／
    /// RsvBXor／RsvBNot），邏輯運算是 `&&`／`||`／`!`。舊版 C# 自寫版把它們當邏輯運算——
    /// 這是兩版之間的語意差異（見 docs/TTL.md）。
    #[test]
    fn word_operators_are_bitwise() {
        assert_eq!(check_reserved_word("and"), Some(Word::BAnd));
        assert_eq!(check_reserved_word("or"), Some(Word::BOr));
        assert_eq!(check_reserved_word("xor"), Some(Word::BXor));
        assert_eq!(check_reserved_word("not"), Some(Word::BNot));
        assert!(Word::BAnd.is_operator() && Word::BNot.is_operator());
        assert!(!Word::If.is_operator());
    }

    /// 還沒實作的指令仍然是保留字（不可以被當成變數名）。
    #[test]
    fn unimplemented_commands_are_still_reserved() {
        assert_eq!(check_reserved_word("send"), Some(Word::Unsupported("send")));
        assert_eq!(check_reserved_word("wait"), Some(Word::Unsupported("wait")));
        assert_eq!(
            check_reserved_word("messagebox"),
            Some(Word::Unsupported("messagebox"))
        );
    }

    /// `setspeed` 是 `setbaud` 的別名（原碼兩個名稱都指到 RsvSetBaud）。
    #[test]
    fn setspeed_alias_exists() {
        assert!(check_reserved_word("setspeed").is_some());
        assert!(check_reserved_word("setbaud").is_some());
    }
}
