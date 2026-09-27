# 舊協定 → 新版對照表

`src/terminal.js` 是舊版原檔（見 `docs/TERMINAL-JS-DIFF.md`），它與 host 之間的
**字串協定一個字都沒改**。舊版的 host 是 WPF：`WebView2.PostWebMessageAsString(字串)`
與 `WebMessageReceived`。新版的 host 是 Rust：

| 方向 | 舊版（WPF + WebView2） | 新版（Tauri 2） |
|---|---|---|
| JS → host | `window.chrome.webview.postMessage(字串)` | `src/bridge.js` 的 `postMessage()` 解析第一個字元，翻成對應的 `invoke(...)` |
| host → JS | `PostWebMessageAsString(字串)` | Rust `host::emit_host()` → tauri event **`host-msg`**（payload＝同一個字串）→ `bridge.js` 包成 `{data}` 交給 `terminal.js` 的 listener |
| 輸出（例外） | `o{id}US{base64}` 字串 → JS `atob` | **二進位 channel**（`session_create` 的 `onEvent`）→ `window.AwayTerm.writeOutput(id, Uint8Array)` |

`US` = `\x1f`。

## 統計

| 方向 | 訊息總數 | 已接 | 未接 |
|---|---|---|---|
| JS → host | 12 | **11** | 1 |
| host → JS | 19 | **16**（含 `o` 以二進位 channel 取代） | 3 |
| 合計 | **31** | **27** | **4** |

（TASK-010 沒有讓數字變大：`b` 與 `a` 兩條**先前就算已接**，這次是把它們的「還沒用到的那半」
用完——`b` 的兩段 base64、`a` 的 `save` kind。功能上是新的，數字上不是。）

- TASK-004 接了 9 條：JS→host 的 `p`、`k`、`z`、`a`；host→JS 的 `t`、`x`、`K`、`L`、`q`。
  （`s` 是 TASK-003 收尾時補的——那時漏掉它導致 pane 永遠不 fit，見
  `docs/REGRESSION-CHECKLIST.md`「分頁」第 1 條。）
- TASK-005 接了 7 條：JS→host 的 `U`、`m`；host→JS 的 `c`、`v`、`S`、`F`、`P`，
  並把 `a`／`q` 的 `sel`／`selpaste`／`all`／`file` 幾個 kind 補完。
- TASK-006 接了 `A`（全選）。**舊版 1.2.x 沒有呼叫端**（見下），由 PM 決定當作新功能補進
  終端機右鍵選單。
- TASK-009 接了 `b`（重連前推 scrollback），但只用了前半：送 `b{id}US US`（兩段 base64 都空）
  ＝「把現有畫面推上去、不要清掉」。
- TASK-010 把 `b` **用完整**：恢復分頁時送 `b{id}US{上次的畫面}US{分隔行}`，
  並把 `a…save`（JS→host 的最後一個 kind）接起來——關閉程式時向每個分頁要 scrollback。
  這兩條合起來就是舊版 1.0.45 的「恢復分頁（含畫面紀錄倒回）」。

剩下的 4 條全部屬於代理團隊（Multi-Agent，階段 4），**都不是恢復分頁的一部分**：

| 訊息 | 歸誰 |
|---|---|
| `g`（外框排版）、`u`（拆掉外框）、`E`（pane 的代理狀態標籤） | **代理團隊**。舊版恢復「代理團隊分頁」時會用到它們（`RestoreAgentGroup` 重開整組要重建外框），但那是代理團隊的功能，階段 4 一起做 |
| `G`（JS→host：上下分隔線拖完的比例） | 同上（目前落到 `host_message` 記 log） |

**未接的 JS→host 訊息不會靜靜消失**：`bridge.js` 一律轉給 Rust 的 `host_message`
指令，後端印 `[AwayTerminal] [host_message 未接] {kind} = {說明}`，
所以「還有哪些沒接」在 `npm run tauri dev` 的輸出裡看得見。

---

## JS → host（12）

| 訊息 | 語意 | 新版 | 狀態 |
|---|---|---|---|
| `ready` | `terminal.js` 載完（檔案最後一行） | `invoke('host_ready')` → Rust 依 `settings.json` emit `T{json}`；接著 `bridge.js` 建第一條 session | ✅ 已接 |
| `i{id}US{text}` | 輸入文字 | `invoke('session_write_text', { id, text })` | ✅ 已接 |
| `r{id}US{cols},{rows}` | 尺寸變更 | `invoke('session_resize', …)`；同時記成 `lastSize`，新分頁用它開 PTY（舊版 `_lastCols/_lastRows`，踩雷：初始尺寸勿寫死 80×24） | ✅ 已接 |
| `y{id}US{text}` | 程式以 **OSC 52** 要求寫剪貼簿（1.1.10） | 前端直接 `navigator.clipboard.writeText()` | ✅ 已接 |
| `D{text}` | 診斷記錄（`dbgLog`，舊版寫 `diag.log`） | `invoke('log_line', …)` → 後端 stdout | ✅ 已接 |
| `p{id}` | 使用者在分割模式點了某個 pane | `invoke('pane_selected')`：只改分頁模型、**不回送 `s`**（舊版同樣註明「不回送避免迴圈」） | ✅ 已接 |
| `k{id1},{id2},…` | pane 拖曳後的新順序 | `invoke('pane_reordered')`：重排分頁清單、**不回送 `K`** | ✅ 已接 |
| `z{size}` | Ctrl+滾輪縮放後的字級 | `invoke('pane_font_size')` → 存進 `settings.json`（只收 6~40，同舊版；寫檔有防抖） | ✅ 已接 |
| `a{id}US{kind}US{text}` | `q` 的回覆 | `invoke('pane_answer')`。處理 `cwd`（分頁改名）與 `save`（關閉程式時的 scrollback，交給等在信箱的 `restore::save`）（提示字元行 → shell 分頁自動改名成目前目錄名稱，舊版 1.1.2）；其餘 kind 記 log | 🟡 部分（`cwd` 已接；`sel`/`all`/`file`/`save`/`text` 未接） |
| `U{url}` | 點了終端機裡的連結 | `host_message` 記 log | ⬜ 未接（需 opener + 選單） |
| `m{id}` | 下一個空選取回覆是因為程式接管滑鼠（1.1.10） | `host_message` 記 log | ⬜ 未接（複製／選取尚未實作） |
| `G{下方id}US{上列比例}` | Multi-Agent 上下分隔線拖完的新比例（1.2.0） | `host_message` 記 log | ⬜ 未接（代理團隊是階段 4） |

---

## host → JS（19）

| 訊息 | 語意 | 新版 | 狀態 |
|---|---|---|---|
| `o{id}US{base64}` | 輸出 | **改走二進位 channel**：`session_create` 的 `onEvent` 收 `ArrayBuffer` → `window.AwayTerm.writeOutput(id, u8)`（`terminal.js` AT2-3）。`o` 字串分支保留可用但不會再被送 | ✅ 已取代 |
| `n{id}US{title}[US{flags}]` | 建立 pane（`flags` 含 `c`＝claude 貼上） | `commands::session_create`，**先 emit `n` 再 spawn PTY**；`flags` 由 `shell::is_claude_exe()` 決定 | ✅ 已接 |
| `s{id}` | 選取某 pane | 三處會送：`session_create`（緊接 `n` 之後，同舊版 `AddTab`→`SelectTab`）、`tab_select`（分頁列點一列）、`tab_close`（關掉作用中那個之後選下一個） | ✅ 已接 |
| `t{id}US{title}` | 改分頁名 | `tab_rename`（右鍵／雙擊改名）與 `pane_answer` 的 cwd 自動改名 | ✅ 已接 |
| `x{id}` | 關閉 pane | `tab_close`；`session_create` 啟動失敗時也會送，把已經建好的空 pane 收回去 | ✅ 已接 |
| `K{id1},{id2},…` | 分頁列拖曳後的新順序（1.1.8，host→JS） | `tabs_reorder`（分頁列拖曳）。`k`（pane 拖曳）進來時**不回送** | ✅ 已接 |
| `L{tab\|split\|columns}` | 切換分頁／分割／分欄三態 | `view_mode_cycle`（工具列按鈕，三態循環順序同舊版 `Split_Click`），並存進 `settings.json` | ✅ 已接 |
| `T{json}` | 套用字型／顏色／`imeQuietMs`／`restoreLines`／搜尋列文字 | `host::host_ready`，內容來自 `settings.json`（TASK-004 起不再是 Rust 常數） | ✅ 已接 |
| `q{id}US{…}` | 向前端查詢（回 `a…`） | 狀態輪詢每 600ms 送 `cwd`（同舊版 `UpdateStatuses`）；工具列／右鍵送 `sel`（複製）、`selpaste`（複製且貼上）、`all`（複製全部）、`file`（複製全部存至檔案） | 🟡 部分（`save`＝關閉程式存 scrollback、`text`＝Telegram 遠端查詢，兩個功能都還沒做） |
| `b{id}US{base64}US{base64}` | 恢復分頁／重連前推進 scrollback（1.0.45） | 兩種都用：**重連**前送 `b{id}US US`（兩段留空＝只推畫面）；**恢復分頁**送 `b{id}US{上次畫面}US{分隔行}`，在 `n` 之後、`s` 之前（順序同舊版 `AddTab`，不能換） | ✅ |
| `c{id}` | 清畫面 | `toolbar_clear`：PowerShell／SSH **不送這條**，改對 session 送 Esc → 等 60ms → Ctrl+L（黏著送會被 PSReadLine 當 escape 序列）；Telnet／COM 才送 `c` 清 xterm 緩衝。兩條路都先跳確認 | ✅ 已接 |
| `S{id}US{up\|down\|top\|bottom}` | 捲動檢視（工具列「翻頁」） | `toolbar_scroll`（翻頁下拉：上一頁／下一頁／最上面／最下面） | ✅ 已接 |
| `v{id}US{base64}` | 貼上（走 `term.paste()`） | `toolbar_paste`（工具列與右鍵的「純文字貼上」，以及 `selpaste` 的貼回）。**Ctrl+V / Shift+Insert 不經這條**——`terminal.js` 自己攔 `paste` 事件走 `doPaste` | ✅ 已接 |
| `A{id}` | 全選 | `toolbar_select_all`（終端機右鍵「全選」）。⚠️ **舊版 1.2.x 沒有呼叫端**——`Loc.cs` 留著 `ctx.selectAll` 字串，但整個 `MainWindow.xaml.cs` 沒有任何 `PostToWeb("A…")`，是死協定。TASK-006 由 PM 決定當**新功能**補上 | ✅ 已接（新增，非搬移） |
| `F` | 開搜尋列 | `toolbar_search`（終端機右鍵「搜尋」）。**Ctrl+F 由 `terminal.js` 自己攔，不經這條** | ✅ 已接 |
| `P{id}US{fg}US{bg}` | 單一分頁配色 | `tab_colors`（分頁右鍵「配色 ▸」，色票來自 `settings.palette`）。`fg`/`bg` 皆空＝回到設定預設 | ✅ 已接 |
| `g{…}` | Multi-Agent 外框排版（1.2.0） | — | ⬜ 未接（階段 4） |
| `u{id}` | 拆掉 Multi-Agent 外框（1.2.0） | — | ⬜ 未接（階段 4） |
| `E{id}US{0..4}` | pane 標題的代理狀態標籤（1.2.0） | — | ⬜ 未接（階段 4） |

---

## 分頁列為什麼不走舊協定

舊版的分頁列是 **WPF**（`MainWindow.xaml` 的 `TabStrip`），不在 WebView2 裡，
所以舊協定裡根本沒有分頁列相關的訊息——狀態燈、tooltip、執行時間、右鍵選單
全都留在 C# 那邊，`n`/`s`/`t`/`x`/`K` 只是拿來同步**分割模式的 pane**。

新版分頁列改用 HTML 做，就需要一條把分頁狀態送進前端的路。這裡**不發明新的單字母協定**
（那會讓上面這 31 條對照失真），改用獨立的 tauri event **`tab-state`**，payload 是 JSON：

```jsonc
{
  "tabs": [{
    "id": 1, "kind": "powershell", "kindLabel": "PowerShell",
    "title": "AwayTerminal2", "cwdPath": "C:\\Users\\me\\AwayTerminal2", "flags": "",
    "busy": false, "startedAt": 1750000000000, "pid": 1234
  }],
  "activeId": 1,
  "viewMode": "tab"
}
```

`terminal.js` 完全看不到這條，舊協定也一個字都沒變。

---

## 新版多出來的東西（舊協定沒有的）

| Tauri command | 用途 |
|---|---|
| `ping` | 最小 IPC 驗證 |
| `log_line(msg)` | 前端一行字 → 後端 stdout |
| `report_renderer(renderer)` | 回報實際用的是 WebGL 還是 DOM |
| `conpty_backend()` | 目前的 ConPTY 主機 |
| `session_create(kind, command?, title?, cols, rows, cwd?, ssh?, telnet?, conn?, restore?, onEvent)` | 開連線。`kind`＝`shell`｜`custom`｜`conn`｜`ssh`｜`telnet`。`restore`＝要倒回第幾筆存檔的畫面。回 `SessionInfo`，並附一條輸出 `Channel` |
| `session_write(id, data)` | 寫入原始位元組（`term.onBinary` 用） |
| `session_write_text` / `session_resize` / `session_list` | `i` / `r` 協定的實作與診斷 |
| `tab_close(id)` | 關分頁：`x` → 收掉 log → 背景關 session（優雅結束鍵 Ctrl+C ×3、60ms）→ `s{下一個}` |
| `toolbar_copy` / `toolbar_copy_all` / `toolbar_copy_all_file` / `toolbar_copy_paste` | 送對應的 `q…` 查詢 |
| `toolbar_paste(id, text)` | 送 `v…`（base64 由 `b64.rs` 編，對得上前端的 `atob`） |
| `toolbar_clear(id)` | 清畫面（見上面 `c` 那列） |
| `toolbar_scroll(id, action)` / `toolbar_search()` | 送 `S…` / `F` |
| `tab_colors(id, fg, bg)` | 送 `P…` 並記在分頁模型裡 |
| `save_text_to_file(id, text)` | 存檔對話框 + 寫檔（UTF-8 **with BOM**，同舊版 `File.WriteAllText(…, Encoding.UTF8)`） |
| `pick_work_dir(title)` | 資料夾選擇（新分頁的工作目錄；記住 `lastDir`） |
| `log_defaults(id)` / `log_pick_path(current)` / `log_start(…)` / `log_stop(id)` | log 記錄（見 `src-tauri/src/logging.rs` 的格式對照表） |
| `open_url(url)` / `reveal_path(path)` | 用系統瀏覽器開網址（只放行 http/https）／在檔案總管選取檔案 |
| `toolbar_select_all(id)` | 送 `A…`（新增功能，舊版沒有呼叫端） |
| `session_create` 的 `kind:"ssh"` + `ssh` 參數 | 內建 SSH（`russh`）。見 `docs/SSH.md` |
| `ssh_hostkey_answer(id, answer)` | 主機金鑰**與弱演算法**對話框的回覆（`acceptandstore` / `acceptonce` / `reject`）。兩者共用同一個回覆通道 |
| `algo_catalog()` | 四組演算法的可選名稱與「在警告線下」的標記（連線對話框的「進階」區用） |
| `session_create` 的 `kind:"telnet"` + `telnet` 參數 | 內建 Telnet。見 `docs/TELNET.md` |
| `session_create` 的 `kind:"com"` + `com` 參數 | 連接埠（`serialport`）。見 `docs/COM.md` |
| `macro_run(id, path)` / `macro_stop(id)` / `macro_answer(id, number, text, cancelled)` | TTL 巨集：開始跑／叫停／回覆對話框。`macro_run` 是 **async**（要讀檔與註冊標籤） |
| `macro_verify(id, path, timeoutMs)` | **只給 `--verify` 用**：跑一支巨集並等它結束 |
| `macro_pick_file(title?)` | 選 `.ttl`（篩選器照舊版：TeraTerm 巨集／所有檔案） |
| `exec_verify(id)` | **只給 `--verify` 用**：在指定分頁跑一支自動產生的巨集，驗 `exec` 的 exit code、子行程的 `TEMP` 有沒有導到沙盒、以及**自己記下的那個 PID** 在巨集結束後有沒有被 Job Object 收掉 |
| `compose_load_file()` | 「輸入文字」載入檔案：選檔 → 解碼（BOM → 嚴格 UTF-8 → Big5）→ 回 `{text, encoding}`；**上限 2MB**，超過回 `Err`。取消回 `null` |
| `compose_save_file(text)` | 把文字框內容存成檔（**UTF-8 無 BOM**）；回存到哪裡，取消回 `null` |
| `compose_send(id, text, sendEnter, remember?)` | 送出：換行統一成 CRLF → emit `v{id}{US}{base64}`（＝**貼上**那條路），`sendEnter` 時再等 200ms 寫一個 `\r`（同舊版 `SendSnippet`）。`remember` 預設 `true`＝把勾選狀態存進 `settings.json` |
| `settings_apply(patch)` | 設定視窗按「確定」：每個欄位都是 `Option`（沒帶的不動），夾好範圍後寫檔、**重送 `T{json}`**、設定 Rust 端的語言，回傳套用後的完整設定。見 `docs/SETTINGS.md` |
| `ssh_weak_clear()` | 清掉「已接受的弱演算法」記錄，回傳清了幾筆（設定視窗的按鈕） |
| `font_list()` | 設定視窗字型下拉的候選（**只回這台機器真的有的**；瀏覽器沒有列出系統字型的標準做法，所以是候選清單 ∩ `%WINDIR%\Fonts`） |
| `about_info()` | 關於頁：版本、編譯時間（exe 的檔案時間）、**實際的** xterm.js 版本（`build.rs` 從 `node_modules` 讀）、Tauri 版本、下載／原始碼網址、作者字串（拆三段，前端用 canvas 畫） |
| `third_party_notices()` | `THIRD-PARTY-NOTICES.md` 的內容（**不複製一份**：dev 讀 repo 根目錄、安裝後讀 `resources/`） |
| `update_check(current, base?)` | 檢查更新（`awaysu.cc/software/api.php`，10 秒逾時）。失敗一律回 `null`＝**安靜失敗**（照舊版）。`base` 只給 `--verify` 用 |
| `update_verify()` | **只給 `--verify` 用**：在 127.0.0.1 開一個假伺服器驗完整條路，再對沒人聽的 port 驗失敗路徑。**不會連真的網站** |
| `compose_verify_roundtrip(text)` | **只給 `--verify` 用**：寫一個 Big5 檔再讀回來，回 `{encoding, textOk, text, crlfOk, bytes}`（比對在 Rust 端做，JS 的字面容易假失敗） |
| `save_text_to_file_at(path, text)` | **只給 `--verify` 用**：直接寫檔，而且**只接受系統暫存資料夾底下的路徑**（不是任意寫檔的後門） |
| `com_ports()` | 目前看得到的埠（名稱＋USB 描述）與四組選項清單（鮑率／資料位元／同位／停止位元／流量控制）。**清單只含函式庫真的支援的值** |
| `restore_list()` | 這次啟動要恢復哪些分頁（空＝開一個預設分頁）。前端照順序呼叫 `session_create(…, restore: i)` |
| `exit_confirm(restore)` / `exit_cancel()` | 離開對話框的回覆。`exit_confirm` **必須是 async**——它要等前端把 `a…save` 送回來，同步 command 會擋住主執行緒讓 IPC 進不來（實際踩過） |
| `restore_verify_save()` / `restore_verify_clear()` | **只給 `--verify` 用**：不關程式就走一次「存」，以及把驗證留下的紀錄清掉 |
| `fav_list()` / `fav_candidate(id)` / `fav_add(item)` / `fav_delete(name)` / `fav_rename(name,newName)` / `fav_move(name,delta)` | 我的最愛。`fav_candidate` 是「目前分頁能不能存成最愛」（不能就把選單那條灰掉，同舊版）。**存的內容不含密碼** |
| `session_create` 的 `kind:"conn"` + `conn` 參數 | 自訂連線（含沙盒模式）。見 `docs/AGENT-SANDBOX.md` |
| `custom_list` / `custom_detect` / `custom_save` / `custom_delete` | 自訂連線的讀取／自動偵測／存檔／刪除 |
| `conn_set_sandbox(name, sandbox)` | 切換某條連線的沙盒開關（**下次啟動生效**） |
| `sandbox_clear(id)` | 移除該分頁的沙盒 worktree（**分支保留**） |
| `sandbox_probe()` / `pid_alive(pid)` / `sandbox_verify(id)` | **只給 `--verify` 用**的檢查指令。`pid_alive` 是唯讀的（不砍任何行程） |
| `temp_dir()` | 系統暫存資料夾（`--verify` 用，以及寫入被擋時的後備位置建議） |
| `launch_args()` | CLI 參數（`--cmd` / `--verify` / `--bench`），URL 參數優先 |
| `tab_select(id)` / `tab_rename(id, title)` / `tabs_reorder(ids)` | 分頁列的點選／改名／拖曳排序 |
| `view_mode_cycle()` | 檢視三態循環，回傳新模式 |
| `tab_panel_set(visible?, width?)` | 分頁列顯示狀態／寬度 → `settings.json` |
| `settings_get()` | 目前設定（前端啟動時讀一次） |
| `pane_selected` / `pane_reordered` / `pane_font_size` / `pane_answer` | `p` / `k` / `z` / `a` 的實作 |
| `host_ready()` | `ready` 的實作（emit `T{json}`） |
| `host_message(msg)` | 未接的 JS→host 訊息落腳處（記 log） |
| `bench_*` | IPC 傳輸量測（`docs/IPC-BENCH.md`；`?bench=1` 才自動跑） |

| Channel 事件 | 用途 |
|---|---|
| `ArrayBuffer` | PTY 輸出（取代 `o` 協定） |
| `{kind:"exit", id, exitCode}` | 行程結束；`bridge.js` 在該 pane 寫一行灰字提示 |

| tauri event | 用途 |
|---|---|
| `host-msg`（String） | **所有** host→JS 的舊協定字串都走這一條 |
| `tab-state`（JSON） | 分頁列狀態（見上一節） |
| `ssh-hostkey`（JSON） | 主機金鑰要使用者確認。**Rust 端會停在交握中間等答案**（最多 180 秒，逾時＝取消），前端一定要回 `ssh_hostkey_answer`。見 `src-tauri/src/ssh/prompt.rs` |
| `macro-dialog`（JSON） | 巨集要問使用者（`messagebox`／`yesnobox`／`inputbox`／`passwordbox`／`listbox`／`statusbox`／`filenamebox`／`dirnamebox`）。**巨集的執行緒會停在那裡等**，前端一定要回 `macro_answer`（`statusbox`／`closesbox` 例外，那兩個不等） |
| `macro-error`（JSON） | 巨集出錯（訊息、檔名、行號、那一行的內容）。畫面上也會有一行紅字 |
| `exit-request`（bool） | 使用者按了視窗的 ✕。Rust 先 `prevent_close()`，payload ＝上次的勾選狀態；前端問完呼叫 `exit_confirm`／`exit_cancel`。**再按一次 ✕ 就不擋了**（前端壞掉時的逃生門） |
| `ssh-weak-algo`（JSON） | 協商到警告線以下的演算法（PuTTY 的 warn-below-this-line）。同樣停在交握中間等答案，回 `ssh_hostkey_answer` |

---

## 更新規則

每個任務只要新接了一條協定，就把對應那列的「新版」欄與「狀態」欄改掉，
並更新最上面的統計表。
