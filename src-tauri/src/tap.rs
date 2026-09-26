//! 輸入／輸出攔截（**TTL 巨集要用的介面**，TASK-011 只留介面，本體是 TASK-012／013）。
//!
//! ## 為什麼現在就要留
//!
//! TTL 巨集的兩個核心指令是 `wait`（等遠端吐出某個字串）與 `send`（替使用者打字），
//! 也就是說它需要**看得到輸出流、寫得進輸入流**。這兩條路在我們的架構裡分別是：
//!
//! - 輸出：`reconnect::pipeline` 的 `on_output`（PTY／SSH／Telnet／COM 都經過這裡）
//! - 輸入：`commands::session_write` / `session_write_text`
//!
//! 如果等到寫巨集時才回頭改這兩處，會動到所有後端的熱路徑；先留好接縫，
//! 之後 TTL 只要實作一個 [`IoTap`] 掛上去就好。**現在每個分頁的 tap 都是 `None`**，
//! 成本是一次 `Mutex` 讀取（輸出是整批 chunk 一次，不是每個位元組）。
//!
//! ## 舊版怎麼做的
//!
//! 舊版的 TTL 解譯器（`Services/Macro/`）直接拿 `TerminalTab.Session` 來寫，
//! 並在 `OnSessionOutput` 裡加一段「巨集在等字串嗎」的判斷。語意一樣，
//! 只是我們把它收成一個明確的介面，而不是散在輸出處理裡。

use std::sync::{Arc, Mutex};

/// 一個掛在分頁上的攔截器。實作者（TTL 解譯器）要注意：
///
/// - [`IoTap::on_output`] 在**後端的讀取執行緒**上被呼叫，不可以阻塞（不要在裡面等使用者）。
/// - [`IoTap::on_input`] 回 `false` ＝這段輸入**不要**送給遠端（例如巨集執行中吃掉鍵盤）。
pub trait IoTap: Send + Sync {
    /// 遠端來的位元組。給 `wait` / `waitln` / `recvln` 這類指令比對用。
    fn on_output(&self, bytes: &[u8]);

    /// 要送出去的位元組（鍵盤或 `send`）。回 `false` 就攔下不送。
    fn on_input(&self, _bytes: &[u8]) -> bool {
        true
    }
}

/// 分頁上放 tap 的槽。和 log 的槽一樣是「事後才掛上」的東西
/// （輸出 callback 在建 session 時就固定了，沒有槽就沒辦法之後掛）。
#[derive(Clone, Default)]
pub struct TapSlot(Arc<Mutex<Option<Arc<dyn IoTap>>>>);

impl TapSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// 掛上（或用 `None` 卸下）。
    pub fn set(&self, tap: Option<Arc<dyn IoTap>>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = tap;
    }

    pub fn get(&self) -> Option<Arc<dyn IoTap>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 轉發輸出。沒掛 tap 時只是一次鎖。
    pub fn output(&self, bytes: &[u8]) {
        if let Some(t) = self.get() {
            t.on_output(bytes);
        }
    }

    /// 轉發輸入；回 `false` 代表要攔下來不送。
    pub fn allow_input(&self, bytes: &[u8]) -> bool {
        match self.get() {
            Some(t) => t.on_input(bytes),
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Recorder {
        seen: Mutex<Vec<u8>>,
        inputs: AtomicUsize,
        /// true ＝吃掉輸入（模擬巨集執行中）
        swallow: bool,
    }

    impl IoTap for Recorder {
        fn on_output(&self, bytes: &[u8]) {
            self.seen
                .lock()
                .unwrap()
                .extend_from_slice(bytes);
        }
        fn on_input(&self, _bytes: &[u8]) -> bool {
            self.inputs.fetch_add(1, Ordering::SeqCst);
            !self.swallow
        }
    }

    /// 沒掛 tap 時：輸出不會爆、輸入一律放行。
    #[test]
    fn empty_slot_allows_everything() {
        let slot = TapSlot::new();
        slot.output(b"hello");
        assert!(slot.allow_input(b"x"));
    }

    /// 掛上之後輸出會轉發、輸入可以被攔。
    #[test]
    fn tap_sees_output_and_can_swallow_input() {
        let slot = TapSlot::new();
        let rec = Arc::new(Recorder::default());
        slot.set(Some(rec.clone()));
        slot.output(b"ab");
        slot.output(b"c");
        assert_eq!(&*rec.seen.lock().unwrap(), b"abc");
        assert!(slot.allow_input(b"k"), "預設放行");
        assert_eq!(rec.inputs.load(Ordering::SeqCst), 1);

        let swallow = Arc::new(Recorder {
            swallow: true,
            ..Default::default()
        });
        slot.set(Some(swallow));
        assert!(!slot.allow_input(b"k"), "巨集執行中可以吃掉鍵盤");

        slot.set(None);
        assert!(slot.allow_input(b"k"), "卸下之後恢復放行");
    }
}
