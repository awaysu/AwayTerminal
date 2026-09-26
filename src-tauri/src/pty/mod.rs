//! PTY 後端。Windows 走自寫 ConPTY；mac / Linux 之後用 forkpty（本階段只有 stub）。

#[cfg(windows)]
pub mod conpty;
#[cfg(windows)]
pub mod conpty_host;
#[cfg(not(windows))]
pub mod unix;

pub mod shell;

use std::io;
use std::sync::Arc;

use crate::session::{OnExit, OnOutput, TerminalSession};

/// 開一個本機 shell 的 PTY 連線。
pub struct SpawnOptions {
    pub command_line: String,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    /// 關閉前送出的位元組。PowerShell / Claude Code＝Ctrl+C ×3；SSH 之後用 Ctrl+D ×2。
    pub graceful_exit_bytes: Vec<u8>,
}

impl SpawnOptions {
    /// 預設的優雅結束鍵：Ctrl+C ×3（同舊版 `ConPtySession.GracefulExitBytes`）。
    pub fn default_graceful_exit_bytes() -> Vec<u8> {
        vec![0x03, 0x03, 0x03]
    }
}

#[cfg(windows)]
pub fn spawn(
    opts: SpawnOptions,
    on_output: OnOutput,
    on_exit: OnExit,
) -> io::Result<Arc<dyn TerminalSession>> {
    let session = conpty::ConPtySession::spawn(
        conpty::ConPtyOptions {
            command_line: opts.command_line,
            cols: opts.cols,
            rows: opts.rows,
            cwd: opts.cwd,
            graceful_exit_bytes: opts.graceful_exit_bytes,
        },
        on_output,
        on_exit,
    )?;
    Ok(Arc::new(session))
}

#[cfg(not(windows))]
pub fn spawn(
    _opts: SpawnOptions,
    _on_output: OnOutput,
    _on_exit: OnExit,
) -> io::Result<Arc<dyn TerminalSession>> {
    unix::spawn_unsupported()
}

/// 目前使用的 ConPTY 主機（診斷字串，啟動時記進 log）。
pub fn backend_name() -> String {
    #[cfg(windows)]
    {
        conpty_host::backend_name()
    }
    #[cfg(not(windows))]
    {
        "unix pty (尚未實作)".to_string()
    }
}
