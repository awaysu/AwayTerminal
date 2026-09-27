// `npm run tauri dev -- -- --verify N` 的包裝：**逾時／被中斷時把整棵行程樹收掉**。
//
// 為什麼需要它（TASK-017 的教訓）：直接用 `timeout` 之類的東西包 `npx tauri dev`，
// 逾時只會砍最外面那一層，留下
//   npx → @tauri-apps/cli → (vite 佔著 1420) 與 target\debug\awayterminal.exe → OpenConsole.exe
// 一整串孤兒；`awayterminal.exe` 抓著 `target\debug`，下一次 `cargo build` 就會噴
// `os error 32`（PM 在 TASK-018 的信裡就是被這個擋住）。
//
// 做法：把 tauri dev 開成子行程、記下**它的 PID**，收尾時用 `taskkill /PID <pid> /T /F`
// （**依 PID 收整棵樹，不是依名稱**——依名稱會把使用者的 AwayTerminal 和這個團隊一起砍掉）。
// 另外把 `%TEMP%` 底下自己留下的驗證資料夾清掉。
//
// 用法：
//   node scripts/dev-verify.mjs [分頁數=2] [逾時秒=600] [--release]
//
// `--release` ＝**不跑 dev，直接跑已經 build 好的 release exe**
// （`src-tauri/target/release/AwayTerminal.exe --verify N`）。用途是確認 release 與 dev
// 行為一致：前端是內嵌的（不經 vite／localhost:1420）、conpty 走安裝後的相對路徑、
// 資源檔要真的在。發佈前一定要跑這一種（`docs/RELEASE.md` 的檢查清單）。
//
// 輸出直接透傳，結束時印一行收尾記錄。

import { spawn, execFileSync } from 'node:child_process';
import { existsSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const release = args.includes('--release');
const positional = args.filter((a) => !a.startsWith('--'));
const tabs = positional[0] || '2';
const timeoutSec = Number(positional[1] || 600);
const isWindows = process.platform === 'win32';

/** `%TEMP%` 底下由 `--verify` 產生的資料夾（名字固定前綴，只刪這些）。 */
const TEMP_PREFIXES = [
  'awayterm-verify-team-',
  'awayterm-agent-probe-',
  'awayterm-chat-probe-',
];

function cleanTemp() {
  const dir = tmpdir();
  const removed = [];
  let names = [];
  try {
    names = readdirSync(dir);
  } catch {
    return removed;
  }
  for (const name of names) {
    if (!TEMP_PREFIXES.some((p) => name.startsWith(p))) continue;
    try {
      rmSync(join(dir, name), { recursive: true, force: true });
      removed.push(name);
    } catch (e) {
      console.log(`[dev-verify] 刪不掉 ${name}：${e.message}`);
    }
  }
  return removed;
}

/** 依 PID 收掉整棵行程樹。**絕不依名稱。** */
function killTree(pid) {
  if (!pid) return;
  try {
    if (isWindows) {
      // /T ＝連子孫一起；/F ＝強制。只認 PID。
      execFileSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore' });
    } else {
      // 子行程是 process group leader（detached），負號＝整組
      process.kill(-pid, 'SIGKILL');
    }
    console.log(`[dev-verify] 已收掉行程樹 PID ${pid}`);
  } catch {
    // 已經自己結束了就沒事
  }
}

const what = release ? 'release exe' : 'dev';
console.log(
  `[AwayTerminal] 開始跑 ${what}（--verify ${tabs}）：視窗會開起來、跑完自己關掉，最多 ${timeoutSec} 秒`
);

// 直接用 node 跑本地的 tauri CLI，**不經過 `npx.cmd`**：
// Node 20.12 起（CVE-2024-27980 的修正）在 Windows 上 spawn `.cmd` 會直接 `EINVAL`，
// 除非開 `shell: true`——而開了 shell 就多一層 cmd.exe，PID 也變成那層的，樹反而更難收。
const cli = join(process.cwd(), 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
const exe = join(
  process.cwd(),
  'src-tauri',
  'target',
  'release',
  isWindows ? 'AwayTerminal.exe' : 'AwayTerminal'
);
if (release && !existsSync(exe)) {
  console.log(`[dev-verify] 找不到 ${exe}——先跑 \`npm run tauri build\``);
  process.exit(1);
}
const child = release
  ? spawn(exe, ['--verify', tabs], { stdio: 'inherit', detached: !isWindows })
  : spawn(process.execPath, [cli, 'dev', '--', '--', '--verify', tabs], {
      stdio: 'inherit',
      detached: !isWindows,
    });

let timedOut = false;
const timer = setTimeout(() => {
  timedOut = true;
  console.log(`[dev-verify] 超過 ${timeoutSec} 秒，收掉整棵行程樹`);
  killTree(child.pid);
}, timeoutSec * 1000);

// Ctrl+C／被上層砍掉時也要收乾淨
for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(sig, () => {
    console.log(`[dev-verify] 收到 ${sig}，收掉整棵行程樹`);
    clearTimeout(timer);
    killTree(child.pid);
    process.exit(130);
  });
}

child.on('exit', (code) => {
  clearTimeout(timer);
  // 正常結束也要確認一次：tauri dev 的 watcher 可能已經重新啟動過 app，
  // 那個新的 app 行程不是 npx 的直接子行程，但仍在同一棵樹裡
  killTree(child.pid);
  const removed = cleanTemp();
  console.log(
    `[AwayTerminal] ${what} 結束（exit ${timedOut ? 'timeout' : code}）` +
      `；清掉 %TEMP% 驗證資料夾 ${removed.length} 個${removed.length ? '：' + removed.join(', ') : ''}`
  );
  process.exit(timedOut ? 124 : (code ?? 1));
});
