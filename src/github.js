// 工具列「GitHub ▾」（2.1.4 使用者要求；原本在我的最愛下拉，2.1.5 拉到工具列並改成下拉選單）。
//
// 下拉選單（同我的最愛）：上面是最常用的 repo（最多 10 個，後端 `github_recent` 依點的次數排），
// 最後是「設定…」。設定視窗上面是 repo 清單（GitHub 帳號的全部 repo），下面設定帳號、token（選填）、
// 暫存目錄（預設桌面）與開啟方式。
// 設定視窗是左右兩欄（2.1.5 使用者要求）：左邊清單點＝選取，右邊按「確定」才動手；下拉選單點了直接動手。
// 動手＝暫存目錄沒有 → 直接下載最新版；已經有 → 先看是不是最新，
// 不是就問要不要更新。做完在那個目錄開一個分頁：預設是「新分頁 ▾」裡第一個 AI agent（2.1.5），
// 可在設定改成指定的 agent 或 PowerShell。git 與 GitHub API 都在後端（`github.rs`）。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { iconImg, setToolLabel } from './icons.js';
import { onLangChange } from './i18n.js';
import { log } from './bridge.js';

const el = {};
let repos = [];
/** 正在 clone／fetch／pull：這段期間點了不算（避免同一個 repo 連點兩次）。 */
let busy = false;
/** 目前存的開啟方式（`settings.githubAgent`）：空＝自動、`@shell`＝PowerShell、其他＝自訂連線名稱。 */
let agentChoice = '';
/** 設定視窗左邊清單目前選到的 repo 名稱（null＝沒選）。 */
let selected = null;
/** 由 tabbar 注入：開分頁、問是否、訊息框、toast、選單、在資料夾開 agent。 */
let hooks = {};

function $(id) {
  return document.getElementById(id);
}

function applyTexts() {
  setToolLabel(el.btn, T['tb.github'] + ' ▾');
  el.btn.title = T['tip.github'];
  el.menuSettings.querySelector('.menu-label').textContent = T['fav.settings'];
  el.title.textContent = T['gh.title'];
  $('gh-user-label').textContent = T['gh.user'];
  $('gh-token-label').textContent = T['gh.token'];
  $('gh-dir-label').textContent = T['gh.dir'];
  $('gh-agent-label').textContent = T['gh.agent'];
  el.browse.textContent = T['gh.browse'];
  el.save.textContent = T['gh.save'];
  el.ok.textContent = T['dlg.ok'];
  el.close.textContent = T['dlg.close'];
  showSelected();
  if (!el.root.hidden) fillAgents();
}

function setNote(text, isError = false) {
  el.note.textContent = text || '';
  el.note.classList.toggle('error', !!isError);
}

/** 進度／結果：設定視窗開著就寫在視窗下面，從下拉選單點的（沒有視窗）就用 toast。 */
function progress(text) {
  if (!el.root.hidden) setNote(text);
  else if (text) hooks.toast(text);
}

function renderList(message) {
  el.list.textContent = '';
  if (message || repos.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'conns-empty';
    empty.textContent = message || T['gh.empty'];
    el.list.appendChild(empty);
    return;
  }
  for (const r of repos) {
    const row = document.createElement('div');
    row.className = 'conns-row';
    row.dataset.repo = r.name;
    row.classList.toggle('active', r.name === selected);
    row.title = [r.fullName, r.description].filter(Boolean).join('\n');
    const name = document.createElement('span');
    name.className = 'conns-row-name';
    name.textContent = r.name;
    row.append(iconImg('git', 'menu-ico'), name);
    if (r.private) {
      const tag = document.createElement('span');
      tag.className = 'conns-row-tag';
      tag.textContent = T['gh.private'];
      row.appendChild(tag);
    }
    if (r.fork) {
      const tag = document.createElement('span');
      tag.className = 'conns-row-tag on';
      tag.textContent = 'fork';
      row.appendChild(tag);
    }
    el.list.appendChild(row);
  }
}

/** 右下角那行「已選：xxx」；沒選就灰掉「確定」。 */
function showSelected() {
  const has = !!selected && repos.some((r) => r.name === selected);
  el.ok.disabled = !has;
  el.selected.textContent = has ? fmt('gh.selected', selected) : T['gh.pickOne'];
}

function select(name) {
  selected = name;
  for (const row of el.list.querySelectorAll('[data-repo]')) {
    row.classList.toggle('active', row.dataset.repo === name);
  }
  showSelected();
}

async function loadRepos() {
  if (!el.user.value.trim() && !el.token.value.trim()) {
    repos = [];
    renderList(T['gh.noUser']);
    return;
  }
  renderList(T['gh.loading']);
  try {
    repos = await invoke('github_list_repos');
    renderList();
    showSelected();
    // 有填 token 卻一個私人 repo 都沒有：fine-grained token 的 Repository access 預設是
    // 「Public repositories」，這時 API 照樣回 200、只是看不到私人的（2.1.5 使用者踩到）
    if (el.token.value.trim() && repos.length > 0 && !repos.some((r) => r.private)) {
      setNote(T['gh.onlyPublic']);
    }
  } catch (e) {
    repos = [];
    renderList(' ');
    showSelected();
    setNote(String(e), true);
  }
}

/** 開啟方式的下拉：自動（顯示實際會開哪個）→ 各自訂連線（新分頁的順序）→ PowerShell／終端機。 */
function fillAgents() {
  const { list, auto } = hooks.githubAgents();
  el.agent.textContent = '';
  const add = (value, text) => {
    const o = document.createElement('option');
    o.value = value;
    o.textContent = text;
    el.agent.appendChild(o);
  };
  add('', fmt('gh.agentAuto', auto ? auto.name : T['gh.agentShell']));
  for (const c of list) add(c.name, c.name);
  add('@shell', T['gh.agentShell']);
  // 存的那一條被刪掉了 → 顯示成「自動」（openInDir 也是退回自動）
  el.agent.value = [...el.agent.options].some((o) => o.value === agentChoice) ? agentChoice : '';
}

async function saveConfig() {
  agentChoice = el.agent.value;
  await invoke('github_set_config', {
    config: { user: el.user.value, token: el.token.value, dir: el.dir.value, agent: agentChoice },
  });
}

/** 點了一個 repo（`repo` 是清單或下拉選單的那一筆）：下載或檢查更新 → 記一次常用 → 開分頁。 */
async function openRepo(repo) {
  if (!repo || busy) return;
  const name = repo.name;
  busy = true;
  el.root.classList.add('busy');
  try {
    progress(fmt('gh.checking', name));
    const st = await invoke('github_local_state', { name });
    let path = st.path;
    if (!st.exists) {
      progress(fmt('gh.cloning', name));
      path = await invoke('github_clone', { name, cloneUrl: repo.cloneUrl });
      hooks.toast(fmt('gh.cloned', name));
    } else if (st.behind > 0) {
      const yes = await hooks.askYesNo(T['gh.title'], fmt('gh.askPull', name, st.behind));
      if (yes) {
        progress(fmt('gh.pulling', name));
        await invoke('github_pull', { name });
        hooks.toast(fmt('gh.updated', name));
      }
    } else {
      hooks.toast(fmt('gh.upToDate', name));
    }
    await invoke('github_record_use', {
      repo: {
        name: repo.name,
        fullName: repo.fullName,
        cloneUrl: repo.cloneUrl,
        private: !!repo.private,
      },
    }).catch((e) => log(`[github] 記錄常用失敗：${e}`));
    setNote('');
    el.root.hidden = true;
    await hooks.openInDir(agentChoice, path);
  } catch (e) {
    log(`[github] ${name}：${e}`);
    if (!el.root.hidden) setNote(String(e), true);
    else await hooks.showInfo(T['gh.title'], String(e));
  } finally {
    busy = false;
    el.root.classList.remove('busy');
  }
}

/** 下拉選單上面那幾個常用 repo。 */
async function renderMenu() {
  let recent = [];
  try {
    recent = await invoke('github_recent');
  } catch (e) {
    log(`[github] 讀不到常用 repo：${e}`);
  }
  el.menuRecent.textContent = '';
  if (recent.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'menu-item disabled';
    empty.textContent = T['gh.recentEmpty'];
    el.menuRecent.appendChild(empty);
  }
  for (const r of recent) {
    const item = document.createElement('div');
    item.className = 'menu-item with-icon';
    item.title = r.fullName;
    const text = document.createElement('span');
    text.className = 'menu-label';
    text.textContent = r.private ? `${r.name}（${T['gh.private']}）` : r.name;
    item.append(iconImg('git', 'menu-ico'), text);
    item.addEventListener('click', () => {
      hooks.hideMenus();
      openRepo(r);
    });
    el.menuRecent.appendChild(item);
  }
}

async function loadConfig() {
  try {
    const c = await invoke('github_get_config');
    el.user.value = c.user || '';
    el.token.value = c.token || '';
    el.dir.value = c.dir || '';
    agentChoice = c.agent || '';
  } catch (e) {
    log(`[github] 讀不到設定：${e}`);
  }
}

/** 打開設定視窗（下拉選單的「設定…」）。 */
export async function openGithub() {
  setNote('');
  await loadConfig();
  fillAgents();
  el.root.hidden = false;
  await loadRepos();
}

export function initGithub(injected) {
  hooks = injected;
  el.root = $('github');
  el.title = $('github-title');
  el.list = $('github-list');
  el.user = $('gh-user');
  el.token = $('gh-token');
  el.dir = $('gh-dir');
  el.agent = $('gh-agent');
  el.browse = $('gh-browse');
  el.save = $('gh-save');
  el.ok = $('gh-ok');
  el.selected = $('gh-selected');
  el.note = $('github-note');
  el.close = $('github-close');
  el.btn = $('btn-github');
  el.menu = $('github-menu');
  el.menuRecent = $('github-recent');
  el.menuSettings = el.menu.querySelector('[data-gh="settings"]');

  applyTexts();
  onLangChange(applyTexts);
  // 開啟方式要先讀出來：沒開過設定視窗就從下拉選單點 repo，也要照存的方式開
  loadConfig();

  el.btn.addEventListener('click', async (e) => {
    e.stopPropagation(); // 同我的最愛：不要讓 document 的 click 馬上把剛開的選單收掉
    const show = el.menu.hidden;
    hooks.hideMenus();
    if (!show) return;
    await renderMenu();
    hooks.showMenuUnder(el.menu, el.btn);
  });
  el.menuSettings.addEventListener('click', () => {
    hooks.hideMenus();
    openGithub().catch((e) => log(`[github] 開啟失敗：${e}`));
  });

  // 清單點＝選取；連點兩下＝等於按「確定」
  el.list.addEventListener('click', (e) => {
    const row = e.target.closest('[data-repo]');
    if (row) select(row.dataset.repo);
  });
  el.list.addEventListener('dblclick', (e) => {
    const row = e.target.closest('[data-repo]');
    if (row && !el.ok.disabled) el.ok.click();
  });
  el.ok.addEventListener('click', () => {
    openRepo(repos.find((r) => r.name === selected));
  });

  el.browse.addEventListener('click', async () => {
    const dir = await invoke('pick_work_dir', { title: T['gh.pickDir'] }).catch(() => null);
    if (dir) el.dir.value = dir;
  });

  // 開啟方式改了馬上存（不用再按「儲存並重新整理」、也不用重列 repo）
  el.agent.addEventListener('change', () => {
    saveConfig().catch((e) => setNote(String(e), true));
  });

  el.save.addEventListener('click', async () => {
    if (busy) return;
    setNote('');
    try {
      await saveConfig();
      // 暫存目錄清空＝用預設，重讀一次讓輸入框顯示實際位置
      const c = await invoke('github_get_config');
      el.dir.value = c.dir || '';
    } catch (e) {
      setNote(String(e), true);
      return;
    }
    await loadRepos();
  });

  el.close.addEventListener('click', () => {
    el.root.hidden = true;
  });
  document.addEventListener('keydown', (e) => {
    // 頁內 #modal 開著時 Esc 只關 modal（同 favs.js）
    if (e.key === 'Escape' && !el.root.hidden && $('modal').hidden) el.root.hidden = true;
  });
}
