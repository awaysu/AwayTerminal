// 原始碼裡不可以有「看不見的控制字元」：`node scripts/audit-control-bytes.mjs`
//
// 掃 `src/` 與 `src-tauri/src/` 底下所有文字原始碼（.js/.mjs/.rs/.css/.html/.json），
// 除了 Tab（0x09）、LF（0x0A）、CR（0x0D）以外，任何 0x00–0x1F 與 0x7F 都算失敗。
//
// 為什麼（2026-09-30 稽核 A9）：`commands.rs` 的 `n` 訊息分隔符曾經是**直接貼進原始碼的
// 0x1F 位元組**，同檔別處卻寫 `\x1f`。這種位元組 grep 找不到、review 看不到，
// 哪天編輯器或格式化工具把它吃掉，所有遠端分頁就壞了，而且沒人知道為什麼。
// 同一類的還有：
//   * 送給終端機的 Ctrl+C 寫成字面 0x03（要寫 `'\x03'`）；
//   * ANSI 顏色寫成字面 ESC（0x1B）——JS 與 Rust 都有 `\x1b` 可以寫，沒有理由用字面；
//   * 註解裡的 `%TEMP%\awayterm…` 被某個工具當成 `\a` 轉成 BEL（0x07）。
// 一律改成跳脫序列，**不設白名單**。
//
// 失敗時印出「檔案:行:欄 位元組」，方便直接跳過去改。

import { readFileSync, readdirSync } from 'node:fs';
import { join, extname } from 'node:path';

const ROOTS = ['src', join('src-tauri', 'src')];
const EXTS = new Set(['.js', '.mjs', '.rs', '.css', '.html', '.json']);
const NAMES = { 0: 'NUL', 3: 'ETX(Ctrl+C)', 7: 'BEL', 8: 'BS', 0x1b: 'ESC', 0x1f: 'US', 0x7f: 'DEL' };

const files = [];
const walk = (dir) => {
  let ents = [];
  try {
    ents = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of ents) {
    const p = join(dir, e.name);
    if (e.isDirectory()) walk(p);
    else if (EXTS.has(extname(e.name))) files.push(p);
  }
};
for (const r of ROOTS) walk(r);

const hits = [];
for (const f of files) {
  const buf = readFileSync(f);
  let line = 1;
  let col = 0;
  for (const b of buf) {
    if (b === 0x0a) {
      line++;
      col = 0;
      continue;
    }
    col++;
    if ((b < 0x20 && b !== 0x09 && b !== 0x0d) || b === 0x7f) {
      const hex = `0x${b.toString(16).padStart(2, '0').toUpperCase()}`;
      hits.push(`${f.replace(/\\/g, '/')}:${line}:${col}  ${hex}${NAMES[b] ? ` ${NAMES[b]}` : ''}`);
    }
  }
}

console.log(`掃了 ${files.length} 個檔案（${ROOTS.join('、')}）`);
if (hits.length) {
  console.log(`FAIL  ${hits.length} 個字面控制字元（改成 \\x.. 跳脫序列；欄位是位元組位置）：`);
  for (const h of hits) console.log(`        ${h}`);
  console.log('\nRESULT: FAIL');
  process.exit(1);
}
console.log('\nRESULT: PASS');
