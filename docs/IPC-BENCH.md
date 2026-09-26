# Tauri IPC 傳輸實測（CLAUDE.md 風險 5）

CLAUDE.md 風險 5：「Tauri 二進位 channel：直接回 `Vec<u8>` 可能被序列化成 JSON 數字陣列
（比 base64 更慢），要用 raw response 並實測。」

**結論：風險成立。** `Vec<u8>` 確實變成 JSON 數字陣列，比 base64 慢約 2 倍、比原始二進位慢約 7.4 倍。

## 量測方式

- 後端：`src-tauri/src/bench.rs`，四個 command 各走一條路。
- 前端：`src/main.js` 的 `awayBench()` / `awayBenchSmall()`，
  `performance.now()` 量往返（含暖機一次不計入），並記錄前端實際收到的型別。
  結果同時印在終端與後端 stdout（`log_line`），所以不必開 devtools 也拿得到數據。
- dev 模式啟動時自動跑一次；任何時候可在 devtools 打 `awayBench(1, 10)`、`awayBenchSmall(200)`。

環境：Windows 10 Pro 19045、WebView2 Runtime 153.0.4234.48、tauri 2.11.6、
`npm run tauri dev`（debug build，未最佳化）。

## 1MB × 10 次

| 路徑 | 後端寫法 | 前端收到 | median | mean | min | max | 吞吐 |
|---|---|---|---|---|---|---|---|
| **raw** | `Response::new(Vec<u8>)` | `ArrayBuffer` | **17.8 ms** | 18.1 ms | 17.1 ms | 21.6 ms | **55.2 MB/s** |
| vec | command 回 `Vec<u8>` | **JSON 數字陣列（1048576 個元素）** | 133.0 ms | 134.3 ms | 130.5 ms | 146.9 ms | 7.4 MB/s |
| base64 | 回 base64 字串 + 前端 `atob` | `ArrayBuffer` | 65.1 ms | 66.3 ms | 63.2 ms | 78.6 ms | 15.1 MB/s |
| **channel** | `Channel` 送 `InvokeResponseBody::Raw` | `ArrayBuffer` | **19.1 ms** | 19.7 ms | 18.5 ms | 21.3 ms | **50.8 MB/s** |

（另一次獨立執行的數字幾乎相同：raw 18.0 / vec 132.0 / base64 68.5 / channel 18.7 ms。）

## 小 payload × 200 次（channel）

| 大小 | median | mean | 前端收到 |
|---|---|---|---|
| 512 bytes | 0.700 ms | 0.762 ms | `ArrayBuffer` |
| 2048 bytes | 1.300 ms | 1.349 ms | `ArrayBuffer` |

## 為什麼 512B 和 2048B 差這麼多（而且和大小不成比例）

看 tauri 原始碼 `tauri-2.11.6/src/ipc/channel.rs`：`Channel` 送 `InvokeResponseBody::Raw`
時有一個門檻 `MAX_RAW_DIRECT_EXECUTE_THRESHOLD = 1024`：

- **小於 1024 bytes**：把 bytes 用 `serde_json` 轉成**數字陣列字串**，包進
  `new Uint8Array([...]).buffer` 用 `webview.eval()` 執行。
  （也就是說，小包其實走的就是「JSON 數字陣列」那條路——只是量小所以不痛。）
- **大於等於 1024 bytes**：把 body 放進 `ChannelDataIpcQueue`，`eval` 一段 JS 去
  `invoke('plugin:__TAURI_CHANNEL__|fetch')` 把資料用自訂協定取回來 → 真正的二進位。

所以兩個大小走的是**不同機制**：512B 是一次 eval（0.70 ms），2048B 是 eval + 一次
fetch 往返（1.30 ms）。差的是固定成本，不是位元組數。

同一份原始碼裡 JSON 也有對應門檻 `MAX_JSON_DIRECT_EXECUTE_THRESHOLD = 8192`，
註解寫著在 WebView2 v135 上 8KB 以下走 eval 比走 fetch 快 2 倍。

## 採用的方式與理由

**PTY 輸出採用 `Channel` + `InvokeResponseBody::Raw`，並在 Rust 端做批次合併**
（`src-tauri/src/output.rs` 的 `OutputPump`，單次上限 256KB）。

理由：

1. **要 push、不要 pull。** PTY 輸出是後端主動產生的，`Response::new` 型的 command 是前端
   發問才有答案；要用它就得長輪詢或每次通知再拉一次，多一趟往返又多一層狀態。
   channel 的大包效能（50.8 MB/s）和 raw command（55.2 MB/s）差不到 10%，不值得換架構。
2. **絕不用 `Vec<u8>` 當 command 回傳值。** 實測就是 CLAUDE.md 擔心的那件事：
   1MB 變成 1048576 個元素的 JSON 數字陣列，7.4 MB/s，比舊版的 base64 還慢一倍。
   （上行方向同理：`session_write` 收 `Vec<u8>` 會走 JSON 數字陣列，所以打字走
   `session_write_text` 收 `String`，只有 `term.onBinary` 才用 `session_write`。）
3. **批次合併把大量輸出推到快的那條路。** 讀取執行緒（blocking `ReadFile`，64KB buffer）
   只把 bytes 丟進 pump，pump 執行緒合併後送一包。`cat` 大檔時每包遠大於 1024 bytes
   → 走 fetch 自訂協定的真二進位；互動打字時每包只有幾個 byte → 走 eval，
   固定成本 0.7 ms，絕對量可忽略。
4. **256KB 上限**避免單包塞爆 webview；超過的留到下一包。

### 和舊版比

舊版是 `o{id}{US}{base64}` 字串 → `PostWebMessageAsString` → JS `atob`，對應這裡的
base64 那一列（15.1 MB/s）。新版的 channel raw 是 **50.8 MB/s，約 3.4 倍**。
這是 CLAUDE.md「速度改善點 2」的實際數字。

### 還沒量的

- release build（`opt-level = "s"` + LTO）下的數字；目前全部是 debug build。
- 真實 PTY 情境的端到端吞吐（例如 `cat` 一個 10MB 檔到畫面），這會同時受 xterm.js
  解析與 WebGL 渲染影響，不只是 IPC。要跟舊版對比時應該量這個，而不是只看 IPC。
- macOS（WKWebView）／Linux（WebKitGTK）上的同一組數字——門檻常數是跨平台共用的，
  但 `eval` 與自訂協定的相對成本不同（tauri 原始碼註解就提到 macOS 的門檻理由不一樣）。
