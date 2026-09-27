// `session_create` 的每個參數，`bridge.js` 的 createSession 都要真的傳出去。
//
// 為什麼要有這支腳本：**同一個 bug 已經發生兩次**。
//   - TASK-011：漏傳 `com` → 每個連接埠分頁都靜靜用設定裡上次的埠，選了別的沒有作用。
//   - TASK-021：漏傳 `adb` → 多台裝置時選好的序號被丟掉，`adb shell` 直接失敗。
// 兩次都是「呼叫端有給、bridge 沒往下傳」，Rust 收到 `None` 就走預設值，
// 所以**不會報錯**，只是行為不對——編譯器與型別都抓不到。
//
// 這裡用最笨但最可靠的方式：讀 `commands.rs` 裡 `session_create` 的參數名單，
// 再確認 `bridge.js` 的 invoke 物件裡每個都出現過。
//
//   node scripts/test-bridge-args.mjs

import { readFileSync } from 'node:fs';

const RUST = 'src-tauri/src/commands.rs';
const JS = 'src/bridge.js';

/** `session_create` 的參數裡，這些不是「呼叫端要給的資料」。 */
const NOT_DATA = new Set([
  'app', // AppHandle
  'cols',
  'rows', // bridge 用自己記的 lastSize
  'on_event', // Channel（bridge 自己建）
  'manager',
  'tabs_state',
  'settings',
  'teams', // State<…>
]);

function snakeToCamel(s) {
  return s.replace(/_([a-z])/g, (_, c) => c.toUpperCase());
}

const rust = readFileSync(RUST, 'utf8');
const start = rust.indexOf('pub fn session_create(');
if (start < 0) throw new Error(`${RUST} 裡找不到 session_create`);
const open = rust.indexOf('(', start);
// 參數清單到對應的 `)` 為止（巢狀的 `<…>` 不含括號，所以數 ( ) 就夠）
let depth = 0;
let end = open;
for (let i = open; i < rust.length; i++) {
  if (rust[i] === '(') depth++;
  else if (rust[i] === ')') {
    depth--;
    if (depth === 0) {
      end = i;
      break;
    }
  }
}
const params = rust
  .slice(open + 1, end)
  .split('\n')
  .map((l) => l.replace(/\/\/.*$/, '').trim())
  .filter(Boolean)
  .map((l) => l.split(':')[0].trim())
  .filter((n) => /^[a-z_][a-z0-9_]*$/.test(n))
  .filter((n) => !NOT_DATA.has(n));

const js = readFileSync(JS, 'utf8');
const at = js.indexOf("invoke('session_create'");
if (at < 0) throw new Error(`${JS} 裡找不到 invoke('session_create')`);
const body = js.slice(at, js.indexOf('});', at));

const missing = params.filter((p) => {
  const camel = snakeToCamel(p);
  return !new RegExp(`\\b${camel}\\s*:`).test(body);
});

console.log(`session_create 的資料參數 ${params.length} 個：${params.join('、')}`);
if (missing.length) {
  console.log(`\n❌ bridge.js 的 createSession 沒有傳：${missing.join('、')}`);
  console.log('   加進 invoke 的物件裡（Rust 收到 None 只會安靜地用預設值，不會報錯）。');
  console.log('\nRESULT: FAIL');
  process.exit(1);
}
console.log('\nRESULT: PASS');
