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
| JS → host | 12 | **5** | 7 |
| host → JS | 19 | **4**（含 `o` 以二進位 channel 取代） | 15 |
| 合計 | **31** | **9** | **22** |

未接的 22 個都不是「漏掉」，而是對應的功能還沒做（分頁列 / 工具列 / 分割分欄 /
設定檔 / 恢復分頁 / 代理團隊 / Telegram 遠端）。前端那半邊全部保留可用，
host 端一開始 emit 就會動。

**未接的 JS→host 訊息不會靜靜消失**：`bridge.js` 一律轉給 Rust 的 `host_message`
指令，後端印
`[AwayTerminal] [host_message 未接] {kind} = {說明} :: {訊息開頭}`，
所以「還有哪些沒接」在 `npm run tauri dev` 的輸出裡看得見。

---

## JS → host（12）

| 訊息 | 語意 | 新版 | 狀態 |
|---|---|---|---|
| `ready` | `terminal.js` 載完（檔案最後一行） | `invoke('host_ready')` → Rust emit `T{json}`；接著 `bridge.js` 依 URL 參數 `invoke('session_create')`，Rust emit `n{id}…` | ✅ 已接 |
| `i{id}US{text}` | 輸入文字 | `invoke('session_write_text', { id, text })` | ✅ 已接 |
| `r{id}US{cols},{rows}` | 尺寸變更 | `invoke('session_resize', { id, cols, rows })` | ✅ 已接 |
| `y{id}US{text}` | 程式以 **OSC 52** 要求寫剪貼簿（1.1.10） | 前端直接 `navigator.clipboard.writeText()`（不必裝 clipboard plugin）；失敗記 log | ✅ 已接 |
| `D{text}` | 診斷記錄（`dbgLog`，舊版寫 `diag.log`） | `invoke('log_line', { msg: '[diag] …' })` → 後端 stdout | ✅ 已接 |
| `p{id}` | 使用者選了某個 pane | `host_message` 記 log | ⬜ 未接（分頁列 UI） |
| `k{id1},{id2},…` | 拖曳 pane 後的新順序 | `host_message` 記 log | ⬜ 未接（分割/分欄 UI） |
| `z{size}` | Ctrl+滾輪縮放後的字級（要存設定） | `host_message` 記 log；**字級當下會變、只是不會記住** | ⬜ 未接（設定檔） |
| `a{id}US{kind}US{text}` | `q` 查詢的回覆（選取文字／全部文字／cwd／save…） | `host_message` 記 log | ⬜ 未接（host 還沒送 `q`） |
| `m{id}` | 下一個空選取回覆是因為程式接管滑鼠（1.1.10） | `host_message` 記 log | ⬜ 未接（同上） |
| `U{url}` | 點了終端機裡的連結（舊版跳「從瀏覽器開啟／複製網址」選單） | `host_message` 記 log | ⬜ 未接（需 shell/opener plugin + 選單） |
| `G{下方id}US{上列比例}` | Multi-Agent 上下分隔線拖完的新比例（1.2.0） | `host_message` 記 log | ⬜ 未接（代理團隊是階段 4） |

---

## host → JS（19）

| 訊息 | 語意 | 新版 | 狀態 |
|---|---|---|---|
| `o{id}US{base64}` | 輸出 | **改走二進位 channel**：`session_create` 的 `onEvent` 收 `ArrayBuffer` → `window.AwayTerm.writeOutput(id, u8)`（`terminal.js` AT2-3）。`o` 字串分支保留可用但不會再被送 | ✅ 已取代（不再送字串） |
| `n{id}US{title}[US{flags}]` | 建立 pane（`flags` 含 `c`＝claude 貼上走 ESC+CR） | `commands::session_create` 裡 `emit_host(format!("n{id}\x1f{title}\x1f{flags}"))`，**先 emit `n` 再 spawn PTY**；`flags` 由 `shell::is_claude_exe()` 決定（同舊版 `IsClaudeExe`） | ✅ 已接 |
| `T{json}` | 套用字型／顏色／`imeQuietMs`／`restoreLines`／搜尋列文字／代理狀態標籤 | `host::host_ready` emit，值照舊版 `Services/AppSettings.cs` 預設值寫死成 Rust 常數 | ✅ 已接（設定檔尚未做，值固定） |
| `t{id}US{title}` | 改分頁名 | — | ⬜ 未接（分頁列 UI） |
| `s{id}` | 選取某 pane | `session_create` 在 emit `n` 之後緊接 `emit_host("s{id}")`（同舊版 `AddTab` → `SelectTab`） | ✅ 已接 |
| `x{id}` | 關閉 pane | — | ⬜ 未接（分頁列 UI；目前只有 `session_close`，還不會 emit `x`） |
| `c{id}` | 清畫面（`term.clear()`；另一半 Esc+60ms+Ctrl+L 在 host） | — | ⬜ 未接（需工具列 + 確認對話框） |
| `L{tab\|split\|columns}` | 切換分頁／分割／分欄三態 | — | ⬜ 未接（檢視模式 UI） |
| `K{id1},{id2},…` | 分頁列拖曳後的新順序（1.1.8，host→JS） | — | ⬜ 未接（分頁列 UI） |
| `S{id}US{up\|down\|top\|bottom}` | 捲動檢視（工具列「翻頁」，不送輸入） | — | ⬜ 未接（工具列） |
| `q{id}US{sel\|selpaste\|all\|text\|file\|cwd\|save}` | 向前端查詢（回 `a…`） | — | ⬜ 未接（複製／存檔／恢復分頁／Telegram 遠端） |
| `v{id}US{base64}` | 貼上（走 `term.paste()`） | — | ⬜ 未接（工具列「純文字貼上」）。**注意：Ctrl+V / Shift+Insert / webview 右鍵貼上不經這條**——`terminal.js` 自己攔 `paste` 事件走 `doPaste`，新版已經能用 |
| `b{id}US{base64 舊內容}US{base64 分隔行}` | 恢復分頁／重連前推進 scrollback（1.0.45） | — | ⬜ 未接（恢復分頁／自動重連） |
| `A{id}` | 全選 | — | ⬜ 未接（工具列） |
| `F` | 開搜尋列 | — | ⬜ 未接（工具列）。**Ctrl+F 由 `terminal.js` 自己攔，已經能用** |
| `P{id}US{fg}US{bg}` | 單一分頁配色（空＝回設定預設） | — | ⬜ 未接（設定檔／逐分頁配色） |
| `g{下方id}US{比例}US{上列id,…}US{標籤\|…}US{外框顏色,…}` | Multi-Agent 外框排版（1.2.0） | — | ⬜ 未接（階段 4） |
| `u{id}` | 拆掉 Multi-Agent 外框（1.2.0） | — | ⬜ 未接（階段 4） |
| `E{id}US{0\|1\|2\|3\|4}` | pane 標題的代理狀態標籤（1.2.0） | — | ⬜ 未接（階段 4） |

---

## 新版多出來的東西（舊協定沒有的）

這些不是舊協定的一部分，是新版為了「不開 devtools、不搶焦點也能驗證」加的。

| Tauri command | 用途 |
|---|---|
| `ping` | 最小 IPC 驗證 |
| `log_line(msg)` | 前端一行字 → 後端 stdout（`bridge.js`、`main.js`、`ime-lab.html` 都用它） |
| `report_renderer(renderer)` | 回報實際用的是 WebGL 還是 DOM |
| `conpty_backend()` | 目前的 ConPTY 主機（`conpty.dll (OpenConsole)` / `inbox conhost`） |
| `session_create` | 開連線。`kind`＝`powershell`｜`custom`（`command` 給指令，`?cmd=<指令>` 的 dev 入口）。回 `SessionInfo`，並附一條輸出 `Channel` |
| `session_write(id, data)` | 寫入原始位元組（`term.onBinary` 用；目前 `terminal.js` 走 `i` → `session_write_text`） |
| `session_write_text(id, text)` | `i` 協定的實作 |
| `session_resize(id, cols, rows)` | `r` 協定的實作 |
| `session_close(id)` / `session_list()` | 收掉／列出連線 |
| `host_ready()` | `ready` 協定的實作（emit `T{json}`） |
| `host_message(msg)` | 未接的 JS→host 訊息落腳處（記 log） |
| `bench_raw` / `bench_vec` / `bench_base64` / `bench_channel` | IPC 傳輸量測（`docs/IPC-BENCH.md`；`?bench=1` 才自動跑） |

| Channel 事件 | 用途 |
|---|---|
| `ArrayBuffer` | PTY 輸出（取代 `o` 協定） |
| `{kind:"exit", id, exitCode}` | 行程結束；`bridge.js` 在該 pane 寫一行黃字提示 |

| tauri event | 用途 |
|---|---|
| `host-msg`（payload: String） | **所有** host→JS 的舊協定字串都走這一條 |

---

## 更新規則

每個任務只要新接了一條協定，就把對應那列的「新版」欄與「狀態」欄改掉，
並更新最上面的統計表。
