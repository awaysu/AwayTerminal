// 掃「用了 `el.xxx` 但 init 裡沒有指定」與「`$('id')` 在 index.html 根本不存在」（TASK-028）。
//
// 為什麼需要它：前端模組都是 `const el = {}` 再在 init 裡一個一個 `el.x = $('id')`。
// 漏掉一行的話**沒有任何工具會抱怨**——`eslint` 的 `no-undef` 只看識別字，看不到物件屬性；
// TypeScript 沒用；Vite 照樣打包。要等使用者按下去才炸，而且常常被 try/catch 吃掉，
// 變成「按了完全沒反應」。實際案例：`setdlg.js` 少了 `el.shellMenu = $('st-shellmenu')`，
// 「其他設定」從 TASK-016 打不開到 TASK-027，中間十幾次 `--verify` 全綠。
//
// 這是**文字比對**，不是型別檢查：只認 `el.x = ...` 這種直接指定。如果哪天有模組改成
// 用迴圈或解構填 `el`，這裡會誤報——那就在下面的 KNOWN 加一條並寫清楚原因。
import { readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const html = readFileSync(join(root, 'index.html'), 'utf8');
const htmlIds = new Set([...html.matchAll(/id="([^"]+)"/g)].map((m) => m[1]));

/** 已知的例外（key＝檔名，值＝允許的 `el.*` 名字）。目前是空的，加的時候要寫原因。 */
const KNOWN = {};

// `terminal.js` 是從舊版原封不動搬過來的（`CLAUDE.md`：不動它），不掃。
const files = readdirSync(join(root, 'src'))
  .filter((f) => f.endsWith('.js') && f !== 'terminal.js')
  .sort();

let problems = 0;
for (const name of files) {
  const src = readFileSync(join(root, 'src', name), 'utf8');
  if (!/\bel\./.test(src)) continue;
  const assigned = new Set([...src.matchAll(/\bel\.(\w+)\s*=(?!=)/g)].map((m) => m[1]));
  const allowed = new Set(KNOWN[name] || []);
  const missing = [...new Set([...src.matchAll(/\bel\.(\w+)\b/g)].map((m) => m[1]))].filter(
    (k) => !assigned.has(k) && !allowed.has(k)
  );
  const ids = [
    ...[...src.matchAll(/\$\('([^']+)'\)/g)].map((m) => m[1]),
    ...[...src.matchAll(/getElementById\('([^']+)'\)/g)].map((m) => m[1]),
    ...[...src.matchAll(/querySelector\('#([\w-]+)'\)/g)].map((m) => m[1]),
  ];
  const badIds = [...new Set(ids)].filter((id) => !htmlIds.has(id));
  if (missing.length || badIds.length) {
    problems += missing.length + badIds.length;
    console.log(`src/${name}`);
    if (missing.length) console.log(`  用了但 init 沒指定的 el.*：${missing.join('、')}`);
    if (badIds.length) console.log(`  index.html 沒有這些 id：${badIds.join('、')}`);
  }
}

console.log(`掃了 ${files.length} 個模組（terminal.js 除外）`);
console.log(problems === 0 ? 'RESULT: PASS' : `RESULT: FAIL（${problems} 個）`);
process.exit(problems === 0 ? 0 : 1);
