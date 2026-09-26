# 連接埠（COM）

搬移舊版 `Sessions/SerialSession.cs`（119 行，`System.IO.Ports`）＋`Dialogs/ComDialog.xaml(.cs)`。
新版用 Rust 的 [`serialport`](https://crates.io/crates/serialport) 4.10（**MPL-2.0**，見
`THIRD-PARTY-NOTICES.md`）。

程式：`src-tauri/src/com/mod.rs`（參數、埠列舉、session）、`src/comdlg.js`（對話框）。
重連與我的最愛／恢復分頁與 SSH／Telnet 共用（`src-tauri/src/reconnect.rs`）。

---

## 1. 舊版行為對照

### session（`SerialSession.cs`）

| 舊版行為 | 出處 | 新版 | 狀態 |
|---|---|---|---|
| `SerialPort(port, baud, parity, dataBits, stopBits)` + `Handshake` | `Start` | `serialport::new(...)` | ✅ 一樣 |
| `DtrEnable = true` | 同上 | `write_data_terminal_ready(true)` | ✅ 一樣 |
| `RtsEnable = true` **只在** flow 是 `None`／`XOnXOff` 時設（硬體流控時設 RTS 會丟例外） | 同上的條件式 | 同（`rts_should_be_set`，有單元測試） | ✅ 一樣 |
| `WriteTimeout = 2000` | 同上 | 寫入 handle 的 timeout ＝2 秒 | ✅ 一樣 |
| `ReadBufferSize = 65536` | 同上 | **沒有對應**（`serialport` 不開放這個設定，緩衝區由 OS 決定） | ⚠️ 差異，影響極小 |
| **寫入走專用執行緒 + 佇列**（流量控制擋住時寫入會等滿 2 秒，直接在 UI 執行緒寫＝每個按鍵凍 2 秒） | `WriteLoop` + 註解 | 同（`mpsc` 佇列 + `com-write` 執行緒） | ✅ 一樣 |
| 寫入逾時／已關閉 → 丟掉那一筆，不卡任何人 | `WriteLoop` 的 catch | 同 | ✅ 一樣 |
| 讀取專用執行緒，blocking read（`ReadTimeout = InfiniteTimeout`；註解說比 `DataReceived` 即時） | `ReadLoop` | **短逾時輪詢（25ms）**，見下方說明 | ⚠️ 刻意不同 |
| 拔線／裝置消失 → 發 `Exited`（自動重連接手） | `ReadLoop` 的 finally | 同 | ✅ 一樣 |
| 使用者關閉 → `Dispose` **自己發** `Exited`（不等讀取執行緒） | `Dispose` 最後一行 | 同（`ExitOnce`，保證只發一次） | ✅ 一樣 |
| 關閉時**不送任何優雅結束鍵** | `Dispose` 只關 port | 同（probe 驗過關閉前後裝置收到的位元組數不變） | ✅ 一樣 |
| `Resize` 是空的 | `Resize` | 同（序列埠沒有視窗大小） | ✅ 一樣 |
| `SendReset()` 是 **TODO**（「待使用者定義要送什麼」，目前不送） | `SendReset` | 同：**不實作**，保留這個未決 | ✅ 一樣 |
| `ProcessId => 0` | 同上 | 同 | ✅ 一樣 |
| 送出／收到的位元組**完全不轉換**（沒有 CR→CR LF、沒有本地回顯） | `Write` / `ReadLoop` | 同（probe 驗過 CR 原樣、4KB 一次寫出） | ✅ 一樣 |

**為什麼讀取改成輪詢**：`serialport` 的 `read` 要靠逾時才回得來，沒有逾時的話關分頁時那條
執行緒會永遠卡在 `read`（Windows 上關 handle 不保證叫醒它）。25ms 對終端機回顯感覺不出來，
閒置時一秒 40 次喚醒可以忽略。**「關分頁」不靠這個逾時**——`close()` 自己就會發結束事件
（第一版偷懶等逾時，`com_probe` 立刻抓到「關分頁後結束事件＝0」）。

### 對話框（`ComDialog`）

| 欄位 | 舊版預設／選項 | 新版 | 狀態 |
|---|---|---|---|
| Port（可編輯下拉） | `SerialPort.GetPortNames()` 排序，**一定含設定裡那個**（就算沒插上） | 同（`com_ports()` + `datalist`），另外顯示 USB 描述 | ✅ ＋加值 |
| Baud rate（可編輯） | 9600 / 19200 / 38400 / 57600 / **115200** / 230400 / 460800 / 921600 | 同順序（`BAUD_RATES`，有單元測試） | ✅ 一樣 |
| Data bits | 5 / 6 / 7 / **8** | 同 | ✅ 一樣 |
| Parity | None / Odd / Even / Mark / Space（`Enum.GetNames`） | **只有 None / Odd / Even**，見第 3 節 | ⚠️ crate 限制 |
| Stop bits | 1 / 1.5 / 2（顯示友善名稱、值是列舉名） | **只有 1 / 2**，見第 3 節 | ⚠️ crate 限制 |
| Flow control | None / XON/XOFF / RTS/CTS / RTS/CTS+XON/XOFF | **前三個**，見第 3 節 | ⚠️ crate 限制 |
| 斷線自動重連 | 有（與 SSH／Telnet 共用 `AutoReconnect`） | 同 | ✅ 一樣 |
| 「回到預設」 | COM5 / 115200 / 8 / None / 1 / None（**不動**自動重連的勾選） | 同 | ✅ 一樣 |
| 「開啟」／「取消」 | 有 | 同 | ✅ 一樣 |
| 「加到我的最愛」 | 沒有（只能從分頁加） | **新增**（和 SSH／Telnet 對話框一致） | ⬜ 新增 |
| 開起來就是上次的值 | 存在 `AppSettings.Com*` | 同（`settings.json` 的 `comPort`…六個欄位，值的字面照舊版） | ✅ 一樣 |
| 入口 | 工具列／新分頁的「連接埠」（tooltip「開 COM 埠」），**獨立對話框** | 同（「新分頁 ▾ → 連接埠…」） | ✅ 一樣 |

分頁標題＝`{埠} {鮑率}`（例：`COM5 115200`），同舊版 `OpenComDirect`。
開埠失敗＝**同步失敗**：`session_create` 回錯誤、分頁不留（舊版是 `StartTab` 的 catch
→ `RemoveTabSilently` + 錯誤視窗）。

### 其他（舊版的分頁行為）

| 項目 | 舊版 | 新版 |
|---|---|---|
| 狀態燈 | 遠端規則（近期有輸出＝紅），不看子行程 | 同 |
| 分頁名稱跟著遠端目錄 | **不**（`TracksCwdTitle` 不含 `Com`） | 同 |
| 清畫面 | 走 `term.clear()`（沒有 shell 可以下 `cls`），會先問確認 | 同 |
| 斷線重連 | 與 SSH／Telnet 共用 `ScheduleReconnect`（退避 3,6,9…30 秒、按 Enter 立刻重連） | 同（`reconnect.rs`） |
| 保持連線（keepalive） | **不適用**（序列埠沒有 keepalive 的概念，舊版也沒送） | 同 |

## 2. 「連上了」＝開埠成功

重連的退避次數要在「真的連上」時歸零。COM 用的是**開埠成功**。

⚠️ **不可以等輸出**：序列裝置可能永遠不主動說話（要你先打字）。用「有輸出」當條件的話，
連得上的裝置也會被當成連不上，退避永遠不歸零。這是
`docs/REGRESSION-CHECKLIST.md`「隱含契約」那條總則的**第三個應用**：

| 後端 | 「連上了」＝ |
|---|---|
| SSH | `request_shell` 成功 |
| Telnet | 從 socket 讀到第一批位元組 |
| **COM** | **開埠成功**（`serialport::open` 回來） |

## 3. ⚠️ `serialport` crate 的限制（**要 PM 決定要不要處理**）

舊版有、`serialport` 4.10 沒有的三個設定：

| 設定 | 舊版（`System.IO.Ports`） | `serialport` | 我們的做法 |
|---|---|---|---|
| 同位 **Mark／Space** | 有 | 只有 None／Odd／Even | 清單**不列**；`settings.json` 裡寫了 Mark/Space 會退成 None **並在終端機印一行黃字** |
| 停止位元 **1.5** | 有（`OnePointFive`） | 只有 One／Two | 同上（退成 1） |
| 流量控制 **RTS/CTS+XON/XOFF** | 有 | 只有 None／Software／Hardware | 同上（退成 RTS/CTS） |

**降級一定會說出來**（有單元測試 `unsupported_values_warn_and_fall_back` 釘住）——
安靜換掉會讓使用者以為設定生效了。

真的遇到需要這三個設定的裝置時，選項是：
(a) 自己用 Win32 `DCB`／`SetCommState` 開埠（只有 Windows，工作量中等）；
(b) fork `serialport` 補這三個值（要維護 fork）；
(c) 告訴使用者改裝置設定。
**建議等使用者真的遇到再處理**——1.5 停止位元與 Mark/Space 同位在現代裝置上極少見。

## 4. 自動驗證：`com_probe`

```
cd src-tauri && cargo run --example com_probe
```

**不需要任何硬體**：`com::spawn_with_link` 吃的是一組 std 的 `Read` + `Write`，
probe 用同程式內的兩條管線當假裝置。2026-09-27 的結果：**13 PASS / 0 FAIL**。

驗的項目：開埠＝「連上了」只回報一次、後端名稱與 PID＝0、裝置→畫面原樣、
打字→裝置原樣（**CR 不轉換**）、貼 4KB 是一次寫出（不是 4096 次 1 byte）、
中文 UTF-8 跨封包不裂、拔線→結束事件正好一次、關分頁→結束事件正好一次、
關分頁**不送任何鍵**、關閉後再打字安靜丟掉、參數降級有說出來、這台機器的埠列舉、預設值照舊版。

### app 端路徑

`--verify` 會呼叫 `com_ports()` 並試著開一個**一定不存在**的埠。2026-09-27 的實際輸出：

```
[verify] 埠列舉：1 個 → COM1
[verify] 選項清單：鮑率 8 個、同位 None/Odd/Even、停止位元 One/Two、流量控制 None/XOnXOff/RequestToSend
[verify] 開不存在的埠有回錯誤：true（開啟 COM999 失敗：系統找不到指定的檔案。）
[verify] 沒有留下死分頁：true（分頁數 2 → 2）
```

（這一步抓到一個真的 bug：`createSession` 沒有把 `com` 參數往下傳，
所以不管選哪個埠都會用設定裡的舊值。錯誤訊息裡的埠名對不上才看出來。）

## 5. 平台差異（這次只做 Windows）

| | Windows | macOS | Linux |
|---|---|---|---|
| 埠名稱 | `COM1`、`COM5`… | `/dev/tty.usbserial-*`、`/dev/cu.*` | `/dev/ttyUSB*`、`/dev/ttyACM*` |
| 權限 | 不用 | 不用 | 使用者要在 **`dialout`** 群組，否則 `open` 會是 Permission denied → 要在錯誤訊息裡提示 |
| 列舉的 USB 資訊 | 有 | 有 | 需要 **libudev**（`serialport` 的預設功能；做 Linux 版時要確認打包環境有它） |
| 拔線的表現 | read 失敗 → 結束事件 | 同 | 同（`/dev` 節點消失） |

程式裡沒有任何 Windows 專屬的路徑假設（埠名稱是使用者輸入或列舉來的字串），
mac/Linux 只要處理上表的三件事。

## 6. ⚠️ 待真機驗證（使用者很可能有 USB 轉序列線）

| # | 項目 | 為什麼 |
|---|---|---|
| C1 | 插上 USB 轉序列線 → 對話框列得出來，而且看得出是哪一條（USB 描述） | 舊版只有 `COM5` 這種編號 |
| C2 | 常用鮑率（9600 / 115200）連得上、字不會掉 | 讀取改成 25ms 輪詢後要確認沒有掉字 |
| C3 | 打字與 Enter 正常（裝置需要 CR LF 時會怎樣） | 我們照舊版只送 CR |
| C4 | 貼上一大段（4KB 以上）不會掉字、不會卡住 | 寫入佇列 + 2 秒逾時 |
| C5 | 對方用 RTS/CTS 硬體流控時能通 | RTS 由驅動管，我們不自己設 |
| C6 | 對方用 XON/XOFF 時能通 | |
| C7 | 中文輸出（UTF-8 裝置） | Big5 裝置會是亂碼（同 Telnet，舊版也一樣） |
| C8 | **拔線** → 畫面有提示、勾了自動重連會自己接回來 | 退避 3,6,9…30 秒 |
| C9 | 拔線後沒勾自動重連 → 按 Enter 重連 | 與 SSH／Telnet 同一套 |
| C10 | 埠被別的程式占用時的錯誤訊息看得懂 | Windows 會回「存取被拒」 |
| C11 | 關分頁 → 埠真的釋放（別的程式馬上開得起來） | |
| C12 | log 記錄 COM 分頁的輸出正常（去 ANSI、時間戳） | |
| C13 | 存成我的最愛 → 從最愛開得起來（埠與鮑率都對） | |
| C14 | 恢復分頁：關程式再開，COM 分頁會回來（**畫面紀錄也在**） | 裝置不在了要能好好報錯 |

## 7. TTL 巨集的預留

TTL 的 `wait`／`send` 需要看得到輸出流、寫得進輸入流。介面已經留好了
（`src-tauri/src/tap.rs` 的 `IoTap` + `TapSlot`），**本體是 TASK-012／013**。
接縫在兩個地方：`reconnect::pipeline` 的 `on_output`（所有後端）與
`session_write`／`session_write_text`。現在每個分頁的 tap 都是空的。
