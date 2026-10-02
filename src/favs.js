// 我的最愛（搬移舊版 `MainWindow.Favorites.cs` + `Dialogs/FavoritesDialog`）。
//
// 舊版的下拉結構：各筆最愛（點了直接開）→ 分隔線 →「加到我的最愛：目前分頁」→「設定…」。
// 照抄，包含「目前分頁沒有可重開的資訊時把『加到我的最愛』灰掉」。
//
// **不存密碼**：一筆最愛就是 `SshConnParams`／工作目錄／自訂連線名稱，密碼當場問。
// 舊版的 `SavedTab` 同樣沒有密碼欄位（逐欄看過）。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { iconImg, setToolLabel } from './icons.js';
import { onLangChange } from './i18n.js';
import { log } from './bridge.js';

const el = {};
let items = [];
let selected = null;
/** 由 tabbar 注入：開一筆最愛、跳對話框、toast。 */
let hooks = {};

function $(id) {
  return document.getElementById(id);
}

/** 把介面文字重設一次（切語言時會被叫；註冊在 `i18n.js`）。 */
function applyTexts() {
  setToolLabel(el.btn, T['tb.favorites'] + ' ▾');
  el.btn.title = T['tip.favorites'];
  $('favs-title').textContent = T['fav.settings'];
  el.up.textContent = T['fav.up'];
  el.down.textContent = T['fav.down'];
  el.rename.textContent = T['menu.rename'];
  el.delete.textContent = T['conn.delete'];
  el.close.textContent = T['dlg.close'];
  // with-icon 項目只能改 `.menu-label`，寫整個 textContent 會把圖示洗掉（BUG-AUDIT B8）
  el.menu.querySelector('[data-fav="manage"] .menu-label').textContent = T['fav.settings'];
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
  if (f.kind === 'com' && f.com) {
    const c = f.com;
    const parts = [`COM  ${c.port} ${c.baud}`, `${c.dataBits}/${c.parity}/${c.stopBits}/${c.flow}`];
    if (c.autoReconnect) parts.push(T['sd.reconnect']);
    return parts.join('\n');
  }
  if (f.kind === 'telnet' && f.telnet) {
    const t = f.telnet;
    const parts = [`Telnet  ${t.host}${t.port === 23 ? '' : `:${t.port}`}`];
    if (t.autoReconnect) parts.push(T['sd.reconnect']);
    if (t.keepaliveMins > 0) parts.push(fmt('fav.tipKeepalive', t.keepaliveMins));
    return parts.join('\n');
  }
  if (f.kind === 'ssh' && f.ssh) {
    const s = f.ssh;
    const who = s.user ? `${s.user}@${s.host}` : s.host;
    const parts = [`SSH  ${who}${s.port === 22 ? '' : `:${s.port}`}`];
    if (s.keyPath) parts.push(fmt('fav.tipKey', s.keyPath));
    if (s.autoReconnect) parts.push(T['sd.reconnect']);
    if (s.keepaliveMins > 0) parts.push(fmt('fav.tipKeepalive', s.keepaliveMins));
    return parts.join('\n');
  }
  if (f.kind === 'conn') {
    const parts = [f.connName];
    if (f.dir) parts.push(f.dir);
    if (f.model) parts.push(fmt('model.tip', f.model));
    return parts.join('\n');
  }
  if (isTeam(f)) {
    // 代理團隊／聊天室：資料夾 ＋ 每一格是誰（舊版 `FavoriteDetail` 的 members）
    const parts = [`${T[f.kind === 'chat' ? 'chat.title' : 'ma.title']}  ${teamMembers(f)}`];
    if (f.dir) parts.push(f.dir);
    return parts.join('\n');
  }
  return f.dir || T['kind.powershell'];
}

/** 這一筆是代理團隊或 AI 聊天室（記的是整組設定）。 */
function isTeam(f) {
  return (f.kind === 'team' || f.kind === 'chat') && !!f.team;
}

/** 團隊成員的簡述：`ClaudeCode／Codex·gpt-6-sol`（有選模型的格多標模型）。 */
function teamMembers(f) {
  const names = { 'claude-code': 'ClaudeCode', codex: 'Codex', opencode: 'OpenCode', geminicli: 'GeminiCLI' };
  return ((f.team && f.team.slots) || [])
    .filter((s) => s.enabled)
    .map((s) => {
      const name = names[s.backend] || s.backend;
      return s.model ? `${name}·${s.model}` : name;
    })
    .join('／');
}

/** 種類 → 圖示 key（沿用分頁列那組 SVG）。 */
/**
 * 這一筆最愛用哪個圖示 key（舊版 `FavoriteIcon` → `HistoryIcon`，和 New 下拉同一組圖）。
 *
 * 自訂連線用那條連線自己的圖示；找不到那條連線（被刪掉了）就退回通用的 `run.png`。
 */
function iconOf(f) {
  // Telnet 沿用 SSH 的圖示（舊版 HistoryIcon 的 `"ssh" or "telnet" => "ssh-telnet.png"`）
  if (f.kind === 'com') return 'com';
  if (f.kind === 'ssh' || f.kind === 'telnet') return 'ssh-telnet';
  if (f.kind === 'conn') {
    const c = (hooks.conns ? hooks.conns() : []).find((x) => x.name === f.connName);
    return (c && c.icon) || 'run';
  }
  if (f.kind === 'team') return 'multi-agent';
  if (f.kind === 'chat') return 'chatroom';
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
    item.className = 'menu-item with-icon';
    item.dataset.favName = f.name;
    item.title = detailOf(f);
    const icon = iconImg(iconOf(f), 'menu-ico');
    const text = document.createElement('span');
    text.className = 'menu-label';
    text.textContent = f.name;
    item.append(icon, text);
    el.menuItems.appendChild(item);
  }
  // 舊版：沒有分頁、或這個分頁沒有可重開的資訊 → 灰掉
  const add = el.menu.querySelector('[data-fav="add"]');
  add.classList.toggle('disabled', !candidate);
  add.querySelector('.menu-label').textContent = candidate
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
    const icon = iconImg(iconOf(f), 'menu-ico');
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
          : f.kind === 'com'
            ? 'COM'
            : f.kind === 'conn'
              ? f.connName
              : f.kind === 'team'
                ? T['ma.title']
                : f.kind === 'chat'
                  ? T['chat.title']
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

/**
 * 就地改名（TASK-030）：把選中那一列的名稱換成輸入框。
 * Enter 確認、Esc 取消、失焦＝確認；存進 settings 後立刻重畫清單（下拉每次開都重讀，會跟上）。
 */
function startRename() {
  if (!selected) return;
  const row = el.list.querySelector(
    `[data-fav-name="${CSS.escape(selected)}"]`
  );
  const span = row && row.querySelector('.conns-row-name');
  if (!span || row.querySelector('input')) return;
  const old = selected;
  const input = document.createElement('input');
  input.type = 'text';
  input.className = 'favs-rename-input';
  input.value = old;
  input.spellcheck = false;
  span.replaceWith(input);
  input.focus();
  input.select();
  let done = false;
  const finish = async (commit) => {
    if (done) return;
    done = true;
    const name = input.value.trim();
    if (!commit || !name || name === old) {
      renderList(); // 取消／沒改：畫回原樣
      return;
    }
    try {
      await invoke('fav_rename', { name: old, newName: name });
      selected = name;
      el.note.textContent = '';
      await refreshAll();
    } catch (e) {
      // 名稱重複之類：顯示原因並留在原名（同舊版）
      el.note.textContent = String(e);
      renderList();
    }
  };
  input.addEventListener('keydown', (e) => {
    // 不讓 Esc 冒泡到 document（那個 listener 會把整個設定視窗關掉）
    e.stopPropagation();
    if (e.key === 'Enter') {
      e.preventDefault();
      finish(true);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      finish(false);
    }
  });
  input.addEventListener('blur', () => finish(true));
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
    if (f.kind === 'com' && f.com) {
      await hooks.createSession({ kind: 'com', com: f.com });
      return;
    }
    if (f.kind === 'conn') {
      // 模型照記下來的；只有它已經不在 CLI 的清單裡才再問一次（取消＝不開）
      const model = await hooks.resolveSavedModel(f.connName, f.model || '');
      if (model === null) return;
      // 有記下實際工作目錄就直接用，不再跳資料夾選擇（同舊版）
      await hooks.createSession({ kind: 'conn', conn: f.connName, cwd: f.dir || null, model });
      return;
    }
    if (isTeam(f)) {
      // 代理團隊／聊天室：照記下來的整組設定重開，不跳設定視窗（同舊版）
      await hooks.openTeam(f);
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
    kind === 'com'
      ? `${params.port} ${params.baud}`
      : kind === 'ssh' && params.user
        ? `${params.user}@${params.host}`
        : params.host;
  const name = await hooks.askText(T['fav.nameTitle'], T['fav.namePrompt'], who);
  if (name === null) return;
  await addItem({
    name: name.trim() || who,
    kind,
    dir: '',
    connName: '',
    ssh: kind === 'ssh' ? params : null,
    telnet: kind === 'telnet' ? params : null,
    com: kind === 'com' ? params : null,
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

  onLangChange(applyTexts);

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
    // 就地改名的輸入框在列裡面：點它不可以觸發重新選取（renderList 會把輸入框拆掉）
    if (e.target.tagName === 'INPUT') return;
    const row = e.target.closest('[data-fav-name]');
    if (!row) return;
    if (selected === row.dataset.favName) return;
    selected = row.dataset.favName;
    el.note.textContent = '';
    renderList();
  });

  // 改名＝就地編輯（TASK-030）：原本用 askText 對話框，但 #modal 疊在 #favs 底下
  // （z-index 300 < 320）根本看不到、也點不到 → 使用者以為「改名沒作用」。
  // 雙擊那一列、或選取後按「改名」都會進編輯；Enter 確認、Esc 取消、失焦＝確認。
  el.rename.addEventListener('click', () => startRename());
  el.list.addEventListener('dblclick', (e) => {
    if (e.target.tagName === 'INPUT') return;
    const row = e.target.closest('[data-fav-name]');
    if (!row) return;
    selected = row.dataset.favName;
    startRename();
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
    // 頁內 #modal（字型清單、確認框…）開著時 Esc 只關 modal，不連底下這個視窗一起關（BUG-AUDIT B3）
    if (e.key === 'Escape' && !el.root.hidden && document.getElementById('modal').hidden) el.root.hidden = true;
  });

  return reloadFavs();
}
