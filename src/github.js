// 我的最愛 →「GitHub…」（2.1.4，使用者要求）。
//
// 視窗上面是 repo 清單（GitHub 帳號的全部 repo），下面設定帳號、token（選填）與暫存目錄（預設桌面）。
// 點一個 repo：暫存目錄沒有 → 直接下載最新版；已經有 → 先看是不是最新，不是就問要不要更新。
// 做完在那個目錄開一個 PowerShell／終端機分頁。git 與 GitHub API 都在後端（`github.rs`）。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { iconImg } from './icons.js';
import { onLangChange } from './i18n.js';
import { log } from './bridge.js';

const el = {};
let repos = [];
/** 正在 clone／fetch／pull：這段期間清單點了不算（避免同一個 repo 連點兩次）。 */
let busy = false;
/** 由 tabbar 注入：開分頁、問是否、訊息框、toast。 */
let hooks = {};

function $(id) {
  return document.getElementById(id);
}

function applyTexts() {
  el.title.textContent = T['gh.title'];
  $('gh-user-label').textContent = T['gh.user'];
  $('gh-token-label').textContent = T['gh.token'];
  $('gh-dir-label').textContent = T['gh.dir'];
  el.browse.textContent = T['gh.browse'];
  el.save.textContent = T['gh.save'];
  el.close.textContent = T['dlg.close'];
}

function setNote(text, isError = false) {
  el.note.textContent = text || '';
  el.note.classList.toggle('error', !!isError);
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
  } catch (e) {
    repos = [];
    renderList(' ');
    setNote(String(e), true);
  }
}

async function saveConfig() {
  await invoke('github_set_config', {
    config: { user: el.user.value, token: el.token.value, dir: el.dir.value },
  });
}

/** 點了一個 repo：沒下載過就下載；有就檢查、落後就問要不要更新；最後開分頁。 */
async function pickRepo(name) {
  const repo = repos.find((r) => r.name === name);
  if (!repo || busy) return;
  busy = true;
  el.root.classList.add('busy');
  try {
    setNote(fmt('gh.checking', name));
    const st = await invoke('github_local_state', { name });
    let path = st.path;
    if (!st.exists) {
      setNote(fmt('gh.cloning', name));
      path = await invoke('github_clone', { name, cloneUrl: repo.cloneUrl });
      hooks.toast(fmt('gh.cloned', name));
    } else if (st.behind > 0) {
      const yes = await hooks.askYesNo(T['gh.title'], fmt('gh.askPull', name, st.behind));
      if (yes) {
        setNote(fmt('gh.pulling', name));
        await invoke('github_pull', { name });
        hooks.toast(fmt('gh.updated', name));
      }
    } else {
      hooks.toast(fmt('gh.upToDate', name));
    }
    setNote('');
    el.root.hidden = true;
    await hooks.createSession({ kind: 'shell', cwd: path });
  } catch (e) {
    log(`[github] ${name}：${e}`);
    setNote(String(e), true);
  } finally {
    busy = false;
    el.root.classList.remove('busy');
  }
}

/** 打開視窗（我的最愛下拉的「GitHub…」）。 */
export async function openGithub() {
  setNote('');
  try {
    const c = await invoke('github_get_config');
    el.user.value = c.user || '';
    el.token.value = c.token || '';
    el.dir.value = c.dir || '';
  } catch (e) {
    log(`[github] 讀不到設定：${e}`);
  }
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
  el.browse = $('gh-browse');
  el.save = $('gh-save');
  el.note = $('github-note');
  el.close = $('github-close');

  onLangChange(applyTexts);

  el.list.addEventListener('click', (e) => {
    const row = e.target.closest('[data-repo]');
    if (row) pickRepo(row.dataset.repo);
  });

  el.browse.addEventListener('click', async () => {
    const dir = await invoke('pick_work_dir', { title: T['gh.pickDir'] }).catch(() => null);
    if (dir) el.dir.value = dir;
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
