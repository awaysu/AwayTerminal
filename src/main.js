// AwayTerminal2 — 階段 1 技術驗證骨架
// 目前只有單一 xterm.js 終端 + 前端 loopback echo。PTY 後端在後續任務接上。
import '@xterm/xterm/css/xterm.css';

import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { SerializeAddon } from '@xterm/addon-serialize';
import { SearchAddon } from '@xterm/addon-search';
import { invoke } from '@tauri-apps/api/core';

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
// 同時回報到後端 stdout，方便從啟動 log 確認 WebGL 有沒有啟用
invoke('report_renderer', { renderer }).catch(() => {});

fitAddon.fit();

// --- 測試輸出 ---
term.writeln(`\x1b[1;36m[AwayTerminal2] renderer = ${renderer}\x1b[0m`);
term.writeln('');
term.writeln('測試 AwayTerminal2 注音 繁體中文');
term.writeln('全形對齊檢查（下面兩行右端應該對齊）：');
term.writeln('  ABCDEFGHIJ|');
term.writeln('  中文字全形５|');
term.writeln('');
term.writeln(
  '\x1b[31m紅\x1b[32m綠\x1b[33m黃\x1b[34m藍\x1b[35m洋紅\x1b[36m青\x1b[37m白\x1b[0m' +
    '  \x1b[1;31m亮紅\x1b[1;32m亮綠\x1b[1;33m亮黃\x1b[1;34m亮藍\x1b[0m'
);
term.writeln('\x1b[48;5;236m\x1b[38;5;208m256 色\x1b[0m \x1b[38;2;120;200;255mTrueColor RGB\x1b[0m');
term.writeln('');
term.writeln(`\x1b[90mxterm.js ${term.options.fontSize}px / ${term.cols}x${term.rows}\x1b[0m`);
term.writeln('');

// --- 驗證 Tauri IPC ---
invoke('ping')
  .then((msg) => term.writeln(`\x1b[32m[IPC]\x1b[0m invoke('ping') → ${msg}`))
  .catch((e) => term.writeln(`\x1b[31m[IPC]\x1b[0m invoke('ping') 失敗：${e}`));

term.writeln('');
term.writeln('\x1b[90m（以下為前端 loopback echo，尚未接 PTY）\x1b[0m');
term.write('\r\n$ ');

// --- 前端 loopback：把輸入 echo 回終端 ---
term.onData((data) => {
  if (data === '\r') {
    term.write('\r\n$ ');
  } else if (data === '\x7f') {
    // Backspace：只在同一行內退格
    term.write('\b \b');
  } else {
    term.write(data);
  }
});

// --- 隨視窗縮放 ---
let resizeTimer = null;
const doFit = () => {
  try {
    fitAddon.fit();
  } catch {
    /* 視窗最小化時 fit 會失敗，忽略 */
  }
};
window.addEventListener('resize', () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(doFit, 30);
});
new ResizeObserver(() => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(doFit, 30);
}).observe(document.getElementById('term-body'));

term.focus();
