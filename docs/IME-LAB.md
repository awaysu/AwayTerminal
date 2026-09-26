# IME 事件錄影頁（`public/ime-lab.html`）

對應 `CLAUDE.md` 風險 1 的解法 (a)：

> 先做純 HTML「IME 事件錄影頁」，在 WebView2 / WKWebView / WebKitGTK 跑同一組操作，列出差異表。

## 用途

舊版 `web/terminal.js` 的輸入調校（IME 水位、補救送出、靜止閘門、去重）是在
**WebView2（Chromium）** 上調出來的。`terminal.js` 搬到新版之後，Windows 仍是 WebView2，
但 macOS 是 WKWebView、Linux 是 WebKitGTK，`keydown` / `compositionend` / `input` 的
**送出順序**可能不同，注音的二次輸入、組字殘留就會重現。

這頁**故意不含 xterm.js**，只是一個裸 `<textarea>`（屬性與 xterm 的
`.xterm-helper-textarea` 一致：`autocorrect=off autocapitalize=off spellcheck=false`）
加一個 `contenteditable` 對照組。它記的是**瀏覽器引擎自己送出的事件**，
所以看到的差異一定是引擎差異，不會被 xterm.js 或 `terminal.js` 的邏輯混進來。

拿到各平台的基準之後，才知道要不要做 adapter 層（`ime-chromium.js` / `ime-webkit.js`），
以及要隔離的到底是哪幾個事件。

## 怎麼開

| 情境 | 方法 |
|---|---|
| dev（Vite） | `npm run dev` 之後開 `http://localhost:1420/ime-lab.html` |
| dev（Tauri 視窗內，**建議**） | `npm run tauri dev`，在終端機視窗的網址上加 `/ime-lab.html`，或在 devtools console 打 `location = '/ime-lab.html'` |
| 正式版 | 同上（Vite 的 `public/` 會原樣複製進 bundle，路徑不變） |

**建議在 Tauri 視窗裡開**，而不是用一般瀏覽器：要量的就是這個 webview 的行為，
而且只有在 Tauri 視窗裡「送到後端 log」按鈕才有用（見下）。

## 頁面元素

- **① textarea**：模擬 xterm 的隱藏輸入框，這是主要觀察對象。
- **② contenteditable**：對照組。有些輸入法對 `contenteditable` 與 `textarea` 的行為不同。
- 事件表格欄位：`#`、`t(ms)`、`target`、`event`、`key`、`code`、`keyCode`、
  `isComposing`、`inputType`、`data`、`value 尾端`（24 字）。
- 記錄的事件：`keydown`、`keypress`、`keyup`、`beforeinput`、`input`、
  `compositionstart`、`compositionupdate`、`compositionend`、`paste`。
- **橘色分隔線＝macrotask 邊界**（用 `setTimeout(…, 0)` 標記）。線以下是新的事件迴圈。
  **這是整頁最重要的資訊**：風險 1 的猜測是「Chromium 先 `keydown`(229, isComposing)
  再 `compositionend`；WebKit 先 `compositionend` 再送 isComposing=false 的 Enter」，
  要判斷這件事，就得知道哪些事件在**同一個**事件迴圈裡。

### 按鈕

| 按鈕 | 作用 |
|---|---|
| 清除 | 清空表格並重設計時起點 |
| 分段標記 | 在表格插一列標記，**每做完一個劇本按一次**，事後才分得出哪段是哪個劇本 |
| 複製為 Markdown | 整份表格複製成 Markdown（clipboard API 失敗時退回 `execCommand`） |
| 下載 .md | 存成 `ime-<時間>.md` |
| 送到後端 log | 透過現有的 `log_line` 指令**整包**送到後端 stdout。不在 Tauri 視窗裡（用瀏覽器開的）會顯示提示並跳過 |

「送到後端 log」是給代理用的：使用者不必截圖、也不用搶焦點，跑完按一下，
結果就出現在 `npm run tauri dev` 的終端輸出裡。

## 九項固定劇本

頁頂也列著同一份清單。**照順序做一次，每做完一項按「分段標記」**：

| # | 劇本 | 要看什麼 |
|---|---|---|
| ① | 注音打「測試」，按 `Enter` 確認組字 | **最關鍵**：`keydown`(Enter) 與 `compositionend` 的先後、Enter 的 `keyCode` 是 229 還是 13、`isComposing` 是 true 還是 false、兩者是否同一個 macrotask |
| ② | 組字中按 `Backspace`（打一半就退格） | 有沒有 `compositionupdate`、有沒有多送一個 Backspace 給終端機 |
| ③ | 組字中按 `Esc` | `compositionend` 的 `data` 是空字串還是原文；Esc 會不會漏到終端機（會中斷 claude） |
| ④ | 候選字視窗出來後用**數字鍵**選字 | 數字鍵會不會同時當成輸入送出（`beforeinput`/`input` 的 `inputType`） |
| ⑤ | `Shift` 切成英文後打 `abc` | 中英切換有沒有殘留的 composition 狀態 |
| ⑥ | 貼上多行文字（`Ctrl+V`，先複製 3 行以上） | `paste` 事件有沒有來、`clipboardData` 拿不拿得到、之後有幾個 `input` |
| ⑦ | 直接打英文 `abc` 再按 `Enter`（**對照組**，沒有組字） | 正常路徑長什麼樣，用來對比 ① |
| ⑧ | 打全形標點（`，` `。` `？`） | 全形標點走 composition 還是直接 `input` |
| ⑨ | 快速連打「ㄘㄜˋ ㄕˋ」**不等候選視窗** | 事件會不會擠在同一個 macrotask、`compositionupdate` 會不會掉 |

劇本 ①⑨ 對應舊版踩雷紀錄裡最麻煩的兩條（`_inputEvent` 與 `compositionend`
重複交付、注音組字閃英數字）；⑥ 對應多行貼上；③ 對應「Esc 中斷 claude」。

## `docs/ime-baseline/`

各平台跑完的結果檔放這裡，一個平台一個檔：

| 檔名 | 平台 | 狀態 |
|---|---|---|
| `webview2-win10.md` | Windows 10 19045 + WebView2 + 微軟注音 | **待使用者操作**（代理不能搶焦點打字） |
| `wkwebview.md` | macOS + WKWebView + 系統注音 | 待階段 1 的 mac 驗證 |
| `webkitgtk-fcitx5.md` | Ubuntu + WebKitGTK + fcitx5 | 待階段 1 的 Linux 驗證 |
| `webkitgtk-ibus.md` | Ubuntu + WebKitGTK + ibus | 同上（ibus 與 fcitx5 行為不同，分開存） |

每個檔案建議的開頭（`複製為 Markdown` 的輸出貼在後面）：

```markdown
# <平台> IME 基準
- 日期：
- OS / 版本：
- Webview 版本：
- 輸入法：
- UA：（頁面上那行）
```

**Windows 的這份是基準**，其餘平台都跟它比。有了兩份以上就可以做差異表，
決定 `terminal.js` 要不要拆 adapter。
