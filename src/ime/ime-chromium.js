// Chromium（WebView2／Electron／Chrome）的 IME adapter。
//
// **刻意是 no-op**：`terminal.js` 裡的注音／組字／貼上調校本來就是為 WebView2 調的
// （舊版在這個引擎上跑了整個 1.x 生命週期），所以這個引擎不需要任何額外修正。
//
// 這個檔案存在的意義是「讓 adapter 層有兩邊」：之後 WebKit 那邊要修的時候，
// 差異會清楚地只出現在 `ime-webkit.js`，不會有人為了 mac 去動 `terminal.js`
// 而弄壞 Windows（那是 `CLAUDE.md` 風險 1 最怕的事）。

/**
 * @param {import('@xterm/xterm').Terminal} _term
 * @param {string} _id pane id
 * @param {object} _helpers `main.js` 提供的工具（目前沒有用到）
 */
export function apply(_term, _id, _helpers) {
  // 什麼都不做＝維持 terminal.js 原本的行為
}
