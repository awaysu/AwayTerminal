//! 連線抽象與 session 管理。
//!
//! `TerminalSession` 對應舊版的 `ITerminalSession`：之後的 SSH / Telnet / 序列埠
//! 都實作同一個 trait，前端與 session 管理層完全不用知道是哪種後端。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// 子行程結束資訊。
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct ExitInfo {
    /// 取不到 exit code 時為 None。
    pub exit_code: Option<i32>,
}

/// 原始位元組輸出（在後端的讀取執行緒上呼叫，不可阻塞太久）。
pub type OnOutput = Arc<dyn Fn(&[u8]) + Send + Sync>;
/// 連線結束（只會被呼叫一次）。
pub type OnExit = Arc<dyn Fn(ExitInfo) + Send + Sync>;

pub trait TerminalSession: Send + Sync {
    /// 寫入原始位元組（鍵盤輸入）。
    fn write(&self, data: &[u8]);
    /// 同步終端機尺寸。
    fn resize(&self, cols: u16, rows: u16);
    /// 子行程 PID（遠端連線可回 0）。
    fn pid(&self) -> u32;
    /// 診斷用後端名稱。
    fn backend_name(&self) -> &'static str;
    /// 先送優雅結束鍵、短暫等待後強制收尾。可重複呼叫（第二次起為 no-op）。
    fn close(&self);
}

/// 所有開著的 session。放在 tauri `State` 裡。
#[derive(Default)]
pub struct SessionManager {
    next_id: AtomicU32,
    sessions: Mutex<HashMap<u32, Arc<dyn TerminalSession>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 先配 id 再建立 session——輸出 callback 需要知道自己的 id。
    pub fn next_id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn insert(&self, id: u32, session: Arc<dyn TerminalSession>) {
        self.lock().insert(id, session);
    }

    pub fn get(&self, id: u32) -> Option<Arc<dyn TerminalSession>> {
        self.lock().get(&id).cloned()
    }

    /// 取出並移除；呼叫端負責 close()。
    pub fn remove(&self, id: u32) -> Option<Arc<dyn TerminalSession>> {
        self.lock().remove(&id)
    }

    pub fn ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.lock().keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// 關閉全部（程式結束時用）。
    pub fn close_all(&self) {
        let all: Vec<Arc<dyn TerminalSession>> = self.lock().drain().map(|(_, s)| s).collect();
        for s in all {
            s.close();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u32, Arc<dyn TerminalSession>>> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }
}
