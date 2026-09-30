//! PTY 輸出的批次合併與送出。
//!
//! 讀取執行緒（blocking `ReadFile`）不直接送 IPC：它只把 bytes 丟進這裡的緩衝，
//! 由 pump 執行緒合併成一包再送。理由有兩個：
//!
//! 1. 讀取執行緒不能被 IPC 的延遲拖住，否則管線滿了會反壓到子行程。
//! 2. tauri 的 `Channel` 對 `InvokeResponseBody::Raw` 有 1024 bytes 門檻：**小於門檻會被
//!    序列化成 JSON 數字陣列用 `eval` 送**（`tauri/src/ipc/channel.rs`
//!    `MAX_RAW_DIRECT_EXECUTE_THRESHOLD`），只有大於門檻才走 fetch 自訂協定拿到真二進位。
//!    合併之後，大量輸出（`cat` 大檔）自然落在快的那條路上。
//!
//! 互動打字時每包很小、還是走 eval 那條路，但那時候的絕對量是幾個 byte，成本可忽略。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

use tauri::ipc::{Channel, InvokeResponseBody};

/// 單次送出的上限。超過的留到下一包，避免一次塞爆 webview。
const MAX_CHUNK: usize = 256 * 1024;

pub struct OutputPump {
    state: Mutex<PumpState>,
    cv: Condvar,
    stopped: AtomicBool,
    /// 留一份 channel：pump 停掉之後（行程自己結束）巨集 `connect`／重連要能用同一條
    /// channel 再開一條 pump（[`ensure_running`](Self::ensure_running)）。
    channel: Channel<InvokeResponseBody>,
}

#[derive(Default)]
struct PumpState {
    buf: Vec<u8>,
    /// 已收到停止要求：排空後結束。
    draining: bool,
}

impl OutputPump {
    pub fn start(channel: Channel<InvokeResponseBody>) -> std::sync::Arc<Self> {
        let pump = std::sync::Arc::new(OutputPump {
            state: Mutex::new(PumpState::default()),
            cv: Condvar::new(),
            stopped: AtomicBool::new(false),
            channel,
        });
        Self::spawn_worker(&pump);
        pump
    }

    fn spawn_worker(pump: &std::sync::Arc<Self>) {
        let worker = pump.clone();
        let channel = pump.channel.clone();
        let spawned = std::thread::Builder::new()
            .name("pty-output-pump".into())
            .spawn(move || worker.run(channel));
        if spawned.is_err() {
            // 執行緒開不起來：標成已停，flush_and_stop 才不會白等 2 秒
            pump.stopped.store(true, Ordering::SeqCst);
        }
    }

    /// pump 已經停了（行程自己結束 → `flush_and_stop`）就用同一條 channel 重開一條。
    ///
    /// 巨集的 `connect` 與重連沿用分頁既有的 pump；PTY 分頁的行程結束後 pump 是
    /// draining／stopped 狀態，不重開的話新連線的輸出會全部被丟掉（C4）。
    /// 還在跑的 pump 什麼都不做。
    pub fn ensure_running(self: &std::sync::Arc<Self>) {
        let draining = self.state.lock().unwrap_or_else(|e| e.into_inner()).draining;
        if !draining {
            return;
        }
        // 正在排空（flush_and_stop 進行中）→ 等它真的停
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !self.stopped.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if !self.stopped.load(Ordering::SeqCst) {
            return; // 舊的 worker 卡住了，不要同時開兩條
        }
        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.buf.clear();
            st.draining = false;
        }
        self.stopped.store(false, Ordering::SeqCst);
        Self::spawn_worker(self);
    }

    /// 關分頁用：丟掉還沒送的輸出、叫 pump 結束，**不等**（在 IPC 執行緒上呼叫）。
    ///
    /// 分頁已經 `x{id}` 拆掉了，剩下的輸出沒有人要看。
    pub fn stop(&self) {
        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.buf.clear();
            st.draining = true;
        }
        self.cv.notify_all();
    }

    /// 讀取執行緒呼叫：只是把 bytes 接到緩衝後面。
    pub fn push(&self, bytes: &[u8]) {
        if bytes.is_empty() || self.stopped.load(Ordering::SeqCst) {
            return;
        }
        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.draining {
                return;
            }
            st.buf.extend_from_slice(bytes);
        }
        self.cv.notify_one();
    }

    /// 叫 pump 立刻把緩衝送出去，但**不結束**它。
    ///
    /// 斷線重連要沿用同一條 pump（同一條輸出 channel），所以不能用
    /// [`flush_and_stop`](Self::flush_and_stop)——那會讓之後的輸出全部被丟掉。
    pub fn flush(&self) {
        self.cv.notify_all();
    }

    /// 要求排空剩餘輸出後結束 pump；回傳前會等 pump 真的排完
    /// （這樣結束事件一定排在最後一批輸出之後）。
    pub fn flush_and_stop(&self) {
        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.draining = true;
        }
        self.cv.notify_all();

        // 等緩衝清空（pump 送完最後一包才會 stopped）
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !self.stopped.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn run(&self, channel: Channel<InvokeResponseBody>) {
        loop {
            let chunk = {
                let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
                while st.buf.is_empty() && !st.draining {
                    st = self.cv.wait(st).unwrap_or_else(|e| e.into_inner());
                }
                if st.buf.is_empty() {
                    // draining 且已排空
                    break;
                }
                if st.buf.len() <= MAX_CHUNK {
                    std::mem::take(&mut st.buf)
                } else {
                    let rest = st.buf.split_off(MAX_CHUNK);
                    std::mem::replace(&mut st.buf, rest)
                }
            };

            if channel.send(InvokeResponseBody::Raw(chunk)).is_err() {
                // webview 已關閉
                break;
            }
        }
        self.stopped.store(true, Ordering::SeqCst);
    }
}
