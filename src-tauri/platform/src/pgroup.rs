//! 行程群組 ＝ Unix 這邊的 Job Object（`src/pty/job.rs` 的對應）。
//!
//! Windows 用 Job Object kill-on-close：關分頁就把整棵子行程樹收乾淨。
//! Unix 沒有同樣的東西，最接近的是**行程群組**：子行程 `setsid()` 自己當 leader
//! （見 [`super::pty::child_exec`]），之後 `killpg(-pid)` 就會送給整組。
//!
//! | | Windows | Unix |
//! |---|---|---|
//! | 建立 | `CreateJobObject` ＋ `AssignProcessToJobObject` | 子行程 `setsid()` |
//! | 關掉整組 | 關 job handle（kill-on-close） | `killpg(SIGHUP)` → 等 → `killpg(SIGKILL)` |
//! | 會漏的情況 | Store app execution alias（AppX 服務建立的行程不在 job 裡） | 子行程自己再 `setsid()` 就脫離這一組 |
//!
//! ⚠️ **兩邊都是防呆不是防壞**（`docs/AGENT-SANDBOX.md` 講過）：agent 想跑掉就跑得掉。

use std::time::{Duration, Instant};

/// 送訊號給整個行程群組。
///
/// `pid` 是群組 leader（也就是我們 fork 出來、`setsid` 過的那個子行程）。
/// `killpg` 要的是**正的** pgid；`kill(-pid)` 也可以，這裡用 `killpg` 讓意圖明顯。
pub fn kill_group(pid: libc::pid_t, sig: libc::c_int) {
    // 失敗（`ESRCH`＝整組都不在了）不用理：呼叫端本來就是「盡量收乾淨」
    unsafe { libc::killpg(pid, sig) };
}

/// 這一組還有行程活著嗎。
///
/// `kill(pid, 0)` 只檢查權限與存在、不真的送訊號。回 false ＝收乾淨了。
pub fn group_alive(pid: libc::pid_t) -> bool {
    // 用 signal 0 問「這一組還在嗎」。leader 變成 zombie 時還是回 0（還沒被 wait 掉），
    // 所以呼叫端應該先 `waitpid` 再問這個。
    unsafe { libc::killpg(pid, 0) == 0 }
}

/// 優雅結束整組：先 `SIGHUP`，等 `grace`，還活著就 `SIGKILL`。
///
/// 順序照 Windows 端（`close()`：送優雅結束鍵 → 等 60ms → 強制收尾）。用 `SIGHUP`
/// 而不是 `SIGTERM`：終端機關掉時 Unix 本來就是送 SIGHUP，shell 與 CLI 對它的處理
/// （存歷史、收子行程）才是「使用者關掉視窗」該有的行為。
pub fn terminate_group(pid: libc::pid_t, grace: Duration) {
    kill_group(pid, libc::SIGHUP);
    let until = Instant::now() + grace;
    while Instant::now() < until {
        if !group_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    kill_group(pid, libc::SIGKILL);
}

/// 我自己的行程群組 id（診斷用）。
pub fn own_pgid() -> libc::pid_t {
    unsafe { libc::getpgrp() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不存在的行程群組：問「還在嗎」要回 false，而且不可以 panic。
    ///
    /// 用一個幾乎不可能存在的 pid（Linux 預設上限 32768、mac 99999）。
    #[test]
    fn a_dead_group_is_not_alive() {
        assert!(!group_alive(0x7fff_fff0));
    }

    /// 送訊號給不存在的群組不會 panic（`ESRCH` 直接忽略）。
    #[test]
    fn killing_a_dead_group_is_harmless() {
        kill_group(0x7fff_fff0, libc::SIGHUP);
        terminate_group(0x7fff_fff0, Duration::from_millis(1));
    }

    /// 自己的 pgid 是正數。
    #[test]
    fn own_pgid_is_positive() {
        assert!(own_pgid() > 0);
    }
}
