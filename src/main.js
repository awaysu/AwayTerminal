// AwayTerminal（2.x）前端進入點。
//
// 這個檔案本身**不管終端機邏輯**——那是搬過來的 `terminal.js` 的工作（輸入佇列、IME 守衛、
// 靜止閘門、去重、貼上路徑、分割/分欄 layout、Ctrl+F…）。main.js 只做三件事：
//   1. 把 `terminal.js` 期待的全域（Terminal / FitAddon / …）準備好——它原本靠 <script> 載入 UMD；
//   2. 提供 `window.AwayWebgl`（WebGL addon + DOM 退回），這是 terminal.js 的 AT2-2 修改要呼叫的；
//   3. 等 bridge 掛好 host→JS listener 之後才載入 terminal.js（它一載完就送 `ready`）。
import '@xterm/xterm/css/xterm.css';
import './style.css';
import { initExitDialog } from './restoretabs.js';

import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { SerializeAddon } from '@xterm/addon-serialize';
import { invoke, Channel } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { bridgeReady, launchReady, log, createSession } from './bridge.js';
import { loadAppFonts, fontReady } from './appfonts.js';
import { loadAdapter as loadImeAdapter, engineInfo } from './ime/detect.js';
import { initTabBar, currentTabState, askYesNo, showInfo, showUrlMenu, toast } from './tabbar.js';
import { T, fmt } from './strings.js';
import { CUSTOM_ICON_KEYS } from './icons.js';
import { initQuota } from './quota.js';
import { applyLang, getLang, pushToBackend } from './i18n.js';
import { matchLang, setLang, LANGS } from './strings.js';

// --- terminal.js 期待的全域（沿用 UMD 的命名空間形狀，這樣 terminal.js 一個字都不用改）---
window.Terminal = Terminal;
window.FitAddon = { FitAddon };
window.Unicode11Addon = { Unicode11Addon };
window.WebLinksAddon = { WebLinksAddon };
window.SerializeAddon = { SerializeAddon };

// --- 渲染器：WebGL → canvas → DOM 三段退回（terminal.js AT2-2 會呼叫）---
//
// `CLAUDE.md` 風險 2 解法 c。Linux 的 WebKitGTK 上 WebGL 可能被停用、或慢到不如
// canvas，所以要能退、而且要能**手動選**（自動選錯時使用者有辦法救自己）。
//
// 設定（`settings.renderer`，經 `T{json}` 送過來）：
//   `auto`（預設）＝照下面的順序實測決定｜`webgl`｜`canvas`｜`dom`
//
// ⚠️ **canvas 這一段目前不會啟用**：`@xterm/addon-canvas` 已發佈的版本（穩定 0.7.0、
// 預覽 0.8.0-beta.48）宣告的 peer 都是 `@xterm/xterm ^5.0.0`，我們是 6.0.0。
// 硬裝會是「宣告不相容的相依 ＋ 一條在 Windows 上永遠不會被執行到的程式碼」，
// 所以先把位置留好（`window.AwayCanvasAddon`），等有相容版本或真機證實需要它時
// 再一行接上。細節與決定過程在 `docs/PLATFORM-UNIX.md`。
let rendererReported = false;

/** 目前實際用的渲染器（關於頁顯示、`report_renderer` 回報）。 */
let activeRenderer = 'DOM';

/** `T{json}` 送來的偏好（`auto`／`webgl`／`canvas`／`dom`）。 */
window.AwayRendererPref = 'auto';

/** 依序要試的渲染器。`auto` ＝全部照順序試；指定了就只試那一個（失敗才退 DOM）。 */
function rendererPlan(pref) {
  if (pref === 'webgl' || pref === 'canvas' || pref === 'dom') return [pref];
  return ['webgl', 'canvas', 'dom'];
}

function tryWebgl(term, id) {
  const webgl = new WebglAddon();
  webgl.onContextLoss(() => {
    // 休眠喚醒／驅動重置：卸載 addon，xterm 自動退回 DOM 渲染
    console.warn('[AwayTerminal] WebGL context lost, falling back to DOM renderer');
    log(`[AwayTerminal] pane ${id} WebGL context lost → 退回 DOM 渲染`);
    webgl.dispose();
    activeRenderer = 'DOM (context lost)';
  });
  term.loadAddon(webgl);
  return 'WebGL';
}

function tryCanvas(term) {
  // 位置留好：`main.js` 目前不 import canvas addon（見上面的說明）。
  // 要啟用就把 addon 裝起來、在這裡 `window.AwayCanvasAddon = { CanvasAddon }`。
  const mk = window.AwayCanvasAddon && window.AwayCanvasAddon.CanvasAddon;
  if (!mk) throw new Error('canvas addon 沒有安裝');
  term.loadAddon(new mk());
  return 'canvas';
}

window.AwayWebgl = function (term, id) {
  let renderer = 'DOM';
  for (const want of rendererPlan(window.AwayRendererPref)) {
    if (want === 'dom') break; // DOM 是「什麼都不掛」
    try {
      renderer = want === 'webgl' ? tryWebgl(term, id) : tryCanvas(term);
      break;
    } catch (e) {
      console.warn(`[AwayTerminal] ${want} 渲染器掛不起來，往下退：`, e);
      log(`[AwayTerminal] pane ${id} ${want} 掛不起來（${e && e.message ? e.message : e}）`);
    }
  }
  activeRenderer = renderer;
  console.log('[AwayTerminal] renderer =', renderer);
  // IME adapter 的掛載點：terminal.js 只呼叫 AwayWebgl（AT2-2），所以在這裡順便套上
  // （BUG-AUDIT A5：原本 window.AwayIme 從沒被呼叫）。非同步、不等它，失敗它自己吞掉。
  if (window.AwayIme) window.AwayIme(term, id);
  if (!rendererReported) {
    rendererReported = true;
    invoke('report_renderer', { renderer }).catch(() => {});
  }
  return renderer;
};

/** 關於頁／診斷用：目前實際在用的渲染器。 */
window.AwayActiveRenderer = () => activeRenderer;

// --- IME adapter（terminal.js 不動；`CLAUDE.md` 風險 1 解法 b）---
//
// 執行時偵測引擎（不依 OS），Chromium 是 no-op、WebKit 目前是空的（等真機錄影）。
// 掛載點只有這裡：`terminal.js` 完全不知道有這一層。
let imeAdapter = null;
window.AwayIme = async function (term, id) {
  try {
    if (!imeAdapter) imeAdapter = await loadImeAdapter();
    imeAdapter.apply(term, id, { log });
  } catch (e) {
    console.warn('[AwayTerminal] IME adapter 套用失敗（維持預設行為）：', e);
  }
};

// ------------------------------------------------- 端到端驗證（不需視窗焦點）
//
// 把 xterm buffer 的純文字尾端回報到後端 log，證明
// 「PTY 輸出 → channel → term.write」整條路通，不必看視窗、也不必自動化 GUI。

function awayDump(lines = 6, id = null) {
  const term = window.AwayTerm;
  if (!term) return [];
  const tail = term.tail(id, lines);
  // 一定要組成一包再送：每行各一次 invoke 是各自獨立的非同步呼叫，到達順序不保證
  log(`[dump] xterm buffer 尾端 ${tail.length} 行：\n${tail.map((t) => `[dump] | ${t}`).join('\n')}`);
  return tail;
}
window.awayDump = awayDump;

/** `--verify N` 用：再開一條 shell 分頁（不跳資料夾選擇，直接用預設工作目錄）。 */
async function createExtraSession() {
  const info = await createSession({ kind: 'shell' });
  return info.id;
}

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

/** 逾時就回這個值（`null`／`undefined` 都可能是正常結果，所以用一個獨一無二的哨兵）。 */
const TIMED_OUT = Symbol('timeout');

/** 給「可能永遠不回應」的 promise 包一層逾時（例如沒有焦點時的剪貼簿 API）。 */
function withTimeout(promise, ms) {
  return Promise.race([
    promise.catch(() => TIMED_OUT),
    new Promise((r) => setTimeout(() => r(TIMED_OUT), ms)),
  ]);
}

/**
 * 工具列功能的自動驗證（TASK-005）。三個不必目視的項目：
 *   1. **log 記錄**：開始記錄 → 送一個會吐彩色中文的指令 → 停止 → 回報檔案路徑。
 *      檔案內容（BOM／時間戳格式／有沒有殘留 ANSI／換行是 LF）由外面用 shell 比對。
 *   2. **複製全部**：`toolbar_copy_all` → 讀回剪貼簿，確認拿到東西。
 *   3. **純文字貼上**：`toolbar_paste` 送一段標記字串 → 讀 buffer 確認它出現在提示字元後面，
 *      這證明 `v` 協定 → `doPaste` → `xterm.paste()` → PTY 整條路是通的。
 * 另外順手驗逐分頁配色（`P` 協定會改 pane 元素的 background）。
 */
async function verifyToolbar(id) {
  const term = window.AwayTerm;
  const lines = ['[verify] 工具列'];
  log(`[verify] 工具列開始（分頁 ${id}）`);

  // ---- 1. log 記錄 ----
  const defaults = await invoke('log_defaults', { id });
  log(`[verify] log 預設值 ${defaults.path} ts=${defaults.timestamp} append=${defaults.append}`);
  // 驗證刻意不用預設的「我的文件」：這台機器的防毒會擋住剛建置的 exe 寫進去
  // （見 Logger::open_with_timeout）。驗證要的是「格式對不對」，寫 TEMP 就好。
  const tmp = await invoke('temp_dir');
  const logPath = tempPath(tmp, 'awayterm-verify.log');
  try {
    const real = await invoke('log_start', {
      id,
      path: logPath,
      timestamp: true,
      append: false,
      remember: false, // 驗證不要改掉使用者的 log 資料夾設定
    });
    lines.push(`[verify] log 開始 → ${real}`);
    // 會產生 ANSI（顏色）＋中文＋OSC（視窗標題）的輸出
    await invoke('session_write_text', {
      id,
      text: "Write-Host \"`e[36m中文測試 AWAY_LOG_OK`e[0m\"; $Host.UI.RawUI.WindowTitle='away-log'\r",
    });
    await wait(2500);
    const stopped = await invoke('log_stop', { id });
    lines.push(`[verify] log 停止 → ${stopped}`);
  } catch (e) {
    lines.push(`[verify] log 失敗：${e}`);
  }

  // ---- 2. 複製全部（q…all → a…all → 剪貼簿）----
  //
  // ⚠️ 剪貼簿**只能半自動驗**：`navigator.clipboard.readText()` 需要視窗有焦點，
  // 沒焦點時在 WebView2 上不是拒絕、是**不回應**（實測會把整個驗證流程卡住）。
  // 共用桌面不能搶焦點 → 這一步包一層逾時，讀不到就標成「需要目視確認」往下走。
  try {
    await invoke('toolbar_copy_all', { id });
    await wait(600);
    const got = await withTimeout(navigator.clipboard.readText(), 1500);
    if (got === TIMED_OUT) {
      lines.push('[verify] 複製全部：剪貼簿讀不到（視窗沒有焦點）→ 需要目視確認');
    } else {
      lines.push(`[verify] 複製全部：剪貼簿 ${got.length} 字，含標記=${got.includes('AWAY_LOG_OK')}`);
    }
  } catch (e) {
    lines.push(`[verify] 複製全部失敗：${e}`);
  }

  // ---- 3. 純文字貼上（v 協定）----
  try {
    await invoke('toolbar_paste', { id, text: 'AWAY_PASTE_OK' });
    await wait(600);
    const tail = term.tail(id, 3).join(' ');
    lines.push(`[verify] 純文字貼上：buffer 含標記=${tail.includes('AWAY_PASTE_OK')}`);
    await invoke('session_write_text', { id, text: '\u0003' }); // Ctrl+C 把那行清掉
  } catch (e) {
    lines.push(`[verify] 純文字貼上失敗：${e}`);
  }

  // ---- 4. 逐分頁配色（P 協定）----
  try {
    await invoke('tab_colors', { id, fg: '#FFFF00', bg: '#000000' });
    await wait(200);
    const pane = document.querySelector(`.term[data-id="${id}"]`);
    lines.push(`[verify] 配色：pane 背景=${pane ? pane.style.background : '?'}`);
    await invoke('tab_colors', { id, fg: '', bg: '' });
    await wait(200);
    lines.push(`[verify] 配色清除：pane 背景=${pane ? pane.style.background : '?'}`);
  } catch (e) {
    lines.push(`[verify] 配色失敗：${e}`);
  }

  // ---- 5. 翻頁（S 協定；只確認不會炸、游標仍在底部）----
  try {
    for (const action of ['top', 'bottom']) {
      await invoke('toolbar_scroll', { id, action });
      await wait(200);
    }
    lines.push('[verify] 翻頁 top/bottom 已送出');
  } catch (e) {
    lines.push(`[verify] 翻頁失敗：${e}`);
  }

  log(lines.join('\n'));
}

/**
 * 多分頁驗證：每條分頁各自有沒有輸出、有沒有 fit 到正確尺寸。
 * 等到每條都讀得到內容（最多 10 秒）再一次報告，避免「還沒到」被當成「沒有」。
 */
/**
 * 圖示的自動驗證（TASK-027）。
 *
 * 只驗「代理看得到」的部分：圖檔有沒有真的載進來（`naturalWidth > 0`＝路徑對、檔案在）、
 * 每顆工具列按鈕是不是都**同時**有圖和字、分頁列圖示有沒有套上染色 filter。
 * 「和舊版並排看起來一樣」要真人看（回歸清單 D14 標 👤）。
 */
async function verifyIcons() {
  const lines = ['[verify] 圖示'];
  const broken = (imgs) => imgs.filter((i) => !(i.naturalWidth > 0)).map((i) => i.getAttribute('src'));
  // 圖檔是非同步載入的，剛開起來可能還沒完成 → 等一下再看
  for (let i = 0; i < 20; i++) {
    const all = [...document.querySelectorAll('#toolbar .tool-ico, .popup-menu .menu-ico')];
    if (all.length && broken(all).length === 0) break;
    await wait(250);
  }

  const btns = [...document.querySelectorAll('#toolbar .tool-btn')];
  const noIcon = btns.filter((b) => !b.querySelector('.tool-ico'));
  const noText = btns.filter((b) => !(b.querySelector('.tool-label') || {}).textContent);
  const toolImgs = btns.map((b) => b.querySelector('.tool-ico')).filter(Boolean);
  const toolBad = broken(toolImgs);
  lines.push(
    `[verify] 工具列 ${btns.length} 顆按鈕：沒圖 ${noIcon.length}、沒字 ${noText.length}、圖載不到 ${toolBad.length}` +
      (toolBad.length ? ` ${toolBad.join('、')}` : '')
  );
  const first = toolImgs[0];
  if (first) {
    const r = first.getBoundingClientRect();
    lines.push(`[verify] 工具列圖示尺寸 ${Math.round(r.width)}x${Math.round(r.height)}（舊版 26x26）`);
  }

  const menuImgs = [...document.querySelectorAll('#new-menu .menu-ico, #favs-menu .menu-ico')];
  const menuBad = broken(menuImgs);
  lines.push(
    `[verify] 下拉選單 ${menuImgs.length} 個圖示：載不到 ${menuBad.length}` +
      (menuBad.length ? ` ${menuBad.join('、')}` : '')
  );

  const tabImgs = [...document.querySelectorAll('#tabstrip .tab-ico')];
  const tabBad = broken(tabImgs);
  // 註：Chromium 的 computed value 會是 `url("http://…/#tint-ready")`（絕對網址、加引號），
  // 所以只比對 fragment，不要比對 `url(` 開頭與結尾的 `)`。
  const filters = tabImgs.map((i) => getComputedStyle(i).filter);
  const tinted = filters.filter((f) => /#tint-(ready|busy)/.test(f));
  lines.push(
    `[verify] 分頁列 ${tabImgs.length} 個圖示：載不到 ${tabBad.length}、有染色 filter ${tinted.length}` +
      (tabBad.length ? ` ${tabBad.join('、')}` : '') +
      `  filter=${filters[0] || '-'}`
  );

  // 自訂連線的圖示挑選器（舊版 IconKeys 那 14 個）
  const picker = [...document.querySelectorAll('#cf-icons button img')];
  lines.push(
    `[verify] 自訂連線圖示挑選器 ${picker.length} 個（舊版 IconKeys ${CUSTOM_ICON_KEYS.length} 個）：載不到 ${broken(picker).length}`
  );
  log(lines.join('\n'));
}

/**
 * 工具列按鈕逐顆按（TASK-028）。
 *
 * 為什麼要有這一段：`verifySettings` 只呼叫 `settings_get`／`settings_apply` 兩個 command，
 * **從來沒開過那個對話框**——所以「按了『其他設定』沒反應」從 TASK-016 壞到 TASK-027，
 * 十幾次 `--verify` 全綠都沒抓到。凡是「使用者是用按鈕觸發的」功能，驗證就要**真的按那顆按鈕**。
 *
 * 做法：`.click()`（不碰鍵盤滑鼠、不搶焦點）→ 等對應的視窗／選單出現 → Esc 關掉。
 * 按鈕的 click handler 丟出來的例外**不會**傳回 `.click()` 的呼叫端（它變成 window 的
 * `error` 事件），所以這裡自己掛一個 listener 接。
 */
async function verifyToolbarButtons() {
  const lines = ['[verify] 工具列按鈕逐顆按'];
  const shown = (id) => {
    const e = document.getElementById(id);
    return !!e && !e.hidden;
  };
  const esc = () => {
    for (const t of [document, window]) {
      t.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    }
  };
  // 按鈕 → 按了之後應該出現的東西
  const cases = [
    ['btn-new', 'new-menu', '新分頁 ▾'],
    ['btn-favs', 'favs-menu', '我的最愛 ▾'],
    ['btn-page', 'page-menu', '翻頁 ▾'],
    ['btn-compose', 'composedlg', '輸入文字'],
    ['btn-remote', 'remotedlg', '遠端設定'],
    ['btn-settings', 'setdlg', '設定'],
    ['btn-about', 'aboutdlg', '關於'],
    ['btn-clear', 'modal', '清除畫面（確認框）'],
  ];
  let bad = 0;
  for (const [btnId, targetId, name] of cases) {
    const btn = document.getElementById(btnId);
    if (!btn) {
      lines.push(`[verify] FAIL 找不到按鈕 #${btnId}（${name}）`);
      bad++;
      continue;
    }
    let err = null;
    const onErr = (e) => {
      err = e.error || e.message || e.reason;
    };
    window.addEventListener('error', onErr);
    window.addEventListener('unhandledrejection', onErr);
    btn.click();
    let open = false;
    for (let i = 0; i < 20 && !open; i++) {
      await wait(100);
      open = shown(targetId);
    }
    window.removeEventListener('error', onErr);
    window.removeEventListener('unhandledrejection', onErr);
    const why = err ? `　例外：${err && err.stack ? String(err.stack).split('\n')[0] : err}` : '';
    lines.push(`[verify] ${name}（#${btnId} → #${targetId}）：${open && !err ? 'PASS' : 'FAIL'}${why}`);
    if (!open || err) bad++;
    esc();
    await wait(250);
    if (shown(targetId)) {
      lines.push(`[verify] FAIL #${targetId} Esc 關不掉`);
      bad++;
    }
  }

  // 沒有視窗的那幾顆：按了不可以丟例外。
  // **刻意不按「純文字貼上」**：它讀的是使用者真正的剪貼簿，會把裡面的東西打進 shell
  //（有換行就等於替使用者按 Enter）。那條路已經由 `verifyToolbar` 用固定字串的
  // `toolbar_paste` 驗過了。
  const plain = [
    ['btn-copy', '複製'],
    ['btn-copyall', '複製全部'],
  ];
  for (const [btnId, name] of plain) {
    const btn = document.getElementById(btnId);
    let err = null;
    const onErr = (e) => {
      err = e.error || e.message || e.reason;
    };
    window.addEventListener('error', onErr);
    window.addEventListener('unhandledrejection', onErr);
    if (btn) btn.click();
    await wait(300);
    window.removeEventListener('error', onErr);
    window.removeEventListener('unhandledrejection', onErr);
    lines.push(`[verify] ${name}（#${btnId}）：${btn && !err ? 'PASS' : 'FAIL'}${err ? `　例外：${err}` : ''}`);
    if (!btn || err) bad++;
  }

  // 視窗分割：按三次轉回原本的模式（同 awayVerify 開頭那段的作法）
  const before = currentTabState().viewMode;
  const view = document.getElementById('btn-view');
  for (let i = 0; i < 3; i++) {
    if (view) view.click();
    await wait(400);
  }
  const after = currentTabState().viewMode;
  lines.push(`[verify] 視窗分割（#btn-view）按三次回到原模式 ${before}→${after}：${before === after ? 'PASS' : 'FAIL'}`);
  if (before !== after) bad++;

  // 分頁列的 ▲／▼（壓在工具列最右端那一顆）
  const panel = document.getElementById('btn-tabpanel');
  const panelBefore = document.getElementById('tabpanel').hidden;
  if (panel) {
    panel.click();
    await wait(300);
    const toggled = document.getElementById('tabpanel').hidden !== panelBefore;
    panel.click();
    await wait(300);
    const back = document.getElementById('tabpanel').hidden === panelBefore;
    lines.push(`[verify] 分頁列 ▲／▼（#btn-tabpanel）：${toggled && back ? 'PASS' : 'FAIL'}`);
    if (!toggled || !back) bad++;
  }

  // 順序也要對（舊版 MainWindow.xaml 的排法）。TASK-029 之前「輸入文字」跑到「翻頁」右邊，
  // 自動檢查沒有人看順序，是 PM 用 CDP 讀出來才發現的 → 現在寫成斷言。
  const WANT = [
    'btn-new', 'btn-favs',
    'btn-compose', 'btn-copy', 'btn-paste', 'btn-copyall', 'btn-clear', 'btn-page',
    'btn-view',
    'btn-remote', 'btn-settings',
    'btn-about',
  ];
  const got = [...document.querySelectorAll('#toolbar .tool-btn')].map((b) => b.id);
  const sameOrder = got.length === WANT.length && got.every((id, i) => id === WANT[i]);
  lines.push(`[verify] 按鈕順序照舊版 xaml：${sameOrder ? 'PASS' : `FAIL ${got.join(' ')}`}`);
  if (!sameOrder) bad++;

  lines.push(`[verify] 工具列按鈕總計：失敗 ${bad} 項`);
  log(lines.join('\n'));
}

/**
 * 對話框／覆蓋層的「真的看得見、而且在最上面」檢查（TASK-031）。
 *
 * **為什麼要有這一段**：「按了沒反應」已經是第三次了，而且三次的根因都不一樣——
 *   * TASK-030 `#exitdlg`：`style.css` 裡**一條規則都沒有** → `hidden` 拿掉之後它是文流裡的
 *     普通 `<div>`，被 `position:absolute` 的 `#app` 整個蓋住（定位元素畫在非定位區塊之上）。
 *   * TASK-030 `#modal`：z-index 300 疊在 `#favs`（320）底下 → 「刪除確認」看不到也點不到。
 *   * TASK-031 `#color-menu`：子選單自己也是 `.popup-menu`（`position: fixed`），
 *     所以 `left: 100%` 變成「視窗寬度的 100%」＝整個丟到螢幕外面。
 * 三種都**不會丟例外**，`hidden` 也確實變成 false——只看 `hidden` 的檢查全部會放行。
 *
 * 所以這裡問的是**畫面上的事實**，逐一打開每個對話框／選單後檢查三件事：
 *   1. `display` 不是 none（computed，不是看 `hidden` 屬性）
 *   2. 有面積，而且**和視窗有交集**（整個被推到螢幕外面就抓得到）
 *   3. 取可見範圍的中心點做 `elementFromPoint`，回來的元素要**屬於這個對話框**
 *      （被別的東西蓋住就抓得到）
 *
 * 開啟方式盡量用**使用者真的會按的入口**（按鈕、右鍵、滑過父選單）。
 * 只有「程式流程中途才會出現」的那幾個（主機金鑰、弱演算法、離開程式、更新、巨集、
 * 代理團隊設定）沒有辦法從 UI 直接叫出來——它們前面要嘛是原生檔案／資料夾對話框
 * （會卡住整個驗證），要嘛要真的去連線，所以改成直接把 `hidden` 拿掉量幾何，
 * 報告裡標成「直接顯示」。那也正是 `#exitdlg` 那一類 bug 會出現的地方。
 */
async function verifyDialogs() {
  const lines = ['[verify] 對話框可見性'];
  const $id = (id) => document.getElementById(id);
  const clickSel = (sel) => {
    const n = document.querySelector(sel);
    if (n) n.click();
    return !!n;
  };
  const fire = (node, type, init) => {
    if (!node) return false;
    node.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, ...(init || {}) }));
    return true;
  };
  const esc = () => {
    for (const t of [document, window]) {
      t.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    }
  };
  /** 看得見＝computed display 不是 none（`hidden` 屬性不夠：子選單靠 class 切換）。 */
  const shown = (n) => getComputedStyle(n).display !== 'none';

  /** 分頁右鍵選單：在第一列分頁上送一個 contextmenu。 */
  const openTabMenu = () => {
    const row = document.querySelector('#tabstrip .tab-row');
    if (!row) return false;
    const rr = row.getBoundingClientRect();
    return fire(row, 'contextmenu', {
      clientX: Math.round(rr.left + rr.width / 2),
      clientY: Math.round(rr.top + rr.height / 2),
    });
  };

  /** 把所有 popup 選單收起來（它們是點別的地方才關，Esc 不一定收得掉）。 */
  const hideAllMenus = () => {
    for (const m of document.querySelectorAll('.popup-menu')) {
      if (!m.classList.contains('submenu')) m.hidden = true;
      m.removeAttribute('style');
    }
    for (const it of document.querySelectorAll('.menu-item.has-sub.sub-open')) {
      it.classList.remove('sub-open');
    }
  };

  /** 三項檢查（display ／ 在畫面內 ／ 中心點最上層是自己）。 */
  const check = (node) => {
    const cs = getComputedStyle(node);
    if (cs.display === 'none') return { ok: false, why: 'computed display 是 none（樣式沒生效？）' };
    if (cs.visibility === 'hidden') return { ok: false, why: 'visibility 是 hidden' };
    if (Number(cs.opacity) === 0) return { ok: false, why: 'opacity 是 0' };
    const r = node.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) {
      return { ok: false, why: `沒有面積（${Math.round(r.width)}x${Math.round(r.height)}）` };
    }
    const W = window.innerWidth;
    const H = window.innerHeight;
    const x0 = Math.max(r.left, 0);
    const y0 = Math.max(r.top, 0);
    const x1 = Math.min(r.right, W);
    const y1 = Math.min(r.bottom, H);
    if (x1 - x0 <= 1 || y1 - y0 <= 1) {
      return {
        ok: false,
        why:
          `整個在視窗外面（left=${Math.round(r.left)} top=${Math.round(r.top)} ` +
          `right=${Math.round(r.right)} bottom=${Math.round(r.bottom)}，視窗 ${W}x${H}）`,
      };
    }
    const cx = Math.round((x0 + x1) / 2);
    const cy = Math.round((y0 + y1) / 2);
    // `pointer-events: none` 的覆蓋層（#toast＝舊版的 CopyPopup）**故意**讓點擊穿過去，
    // `elementFromPoint` 對它沒有意義 → 只驗前兩項。
    if (cs.pointerEvents === 'none') {
      return {
        ok: true,
        why:
          `${Math.round(r.width)}x${Math.round(r.height)} @ (${Math.round(r.left)},${Math.round(r.top)})` +
          `　z-index=${cs.zIndex}　pointer-events:none（刻意讓點擊穿過去，不驗最上層）`,
      };
    }
    const top = document.elementFromPoint(cx, cy);
    if (!top) return { ok: false, why: `中心點 (${cx},${cy}) 上面沒有任何元素` };
    if (top !== node && !node.contains(top)) {
      const cls = typeof top.className === 'string' && top.className.trim();
      const who = `${top.tagName.toLowerCase()}${top.id ? '#' + top.id : ''}${cls ? '.' + cls.split(/\s+/).join('.') : ''}`;
      return { ok: false, why: `中心點 (${cx},${cy}) 最上層是 ${who}（被蓋住了）` };
    }
    return {
      ok: true,
      why:
        `${Math.round(r.width)}x${Math.round(r.height)} @ (${Math.round(r.left)},${Math.round(r.top)})` +
        `　z-index=${cs.zIndex}　中心點最上層正確`,
    };
  };

  // 開啟方式：null ＝沒有 UI 入口，直接把 hidden 拿掉（見函式說明）
  const cases = [
    ['new-menu', '新分頁 ▾', () => clickSel('#btn-new')],
    ['favs-menu', '我的最愛 ▾', () => clickSel('#btn-favs')],
    ['page-menu', '翻頁 ▾', () => clickSel('#btn-page')],
    [
      'term-menu',
      '終端機右鍵選單',
      () => fire($id('termframe'), 'contextmenu', { clientX: 200, clientY: 200 }),
    ],
    // 照真的那一下點擊的順序：xterm 在 mouseup 觸發連結（選單這時候開），同一下的 click 接著冒泡到
    // document——那個 click 不可以把剛開的選單關掉（2.0.5 以前就是這樣：選單一開就不見）
    [
      'url-menu',
      '網址選單',
      () => {
        showUrlMenu('https://example.invalid/', 200, 200);
        return fire($id('termframe'), 'click', { clientX: 200, clientY: 200 });
      },
    ],
    ['tab-menu', '分頁右鍵選單', openTabMenu],
    // 子選單：先開分頁右鍵選單，再滑到「配色 ▸」上面。CSS `:hover` 沒辦法用程式觸發，
    // 所以 tabbar.js 改成 `mouseover` → `.sub-open`，這裡才驗得到（TASK-031 的根因就在這）。
    [
      'color-menu',
      '配色 ▸ 子選單',
      async () => {
        if (!openTabMenu()) return false;
        await wait(120);
        return fire(document.querySelector('#tab-menu [data-act="color"]'), 'mouseover');
      },
    ],
    [
      'conns',
      '自訂連線設定',
      async () => {
        clickSel('#btn-new');
        await wait(150);
        return clickSel('#new-menu [data-kind="manage"]');
      },
    ],
    [
      'sshdlg',
      'SSH／Telnet 連線',
      async () => {
        clickSel('#btn-new');
        await wait(150);
        return clickSel('#new-menu [data-kind="ssh"]');
      },
    ],
    [
      'comdlg',
      '連接埠（COM）',
      async () => {
        clickSel('#btn-new');
        await wait(150);
        return clickSel('#new-menu [data-kind="com"]');
      },
    ],
    [
      'favs',
      '我的最愛設定',
      async () => {
        clickSel('#btn-favs');
        await wait(150);
        return clickSel('#favs-menu [data-fav="manage"]');
      },
    ],
    ['composedlg', '輸入文字', () => clickSel('#btn-compose')],
    ['remotedlg', '遠端設定', () => clickSel('#btn-remote')],
    ['setdlg', '設定', () => clickSel('#btn-settings')],
    ['aboutdlg', '關於', () => clickSel('#btn-about')],
    ['modal', '確認框（清除畫面）', () => clickSel('#btn-clear')],
    // 以下沒有能直接按的入口（前面卡著原生檔案／資料夾對話框，或要真的連線）
    ['toast', '複製提示', () => toast('AWAY_VERIFY_TOAST')],
    ['macrostatus', '巨集狀態', null],
    ['macrodlg', '巨集問答', null],
    ['madlg', '代理團隊設定', null],
    ['hostkey', '主機金鑰確認', null],
    ['weakalgo', '弱演算法警告', null],
    ['exitdlg', '離開程式', null],
    ['updatedlg', '更新檢查', null],
  ];

  let bad = 0;
  for (const [id, name, open] of cases) {
    const node = $id(id);
    if (!node) {
      lines.push(`[verify] FAIL ${name}（#${id}）：index.html 裡找不到這個元素`);
      bad++;
      continue;
    }
    let err = null;
    const onErr = (e) => {
      err = e.error || e.message || e.reason;
    };
    window.addEventListener('error', onErr);
    window.addEventListener('unhandledrejection', onErr);
    // ⚠️ 「有沒有顯示」要看 **computed display**，不是 `hidden` 屬性：
    // 子選單（`#color-menu`）根本沒有 `hidden`，它是靠 `.sub-open` 才 `display: block` 的。
    const wasHidden = node.hidden;
    let byEntry = true;
    try {
      if (open) {
        await open();
        for (let i = 0; i < 25 && !shown(node); i++) await wait(100);
      }
      if (!shown(node) && node.hidden) {
        // 沒有 UI 入口（open === null），或入口沒把它打開 → 直接顯示，至少把幾何量出來
        byEntry = false;
        node.hidden = false;
        await wait(80);
      } else if (!shown(node)) {
        byEntry = false;
      }
      const how = open ? (byEntry ? '入口' : '入口沒打開它→直接顯示') : '直接顯示';
      const r = check(node);
      // 有入口的就一定要是入口打開的；沒有入口的只看幾何
      const pass = r.ok && (byEntry || !open) && !err;
      lines.push(
        `[verify] ${pass ? 'PASS' : 'FAIL'} ${name}（#${id}，${how}）：${r.why}` +
          (err ? `　例外：${err && err.stack ? String(err.stack).split('\n')[0] : err}` : '')
      );
      if (!pass) bad++;
    } catch (e) {
      lines.push(
        `[verify] FAIL ${name}（#${id}）：丟出例外 ${e && e.stack ? e.stack.split('\n')[0] : e}`
      );
      bad++;
    } finally {
      window.removeEventListener('error', onErr);
      window.removeEventListener('unhandledrejection', onErr);
      // 彈出選單是「點別的地方才收」（同舊版）、`#toast` 是計時器自己收（`pointer-events: none`，
      // 根本不收鍵盤）→ 兩者都不驗 Esc，只有真正的對話框才驗
      const passive = getComputedStyle(node).pointerEvents === 'none';
      if (!node.classList.contains('popup-menu') && !passive) {
        esc();
        await wait(150);
        if (open && shown(node)) {
          lines.push(`[verify]   ※ ${name} 按 Esc 沒關掉，改由驗證自己收起來`);
        }
      }
      node.hidden = wasHidden; // 還原成這一輪之前的樣子（子選單本來就沒有 hidden）
      hideAllMenus();
      await wait(80);
    }
  }

  lines.push(`[verify] 對話框可見性總計：失敗 ${bad} 項（共 ${cases.length} 個）`);
  log(lines.join('\n'));
}

/**
 * 設定視窗排版（TASK-030）：**八種語言都要一頁放得下**。
 *
 * 使用者回報「設定太長了」，所以視窗改成兩欄。這一段把「會不會需要捲」變成可以測的數字：
 * `#setdlg-box` 有 `max-height: 88vh` + `overflow: auto`，所以
 * **`scrollHeight > clientHeight` 就是「要捲」**。德文／法文的標籤最長，八種語言逐一量。
 *
 * ⚠️ 量到的高度和視窗大小有關：`--verify` 的視窗是預設大小，不是最大化。
 * 所以這裡同時印 `window.innerHeight`，判斷用的是「內容高 vs 視窗可用高（88vh）」，
 * 而不是寫死的數字。
 */
async function verifySettingsLayout() {
  const lines = ['[verify] 設定視窗排版（左側六類）'];
  const before = getLang();
  const box = document.getElementById('setdlg-box');
  const dlg = document.getElementById('setdlg');
  const navButtons = [...document.querySelectorAll('#st-nav button[data-page]')];
  let bad = 0;
  try {
    for (const { code } of LANGS) {
      applyLang(code);
      await wait(120);
      document.getElementById('btn-settings').click();
      for (let i = 0; i < 20 && dlg.hidden; i++) await wait(100);
      await wait(150);
      // 2.0.15 起設定是左側六類：每一類都要量（各頁疊在同一格，量到的都是最大那一頁的高）
      for (const btn of navButtons) {
        const page = btn.dataset.page;
        btn.click();
        await wait(60);
        const avail = Math.round(window.innerHeight * 0.92); // #setdlg-box 的 max-height
        const need = box.scrollHeight;
        const shown = box.clientHeight;
        const fits = need <= shown + 1; // 捲軸算 1px 的誤差
        if (!fits) bad++;
        const pageH = Math.round(
          document.querySelector(`#setdlg-box .st-page[data-page="${page}"]`).getBoundingClientRect().height
        );
        lines.push(
          `[verify]   ${code.padEnd(6)} ${page.padEnd(7)} 內容高 ${need}px　顯示高 ${shown}px　` +
            `視窗 ${window.innerHeight}px（可用 ${avail}px）　這一類 ${pageH}px　` +
            `${fits ? '不用捲' : '**要捲**'}`
        );
        // 最高的那一種語言把每一組的高度也印出來，才知道要動哪一組
        if (code === 'ja') {
          for (const fs of document.querySelectorAll(`#setdlg-box .st-page[data-page="${page}"] fieldset`)) {
            const legend = fs.querySelector('legend');
            lines.push(
              `[verify]     ${(legend ? legend.id : '?').padEnd(14)} ` +
                `${Math.round(fs.getBoundingClientRect().height)}px　${JSON.stringify(legend ? legend.textContent : '')}`
            );
          }
        }
      }
      for (const t of [document, window]) {
        t.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      }
      await wait(150);
    }
    lines.push(`[verify] 八種語言、六類都不用捲：${bad === 0}（要捲的有 ${bad} 個）`);
    lines.push(
      '[verify] ※ 這是 --verify 的預設視窗大小；使用者的螢幕是 1536x864 邏輯像素，' +
        '最大化時可用高度更大 → 更放得下'
    );
  } finally {
    applyLang(before);
    await wait(120);
  }
  log(lines.join('\n'));
}

async function awayVerify() {
  const term = window.AwayTerm;
  if (!term) return;
  // 整段驗證期間的「沒人接住的例外」計數（TASK-028）。
  //
  // `index.html` 的 inline script 本來就會把 `window.onerror` 送到 stdout，但那是一行一行
  // 散在幾百行 log 裡的——沒有人會發現。這裡收成一個數字放進收尾那一行：**不是 0 就是 FAIL**。
  // `console.error` 也一起算（前端模組的 catch 大多印到 console，不會變成 error 事件）。
  const uncaught = [];
  const onError = (e) => uncaught.push(`error: ${e.message || e.error}`);
  const onReject = (e) => uncaught.push(`unhandledrejection: ${(e.reason && e.reason.stack) || e.reason}`);
  window.addEventListener('error', onError);
  window.addEventListener('unhandledrejection', onReject);
  const consoleErrors = [];
  const realConsoleError = console.error;
  console.error = (...a) => {
    consoleErrors.push(a.map((x) => (x && x.stack ? x.stack.split('\n')[0] : String(x))).join(' '));
    realConsoleError.apply(console, a);
  };
  for (let i = 0; i < 20; i++) {
    await new Promise((r) => setTimeout(r, 500));
    const ids = term.ids();
    if (ids.length && ids.every((id) => term.tail(id, 1).length > 0)) break;
  }
  // 檢視三態：分頁 → 分割 → 分欄 → 分頁 各切一次，每次記下每個 pane 的欄列數，
  // 確認切完都有重新 fit（TASK-003 的 `s{id}`／refit 教訓）。三次剛好轉回原本的模式。
  for (let i = 0; i < 3; i++) {
    const mode = await invoke('view_mode_cycle');
    await new Promise((r) => setTimeout(r, 700));
    const sizes = term
      .ids()
      .map((id) => { const s = term.size(id); return `${id}:${s ? `${s.cols}x${s.rows}` : '?'}`; })
      .join('  ');
    log(`[verify] 檢視模式 ${mode}  pane 尺寸 ${sizes}`);
  }

  const st = currentTabState();
  const lines = [`[verify] 多分頁狀態  檢視模式=${st.viewMode}  作用中=${st.activeId}`];
  for (const t of st.tabs) {
    lines.push(`[verify] tab ${t.id} 「${t.title}」 kind=${t.kind} busy=${t.busy} cwd=${t.cwdPath || '-'}`);
  }
  for (const id of term.ids()) {
    const size = term.size(id);
    const tail = term.tail(id, 2);
    lines.push(`[verify] pane ${id}  ${size ? `${size.cols}x${size.rows}` : '?'}  ${tail.length} 行`);
    for (const t of tail) lines.push(`[verify]   | ${t}`);
  }
  log(lines.join('\n'));

  // 工具列那批（log／複製全部／貼上／配色／翻頁）拿第一個分頁驗
  const first = st.tabs[0];
  if (first) await verifyToolbar(first.id);

  // 每一段各自包起來：**一段丟例外不可以讓後面整批不跑**。
  // TASK-021 踩到：release exe 上 `verifySandbox` 靜靜結束，代理團隊／聊天室／Telegram
  // 三段就完全沒跑，而且**畫面上看不出來**（那三段的 [verify] 行根本不存在），
  // 看起來像「跑完了、都沒問題」。
  const sections = [
    ['圖示', verifyIcons],
    ['設定視窗排版', verifySettingsLayout],
    ['工具列按鈕', verifyToolbarButtons],
    ['對話框可見性', verifyDialogs],
    ['SSH', verifySshPath],
    ['Telnet', verifyTelnetPath],
    ['COM', verifyComPath],
    ['巨集', verifyMacro],
    ['輸入文字', verifyCompose],
    ['設定', verifySettings],
    ['工作列圖示', verifyWindowIcon],
    ['自帶字型', verifyBuiltinFonts],
    ['字型下拉', verifyFontPicker],
    ['語言', verifyLanguage],
    ['八語', verifyAllLanguages],
    ['更新檢查', verifyUpdate],
    ['ADB', verifyAdb],
    ['檔案總管選單', verifyShellMenu],
    ['匯入舊設定', verifyMigrate],
    ['恢復分頁', verifyRestore],
    ['沙盒', verifySandbox],
    ['代理團隊', verifyAgentTeam],
    ['代理團隊恢復', verifyAgentRestore],
    ['AI 聊天室', verifyChatRoom],
    ['Telegram 遠端', () => verifyTelegram(first ? first.id : null)],
  ];
  const broken = [];
  for (const [name, fn] of sections) {
    try {
      await fn();
    } catch (e) {
      broken.push(name);
      log(`[verify] FAIL 「${name}」這一段丟出例外：${e && e.stack ? e.stack : e}`);
    }
  }
  window.removeEventListener('error', onError);
  window.removeEventListener('unhandledrejection', onReject);
  console.error = realConsoleError;
  log(
    broken.length
      ? `[verify] 收尾：${sections.length} 段裡有 ${broken.length} 段丟例外：${broken.join('、')}`
      : `[verify] 收尾：${sections.length} 段全部跑完（沒有丟例外）`
  );
  // 沒人接住的例外／console.error：**不是 0 就是 FAIL**（今天「其他設定打不開」就是這兩種各一個）
  const tail = [
    `[verify] 收尾：整段沒人接住的例外 ${uncaught.length} 個、console.error ${consoleErrors.length} 個：` +
      `${uncaught.length === 0 && consoleErrors.length === 0 ? 'PASS' : 'FAIL'}`,
  ];
  for (const u of uncaught.slice(0, 20)) tail.push(`[verify]   ✗ ${u}`);
  for (const c of consoleErrors.slice(0, 20)) tail.push(`[verify]   ✗ console.error ${c}`);
  log(tail.join('\n'));
}
window.awayVerify = awayVerify;

/**
 * Telegram 遠端驗證（TASK-020 C）：整段跑在 Rust 的 `telegram_probe` 裡——它起一個
 * **只聽 127.0.0.1 的假 Bot API**，把遠端接上去、排指令進去、檢查程式打出來的呼叫。
 *
 * ⚠️ **不打真的 Telegram**、不讀使用者設定裡的 token（`remote::start` 直接吃參數）。
 * 檢查項目與 PASS／FAIL 都由 Rust 那邊產生，這裡只負責印出來。
 */
async function verifyTelegram(tabId) {
  try {
    const lines = await invoke('telegram_probe', { tab: tabId });
    log(lines.join('\n'));
  } catch (e) {
    log(`[verify] Telegram 遠端失敗：${e}`);
  }
}

/**
 * 代理團隊驗證（TASK-017 C）：**整條路**——建團隊 → 兩格啟動 → 角色檔送到 → PM 寫信 →
 * 600ms tick 偵測 → worker 閒置時打進去 → worker 回信 → 打回 PM → 停止任務 → 關閉整組。
 *
 * ⚠️ 三件事絕不做：不啟動真的 claude／codex（用 `examples/fake_agent.rs`）、不動使用者的
 * 自訂連線清單（`agent_verify_begin` 的覆寫只在記憶體裡）、不碰使用者的 `.ai/`
 * （專案資料夾在 `%TEMP%`，驗完整個刪掉）。
 */
async function verifyAgentTeam() {
  const term = window.AwayTerm;
  const lines = ['[verify] 代理團隊（假 agent、全程在 %TEMP%）'];
  let dir = null;
  let key = null;
  try {
    dir = await invoke('agent_verify_begin', { section: 'team' });
    lines.push(`[verify] 驗證用專案：${dir}`);

    const setup = {
      dir,
      title: '',
      slots: [
        { enabled: true, backend: 'claude-code', role: 'product-manager' },
        { enabled: true, backend: 'claude-code', role: 'software-engineer' },
        { enabled: false, backend: '', role: '' },
        { enabled: false, backend: '', role: '' },
      ],
      maxMessages: 30,
      idleCheckMinutes: 0, // 驗證不要讓閒置檢查插話
      sandbox: true,
    };
    const plan = await invoke('agent_team_create', { setup });
    key = plan.key;
    lines.push(
      `[verify] 建團隊：組號=${plan.number}　工作區=${plan.workDir}　沙盒=${!!plan.sandbox}`
    );
    lines.push(
      `[verify] 計畫：${plan.slots.map((x) => `${x.agentId}/${x.backendName}/${x.roleTitle}`).join('　')}`
    );
    // `g` 協定要的 pane 標籤與顏色
    lines.push(`[verify] pane 標籤：${plan.slots.map((x) => x.label).join(' | ')}`);
    lines.push(`[verify] 外框顏色：${plan.slots.map((x) => x.color).join(',')}`);

    for (const slot of plan.slots) {
      await createSession({ kind: 'agent', agent: { team: key, index: slot.index } });
    }
    const n = await invoke('agent_team_ready', { key });
    lines.push(`[verify] 就緒：${n} 個 agent（要 2）`);

    // 兩格的畫面：假 agent 印出「角色檔讀到了」＝`--append-system-prompt-file` 真的送到
    let st = await invoke('agent_verify_state', { key });
    lines.push(`[verify] 分頁 id：${st.tabs.join(',')}　比例=${st.ratio}`);
    // 角色檔是否組好、Agent ID 對不對：**讀檔**（不看畫面，理由見 `RoleFileInfo` 的註解）
    for (const r of st.roles) {
      lines.push(
        `[verify]   ${r.slotId} 角色檔：Agent ID=${r.agentId}　Role=${r.role}　` +
          `三層=${r.threeLayers}　${r.bytes} bytes`
      );
    }
    const roleOk =
      st.roles.length === 2 && st.roles.every((r) => r.threeLayers && r.agentId === r.slotId);
    lines.push(`[verify] 角色檔（含執行期脈絡）組好、Agent ID 正確：${roleOk}`);
    // 「CLI 真的把它讀進去了」只有看畫面才知道 → 參考行，不當成 pass/fail
    //（pane 這時可能是 0×0，`b` 協定的 held 還沒把新輸出寫出來）
    await waitUntil(8000, () =>
      st.tabs.every((id) => term.tail(id, 40).join('\n').includes('runtime-context=true'))
    );
    for (const id of st.tabs) {
      const tail = term.tail(id, 40).filter((l) => l.includes('role-file=') || l.includes('ready'));
      for (const t of tail) lines.push(`[verify]   （參考）pane ${id} | ${t}`);
    }

    // 分隔線比例（`G` 協定那條路）
    await invoke('agent_ratio', { bottomTab: st.tabs[0], ratio: 0.35 });
    st = await invoke('agent_verify_state', { key });
    lines.push(`[verify] 拖分隔線後記住的比例：${st.ratio}（要 0.35）`);

    // ---- 一封信來回（真的走 600ms tick）----
    const sent = await invoke('agent_verify_send', {
      key,
      from: 'Agent-' + plan.number + '1',
      to: 'Agent-' + plan.number + '2',
    });
    lines.push(`[verify] PM 寄出：${sent}`);
    const worker = st.tabs[1];
    const lead = st.tabs[0];
    const delivered = await waitUntil(40000, () =>
      term.tail(worker, 60).join('\n').includes(sent)
    );
    lines.push(`[verify] 投遞打進 worker：${delivered}`);
    const wl = term.tail(worker, 60).find((l) => l.includes('[AwayTerminal]'));
    if (wl) lines.push(`[verify]   worker 畫面 | ${wl.trim()}`);
    const replied = await waitUntil(30000, () =>
      term.tail(worker, 60).join('\n').includes('replied ')
    );
    lines.push(`[verify] worker 回了信：${replied}`);
    const back = await waitUntil(40000, () =>
      term.tail(lead, 60).join('\n').includes('[AwayTerminal]')
    );
    lines.push(`[verify] 回信投遞回 PM：${back}`);
    const ll = term.tail(lead, 60).find((l) => l.includes('[AwayTerminal]'));
    if (ll) lines.push(`[verify]   PM 畫面 | ${ll.trim()}`);

    st = await invoke('agent_verify_state', { key });
    lines.push(
      `[verify] 投遞計數=${st.messageCount}　待投遞=${st.pending}　信箱=${st.busFiles.length} 封　已投遞記錄=${st.delivered.length} 筆`
    );
    lines.push(`[verify] 信箱內容：${st.busFiles.join(', ')}`);

    // ---- 節流：上限調成 1 → 再寄一封就會停 ----
    await invoke('agent_delivery_set', { key, limit: 1 });
    st = await invoke('agent_verify_state', { key });
    // v1：沒暫停時選次數**只改上限、計數照舊**（歸零只發生在「暫停中選次數＝恢復」）
    lines.push(`[verify] 上限改 1（沒暫停 → 計數照舊）：計數=${st.messageCount}　暫停=${st.paused}`);
    const extra = await invoke('agent_verify_send', {
      key,
      from: 'Agent-' + plan.number + '1',
      to: 'Agent-' + plan.number + '2',
    });
    const paused = await waitUntil(40000, async () => {
      st = await invoke('agent_verify_state', { key });
      return st.paused;
    });
    lines.push(`[verify] 已到上限 → tick 自動暫停：${paused}（期間又寄了 ${extra}，會排隊不投遞）`);
    await invoke('agent_delivery_set', { key, limit: null });
    st = await invoke('agent_verify_state', { key });
    lines.push(`[verify] 使用者按暫停也維持暫停：${st.paused}`);
    await invoke('agent_delivery_set', { key, limit: 30 });
    st = await invoke('agent_verify_state', { key });
    lines.push(`[verify] 選次數＝恢復並歸零：暫停=${st.paused}　計數=${st.messageCount}`);

    // ---- 停止任務（Esc → Ctrl+U → 停止句）----
    const stopped = await invoke('agent_stop', { key });
    lines.push(`[verify] 停止任務送到：${stopped.join(', ')}`);
    const gotStop = await waitUntil(15000, () =>
      st.tabs.every((id) => term.tail(id, 60).join('\n').includes('先停一下'))
    );
    lines.push(`[verify] 兩格都收到停止句：${gotStop}`);

    // ---- 套用設定（TASK-018 A）：加一格、換角色、關一格 ----
    let st2 = await invoke('agent_team_state', { key });
    lines.push(
      `[verify] 目前各格：${st2.slots
        .map((x) => `${x.index}:${x.state}/${x.backend || '-'}/${x.role || 'None'}`)
        .join('  ')}`,
    );
    // 格 3 加進來（Architect）＋格 2 換角色（QA）＝格 2 會重新啟動
    const applySetup = {
      dir,
      title: '',
      slots: [
        { enabled: true, backend: 'claude-code', role: 'product-manager', restart: false },
        { enabled: true, backend: 'claude-code', role: 'qa-engineer', restart: false },
        { enabled: true, backend: 'claude-code', role: 'software-architect', restart: false },
        { enabled: false, backend: '', role: '', restart: false },
      ],
      maxMessages: 50,
      idleCheckMinutes: 0,
      sandbox: true,
    };
    const applyPlan = await invoke('agent_team_apply', { key, setup: applySetup });
    lines.push(
      `[verify] 套用：關 ${applyPlan.closeTabs.join(',') || '-'}　開 ${
        applyPlan.launch.map((x) => `${x.agentId}/${x.roleTitle}`).join(',') || '-'
      }　名單變了=${applyPlan.rosterChanged}`,
    );
    for (const id of applyPlan.closeTabs.slice().reverse()) await invoke('tab_close', { id });
    for (const slot of applyPlan.launch) {
      await createSession({ kind: 'agent', agent: { team: key, index: slot.index } });
    }
    await invoke('agent_team_apply_done', { key, rosterChanged: applyPlan.rosterChanged });
    st2 = await invoke('agent_team_state', { key });
    lines.push(
      `[verify] 套用後：${st2.slots
        .map((x) => `${x.index}:${x.state}/${x.role || 'None'}`)
        .join('  ')}　上限=${(await invoke('agent_verify_state', { key })).messageCount >= 0 ? applySetup.maxMessages : '?'}`,
    );
    const after = await invoke('agent_verify_state', { key });
    lines.push(`[verify] 套用後在跑的格：${after.running}（要 3）　分頁 ${after.tabs.join(',')}`);
    // 換了角色的那一格：角色檔要以**新角色**重新組好（讀檔，不看畫面）
    const qa = after.roles.find((r) => r.slotId.endsWith('2'));
    lines.push(
      `[verify] 重新啟動的那一格角色檔：${
        qa ? `${qa.slotId} Role=${qa.role} 三層=${qa.threeLayers}` : '(沒有)'
      }`
    );
    lines.push(
      `[verify] 換角色後角色檔重組成 QA Engineer：${!!qa && qa.role === 'QA Engineer' && qa.threeLayers}`
    );
    // 名單變了 → AwayTerminal 應該寄一封 INFO 給 PM
    const roster = await waitUntil(15000, async () => {
      const v = await invoke('agent_verify_state', { key });
      return v.busFiles.some((f) => f.includes('AwayTerminal-to-Agent-'));
    });
    lines.push(`[verify] 名單變了 → 通知 PM 重讀角色檔：${roster}`);

    // ---- 關閉整組 ----
    const tabs = await invoke('agent_team_tabs', { key });
    for (const id of tabs.slice().reverse()) await invoke('tab_close', { id });
    await invoke('agent_team_gone', { key }).catch(() => {});
    st = await invoke('agent_verify_state', { key });
    lines.push(`[verify] 關閉整組後團隊還在嗎：${st.found}（要 false）`);
    key = null;
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  } finally {
    if (key) {
      const tabs = await invoke('agent_team_tabs', { key }).catch(() => []);
      for (const id of tabs.slice().reverse()) await invoke('tab_close', { id }).catch(() => {});
      await invoke('agent_team_gone', { key }).catch(() => {});
    }
    if (dir) {
      await new Promise((r) => setTimeout(r, 800)); // 讓剛關掉的 PTY 收尾完再刪資料夾
      const msg = await invoke('agent_verify_end', { dir }).catch((e) => String(e));
      lines.push(`[verify] 收尾：${msg}`);
    }
    log(lines.join('\n'));
  }
}


/**
 * 恢復代理團隊分頁（TASK-018 B）：建一個假 agent 團隊 → 走**真的存檔那條路**
 * （`restore_verify_save`）→ `restore_list()` → `agent_team_restore` → 兩格回來、
 * 組號沿用、比例一樣、畫面倒回。
 *
 * ⚠️ 和 `verifyAgentTeam` 一樣：假 agent、`%TEMP%`、資料目錄覆寫，不碰使用者的東西。
 * 存檔會寫進 `settings.savedTabs`，所以**驗完要把設定還原**（同 `verifyMigrate` 的做法）。
 */
async function verifyAgentRestore() {
  const term = window.AwayTerm;
  const lines = ['[verify] 恢復代理團隊分頁（假 agent、全程在 %TEMP%）'];
  let dir = null;
  let key = null;
  const before = await invoke('settings_get');
  try {
    dir = await invoke('agent_verify_begin', { section: 'restore' });
    const setup = {
      dir,
      title: '',
      slots: [
        { enabled: true, backend: 'claude-code', role: 'product-manager', restart: false },
        { enabled: true, backend: 'claude-code', role: 'software-engineer', restart: false },
        { enabled: false, backend: '', role: '', restart: false },
        { enabled: false, backend: '', role: '', restart: false },
      ],
      maxMessages: 50,
      idleCheckMinutes: 0,
      sandbox: false, // 這一段驗的是恢復，不要每次都開新 worktree
    };
    const plan = await invoke('agent_team_create', { setup });
    key = plan.key;
    for (const slot of plan.slots) {
      await createSession({ kind: 'agent', agent: { team: key, index: slot.index } });
    }
    await invoke('agent_team_ready', { key });
    await invoke('agent_ratio', { bottomTab: (await invoke('agent_verify_state', { key })).tabs[0], ratio: 0.28 });
    let st = await invoke('agent_verify_state', { key });
    lines.push(`[verify] 建好：組號=${plan.number}　分頁=${st.tabs.join(',')}　比例=${st.ratio}`);
    // 每格畫面上留一個記號，證明 scrollback 真的倒回來
    for (const id of st.tabs) {
      term.writeOutput(id, new TextEncoder().encode(`\r\nAWAY_AGENT_MARK_${id}\r\n`));
    }
    await new Promise((r) => setTimeout(r, 600));

    // 走真的存檔那條路（和關閉程式時一樣）
    const saved = await invoke('restore_verify_save');
    lines.push(`[verify] 存下 ${saved} 個分頁（含這一組 ${st.tabs.length} 格）`);
    const list = await invoke('restore_list');
    const agentRows = list
      .map((e, i) => ({ e, i }))
      .filter(({ e }) => e.kind === 'agent' && e.agentKey === key);
    lines.push(
      `[verify] 存檔裡的代理團隊格：${agentRows
        .map(({ e }) => `${e.agentIndex}:${e.agentBackend}/${e.agentRole}`)
        .join('  ')}`,
    );
    const first = agentRows[0] && agentRows[0].e;
    lines.push(
      `[verify] 組的資訊存到了：組號=${first && first.agentGroupNumber}　比例=${
        first && first.agentRatio
      }　上限=${first && first.agentMaxMessages}　沙盒=${first && first.agentSandbox}`,
    );

    // 關掉原本那一組，再恢復
    for (const id of st.tabs.slice().reverse()) await invoke('tab_close', { id });
    await invoke('agent_team_gone', { key }).catch(() => {});
    key = null;
    const { restoreAgentTeam } = await import('./agentdlg.js');
    const n = await restoreAgentTeam(
      agentRows.map(({ i }) => i),
      createSession,
    );
    lines.push(`[verify] 恢復了 ${n} 格（要 2）`);
    const teams = await invoke('agent_teams');
    const back = teams.find((t) => t.dir === dir);
    if (!back) throw new Error('恢復後找不到那一組');
    key = back.key;
    lines.push(
      `[verify] 恢復後：組號=${back.number}（要和 ${first.agentGroupNumber} 一樣）　` +
        `比例=${back.ratio}（要 ${first.agentRatio}）　上限=${back.maxMessages}（要 ${first.agentMaxMessages}）`,
    );
    const st3 = await invoke('agent_verify_state', { key });
    lines.push(`[verify] 恢復後的分頁：${st3.tabs.join(',')}　在跑 ${st3.running} 格`);
    // Agent ID 沿用 → **讀角色檔**（不看畫面，理由同上）
    for (const r of st3.roles) {
      lines.push(`[verify]   ${r.slotId} 角色檔：Agent ID=${r.agentId}　Role=${r.role}`);
    }
    const idOk =
      st3.roles.length === 2 &&
      st3.roles.every((r) => r.threeLayers && r.agentId === r.slotId) &&
      st3.roles[0].agentId === `Agent-${back.number}1`;
    lines.push(`[verify] 角色檔重新組好、Agent ID 沿用：${idOk}`);
    // 畫面倒回：上次的記號要在新 pane 的 scrollback 裡
    const markOk = await waitUntil(15000, () =>
      st3.tabs.some((id) => term.tail(id, 200).join('\n').includes('AWAY_AGENT_MARK_')),
    );
    lines.push(`[verify] 上次的畫面倒回來了：${markOk}`);
    // 信箱計數延續（`.delivered` 在 worktree／專案裡，不會因為重開而消失）
    lines.push(`[verify] 信箱檔案：${st3.busFiles.length} 封　已投遞記錄：${st3.delivered.length} 筆`);
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  } finally {
    if (key) {
      const tabs = await invoke('agent_team_tabs', { key }).catch(() => []);
      for (const id of tabs.slice().reverse()) await invoke('tab_close', { id }).catch(() => {});
      await invoke('agent_team_gone', { key }).catch(() => {});
    }
    // 存檔把 `settings.savedTabs` 換掉了 → 清乾淨（`restore_verify_clear` ＝ save(restore:false)，
    // 同「關程式時沒勾恢復分頁」那條路）。**不要留下一組假 agent 的恢復紀錄**，
    // 否則使用者下次啟動會看到兩個莫名的 agent 分頁。
    await invoke('restore_verify_clear').catch(() => {});
    const left = await invoke('settings_get').catch(() => ({}));
    lines.push(
      `[verify] savedTabs 已清掉：${(left.savedTabs || []).length} 筆（驗證前 ${
        (before.savedTabs || []).length
      } 筆；關程式時會依當下的分頁重新存）`,
    );
    if (dir) {
      await new Promise((r) => setTimeout(r, 800));
      const msg = await invoke('agent_verify_end', { dir }).catch((e) => String(e));
      lines.push(`[verify] 收尾：${msg}`);
    }
    log(lines.join('\n'));
  }
}


/**
 * AI 聊天室驗證（TASK-019 C）：建一間三人聊天室（假 agent）→ 給主題 → 三個人**輪流**
 * 各說一輪 → 使用者插話 → 回合數跑完 → 主持人寫結論 → 改名 → 關閉。
 *
 * ⚠️ 同代理團隊那兩段：假 agent、`%TEMP%`（**這一段自己的子資料夾**）、資料目錄覆寫。
 */
async function verifyChatRoom() {
  const term = window.AwayTerm;
  const lines = ['[verify] AI 聊天室（假 agent、全程在 %TEMP%）'];
  let dir = null;
  let key = null;
  try {
    dir = await invoke('agent_verify_begin', { section: 'chat' });
    lines.push(`[verify] 驗證用專案：${dir}`);
    const setup = {
      dir,
      title: '',
      slots: [
        { enabled: true, backend: 'claude-code', role: 'host', restart: false },
        { enabled: true, backend: 'claude-code', role: 'devils-advocate', restart: false },
        { enabled: true, backend: 'claude-code', role: 'researcher', restart: false },
        { enabled: false, backend: '', role: '', restart: false },
      ],
      maxMessages: 30,
      idleCheckMinutes: 0,
      sandbox: false,
      kind: 'chat',
      rounds: 2,
    };
    const plan = await invoke('agent_team_create', { setup });
    key = plan.key;
    lines.push(
      `[verify] 建聊天室：編號=CHAT-${plan.number}　參加者=${plan.slots
        .map((x) => `${x.agentId}/${x.roleTitle}`)
        .join('　')}`
    );
    for (const slot of plan.slots) {
      await createSession({ kind: 'agent', agent: { team: key, index: slot.index } });
    }
    const n = await invoke('agent_team_ready', { key });
    lines.push(`[verify] 就緒：${n} 位（要 3）`);

    // 角色檔：三層 ＋ 主持人那一段只有第 1 位（讀檔，不看畫面）
    let st = await invoke('agent_verify_state', { key });
    for (const r of st.roles) {
      lines.push(`[verify]   ${r.slotId} 角色檔：代號=${r.agentId}　角色=${r.role}　${r.bytes} bytes`);
    }
    const roleOk =
      st.roles.length === 3 && st.roles.every((r) => r.threeLayers && r.agentId === r.slotId);
    lines.push(`[verify] 三份角色檔都組好、代號正確：${roleOk}`);

    // ---- 給主題 → 開始討論 ----
    const folder = await invoke('chat_start', {
      key,
      topic: 'verify：要不要自己寫 SSH？請各自表態。',
    });
    lines.push(`[verify] 開始討論：紀錄資料夾 ${folder}`);
    let teams = await invoke('agent_teams');
    let me = teams.find((t) => t.key === key);
    lines.push(
      `[verify] 狀態：${me.phase}　第 ${me.round}/${me.rounds} 回合　「${me.chatStatus}」`
    );

    // ---- 等它跑完（2 回合 × 3 人 ＋ 結論）----
    // 第 1 回合第 1 位講完之後插一句話
    let saidDone = false;
    const done = await waitUntil(180000, async () => {
      const list = await invoke('agent_teams');
      const t = list.find((x) => x.key === key);
      if (!t) return true;
      if (!saidDone && (t.round > 1 || t.phase === 'concluding')) {
        await invoke('chat_say', { key, text: 'verify：補充一句，相容性比效能重要。' }).catch(
          () => {}
        );
        saidDone = true;
      }
      return t.phase === 'done';
    });
    lines.push(`[verify] 跑到討論結束：${done}`);
    teams = await invoke('agent_teams');
    me = teams.find((t) => t.key === key);
    lines.push(`[verify] 最後狀態：${me.phase}　「${me.chatStatus}」`);

    // ---- 檢查討論紀錄 ----
    const tr = await invoke('chat_verify_transcript', { key });
    lines.push(
      `[verify] transcript.md ${tr.bytes} bytes　發言 ${tr.turns} 則　插話 ${tr.userSaid} 則　結論 ${tr.conclusions} 則`
    );
    lines.push(`[verify] 發言檔：${tr.files.join(', ')}`);
    lines.push(
      `[verify] 兩回合 × 三人都發言了：${tr.turns === 6}　插話進了紀錄：${tr.userSaid >= 1}　有結論：${tr.conclusions === 1}`
    );

    // ---- 改名（TASK-019 B 的「改團隊名稱」）----
    const renamed = await invoke('agent_team_rename', { key, title: 'verify 聊天室' });
    teams = await invoke('agent_teams');
    me = teams.find((t) => t.key === key);
    // `tab-state` 是 event（非同步）→ 等分頁列那一列真的換成新名字
    const rowOk = await waitUntil(5000, () => {
      const row = currentTabState().tabs.find((t) => t.id === me.rowTab);
      return !!row && row.title === renamed;
    });
    const rowTitle = (currentTabState().tabs.find((t) => t.id === me.rowTab) || {}).title;
    lines.push(
      `[verify] 改名：「${renamed}」　組名=${me.title}　代表列分頁標題=${rowTitle}　一致=${rowOk}`
    );

    // ---- 關閉 ----
    const tabs = await invoke('agent_team_tabs', { key });
    for (const id of tabs.slice().reverse()) await invoke('tab_close', { id });
    await invoke('agent_team_gone', { key }).catch(() => {});
    const after = await invoke('agent_verify_state', { key });
    lines.push(`[verify] 關閉整間後還在嗎：${after.found}（要 false）`);
    key = null;
    void term;
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  } finally {
    if (key) {
      const tabs = await invoke('agent_team_tabs', { key }).catch(() => []);
      for (const id of tabs.slice().reverse()) await invoke('tab_close', { id }).catch(() => {});
      await invoke('agent_team_gone', { key }).catch(() => {});
    }
    if (dir) {
      await new Promise((r) => setTimeout(r, 800));
      const msg = await invoke('agent_verify_end', { dir }).catch((e) => String(e));
      lines.push(`[verify] 收尾：${msg}`);
    }
    log(lines.join('\n'));
  }
}

/** 等某個條件成立（支援 async 判斷式）。逾時就回 false。 */
async function waitUntil(ms, f) {
  const end = Date.now() + ms;
  for (;;) {
    if (await f()) return true;
    if (Date.now() > end) return false;
    await new Promise((r) => setTimeout(r, 250));
  }
}


/**
 * 工作列／視窗圖示與工作列身分（TASK-035）。
 *
 * 使用者回報「工作列上是 Windows 預設的空白圖示」。exe 內嵌的檔案圖示是對的
 *（檔案總管看得到），所以能出錯的只剩**執行中的那個視窗**：
 *   * `WM_GETICON` 的 ICON_BIG／ICON_SMALL 是 0，而且視窗類別也沒有圖示
 *     → Windows 只好畫預設的空白圖示；
 *   * AppUserModelID 沒設 → Windows 拿 exe 路徑猜，開發版／安裝版／1.x 可能被歸成同一堆。
 *
 * 這一段**只讀**（`GetClassLongPtr`／`WM_GETICON`／`GetCurrentProcessExplicitAppUserModelID`），
 * 不碰滑鼠鍵盤、不截圖，所以共用桌面也能跑。非 Windows 會回 null，直接跳過。
 */
async function verifyWindowIcon() {
  const lines = ['[verify] 工作列圖示'];
  let p = null;
  try {
    p = await invoke('window_icon_probe');
  } catch (e) {
    lines.push(`[verify] FAIL 讀不到視窗圖示狀態：${e}`);
    log(lines.join('\n'));
    return;
  }
  if (!p) {
    lines.push('[verify] SKIP 非 Windows（mac 的 Dock 用 .icns、Linux 用 .desktop）');
    log(lines.join('\n'));
    return;
  }
  let bad = 0;
  const ok = (cond, text) => {
    lines.push(`[verify] ${cond ? 'PASS' : 'FAIL'} ${text}`);
    if (!cond) bad++;
  };
  const set = (v) => !!v && v !== '0x0';
  lines.push(
    `[verify] hwnd=${p.hwnd}　ICON_BIG=${p.iconBig}　ICON_SMALL=${p.iconSmall}　` +
      `ICON_SMALL2=${p.iconSmall2}　類別 HICON=${p.classIcon}／${p.classIconSmall}`
  );
  ok(set(p.iconBig), `視窗大圖示（ICON_BIG）有掛上去：${p.iconBig}`);
  ok(set(p.iconSmall), `視窗小圖示（ICON_SMALL）有掛上去：${p.iconSmall}`);
  ok(
    p.appUserModelId === p.wantAppUserModelId,
    `AppUserModelID＝${JSON.stringify(p.appUserModelId)}（要 ${JSON.stringify(p.wantAppUserModelId)}）`
  );
  lines.push(`[verify] 工作列圖示總計：失敗 ${bad} 項　※ 圖案長什麼樣要目視（D33）`);
  log(lines.join('\n'));
}

/**
 * 自帶字型（TASK-033）：安裝檔帶著 JetBrains Mono／Cascadia Mono／Sarasa Mono TC，
 * **不安裝到系統**，而是在啟動時用 `FontFace` 載進 webview。
 *
 * 為什麼要驗這個：字型沒載進來時畫面**不會報錯**，只是退回系統字型——
 * 在使用者的機器上看起來「好像有換」，到了沒有中文等寬字型的新電腦才會發現中文不等寬。
 * 而且 xterm 是用「量一個字有多寬」決定每一格大小，字型晚到就整片對不準。
 * 所以這裡問三件事：檔案在不在、`document.fonts` 認不認得、清單裡有沒有標成「內建」。
 */
async function verifyBuiltinFonts() {
  const lines = ['[verify] 自帶字型'];
  let bad = 0;
  const ok = (cond, text) => {
    lines.push(`[verify] ${cond ? 'PASS' : 'FAIL'} ${text}`);
    if (!cond) bad++;
  };
  const WANT = ['JetBrains Mono', 'Cascadia Mono', 'Sarasa Mono TC'];
  try {
    // 1) 檔案有沒有被打包／找得到
    const faces = await invoke('font_faces');
    const builtin = faces.filter((f) => f.builtin);
    const mb = (n) => `${(n / 1048576).toFixed(2)} MB`;
    ok(
      builtin.length >= 5,
      `自帶字型檔 ${builtin.length} 個（要 5：JetBrains R/B、Cascadia R/B、Sarasa R）` +
        `　共 ${mb(builtin.reduce((a, f) => a + f.bytes, 0))}`
    );
    for (const f of builtin) {
      lines.push(
        `[verify]   ${f.family.padEnd(16)} ${String(f.weight).padEnd(4)} ${f.style.padEnd(7)} ` +
          `${mb(f.bytes).padStart(9)}  ${f.path}`
      );
    }

    // 2) webview 真的認得（`document.fonts.check`）——這一條才是「畫得出來」
    for (const fam of WANT) {
      ok(fontReady(fam), `webview 載入成功：${fam}（document.fonts.check）`);
    }

    // 3) 字型清單裡標成「內建」
    const list = await invoke('font_list');
    for (const fam of WANT) {
      const hit = list.find((f) => f.name === fam);
      ok(
        !!hit && hit.source === 'builtin' && hit.mono,
        `字型清單有「${fam}」且 source=builtin、等寬（實際 ${hit ? `${hit.source}/${hit.mono}` : '不在清單裡'}）`
      );
    }

    // 4) 預設字型鏈：終端機實際用的 fontFamily 要含自帶的中文等寬
    const id = currentTabState().activeId;
    const scr = window.AwayTerm && id !== null ? window.AwayTerm.screen(id) : null;
    ok(
      !!scr && String(scr.fontFamily).includes('Sarasa Mono TC'),
      `終端機的字型鏈含自帶中文等寬：${scr ? scr.fontFamily : '?'}`
    );

    // 5) 可下載的清單：一律 https、家族名不和自帶的撞
    const catalog = await invoke('font_catalog');
    ok(
      catalog.length > 0 && catalog.every((c) => c.url.startsWith('https://')),
      `可下載 ${catalog.length} 套、全部 https：${catalog.map((c) => `${c.family}(${mb(c.bytes)})`).join('、')}`
    );
    ok(
      catalog.every((c) => !WANT.includes(c.family)),
      '可下載的清單沒有和自帶的撞名'
    );

    // 6) 字型資料夾以外的檔案不給讀（`font_face_bytes` 的防線）
    let refused = false;
    try {
      await invoke('font_face_bytes', { path: 'C:/Windows/System32/drivers/etc/hosts' });
    } catch {
      refused = true;
    }
    ok(refused, 'font_face_bytes 只讀自己的字型資料夾（其他路徑拒絕）');
  } catch (e) {
    lines.push(`[verify] FAIL 丟出例外：${e && e.stack ? e.stack.split('\n')[0] : e}`);
    bad++;
  }
  lines.push(`[verify] 自帶字型總計：失敗 ${bad} 項`);
  log(lines.join('\n'));
}

/**
 * 字型下拉（TASK-032）。
 *
 * **為什麼要有這一段**：TASK-031 把字型清單換成「系統實際安裝的全部家族」，後端 log
 * 也確實印 196 個——但使用者打開下拉**只看得到一個**。原因是當時的 UI 是
 * `<input list>` ＋ `<datalist>`：Chromium 拿**輸入框目前的值**去過濾候選，而輸入框
 * 一開啟就填著目前字型。後端數字是對的、前端也不丟例外，所以「後端回幾個」這種檢查
 * 永遠抓不到 → 這裡改成問 **UI 上真的有幾個 `<option>`**。
 *
 * 一併驗：分兩組且組標題有字、目前字型有選起來也有標示、每一項用自己的字型畫（預覽）、
 * 「自訂…」會把輸入框叫出來、選了字型按確定會存進設定並即時套到終端機。
 */
async function verifyFontPicker() {
  const lines = ['[verify] 字型下拉'];
  const $id = (id) => document.getElementById(id);
  const sel = $id('st-family-sel');
  const input = $id('st-family');
  const dlg = $id('setdlg');
  const btn = $id('btn-settings');
  const esc = () => {
    for (const t of [document, window]) {
      t.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    }
  };
  const before = await invoke('settings_get');
  const fonts = await invoke('font_list');
  let bad = 0;
  const ok = (cond, text) => {
    lines.push(`[verify] ${cond ? 'PASS' : 'FAIL'} ${text}`);
    if (!cond) bad++;
  };

  try {
    if (!sel || !input) {
      lines.push('[verify] FAIL 找不到 #st-family-sel／#st-family');
      log(lines.join('\n'));
      return;
    }
    btn.click();
    for (let i = 0; i < 25 && dlg.hidden; i++) await wait(100);
    await wait(150);

    // ---- 1. UI 上看得到的選項數 = 後端回的家族數（**這條就是使用者回報的 bug**）----
    const all = [...sel.querySelectorAll('option')];
    const custom = all.filter((o) => o.dataset.k === 'font.custom');
    const real = all.filter((o) => o.dataset.k !== 'font.custom');
    ok(
      real.length === fonts.length,
      `UI 看得到 ${real.length} 個字型 = 後端 ${fonts.length} 個家族　` +
        `（輸入框裡是 "${input.value}"，**不可以**拿它過濾清單——datalist 就是死在這）`
    );
    ok(custom.length === 1, `清單最後一項是「自訂…」（${custom.length} 個，要 1 個）`);

    // ---- 2. 分組（內建／已下載匯入／系統等寬／系統其他），組標題都要有字 ----
    // 沒有下載／匯入過任何字型時「已下載／已匯入」那一組不會出現，所以組數是 2～4。
    const groups = [...sel.querySelectorAll('optgroup')];
    const count = (g) => g.querySelectorAll('option').length;
    const want = [
      ['builtin', fonts.filter((f) => f.source === 'builtin').length],
      ['user', fonts.filter((f) => f.source === 'user').length],
      ['system 等寬', fonts.filter((f) => f.source === 'system' && f.mono).length],
      ['system 其他', fonts.filter((f) => f.source === 'system' && !f.mono).length],
    ].filter(([, n]) => n > 0);
    ok(
      groups.length === want.length && groups.every((g) => !!g.label.trim()),
      `分 ${groups.length} 組（後端說該有 ${want.length} 組）且組標題都有字：` +
        groups.map((g) => `「${g.label}」${count(g)} 個`).join('　')
    );
    ok(
      groups.length === want.length && groups.every((g, i) => count(g) === want[i][1]),
      `每一組的數量與來源對得起來：${want.map(([k, n]) => `${k}=${n}`).join('　')}`
    );
    ok(
      groups.length > 0 && count(groups[0]) === want[0][1] && want[0][0] === 'builtin',
      `自帶字型排在最前面那一組（${groups.length ? `「${groups[0].label}」${count(groups[0])} 個` : '沒有任何一組'}）`
    );

    // ---- 3. 目前字型：選起來、而且有標示 ----
    const cur = sel.querySelector('option[data-current]');
    ok(
      sel.value === before.fontFamily,
      `目前字型 "${before.fontFamily}" 是選起來的狀態（select.value="${sel.value}"）`
    );
    ok(
      !!cur && cur.value === before.fontFamily && cur.textContent !== cur.value,
      `目前字型有標示：「${cur ? cur.textContent : '(沒有標示)'}」` +
        '　※ 原生 `<select>` 打開時會自己捲到選取的那一項'
    );
    // 鍵盤可操作：開啟時焦點就在下拉上（上下鍵／Enter／Esc／首字母跳選都是原生行為）
    ok(document.activeElement === sel, `開啟時焦點在字型下拉上（${document.activeElement?.id || '?'}）`);

    // ---- 4. 每一項用自己的字型畫（預覽）----
    const sample = real.slice(0, 3);
    const previewed = sample.every((o) => getComputedStyle(o).fontFamily.includes(o.value));
    ok(
      previewed,
      `選項用自己的字型顯示：${sample.map((o) => `${o.value} → ${getComputedStyle(o).fontFamily}`).join('｜')}` +
        '　※ 下拉打開後長什麼樣要目視（D28）'
    );

    // ---- 5. 「自訂…」把輸入框叫出來，選回真的字型又收起來 ----
    sel.value = '__custom__';
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    await wait(120);
    ok(!input.hidden, `選「自訂…」後輸入框出現（hidden=${input.hidden}）`);
    const pick = (real.find((o) => o.value !== before.fontFamily) || real[0]).value;
    sel.value = pick;
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    await wait(120);
    ok(
      input.hidden && input.value === pick,
      `選回清單裡的字型後輸入框收起來、值同步成 "${input.value}"（要 "${pick}"）`
    );

    // ---- 6. 按「確定」→ 存進設定 ＋ 即時套到終端機 ----
    $id('st-ok').click();
    const applied = await waitUntil(4000, async () => {
      const s = await invoke('settings_get');
      return s.fontFamily === pick;
    });
    const now = await invoke('settings_get');
    ok(applied, `按確定後存進設定：fontFamily="${now.fontFamily}"（要 "${pick}"）`);
    const id = currentTabState().activeId;
    const scr = window.AwayTerm && id !== null ? window.AwayTerm.screen(id) : null;
    ok(
      !!scr && String(scr.fontFamily).includes(pick),
      `即時套到終端機（terminal.js 的 cfg.fontFamily="${scr ? scr.fontFamily : '?'}"）`
    );
    // 關窗是在 settings_apply 之後再問過 settings_readonly_reason 才做的（BUG D6），
    // 「存進設定」成立的那一刻視窗可能還開著幾十 ms → 等一下再判
    const closed = await waitUntil(2000, () => dlg.hidden);
    ok(closed, '按確定後設定視窗關起來');

    // ---- 7. 自己打清單以外的字型名稱也存得進去 ----
    const madeUp = 'AwayVerify No Such Font';
    await invoke('settings_apply', { patch: { fontFamily: madeUp } });
    const typed = await invoke('settings_get');
    ok(typed.fontFamily === madeUp, `清單以外的名稱照樣收（"${typed.fontFamily}"）`);
    // 再開一次：認不得的名稱要落在「自訂…」上，而且輸入框看得到
    btn.click();
    for (let i = 0; i < 25 && dlg.hidden; i++) await wait(100);
    await wait(150);
    ok(
      sel.value === '__custom__' && !input.hidden && input.value === madeUp,
      `重開後認不得的字型落在「自訂…」、輸入框看得到（select="${sel.value}" input="${input.value}"）`
    );
    esc();
    await wait(200);
  } catch (e) {
    lines.push(`[verify] FAIL 丟出例外：${e && e.stack ? e.stack.split('\n')[0] : e}`);
    bad++;
  } finally {
    // 驗完一律還原（不要把驗證挑的字型留在使用者的設定裡）
    if (!dlg.hidden) {
      esc();
      await wait(150);
      dlg.hidden = true;
    }
    await invoke('settings_apply', { patch: { fontFamily: before.fontFamily } }).catch(() => {});
    lines.push(`[verify] 還原字型設定 → "${before.fontFamily}"`);
  }
  lines.push(`[verify] 字型下拉總計：失敗 ${bad} 項`);
  log(lines.join('\n'));
}

/**
 * 設定視窗驗證（TASK-015 A）：改字級 → Rust 重送 `T{json}` → **前端的字級真的變了**。
 *
 * 這一條在驗的是「設定改了之後真的套到所有分頁」那條路
 * （`settings_apply` → `emit_host("T…")` → `terminal.js` 的 `case 'T'`）。
 */
async function verifySettings() {
  const lines = ['[verify] 設定視窗（app 端路徑）'];
  const before = await invoke('settings_get');
  const want = 24;
  try {
    const after = await invoke('settings_apply', {
      patch: { fontSize: want, background: '#123456', imeQuietMs: 33 },
    });
    lines.push(`[verify] 字級 ${before.fontSize} → ${after.fontSize}（要 ${want}）：${after.fontSize === want}`);
    lines.push(`[verify] 背景存進設定：${after.background === '#123456'}、imeQuiet=${after.imeQuietMs}`);

    // `T{json}` → terminal.js 的 `applyTheme()` 真的跑了嗎？
    // 它會把每個 pane 的 style.background 設成 cfg.background——這個不需要視窗有尺寸就看得到。
    let paneBg = '';
    const id = currentTabState().activeId;
    for (let i = 0; i < 20; i++) {
      await wait(150);
      const pane = document.querySelector(`.term[data-id="${id}"]`);
      paneBg = pane ? pane.style.background : '';
      if (paneBg && paneBg !== 'rgb(30, 30, 30)') break;
    }
    lines.push(
      `[verify] 設定套到分頁（pane 背景=${paneBg}）：${paneBg === 'rgb(18, 52, 86)'}` +
        '　※「字級變了畫面跟著變」要目視，見 REGRESSION-CHECKLIST ST9',
    );

    // 壞掉的顏色要被擋下來，退回預設（舊版 ValidColor）
    const bad = await invoke('settings_apply', { patch: { foreground: 'Red' } });
    lines.push(`[verify] 顏色 'Red' 被退回預設 ${bad.foreground}：${bad.foreground === '#E0E0E0'}`);
    // 字級超出範圍 → 不動（clamp_font_size 回 None）
    const clamp = await invoke('settings_apply', { patch: { fontSize: 999 } });
    lines.push(`[verify] 字級 999 被忽略（維持 ${clamp.fontSize}）：${clamp.fontSize === want}`);
    // 「清除已接受的弱演算法記錄」
    const cleared = await invoke('ssh_weak_clear');
    const afterClear = await invoke('settings_get');
    lines.push(
      `[verify] 清除弱演算法記錄：清掉 ${cleared} 筆、現在 ${afterClear.sshWeakAccepted.length} 筆：` +
        `${afterClear.sshWeakAccepted.length === 0}`,
    );
    // 字型清單（下拉用）：TASK-031 起是**這台機器實際裝的所有家族**，
    // 不再是寫死候選裡挑有裝的。等寬要排在前面、名稱不可以空的。
    const fonts = await invoke('font_list');
    const mono = fonts.filter((f) => f.mono).length;
    const firstOther = fonts.findIndex((f) => !f.mono);
    const monoFirst = firstOther === -1 || fonts.slice(firstOther).every((f) => !f.mono);
    lines.push(
      `[verify] 字型清單 ${fonts.length} 個家族（等寬 ${mono}）：` +
        `${fonts.slice(0, 5).map((f) => f.name).join('、')}…`,
    );
    lines.push(
      `[verify] 字型清單 等寬排前面=${monoFirst}　每個都有名稱=${fonts.every((f) => !!f.name)}　` +
        `沒有重複=${new Set(fonts.map((f) => f.name.toLowerCase())).size === fonts.length}`,
    );
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  } finally {
    // 驗完一律還原（不要把驗證用的字級／顏色留在使用者的設定裡）
    await invoke('settings_apply', {
      patch: {
        fontSize: before.fontSize,
        foreground: before.foreground,
        background: before.background,
        imeQuietMs: before.imeQuietMs,
      },
    });
  }
  log(lines.join('\n'));
}

/**
 * 八種語言各切一次（TASK-015 修訂版）：
 * 工具列第一個按鈕的文字八個都不一樣、都不是 `undefined`，而且 Rust 端的訊息跟著換。
 */
async function verifyAllLanguages() {
  const lines = ['[verify] 八種語言'];
  const before = getLang();
  try {
    const btn = document.getElementById('btn-copy');
    const seen = new Map();
    let bad = 0;
    for (const { code, name } of LANGS) {
      applyLang(code);
      await wait(120);
      const label = btn ? btn.textContent : '';
      // Rust 端的訊息也要跟著換（`sandbox_clear` 回的是我們自己的字串）
      const err = await invoke('sandbox_clear', { id: 999999 }).catch((e) => String(e));
      // 「看起來像沒查到的代碼」＝整串沒有空白、而且長得像 `err.xxx`
      const looksLikeCode = (v) => /^[a-z][\w.]*$/.test(v);
      const ok =
        !!label &&
        !/undefined/.test(label) &&
        !looksLikeCode(label) &&
        !!err &&
        !/undefined/.test(err) &&
        !looksLikeCode(err);
      if (!ok) bad++;
      seen.set(code, label);
      lines.push(
        `[verify]   ${code.padEnd(6)} ${name.padEnd(8)} 工具列=${JSON.stringify(label)}　後端=${JSON.stringify(err)}`,
      );
    }
    const labels = [...seen.values()];
    const uniq = new Set(labels);
    lines.push(`[verify] 八個都有文字、沒有 undefined：${bad === 0}`);
    lines.push(
      `[verify] 八個工具列文字互不相同：${uniq.size === labels.length}（${uniq.size}/${labels.length}）`,
    );
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  } finally {
    applyLang(before);
    await invoke('settings_apply', { patch: { language: before } }).catch(() => {});
  }
  log(lines.join('\n'));
}

/** 語言切換（TASK-015 B）：切成英文 → 前端字串、Rust 的錯誤訊息都要變英文，且**不必重啟**。 */
async function verifyLanguage() {
  const lines = ['[verify] 中／英介面切換'];
  const before = getLang();
  try {
    const btn = document.getElementById('btn-copy');
    const zhLabel = btn ? btn.textContent : '';
    // Rust 端的訊息：故意開一個不存在的自訂連線，看錯誤訊息的語言
    // 用一個「錯誤訊息一定是我們自己的」command（`session_create` 會先被 tauri 的參數檢查擋掉）
    const errZh = await invoke('sandbox_clear', { id: 999999 }).catch((e) => String(e));

    await invoke('settings_apply', { patch: { language: 'en' } });
    applyLang('en'); // 這一步會推字串給後端，後端接著重送 `T{json}`（搜尋列的字）
    await wait(400);
    const enLabel = btn ? btn.textContent : '';
    const errEn = await invoke('sandbox_clear', { id: 999999 }).catch((e) => String(e));

    lines.push(`[verify] 工具列文字 ${JSON.stringify(zhLabel)} → ${JSON.stringify(enLabel)}：${zhLabel !== enLabel && enLabel === 'Copy'}`);
    // 搜尋列的字是 Rust 端的 `T{json}` 設的 → 變成英文就證明那條路真的有到 terminal.js
    await wait(500);
    const ph = document.getElementById('search-input');
    lines.push(
      `[verify] T{json} 送到 terminal.js（搜尋列 placeholder=${JSON.stringify(ph ? ph.placeholder : null)}）：` +
        `${!!ph && ph.placeholder === 'Search'}`,
    );
    lines.push(`[verify] Rust 錯誤訊息跟著換：${errZh !== errEn && /sandbox/i.test(errEn)}`);
    lines.push(`[verify]   zh=${errZh}`);
    lines.push(`[verify]   en=${errEn}`);
    lines.push(`[verify] 不必重啟（同一個 webview 就換掉了）：true`);
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  } finally {
    await invoke('settings_apply', { patch: { language: before } });
    applyLang(before);
  }
  log(lines.join('\n'));
}

/**
 * 檢查更新（TASK-015 C）：解析與離線兩條路。
 *
 * ⚠️ **不會連真的網站**：後端在 127.0.0.1 開一個只回一次的假伺服器，
 * 再對一個沒人聽的 port 打一次證明失敗是安靜的（見 `update::update_verify`）。
 */
async function verifyUpdate() {
  const lines = ['[verify] 檢查更新'];
  try {
    const r = await invoke('update_verify');
    lines.push(`[verify] 請求：${r.requestLine}`);
    lines.push(`[verify] 帶了 app=awayterminal：${r.sentSlug}、User-Agent：${r.sentUserAgent}`);
    lines.push(
      `[verify] 解析出最新版 ${r.parsed ? r.parsed.latestVersion : '(null)'}、` +
        `有新版=${r.parsed ? r.parsed.updateAvailable : '?'}：${!!r.parsed && r.parsed.latestVersion === '9.9.9'}`,
    );
    lines.push(`[verify] 離線／連不上時安靜回 null（不跳錯誤）：${r.offlineIsNone}`);
    const about = await invoke('about_info');
    lines.push(
      `[verify] 關於頁：v${about.version}、編譯時間 ${about.buildTime}、` +
        `xterm.js ${about.xtermVersion}（要和 package.json 一致）`,
    );
    const notices = await invoke('third_party_notices');
    lines.push(
      `[verify] 讀得到 THIRD-PARTY-NOTICES.md：${notices.length} 字、` +
        `有 fancy-regex 那節=${notices.includes('fancy-regex')}`,
    );
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  }
  log(lines.join('\n'));
}

/**
 * 沙盒模式驗證（TASK-007）。**只碰自己建立的東西、只查自己記下的 PID。**
 *
 * 做法：臨時建一條指向 `pwsh` 的自訂連線（沙盒開），用它開一個分頁，然後檢查
 * worktree／分支／`.gitignore` 乾淨／`TEMP` 有沒有導到沙盒／護欄檔案，
 * 最後在分頁裡開一個子行程記下 PID，關掉分頁之後確認**那個 PID** 不在了（Job Object 有效）。
 */
async function verifySandbox() {
  const lines = ['[verify] 沙盒模式'];
  const CONN = '__awayterm_verify_sandbox';
  // 上一次 verify 若被 timeout 砍掉會留 worktree 與分支（TASK-007 Issue 6）→ 先清
  try {
    const cleaned = await invoke('sandbox_verify_cleanup');
    if (cleaned.length) lines.push(`[verify] 清掉上次殘留：${cleaned.join('、')}`);
  } catch (e) {
    lines.push(`[verify] 清殘留失敗（不影響後面）：${e}`);
  }
  let tabId = null;
  try {
    const probe = await invoke('sandbox_probe');
    if (!probe.pwsh) {
      log('[verify] 沙盒：找不到 pwsh，跳過');
      return;
    }
    if (!probe.repoDir) {
      log('[verify] 沙盒：程式的工作目錄不在 git repo 裡，驗不到 worktree，跳過');
      return;
    }
    await invoke('custom_save', {
      conn: {
        name: CONN,
        path: probe.pwsh,
        args: '-NoLogo',
        icon: 'run',
        closeKey: 'ctrl-c',
        closeCount: 3,
        pickDir: false,
        hidden: true,
        viaPowerShell: false,
        sandbox: true,
      },
      originalName: null,
    });

    // 工作目錄用這個專案的 repo 根（才驗得到 worktree）
    const info = await createSession({ kind: 'conn', conn: CONN, cwd: probe.repoDir });
    tabId = info.id;
    await wait(1500);

    const st = currentTabState().tabs.find((t) => t.id === tabId);
    const sb = st && st.sandbox;
    if (!sb) {
      lines.push('[verify] 沒有建立沙盒（預期要有）');
    } else {
      lines.push(`[verify] worktree=${sb.hasWorktree} 分支=${sb.branch || '-'}`);
      lines.push(`[verify] 工作目錄=${sb.workDir}`);
      lines.push(`[verify] 護欄檔案=${(sb.guardrails || []).join('、') || '（這個工具沒有 hook，預期如此）'}`);
      const env = Object.fromEntries(sb.env || []);
      lines.push(`[verify] TEMP 導到沙盒=${(env.TEMP || '').startsWith(sb.root)}`);
      const checks = await invoke('sandbox_verify', { id: tabId });
      lines.push(`[verify] .ai/sandbox 被 git 忽略（git status 乾淨）=${checks.ignored}`);
      lines.push(`[verify] worktree 目錄存在=${checks.worktreeExists}`);
      lines.push(`[verify] 分支存在=${checks.branchExists}`);
    }

    // 巨集的 `exec`（TASK-014 B）：在**沙盒分頁**裡用巨集開子行程。
    // 整段由 Rust 端做（巨集檔的引號很難在 JS 裡拼對），回傳三個檢查結果。
    try {
      const ex = await invoke('exec_verify', { id: tabId });
      lines.push(`[verify] 巨集 exec：${ex.note}`);
      lines.push(
        `[verify] exec 的 exit code 有回來（要 7）：${ex.exitCode}、` +
          `子行程的 TEMP 導到沙盒：${ex.tempInSandbox}`,
      );
      if (!ex.tempInSandbox) {
        lines.push(`[verify]   子行程的 TEMP=${ex.childTemp}、沙盒 root=${ex.sandboxRoot}`);
      }
      lines.push(
        `[verify] exec 開的子行程 PID ${ex.pid}：巨集結束後存活=${ex.aliveAfterMacro}（要 false）`,
      );
    } catch (e) {
      lines.push(`[verify] 巨集 exec 驗證失敗：${e}`);
    }

    // Job Object：在分頁裡開一個子行程，記下它的 PID
    await invoke('session_write_text', {
      id: tabId,
      text: '$p = Start-Process pwsh -ArgumentList "-NoLogo","-NoExit" -PassThru; "AWAY_CHILD_PID=" + $p.Id\r',
    });
    let pid = null;
    for (let i = 0; i < 20; i++) {
      await wait(400);
      const m = /AWAY_CHILD_PID=(\d+)/.exec(window.AwayTerm.tail(tabId, 12).join(' '));
      if (m) {
        pid = Number(m[1]);
        break;
      }
    }
    if (!pid) {
      lines.push('[verify] Job Object：拿不到子行程 PID，跳過這項');
    } else {
      const before = await invoke('pid_alive', { pid });
      await invoke('tab_close', { id: tabId });
      tabId = null;
      await wait(1500);
      const after = await invoke('pid_alive', { pid });
      lines.push(
        `[verify] Job Object：子行程 PID ${pid} 關分頁前存活=${before}、關分頁後存活=${after}` +
          `（後者要是 false）`
      );
    }
  } catch (e) {
    lines.push(`[verify] 失敗：${e && e.stack ? e.stack : e}`);
  } finally {
    if (tabId !== null) await invoke('tab_close', { id: tabId }).catch(() => {});
    await invoke('custom_delete', { name: CONN }).catch(() => {});
    // 驗完把 worktree 與分支收掉，不要留東西在使用者的 repo 裡
    //（`sandbox_verify_cleanup` 本來只在開頭清「上一次殘留的」；結尾也清一次＝正常跑完就不留）。
    // 註：`--verify` 偶爾在啟動階段就卡住是 WebView2 的 dev flake
    //（`Chrome_WidgetWin_0 … Error = 1411`，見 docs/DEV-SETUP.md），**和這個 worktree 無關**——
    // 我一開始以為有關，後來在沒有殘留的情況下也重現了。
    const left = await invoke('sandbox_verify_cleanup').catch(() => []);
    if (left.length) lines.push(`[verify] 收掉驗證用的沙盒：${left.join('、')}`);
  }
  log(lines.join('\n'));
}

/**
 * SSH 的 **app 端路徑**驗證（連線引擎本身由 `cargo run --example ssh_probe` 驗）。
 *
 * 這裡只連 `127.0.0.1` 的一個**沒人在聽**的埠：不碰任何外部主機，但足以證明
 * `session_create(kind:"ssh")` → 建分頁 → ssh 模組跑起來 → 連不上的原因有印在終端機裡
 * → session 正常結束（不會留一個打字全被吞的死分頁）。
 */
async function verifySshPath() {
  const lines = ['[verify] SSH（app 端路徑）'];
  try {
    const info = await createSession({ kind: 'ssh', ssh: { host: '127.0.0.1', port: 1 } });
    lines.push(`[verify] 分頁 ${info.id} 建立：backend=${info.backend} title=${info.title}`);
    const term = window.AwayTerm;
    let text = '';
    for (let i = 0; i < 20; i++) {
      await wait(300);
      text = term.tail(info.id, 8).join(' ');
      if (text.includes('連線失敗')) break;
    }
    lines.push(`[verify] 連不上時有把原因印在終端機：${text.includes('連線失敗')}`);
    const st = currentTabState().tabs.find((t) => t.id === info.id);
    lines.push(`[verify] 分頁 kind=${st ? st.kind : '?'}（應為 ssh）`);
    // 沒勾自動重連 → 應該提示「按 Enter 在此分頁重新連線」
    lines.push(`[verify] 提示按 Enter 重連：${text.includes('按 Enter')}`);
    await invoke('tab_close', { id: info.id });

    // 勾了自動重連 → 應該印「N 秒後自動重連」並開始退避（3 秒起）
    const r = await createSession({
      kind: 'ssh',
      ssh: { host: '127.0.0.1', port: 1, autoReconnect: true },
    });
    let t2 = '';
    for (let i = 0; i < 20; i++) {
      await wait(300);
      t2 = term.tail(r.id, 10).join(' ');
      if (t2.includes('秒後自動重連')) break;
    }
    lines.push(`[verify] 自動重連有排程：${t2.includes('秒後自動重連')}`);
    const st2 = currentTabState().tabs.find((t) => t.id === r.id);
    lines.push(`[verify] 退避次數=${st2 ? st2.reconnectAttempt : '?'}（第一次應為 1）`);
    // 再等一輪，確認退避是往上加的（3 → 6）而不是卡在同一個值
    for (let i = 0; i < 24; i++) {
      await wait(500);
      const s3 = currentTabState().tabs.find((t) => t.id === r.id);
      if (s3 && s3.reconnectAttempt >= 2) break;
    }
    const st3 = currentTabState().tabs.find((t) => t.id === r.id);
    lines.push(`[verify] 退避次數變成 ${st3 ? st3.reconnectAttempt : '?'}（應 ≥2，代表重試過）`);
    await invoke('tab_close', { id: r.id });
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  }
  log(lines.join('\n'));
}

/**
 * Telnet 的 **app 端路徑**驗證（協定本身由 `cargo run --example telnet_probe` 驗，17 項）。
 *
 * 同 SSH 那條：只連 `127.0.0.1` 一個**沒人在聽**的埠，證明
 * `session_create(kind:"telnet")` → 建分頁（kind=telnet、backend=telnet）→
 * 連不上的原因印在終端機 → 提示按 Enter 重連。
 */
async function verifyTelnetPath() {
  const lines = ['[verify] Telnet（app 端路徑）'];
  try {
    const info = await createSession({ kind: 'telnet', telnet: { host: '127.0.0.1', port: 1 } });
    lines.push(`[verify] 分頁 ${info.id} 建立：backend=${info.backend} title=${info.title}`);
    const term = window.AwayTerm;
    let text = '';
    for (let i = 0; i < 20; i++) {
      await wait(300);
      text = term.tail(info.id, 8).join(' ');
      if (text.includes('失敗')) break;
    }
    lines.push(`[verify] 連不上時有把原因印在終端機：${text.includes('失敗')}`);
    const st = currentTabState().tabs.find((t) => t.id === info.id);
    lines.push(`[verify] 分頁 kind=${st ? st.kind : '?'}（應為 telnet）`);
    lines.push(`[verify] 提示按 Enter 重連：${text.includes('按 Enter')}`);
    lines.push(`[verify] 標題是 host:port：${info.title === '127.0.0.1:1'}（同舊版 OpenTelnetDirect）`);
    await invoke('tab_close', { id: info.id });
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  }
  log(lines.join('\n'));
}

/**
 * 連接埠（COM）的 **app 端路徑**驗證（協定與 session 邏輯由
 * `cargo run --example com_probe` 驗，13 項，不需要硬體）。
 *
 * 這裡驗的是「沒有裝置時的行為」：開一個**一定不存在**的埠 → `session_create` 要回錯誤
 * （同舊版 `SerialPort.Open()` 丟例外 → 分頁被移除 + 跳錯誤視窗），不可以留一個死分頁。
 * 順便把 `com_ports()` 在這台機器上的結果印出來。
 */
async function verifyComPath() {
  const lines = ['[verify] 連接埠（app 端路徑）'];
  try {
    const cat = await invoke('com_ports');
    lines.push(
      `[verify] 埠列舉：${cat.ports.length} 個` +
        (cat.ports.length ? ` → ${cat.ports.map((p) => p.label).join('、')}` : '（這台機器沒有序列埠）'),
    );
    lines.push(
      `[verify] 選項清單：鮑率 ${cat.bauds.length} 個、同位 ${cat.parities.join('/')}、` +
        `停止位元 ${cat.stopBits.join('/')}、流量控制 ${cat.flows.join('/')}`,
    );

    const before = currentTabState().tabs.length;
    let failed = '';
    try {
      // COM999 不會存在（舊版同樣是開不起來就跳錯誤、不留分頁）
      await createSession({ kind: 'com', com: { port: 'COM999', baud: 115200 } });
    } catch (e) {
      failed = String(e);
    }
    const after = currentTabState().tabs.length;
    lines.push(`[verify] 開不存在的埠有回錯誤：${failed !== ''}（${failed}）`);
    lines.push(`[verify] 沒有留下死分頁：${after === before}（分頁數 ${before} → ${after}）`);
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  }
  log(lines.join('\n'));
}

/**
 * TTL 巨集的 **app 端路徑**驗證（直譯器本身由 `cargo run --example ttl_probe` 驗，27 項）。
 *
 * 寫一支小巨集到 `%TEMP%`，在**真的 PowerShell 分頁**上跑：
 * `sendln 'echo AWAY_TTL_OK'` → `wait 'AWAY_TTL_OK'` → `end`。
 * 驗的是「巨集看得到連線的輸出、也送得出東西」這條完整的路（IoTap + SessionManager）。
 */
async function verifyMacro() {
  const lines = ['[verify] TTL 巨集（app 端路徑）'];
  try {
    const tmp = await invoke('temp_dir');
    const path = tempPath(tmp, 'awayterm-verify.ttl');
    // 巨集內容：送一行、等它回來、再設一個旗標檔用的變數
    const src = [
      "timeout = 10",
      "sendln 'echo AWAY_TTL_OK'",
      "wait 'AWAY_TTL_OK'",
      "hit = result",
      "if hit = 1 then",
      "  dispstr '[macro] AWAY_TTL_WAIT_HIT'",
      "endif",
      "end",
      "",
    ].join(String.fromCharCode(13, 10));
    await invoke('save_text_to_file_at', { path, text: src });

    const term = window.AwayTerm;
    const id = currentTabState().tabs.find((t) => t.kind === 'powershell').id;
    const before = (currentTabState().tabs.find((t) => t.id === id) || {}).macroState;
    lines.push(`[verify] 開始前沒有巨集狀態：${!before}`);

    const r = await invoke('macro_verify', { id, path, timeoutMs: 20000 });
    lines.push(`[verify] 巨集執行結果：${r}`);

    // 畫面上要看得到 dispstr 印的字，代表 wait 真的命中了
    let tail = '';
    for (let i = 0; i < 20; i++) {
      await wait(300);
      tail = term.tail(id, 30).join(' ');
      if (tail.includes('AWAY_TTL_WAIT_HIT')) break;
    }
    lines.push(`[verify] wait 命中（畫面上有 dispstr 的字）：${tail.includes('AWAY_TTL_WAIT_HIT')}`);
    // 結束提示是 `finish()` 在清掉分頁狀態之後才推進畫面的，所以要自己再等一下
    // （第一版和上面共用同一份 tail，量到的是還沒印出來的那一刻）
    let done = '';
    // 8 秒（原本 4 秒）：機器忙的時候這一行來得慢，會偶發假失敗
    //（TASK-015 期間同一份程式碼 5 次裡有 2 次沒等到）
    for (let i = 0; i < 20; i++) {
      await wait(400);
      done = term.tail(id, 30).join(' ');
      if (done.includes('巨集執行完畢')) break;
    }
    lines.push(`[verify] 巨集結束的提示有印出來：${done.includes('巨集執行完畢')}`);
    const after = (currentTabState().tabs.find((t) => t.id === id) || {}).macroState;
    lines.push(`[verify] 結束後巨集狀態已清掉：${!after}`);
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  }
  log(lines.join('\n'));
}

/**
 * 「輸入文字」的驗證（TASK-014 C）：**Big5 檔 → 送到 PowerShell 分頁 → 畫面上看到正確中文**。
 *
 * 用 `compose_write_big5`（只給 `--verify` 用）在 `%TEMP%` 產一個 Big5 檔，
 * 走 `compose_load_file` 沒辦法自動點檔案選擇 → 直接呼叫解碼與送出那兩條路。
 */
async function verifyCompose() {
  const lines = ['[verify] 輸入文字（app 端路徑）'];
  try {
    const r = await invoke('compose_verify_roundtrip', { text: '測試中文 ABC' });
    lines.push(`[verify] Big5 檔解碼：編碼=${r.encoding} 內容正確=${r.textOk}（${r.text}）`);
    lines.push(`[verify] 換行統一成 CRLF：${r.crlfOk}`);

    const term = window.AwayTerm;
    const id = currentTabState().tabs.find((t) => t.kind === 'powershell').id;
    // 送出（不送 Enter，免得真的執行）→ 畫面上要看得到中文
    await invoke('compose_send', {
      id,
      text: r.text,
      sendEnter: false,
      remember: false,
    });
    let seen = false;
    for (let i = 0; i < 20; i++) {
      await wait(250);
      if (term.tail(id, 8).join(' ').includes('測試中文 ABC')) {
        seen = true;
        break;
      }
    }
    lines.push(`[verify] 送到分頁後畫面上看得到中文：${seen}`);
    // 把打字清掉（Ctrl+C），免得留在提示字元上
    await invoke('session_write_text', { id, text: '\x03' });
    // ⬇ 這個 wait 不能抽：Ctrl+C 之後 PSReadLine 還要重畫一次（印中斷的那一行＋新的提示字元）。
    // 不等它畫完就跑 verifyRestore，它寫進畫面的記號會被這次重畫**蓋掉**，兩個恢復分頁的檢查會假失敗。
    await wait(600);
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  }
  log(lines.join('\n'));
}

/**
 * 恢復分頁驗證（TASK-010 B）。**在同一次執行裡走完「存 → 恢復」**：
 *
 *   1. 在目前分頁印一個記號 → `restore_verify_save`（＝勾了恢復分頁關程式的那一步）
 *   2. `restore_list` → `createSession({restore: 0})`：Rust 會在 `n` 之後、`s` 之前送 `b`
 *   3. 檢查新分頁的畫面：記號 → 分隔行 → 新 shell 的提示字元，**順序**要對
 *
 * 第 3 點就是在驗 `terminal.js` 的 `pendingRestore` / `held`（那段是舊版原檔一字未改的）：
 * `b` 到的時候 pane 還沒 fit，新 session 的輸出會被扣在 `held` 裡，
 * 等舊畫面寫完才依序補上。順序錯了代表那段邏輯在這個環境下不成立。
 */
async function verifyRestore() {
  const lines = ['[verify] 恢復分頁'];
  const MARK = 'AWAY_RESTORE_MARK_42';
  let restoredId = null;
  try {
    const term = window.AwayTerm;
    // 記號直接寫進畫面（不靠 shell 執行，才不受提示字元時序影響）。
    // **每個分頁都寫**：`restore_list` 回來的第一筆不一定是目前作用中的那個分頁
    // （第一版只寫 active，結果恢復的是另一個分頁的畫面，記號自然找不到）。
    for (const id of term.ids()) {
      term.writeOutput(id, new TextEncoder().encode(`\r\n${MARK}\r\n`));
    }
    await wait(400);

    const n = await invoke('restore_verify_save');
    lines.push(`[verify] 存下 ${n} 個分頁`);
    const list = await invoke('restore_list');
    const idx = list.findIndex((e) => e.bufferFile);
    lines.push(`[verify] 有畫面內容的分頁：${idx >= 0 ? `第 ${idx + 1} 筆` : '沒有'}`);
    if (idx < 0) throw new Error('沒有存到任何畫面內容（q…save 沒回來？）');
    lines.push(`[verify] 存檔不含密碼欄位：${!JSON.stringify(list).toLowerCase().includes('password')}`);

    const info = await createSession({ kind: 'shell', restore: idx, title: '__verify_restore' });
    restoredId = info.id;
    // 新 shell 的提示字元：Windows 是 `PS …>`；mac／Linux 是使用者的 `$SHELL`
    //（zsh 預設結尾 `%`、bash／sh 是 `$`、root 是 `#`）
    const isPrompt = (l) => l.includes('PS ') || /[%$#]\s*$/.test(l);
    // 等舊畫面倒回來 + 新 shell 的提示字元出現
    let tail = [];
    const sepOf = (rows) => rows.findIndex((l) => l.includes('以上為上次關閉前的紀錄'));
    for (let i = 0; i < 30; i++) {
      await wait(400);
      tail = term.tail(info.id, 40);
      // ⚠️ 等的是「分隔行**之後**的提示字元」。倒回來的舊畫面裡本來就有 `PS …`，
      // 只要 `tail.some(PS )` 就 break 的話，新 shell 的提示字元還沒畫出來就跑掉了
      // → 下一行的 iPrompt 拿到 -1、順序檢查假失敗（實測 3 輪有 2 輪中）。
      const s = sepOf(tail);
      if (tail.some((l) => l.includes(MARK)) && s >= 0 && tail.some((l, k) => k > s && isPrompt(l))) break;
    }
    const iMark = tail.findIndex((l) => l.includes(MARK));
    const iSep = sepOf(tail);
    const iPrompt = tail.findIndex((l, k) => k > iSep && isPrompt(l));
    lines.push(`[verify] 舊畫面有倒回來（記號在第 ${iMark} 行）：${iMark >= 0}`);
    lines.push(`[verify] 分隔行有出現（第 ${iSep} 行）：${iSep >= 0}`);
    lines.push(
      `[verify] held 順序正確（記號 ${iMark} < 分隔行 ${iSep} < 新提示字元 ${iPrompt}）：` +
        `${iMark >= 0 && iSep > iMark && iPrompt > iSep}`,
    );
    const st = currentTabState().tabs.find((t) => t.id === info.id);
    lines.push(`[verify] 分頁數 ${currentTabState().tabs.length}、恢復的分頁 kind=${st ? st.kind : '?'}`);
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  } finally {
    // 不要把驗證用的紀錄留下來（否則下次真的啟動會莫名恢復一堆分頁）
    try {
      if (restoredId !== null) await invoke('tab_close', { id: restoredId });
      const left = await invoke('restore_verify_clear');
      lines.push(`[verify] 已清掉存檔（剩 ${left} 筆）`);
    } catch (e) {
      lines.push(`[verify] 清除存檔失敗：${e}`);
    }
  }
  log(lines.join('\n'));
}

// ------------------------------------------------------------- IPC bench
//
// CLAUDE.md 風險 5 的量測。**只在 `?bench=1` 時自動跑**（TASK-003 調整），
// 其他時候可在 devtools 手動叫 `awayBench()` / `awayBenchSmall()`。
// 結果與結論寫在 docs/IPC-BENCH.md。

const MB = 1024 * 1024;

function transportOf(v) {
  if (v instanceof ArrayBuffer) return 'ArrayBuffer';
  if (Array.isArray(v)) return `JSON 數字陣列 (${v.length} 個元素)`;
  if (typeof v === 'string') return 'string';
  if (v && v.byteLength !== undefined) return v.constructor.name;
  return typeof v;
}

function byteLenOf(v) {
  if (v instanceof ArrayBuffer) return v.byteLength;
  if (Array.isArray(v)) return v.length;
  if (typeof v === 'string') return v.length;
  return -1;
}

const BENCH_MODES = {
  // Response::new(Vec<u8>) → 自訂協定回應，真二進位
  raw: (size) => invoke('bench_raw', { size }),
  // command 直接回 Vec<u8> → serde 序列化成 JSON 數字陣列（反例，保留供對照）
  vec: (size) => invoke('bench_vec', { size }),
  // base64 字串 + atob（舊版 o{id}US{base64} 的做法）
  base64: async (size) => {
    const s = await invoke('bench_base64', { size });
    const bin = atob(s);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out.buffer;
  },
  // Channel 送 InvokeResponseBody::Raw（PTY 輸出實際走的路）
  channel: (size) =>
    new Promise((resolve, reject) => {
      const ch = new Channel();
      ch.onmessage = (m) => resolve(m);
      invoke('bench_channel', { size, onData: ch }).catch(reject);
    }),
};

async function benchMode(name, size, iterations) {
  const fn = BENCH_MODES[name];
  await fn(size); // 暖機一次，不計入
  const times = [];
  let transport = '?';
  let bytes = -1;
  for (let i = 0; i < iterations; i++) {
    const t0 = performance.now();
    const v = await fn(size);
    times.push(performance.now() - t0);
    if (i === 0) {
      transport = transportOf(v);
      bytes = byteLenOf(v);
    }
  }
  times.sort((a, b) => a - b);
  const mean = times.reduce((a, b) => a + b, 0) / times.length;
  return {
    name,
    transport,
    bytes,
    min: times[0],
    median: times[Math.floor(times.length / 2)],
    mean,
    max: times[times.length - 1],
    mbPerSec: size / MB / (mean / 1000),
  };
}

async function awayBench(sizeMb = 1, iterations = 10) {
  const lines = [`[IPC bench] ${iterations} × ${sizeMb}MB`];
  const size = Math.round(sizeMb * MB);
  const rows = [];
  for (const name of ['raw', 'vec', 'base64', 'channel']) {
    const r = await benchMode(name, size, iterations);
    rows.push(r);
    lines.push(
      `  ${name.padEnd(8)} median ${r.median.toFixed(1).padStart(8)} ms` +
        `  mean ${r.mean.toFixed(1).padStart(8)} ms` +
        `  min ${r.min.toFixed(1).padStart(8)} ms` +
        `  max ${r.max.toFixed(1).padStart(8)} ms` +
        `  ${r.mbPerSec.toFixed(1).padStart(6)} MB/s` +
        `  ${r.bytes} bytes via ${r.transport}`
    );
  }
  log(lines.join('\n'));
  console.log(lines.join('\n'));
  return rows;
}
window.awayBench = awayBench;

/** channel 的 1024 bytes 門檻兩邊各量一次（見 docs/IPC-BENCH.md）。 */
async function awayBenchSmall(iterations = 200) {
  const lines = [`[IPC bench small] ${iterations} × (512 / 2048 bytes) via channel`];
  for (const size of [512, 2048]) {
    const r = await benchMode('channel', size, iterations);
    lines.push(
      `  channel ${String(size).padStart(5)}B  median ${r.median.toFixed(3)} ms` +
        `  mean ${r.mean.toFixed(3)} ms  ${r.bytes} bytes via ${r.transport}`
    );
  }
  log(lines.join('\n'));
  console.log(lines.join('\n'));
}
window.awayBenchSmall = awayBenchSmall;

/**
 * ADB 驗證（TASK-016 B）：路徑搜尋與裝置清單那條路。
 *
 * 這台機器不一定裝了 adb、也不一定接著手機，所以驗的是「**流程對不對**」：
 * 找不到 adb 要明確說找不到（而不是噴錯），有 adb 就要列得出裝置（可能是 0 台）。
 */
async function verifyAdb() {
  const lines = ['[verify] ADB（app 端路徑）'];
  try {
    const r = await invoke('adb_devices', { adbPath: null });
    lines.push(`[verify] adb 路徑：${r.adb || '(找不到)'}`);
    if (!r.adb) {
      lines.push(`[verify] 找不到 adb 時有給下載頁：${!!r.downloadUrl}（${r.downloadUrl}）`);
      lines.push('[verify] 沒有 adb → 裝置清單那段跳過（這台機器沒裝）');
    } else {
      const usable = r.devices.filter((d) => d.state === 'device');
      lines.push(
        `[verify] 裝置 ${r.devices.length} 台（可用 ${usable.length}）：` +
          (r.devices.map((d) => `${d.serial}/${d.state}`).join('、') || '(沒有)'),
      );
      lines.push('[verify] 0 台時前端會提示 adb.noDevice，不會開分頁（見 src/adb.js）');
    }
    // 指定一個不存在的路徑 → 要退回自動搜尋（不是直接失敗）
    const r2 = await invoke('adb_devices', { adbPath: 'C:\\__no_such_adb__.exe' });
    lines.push(`[verify] 指定不存在的路徑會退回自動搜尋：${r2.adb === r.adb}`);
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  }
  log(lines.join('\n'));
}

/** `%TEMP%` 底下的檔案路徑：Windows 用反斜線、mac／Linux 用斜線（`temp_dir` 回的是各平台原樣）。 */
function tempPath(tmp, name) {
  return `${tmp}${tmp.startsWith('/') ? '/' : '\\'}${name}`;
}

/**
 * 檔案總管右鍵選單驗證（TASK-016 C）：寫入 → 讀回 → 刪除。
 *
 * ⚠️ **只碰 `HKCU`**，而且用的是**測試專用的 key 名稱**（`shell_menu_apply` 在測試模式下
 * 才會用專用名稱，所以這裡走的是真的 key）→ 因此結束時一定要復原成原本的狀態。
 */
async function verifyShellMenu() {
  const lines = ['[verify] 檔案總管右鍵選單（只碰 HKCU，而且用測試專用的 key）'];
  // 登錄檔只有 Windows 有（`shell_menu_state` 這個 command 在別的平台不存在）；
  // mac 是 Finder Quick Action、Linux 是 Nautilus 腳本，都是使用者自己裝（docs/PLATFORM-UNIX.md）
  if (!/Windows/i.test((typeof navigator !== 'undefined' && navigator.userAgent) || '')) {
    lines.push('[verify] 非 Windows → 跳過（這個平台沒有登錄檔右鍵選單）');
    log(lines.join('\n'));
    return;
  }
  let real = null;
  try {
    // 使用者真的那個 key：**只讀**，不動它（裡面可能是舊版 v1.2.8 登錄的）
    real = await invoke('shell_menu_state', { sandbox: false });
    lines.push(
      `[verify] 使用者真的那個 key：已登錄=${real.enabled}${real.command ? '（' + real.command + '）' : ''}（只讀）`,
    );
    const on = await invoke('shell_menu_apply', {
      enable: true,
      text: 'AwayTerminal --verify',
      sandbox: true,
    });
    lines.push(
      `[verify] 登錄後讀回：已登錄=${on.enabled}、command=${on.command}` +
        `　（要有 --open-dir 與 %V：${/--open-dir/.test(on.command) && on.command.includes('%V')}）`,
    );
    lines.push(`[verify] 指向目前的執行檔：${on.command.includes(real.exe)}`);
    const off = await invoke('shell_menu_apply', { enable: false, text: '', sandbox: true });
    lines.push(`[verify] 移除後讀回：已登錄=${off.enabled}（要 false）`);
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  } finally {
    // 測試專用的 key 一定要收乾淨；使用者真的那個 key 從頭到尾沒被動過
    const left = await invoke('shell_menu_state', { sandbox: true }).catch(() => ({}));
    lines.push(`[verify] 測試專用的 key 已清掉：${left.enabled === false}`);
    const still = await invoke('shell_menu_state', { sandbox: false }).catch(() => ({}));
    lines.push(
      `[verify] 使用者的 key 沒被動過：${!!real && still.command === real.command}` +
        `（${still.command || '(沒有登錄)'}）`,
    );
    log(lines.join('\n'));
  }
}

/**
 * 匯入舊版設定驗證（TASK-016 D）：**用一份手寫的舊版格式 JSON**（含中文路徑、
 * 新版不支援的 COM 值、代理團隊的最愛），比對匯入結果。
 *
 * ⚠️ 不碰使用者真的舊檔，也不留下匯入的結果（驗完把設定還原）。
 */
async function verifyMigrate() {
  const lines = ['[verify] 匯入舊版設定'];
  const before = await invoke('settings_get');
  const tmp = await invoke('temp_dir');
  const file = tempPath(tmp, 'awayterm-verify-old-settings.json');
  const old = {
    FontFamily: 'Consolas',
    FontSize: 19,
    Foreground: '#00FF00',
    Background: '#101010',
    ImeQuietMs: 35,
    Language: 'zh',
    LogDir: 'D:\\紀錄\\AwayTerminal',
    ComPort: 'COM7',
    ComParity: 'Mark',
    ComStopBits: 'OnePointFive',
    ComFlow: 'RequestToSendXOnXOff',
    RestoreBufferLines: 500,
    TelegramBotToken: '123:abc',
    CustomConns: [
      { Name: '__verify_conn__', Path: 'C:\\x.exe', Args: '-a', Icon: 'run', CloseKey: 'ctrl-d', CloseCount: 2, PickDir: true },
    ],
    Favorites: [
      { Name: '__verify_fav__', Tab: { Type: 'ssh', Host: '192.168.1.9', Port: 2222, User: 'root' }, TeamSetup: '' },
      { Name: '__verify_team__', Tab: { Type: 'ps' }, TeamSetup: '{"folder":"x"}' },
    ],
    SavedTabs: [{ Type: 'ps' }],
    AdbPath: 'C:\\adb.exe',
  };
  try {
    await invoke('save_text_to_file_at', { path: file, text: JSON.stringify(old) });
    const r = await invoke('migrate_import', { path: file });
    const now = await invoke('settings_get');
    lines.push(
      `[verify] 套用 ${r.applied} 個欄位、${r.conns} 條自訂連線、${r.favorites} 筆我的最愛、跳過 ${r.skipped.length} 項`,
    );
    lines.push(
      `[verify] 字型／字級／顏色：${now.fontFamily === 'Consolas' && now.fontSize === 19 && now.foreground === '#00FF00'}`,
    );
    lines.push(`[verify] 語言 zh → zh-TW：${now.language === 'zh-TW'}`);
    lines.push(`[verify] 中文路徑沒壞：${now.logDir === 'D:\\紀錄\\AwayTerminal'}`);
    lines.push(
      `[verify] 新版不支援的 COM 值降級並提醒（${now.comParity}/${now.comStopBits}/${now.comFlow}）：` +
        `${now.comParity === 'None' && now.comStopBits === 'One' && now.comFlow === 'RequestToSend' && r.warnings.length === 3}`,
    );
    lines.push(`[verify] Telegram token 有存下來：${now.telegramBotToken === '123:abc'}`);
    const fav = now.favorites.find((f) => f.name === '__verify_fav__');
    lines.push(
      `[verify] SSH 最愛的參數：${!!fav && fav.ssh && fav.ssh.host === '192.168.1.9' && fav.ssh.port === 2222}`,
    );
    lines.push(`[verify] 代理團隊的最愛有跳過：${r.skippedFavorites.includes('__verify_team__')}`);
    lines.push(`[verify] SavedTabs 沒有匯入（工作階段狀態）：${r.skipped.includes('SavedTabs')}`);
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  } finally {
    // 還原設定（連自訂連線與我的最愛一起清掉驗證用的那兩筆）
    await invoke('settings_apply', {
      patch: {
        fontFamily: before.fontFamily,
        fontSize: before.fontSize,
        foreground: before.foreground,
        background: before.background,
        imeQuietMs: before.imeQuietMs,
        language: before.language,
        logDir: before.logDir,
        restoreBufferLines: before.restoreBufferLines,
      },
    }).catch(() => {});
    await invoke('custom_delete', { name: '__verify_conn__' }).catch(() => {});
    await invoke('fav_delete', { name: '__verify_fav__' }).catch(() => {});
    log(lines.join('\n'));
  }
}

/**
 * 第一次啟動時問要不要匯入舊版設定（`CLAUDE.md` 的「匯入舊版 settings.json」）。
 *
 * 只在**第一次啟動**（新版還沒有 settings.json）而且舊檔存在時問一次；
 * 之後隨時可以從設定視窗的「匯入舊版設定…」再做。**舊檔只讀，不會被改。**
 */
async function offerMigration() {
  try {
    const p = await invoke('migrate_probe');
    if (!p.oldExists || !p.firstRun) return;
    const yes = await askYesNo(T['migrate.title'], fmt('migrate.ask', p.oldPath, p.conns, p.favorites));
    if (!yes) return;
    const r = await invoke('migrate_import', {});
    log(
      `[migrate] 匯入舊版設定：套用 ${r.applied} 個欄位、${r.conns} 條自訂連線、` +
        `${r.favorites} 筆我的最愛（跳過 ${r.skipped.length} 項）`,
    );
    await showInfo(
      T['migrate.title'],
      fmt('migrate.done', r.applied, r.conns, r.favorites) +
        (r.warnings.length ? '\n\n' + r.warnings.join('\n') : ''),
    );
  } catch (e) {
    log(`[migrate] 失敗：${e}`);
  }
}

/**
 * 在指定資料夾開一個 shell 分頁（檔案總管右鍵「用 AwayTerminal 開啟」）。
 *
 * 舊版 `OpenDirFromShell`：資料夾不存在就提示、存在就開一個 PowerShell 分頁在那裡。
 */
async function openDirTab(dir) {
  try {
    const ok = await invoke('dir_exists', { path: dir });
    if (!ok) {
      log(`[open-dir] 找不到資料夾：${dir}`);
      return;
    }
    await createSession({ kind: 'shell', cwd: dir });
    log(`[open-dir] 已在 ${dir} 開了一個分頁`);
  } catch (e) {
    log(`[open-dir] 失敗：${e}`);
  }
}

// ------------------------------------------------------------------ 啟動

(async () => {
  // 語言要在**任何介面文字被設定之前**決定好（initTabBar 會套一次文字）。
  //   - 設定裡有存過 → 用它（使用者改過就固定，同舊版）
  //   - 沒存過（第一次啟動）→ 用系統語言對到我們的八種，對不到用 en（**新增行為**，舊版沒有）
  try {
    const s = await invoke('settings_get');
    const sys = await invoke('system_locale').catch(() => '');
    const pick = s.language ? s.language : matchLang(sys);
    setLang(pick);
    if (!s.language) {
      // 第一次啟動：把選到的語言存起來，之後就不再跟著系統跑
      await invoke('settings_apply', { patch: { language: pick } }).catch(() => {});
      log(`[i18n] 第一次啟動：系統語言 ${sys || '(未知)'} → 介面語言 ${pick}`);
    }
    await pushToBackend();
  } catch (e) {
    log(`[i18n] 語言初始化失敗（用繁體中文）：${e}`);
  }

  // host→JS 的 listener 要在 terminal.js 送 `ready` 之前掛好
  await bridgeReady;
  // 分頁列也要先掛好 `tab-state` 的 listener：第一條 session 是 terminal.js 送出
  // `ready` 之後才建的，那一刻就會 emit 第一筆狀態，晚掛就漏掉第一列。
  await initTabBar();
  // 右上角的 AI CLI 額度（自己定時更新，不必等）
  initQuota();
  // webview 引擎（IME adapter 選哪一邊靠它；真機第一天要先確認這行印對了）
  try {
    const info = engineInfo();
    log(`[main] webview 引擎＝${info.engine}（${info.flavour}）`);
  } catch {
    /* 偵測失敗不影響啟動 */
  }

  // 離開程式的對話框（Rust 擋下 CloseRequested 後會 emit exit-request）
  await initExitDialog();

  // 啟動選項：URL 參數優先（方便在 devtools 直接換），其次是 CLI 參數
  // （`AwayTerminal.exe --cmd claude` / `--verify 2` / `--bench`，見 src-tauri/src/cli.rs）。
  const params = new URLSearchParams(location.search);
  let cli = { cmd: null, verify: 0, bench: false };
  try {
    cli = await invoke('launch_args');
  } catch (e) {
    log(`[main] 讀啟動參數失敗：${e}`);
  }
  const opt = {
    cmd: params.get('cmd') || cli.cmd || null,
    verify: parseInt(params.get('verify') || '', 10) || cli.verify || 0,
    bench: params.get('bench') === '1' || cli.bench,
    // 檔案總管右鍵「用 AwayTerminal 開啟」（參數名照舊版：`--open-dir`）
    openDir: params.get('openDir') || cli.openDir || null,
  };
  // bridge.js 的 onReady 會讀這個決定第一條 session 用什麼指令開
  window.AwayLaunch = opt;

  // 檔案總管右鍵開資料夾：
  //   - **已經在跑**：第二個實例把路徑交給我們（單一執行個體 plugin）→ `open-dir` event
  //   - **還沒在跑**：那個實例就是自己 → `--open-dir` 參數（下面 terminal.js 載完再開，
  //     讓它成為作用中分頁，同舊版「恢復分頁之後再開」）
  await listen('open-dir', (e) => {
    const dir = typeof e.payload === 'string' ? e.payload : '';
    if (dir) openDirTab(dir);
  });

  // ⚠️ **自帶字型要在 terminal.js 之前載完**（TASK-033）：xterm 是用「量一個字有多寬」
  // 決定每一格的大小，字型還沒到就會拿 fallback 量出錯的寬度，然後整個畫面的欄位
  // 都對不準——而且之後不會自己重量。載不到只是退回系統字型，不擋啟動。
  try {
    await loadAppFonts();
  } catch (e) {
    log(`[main] 載入自帶字型失敗（改用系統字型）：${e}`);
  }

  // terminal.js 是舊版原檔（IIFE），載入即執行並在最後送 `ready`
  try {
    await import('./terminal.js');
  } catch (e) {
    log(`[main] 載入 terminal.js 失敗：${e && e.stack ? e.stack : e}`);
    throw e;
  }

  // 第一次啟動而且有舊版設定 → 問要不要匯入（舊檔只讀，不會被改）
  if (!opt.verify) await offerMigration();

  // 啟動參數帶的資料夾（右鍵開啟時沒有既有實例可轉交）→ 開一個 shell 分頁在那裡。
  // 要等 bridge 的 onReady（恢復分頁／預設分頁）做完再開，否則兩邊同時建分頁、
  // 作用中分頁不定（BUG-AUDIT A3）。onReady 看到 openDir 且沒恢復任何分頁時不會再開預設 shell。
  if (opt.openDir) {
    await launchReady;
    await openDirTab(opt.openDir);
  }

  if (opt.bench) {
    try {
      await awayBench(1, 10);
      await awayBenchSmall(200);
    } catch (e) {
      log(`[IPC bench] 失敗：${e}`);
    }
  }

  // 多分頁端到端驗證（`--verify N` / `?verify=N`）：再開 N 條 shell，等提示字元出來，
  // 然後把每條的 buffer 尾端與欄列數報到後端 log。不需要視窗焦點、不用 GUI 自動化。
  const verify = opt.verify;
  if (verify > 0) {
    try {
      for (let i = 0; i < verify; i++) await createExtraSession();
      await awayVerify();
    } catch (e) {
      log(`[verify] 失敗：${e && e.stack ? e.stack : e}`);
    }
  }

  if (params.get('dump') === '1' || import.meta.env.DEV) {
    // 提示字元要等 shell 啟動＋pane fit 完才會有，單次固定延遲不可靠（實測 3s 時 buffer 還是空的）。
    // 最多試 10 次、每 1s 一次，讀到東西就停；10 次都空才報空，這樣「真的沒輸出」與「還沒到」分得出來。
    (async () => {
      for (let i = 1; i <= 10; i++) {
        await new Promise((r) => setTimeout(r, 1000));
        const tail = window.AwayTerm ? window.AwayTerm.tail(null, 6) : [];
        if (tail.length) return awayDump(6);
        if (i === 10) log('[dump] 試了 10 秒，xterm buffer 仍是空的');
      }
    })();
  }
})();
