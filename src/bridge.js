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

const US = '\x1f';

/** host → JS 的 listener（terminal.js 會註冊一個）。 */
const listeners = [];

/** 還沒 makeTerm 就先到的輸出：暫存，等 `n` 處理完再補寫。 */
const pendingOut = new Map();

/** 目前這條 session 的 id（本階段只有一條；分頁 UI 是之後的任務）。 */
let firstSessionId = null;

function log(msg) {
  invoke('log_line', { msg: String(msg) }).catch(() => {});
}

/** 把一個舊協定字串交給 terminal.js。 */
function deliver(str) {
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
    case 'D':
      // 診斷（terminal.js 的 dbgLog）→ 後端 log，等效舊版的 diag.log
      log(`[diag] ${rest}`);
      return;
    default:
      // p / k / z / G / a / m / U 等尚未接上的：交給 Rust 記 log（不靜靜丟掉）
      invoke('host_message', { msg }).catch(() => {});
      return;
  }
}

// ------------------------------------------------------------- 啟動流程

/**
 * terminal.js 載完會送 `ready`（檔案最後一行）。之後：
 *   1. `host_ready` → Rust emit `T{json}` 設定
 *   2. 依 URL 參數決定第一條 session（`?cmd=<指令>`，預設 PowerShell）
 *   3. `session_create` 建好後 Rust emit `n{id}US{title}US{flags}` → terminal.js makeTerm
 *   4. makeTerm 之後 terminal.js 自己 fit 並送 `r{id}US{cols},{rows}` 回來
 * 尺寸：`n` 之前還沒有 term，所以先用一個合理的預設值開 PTY，`r` 到了立刻校正
 *（同舊版：C# 建分頁時用預設尺寸開 session，之後靠 `r` 校正）。
 */
async function onReady() {
  await invoke('host_ready');

  const params = new URLSearchParams(location.search);
  const cmd = params.get('cmd');

  const onEvent = new Channel();
  onEvent.onmessage = (m) => {
    if (m instanceof ArrayBuffer) {
      onOutput(firstSessionId, new Uint8Array(m));
      return;
    }
    if (m && m.kind === 'exit') {
      const code = m.exitCode === null || m.exitCode === undefined ? '?' : m.exitCode;
      log(`[bridge] session ${m.id} 結束，exit code ${code}`);
      const term = window.AwayTerm;
      if (term && term.hasTerm(m.id)) {
        term.writeOutput(
          m.id,
          new TextEncoder().encode(`\r\n\x1b[33m[行程已結束，exit code ${code}]\x1b[0m\r\n`)
        );
      }
    }
  };

  const info = await invoke('session_create', {
    kind: cmd ? 'custom' : 'powershell',
    command: cmd || null,
    cols: 120,
    rows: 30,
    cwd: null,
    onEvent,
  });
  firstSessionId = info.id;
  log(
    `[bridge] session ${info.id} pid=${info.pid} shell=${info.shell} flags=${info.flags || '-'} backend=${info.backend}`
  );
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

/** 頁面卸載（dev 的 Vite 重載）時收掉 session，否則舊的 pwsh + OpenConsole 會留到程式結束。 */
window.addEventListener('beforeunload', () => {
  if (firstSessionId === null) return;
  const id = firstSessionId;
  firstSessionId = null;
  invoke('session_close', { id }).catch(() => {});
});

export { log };
