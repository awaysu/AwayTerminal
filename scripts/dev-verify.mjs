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
// ⚠️ **PID 會被回收**（2026-09-30 稽核 I2／I3）。另一個 agent 正在 `cargo build` 時，
// 一個剛結束的 PID 幾秒內就可能被別人的行程拿走，所以「PID 對得上」不等於「是我的」：
//   * 子行程**已經結束之後不再對它的 PID 下 taskkill**（以前 exit handler 會 `taskkill /PID <child> /T`）；
//   * 認子孫時比對 `CreationDate`：子行程一定比父行程晚建立，而「已結束的根」的子行程
//     一定建立在它活著的那段時間內（`spawnedAt`～`exitedAt`）——對不上的就是 PID 被回收了；
//   * 記下的「我的行程」是 **(PID, 建立時間)** 一對，收尾掃描兩個都對得上才動手；
//   * `%TEMP%` 的驗證資料夾名字尾端是建立者的 PID（`agent/mod.rs`），只刪「是我的」
//     或「那個 PID 已經不在了」的——安裝版或另一個 worktree 正在跑的 `--verify` 不會被刪。
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
import { createConnection } from 'node:net';
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

/** 這個 PID 現在有沒有行程（`kill(pid, 0)` 只檢查、不送訊號；EPERM＝有、只是不是我們的）。 */
function pidAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === 'EPERM';
  }
}

/**
 * 清掉 `%TEMP%` 底下的驗證資料夾——**只清「是我的」或「建立者已經不在」的**（稽核 I3）。
 *
 * 資料夾名字尾端是建立者的 PID（`awayterm-verify-team-<pid>`，`agent/mod.rs`；
 * probe 也一樣）。那個 PID 還活著又不是我啟動的 → 可能是安裝版、另一個 worktree
 * 或另一個 agent 正在跑的 `--verify` → 不碰，只印出來。
 * 名字尾端沒有 PID 的（不是這幾支程式建的）一律不碰。
 */
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
    const m = /-(\d+)$/.exec(name);
    if (!m) continue;
    const owner = Number(m[1]);
    if (!ourPids.has(owner) && pidAlive(owner)) {
      console.log(`[dev-verify] 留著 ${name}：建立它的 PID ${owner} 還活著，而且不是我啟動的`);
      continue;
    }
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
 * 「這些是我啟動的」——收尾掃描只會動這裡面的行程。PID → 建立時間（Unix ms；拿不到是 0）。
 *
 * 每次收樹**之前**先把那棵樹的子孫記進來：`taskkill` 之後父子關係就沒了，
 * 事後再問就認不出誰是誰（tauri dev 的 watcher 重啟過 app 時尤其明顯）。
 * 記建立時間是因為 PID 會被回收：收尾時 PID 與建立時間**都**對得上才算同一隻。
 */
const ourPids = new Map();

/** 子行程 spawn 的時間、結束的時間（Unix ms）。用來判斷「PID 是不是被回收了」。 */
let spawnedAt = 0;
let exitedAt = 0;
/** 時鐘誤差的寬限（WMI 的 CreationDate 與 Date.now() 取樣點不同）。 */
const SLACK_MS = 2000;

/** 系統上所有行程：PID → { ppid, created }。只在 Windows 上用得到。 */
function snapshotProcesses() {
  const table = new Map();
  let text = '';
  try {
    text = execFileSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        'Get-CimInstance Win32_Process | ForEach-Object { ' +
          '$c = 0; if ($_.CreationDate) { $c = ([DateTimeOffset]$_.CreationDate).ToUnixTimeMilliseconds() }; ' +
          '"$($_.ProcessId) $($_.ParentProcessId) $c" }',
      ],
      { encoding: 'utf8' }
    );
  } catch {
    return table;
  }
  for (const line of text.split(/\r?\n/)) {
    const [pid, ppid, created] = line.trim().split(/\s+/).map(Number);
    if (pid > 0) table.set(pid, { ppid, created: created || 0 });
  }
  return table;
}

/**
 * 我 spawn 的那個根（`rootPid`）的子孫：PID → 建立時間。根還活著的話也包含根。
 *
 * 只靠 ParentProcessId 會認錯（稽核 I2）：父行程結束後它的 PID 可能被別人拿走，
 * 那個新行程的子行程 ParentProcessId 也是同一個數字。所以：
 *   * 根：還在的話建立時間要落在 spawn 之後，否則那是別人（PID 已被回收）→ 回傳空的；
 *   * 根的直接子行程：建立時間要在根活著的期間（`spawnedAt`～根結束／現在）；
 *   * 更下層：建立時間不可早於父行程（早於＝父的 PID 是回收來的）。
 */
function collectDescendants(rootPid) {
  const out = new Map();
  if (!isWindows || !rootPid) return out;
  const table = snapshotProcesses();
  const root = table.get(rootPid);
  const lo = spawnedAt - SLACK_MS;
  // 根已結束：exit 事件一定晚於真正結束，所以 exitedAt 本身就是上限，不加寬限
  //（加了寬限，回收那個 PID 的新行程剛生的子行程就可能被算進來）
  const hi = exitedAt || Date.now() + SLACK_MS;
  const rootAlive = !exitedAt && root && root.created >= lo;
  if (root && !rootAlive && !exitedAt) {
    // 還沒收到 exit，PID 上卻是一隻 spawn 之前就在的行程——不是我的，整棵都不碰
    return out;
  }
  if (rootAlive) out.set(rootPid, root.created);
  const inWindow = (c) => c.created && c.created >= lo && c.created <= hi;
  let grew = true;
  while (grew) {
    grew = false;
    for (const [pid, info] of table) {
      if (out.has(pid) || pid === rootPid) continue;
      let ok = false;
      if (info.ppid === rootPid) ok = inWindow(info);
      else if (out.has(info.ppid)) ok = info.created && info.created >= out.get(info.ppid);
      if (ok) {
        out.set(pid, info.created);
        grew = true;
      }
    }
  }
  return out;
}

/** 依 PID 逐一收掉（Windows）。**不用 /T**：`/T` 自己再依 ParentProcessId 找子孫，不看建立時間。 */
function killPids(pids) {
  const killed = [];
  for (const pid of pids) {
    try {
      execFileSync('taskkill', ['/PID', String(pid), '/F'], { stdio: 'ignore' });
      killed.push(pid);
    } catch {
      // 已經自己結束了
    }
  }
  return killed;
}

/**
 * 收掉我 spawn 的那棵樹。**絕不依名稱，也絕不收不是自己啟動的樹。**
 *
 * 子行程已經結束（`exitedAt`）之後**不再對它的 PID 下手**——那個 PID 可能已經是別人的
 *（稽核 I2）；只收「建立時間落在它活著期間」的遺孤。
 */
function killTree(pid) {
  if (!pid) return;
  if (!isWindows) {
    if (exitedAt) return; // 同上：結束後 process group 的號碼也可能被重用
    try {
      // 子行程是 process group leader（detached），負號＝整組
      process.kill(-pid, 'SIGKILL');
      console.log(`[dev-verify] 已收掉行程群組 ${pid}`);
    } catch {
      // 已經自己結束了就沒事
    }
    return;
  }
  // 收兩輪：收的時候 tauri dev 的 watcher 可能剛好又生出新的子行程
  let total = 0;
  for (let round = 0; round < 2; round++) {
    const tree = collectDescendants(pid);
    const todo = [...tree.keys()]; // 都是快照當下活著的
    if (!todo.length) break;
    for (const [p, c] of tree) ourPids.set(p, c);
    // 根先收（它不在了 watcher 就不會再重生 app），再收其他
    todo.sort((a, b) => (a === pid ? -1 : b === pid ? 1 : 0));
    total += killPids(todo).length;
  }
  if (total) console.log(`[dev-verify] 已收掉 PID ${pid} 那棵樹（${total} 隻）`);
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
          'ForEach-Object { $c = 0; if ($_.CreationDate) { $c = ([DateTimeOffset]$_.CreationDate).ToUnixTimeMilliseconds() }; ' +
          '"$($_.ProcessId)`t$c`t$($_.CreationDate.ToString(\'HH:mm:ss\'))`t$($_.ExecutablePath)" }',
      ],
      { encoding: 'utf8' }
    );
  } catch {
    return [];
  }
  const rows = [];
  for (const line of out.split(/\r?\n/)) {
    const [pid, created, started, ...rest] = line.trim().split('\t');
    const n = Number(pid);
    if (n > 0) rows.push({ pid: n, created: Number(created) || 0, started: started || '?', path: rest.join('\t') });
  }
  return rows;
}

/** 1420 有沒有人在聽（vite 的 `localhost` 可能綁 IPv4 或 IPv6，兩個都探）。 */
function portInUse(port) {
  const probe = (host) =>
    new Promise((resolve) => {
      const s = createConnection({ host, port });
      const done = (v) => {
        s.destroy();
        resolve(v);
      };
      s.setTimeout(1000, () => done(false));
      s.once('connect', () => done(true));
      s.once('error', () => done(false));
    });
  return Promise.all([probe('127.0.0.1'), probe('::1')]).then((r) => r.some(Boolean));
}

/**
 * 啟動前的安全檢查：**別人正在跑就直接中止，一個行程都不碰。**
 *
 * 兩種「別人」：同 repo 的另一個 agent（他的 exe 也在 `target\` 底下）、
 * 以及任何佔著 vite 1420 的東西。撞上了硬跑下去只會兩敗俱傷
 *（`beforeDevCommand` 失敗 → 什麼都沒驗到，收尾還把對方砍了）。
 */
async function preflight() {
  const running = listTargetProcesses();
  if (running.length) {
    console.log('[dev-verify] 中止：這個 repo 的 target 底下已經有行程在跑（不是我啟動的，我不會碰它）：');
    for (const r of running) console.log(`[dev-verify]   PID ${r.pid}　啟動 ${r.started}　${r.path}`);
    console.log('[dev-verify] 可能是同一個團隊的另一個 agent 或使用者開著 dev。請先協調好再跑 verify。');
    process.exit(1);
  }
  // dev 模式要用 vite 的 1420。**spawn 之前**就真的探一次（稽核 I6）——以前只靠 spawn 之後
  // 比對 vite 的錯誤字串，那時 tauri CLI 已經跑起來了。release 模式不經 vite，不用查。
  if (!release && (await portInUse(1420))) {
    console.log('[dev-verify] 中止：1420 已經有人在聽（通常是有人正在跑 dev：同團隊的另一個 agent 或使用者）。');
    console.log('[dev-verify] 我不會去動它；請先協調好再跑 verify。');
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
    // PID **和建立時間**都要對得上：只看 PID 的話，我的行程結束後被別人拿走的 PID 也會中（稽核 I2）
    if (!ourPids.has(r.pid) || !r.created || ourPids.get(r.pid) !== r.created) {
      others.push(r);
      continue;
    }
    killed.push(...killPids([r.pid]));
  }
  return { killed, others };
}

// 別人正在跑就直接中止（TASK-034）。要在 spawn 之前，才不會白跑一輪又把對方收掉。
await preflight();

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
spawnedAt = Date.now();
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
  exitedAt = Date.now();
  clearTimeout(timer);
  // 正常結束也要確認一次：tauri dev 的 watcher 可能已經重新啟動過 app，
  // 那個新的 app 行程不是 npx 的直接子行程，但仍掛在它底下。
  // ⚠️ child 已經結束了：**不再對 child.pid 下 taskkill**（PID 可能已被回收，稽核 I2）；
  // exitedAt 設了之後 killTree 只收「建立時間落在 child 活著期間」的遺孤。
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
