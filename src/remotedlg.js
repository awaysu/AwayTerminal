// 遠端設定（Telegram）——舊版 `Dialogs/RemoteDialog.xaml(.cs)` 的搬移版（TASK-029）。
//
// 放在哪裡的來回：v2 一開始併進「其他設定」的一個區塊，使用者以為這個功能沒做
//（`0066-Agent-11-to-Agent-12.md`）→ TASK-029 照舊版改回工具列獨立按鈕＋獨立視窗。
// 2.1.5 使用者要工具列騰位置給 GitHub：欄位**直接放在設定視窗的「遠端」那一類**（不再另開視窗），
// 和其他設定一起按「確定」才套用、按「取消」什麼都不動。設定視窗的左側分類寫著「遠端」，
// 不會再像當年那樣埋在一大頁裡找不到。
//
// 逐項照舊版 `RemoteDialog`：
//   欄位＝啟用／Bot Token／允許的 Chat ID（＋「取得 chat id」）／其他分頁完成也推播／說明；
//   **未勾「啟用」時下面全部鎖住**（舊版用 IsEnabled + Opacity 0.45，這裡是 disabled + `.rm-off`）。
//
// 由 `setdlg.js` 驅動：開設定視窗時 `fillRemote()`、按確定時先 `checkRemote()`（不過就不存、
// 視窗不關）再 `applyRemote()`。`telegram_apply` 會重啟遠端，所以**欄位沒改就不呼叫**。
//
// ⚠️ token：後端**不回傳** token 本身（只回 `hasToken`），所以這個欄位永遠是空的，
// placeholder 顯示「已設定，留空＝不變更」／「尚未設定」。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { onLangChange } from './i18n.js';
import { log } from './bridge.js';

const el = {};
let hooks = {};
/** 後端回報的目前狀態（`hasToken` 決定 placeholder 的字）。 */
let state = { hasToken: false, running: false, stoppedReason: null };
/** 開設定視窗時的值：按確定時和它比，沒改就不重啟遠端。 */
let opened = null;

function $(id) {
  return document.getElementById(id);
}

/** 未勾「啟用」→ 下面的欄位全部鎖住（舊版 `UpdateEnabledUI`）。 */
function applyEnabled() {
  const on = el.enabled.checked;
  el.token.disabled = !on;
  el.chat.disabled = !on;
  el.getId.disabled = !on;
  el.notify.disabled = !on;
  el.box.classList.toggle('rm-off', !on);
}

function current() {
  return {
    enabled: el.enabled.checked,
    chat: Number(el.chat.value.trim()) || 0,
    notify: el.notify.checked,
    token: el.token.value.trim(),
  };
}

function showState() {
  el.token.placeholder = T[state.hasToken ? 'remote.tokenSet' : 'remote.tokenNone'];
  // 停掉的原因優先顯示（TASK-036）：只寫「已停止」的話，token 失效的人
  // 會以為是程式壞了——後端會在 401／404 時自動關掉遠端並記下原因。
  el.note.textContent = state.stoppedReason
    ? state.stoppedReason
    : T[state.running ? 'remote.running' : 'remote.stopped'];
  el.note.classList.toggle('rm-error', !!state.stoppedReason);
}

/** 設定視窗打開時呼叫：把目前的值填進欄位。`s` 是 `settings_get` 的結果。 */
export async function fillRemote(s) {
  el.enabled.checked = !!s.remoteEnabled;
  el.chat.value = s.telegramChatId ? String(s.telegramChatId) : '';
  el.notify.checked = !!s.remoteNotify;
  el.token.value = '';
  applyEnabled();
  opened = current();
  try {
    state = await invoke('telegram_state');
  } catch {
    state = { hasToken: false, running: false, stoppedReason: null };
  }
  showState();
}

/** 「取得 chat id」（舊版 `GetId_Click`）。 */
async function getChatId() {
  // 欄位留空＝沿用已存的 token；兩邊都沒有才擋（舊版只看欄位，因為它會回填 token）
  const typed = el.token.value.trim();
  if (!typed && !state.hasToken) {
    await hooks.showInfo(T['remote.title'], T['remote.needToken']);
    return;
  }
  el.getId.disabled = true;
  try {
    const id = await invoke('telegram_get_chat_id', { token: typed || null });
    if (id === null || id === undefined) {
      await hooks.showInfo(T['remote.title'], T['remote.noUpdates']);
      return;
    }
    el.chat.value = String(id);
    await hooks.showInfo(T['remote.title'], fmt('remote.gotChatId', id));
  } catch (e) {
    // 錯誤訊息裡不會有 URL／token（後端的 `describe()` 只留型別與 HTTP 狀態）
    el.note.textContent = String(e);
    log(`[remote] 取得 chat id 失敗：${e}`);
  } finally {
    // 查詢期間若取消勾選，仍要維持灰色（舊版註解就是這一句）
    el.getId.disabled = !el.enabled.checked;
  }
}

function changed() {
  const now = current();
  return (
    !opened ||
    now.token !== '' ||
    now.enabled !== opened.enabled ||
    now.chat !== opened.chat ||
    now.notify !== opened.notify
  );
}

/**
 * 按確定前的檢查。勾了「啟用」卻沒有 token／chat id：後端只會記 log 回「未啟動」，
 * 使用者看到視窗關了以為開好了（BUG-AUDIT B15）→ 先擋下來。回傳 false＝不要存。
 */
export async function checkRemote() {
  if (!changed() || !el.enabled.checked) return true;
  if (!el.token.value.trim() && !state.hasToken) {
    await hooks.showInfo(T['remote.title'], T['remote.needToken']);
    el.token.focus();
    return false;
  }
  if (!(Number(el.chat.value.trim()) > 0)) {
    await hooks.showInfo(T['remote.title'], T['remote.needChatId']);
    el.chat.focus();
    return false;
  }
  return true;
}

/** 套用（有改才呼叫 `telegram_apply`）。回傳錯誤訊息；成功或沒改回 null。 */
export async function applyRemote() {
  if (!changed()) return null;
  const now = current();
  try {
    const st = await invoke('telegram_apply', {
      enabled: now.enabled,
      // 留空＝不變更（後端收到 null 就不動已存的 token）
      token: now.token ? now.token : null,
      chatId: now.chat,
      notify: now.notify,
    });
    // ⚠️ 這一行**不可以**印 token（後端也不回傳）
    log(
      `[remote] Telegram 遠端：${st.running ? '已啟動' : '未啟動'} chat=${st.chatId} ` +
        `token=${st.hasToken ? '已設定' : '未設定'}`
    );
    state = st;
    el.token.value = '';
    opened = current();
    showState();
    if (now.enabled && !st.running) {
      // 前面擋過了還是沒啟動（例：後端另有原因）→ 說出來，不要靜靜關窗
      return st.stoppedReason || T['remote.notStarted'];
    }
    return null;
  } catch (e) {
    return String(e);
  }
}

function applyTexts() {
  el.legend.textContent = T['tip.remote'];
  el.lEnabled.textContent = T['remote.enable'];
  el.lToken.textContent = T['remote.token'];
  el.lChat.textContent = T['remote.chatId'];
  el.getId.textContent = T['remote.getChatId'];
  el.lNotify.textContent = T['remote.notify'];
  el.hint.textContent = T['remote.hint'];
  showState();
}

export function initRemoteDialog(injected) {
  hooks = injected || {};
  el.box = $('rm-box');
  el.legend = $('st-l-remote');
  el.enabled = $('rm-enabled');
  el.lEnabled = $('rm-l-enabled');
  el.token = $('rm-token');
  el.lToken = $('rm-l-token');
  el.chat = $('rm-chat');
  el.lChat = $('rm-l-chat');
  el.getId = $('rm-getid');
  el.notify = $('rm-notify');
  el.lNotify = $('rm-l-notify');
  el.hint = $('rm-hint');
  el.note = $('rm-note');

  applyTexts();
  onLangChange(applyTexts);

  el.enabled.addEventListener('change', applyEnabled);
  el.getId.addEventListener('click', () => {
    getChatId().catch((e) => log(`[remote] 取得 chat id 失敗：${e}`));
  });
}
