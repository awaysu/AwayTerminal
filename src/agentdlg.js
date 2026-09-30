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
/** 既有團隊的目前狀態（`agent_team_state()`）；`null`＝正在建新團隊。 */
let existing = null;
/** 這次開的是 `team`（代理團隊）還是 `chat`（AI 聊天室）。 */
let kind = 'team';
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
    // 狀態列（執行中／已結束／未啟用 ＋ 套用後會怎樣 ＋ 重新啟動）：**只有既有的組才顯示**，
    // 新開的組不留空白（同舊版 `foot.Visibility = _group == null ? Collapsed : Visible`）
    const foot = document.createElement('div');
    foot.className = 'ma-slot-foot';
    const status = document.createElement('span');
    status.className = 'ma-slot-status';
    const restart = document.createElement('button');
    restart.type = 'button';
    restart.className = 'ma-slot-restart';
    foot.append(status, restart);
    box.append(head, cliLabel, backend, roleLabel, role, foot);
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
      foot,
      status,
      restart,
      // 既有團隊：這一格目前的狀態與原本的選擇（判斷「改了沒有」用）
      state: 'notRunning',
      origBackend: '',
      origRole: '',
      wantRestart: false,
    };
    el.slotUi.push(ui);
    enable.addEventListener('change', () => refreshSlot(i));
    backend.addEventListener('change', () => refreshSlot(i));
    role.addEventListener('change', () => refreshSlot(i));
    restart.addEventListener('click', () => {
      ui.wantRestart = !ui.wantRestart;
      refreshSlot(i);
    });
  }
}

/**
 * 這一格按「套用」後會怎樣（舊版 `MultiAgentDialog.ActionOf`）。
 * 回 `''`（不動）／`start`／`restart`／`close`。新開的組只有 `start`／`''`。
 */
function actionOf(i) {
  const ui = el.slotUi[i];
  const on = ui.enable.checked || i === 0;
  if (!existing) return on ? 'start' : '';
  if (ui.state === 'notRunning') return on ? 'start' : '';
  if (!on) return 'close';
  const changed =
    ui.backend.value.toLowerCase() !== (ui.origBackend || '').toLowerCase() ||
    ui.role.value.toLowerCase() !== (ui.origRole || '').toLowerCase();
  return changed || ui.wantRestart ? 'restart' : '';
}

function refreshSlot(i) {
  const ui = el.slotUi[i];
  // 格 1（下方全寬那一格）一定啟用（舊版 `ui.Enable.IsEnabled = index != 1`）
  if (i === 0) {
    ui.enable.checked = true;
    ui.enable.disabled = true;
  }
  const on = ui.enable.checked;
  ui.backend.disabled = !on;
  // 聊天室第 1 位的角色固定主持人 → 永遠停用
  ui.role.disabled = !on || (kind === 'chat' && i === 0);
  ui.box.classList.toggle('off', !on);

  ui.foot.hidden = !existing;
  if (!existing) return;
  // 「重新啟動」只對已經啟動過的格有意義
  ui.restart.hidden = !(on && ui.state !== 'notRunning');
  ui.restart.textContent = T[ui.wantRestart ? 'ma.dlgRestartOn' : 'ma.dlgRestart'];
  const status =
    ui.state === 'running'
      ? T['ma.dlgRunning']
      : ui.state === 'exited'
        ? T['ma.dlgExited']
        : T[on ? 'ma.dlgNotStarted' : 'ma.dlgNotRunning'];
  const will = { start: 'ma.dlgWillStart', restart: 'ma.dlgWillRestart', close: 'ma.dlgWillClose' }[
    actionOf(i)
  ];
  ui.status.textContent = will ? status + T[will] : status;
}

/** 把介面文字重設一次（切語言時會被叫；註冊在 `i18n.js`）。 */
function applyTexts() {
  const chat = kind === 'chat';
  $('madlg-title').textContent = T[chat ? 'chat.title' : 'ma.title'];
  // 聊天室：第一列放「討論回合」，第二列（閒置檢查）整列隱藏——照舊版
  $('ma-l-limit').textContent = T[chat ? 'chat.dlgRounds' : 'ma.dlgLimit'];
  $('ma-limit-hint').textContent = T[chat ? 'chat.dlgRoundsHint' : 'ma.dlgLimitHint'];
  $('ma-l-idle').textContent = T['ma.dlgIdleCheck'];
  $('ma-idle-hint').textContent = T['ma.dlgIdleHint'];
  $('ma-l-sandbox').textContent = T['ma.dlgSandbox'];
  el.hint.textContent = T[chat ? 'chat.dlgHint' : 'ma.dlgHint'];
  el.restoreRoles.textContent = T['ma.dlgRestoreRoles'];
  el.openRoles.textContent = T['ma.dlgOpenRoles'];
  // 既有團隊是「套用」：切語言時也要維持
  el.ok.textContent = T[existing ? 'ma.dlgApply' : 'ma.dlgOpen'];
  el.cancel.textContent = T['dlg.cancel'];
  if (!el.slotUi) return;
  for (let i = 0; i < el.slotUi.length; i++) {
    el.slotUi[i].enableText.textContent = T['ma.dlgEnable'];
    el.slotUi[i].cliLabel.textContent = T[chat ? 'chat.dlgAiType' : 'ma.dlgAgentType'];
    el.slotUi[i].roleLabel.textContent = T[chat ? 'chat.dlgRole' : 'ma.dlgAgentRole'];
  }
  if (opts) {
    if (chat) {
      fillSelect(el.limit, opts.roundChoices, (n) => fmt('chat.rounds', n));
    } else {
      fillSelect(el.limit, opts.limitChoices, limitLabel);
    }
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
  // 閒置檢查那一列有兩個元素（label ＋ 欄位），聊天室要整列隱藏
  el.idleRow = Array.from(document.querySelectorAll('#ma-opts [data-row="idle"]'));
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
    // 頁內 #modal（確認框之類）開著時 Esc 是給它的，不能連底下這個對話框一起關（B3）
    if (!document.getElementById('modal').hidden) return;
    if (!el.root.hidden && e.key === 'Escape') close(null);
  });

  el.restoreRoles.addEventListener('click', async () => {
    const { askYesNo } = await import('./tabbar.js');
    if (!(await askYesNo(T[kind === 'chat' ? 'chat.title' : 'ma.title'], T['ma.dlgRestoreRolesAsk'])))
      return;
    try {
      // 要帶 kind：不帶的話 Rust 端預設是代理團隊，聊天室按了會去還原團隊的角色庫（B5）
      opts.roles = await invoke('agent_roles_restore', { kind });
      fillRoles();
    } catch (err) {
      log(`[agentdlg] 還原角色檔失敗：${err}`);
    }
  });
  el.openRoles.addEventListener('click', async () => {
    try {
      const dir = await invoke('agent_roles_dir', { kind });
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
    restart: ui.wantRestart,
  }));
  for (let i = 0; i < slots.length; i++) {
    // 只有真的要啟動／重開的格才需要選代理人類型（舊版同一個判斷）
    const act = actionOf(i);
    if ((act === 'start' || act === 'restart') && !slots[i].backend) {
      el.noBackend.hidden = false;
      el.noBackend.textContent = fmt('ma.dlgNeedBackend', `Agent-x${i + 1}`);
      return null;
    }
  }
  if (kind === 'chat' && slots.filter((s) => s.enabled).length < 2) {
    el.noBackend.hidden = false;
    el.noBackend.textContent = T['chat.dlgNeedTwo'];
    return null;
  }
  return {
    dir: el.dirPath,
    title: '',
    slots,
    // 聊天室的第一個下拉是「討論回合」，不是投遞上限
    maxMessages: kind === 'chat' ? 30 : Number(el.limit.value) || 0,
    idleCheckMinutes: kind === 'chat' ? 0 : Number(el.idle.value) || 0,
    sandbox: el.sandbox.checked,
    kind,
    rounds: kind === 'chat' ? Number(el.limit.value) || 3 : 3,
  };
}

/**
 * 開對話框，回傳 `TeamSetup`（取消＝null）。
 * `state` 給了＝既有團隊的「代理團隊設定…」（舊版 `MultiAgentDialog(dir, group)`）。
 */
async function openDialog(dir, state, wantKind) {
  kind = wantKind || (state && state.kind) || 'team';
  try {
    opts = await invoke('agent_setup_options', { kind });
  } catch (e) {
    log(`[agentdlg] 讀取設定選項失敗：${e}`);
    return null;
  }
  existing = state || null;
  // 聊天室沒有「閒置檢查」那一列（它不投遞信）
  for (const n of el.idleRow) n.hidden = kind === 'chat';
  el.dirPath = dir;
  el.dir.textContent = dir;
  // **先**依這次的 kind 建好下拉選項、**再**設值（B2）：以前順序相反，第一次開時選項還沒建，
  // 設的值無效 → 顯示第一個選項（10），按套用就把既有團隊的 30 改掉；團隊↔聊天室切換時
  // 選項是上一種的，回合數 3 也設不進去。
  applyTexts();
  el.limit.value =
    kind === 'chat'
      ? String(existing ? existing.rounds : opts.defaultRounds)
      : String(existing ? existing.maxMessages : opts.defaultMaxMessages);
  el.idle.value = String(existing ? existing.idleCheckMinutes : opts.defaultIdleCheck);
  // 沙盒是建團隊時決定的（worktree 已經開好），既有團隊不能改 → 顯示目前狀態並停用
  el.sandbox.checked = existing ? existing.sandbox : true;
  el.sandbox.disabled = !!existing;
  el.ok.textContent = existing ? T['ma.dlgApply'] : T['ma.dlgOpen'];

  // 沒有裝任何一家 CLI＝不能開新團隊（舊版 `ma.dlgNoBackend` ＋ 停用「開啟」）。
  // 既有團隊照樣能按套用——可能只是要關掉一格或改上限。
  const none = opts.backends.length === 0;
  el.noBackend.hidden = !none;
  if (none) el.noBackend.textContent = T['ma.dlgNoBackend'];
  el.ok.disabled = none && !existing;

  for (let i = 0; i < el.slotUi.length; i++) {
    const ui = el.slotUi[i];
    const slot = existing ? existing.slots.find((x) => x.index === i + 1) : null;
    const list = opts.backends.map((b) => ({ value: b.key, title: b.name }));
    // 用過的 CLI 這台已經找不到了也照樣列出來（舊版同款：否則那一格會顯示成別家的）
    if (slot && slot.backend && !list.some((b) => b.value === slot.backend)) {
      list.push({ value: slot.backend, title: slot.backend });
    }
    fillSelect(ui.backend, [{ value: '', title: '' }].concat(list), (b) => b.title);
    ui.wantRestart = false;
    if (slot && (slot.enabled || slot.state !== 'notRunning')) {
      // 執行中／已結束的格：沿用目前的 CLI 與角色
      ui.state = slot.state;
      ui.origBackend = slot.backend;
      ui.origRole = slot.role;
      ui.backend.value = slot.backend;
      ui.role.value = slot.role;
      ui.enable.checked = true;
    } else if (slot) {
      // 從沒啟動過＝預設；之前開過又被關掉的格＝沿用它上次的 CLI／角色（舊版同款）
      ui.state = 'notRunning';
      ui.origBackend = '';
      ui.origRole = '';
      ui.backend.value = slot.backend || (opts.backends.length ? opts.backends[0].key : '');
      ui.role.value = slot.backend ? slot.role : opts.defaultRoles[i] || '';
      ui.enable.checked = false;
    } else {
      ui.state = 'notRunning';
      ui.origBackend = '';
      ui.origRole = '';
      // 預設：每一格都用第一家找得到的 CLI（舊版也是拿第一個可用的）
      ui.backend.value = opts.backends.length ? opts.backends[0].key : '';
      ui.role.value = opts.defaultRoles[i] || '';
      // 新開的組：格 1、2 預設啟用（PM ＋ SE 是最小可用團隊），3、4 使用者自己勾
      ui.enable.checked = i < 2;
    }
    // 聊天室的第 1 位固定主持人（角色下拉停用，同舊版 `chat.hostFixed`）
    if (kind === 'chat' && i === 0) {
      ui.role.value = opts.hostRole;
      ui.role.disabled = true;
      ui.role.title = T['chat.hostFixed'];
    } else {
      ui.role.title = '';
    }
    refreshSlot(i);
  }

  el.root.hidden = false;
  return new Promise((resolve) => {
    resolveOpen = resolve;
  });
}

/**
 * 分頁右鍵「代理團隊設定…」：對**已經開著的**團隊套用設定（舊版 `AgentSetup_Click`）。
 *
 * 順序很重要：`agent_team_apply`（後端算出要關哪些、開哪些，並把 `suspend_relink` 打開）
 * → 前端由後往前關 → 逐格 `session_create` → `agent_team_apply_done`（重綁／拆組、通知 PM）。
 */
export async function openAgentSetup(key, createSession) {
  const { showInfo, askYesNo } = await import('./tabbar.js');
  let state;
  try {
    state = await invoke('agent_team_state', { key });
  } catch (e) {
    log(`[agentdlg] 讀團隊狀態失敗：${e}`);
    return;
  }
  if (!state) return;
  const setup = await openDialog(state.dir, state, state.kind);
  if (!setup) return;

  // 會結束執行中 agent 的變更要先確認（舊版 `ma.applyAsk`）
  const ends = [];
  for (let i = 0; i < 4; i++) {
    const act = actionOf(i);
    const slot = state.slots.find((x) => x.index === i + 1);
    if (!slot || slot.state !== 'running') continue;
    if (act === 'close') ends.push(fmt('ma.applyClose', agentLabel(state, i)));
    else if (act === 'restart') ends.push(fmt('ma.applyRestart', agentLabel(state, i)));
  }
  if (ends.length && !(await askYesNo(T['ma.title'], fmt('ma.applyAsk', ends.join('\n'))))) return;

  let plan;
  try {
    plan = await invoke('agent_team_apply', { key, setup });
  } catch (e) {
    log(`[agentdlg] 套用設定失敗：${e}`);
    await showInfo(T['ma.title'], String(e));
    return;
  }
  if (!plan.changed) {
    log('[agentdlg] 套用設定：沒有要變動的');
    return;
  }
  log(
    `[agentdlg] 套用設定：關 ${plan.closeTabs.join(',') || '-'}　開 ${
      plan.launch.map((x) => x.agentId).join(',') || '-'
    }`
  );
  // 由後往前關（前面的格還在時不會每關一個就重排——後端也擋著 suspend_relink）
  for (const id of plan.closeTabs.slice().reverse()) {
    await invoke('tab_close', { id }).catch((e) => log(`[agentdlg] 關閉分頁 ${id} 失敗：${e}`));
  }
  const failed = [];
  for (const slot of plan.launch) {
    try {
      await createSession({ kind: 'agent', agent: { team: key, index: slot.index } });
    } catch (e) {
      log(`[agentdlg] ${slot.agentId} 啟動失敗：${e}`);
      failed.push(String(e));
      await invoke('agent_slot_failed', { key, index: slot.index }).catch(() => {});
    }
  }
  await invoke('agent_team_apply_done', { key, rosterChanged: plan.rosterChanged }).catch((e) =>
    log(`[agentdlg] 套用收尾失敗：${e}`)
  );
  if (failed.length) await showInfo(T['ma.title'], failed.join('\n'));
}

/** pane 標題上的全名（`Agent-12 · Software Engineer · Codex`），確認對話框用。 */
function agentLabel(state, i) {
  const slot = state.slots.find((x) => x.index === i + 1);
  if (!slot) return `Agent-x${i + 1}`;
  const b = opts && opts.backends.find((x) => x.key === slot.backend);
  const role = opts && opts.roles.find((x) => x.key === slot.role);
  // 組號用後端給的 `number`：`key` 的前綴是建組當時的組號，恢復後可能已經換了（G8）
  return `Agent-${state.number}${slot.index} · ${role ? role.title : 'None'} · ${
    b ? b.name : slot.backend
  }`;
}

/**
 * 恢復代理團隊分頁（舊版 `RestoreAgentGroup`）。`entries` ＝`restore_list()` 的
 * `[{ index, tab }]`，同一個 `agentKey` 的那幾筆。
 */
export async function restoreAgentTeam(indices, createSession) {
  let plan;
  try {
    plan = await invoke('agent_team_restore', { indices });
  } catch (e) {
    // 資料夾不見了之類 → 這一組不恢復，其餘分頁照開（同舊版的 log-and-skip）
    log(`[agentdlg] 代理團隊不恢復：${e}`);
    return 0;
  }
  let ok = 0;
  for (const slot of plan.slots) {
    try {
      await createSession({
        kind: 'agent',
        agent: { team: plan.key, index: slot.index },
        restore: slot.restore === null ? undefined : slot.restore,
      });
      ok++;
    } catch (e) {
      log(`[agentdlg] 恢復 ${slot.agentId} 失敗：${e}`);
      await invoke('agent_slot_failed', { key: plan.key, index: slot.index }).catch(() => {});
    }
  }
  try {
    const n = await invoke('agent_team_ready', { key: plan.key });
    log(`[agentdlg] 代理團隊 ${plan.number} 已恢復：${n} 個 agent（工作區 ${plan.workDir}）`);
  } catch (e) {
    log(`[agentdlg] 代理團隊恢復收尾失敗：${e}`);
  }
  return ok;
}

/**
 * 「新分頁 ▾ → AI聊天室…」：選資料夾 → 設定視窗 → 開好之後**問主題**（舊版
 * `OpenChatRoom` → `AskChatTopic`）。取消主題也沒關係，之後右鍵「開始討論…」再給。
 */
export async function openChatRoom(createSession) {
  const { showInfo, askMultiline } = await import('./tabbar.js');
  let dir;
  try {
    dir = await invoke('pick_work_dir', { title: T['chat.pickDir'] });
  } catch (e) {
    log(`[agentdlg] 選資料夾失敗：${e}`);
    return;
  }
  if (!dir) return;
  const setup = await openDialog(dir, null, 'chat');
  if (!setup) return;

  let plan;
  try {
    plan = await invoke('agent_team_create', { setup });
  } catch (e) {
    log(`[agentdlg] 建聊天室失敗：${e}`);
    await showInfo(T['chat.title'], String(e));
    return;
  }
  log(
    `[agentdlg] 聊天室 ${plan.number} 計畫：${plan.slots
      .map((s) => `${s.agentId}/${s.backendName}/${s.roleTitle}`)
      .join(' ')}（工作區 ${plan.workDir}）`
  );
  const failed = [];
  for (const slot of plan.slots) {
    try {
      await createSession({ kind: 'agent', agent: { team: plan.key, index: slot.index } });
    } catch (e) {
      log(`[agentdlg] ${slot.agentId} 啟動失敗：${e}`);
      failed.push(String(e));
      await invoke('agent_slot_failed', { key: plan.key, index: slot.index }).catch(() => {});
    }
  }
  try {
    const n = await invoke('agent_team_ready', { key: plan.key });
    log(`[agentdlg] 聊天室 ${plan.number} 就緒：${n} 位參加者`);
  } catch (e) {
    await showInfo(T['chat.title'], String(e));
    return;
  }
  if (failed.length) await showInfo(T['chat.title'], failed.join('\n'));

  // 開好就問主題（舊版 `AskChatTopic`）。按取消也行，之後右鍵「開始討論…」再給。
  const topic = await askMultiline(T['chat.title'], T['chat.topicPrompt'], '');
  if (topic === null || !topic.trim()) return;
  try {
    const folder = await invoke('chat_start', { key: plan.key, topic });
    log(`[agentdlg] 聊天室 ${plan.number}：開始討論，紀錄資料夾 ${folder}`);
  } catch (e) {
    await showInfo(T['chat.title'], String(e));
  }
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
  const setup = await openDialog(dir, null, 'team');
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
