//! mac / Linux PTY stub。TASK-002 只做 Windows ConPTY；forkpty 之後的任務才實作。

use std::io;
use std::sync::Arc;

use crate::session::TerminalSession;

pub fn spawn_unsupported() -> io::Result<Arc<dyn TerminalSession>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        crate::i18n::t("err.unixPtyTodo"),
    ))
}
