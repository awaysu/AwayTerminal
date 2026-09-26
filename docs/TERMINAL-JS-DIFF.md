# `src/terminal.js` 與舊版的差異

`src/terminal.js` 是舊版 `reference/AwayTerminal/web/terminal.js` 的**複製檔**，
原則是「能不改就不改」。目前總 diff 只有 54 行、三處修改，每處都用
`// AT2-n:` 註解標在原地。

隨時可以重跑這個比對：

```powershell
git -C . diff --no-index reference/AwayTerminal/web/terminal.js src/terminal.js
```

---

## 第一節：`AT2-` 修改清單（共 3 處）

### AT2-1 — `window.chrome.webview` → `window.AwayBridge`（第 24 行）

| | |
|---|---|
| **改了什麼** | `var ws = window.chrome.webview;` → `var ws = window.AwayBridge;` |
| **為什麼** | 舊版是 WPF + WebView2，host↔JS 走 `PostWebMessageAsString`。新版沒有 WebView2 那個物件。`src/bridge.js` 提供**同名 API**（`postMessage(string)`、`addEventListener("message", fn)`，事件物件有 `.data` 字串），內部把舊字串協定翻成 Tauri command / event。 |
| **影響範圍** | 一行。**舊協定字串一個字都沒改**——`terminal.js` 裡 13 處 `ws.postMessage(…)` 與整個 `ws.addEventListener("message", …)` 分派完全原樣。協定對照見 `docs/PROTOCOL.md`。 |
| **風險** | 低。唯一要小心的是 bridge 那邊：`ready` 的第一個字元 `r` 和 `r{id}US{cols},{rows}` 撞號，必須先比對完整字串（`bridge.js` 已註明，第一版就踩過，畫面全黑）。 |

### AT2-2 — `open()` 之後掛 WebGL addon（第 116 行）

| | |
|---|---|
| **改了什麼** | `term.open(body)` 之後加一行 `if (window.AwayWebgl) window.AwayWebgl(term, id);` |
| **為什麼** | 舊版沒載 WebGL addon＝DOM 渲染，是 `CLAUDE.md` 列的速度改善點 1。WebGL addon 必須在 `open()` 之後才能掛。 |
| **影響範圍** | 一行加在 `makeTerm()` 裡；實作（addon 建立、失敗退回 DOM、`onContextLoss` 卸載、渲染器回報後端 log）全部在 `src/main.js` 的 `window.AwayWebgl`，`terminal.js` 不含任何 WebGL 程式碼。`window.AwayWebgl` 不存在時這行就是 no-op（等於舊版行為）。 |
| **風險** | 中（`CLAUDE.md` 風險 6）：全形字寬、中文字型 fallback、context lost 都可能出問題。已做的防護＝載入失敗退回 DOM、context lost 時 `dispose()` 讓 xterm 自動退回 DOM。**尚未做**：`@xterm/addon-canvas` 中間層與手動選渲染器。 |

### AT2-3 — 輸出寫入抽成 `writeOutput()` + `window.AwayTerm` 出口（第 907、932、1073 行）

| | |
|---|---|
| **改了什麼** | ① 新增 `function writeOutput(id, bytes)`（第 913 行），內容就是原本寫在 `o` 分支裡的那三行；② `o` 分支改叫 `writeOutput(id, bytes)`（少了一個區域變數 `rec`）；③ 檔尾加 `window.AwayTerm = { writeOutput, doPaste, hasTerm, tail }`。 |
| **為什麼** | 新版輸出走 Tauri 的二進位 channel（bytes 直達，不經 `o{id}US{base64}` 字串也不經 `atob`），bridge 收到 `Uint8Array` 要能直接呼叫同一段寫入邏輯。 |
| **影響範圍** | **`pendingRestore` / `held` / `lastOutMs` 三者的邏輯一字未改**，只是換了位置：<br>• `pendingRestore !== null` → 推進 `held`，等 `applyRestore()` 補寫（1.0.45 恢復分頁）<br>• `lastOutMs = performance.now()` → 靜止閘門用（1.0.43）<br>`o` 字串分支保留可用（`atob` 那段還在），只是這個專案不會再送它。 |
| **`tail()` 是新增的** | 純讀取 `term.buffer.active` 回傳純文字尾端，給 `main.js` 的 `awayDump()` 做端到端驗證（共用桌面、不能搶焦點，所以不靠看視窗）。不影響任何既有行為。 |

### 沒有改的地方（刻意）

- 舊版靠 `<script>` 載 UMD 的 `Terminal` / `FitAddon` / `Unicode11Addon` /
  `WebLinksAddon` / `SerializeAddon` 全域 → 新版在 `main.js` 先 `window.Terminal = Terminal`、
  `window.FitAddon = { FitAddon }` …（**維持 UMD 命名空間的形狀**）再 `import('./terminal.js')`。
  所以 `terminal.js` 的 `new FitAddon.FitAddon()` 這類寫法不用改。
- `SearchAddon`：舊版的 Ctrl+F 搜尋是 `terminal.js` **自己實作**的
  （`openSearch` / `runSearch` / `gotoHit`，直接掃 `term.buffer`），不是 search addon。
  所以新版也沒裝 `@xterm/addon-search`，行為與舊版一致。

---

## 第二節：已沿用的踩雷修正

出處＝舊版 `reference/AwayTerminal/CLAUDE.md` 第 186–247 行「踩雷紀錄」。
以下挑出與**前端輸入 / IME / 貼上 / xterm 選項 / 輸出時序**有關的條目。
「狀態」欄：✅ 原樣沿用（在 `terminal.js` 裡，本任務沒動）／
⚠️ 只在前端保留、對應的 host 端行為尚未實作／❌ 本任務無法保留。

### IME / 輸入

| 狀態 | 踩雷 | 落在哪裡 |
|---|---|---|
| ✅ | xterm.js 6.0.0 `_inputEvent` 會與 `compositionend` 重複交付同一段 IME 文字（浮動視窗組字走 `input(insertText)`；`_finalizeComposition` 的 setTimeout 再送一次） | `sendTyped()`（第 331 行）同字串＋時間窗去重；`isTypedText()`（299）判斷哪些 data 要套 |
| ✅ | IME 文字偶爾整段漏送（時序競賽，`insertText` 差 1ms 趕在 timer 前落地就正常） | `setupImeGuard()`（380）掛在 `.xterm-helper-textarea`；`taUnsent()`（366）算「textarea 裡還沒送出的尾段」、`rescueUnsent()`（371）40ms 後補送 |
| ✅ | 補救送出與 xterm 自己的延遲送出會**重複** | `makeTerm()` 的 `onData`（123–135）用「IME 水位」`rec.taMark`：`d` 已在水位後方＝抑制（`ime-already-sent`）；中間夾著漏送的先補送保序（`ime-rescue ondata`） |
| ✅ | `compositionend` 排的 xterm 延遲送出還沒跑時不該補救（Enter 可能同批進來） | `setupImeGuard()` 的 `compEndPending` 旗標（390–403） |
| ✅ | 注音組字閃英文字（微軟注音每鍵先回報原始鍵值 `h`=ㄏ、`8`=ㄚ 再更新成注音）。v1.0.8 的「內容一變先藏 30ms」造成預覽閃爍、「含字母永久隱藏」讓英數組字（嘸蝦米/倉頡/拼音）整段看不見 | `makeTerm()` 裡 `.composition-view` 的 `MutationObserver`（170–183）：**依目前內容**決定顯示（含英數＝殘影先藏、純注音/中文立刻顯示），加 250ms 後備顯示。只動顯示層、不碰輸入流 |
| ✅ | 貼上（不經 textarea）不能套 IME 水位檢查，否則一般分頁貼單行文字完全沒反應（1.2.6 修） | `makeTerm()` 的 `onData` 檢查條件 `!rec0.pasting`（129） |
| ✅ | Esc / Ctrl+C / Enter 之前要先把 textarea 裡沒送出的補送掉 | `setupImeGuard()` 的 keydown（399）→ `rescueUnsent(rec, id, "enter")` |

### 貼上

| 狀態 | 踩雷 | 落在哪裡 |
|---|---|---|
| ✅ | **多行貼上必須走 `xterm.paste()`**（v1.0.7）：直接寫入 PTY 會把每個換行當 Enter 送出，只剩最後一行留在輸入框 | `doPaste()`（412）的 else 分支：`rec.term.paste(text)`，由 xterm 做 `\r\n`→`\r` 正規化與 bracketed paste 包裝 |
| ✅ | claude 分頁多行貼上「有時候分開貼上」（v1.0.10）：Win10 19045 conhost 會把輸入流的 `ESC[200~`/`ESC[201~` 整組丟棄，claude 只能靠時序猜 | `doPaste()`（415–418）的 claude 分支：換行改 `ESC`+`CR` 軟換行，整段**一次**送，前面先送 `ESC[I` 吸收懸置狀態 |
| ✅ | 瀏覽器原生 Ctrl+V / Shift+Insert 一律要走同一個 `doPaste` | `makeTerm()` 裡 `el` 的 `paste` capture 監聽（247–254）：`preventDefault()` 後自己取 `text/plain` 交給 `doPaste`。**所以新版 Ctrl+V / Shift+Insert / 右鍵貼上（webview 原生選單）已經能用，不需要 host 的 `v` 協定** |
| ⚠️ | 工具列「純文字貼上」走 `v{id}US{base64}` 協定 | `terminal.js` 的 `v` 分支（1019）還在，但**新版還沒有工具列**，host 端也還沒 emit `v`。等分頁/工具列 UI 的任務 |
| ❌ | **對 claude 送「文字＋CR」一次寫入可能不會送出**（1.1.10）：要「文字 → 等 200~300ms → 單獨 CR」 | 這是舊版 **C# 端** `SendSnippet` / 遠端 `SendInputToTab` 的行為，不在 `terminal.js`。新版還沒有「輸入文字視窗」與 Telegram 遠端，**尚未實作** |
| ❌ | **claude 對「多字元輸入塊」的解析脆弱**（v1.0.28 以逐字節流繞過）：前面有 lone ESC 時整塊被吞 | 前端這半邊（claude 輸入佇列 `qPush`/`emitTyped`、`ESC[I` 前置）已沿用；但舊版 C# 端另有「逐字節寫入 ConPTY」的處理，**新版 Rust 端尚未做**（`session_write_text` 是整段 write）。列為 `CLAUDE.md` 風險 7 的待驗證項 |
| — | UI 自動化的 `SendKeys ^v` 不會觸發 webview 的 paste 事件 | 只影響**測試方法**：驗貼上不能用合成按鍵，要真人按或點按鈕。已寫進本次結果的「需要使用者目視確認」 |

### xterm 選項 / CSS

| 狀態 | 踩雷 | 落在哪裡 |
|---|---|---|
| ✅ | **`windowsPty` 不可加**（造成 claude 輸入列殘字；OpenConsole 後端本身折行正確，也不需要它） | `makeTerm()` 的 options（85–86），原註解一併保留 |
| ✅ | `allowProposedApi: true`、`scrollback: 50000` | 同上（85） |
| ✅ | **fit 截行**：padding 要放在 xterm 元素本身（fit addon 會扣除），放父層最後一行被截 1/3 | `src/style.css` 第 14–16 行（從舊版 `web/index.html` 的 `<style>` 原樣搬過來，含原註解） |
| ✅ | **xterm 黑帶**：`xterm.css` 的 `.xterm-viewport` 預設純黑，最後一行下方露黑帶 | `src/style.css` 第 17–18 行 `background-color: transparent !important` |
| ✅ | **初始尺寸勿寫死 80×24** | 新版由 `bridge.js` 開 session 時給 120×30，`n` 之後 `terminal.js` 自己 fit 並送 `r` 校正（舊版是 C# 的 `_lastCols/_lastRows`；設定檔還沒做，所以先用常數） |
| ✅ | **控制字元用常值**（`US = "\x1f"`，勿貼不可見原始字元） | `terminal.js` 第 25 行、`bridge.js`、Rust 側都用 `\x1f` 轉義 |
| ✅ | **BEL→綠燈機制別加回**（曾造成卡橘燈與中文回顯破綻，注音「分段多空格」其實是它插隊） | 沒有實作，也不要加 |

### 輸出時序 / 恢復

| 狀態 | 踩雷 | 落在哪裡 |
|---|---|---|
| ✅ | **恢復 scrollback 必須在 pane fit 到最終寬度之後寫**（在 80×24 寫入後變寬，reflow 的 `_reflowLargerAdjustViewport` 會 `ybase--` 把 scrollback 拉回可視區再被 2J 清掉） | `rec.fitted` / `pendingRestore` / `held` 三者：`markFitted()`（462）、`refit()`（475）、`measureHidden()`（467，分頁模式的隱藏 pane 量不到尺寸）、`applyRestore()`（673）；寫入端在 `writeOutput()`（913） |
| ✅ | **ConPTY 新 session 第一幀一定送 `ESC[2J`**（conhost XtermEngine 首次 StartPaint 會 `_ClearScreen()`），所以在既有 xterm 接新 session 前要先用 `b` 協定推進 scrollback 並把游標歸位左上 | `terminal.js` 的 `b` 分支（933）+ `applyRestore()`（673）都在。⚠️ host 端還沒 emit `b`（還沒有恢復分頁／重連），等後續任務 |
| ✅ | **xterm 卡在「使用者往上捲」＝整格看起來空白**（pane 在 `display:none` 期間被 resize，捲動狀態停住） | `isAtBottom()`（519）、`noteScrollIntent()`（520）、`settleBottoms()`（523）；滾輪／mouseup／PgUp-PgDn 都記意圖（197–200） |
| ✅ | 靜止閘門：claude 輸出靜止 `imeQuietMs` 才送下一筆輸入（1.0.43） | `QUIET_MS`（287，由 `T{json}` 的 `imeQuietMs` 覆寫）、`qPush()`（309）、`writeOutput()` 更新 `lastOutMs` |
| ✅ | **ConPTY 折行是硬換行、`isWrapped` 不會設**，要看「上一列最後一格有沒有字」才接回邏輯行 | `promptLine()`（880）、`lastPlainText()`（788） |
| ✅ | 關閉程式時序列化 scrollback 供下次恢復，**不帶模式切換序列** | `saveBuffer()`（757），`excludeAltBuffer: true, excludeModes: true` |
| ⚠️ | 遠端 `/last` 絕不能用「原始位元組流去 ANSI」，要向 xterm 查渲染後文字（`q…text` → `a…text`） | `lastPlainText()`（788）與 `q`/`a` 協定分支（1008–1018）都在，但 host 端還沒 emit `q`（Telegram 遠端是階段 4） |
| ⚠️ | **清除畫面**：Esc → 延遲 ~60ms → Ctrl+L（黏著送會被 PSReadLine 當 escape 序列）；Telnet/COM 走 `term.clear()` 會清掉 scrollback，所以一律先確認 | `terminal.js` 的 `c` 分支（974）＝`term.clear()` 那一半還在；Esc+60ms+Ctrl+L 與確認對話框是舊版 C# 端的事，**新版尚未實作** |
| ❌ | **Win10 ConPTY 不轉送 alt-screen**（conhost 吃掉 `ESC[?1049h`，用 CUP 絕對定位重畫），所以要設 `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` | 環境變數那半已做在 `src-tauri/src/startup.rs`（同時清掉繼承的 `NO_COLOR`、設 `TERM`/`COLORTERM`）。conhost 本身的行為改用 OpenConsole 後端規避（TASK-002）。**Unix PTY 下要重新驗證**（`CLAUDE.md` 風險 7） |
| ❌ | `std handle`「值」會傳播給 console 子行程＝「分頁一開就死」 | 已在 TASK-002 用 `with_null_std_handles` 處理（只包住 `CreateProcess`），不在前端 |

### 只是「別誤判」的紀錄（沒有對應程式碼）

- **claude 輸入列「第一字後空一格」是顯示殘影不是資料錯誤**：序列化看得到 `❯一[1C]二三`，
  但送出的 prompt 實測是連續的「一二三」，下次整行重繪就消失。上游行為，別當成輸入層 bug。
- **Codex 畫面「少一行」查無 AwayTerminal 端原因**（2026-09-14，未重現）。
- **強殺程式會留殭屍 conhost / OpenConsole**：正常關閉要走 `ClosePseudoConsole`。
  新版在 `lib.rs` 的 `RunEvent::Exit` 收 `close_all()`。
