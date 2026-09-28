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
 * **只認路徑**（這個 repo 的 target 目錄），所以絕不會碰到使用者安裝在
 * `C:\Program Files\AwayTerminal` 的舊版；找到的一律依 PID 收掉。
 * 回傳收掉的 PID 清單（空的＝乾淨）。
 */
function sweepStrays() {
  if (!isWindows) return []; // mac/Linux 用 process group（detached），這裡不重複做
  const target = join(process.cwd(), 'src-tauri', 'target');
  let out = '';
  try {
    out = execFileSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -like '${target}\\*' } | ForEach-Object { $_.ProcessId }`,
      ],
      { encoding: 'utf8' }
    );
  } catch {
    return [];
  }
  const pids = out.split(/\s+/).map((x) => Number(x)).filter((x) => x > 0);
  for (const pid of pids) {
    try {
      execFileSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore' });
    } catch {
      // 已經自己結束了
    }
  }
  return pids;
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
let done = false;
let tail = '';
const watch = (buf) => {
  if (done) return;
  tail = (tail + buf).slice(-4000);
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
  const strays = sweepStrays();
  const removed = cleanTemp();
  console.log(
    `[AwayTerminal] ${what} 結束（exit ${timedOut ? 'timeout' : code}）` +
      `；清掉 %TEMP% 驗證資料夾 ${removed.length} 個${removed.length ? '：' + removed.join(', ') : ''}`
  );
  console.log(
    strays.length
      ? `[dev-verify] 收尾掃描：target 底下還有 ${strays.length} 隻，已依 PID 收掉（${strays.join(', ')}）`
      : '[dev-verify] 收尾掃描：target 底下沒有殘留行程'
  );
  process.exit(timedOut ? 124 : (code ?? 1));
});
