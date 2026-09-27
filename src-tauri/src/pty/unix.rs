//! mac／Linux 的本機 shell session。對應 Windows 的 `conpty.rs`。
//!
//! 這個檔案**刻意很薄**：系統呼叫那一層在 `awayterm-platform` crate
//! （`platform/src/pty.rs`），這裡只負責把它包成 [`TerminalSession`]、開讀取執行緒、
//! 照 Windows 端的約定處理關閉流程。
//!
//! 為什麼要分兩層：主 crate 依賴 tauri，而 tauri 在 Linux／mac 會拉進需要
//! GTK 標頭檔／Apple SDK 的 `-sys` crate，**所以在 Windows 上沒辦法跨 target
//! `cargo check` 主 crate**。把系統呼叫搬到只依賴 `libc` 的 crate 之後，那一層
//! 真的可以在 Windows 上被三個 target 的編譯器檢查過（見 `platform/src/lib.rs`）。
//! 剩在這裡的接線很薄，風險小。
//!
//! ⚠️ **整個檔案都還沒在真的 mac／Linux 上跑過**（開發機是 Windows）。
//! 待驗證的項目在 `docs/PLATFORM-UNIX.md`。

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use awayterm_platform::pty as sys;

use crate::session::{ExitInfo, OnExit, OnOutput, TerminalSession};

/// 一次讀多少（和 Windows 端一樣：4 KiB，claude 重繪時一批就幾 KB）。
const READ_BUF: usize = 4096;
/// 送完優雅結束鍵之後等多久才強制收（同 Windows 端的 60ms）。
const GRACE: Duration = Duration::from_millis(60);
/// `SIGHUP` 之後等多久才 `SIGKILL`。
const KILL_AFTER: Duration = Duration::from_millis(400);

pub struct UnixPtySession {
    pty: Arc<sys::Pty>,
    graceful_exit_bytes: Vec<u8>,
    closed: Arc<AtomicBool>,
    /// 寫入要序列化（和 Windows 端一樣：同時來的輸入不能交錯）。
    write_lock: Mutex<()>,
}

impl UnixPtySession {
    pub fn spawn(
        opts: super::SpawnOptions,
        on_output: OnOutput,
        on_exit: OnExit,
    ) -> io::Result<Self> {
        let argv = awayterm_platform::cmdline::split(&opts.command_line);
        let pty = Arc::new(sys::spawn(&sys::Options {
            argv,
            cols: opts.cols,
            rows: opts.rows,
            cwd: opts.cwd.clone(),
            env: opts.env.clone(),
            // 沙盒模式＝自己一個行程群組，關分頁時整組收掉（Job Object 的對應）
            own_process_group: opts.kill_on_close,
        })?);

        let closed = Arc::new(AtomicBool::new(false));

        // 讀取執行緒：讀到 0 ＝子行程結束（Linux 的 EIO 已經在 platform 那層翻成 0）
        {
            let pty = pty.clone();
            let closed = closed.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; READ_BUF];
                loop {
                    match pty.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => on_output(&buf[..n]),
                        Err(e) => {
                            // 我們自己關掉 fd 時會拿到 EBADF，那是預期的
                            if !closed.load(Ordering::SeqCst) {
                                println!("[AwayTerminal] PTY 讀取結束：{e}");
                            }
                            break;
                        }
                    }
                }
                // 收 zombie 並回報離開碼（同 Windows 端在 exit thread 做的事）
                let code = pty.wait();
                on_exit(ExitInfo { exit_code: code });
            });
        }

        Ok(Self {
            pty,
            graceful_exit_bytes: opts.graceful_exit_bytes,
            closed,
            write_lock: Mutex::new(()),
        })
    }
}

impl TerminalSession for UnixPtySession {
    fn write(&self, data: &[u8]) {
        if data.is_empty() || self.closed.load(Ordering::SeqCst) {
            return;
        }
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        // 寫失敗＝對方已經關了，丟掉即可（同 Windows 端吞掉 IOException）
        let _ = self.pty.write_all(data);
    }

    fn resize(&self, cols: u16, rows: u16) {
        if cols < 1 || rows < 1 || self.closed.load(Ordering::SeqCst) {
            return;
        }
        let _ = self.pty.resize(cols, rows);
    }

    fn pid(&self) -> u32 {
        self.pty.pid()
    }

    fn backend_name(&self) -> &'static str {
        if cfg!(target_os = "macos") {
            "openpty (macOS)"
        } else {
            "openpty (Linux)"
        }
    }

    /// 順序照 Windows 端：送優雅結束鍵 → 等 60ms → 關 fd → `SIGHUP` → 等 → `SIGKILL`。
    ///
    /// 可重複呼叫（第二次起是 no-op）。
    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        if !self.graceful_exit_bytes.is_empty() {
            let _ = self.pty.write_all(&self.graceful_exit_bytes.clone());
            std::thread::sleep(GRACE);
        }
        // 先關 fd：slave 端讀到 EOF，多數 shell 會自己退出
        self.pty.close_fd();
        let pty = self.pty.clone();
        // `terminate_group` 會等最多 400ms，不要卡在呼叫端（關分頁要立刻有反應）
        std::thread::spawn(move || {
            if pty.try_wait().is_some() {
                return; // 已經自己走了
            }
            pty.hangup();
            std::thread::sleep(KILL_AFTER);
            if pty.try_wait().is_none() {
                pty.kill();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    /// 命令列切開的規則在 `awayterm-platform` 的 `cmdline` 模組測
    /// （放那邊才會在 Windows 上也跑到——這個檔案是 `#[cfg(not(windows))]`）。
    #[test]
    fn command_line_splitting_lives_in_the_platform_crate() {
        assert_eq!(
            awayterm_platform::cmdline::split("\"/a b/tool\" -x"),
            vec!["/a b/tool", "-x"]
        );
    }
}
