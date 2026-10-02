// 連接埠（COM）對話框。搬移舊版 `Dialogs/ComDialog.xaml(.cs)`。
//
// 舊版是**獨立的對話框**（不是 SSH/Telnet 那個「類型」下拉的一部分）：欄位完全不同，
// 而且有一個「回到預設」按鈕。這裡照舊版做成獨立對話框，入口是「新分頁 ▾ → 連接埠…」
// （舊版工具列的字面是「連接埠」，tooltip 是「開 COM 埠」）。
//
// 欄位、順序、預設值（COM5 / 115200 / 8 / None / 1 / None）都照舊版；
// 「加到我的最愛」是新版才有（舊版只能從分頁加）。
//
// ⚠️ 選項清單一律由後端的 `com_ports()` 給，不在前端寫死：`serialport` 不支援的值
// （Mark／Space 同位、1.5 停止位元、RTS/CTS+XON/XOFF）不會出現在清單裡。見 docs/COM.md。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { onLangChange } from './i18n.js';
import { log } from './bridge.js';

const el = {};
/** `com_ports()` 的結果。 */
let catalog = null;
let resolveOpen = null;

function $(id) {
  return document.getElementById(id);
}

/** 停止位元／流量控制的顯示文字（舊版 ComDialog 的 `AddTagged`：顯示友善名稱、值是列舉名）。 */
const STOP_LABEL = { One: '1', Two: '2' };
const FLOW_LABEL = {
  None: 'None',
  XOnXOff: 'XON/XOFF',
  RequestToSend: 'RTS/CTS',
  RequestToSendXOnXOff: 'RTS/CTS+XON/XOFF',
};

/** 可編輯下拉（Port／Baud rate）底下那個 `<select>` 的選項。 */
function fillList(node, values) {
  node.textContent = '';
  for (const v of values) {
    const opt = document.createElement('option');
    opt.value = String(v);
    opt.textContent = String(v);
    node.appendChild(opt);
  }
}

/** 讓底下的 `<select>` 跟著輸入框：字剛好是清單裡的就選起來，自己打的值＝不選任何一項。 */
function syncCombo(input, list) {
  list.value = input.value.trim();
}

/** 把一組 `<input>` ＋ `<select>` 接成可編輯下拉（舊版 `ComboBox IsEditable="True"`）。 */
function wireCombo(input, list) {
  list.addEventListener('change', () => {
    input.value = list.value;
    input.focus();
  });
  input.addEventListener('input', () => syncCombo(input, list));
  // 上下鍵在清單裡移動（同舊版的 ComboBox）
  input.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const n = list.options.length;
    if (n === 0) return;
    e.preventDefault();
    const step = e.key === 'ArrowDown' ? 1 : -1;
    const at = list.selectedIndex;
    list.selectedIndex = at < 0 ? (step > 0 ? 0 : n - 1) : Math.min(n - 1, Math.max(0, at + step));
    input.value = list.value;
  });
}

function fillSelect(node, values, labels) {
  node.textContent = '';
  for (const v of values) {
    const opt = document.createElement('option');
    opt.value = String(v);
    opt.textContent = labels ? labels[v] || String(v) : String(v);
    node.appendChild(opt);
  }
}

/** 重新掃描可用的埠（舊版每次開對話框都重新列一次）。 */
async function loadPorts(keep) {
  try {
    catalog = await invoke('com_ports');
  } catch (e) {
    log(`[comdlg] 列舉連接埠失敗：${e}`);
    catalog = null;
  }
  const ports = (catalog && catalog.ports) || [];
  // 舊版：清單一定含設定裡的那個埠（就算現在沒插上去也要能選）
  const names = ports.map((p) => p.name);
  if (keep && !names.includes(keep)) names.unshift(keep);
  fillList(el.portList, names);

  fillList(el.baudList, (catalog && catalog.bauds) || [115200]);
  fillSelect(el.data, (catalog && catalog.dataBits) || [8]);
  fillSelect(el.parity, (catalog && catalog.parities) || ['None']);
  fillSelect(el.stop, (catalog && catalog.stopBits) || ['One'], STOP_LABEL);
  fillSelect(el.flow, (catalog && catalog.flows) || ['None'], FLOW_LABEL);
  // 清單重建了（「重新掃描」）：把目前填的值重新對回去
  syncCombo(el.port, el.portList);
  syncCombo(el.baud, el.baudList);

  // 埠旁邊顯示 USB 描述，讓使用者分得出哪個是哪條線（舊版只有 COM 編號）
  el.portsNote.textContent =
    ports.length === 0
      ? T['cd.noPorts']
      : ports.map((p) => p.label).join('\n');
  el.portsNote.title = el.portsNote.textContent;
}

/** 把欄位讀成 `ComParams`。 */
function read() {
  return {
    // 不補預設埠：留空要讓送出時的 `cd.needPort` 擋下來，不是安靜地變成 COM1（BUG-AUDIT E14）
    port: el.port.value.trim(),
    baud: Math.max(1, Number(el.baud.value) || 115200),
    dataBits: Number(el.data.value) || 8,
    parity: el.parity.value || 'None',
    stopBits: el.stop.value || 'One',
    flow: el.flow.value || 'None',
    autoReconnect: el.reconnect.checked,
  };
}

function write(p) {
  el.port.value = p.port || 'COM5';
  el.baud.value = p.baud || 115200;
  el.data.value = String(p.dataBits || 8);
  el.parity.value = p.parity || 'None';
  el.stop.value = p.stopBits || 'One';
  el.flow.value = p.flow || 'None';
  el.reconnect.checked = !!p.autoReconnect;
  syncCombo(el.port, el.portList);
  syncCombo(el.baud, el.baudList);
}

/** 舊版「回到預設」：COM5 / 115200 / 8 / None / 1 / None（**不動**自動重連的勾選）。 */
function resetFields() {
  el.port.value = 'COM5';
  el.baud.value = '115200';
  el.data.value = '8';
  el.parity.value = 'None';
  el.stop.value = 'One';
  el.flow.value = 'None';
  syncCombo(el.port, el.portList);
  syncCombo(el.baud, el.baudList);
}

function close(result) {
  el.root.hidden = true;
  const r = resolveOpen;
  resolveOpen = null;
  if (r) r(result);
}

/**
 * 開對話框。回傳 `{action: 'open'|'favorite', params}` 或 `null`（取消）。
 *
 * `defaults` ＝設定裡上次用的值（同舊版：對話框開起來就是上次的設定）。
 */
/** 把介面文字重設一次（切語言時會被叫；註冊在 `i18n.js`）。 */
function applyTexts() {
  $('comdlg-title').textContent = T['com.title'];
  $('cd-l-port').textContent = T['cd.port'];
  $('cd-l-baud').textContent = T['cd.baud'];
  $('cd-l-data').textContent = T['cd.data'];
  $('cd-l-parity').textContent = T['cd.parity'];
  $('cd-l-stop').textContent = T['cd.stop'];
  $('cd-l-flow').textContent = T['cd.flow'];
  $('cd-l-reconnect').textContent = T['sd.reconnect'];
  el.refresh.textContent = T['cd.rescan'];
  el.reset.textContent = T['common.reset'];
  el.ok.textContent = T['com.open'];
  el.fav.textContent = T['sd.addFav'];
  el.cancel.textContent = T['dlg.cancel'];
}

export async function openComDialog(defaults) {
  const d = defaults || {};
  await loadPorts(d.port);
  write(d);
  el.note.textContent = '';
  el.root.hidden = false;
  el.port.focus();
  el.port.select();
  return new Promise((resolve) => {
    resolveOpen = resolve;
  });
}

export function initComDialog() {
  el.root = $('comdlg');
  el.form = $('comdlg-box');
  el.port = $('cd-port');
  el.portList = $('cd-port-list');
  el.refresh = $('cd-refresh');
  el.baud = $('cd-baud');
  el.baudList = $('cd-baud-list');
  el.data = $('cd-data');
  el.parity = $('cd-parity');
  el.stop = $('cd-stop');
  el.flow = $('cd-flow');
  el.reconnect = $('cd-reconnect');
  el.portsNote = $('cd-ports-note');
  el.note = $('cd-note');
  el.reset = $('cd-reset');
  el.ok = $('cd-ok');
  el.fav = $('cd-fav');
  el.cancel = $('cd-cancel');

  onLangChange(applyTexts);
  wireCombo(el.port, el.portList);
  wireCombo(el.baud, el.baudList);

  el.refresh.addEventListener('click', () => loadPorts(el.port.value.trim()));
  el.reset.addEventListener('click', resetFields);

  el.form.addEventListener('submit', (e) => {
    e.preventDefault();
    const params = read();
    if (!params.port) {
      el.note.textContent = T['cd.needPort'];
      return;
    }
    close({ action: 'open', params });
  });
  el.fav.addEventListener('click', () => {
    const params = read();
    if (!params.port) {
      el.note.textContent = T['cd.needPort'];
      return;
    }
    close({ action: 'favorite', params });
  });
  el.cancel.addEventListener('click', () => close(null));
  document.addEventListener('keydown', (e) => {
    // 頁內 #modal（字型清單、確認框…）開著時 Esc 只關 modal，不連底下這個視窗一起關（BUG-AUDIT B3）
    if (e.key === 'Escape' && !el.root.hidden && document.getElementById('modal').hidden) close(null);
  });
}

export { fmt };
