// 自訂連線設定 + 沙盒模式的前端（TASK-007）。
//
// 舊版對應 `Dialogs/CustomConnDialog.xaml(.cs)`：左邊清單、右邊編輯區、「自動偵測」按鈕。
// 這裡是頁內對話框（同本專案其他對話框的做法，見 REGRESSION-CHECKLIST 的「刻意不同」表）。
//
// **沙盒那個核取方塊是新功能**（`CLAUDE.md`「新增功能 → 沙盒模式」），舊版沒有。
// 規格重點：預設開啟；改設定**下次啟動該分頁才生效**（所以切換後要提示）。

import { invoke } from '@tauri-apps/api/core';

import { T } from './strings.js';
import { log } from './bridge.js';

const el = {};
let conns = [];
/** 目前在編輯哪一條（`null`＝新增中）。用名稱記，改名時當 `originalName` 送回後端。 */
let editingName = null;
let onChanged = () => {};

function $(id) {
  return document.getElementById(id);
}

/** 目前的自訂連線清單（「新分頁 ▾」要用）。 */
export function currentConns() {
  return conns;
}

export async function reload() {
  try {
    conns = await invoke('custom_list');
  } catch (e) {
    log(`[conns] 讀不到自訂連線：${e}`);
    conns = [];
  }
  onChanged(conns);
  if (!el.root.hidden) renderList();
  return conns;
}

function renderList() {
  el.list.textContent = '';
  if (conns.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'conns-empty';
    empty.textContent = T['conn.empty'];
    el.list.appendChild(empty);
    return;
  }
  for (const c of conns) {
    const row = document.createElement('div');
    row.className = 'conns-row' + (c.name === editingName ? ' active' : '');
    row.dataset.name = c.name;
    const name = document.createElement('span');
    name.className = 'conns-row-name';
    name.textContent = c.name;
    row.appendChild(name);
    // 沒開沙盒的要看得出來——這是使用者最在意的那個開關
    const tag = document.createElement('span');
    tag.className = 'conns-row-tag' + (c.sandbox ? ' on' : '');
    tag.textContent = c.sandbox ? T['conn.sandboxOn'] : T['conn.sandboxOff'];
    row.appendChild(tag);
    if (c.hidden) {
      const h = document.createElement('span');
      h.className = 'conns-row-tag';
      h.textContent = T['conn.hiddenTag'];
      row.appendChild(h);
    }
    el.list.appendChild(row);
  }
}

function loadForm(c) {
  el.name.value = c.name || '';
  el.path.value = c.path || '';
  el.args.value = c.args || '';
  el.closeKey.value = c.closeKey || 'ctrl-c';
  el.closeCount.value = c.closeCount || 3;
  el.pickDir.checked = !!c.pickDir;
  el.viaPs.checked = !!c.viaPowerShell;
  el.hidden.checked = !!c.hidden;
  // 新增時預設開沙盒（後端 CustomConn::default 也是 true，兩邊要一致）
  el.sandbox.checked = c.sandbox === undefined ? true : !!c.sandbox;
  el.delete.disabled = editingName === null;
}

function formToConn() {
  return {
    name: el.name.value.trim(),
    path: el.path.value.trim(),
    args: el.args.value.trim(),
    icon: (conns.find((c) => c.name === editingName) || {}).icon || 'run',
    closeKey: el.closeKey.value,
    closeCount: Math.min(5, Math.max(1, Number(el.closeCount.value) || 3)),
    pickDir: el.pickDir.checked,
    hidden: el.hidden.checked,
    viaPowerShell: el.viaPs.checked,
    sandbox: el.sandbox.checked,
  };
}

function note(text) {
  el.note.textContent = text || '';
}

export function openManager() {
  el.root.hidden = false;
  editingName = conns.length > 0 ? conns[0].name : null;
  loadForm(conns[0] || {});
  renderList();
  note('');
  el.name.focus();
}

function closeManager() {
  el.root.hidden = true;
}

export function initConns(onChangedCb) {
  onChanged = onChangedCb || (() => {});
  el.root = $('conns');
  el.list = $('conns-list');
  el.form = $('conns-form');
  el.note = $('conns-note');
  el.name = $('cf-name');
  el.path = $('cf-path');
  el.args = $('cf-args');
  el.closeKey = $('cf-closekey');
  el.closeCount = $('cf-closecount');
  el.pickDir = $('cf-pickdir');
  el.viaPs = $('cf-viaps');
  el.hidden = $('cf-hidden');
  el.sandbox = $('cf-sandbox');
  el.save = $('cf-save');
  el.delete = $('cf-delete');
  el.browse = $('cf-browse');
  el.detect = $('conns-detect');
  el.new = $('conns-new');
  el.close = $('conns-close');

  $('conns-title').textContent = T['conn.title'];
  el.detect.textContent = T['conn.detect'];
  el.new.textContent = T['conn.new'];
  el.close.textContent = T['dlg.close'];
  el.save.textContent = T['conn.save'];
  el.delete.textContent = T['conn.delete'];
  el.browse.textContent = T['log.browse'];

  el.list.addEventListener('click', (e) => {
    const row = e.target.closest('.conns-row');
    if (!row) return;
    editingName = row.dataset.name;
    loadForm(conns.find((c) => c.name === editingName) || {});
    renderList();
    note('');
  });

  el.new.addEventListener('click', () => {
    editingName = null;
    loadForm({});
    renderList();
    note(T['conn.newHint']);
    el.name.focus();
  });

  el.browse.addEventListener('click', async () => {
    // 沿用 log 的檔案選擇對話框（系統原生）；它回傳一個可寫入的路徑，選既有檔也可以
    const picked = await invoke('log_pick_path', { current: el.path.value });
    if (picked) el.path.value = picked;
  });

  el.form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const conn = formToConn();
    try {
      await invoke('custom_save', { conn, originalName: editingName });
      editingName = conn.name;
      await reload();
      renderList();
      note(T['conn.saved']);
    } catch (err) {
      note(String(err));
    }
  });

  el.delete.addEventListener('click', async () => {
    if (editingName === null) return;
    await invoke('custom_delete', { name: editingName });
    editingName = null;
    await reload();
    loadForm({});
    renderList();
    note(T['conn.deleted']);
  });

  el.detect.addEventListener('click', async () => {
    try {
      const added = await invoke('custom_detect');
      await reload();
      renderList();
      note(added.length === 0 ? T['conn.detectNone'] : `${T['conn.detectDone']}${added.join('、')}`);
    } catch (err) {
      note(String(err));
    }
  });

  el.close.addEventListener('click', closeManager);
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !el.root.hidden) closeManager();
  });

  return reload();
}
