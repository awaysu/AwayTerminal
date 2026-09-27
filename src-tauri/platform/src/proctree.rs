//! 子行程樹 ＝ 狀態燈要用的東西（`src/status.rs` 的 `parents_with_children`）。
//!
//! 舊版的規則：**PowerShell 分頁「有外部子行程」且「近 1.5 秒持續有輸出」才算忙**。
//! 「有外部子行程」在 Windows 是用 Toolhelp 掃一遍所有行程的 parent pid
//! （`src/status.rs`）；這裡是 mac／Linux 的對應。
//!
//! | 平台 | 做法 |
//! |---|---|
//! | Linux | 掃 `/proc/*/stat` 的第 4 欄（ppid）。**不用** `/proc/<pid>/task/*/children`——它需要 `CONFIG_PROC_CHILDREN`，不是每個發行版都開 |
//! | macOS | `proc_listpids(PROC_ALL_PIDS)` ＋ 對每個 pid 問 `proc_pidinfo` 拿 ppid |
//!
//! ⚠️ 兩邊都**只讀不寫**，不開任何 handle、不送任何訊號（同 Windows 端的註解）。
//!
//! 待真機驗證：`/proc` 的欄位位置在有空白的行程名下是否解析正確（見 [`ppid_from_stat`]
//! 的註解與測試）、mac 的 `proc_pidinfo` 在沒有權限看別人的行程時的行為。

use std::collections::HashSet;

/// 目前有哪些 pid 是「某個人的 parent」。
///
/// 回傳的是**parent 的 pid 集合**，所以 `set.contains(&my_pid)` ＝「我有子行程」。
pub fn parents_with_children() -> HashSet<u32> {
    #[cfg(target_os = "linux")]
    {
        linux_parents()
    }
    #[cfg(target_os = "macos")]
    {
        macos_parents()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        // 其他平台（含 Windows——那邊有自己的 Toolhelp 實作）不從這裡拿
        HashSet::new()
    }
}

// ----------------------------------------------------------------- Linux

/// 掃 `/proc/*/stat`，把每一個行程的 ppid 收起來。
#[cfg(target_os = "linux")]
fn linux_parents() -> HashSet<u32> {
    let mut out = HashSet::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // 只看純數字的目錄（`/proc/self`、`/proc/meminfo` 之類跳過）
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        // 行程隨時會消失 → 讀失敗是正常的，不要當錯誤
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{name}/stat")) else {
            continue;
        };
        if let Some(ppid) = ppid_from_stat(&stat) {
            if ppid != 0 {
                out.insert(ppid);
            }
        }
    }
    out
}

/// 從 `/proc/<pid>/stat` 的內容取出 ppid。
///
/// ⚠️ **不可以直接用空白切**：第 2 欄是行程名、包在括號裡，而且**可以含空白與括號**
/// （例：`1234 (My App (beta)) S 1 ...`）。正確做法是找**最後一個** `)`，
/// 從那之後開始算——`state` 是第 3 欄、`ppid` 是第 4 欄。
///
/// 這個函式**在所有平台都編得過也測得到**（吃字串），所以 Windows 上也跑得到它的測試。
pub fn ppid_from_stat(stat: &str) -> Option<u32> {
    let close = stat.rfind(')')?;
    let rest = stat.get(close + 1..)?;
    let mut it = rest.split_whitespace();
    let _state = it.next()?; // 第 3 欄
    it.next()?.parse().ok() // 第 4 欄 ＝ ppid
}

// ----------------------------------------------------------------- macOS

/// `libproc` 的兩個函式。**刻意自己宣告、不用 `libproc` crate**：那個 crate 會編一段
/// C（build script 要 clang ＋ Apple SDK），就沒辦法在 Windows 上 `cargo check` 了。
///
/// 兩個都在 `libproc.h`／`sys/proc_info.h`，是 macOS 的公開 API。
#[cfg(target_os = "macos")]
mod sys {
    /// `proc_listpids(PROC_ALL_PIDS, 0, buf, buflen)` → 寫進去幾個 byte。
    pub const PROC_ALL_PIDS: u32 = 1;
    /// `proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size)`
    pub const PROC_PIDTBSDINFO: libc::c_int = 3;

    /// `sys/proc_info.h` 的 `struct proc_bsdinfo`。**欄位順序與型別要和標頭檔一致**，
    /// 我們只讀 `pbi_ppid`，但整個結構的大小必須對，不然 `proc_pidinfo` 會回 0。
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct ProcBsdInfo {
        pub pbi_flags: u32,
        pub pbi_status: u32,
        pub pbi_xstatus: u32,
        pub pbi_pid: u32,
        pub pbi_ppid: u32,
        pub pbi_uid: libc::uid_t,
        pub pbi_gid: libc::gid_t,
        pub pbi_ruid: libc::uid_t,
        pub pbi_rgid: libc::gid_t,
        pub pbi_svuid: libc::uid_t,
        pub pbi_svgid: libc::gid_t,
        pub rfu_1: u32,
        pub pbi_comm: [libc::c_char; 16],
        pub pbi_name: [libc::c_char; 32],
        pub pbi_nfiles: u32,
        pub pbi_pgid: u32,
        pub pbi_pjobc: u32,
        pub e_tdev: u32,
        pub e_tpgid: u32,
        pub pbi_nice: i32,
        pub pbi_start_tvsec: u64,
        pub pbi_start_tvusec: u64,
    }

    extern "C" {
        pub fn proc_listpids(
            r#type: u32,
            typeinfo: u32,
            buffer: *mut libc::c_void,
            buffersize: libc::c_int,
        ) -> libc::c_int;

        pub fn proc_pidinfo(
            pid: libc::c_int,
            flavor: libc::c_int,
            arg: u64,
            buffer: *mut libc::c_void,
            buffersize: libc::c_int,
        ) -> libc::c_int;
    }
}

#[cfg(target_os = "macos")]
fn macos_parents() -> HashSet<u32> {
    let mut out = HashSet::new();
    // 先問要多大（buffer 傳 null），再配置。行程數會變，所以多留一些空間。
    let need = unsafe { sys::proc_listpids(sys::PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
    if need <= 0 {
        return out;
    }
    let count = (need as usize / std::mem::size_of::<libc::c_int>()) + 64;
    let mut pids: Vec<libc::c_int> = vec![0; count];
    let got = unsafe {
        sys::proc_listpids(
            sys::PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr().cast(),
            (pids.len() * std::mem::size_of::<libc::c_int>()) as libc::c_int,
        )
    };
    if got <= 0 {
        return out;
    }
    let n = got as usize / std::mem::size_of::<libc::c_int>();
    for &pid in pids.iter().take(n) {
        if pid <= 0 {
            continue;
        }
        let mut info = sys::ProcBsdInfo::default();
        let size = std::mem::size_of::<sys::ProcBsdInfo>() as libc::c_int;
        let r = unsafe {
            sys::proc_pidinfo(
                pid,
                sys::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut sys::ProcBsdInfo).cast(),
                size,
            )
        };
        // 回的 byte 數不對就跳過（看不到別人的行程時會這樣，不是錯誤）
        if r != size {
            continue;
        }
        if info.pbi_ppid != 0 {
            out.insert(info.pbi_ppid);
        }
    }
    out
}

/// 這個 pid 還在嗎。**唯讀**（`kill(pid, 0)` 只檢查存在與權限，不送訊號）。
///
/// 對應 Windows 的 `status::pid_exists`（給 `--verify` 驗沙盒收行程用）。
#[cfg(unix)]
pub fn pid_exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/proc/<pid>/stat` 的解析——**行程名可以含空白和括號**，這是最容易寫錯的地方。
    #[test]
    fn parses_ppid_even_with_a_nasty_process_name() {
        // 正常
        assert_eq!(ppid_from_stat("1234 (bash) S 1000 1234 ..."), Some(1000));
        // 名字含空白
        assert_eq!(ppid_from_stat("77 (My App) S 42 77 ..."), Some(42));
        // 名字含括號（`rfind(')')` 才對，`find` 會切在中間）
        assert_eq!(ppid_from_stat("88 (My App (beta)) S 43 88 ..."), Some(43));
        // 名字就是一個括號
        assert_eq!(ppid_from_stat("99 ()) R 44 99 ..."), Some(44));
    }

    /// 壞掉的內容回 None，不 panic。
    #[test]
    fn broken_stat_lines_return_none() {
        assert_eq!(ppid_from_stat(""), None);
        assert_eq!(ppid_from_stat("no parens here"), None);
        assert_eq!(ppid_from_stat("1 (init)"), None); // 後面沒東西
        assert_eq!(ppid_from_stat("1 (init) S"), None); // 只有 state，沒有 ppid
        assert_eq!(ppid_from_stat("1 (init) S notanumber"), None);
    }

    /// 真的跑一次（在 Windows 上回空集合，在 Unix 上至少該有 pid 1 之類的 parent）。
    #[test]
    fn parents_with_children_does_not_panic() {
        let set = parents_with_children();
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            // 任何一台機器上一定有行程有子行程（init／launchd）
            assert!(!set.is_empty(), "一個 parent 都沒找到？");
        } else {
            assert!(set.is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn my_own_pid_exists() {
        assert!(pid_exists(std::process::id()));
        assert!(!pid_exists(0x7fff_fff0));
    }
}
