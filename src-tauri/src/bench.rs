//! IPC 傳輸量測（CLAUDE.md 風險 5：「Tauri 二進位 channel 可能被序列化成 JSON 數字陣列」）。
//!
//! 四條路各一個指令，前端量往返時間後寫進 `docs/IPC-BENCH.md`：
//!
//! | 指令 | 走哪條路 |
//! |---|---|
//! | `bench_raw` | `tauri::ipc::Response::new(Vec<u8>)` → 自訂協定回應，真二進位 |
//! | `bench_vec` | command 直接回 `Vec<u8>` → serde 變成 JSON 數字陣列 |
//! | `bench_base64` | 回 base64 字串，前端 `atob`（舊版的做法） |
//! | `bench_channel` | `Channel` 送 `InvokeResponseBody::Raw`（PTY 輸出實際走的路） |

use tauri::ipc::{Channel, InvokeResponseBody, Response};

/// 產生可辨識、非零的測試資料（避免壓縮／特例最佳化影響量測）。
fn payload(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

/// 自訂協定原始位元組回應。
#[tauri::command]
pub fn bench_raw(size: usize) -> Response {
    Response::new(payload(size))
}

/// 直接回 `Vec<u8>`：tauri 會用 serde 序列化，結果是 JSON 數字陣列。
#[tauri::command]
pub fn bench_vec(size: usize) -> Vec<u8> {
    payload(size)
}

/// base64 字串（舊版 `o{id}{US}{base64}` 的做法）。
#[tauri::command]
pub fn bench_base64(size: usize) -> String {
    crate::b64::encode(&payload(size))
}

/// 用 `Channel` 送 `Raw`，也就是 PTY 輸出實際走的路。
#[tauri::command]
pub fn bench_channel(size: usize, on_data: Channel<InvokeResponseBody>) -> Result<(), String> {
    on_data
        .send(InvokeResponseBody::Raw(payload(size)))
        .map_err(|e| e.to_string())
}
