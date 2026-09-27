// WKWebView（macOS）／WebKitGTK（Linux）的 IME adapter。
//
// ⚠️ **目前是空的，而且是刻意的。**
//
// `CLAUDE.md` 風險 1 列出最可能的差異：Enter 確認組字時 Chromium 先
// `keydown`(229, isComposing=true) 再 `compositionend`，而 WebKit 先 `compositionend`
// 再送一個 isComposing=false 的 Enter → **多送一個 Enter**。解法 (c) 是「`compositionend`
// 之後同一個事件迴圈內的 Enter keydown 吞掉」。
//
// 但那是**推測**，還沒有實測資料。沒有錄影就寫修正有兩個風險：
//   1. 修錯地方（真正的差異可能在 `input(insertText)` 的時機，不是 Enter）；
//   2. 更糟——在 Chromium 上本來好的行為被這個「修正」弄壞，而我們在 Windows 上
//      不會注意到，因為這個檔案在 Chromium 上不會被載入。
//
// # 拿到 mac／Linux 機器的第一天要做的事
//
// 1. 開 `public/ime-lab.html`（IME 事件錄影頁），在 WKWebView／WebKitGTK 各跑一次
//    `docs/IME-LAB.md` 列的九項劇本（注音+Enter、組字中 Backspace、Esc、候選字選字、
//    中英切換、嘸蝦米／倉頡、Claude Code 多行貼上、單行貼上、4KB 貼上）。
// 2. 把錄到的事件序列和 Windows 的基準（`docs/ime-baseline/`）做差異表，寫進
//    `docs/IME-LAB.md`。
// 3. **照差異表**在這裡寫修正，一項差異一個函式、一個註解說明它修的是哪一條。
// 4. 每寫一條就回 Windows 跑一次 `npm run verify` ＋ `docs/MANUAL-TEST-PLAN.md`
//    的 P0-IME 那 12 條，確認沒有影響 Chromium。

/**
 * @param {import('@xterm/xterm').Terminal} _term
 * @param {string} _id pane id
 * @param {object} _helpers `main.js` 提供的工具
 */
export function apply(_term, _id, _helpers) {
  // TODO(mac/Linux 真機)：照 docs/IME-LAB.md 的差異表實作。
  // 現在什麼都不做＝和 Chromium 一樣的行為（也就是今天 Windows 上的行為）。
}
