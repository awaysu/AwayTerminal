// 執行時偵測 webview 引擎，決定要掛哪一個 IME adapter。
//
// 為什麼要這一層（`CLAUDE.md` 風險 1）：`terminal.js` 的注音／組字調校是針對
// **WebView2（Chromium）** 調的。WKWebView 與 WebKitGTK 的
// `keydown`／`compositionend`／`input` 事件順序不一樣，最可能的差異是：
//
// | 引擎 | Enter 確認組字時 |
// |---|---|
// | Chromium | 先 `keydown`(229, isComposing=true) → 再 `compositionend` |
// | WebKit | 先 `compositionend` → 再送 isComposing=false 的 Enter → **多一個 Enter** |
//
// 解法照 CLAUDE.md 的 (b)：把引擎差異隔離成 adapter，**`terminal.js` 主體不動**，
// 而且**執行時偵測引擎、不依 OS**（mac 上也可能有人跑 Chromium 系的 webview，
// 而 Linux 的 WebKitGTK 與 mac 的 WKWebView 雖然都是 WebKit，版本差很多）。
//
// ⚠️ 目前 `ime-webkit.js` 是**空的**：差異表要等真機錄影（`public/ime-lab.html`
// 在 WKWebView／WebKitGTK 各跑一次），沒有實測資料就寫修正等於猜。
// 所以現在三個引擎都是 no-op，行為和 TASK-021 之前完全一樣。

/** @typedef {'chromium'|'webkit'|'unknown'} Engine */

/**
 * 這個 webview 是什麼引擎。
 *
 * 判斷順序（**特徵優先於 UA 字串**：UA 可以被改，特徵不會）：
 *   1. `window.chrome` ＋ `CSS.supports('-webkit-app-region', 'drag')` → Chromium
 *      （WebView2／Electron／Chrome 都有 `window.chrome`）
 *   2. UA 含 `Edg/`／`Chrome/` → Chromium（WebView2 的 UA 有 `Edg/`）
 *   3. `window.webkit` 或 UA 含 `AppleWebKit` 但沒有 `Chrome/` → WebKit
 *      （Safari／WKWebView／WebKitGTK；Chromium 的 UA 也有 `AppleWebKit`，所以要排除）
 *   4. 其他 → unknown（當成 Chromium 處理，因為現在的調校就是為它調的）
 *
 * @returns {Engine}
 */
export function detectEngine() {
  const ua = (typeof navigator !== 'undefined' && navigator.userAgent) || '';
  const hasChromeObj = typeof window !== 'undefined' && !!window.chrome;
  const uaChromium = /\bEdg\/|\bChrome\/|\bChromium\//.test(ua);
  if (hasChromeObj || uaChromium) return 'chromium';

  const hasWebkitObj = typeof window !== 'undefined' && !!window.webkit;
  if (hasWebkitObj || (/AppleWebKit/.test(ua) && !/\bChrome\//.test(ua))) return 'webkit';

  return 'unknown';
}

/**
 * 更細的引擎資訊（診斷與 `docs/IME-LAB.md` 的錄影對照用）。
 *
 * `flavour` 盡量分出 `webview2`／`wkwebview`／`webkitgtk`——WebKitGTK 的 UA 通常含
 * `Linux` 而 WKWebView 含 `Macintosh`，但兩者都只是提示，程式邏輯只依 [`detectEngine`]。
 */
export function engineInfo() {
  const ua = (typeof navigator !== 'undefined' && navigator.userAgent) || '';
  const engine = detectEngine();
  let flavour = engine;
  if (engine === 'chromium' && /\bEdg\//.test(ua)) flavour = 'webview2';
  else if (engine === 'webkit' && /Macintosh|Mac OS X/.test(ua)) flavour = 'wkwebview';
  else if (engine === 'webkit' && /Linux/.test(ua)) flavour = 'webkitgtk';
  return { engine, flavour, userAgent: ua };
}

/**
 * 載入並套用這個引擎的 adapter。
 *
 * adapter 的介面只有一個函式：`apply(term, id, helpers)`，在 `terminal.js` 建好一個
 * pane 之後被呼叫（掛載點在 `main.js` 的 `window.AwayIme`，**不在 `terminal.js` 裡**）。
 *
 * @returns {Promise<{engine: Engine, applied: boolean}>}
 */
export async function loadAdapter() {
  const { engine } = engineInfo();
  try {
    const mod =
      engine === 'webkit'
        ? await import('./ime-webkit.js')
        : await import('./ime-chromium.js');
    return { engine, apply: mod.apply, applied: true };
  } catch (e) {
    console.warn('[AwayTerminal] IME adapter 載入失敗，用預設行為：', e);
    return { engine, apply: () => {}, applied: false };
  }
}
