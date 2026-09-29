// 遠端設定（Telegram）——舊版 `Dialogs/RemoteDialog.xaml(.cs)` 的搬移版（TASK-029）。
//
// 為什麼獨立成一個視窗：舊版工具列就有「遠端設定」這顆按鈕、按了開自己的視窗。
// v2 一開始把它併進「其他設定」的一個區塊，結果使用者以為這個功能沒做
//（`0066-Agent-11-to-Agent-12.md`）。現在照舊版：獨立按鈕 ＋ 獨立視窗，
// 設定視窗那一塊同時**拿掉**——兩個地方改同一組欄位會互相蓋。
//
// 逐項照舊版 `RemoteDialog`：
//   欄位＝啟用／Bot Token／允許的 Chat ID（＋「取得 chat id」）／其他分頁完成也推播／說明；
//   **未勾「啟用」時下面全部鎖住**（舊版用 IsEnabled + Opacity 0.45，這裡是 disabled + `.rm-off`）；
//   按「儲存」才寫進設定並重啟遠端，「取消」什麼都不動。
//
// ⚠️ token：後端**不回傳** token 本身（只回 `hasToken`），所以這個欄位永遠是空的，
// placeholder 顯示「已設定，留空＝不變更」／「尚未設定」。同 `setdlg.js` 本來的做法。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { setToolLabel } from './icons.js';
import { onLangChange } from './i18n.js';
import { log } from './bridge.js';

const el = {};
let hooks = {};
/** 後端回報的目前狀態（`hasToken` 決定 placeholder 的字）。 */
let state = { hasToken: false, running: false, stoppedReason: null };

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
  el.root.classList.toggle('rm-off', !on);
}

async function fill() {
  const s = await invoke('settings_get');
  el.enabled.checked = !!s.remoteEnabled;
  el.chat.value = s.telegramChatId ? String(s.telegramChatId) : '';
  el.notify.checked = !!s.remoteNotify;
  el.token.value = '';
  el.note.textContent = '';
  applyEnabled();
  try {
    state = await invoke('telegram_state');
  } catch {
    state = { hasToken: false, running: false, stoppedReason: null };
  }
  el.token.placeholder = T[state.hasToken ? 'remote.tokenSet' : 'remote.tokenNone'];
  // 停掉的原因優先顯示（TASK-036）：只寫「已停止」的話，token 失效的人
  // 會以為是程式壞了——後端會在 401／404 時自動關掉遠端並記下原因。
  el.note.textContent = state.stoppedReason
    ? state.stoppedReason
    : T[state.running ? 'remote.running' : 'remote.stopped'];
  el.note.classList.toggle('rm-error', !!state.stoppedReason);
}

export async function openRemote() {
  await fill();
  el.root.hidden = false;
  el.enabled.focus();
}

function close() {
  el.root.hidden = true;
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

async function save() {
  try {
    const st = await invoke('telegram_apply', {
      enabled: el.enabled.checked,
      // 留空＝不變更（後端收到 null 就不動已存的 token）
      token: el.token.value.trim() ? el.token.value.trim() : null,
      chatId: Number(el.chat.value.trim()) || 0,
      notify: el.notify.checked,
    });
    // ⚠️ 這一行**不可以**印 token（後端也不回傳）
    log(
      `[remote] Telegram 遠端：${st.running ? '已啟動' : '未啟動'} chat=${st.chatId} ` +
        `token=${st.hasToken ? '已設定' : '未設定'}`
    );
    close();
  } catch (e) {
    el.note.textContent = String(e);
  }
}

function applyTexts() {
  el.title.textContent = T['remote.title'];
  el.lEnabled.textContent = T['remote.enable'];
  el.lToken.textContent = T['remote.token'];
  el.lChat.textContent = T['remote.chatId'];
  el.getId.textContent = T['remote.getChatId'];
  el.lNotify.textContent = T['remote.notify'];
  el.hint.textContent = T['remote.hint'];
  el.ok.textContent = T['dlg.save'];
  el.cancel.textContent = T['dlg.cancel'];
  setToolLabel(el.btn, T['tb.remote']);
  el.btn.title = T['tip.remote'];
  el.token.placeholder = T[state.hasToken ? 'remote.tokenSet' : 'remote.tokenNone'];
}

export function initRemoteDialog(injected) {
  hooks = injected || {};
  el.root = $('remotedlg');
  el.box = $('remotedlg-box');
  el.title = $('remotedlg-title');
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
  el.ok = $('rm-ok');
  el.cancel = $('rm-cancel');
  el.btn = $('btn-remote');

  applyTexts();
  onLangChange(applyTexts);

  el.btn.addEventListener('click', () => {
    if (hooks.hideMenus) hooks.hideMenus();
    openRemote().catch((e) => log(`[remote] 開啟失敗：${e}`));
  });
  el.enabled.addEventListener('change', applyEnabled);
  el.getId.addEventListener('click', () => {
    getChatId().catch((e) => log(`[remote] 取得 chat id 失敗：${e}`));
  });
  el.cancel.addEventListener('click', close);
  el.box.addEventListener('submit', (e) => {
    e.preventDefault();
    save();
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !el.root.hidden) close();
  });
}
