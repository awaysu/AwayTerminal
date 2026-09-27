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
import { onLangChange } from './i18n.js';

/**
 * 照上次存下的清單重開分頁。回傳成功恢復幾個（0＝沒有紀錄，呼叫端要開預設分頁）。
 *
 * 順序就是上次的分頁順序（Rust 端照分頁列的順序存）。
 */
/** 把介面文字重設一次（切語言時會被叫；註冊在 `i18n.js`）。 */
function applyTexts() {
  $('exitdlg-title').textContent = T['exit.title'];
  $('exitdlg-body').textContent = T['exit.body'];
  $('ex-l-restore').textContent = T['exit.restore'];
  $('ex-l-md').textContent = T['exit.updateMd'];
  el.go.textContent = T['exit.go'];
  el.cancel.textContent = T['dlg.cancel'];
}

export async function restoreSavedTabs(createSession) {
  let list = [];
  try {
    list = await invoke('restore_list');
  } catch (e) {
    console.warn('[restore] 讀不到恢復清單', e);
    return 0;
  }
  if (!Array.isArray(list) || list.length === 0) return 0;

  // 代理團隊的格要**整組一起**恢復（同一個 agentKey 的那幾筆交給 `agent_team_restore`，
  // 它要重新算組號、開沙盒、組角色檔，然後前端才逐格開 session）。
  // 依「第一次出現的位置」處理，其餘分頁照原順序。
  const teamOrder = [];
  const teamIndices = new Map();
  for (let i = 0; i < list.length; i++) {
    const key = list[i].kind === 'agent' ? list[i].agentKey || `#${i}` : null;
    if (!key) continue;
    if (!teamIndices.has(key)) {
      teamIndices.set(key, []);
      teamOrder.push(key);
    }
    teamIndices.get(key).push(i);
  }

  let ok = 0;
  const doneTeams = new Set();
  for (let i = 0; i < list.length; i++) {
    const st = list[i];
    try {
      if (st.kind === 'agent') {
        const key = st.agentKey || `#${i}`;
        if (doneTeams.has(key)) continue; // 這一組已經在前面整組恢復過了
        doneTeams.add(key);
        const { restoreAgentTeam } = await import('./agentdlg.js');
        ok += await restoreAgentTeam(teamIndices.get(key) || [i], createSession);
        continue;
      }
      const args = argsOf(st, i);
      if (!args) continue;
      await createSession(args);
      ok++;
    } catch (e) {
      // 個別分頁恢復失敗就跳過，其餘照開（同舊版 RestoreTabs 的 catch）
      console.warn(`[restore] 第 ${i + 1} 筆恢復失敗`, e);
    }
  }
  void teamOrder;
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
    case 'agent':
      // 代理團隊走 `restoreAgentTeam()`（整組），不會經過這裡
      return null;
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
  el.md = $('ex-md');
  el.mdLabel = $('ex-l-md');
  el.busy = $('ex-busy');
  el.go = $('ex-go');
  el.cancel = $('ex-cancel');

  onLangChange(applyTexts);

  // 勾了「離開前更新 CLAUDE.md」就先請 Claude Code 分頁寫完再真的離開（舊版 ExitDialog 的
  // `UpdateAction`：對話框停在原地、顯示「正在請 Claude Code 更新 CLAUDE.md，請稍候…」、
  // 按鈕停用）。代理團隊的格不算——好幾個 agent 同時改同一份會互相覆蓋。
  let leaving = false;
  const leave = async () => {
    if (leaving) return;
    leaving = true;
    const updateMd = el.md.checked && !el.md.disabled;
    if (updateMd) {
      el.busy.hidden = false;
      el.busy.textContent = T['exit.updating'];
      el.go.disabled = el.cancel.disabled = el.md.disabled = true;
      try {
        await invoke('claude_md_update');
      } catch (e) {
        console.warn('[restore] 更新 CLAUDE.md 失敗', e);
      }
    }
    el.root.hidden = true;
    invoke('exit_confirm', { restore: el.restore.checked, updateMd }).catch(() => {});
  };
  const stay = () => {
    if (leaving) return; // 已經在等 CLAUDE.md 寫完，取消鈕是停用的
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

  await listen('exit-request', async (e) => {
    // payload：`{ restore, updateMd }`（上次的勾選狀態）
    const p = e.payload && typeof e.payload === 'object' ? e.payload : {};
    el.restore.checked = p.restore !== false;
    leaving = false;
    el.busy.hidden = true;
    el.go.disabled = el.cancel.disabled = false;
    // 沒有**一般的** Claude Code 分頁就停用那個勾選（舊版 `SetClaudeAvailable`）
    let available = false;
    try {
      available = await invoke('claude_md_available');
    } catch (_) {}
    el.md.disabled = !available;
    el.md.checked = available && p.updateMd === true;
    el.mdLabel.style.opacity = available ? '' : '0.5';
    el.root.hidden = false;
    el.go.focus();
  });
}
