// 介面文字（繁體中文）與分頁圖示。
//
// 中英切換是之後的任務，所以現在只有一份 zh。文字**集中在這裡**，
// 之後加 en 只要把每個值改成 { zh, en } 再由 settings 的 language 挑一個。
// 來源＝舊版 `Localization/Loc.cs` 的同名 key，key 名稱刻意保留，之後對照得起來。

export const T = {
  // 工具列（舊版 tb.*）
  'tb.new': '新分頁',
  'tb.powershell': 'PowerShell',
  'tb.customCmd': '自訂指令…',
  'tb.split': '視窗分割',
  'tb.tabs': '視窗分頁',
  'tb.columns': '視窗分欄',
  // 舊版三個 tip 的文字一樣，照抄
  'tip.viewCycle': '點按循環：分頁 → 分割 → 分欄',
  'tip.tabPanel': '顯示／隱藏分頁列表',
  'tip.tabClose': '關閉',

  // 分頁右鍵選單（舊版 menu.*）
  'menu.rename': '更改名稱',
  'menu.close': '關閉',

  // 分頁 tooltip（舊版 tip.tabElapsed）
  'tip.tabElapsed': '執行',

  // 對話框（舊版 dlg.* / msg.*）
  'dlg.renameTitle': '更改名稱',
  'dlg.renamePrompt': '分頁名稱：',
  'dlg.customTitle': '自訂指令',
  'dlg.customPrompt': '要執行的指令（例：claude、codex、wsl）：',
  'msg.closeTabTitle': '關閉分頁',
  // {0} = 分頁名稱
  'msg.closeTabConfirm': '確定要關閉「{0}」？',
  'dlg.ok': '確定',
  'dlg.cancel': '取消',
  'dlg.yes': '是',
  'dlg.no': '否',

  // 連線種類（舊版 kind.*；後端也會送 kindLabel，這份是前端的後備）
  'kind.powershell': 'PowerShell',
  'kind.claude': 'Claude Code',
  'kind.custom': '自訂連線',

  'msg.connectFail': '連線失敗',
  'msg.noTabs': '沒有分頁。按「新分頁」開一個。',
};

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
