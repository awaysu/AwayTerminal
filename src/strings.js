// 介面文字（八種語言）與分頁圖示。
//
// 字串本體在 `src/lang/<代碼>.js`（一種語言一個檔）。這裡只做三件事：
//   1. 把八個語言檔合起來；
//   2. 提供 `T['key']`（Proxy，存取的那一刻才挑語言）與 `fmt('key', 參數…)`；
//   3. 退回鏈：**選的語言 → en → zh-TW**，所以少翻一條也不會出現 `undefined`。
//
// key 名稱刻意和舊版 `Localization/Loc.cs` 一樣，對照得起來。
// 要**新增一種語言**：複製 `src/lang/en.js`、翻好、加進下面的 `LANGS` 與 `TABLES`，
// 然後跑 `node scripts/test-i18n.mjs`（會列出漏掉的 key）。
//
// ⚠️ **日期／時間格式不跟著語言變**：log 的時間戳是舊版的相容格式，
// 分頁 tooltip 的「執行 時:分」也照舊版（見 `elapsedText`）。這裡只翻文字。

import zhTW from './lang/zh-TW.js';
import en from './lang/en.js';
import zhCN from './lang/zh-CN.js';
import ja from './lang/ja.js';
import ko from './lang/ko.js';
import es from './lang/es.js';
import de from './lang/de.js';
import fr from './lang/fr.js';

/** 設定視窗下拉的順序＝這個順序（PM 在 TASK-015 修訂版定的）。 */
export const LANGS = [
  { code: 'zh-TW', name: '繁體中文' },
  { code: 'en', name: 'English' },
  { code: 'zh-CN', name: '简体中文' },
  { code: 'ja', name: '日本語' },
  { code: 'ko', name: '한국어' },
  { code: 'es', name: 'Español' },
  { code: 'de', name: 'Deutsch' },
  { code: 'fr', name: 'Français' },
];

const TABLES = {
  'zh-TW': zhTW,
  en,
  'zh-CN': zhCN,
  ja,
  ko,
  es,
  de,
  fr,
};

/** 主語言：key 的來源，也是最後一層退回。 */
export const BASE_LANG = 'zh-TW';
/** 第一層退回（國際上比較可能看得懂的）。 */
export const FALLBACK_LANG = 'en';

let lang = BASE_LANG;

/** 這個代碼是我們支援的語言嗎。 */
export function isLang(code) {
  return Object.prototype.hasOwnProperty.call(TABLES, code);
}

/**
 * 把系統語言（例如 `zh-Hant-TW`、`ja-JP`、`pt-BR`）對到我們的八種。
 *
 * 規則：完全相同 → 中文看書寫系統／地區（`Hant`／TW／HK／MO ＝繁中，其餘中文＝簡中）
 * → 只比前面的語言碼 → 都對不上回 `en`（PM 定的）。
 */
export function matchLang(locale) {
  const raw = String(locale || '').replace('_', '-');
  if (!raw) return FALLBACK_LANG;
  if (isLang(raw)) return raw;
  const lower = raw.toLowerCase();
  const parts = lower.split('-');
  if (parts[0] === 'zh') {
    const hant = parts.some((p) => ['hant', 'tw', 'hk', 'mo'].includes(p));
    return hant ? 'zh-TW' : 'zh-CN';
  }
  const hit = LANGS.find((l) => l.code.toLowerCase() === parts[0]);
  return hit ? hit.code : FALLBACK_LANG;
}

/**
 * 設定語言。
 *
 * - `zh`＝**舊設定檔**的寫法（只有中英兩種的時期）→ 當成 `zh-TW`
 * - 其他認不出來的代碼 → `en`（同舊版 `Loc.SetLang` 的「不認識就用預設」）
 */
export function setLang(code) {
  const c = code === 'zh' ? BASE_LANG : code;
  lang = isLang(c) ? c : FALLBACK_LANG;
}

/** 目前語言。 */
export function getLang() {
  return lang;
}

/** 這個 key 在主語言的表裡嗎。 */
export function hasKey(key) {
  return Object.prototype.hasOwnProperty.call(TABLES[BASE_LANG], key);
}

/** 主語言的所有 key（i18n 測試與稽核用）。 */
export function allKeys() {
  return Object.keys(TABLES[BASE_LANG]);
}

/** 某個語言的表（測試用）。 */
export function tableOf(code) {
  return TABLES[code];
}

/** 退回鏈：選的語言 → en → zh-TW → key 本身。 */
function pick(key) {
  for (const code of [lang, FALLBACK_LANG, BASE_LANG]) {
    const v = TABLES[code] && TABLES[code][key];
    if (typeof v === 'string' && v !== '') return v;
  }
  return key;
}

/**
 * 介面文字。用起來和普通物件一樣（`T['tb.new']`），值是**現在**這個語言的。
 * 查不到的 key 回 key 本身（fail-soft，和 Rust 端一致）。
 */
export const T = new Proxy(
  {},
  {
    get: (_t, key) => (typeof key === 'string' ? pick(key) : undefined),
    has: (_t, key) => typeof key === 'string' && hasKey(key),
    ownKeys: () => allKeys(),
    getOwnPropertyDescriptor: (_t, key) => ({
      value: pick(key),
      enumerable: true,
      configurable: true,
    }),
  },
);

/** `msg.closeTabConfirm` 這類帶 {0} 的字串。 */
export function fmt(key, ...args) {
  return String(T[key] ?? key).replace(/\{(\d+)\}/g, (m, i) => (args[i] ?? m));
}

// 分頁列圖示。
//
// 舊版用 `icon/*.png` 的灰階圖，以 `IconTint` 逐像素染成綠／紅。新版改成 inline SVG、
// 用 `currentColor` 染色——效果一樣（同一組顏色），但不必把二進位圖檔搬進這個 repo，
// 也不用在瀏覽器裡做 canvas 逐像素處理。種類對得起來就好。
const ICONS = {
  powershell:
    '<path d="M3 4h18a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zm0 2v12h18V6H3zm3.7 2.3 4 3a.9.9 0 0 1 0 1.4l-4 3-1.1-1.4L8.7 12 5.6 9.7l1.1-1.4zM12.5 14H18v1.8h-5.5V14z"/>',
  claude:
    '<path d="M12 2.2 14.4 9l6.8 2.4-6.8 2.4L12 20.6 9.6 13.8 2.8 11.4 9.6 9 12 2.2z"/>',
  custom:
    '<path d="M8 5v14l11-7L8 5zm-4 0h2v14H4V5z"/>',
  ssh: '<path d="M4 6h16v12H4V6zm2 2v8h12V8H6zm1.5 1.5 3 2.5-3 2.5V9.5zM12 14h5v1.5h-5V14z"/>',
  telnet: '<path d="M4 6h16v12H4V6zm2 2v8h12V8H6zm1.5 1.5 3 2.5-3 2.5V9.5zM12 14h5v1.5h-5V14z"/>',
  com: '<path d="M5 8h14a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-6a2 2 0 0 1 2-2zm1 3v4h2v-4H6zm4 0v4h2v-4h-2zm4 0v4h2v-4h-2z"/>',
  adb: '<path d="M7 9h10v7a3 3 0 0 1-3 3h-4a3 3 0 0 1-3-3V9zm1.6-4.6 1.2 2M15.4 4.4l-1.2 2M4 11h2v5H4zm14 0h2v5h-2z"/>',
};

/** 連線種類 → inline SVG（`fill: currentColor`，顏色由 CSS 的狀態 class 決定）。 */
export function iconSvg(kind) {
  const path = ICONS[kind] || ICONS.custom;
  return `<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">${path}</svg>`;
}

/**
 * 執行時長：格式「日:時:分」（日不補零、時分兩位）。
 * 逐字照抄舊版 `TerminalTab.ElapsedText`——未來時間／時鐘倒退視為 0。
 */
export function elapsedText(startedAtMs) {
  let mins = Math.floor((Date.now() - startedAtMs) / 60000);
  if (!(mins >= 0)) mins = 0;
  const pad = (n) => String(n).padStart(2, '0');
  return `${Math.floor(mins / 1440)}:${pad(Math.floor(mins / 60) % 24)}:${pad(mins % 60)}`;
}
