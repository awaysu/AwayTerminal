//! Unix PTY：`openpty` ＋ `fork`／`execvp`，對應 Windows 的 ConPTY
//! （`src/pty/conpty.rs`）。
//!
//! # 為什麼是 `openpty` + 自己 fork，不是 `forkpty`
//!
//! `forkpty` 等於 `openpty` ＋ `fork` ＋ 子行程端把 slave 設成控制終端。自己寫這幾步是
//! 因為中間還要做**只能在子行程裡做**的事（`setsid`、`chdir`、環境變數、把 slave 接成
//! 0/1/2），而 `forkpty` 之後才做的話 `setsid` 已經被它做過、順序不由我們控制。
//!
//! # ⚠️ fork 之後只能呼叫 async-signal-safe 的函式
//!
//! 子行程那一段（[`child_exec`]）在 `fork` 回來之後、`execvp` 之前執行。那個時候
//! **只有這個執行緒還在**，但其他執行緒持有的鎖還是鎖著的——所以那一段裡不可以
//! 配置記憶體（`malloc` 可能正被別的執行緒鎖著 → 死鎖）、不可以 `println!`、
//! 不可以碰 Rust 的 `std::env`（它有自己的鎖）。所有字串都在 fork **之前**就先轉成
//! `CString` 準備好，子行程裡只做系統呼叫。
//!
//! # 對應 Windows 的行為
//!
//! | 項目 | Windows（ConPTY） | 這裡 |
//! |---|---|---|
//! | 改變大小 | `ResizePseudoConsole` | `ioctl(TIOCSWINSZ)` |
//! | 優雅結束 | 寫 Ctrl+C ×3，等 60ms，再收 | 同（寫進 master fd） |
//! | 強制結束 | Job Object kill-on-close ／ `TerminateProcess` | `killpg(SIGHUP)` → 等 → `killpg(SIGKILL)`（見 [`super::pgroup`]） |
//! | 沙盒收行程樹 | Job Object | 行程群組（子行程 `setsid` 自己當 leader） |
//!
//! 待真機驗證的項目在 `docs/PLATFORM-UNIX.md`。

use std::ffi::{CStr, CString};
use std::io;
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 開一個 PTY 要給的東西。欄位刻意和主 crate 的 `SpawnOptions` 對齊。
#[derive(Debug, Clone)]
pub struct Options {
    /// 要跑的程式與參數。**已經切好**（Unix 的 `execvp` 吃陣列，不像 Windows 吃一整條字串）。
    pub argv: Vec<String>,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    /// 追加的環境變數（沙盒模式的 `TMPDIR`／`CARGO_TARGET_DIR` 等）。
    pub env: Vec<(String, String)>,
    /// 沙盒模式：子行程自己開一個 session／行程群組，關閉時整組收掉。
    pub own_process_group: bool,
}

/// 開好的 PTY。`master` 是要讀寫的 fd，`pid` 是子行程。
#[derive(Debug)]
pub struct Pty {
    master: RawFd,
    pid: libc::pid_t,
    /// 子行程是不是自己的行程群組 leader（決定要 `killpg` 還是 `kill`）。
    own_group: bool,
    closed: Arc<AtomicBool>,
}

impl Pty {
    pub fn master_fd(&self) -> RawFd {
        self.master
    }

    pub fn pid(&self) -> u32 {
        self.pid as u32
    }

    /// 讀一批輸出。回 `Ok(0)` ＝對方關了（子行程結束）。
    ///
    /// 子行程結束時 Linux 的 master 會回 `EIO` 而不是 0，這裡把它也翻成 0，
    /// 呼叫端只要看 0 就知道要收攤（mac 回 0）。
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let n = unsafe { libc::read(self.master, buf.as_mut_ptr().cast(), buf.len()) };
            if n >= 0 {
                return Ok(n as usize);
            }
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                Some(libc::EINTR) => continue, // 被訊號打斷：重試
                // Linux：slave 全部關掉之後讀 master 會得到 EIO，那就是「結束了」
                Some(libc::EIO) => return Ok(0),
                _ => return Err(err),
            }
        }
    }

    /// 寫入（鍵盤輸入）。會處理部分寫入與 `EINTR`。
    pub fn write_all(&self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let n = unsafe { libc::write(self.master, data.as_ptr().cast(), data.len()) };
            if n > 0 {
                data = &data[n as usize..];
                continue;
            }
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "pty write 回 0"));
            }
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                Some(libc::EINTR) => continue,
                // 對方已經關了：當成寫完（同 Windows 端寫進已關閉的 pipe 的處理）
                Some(libc::EIO) => return Ok(()),
                _ => return Err(err),
            }
        }
        Ok(())
    }

    /// 同步終端機大小（對應 `ResizePseudoConsole`）。
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let r = unsafe { libc::ioctl(self.master, libc::TIOCSWINSZ, &ws) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// 子行程還活著嗎（非阻塞 `waitpid`）。回 `Some(exit code)` ＝已結束。
    ///
    /// **只有開了這個 PTY 的那個 crate 可以呼叫**：`waitpid` 會把 zombie 收掉，
    /// 收過一次之後第二次會回 `ECHILD`（這裡翻成「已結束、但拿不到 code」）。
    pub fn try_wait(&self) -> Option<Option<i32>> {
        let mut status: libc::c_int = 0;
        let r = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
        if r == 0 {
            return None; // 還在跑
        }
        if r < 0 {
            // ECHILD＝已經被收掉了；其他錯誤也當成結束（拿不到 code）
            return Some(None);
        }
        Some(exit_code(status))
    }

    /// 阻塞等到子行程結束。
    pub fn wait(&self) -> Option<i32> {
        let mut status: libc::c_int = 0;
        loop {
            let r = unsafe { libc::waitpid(self.pid, &mut status, 0) };
            if r < 0 {
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return None;
            }
            return exit_code(status);
        }
    }

    /// 送訊號。`own_process_group` 開著就送給整組（沙盒模式要收整棵樹）。
    pub fn signal(&self, sig: libc::c_int) {
        if self.own_group {
            super::pgroup::kill_group(self.pid, sig);
        } else {
            unsafe { libc::kill(self.pid, sig) };
        }
    }

    /// `SIGHUP`（＝「終端機關掉了」）。shell 與 CLI 對它的處理才是關視窗該有的行為。
    pub fn hangup(&self) {
        self.signal(libc::SIGHUP);
    }

    /// `SIGKILL`。最後手段。
    pub fn kill(&self) {
        self.signal(libc::SIGKILL);
    }

    /// 收攤：關 master fd。**不送訊號**（訊號由呼叫端照優雅結束流程決定）。
    ///
    /// 可重複呼叫（第二次起是 no-op），同 Windows 端 `close()` 的約定。
    pub fn close_fd(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        unsafe { libc::close(self.master) };
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.close_fd();
    }
}

fn exit_code(status: libc::c_int) -> Option<i32> {
    // libc 的 WIFEXITED／WEXITSTATUS 是巨集，Rust 這邊自己算（和 C 的定義一樣）
    if status & 0x7f == 0 {
        return Some((status >> 8) & 0xff);
    }
    // 被訊號殺掉：照 shell 的慣例回 128 + 訊號
    let sig = status & 0x7f;
    if sig != 0 {
        return Some(128 + sig);
    }
    None
}

/// 開一個 PTY 並在裡面跑起 [`Options::argv`]。
pub fn spawn(opts: &Options) -> io::Result<Pty> {
    if opts.argv.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "argv 是空的"));
    }

    // ---- fork 之前把所有東西準備好（fork 之後不可以配置記憶體，見檔頭註解）----
    let prog = CString::new(opts.argv[0].clone())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "程式路徑含 NUL"))?;
    let args: Vec<CString> = opts
        .argv
        .iter()
        .map(|a| CString::new(a.clone()))
        .collect::<Result<_, _>>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "參數含 NUL"))?;
    let mut argv_ptrs: Vec<*const libc::c_char> = args.iter().map(|a| a.as_ptr()).collect();
    argv_ptrs.push(std::ptr::null());

    let cwd = match &opts.cwd {
        Some(d) if !d.is_empty() => Some(
            CString::new(d.clone())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "工作目錄含 NUL"))?,
        ),
        _ => None,
    };
    // 環境變數：`KEY=VALUE` 的形式，fork 之後用 `putenv` 塞（`setenv` 會配置記憶體）
    let envs: Vec<CString> = opts
        .env
        .iter()
        .filter_map(|(k, v)| CString::new(format!("{k}={v}")).ok())
        .collect();

    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let ws = libc::winsize {
        ws_row: opts.rows.max(1),
        ws_col: opts.cols.max(1),
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // `openpty` 的 termios 傳 null ＝用系統預設（和使用者自己開終端機時一致）
    let r = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &ws as *const libc::winsize as *mut libc::winsize,
        )
    };
    if r < 0 {
        return Err(io::Error::last_os_error());
    }

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let err = io::Error::last_os_error();
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
        return Err(err);
    }

    if pid == 0 {
        // ---- 子行程：只做系統呼叫，不配置記憶體、不 panic ----
        unsafe {
            child_exec(
                master,
                slave,
                prog.as_ptr(),
                argv_ptrs.as_ptr(),
                cwd.as_ref().map(|c| c.as_ptr()),
                &envs,
                opts.own_process_group,
            );
        }
        // `execvp` 成功就不會回來；回來了就是失敗，直接離開（不可以跑 Rust 的清理流程）
        unsafe { libc::_exit(127) };
    }

    // ---- 父行程 ----
    unsafe { libc::close(slave) };
    set_cloexec(master);
    Ok(Pty {
        master,
        pid,
        own_group: opts.own_process_group,
        closed: Arc::new(AtomicBool::new(false)),
    })
}

/// `fork` 之後、`execvp` 之前的那一段。**只能呼叫 async-signal-safe 的東西。**
unsafe fn child_exec(
    master: RawFd,
    slave: RawFd,
    prog: *const libc::c_char,
    argv: *const *const libc::c_char,
    cwd: Option<*const libc::c_char>,
    envs: &[CString],
    own_group: bool,
) {
    // 父行程那一端在子行程裡沒有用處，而且不關的話對方讀不到 EOF
    libc::close(master);

    // 自己當 session leader：slave 才能成為控制終端，Ctrl+C 之類的訊號才會送到這一組。
    // 沙盒模式還靠它收整棵樹（對應 Windows 的 Job Object）。
    if own_group {
        libc::setsid();
    } else {
        // 不開沙盒時也要 setsid——沒有控制終端的話 `tcsetpgrp`／job control 會怪怪的，
        // 而且父行程（我們的 GUI）的 Ctrl+C 會打到子行程去。
        libc::setsid();
    }
    // 把 slave 設成控制終端（mac 需要明確做，Linux 在 setsid 之後 open 也會自動接）
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    libc::ioctl(slave, libc::TIOCSCTTY as libc::c_ulong, 0);
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    libc::ioctl(slave, libc::TIOCSCTTY, 0);

    libc::dup2(slave, 0);
    libc::dup2(slave, 1);
    libc::dup2(slave, 2);
    if slave > 2 {
        libc::close(slave);
    }

    if let Some(dir) = cwd {
        // 進不去就照原本的工作目錄跑（同 Windows 端：cwd 無效不算致命）
        libc::chdir(dir);
    }

    for e in envs {
        // `putenv` 不複製字串，所以要傳一份活得夠久的——`envs` 由父行程配置、
        // fork 之後這份記憶體還在，而且我們馬上就 exec，所以安全。
        libc::putenv(e.as_ptr() as *mut libc::c_char);
    }

    // 訊號處理回預設：Rust runtime 會把 SIGPIPE 設成 ignore，繼承下去會讓
    // 子行程裡的 shell 行為和使用者自己開終端機不一樣。
    libc::signal(libc::SIGPIPE, libc::SIG_DFL);

    libc::execvp(prog, argv);
}

/// 讓 fd 不被之後 fork 出去的行程繼承（避免我們自己開的其他子行程抓著它）。
fn set_cloexec(fd: RawFd) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}

/// 使用者的預設 shell：`$SHELL`，沒有就照平台猜（mac zsh、Linux bash）。
///
/// 照 `CLAUDE.md` 的平台差異表：「預設開使用者的 `$SHELL`」。
pub fn default_shell() -> Vec<String> {
    if let Ok(sh) = std::env::var("SHELL") {
        if !sh.trim().is_empty() && std::path::Path::new(&sh).is_file() {
            // `-l`（login shell）**刻意不加**：使用者自己開終端機時多半是 login shell，
            // 但我們是 GUI 啟動的，加 `-l` 會重跑一次 profile、拖慢啟動，而且和
            // Windows 端（直接開 pwsh、不特別加參數）不一致。
            return vec![sh];
        }
    }
    let fallback = if cfg!(target_os = "macos") {
        "/bin/zsh" // macOS Catalina 之後的預設
    } else {
        "/bin/bash"
    };
    vec![fallback.to_string()]
}

/// 程式路徑的第一個 token 用 `CStr` 檢查（給主 crate 診斷用）。
pub fn program_name(argv: &[String]) -> &str {
    argv.first().map(|s| s.as_str()).unwrap_or("")
}

/// `execvp` 失敗時子行程的離開碼（`127`，照 shell 的慣例「找不到指令」）。
pub const EXEC_FAILED_CODE: i32 = 127;

/// 只是為了讓 `CStr` 這個 import 在所有 target 上都有用到（診斷用小工具）。
pub fn cstr_is_empty(s: &CStr) -> bool {
    s.to_bytes().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 離開碼的算法和 C 的 `WIFEXITED`／`WEXITSTATUS` 一致。
    #[test]
    fn exit_code_matches_c_macros() {
        assert_eq!(exit_code(0), Some(0)); // exit(0)
        assert_eq!(exit_code(7 << 8), Some(7)); // exit(7)
        assert_eq!(exit_code(libc::SIGKILL), Some(128 + 9)); // 被 SIGKILL
    }

    /// `argv` 空的要回錯，不可以真的去 fork。
    #[test]
    fn empty_argv_is_rejected() {
        let o = Options {
            argv: vec![],
            cols: 80,
            rows: 24,
            cwd: None,
            env: vec![],
            own_process_group: false,
        };
        assert!(spawn(&o).is_err());
    }

    /// `$SHELL` 沒設時的後備要跟著平台。
    #[test]
    fn default_shell_has_a_fallback() {
        let sh = default_shell();
        assert_eq!(sh.len(), 1);
        assert!(sh[0].starts_with('/'), "回的不是絕對路徑：{:?}", sh);
    }

    #[test]
    fn program_name_of_empty_is_empty() {
        assert_eq!(program_name(&[]), "");
        assert_eq!(program_name(&["/bin/zsh".to_string()]), "/bin/zsh");
    }
}
