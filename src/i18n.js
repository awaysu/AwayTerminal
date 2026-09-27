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

import { setLang, getLang } from './strings.js';

/** 已註冊的「重套文字」函式。 */
const hooks = [];

/**
 * 註冊一個「把介面文字重設一次」的函式。**註冊的當下就會呼叫一次**，
 * 所以模組 init 時只要 `onLangChange(applyTexts)`，不用再自己套一次。
 */
export function onLangChange(fn) {
  hooks.push(fn);
  try {
    fn();
  } catch (e) {
    console.error('[i18n] 套用文字失敗', e);
  }
}

/** 切語言：換掉 `strings.js` 的語言，然後叫每個模組重套文字。 */
export function applyLang(code) {
  if (code === getLang()) return;
  setLang(code);
  for (const fn of hooks) {
    try {
      fn();
    } catch (e) {
      console.error('[i18n] 套用文字失敗', e);
    }
  }
}

export { getLang };
