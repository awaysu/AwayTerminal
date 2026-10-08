// 語言切換的通知中心（搬移舊版 `Loc.Changed` ＋各視窗的 `ApplyTexts()`）。
//
// 舊版的對話框是**每次開啟才建立**的，所以建構子裡讀 `Loc.T` 就自動是最新語言；
// 只有主視窗長駐，因此它訂了 `Loc.Changed` 事件，在事件裡把每個按鈕的文字重設一次。
//
// 新版的對話框是**頁內、一直存在**的 DOM，所以每個模組都要有一個「把文字重設一次」的
// 函式；在這裡集中註冊，切語言時一起呼叫 → **不必重新啟動程式**（同舊版）。
//
// 用法：
// ```js
// import { onLangChange } from './i18n.js';
// onLangChange(applyTexts);   // 註冊時就會先套一次，所以不必自己再呼叫一次
// ```

import { invoke } from '@tauri-apps/api/core';

import { setLang, getLang, T, hasKey } from './strings.js';

/** 已註冊的「重套文字」函式。 */
const hooks = [];

/**
 * 註冊一個「把介面文字重設一次」的函式。**註冊的當下就會呼叫一次**，
 * 所以模組 init 時只要 `onLangChange(applyTexts)`，不用再自己套一次。
 */
/**
 * 跑在 Windows 上嗎（看作業系統，不是 webview 引擎）。本機 shell 的標籤靠它：
 * Windows 開的是 PowerShell；mac／Linux 開的是使用者的 `$SHELL`（`pty/shell.rs` 的 `local_shell`）→ 叫 Terminal。
 */
export function isWindowsOS() {
  const ua = (typeof navigator !== 'undefined' && navigator.userAgent) || '';
  return /Windows/i.test(ua);
}

/** 跑在 macOS 上嗎（同 `isWindowsOS` 看 user agent；WKWebView 的 UA 有 `Macintosh`）。 */
export function isMacOS() {
  const ua = (typeof navigator !== 'undefined' && navigator.userAgent) || '';
  return /Macintosh|Mac OS X/i.test(ua);
}

/** 檔案管理程式的名字：Windows＝檔案總管、mac＝Finder、Linux＝檔案管理員。 */
export function fileManagerKey(winKey, macKey, linuxKey) {
  if (isWindowsOS()) return winKey;
  return isMacOS() ? macKey : linuxKey;
}

/** 本機 shell 相關的字串：Windows 用 `psKey`、其他平台用 `termKey`。 */
export function shellKey(psKey, termKey) {
  return isWindowsOS() ? psKey : termKey;
}

export function onLangChange(fn) {
  hooks.push(fn);
  try {
    fn();
  } catch (e) {
    console.error('[i18n] 套用文字失敗', e);
  }
}

/**
 * Rust 端會用到的 key（第一次問過就記住；它是編譯進去的，不會變）。
 * @type {string[] | null}
 */
let rustKeys = null;

/**
 * 把 Rust 端需要的字串推給後端。
 *
 * **翻譯只有一份**：八種語言都在 `src/lang/*.js`，Rust 端不存八種語言，
 * 只在啟動與切語言時收到「已經是目前語言」的那幾十條
 * （寫進終端機畫面的訊息是背景執行緒產生的，沒辦法回代碼讓前端查表——
 *  理由寫在 `src-tauri/src/i18n.rs` 的最上面）。
 *
 * 失敗不影響使用：Rust 端有內建的繁中／英文後備。
 */
export async function pushToBackend() {
  try {
    if (!rustKeys) rustKeys = await invoke('i18n_keys');
    const strings = {};
    for (const k of rustKeys) {
      // ⚠️ 只推**表裡真的有**的 key：查不到時 `T[k]` 會回 key 本身，
      // 推過去 Rust 就會把 `err.needHost` 這種字直接顯示給使用者。
      // 少推一條 → Rust 用它內建的繁中／英文後備（難看但看得懂）。
      if (hasKey(k)) strings[k] = T[k];
    }
    const n = await invoke('i18n_push', { lang: getLang(), strings });
    return n;
  } catch (e) {
    console.error('[i18n] 推字串給後端失敗（後端會用內建的繁中／英文）', e);
    return 0;
  }
}

/** 切語言：換掉 `strings.js` 的語言、通知後端、然後叫每個模組重套文字。 */
export function applyLang(code) {
  if (code === getLang()) return;
  setLang(code);
  // 後端的訊息（錯誤、終端機畫面上的字）也要跟著換
  pushToBackend();
  for (const fn of hooks) {
    try {
      fn();
    } catch (e) {
      console.error('[i18n] 套用文字失敗', e);
    }
  }
}

export { getLang };
