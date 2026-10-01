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

use tauri::{AppHandle, Manager};

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
        // 沒有分頁 → 不顯示工作列的忙碌球（舊版 `Tabs.Count == 0` 那一行）
        crate::taskbar::set_busy(app, false);
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

    // Telegram 遠端：忙→閒的那一刻交給遠端服務（先算，`apply_busy` 會把舊值蓋掉）
    let idled = if crate::telegram::remote::is_running() {
        manager.busy_transitions(&busy)
    } else {
        Vec::new()
    };

    if manager.apply_busy(&busy) {
        tabs::emit_state(app, manager);
    }
    // 工作列 icon 右下：有分頁忙碌＝紅球跳動、全部閒置＝不顯示（舊版 `SetTaskbarBusy`）
    crate::taskbar::set_busy(app, busy.iter().any(|&(_, b)| b));

    for ev in idled {
        if should_push(&ev) {
            // 代理團隊只有代表列（Agent-x1）會推播（舊版 `RemoteVisible`）
            let teams = app.try_state::<Arc<crate::agent::TeamManager>>();
            if let Some(t) = &teams {
                if !t.is_strip_row(ev.id) {
                    continue;
                }
            }
            let title = teams
                .and_then(|t| t.remote_title(ev.id))
                .or_else(|| manager.title_of(ev.id))
                .unwrap_or_default();
            crate::telegram::remote::on_tab_idle(app, ev.id, &title);
        }
    }
}

/// 何時推播「完成」？區分「真的送出指令跑東西」與「只是在輸入框打字」（舊版註解照抄）：
///
/// 舊法只看「最後按鍵距轉閒 <2.5s＝打字回顯」，但在程式裡打字送出、AI 很快回答時，
/// 轉閒也在 2.5s 內 → 被誤判成打字、不推（舊版使用者實測「改在 App 發問沒丟給手機」）。
/// 改用「這段忙碌期間有沒有送出過（按 Enter／遠端 enter=true → `last_submit`）」判斷：
///   - 有送出＝真工作 → 忙 ≥0.8s 就推（含程式裡打字送出、遠端送出、快答）。
///   - 沒送出＝純打字 → 維持 2.5s 打字回顯抑制 ＋ 忙 ≥3s 門檻（擋輸入框打字噪音）。
///
/// 送出時間允許比忙碌起點早 2 秒（送出→開始輸出有延遲，尤其遠端），才不會漏判。
fn should_push(ev: &tabs::IdleEvent) -> bool {
    let submitted_this_busy = ev.last_submit + 2_000 >= ev.busy_since;
    let echo_from_typing = !submitted_this_busy && ev.since_input_ms < 2_500;
    let long_enough = if submitted_this_busy {
        ev.busy_ms >= 800
    } else {
        ev.busy_ms >= 3_000
    };
    !echo_from_typing && long_enough
}

/// 某個 PID 現在還在嗎。**唯讀**（用 Toolhelp 掃一遍，不開 handle、不砍任何東西）。
///
/// 只給 `--verify` 驗 Job Object 用（`sandbox::pid_alive`）。
#[cfg(windows)]
pub fn pid_exists(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        if Process32FirstW(snap, &mut pe) != 0 {
            loop {
                if pe.th32ProcessID == pid {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut pe) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        found
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
/// mac／Linux：掃 `/proc`（Linux）或 `proc_listpids`（mac）。實作在
/// `awayterm-platform::proctree`，那一層在 Windows 上也被三個 target 的編譯器檢查過。
#[cfg(not(windows))]
fn parents_with_children() -> std::collections::HashSet<u32> {
    awayterm_platform::proctree::parents_with_children()
}

/// 某個 PID 現在還在嗎。**唯讀**（`kill(pid, 0)` 只檢查存在與權限，不送訊號）。
///
/// 只給 `--verify` 驗沙盒的行程群組用（對應 Windows 的 Toolhelp 版本）。
#[cfg(not(windows))]
pub fn pid_exists(pid: u32) -> bool {
    awayterm_platform::proctree::pid_exists(pid)
}
