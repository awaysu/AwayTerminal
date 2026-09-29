// `npm run tauri dev -- -- --verify N` 的包裝：**逾時／被中斷時把整棵行程樹收掉**。
//
// 為什麼需要它（TASK-017 的教訓）：直接用 `timeout` 之類的東西包 `npx tauri dev`，
// 逾時只會砍最外面那一層，留下
//   npx → @tauri-apps/cli → (vite 佔著 1420) 與 target\debug\awayterminal.exe → OpenConsole.exe
// 一整串孤兒；`awayterminal.exe` 抓著 `target\debug`，下一次 `cargo build` 就會噴
// `os error 32`（PM 在 TASK-018 的信裡就是被這個擋住）。
//
// `--verify` 跑完之後 app **不會自己結束**（它就是一個開著的視窗），所以這支腳本還要負責
// 「驗完就收工」：把子行程的輸出接起來看，看到最後一行收尾就收樹，不必等到逾時
//（TASK-029：release 模式留下 exe 與 node 各一隻活到天亮，PM 得自己依 PID 收）。
// 收完再**掃一次** `src-tauri/target/{debug,release}` 底下還有沒有活著的行程，有就依 PID 補收並印出來。
//
// 做法：把 tauri dev 開成子行程、記下**它的 PID**，收尾時用 `taskkill /PID <pid> /T /F`
// （**依 PID 收整棵樹，不是依名稱**——依名稱會把使用者的 AwayTerminal 和這個團隊一起砍掉）。
// 另外把 `%TEMP%` 底下自己留下的驗證資料夾清掉。
//
// ⚠️ **只收自己啟動的東西**（TASK-034）。原本的收尾掃描是「`src-tauri\target\` 底下的
// 行程一律依 PID 收掉」——路徑條件擋得住使用者裝在 `C:\Program Files` 的 1.x，卻擋不住
// **同一個 repo 裡的另一個 agent**：2026-09-29 Agent-22 的 verify 就這樣把 Agent-21 開著
// 給使用者看的 `npm run tauri dev`（PID 16556）收掉了。現在：
//   * 啟動前先看 1420 有沒有人在用、target 底下有沒有別人的行程 → 有就**直接中止**，一個都不碰；
//   * 收尾只收「自己這棵樹」（`killTree` 動手前先記下子孫 PID）與自己建立的 `%TEMP%` 資料夾；
//   * 掃到其他 target 行程只**印警告**（附 PID、路徑、啟動時間與可以自己下的 taskkill 指令），
//     絕不自動砍。
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

/**
 * 「這些是我啟動的」——收尾掃描只會動這裡面的 PID。
 *
 * 每次 `killTree()` 動手**之前**先把那棵樹的子孫記進來：`taskkill /T` 之後父子關係就沒了，
 * 事後再問就認不出誰是誰（tauri dev 的 watcher 重啟過 app 時尤其明顯）。
 */
const ourPids = new Set();

/** `pid` 的所有子孫（含自己）。只在 Windows 上用得到。 */
function collectDescendants(pid) {
  const out = new Set([Number(pid)]);
  if (!isWindows) return out;
  let text = '';
  try {
    text = execFileSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        'Get-CimInstance Win32_Process | ForEach-Object { "$($_.ProcessId) $($_.ParentProcessId)" }',
      ],
      { encoding: 'utf8' }
    );
  } catch {
    return out;
  }
  const parents = new Map();
  for (const line of text.split(/\r?\n/)) {
    const [a, b] = line.trim().split(/\s+/).map(Number);
    if (a > 0) parents.set(a, b);
  }
  // 廣度優先展開（行程數量頂多幾百，直接掃）
  let grew = true;
  while (grew) {
    grew = false;
    for (const [child, parent] of parents) {
      if (out.has(parent) && !out.has(child)) {
        out.add(child);
        grew = true;
      }
    }
  }
  return out;
}

/** 依 PID 收掉整棵行程樹。**絕不依名稱，也絕不收不是自己啟動的樹。** */
function killTree(pid) {
  if (!pid) return;
  for (const p of collectDescendants(pid)) ourPids.add(p);
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

/** `src-tauri/target/{debug,release}` 底下活著的行程（PID、路徑、啟動時間）。 */
function listTargetProcesses() {
  if (!isWindows) return []; // mac/Linux 用 process group（detached），這裡不重複做
  const target = join(process.cwd(), 'src-tauri', 'target');
  let out = '';
  try {
    out = execFileSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -like '${target}\\*' } | ` +
          'ForEach-Object { "$($_.ProcessId)`t$($_.CreationDate.ToString(\'HH:mm:ss\'))`t$($_.ExecutablePath)" }',
      ],
      { encoding: 'utf8' }
    );
  } catch {
    return [];
  }
  const rows = [];
  for (const line of out.split(/\r?\n/)) {
    const [pid, started, ...rest] = line.trim().split('\t');
    const n = Number(pid);
    if (n > 0) rows.push({ pid: n, started: started || '?', path: rest.join('\t') });
  }
  return rows;
}

/**
 * 啟動前的安全檢查：**別人正在跑就直接中止，一個行程都不碰。**
 *
 * 兩種「別人」：同 repo 的另一個 agent（他的 exe 也在 `target\` 底下）、
 * 以及任何佔著 vite 1420 的東西。撞上了硬跑下去只會兩敗俱傷
 *（`beforeDevCommand` 失敗 → 什麼都沒驗到，收尾還把對方砍了）。
 */
function preflight() {
  const running = listTargetProcesses();
  if (running.length) {
    console.log('[dev-verify] 中止：這個 repo 的 target 底下已經有行程在跑（不是我啟動的，我不會碰它）：');
    for (const r of running) console.log(`[dev-verify]   PID ${r.pid}　啟動 ${r.started}　${r.path}`);
    console.log('[dev-verify] 可能是同一個團隊的另一個 agent 或使用者開著 dev。請先協調好再跑 verify。');
    process.exit(1);
  }
}

// 先跑 eslint（TASK-028）：`no-undef` 一秒就抓得到「用了但沒 import」，
// 那一類 bug 會一路活到使用者按下去為止（`setdlg.js` 的 `setToolLabel` 就是）。
// 跑五分鐘的驗證之前先擋掉，比跑完再回頭找便宜太多。
try {
  execFileSync(process.execPath, [join(process.cwd(), 'node_modules', 'eslint', 'bin', 'eslint.js'), '.'], {
    stdio: 'inherit',
  });
  console.log('[dev-verify] eslint 通過');
} catch {
  console.log('[dev-verify] eslint 有錯誤（上面那些），先修掉再跑驗證');
  process.exit(1);
}

/**
 * 收尾掃描：`src-tauri/target/{debug,release}` 底下還有沒有活著的行程。
 *
 * **只收自己啟動的那些**（`ourPids`＝`killTree` 動手前記下的子孫）。
 * 掃到別人的（同 repo 的另一個 agent、使用者自己開的 dev）**只印警告、不砍**——
 * 路徑條件擋得住 `C:\Program Files` 的 1.x，擋不住同一個 repo 裡的另一個人（TASK-034）。
 * 回傳 `{ killed, others }`。
 */
function sweepStrays() {
  const rows = listTargetProcesses();
  const killed = [];
  const others = [];
  for (const r of rows) {
    if (!ourPids.has(r.pid)) {
      others.push(r);
      continue;
    }
    try {
      execFileSync('taskkill', ['/PID', String(r.pid), '/T', '/F'], { stdio: 'ignore' });
      killed.push(r.pid);
    } catch {
      // 已經自己結束了
    }
  }
  return { killed, others };
}

// 別人正在跑就直接中止（TASK-034）。要在 spawn 之前，才不會白跑一輪又把對方收掉。
preflight();

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
// stdio 用 pipe 而不是 inherit：輸出照樣原封不動轉出去（下面的 write），
// 但這樣才看得到「驗證跑完了」那一行 → 可以當場收工，不必等逾時。
const child = release
  ? spawn(exe, ['--verify', tabs], { stdio: ['ignore', 'pipe', 'pipe'], detached: !isWindows })
  : spawn(process.execPath, [cli, 'dev', '--', '--', '--verify', tabs], {
      stdio: ['ignore', 'pipe', 'pipe'],
      detached: !isWindows,
    });

// 驗證的最後一行（`awayVerify` 的收尾）。看到它就代表「該驗的都驗完了」。
const DONE_MARK = '[verify] 收尾：整段沒人接住的例外';
// `beforeDevCommand`（vite）起不來的樣子。最常見的是 1420 被別人佔著——
// 那代表**有人正在跑 dev**，硬跑下去什麼都驗不到，而且收尾掃描還可能動到對方（TASK-034）。
const BOOT_FAIL = [
  'Port 1420 is already in use',
  'The "beforeDevCommand" terminated with a non-zero status code',
];
let done = false;
let bootFailed = false;
let tail = '';
const watch = (buf) => {
  if (done) return;
  tail = (tail + buf).slice(-4000);
  const hit = BOOT_FAIL.find((m) => tail.includes(m));
  if (hit && !bootFailed) {
    bootFailed = true;
    done = true;
    console.log(`[dev-verify] 中止：前置指令起不來（${hit}）`);
    console.log('[dev-verify] 1420 被佔住通常是有人正在跑 dev（同團隊的另一個 agent 或使用者）。');
    console.log('[dev-verify] 我只收自己啟動的行程，不會去動對方；請先協調好再跑一次。');
    clearTimeout(timer);
    killTree(child.pid);
    return;
  }
  if (!tail.includes(DONE_MARK)) return;
  done = true;
  // 收尾那一行之後還有幾行輸出（log 是非同步送出的），等一下再收
  setTimeout(() => {
    console.log('[dev-verify] 驗證跑完了，收掉整棵行程樹');
    clearTimeout(timer);
    killTree(child.pid);
  }, 1500);
};
for (const [stream, out] of [
  [child.stdout, process.stdout],
  [child.stderr, process.stderr],
]) {
  if (!stream) continue;
  stream.on('data', (b) => {
    out.write(b);
    watch(String(b));
  });
}

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
  const { killed, others } = sweepStrays();
  const removed = cleanTemp();
  console.log(
    `[AwayTerminal] ${what} 結束（exit ${timedOut ? 'timeout' : bootFailed ? 'boot-fail' : code}）` +
      `；清掉 %TEMP% 驗證資料夾 ${removed.length} 個${removed.length ? '：' + removed.join(', ') : ''}`
  );
  console.log(
    killed.length
      ? `[dev-verify] 收尾掃描：我自己啟動的還有 ${killed.length} 隻，已依 PID 收掉（${killed.join(', ')}）`
      : '[dev-verify] 收尾掃描：我自己啟動的行程都收乾淨了'
  );
  // 別人的：**只報告，不動手**（TASK-034）
  if (others.length) {
    console.log(`[dev-verify] ⚠ target 底下還有 ${others.length} 隻**不是我啟動的**，我不會碰：`);
    for (const r of others) console.log(`[dev-verify]     PID ${r.pid}　啟動 ${r.started}　${r.path}`);
    console.log('[dev-verify]     確定要收的話自己下：taskkill /PID <pid> /T /F');
  }
  process.exit(timedOut ? 124 : bootFailed ? 1 : (code ?? 1));
});
