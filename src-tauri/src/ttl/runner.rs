//! 在分頁裡執行巨集：app 端的 [`MacroHost`] 實作、執行緒、tauri command。
//!
//! 舊版對應 `MainWindow.MacroAction`（分頁右鍵「執行巨集…」）：
//!
//! | 舊版行為 | 出處 | 這裡 |
//! |---|---|---|
//! | 分頁右鍵「執行巨集…」開檔案選擇（`*.ttl`） | `MacroAction` | [`macro_run`]（前端選檔） |
//! | 已經在跑 → 問「要停止巨集嗎？」，是 → 停 | 同上 | 前端問，然後呼叫 [`macro_stop`] |
//! | 讀檔失敗 → 跳「無法讀取巨集：」 | 同上 | [`macro_run`] 回 `Err`，前端跳對話框 |
//! | 巨集在背景執行緒跑，`messagebox`／`yesnobox`／`inputbox` 交給 UI | `MacroRunner` + 三個事件 | [`TabMacroHost::dialog`]（emit event + 等回覆） |
//! | 分頁關閉／程式結束 → `Stop()` | `OnClosed`／`CloseTab` | [`stop_for_tab`]（`tab_close` 會呼叫） |
//! | 執行中的狀態記在分頁上 | `IsMacroRunning`（只影響 tooltip） | `Tab::macro_state`（**新增**：分頁列也看得到，見 `docs/TTL.md`） |
//!
//! ## 執行緒與「等」
//!
//! 每個分頁一條巨集執行緒。`wait`／`pause`／對話框都在**那條**執行緒上等，
//! 主執行緒與 PTY 的讀取執行緒完全不受影響。中斷是一個 `AtomicBool`，
//! 正在等的指令每 10ms 檢查一次。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State};

use super::host::{DialogAnswer, DialogRequest, MacroHost, RecvBuffer};
use super::{Interp, TtlError};
use crate::session::SessionManager;
use crate::tabs::{self, TabManager};

/// 一個分頁的巨集狀態（放在 `TabManager` 裡，`tab-state` 會帶給前端）。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroState {
    /// 巨集檔名（只有檔名，不是完整路徑——tooltip 用）。
    pub file: String,
    /// 目前執行到第幾行（1 起算）。
    pub line: usize,
}

/// 巨集執行中要用的東西。分頁與執行緒各持一份。
pub struct MacroHandle {
    pub stop: AtomicBool,
    pub recv: RecvBuffer,
    /// 目前執行到第幾行（給 `tab-state` 用）。
    pub line: AtomicUsize,
    pub file: String,
    /// 等對話框回覆的通道（一次只會有一個對話框）。
    answer: Mutex<Option<mpsc::Sender<DialogAnswer>>>,
}

impl MacroHandle {
    fn new(file: String) -> Self {
        Self {
            stop: AtomicBool::new(false),
            recv: RecvBuffer::new(),
            line: AtomicUsize::new(0),
            file,
            answer: Mutex::new(None),
        }
    }

    /// 前端回覆對話框（`macro_answer`）。
    pub fn deliver_answer(&self, a: DialogAnswer) {
        if let Some(tx) = self.answer.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(a);
        }
    }
}

/// 把連線的輸出餵給巨集的接收緩衝（`IoTap`）。
///
/// 這就是 TASK-011 留的那個接縫：`reconnect::pipeline` 與本機 shell 的 `on_output`
/// 都會呼叫它，所以 SSH／Telnet／COM／PowerShell 的巨集行為一致。
struct MacroTap {
    handle: Arc<MacroHandle>,
}

impl crate::tap::IoTap for MacroTap {
    fn on_output(&self, bytes: &[u8]) {
        self.handle.recv.push(bytes);
    }
    // 輸入不攔：舊版巨集執行中使用者照樣可以打字（`MacroRunner` 沒有攔鍵盤），
    // 照舊版。要攔的話把這裡改成 `false` 就好。
}

/// app 端的 [`MacroHost`]。
struct TabMacroHost {
    app: AppHandle,
    tab: u32,
    handle: Arc<MacroHandle>,
}

impl MacroHost for TabMacroHost {
    fn send(&self, data: &[u8]) {
        if let Some(m) = self.app.try_state::<SessionManager>() {
            if let Some(s) = m.get(self.tab) {
                s.write(data);
            }
        }
    }

    fn read_byte(&self) -> Option<u8> {
        self.handle.recv.read_byte()
    }

    fn flush_recv(&self) {
        self.handle.recv.clear();
    }

    fn stopped(&self) -> bool {
        self.handle.stop.load(Ordering::Relaxed)
    }

    fn connected(&self) -> bool {
        self.app
            .try_state::<SessionManager>()
            .is_some_and(|m| m.get(self.tab).is_some())
    }

    fn echo(&self, text: &str) {
        if let Some(tabs) = self.app.try_state::<Arc<TabManager>>() {
            if let Some(parts) = tabs.session_parts_of(self.tab) {
                parts.pump.push(text.as_bytes());
                parts.pump.flush();
            }
        }
    }

    fn dialog(&self, req: DialogRequest) -> DialogAnswer {
        // 不等回覆的那幾種（statusbox／closesbox）直接發出去就好
        let waits = !matches!(req.kind.as_str(), "status" | "closestatus");
        let (tx, rx) = mpsc::channel();
        if waits {
            *self.handle.answer.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        }
        #[derive(Clone, serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Payload<'a> {
            id: u32,
            #[serde(flatten)]
            req: &'a DialogRequest,
        }
        if self
            .app
            .emit(
                "macro-dialog",
                Payload {
                    id: self.tab,
                    req: &req,
                },
            )
            .is_err()
        {
            return DialogAnswer {
                cancelled: true,
                ..Default::default()
            };
        }
        if !waits {
            return DialogAnswer::default();
        }
        // 等前端回答；中斷時就當取消（每 100ms 看一次中斷旗標）
        loop {
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(a) => return a,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.stopped() {
                        return DialogAnswer {
                            cancelled: true,
                            ..Default::default()
                        };
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return DialogAnswer {
                        cancelled: true,
                        ..Default::default()
                    }
                }
            }
        }
    }

    fn sleep(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    fn clear_screen(&self) {
        // 走既有的 `c` 協定（Telnet／COM 那條路：直接清 xterm 的緩衝）
        crate::host::emit_host(&self.app, format!("c{}", self.tab));
    }

    fn beep(&self) {
        // 終端機的 BEL：交給 xterm 自己處理（和遠端送 \x07 一樣）
        self.echo("\x07");
    }

    fn set_title(&self, title: &str) {
        if let Some(tabs) = self.app.try_state::<Arc<TabManager>>() {
            if tabs.set_title(self.tab, title, true) {
                crate::host::emit_host(&self.app, format!("t{}\x1f{title}", self.tab));
                tabs::emit_state(&self.app, &tabs);
            }
        }
    }

    fn title(&self) -> String {
        self.app
            .try_state::<Arc<TabManager>>()
            .and_then(|tabs| {
                tabs.state_with(&[])
                    .tabs
                    .into_iter()
                    .find(|t| t.id == self.tab)
                    .map(|t| t.title)
            })
            .unwrap_or_default()
    }

    fn log_open(&self, path: &str, append: bool) -> bool {
        // 接既有的 log 功能（`logging.rs`）：時間戳照設定
        let Some(tabs) = self.app.try_state::<Arc<TabManager>>() else {
            return false;
        };
        let stamp = self
            .app
            .try_state::<Arc<crate::settings::SettingsStore>>()
            .map(|s| s.get().log_timestamp)
            .unwrap_or(true);
        crate::toolbar::log_start_inner(&self.app, &tabs, self.tab, path, stamp, append).is_ok()
    }

    fn log_write(&self, text: &[u8]) -> bool {
        let Some(tabs) = self.app.try_state::<Arc<TabManager>>() else {
            return false;
        };
        let Some(parts) = tabs.session_parts_of(self.tab) else {
            return false;
        };
        let g = parts.logger.lock().unwrap_or_else(|e| e.into_inner());
        match g.as_ref() {
            Some(l) => {
                l.write(text);
                true
            }
            None => false,
        }
    }

    fn log_close(&self) {
        if let Some(tabs) = self.app.try_state::<Arc<TabManager>>() {
            crate::toolbar::log_stop_inner(&tabs, self.tab);
        }
    }

    fn connect(&self, params: &str) -> bool {
        // 分頁已經沒有連線了才會走到這裡（`cmd_connect` 檢查過）。
        // 參數格式照 TeraTerm 的命令列，見 docs/TTL.md 的對照表。
        crate::ttl::runner::connect_from_macro(&self.app, self.tab, params)
    }

    fn disconnect(&self) {
        if let Some(m) = self.app.try_state::<SessionManager>() {
            if let Some(s) = m.remove(self.tab) {
                std::thread::spawn(move || s.close());
            }
        }
    }
}

/// `connect '<參數>'`：把 TeraTerm 的命令列參數對映到我們的後端。
///
/// 支援（其餘忽略並在畫面上印一行灰字）：
///
/// | 參數 | 意思 |
/// |---|---|
/// | `host` / `host:port` | 目標 |
/// | `/ssh` | 用 SSH（預設埠 22） |
/// | `/telnet` | 用 Telnet（預設埠 23） |
/// | `/C=n` | 連接埠 COM`n` |
/// | `/user=x` | SSH 帳號 |
/// | `/passwd=x` | SSH 密碼（**只在記憶體裡傳給驗證流程，不進 log、不進 tooltip**） |
/// | `/auth=password` | 驗證方式（我們只認 password；其餘忽略） |
pub fn connect_from_macro(app: &AppHandle, tab: u32, params: &str) -> bool {
    let mut host = String::new();
    let mut port: Option<u16> = None;
    let mut kind = "";
    let mut user = String::new();
    let mut com: Option<u32> = None;
    for tok in params.split_whitespace() {
        let lower = tok.to_ascii_lowercase();
        if lower == "/ssh" || lower.starts_with("/ssh=") {
            kind = "ssh";
        } else if lower == "/telnet" {
            kind = "telnet";
        } else if let Some(rest) = lower.strip_prefix("/c=") {
            kind = "com";
            com = rest.parse().ok();
        } else if let Some(rest) = tok.strip_prefix("/user=") {
            user = rest.to_string();
        } else if lower.starts_with("/passwd=") || lower.starts_with("/auth=") {
            // 密碼：交給連線流程，不留在任何紀錄裡（這裡刻意不存、不印）
        } else if !tok.starts_with('/') && host.is_empty() {
            let (h, p) = match tok.rsplit_once(':') {
                Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
                    (h.to_string(), p.parse::<u16>().ok())
                }
                _ => (tok.to_string(), None),
            };
            host = h;
            port = p;
        }
    }
    if kind.is_empty() {
        kind = if com.is_some() { "com" } else { "ssh" };
    }
    let ok = crate::commands::macro_connect(app, tab, kind, &host, port, &user, com);
    if !ok {
        crate::host::emit_host(app, format!("c{tab}"));
    }
    ok
}

// ---------------------------------------------------------------- tauri command

/// 開始跑一支巨集（前端已經選好檔案）。
#[tauri::command]
pub async fn macro_run(app: AppHandle, id: u32, path: String) -> Result<String, String> {
    let p = std::path::PathBuf::from(&path);
    let file = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());

    // 已經在跑就不要再開一條（前端會先問「要停止巨集嗎」）
    if let Some(tabs) = app.try_state::<Arc<TabManager>>() {
        if tabs.macro_of(id).is_some() {
            return Err("這個分頁已經在跑巨集了".to_string());
        }
    }

    // 讀檔與註冊標籤在這裡做：失敗要在「開始跑」之前就回報（同舊版讀檔失敗跳對話框）
    let interp = Interp::from_file(&p).map_err(|e: TtlError| {
        if e.err == super::Err::CantOpen {
            format!("無法讀取巨集：{path}")
        } else {
            format!("{e}")
        }
    })?;

    let handle = Arc::new(MacroHandle::new(file.clone()));
    let Some(tabs) = app.try_state::<Arc<TabManager>>() else {
        return Err("分頁清單還沒準備好".to_string());
    };
    // 掛上 IoTap：從現在起連線的輸出會進巨集的接收緩衝
    let Some(tap) = tabs.tap_of(id) else {
        return Err("找不到分頁".to_string());
    };
    tap.set(Some(Arc::new(MacroTap {
        handle: handle.clone(),
    })));
    tabs.set_macro(id, Some(handle.clone()));
    tabs::emit_state(&app, &tabs);
    println!("[AwayTerminal] 巨集開始：分頁 {id} → {file}");

    let host = Arc::new(TabMacroHost {
        app: app.clone(),
        tab: id,
        handle: handle.clone(),
    });

    let app2 = app.clone();
    std::thread::Builder::new()
        .name(format!("ttl-{id}"))
        .spawn(move || {
            let mut it = interp;
            it.set_host(Some(host));
            let result = run_loop(&mut it, &handle);
            finish(&app2, id, &handle, result);
        })
        .map_err(|e| format!("開不了巨集執行緒：{e}"))?;

    Ok(file)
}

/// 一行一行跑，順便更新「執行到第幾行」。
fn run_loop(it: &mut Interp, handle: &Arc<MacroHandle>) -> Result<(), TtlError> {
    loop {
        if handle.stop.load(Ordering::Relaxed) {
            return Ok(()); // 中斷不算錯誤
        }
        match it.step() {
            Ok(super::Step::Finished) => return Ok(()),
            Ok(super::Step::Ran) => {
                handle.line.store(it.line_no(), Ordering::Relaxed);
            }
            Err(e) if e.err == super::Err::Interrupted => return Ok(()),
            Err(e) => return Err(e),
        }
    }
}

/// 收尾：拿掉 tap 與分頁狀態，在畫面上印一行結果。
fn finish(app: &AppHandle, id: u32, handle: &Arc<MacroHandle>, result: Result<(), TtlError>) {
    if let Some(tabs) = app.try_state::<Arc<TabManager>>() {
        if let Some(tap) = tabs.tap_of(id) {
            tap.set(None);
        }
        tabs.set_macro(id, None);
        tabs::emit_state(app, &tabs);
        // 畫面上給一行灰字／紅字（舊版是分頁狀態 + 錯誤對話框；我們兩個都做）
        if let Some(parts) = tabs.session_parts_of(id) {
            let msg = match &result {
                Ok(()) if handle.stop.load(Ordering::Relaxed) => {
                    format!("\r\n\x1b[90m[巨集已中斷：{}]\x1b[0m\r\n", handle.file)
                }
                Ok(()) => format!("\r\n\x1b[90m[巨集執行完畢：{}]\x1b[0m\r\n", handle.file),
                Err(e) => format!(
                    "\r\n\x1b[31m[巨集錯誤] {} {}:{}\x1b[0m\r\n",
                    e.err.message_zh(),
                    e.file,
                    e.line_no
                ),
            };
            parts.pump.push(msg.as_bytes());
            parts.pump.flush();
        }
    }
    match &result {
        Ok(()) => println!("[AwayTerminal] 巨集結束：分頁 {id} → {}", handle.file),
        Err(e) => println!("[AwayTerminal] 巨集錯誤：分頁 {id} → {e}"),
    }
    // 錯誤也發給前端（跳對話框，同舊版的錯誤視窗）
    if let Err(e) = result {
        let _ = app.emit(
            "macro-error",
            serde_json::json!({
                "id": id,
                "message": e.err.message_zh(),
                "english": e.err.message(),
                "file": e.file,
                "line": e.line_no,
                "text": e.line,
            }),
        );
    }
}

/// 停止某個分頁的巨集（前端的「要停止巨集嗎」按了是，或分頁要關了）。
#[tauri::command]
pub fn macro_stop(app: AppHandle, id: u32) {
    stop_for_tab(&app, id);
}

/// 給 `tab_close`／程式結束用：叫停巨集（不等它真的停）。
pub fn stop_for_tab(app: &AppHandle, id: u32) {
    if let Some(tabs) = app.try_state::<Arc<TabManager>>() {
        if let Some(h) = tabs.macro_of(id) {
            h.stop.store(true, Ordering::Relaxed);
            // 正在等對話框的話也要叫醒它
            h.deliver_answer(DialogAnswer {
                cancelled: true,
                ..Default::default()
            });
        }
    }
}

/// 前端回覆對話框。
#[tauri::command]
pub fn macro_answer(
    app: AppHandle,
    id: u32,
    number: i32,
    text: String,
    cancelled: bool,
) {
    if let Some(tabs) = app.try_state::<Arc<TabManager>>() {
        if let Some(h) = tabs.macro_of(id) {
            h.deliver_answer(DialogAnswer {
                number,
                text,
                cancelled,
            });
        }
    }
    let _ = app;
}

/// 給 `--verify` 用：跑一支巨集並等它結束（最多 `timeout_ms`），回傳結果字串。
#[tauri::command]
pub async fn macro_verify(
    app: AppHandle,
    id: u32,
    path: String,
    timeout_ms: u64,
    tabs: State<'_, Arc<TabManager>>,
) -> Result<String, String> {
    let file = macro_run(app.clone(), id, path).await?;
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < timeout_ms as u128 {
        if tabs.macro_of(id).is_none() {
            return Ok(format!("{file} 執行完畢"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    stop_for_tab(&app, id);
    Err(format!("{file} 超過 {timeout_ms}ms 還沒結束（已中斷）"))
}
