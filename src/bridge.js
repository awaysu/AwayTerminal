// AwayTerminal2 bridge：把舊版 `terminal.js` 期待的 WebView2 介面接到 Tauri IPC。
//
// 舊版 `terminal.js` 只認 `window.chrome.webview` 的兩個東西：
//   ws.postMessage(字串)                      JS → host
//   ws.addEventListener("message", fn)        host → JS（事件物件的 .data 是字串）
// 這裡提供同名的 `window.AwayBridge`，協定字串**一個字都沒改**，只是內部翻成
// Tauri command / event。輸出例外：不走 `o{id}US{base64}` 字串，改用 TASK-002 的二進位
// channel 直接把 bytes 交給 `window.AwayTerm.writeOutput(id, u8)`（見 terminal.js AT2-3）。
//
// 協定對照表在 docs/PROTOCOL.md。

import { invoke, Channel } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { restoreSavedTabs } from './restoretabs.js';

import { T } from './strings.js';
import { showUrlMenu, noteMouseHint, noSelectionToast, toast, writeClipboard } from './tabbar.js';

const US = '\x1f';

/** host → JS 的 listener（terminal.js 會註冊一個）。 */
const listeners = [];

/** 還沒 makeTerm 就先到的輸出：暫存，等 `n` 處理完再補寫。 */
const pendingOut = new Map();

/** 目前開著的 session id（TASK-004：多分頁，不再是單一個 firstSessionId）。 */
const openSessions = new Set();

function log(msg) {
  invoke('log_line', { msg: String(msg) }).catch(() => {});
}

/**
 * Telegram 遠端截圖：`q{id}US shot` → 把畫面文字畫進一張**新的** `<canvas>` → PNG。
 *
 * 為什麼不是抓 xterm 自己的 canvas：WebGL renderer 的 drawing buffer 畫完就可以被丟掉
 * （`preserveDrawingBuffer` 預設 false），`toDataURL` 多半拿到全黑；把它打開會讓每一格
 * 都多留一份 buffer、拖慢渲染——而渲染速度正是這一版的重點（CLAUDE.md 速度改善點 1）。
 * 所以改成「要文字、自己畫一張」：完全不碰 xterm 的渲染路徑，也不管它是 WebGL／canvas／DOM。
 *
 * 舊版是 WPF 把 pane 整塊 render 成點陣圖，**有每個字的顏色**；這一版是單色（前景／背景）。
 * 差異寫在 `docs/REGRESSION-CHECKLIST.md` 的「刻意與舊版不同」。
 */
function shotPng(id) {
  const api = window.AwayTerm;
  const s = api && api.screen ? api.screen(id) : null;
  if (!s) return '';
  const size = Math.max(10, Math.min(28, s.fontSize || 14));
  const cw = Math.ceil(size * 0.6);   // 等寬字的寬高比約 0.6
  const lh = Math.ceil(size * 1.35);
  const pad = 8;
  const cv = document.createElement('canvas');
  // 手機看的圖：2 倍取樣才不會糊掉（Telegram 會再壓一次）
  const dpr = 2;
  cv.width = (s.cols * cw + pad * 2) * dpr;
  cv.height = (s.rows * lh + pad * 2) * dpr;
  const g = cv.getContext('2d');
  if (!g) return '';
  g.scale(dpr, dpr);
  g.fillStyle = s.bg || '#1e1e1e';
  g.fillRect(0, 0, cv.width, cv.height);
  g.fillStyle = s.fg || '#e0e0e0';
  g.font = `${size}px ${s.fontFamily || 'Consolas, monospace'}`;
  g.textBaseline = 'top';
  for (let y = 0; y < s.lines.length; y++) {
    const line = s.lines[y];
    if (!line) continue;
    // 逐字畫在格子上：中文與框線字才會和終端機一樣對齊（整行 fillText 會因字寬不同跑掉）
    let col = 0;
    for (const ch of line) {
      if (ch !== ' ') g.fillText(ch, pad + col * cw, pad + y * lh);
      col += wcwidth(ch);
      if (col > s.cols) break;
    }
  }
  const url = cv.toDataURL('image/png');
  return url.slice(url.indexOf(',') + 1);
}

/** 這個字佔幾欄（只要分「全形 2、其餘 1」，和 xterm 的排版一致就夠）。 */
function wcwidth(ch) {
  const c = ch.codePointAt(0);
  if (c >= 0x1100 && (c <= 0x115f || c === 0x2329 || c === 0x232a
      || (c >= 0x2e80 && c <= 0xa4cf && c !== 0x303f)
      || (c >= 0xac00 && c <= 0xd7a3) || (c >= 0xf900 && c <= 0xfaff)
      || (c >= 0xfe30 && c <= 0xfe6f) || (c >= 0xff00 && c <= 0xff60)
      || (c >= 0xffe0 && c <= 0xffe6) || (c >= 0x20000 && c <= 0x3fffd))) return 2;
  return 1;
}

/** 把一個舊協定字串交給 terminal.js。 */
function deliver(str) {
  // v2 自己多的 `q…shot`：**在這裡處理掉、不往下傳**——terminal.js 的 `q` 分支對不認識的
  // 種類會回傳選取文字（那個檔案要和舊版逐字一樣，不能為了截圖去改它）。
  if (str.charCodeAt(0) === 113 /* q */) {
    const k = str.indexOf(US, 1);
    if (k > 0 && str.slice(k + 1) === 'shot') {
      const id = str.slice(1, k);
      let png = '';
      try {
        png = shotPng(id);
      } catch (e) {
        log(`[bridge] 截圖失敗：${e}`);
      }
      invoke('pane_answer', { id: Number(id), kind: 'shot', text: png }).catch(() => {});
      return;
    }
  }
  const ev = { data: str };
  for (const fn of listeners) {
    try {
      fn(ev);
    } catch (e) {
      log(`[bridge] listener 失敗：${e}`);
    }
  }
  flushPending();
}

/** terms[id] 一出現就把扣住的輸出依序補寫。 */
function flushPending() {
  if (pendingOut.size === 0) return;
  const term = window.AwayTerm;
  if (!term) return;
  for (const [id, chunks] of [...pendingOut]) {
    if (!term.hasTerm(id)) continue;
    pendingOut.delete(id);
    for (const c of chunks) term.writeOutput(id, c);
  }
}

function onOutput(id, bytes) {
  const term = window.AwayTerm;
  if (term && term.hasTerm(id)) {
    term.writeOutput(id, bytes);
    return;
  }
  // `n` 還沒被 terminal.js 處理到（event 與 invoke 回覆的先後不保證）→ 扣住，別丟掉
  if (!pendingOut.has(id)) pendingOut.set(id, []);
  pendingOut.get(id).push(bytes);
  setTimeout(flushPending, 0);
}

// ------------------------------------------------------------- JS → host

/**
 * 舊協定的第一個字元決定種類。沒接上的種類一律送 `host_message`，
 * Rust 端記 log——這樣「哪些還沒接」在 dev log 裡看得出來，不會靜靜消失。
 */
function postMessage(raw) {
  const msg = String(raw);
  if (!msg.length) return;

  // `ready` 沒有 id 欄位，而且它的第一個字元 `r` 和 `r{id}US{cols},{rows}` 撞號——
  // 一定要先比對完整字串，否則 ready 會被尺寸分支吃掉（第一版就是這樣，畫面全黑）。
  if (msg === 'ready') {
    onReady().catch((e) => log(`[bridge] ready 失敗：${e}`));
    return;
  }

  const kind = msg.charAt(0);
  const rest = msg.slice(1);

  switch (kind) {
    case 'i': {
      // i{id}US{text} 輸入
      const k = rest.indexOf(US);
      if (k < 0) return;
      const id = Number(rest.slice(0, k));
      const text = rest.slice(k + 1);
      invoke('session_write_text', { id, text }).catch((e) => log(`[bridge] write 失敗：${e}`));
      return;
    }
    case 'r': {
      // r{id}US{cols},{rows} 尺寸
      const k = rest.indexOf(US);
      if (k < 0) return;
      const id = Number(rest.slice(0, k));
      const wh = rest.slice(k + 1).split(',');
      const cols = parseInt(wh[0], 10);
      const rows = parseInt(wh[1], 10);
      if (!(cols > 0 && rows > 0)) return;
      lastSize = { cols, rows };
      invoke('session_resize', { id, cols, rows }).catch(() => {});
      return;
    }
    case 'y': {
      // y{id}US{text}：程式以 OSC 52 要求寫剪貼簿（1.1.10）
      const k = rest.indexOf(US);
      const text = k < 0 ? '' : rest.slice(k + 1);
      if (!text) return;
      // 用 webview 自己的 clipboard API，不必多裝 tauri clipboard plugin
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).catch((e) => log(`[bridge] OSC52 寫剪貼簿失敗：${e}`));
      } else {
        log(`[bridge] 這個 webview 沒有 clipboard API，OSC52 ${text.length} 字未寫入`);
      }
      return;
    }
    case 'p':
      // p{id}：使用者在分割模式點了某個 pane → 設為作用中。後端**不回送 `s`**（避免迴圈）
      invoke('pane_selected', { id: Number(rest) }).catch(() => {});
      return;
    case 'k': {
      // k{id1},{id2},…：pane 拖曳後的新順序。後端**不回送 `K`**
      const ids = rest
        .split(',')
        .map((s) => Number(s))
        .filter((n) => Number.isFinite(n));
      invoke('pane_reordered', { ids }).catch(() => {});
      return;
    }
    case 'z':
      // z{size}：Ctrl+滾輪縮放後的字級 → 存設定（後端有防抖）
      invoke('pane_font_size', { size: Number(rest) }).catch(() => {});
      return;
    case 'a': {
      // a{id}US{kind}US{text}：對 q 的回覆
      const k1 = rest.indexOf(US);
      if (k1 < 0) return;
      const k2 = rest.indexOf(US, k1 + 1);
      if (k2 < 0) return;
      onAnswer(Number(rest.slice(0, k1)), rest.slice(k1 + 1, k2), rest.slice(k2 + 1));
      return;
    }
    case 'U':
      // U{url}：點了終端機裡的連結 → 跳「從瀏覽器開啟／複製網址」選單（舊版 ShowUrlMenu）。
      // 選單開在游標位置（舊版 PlacementMode.MousePoint）
      showUrlMenu(rest, lastMouse.x, lastMouse.y);
      return;
    case 'm':
      // m{id}：下一個空的選取回覆是因為程式接管了滑鼠（舊版 _selMouseHintId）
      noteMouseHint(rest);
      return;
    case 'D':
      // 診斷（terminal.js 的 dbgLog）→ 後端 log，等效舊版的 diag.log
      log(`[diag] ${rest}`);
      return;
    case 'G': {
      // G{下方id}US{上列比例}：代理團隊上下分隔線拖完的新比例（雙擊回 0.5 也走這裡）
      const gi = rest.indexOf(US);
      if (gi < 0) return;
      invoke('agent_ratio', {
        bottomTab: Number(rest.slice(0, gi)),
        ratio: Number(rest.slice(gi + 1)),
      }).catch(() => {});
      return;
    }
    default:
      // 尚未接上的：交給 Rust 記 log（不靜靜丟掉）
      invoke('host_message', { msg }).catch(() => {});
      return;
  }
}

/**
 * `q` 的回覆（`a` 協定）。舊版是 C# 端統一處理，新版按「誰做得最省」分：
 *   - `sel` / `selpaste` / `all`：寫剪貼簿 + toast。**在前端做**——webview 自己有
 *     clipboard API，不必為此多裝一個 tauri plugin。`selpaste` 再貼回同一個分頁
 *     （走 `toolbar_paste` → `v` 協定，所以 claude 分頁照樣是 ESC+CR）。
 *   - `file`：轉呼叫 Rust 的 `save_text_to_file`（存檔對話框與寫檔在後端）。
 *   - `cwd`：轉給 Rust 改分頁名稱。
 *   - `save` / `text`：恢復分頁與 Telegram 遠端還沒做 → Rust 記 log。
 * toast 文字全部照舊版 `toast.*`。
 */
async function onAnswer(id, kind, text) {
  if (kind === 'cwd' || kind === 'save' || kind === 'text') {
    invoke('pane_answer', { id, kind, text }).catch(() => {});
    return;
  }

  if (kind === 'file') {
    try {
      const saved = await invoke('save_text_to_file', { id, text });
      if (saved) toast(T['toast.saved']);
    } catch (e) {
      log(`[bridge] 存檔失敗：${e}`);
    }
    return;
  }

  // sel / selpaste / all（未列出的種類舊版一律當成選取文字，照抄）
  if (!text) {
    toast(noSelectionToast(id));
    return;
  }
  await writeClipboard(text);
  if (kind === 'selpaste') {
    await invoke('toolbar_paste', { id, text }).catch(() => {});
    toast(T['toast.copiedPasted']);
  } else {
    toast(kind === 'all' ? T['toast.copiedAll'] : T['toast.copied']);
  }
}

/** 網址選單要開在游標位置（舊版 PlacementMode.MousePoint）。 */
const lastMouse = { x: 0, y: 0 };
window.addEventListener(
  'mousedown',
  (e) => {
    lastMouse.x = e.clientX;
    lastMouse.y = e.clientY;
  },
  true
);

// ------------------------------------------------------------- 開新連線

/**
 * 最後一次由前端回報的尺寸。新分頁用它開 PTY（同舊版的 `_lastCols/_lastRows`，
 * 踩雷紀錄：「初始尺寸勿寫死 80×24」）。第一條連線還沒有任何 pane，用 120×30 起手，
 * `n` 之後 terminal.js 自己 fit 再送 `r` 校正。
 */
let lastSize = { cols: 120, rows: 30 };

/**
 * 開一條連線。`kind`＝`shell`（PowerShell）或 `custom`（帶 `command`）。
 *
 * 輸出走 Channel：Rust 送 ArrayBuffer＝畫面資料，送 JSON 物件＝結束事件。
 * **channel 可能比 `session_create` 的回覆更早送資料**，所以 id 還不知道時先扣住。
 */
export async function createSession(opts = {}) {
  const holder = { id: null, pending: [] };
  const onEvent = new Channel();
  onEvent.onmessage = (m) => {
    if (m instanceof ArrayBuffer) {
      const u8 = new Uint8Array(m);
      if (holder.id === null) {
        holder.pending.push(u8);
        return;
      }
      onOutput(holder.id, u8);
      return;
    }
    if (m && m.kind === 'exit') {
      const code = m.exitCode === null || m.exitCode === undefined ? '?' : m.exitCode;
      log(`[bridge] session ${m.id} 結束，exit code ${code}`);
      openSessions.delete(m.id);
      const term = window.AwayTerm;
      if (term && term.hasTerm(m.id)) {
        term.writeOutput(
          m.id,
          new TextEncoder().encode(`\r\n\x1b[90m[行程已結束，exit code ${code}]\x1b[0m\r\n`)
        );
      }
    }
  };

  const info = await invoke('session_create', {
    kind: opts.kind || 'shell',
    command: opts.command || null,
    title: opts.title || null,
    cols: lastSize.cols,
    rows: lastSize.rows,
    cwd: opts.cwd || null,
    ssh: opts.ssh || null,
    telnet: opts.telnet || null,
    com: opts.com || null,
    conn: opts.conn || null,
    // 代理團隊的一格（`kind: 'agent'`）：要開哪個團隊的第幾格
    agent: opts.agent || null,
    // 恢復分頁：要倒回第幾筆的畫面（`restore_list` 的索引）
    restore: opts.restore === undefined ? null : opts.restore,
    onEvent,
  });

  holder.id = info.id;
  openSessions.add(info.id);
  for (const c of holder.pending) onOutput(info.id, c);
  holder.pending.length = 0;

  log(
    `[bridge] session ${info.id} pid=${info.pid} shell=${info.shell} flags=${info.flags || '-'} backend=${info.backend} title=${info.title}`
  );
  return info;
}

// ------------------------------------------------------------- 啟動流程

/**
 * terminal.js 載完會送 `ready`（檔案最後一行）。之後：
 *   1. `host_ready` → Rust 依 settings.json emit `T{json}`
 *   2. 上次關閉時存了分頁就先恢復它們（含畫面），否則依 URL 參數決定第一條 session
 *      （`?cmd=<指令>`，預設 PowerShell）
 *   3. `session_create` 建好後 Rust emit `n{id}…` 與 `s{id}` → terminal.js makeTerm + 選取
 *   4. 之後 terminal.js 自己 fit 並送 `r{id}US{cols},{rows}` 回來
 */
async function onReady() {
  await invoke('host_ready');

  // 恢復分頁（1.0.45）：上次關閉時勾了「下次開啟恢復目前分頁」就照那份清單重開，
  // **不問使用者**（同舊版 OnLoaded）。`--cmd` 指定了指令時不恢復——那是「這次要開這個」的意思。
  const cmd = (window.AwayLaunch && window.AwayLaunch.cmd) || null;
  if (!cmd) {
    try {
      const n = await restoreSavedTabs(createSession);
      if (n > 0) return;
    } catch (e) {
      log(`[bridge] 恢復分頁失敗，改開預設分頁：${e}`);
    }
  }

  // 第一條 session：`--cmd` / `?cmd=` 指定的指令，否則預設 shell。
  // main.js 已經把兩個來源合好放在 window.AwayLaunch（URL 優先）。
  await createSession(cmd ? { kind: 'custom', command: cmd } : { kind: 'shell' });
}

// ------------------------------------------------------------- host → JS

async function installHostListener() {
  await listen('host-msg', (e) => {
    if (typeof e.payload === 'string') deliver(e.payload);
  });
}

// ------------------------------------------------------------- 對外介面

const AwayBridge = {
  postMessage,
  addEventListener(type, fn) {
    if (type === 'message' && typeof fn === 'function') listeners.push(fn);
  },
  removeEventListener(type, fn) {
    if (type !== 'message') return;
    const i = listeners.indexOf(fn);
    if (i >= 0) listeners.splice(i, 1);
  },
};

window.AwayBridge = AwayBridge;

/** main.js 會 await 這個，確保 host→JS 的 listener 在 terminal.js 送 ready 之前就掛好。 */
export const bridgeReady = installHostListener();

/** 頁面卸載（dev 的 Vite 重載）時收掉所有 session，否則舊的 pwsh + OpenConsole 會留到程式結束。 */
window.addEventListener('beforeunload', () => {
  for (const id of [...openSessions]) {
    openSessions.delete(id);
    invoke('tab_close', { id }).catch(() => {});
  }
});

export { log };
