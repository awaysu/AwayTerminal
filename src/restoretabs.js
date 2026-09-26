// 恢復分頁（前端那一半）與離開程式的對話框。
//
// 舊版對應 `MainWindow.RestoreTabs`（啟動時）與 `Dialogs/ExitDialog`（關閉時）。
// Rust 端在 `src-tauri/src/restore.rs`，兩邊的分工：
//
//   - **Rust**：存／讀畫面內容（`restore/tab{n}.txt`）、分頁清單（settings 的 `savedTabs`）、
//     在 `n` 之後、`s` 之前發 `b{id}US{內容}US{分隔行}`。
//   - **這裡**：照清單順序呼叫 `session_create`（一筆一筆 try，壞掉的跳過，同舊版的 `catch`），
//     以及離開程式的對話框。
//
// 為什麼恢復要由前端驅動：`session_create` 的輸出 `Channel` 是前端建的，Rust 沒辦法自己生一個。

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { T } from './strings.js';

/**
 * 照上次存下的清單重開分頁。回傳成功恢復幾個（0＝沒有紀錄，呼叫端要開預設分頁）。
 *
 * 順序就是上次的分頁順序（Rust 端照分頁列的順序存）。
 */
export async function restoreSavedTabs(createSession) {
  let list = [];
  try {
    list = await invoke('restore_list');
  } catch (e) {
    console.warn('[restore] 讀不到恢復清單', e);
    return 0;
  }
  if (!Array.isArray(list) || list.length === 0) return 0;

  let ok = 0;
  for (let i = 0; i < list.length; i++) {
    const st = list[i];
    try {
      const args = argsOf(st, i);
      if (!args) continue;
      await createSession(args);
      ok++;
    } catch (e) {
      // 個別分頁恢復失敗就跳過，其餘照開（同舊版 RestoreTabs 的 catch）
      console.warn(`[restore] 第 ${i + 1} 筆恢復失敗`, e);
    }
  }
  return ok;
}

/** 一筆存檔 → `session_create` 的參數。認不出來的種類回 null（略過）。 */
function argsOf(st, index) {
  const title = st.title || null;
  switch (st.kind) {
    case 'shell':
      return { kind: 'shell', cwd: st.dir || null, title, restore: index };
    case 'conn':
      // 工作目錄直接用上次的（不跳資料夾選擇卡住啟動，同舊版）。
      // 沙盒是這條連線自己的設定，`session_create` 會重新準備一次：
      // worktree 還在就沿用，不在就重開一個。
      return {
        kind: 'conn',
        conn: st.connName,
        cwd: st.dir || null,
        title,
        restore: index,
      };
    case 'ssh':
      if (!st.conn) return null;
      // 帳號已經記在參數裡（登入時記下的）→ 直接連；沒有才會問 login as:。**密碼一律重問。**
      return { kind: 'ssh', ssh: st.conn, title, restore: index };
    case 'telnet':
      if (!st.conn) return null;
      return { kind: 'telnet', telnet: st.conn, title, restore: index };
    default:
      return null;
  }
}

// ------------------------------------------------------------ 離開程式的對話框

const el = {};

function $(id) {
  return document.getElementById(id);
}

/**
 * 掛好離開對話框。Rust 在 `CloseRequested` 時擋下關閉並 emit `exit-request`
 * （payload ＝上次的勾選狀態），我們問完使用者再呼叫 `exit_confirm` / `exit_cancel`。
 *
 * ⚠️ 使用者**再按一次 X** 時 Rust 就不再擋（見 `lib.rs`）——這是給「前端壞掉、對話框沒出來」
 * 留的逃生門，視窗不會被鎖死。
 */
export async function initExitDialog() {
  el.root = $('exitdlg');
  el.restore = $('ex-restore');
  el.go = $('ex-go');
  el.cancel = $('ex-cancel');

  $('exitdlg-title').textContent = T['exit.title'];
  $('exitdlg-body').textContent = T['exit.body'];
  $('ex-l-restore').textContent = T['exit.restore'];
  el.go.textContent = T['exit.go'];
  el.cancel.textContent = T['dlg.cancel'];

  const leave = () => {
    el.root.hidden = true;
    invoke('exit_confirm', { restore: el.restore.checked }).catch(() => {});
  };
  const stay = () => {
    el.root.hidden = true;
    invoke('exit_cancel').catch(() => {});
  };

  el.go.addEventListener('click', leave);
  el.cancel.addEventListener('click', stay);
  document.addEventListener('keydown', (e) => {
    if (el.root.hidden) return;
    if (e.key === 'Escape') stay();
    if (e.key === 'Enter') leave();
  });

  await listen('exit-request', (e) => {
    el.restore.checked = e.payload !== false;
    el.root.hidden = false;
    el.go.focus();
  });
}
