//! TTL 巨集直譯器（TeraTerm 的 `.ttl`）。
//!
//! **藍本是 TeraTerm 的 `ttpmacro/`（BSD-3）**，照 `CLAUDE.md`「連接埠 / 巨集」那一列的
//! 決定逐段翻寫成 Rust；舊版 AwayTerminal 的 C# 自寫版是**行為下限**（舊版跑得動的巨集
//! 新版一定要跑得動、結果一樣）。檔案對照表、已實作的指令清單、與兩個版本的差異都在
//! `docs/TTL.md`。
//!
//! | `ttpmacro/` | 這裡 |
//! |---|---|
//! | `ttmparse.h`（`Err*`）、`errdlg.cpp`（`DispErr`） | [`error`] |
//! | `ttmparse.cpp`：`CheckReservedWord` | [`words`] |
//! | `ttmparse.cpp`：`GetFirstChar`／`GetIdentifier`／`GetString`／`GetNumber`／`GetOperator` | [`lex`] |
//! | `ttmparse.cpp`：`GetFactor`…`GetExpression`（11 層優先權） | [`expr`] |
//! | `ttmparse.cpp`：`Variables[]` 那一組 | [`vars`] |
//! | `ttl.cpp`：`Exec`／`ExecCmnd`＋`ttmbuff.c`：buffer／stack | [`exec`] |
//! | `ttl.cpp`：`TTL*`（不碰 I/O 的那些） | [`cmds`] |
//!
//! ## 這一批（TASK-012）不做的事
//!
//! 連線（`send`／`wait`／`connect`…）、對話框（`messagebox`…）、檔案存取、
//! 正規表示式（`strmatch`／`strreplace`／`waitregex`）、剪貼簿、`exec`。
//! 那些指令**仍然是保留字**，執行到會回 `Unknown command.`——不會被誤認成變數。
//!
//! 直譯器是**可暫停的狀態機**（[`exec::Interp::step`] 一次跑一行），所以第二批要讓
//! `wait`／`pause` 掛起、或讓使用者按鍵打斷，都不必改寫執行模型。

pub mod cmds;
pub mod error;
pub mod host;
pub mod exec;
pub mod execverify;
pub mod expr;
pub mod files;
pub mod io;
pub mod lex;
pub mod regex;
pub mod runner;
pub mod vars;
pub mod words;

pub use error::{Err, TtlError};
pub use host::{DialogAnswer, DialogRequest, MacroHost, RecvBuffer, WaitMatcher};
pub use exec::{Interp, Source, Step};
pub use vars::{VarType, Vars};

/// 跑一個 `.ttl` 檔到結束。`max_steps` 是防呆上限（一步＝一行）。
///
/// 第二批會有「在分頁裡執行巨集」的入口（要接 `IoTap`）；這個函式是給
/// `ttl_probe` 與測試用的。
pub fn run_file(path: &std::path::Path, max_steps: u64) -> Result<Vars, TtlError> {
    let mut it = Interp::from_file(path)?;
    it.run(max_steps)?;
    Ok(std::mem::take(&mut it.vars))
}
