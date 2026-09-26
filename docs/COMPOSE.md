# 輸入文字視窗（搬移舊版 `Dialogs/ComposeDialog`）

TASK-014 C。舊版的來源：`reference/AwayTerminal/Dialogs/ComposeDialog.xaml(.cs)` 與
`MainWindow` 的工具列按鈕（`ComposeAction`）。

## 1. 為什麼有這個功能

舊版加它的理由**不是**「方便打長文」，而是：

- Claude Code／Codex 這類 TUI 會對**逐鍵輸入**做自己的處理（重畫、自動完成、
  多行貼上的時序），中文在 IME 組字中送進去容易變成重複字或亂碼；
- 所以先在一個**普通的輸入框**把整段文字打好（組字完全在系統輸入法 + 該輸入框裡發生，
  完全不經過 xterm.js 與 ConPTY），按「送出」時才一次整段送進分頁。

新版照舊：送出走**既有的貼上路徑**（`v{id}{US}{base64}` → `terminal.js` 的 `doPaste`），
所以 claude 的多行貼上處理（`bracketed paste`、輸入佇列、靜止閘門）**一行都不必改**。

## 2. 為什麼做成「頁內對話框」而不是另一個視窗

舊版是 WPF 的**模態視窗**。新版是頁內對話框（同前面幾個：SSH 設定、我的最愛、搜尋…）。

| 理由 | 說明 |
|---|---|
| IME 行為一樣 | 組字發生在 `<textarea>`，那是**系統輸入法直接處理**的元件，和 WPF `TextBox` 一樣不經過我們的鍵盤處理。這個功能的重點就是「不要碰 xterm 的輸入路徑」，頁內或另開視窗對這點沒有差別 |
| 不必多一套視窗管理 | Tauri 的多視窗要各自管大小／位置／焦點／關閉，還要再包一層 IPC；跨平台（mac 的 sheet、Linux 的 WM）行為又不一致 |
| 啟動與記憶體 | 多一個 webview 就多一份 WebView2／WKWebView 執行個體（`CLAUDE.md` 的目標是**更快**） |
| 焦點 | 團隊模式下桌面是共用的（`.ai/bus/` 的規則），另開視窗更容易被別的視窗搶走焦點 |

**可回退**：真的需要獨立視窗（例如使用者想一邊看分頁一邊打字），把 `src/compose.js`
的 DOM 搬到一個 Tauri window 就好——邏輯與 Rust 端的 command 都不用改。

## 3. 行為對照表（舊版 → 新版）

| 舊版 `ComposeDialog` | 出處 | 新版 |
|---|---|---|
| 工具列按鈕開啟；沒有分頁時提示 | `ComposeAction` | 同（`#btn-compose`，`compose.noTab`） |
| 送到**開視窗那一刻**的作用中分頁 | 建構子存 `_targetTab` | 同（`composedlg` 的 `data-tab-id`） |
| 「送出」＝整段貼進分頁；可選**送出後補一個 Enter** | `SendSnippet` | 同（`compose_send` 的 `sendEnter`） |
| 送 Enter 前先等一下（讓 TUI 把貼上的內容吃完） | `await Task.Delay(200)` | 同（Rust 端 `sleep(200ms)` 再寫 `\r`） |
| 勾選狀態記在設定裡，預設**開** | `AppSettings.ComposeSendEnter` | 同（`settings.composeSendEnter`） |
| 「載入檔案」：UTF-8／Big5 都讀得進來 | `LoadFile`（BOM → UTF-8 → ANSI） | 同（`encoding_rs`，順序：BOM(UTF-8/UTF-16) → 嚴格 UTF-8 → Big5） |
| 載入檔案上限 **2MB**，超過跳「檔案太大」 | `LoadMaxMB = 2` + `compose.loadTooBig` | 同（`compose::LOAD_MAX_MB`） |
| 送出走**貼上**的路徑（`v{id}{US}{base64}`） | `PasteToTab` | 同——所以 claude 分頁「換行→ESC+CR 軟換行」那段是 `terminal.js` 裡原本就有的邏輯，**不必再做一次** |
| 「儲存」：存成 UTF-8 | `SaveFile` | 同（**無 BOM**） |
| 「清除」把內容清掉，但可以「復原」救回來（**關掉重開也還在**） | `_lastCleared`（static） | 同（`compose.js` 的 `lastCleared`） |
| 關掉（X／返回）**不清**未送出的文字，下次開還在 | `_draft`（static） | 同（`draft`） |
| 送出之後才清空 | 同上 | 同 |
| Ctrl+Enter 送出 | `PreviewKeyDown` | 同 |
| Esc 關閉 | 視窗的 `Cancel` | 同 |

### 刻意不同

| 項目 | 舊版 | 新版 | 為什麼 |
|---|---|---|---|
| 「檔案太大」的訊息 | `Loc.T("compose.loadTooBig")`（中／英兩版） | **Rust 端固定繁中**（`compose.rs` 回的 `Err`） | 只有這一個字串沒進 `strings.js`（它是後端的錯誤訊息）。英文介面要一起翻的話，改成回一個代碼讓前端查表——**列進待辦**，不是刻意 |
| 換行 | 原樣送出（WPF `TextBox` 的多行本來就是 CRLF） | **明確**轉成 CRLF 再送 | 網頁的 `<textarea>` 給的是 `\n`；不轉的話同一份文字在新舊版送出的位元組不一樣（貼上路徑對 `\n` 的處理在各 TUI 下不一致） |
| ANSI 系統編碼 | `Encoding.Default`（cp950） | **固定 Big5** | 跨平台沒有「系統 ANSI」這回事；使用者的舊檔就是 Big5。要別的編碼再加選單 |
| 復原（Ctrl+Z） | 靠 WPF `TextBox` 的原生 undo | **自己維護一個 50 層的堆疊** | 程式用 JS 改過 `value` 之後瀏覽器的原生 undo 就不可靠了 |

## 4. Rust 端的 command

見 `docs/PROTOCOL.md`：`compose_load_file`／`compose_save_file`／`compose_send`／
`compose_verify_roundtrip`。

解碼與換行統一的單元測試在 `src-tauri/src/compose.rs`（5 個），
`--verify` 會**真的**寫一個 Big5 檔、讀回來、送進一個 PowerShell 分頁，
再從畫面上抓那段中文（見 `docs/REGRESSION-CHECKLIST.md` 的「輸入文字」那一組）。
