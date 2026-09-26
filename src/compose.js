// 「輸入文字」視窗（搬移舊版 `Dialogs/ComposeDialog`）。
//
// 用途照舊版：先在一般輸入框把文字打好（IME 在這裡組字，不經 xterm／ConPTY），
// 按「送出」才整段貼進作用中分頁——繞過 claude 對逐鍵輸入的重複／亂碼問題。
//
// 舊版是 WPF 的**模態**視窗；我們做成頁內對話框（同前面幾個），理由寫在 docs/COMPOSE.md：
// 頁內不必多裝視窗管理，而且 IME 組字在 textarea 裡和 WPF TextBox 一樣是「系統輸入法自己處理」。
//
// 草稿行為照舊版：
//   - 按 X／「返回」關掉 → 文字**留著**（下次叫回來還在）
//   - 「送出」之後才清空
//   - 「清除」掉的內容另存一份，即使關掉重開，按「復原」還救得回來

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { log } from './bridge.js';

const el = {};
let hooks = {};
/** 未送出的草稿（同舊版的 static `_draft`，程式存活期間有效）。 */
let draft = '';
/** 最近一次「清除」掉的內容（同舊版 `_lastCleared`，跨開關也能復原）。 */
let lastCleared = '';
/** 自己維護的 undo 堆疊（textarea 的原生 undo 在程式改值之後就不可靠了）。 */
let undoStack = [];

function $(id) {
  return document.getElementById(id);
}

function refresh() {
  const empty = el.input.value.length === 0;
  el.placeholder.hidden = !empty;
  el.send.disabled = empty;
}

/** 改文字並推進 undo 堆疊（同舊版走「全選＋取代選取」讓 Ctrl+Z 救得回來）。 */
function setText(text) {
  undoStack.push(el.input.value);
  if (undoStack.length > 50) undoStack.shift();
  el.input.value = text;
  el.input.selectionStart = el.input.selectionEnd = text.length;
  refresh();
}

export function openCompose(state) {
  const tabs = state.tabs || [];
  if (tabs.length === 0 || state.activeId === null || state.activeId === undefined) {
    hooks.toast(T['compose.noTab']);
    return;
  }
  // 送到「開視窗的那一刻」的作用中分頁（同舊版：期間切分頁也不會送錯）
  el.root.dataset.tabId = String(state.activeId);
  el.input.value = draft;
  undoStack = [];
  el.note.textContent = '';
  refresh();
  el.root.hidden = false;
  el.input.focus();
  el.input.selectionStart = el.input.selectionEnd = el.input.value.length;
}

function close(sent) {
  // 送出＝清空草稿；X／返回＝留著（同舊版 Closing 的那一行）
  draft = sent ? '' : el.input.value;
  el.root.hidden = true;
}

async function send() {
  const text = el.input.value;
  if (!text) return; // 空白不送（想只送 Enter 請直接在終端機按）
  const id = Number(el.root.dataset.tabId);
  try {
    await invoke('compose_send', { id, text, sendEnter: el.sendEnter.checked });
    close(true);
  } catch (e) {
    el.note.textContent = String(e);
  }
}

export async function initCompose(injected) {
  hooks = injected;
  el.root = $('composedlg');
  el.box = $('composedlg-box');
  el.title = $('composedlg-title');
  el.input = $('cp-input');
  el.placeholder = $('cp-placeholder');
  el.load = $('cp-load');
  el.clear = $('cp-clear');
  el.undo = $('cp-undo');
  el.save = $('cp-save');
  el.sendEnter = $('cp-sendenter');
  el.sendEnterLabel = $('cp-l-sendenter');
  el.back = $('cp-back');
  el.send = $('cp-send');
  el.note = $('cp-note');

  el.title.textContent = T['compose.title'];
  el.placeholder.textContent = T['compose.placeholder'];
  el.load.textContent = T['compose.loadFile'];
  el.clear.textContent = T['compose.clear'];
  el.undo.textContent = T['compose.undo'];
  el.save.textContent = T['compose.save'];
  el.sendEnterLabel.textContent = T['compose.sendEnter'];
  el.back.textContent = T['compose.back'];
  el.send.textContent = T['compose.send'];

  // 勾選狀態從設定來（舊版 AppSettings.ComposeSendEnter，預設開）
  try {
    const s = await invoke('settings_get');
    el.sendEnter.checked = s.composeSendEnter !== false;
  } catch {
    el.sendEnter.checked = true;
  }

  el.input.addEventListener('input', refresh);
  // Ctrl+Enter 送出（同舊版）
  el.input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      send();
    }
    // Ctrl+Z 走我們自己的堆疊（程式改過值之後原生 undo 不可靠）
    if (e.key === 'z' && (e.ctrlKey || e.metaKey) && undoStack.length > 0) {
      e.preventDefault();
      doUndo();
    }
  });

  el.load.addEventListener('click', async () => {
    try {
      const r = await invoke('compose_load_file');
      if (!r) return;
      if (el.input.value.length > 0) lastCleared = el.input.value;
      setText(r.text);
      el.note.textContent = fmt('compose.loaded', r.encoding);
    } catch (e) {
      el.note.textContent = String(e);
      log(`[compose] 載入失敗：${e}`);
    }
    el.input.focus();
  });

  el.clear.addEventListener('click', () => {
    if (el.input.value.length > 0) {
      lastCleared = el.input.value;
      setText('');
    }
    el.input.focus();
  });

  el.undo.addEventListener('click', doUndo);

  el.save.addEventListener('click', async () => {
    try {
      const path = await invoke('compose_save_file', { text: el.input.value });
      if (path) el.note.textContent = fmt('compose.saved', path);
    } catch (e) {
      el.note.textContent = String(e);
    }
    el.input.focus();
  });

  el.send.addEventListener('click', send);
  el.back.addEventListener('click', () => close(false));
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !el.root.hidden) close(false);
  });
}

/** 復原：先走自己的堆疊；堆疊空了而框是空的 → 把最近一次清除的內容放回（同舊版）。 */
function doUndo() {
  if (undoStack.length > 0) {
    el.input.value = undoStack.pop();
  } else if (el.input.value.length === 0 && lastCleared.length > 0) {
    el.input.value = lastCleared;
  }
  el.input.selectionStart = el.input.selectionEnd = el.input.value.length;
  refresh();
  el.input.focus();
}
