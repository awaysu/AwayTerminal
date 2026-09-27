//! 離開程式前請 Claude Code 更新 `CLAUDE.md`（搬移舊版 `MainWindow.UpdateClaudeMdAsync`
//! ＋ `Dialogs/ExitDialog` 的那個勾選）。
//!
//! 對象＝**一般的 Claude Code 分頁**：連線還活著、而且**不是代理團隊的一格**。
//! 舊版 1.2.0 的註解寫得很清楚：好幾個 agent 同時改同一份 `CLAUDE.md` 會互相覆蓋，
//! 所以代理團隊的 claude 不算。
//!
//! 時序照舊版，一格都不能省：
//! 1. 對每個 claude 分頁寫入提示句（**不含 CR**）
//! 2. 等 300ms（`ENTER_DELAY_MS`）再單獨送 `\r`
//!    ——一次寫入「文字＋CR」claude 會當成貼上、CR 變成輸入框換行、**沒送出**（舊版 1.1.10 實測）
//! 3. 等 2500ms 讓 claude 開始處理
//! 4. 最多等 180 秒，直到**每個** claude 分頁都超過 4000ms 沒有新輸出

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager, State};

use crate::agent::TeamManager;
use crate::session::SessionManager;
use crate::tabs::{TabKind, TabManager};

/// 送出提示之後先等多久才開始看「有沒有安靜下來」。
const SETTLE_MS: u64 = 2500;
/// 最多等多久（舊版 180 秒）。
const DEADLINE_SEC: u64 = 180;
/// 連續這麼久沒有新輸出就算寫完了（舊版 4000ms）。
const QUIET_MS: u64 = 4000;

/// 哪些分頁要請它更新（舊版 `Tabs.Where(t => t.ClaudePaste && t.Session != null && t.Agent == null)`）。
pub fn targets(
    tabs: &Arc<TabManager>,
    sessions: &SessionManager,
    teams: &Arc<TeamManager>,
) -> Vec<u32> {
    tabs.ids()
        .into_iter()
        .filter(|id| tabs.kind_of(*id) == Some(TabKind::Claude))
        .filter(|id| sessions.get(*id).is_some())
        .filter(|id| teams.find_tab(*id).is_none())
        .collect()
}

/// 有沒有可以請的分頁（離開對話框用它決定要不要停用那個勾選）。
#[tauri::command]
pub fn claude_md_available(
    tabs: State<'_, Arc<TabManager>>,
    sessions: State<'_, SessionManager>,
    teams: State<'_, Arc<TeamManager>>,
) -> bool {
    !targets(&tabs, &sessions, &teams).is_empty()
}

/// 請每個 Claude Code 分頁更新 `CLAUDE.md`，等到它們都閒下來（或逾時）。回傳問了幾個分頁。
///
/// `async`＋`spawn`：這條路要等最多 3 分鐘，絕不能佔著 tauri 的主執行緒
/// （sync command 會讓整個視窗停住）。
#[tauri::command]
pub async fn claude_md_update(app: AppHandle) -> usize {
    let (tabs, sessions, teams) = {
        let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
            return 0;
        };
        let Some(sessions) = app.try_state::<SessionManager>() else {
            return 0;
        };
        let Some(teams) = app.try_state::<Arc<TeamManager>>() else {
            return 0;
        };
        (tabs, sessions, teams)
    };
    let ids = targets(&tabs, &sessions, &teams);
    if ids.is_empty() {
        return 0;
    }
    let prompt = crate::i18n::t("exit.mdPrompt");
    println!(
        "[AwayTerminal] 離開前請 {} 個 Claude Code 分頁更新 CLAUDE.md（代理團隊的格不算）",
        ids.len()
    );
    for id in &ids {
        // 重置活動計時：底下的「安靜 4 秒」要從現在開始算
        tabs.mark_input(*id, false);
        if let Some(s) = sessions.get(*id) {
            s.write(prompt.as_bytes());
        }
    }
    tokio::time::sleep(Duration::from_millis(
        crate::agent::deliver::ENTER_DELAY_MS,
    ))
    .await;
    for id in &ids {
        if let Some(s) = sessions.get(*id) {
            s.write(b"\r");
        }
    }
    tokio::time::sleep(Duration::from_millis(SETTLE_MS)).await;

    let deadline = std::time::Instant::now() + Duration::from_secs(DEADLINE_SEC);
    while std::time::Instant::now() < deadline {
        let now = crate::tabs::now_ms();
        let all_quiet = ids.iter().all(|id| {
            tabs.agent_signals(*id)
                .map(|g| now.saturating_sub(g.last_output) > QUIET_MS)
                .unwrap_or(true)
        });
        if all_quiet {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    println!("[AwayTerminal] CLAUDE.md 更新等待結束");
    ids.len()
}
