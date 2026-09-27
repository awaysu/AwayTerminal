//! 恢復分頁（含畫面紀錄倒回）——搬移舊版 1.0.45 的功能。
//!
//! ## 舊版流程（`MainWindow.xaml.cs`）
//!
//! | 步驟 | 舊版 | 這裡 |
//! |---|---|---|
//! | 1. 關閉程式時先問 | `OnClosingAsk` → `ExitDialog`（勾「下次開啟恢復目前分頁（含畫面上的舊訊息）」，勾選狀態記在 `ExitRestoreTabs`） | `CloseRequested` → 前端的離開對話框 → [`exit_confirm`] |
//! | 2. 視窗先藏起來 | `Hide()`（使用者看到的是「立刻關掉」） | 同（`window.hide()`） |
//! | 3. 向前端要每個分頁的 scrollback | `CaptureBuffersAsync`：每個分頁送 `q{id}US save`，**全部一起等、最多 2.5 秒** | [`capture`]（同樣 2.5 秒上限） |
//! | 4. 暫存目錄每次重寫 | `RestoreDir` 先清空，再寫 `tab1.txt`、`tab2.txt`…（UTF-8 **不含 BOM**） | [`save`]（同檔名、同編碼） |
//! | 5. 分頁清單存進設定 | `AppSettings.SavedTabs` ＝ `List<SavedTab>`（在 settings.json 裡），`BufferFile` 記檔名 | `settings.saved_tabs`（同樣在 settings.json） |
//! | 6. 下次啟動自動恢復 | `OnLoaded`：`SavedTabs` 不是空的就 `RestoreTabs`，**不問使用者** | [`restore_list`] + 前端照序開分頁 |
//! | 7. 畫面倒回去 | `AddTab` 在 `n` 之後、`SelectTab`（`s`）之前送 `b{id}US{內容}US{分隔行}` | [`emit_buffer`]（在 `session_create` 裡呼叫，順序一樣） |
//! | 8. 分隔行 | 灰字 `──── 以上為上次關閉前的紀錄（存檔時間）────` | [`SEP_FORMAT`] |
//! | 9. 原始開啟時間 | `SavedTab.OpenedUtc` → `tab.StartUtc`，讓 tooltip 的執行時長接著算、不歸零 | `SavedTab::opened_ms` → `Tab::started_at` |
//! | 10. 個別分頁恢復失敗 | `catch { }` 跳過那一筆，其餘照開 | 前端逐筆 try |
//!
//! ## 兩個刻意的差別
//!
//! 1. **SSH 帳號**：舊版恢復 SSH 分頁時只印 `login as: ` 等使用者打帳號（因為它要把帳號塞進
//!    `ssh.exe` 的命令列）。我們存的 `SshConnParams` 已經有帳號（登入時記下來的），所以直接連；
//!    沒有帳號才會問 `login as:`——和第一次連線的行為一致。
//! 2. **密碼**：一律重問。存下來的東西裡沒有密碼欄位（見 `reconnect.rs` 的單元測試）。

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, State};

use crate::host::emit_host;
use crate::settings::SettingsStore;
use crate::tabs::TabManager;

/// 分隔行的文字（舊版 `Loc.T("term.restoredSep")`，`{0}`＝上次關閉時間）。
/// 隨語言換，所以是函式不是常數（`i18n` 的 key＝`term.restoreSeparator`）。
pub fn sep_format() -> String {
    crate::i18n::t("term.restoreSeparator")
}

/// 一個存下來的分頁（舊版 `Models/SavedTab.cs` 的子集：我們只做已經搬好的連線種類）。
///
/// **沒有密碼欄位**——舊版的 `SavedTab` 也沒有。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedTab {
    /// `shell` | `conn` | `ssh` | `telnet` | `com`（對得上 `session_create` 的 `kind`）。
    pub kind: String,
    pub title: String,
    /// `shell`／`conn`：工作目錄（**沙盒之前**的那個；沙盒會自己重新準備）。
    pub dir: String,
    /// `conn`：自訂連線的名稱。
    pub conn_name: String,
    /// 遠端／裝置連線的參數（`ssh`／`telnet`／`com`）。
    pub conn: Option<crate::reconnect::ConnParams>,
    /// scrollback 的檔名（位於 [`dir_of`]）。空＝沒存到。
    pub buffer_file: String,
    /// 分頁最初開啟的時間（epoch ms）。恢復後 tooltip 的執行時長接著算、不歸零（舊版 1.1.4）。
    pub opened_ms: u64,
}

/// scrollback 的暫存目錄（舊版 `%LOCALAPPDATA%\AwayTerminal\restore`）。
pub fn dir_of(settings: &SettingsStore) -> PathBuf {
    settings.dir().join("restore")
}

// ---------------------------------------------------------------- 收 scrollback

/// 等 `a{id}US save US…` 回來的信箱（舊版的 `_saveBufTcs`）。
type Mailbox = Mutex<Vec<(u32, Sender<String>)>>;

fn mailbox() -> &'static Mailbox {
    static M: OnceLock<Mailbox> = OnceLock::new();
    M.get_or_init(|| Mutex::new(Vec::new()))
}

/// `pane_answer` 收到 `save` 的回覆時呼叫。
pub fn deliver(id: u32, text: String) {
    let mut g = mailbox().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pos) = g.iter().position(|(i, _)| *i == id) {
        let (_, tx) = g.remove(pos);
        let _ = tx.send(text);
    }
}

/// 向前端要這些分頁的 scrollback。**全部一起等**，總共最多 `timeout`（同舊版 2.5 秒）。
fn capture(app: &AppHandle, ids: &[u32], timeout: Duration) -> Vec<(u32, String)> {
    let mut waits: Vec<(u32, Receiver<String>)> = Vec::new();
    {
        let mut g = mailbox().lock().unwrap_or_else(|e| e.into_inner());
        g.clear();
        for &id in ids {
            let (tx, rx) = mpsc::channel();
            g.push((id, tx));
            waits.push((id, rx));
        }
    }
    // 送完再等：先全部送出去，前端才能平行序列化（舊版也是先 for 迴圈送、再 WhenAll）
    for &id in ids {
        emit_host(app, format!("q{id}\x1fsave"));
    }

    let deadline = Instant::now() + timeout;
    let mut out = Vec::new();
    for (id, rx) in waits {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(text) if !text.is_empty() => out.push((id, text)),
            _ => {}
        }
    }
    mailbox().lock().unwrap_or_else(|e| e.into_inner()).clear();
    out
}

// ---------------------------------------------------------------- 存

/// 關閉程式時把可恢復的分頁存起來。`restore = false` ＝清空紀錄（同舊版沒勾的情形）。
///
/// 回傳存了幾個分頁（`--verify` 與後端 log 用）。
pub fn save(app: &AppHandle, restore: bool) -> usize {
    let Some(settings) = app.try_state::<Arc<SettingsStore>>() else {
        return 0;
    };
    let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
        return 0;
    };

    // (分頁 id, 存下來的內容)：id 只用來對回 scrollback，不會寫進設定
    let entries: Vec<(u32, SavedTab)> = if restore {
        tabs.restorable()
    } else {
        Vec::new()
    };
    // `restoreBufferLines = 0` ＝不保留畫面（同舊版：只存分頁、不存 scrollback）
    let ids: Vec<u32> = if settings.get().restore_buffer_lines > 0 {
        entries.iter().map(|(id, _)| *id).collect()
    } else {
        Vec::new()
    };
    let bufs = if ids.is_empty() {
        Vec::new()
    } else {
        capture(app, &ids, Duration::from_millis(2500))
    };

    // 暫存目錄每次重寫：舊檔全清，不累積（同舊版）
    let dir = dir_of(&settings);
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for f in rd.flatten() {
            let _ = std::fs::remove_file(f.path());
        }
    }

    let mut saved = Vec::new();
    for (n, (id, mut entry)) in entries.into_iter().enumerate() {
        if let Some((_, text)) = bufs.iter().find(|(i, _)| *i == id) {
            let name = format!("tab{}.txt", n + 1);
            // UTF-8 **不含 BOM**（舊版 `new UTF8Encoding(false)`）——log 檔那邊才有 BOM，別搞混
            match std::fs::write(dir.join(&name), text.as_bytes()) {
                Ok(()) => entry.buffer_file = name,
                Err(e) => println!("[AwayTerminal] 存畫面紀錄失敗（{name}）：{e}"),
            }
        }
        saved.push(entry);
    }

    let n = saved.len();
    settings.update(|s| s.saved_tabs = saved);
    settings.update(|s| s.exit_restore_tabs = restore);
    settings.flush();
    n
}

// ---------------------------------------------------------------- 恢復

/// 這次啟動要恢復的清單（`restore_list` 讀到之後放這裡，`emit_buffer` 依索引取用）。
fn pending() -> &'static Mutex<Vec<SavedTab>> {
    static P: OnceLock<Mutex<Vec<SavedTab>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(Vec::new()))
}

/// 前端啟動時呼叫一次：要恢復哪些分頁（空＝照平常開一個預設 shell）。
///
/// 舊版是 `OnLoaded` 直接 `RestoreTabs`，**不問使用者**——照抄。
#[tauri::command]
pub fn restore_list(settings: State<'_, Arc<SettingsStore>>) -> Vec<SavedTab> {
    let list = settings.get().saved_tabs;
    *pending().lock().unwrap_or_else(|e| e.into_inner()) = list.clone();
    if !list.is_empty() {
        println!("[AwayTerminal] 恢復分頁：{} 個", list.len());
    }
    list
}

/// 把第 `index` 筆存下的畫面倒回分頁 `id`（`b{id}US{內容}US{分隔行}`）。
///
/// 在 `session_create` 的 `n` 之後、`s` 之前呼叫——順序同舊版 `AddTab`。
/// `terminal.js` 收到 `b` 會先扣住輸出（`held`），等 pane fit 到最終寬度才寫舊內容，
/// 然後把扣住的新輸出依序補上；那段邏輯是舊版原樣搬過來的，不要改。
pub fn emit_buffer(app: &AppHandle, id: u32, index: Option<usize>) {
    let Some(index) = index else { return };
    let Some(settings) = app.try_state::<Arc<SettingsStore>>() else {
        return;
    };
    let entry = {
        let g = pending().lock().unwrap_or_else(|e| e.into_inner());
        match g.get(index) {
            Some(e) if !e.buffer_file.is_empty() => e.clone(),
            _ => return,
        }
    };
    let path = dir_of(&settings).join(&entry.buffer_file);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => return,
        Err(e) => {
            println!("[AwayTerminal] 讀畫面紀錄失敗（{}）：{e}", path.display());
            return;
        }
    };
    let when = file_time(&path);
    let sep = format!(
        "\x1b[90m{}\x1b[0m",
        sep_format().replace("{0}", &when)
    );
    // 舊內容尾端補 SGR 重置（同舊版 `text + "\x1b[0m"`）：上次死在某個顏色裡時，
    // 分隔行與新連線的輸出不該跟著那個顏色。
    emit_host(
        app,
        format!(
            "b{id}\x1f{}\x1f{}",
            crate::b64::encode(format!("{text}\x1b[0m").as_bytes()),
            crate::b64::encode(sep.as_bytes())
        ),
    );
}

/// 恢復分頁時把「最初的開啟時間」填回分頁（tooltip 的執行時長不歸零，舊版 1.1.4）。
///
/// 在 `session_create` 把分頁放進 `TabManager` 之後呼叫。
pub fn apply_opened(app: &AppHandle, id: u32, index: Option<usize>) {
    let Some(index) = index else { return };
    let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
        return;
    };
    let ms = {
        let g = pending().lock().unwrap_or_else(|e| e.into_inner());
        g.get(index).map(|e| e.opened_ms).unwrap_or(0)
    };
    tabs.set_started_at(id, ms);
}

// ---------------------------------------------------------------- 只給 --verify 用

/// **只給 `--verify` 用**：把現在的分頁存起來（等於「勾了恢復分頁就關程式」那一步），
/// 回傳存了幾筆。不會結束程式。
/// 同 [`exit_confirm`]：必須 async，否則等不到前端的回覆。
#[tauri::command]
pub async fn restore_verify_save(app: AppHandle) -> usize {
    tokio::task::spawn_blocking(move || save(&app, true))
        .await
        .unwrap_or(0)
}

/// **只給 `--verify` 用**：把存下來的紀錄清掉（不然下次真的啟動會莫名恢復一堆分頁）。
#[tauri::command]
pub async fn restore_verify_clear(app: AppHandle) -> usize {
    let n = tokio::task::spawn_blocking({
        let app = app.clone();
        move || save(&app, false)
    })
    .await
    .unwrap_or(0);
    pending().lock().unwrap_or_else(|e| e.into_inner()).clear();
    n
}

/// 檔案的最後修改時間，`yyyy-MM-dd HH:mm`（同舊版 `File.GetLastWriteTime`）。
fn file_time(path: &std::path::Path) -> String {
    let Ok(modified) = path.metadata().and_then(|m| m.modified()) else {
        return String::new();
    };
    chrono::DateTime::<chrono::Local>::from(modified)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

// ---------------------------------------------------------------- 離開程式

/// 前端的離開對話框按了「離開」。`restore` ＝勾選狀態。
///
/// 存完才真的結束程式（舊版 `FinishExitAsync` 的順序：抓畫面 → 存設定 → 收行程 → 結束）。
///
/// ⚠️ **一定要是 `async` + `spawn_blocking`**：`save` 會等前端把 `a…save` 送回來，
/// 而同步 command 在 tauri 2 是跑在**主執行緒**上——擋住主執行緒的話 webview 的 IPC
/// 根本進不來，答案永遠等不到（`--verify` 實際踩到：「存下 2 個分頁」卻一個畫面都沒存到）。
#[tauri::command]
pub async fn exit_confirm(app: AppHandle, restore: bool) {
    // 舊版 1.2.6：確認離開就先把視窗藏起來＝使用者看到的是「立刻關掉」，
    // 存畫面／設定都在看不見的狀態下做完。藏起來的 webview 照樣處理訊息（`q…save` 回得來）。
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    let n = {
        let app = app.clone();
        tokio::task::spawn_blocking(move || save(&app, restore))
            .await
            .unwrap_or(0)
    };
    println!("[AwayTerminal] 離開：恢復分頁={restore}，存了 {n} 個分頁");
    app.exit(0);
}

/// 前端的離開對話框按了「取消」——把「使用者已經按過 X」的狀態清掉。
#[tauri::command]
pub fn exit_cancel() {
    asked().store(false, std::sync::atomic::Ordering::SeqCst);
}

/// 「已經問過了嗎」。第二次按 X 時直接離開，不再等前端回答
/// （前端壞掉／listener 沒掛上時，視窗還是關得掉——舊版有 WPF 對話框保證，我們沒有）。
pub fn asked() -> &'static std::sync::atomic::AtomicBool {
    static A: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    &A
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 存下來的東西不可以有密碼欄位（連帶 `ConnParams` 也測過一次）。
    #[test]
    fn saved_tab_has_no_password_field() {
        let json = serde_json::to_string(&SavedTab {
            kind: "ssh".into(),
            conn: Some(crate::reconnect::ConnParams::Ssh(Default::default())),
            ..Default::default()
        })
        .unwrap();
        for bad in ["password", "passwd", "passphrase", "secret"] {
            assert!(!json.contains(bad), "恢復分頁不可以存 {bad}：{json}");
        }
    }

    /// 分隔行的格式（舊版 `term.restoredSep`）：灰字、含存檔時間。
    #[test]
    fn separator_matches_old_text() {
        let _g = crate::i18n::test_lock();
        crate::i18n::set_lang("zh");
        let sep = format!(
            "\x1b[90m{}\x1b[0m",
            sep_format().replace("{0}", "2026-09-27 01:23")
        );
        assert_eq!(
            sep,
            "\x1b[90m──── 以上為上次關閉前的紀錄（2026-09-27 01:23）────\x1b[0m"
        );
    }

    /// 舊設定（沒有 `savedTabs` 欄位）讀回來是空清單，不是錯誤。
    #[test]
    fn missing_field_reads_as_empty() {
        let s: SavedTab = serde_json::from_str("{}").unwrap();
        assert!(s.kind.is_empty() && s.conn.is_none() && s.buffer_file.is_empty());
    }

    /// 信箱：對得上 id 的回覆才會被拿走，其餘 id 不受影響。
    #[test]
    fn mailbox_delivers_by_id() {
        let (tx1, rx1) = mpsc::channel();
        let (tx2, _rx2) = mpsc::channel();
        {
            let mut g = mailbox().lock().unwrap();
            g.clear();
            g.push((7, tx1));
            g.push((9, tx2));
        }
        deliver(7, "hello".into());
        assert_eq!(rx1.recv_timeout(Duration::from_secs(1)).unwrap(), "hello");
        assert_eq!(mailbox().lock().unwrap().len(), 1, "只拿走 7 那一筆");
        mailbox().lock().unwrap().clear();
    }
}
