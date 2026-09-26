// TTL 巨集的前端：選檔執行、對話框、錯誤顯示。
//
// 舊版對應 `MainWindow.MacroAction`：分頁右鍵「執行巨集…」→ 選 `.ttl` → 背景執行；
// 已經在跑的話先問「要停止巨集嗎？」。讀檔失敗跳「無法讀取巨集：」。
//
// Rust 端在 `src-tauri/src/ttl/runner.rs`：巨集在自己的執行緒上跑，要問使用者時
// emit `macro-dialog`，我們問完呼叫 `macro_answer`。
//
// ⚠️ 一個元件應付所有對話框種類（message／yesno／input／password／list）：
// 舊版是五個 WPF 視窗，我們用同一個頁內對話框換內容——少一堆重複的 markup。

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { T, fmt } from './strings.js';
import { log } from './bridge.js';

const el = {};
/** 目前正在等回答的對話框是哪個分頁的。 */
let pending = null;
let hooks = {};

function $(id) {
  return document.getElementById(id);
}

/** 分頁右鍵「執行巨集…」。已經在跑就先問要不要停。 */
export async function runMacroForTab(id, state) {
  const tab = (state.tabs || []).find((t) => t.id === id);
  if (tab && tab.macroState) {
    const stop = await hooks.askYesNo(T['macro.title'], T['macro.stopAsk']);
    if (stop) await invoke('macro_stop', { id });
    return;
  }
  // 選檔（舊版的篩選器：TeraTerm 巨集 (*.ttl) / 所有檔案）
  let path = null;
  try {
    path = await invoke('macro_pick_file');
  } catch (e) {
    log(`[macro] 選檔失敗：${e}`);
    return;
  }
  if (!path) return;
  try {
    const file = await invoke('macro_run', { id, path });
    log(`[macro] 開始執行 ${file}（分頁 ${id}）`);
  } catch (e) {
    await hooks.showInfo(T['macro.title'], String(e));
  }
}

// ------------------------------------------------------------ 對話框

function hideDialog() {
  el.root.hidden = true;
  el.input.hidden = true;
  el.list.hidden = true;
  el.no.hidden = true;
  el.cancel.hidden = true;
  el.input.type = 'text';
}

function answer(number, text, cancelled) {
  const id = pending;
  pending = null;
  hideDialog();
  if (id === null) return;
  invoke('macro_answer', { id, number, text, cancelled }).catch(() => {});
}

function showDialog(p) {
  pending = p.id;
  el.title.textContent = p.title || T['macro.title'];
  el.body.textContent = p.message || '';
  hideDialogParts();
  switch (p.kind) {
    case 'yesno':
      el.ok.textContent = T['dlg.yes'];
      el.no.hidden = false;
      el.no.textContent = T['dlg.no'];
      break;
    case 'input':
    case 'password':
      el.input.hidden = false;
      el.input.type = p.kind === 'password' ? 'password' : 'text';
      el.input.value = p.default || '';
      el.ok.textContent = T['dlg.ok'];
      el.cancel.hidden = false;
      break;
    case 'list':
      el.list.hidden = false;
      el.list.textContent = '';
      for (const item of p.items || []) {
        const opt = document.createElement('option');
        opt.textContent = item;
        el.list.appendChild(opt);
      }
      if (el.list.options.length > 0) el.list.selectedIndex = 0;
      el.ok.textContent = T['dlg.ok'];
      el.cancel.hidden = false;
      break;
    default: // message
      el.ok.textContent = T['dlg.ok'];
  }
  el.root.hidden = false;
  if (!el.input.hidden) {
    el.input.focus();
    el.input.select();
  } else {
    el.ok.focus();
  }
}

function hideDialogParts() {
  el.input.hidden = true;
  el.list.hidden = true;
  el.no.hidden = true;
  el.cancel.hidden = true;
}

/** `statusbox`：不等回覆的常駐提示（右下角）；`closesbox` 收掉。 */
function showStatus(p) {
  if (p.kind === 'closestatus') {
    el.status.hidden = true;
    el.status.textContent = '';
    return;
  }
  el.status.textContent = p.title ? `${p.title}：${p.message}` : p.message;
  el.status.hidden = false;
}

export async function initMacro(injected) {
  hooks = injected;
  el.root = $('macrodlg');
  el.form = $('macrodlg-box');
  el.title = $('macrodlg-title');
  el.body = $('macrodlg-body');
  el.input = $('md-input');
  el.list = $('md-list');
  el.ok = $('md-ok');
  el.no = $('md-no');
  el.cancel = $('md-cancel');
  el.status = $('macrostatus');

  el.form.addEventListener('submit', (e) => {
    e.preventDefault();
    if (!el.list.hidden) {
      answer(el.list.selectedIndex, '', false);
      return;
    }
    if (!el.input.hidden) {
      answer(1, el.input.value, false);
      return;
    }
    // message／yesno 的「是」
    answer(1, '', false);
  });
  el.no.addEventListener('click', () => answer(0, '', false));
  el.cancel.addEventListener('click', () => answer(-1, '', true));
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !el.root.hidden) {
      // 取消：`inputbox` 會是 result=0、`listbox` 是 -1（Rust 端看 cancelled）
      answer(-1, '', true);
    }
  });

  await listen('macro-dialog', (e) => {
    const p = e.payload;
    if (!p) return;
    if (p.kind === 'status' || p.kind === 'closestatus') {
      showStatus(p);
      return;
    }
    if (p.kind === 'filename' || p.kind === 'dirname') {
      // 走系統的檔案／資料夾選擇（Rust 端的 plugin）
      invoke(p.kind === 'dirname' ? 'pick_work_dir' : 'macro_pick_file', {
        title: p.message || p.title || '',
      })
        .then((picked) => answer(picked ? 1 : 0, picked || '', !picked))
        .catch(() => answer(0, '', true));
      return;
    }
    showDialog(p);
  });

  await listen('macro-error', (e) => {
    const p = e.payload;
    if (!p) return;
    // 舊版：巨集錯誤跳訊息視窗（我們畫面上也印了一行紅字）
    hooks.showInfo(
      T['macro.errorTitle'],
      fmt('macro.errorBody', p.message, p.file, p.line, p.text || ''),
    );
  });
}
