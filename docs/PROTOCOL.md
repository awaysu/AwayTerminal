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
| JS → host | 12 | **9** | 3 |
| host → JS | 19 | **9**（含 `o` 以二進位 channel 取代） | 10 |
| 合計 | **31** | **18** | **13** |

TASK-004 新接的 9 條：JS→host 的 `p`、`k`、`z`、`a`；host→JS 的 `t`、`x`、`K`、`L`、`q`。
（`s` 是 TASK-003 收尾時補的——那時漏掉它導致 pane 永遠不 fit，見
`docs/REGRESSION-CHECKLIST.md`「分頁」第 1 條。）

未接的 13 條都不是「漏掉」，是對應的功能還沒做（工具列的複製／貼上／清畫面／翻頁／全選、
逐分頁配色、恢復分頁、代理團隊）。前端那半邊全部保留可用，host 端一開始 emit 就會動。

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
| `a{id}US{kind}US{text}` | `q` 的回覆 | `invoke('pane_answer')`。**目前只處理 `cwd`**（提示字元行 → shell 分頁自動改名成目前目錄名稱，舊版 1.1.2）；其餘 kind 記 log | 🟡 部分（`cwd` 已接；`sel`/`all`/`file`/`save`/`text` 未接） |
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
| `q{id}US{…}` | 向前端查詢（回 `a…`） | 狀態輪詢每 600ms 送 `q{id}US cwd`（同舊版 `UpdateStatuses`）。其餘 kind 還沒送 | 🟡 部分（`cwd` 已接） |
| `b{id}US{base64}US{base64}` | 恢復分頁／重連前推進 scrollback（1.0.45） | — | ⬜ 未接（恢復分頁／自動重連） |
| `c{id}` | 清畫面 | — | ⬜ 未接（需工具列按鈕＋確認對話框） |
| `S{id}US{up\|down\|top\|bottom}` | 捲動檢視（工具列「翻頁」） | — | ⬜ 未接（工具列） |
| `v{id}US{base64}` | 貼上（走 `term.paste()`） | — | ⬜ 未接（工具列「純文字貼上」）。**Ctrl+V / Shift+Insert / webview 右鍵貼上不經這條**——`terminal.js` 自己攔 `paste` 事件走 `doPaste`，已經能用 |
| `A{id}` | 全選 | — | ⬜ 未接（工具列） |
| `F` | 開搜尋列 | — | ⬜ 未接（工具列）。**Ctrl+F 由 `terminal.js` 自己攔，已經能用** |
| `P{id}US{fg}US{bg}` | 單一分頁配色 | — | ⬜ 未接（逐分頁配色的右鍵選單） |
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
| `session_create(kind, command?, title?, cols, rows, cwd?, onEvent)` | 開連線。`kind`＝`shell`｜`custom`。回 `SessionInfo`，並附一條輸出 `Channel` |
| `session_write(id, data)` | 寫入原始位元組（`term.onBinary` 用） |
| `session_write_text` / `session_resize` / `session_list` | `i` / `r` 協定的實作與診斷 |
| `tab_close(id)` | 關分頁：`x` → 背景關 session（優雅結束鍵 Ctrl+C ×3、60ms）→ `s{下一個}` |
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

---

## 更新規則

每個任務只要新接了一條協定，就把對應那列的「新版」欄與「狀態」欄改掉，
並更新最上面的統計表。
