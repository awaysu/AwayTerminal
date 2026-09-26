// AwayTerminal2 — 階段 1 技術驗證
//
// 目前：單一 xterm.js 終端 + ConPTY 後端跑 PowerShell。
// 輸入處理刻意保持最簡單（直接 onData → session_write_text）；
// 舊版 web/terminal.js 的輸入佇列 / IME / 貼上調校在後續任務才搬進來。
import '@xterm/xterm/css/xterm.css';

import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { SerializeAddon } from '@xterm/addon-serialize';
import { SearchAddon } from '@xterm/addon-search';
import { invoke, Channel } from '@tauri-apps/api/core';

const term = new Terminal({
  fontFamily: '"Cascadia Mono", "Consolas", "Microsoft JhengHei", "微軟正黑體", monospace',
  fontSize: 14,
  cursorBlink: true,
  allowProposedApi: true, // unicode11 addon 需要
  scrollback: 10000,
  theme: {
    background: '#1e1e1e',
    foreground: '#cccccc',
    cursor: '#ffffff',
  },
});

const fitAddon = new FitAddon();
term.loadAddon(fitAddon);
term.loadAddon(new WebLinksAddon());
term.loadAddon(new SerializeAddon());
term.loadAddon(new SearchAddon());

// 全形字寬度：unicode 11 版本比內建 v6 準
const unicode11 = new Unicode11Addon();
term.loadAddon(unicode11);
term.unicode.activeVersion = '11';

term.open(document.getElementById('term-body'));

// --- 渲染器：WebGL → DOM 退回 ---
let renderer = 'DOM';
try {
  const webgl = new WebglAddon();
  // context lost（休眠喚醒 / 驅動重置）時卸載 addon，xterm 自動退回 DOM 渲染
  webgl.onContextLoss(() => {
    console.warn('[AwayTerminal] WebGL context lost, falling back to DOM renderer');
    webgl.dispose();
    term.writeln('\r\n\x1b[33m[AwayTerminal] WebGL context lost → 已退回 DOM 渲染\x1b[0m');
  });
  term.loadAddon(webgl);
  renderer = 'WebGL';
} catch (e) {
  console.warn('[AwayTerminal] WebGL addon 載入失敗，退回 DOM 渲染:', e);
  renderer = 'DOM (WebGL 載入失敗)';
}
console.log('[AwayTerminal] renderer =', renderer);
// 也回報到後端 stdout，方便從啟動 log 確認 WebGL 有沒有啟用（不必開 devtools）
invoke('report_renderer', { renderer }).catch(() => {});

fitAddon.fit();

const log = (msg) => invoke('log_line', { msg }).catch(() => {});

// ------------------------------------------------------------------ session

/** 目前這個終端對應的 session id（本階段只有一條）。 */
let sessionId = null;
/** 已收到結束事件，不再送輸入。 */
let sessionEnded = false;

async function startSession() {
  const onEvent = new Channel();
  onEvent.onmessage = (msg) => {
    if (msg instanceof ArrayBuffer) {
      // PTY 原始輸出：直接餵 bytes，不要先 decode 成字串
      term.write(new Uint8Array(msg));
      return;
    }
    if (msg && msg.kind === 'exit') {
      sessionEnded = true;
      const code = msg.exitCode === null || msg.exitCode === undefined ? '?' : msg.exitCode;
      term.writeln(`\r\n\x1b[33m[行程已結束，exit code ${code}]\x1b[0m`);
    }
  };

  const info = await invoke('session_create', {
    kind: 'powershell',
    cols: term.cols,
    rows: term.rows,
    cwd: null,
    onEvent,
  });
  sessionId = info.id;
  console.log('[AwayTerminal] session', info);
  return info;
}

// 頁面卸載（dev 的 Vite 重載、或之後的視窗重整）時把 session 收掉，
// 否則舊的 pwsh + OpenConsole 會留到整個程式結束才被清（dev log 曾出現 session 1／2 並存）。
window.addEventListener('beforeunload', () => {
  if (sessionId === null) return;
  const id = sessionId;
  sessionId = null;
  invoke('session_close', { id }).catch(() => {});
});

term.onData((data) => {
  if (sessionId === null || sessionEnded) return;
  invoke('session_write_text', { id: sessionId, text: data }).catch(() => {});
});

term.onBinary((data) => {
  if (sessionId === null || sessionEnded) return;
  const bytes = new Uint8Array(data.length);
  for (let i = 0; i < data.length; i++) bytes[i] = data.charCodeAt(i) & 0xff;
  invoke('session_write', { id: sessionId, data: Array.from(bytes) }).catch(() => {});
});

// --- 隨視窗縮放 ---
let resizeTimer = null;
let lastCols = 0;
let lastRows = 0;
const doFit = () => {
  try {
    fitAddon.fit();
  } catch {
    return; // 視窗最小化時 fit 會失敗
  }
  if (sessionId === null || sessionEnded) return;
  if (term.cols === lastCols && term.rows === lastRows) return;
  lastCols = term.cols;
  lastRows = term.rows;
  invoke('session_resize', { id: sessionId, cols: term.cols, rows: term.rows }).catch(() => {});
};
const scheduleFit = () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(doFit, 30);
};
window.addEventListener('resize', scheduleFit);
new ResizeObserver(scheduleFit).observe(document.getElementById('term-body'));

// ------------------------------------------------------------- IPC bench
//
// CLAUDE.md 風險 5：「Tauri 二進位 channel 可能被序列化成 JSON 數字陣列（比 base64 更慢）」。
// 這裡實測四條路，結果同時印在終端與後端 log（後端 log 方便寫進 docs/IPC-BENCH.md）。
// dev 模式自動跑一次；任何時候都可以在 devtools 打 `awayBench()` 重跑。

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
  // command 直接回 Vec<u8> → serde 序列化成 JSON 數字陣列
  vec: (size) => invoke('bench_vec', { size }),
  // base64 字串 + atob（舊版 o{id}{US}{base64} 的做法）
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
  const size = Math.round(sizeMb * MB);
  const header = `[IPC bench] ${iterations} × ${sizeMb}MB`;
  term.writeln(`\r\n\x1b[36m${header}\x1b[0m`);
  log(header);
  const rows = [];
  for (const name of ['raw', 'vec', 'base64', 'channel']) {
    const r = await benchMode(name, size, iterations);
    rows.push(r);
    const line =
      `  ${name.padEnd(8)} median ${r.median.toFixed(1).padStart(8)} ms` +
      `  mean ${r.mean.toFixed(1).padStart(8)} ms` +
      `  min ${r.min.toFixed(1).padStart(8)} ms` +
      `  max ${r.max.toFixed(1).padStart(8)} ms` +
      `  ${r.mbPerSec.toFixed(1).padStart(6)} MB/s` +
      `  ${r.bytes} bytes via ${r.transport}`;
    term.writeln(line);
    log(line);
  }
  return rows;
}
window.awayBench = awayBench;

// --- 小 payload 也量一次：channel 的 1024 bytes 門檻兩邊各一次 ---
async function awayBenchSmall(iterations = 200) {
  const header = `[IPC bench small] ${iterations} × (512 / 2048 bytes) via channel`;
  term.writeln(`\r\n\x1b[36m${header}\x1b[0m`);
  log(header);
  for (const size of [512, 2048]) {
    const r = await benchMode('channel', size, iterations);
    const line =
      `  channel ${String(size).padStart(5)}B  median ${r.median.toFixed(3)} ms` +
      `  mean ${r.mean.toFixed(3)} ms  ${r.bytes} bytes via ${r.transport}`;
    term.writeln(line);
    log(line);
  }
}
window.awayBenchSmall = awayBenchSmall;

// ------------------------------------------------- 端到端驗證（不需視窗焦點）
//
// 把 xterm buffer 的純文字尾端回報到後端 log，用來證明
// 「PTY 輸出 → channel → term.write」整條路真的通，不必看視窗、也不必自動化 GUI。
// dev 模式在 session 起來後自動跑一次；任何時候可在 devtools 打 `awayDump()`。

function bufferTail(lines = 6) {
  const buf = term.buffer.active;
  const out = [];
  for (let y = 0; y < buf.length; y++) {
    const line = buf.getLine(y);
    if (!line) continue;
    const text = line.translateToString(true);
    if (text.trim() !== '') out.push(text);
  }
  return out.slice(-lines);
}

function awayDump(lines = 6) {
  const tail = bufferTail(lines);
  // 一定要組成一包再送：每行各一次 invoke 是各自獨立的非同步呼叫，
  // 到後端的順序不保證（第一版就這樣印出亂序的行）。
  const body = tail.map((t) => `[dump] | ${t}`).join('\n');
  log(
    `[dump] xterm buffer 尾端 ${tail.length} 行（共 ${term.buffer.active.length} 行）：\n${body}`
  );
  return tail;
}
window.awayDump = awayDump;

// ------------------------------------------------------------------ 啟動

(async () => {
  term.writeln(`\x1b[1;36m[AwayTerminal2] renderer = ${renderer}\x1b[0m`);
  try {
    const backend = await invoke('conpty_backend');
    term.writeln(`\x1b[1;36m[AwayTerminal2] ConPTY backend = ${backend}\x1b[0m`);
  } catch (e) {
    term.writeln(`\x1b[31m[AwayTerminal2] conpty_backend 失敗：${e}\x1b[0m`);
  }
  try {
    const pong = await invoke('ping');
    term.writeln(`\x1b[90m[IPC] ${pong}\x1b[0m`);
  } catch (e) {
    term.writeln(`\x1b[31m[IPC] ping 失敗：${e}\x1b[0m`);
  }

  // dev 模式自動跑一次 IPC bench（release 不跑；用 awayBench() 手動觸發）
  if (import.meta.env.DEV) {
    try {
      await awayBench(1, 10);
      await awayBenchSmall(200);
    } catch (e) {
      term.writeln(`\x1b[31m[IPC bench] 失敗：${e}\x1b[0m`);
    }
  }

  term.writeln('');
  try {
    const info = await startSession();
    lastCols = term.cols;
    lastRows = term.rows;
    term.writeln(
      `\x1b[90m[session ${info.id}] pid=${info.pid} shell=${info.shell} backend=${info.backend}\x1b[0m`
    );
  } catch (e) {
    term.writeln(`\x1b[31m[session] 啟動失敗：${e}\x1b[0m`);
  }
  term.focus();

  // dev：等 shell 印完提示字元後傾印一次，證明輸出真的進到 xterm
  if (import.meta.env.DEV && sessionId !== null) {
    setTimeout(() => awayDump(6), 2500);
  }
})();
