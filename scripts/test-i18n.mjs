// 八種語言的字串表檢查：`node scripts/test-i18n.mjs`
//
// **每個任務都要跑這一支**（PM 在 TASK-015 修訂版定的）：新加的字串沒補齊八種語言
// 就算沒做完。檢查項目：
//
//   1. 每種語言的 key 是不是和主語言（zh-TW）一樣齊 —— 缺的逐條印出來
//   2. 有沒有空字串（`''` 會讓畫面變空白，比缺 key 更難發現）
//   3. 有沒有主語言沒有的多餘 key（通常是拼錯）
//   4. `{0}`／`{1}` 這些參數編號，每種語言都要和主語言一致（不然換語言就掉參數）
//   5. Rust 端會送給前端查表的**代碼**，前端表裡一定要有（不然使用者會看到 `err.xxx`）
//   6. 各語言的長度：太長的字串會把工具列／按鈕撐爆 —— 只提醒，不當失敗
//
// 退回鏈是「選的語言 → en → zh-TW」，所以缺 key 不會顯示 undefined；
// 但那代表使用者看到的是別的語言，仍然算沒做完。

import { readFileSync } from 'node:fs';

const { LANGS, BASE_LANG, allKeys, tableOf } = await import('../src/strings.js');

let fail = 0;
let warn = 0;
const baseKeys = allKeys();
const base = tableOf(BASE_LANG);

console.log(`主語言 ${BASE_LANG}：${baseKeys.length} 個 key\n`);

// ---- 1~4：逐語言比對 ----
const rows = [];
for (const { code, name } of LANGS) {
  const table = tableOf(code);
  const keys = Object.keys(table);
  const missing = baseKeys.filter((k) => !(k in table));
  const empty = keys.filter((k) => typeof table[k] !== 'string' || table[k] === '');
  const extra = keys.filter((k) => !baseKeys.includes(k));
  const badArgs = [];
  for (const k of baseKeys) {
    if (!(k in table)) continue;
    for (let i = 0; i < 4; i++) {
      const p = `{${i}}`;
      if (String(base[k]).includes(p) !== String(table[k]).includes(p)) badArgs.push(`${k} ${p}`);
    }
  }
  rows.push({ code, name, count: keys.length, missing, empty, extra, badArgs });
}

console.log('語言      字串數  缺漏  空字串  多餘  參數不符');
for (const r of rows) {
  console.log(
    `${r.code.padEnd(8)} ${String(r.count).padStart(6)} ${String(r.missing.length).padStart(5)} ` +
      `${String(r.empty.length).padStart(7)} ${String(r.extra.length).padStart(5)} ${String(r.badArgs.length).padStart(9)}`,
  );
}
console.log('');

for (const r of rows) {
  if (r.missing.length) {
    fail++;
    console.log(`FAIL  ${r.code} 缺 ${r.missing.length} 個 key：`);
    for (const k of r.missing) console.log(`        ${k}  ＝ ${JSON.stringify(base[k]).slice(0, 60)}`);
  }
  if (r.empty.length) {
    fail++;
    console.log(`FAIL  ${r.code} 有空字串：${r.empty.join(', ')}`);
  }
  if (r.extra.length) {
    fail++;
    console.log(`FAIL  ${r.code} 有主語言沒有的 key（拼錯？）：${r.extra.join(', ')}`);
  }
  if (r.badArgs.length) {
    fail++;
    console.log(`FAIL  ${r.code} 的參數編號和主語言不一致：${r.badArgs.join(', ')}`);
  }
}

// ---- 5：Rust 端需要的 key，前端表裡一定要有 ----
// Rust 不存八種語言：前端啟動與切語言時把「Rust 會用到的那幾十條」推過去
// （`i18n_push`，理由見 src-tauri/src/i18n.rs）。少一條就會退回內建的繁中／英文，
// 使用者會看到夾雜的語言 → 這裡當失敗。
const rustKeys = [];
{
  const src = readFileSync('src-tauri/src/i18n.rs', 'utf8');
  const table = src.slice(src.indexOf('static TABLE'));
  for (const m of table.matchAll(/^\s*\("([\w.]+)",/gm)) rustKeys.push(m[1]);
}
const unknownCodes = rustKeys.filter((c) => !baseKeys.includes(c));
console.log(`
Rust 端會用到的 key：${rustKeys.length} 個`);
if (!rustKeys.length) {
  fail++;
  console.log('FAIL  抓不到 Rust 的字串表（i18n.rs 的 TABLE 改了格式？）');
}
if (unknownCodes.length) {
  fail++;
  console.log(`FAIL  這些 key 前端表裡沒有（Rust 會退回內建的繁中／英文）：${unknownCodes.join(', ')}`);
}

// ---- 6：長度提醒（德文／法文最容易爆版）----
const TIGHT = [
  'tb.new', 'tb.copy', 'tb.paste', 'tb.copyall', 'tb.clear', 'tb.page', 'tb.compose',
  'tb.settings', 'tb.about', 'tb.favorites', 'tb.split', 'tb.tabs', 'tb.columns',
];
const LIMIT = 22; // 工具列按鈕的文字超過這個長度就提醒（CSS 允許橫向捲，但會不好看）
for (const { code } of LANGS) {
  const table = tableOf(code);
  const longOnes = TIGHT.filter((k) => table[k] && table[k].length > LIMIT);
  if (longOnes.length) {
    warn++;
    console.log(
      `WARN  ${code} 的工具列文字偏長（>${LIMIT}）：` +
        longOnes.map((k) => `${k}="${table[k]}"(${table[k].length})`).join('、'),
    );
  }
}

console.log(`\nRESULT: ${fail === 0 ? 'PASS' : `FAIL（${fail} 項）`}${warn ? `，${warn} 個提醒` : ''}`);
process.exit(fail === 0 ? 0 : 1);
