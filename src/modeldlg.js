// 選模型（2.0.2 新增，舊版沒有）。後端在 `src-tauri/src/agent/models.rs`。
//
// 使用者定的流程（2026-10-02）：
//
//   - **新開**（自訂連線：選完目錄之後；代理團隊：設定視窗每一格）→ 先問 CLI 目前有哪些
//     模型，再讓使用者選，預選上次用的。
//   - **恢復分頁／我的最愛**→ 照記下來的模型直接開；只有「原本的模型已經不在清單裡」才問。
//
// 「不在清單裡」只對**權威的清單**有意義（Codex／OpenCode 是 CLI 自己回報的）。Claude Code
// 與 Gemini 沒有列清單的指令，清單只是常用別名，使用者自己打的名稱不在裡面很正常。
//
// 元件 id 用 `mo-` 開頭——`md-` 已經被巨集對話框（macro.js）用掉了。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { onLangChange } from './i18n.js';
import { fillCombo, syncCombo, wireCombo } from './combo.js';

const el = {};
let resolveOpen = null;

function $(id) {
  return document.getElementById(id);
}

/** 模型名稱可以接在命令列上嗎（和後端 `valid_model` 同一條規則；空＝預設，永遠可以）。 */
export function validModel(model) {
  return model === '' || (/^[A-Za-z0-9._\-/:@[\]]{1,120}$/.test(model) && !model.startsWith('-'));
}

/** 這個模型是不是「原本有、現在清單裡沒有了」。清單不權威（或沒選模型）一律回 false。 */
export function isMissing(list, model) {
  if (!model || !list || !list.authoritative) return false;
  return !list.models.some((m) => m.id === model);
}

/** 下拉的選項：第一筆是「預設」，其餘是 CLI 回報的模型。 */
export function modelItems(list) {
  const items = [{ value: '', label: T['model.default'] }];
  for (const m of (list && list.models) || []) {
    items.push({ value: m.id, label: m.label && m.label !== m.id ? `${m.label}  (${m.id})` : m.id });
  }
  return items;
}

/** 下拉底下那一行說明：清單是哪裡來的。 */
export function modelHint(list) {
  if (!list) return '';
  if (list.source === 'cli' || list.source === 'cache') return fmt('model.hintCli', list.note);
  if (list.source === 'builtin') return T['model.hintBuiltin'];
  return list.note ? fmt('model.hintFailed', list.note) : T['model.hintNone'];
}

function applyTexts() {
  $('modeldlg-title').textContent = T['model.title'];
  $('mo-l-model').textContent = T['model.label'];
  el.model.placeholder = T['model.default'];
  el.ok.textContent = T['dlg.ok'];
  el.cancel.textContent = T['dlg.cancel'];
}

function close(value) {
  el.root.hidden = true;
  const r = resolveOpen;
  resolveOpen = null;
  if (r) r(value);
}

/**
 * 跳「選擇模型」視窗。回傳選的模型（`''`＝預設），取消＝`null`。
 *
 * @param {{ prompt: string, list: object|null, current?: string }} o
 */
export function askModel(o) {
  // 上一個還開著（不該發生）→ 當成取消
  if (resolveOpen) close(null);
  el.prompt.textContent = o.prompt || '';
  fillCombo(el.list, modelItems(o.list));
  el.model.value = o.current || '';
  syncCombo(el.model, el.list);
  el.hint.textContent = modelHint(o.list);
  el.note.textContent = '';
  el.root.hidden = false;
  el.model.focus();
  el.model.select();
  return new Promise((resolve) => {
    resolveOpen = resolve;
  });
}

/**
 * 設定裡的「開啟時選模型」有沒有勾（預設沒勾）。沒勾＝完全不問模型：開連線不跳視窗、
 * 團隊設定視窗沒有模型欄位、恢復分頁與我的最愛也不檢查模型還在不在。
 */
export async function askModelEnabled() {
  try {
    return !!(await invoke('settings_get')).askModelOnOpen;
  } catch {
    return false;
  }
}

/** 記住這家 CLI 這次選的模型（下次預選它）。 */
export function rememberModel(backend, model) {
  if (!backend) return;
  invoke('model_remember', { backend, model: model || '' }).catch(() => {});
}

/** 這條自訂連線看起來是 AI CLI 嗎（圖示或執行檔名；和後端 `adapters::backend_of` 同一個想法）。 */
function looksLikeAiCli(conn) {
  if (['claude-code', 'codex', 'opencode', 'geminicli'].includes(String(conn.icon || '').toLowerCase())) {
    return true;
  }
  const exe = String(conn.path || '').split(/[\\/]/).pop().toLowerCase();
  return /claude|codex|opencode|gemini/.test(exe);
}

/**
 * 開一條自訂連線之前問模型。
 *
 * 回傳：`undefined`＝不用問（這條連線不是 AI CLI，或設定裡沒勾「開啟時選模型」）；
 * `null`＝使用者取消；字串＝選的模型（`''`＝預設）。
 */
export async function pickConnModel(conn, toast) {
  // 設定裡沒勾「開啟時選模型」＝不問，照舊直接開
  if (!(await askModelEnabled())) return undefined;
  const connName = conn.name;
  // 「正在讀取模型清單…」只對看起來是 AI CLI 的連線顯示（WSL 之類的不要閃這一句）；
  // 到底是不是，由後端的 `conn_models` 說了算
  if (toast && looksLikeAiCli(conn)) toast(T['model.loading']);
  let list = null;
  try {
    list = await invoke('conn_models', { name: connName });
  } catch {
    return undefined; // 問不到就照舊直接開（不要因為清單壞掉而開不了分頁）
  }
  if (!list) return undefined;
  const picked = await askModel({
    prompt: fmt('model.prompt', connName),
    list,
    current: list.last || '',
  });
  if (picked === null) return null;
  rememberModel(list.backend, picked);
  return picked;
}

/**
 * 恢復分頁／我的最愛：記下來的模型還能用嗎。
 *
 * 還在清單裡（或清單不權威、或問不到）→ 原樣回傳，**不打擾使用者**。
 * 不在了 → 請使用者重選；取消＝`null`（呼叫端決定是不開、還是退回預設）。
 */
export async function resolveSavedModel(connName, model) {
  if (!model) return '';
  // 沒勾「開啟時選模型」＝不檢查、也不跳視窗，記下來的模型照用
  if (!(await askModelEnabled())) return model;
  let list = null;
  try {
    list = await invoke('conn_models', { name: connName });
  } catch {
    return model;
  }
  if (!isMissing(list, model)) return model;
  return askModel({
    prompt: fmt('model.missing', connName, model),
    list,
    current: '',
  });
}

export function initModelDialog() {
  el.root = $('modeldlg');
  el.form = $('modeldlg-box');
  el.prompt = $('mo-prompt');
  el.model = $('mo-model');
  el.list = $('mo-list');
  el.hint = $('mo-hint');
  el.note = $('mo-note');
  el.ok = $('mo-ok');
  el.cancel = $('mo-cancel');
  onLangChange(applyTexts);
  wireCombo(el.model, el.list, () => {
    el.note.textContent = '';
  });

  el.form.addEventListener('submit', (e) => {
    e.preventDefault();
    const model = el.model.value.trim();
    if (!validModel(model)) {
      el.note.textContent = T['model.invalid'];
      return;
    }
    close(model);
  });
  el.cancel.addEventListener('click', () => close(null));
  document.addEventListener(
    'keydown',
    (e) => {
      if (e.key === 'Escape' && !el.root.hidden) {
        // 蓋在別的對話框上面時，Esc 只關自己
        e.stopPropagation();
        close(null);
      }
    },
    true,
  );
}
