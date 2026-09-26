// 我的最愛（搬移舊版 `MainWindow.Favorites.cs` + `Dialogs/FavoritesDialog`）。
//
// 舊版的下拉結構：各筆最愛（點了直接開）→ 分隔線 →「加到我的最愛：目前分頁」→「設定…」。
// 照抄，包含「目前分頁沒有可重開的資訊時把『加到我的最愛』灰掉」。
//
// **不存密碼**：一筆最愛就是 `SshConnParams`／工作目錄／自訂連線名稱，密碼當場問。
// 舊版的 `SavedTab` 同樣沒有密碼欄位（逐欄看過）。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt, iconSvg } from './strings.js';
import { log } from './bridge.js';

const el = {};
let items = [];
let selected = null;
/** 由 tabbar 注入：開一筆最愛、跳對話框、toast。 */
let hooks = {};

function $(id) {
  return document.getElementById(id);
}

export async function reloadFavs() {
  try {
    items = await invoke('fav_list');
  } catch (e) {
    log(`[favs] 讀不到我的最愛：${e}`);
    items = [];
  }
  return items;
}

/** 一筆最愛的說明文字（下拉的 tooltip，同舊版 `FavoriteDetail`）。 */
function detailOf(f) {
  if (f.kind === 'telnet' && f.telnet) {
    const t = f.telnet;
    const parts = [`Telnet  ${t.host}${t.port === 23 ? '' : `:${t.port}`}`];
    if (t.autoReconnect) parts.push('斷線自動重連');
    if (t.keepaliveMins > 0) parts.push(`保持連線 ${t.keepaliveMins} 分鐘`);
    return parts.join('\n');
  }
  if (f.kind === 'ssh' && f.ssh) {
    const s = f.ssh;
    const who = s.user ? `${s.user}@${s.host}` : s.host;
    const parts = [`SSH  ${who}${s.port === 22 ? '' : `:${s.port}`}`];
    if (s.keyPath) parts.push(`金鑰：${s.keyPath}`);
    if (s.autoReconnect) parts.push('斷線自動重連');
    if (s.keepaliveMins > 0) parts.push(`保持連線 ${s.keepaliveMins} 分鐘`);
    return parts.join('\n');
  }
  if (f.kind === 'conn') return `${f.connName}${f.dir ? `\n${f.dir}` : ''}`;
  return f.dir || T['kind.powershell'];
}

/** 種類 → 圖示 key（沿用分頁列那組 SVG）。 */
function iconOf(f) {
  // Telnet 沿用 SSH 的圖示（舊版的分頁圖示也是同一個「遠端連線」概念）
  if (f.kind === 'ssh' || f.kind === 'telnet') return 'ssh';
  if (f.kind === 'conn') return 'custom';
  return 'powershell';
}

function renderMenu(candidate) {
  el.menuItems.textContent = '';
  if (items.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'menu-item disabled';
    empty.textContent = T['fav.empty'];
    el.menuItems.appendChild(empty);
  }
  for (const f of items) {
    const item = document.createElement('div');
    item.className = 'menu-item';
    item.dataset.favName = f.name;
    item.title = detailOf(f);
    const icon = document.createElement('span');
    icon.className = 'menu-icon';
    icon.innerHTML = iconSvg(iconOf(f));
    const text = document.createElement('span');
    text.textContent = f.name;
    item.append(icon, text);
    el.menuItems.appendChild(item);
  }
  // 舊版：沒有分頁、或這個分頁沒有可重開的資訊 → 灰掉
  const add = el.menu.querySelector('[data-fav="add"]');
  add.classList.toggle('disabled', !candidate);
  add.textContent = candidate
    ? fmt('fav.addNamed', candidate.name)
    : T['fav.add'];
}

// ------------------------------------------------------------ 設定對話框

function renderList() {
  el.list.textContent = '';
  if (items.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'conns-empty';
    empty.textContent = T['fav.empty'];
    el.list.appendChild(empty);
    return;
  }
  for (const f of items) {
    const row = document.createElement('div');
    row.className = 'conns-row' + (f.name === selected ? ' active' : '');
    row.dataset.favName = f.name;
    const icon = document.createElement('span');
    icon.className = 'menu-icon';
    icon.innerHTML = iconSvg(iconOf(f));
    const name = document.createElement('span');
    name.className = 'conns-row-name';
    name.textContent = f.name;
    const tag = document.createElement('span');
    tag.className = 'conns-row-tag on';
    tag.textContent =
      f.kind === 'ssh'
        ? 'SSH'
        : f.kind === 'telnet'
          ? 'Telnet'
          : f.kind === 'conn'
            ? f.connName
            : 'PowerShell';
    row.append(icon, name, tag);
    row.title = detailOf(f);
    el.list.appendChild(row);
  }
}

function openManager() {
  selected = items.length > 0 ? items[0].name : null;
  el.note.textContent = '';
  renderList();
  el.root.hidden = false;
}

async function refreshAll() {
  await reloadFavs();
  renderList();
}

// ------------------------------------------------------------------ 對外

/** 把一筆最愛開起來。SSH 走 `SshConnParams`，其餘走既有的入口。 */
export async function openFavorite(name) {
  const f = items.find((x) => x.name === name);
  if (!f) return;
  try {
    if (f.kind === 'ssh' && f.ssh) {
      await hooks.createSession({ kind: 'ssh', ssh: f.ssh });
      return;
    }
    if (f.kind === 'telnet' && f.telnet) {
      await hooks.createSession({ kind: 'telnet', telnet: f.telnet });
      return;
    }
    if (f.kind === 'conn') {
      // 有記下實際工作目錄就直接用，不再跳資料夾選擇（同舊版）
      await hooks.createSession({ kind: 'conn', conn: f.connName, cwd: f.dir || null });
      return;
    }
    await hooks.createSession({ kind: 'shell', cwd: f.dir || null });
  } catch (e) {
    log(`[favs] 開啟失敗：${e}`);
    await hooks.showInfo(T['msg.connectFail'], String(e));
  }
}

/** 把一組連線參數存成最愛（連線對話框的「加到我的最愛」按鈕）。 */
export async function addConnFavorite(kind, params) {
  const who =
    kind === 'ssh' && params.user ? `${params.user}@${params.host}` : params.host;
  const name = await hooks.askText(T['fav.nameTitle'], T['fav.namePrompt'], who);
  if (name === null) return;
  await addItem({
    name: name.trim() || who,
    kind,
    dir: '',
    connName: '',
    ssh: kind === 'ssh' ? params : null,
    telnet: kind === 'telnet' ? params : null,
  });
}

async function addItem(item) {
  try {
    const saved = await invoke('fav_add', { item });
    await reloadFavs();
    hooks.toast(fmt('fav.added', saved));
  } catch (e) {
    // 「已經在我的最愛裡了」也走這裡——舊版同樣只是提示，不是錯誤
    hooks.toast(String(e));
  }
}

export function initFavs(injected) {
  hooks = injected;
  el.btn = $('btn-favs');
  el.menu = $('favs-menu');
  el.menuItems = $('favs-items');
  el.root = $('favs');
  el.list = $('favs-list');
  el.note = $('favs-note');
  el.up = $('favs-up');
  el.down = $('favs-down');
  el.rename = $('favs-rename');
  el.delete = $('favs-delete');
  el.close = $('favs-close');

  el.btn.textContent = T['tb.favorites'] + ' ▾';
  el.btn.title = T['tip.favorites'];
  $('favs-title').textContent = T['fav.settings'];
  el.up.textContent = T['fav.up'];
  el.down.textContent = T['fav.down'];
  el.rename.textContent = T['menu.rename'];
  el.delete.textContent = T['conn.delete'];
  el.close.textContent = T['dlg.close'];
  el.menu.querySelector('[data-fav="manage"]').textContent = T['fav.settings'];

  el.btn.addEventListener('click', async (e) => {
    e.stopPropagation();
    const show = el.menu.hidden;
    hooks.hideMenus();
    if (!show) return;
    await reloadFavs();
    let candidate = null;
    const active = hooks.activeId();
    if (active !== null && active !== undefined) {
      candidate = await invoke('fav_candidate', { id: active }).catch(() => null);
    }
    renderMenu(candidate);
    hooks.showMenuUnder(el.menu, el.btn);
    el.menu.dataset.candidate = candidate ? JSON.stringify(candidate) : '';
  });

  el.menu.addEventListener('click', async (e) => {
    const open = e.target.closest('[data-fav-name]');
    if (open) {
      hooks.hideMenus();
      await openFavorite(open.dataset.favName);
      return;
    }
    const act = e.target.closest('[data-fav]');
    if (!act || act.classList.contains('disabled')) return;
    hooks.hideMenus();
    if (act.dataset.fav === 'manage') {
      openManager();
      return;
    }
    // 加到我的最愛：目前分頁
    const raw = el.menu.dataset.candidate;
    if (!raw) return;
    await addItem(JSON.parse(raw));
  });

  el.list.addEventListener('click', (e) => {
    const row = e.target.closest('[data-fav-name]');
    if (!row) return;
    selected = row.dataset.favName;
    el.note.textContent = '';
    renderList();
  });

  el.rename.addEventListener('click', async () => {
    if (!selected) return;
    const name = await hooks.askText(T['fav.nameTitle'], T['fav.namePrompt'], selected);
    if (name === null || !name.trim()) return;
    try {
      await invoke('fav_rename', { name: selected, newName: name.trim() });
      selected = name.trim();
      await refreshAll();
    } catch (e) {
      el.note.textContent = String(e);
    }
  });

  el.delete.addEventListener('click', async () => {
    if (!selected) return;
    const ok = await hooks.askYesNo(T['fav.settings'], fmt('fav.deleteAsk', selected));
    if (!ok) return;
    await invoke('fav_delete', { name: selected });
    selected = null;
    await refreshAll();
  });

  for (const [btn, delta] of [
    [el.up, -1],
    [el.down, 1],
  ]) {
    btn.addEventListener('click', async () => {
      if (!selected) return;
      await invoke('fav_move', { name: selected, delta });
      await refreshAll();
    });
  }

  el.close.addEventListener('click', () => {
    el.root.hidden = true;
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !el.root.hidden) el.root.hidden = true;
  });

  return reloadFavs();
}
