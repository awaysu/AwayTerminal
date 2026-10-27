# WKWebView（macOS）IME 基準

- 日期：2026-10-27
- OS / 版本：macOS 26.6.2（25G83），Mac mini arm64
- Webview：WKWebView（UA `AppleWebKit/605.1.15`）
- 輸入法：系統內建注音（`com.apple.inputmethod.TCIM.Zhuyin`）、ABC
- 錄法：`npm run ime-lab`（Tauri dev 視窗）。⚠️ 使用者是**遠端桌面＋Windows 鍵盤**操作，事件先後可信、時間間隔不可信；
  ⑧（全形標點，遠端打不出來）、⑨（快速連打，看時序）**還沒錄**，要在 Mac 前面用實體鍵盤補

## 摘要（和 Windows 基準比之前的觀察）

| 劇本 | dump | 觀察 |
|---|---|---|
| ① 注音＋Enter | 1 | 每個鍵都是**組字事件先、`keydown`(229) 後**。Enter：`compositionend` 先，再來 `keydown` Enter **keyCode 229、isComposing=false** → xterm 走 229 分支，**不會多送 Enter** |
| ② 組字中 Backspace | 2 | `compositionupdate` 先、`keydown`(229) 後，不漏給終端機 |
| ③ 組字中 Esc | 3 | ⚠️ **Esc 不是取消組字**：先把 `ㄘㄜ` **確認送出**（`compositionend` data=`ㄘㄜ`），再送一個真的 Esc（27＋keypress）→ 終端機收到注音符號＋ESC（Claude Code 會被中斷） |
| ④ ↓ 叫候選 | 4 | ⚠️ ↓ 一樣先確認組字、再送一般 `ArrowDown`(40) → 終端機收到「測」＋下方向鍵 |
| ④ 空白叫候選＋Enter 選字 | 5 | 候選視窗內的鍵全是 229／isComposing=true，不漏；數字鍵選字沒錄到 |
| ⑤ Caps Lock 切英文 | 6 | 切換當下不結束組字，打下一個字母時才確認 `ㄘㄜ`；**之後的英文字母是 `input(insertText)` 先、`keydown`(229) 後** |
| ⑥ Cmd+V 多行貼上 | 7 | `paste` 事件正常、`clipboardData` 拿得到三行；Cmd+V 多一個 `keypress` v（有 meta，xterm 略過） |
| ⑦ 注音＋Caps Lock 打 abc（誤錄，當成 ⑤ 的補充） | 8、9 | 同 ⑤：字母 insertText 先、229 後；Enter 是正常 13 |
| ⑦ ABC 打 abc＋Enter（對照組） | 10 | 正常：`keydown`(65)→`keypress`→`input`；Enter 13 |
| ⑪ ABC 按空白 | 11 | 正常：`keydown`(32)→`keypress`→`input` |
| ⑩ 注音（沒組字）按空白 | 12 | 同 ⑪，正常路徑 |
| ⑩b 注音＋Caps Lock 打 `ls la` | 13 | 字母 insertText 先、229 後；空白仍是正常 `keydown`(32) 路徑 |

### 2.1.10 的空白變兩個

錄影頁是裸 textarea，**看不到**這個問題。在真的 app 開 `IMEDBG`（`src/terminal.js`）用 AppleScript 送鍵才重現：
`onData " "` 之後 WebKit 仍把 **U+00A0** 插進 xterm 的 textarea（keypress 的 preventDefault 沒擋住），
40ms 後 `ime-rescue timer "\u00a0"` 又送一次。修法見 `terminal.js` 的 `IME_RESCUE_MS` 註解。

## 原始紀錄

<!-- dump 1 -->
# IME 事件錄影：2026-10-27T08:04:07.980Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 34

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 2591.00 | ta | compositionstart |  |  |  |  |  | "" | "" |
|  | 2 | 2593.00 | ta | compositionupdate |  |  |  |  |  | ㄘ | "" |
|  | 3 | 2594.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘ | "" |
|  | 4 | 2595.00 | ta | input |  |  |  | true | insertCompositionText | ㄘ | ㄘ |
| ▲ | 5 | 2599.00 | ta | keydown | ㄘ | KeyH | 229 | true |  |  | ㄘ |
| ▲ | 6 | 2690.00 | ta | keyup | ㄘ | KeyH | 72 | true |  |  | ㄘ |
| ▲ | 7 | 2761.00 | ta | compositionupdate |  |  |  |  |  | ㄘㄜ | ㄘ |
|  | 8 | 2763.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘ |
|  | 9 | 2765.00 | ta | input |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘㄜ |
|  | 10 | 2766.00 | ta | keydown | ㄜ | KeyK | 229 | true |  |  | ㄘㄜ |
| ▲ | 11 | 2957.00 | ta | keyup | ㄜ | KeyK | 75 | true |  |  | ㄘㄜ |
| ▲ | 12 | 2993.00 | ta | compositionupdate |  |  |  |  |  | 測 | ㄘㄜ |
|  | 13 | 2994.00 | ta | beforeinput |  |  |  | true | insertCompositionText | 測 | ㄘㄜ |
|  | 14 | 2996.00 | ta | input |  |  |  | true | insertCompositionText | 測 | 測 |
| ▲ | 15 | 2997.00 | ta | keydown | ˋ | Digit4 | 229 | true |  |  | 測 |
| ▲ | 16 | 3114.00 | ta | keyup | ˋ | Digit4 | 52 | true |  |  | 測 |
| ▲ | 17 | 3739.00 | ta | compositionupdate |  |  |  |  |  | 測ㄕ | 測 |
|  | 18 | 3741.00 | ta | beforeinput |  |  |  | true | insertCompositionText | 測ㄕ | 測 |
|  | 19 | 3742.00 | ta | input |  |  |  | true | insertCompositionText | 測ㄕ | 測ㄕ |
|  | 20 | 3744.00 | ta | keydown | ㄕ | KeyG | 229 | true |  |  | 測ㄕ |
| ▲ | 21 | 3861.00 | ta | keyup | ㄕ | KeyG | 71 | true |  |  | 測ㄕ |
| ▲ | 22 | 3998.00 | ta | compositionupdate |  |  |  |  |  | 測試 | 測ㄕ |
|  | 23 | 4001.00 | ta | beforeinput |  |  |  | true | insertCompositionText | 測試 | 測ㄕ |
|  | 24 | 4003.00 | ta | input |  |  |  | true | insertCompositionText | 測試 | 測試 |
|  | 25 | 4005.00 | ta | keydown | ˋ | Digit4 | 229 | true |  |  | 測試 |
| ▲ | 26 | 4131.00 | ta | keyup | ˋ | Digit4 | 52 | true |  |  | 測試 |
| ▲ | 27 | 4185.00 | ta | beforeinput |  |  |  | true | deleteCompositionText | null | 測試 |
|  | 28 | 4188.00 | ta | input |  |  |  | true | deleteCompositionText | null | "" |
|  | 29 | 4190.00 | ta | beforeinput |  |  |  | true | insertFromComposition | 測試 | "" |
|  | 30 | 4191.00 | ta | input |  |  |  | true | insertFromComposition | 測試 | 測試 |
|  | 31 | 4193.00 | ta | compositionend |  |  |  |  |  | 測試 | 測試 |
| ▲ | 32 | 4195.00 | ta | keydown | Enter | Enter | 229 | false |  |  | 測試 |
| ▲ | 33 | 4278.00 | ta | keyup | Enter | Enter | 13 | false |  |  | 測試 |
| ▲ | 34 | 7044.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 2 -->
# IME 事件錄影：2026-10-27T08:04:56.824Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 25

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 2048.00 | ta | compositionstart |  |  |  |  |  | "" | "" |
|  | 2 | 2050.00 | ta | compositionupdate |  |  |  |  |  | ㄘ | "" |
|  | 3 | 2051.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘ | "" |
|  | 4 | 2052.00 | ta | input |  |  |  | true | insertCompositionText | ㄘ | ㄘ |
| ▲ | 5 | 2054.00 | ta | keydown | ㄘ | KeyH | 229 | true |  |  | ㄘ |
| ▲ | 6 | 2114.00 | ta | keyup | ㄘ | KeyH | 72 | true |  |  | ㄘ |
| ▲ | 7 | 2388.00 | ta | compositionupdate |  |  |  |  |  | ㄘㄜ | ㄘ |
|  | 8 | 2390.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘ |
|  | 9 | 2391.00 | ta | input |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘㄜ |
|  | 10 | 2395.00 | ta | keydown | ㄜ | KeyK | 229 | true |  |  | ㄘㄜ |
| ▲ | 11 | 2475.00 | ta | keyup | ㄜ | KeyK | 75 | true |  |  | ㄘㄜ |
| ▲ | 12 | 4048.00 | ta | compositionupdate |  |  |  |  |  | ㄘ | ㄘㄜ |
|  | 13 | 4050.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘ | ㄘㄜ |
|  | 14 | 4051.00 | ta | input |  |  |  | true | insertCompositionText | ㄘ | ㄘ |
|  | 15 | 4053.00 | ta | keydown | Backspace | Backspace | 229 | true |  |  | ㄘ |
| ▲ | 16 | 4148.00 | ta | keyup | Backspace | Backspace | 8 | true |  |  | ㄘ |
| ▲ | 17 | 4830.00 | ta | beforeinput |  |  |  | true | deleteCompositionText | null | ㄘ |
|  | 18 | 4833.00 | ta | input |  |  |  | true | deleteCompositionText | null | "" |
|  | 19 | 4834.00 | ta | beforeinput |  |  |  | true | insertFromComposition | ㄘ | "" |
|  | 20 | 4835.00 | ta | input |  |  |  | true | insertFromComposition | ㄘ | ㄘ |
|  | 21 | 4837.00 | ta | compositionend |  |  |  |  |  | ㄘ | ㄘ |
| ▲ | 22 | 4845.00 | ta | keydown | Escape | Escape | 27 | false |  |  | ㄘ |
|  | 23 | 4847.00 | ta | keypress | Escape | Escape | 27 | false |  |  | ㄘ |
| ▲ | 24 | 4937.00 | ta | keyup | Escape | Escape | 27 | false |  |  | ㄘ |
| ▲ | 25 | 7446.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 3 -->
# IME 事件錄影：2026-10-27T08:05:56.967Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 20

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 1451.00 | ta | compositionstart |  |  |  |  |  | "" | "" |
|  | 2 | 1453.00 | ta | compositionupdate |  |  |  |  |  | ㄘ | "" |
|  | 3 | 1454.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘ | "" |
|  | 4 | 1456.00 | ta | input |  |  |  | true | insertCompositionText | ㄘ | ㄘ |
| ▲ | 5 | 1459.00 | ta | keydown | ㄘ | KeyH | 229 | true |  |  | ㄘ |
| ▲ | 6 | 1574.00 | ta | keyup | ㄘ | KeyH | 72 | true |  |  | ㄘ |
| ▲ | 7 | 1724.00 | ta | compositionupdate |  |  |  |  |  | ㄘㄜ | ㄘ |
|  | 8 | 1725.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘ |
|  | 9 | 1727.00 | ta | input |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘㄜ |
|  | 10 | 1728.00 | ta | keydown | ㄜ | KeyK | 229 | true |  |  | ㄘㄜ |
| ▲ | 11 | 1821.00 | ta | keyup | ㄜ | KeyK | 75 | true |  |  | ㄘㄜ |
| ▲ | 12 | 2567.00 | ta | beforeinput |  |  |  | true | deleteCompositionText | null | ㄘㄜ |
|  | 13 | 2570.00 | ta | input |  |  |  | true | deleteCompositionText | null | "" |
|  | 14 | 2571.00 | ta | beforeinput |  |  |  | true | insertFromComposition | ㄘㄜ | "" |
|  | 15 | 2572.00 | ta | input |  |  |  | true | insertFromComposition | ㄘㄜ | ㄘㄜ |
|  | 16 | 2573.00 | ta | compositionend |  |  |  |  |  | ㄘㄜ | ㄘㄜ |
| ▲ | 17 | 2577.00 | ta | keydown | Escape | Escape | 27 | false |  |  | ㄘㄜ |
|  | 18 | 2580.00 | ta | keypress | Escape | Escape | 27 | false |  |  | ㄘㄜ |
| ▲ | 19 | 2658.00 | ta | keyup | Escape | Escape | 27 | false |  |  | ㄘㄜ |
| ▲ | 20 | 6475.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 4 -->
# IME 事件錄影：2026-10-27T08:07:08.043Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 24

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 1087.00 | ta | compositionstart |  |  |  |  |  | "" | "" |
|  | 2 | 1089.00 | ta | compositionupdate |  |  |  |  |  | ㄘ | "" |
|  | 3 | 1090.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘ | "" |
|  | 4 | 1092.00 | ta | input |  |  |  | true | insertCompositionText | ㄘ | ㄘ |
| ▲ | 5 | 1095.00 | ta | keydown | ㄘ | KeyH | 229 | true |  |  | ㄘ |
| ▲ | 6 | 1158.00 | ta | keyup | ㄘ | KeyH | 72 | true |  |  | ㄘ |
| ▲ | 7 | 1310.00 | ta | compositionupdate |  |  |  |  |  | ㄘㄜ | ㄘ |
|  | 8 | 1313.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘ |
|  | 9 | 1315.00 | ta | input |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘㄜ |
|  | 10 | 1316.00 | ta | keydown | ㄜ | KeyK | 229 | true |  |  | ㄘㄜ |
| ▲ | 11 | 1399.00 | ta | keyup | ㄜ | KeyK | 75 | true |  |  | ㄘㄜ |
| ▲ | 12 | 2602.00 | ta | compositionupdate |  |  |  |  |  | 測 | ㄘㄜ |
|  | 13 | 2605.00 | ta | beforeinput |  |  |  | true | insertCompositionText | 測 | ㄘㄜ |
|  | 14 | 2606.00 | ta | input |  |  |  | true | insertCompositionText | 測 | 測 |
|  | 15 | 2607.00 | ta | keydown | ˋ | Digit4 | 229 | true |  |  | 測 |
| ▲ | 16 | 2696.00 | ta | keyup | ˋ | Digit4 | 52 | true |  |  | 測 |
| ▲ | 17 | 3816.00 | ta | beforeinput |  |  |  | true | deleteCompositionText | null | 測 |
|  | 18 | 3820.00 | ta | input |  |  |  | true | deleteCompositionText | null | "" |
|  | 19 | 3821.00 | ta | beforeinput |  |  |  | true | insertFromComposition | 測 | "" |
|  | 20 | 3822.00 | ta | input |  |  |  | true | insertFromComposition | 測 | 測 |
|  | 21 | 3823.00 | ta | compositionend |  |  |  |  |  | 測 | 測 |
| ▲ | 22 | 3829.00 | ta | keydown | ArrowDown | ArrowDown | 40 | false |  |  | 測 |
| ▲ | 23 | 3948.00 | ta | keyup | ArrowDown | ArrowDown | 40 | false |  |  | 測 |
| ▲ | 24 | 7693.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 5 -->
# IME 事件錄影：2026-10-27T08:08:08.068Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 34

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 5694.00 | ta | compositionstart |  |  |  |  |  | "" | "" |
|  | 2 | 5695.00 | ta | compositionupdate |  |  |  |  |  | ㄘ | "" |
|  | 3 | 5697.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘ | "" |
|  | 4 | 5698.00 | ta | input |  |  |  | true | insertCompositionText | ㄘ | ㄘ |
| ▲ | 5 | 5701.00 | ta | keydown | ㄘ | KeyH | 229 | true |  |  | ㄘ |
| ▲ | 6 | 5780.00 | ta | keyup | ㄘ | KeyH | 72 | true |  |  | ㄘ |
| ▲ | 7 | 5962.00 | ta | compositionupdate |  |  |  |  |  | ㄘㄜ | ㄘ |
|  | 8 | 5964.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘ |
|  | 9 | 5965.00 | ta | input |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘㄜ |
|  | 10 | 5966.00 | ta | keydown | ㄜ | KeyK | 229 | true |  |  | ㄘㄜ |
| ▲ | 11 | 6066.00 | ta | keyup | ㄜ | KeyK | 75 | true |  |  | ㄘㄜ |
| ▲ | 12 | 6566.00 | ta | compositionupdate |  |  |  |  |  | 測 | ㄘㄜ |
|  | 13 | 6568.00 | ta | beforeinput |  |  |  | true | insertCompositionText | 測 | ㄘㄜ |
|  | 14 | 6570.00 | ta | input |  |  |  | true | insertCompositionText | 測 | 測 |
|  | 15 | 6571.00 | ta | keydown | ˋ | Digit4 | 229 | true |  |  | 測 |
| ▲ | 16 | 6677.00 | ta | keyup | ˋ | Digit4 | 52 | true |  |  | 測 |
| ▲ | 17 | 7520.00 | ta | compositionupdate |  |  |  |  |  | 測 | 測 |
|  | 18 | 7522.00 | ta | beforeinput |  |  |  | true | insertCompositionText | 測 | 測 |
|  | 19 | 7523.00 | ta | input |  |  |  | true | insertCompositionText | 測 | 測 |
| ▲ | 20 | 7524.00 | ta | keydown |   | Space | 229 | true |  |  | 測 |
| ▲ | 21 | 7610.00 | ta | keyup |   | Space | 32 | true |  |  | 測 |
| ▲ | 22 | 9198.00 | ta | compositionupdate |  |  |  |  |  | 測 | 測 |
|  | 23 | 9201.00 | ta | beforeinput |  |  |  | true | insertCompositionText | 測 | 測 |
|  | 24 | 9203.00 | ta | input |  |  |  | true | insertCompositionText | 測 | 測 |
| ▲ | 25 | 9209.00 | ta | keydown | Enter | Enter | 229 | true |  |  | 測 |
| ▲ | 26 | 9385.00 | ta | keyup | Enter | Enter | 13 | true |  |  | 測 |
| ▲ | 27 | 11058.00 | ta | beforeinput |  |  |  | true | deleteCompositionText | null | 測 |
|  | 28 | 11067.00 | ta | input |  |  |  | true | deleteCompositionText | null | "" |
|  | 29 | 11068.00 | ta | beforeinput |  |  |  | true | insertFromComposition | 測 | "" |
|  | 30 | 11069.00 | ta | input |  |  |  | true | insertFromComposition | 測 | 測 |
|  | 31 | 11070.00 | ta | compositionend |  |  |  |  |  | 測 | 測 |
| ▲ | 32 | 11071.00 | ta | keydown | Enter | Enter | 229 | false |  |  | 測 |
| ▲ | 33 | 11144.00 | ta | keyup | Enter | Enter | 13 | false |  |  | 測 |
| ▲ | 34 | 12468.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 6 -->
# IME 事件錄影：2026-10-27T08:08:59.315Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 31

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 1412.00 | ta | compositionstart |  |  |  |  |  | "" | "" |
|  | 2 | 1414.00 | ta | compositionupdate |  |  |  |  |  | ㄘ | "" |
|  | 3 | 1415.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘ | "" |
|  | 4 | 1416.00 | ta | input |  |  |  | true | insertCompositionText | ㄘ | ㄘ |
| ▲ | 5 | 1419.00 | ta | keydown | ㄘ | KeyH | 229 | true |  |  | ㄘ |
| ▲ | 6 | 1490.00 | ta | keyup | ㄘ | KeyH | 72 | true |  |  | ㄘ |
| ▲ | 7 | 1698.00 | ta | compositionupdate |  |  |  |  |  | ㄘㄜ | ㄘ |
|  | 8 | 1700.00 | ta | beforeinput |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘ |
|  | 9 | 1701.00 | ta | input |  |  |  | true | insertCompositionText | ㄘㄜ | ㄘㄜ |
|  | 10 | 1702.00 | ta | keydown | ㄜ | KeyK | 229 | true |  |  | ㄘㄜ |
| ▲ | 11 | 1778.00 | ta | keyup | ㄜ | KeyK | 75 | true |  |  | ㄘㄜ |
| ▲ | 12 | 6761.00 | ta | keydown | CapsLock | CapsLock | 20 | true |  |  | ㄘㄜ |
| ▲ | 13 | 6855.00 | ta | keydown | CapsLock | CapsLock | 20 | true |  |  | ㄘㄜ |
| ▲ | 14 | 7772.00 | ta | beforeinput |  |  |  | true | deleteCompositionText | null | ㄘㄜ |
|  | 15 | 7775.00 | ta | input |  |  |  | true | deleteCompositionText | null | "" |
|  | 16 | 7776.00 | ta | beforeinput |  |  |  | true | insertFromComposition | ㄘㄜ | "" |
|  | 17 | 7777.00 | ta | input |  |  |  | true | insertFromComposition | ㄘㄜ | ㄘㄜ |
|  | 18 | 7777.00 | ta | compositionend |  |  |  |  |  | ㄘㄜ | ㄘㄜ |
| ▲ | 19 | 7782.00 | ta | beforeinput |  |  |  | false | insertText | a | ㄘㄜ |
|  | 20 | 7783.00 | ta | input |  |  |  | false | insertText | a | ㄘㄜa |
|  | 21 | 7785.00 | ta | keydown | a | KeyA | 229 | false |  |  | ㄘㄜa |
| ▲ | 22 | 7884.00 | ta | keyup | a | KeyA | 65 | false |  |  | ㄘㄜa |
| ▲ | 23 | 8122.00 | ta | beforeinput |  |  |  | false | insertText | b | ㄘㄜa |
|  | 24 | 8125.00 | ta | input |  |  |  | false | insertText | b | ㄘㄜab |
| ▲ | 25 | 8130.00 | ta | keydown | b | KeyB | 229 | false |  |  | ㄘㄜab |
| ▲ | 26 | 8193.00 | ta | keyup | b | KeyB | 66 | false |  |  | ㄘㄜab |
| ▲ | 27 | 8381.00 | ta | beforeinput |  |  |  | false | insertText | c | ㄘㄜab |
|  | 28 | 8383.00 | ta | input |  |  |  | false | insertText | c | ㄘㄜabc |
| ▲ | 29 | 8389.00 | ta | keydown | c | KeyC | 229 | false |  |  | ㄘㄜabc |
| ▲ | 30 | 8468.00 | ta | keyup | c | KeyC | 67 | false |  |  | ㄘㄜabc |
| ▲ | 31 | 10957.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 7 -->
# IME 事件錄影：2026-10-27T08:10:34.590Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 11

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 8658.00 | ta | keydown | Meta | MetaLeft | 91 | false |  |  | "" |
| ▲ | 2 | 8704.00 | ta | keyup | Meta | MetaLeft | 91 | false |  |  | "" |
| ▲ | 3 | 12008.00 | ta | keydown | Meta | MetaLeft | 91 | false |  |  | "" |
| ▲ | 4 | 12411.00 | ta | keydown | v | KeyV | 86 | false |  |  | "" |
|  | 5 | 12413.00 | ta | keypress | v | KeyV | 118 | false |  |  | "" |
| ▲ | 6 | 12417.00 | ta | keyup | v | KeyV | 86 | false |  |  | "" |
| ▲ | 7 | 12436.00 | ta | paste |  |  |  |  |  | 123\nAbc\nGgghhh | "" |
|  | 8 | 12454.00 | ta | beforeinput |  |  |  | false | insertFromPaste | 123\nAbc\nGgghhh | "" |
|  | 9 | 12455.00 | ta | input |  |  |  | false | insertFromPaste | 123\nAbc\nGgghhh | 123\nAbc\nGgghhh |
| ▲ | 10 | 12473.00 | ta | keyup | Meta | MetaLeft | 91 | false |  |  | 123\nAbc\nGgghhh |
| ▲ | 11 | 24328.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 8 -->
# IME 事件錄影：2026-10-27T08:11:16.850Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 13

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 985.00 | ta | beforeinput |  |  |  | false | insertText | a | "" |
|  | 2 | 988.00 | ta | input |  |  |  | false | insertText | a | a |
| ▲ | 3 | 989.00 | ta | keydown | a | KeyA | 229 | false |  |  | a |
| ▲ | 4 | 1081.00 | ta | keyup | a | KeyA | 65 | false |  |  | a |
| ▲ | 5 | 1353.00 | ta | beforeinput |  |  |  | false | insertText | b | a |
|  | 6 | 1356.00 | ta | input |  |  |  | false | insertText | b | ab |
| ▲ | 7 | 1359.00 | ta | keydown | b | KeyB | 229 | false |  |  | ab |
| ▲ | 8 | 1426.00 | ta | keyup | b | KeyB | 66 | false |  |  | ab |
| ▲ | 9 | 1833.00 | ta | beforeinput |  |  |  | false | insertText | c | ab |
|  | 10 | 1836.00 | ta | input |  |  |  | false | insertText | c | abc |
| ▲ | 11 | 1840.00 | ta | keydown | c | KeyC | 229 | false |  |  | abc |
| ▲ | 12 | 1912.00 | ta | keyup | c | KeyC | 67 | false |  |  | abc |
| ▲ | 13 | 3040.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 9 -->
# IME 事件錄影：2026-10-27T08:12:21.169Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 18

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 1250.00 | ta | beforeinput |  |  |  | false | insertText | a | "" |
|  | 2 | 1253.00 | ta | input |  |  |  | false | insertText | a | a |
| ▲ | 3 | 1254.00 | ta | keydown | a | KeyA | 229 | false |  |  | a |
| ▲ | 4 | 1373.00 | ta | keyup | a | KeyA | 65 | false |  |  | a |
| ▲ | 5 | 5860.00 | ta | beforeinput |  |  |  | false | insertText | b | a |
|  | 6 | 5863.00 | ta | input |  |  |  | false | insertText | b | ab |
| ▲ | 7 | 5866.00 | ta | keydown | b | KeyB | 229 | false |  |  | ab |
| ▲ | 8 | 5965.00 | ta | keyup | b | KeyB | 66 | false |  |  | ab |
| ▲ | 9 | 6861.00 | ta | beforeinput |  |  |  | false | insertText | c | ab |
|  | 10 | 6863.00 | ta | input |  |  |  | false | insertText | c | abc |
| ▲ | 11 | 6867.00 | ta | keydown | c | KeyC | 229 | false |  |  | abc |
| ▲ | 12 | 6997.00 | ta | keyup | c | KeyC | 67 | false |  |  | abc |
| ▲ | 13 | 7909.00 | ta | keydown | Enter | Enter | 13 | false |  |  | abc |
|  | 14 | 7915.00 | ta | keypress | Enter | Enter | 13 | false |  |  | abc |
|  | 15 | 7915.00 | ta | beforeinput |  |  |  | false | insertLineBreak | null | abc |
|  | 16 | 7918.00 | ta | input |  |  |  | false | insertLineBreak | null | abc\n |
| ▲ | 17 | 8016.00 | ta | keyup | Enter | Enter | 13 | false |  |  | abc\n |
| ▲ | 18 | 9299.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 10 -->
# IME 事件錄影：2026-10-27T08:12:54.555Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 21

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 1081.00 | ta | keydown | A | KeyA | 65 | false |  |  | "" |
|  | 2 | 1083.00 | ta | keypress | A | KeyA | 65 | false |  |  | "" |
|  | 3 | 1084.00 | ta | beforeinput |  |  |  | false | insertText | A | "" |
|  | 4 | 1086.00 | ta | input |  |  |  | false | insertText | A | A |
| ▲ | 5 | 1193.00 | ta | keyup | A | KeyA | 65 | false |  |  | A |
| ▲ | 6 | 2641.00 | ta | keydown | B | KeyB | 66 | false |  |  | A |
|  | 7 | 2643.00 | ta | keypress | B | KeyB | 66 | false |  |  | A |
|  | 8 | 2644.00 | ta | beforeinput |  |  |  | false | insertText | B | A |
|  | 9 | 2645.00 | ta | input |  |  |  | false | insertText | B | AB |
| ▲ | 10 | 2735.00 | ta | keyup | B | KeyB | 66 | false |  |  | AB |
| ▲ | 11 | 3083.00 | ta | keydown | C | KeyC | 67 | false |  |  | AB |
|  | 12 | 3085.00 | ta | keypress | C | KeyC | 67 | false |  |  | AB |
|  | 13 | 3086.00 | ta | beforeinput |  |  |  | false | insertText | C | AB |
|  | 14 | 3087.00 | ta | input |  |  |  | false | insertText | C | ABC |
| ▲ | 15 | 3176.00 | ta | keyup | C | KeyC | 67 | false |  |  | ABC |
| ▲ | 16 | 3643.00 | ta | keydown | Enter | Enter | 13 | false |  |  | ABC |
|  | 17 | 3648.00 | ta | keypress | Enter | Enter | 13 | false |  |  | ABC |
|  | 18 | 3649.00 | ta | beforeinput |  |  |  | false | insertLineBreak | null | ABC |
|  | 19 | 3653.00 | ta | input |  |  |  | false | insertLineBreak | null | ABC\n |
| ▲ | 20 | 3718.00 | ta | keyup | Enter | Enter | 13 | false |  |  | ABC\n |
| ▲ | 21 | 4981.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 11 -->
# IME 事件錄影：2026-10-27T08:15:04.349Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 16

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 1158.00 | ta | keydown |   | Space | 32 | false |  |  | "" |
|  | 2 | 1160.00 | ta | keypress |   | Space | 32 | false |  |  | "" |
|  | 3 | 1162.00 | ta | beforeinput |  |  |  | false | insertText |   | "" |
|  | 4 | 1164.00 | ta | input |  |  |  | false | insertText |   |   |
| ▲ | 5 | 1307.00 | ta | keyup |   | Space | 32 | false |  |  |   |
| ▲ | 6 | 2196.00 | ta | keydown |   | Space | 32 | false |  |  |   |
|  | 7 | 2198.00 | ta | keypress |   | Space | 32 | false |  |  |   |
|  | 8 | 2200.00 | ta | beforeinput |  |  |  | false | insertText |   |   |
|  | 9 | 2201.00 | ta | input |  |  |  | false | insertText |   |    |
| ▲ | 10 | 2320.00 | ta | keyup |   | Space | 32 | false |  |  |    |
| ▲ | 11 | 2983.00 | ta | keydown |   | Space | 32 | false |  |  |    |
|  | 12 | 2985.00 | ta | keypress |   | Space | 32 | false |  |  |    |
|  | 13 | 2986.00 | ta | beforeinput |  |  |  | false | insertText |   |    |
|  | 14 | 2987.00 | ta | input |  |  |  | false | insertText |   |     |
| ▲ | 15 | 3112.00 | ta | keyup |   | Space | 32 | false |  |  |     |
| ▲ | 16 | 4443.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 12 -->
# IME 事件錄影：2026-10-27T08:15:41.261Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 16

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 5531.00 | ta | keydown |   | Space | 32 | false |  |  | "" |
|  | 2 | 5534.00 | ta | keypress |   | Space | 32 | false |  |  | "" |
|  | 3 | 5535.00 | ta | beforeinput |  |  |  | false | insertText |   | "" |
|  | 4 | 5537.00 | ta | input |  |  |  | false | insertText |   |   |
| ▲ | 5 | 5652.00 | ta | keyup |   | Space | 32 | false |  |  |   |
| ▲ | 6 | 6336.00 | ta | keydown |   | Space | 32 | false |  |  |   |
|  | 7 | 6338.00 | ta | keypress |   | Space | 32 | false |  |  |   |
|  | 8 | 6339.00 | ta | beforeinput |  |  |  | false | insertText |   |   |
|  | 9 | 6340.00 | ta | input |  |  |  | false | insertText |   |    |
| ▲ | 10 | 6468.00 | ta | keyup |   | Space | 32 | false |  |  |    |
| ▲ | 11 | 6937.00 | ta | keydown |   | Space | 32 | false |  |  |    |
|  | 12 | 6939.00 | ta | keypress |   | Space | 32 | false |  |  |    |
|  | 13 | 6940.00 | ta | beforeinput |  |  |  | false | insertText |   |    |
|  | 14 | 6941.00 | ta | input |  |  |  | false | insertText |   |     |
| ▲ | 15 | 7061.00 | ta | keyup |   | Space | 32 | false |  |  |     |
| ▲ | 16 | 8217.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |

<!-- dump 13 -->
# IME 事件錄影：2026-10-27T08:16:42.377Z

- UA: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)`
- 平台: `MacIntel`
- 筆數: 29

`▲` 那一列之前是 macrotask 邊界（新的事件迴圈）。

| ▲ | # | t(ms) | target | event | key | code | keyCode | isComposing | inputType | data | value 尾端 |
|---|---|---|---|---|---|---|---|---|---|---|---|
|  | 1 | 2390.00 | ta | keydown | CapsLock | CapsLock | 20 | false |  |  | "" |
| ▲ | 2 | 2478.00 | ta | keydown | CapsLock | CapsLock | 20 | false |  |  | "" |
| ▲ | 3 | 7405.00 | ta | beforeinput |  |  |  | false | insertText | l | "" |
|  | 4 | 7408.00 | ta | input |  |  |  | false | insertText | l | l |
| ▲ | 5 | 7411.00 | ta | keydown | l | KeyL | 229 | false |  |  | l |
| ▲ | 6 | 7539.00 | ta | keyup | l | KeyL | 76 | false |  |  | l |
| ▲ | 7 | 8124.00 | ta | beforeinput |  |  |  | false | insertText | s | l |
|  | 8 | 8127.00 | ta | input |  |  |  | false | insertText | s | ls |
| ▲ | 9 | 8131.00 | ta | keydown | s | KeyS | 229 | false |  |  | ls |
| ▲ | 10 | 8259.00 | ta | keyup | s | KeyS | 83 | false |  |  | ls |
| ▲ | 11 | 9370.00 | ta | keydown |   | Space | 32 | false |  |  | ls |
|  | 12 | 9372.00 | ta | keypress |   | Space | 32 | false |  |  | ls |
|  | 13 | 9373.00 | ta | beforeinput |  |  |  | false | insertText |   | ls |
|  | 14 | 9374.00 | ta | input |  |  |  | false | insertText |   | ls  |
| ▲ | 15 | 9495.00 | ta | keyup |   | Space | 32 | false |  |  | ls  |
| ▲ | 16 | 10017.00 | ta | beforeinput |  |  |  | false | insertText | l | ls  |
|  | 17 | 10023.00 | ta | input |  |  |  | false | insertText | l | ls l |
| ▲ | 18 | 10025.00 | ta | keydown | l | KeyL | 229 | false |  |  | ls l |
| ▲ | 19 | 10163.00 | ta | keyup | l | KeyL | 76 | false |  |  | ls l |
| ▲ | 20 | 10490.00 | ta | beforeinput |  |  |  | false | insertText | a | ls l |
|  | 21 | 10494.00 | ta | input |  |  |  | false | insertText | a | ls la |
| ▲ | 22 | 10499.00 | ta | keydown | a | KeyA | 229 | false |  |  | ls la |
| ▲ | 23 | 10620.00 | ta | keyup | a | KeyA | 65 | false |  |  | ls la |
| ▲ | 24 | 15427.00 | ta | keydown |   | Space | 32 | false |  |  | ls la |
|  | 25 | 15429.00 | ta | keypress |   | Space | 32 | false |  |  | ls la |
|  | 26 | 15431.00 | ta | beforeinput |  |  |  | false | insertText |   | ls la |
|  | 27 | 15433.00 | ta | input |  |  |  | false | insertText |   | ls la  |
| ▲ | 28 | 15544.00 | ta | keyup |   | Space | 32 | false |  |  | ls la  |
| ▲ | 29 | 16823.00 | ce | --- 分段 --- | "" | "" |  |  | "" | "" | "" |
