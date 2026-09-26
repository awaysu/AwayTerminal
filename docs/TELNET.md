# Telnet（內建）

舊版就已經是**內建** Telnet（`Sessions/TelnetSession.cs`，160 行，不呼叫 Windows 的
`telnet.exe`），所以這次是真正的「逐行搬移」，不像 SSH 要重新設計。
基準：**舊版的行為**；舊版沒處理而 PuTTY 有的東西列在第 4 節，由 PM 決定做或不做。

程式在 `src-tauri/src/telnet/mod.rs`：`Iac`（協商狀態機，可單獨單元測試）＋
`TelnetSession`（socket、keepalive、NAWS）。連線參數與重連在 `src-tauri/src/reconnect.rs`
（和 SSH 共用一套，見 `docs/SSH.md` 第 5 節）。

---

## 1. 舊版行為對照

| 舊版行為 | 出處（`TelnetSession.cs`） | 新版 | 狀態 |
|---|---|---|---|
| 連線放背景執行緒（同步 connect 在主機不通時會卡到 TCP 逾時約 20 秒） | `Start` → `Task.Run` | 自己的執行緒（`connect_timeout` 20 秒） | ✅ 一樣 |
| `TcpClient { NoDelay = true }` | 同上 | `set_nodelay(true)` | ✅ 一樣 |
| 連不上：紅字印錯誤 + 觸發 `Exited`（勾了自動重連就接手） | `ConnectAndReadAsync` 的 catch | 同 | ✅ 一樣 |
| `WILL ECHO`／`WILL SGA` → 回 `DO`；其餘 `WILL` → 回 `DONT` | `RespondOption` | 同（`Iac::respond`） | ✅ 一樣 |
| `DO SGA` → 回 `WILL`；其餘 `DO` → 回 `WONT` | 同上 | 同（多了 `DO NAWS`，見第 2 節） | ✅ 一樣 |
| `WONT`／`DONT` **不回應** | 同上（`else return;`） | **改成會回**（PM 在 TASK-011 決定照 PuTTY），但只在狀態真的改變時 | ⚠️ 見第 2b 節 |
| 子協商（`IAC SB … IAC SE`）內容一律略過 | `IacState.Sb` / `SbIac` | **TTYPE 以外**一律略過（要答 TTYPE 就得看內容，有 64 bytes 上限） | ⚠️ 見第 2a 節 |
| IAC 解析狀態**跨讀取邊界保留** | `_iac` 欄位＋那段註解 | `Iac` 是有狀態的結構，`read` 之間不重設 | ✅ 一樣（單元測試＋probe 各一條） |
| 收到的 `IAC IAC` ＝資料裡的一個 `0xFF` | `IacState.Iac` | 同 | ✅ 一樣 |
| `IAC NOP`／`GA`／`AYT` 這類兩位元組指令丟掉 | 同上 `else` | 同 | ✅ 一樣 |
| 送出時 `0xFF` 轉義成 `FF FF`，**一次寫出**（不逐 byte，否則貼 4KB 會變上千個封包） | `Write` | 同（`escape` + 單次 `write_all`） | ✅ 一樣 |
| Enter 送**單一 CR**（不做 CR LF / CR NUL 轉換） | `Write` 原樣送 | 同 | ✅ 一樣（probe 驗過；PuTTY 不同，見第 4 節） |
| keepalive＝每 N 分鐘一個 `IAC NOP`（0＝關），**直接寫串流不轉義** | `SendNop` | 同（`keepalive_thread`） | ✅ 一樣 |
| keepalive 的分鐘數與 SSH 共用設定 | `AppSettings.KeepAliveMins` | 同（`keepAliveMins`，預設 10） | ✅ 一樣 |
| 關閉分頁**不送優雅結束鍵**（只關 socket） | `Dispose` | 同（`close` → `shutdown(Both)`） | ✅ 一樣 |
| `ProcessId` ＝ 0（遠端連線沒有本機子行程） | `ProcessId => 0` | 同 | ✅ 一樣 |
| 分頁標題＝`host:port` | `OpenTelnetDirect` | 同 | ✅ 一樣 |
| 狀態燈看「近期有輸出」（不看子行程） | `UpdateStatuses` 的 else | 同（`TabKind::Telnet` 不是 `is_local_shell`） | ✅ 一樣 |
| 分頁名稱跟著遠端目錄（提示行解析） | `TracksCwdTitle` 含 `Telnet` | 同 | ✅ 一樣 |
| 清畫面走 `term.clear()`（不是 shell 的 `cls`） | `c` 協定 | 同 | ✅ 一樣 |
| 斷線重連／保持連線與 SSH 共用同一組欄位 | `ConnectDialog.xaml` | 同（`docs/SSH.md` 第 5 節、第 7 節） | ✅ 一樣 |
| `Resize` 是空的（`// NAWS 可選，暫略`） | `Resize` | **改成真的送 NAWS** | ⚠️ 新增，見第 2 節 |

## 2. NAWS（視窗大小通知）——唯一的新增功能

`CLAUDE.md` 定案：「Telnet 自己實作（**加 NAWS**）」。舊版的 `Resize` 是空的，
所以遠端永遠以為視窗是 80×24，`vi`／`top` 會畫錯。

做法（RFC 1073）：

1. 連上就送 `IAC WILL NAWS`（**主動開口**——多數 telnet 伺服器不會問；PuTTY 也是連上就送）。
2. 對方回 `IAC DO NAWS` 才算談成，這時立刻送一次目前尺寸。
3. 之後每次 `resize` 都重送 `IAC SB NAWS <寬16> <高16> IAC SE`；
   **尺寸沒變不送**（前端每次 fit 都會呼叫 `resize`，不擋會洗頻）。
4. 尺寸位元組剛好是 `0xFF` 時要轉義成 `FF FF`（不然會被當成 IAC）。

對方沒答應 NAWS 就**不送**子協商（RFC 要求先 `DO`）。

## 2a. TTYPE：回報終端機類型（TASK-011 新增）

RFC 1091。舊版不認這個選項（`WILL TTYPE` 一律回 `DONT`），PM 在 TASK-011 決定補上，
理由是「最常影響畫面的一條」——遠端不知道我們是什麼終端機時會少送顏色與功能鍵序列。

1. 連上就送 `IAC WILL TTYPE`（和 NAWS 一起，同 PuTTY）。
2. 對方回 `IAC DO TTYPE` ＝談成。
3. 對方送 `IAC SB TTYPE SEND IAC SE` → 我們回 `IAC SB TTYPE IS xterm IAC SE`。

**回 `xterm`**，和 PuTTY 的預設一樣：xterm.js 就是照 xterm 的能力做的，回別的名字
（例如 `ansi`）會讓遠端少送東西。

⚠️ 伺服器主動說 `IAC WILL TTYPE`（它要告訴我們**它的**類型）仍然回 `DONT`——
那是另一件事，別搞混（有單元測試 `server_offering_its_own_ttype_is_declined` 釘住）。

為了看得到 `SB TTYPE SEND`，子協商內容現在會先收起來（上限 64 bytes，**一定要有上限**，
不然對方一直送就會讓我們無限長大），`IAC SE` 到了再決定要不要回。TTYPE 以外的還是丟掉。

## 2b. 回應 `WONT` / `DONT`（TASK-011 新增）

舊版完全不回（`RespondOption` 的 `else return;`）。PuTTY 會回，PM 決定照 PuTTY。

⚠️ **只在狀態真的改變時回答**：對方說「我不做 X」而我們本來就沒請它做 X，回答沒有意義，
而且兩邊都「有來有往」時會變成無限乒乓（RFC 854 明文要求只在改變狀態時回應）。
所以我們記得自己說過哪些 `DO`／`WILL`：

| 收到 | 條件 | 回 |
|---|---|---|
| `WONT x` | 我們說過 `DO x` | `DONT x`（並忘掉 x） |
| `DONT x` | 我們說過 `WILL x` | `WONT x`（並忘掉 x；x 是 NAWS 時同時停止送尺寸） |
| 其餘 | — | **不回** |

## 3. 「連上了」怎麼判斷

重連的退避次數要在「真的連上」時歸零。Telnet 沒有 SSH 的 shell channel，
所以用**從 socket 讀到第一批位元組**（連協商也算：對方在跟我們說話）。

⚠️ 不可以用「有輸出」當條件——那是舊版 SSH 的做法，而舊版的輸出全部來自 `ssh.exe`。
內建實作裡我們自己的狀態訊息（錯誤訊息、提示）也走同一條輸出 callback，
會被誤判成「連上了」。這是 TASK-009 實際踩到的雷，詳見
`docs/REGRESSION-CHECKLIST.md`「隱含契約」的總則那條。

## 4. ⚠️ 舊版沒有、PuTTY 有的（**等 PM 決定**）

一個都還沒做。每一條都寫了「不做會怎樣」，方便判斷。

| 項目 | PuTTY 怎麼做 | 舊版 | 不做會怎樣 | 建議 |
|---|---|---|---|---|
| ~~`TERMINAL-TYPE`（TTYPE）~~ | 回 `xterm` | 不認 | — | ✅ **已做**（TASK-011，見第 2a 節） |
| **Enter 送什麼** | 非 BINARY 模式送 `CR LF`（RFC 854 的 NVT）；也有「Telnet 換行用 CR NUL」的選項 | 送單一 `CR` | 有些老設備吃不到換行（一直沒反應），或反而多一行 | **建議：先照舊版**（使用者的設備現在能用）。真遇到問題時做成每條連線可選 |
| ~~回應 `WONT`／`DONT`~~ | 會回 | 不回 | — | ✅ **已做**（TASK-011，見第 2b 節） |
| `IAC AYT`（Are You There） | 「Telnet 特殊指令」選單可送 | 沒有 | 少一個手動探測的工具 | 不做（沒有 UI 位置） |
| 特殊指令選單（`IP`／`ABORT`／`EOF`…） | 有一整個選單 | 沒有 | 少一組手動指令 | 不做 |
| BINARY（選項 0） | 可談 | 沒有 | 8-bit 資料仍然過得去（我們不改位元組），只有極端情況有差 | 不做 |
| 選項協商的逾時／重送 | 有 | 沒有 | 對方不回答就當沒談成（我們的行為） | 不做 |
| `NEW-ENVIRON`（送環境變數） | 有 | 沒有 | Telnet 沒辦法像 SSH 那樣送 `CLAUDE_CODE_…` | 不做（要用就用 SSH） |

## 5. 自動驗證：`telnet_probe`

```
cd src-tauri && cargo run --example telnet_probe
```

測試伺服器在**同一支程式裡**（`127.0.0.1` 的臨時埠），**不連任何外部主機**。
2026-09-27 的結果：**20 PASS / 0 FAIL**（TASK-011 加了 TTYPE 與 WONT／DONT 三條）。

驗的項目：連上／收資料／「連上了」只回報一次、`DO NAWS` → `WILL NAWS`＋尺寸、
resize 重送 NAWS、尺寸沒變不重送、`WILL ECHO`→`DO`／`WILL TTYPE`→`DONT`、
連上也主動提供 TTYPE、`SB TTYPE SEND`→`IS xterm`、`WONT`／`DONT` 只在狀態改變時回答、
`DO SGA`→`WILL`／其餘 `DO`→`WONT`、協商序列切在封包邊界、送出的 `0xFF` 轉義、
收到的 `IAC IAC` 還原、Enter 送單一 CR、中文 UTF-8 跨封包不裂、
伺服器斷線 → 結束事件正好一次、重連到同一台、重連後 NAWS 用新尺寸、連不上 → 紅字＋結束。

**probe 測不到的**（`SKIP`）：keepalive 的最短間隔是 1 分鐘，不等它；組包本身有單元測試。

### app 端路徑

`--verify` 會連 `127.0.0.1:1`（**沒人在聽**的埠），驗
`session_create(kind:"telnet")` → 建分頁（`kind=telnet`、`backend=telnet`、標題 `host:port`）→
連不上的原因印在終端機 → 提示按 Enter 重連。2026-09-27 的實際輸出：

```
[verify] 分頁 5 建立：backend=telnet title=127.0.0.1:1
[verify] 連不上時有把原因印在終端機：true
[verify] 分頁 kind=telnet（應為 telnet）
[verify] 提示按 Enter 重連：true
[verify] 標題是 host:port：true（同舊版 OpenTelnetDirect）
```

## 6. ⚠️ 待真機驗證

和 SSH 一樣卡在「要使用者的設備清單」。Telnet 特別要看的：

| # | 項目 | 為什麼 |
|---|---|---|
| T1 | 登入提示、密碼不回顯 | Telnet 的登入是**遠端**在問（不是我們），要看 `WILL ECHO` 談成後密碼有沒有被回顯出來 |
| T2 | 中文（Big5 設備） | 我們原樣轉給 xterm；Big5 設備會是亂碼——這是舊版就有的限制，要不要做編碼轉換由 PM 決定 |
| T3 | NAWS 真的有效（`vi`／`top` 不畫錯） | 新增功能，只有 probe 驗過 |
| T4 | 不認 NAWS 的老設備不會被我們的 `WILL NAWS` 弄壞 | 應該只會回 `DONT`；真設備確認 |
| T5 | Enter 的行為（CR 夠不夠） | 見第 4 節第二列。**這條最可能需要改** |
| T6 | keepalive（`IAC NOP`）不會在畫面上留東西 | 舊版註解說伺服器會忽略；真設備確認 |
| T7 | 閒置很久不會被切 | 同 SSH 的 S15 |
| T8 | 斷線自動重連（拔網路線／設備重開） | 與 SSH 共用同一套，`docs/SSH.md` 第 5 節 |
