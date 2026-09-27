//! Windows ConPTY 連線後端。
//!
//! 照舊版 `ConPty/*.cs` 逐段翻寫，**不用 `portable-pty`**：因為要能載入 Windows Terminal 的
//! `conpty.dll`（OpenConsole）並保留舊版踩過的雷（行程結束偵測、優雅結束鍵、輸入管線加鎖…）。

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, FALSE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, GetStdHandle, ResizePseudoConsole, SetStdHandle, COORD,
    HPCON, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
    ResumeThread, WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    STARTUPINFOEXW,
};

use super::conpty_host;
use crate::session::{ExitInfo, OnExit, OnOutput, TerminalSession};

/// `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`（windows-sys 沒有匯出這個常數）。
const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x0002_0016;

/// 讀取緩衝大小，同舊版 `ConPtySession.ReadLoopAsync`。
const READ_BUF: usize = 65536;

/// 行程結束後延遲多久才通知前端，讓 conhost 把最後一批輸出送完（舊版：150ms）。
const EXIT_SETTLE: Duration = Duration::from_millis(150);

/// 送出優雅結束鍵之後等多久才強制收尾（舊版：60ms）。
const GRACEFUL_WAIT: Duration = Duration::from_millis(60);

/// HANDLE 是 `*mut c_void`、本身不是 Send；這些 handle 的擁有者執行緒是明確的
/// （讀取執行緒擁有輸出讀端、等待執行緒擁有自己複製的 process handle）。
#[derive(Clone, Copy)]
struct SendHandle(HANDLE);
unsafe impl Send for SendHandle {}
unsafe impl Sync for SendHandle {}

impl SendHandle {
    /// 取出原始 handle。
    ///
    /// 用方法而不是直接讀 `.0`：edition 2021 的 closure 精準捕捉（RFC 2229）看到 `h.0`
    /// 只會捕捉那個欄位（`*mut c_void`，不是 Send），整個 struct 才有我們的 `unsafe impl Send`。
    fn raw(self) -> HANDLE {
        self.0
    }
}

pub struct ConPtySession {
    hpc: isize,
    /// 這條 pseudoconsole 是不是跑在新版 OpenConsole 上（決定 resize/close 走哪個 API）。
    open_console: bool,
    /// 輸入管線的「寫端」（送鍵盤）。
    write_side: SendHandle,
    h_process: SendHandle,
    h_thread: SendHandle,
    pid: u32,
    /// 輸入管線不是執行緒安全的：UI 打字、巨集、遠端指令、close() 的 Ctrl+C 可能同時寫，
    /// 沒鎖會互相蓋掉位元組（按鍵消失）。照舊版 `_writeLock`。
    write_lock: Mutex<()>,
    /// 關閉前送出的「優雅結束」位元組。預設 Ctrl+C ×3（PowerShell / Claude Code 的離開方式）。
    graceful_exit_bytes: Vec<u8>,
    /// 已由我方主動 close()：之後的結束事件不再往前端送。
    closing: Arc<AtomicBool>,
    closed: AtomicBool,
    /// 沙盒模式的 Job Object。**drop 就終止 job 裡剩下的整棵行程樹**。
    /// `None`＝沒開沙盒（行為與 TASK-002 起完全一樣）。
    ///
    /// 這個欄位刻意沒有讀取者：它的作用**就是被持有**——session 活著 job 就活著，
    /// session 被 drop（分頁關閉／程式結束）時 `JobObject::drop` 關掉 handle，
    /// 系統就把 job 裡剩下的行程一起收掉。
    #[allow(dead_code)]
    job: Option<super::job::JobObject>,
}

pub struct ConPtyOptions {
    /// 完整命令列（含引號），同舊版 `ProcessFactory.Start` 的 commandLine。
    pub command_line: String,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub graceful_exit_bytes: Vec<u8>,
    /// 追加的環境變數（沙盒模式用）。空的＝完全繼承父行程的環境。
    pub env: Vec<(String, String)>,
    /// 把子行程放進 kill-on-close 的 Job Object（沙盒模式）。
    pub kill_on_close: bool,
}

impl ConPtySession {
    pub fn spawn(opts: ConPtyOptions, on_output: OnOutput, on_exit: OnExit) -> io::Result<Self> {
        let cols = if opts.cols < 1 { 80 } else { opts.cols };
        let rows = if opts.rows < 1 { 24 } else { opts.rows };

        unsafe {
            // 兩對匿名管線：輸入（我方寫 / pty 讀）、輸出（pty 寫 / 我方讀）
            let (in_read, in_write) = create_pipe()?;
            let (out_read, out_write) = create_pipe()?;

            let size = COORD {
                X: cols as i16,
                Y: rows as i16,
            };

            // 不要用 PSEUDOCONSOLE_INHERIT_CURSOR：會讓 conhost 發 DSR，
            // 時機不對會把 ESC[r;cR 漏進子行程的輸入（舊版踩雷紀錄）。
            let host = conpty_host::host();
            let hpc: isize = match host {
                Some(h) => match h.create(size, in_read, out_write, 0) {
                    Ok(hpc) => hpc,
                    Err(hr) => {
                        close(in_read);
                        close(in_write);
                        close(out_read);
                        close(out_write);
                        return Err(io::Error::other(crate::i18n::tf(
                            "err.conptyCreate",
                            &[&format!("{hr:08X}")],
                        )));
                    }
                },
                None => {
                    let mut hpc: HPCON = 0;
                    let hr = CreatePseudoConsole(size, in_read, out_write, 0, &mut hpc);
                    if hr != 0 {
                        close(in_read);
                        close(in_write);
                        close(out_read);
                        close(out_write);
                        return Err(io::Error::other(crate::i18n::tf(
                            "err.createPseudoCon",
                            &[&format!("{hr:08X}")],
                        )));
                    }
                    hpc
                }
            };

            // 沙盒模式：先把 job 建好，子行程才能在「還沒開始跑」之前就被放進去
            // （見 job.rs；用 CREATE_SUSPENDED + AssignProcessToJobObject + ResumeThread）。
            let job = if opts.kill_on_close {
                match super::job::JobObject::create() {
                    Ok(j) => Some(j),
                    Err(e) => {
                        // 建不起來不該讓連線開不了：退成「沒有 job」並講出來
                        println!("[AwayTerminal] 沙盒：Job Object 建立失敗，分頁關閉時不會自動收乾淨（{e}）");
                        None
                    }
                }
            } else {
                None
            };

            let proc_info = match start_process(
                &opts.command_line,
                hpc,
                opts.cwd.as_deref(),
                &opts.env,
                job.as_ref(),
            ) {
                Ok(pi) => pi,
                Err(e) => {
                    close_pty(hpc, host.is_some());
                    close(in_read);
                    close(in_write);
                    close(out_read);
                    close(out_write);
                    return Err(e);
                }
            };

            // 子行程已掛上：放掉 conpty.dll 持有的參考 handle（內建 conhost 後端沒這個 API）
            if let Some(h) = host {
                h.release(hpc);
            }

            // 另兩端已由 pseudoconsole 複製走，釋放父行程持有的副本
            close(in_read);
            close(out_write);

            let exit_raised = Arc::new(AtomicBool::new(false));
            let closing = Arc::new(AtomicBool::new(false));

            let session = ConPtySession {
                hpc,
                open_console: host.is_some(),
                write_side: SendHandle(in_write),
                h_process: SendHandle(proc_info.hProcess),
                h_thread: SendHandle(proc_info.hThread),
                pid: proc_info.dwProcessId,
                write_lock: Mutex::new(()),
                graceful_exit_bytes: opts.graceful_exit_bytes,
                closing: closing.clone(),
                closed: AtomicBool::new(false),
                job,
            };

            // 讀取執行緒：擁有輸出讀端，自己在結束時關掉它
            // （不要從外面關正在 blocking ReadFile 的 handle）。
            // 它拿一份自己的 process handle 複製，才能在管線 EOF 時安全查 exit code。
            spawn_reader(
                SendHandle(out_read),
                duplicate(proc_info.hProcess)?,
                on_output,
                on_exit.clone(),
                exit_raised.clone(),
                closing.clone(),
            );

            // 行程結束偵測：**一定要等 process handle**。ConPTY 的內建 conhost 在子行程結束後
            // 不會關輸出管線（要等我們 ClosePseudoConsole），光靠讀取迴圈的 EOF 永遠等不到
            // ——舊版實測 claude 按 Ctrl+C 離開、powershell 打 exit 後分頁毫無反應就是這個原因。
            spawn_waiter(
                duplicate(proc_info.hProcess)?,
                on_exit,
                exit_raised,
                closing,
            );

            Ok(session)
        }
    }
}

fn spawn_reader(
    read_side: SendHandle,
    h_process_dup: SendHandle,
    on_output: OnOutput,
    on_exit: OnExit,
    exit_raised: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
) {
    let _ = std::thread::Builder::new()
        .name("conpty-read".into())
        .spawn(move || {
            let mut buf = vec![0u8; READ_BUF];
            loop {
                let mut read: u32 = 0;
                let ok = unsafe {
                    ReadFile(
                        read_side.raw(),
                        buf.as_mut_ptr(),
                        buf.len() as u32,
                        &mut read,
                        std::ptr::null_mut(),
                    )
                };
                if ok == 0 || read == 0 {
                    break;
                }
                on_output(&buf[..read as usize]);
            }
            unsafe { close(read_side.raw()) };
            // 管線真的關了（例如 OpenConsole 自己收掉）也算結束；已發過就不重發。
            raise_exit(&exit_raised, &closing, &on_exit, h_process_dup, false);
            unsafe { close(h_process_dup.raw()) };
        });
}

fn spawn_waiter(
    h_process_dup: SendHandle,
    on_exit: OnExit,
    exit_raised: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
) {
    let _ = std::thread::Builder::new()
        .name("conpty-wait".into())
        .spawn(move || {
            unsafe {
                WaitForSingleObject(h_process_dup.raw(), INFINITE);
            }
            raise_exit(&exit_raised, &closing, &on_exit, h_process_dup, true);
            unsafe { close(h_process_dup.raw()) };
        });
}

/// 結束通知：只發一次；我方主動 close() 之後不發。
fn raise_exit(
    exit_raised: &AtomicBool,
    closing: &AtomicBool,
    on_exit: &OnExit,
    h_process: SendHandle,
    settle: bool,
) {
    if exit_raised.swap(true, Ordering::SeqCst) {
        return;
    }
    if settle {
        // 讓 conhost 把最後一批輸出送完再通知（舊版 150ms）
        std::thread::sleep(EXIT_SETTLE);
    }
    if closing.load(Ordering::SeqCst) {
        return;
    }
    let code = unsafe {
        let mut code: u32 = 0;
        if GetExitCodeProcess(h_process.raw(), &mut code) != 0 {
            Some(code as i32)
        } else {
            None
        }
    };
    on_exit(ExitInfo { exit_code: code });
}

impl TerminalSession for ConPtySession {
    fn write(&self, data: &[u8]) {
        if data.is_empty() || self.closed.load(Ordering::SeqCst) {
            return;
        }
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut offset = 0usize;
        while offset < data.len() {
            let mut written: u32 = 0;
            let ok = unsafe {
                WriteFile(
                    self.write_side.raw(),
                    data[offset..].as_ptr(),
                    (data.len() - offset) as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || written == 0 {
                break; // 管線已關（行程結束）——丟掉即可，同舊版吞掉 IOException
            }
            offset += written as usize;
        }
    }

    fn resize(&self, cols: u16, rows: u16) {
        if cols < 1 || rows < 1 || self.closed.load(Ordering::SeqCst) {
            return;
        }
        let size = COORD {
            X: cols as i16,
            Y: rows as i16,
        };
        unsafe {
            if self.open_console {
                if let Some(h) = conpty_host::host() {
                    h.resize(self.hpc, size);
                }
            } else {
                ResizePseudoConsole(self.hpc, size);
            }
        }
    }

    fn pid(&self) -> u32 {
        self.pid
    }

    fn backend_name(&self) -> &'static str {
        if self.open_console {
            "conpty.dll (OpenConsole)"
        } else {
            "inbox conhost"
        }
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.closing.store(true, Ordering::SeqCst);

        // 先一次送出「優雅結束」按鍵（PowerShell / Claude Code＝Ctrl+C ×3；
        // 之後做 SSH 時建議設 Ctrl+D ×2），只短暫等待就往下強制收尾。
        if !self.graceful_exit_bytes.is_empty() {
            {
                let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
                let mut written: u32 = 0;
                unsafe {
                    WriteFile(
                        self.write_side.raw(),
                        self.graceful_exit_bytes.as_ptr(),
                        self.graceful_exit_bytes.len() as u32,
                        &mut written,
                        std::ptr::null_mut(),
                    );
                }
            }
            std::thread::sleep(GRACEFUL_WAIT);
        }

        unsafe {
            // 先終止子行程，ClosePseudoConsole 與管線關閉才會立即返回（否則會阻塞）
            TerminateProcess(self.h_process.raw(), 0);
            close_pty(self.hpc, self.open_console);
            // 關掉輸入寫端；輸出讀端由讀取執行緒自己關
            // （它此刻的 ReadFile 會因為管線被關而返回）。
            close(self.write_side.raw());
            close(self.h_thread.raw());
            close(self.h_process.raw());
        }
    }
}

impl Drop for ConPtySession {
    fn drop(&mut self) {
        TerminalSession::close(self);
    }
}

// ---------------------------------------------------------------- Win32 助手

unsafe fn create_pipe() -> io::Result<(HANDLE, HANDLE)> {
    let mut read: HANDLE = INVALID_HANDLE_VALUE;
    let mut write: HANDLE = INVALID_HANDLE_VALUE;
    if CreatePipe(&mut read, &mut write, std::ptr::null(), 0) == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((read, write))
}

unsafe fn close(h: HANDLE) {
    if !h.is_null() && h != INVALID_HANDLE_VALUE {
        CloseHandle(h);
    }
}

unsafe fn close_pty(hpc: HPCON, open_console: bool) {
    if open_console {
        if let Some(h) = conpty_host::host() {
            h.close(hpc);
            return;
        }
    }
    ClosePseudoConsole(hpc);
}

unsafe fn duplicate(h: HANDLE) -> io::Result<SendHandle> {
    let mut dup: HANDLE = INVALID_HANDLE_VALUE;
    let me = GetCurrentProcess();
    if DuplicateHandle(me, h, me, &mut dup, 0, FALSE, DUPLICATE_SAME_ACCESS) == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(SendHandle(dup))
}

/// 以指定的 ConPTY 啟動子行程（舊版 `ProcessFactory.Start`）。
unsafe fn start_process(
    command_line: &str,
    hpc: isize,
    cwd: Option<&str>,
    extra_env: &[(String, String)],
    job: Option<&super::job::JobObject>,
) -> io::Result<PROCESS_INFORMATION> {
    let mut si: STARTUPINFOEXW = std::mem::zeroed();
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;

    // 先問所需的屬性清單大小（第一次呼叫預期失敗）
    let mut attr_size: usize = 0;
    InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attr_size);
    if attr_size == 0 {
        return Err(io::Error::other(
            crate::i18n::t("err.attrListSizeZero"),
        ));
    }
    let mut attr_buf = vec![0u8; attr_size];
    let attr_list = attr_buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;

    if InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_size) == 0 {
        return Err(io::Error::last_os_error());
    }

    let result = (|| {
        if UpdateProcThreadAttribute(
            attr_list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            hpc as *const std::ffi::c_void,
            std::mem::size_of::<isize>(),
            std::ptr::null_mut(),
            std::ptr::null(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        si.lpAttributeList = attr_list;

        let mut cmd: Vec<u16> = command_line
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let cwd_w: Option<Vec<u16>> = cwd
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.encode_utf16().chain(std::iter::once(0)).collect());

        // 沙盒模式的環境變數：**在現有環境之上覆寫**（絕對不是只給這幾個），
        // 否則子行程會失去 PATH、SystemRoot 等一切東西。
        // `HOME`／`APPDATA`／`USERPROFILE` 由呼叫端保證不在 extra_env 裡（見 sandbox.rs）。
        let env_block = build_env_block(extra_env);
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        // job 要在子行程開始跑之前就掛上，否則它可能先開出孫行程而漏掉
        let mut flags = EXTENDED_STARTUPINFO_PRESENT;
        if job.is_some() {
            flags |= CREATE_SUSPENDED;
        }
        if env_block.is_some() {
            flags |= CREATE_UNICODE_ENVIRONMENT;
        }
        // std handle 一定要在 CreateProcess 的那一刻是空的，見 with_null_std_handles。
        let ok = with_null_std_handles(|| {
            CreateProcessW(
                std::ptr::null(),
                cmd.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                FALSE,
                flags,
                env_block
                    .as_ref()
                    .map_or(std::ptr::null(), |v| v.as_ptr() as *const _),
                cwd_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
                &mut si as *mut STARTUPINFOEXW as *mut _,
                &mut pi,
            )
        });
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if let Some(job) = job {
            if let Err(e) = job.assign(pi.hProcess) {
                println!("[AwayTerminal] 沙盒：行程放不進 Job Object（{e}）");
            }
            // 不管有沒有掛成功都一定要放它跑，否則分頁永遠是空的
            ResumeThread(pi.hThread);
        }
        Ok(pi)
    })();

    DeleteProcThreadAttributeList(attr_list);
    result
}

/// 組出 `CreateProcessW` 要的 UTF-16 環境區塊：現有環境 + `extra` 覆寫。
///
/// `extra` 空的時候回 `None`＝讓子行程直接繼承（和沙盒關閉時完全一樣的行為）。
unsafe fn build_env_block(extra: &[(String, String)]) -> Option<Vec<u16>> {
    if extra.is_empty() {
        return None;
    }
    // 環境變數名在 Windows 不分大小寫，所以用大寫當 key 比對才不會出現兩個 TEMP
    let mut map: std::collections::BTreeMap<String, (String, String)> = std::env::vars()
        .map(|(k, v)| (k.to_ascii_uppercase(), (k, v)))
        .collect();
    for (k, v) in extra {
        map.insert(k.to_ascii_uppercase(), (k.clone(), v.clone()));
    }
    let mut block: Vec<u16> = Vec::new();
    for (_, (k, v)) in map {
        block.extend(format!("{k}={v}").encode_utf16());
        block.push(0);
    }
    block.push(0); // 區塊結尾再一個 NUL
    Some(block)
}

/// 建立 console 子行程期間把自己的三個 std handle 暫時歸零。
///
/// 舊版踩雷（2026-08-15，v1.0.28）：`CreateProcess` 對 console 子行程會把父行程的 std handle
/// **值**原樣帶進子行程，而 **ConPTY 連接只在 std handle 為空時才換成 pseudoconsole 的 handle**。
/// 所以父行程若有 stdout（從腳本／CI／Claude Code 工具環境啟動的 pipe，或 `cargo run` 的主控台），
/// 子行程會寫到那裡而不是 PTY：pipe 的情況 powershell / claude 0.5 秒內死掉（分頁只剩游標），
/// 主控台的情況輸出跑到父行程的視窗、PTY 只收到 conhost 的開場序列
/// （本專案 pty_probe 首次執行就重現了後者）。
///
/// 舊版是在 `App.OnStartup` 一次性全域歸零；這裡改成**只包住 `CreateProcess` 這一瞬間**再還原，
/// 好處是不會弄壞自己的 stdout（debug build 的診斷 log、pty_probe 都還能印），
/// 副作用範圍也小。用 mutex 串起來避免兩條 session 同時開在歸零視窗裡互相干擾。
unsafe fn with_null_std_handles<T>(f: impl FnOnce() -> T) -> T {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

    const IDS: [u32; 3] = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
    let saved = IDS.map(|id| GetStdHandle(id));
    for id in IDS {
        SetStdHandle(id, std::ptr::null_mut());
    }
    let result = f();
    for (id, h) in IDS.into_iter().zip(saved) {
        SetStdHandle(id, h);
    }
    result
}
