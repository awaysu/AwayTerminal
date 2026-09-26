//! Windows Job Object：沙盒模式的第二層「分頁關掉就把整棵行程樹收乾淨」。
//!
//! 規格＝`CLAUDE.md`「沙盒模式」第 2 層：
//! > 整棵 agent 行程樹放進 Windows Job Object（kill-on-close，分頁關掉就收乾淨；
//! > mac/Linux 用 process group）
//!
//! 做法：`CreateJobObject` → 設 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` →
//! `AssignProcessToJobObject(子行程)`。**關掉 job handle 的那一刻**，
//! job 裡剩下的行程會被系統一起終止。子行程再開出來的孫行程預設也在同一個 job 裡，
//! 所以 `claude` 開的 `node`、`git`、`cargo` 全都跟著收。
//!
//! ## ⚠️ 為什麼這條特別重要
//! 這個團隊自己就跑在舊版 AwayTerminal 底下（`.ai/bus/0014`）。
//! **按名稱砍行程（`taskkill /IM`）會把團隊連自己一起砍掉。**
//! Job Object 是「只收我自己開的那一棵」的正確做法——它按 handle 收，不按名稱。
//!
//! Windows 8 以後允許 nested job，所以即使 AwayTerminal 自己已經在某個 job 裡
//! （被 cargo／npm 之類放進去），再開一層也沒問題。

#![cfg(windows)]

use std::io;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
    JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// 一個 job handle。**drop 就等於終止 job 裡剩下的所有行程**（kill-on-close）。
pub struct JobObject(HANDLE);

// HANDLE 是裸指標，但這個 handle 只由持有者使用（存在 session 裡），送到別的執行緒是安全的。
unsafe impl Send for JobObject {}
unsafe impl Sync for JobObject {}

impl JobObject {
    /// 建一個 kill-on-close 的 job。
    pub fn create() -> io::Result<Self> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                let e = io::Error::last_os_error();
                CloseHandle(job);
                return Err(e);
            }
            Ok(Self(job))
        }
    }

    /// 把一個行程（以及它之後開的子孫）放進這個 job。
    ///
    /// # Safety
    /// `process` 必須是有效的行程 handle（呼叫端是剛 `CreateProcess` 出來的那個）。
    pub unsafe fn assign(&self, process: HANDLE) -> io::Result<()> {
        if AssignProcessToJobObject(self.0, process) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        // 這一行就是「分頁關掉收乾淨」：handle 關閉時系統終止 job 裡剩下的行程。
        unsafe {
            CloseHandle(self.0);
        }
    }
}
