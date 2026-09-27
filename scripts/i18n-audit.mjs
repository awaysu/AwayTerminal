// i18n 稽核：`node scripts/i18n-audit.mjs`
//
// TASK-015 B 要求「列出 Rust 端所有使用者可見字串的清單，確認沒漏」。
// 這支就是那份清單的產生器**兼**回歸測試：掃 `src-tauri/src/**/*.rs`，
// 找出還有中文的字串字面，逐條判斷它屬於哪一類；出現「沒歸類的」就 exit 1。
//
// 分類規則（和 `src-tauri/src/i18n.rs` 最上面那張表一致）：
//
//   table   `i18n.rs` 的字串表本身（那裡就是要放中文的地方）
//   errmsg  `ttl/error.rs` 的 `message_zh()`（原碼的 22 條錯誤訊息，另有英文版）
//   log     `println!(...)`：開發診斷，打包後沒有 stdout，**刻意不翻**
//   allow   下面 ALLOW 列的例外（寫進檔案的內容、字型名稱、只有 --verify 跑得到的）
//   MISSED  以上都不是 → 使用者可能看得到卻沒進字串表，要處理
//
// 輸出的統計表可以直接貼進 docs/SETTINGS.md。

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = 'src-tauri/src';
const CJK = /[一-鿿]/;
const LITERAL = /"((?:[^"\\]|\\.)*)"/g;

/** 例外：`檔案:關鍵字` → 為什麼不必翻。 */
const ALLOW = [
  ['src/settings.rs', 'Microsoft JhengHei', '字型 fallback 清單，是字型名稱不是句子'],
  ['src/sandbox.rs', 'AwayTerminal 沙盒模式（自動加入', '寫進 .git/info/exclude 的註解，不是介面文字'],
  ['src/ttl/execverify.rs', '', '只有 --verify 跑得到（測試碼）'],
  ['src/ttl/runner.rs', '執行完畢', 'macro_verify 的回傳值，只有 --verify 用'],
  ['src/ttl/runner.rs', '還沒結束（已中斷）', '同上'],
  ['src/compose.rs', '第二行', 'compose_verify_roundtrip 的測試資料'],
  ['src/update.rs', '假伺服器', 'update_verify 的訊息，只有 --verify 跑得到'],
];

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (name.endsWith('.rs')) out.push(p);
  }
  return out;
}

const counts = { table: 0, errmsg: 0, log: 0, allow: 0 };
const missed = [];
const perFile = new Map();

for (const file of walk(ROOT)) {
  const rel = file.replace(/\\/g, '/');
  const lines = readFileSync(file, 'utf8').split('\n');
  let inTest = false;
  // `println!(` 常常跨行（字串在下一行），所以要記住「還在 println! 的括號裡」，
  // 不然那些診斷訊息會被當成沒歸類的（第一版就是這樣，7 條假警報）。
  let printlnDepth = 0;
  lines.forEach((line, i) => {
    if (/^\s*#\[cfg\(test\)\]/.test(line)) inTest = true;
    if (inTest) return;
    const trimmed = line.trim();
    if (trimmed.startsWith('//')) return;
    const inPrintln = printlnDepth > 0;
    if (printlnDepth > 0 || line.includes('println!(')) {
      const from = printlnDepth > 0 ? 0 : line.indexOf('println!(') + 'println!'.length;
      for (const ch of line.slice(from)) {
        if (ch === '(') printlnDepth++;
        else if (ch === ')') printlnDepth = Math.max(0, printlnDepth - 1);
      }
    }
    for (const m of line.matchAll(LITERAL)) {
      const text = m[1];
      if (!CJK.test(text)) continue;
      let kind;
      if (rel.endsWith('/i18n.rs')) kind = 'table';
      else if (rel.endsWith('/ttl/error.rs')) kind = 'errmsg';
      else if (inPrintln || line.includes('println!')) kind = 'log';
      else if (ALLOW.some(([f, needle]) => rel.endsWith(f.replace('src/', 'src/')) && (needle === '' || text.includes(needle)))) kind = 'allow';
      else kind = 'missed';

      if (kind === 'missed') missed.push({ rel, line: i + 1, text });
      else counts[kind]++;
      const key = `${rel}|${kind}`;
      perFile.set(key, (perFile.get(key) ?? 0) + 1);
    }
  });
}

console.log('含中文的字串字面（跳過註解與 #[cfg(test)] 之後）：');
console.log(`  字串表（i18n.rs）        ${counts.table}`);
console.log(`  TTL 錯誤訊息（有英文版） ${counts.errmsg}`);
console.log(`  開發診斷 println!        ${counts.log}`);
console.log(`  例外（見 ALLOW）         ${counts.allow}`);
console.log(`  沒歸類的                 ${missed.length}`);

if (missed.length) {
  console.log('\n以下字串使用者可能看得到，但沒進 i18n 字串表：');
  for (const m of missed) console.log(`  ${m.rel}:${m.line}  ${m.text.slice(0, 80)}`);
  console.log('\n處理方式：加進 i18n.rs 的表並改成 t()/tf()，或加進這支腳本的 ALLOW 並寫原因。');
}

console.log('\n逐檔（只列非 table 的）：');
for (const [key, n] of [...perFile.entries()].sort((a, b) => b[1] - a[1])) {
  const [rel, kind] = key.split('|');
  if (kind === 'table') continue;
  console.log(`  ${String(n).padStart(3)}  ${kind.padEnd(7)} ${rel}`);
}

console.log(`\nRESULT: ${missed.length === 0 ? 'PASS' : `FAIL（${missed.length} 條沒歸類）`}`);
process.exit(missed.length === 0 ? 0 : 1);
