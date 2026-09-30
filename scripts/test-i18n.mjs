// 八種語言的字串表檢查：`node scripts/test-i18n.mjs`
//
// **每個任務都要跑這一支**（PM 在 TASK-015 修訂版定的）：新加的字串沒補齊八種語言
// 就算沒做完。檢查項目：
//
//   1. 每種語言的 key 是不是和主語言（zh-TW）一樣齊 —— 缺的逐條印出來
//   2. 有沒有空字串（`''` 會讓畫面變空白，比缺 key 更難發現）
//   3. 有沒有主語言沒有的多餘 key（通常是拼錯）
//   4. `{0}`／`{1}` 這些參數編號，每種語言都要和主語言一致（不然換語言就掉參數）
//   5. Rust 端會送給前端查表的**代碼**，前端表裡一定要有（不然使用者會看到 `err.xxx`）；
//      反過來，Rust 程式碼 `t("…")`／`tf("…")` 用到的字面 key，i18n.rs 的 TABLE 也一定要有
//   6. 各語言的長度：太長的字串會把工具列／按鈕撐爆 —— 只提醒，不當失敗
//
// 退回鏈是「選的語言 → en → zh-TW」，所以缺 key 不會顯示 undefined；
// 但那代表使用者看到的是別的語言，仍然算沒做完。

import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const { LANGS, BASE_LANG, allKeys, tableOf } = await import('../src/strings.js');

let fail = 0;
let warn = 0;
const baseKeys = allKeys();
const base = tableOf(BASE_LANG);

/** 字串裡出現的參數編號集合（`{0}`、`{12}`…）。 */
function placeholders(s) {
  return new Set([...String(s).matchAll(/\{(\d+)\}/g)].map((m) => `{${m[1]}}`));
}

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
    // 參數集合直接從字串抓（`{0}`…`{N}`，編號不設上限——原本只查 0~3，字串表早就用到 {4}）。
    // 兩個方向都比：主語言有、這語言沒有（掉參數）；這語言有、主語言沒有（會印出字面 `{5}`）。
    const want = placeholders(base[k]);
    const got = placeholders(table[k]);
    for (const p of want) if (!got.has(p)) badArgs.push(`${k} 少 ${p}`);
    for (const p of got) if (!want.has(p)) badArgs.push(`${k} 多 ${p}`);
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

// ---- 5b：Rust 程式碼用到的 key，TABLE 裡一定要有 ----
// 前端只推 TABLE 裡有的 key（`i18n_push`）。程式碼 `t("x")` 的 x 不在 TABLE：
// release 直接回字面 key（例如把 `exit.mdPrompt` 當文字打進 Claude Code），
// debug 在 `debug_assert!` panic（稽核 D3）。這裡掃 `src-tauri/src/**/*.rs` 裡
// **字面** key 的呼叫：`crate::i18n::t("…")`／`i18n::tf("…")`，以及 `use crate::i18n::{t, tf}`
// 的檔案裡不帶前綴的 `t("…")`／`tf("…")`。key 是變數或 `if` 運算式的呼叫抓不到（只能靠 debug_assert）。
{
  const rustTable = new Set(rustKeys);
  const files = [];
  const walk = (dir) => {
    for (const ent of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, ent.name);
      if (ent.isDirectory()) walk(p);
      else if (ent.name.endsWith('.rs')) files.push(p);
    }
  };
  walk(join('src-tauri', 'src'));
  const missingUse = [];
  let calls = 0;
  for (const f of files) {
    const raw = readFileSync(f, 'utf8');
    // 去掉 `//` 註解（文件註解裡的範例不算），保留行數好報行號
    const text = raw.replace(/\/\/.*$/gm, '');
    const imported = /use\s+crate::i18n::\{?[^;]*\btf?\b/.test(text);
    const re = imported
      ? /(?<![\w.])(?:crate::i18n::|i18n::)?(tf?)\(\s*"([^"\\]*)"/g
      : /(?:crate::i18n::|\bi18n::)(tf?)\(\s*"([^"\\]*)"/g;
    for (const m of text.matchAll(re)) {
      calls++;
      if (rustTable.has(m[2])) continue;
      const line = text.slice(0, m.index).split('\n').length;
      missingUse.push(`${f.replace(/\\/g, '/')}:${line} ${m[1]}("${m[2]}")`);
    }
  }
  console.log(`Rust 程式碼裡的字面 key 呼叫：${calls} 處`);
  if (!calls) {
    fail++;
    console.log('FAIL  一處 t()/tf() 呼叫都抓不到（掃描的寫法跟不上程式碼了？）');
  }
  if (missingUse.length) {
    fail++;
    console.log('FAIL  程式碼用到、但 i18n.rs 的 TABLE 裡沒有的 key（release 會顯示字面 key、debug 會 panic）：');
    for (const s of missingUse) console.log(`        ${s}`);
  }
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
