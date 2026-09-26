// AwayTerminal2 前端進入點。
//
// 這個檔案本身**不管終端機邏輯**——那是搬過來的 `terminal.js` 的工作（輸入佇列、IME 守衛、
// 靜止閘門、去重、貼上路徑、分割/分欄 layout、Ctrl+F…）。main.js 只做三件事：
//   1. 把 `terminal.js` 期待的全域（Terminal / FitAddon / …）準備好——它原本靠 <script> 載入 UMD；
//   2. 提供 `window.AwayWebgl`（WebGL addon + DOM 退回），這是 terminal.js 的 AT2-2 修改要呼叫的；
//   3. 等 bridge 掛好 host→JS listener 之後才載入 terminal.js（它一載完就送 `ready`）。
import '@xterm/xterm/css/xterm.css';
import './style.css';

import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { SerializeAddon } from '@xterm/addon-serialize';
import { invoke, Channel } from '@tauri-apps/api/core';

import { bridgeReady, log } from './bridge.js';

// --- terminal.js 期待的全域（沿用 UMD 的命名空間形狀，這樣 terminal.js 一個字都不用改）---
window.Terminal = Terminal;
window.FitAddon = { FitAddon };
window.Unicode11Addon = { Unicode11Addon };
window.WebLinksAddon = { WebLinksAddon };
window.SerializeAddon = { SerializeAddon };

// --- WebGL 渲染器（terminal.js AT2-2 會呼叫）---
let rendererReported = false;
window.AwayWebgl = function (term, id) {
  let renderer = 'DOM';
  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => {
      // 休眠喚醒 / 驅動重置：卸載 addon，xterm 自動退回 DOM 渲染
      console.warn('[AwayTerminal] WebGL context lost, falling back to DOM renderer');
      log(`[AwayTerminal] pane ${id} WebGL context lost → 退回 DOM 渲染`);
      webgl.dispose();
    });
    term.loadAddon(webgl);
    renderer = 'WebGL';
  } catch (e) {
    console.warn('[AwayTerminal] WebGL addon 載入失敗，退回 DOM 渲染:', e);
    renderer = 'DOM (WebGL 載入失敗)';
  }
  console.log('[AwayTerminal] renderer =', renderer);
  if (!rendererReported) {
    rendererReported = true;
    invoke('report_renderer', { renderer }).catch(() => {});
  }
  return renderer;
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

// ------------------------------------------------------------------ 啟動

(async () => {
  // host→JS 的 listener 要在 terminal.js 送 `ready` 之前掛好
  await bridgeReady;

  const params = new URLSearchParams(location.search);

  // terminal.js 是舊版原檔（IIFE），載入即執行並在最後送 `ready`
  try {
    await import('./terminal.js');
  } catch (e) {
    log(`[main] 載入 terminal.js 失敗：${e && e.stack ? e.stack : e}`);
    throw e;
  }

  if (params.get('bench') === '1') {
    try {
      await awayBench(1, 10);
      await awayBenchSmall(200);
    } catch (e) {
      log(`[IPC bench] 失敗：${e}`);
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
