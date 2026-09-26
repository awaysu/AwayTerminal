//! 狀態燈輪詢（分頁列圖示染綠／紅）＋ 提示字元行查詢。
//!
//! 照抄舊版 `MainWindow.UpdateStatuses`（600ms 的 `DispatcherTimer`）：
//!   - PowerShell 分頁：**有外部子行程「且」近 1.5 秒持續有輸出**＝忙。
//!     停下來等使用者時行程仍活著、但不再送資料 → 閒。
//!   - claude / 自訂 exe：近 1.2 秒有輸出＝忙。
//!   - 遠端（SSH/Telnet/COM，尚未實作）：近 0.5 秒有輸出＝忙。
//!
//! 子行程判斷用 Toolhelp 快照（舊版 `ProcessTree.cs`）。
//!
//! 同一個 tick 也負責問提示字元行（`q{id}US cwd`），回覆走 `a` 協定進
//! `commands::host_answer`，用來把 shell 分頁改名成目前目錄名稱（舊版 1.1.2）。

use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;

use crate::host::emit_host;
use crate::tabs::{self, TabKind, TabManager};

/// 輪詢週期（舊版 `_statusTimer` 就是 600ms）。
const TICK: Duration = Duration::from_millis(600);

/// 各種分頁「多久沒輸出就算閒下來」的門檻，照舊版逐條對應。
const SHELL_STREAMING_MS: u64 = 1500;
const DIRECT_EXE_MS: u64 = 1200;
const REMOTE_MS: u64 = 500;

pub fn spawn(app: AppHandle, manager: Arc<TabManager>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(TICK);
        tick(&app, &manager);
    });
}

fn tick(app: &AppHandle, manager: &TabManager) {
    let snapshot = manager.poll_snapshot();
    if snapshot.is_empty() {
        return;
    }

    // 提示字元行：作用中那個一定問，其餘依種類（舊版 TracksCwdTitle）
    for id in manager.cwd_query_ids() {
        emit_host(app, format!("q{id}\x1fcwd"));
    }

    let busy_parents = parents_with_children();
    let now = tabs::now_ms();
    let busy: Vec<(u32, bool)> = snapshot
        .iter()
        .map(|&(id, kind, pid, last_output)| {
            let since = now.saturating_sub(last_output);
            let busy = match kind {
                // 有外部子行程「且」近 1.5 秒持續有輸出
                k if k.is_local_shell() => {
                    pid != 0 && busy_parents.contains(&pid) && since < SHELL_STREAMING_MS
                }
                TabKind::Claude | TabKind::Custom => since < DIRECT_EXE_MS,
                _ => since < REMOTE_MS,
            };
            (id, busy)
        })
        .collect();

    if manager.apply_busy(&busy) {
        tabs::emit_state(app, manager);
    }
}

/// 回傳「至少有一個子行程」的父 PID 集合。分頁的 PID 在裡面＝正在跑外部程式。
#[cfg(windows)]
fn parents_with_children() -> std::collections::HashSet<u32> {
    use std::collections::HashSet;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut parents = HashSet::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap == INVALID_HANDLE_VALUE {
            return parents;
        }
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        // 一定要用 W 版（結構是 Unicode 的 szExeFile[260]）——舊版 ProcessTree.cs 的註解就是踩過這個
        if Process32FirstW(snap, &mut pe) != 0 {
            loop {
                if pe.th32ParentProcessID != 0 {
                    parents.insert(pe.th32ParentProcessID);
                }
                if Process32NextW(snap, &mut pe) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    parents
}

/// mac／Linux 之後各自實作（`CLAUDE.md`：Linux 讀 `/proc`、mac 用 `libproc`）。
/// 在那之前一律回空集合＝本機 shell 分頁不會被判定成忙碌，不會亂閃。
#[cfg(not(windows))]
fn parents_with_children() -> std::collections::HashSet<u32> {
    std::collections::HashSet::new()
}
