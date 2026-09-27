//! 作業系統開機到現在多久（TTL 的 `uptime` 指令）。
//!
//! | 平台 | 做法 |
//! |---|---|
//! | Windows | `GetTickCount64`（原碼用 32 位元的 `GetTickCount`，見下） |
//! | Linux | `/proc/uptime` 的第一個欄位（秒，小數） |
//! | macOS | `sysctl kern.boottime` ＋ 現在的時間 |
//!
//! # 為什麼要截成 32 位元
//!
//! TeraTerm 的 `uptime` 用 `GetTickCount()`，**49.7 天會繞回 0**。原碼的註解明講這件事，
//! 並說「用 `GetTickCount64` 就不會溢位，但 TeraTerm 本來就不支援 64 位元變數，
//! 所以沒有意義」。TTL 的整數是 32 位元有號，所以我們照樣繞回——舊的 `.ttl` 檔如果
//! 有處理繞回的邏輯，行為才一致。
//!
//! 放在這個 crate 的理由：Linux／mac 的取法要用 `/proc` 與 `sysctl`，
//! 在 Windows 上編不到也測不到，所以要在跨 target check 的範圍內
//! （見 `lib.rs` 的檔頭）。

/// 開機到現在的毫秒數（64 位元，取得失敗回 0）。
pub fn uptime_ms() -> u64 {
    #[cfg(windows)]
    {
        windows_uptime()
    }
    #[cfg(target_os = "linux")]
    {
        linux_uptime()
    }
    #[cfg(target_os = "macos")]
    {
        macos_uptime()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

/// 同 [`uptime_ms`]，但**截成 32 位元**（照 TeraTerm 的 `GetTickCount`，49.7 天繞回）。
pub fn uptime_ms_u32() -> u32 {
    (uptime_ms() & 0xFFFF_FFFF) as u32
}

#[cfg(windows)]
fn windows_uptime() -> u64 {
    // 自己宣告：這個 crate 刻意不依賴 `windows-sys`（主 crate 才有），
    // 而 `GetTickCount64` 只是 kernel32 的一個無參數函式。
    #[link(name = "kernel32")]
    extern "system" {
        fn GetTickCount64() -> u64;
    }
    unsafe { GetTickCount64() }
}

/// `/proc/uptime` 的內容長這樣：`"12345.67 98765.43\n"`（開機秒數、idle 秒數）。
#[cfg(target_os = "linux")]
fn linux_uptime() -> u64 {
    let Ok(s) = std::fs::read_to_string("/proc/uptime") else {
        return 0;
    };
    parse_proc_uptime(&s).unwrap_or(0)
}

/// 把 `/proc/uptime` 的第一個欄位換成毫秒。**在所有平台都編得過也測得到**。
pub fn parse_proc_uptime(s: &str) -> Option<u64> {
    let first = s.split_whitespace().next()?;
    let secs: f64 = first.parse().ok()?;
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    Some((secs * 1000.0) as u64)
}

/// mac 沒有 `/proc`：問 `sysctl kern.boottime` 拿開機的絕對時間，再和現在相減。
#[cfg(target_os = "macos")]
fn macos_uptime() -> u64 {
    // `kern.boottime` 回一個 `struct timeval`
    let mut tv = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let mut size = std::mem::size_of::<libc::timeval>();
    // CTL_KERN = 1、KERN_BOOTTIME = 21（`sys/sysctl.h`）
    let mib: [libc::c_int; 2] = [1, 21];
    let r = unsafe {
        libc::sysctl(
            mib.as_ptr() as *mut libc::c_int,
            2,
            (&mut tv as *mut libc::timeval).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 || tv.tv_sec == 0 {
        return 0;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let boot = (tv.tv_sec as u64) * 1000 + (tv.tv_usec as u64) / 1000;
    now.saturating_sub(boot)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/proc/uptime` 的解析（在 Windows 上也跑得到——吃字串）。
    #[test]
    fn parses_proc_uptime() {
        assert_eq!(parse_proc_uptime("12345.67 98765.43\n"), Some(12_345_670));
        assert_eq!(parse_proc_uptime("0.00 0.00"), Some(0));
        // 只有一個欄位也要能吃
        assert_eq!(parse_proc_uptime("5.5"), Some(5_500));
    }

    /// 壞掉的內容回 None，不 panic。
    #[test]
    fn rejects_broken_proc_uptime() {
        assert_eq!(parse_proc_uptime(""), None);
        assert_eq!(parse_proc_uptime("notanumber x"), None);
        assert_eq!(parse_proc_uptime("-1.0 0.0"), None);
        assert_eq!(parse_proc_uptime("inf 0"), None);
    }

    /// 真的問一次：這台機器一定開機超過一秒。
    #[test]
    fn this_machine_has_been_up() {
        let ms = uptime_ms();
        assert!(ms > 1000, "開機時間看起來不對：{ms}ms");
    }

    /// 32 位元版就是截低位（照 TeraTerm 的 `GetTickCount` 繞回）。
    #[test]
    fn truncates_to_32_bits() {
        let full = uptime_ms();
        assert_eq!(uptime_ms_u32() as u64, full & 0xFFFF_FFFF);
    }
}
