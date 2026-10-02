// 工具列 + 右側分頁列 + 各種選單／對話框（TASK-004 建立，TASK-005 補上工具列全套）。
//
// ⚠️ 舊版這一塊全部在 **WPF**（`MainWindow.xaml` 的工具列與 `TabStrip` ItemsControl），
// 不在 WebView2 裡，所以沒有可以「搬過來」的舊程式碼——是照舊版的 XAML 與
// `MainWindow.xaml.cs` 的 handler 在 HTML/CSS/JS 重做。顏色、文字、順序、確認對話框、
// toast 訊息都對著舊版抄，來源寫在各處註解。
//
// 分頁狀態來源是 Rust 的 `tab-state` event（JSON），**不是**舊字串協定——舊協定裡本來就
// 沒有分頁列（見 src-tauri/src/tabs.rs 的說明）。`terminal.js` 看不到這條。
//
// 動作一律經 Rust command → Rust 發舊協定字串 → `terminal.js`。前端**不直接**呼叫
// `window.AwayTerm`，這樣 `v`／`S`／`P` 這些協定走的是真正的路，之後遠端／巨集要用同一條。

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { T, fmt, elapsedText } from './strings.js';
import { iconImg, kindIcon, setToolLabel } from './icons.js';
import { createSession, log } from './bridge.js';
import { initConns, openManager, currentConns, reload as reloadConns } from './conns.js';
import { initConnDialog, openConnDialog, parseHostPort } from './sshdlg.js';
import { initComDialog, openComDialog } from './comdlg.js';
import { initMacro, runMacroForTab } from './macro.js';
import { initCompose, openCompose } from './compose.js';
import { initSettings } from './setdlg.js';
import { initAbout } from './about.js';
import { initRemoteDialog } from './remotedlg.js';
import { initAdb, openAdb, isAdbConn } from './adb.js';
import {
  initAgentDialog,
  openAgentTeam,
  openAgentSetup,
  openChatRoom,
} from './agentdlg.js';
import { onLangChange } from './i18n.js';
import { initFavs, addConnFavorite } from './favs.js';

const MIN_PANEL_WIDTH = 120; // 舊版 TabPanelMinWidth

let state = { tabs: [], activeId: null, viewMode: 'tab' };
let panelWidth = 220;
let panelVisible = true;
let palette = [];
/** 目前 `U` 協定帶進來的網址（網址選單用）。 */
let pendingUrl = '';
/**
 * 下一個「空的選取回覆」是因為程式接管了滑鼠（`m` 協定，舊版 `_selMouseHintId`）
 * → toast 要改成提示按住 Shift 拖曳。
 */
let selMouseHintId = null;

const el = {};

/** 目前的分頁狀態（`awayVerify()` 用，讓驗證不必看視窗）。 */
export function currentTabState() {
  return state;
}

// ------------------------------------------------------------------ 小工具

function $(id) {
  return document.getElementById(id);
}

function activeId() {
  return state.activeId;
}

/** 分頁 tooltip：完整名稱 + 執行了多久（`00d00h00m`），第二行＝目前路徑。同舊版 `ToolTipText`。 */
function tooltipFor(tab) {
  let s = `${tab.title}  ${T['tip.tabElapsed']} ${elapsedText(tab.startedAt)}`;
  if (tab.cwdPath && tab.cwdPath !== tab.title) s += `\n${tab.cwdPath}`;
  if (tab.logging) s += `\n${T['tip.tabLogging']}`;
  // 沙盒狀態（新功能）：有 worktree 就顯示路徑與分支，沒有就說明為什麼
  if (tab.sandbox) {
    s += `\n${fmt('sb.tipOn', tab.sandbox.workDir)}`;
    s += tab.sandbox.hasWorktree
      ? `\n${fmt('sb.tipBranch', tab.sandbox.branch)}`
      : `\n${T['sb.tipNoWorktree']}`;
    // BUG D15：護欄沒真的生效（例如 Claude Code 的 hook 要 node，但找不到 node）
    if (tab.sandbox.guardWarning) s += `\n⚠ ${tab.sandbox.guardWarning}`;
  } else if (tab.connSandbox === false) {
    s += `\n${T['sb.tipOff']}`;
  }
  return s;
}

/**
 * 分頁列這一列要用哪個圖示（舊版 `TerminalTab.StatusIcon` / `IconFile`）。
 *
 * 代理團隊／AI 聊天室的代表列用 `multi-agent.png`／`chatroom.png`；自訂連線用**那條連線
 * 自己的圖示**（舊版 1.1.2 的行為，`tab.IconFile = CustomIconFile(conn.Icon)`）；其餘照種類。
 */
function tabIconKey(tab, team) {
  if (team) return team.kind === 'chat' ? 'chatroom' : 'multi-agent';
  if (tab.kind === 'custom') {
    const c = currentConns().find((x) => x.name === tab.connName);
    return (c && c.icon) || 'run';
  }
  return kindIcon(tab.kind);
}

/** 圖示 tooltip：「是哪一種連線」＝種類名稱＋補充。同舊版 `KindTip`。 */
function kindTipFor(tab) {
  return tab.cwdPath ? `${tab.kindLabel}  ${tab.cwdPath}` : tab.kindLabel;
}

// -------------------------------------------------------------------- toast
//
// 舊版是 WPF 的 `CopyPopup` + `ShowCopyFeedback`（浮在被按的按鈕旁邊、1.2 秒後消失）。
// 這裡做成固定浮在工具列下方，時間一樣。

let toastTimer = null;

export function toast(text) {
  if (!text) return;
  el.toast.textContent = text;
  el.toast.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    el.toast.hidden = true;
  }, 1200); // 舊版 _copyPopupTimer 就是 1200ms
}

// ------------------------------------------------------------------ 對話框
//
// 舊版用 WPF 的 `InputDialog`／`MessageBox`／`LogDialog`。這裡做一個同樣形狀的頁內
// 對話框：不加 tauri dialog plugin 的 JS 端，也不用 `window.confirm`（webview 的原生
// 對話框會擋住整個事件迴圈，而且樣式和程式其他地方對不起來）。
// **刻意不同**：外觀是本程式自己的深色樣式，不是系統對話框。

function resetModal() {
  el.modalInput.hidden = true;
  el.modalArea.hidden = true;
  el.modalArea.onkeydown = null;
  el.modalList.hidden = true;
  el.modalExtra.hidden = true;
  el.modalCheck1.checked = false;
  el.modalCheck2.checked = false;
  el.modalBrowse.onclick = null;
}

function closeModal() {
  el.modal.hidden = true;
  el.modalForm.onsubmit = null;
  resetModal();
}

/**
 * 從清單挑一個。回傳 Promise<string|null>（取消＝null）。
 *
 * 舊版的 ADB 選裝置是「新分頁」按鈕底下的 ContextMenu；我們用頁內對話框的清單，
 * 因為還要能顯示**不能選**的項目（offline／unauthorized 的裝置）。
 *
 * @param {{value: string, label: string, disabled?: boolean}[]} items
 * @param {{value: string, label: string, disabled?: boolean}[]} [extra] 灰掉、不能選的
 */
function askFromList(title, items, extra) {
  return new Promise((resolve) => {
    resetModal();
    el.modalTitle.textContent = title;
    el.modalPrompt.textContent = '';
    el.modalList.textContent = '';
    for (const it of [...items, ...(extra || [])]) {
      const o = document.createElement('option');
      o.value = it.value;
      o.textContent = it.label;
      if (it.disabled) o.disabled = true;
      el.modalList.appendChild(o);
    }
    el.modalList.hidden = false;
    if (items.length) el.modalList.selectedIndex = 0;
    el.modalOk.textContent = T['dlg.ok'];
    el.modalCancel.textContent = T['dlg.cancel'];
    el.modal.hidden = false;
    el.modalList.focus();
    const done = (v) => {
      closeModal();
      resolve(v);
    };
    el.modalForm.onsubmit = (e) => {
      e.preventDefault();
      const o = el.modalList.selectedOptions[0];
      done(o && !o.disabled ? o.value : null);
    };
    // 雙擊直接選（清單的自然操作）
    el.modalList.ondblclick = () => {
      const o = el.modalList.selectedOptions[0];
      if (o && !o.disabled) done(o.value);
    };
    el.modalCancel.onclick = () => done(null);
  });
}

/** 文字輸入對話框。回傳 Promise<string|null>（取消＝null）。 */
/**
 * 多行輸入（AI 聊天室的主題與插話；舊版 `InputDialog(multiline: true)`）。
 * 回傳 Promise<string|null>（取消＝null）。Enter 是換行，**Ctrl+Enter 才是確定**。
 */
export function askMultiline(title, prompt, initial) {
  return new Promise((resolve) => {
    resetModal();
    el.modalTitle.textContent = title;
    el.modalPrompt.textContent = prompt;
    el.modalArea.hidden = false;
    el.modalArea.value = initial || '';
    el.modalOk.textContent = T['dlg.ok'];
    el.modalCancel.textContent = T['dlg.cancel'];
    el.modal.hidden = false;
    el.modalArea.focus();
    const done = (value) => {
      closeModal();
      resolve(value);
    };
    el.modalForm.onsubmit = (e) => {
      e.preventDefault();
      done(el.modalArea.value);
    };
    el.modalCancel.onclick = () => done(null);
    // textarea 裡的 Enter 要換行，所以用 Ctrl+Enter 送出（Esc 取消照舊）
    el.modalArea.onkeydown = (e) => {
      if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        done(el.modalArea.value);
      }
    };
  });
}

function askText(title, prompt, initial) {
  return new Promise((resolve) => {
    resetModal();
    el.modalTitle.textContent = title;
    el.modalPrompt.textContent = prompt;
    el.modalInput.hidden = false;
    el.modalInput.value = initial || '';
    el.modalOk.textContent = T['dlg.ok'];
    el.modalCancel.textContent = T['dlg.cancel'];
    el.modal.hidden = false;
    el.modalInput.focus();
    el.modalInput.select();
    el.modalForm.onsubmit = (e) => {
      e.preventDefault();
      const v = el.modalInput.value;
      closeModal();
      resolve(v);
    };
    el.modalCancel.onclick = () => {
      closeModal();
      resolve(null);
    };
  });
}

/** 是／否確認。回傳 Promise<boolean>。 */
export function askYesNo(title, prompt) {
  return new Promise((resolve) => {
    resetModal();
    el.modalTitle.textContent = title;
    el.modalPrompt.textContent = prompt;
    el.modalOk.textContent = T['dlg.yes'];
    el.modalCancel.textContent = T['dlg.no'];
    el.modal.hidden = false;
    el.modalOk.focus();
    el.modalForm.onsubmit = (e) => {
      e.preventDefault();
      closeModal();
      resolve(true);
    };
    el.modalCancel.onclick = () => {
      closeModal();
      resolve(false);
    };
  });
}

/** 兩個自訂按鈕的問句（例：「重新啟動分頁 / 稍後」）。回傳 true＝按了第一個。 */
function askTwo(title, prompt, yes, no) {
  return new Promise((resolve) => {
    resetModal();
    el.modalTitle.textContent = title;
    el.modalPrompt.textContent = prompt;
    el.modalOk.textContent = yes;
    el.modalCancel.textContent = no;
    el.modal.hidden = false;
    el.modalCancel.focus(); // 破壞性／會關掉連線的那個不要當預設
    el.modalForm.onsubmit = (e) => {
      e.preventDefault();
      closeModal();
      resolve(true);
    };
    el.modalCancel.onclick = () => {
      closeModal();
      resolve(false);
    };
  });
}

/** 只有一個「確定」的訊息框（錯誤訊息用）。 */
export function showInfo(title, prompt) {
  return new Promise((resolve) => {
    resetModal();
    el.modalTitle.textContent = title;
    el.modalPrompt.textContent = prompt;
    el.modalOk.textContent = T['dlg.ok'];
    el.modalCancel.textContent = T['dlg.cancel'];
    el.modal.hidden = false;
    el.modalOk.focus();
    const done = () => {
      closeModal();
      resolve();
    };
    el.modalForm.onsubmit = (e) => {
      e.preventDefault();
      done();
    };
    el.modalCancel.onclick = done;
  });
}

/**
 * log 對話框（舊版 `LogDialog`）：路徑 + 瀏覽… + 兩個核取方塊。
 * 回傳 `{path, timestamp, append}` 或 null。
 */
function askLogOptions(defaults) {
  return new Promise((resolve) => {
    resetModal();
    el.modalTitle.textContent = T['dlg.logTitle'];
    el.modalPrompt.textContent = T['log.path'];
    el.modalInput.hidden = false;
    el.modalInput.value = defaults.path;
    el.modalExtra.hidden = false;
    el.modalBrowse.textContent = T['log.browse'];
    el.modalCheck1Text.textContent = T['log.timestamp'];
    el.modalCheck2Text.textContent = T['log.append'];
    el.modalCheck1.checked = !!defaults.timestamp;
    el.modalCheck2.checked = !!defaults.append;
    el.modalOk.textContent = T['log.start'];
    el.modalCancel.textContent = T['dlg.cancel'];
    el.modal.hidden = false;
    el.modalInput.focus();

    el.modalBrowse.onclick = async () => {
      const picked = await invoke('log_pick_path', { current: el.modalInput.value });
      if (picked) el.modalInput.value = picked;
    };
    el.modalForm.onsubmit = (e) => {
      e.preventDefault();
      const path = el.modalInput.value.trim();
      if (!path) {
        // 舊版 LogDialog.Ok_Click 也是擋在這裡不關視窗
        el.modalPrompt.textContent = T['log.needPath'];
        return;
      }
      const out = {
        path,
        timestamp: el.modalCheck1.checked,
        append: el.modalCheck2.checked,
      };
      closeModal();
      resolve(out);
    };
    el.modalCancel.onclick = () => {
      closeModal();
      resolve(null);
    };
  });
}

// ------------------------------------------------------------ 分頁列渲染

function render() {
  // 三態按鈕顯示的是「點了會變成的樣子」（同舊版 `UpdateSplitButton`）
  setToolLabel(
    el.btnView,
    state.viewMode === 'tab'
      ? T['tb.split']
      : state.viewMode === 'split'
        ? T['tb.columns']
        : T['tb.tabs']
  );
  // 分頁模式＝終端機外框細黃線；分割／分欄＝讓給各 pane 自己的黃框（同舊版 Split_Click）
  el.termFrame.classList.toggle('split-mode', state.viewMode !== 'tab');

  // 沒有分頁時，需要作用中分頁的按鈕一律停用
  const none = state.tabs.length === 0;
  for (const b of [el.btnCopy, el.btnPaste, el.btnCopyAll, el.btnClear, el.btnPage]) {
    b.disabled = none;
  }

  el.strip.textContent = '';
  if (none) {
    const empty = document.createElement('div');
    empty.className = 'tab-empty';
    empty.textContent = T['msg.noTabs'];
    el.strip.appendChild(empty);
    return;
  }

  for (const tab of state.tabs) {
    // 代理團隊：一組只出現一列（其餘格藏在組裡，同舊版 `_stripView` 的過濾）
    if (!isStripRow(tab.id)) continue;
    const row = document.createElement('div');
    const team = teamOfTab(tab.id);
    row.className =
      'tab-row' +
      (rowOf(state.activeId) === tab.id ? ' active' : '') +
      (team ? ' tab-team' : '');
    row.dataset.id = String(tab.id);
    row.draggable = true;
    row.title = team ? `${tooltipFor(tab)}\n${teamTip(team)}` : tooltipFor(tab);

    const icon = document.createElement('span');
    // 閒置染綠 #A5D6A7、忙碌染紅 #EF9A9A（舊版 TerminalTab.ReadyColor / BusyColor）。
    // 代理團隊整組只有這一列：只要有一格在忙就染紅，不是只看代表列那一格
    const busy = team
      ? team.agents.some((a) => a.tab !== null && state.tabs.some((t) => t.id === a.tab && t.busy))
      : tab.busy;
    icon.className = 'tab-icon' + (busy ? ' busy' : '');
    icon.appendChild(iconImg(tabIconKey(tab, team), 'tab-ico'));
    icon.title = kindTipFor(tab);

    const title = document.createElement('span');
    title.className = 'tab-title';
    title.textContent = tab.title;

    const close = document.createElement('button');
    close.className = 'tab-close';
    close.type = 'button';
    close.textContent = '✕';
    close.title = T['tip.tabClose'];

    row.append(icon, title, close);
    // 代理團隊那一列尾端的小字（舊版 `TerminalTab.AgentStateText`）
    if (team) {
      let text;
      if (team.kind === 'chat') {
        // 聊天室：人數 ＋ 第幾回合（等主題時只有人數）
        const n = team.agents.filter((a) => a.tab !== null).length;
        text =
          team.phase === 'discussing' ? `${n}\u00b7${team.round}/${team.rounds}` : String(n);
      } else {
        // \u5df2\u6295\u905e 3 \u5247\uff1d\u300c\u27093/50\u300d\uff08\u4e0d\u9650\uff1d\u300c\u27093/\u221e\u300d\uff09\u3001\u66ab\u505c\uff1d\u300c\u23f83/50\u300d\u3002\u820a\u7248\u9084\u6c92\u6295\u905e\u904e\u6642\u4e0d\u986f\u793a\uff1b
        // \u4f7f\u7528\u8005\u8981\u4e00\u958b\u59cb\u5c31\u770b\u5f97\u5230\uff08\u525b\u958b\uff0f\u525b\u6062\u5fa9\u7684\u5718\u968a\u662f\u300c\u27090/50\u300d\uff09
        const count = `${team.messageCount || 0}/${team.maxMessages > 0 ? team.maxMessages : '\u221e'}`;
        // U+FE0E\uff1a\u8981\u6587\u5b57\u6a23\u5f0f\u7684 \u23f8\uff0c\u4e0d\u8981\u5f69\u8272 emoji\uff08\u624d\u5403\u5f97\u5230\u66ab\u505c\u7684\u984f\u8272\uff09
        text = team.paused ? `\u23f8\ufe0e${count}` : `\u2709${count}`;
      }
      if (text) {
        const badge = document.createElement('span');
        badge.className = 'tab-team-badge' + (team.paused && team.kind !== 'chat' ? ' paused' : '');
        badge.textContent = text;
        badge.title = teamTip(team);
        row.insertBefore(badge, close);
      }
    }
    // 記錄 log 中：舊版 1.1.2 起分頁列不再放 log 圖示、只在 tooltip 註明，
    // 但一個小紅點不占空間又看得出來，所以這裡多一個（刻意不同，見文件）
    if (tab.logging) {
      const dot = document.createElement('span');
      dot.className = 'tab-logdot';
      dot.textContent = '●';
      dot.title = T['tip.tabLogging'];
      row.insertBefore(dot, close);
    }
    // 巨集執行中的小標記（新增）：M + 目前行號，tooltip 有檔名
    if (tab.macroState) {
      const m = document.createElement('span');
      m.className = 'tab-macro';
      m.textContent = 'M';
      m.title = fmt('tip.tabMacro', tab.macroState.file, tab.macroState.line);
      row.insertBefore(m, close);
    }
    // 沙盒模式的小標記（新功能，舊版沒有——已寫進 checklist 的「刻意不同」表）
    if (tab.sandbox) {
      const sb = document.createElement('span');
      sb.className = 'tab-sandbox' + (tab.sandbox.hasWorktree ? '' : ' partial');
      sb.textContent = '⬚';
      sb.title = tab.sandbox.hasWorktree
        ? fmt('sb.tipBranch', tab.sandbox.branch)
        : T['sb.tipNoWorktree'];
      if (tab.sandbox.guardWarning) {
        if (tab.sandbox.hasWorktree) sb.className += ' partial';
        sb.title += `\n⚠ ${tab.sandbox.guardWarning}`;
      }
      row.insertBefore(sb, close);
    }
    el.strip.appendChild(row);
  }
}

/** tooltip 的「執行 00d00h00m」要跟著走。分鐘級精度，30 秒刷一次就夠。 */
function refreshTooltips() {
  for (const row of el.strip.querySelectorAll('.tab-row')) {
    const tab = state.tabs.find((t) => String(t.id) === row.dataset.id);
    if (!tab) continue;
    // 代理團隊那一列要連附加行一起刷（和 render 時同一個組法；BUG-AUDIT B12）
    const team = teamOfTab(tab.id);
    row.title = team ? `${tooltipFor(tab)}\n${teamTip(team)}` : tooltipFor(tab);
  }
}

function renderPalette() {
  el.colorItems.textContent = '';
  for (const c of palette) {
    const item = document.createElement('div');
    item.className = 'menu-item';
    item.dataset.color = `${c.fg}|${c.bg}`;
    item.textContent = T['menu.colorSample'];
    item.style.color = c.fg;
    item.style.background = c.bg;
    el.colorItems.appendChild(item);
  }
}

// ------------------------------------------------------------ 分頁列互動

async function closeTab(id) {
  const tab = state.tabs.find((t) => t.id === id);
  if (!tab) return;
  // 舊版 CloseTab 一律先跳 Yes/No 確認
  const ok = await askYesNo(T['msg.closeTabTitle'], fmt('msg.closeTabConfirm', tab.title));
  if (!ok) return;
  await invoke('tab_close', { id });
}

async function renameTab(id) {
  const tab = state.tabs.find((t) => t.id === id);
  if (!tab) return;
  const name = await askText(T['dlg.renameTitle'], T['dlg.renamePrompt'], tab.title);
  if (name === null || !name.trim()) return;
  await invoke('tab_rename', { id, title: name.trim() });
}

/** 記錄 log（舊版 `LogAction`）：正在記錄就問要不要停止並開資料夾，否則開設定對話框。 */
async function logAction(id) {
  const tab = state.tabs.find((t) => t.id === id);
  if (!tab) return;
  if (tab.logging) {
    const stop = await askYesNo(T['dlg.logTitle'], T['msg.stopLogAsk']);
    if (!stop) return;
    const path = await invoke('log_stop', { id });
    if (path) await invoke('reveal_path', { path }).catch(() => {});
    return;
  }
  const defaults = await invoke('log_defaults', { id });
  const opts = await askLogOptions(defaults);
  if (!opts) return;
  try {
    await invoke('log_start', { id, ...opts });
  } catch (e) {
    await showInfo(T['dlg.logTitle'], `${T['msg.logFail']}\n${e}`);
  }
}

// ------------------------------------------------------------ 沙盒模式（新功能）

/**
 * 切換這條自訂連線的沙盒開關。
 *
 * `CLAUDE.md` 明寫「改變在**下次啟動**該分頁時生效，需提示」，所以這裡改完設定之後
 * 一定要問使用者要不要現在重開分頁。重開＝關掉（走既有的優雅結束流程）再用同設定開一個。
 */
/** 分頁右鍵「推播到 Telegram」：只留在記憶體、立刻生效（不必重開分頁）。 */
async function toggleTgNotify(id) {
  try {
    const on = await invoke('telegram_tab_state', { id });
    await invoke('telegram_tab_notify', { id, on: !on });
    toast(fmt('menu.tgNotifySet', !on ? T['sb.on'] : T['sb.off']));
  } catch (e) {
    log(`[tabbar] 推播到 Telegram 切換失敗：${e}`);
  }
}

async function toggleSandbox(id) {
  const tab = state.tabs.find((t) => t.id === id);
  if (!tab || !tab.connName) return;
  const next = tab.connSandbox !== true; // 目前關著就要打開
  try {
    await invoke('conn_set_sandbox', { name: tab.connName, sandbox: next });
  } catch (e) {
    await showInfo(T['sb.changedTitle'], String(e));
    return;
  }
  await reloadConns();

  const restart = await askTwo(
    T['sb.changedTitle'],
    fmt('sb.changedBody', tab.connName, next ? T['sb.on'] : T['sb.off']),
    T['sb.restartNow'],
    T['sb.later']
  );
  if (!restart) return;
  const conn = tab.connName;
  // 重開要回到原本的工作目錄：沒帶 cwd 會開在預設目錄（桌面），不是 repo 就沒有 worktree，
  // pickDir 也不會再問（BUG-AUDIT B6）。舊後端沒送 workDir 時退回預設（等於原本行為）。
  const cwd = tab.workDir || null;
  await invoke('tab_close', { id });
  try {
    await createSession({ kind: 'conn', conn, cwd });
  } catch (e) {
    await showInfo(T['msg.connectFail'], String(e));
  }
}

/** 清除沙盒 worktree（**分支保留**）。 */
async function clearSandbox(id) {
  const tab = state.tabs.find((t) => t.id === id);
  if (!tab || !tab.sandbox) {
    await showInfo(T['sb.clearTitle'], T['sb.noSandbox']);
    return;
  }
  const ok = await askYesNo(
    T['sb.clearTitle'],
    fmt('sb.clearBody', tab.sandbox.workDir, tab.sandbox.branch || '-')
  );
  if (!ok) return;
  try {
    await invoke('sandbox_clear', { id });
    toast(T['sb.cleared']);
  } catch (e) {
    await showInfo(T['sb.clearTitle'], String(e));
  }
}

// ------------------------------------------------- 代理團隊（一組一列）

/** `agent-state` event 的最新內容（每個團隊一筆）。 */
let teams = [];

/** 這個分頁屬於哪個團隊（不是團隊的格＝null）。 */
function teamOfTab(id) {
  return teams.find((t) => t.agents.some((a) => a.tab === id)) || null;
}

/** 分頁列要不要列這個分頁：一般分頁都列；代理團隊只列代表列那一列（舊版 `IsStripRow`）。 */
function isStripRow(id) {
  const t = teamOfTab(id);
  return !t || t.rowTab === id;
}

/** 代表這個分頁的那一列（代理團隊＝整組的代表列，舊版 `RowOf`）。 */
function rowOf(id) {
  const t = teamOfTab(id);
  return t && t.rowTab !== null ? t.rowTab : id;
}

/** 分頁列那一列的 tooltip 要多的幾行（已投遞／暫停／待投遞，舊版 `ma.tip*`）。 */
function teamTip(t) {
  // 聊天室：顯示進行到哪（等主題／第 n/N 回合／寫結論／已結束），不是投遞計數
  const lines =
    t.kind === 'chat'
      ? [t.chatStatus || T['chat.title']]
      : [fmt('ma.tipMessages', t.messageCount, t.maxMessages > 0 ? t.maxMessages : '\u221e')];
  if (t.kind !== 'chat' && t.paused) lines.push(T['ma.tipPaused']);
  if (t.kind !== 'chat' && t.pending > 0) lines.push(fmt('ma.tipPending', t.pending));
  for (const a of t.agents) {
    if (a.tab !== null) lines.push(`${a.agentId} ${a.roleTitle} (${a.backendName})`);
  }
  return lines.join('\n');
}

/** 右鍵選單的「投遞」子選單（10／30／50／100／不限／暫停）。 */
function renderDeliveryMenu(t) {
  const menu = $('ma-delivery-menu');
  menu.textContent = '';
  const add = (tag, text, checked) => {
    const item = document.createElement('div');
    item.className = 'menu-item';
    item.dataset.act = 'ma-limit';
    item.dataset.limit = tag;
    item.textContent = `${checked ? '\u2713 ' : '\u3000'}${text}`;
    menu.appendChild(item);
  };
  for (const n of [10, 30, 50, 100, 0]) {
    add(String(n), n === 0 ? T['ma.limitUnlimited'] : String(n), !t.paused && t.maxMessages === n);
  }
  const sep = document.createElement('div');
  sep.className = 'menu-sep';
  menu.appendChild(sep);
  add('pause', T['ma.menuPauseItem'], t.paused);
}

// ---- AI 聊天室 ----

async function chatTopic(team) {
  // 討論中換主題會從第 1 回合重新開始 → 先確認（舊版 `chat.newTopicAsk`）
  if (
    (team.phase === 'discussing' || team.phase === 'concluding') &&
    !(await askYesNo(T['chat.title'], T['chat.newTopicAsk']))
  ) {
    return;
  }
  const topic = await askMultiline(T['chat.title'], T['chat.topicPrompt'], team.topic || '');
  if (topic === null || !topic.trim()) return;
  try {
    const folder = await invoke('chat_start', { key: team.key, topic });
    log(`[tabbar] 聊天室 ${team.number}：開始討論，紀錄資料夾 ${folder}`);
  } catch (e) {
    await showInfo(T['chat.title'], String(e));
  }
}

async function chatSay(team) {
  const text = await askMultiline(T['chat.title'], T['chat.sayPrompt'], '');
  if (text === null || !text.trim()) return;
  try {
    await invoke('chat_say', { key: team.key, text });
    toast(T['chat.saidToast']);
  } catch (e) {
    await showInfo(T['chat.title'], String(e));
  }
}

async function chatEnd(team) {
  const ok = await invoke('chat_end', { key: team.key }).catch((e) => {
    log(`[tabbar] 結束討論失敗：${e}`);
    return false;
  });
  if (ok) toast(T['chat.endToast']);
}

async function chatFolder(team) {
  try {
    const dir = await invoke('chat_folder', { key: team.key });
    if (dir) await invoke('open_dir', { path: dir });
  } catch (e) {
    log(`[tabbar] 開啟討論紀錄資料夾失敗：${e}`);
  }
}

/** 改組名／聊天室名（代表列的標題每次重綁都會被組名蓋回去，所以要改 `Team::title`）。 */
async function renameTeam(team) {
  const name = await askText(T['dlg.renameTitle'], T['dlg.renamePrompt'], team.title);
  if (name === null || !name.trim()) return;
  try {
    await invoke('agent_team_rename', { key: team.key, title: name.trim() });
  } catch (e) {
    await showInfo(T['ma.title'], String(e));
  }
}

async function stopTeam(key) {
  try {
    const ids = await invoke('agent_stop', { key });
    if (ids.length) toast(fmt('ma.stopSent', ids.join(', ')));
  } catch (e) {
    log(`[tabbar] 停止任務失敗：${e}`);
  }
}

async function openTeamBus(key) {
  try {
    const dir = await invoke('agent_bus_dir', { key });
    if (dir) await invoke('open_dir', { path: dir });
  } catch (e) {
    log(`[tabbar] 開啟訊息資料夾失敗：${e}`);
  }
}

async function setTeamLimit(key, tag) {
  const limit = tag === 'pause' ? null : Number(tag);
  await invoke('agent_delivery_set', { key, limit }).catch((e) =>
    log(`[tabbar] 投遞設定失敗：${e}`)
  );
}

/** 關閉整組（分頁列只有一列＝一起關，舊版 `CloseAgentGroup`：先確認）。 */
async function closeTeam(t) {
  const n = t.agents.filter((a) => a.tab !== null).length;
  if (!(await askYesNo(T['msg.closeTabTitle'], fmt('ma.closeConfirm', t.title, n)))) return;
  let ids = [];
  try {
    ids = await invoke('agent_team_tabs', { key: t.key });
  } catch (e) {
    log(`[tabbar] 取得團隊分頁失敗：${e}`);
    return;
  }
  // 由後往前關（前面的格還在時不會每關一個就重排一次）
  for (const id of ids.slice().reverse()) {
    await invoke('tab_close', { id }).catch((e) => log(`[tabbar] 關閉分頁 ${id} 失敗：${e}`));
  }
  await invoke('agent_team_gone', { key: t.key }).catch(() => {});
}

function idOfRow(target) {
  const row = target.closest ? target.closest('.tab-row') : null;
  return row ? Number(row.dataset.id) : null;
}

function installStripEvents() {
  el.strip.addEventListener('click', (e) => {
    const id = idOfRow(e.target);
    if (id === null) return;
    const team = teamOfTab(id);
    if (e.target.closest('.tab-close')) {
      e.stopPropagation();
      // 代理團隊：分頁列只有一列＝整組一起關（舊版 `CloseAgentGroup`）
      if (team) closeTeam(team);
      else closeTab(id);
      return;
    }
    // 代理團隊：切到最後點過的那一格（舊版 `FocusTargetOf`）
    const target = team && team.lastFocused ? team.lastFocused : id;
    invoke('tab_select', { id: target }).catch((err) => log(`[tabbar] 選取失敗：${err}`));
  });

  el.strip.addEventListener('dblclick', (e) => {
    const id = idOfRow(e.target);
    if (id === null || e.target.closest('.tab-close')) return;
    // 代理團隊代表列的標題＝組名：改 `Team::title`，不是分頁標題（改分頁標題會被重綁蓋回去；
    // 同右鍵「改名」那條，BUG-AUDIT B11）
    const team = teamOfTab(id);
    if (team) renameTeam(team);
    else renameTab(id);
  });

  el.strip.addEventListener('contextmenu', (e) => {
    const id = idOfRow(e.target);
    if (id === null) return;
    e.preventDefault();
    const tab = state.tabs.find((t) => t.id === id);
    const team = teamOfTab(id);
    const isChat = !!team && team.kind === 'chat';
    // 代理團隊那幾項只在**代理團隊**那一列出現；聊天室那幾項只在聊天室那一列出現
    for (const node of el.tabMenu.querySelectorAll('[data-ma]')) {
      node.hidden = !team || isChat;
    }
    for (const node of el.tabMenu.querySelectorAll('[data-chat]')) node.hidden = !isChat;
    if (isChat) {
      // 「插話」只有討論中／寫結論中才有用；「結束討論」只有討論中才有用（舊版同款）
      const say = el.tabMenu.querySelector('[data-act="chat-say"]');
      const end = el.tabMenu.querySelector('[data-act="chat-end"]');
      const live = team.phase === 'discussing' || team.phase === 'concluding';
      if (say) say.classList.toggle('disabled', !live);
      if (end) end.classList.toggle('disabled', team.phase !== 'discussing');
    }
    if (team && !isChat) renderDeliveryMenu(team);
    // 沙盒那兩項只對「自訂連線開的分頁」有意義（PowerShell／SSH 分頁沒有連線設定）
    // 代理團隊的沙盒是整組的、在建團隊時決定 → 不給逐分頁切換
    const hasConn = !!(tab && tab.connName) && !team;
    el.menuSandbox.hidden = !hasConn;
    el.menuSandboxClear.hidden = !(tab && tab.sandbox && tab.sandbox.hasWorktree);
    if (hasConn) {
      // 勾勾顯示的是**連線設定**的值（改了下次啟動才生效），不是目前分頁的狀態。
      // 勾勾放在文字**後面**：放前面的話這一項的字會比其他項目往右縮，整排對不齊
      const on = tab.connSandbox === true;
      el.menuSandbox.textContent = `${T['sb.menu']}${on ? ' ✓' : ''}`;
    }
    // 推播到 Telegram：遠端沒開就整項隱藏（沒開的話這個勾勾沒有任何意義）
    el.menuTgNotify.hidden = true;
    invoke('telegram_state')
      .then((st) => {
        if (!st.running) return;
        el.menuTgNotify.hidden = false;
        return invoke('telegram_tab_state', { id }).then((on) => {
          el.menuTgNotify.textContent = `${T['menu.tgNotify']}${on ? ' ✓' : ''}`;
        });
      })
      .catch(() => {});
    showMenu(el.tabMenu, e.clientX, e.clientY, { id: String(id) });
  });

  // ---- 拖曳排序（舊版 1.1.8）----
  let dragId = null;
  el.strip.addEventListener('dragstart', (e) => {
    dragId = idOfRow(e.target);
    if (dragId === null) return;
    e.dataTransfer.effectAllowed = 'move';
    try {
      e.dataTransfer.setData('text/plain', String(dragId));
    } catch {
      /* 有些 webview 不給 setData，拖曳照樣靠 dragId */
    }
  });
  el.strip.addEventListener('dragover', (e) => {
    if (dragId === null) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    const row = e.target.closest && e.target.closest('.tab-row');
    for (const r of el.strip.querySelectorAll('.tab-row')) {
      r.classList.toggle('drag-over', r === row && Number(r.dataset.id) !== dragId);
    }
  });
  el.strip.addEventListener('dragleave', (e) => {
    const row = e.target.closest && e.target.closest('.tab-row');
    if (row) row.classList.remove('drag-over');
  });
  const endDrag = () => {
    dragId = null;
    for (const r of el.strip.querySelectorAll('.tab-row')) r.classList.remove('drag-over');
  };
  el.strip.addEventListener('dragend', endDrag);
  el.strip.addEventListener('drop', (e) => {
    e.preventDefault();
    const targetId = idOfRow(e.target);
    const from = dragId;
    endDrag();
    if (from === null || targetId === null || from === targetId) return;
    const all = state.tabs.map((t) => t.id);
    const fromIdx = all.indexOf(from);
    const targetIdx = all.indexOf(targetId);
    const ids = all.filter((x) => x !== from);
    let at = ids.indexOf(targetId);
    if (at < 0) return;
    // 往下拖＝放在目標**後面**，否則永遠拖不到最後一個（BUG-AUDIT B7）。
    // 目標是代理團隊代表列時，要放在整組的最後一格後面，不要插進組裡。
    if (fromIdx >= 0 && fromIdx < targetIdx) {
      let last = at;
      const team = teamOfTab(targetId);
      if (team) {
        for (const a of team.agents) {
          const k = a.tab === null ? -1 : ids.indexOf(a.tab);
          if (k > last) last = k;
        }
      }
      at = last + 1;
    }
    ids.splice(at, 0, from);
    invoke('tabs_reorder', { ids }).catch((err) => log(`[tabbar] 排序失敗：${err}`));
  });
}

// ------------------------------------------------------------------ 選單

function hideMenus() {
  for (const m of [el.tabMenu, el.newMenu, el.pageMenu, el.termMenu, el.urlMenu, el.favsMenu]) {
    if (!m) continue;
    m.hidden = true;
    closeSubmenus(m);
  }
}

/** 收掉選單裡所有展開的子選單（下次打開時要從頭量位置）。 */
function closeSubmenus(menu) {
  for (const it of menu.querySelectorAll('.menu-item.has-sub.sub-open')) {
    it.classList.remove('sub-open');
    const sub = it.querySelector(':scope > .submenu');
    if (sub) sub.removeAttribute('style');
  }
}

/**
 * 子選單（「配色 ▸」「投遞 ▸」）的展開與定位。
 *
 * **為什麼不只靠 CSS `:hover`**（TASK-031 的「點配色沒反應」）：
 *  1. 分頁列在畫面**最右邊**，子選單往右開一定超出視窗 → 要能往左翻，而 CSS 量不到。
 *  2. `:hover` 沒辦法用程式觸發，`--verify` 就永遠檢查不到子選單有沒有真的出現。
 * 所以改成 `mouseover` 委派 → 加 `.sub-open` → 當場量一次、需要就翻邊。
 */
function installSubmenus(menu) {
  if (!menu) return;
  menu.addEventListener('mouseover', (e) => {
    const parent = e.target.closest ? e.target.closest('.menu-item.has-sub') : null;
    for (const it of menu.querySelectorAll('.menu-item.has-sub.sub-open')) {
      if (it !== parent) {
        it.classList.remove('sub-open');
        const sub = it.querySelector(':scope > .submenu');
        if (sub) sub.removeAttribute('style');
      }
    }
    if (parent && !parent.classList.contains('sub-open')) {
      parent.classList.add('sub-open');
      placeSubmenu(parent);
    }
  });
}

/** 子選單擺哪裡：預設在父項右邊（CSS 的 `left:100%`），超出視窗就翻到左邊／往上收。 */
function placeSubmenu(item) {
  const sub = item.querySelector(':scope > .submenu');
  if (!sub) return;
  sub.removeAttribute('style'); // 先回到 CSS 的預設位置再量
  const pad = 4;
  let r = sub.getBoundingClientRect();
  if (r.right > window.innerWidth - pad) {
    // 往左翻（父項的左邊）；左邊也放不下就貼著視窗右緣
    sub.style.left = 'auto';
    sub.style.right = '100%';
    r = sub.getBoundingClientRect();
    if (r.left < pad) {
      sub.style.right = 'auto';
      sub.style.left = `${pad - item.getBoundingClientRect().left}px`;
      r = sub.getBoundingClientRect();
    }
  }
  // CSS 的 `top: -5px` 是相對父項的；超出下緣就往上移同樣的差距
  if (r.bottom > window.innerHeight - pad) {
    const shift = window.innerHeight - pad - r.bottom;
    sub.style.top = `${-5 + Math.round(shift)}px`;
    r = sub.getBoundingClientRect();
  }
  if (r.top < pad) sub.style.top = `${-5 + Math.round(pad - r.top)}px`;
}

function showMenu(menu, x, y, data) {
  hideMenus();
  Object.assign(menu.dataset, data || {});
  menu.hidden = false;
  menu.style.left = `${x}px`;
  menu.style.top = `${y}px`;
  // 開在視窗邊緣時往回收
  const r = menu.getBoundingClientRect();
  if (r.right > window.innerWidth) menu.style.left = `${Math.max(0, window.innerWidth - r.width - 4)}px`;
  if (r.bottom > window.innerHeight) menu.style.top = `${Math.max(0, window.innerHeight - r.height - 4)}px`;
}

function showMenuUnder(menu, button) {
  const r = button.getBoundingClientRect();
  showMenu(menu, r.left, r.bottom + 2);
}

/** 網址選單（舊版 `ShowUrlMenu`，由 `U` 協定觸發）。 */
export function showUrlMenu(url, x, y) {
  if (!url) return;
  pendingUrl = url;
  showMenu(el.urlMenu, x, y);
}

/**
 * 弱演算法警告（PuTTY 的 warn-below-this-line）。
 *
 * 和主機金鑰一樣，Rust 端**停在交握中間等答案**（逾時 180 秒＝取消），
 * 所以一定要回 `ssh_hostkey_answer`（兩邊借用同一個回覆通道：
 * `acceptandstore`＝繼續並記住這台主機、`reject`＝取消）。
 */
function installWeakAlgoDialog() {
  let done = () => {};
  const answer = (id, value) => {
    el.weakAlgo.hidden = true;
    invoke('ssh_hostkey_answer', { id, answer: value }).catch((e) =>
      log(`[tabbar] 弱演算法回覆失敗：${e}`)
    );
    done();
  };

  listen('ssh-weak-algo', (e) => {
    enqueueSshAsk((finish) => {
      done = finish;
      showWeakAlgo(e.payload);
    });
  }).catch((e) => log(`[tabbar] 掛弱演算法 listener 失敗：${e}`));

  const showWeakAlgo = (r) => {
    const where = withTabName(r, r.port === 22 ? r.host : `${r.host}:${r.port}`);
    el.weakAlgoTitle.textContent = T['wa.title'];
    el.weakAlgoBody.textContent = fmt('wa.body', where);
    const tbody = el.weakAlgoList.querySelector('tbody');
    tbody.textContent = '';
    for (const [kind, name] of r.items) {
      const tr = document.createElement('tr');
      const th = document.createElement('th');
      th.textContent = kind;
      const td = document.createElement('td');
      td.textContent = name;
      tr.append(th, td);
      tbody.appendChild(tr);
    }
    el.weakAlgoNote.textContent = T['wa.note'];
    el.weakAlgo.hidden = false;
    el.waCancel.focus(); // 危險選項不要當預設

    el.waGo.onclick = () => answer(r.id, 'acceptandstore');
    el.waCancel.onclick = () => answer(r.id, 'reject');
  };
}

// 主機金鑰／弱演算法詢問的佇列（BUG-AUDIT B9）：兩種對話框都只有一份，兩個 SSH 分頁
// 同時問時第二個會蓋掉第一個，第一個的 id 永遠得不到答覆（Rust 等到 180 秒逾時）。
// 所以排隊、一次只顯示一個，答完一個才顯示下一個。
const sshAskQueue = [];
let sshAskBusy = false;

/** `show(finish)`：顯示對話框，使用者答完要呼叫 `finish()` 讓下一個上來。 */
function enqueueSshAsk(show) {
  sshAskQueue.push(show);
  pumpSshAsk();
}

function pumpSshAsk() {
  if (sshAskBusy || sshAskQueue.length === 0) return;
  sshAskBusy = true;
  const show = sshAskQueue.shift();
  let finished = false;
  const finish = () => {
    if (finished) return;
    finished = true;
    sshAskBusy = false;
    pumpSshAsk();
  };
  try {
    show(finish);
  } catch (e) {
    log(`[tabbar] SSH 詢問對話框失敗：${e}`);
    finish();
  }
}

/** 對話框裡的主機名稱後面加上是哪個分頁問的（同時有好幾個 SSH 分頁時才分得出來）。 */
function withTabName(r, where) {
  const tab = state.tabs.find((t) => t.id === r.tabId);
  return tab && tab.title ? `${where} (${tab.title})` : where;
}

/** `m` 協定：下一個空的選取回覆是因為程式接管了滑鼠。 */
export function noteMouseHint(id) {
  selMouseHintId = String(id);
}

/** 沒有選取文字時的 toast 文字（舊版 `toast.noSelection` / `noSelectionMouse`）。 */
export function noSelectionToast(id) {
  const owned = selMouseHintId === String(id);
  selMouseHintId = null;
  return owned ? T['toast.noSelectionMouse'] : T['toast.noSelection'];
}

function installMenus() {
  // 有子選單的選單（目前只有分頁右鍵）：滑過父項才展開，並且當場量位置
  for (const m of [el.tabMenu, el.newMenu, el.pageMenu, el.termMenu, el.urlMenu, el.favsMenu]) {
    installSubmenus(m);
  }
  el.tabMenu.addEventListener('click', (e) => {
    const color = e.target.closest('[data-color]');
    const id = Number(el.tabMenu.dataset.id);
    if (color) {
      const [fg = '', bg = ''] = (color.dataset.color || '').split('|');
      hideMenus();
      invoke('tab_colors', { id, fg, bg }).catch((err) => log(`[tabbar] 配色失敗：${err}`));
      return;
    }
    const item = e.target.closest('[data-act]');
    if (!item) return;
    if (item.dataset.act === 'color' || item.dataset.act === 'ma-delivery') return; // 有子選單，點父項不動作
    const team = teamOfTab(id);
    hideMenus();
    if (item.dataset.act === 'chat-topic') {
      if (team) chatTopic(team);
      return;
    }
    if (item.dataset.act === 'chat-say') {
      if (team && !item.classList.contains('disabled')) chatSay(team);
      else if (team) showInfo(T['chat.title'], T['chat.sayNotNow']);
      return;
    }
    if (item.dataset.act === 'chat-end') {
      if (team && !item.classList.contains('disabled')) chatEnd(team);
      return;
    }
    if (item.dataset.act === 'chat-folder') {
      if (team) chatFolder(team);
      return;
    }
    if (item.dataset.act === 'chat-setup') {
      if (team) openAgentSetup(team.key, createSession);
      return;
    }
    if (item.dataset.act === 'rename' && team) {
      // 代表列的標題＝組名，要改 `Team::title`（改分頁標題會被重綁蓋回去）
      renameTeam(team);
      return;
    }
    if (item.dataset.act === 'ma-setup') {
      if (team) openAgentSetup(team.key, createSession);
      return;
    }
    if (item.dataset.act === 'ma-limit') {
      if (team) setTeamLimit(team.key, item.dataset.limit || '50');
      return;
    }
    if (item.dataset.act === 'ma-stop') {
      if (team) stopTeam(team.key);
      return;
    }
    if (item.dataset.act === 'ma-bus') {
      if (team) openTeamBus(team.key);
      return;
    }
    if (item.dataset.act === 'close' && team) {
      closeTeam(team);
      return;
    }
    if (item.dataset.act === 'rename') renameTab(id);
    else if (item.dataset.act === 'log') logAction(id);
    else if (item.dataset.act === 'macro') runMacroForTab(id, state);
    else if (item.dataset.act === 'sandbox') toggleSandbox(id);
    else if (item.dataset.act === 'sandbox-clear') clearSandbox(id);
    else if (item.dataset.act === 'tg-notify') toggleTgNotify(id);
    else if (item.dataset.act === 'close') closeTab(id);
  });

  setToolLabel(el.btnCompose, T['tb.compose']);
  el.btnCompose.title = T['tip.compose'];
  el.btnCompose.addEventListener('click', () => {
    hideMenus();
    openCompose(state);
  });

  el.btnNew.addEventListener('click', (e) => {
    e.stopPropagation();
    const show = el.newMenu.hidden;
    hideMenus();
    if (show) showMenuUnder(el.newMenu, el.btnNew);
  });

  el.newMenu.addEventListener('click', async (e) => {
    // 使用者自己的自訂連線那一區（data-conn），以及預設區（data-kind）
    const conn = e.target.closest('[data-conn]');
    if (conn) {
      hideMenus();
      await openConn(conn.dataset.conn);
      return;
    }
    const item = e.target.closest('[data-kind]');
    if (!item) return;
    hideMenus();
    if (item.dataset.kind === 'manage') {
      openManager();
      return;
    }
    await newSession(item.dataset.kind);
  });

  el.btnPage.addEventListener('click', (e) => {
    e.stopPropagation();
    const show = el.pageMenu.hidden;
    hideMenus();
    if (show && activeId() !== null) showMenuUnder(el.pageMenu, el.btnPage);
  });

  el.pageMenu.addEventListener('click', (e) => {
    const item = e.target.closest('[data-scroll]');
    if (!item) return;
    hideMenus();
    const id = activeId();
    if (id === null) return;
    invoke('toolbar_scroll', { id, action: item.dataset.scroll }).catch(() => {});
  });

  // 終端機右鍵選單（舊版 OnWebContextMenu：擋掉 webview 預設選單）
  el.termFrame.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    if (activeId() === null) return;
    showMenu(el.termMenu, e.clientX, e.clientY);
  });

  el.termMenu.addEventListener('click', (e) => {
    const item = e.target.closest('[data-term]');
    if (!item) return;
    hideMenus();
    const id = activeId();
    if (id === null) return;
    switch (item.dataset.term) {
      case 'copy':
        invoke('toolbar_copy', { id });
        break;
      case 'copyPaste':
        invoke('toolbar_copy_paste', { id });
        break;
      case 'paste':
        pasteFromClipboard(id);
        break;
      case 'copyall':
        invoke('toolbar_copy_all', { id });
        break;
      case 'copyAllFile':
        invoke('toolbar_copy_all_file', { id });
        break;
      case 'selectAll':
        invoke('toolbar_select_all', { id });
        break;
      case 'search':
        invoke('toolbar_search');
        break;
    }
    // 搜尋會把焦點放進搜尋列，其餘動作做完焦點要回到終端機（點選單時被帶走了）
    if (item.dataset.term !== 'search') focusTerminal();
  });

  el.urlMenu.addEventListener('click', async (e) => {
    const item = e.target.closest('[data-url]');
    if (!item) return;
    const url = pendingUrl;
    hideMenus();
    if (!url) return;
    if (item.dataset.url === 'open') {
      try {
        await invoke('open_url', { url });
      } catch (err) {
        log(`[tabbar] 開啟網址失敗：${err}`);
      }
    } else {
      await writeClipboard(url);
      toast(T['toast.urlCopied']);
    }
  });

  document.addEventListener('click', hideMenus);
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      hideMenus();
      // 延到這一輪 keydown 分派完才關：其他對話框的 Esc handler 靠「#modal 還開著」判斷
      // 這一下是給 modal 的（BUG-AUDIT B3）。這個 listener 最早掛，同步關掉的話它們
      // 會看到 modal 已經關了，然後把底下的視窗也一起關掉。
      if (!el.modal.hidden) {
        setTimeout(() => {
          if (!el.modal.hidden) el.modalCancel.click();
        }, 0);
      }
    }
  });
}

// ------------------------------------------------------------------ 工具列

/**
 * 開新分頁。舊版工具列開 PowerShell 會**先跳資料夾選擇視窗**（`PickWorkDir`），
 * 取消就不開；自訂連線勾了 `PickDir` 也走同一個流程。這裡照做：
 * 兩種都先選工作目錄，取消＝不開分頁。
 */
async function newSession(kind) {
  try {
    // SSH 不需要本機工作目錄；帳號在終端機裡問（`login as:`，同 PuTTY／舊版）。
    // 完整的 SSH 對話框（帳號、金鑰、保持連線、斷線重連）是 TASK-007。
    if (kind === 'ssh' || kind === 'ssh-quick') {
      // 快速連線：一行 host[:port]，其餘用設定的預設值（B6 之前的入口，保留）
      if (kind === 'ssh-quick') {
        const target = await askText(T['dlg.sshTitle'], T['dlg.sshPrompt'], '');
        if (target === null || !target.trim()) return;
        const { host, port } = parseHostPort(target.trim());
        if (!host) return;
        await createSession({ kind: 'ssh', ssh: { host, port } });
        return;
      }
      // 完整對話框（B6；TASK-010 起同一個對話框也能選 Telnet，同舊版的一個入口）
      const s = await invoke('settings_get');
      const r = await openConnDialog(
        {
          port: 22,
          useAgent: true,
          keepaliveMins: s.keepAliveMins,
          autoReconnect: s.autoReconnect,
        },
        'ssh',
      );
      if (!r) return;
      if (r.action === 'favorite') {
        await addConnFavorite(r.kind, r.params);
        return;
      }
      await createSession(
        r.kind === 'telnet'
          ? { kind: 'telnet', telnet: r.params }
          : { kind: 'ssh', ssh: r.params },
      );
      return;
    }

    // 連接埠（舊版是獨立的 ComDialog，不在 SSH/Telnet 的「類型」裡）
    if (kind === 'com') {
      const s = await invoke('settings_get');
      const r = await openComDialog({
        port: s.comPort,
        baud: s.comBaud,
        dataBits: s.comDataBits,
        parity: s.comParity,
        stopBits: s.comStopBits,
        flow: s.comFlow,
        autoReconnect: s.autoReconnect,
      });
      if (!r) return;
      if (r.action === 'favorite') {
        await addConnFavorite('com', r.params);
        return;
      }
      await createSession({ kind: 'com', com: r.params });
      return;
    }

    if (kind === 'multiagent') {
      await openAgentTeam(createSession);
      return;
    }
    if (kind === 'chatroom') {
      await openChatRoom(createSession);
      return;
    }

    const title = kind === 'shell' ? T['dlg.pickDirPs'] : T['dlg.pickDirCustom'];
    let command = null;
    if (kind === 'custom') {
      const cmd = await askText(T['dlg.customTitle'], T['dlg.customPrompt'], '');
      if (cmd === null || !cmd.trim()) return;
      command = cmd.trim();
    }
    const cwd = await invoke('pick_work_dir', { title });
    if (!cwd) return; // 使用者取消（同舊版：PickWorkDir 回 null 就不開分頁）
    await createSession({ kind, command, cwd });
  } catch (err) {
    log(`[tabbar] ${T['msg.connectFail']}：${err}`);
    await showInfo(T['msg.connectFail'], String(err));
  }
}

/**
 * 開一條自訂連線。`pickDir` 的連線會先跳資料夾選擇（同舊版 `OpenCustom`），
 * 取消就不開分頁。沙盒是後端依連線設定決定的，前端不用管。
 */
async function openConn(name) {
  const c = currentConns().find((x) => x.name === name);
  if (!c) return;
  // 指向 adb 的自訂連線：先 `adb devices` 再開（同舊版 `OpenCustom` 的 `IsAdbExe` 分支）。
  // 直接跑 `adb shell` 在接了兩台以上時只會噴錯。
  if (isAdbConn(c)) {
    await openAdb(c.path);
    return;
  }
  try {
    let cwd = null;
    if (c.pickDir) {
      cwd = await invoke('pick_work_dir', { title: T['dlg.pickDirCustom'] });
      if (!cwd) return; // 取消（同舊版：PickWorkDir 回 null 就不開）
    }
    await createSession({ kind: 'conn', conn: name, cwd });
  } catch (err) {
    log(`[tabbar] ${T['msg.connectFail']}：${err}`);
    await showInfo(T['msg.connectFail'], String(err));
  }
}

/** 把自訂連線填進「新分頁 ▾」（隱藏的不列，同舊版）。 */
function renderConnMenu(list) {
  el.newConns.textContent = '';
  for (const c of list) {
    if (c.hidden) continue;
    const item = document.createElement('div');
    item.className = 'menu-item with-icon';
    item.dataset.conn = c.name;
    // 圖示＝這條連線自己選的那個（舊版 New 下拉同一組圖）
    item.appendChild(iconImg(c.icon || 'run', 'menu-ico'));
    const label = document.createElement('span');
    label.className = 'menu-label';
    label.textContent = c.name;
    item.appendChild(label);
    if (!c.sandbox) {
      // 沒開沙盒的要一眼看得出來——這是使用者最在意的那個開關
      const tag = document.createElement('span');
      tag.className = 'menu-tag';
      tag.textContent = T['conn.sandboxOff'];
      item.appendChild(tag);
    }
    el.newConns.appendChild(item);
  }
}

// ------------------------------------------------- 主機金鑰確認（照 PuTTY）

/**
 * Rust 的 SSH 任務在交握中間需要答案，所以它會 emit `ssh-hostkey` 並**停在那裡等**。
 * 我們一定要回一個答案（逾時 180 秒後 Rust 端會自己當成取消）。
 */
function installHostKeyDialog() {
  let done = () => {};
  const answer = (id, value) => {
    el.hostkey.hidden = true;
    invoke('ssh_hostkey_answer', { id, answer: value }).catch((e) =>
      log(`[tabbar] 主機金鑰回覆失敗：${e}`)
    );
    done();
  };

  // 排隊一次一個（BUG-AUDIT B9，佇列和弱演算法共用）
  listen('ssh-hostkey', (e) => {
    enqueueSshAsk((finish) => {
      done = finish;
      showHostKey(e.payload);
    });
  }).catch((e) => log(`[tabbar] 掛主機金鑰 listener 失敗：${e}`));

  const showHostKey = (r) => {
    const changed = r.kind === 'changed';
    el.hostkeyBox.classList.toggle('danger', changed);
    el.hostkeyTitle.textContent = changed ? T['hk.titleChanged'] : T['hk.titleUnknown'];
    el.hostkeyBody.textContent = changed ? T['hk.bodyChanged'] : T['hk.bodyUnknown'];
    el.hkHost.textContent = withTabName(r, r.port === 22 ? r.host : `${r.host}:${r.port}`);
    const f = r.fingerprints;
    el.hkAlg.textContent = f.bits ? `${f.algorithm} (${f.bits} bits)` : f.algorithm;
    el.hkSha256.textContent = f.sha256;
    el.hkMd5.textContent = f.md5;
    el.hostkeyNote.textContent = changed
      ? fmt('hk.noteChanged', r.storePath, r.line || '?')
      : T['hk.noteUnknown'];
    el.hostkey.hidden = false;

    el.hkStore.onclick = () => answer(r.id, 'acceptandstore');
    el.hkOnce.onclick = () => answer(r.id, 'acceptonce');
    el.hkCancel.onclick = () => answer(r.id, 'reject');
    // 金鑰變更時預設焦點放「取消」（PuTTY 也是把危險選項放在最不順手的位置）
    (changed ? el.hkCancel : el.hkStore).focus();
  };
}

/**
 * 把鍵盤焦點還給作用中的終端機（舊版每個工具列／右鍵動作後面的 `Web.Focus()`）。
 *
 * 舊版的工具列與選單是 WPF 的，網頁裡的焦點從頭到尾沒離開過終端機；我們的按鈕與選單
 * 是同一頁裡的元素，一點下去焦點就被帶走——貼上之後接著打字會沒反應。
 */
function focusTerminal() {
  const ta = document.querySelector('.active-pane .xterm-helper-textarea');
  if (ta) ta.focus();
}

/** 純文字貼上（舊版 `Paste_Click`）：讀剪貼簿 → `v` 協定 → `terminal.js` 的 `doPaste`。 */
async function pasteFromClipboard(id) {
  let text = '';
  try {
    text = await navigator.clipboard.readText();
  } catch (e) {
    log(`[tabbar] 讀剪貼簿失敗：${e}`);
  }
  if (!text) return;
  await invoke('toolbar_paste', { id, text });
}

export async function writeClipboard(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch (e) {
    log(`[tabbar] 寫剪貼簿失敗：${e}`);
    return false;
  }
}

function installToolbar() {
  el.btnCopy.addEventListener('click', () => {
    const id = activeId();
    if (id !== null) invoke('toolbar_copy', { id });
    focusTerminal();
  });
  el.btnCopyAll.addEventListener('click', () => {
    const id = activeId();
    if (id !== null) invoke('toolbar_copy_all', { id });
    focusTerminal();
  });
  el.btnPaste.addEventListener('click', () => {
    const id = activeId();
    if (id !== null) pasteFromClipboard(id);
    focusTerminal();
  });
  el.btnClear.addEventListener('click', async () => {
    const id = activeId();
    if (id === null) return;
    const tab = state.tabs.find((t) => t.id === id);
    // 舊版 v1.0.27 起一律先問（使用者指定）：Telnet/COM 走 term.clear() 會洗掉整個 scrollback
    const ok = await askYesNo(T['msg.clearTitle'], fmt('msg.clearConfirm', tab ? tab.title : id));
    if (!ok) return;
    await invoke('toolbar_clear', { id });
  });
  el.btnView.addEventListener('click', () => {
    invoke('view_mode_cycle').catch((err) => log(`[tabbar] 切換檢視失敗：${err}`));
  });
}

// --------------------------------------------------- 分頁列寬度 / 顯示隱藏

function applyPanel() {
  el.panel.hidden = !panelVisible;
  el.splitter.hidden = !panelVisible;
  el.panel.style.width = `${Math.max(MIN_PANEL_WIDTH, panelWidth)}px`;
  el.btnPanel.textContent = panelVisible ? '▲' : '▼';
  // 版面變了 → xterm 要重新 fit。terminal.js 自己有 ResizeObserver，這裡只要讓它有機會跑
  window.dispatchEvent(new Event('resize'));
}

function installPanelResize() {
  let dragging = false;
  el.splitter.addEventListener('mousedown', (e) => {
    dragging = true;
    e.preventDefault();
    document.body.classList.add('col-resizing');
  });
  window.addEventListener('mousemove', (e) => {
    if (!dragging) return;
    // 分頁列在右邊：寬度＝視窗右緣到游標
    panelWidth = Math.max(MIN_PANEL_WIDTH, Math.round(window.innerWidth - e.clientX - 4));
    el.panel.style.width = `${panelWidth}px`;
  });
  window.addEventListener('mouseup', () => {
    if (!dragging) return;
    dragging = false;
    document.body.classList.remove('col-resizing');
    // 拖完才存（同舊版 TabSplitter_DragCompleted）
    invoke('tab_panel_set', { width: panelWidth }).catch(() => {});
    window.dispatchEvent(new Event('resize'));
  });

  el.btnPanel.addEventListener('click', (e) => {
    e.stopPropagation();
    panelVisible = !panelVisible;
    applyPanel();
    invoke('tab_panel_set', { visible: panelVisible }).catch(() => {});
  });
}

/**
 * 把工具列／選單／固定對話框的文字重設一次。
 *
 * 切語言時會再被呼叫一次（註冊在 `i18n.js`，同舊版 `Loc.Changed` → `ApplyTexts()`），
 * 所以**不要**在這裡做除了設定文字以外的事。
 */
function applyTexts() {
    setToolLabel(el.btnNew, T['tb.new'] + ' ▾');
    setToolLabel(el.btnCopy, T['tb.copy']);
    el.btnCopy.title = T['tip.copy'];
    setToolLabel(el.btnPaste, T['tb.paste']);
    el.btnPaste.title = T['tip.paste'];
    setToolLabel(el.btnCopyAll, T['tb.copyall']);
    el.btnCopyAll.title = T['tip.copyall'];
    setToolLabel(el.btnClear, T['tb.clear']);
    el.btnClear.title = T['tip.clear'];
    setToolLabel(el.btnPage, T['tb.page'] + ' ▾');
    el.btnPage.title = T['tip.page'];
    el.btnPanel.title = T['tip.tabPanel'];
    el.btnView.title = T['tip.viewCycle'];
    setText(el.newMenu, '[data-kind="shell"]', T['tb.powershell']);
    setText(el.newMenu, '[data-kind="ssh"]', T['tb.ssh']);
    setText(el.newMenu, '[data-kind="ssh-quick"]', T['sd.quick']);
    setText(el.newMenu, '[data-kind="com"]', T['tb.com'] + '…');
    setText(el.newMenu, '[data-kind="multiagent"]', T['ma.title'] + '\u2026');
    setText(el.newMenu, '[data-kind="custom"]', T['tb.customCmd']);
    setText(el.tabMenu, '[data-act="rename"]', T['menu.rename']);
    setText(el.tabMenu, '[data-act="log"]', T['menu.log']);
    setText(el.tabMenu, '[data-act="macro"]', T['menu.macro']);
    setText(el.tabMenu, '[data-act="close"]', T['menu.close']);
    setText(el.tabMenu, '[data-act="sandbox-clear"]', T['sb.clear']);
    setText(el.tabMenu, '[data-act="tg-notify"]', T['menu.tgNotify']);
    // 代理團隊那幾項（「投遞」有子選單，只換前面那段文字）
    const dev = el.tabMenu.querySelector('[data-act="ma-delivery"]');
    if (dev && dev.firstChild) dev.firstChild.nodeValue = T['ma.menuDelivery'];
    setText(el.tabMenu, '[data-act="ma-setup"]', T['ma.menuSetup']);
    setText(el.newMenu, '[data-kind="chatroom"]', T['chat.title'] + '\u2026');
    setText(el.tabMenu, '[data-act="chat-setup"]', T['chat.menuSetup']);
    setText(el.tabMenu, '[data-act="chat-topic"]', T['chat.menuTopic']);
    setText(el.tabMenu, '[data-act="chat-say"]', T['chat.menuSay']);
    setText(el.tabMenu, '[data-act="chat-end"]', T['chat.menuEnd']);
    setText(el.tabMenu, '[data-act="chat-folder"]', T['chat.menuOpenFolder']);
    setText(el.tabMenu, '[data-act="ma-stop"]', T['ma.menuStop']);
    setText(el.tabMenu, '[data-act="ma-bus"]', T['ma.menuOpenBus']);
    setText(el.newMenu, '[data-kind="manage"]', T['tb.manageConns']);
    setText(el.tabMenu, '[data-color=""]', T['menu.colorDefault']);
    for (const [sel, key] of [
      ['[data-scroll="up"]', 'page.up'],
      ['[data-scroll="down"]', 'page.down'],
      ['[data-scroll="top"]', 'page.top'],
      ['[data-scroll="bottom"]', 'page.bottom'],
    ]) {
      setText(el.pageMenu, sel, T[key]);
    }
    for (const [sel, key] of [
      ['[data-term="paste"]', 'tb.paste'],
      ['[data-term="copyPaste"]', 'ctx.copyPaste'],
      ['[data-term="copy"]', 'ctx.copy'],
      ['[data-term="copyall"]', 'tb.copyall'],
      ['[data-term="copyAllFile"]', 'ctx.copyAllFile'],
      ['[data-term="selectAll"]', 'ctx.selectAll'],
      ['[data-term="search"]', 'ctx.search'],
    ]) {
      setText(el.termMenu, sel, T[key]);
    }
    setText(el.urlMenu, '[data-url="open"]', T['ctx.openUrl']);
    setText(el.urlMenu, '[data-url="copy"]', T['ctx.copyUrl']);
    // 配色父項的文字要保留子選單的箭頭，所以只改第一個文字節點
    el.tabMenu.querySelector('[data-act="color"]').firstChild.nodeValue = T['menu.color'];

    el.hkStore.textContent = T['hk.store'];
    el.hkOnce.textContent = T['hk.once'];
    el.hkCancel.textContent = T['hk.cancel'];
    el.waGo.textContent = T['wa.go'];
    el.waCancel.textContent = T['wa.cancel'];

  // 工具列的「輸入文字」在 installToolbar() 裡設（那個函式只跑一次），這裡補上
  if (el.btnCompose) {
    setToolLabel(el.btnCompose, T['tb.compose']);
    el.btnCompose.title = T['tip.compose'];
  }
  // 分頁列、三態按鈕、右鍵選單的動態部分都在 render() 裡（它也讀 T[...]）
  render();
}

// ------------------------------------------------------------------ 啟動

export async function initTabBar() {
  el.btnNew = $('btn-new');
  el.btnCompose = $('btn-compose');
  el.btnCopy = $('btn-copy');
  el.btnPaste = $('btn-paste');
  el.btnCopyAll = $('btn-copyall');
  el.btnClear = $('btn-clear');
  el.btnPage = $('btn-page');
  el.btnView = $('btn-view');
  el.btnPanel = $('btn-tabpanel');
  el.termFrame = $('termframe');
  el.panel = $('tabpanel');
  el.splitter = $('tabsplitter');
  el.strip = $('tabstrip');
  el.tabMenu = $('tab-menu');
  el.newMenu = $('new-menu');
  el.pageMenu = $('page-menu');
  el.termMenu = $('term-menu');
  el.urlMenu = $('url-menu');
  el.colorItems = $('color-items');
  el.newConns = $('new-conns');
  el.favsMenu = $('favs-menu');
  el.menuSandbox = el.tabMenu.querySelector('[data-act="sandbox"]');
  el.menuSandboxClear = el.tabMenu.querySelector('[data-act="sandbox-clear"]');
  el.menuTgNotify = el.tabMenu.querySelector('[data-act="tg-notify"]');
  el.toast = $('toast');
  el.hostkey = $('hostkey');
  el.hostkeyBox = $('hostkey-box');
  el.hostkeyTitle = $('hostkey-title');
  el.hostkeyBody = $('hostkey-body');
  el.hostkeyNote = $('hostkey-note');
  el.hkHost = $('hk-host');
  el.hkAlg = $('hk-alg');
  el.hkSha256 = $('hk-sha256');
  el.hkMd5 = $('hk-md5');
  el.hkStore = $('hk-store');
  el.hkOnce = $('hk-once');
  el.hkCancel = $('hk-cancel');
  el.weakAlgo = $('weakalgo');
  el.weakAlgoTitle = $('weakalgo-title');
  el.weakAlgoBody = $('weakalgo-body');
  el.weakAlgoList = $('weakalgo-list');
  el.weakAlgoNote = $('weakalgo-note');
  el.waGo = $('wa-go');
  el.waCancel = $('wa-cancel');
  el.modal = $('modal');
  el.modalForm = $('modal-form');
  el.modalTitle = $('modal-title');
  el.modalPrompt = $('modal-prompt');
  el.modalInput = $('modal-input');
  el.modalArea = $('modal-area');
  el.modalExtra = $('modal-extra');
  el.modalBrowse = $('modal-browse');
  el.modalCheck1 = $('modal-check1');
  el.modalCheck2 = $('modal-check2');
  el.modalCheck1Text = $('modal-check1-text');
  el.modalCheck2Text = $('modal-check2-text');
  el.modalList = $('modal-list');
  el.modalOk = $('modal-ok');
  el.modalCancel = $('modal-cancel');

  onLangChange(applyTexts);

  installToolbar();
  installStripEvents();
  installMenus();
  installPanelResize();
  installHostKeyDialog();
  installWeakAlgoDialog();
  await initConnDialog();
  initComDialog();
  await initMacro({ askYesNo, showInfo });
  await initCompose({ toast });
  initSettings({ showInfo, hideMenus, askYesNo, askFromList });
  initAbout({ showInfo, hideMenus });
  // 遠端設定（Telegram）：舊版就是工具列獨立一顆按鈕 ＋ 獨立視窗
  initRemoteDialog({ showInfo, hideMenus });
  initAdb({ showInfo, askYesNo: (body) => askYesNo(T['tb.adb'], body), pickFromList: askFromList, createSession });
  initAgentDialog();
  await initFavs({
    createSession,
    askText,
    askYesNo,
    showInfo,
    toast,
    hideMenus,
    showMenuUnder,
    activeId,
    // 自訂連線那幾筆最愛，圖示要用連線自己選的那個（舊版 FavoriteIcon → CustomIconFile）
    conns: currentConns,
  });
  // 自訂連線：清單一變就重畫「新分頁 ▾」那一區
  await initConns(renderConnMenu);

  await listen('tab-state', (e) => {
    state = e.payload;
    render();
  });
  // 代理團隊的狀態（每個團隊一筆）：分頁列那一列、tooltip、右鍵選單都看它
  await listen('agent-state', (e) => {
    teams = Array.isArray(e.payload) ? e.payload : [];
    render();
  });
  // Telegram 的致命錯誤（token 失效）：後端已經自己停掉遠端並關掉設定裡的開關，
  // 這裡只負責讓使用者**看到一次**——不然遠端會安靜地不動，沒有人知道為什麼（TASK-036）。
  await listen('telegram-fatal', (e) => {
    const msg = typeof e.payload === 'string' ? e.payload : String(e.payload);
    log(`[tabbar] Telegram 遠端停止：${msg}`);
    showInfo(T['remote.title'], msg);
  }).catch((err) => log(`[tabbar] 掛 telegram-fatal listener 失敗：${err}`));

  try {
    teams = await invoke('agent_teams');
  } catch (err) {
    log(`[tabbar] 讀代理團隊狀態失敗：${err}`);
  }

  try {
    const s = await invoke('settings_get');
    panelVisible = s.tabPanelVisible;
    panelWidth = s.tabPanelWidth;
    palette = s.palette || [];
  } catch (err) {
    log(`[tabbar] 讀設定失敗，用預設值：${err}`);
  }
  renderPalette();
  applyPanel();
  render();

  setInterval(refreshTooltips, 30000);
}

function setText(root, selector, text) {
  const node = root.querySelector(selector);
  if (!node) return;
  // 圖左字右的項目：只換文字那一段，不然 `textContent` 會把 <img> 一起洗掉
  const label = node.querySelector(':scope > .menu-label');
  if (label) label.textContent = text;
  else node.textContent = text;
}

