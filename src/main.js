// AwayTerminal2 前端進入點。
//
// 這個檔案本身**不管終端機邏輯**——那是搬過來的 `terminal.js` 的工作（輸入佇列、IME 守衛、
// 靜止閘門、去重、貼上路徑、分割/分欄 layout、Ctrl+F…）。main.js 只做三件事：
//   1. 把 `terminal.js` 期待的全域（Terminal / FitAddon / …）準備好——它原本靠 <script> 載入 UMD；
//   2. 提供 `window.AwayWebgl`（WebGL addon + DOM 退回），這是 terminal.js 的 AT2-2 修改要呼叫的；
//   3. 等 bridge 掛好 host→JS listener 之後才載入 terminal.js（它一載完就送 `ready`）。
import '@xterm/xterm/css/xterm.css';
import './style.css';
import { initExitDialog } from './restoretabs.js';

import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { SerializeAddon } from '@xterm/addon-serialize';
import { invoke, Channel } from '@tauri-apps/api/core';

import { bridgeReady, log, createSession } from './bridge.js';
import { initTabBar, currentTabState } from './tabbar.js';

// --- terminal.js 期待的全域（沿用 UMD 的命名空間形狀，這樣 terminal.js 一個字都不用改）---
window.Terminal = Terminal;
window.FitAddon = { FitAddon };
window.Unicode11Addon = { Unicode11Addon };
window.WebLinksAddon = { WebLinksAddon };
window.SerializeAddon = { SerializeAddon };

// --- WebGL 渲染器（terminal.js AT2-2 會呼叫）---
let rendererReported = false;
window.AwayWebgl = function (term, id) {
  let renderer = 'DOM';
  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => {
      // 休眠喚醒 / 驅動重置：卸載 addon，xterm 自動退回 DOM 渲染
      console.warn('[AwayTerminal] WebGL context lost, falling back to DOM renderer');
      log(`[AwayTerminal] pane ${id} WebGL context lost → 退回 DOM 渲染`);
      webgl.dispose();
    });
    term.loadAddon(webgl);
    renderer = 'WebGL';
  } catch (e) {
    console.warn('[AwayTerminal] WebGL addon 載入失敗，退回 DOM 渲染:', e);
    renderer = 'DOM (WebGL 載入失敗)';
  }
  console.log('[AwayTerminal] renderer =', renderer);
  if (!rendererReported) {
    rendererReported = true;
    invoke('report_renderer', { renderer }).catch(() => {});
  }
  return renderer;
};

// ------------------------------------------------- 端到端驗證（不需視窗焦點）
//
// 把 xterm buffer 的純文字尾端回報到後端 log，證明
// 「PTY 輸出 → channel → term.write」整條路通，不必看視窗、也不必自動化 GUI。

function awayDump(lines = 6, id = null) {
  const term = window.AwayTerm;
  if (!term) return [];
  const tail = term.tail(id, lines);
  // 一定要組成一包再送：每行各一次 invoke 是各自獨立的非同步呼叫，到達順序不保證
  log(`[dump] xterm buffer 尾端 ${tail.length} 行：\n${tail.map((t) => `[dump] | ${t}`).join('\n')}`);
  return tail;
}
window.awayDump = awayDump;

/** `--verify N` 用：再開一條 shell 分頁（不跳資料夾選擇，直接用預設工作目錄）。 */
async function createExtraSession() {
  const info = await createSession({ kind: 'shell' });
  return info.id;
}

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

/** 逾時就回這個值（`null`／`undefined` 都可能是正常結果，所以用一個獨一無二的哨兵）。 */
const TIMED_OUT = Symbol('timeout');

/** 給「可能永遠不回應」的 promise 包一層逾時（例如沒有焦點時的剪貼簿 API）。 */
function withTimeout(promise, ms) {
  return Promise.race([
    promise.catch(() => TIMED_OUT),
    new Promise((r) => setTimeout(() => r(TIMED_OUT), ms)),
  ]);
}

/**
 * 工具列功能的自動驗證（TASK-005）。三個不必目視的項目：
 *   1. **log 記錄**：開始記錄 → 送一個會吐彩色中文的指令 → 停止 → 回報檔案路徑。
 *      檔案內容（BOM／時間戳格式／有沒有殘留 ANSI／換行是 LF）由外面用 shell 比對。
 *   2. **複製全部**：`toolbar_copy_all` → 讀回剪貼簿，確認拿到東西。
 *   3. **純文字貼上**：`toolbar_paste` 送一段標記字串 → 讀 buffer 確認它出現在提示字元後面，
 *      這證明 `v` 協定 → `doPaste` → `xterm.paste()` → PTY 整條路是通的。
 * 另外順手驗逐分頁配色（`P` 協定會改 pane 元素的 background）。
 */
async function verifyToolbar(id) {
  const term = window.AwayTerm;
  const lines = ['[verify] 工具列'];
  log(`[verify] 工具列開始（分頁 ${id}）`);

  // ---- 1. log 記錄 ----
  const defaults = await invoke('log_defaults', { id });
  log(`[verify] log 預設值 ${defaults.path} ts=${defaults.timestamp} append=${defaults.append}`);
  // 驗證刻意不用預設的「我的文件」：這台機器的防毒會擋住剛建置的 exe 寫進去
  // （見 Logger::open_with_timeout）。驗證要的是「格式對不對」，寫 TEMP 就好。
  const tmp = await invoke('temp_dir');
  const logPath = `${tmp}\awayterm-verify.log`;
  try {
    const real = await invoke('log_start', {
      id,
      path: logPath,
      timestamp: true,
      append: false,
      remember: false, // 驗證不要改掉使用者的 log 資料夾設定
    });
    lines.push(`[verify] log 開始 → ${real}`);
    // 會產生 ANSI（顏色）＋中文＋OSC（視窗標題）的輸出
    await invoke('session_write_text', {
      id,
      text: "Write-Host \"`e[36m中文測試 AWAY_LOG_OK`e[0m\"; $Host.UI.RawUI.WindowTitle='away-log'\r",
    });
    await wait(2500);
    const stopped = await invoke('log_stop', { id });
    lines.push(`[verify] log 停止 → ${stopped}`);
  } catch (e) {
    lines.push(`[verify] log 失敗：${e}`);
  }

  // ---- 2. 複製全部（q…all → a…all → 剪貼簿）----
  //
  // ⚠️ 剪貼簿**只能半自動驗**：`navigator.clipboard.readText()` 需要視窗有焦點，
  // 沒焦點時在 WebView2 上不是拒絕、是**不回應**（實測會把整個驗證流程卡住）。
  // 共用桌面不能搶焦點 → 這一步包一層逾時，讀不到就標成「需要目視確認」往下走。
  try {
    await invoke('toolbar_copy_all', { id });
    await wait(600);
    const got = await withTimeout(navigator.clipboard.readText(), 1500);
    if (got === TIMED_OUT) {
      lines.push('[verify] 複製全部：剪貼簿讀不到（視窗沒有焦點）→ 需要目視確認');
    } else {
      lines.push(`[verify] 複製全部：剪貼簿 ${got.length} 字，含標記=${got.includes('AWAY_LOG_OK')}`);
    }
  } catch (e) {
    lines.push(`[verify] 複製全部失敗：${e}`);
  }

  // ---- 3. 純文字貼上（v 協定）----
  try {
    await invoke('toolbar_paste', { id, text: 'AWAY_PASTE_OK' });
    await wait(600);
    const tail = term.tail(id, 3).join(' ');
    lines.push(`[verify] 純文字貼上：buffer 含標記=${tail.includes('AWAY_PASTE_OK')}`);
    await invoke('session_write_text', { id, text: '\u0003' }); // Ctrl+C 把那行清掉
  } catch (e) {
    lines.push(`[verify] 純文字貼上失敗：${e}`);
  }

  // ---- 4. 逐分頁配色（P 協定）----
  try {
    await invoke('tab_colors', { id, fg: '#FFFF00', bg: '#000000' });
    await wait(200);
    const pane = document.querySelector(`.term[data-id="${id}"]`);
    lines.push(`[verify] 配色：pane 背景=${pane ? pane.style.background : '?'}`);
    await invoke('tab_colors', { id, fg: '', bg: '' });
    await wait(200);
    lines.push(`[verify] 配色清除：pane 背景=${pane ? pane.style.background : '?'}`);
  } catch (e) {
    lines.push(`[verify] 配色失敗：${e}`);
  }

  // ---- 5. 翻頁（S 協定；只確認不會炸、游標仍在底部）----
  try {
    for (const action of ['top', 'bottom']) {
      await invoke('toolbar_scroll', { id, action });
      await wait(200);
    }
    lines.push('[verify] 翻頁 top/bottom 已送出');
  } catch (e) {
    lines.push(`[verify] 翻頁失敗：${e}`);
  }

  log(lines.join('\n'));
}

/**
 * 多分頁驗證：每條分頁各自有沒有輸出、有沒有 fit 到正確尺寸。
 * 等到每條都讀得到內容（最多 10 秒）再一次報告，避免「還沒到」被當成「沒有」。
 */
async function awayVerify() {
  const term = window.AwayTerm;
  if (!term) return;
  for (let i = 0; i < 20; i++) {
    await new Promise((r) => setTimeout(r, 500));
    const ids = term.ids();
    if (ids.length && ids.every((id) => term.tail(id, 1).length > 0)) break;
  }
  // 檢視三態：分頁 → 分割 → 分欄 → 分頁 各切一次，每次記下每個 pane 的欄列數，
  // 確認切完都有重新 fit（TASK-003 的 `s{id}`／refit 教訓）。三次剛好轉回原本的模式。
  for (let i = 0; i < 3; i++) {
    const mode = await invoke('view_mode_cycle');
    await new Promise((r) => setTimeout(r, 700));
    const sizes = term
      .ids()
      .map((id) => { const s = term.size(id); return `${id}:${s ? `${s.cols}x${s.rows}` : '?'}`; })
      .join('  ');
    log(`[verify] 檢視模式 ${mode}  pane 尺寸 ${sizes}`);
  }

  const st = currentTabState();
  const lines = [`[verify] 多分頁狀態  檢視模式=${st.viewMode}  作用中=${st.activeId}`];
  for (const t of st.tabs) {
    lines.push(`[verify] tab ${t.id} 「${t.title}」 kind=${t.kind} busy=${t.busy} cwd=${t.cwdPath || '-'}`);
  }
  for (const id of term.ids()) {
    const size = term.size(id);
    const tail = term.tail(id, 2);
    lines.push(`[verify] pane ${id}  ${size ? `${size.cols}x${size.rows}` : '?'}  ${tail.length} 行`);
    for (const t of tail) lines.push(`[verify]   | ${t}`);
  }
  log(lines.join('\n'));

  // 工具列那批（log／複製全部／貼上／配色／翻頁）拿第一個分頁驗
  const first = st.tabs[0];
  if (first) await verifyToolbar(first.id);

  await verifySshPath();
  await verifyTelnetPath();
  await verifyRestore();
  await verifySandbox();
}
window.awayVerify = awayVerify;

/**
 * 沙盒模式驗證（TASK-007）。**只碰自己建立的東西、只查自己記下的 PID。**
 *
 * 做法：臨時建一條指向 `pwsh` 的自訂連線（沙盒開），用它開一個分頁，然後檢查
 * worktree／分支／`.gitignore` 乾淨／`TEMP` 有沒有導到沙盒／護欄檔案，
 * 最後在分頁裡開一個子行程記下 PID，關掉分頁之後確認**那個 PID** 不在了（Job Object 有效）。
 */
async function verifySandbox() {
  const lines = ['[verify] 沙盒模式'];
  const CONN = '__awayterm_verify_sandbox';
  // 上一次 verify 若被 timeout 砍掉會留 worktree 與分支（TASK-007 Issue 6）→ 先清
  try {
    const cleaned = await invoke('sandbox_verify_cleanup');
    if (cleaned.length) lines.push(`[verify] 清掉上次殘留：${cleaned.join('、')}`);
  } catch (e) {
    lines.push(`[verify] 清殘留失敗（不影響後面）：${e}`);
  }
  let tabId = null;
  try {
    const probe = await invoke('sandbox_probe');
    if (!probe.pwsh) {
      log('[verify] 沙盒：找不到 pwsh，跳過');
      return;
    }
    if (!probe.repoDir) {
      log('[verify] 沙盒：程式的工作目錄不在 git repo 裡，驗不到 worktree，跳過');
      return;
    }
    await invoke('custom_save', {
      conn: {
        name: CONN,
        path: probe.pwsh,
        args: '-NoLogo',
        icon: 'run',
        closeKey: 'ctrl-c',
        closeCount: 3,
        pickDir: false,
        hidden: true,
        viaPowerShell: false,
        sandbox: true,
      },
      originalName: null,
    });

    // 工作目錄用這個專案的 repo 根（才驗得到 worktree）
    const info = await createSession({ kind: 'conn', conn: CONN, cwd: probe.repoDir });
    tabId = info.id;
    await wait(1500);

    const st = currentTabState().tabs.find((t) => t.id === tabId);
    const sb = st && st.sandbox;
    if (!sb) {
      lines.push('[verify] 沒有建立沙盒（預期要有）');
    } else {
      lines.push(`[verify] worktree=${sb.hasWorktree} 分支=${sb.branch || '-'}`);
      lines.push(`[verify] 工作目錄=${sb.workDir}`);
      lines.push(`[verify] 護欄檔案=${(sb.guardrails || []).join('、') || '（這個工具沒有 hook，預期如此）'}`);
      const env = Object.fromEntries(sb.env || []);
      lines.push(`[verify] TEMP 導到沙盒=${(env.TEMP || '').startsWith(sb.root)}`);
      const checks = await invoke('sandbox_verify', { id: tabId });
      lines.push(`[verify] .ai/sandbox 被 git 忽略（git status 乾淨）=${checks.ignored}`);
      lines.push(`[verify] worktree 目錄存在=${checks.worktreeExists}`);
      lines.push(`[verify] 分支存在=${checks.branchExists}`);
    }

    // Job Object：在分頁裡開一個子行程，記下它的 PID
    await invoke('session_write_text', {
      id: tabId,
      text: '$p = Start-Process pwsh -ArgumentList "-NoLogo","-NoExit" -PassThru; "AWAY_CHILD_PID=" + $p.Id\r',
    });
    let pid = null;
    for (let i = 0; i < 20; i++) {
      await wait(400);
      const m = /AWAY_CHILD_PID=(\d+)/.exec(window.AwayTerm.tail(tabId, 12).join(' '));
      if (m) {
        pid = Number(m[1]);
        break;
      }
    }
    if (!pid) {
      lines.push('[verify] Job Object：拿不到子行程 PID，跳過這項');
    } else {
      const before = await invoke('pid_alive', { pid });
      await invoke('tab_close', { id: tabId });
      tabId = null;
      await wait(1500);
      const after = await invoke('pid_alive', { pid });
      lines.push(
        `[verify] Job Object：子行程 PID ${pid} 關分頁前存活=${before}、關分頁後存活=${after}` +
          `（後者要是 false）`
      );
    }
  } catch (e) {
    lines.push(`[verify] 失敗：${e && e.stack ? e.stack : e}`);
  } finally {
    if (tabId !== null) await invoke('tab_close', { id: tabId }).catch(() => {});
    await invoke('custom_delete', { name: CONN }).catch(() => {});
  }
  log(lines.join('\n'));
}

/**
 * SSH 的 **app 端路徑**驗證（連線引擎本身由 `cargo run --example ssh_probe` 驗）。
 *
 * 這裡只連 `127.0.0.1` 的一個**沒人在聽**的埠：不碰任何外部主機，但足以證明
 * `session_create(kind:"ssh")` → 建分頁 → ssh 模組跑起來 → 連不上的原因有印在終端機裡
 * → session 正常結束（不會留一個打字全被吞的死分頁）。
 */
async function verifySshPath() {
  const lines = ['[verify] SSH（app 端路徑）'];
  try {
    const info = await createSession({ kind: 'ssh', ssh: { host: '127.0.0.1', port: 1 } });
    lines.push(`[verify] 分頁 ${info.id} 建立：backend=${info.backend} title=${info.title}`);
    const term = window.AwayTerm;
    let text = '';
    for (let i = 0; i < 20; i++) {
      await wait(300);
      text = term.tail(info.id, 8).join(' ');
      if (text.includes('連線失敗')) break;
    }
    lines.push(`[verify] 連不上時有把原因印在終端機：${text.includes('連線失敗')}`);
    const st = currentTabState().tabs.find((t) => t.id === info.id);
    lines.push(`[verify] 分頁 kind=${st ? st.kind : '?'}（應為 ssh）`);
    // 沒勾自動重連 → 應該提示「按 Enter 在此分頁重新連線」
    lines.push(`[verify] 提示按 Enter 重連：${text.includes('按 Enter')}`);
    await invoke('tab_close', { id: info.id });

    // 勾了自動重連 → 應該印「N 秒後自動重連」並開始退避（3 秒起）
    const r = await createSession({
      kind: 'ssh',
      ssh: { host: '127.0.0.1', port: 1, autoReconnect: true },
    });
    let t2 = '';
    for (let i = 0; i < 20; i++) {
      await wait(300);
      t2 = term.tail(r.id, 10).join(' ');
      if (t2.includes('秒後自動重連')) break;
    }
    lines.push(`[verify] 自動重連有排程：${t2.includes('秒後自動重連')}`);
    const st2 = currentTabState().tabs.find((t) => t.id === r.id);
    lines.push(`[verify] 退避次數=${st2 ? st2.reconnectAttempt : '?'}（第一次應為 1）`);
    // 再等一輪，確認退避是往上加的（3 → 6）而不是卡在同一個值
    for (let i = 0; i < 24; i++) {
      await wait(500);
      const s3 = currentTabState().tabs.find((t) => t.id === r.id);
      if (s3 && s3.reconnectAttempt >= 2) break;
    }
    const st3 = currentTabState().tabs.find((t) => t.id === r.id);
    lines.push(`[verify] 退避次數變成 ${st3 ? st3.reconnectAttempt : '?'}（應 ≥2，代表重試過）`);
    await invoke('tab_close', { id: r.id });
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  }
  log(lines.join('\n'));
}

/**
 * Telnet 的 **app 端路徑**驗證（協定本身由 `cargo run --example telnet_probe` 驗，17 項）。
 *
 * 同 SSH 那條：只連 `127.0.0.1` 一個**沒人在聽**的埠，證明
 * `session_create(kind:"telnet")` → 建分頁（kind=telnet、backend=telnet）→
 * 連不上的原因印在終端機 → 提示按 Enter 重連。
 */
async function verifyTelnetPath() {
  const lines = ['[verify] Telnet（app 端路徑）'];
  try {
    const info = await createSession({ kind: 'telnet', telnet: { host: '127.0.0.1', port: 1 } });
    lines.push(`[verify] 分頁 ${info.id} 建立：backend=${info.backend} title=${info.title}`);
    const term = window.AwayTerm;
    let text = '';
    for (let i = 0; i < 20; i++) {
      await wait(300);
      text = term.tail(info.id, 8).join(' ');
      if (text.includes('失敗')) break;
    }
    lines.push(`[verify] 連不上時有把原因印在終端機：${text.includes('失敗')}`);
    const st = currentTabState().tabs.find((t) => t.id === info.id);
    lines.push(`[verify] 分頁 kind=${st ? st.kind : '?'}（應為 telnet）`);
    lines.push(`[verify] 提示按 Enter 重連：${text.includes('按 Enter')}`);
    lines.push(`[verify] 標題是 host:port：${info.title === '127.0.0.1:1'}（同舊版 OpenTelnetDirect）`);
    await invoke('tab_close', { id: info.id });
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  }
  log(lines.join('\n'));
}

/**
 * 恢復分頁驗證（TASK-010 B）。**在同一次執行裡走完「存 → 恢復」**：
 *
 *   1. 在目前分頁印一個記號 → `restore_verify_save`（＝勾了恢復分頁關程式的那一步）
 *   2. `restore_list` → `createSession({restore: 0})`：Rust 會在 `n` 之後、`s` 之前送 `b`
 *   3. 檢查新分頁的畫面：記號 → 分隔行 → 新 shell 的提示字元，**順序**要對
 *
 * 第 3 點就是在驗 `terminal.js` 的 `pendingRestore` / `held`（那段是舊版原檔一字未改的）：
 * `b` 到的時候 pane 還沒 fit，新 session 的輸出會被扣在 `held` 裡，
 * 等舊畫面寫完才依序補上。順序錯了代表那段邏輯在這個環境下不成立。
 */
async function verifyRestore() {
  const lines = ['[verify] 恢復分頁'];
  const MARK = 'AWAY_RESTORE_MARK_42';
  let restoredId = null;
  try {
    const term = window.AwayTerm;
    // 記號直接寫進畫面（不靠 shell 執行，才不受提示字元時序影響）。
    // **每個分頁都寫**：`restore_list` 回來的第一筆不一定是目前作用中的那個分頁
    // （第一版只寫 active，結果恢復的是另一個分頁的畫面，記號自然找不到）。
    for (const id of term.ids()) {
      term.writeOutput(id, new TextEncoder().encode(`\r\n${MARK}\r\n`));
    }
    await wait(400);

    const n = await invoke('restore_verify_save');
    lines.push(`[verify] 存下 ${n} 個分頁`);
    const list = await invoke('restore_list');
    const idx = list.findIndex((e) => e.bufferFile);
    lines.push(`[verify] 有畫面內容的分頁：${idx >= 0 ? `第 ${idx + 1} 筆` : '沒有'}`);
    if (idx < 0) throw new Error('沒有存到任何畫面內容（q…save 沒回來？）');
    lines.push(`[verify] 存檔不含密碼欄位：${!JSON.stringify(list).toLowerCase().includes('password')}`);

    const info = await createSession({ kind: 'shell', restore: idx, title: '__verify_restore' });
    restoredId = info.id;
    // 等舊畫面倒回來 + 新 shell 的提示字元出現
    let tail = [];
    for (let i = 0; i < 30; i++) {
      await wait(400);
      tail = term.tail(info.id, 40);
      if (tail.some((l) => l.includes(MARK)) && tail.some((l) => l.includes('PS '))) break;
    }
    const iMark = tail.findIndex((l) => l.includes(MARK));
    const iSep = tail.findIndex((l) => l.includes('以上為上次關閉前的紀錄'));
    const iPrompt = tail.findIndex((l, k) => k > iSep && l.includes('PS '));
    lines.push(`[verify] 舊畫面有倒回來（記號在第 ${iMark} 行）：${iMark >= 0}`);
    lines.push(`[verify] 分隔行有出現（第 ${iSep} 行）：${iSep >= 0}`);
    lines.push(
      `[verify] held 順序正確（記號 ${iMark} < 分隔行 ${iSep} < 新提示字元 ${iPrompt}）：` +
        `${iMark >= 0 && iSep > iMark && iPrompt > iSep}`,
    );
    const st = currentTabState().tabs.find((t) => t.id === info.id);
    lines.push(`[verify] 分頁數 ${currentTabState().tabs.length}、恢復的分頁 kind=${st ? st.kind : '?'}`);
  } catch (e) {
    lines.push(`[verify] 失敗：${e}`);
  } finally {
    // 不要把驗證用的紀錄留下來（否則下次真的啟動會莫名恢復一堆分頁）
    try {
      if (restoredId !== null) await invoke('tab_close', { id: restoredId });
      const left = await invoke('restore_verify_clear');
      lines.push(`[verify] 已清掉存檔（剩 ${left} 筆）`);
    } catch (e) {
      lines.push(`[verify] 清除存檔失敗：${e}`);
    }
  }
  log(lines.join('\n'));
}

// ------------------------------------------------------------- IPC bench
//
// CLAUDE.md 風險 5 的量測。**只在 `?bench=1` 時自動跑**（TASK-003 調整），
// 其他時候可在 devtools 手動叫 `awayBench()` / `awayBenchSmall()`。
// 結果與結論寫在 docs/IPC-BENCH.md。

const MB = 1024 * 1024;

function transportOf(v) {
  if (v instanceof ArrayBuffer) return 'ArrayBuffer';
  if (Array.isArray(v)) return `JSON 數字陣列 (${v.length} 個元素)`;
  if (typeof v === 'string') return 'string';
  if (v && v.byteLength !== undefined) return v.constructor.name;
  return typeof v;
}

function byteLenOf(v) {
  if (v instanceof ArrayBuffer) return v.byteLength;
  if (Array.isArray(v)) return v.length;
  if (typeof v === 'string') return v.length;
  return -1;
}

const BENCH_MODES = {
  // Response::new(Vec<u8>) → 自訂協定回應，真二進位
  raw: (size) => invoke('bench_raw', { size }),
  // command 直接回 Vec<u8> → serde 序列化成 JSON 數字陣列（反例，保留供對照）
  vec: (size) => invoke('bench_vec', { size }),
  // base64 字串 + atob（舊版 o{id}US{base64} 的做法）
  base64: async (size) => {
    const s = await invoke('bench_base64', { size });
    const bin = atob(s);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out.buffer;
  },
  // Channel 送 InvokeResponseBody::Raw（PTY 輸出實際走的路）
  channel: (size) =>
    new Promise((resolve, reject) => {
      const ch = new Channel();
      ch.onmessage = (m) => resolve(m);
      invoke('bench_channel', { size, onData: ch }).catch(reject);
    }),
};

async function benchMode(name, size, iterations) {
  const fn = BENCH_MODES[name];
  await fn(size); // 暖機一次，不計入
  const times = [];
  let transport = '?';
  let bytes = -1;
  for (let i = 0; i < iterations; i++) {
    const t0 = performance.now();
    const v = await fn(size);
    times.push(performance.now() - t0);
    if (i === 0) {
      transport = transportOf(v);
      bytes = byteLenOf(v);
    }
  }
  times.sort((a, b) => a - b);
  const mean = times.reduce((a, b) => a + b, 0) / times.length;
  return {
    name,
    transport,
    bytes,
    min: times[0],
    median: times[Math.floor(times.length / 2)],
    mean,
    max: times[times.length - 1],
    mbPerSec: size / MB / (mean / 1000),
  };
}

async function awayBench(sizeMb = 1, iterations = 10) {
  const lines = [`[IPC bench] ${iterations} × ${sizeMb}MB`];
  const size = Math.round(sizeMb * MB);
  const rows = [];
  for (const name of ['raw', 'vec', 'base64', 'channel']) {
    const r = await benchMode(name, size, iterations);
    rows.push(r);
    lines.push(
      `  ${name.padEnd(8)} median ${r.median.toFixed(1).padStart(8)} ms` +
        `  mean ${r.mean.toFixed(1).padStart(8)} ms` +
        `  min ${r.min.toFixed(1).padStart(8)} ms` +
        `  max ${r.max.toFixed(1).padStart(8)} ms` +
        `  ${r.mbPerSec.toFixed(1).padStart(6)} MB/s` +
        `  ${r.bytes} bytes via ${r.transport}`
    );
  }
  log(lines.join('\n'));
  console.log(lines.join('\n'));
  return rows;
}
window.awayBench = awayBench;

/** channel 的 1024 bytes 門檻兩邊各量一次（見 docs/IPC-BENCH.md）。 */
async function awayBenchSmall(iterations = 200) {
  const lines = [`[IPC bench small] ${iterations} × (512 / 2048 bytes) via channel`];
  for (const size of [512, 2048]) {
    const r = await benchMode('channel', size, iterations);
    lines.push(
      `  channel ${String(size).padStart(5)}B  median ${r.median.toFixed(3)} ms` +
        `  mean ${r.mean.toFixed(3)} ms  ${r.bytes} bytes via ${r.transport}`
    );
  }
  log(lines.join('\n'));
  console.log(lines.join('\n'));
}
window.awayBenchSmall = awayBenchSmall;

// ------------------------------------------------------------------ 啟動

(async () => {
  // host→JS 的 listener 要在 terminal.js 送 `ready` 之前掛好
  await bridgeReady;
  // 分頁列也要先掛好 `tab-state` 的 listener：第一條 session 是 terminal.js 送出
  // `ready` 之後才建的，那一刻就會 emit 第一筆狀態，晚掛就漏掉第一列。
  await initTabBar();
  // 離開程式的對話框（Rust 擋下 CloseRequested 後會 emit exit-request）
  await initExitDialog();

  // 啟動選項：URL 參數優先（方便在 devtools 直接換），其次是 CLI 參數
  // （`AwayTerminal.exe --cmd claude` / `--verify 2` / `--bench`，見 src-tauri/src/cli.rs）。
  const params = new URLSearchParams(location.search);
  let cli = { cmd: null, verify: 0, bench: false };
  try {
    cli = await invoke('launch_args');
  } catch (e) {
    log(`[main] 讀啟動參數失敗：${e}`);
  }
  const opt = {
    cmd: params.get('cmd') || cli.cmd || null,
    verify: parseInt(params.get('verify') || '', 10) || cli.verify || 0,
    bench: params.get('bench') === '1' || cli.bench,
  };
  // bridge.js 的 onReady 會讀這個決定第一條 session 用什麼指令開
  window.AwayLaunch = opt;

  // terminal.js 是舊版原檔（IIFE），載入即執行並在最後送 `ready`
  try {
    await import('./terminal.js');
  } catch (e) {
    log(`[main] 載入 terminal.js 失敗：${e && e.stack ? e.stack : e}`);
    throw e;
  }

  if (opt.bench) {
    try {
      await awayBench(1, 10);
      await awayBenchSmall(200);
    } catch (e) {
      log(`[IPC bench] 失敗：${e}`);
    }
  }

  // 多分頁端到端驗證（`--verify N` / `?verify=N`）：再開 N 條 shell，等提示字元出來，
  // 然後把每條的 buffer 尾端與欄列數報到後端 log。不需要視窗焦點、不用 GUI 自動化。
  const verify = opt.verify;
  if (verify > 0) {
    try {
      for (let i = 0; i < verify; i++) await createExtraSession();
      await awayVerify();
    } catch (e) {
      log(`[verify] 失敗：${e && e.stack ? e.stack : e}`);
    }
  }

  if (params.get('dump') === '1' || import.meta.env.DEV) {
    // 提示字元要等 shell 啟動＋pane fit 完才會有，單次固定延遲不可靠（實測 3s 時 buffer 還是空的）。
    // 最多試 10 次、每 1s 一次，讀到東西就停；10 次都空才報空，這樣「真的沒輸出」與「還沒到」分得出來。
    (async () => {
      for (let i = 1; i <= 10; i++) {
        await new Promise((r) => setTimeout(r, 1000));
        const tail = window.AwayTerm ? window.AwayTerm.tail(null, 6) : [];
        if (tail.length) return awayDump(6);
        if (i === 10) log('[dump] 試了 10 秒，xterm buffer 仍是空的');
      }
    })();
  }
})();
