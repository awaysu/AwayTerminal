// 連線對話框（SSH / Telnet）。
//
// 欄位照舊版 `Dialogs/ConnectDialog.xaml`：**類型（SSH／Telnet）**、IP/主機、Port、
// 保持連線（分鐘，0=關）、斷線自動重連。後面四個欄位舊版兩種類型是共用的，照抄。
// 切換類型時把 22 ↔ 23 換掉（同舊版 `TypeCombo` 的 SelectionChanged）。
// 舊版沒有的：帳號、金鑰檔、Pageant、以及「進階」的演算法與環境變數——
// 那些在舊版是 `ssh.exe` 的命令列參數（`-o SendEnv=…`）或根本沒有（內建 SSH 才有的東西）。
// Telnet 沒有驗證的概念，所以選 Telnet 時那幾列會收起來。
//
// **密碼沒有欄位**：連上之後在終端機裡問（同 PuTTY 與舊版），所以也不會被存起來。
// 這個對話框產出的物件就是 `SshConnParams`，也是「我的最愛」要存的內容。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { onLangChange } from './i18n.js';
import { log } from './bridge.js';

const el = {};
/** `algo_catalog()` 的結果：四組名稱 + 哪些在警告線下。 */
let catalog = null;
let resolveOpen = null;

function $(id) {
  return document.getElementById(id);
}

const GROUPS = [
  ['kex', 'sd.kex'],
  ['hostKey', 'sd.hostKey'],
  ['cipher', 'sd.cipher'],
  ['mac', 'sd.mac'],
];

function renderAlgos() {
  el.algos.textContent = '';
  if (!catalog) return;
  for (const [key, label] of GROUPS) {
    const wrap = document.createElement('div');
    wrap.className = 'sd-algo';
    const cap = document.createElement('div');
    cap.className = 'sd-algo-label';
    cap.textContent = T[label];
    const sel = document.createElement('select');
    sel.multiple = true;
    sel.size = 5;
    sel.dataset.group = key;
    for (const item of catalog[key] || []) {
      const opt = document.createElement('option');
      opt.value = item.name;
      // 警告線以下的標出來（同 PuTTY 的 warn below this line）
      opt.textContent = item.weak ? `⚠ ${item.name}` : item.name;
      if (item.weak) opt.className = 'weak';
      sel.appendChild(opt);
    }
    wrap.append(cap, sel);
    el.algos.appendChild(wrap);
  }
}

/** 讀「進階」的演算法選擇。都沒選＝不覆寫（用預設順序）。 */
function readAlgos() {
  const out = { kex: [], hostKey: [], cipher: [], mac: [] };
  for (const sel of el.algos.querySelectorAll('select')) {
    out[sel.dataset.group] = [...sel.selectedOptions].map((o) => o.value);
  }
  return out;
}

function writeAlgos(algos) {
  for (const sel of el.algos.querySelectorAll('select')) {
    const want = (algos && algos[sel.dataset.group]) || [];
    for (const opt of sel.options) opt.selected = want.includes(opt.value);
  }
}

/** 一行一個 `KEY=VALUE`。 */
function readEnv() {
  return el.env.value
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith('#'))
    .map((l) => {
      const i = l.indexOf('=');
      return i < 0 ? [l, ''] : [l.slice(0, i).trim(), l.slice(i + 1).trim()];
    })
    .filter(([k]) => k);
}

function writeEnv(env) {
  el.env.value = (env || []).map(([k, v]) => `${k}=${v}`).join('\n');
}

/** 目前選的類型（`ssh` / `telnet`）。 */
function kindOf() {
  return el.type.value === 'telnet' ? 'telnet' : 'ssh';
}

/** 依類型顯示／收起 SSH 專屬的列（Telnet 沒有帳號、金鑰、Pageant、演算法）。 */
function applyKind() {
  const telnet = kindOf() === 'telnet';
  for (const node of el.root.querySelectorAll('[data-ssh-only]')) {
    node.hidden = telnet;
  }
}

/** 把對話框的欄位讀成連線參數（`SshConnParams` 或 `TelnetParams`）。 */
function read() {
  if (kindOf() === 'telnet') {
    return {
      host: el.host.value.trim(),
      port: Math.min(65535, Math.max(1, Number(el.port.value) || 23)),
      keepaliveMins: Math.max(0, Number(el.keep.value) || 0),
      autoReconnect: el.reconnect.checked,
    };
  }
  return {
    host: el.host.value.trim(),
    port: Math.min(65535, Math.max(1, Number(el.port.value) || 22)),
    user: el.user.value.trim(),
    keyPath: el.key.value.trim(),
    useAgent: el.agent.checked,
    keepaliveMins: Math.max(0, Number(el.keep.value) || 0),
    autoReconnect: el.reconnect.checked,
    algos: readAlgos(),
    env: readEnv(),
  };
}

function write(p, kind) {
  el.type.value = kind === 'telnet' ? 'telnet' : 'ssh';
  applyKind();
  el.host.value = p.host || '';
  el.port.value = p.port || (kind === 'telnet' ? 23 : 22);
  el.user.value = p.user || '';
  el.key.value = p.keyPath || '';
  el.agent.checked = p.useAgent !== false;
  el.keep.value = p.keepaliveMins === undefined ? 10 : p.keepaliveMins;
  el.reconnect.checked = !!p.autoReconnect;
  writeAlgos(p.algos);
  writeEnv(p.env);
  // 有覆寫或環境變數就把「進階」展開，不然使用者不知道自己設過
  const adv = readAlgos();
  el.adv.open =
    (p.env && p.env.length > 0) ||
    GROUPS.some(([k]) => (adv[k] || []).length > 0);
}

function close(result) {
  el.root.hidden = true;
  const r = resolveOpen;
  resolveOpen = null;
  if (r) r(result);
}

/**
 * 開對話框。回傳 `{action: 'connect'|'favorite', kind, params}` 或 `null`（取消）。
 *
 * `defaults` 是設定裡的預設值（保持連線的分鐘數、自動重連），或是編輯既有的一筆最愛。
 * `kind` ＝一開始要選哪個類型（同舊版記住 `LastConnType`）。
 */
/** 把介面文字重設一次（切語言時會被叫；註冊在 `i18n.js`）。 */
function applyTexts() {
  $('sshdlg-title').textContent = T['sd.title'];
  $('sd-l-type').textContent = T['sd.type'];
  $('sd-l-host').textContent = T['sd.host'];
  $('sd-l-port').textContent = T['sd.port'];
  $('sd-l-user').textContent = T['sd.user'];
  $('sd-user-hint').textContent = T['sd.userHint'];
  $('sd-l-key').textContent = T['sd.key'];
  el.keyBrowse.textContent = T['log.browse'];
  $('sd-l-keep').textContent = T['sd.keep'];
  $('sd-keep-hint').textContent = T['sd.keepHint'];
  $('sd-l-agent').textContent = T['sd.agent'];
  $('sd-l-reconnect').textContent = T['sd.reconnect'];
  $('sd-l-adv').textContent = T['sd.adv'];
  $('sd-adv-note').textContent = T['sd.advNote'];
  $('sd-l-env').textContent = T['sd.env'];
  $('sd-env-hint').textContent = T['sd.envHint'];
  el.ok.textContent = T['sd.connect'];
  el.fav.textContent = T['sd.addFav'];
  el.cancel.textContent = T['dlg.cancel'];
}

export function openConnDialog(defaults, kind) {
  return new Promise((resolve) => {
    resolveOpen = resolve;
    write(defaults || {}, kind);
    el.note.textContent = '';
    el.root.hidden = false;
    el.host.focus();
    el.host.select();
  });
}

export async function initConnDialog() {
  el.root = $('sshdlg');
  el.form = $('sshdlg-box');
  el.type = $('sd-type');
  el.host = $('sd-host');
  el.port = $('sd-port');
  el.user = $('sd-user');
  el.key = $('sd-key');
  el.keyBrowse = $('sd-key-browse');
  el.keep = $('sd-keep');
  el.agent = $('sd-agent');
  el.reconnect = $('sd-reconnect');
  el.adv = $('sd-adv');
  el.algos = $('sd-algos');
  el.env = $('sd-env');
  el.note = $('sd-note');
  el.ok = $('sd-ok');
  el.fav = $('sd-fav');
  el.cancel = $('sd-cancel');

  onLangChange(applyTexts);

  try {
    catalog = await invoke('algo_catalog');
  } catch (e) {
    log(`[sshdlg] 讀不到演算法清單：${e}`);
  }
  renderAlgos();

  // 切換類型：把另一種的預設埠換掉（只換「還是預設值」的那個，使用者自己填的不動——同舊版）
  el.type.addEventListener('change', () => {
    const cur = Number(el.port.value) || 0;
    if (kindOf() === 'telnet' && cur === 22) el.port.value = 23;
    else if (kindOf() === 'ssh' && cur === 23) el.port.value = 22;
    applyKind();
  });

  el.keyBrowse.addEventListener('click', async () => {
    // 金鑰檔用系統的檔案選擇（OpenSSH 與 .ppk 都是純文字，不限副檔名）
    const picked = await invoke('log_pick_path', { current: el.key.value });
    if (picked) el.key.value = picked;
  });

  el.form.addEventListener('submit', (e) => {
    e.preventDefault();
    const params = read();
    if (!params.host) {
      el.note.textContent = T['sd.needHost'];
      return;
    }
    close({ action: 'connect', kind: kindOf(), params });
  });
  el.fav.addEventListener('click', () => {
    const params = read();
    if (!params.host) {
      el.note.textContent = T['sd.needHost'];
      return;
    }
    close({ action: 'favorite', kind: kindOf(), params });
  });
  el.cancel.addEventListener('click', () => close(null));
  document.addEventListener('keydown', (e) => {
    // 頁內 #modal（字型清單、確認框…）開著時 Esc 只關 modal，不連底下這個視窗一起關（BUG-AUDIT B3）
    if (e.key === 'Escape' && !el.root.hidden && document.getElementById('modal').hidden) close(null);
  });
}

/** `host` / `host:2222` / `[::1]:22` → 主機與埠（快速連線入口用；`fallback` ＝沒寫埠時用哪個）。 */
export function parseHostPort(text, fallback = 22) {
  const m = /^\[([^\]]+)\](?::(\d+))?$/.exec(text);
  if (m) return { host: m[1], port: m[2] ? Number(m[2]) : fallback };
  const i = text.lastIndexOf(':');
  if (i > 0 && text.indexOf(':') === i) {
    const port = Number(text.slice(i + 1));
    if (Number.isInteger(port) && port > 0 && port < 65536) {
      return { host: text.slice(0, i), port };
    }
  }
  return { host: text, port: fallback };
}

export { fmt };
