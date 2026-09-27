// 代理團隊（Multi-Agent）：建團隊對話框與整組的啟動流程。
// 搬移舊版 `Dialogs/MultiAgentDialog.xaml(.cs)` 與 `MainWindow.MultiAgent.cs` 的開組那半段。
//
// ## 分工（為什麼不是 Rust 一個人做完）
// 每個 agent 的 PTY 輸出走各自的 tauri `Channel`，而 `Channel` 只能由前端建 → 所以：
//
//   1. `agent_team_create(setup)`：Rust 決定組號、開沙盒（**一個團隊一棵 worktree**）、
//      寫護欄、組每一格的角色檔（含執行期脈絡），回傳啟動計畫。
//   2. 這裡照計畫逐格 `createSession({ kind: 'agent', agent: { team, index } })`
//      ——連線、參數（`--append-system-prompt-file` 之類）、環境變數、Job Object 全都是
//      Rust 依計畫決定的，前端只負責「開一條 session」。
//   3. `agent_team_ready(key)`：Rust 綁組、送 `g` 協定排版、開始監看 `.ai/bus/`。
//
// 開起來之後就不需要前端了：投遞由 Rust 的 600ms tick 做（`agent/deliver.rs`）。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { onLangChange } from './i18n.js';
import { log } from './bridge.js';

const el = {};
/** `agent_setup_options()` 的結果。 */
let opts = null;
let resolveOpen = null;

function $(id) {
  return document.getElementById(id);
}

function fillSelect(node, values, labelOf) {
  const keep = node.value;
  node.textContent = '';
  for (const v of values) {
    const o = document.createElement('option');
    o.value = String(v.value !== undefined ? v.value : v);
    o.textContent = labelOf ? labelOf(v) : String(v);
    node.appendChild(o);
  }
  if (keep && Array.from(node.options).some((o) => o.value === keep)) node.value = keep;
}

/** 投遞上限的顯示文字（0＝不限，同舊版 `ma.limitUnlimited`）。 */
function limitLabel(n) {
  return n === 0 ? T['ma.limitUnlimited'] : String(n);
}

/** 閒置檢查的顯示文字（0＝不檢查）。 */
function idleLabel(n) {
  return n === 0 ? T['ma.idleOff'] : fmt('ma.idleMinutes', n);
}

/** 四格的外框顏色（和 pane 外框、`g` 協定的顏色同一組）。 */
const SLOT_COLORS = ['#EF9A9A', '#90CAF9', '#A5D6A7', '#CE93D8'];

/** 建四格（2×2）。格 1 的「啟用」永遠勾著且不能改——使用者就是要跟它說話。 */
function buildSlots() {
  el.slots.textContent = '';
  el.slotUi = [];
  for (let i = 0; i < 4; i++) {
    const box = document.createElement('div');
    box.className = 'ma-slot';
    box.style.borderColor = SLOT_COLORS[i];

    const head = document.createElement('div');
    head.className = 'ma-slot-head';
    const id = document.createElement('span');
    id.className = 'ma-slot-id';
    id.style.color = SLOT_COLORS[i];
    id.textContent = `Agent-x${i + 1}`;
    const enableLabel = document.createElement('label');
    enableLabel.className = 'sd-check';
    enableLabel.style.margin = '0';
    const enable = document.createElement('input');
    enable.type = 'checkbox';
    const enableText = document.createElement('span');
    enableLabel.append(enable, enableText);
    head.append(id, enableLabel);

    const cliLabel = document.createElement('label');
    const backend = document.createElement('select');
    const roleLabel = document.createElement('label');
    const role = document.createElement('select');
    box.append(head, cliLabel, backend, roleLabel, role);
    el.slots.appendChild(box);

    const ui = {
      box,
      id,
      enable,
      enableText,
      cliLabel,
      backend,
      roleLabel,
      role,
    };
    el.slotUi.push(ui);
    enable.addEventListener('change', () => refreshSlot(i));
  }
}

function refreshSlot(i) {
  const ui = el.slotUi[i];
  // 格 1（下方全寬那一格）一定啟用（舊版 `ui.Enable.IsEnabled = index != 1`）
  if (i === 0) {
    ui.enable.checked = true;
    ui.enable.disabled = true;
  }
  const on = ui.enable.checked;
  ui.backend.disabled = ui.role.disabled = !on;
  ui.box.classList.toggle('off', !on);
}

/** 把介面文字重設一次（切語言時會被叫；註冊在 `i18n.js`）。 */
function applyTexts() {
  $('madlg-title').textContent = T['ma.title'];
  $('ma-l-limit').textContent = T['ma.dlgLimit'];
  $('ma-limit-hint').textContent = T['ma.dlgLimitHint'];
  $('ma-l-idle').textContent = T['ma.dlgIdleCheck'];
  $('ma-idle-hint').textContent = T['ma.dlgIdleHint'];
  $('ma-l-sandbox').textContent = T['ma.dlgSandbox'];
  el.hint.textContent = T['ma.dlgHint'];
  el.restoreRoles.textContent = T['ma.dlgRestoreRoles'];
  el.openRoles.textContent = T['ma.dlgOpenRoles'];
  el.ok.textContent = T['ma.dlgOpen'];
  el.cancel.textContent = T['dlg.cancel'];
  if (!el.slotUi) return;
  for (let i = 0; i < el.slotUi.length; i++) {
    el.slotUi[i].enableText.textContent = T['ma.dlgEnable'];
    el.slotUi[i].cliLabel.textContent = T['ma.dlgAgentType'];
    el.slotUi[i].roleLabel.textContent = T['ma.dlgAgentRole'];
  }
  if (opts) {
    fillSelect(el.limit, opts.limitChoices, limitLabel);
    fillSelect(el.idle, opts.idleChoices, idleLabel);
    fillRoles();
  }
}

function fillRoles() {
  for (const ui of el.slotUi) {
    fillSelect(
      ui.role,
      [{ value: '', title: T['ma.dlgRoleNone'] }].concat(
        opts.roles.map((r) => ({ value: r.key, title: r.title })),
      ),
      (r) => r.title,
    );
  }
}

export function initAgentDialog() {
  el.root = $('madlg');
  el.form = $('madlg-box');
  el.dir = $('ma-dir');
  el.limit = $('ma-limit');
  el.idle = $('ma-idle');
  el.sandbox = $('ma-sandbox');
  el.slots = $('ma-slots');
  el.noBackend = $('ma-nobackend');
  el.hint = $('ma-hint');
  el.restoreRoles = $('ma-restore-roles');
  el.openRoles = $('ma-open-roles');
  el.ok = $('ma-ok');
  el.cancel = $('ma-cancel');
  buildSlots();
  onLangChange(applyTexts);

  const close = (value) => {
    el.root.hidden = true;
    const r = resolveOpen;
    resolveOpen = null;
    if (r) r(value);
  };
  el.form.addEventListener('submit', (e) => {
    e.preventDefault();
    const setup = read();
    if (!setup) return;
    close(setup);
  });
  el.cancel.addEventListener('click', () => close(null));
  document.addEventListener('keydown', (e) => {
    if (!el.root.hidden && e.key === 'Escape') close(null);
  });

  el.restoreRoles.addEventListener('click', async () => {
    const { askYesNo } = await import('./tabbar.js');
    if (!(await askYesNo(T['ma.title'], T['ma.dlgRestoreRolesAsk']))) return;
    try {
      opts.roles = await invoke('agent_roles_restore');
      fillRoles();
    } catch (err) {
      log(`[agentdlg] 還原角色檔失敗：${err}`);
    }
  });
  el.openRoles.addEventListener('click', async () => {
    try {
      const dir = await invoke('agent_roles_dir');
      await invoke('open_dir', { path: dir });
    } catch (err) {
      log(`[agentdlg] 開啟角色檔資料夾失敗：${err}`);
    }
  });
}

/** 讀成 `TeamSetup`；沒選代理人類型就擋下來（同舊版 `ma.dlgNeedBackend`）。 */
function read() {
  const slots = el.slotUi.map((ui, i) => ({
    enabled: i === 0 ? true : ui.enable.checked,
    backend: ui.backend.value,
    role: ui.role.value,
  }));
  for (let i = 0; i < slots.length; i++) {
    if (slots[i].enabled && !slots[i].backend) {
      el.noBackend.hidden = false;
      el.noBackend.textContent = fmt('ma.dlgNeedBackend', `Agent-x${i + 1}`);
      return null;
    }
  }
  return {
    dir: el.dirPath,
    title: '',
    slots,
    maxMessages: Number(el.limit.value) || 0,
    idleCheckMinutes: Number(el.idle.value) || 0,
    sandbox: el.sandbox.checked,
  };
}

/** 開對話框，回傳 `TeamSetup`（取消＝null）。 */
async function openDialog(dir) {
  try {
    opts = await invoke('agent_setup_options');
  } catch (e) {
    log(`[agentdlg] 讀取設定選項失敗：${e}`);
    return null;
  }
  el.dirPath = dir;
  el.dir.textContent = dir;
  el.limit.value = String(opts.defaultMaxMessages);
  el.idle.value = String(opts.defaultIdleCheck);
  el.sandbox.checked = true;
  applyTexts();

  // 沒有裝任何一家 CLI＝不能開（舊版 `ma.dlgNoBackend` ＋ 停用「開啟」）
  const none = opts.backends.length === 0;
  el.noBackend.hidden = !none;
  if (none) el.noBackend.textContent = T['ma.dlgNoBackend'];
  el.ok.disabled = none;

  for (let i = 0; i < el.slotUi.length; i++) {
    const ui = el.slotUi[i];
    fillSelect(
      ui.backend,
      [{ value: '', title: '' }].concat(
        opts.backends.map((b) => ({ value: b.key, title: b.name })),
      ),
      (b) => b.title,
    );
    // 預設：每一格都用第一家找得到的 CLI（舊版也是拿第一個可用的）
    ui.backend.value = opts.backends.length ? opts.backends[0].key : '';
    ui.role.value = opts.defaultRoles[i] || '';
    // 新開的組：格 1、2 預設啟用（PM ＋ SE 是最小可用團隊），3、4 使用者自己勾
    ui.enable.checked = i < 2;
    refreshSlot(i);
  }

  el.root.hidden = false;
  return new Promise((resolve) => {
    resolveOpen = resolve;
  });
}

/**
 * 「新分頁 ▾ → 代理團隊…」：先選專案資料夾（同其他「啟動前選擇資料夾」的連線）→ 設定視窗
 * → 建團隊 → 逐格啟動 → 綁組。
 */
export async function openAgentTeam(createSession) {
  const { showInfo } = await import('./tabbar.js');
  let dir;
  try {
    dir = await invoke('pick_work_dir', { title: T['ma.pickDir'] });
  } catch (e) {
    log(`[agentdlg] 選資料夾失敗：${e}`);
    return;
  }
  if (!dir) return; // 取消（同舊版：PickWorkDir 回 null 就不開）
  const setup = await openDialog(dir);
  if (!setup) return;

  let plan;
  try {
    plan = await invoke('agent_team_create', { setup });
  } catch (e) {
    log(`[agentdlg] 建團隊失敗：${e}`);
    await showInfo(T['ma.title'], String(e));
    return;
  }
  log(
    `[agentdlg] 代理團隊 ${plan.number} 計畫：${plan.slots
      .map((s) => `${s.agentId}/${s.backendName}/${s.roleTitle}`)
      .join(' ')}（工作區 ${plan.workDir}）`,
  );

  const failed = [];
  for (const slot of plan.slots) {
    try {
      await createSession({
        kind: 'agent',
        agent: { team: plan.key, index: slot.index },
      });
    } catch (e) {
      // CLI 找不到／被移除：那一格從名單裡拿掉，其餘照開（舊版 `anyFailed` 那條路）
      log(`[agentdlg] ${slot.agentId} 啟動失敗：${e}`);
      failed.push(String(e));
      await invoke('agent_slot_failed', {
        key: plan.key,
        index: slot.index,
      }).catch(() => {});
    }
  }
  try {
    const n = await invoke('agent_team_ready', { key: plan.key });
    log(`[agentdlg] 代理團隊 ${plan.number} 就緒：${n} 個 agent`);
  } catch (e) {
    await showInfo(T['ma.title'], String(e));
    return;
  }
  if (failed.length) await showInfo(T['ma.title'], failed.join('\n'));
}
