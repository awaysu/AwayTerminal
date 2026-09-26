// 右側分頁列 + 工具列（TASK-004）。
//
// ⚠️ 舊版的分頁列是 **WPF**（`MainWindow.xaml` 的 `TabStrip` ItemsControl），不在 WebView2 裡，
// 所以這個檔案沒有可以「搬過來」的舊程式碼——是照舊版的 XAML 與 `MainWindow.xaml.cs`
// 行為在 HTML/CSS/JS 重做。顏色、tooltip 格式、狀態燈規則、三態循環順序都對著舊版抄。
//
// 狀態來源是 Rust 的 `tab-state` event（JSON），**不是**舊字串協定——舊協定裡本來就沒有
// 分頁列（見 src-tauri/src/tabs.rs 的說明）。`terminal.js` 看不到這條。

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { T, fmt, iconSvg, elapsedText } from './strings.js';
import { createSession, log } from './bridge.js';

const MIN_PANEL_WIDTH = 120; // 舊版 TabPanelMinWidth

let state = { tabs: [], activeId: null, viewMode: 'tab' };
let panelWidth = 220;
let panelVisible = true;

const el = {};

/** 目前的分頁狀態（`awayVerify()` 用，讓驗證不必看視窗）。 */
export function currentTabState() {
  return state;
}

// ------------------------------------------------------------------ 小工具

function $(id) {
  return document.getElementById(id);
}

/** 分頁 tooltip：完整名稱 + 執行了多久（日:時:分），第二行＝目前路徑。同舊版 `ToolTipText`。 */
function tooltipFor(tab) {
  let s = `${tab.title}  ${T['tip.tabElapsed']} ${elapsedText(tab.startedAt)}`;
  if (tab.cwdPath && tab.cwdPath !== tab.title) s += `\n${tab.cwdPath}`;
  return s;
}

/** 圖示 tooltip：「是哪一種連線」＝種類名稱＋補充。同舊版 `KindTip`。 */
function kindTipFor(tab) {
  return tab.cwdPath ? `${tab.kindLabel}  ${tab.cwdPath}` : tab.kindLabel;
}

// ------------------------------------------------------------------ 對話框
//
// 舊版用 WPF 的 `InputDialog` 與 `MessageBox`。這裡做一個同樣形狀的頁內對話框：
// 不加 tauri dialog plugin，也不用 `window.confirm`（webview 的原生對話框會擋住整個
// 事件迴圈，而且樣式和程式其他地方對不起來）。

function closeModal() {
  el.modal.hidden = true;
  el.modalForm.onsubmit = null;
}

/** 文字輸入對話框。回傳 Promise<string|null>（取消＝null）。 */
function askText(title, prompt, initial) {
  return new Promise((resolve) => {
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
function askYesNo(title, prompt) {
  return new Promise((resolve) => {
    el.modalTitle.textContent = title;
    el.modalPrompt.textContent = prompt;
    el.modalInput.hidden = true;
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

// ------------------------------------------------------------ 分頁列渲染

function render() {
  // 三態按鈕顯示的是「點了會變成的樣子」（同舊版 `UpdateSplitButton`）
  el.btnView.textContent =
    state.viewMode === 'tab'
      ? T['tb.split']
      : state.viewMode === 'split'
        ? T['tb.columns']
        : T['tb.tabs'];
  // 分頁模式＝終端機外框細黃線；分割／分欄＝讓給各 pane 自己的黃框（同舊版 Split_Click）
  el.termFrame.classList.toggle('split-mode', state.viewMode !== 'tab');

  el.strip.textContent = '';
  if (state.tabs.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'tab-empty';
    empty.textContent = T['msg.noTabs'];
    el.strip.appendChild(empty);
    return;
  }

  for (const tab of state.tabs) {
    const row = document.createElement('div');
    row.className = 'tab-row' + (tab.id === state.activeId ? ' active' : '');
    row.dataset.id = String(tab.id);
    row.draggable = true;
    row.title = tooltipFor(tab);

    const icon = document.createElement('span');
    // 閒置染綠 #A5D6A7、忙碌染紅 #EF9A9A（舊版 TerminalTab.ReadyColor / BusyColor）
    icon.className = 'tab-icon' + (tab.busy ? ' busy' : '');
    icon.innerHTML = iconSvg(tab.kind);
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
    el.strip.appendChild(row);
  }
}

/** tooltip 的「執行 日:時:分」要跟著走。分鐘級精度，30 秒刷一次就夠。 */
function refreshTooltips() {
  for (const row of el.strip.querySelectorAll('.tab-row')) {
    const tab = state.tabs.find((t) => String(t.id) === row.dataset.id);
    if (tab) row.title = tooltipFor(tab);
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

function idOfRow(target) {
  const row = target.closest ? target.closest('.tab-row') : null;
  return row ? Number(row.dataset.id) : null;
}

function installStripEvents() {
  el.strip.addEventListener('click', (e) => {
    const id = idOfRow(e.target);
    if (id === null) return;
    if (e.target.closest('.tab-close')) {
      e.stopPropagation();
      closeTab(id);
      return;
    }
    invoke('tab_select', { id }).catch((err) => log(`[tabbar] 選取失敗：${err}`));
  });

  el.strip.addEventListener('dblclick', (e) => {
    const id = idOfRow(e.target);
    if (id !== null && !e.target.closest('.tab-close')) renameTab(id);
  });

  el.strip.addEventListener('contextmenu', (e) => {
    const id = idOfRow(e.target);
    if (id === null) return;
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, id);
  });

  // ---- 拖曳排序（舊版 1.1.8）----
  let dragId = null;
  el.strip.addEventListener('dragstart', (e) => {
    dragId = idOfRow(e.target);
    if (dragId === null) return;
    e.dataTransfer.effectAllowed = 'move';
    // Firefox 需要 setData 才會真的開始拖
    try {
      e.dataTransfer.setData('text/plain', String(dragId));
    } catch (_) {}
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
    const ids = state.tabs.map((t) => t.id).filter((x) => x !== from);
    const at = ids.indexOf(targetId);
    if (at < 0) return;
    ids.splice(at, 0, from);
    invoke('tabs_reorder', { ids }).catch((err) => log(`[tabbar] 排序失敗：${err}`));
  });
}

// ------------------------------------------------------------------ 選單

function hideMenus() {
  el.tabMenu.hidden = true;
  el.newMenu.hidden = true;
}

function showContextMenu(x, y, id) {
  el.tabMenu.hidden = false;
  el.tabMenu.style.left = `${x}px`;
  el.tabMenu.style.top = `${y}px`;
  el.tabMenu.dataset.id = String(id);
  // 開在視窗邊緣時往回收
  const r = el.tabMenu.getBoundingClientRect();
  if (r.right > window.innerWidth) el.tabMenu.style.left = `${window.innerWidth - r.width - 4}px`;
  if (r.bottom > window.innerHeight) el.tabMenu.style.top = `${window.innerHeight - r.height - 4}px`;
}

function installMenus() {
  el.tabMenu.addEventListener('click', (e) => {
    const item = e.target.closest('[data-act]');
    if (!item) return;
    const id = Number(el.tabMenu.dataset.id);
    hideMenus();
    if (item.dataset.act === 'rename') renameTab(id);
    else if (item.dataset.act === 'close') closeTab(id);
  });

  el.btnNew.addEventListener('click', (e) => {
    e.stopPropagation();
    const show = el.newMenu.hidden;
    hideMenus();
    if (!show) return;
    const r = el.btnNew.getBoundingClientRect();
    el.newMenu.hidden = false;
    el.newMenu.style.left = `${r.left}px`;
    el.newMenu.style.top = `${r.bottom + 2}px`;
  });

  el.newMenu.addEventListener('click', async (e) => {
    const item = e.target.closest('[data-kind]');
    if (!item) return;
    hideMenus();
    try {
      if (item.dataset.kind === 'shell') {
        await createSession({ kind: 'shell' });
      } else {
        const cmd = await askText(T['dlg.customTitle'], T['dlg.customPrompt'], '');
        if (cmd === null || !cmd.trim()) return;
        await createSession({ kind: 'custom', command: cmd.trim() });
      }
    } catch (err) {
      log(`[tabbar] ${T['msg.connectFail']}：${err}`);
      await askYesNo(T['msg.connectFail'], String(err));
    }
  });

  document.addEventListener('click', hideMenus);
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      hideMenus();
      if (!el.modal.hidden) el.modalCancel.click();
    }
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

// ------------------------------------------------------------------ 啟動

export async function initTabBar() {
  el.toolbar = $('toolbar');
  el.btnNew = $('btn-new');
  el.btnView = $('btn-view');
  el.btnPanel = $('btn-tabpanel');
  el.termFrame = $('termframe');
  el.panel = $('tabpanel');
  el.splitter = $('tabsplitter');
  el.strip = $('tabstrip');
  el.tabMenu = $('tab-menu');
  el.newMenu = $('new-menu');
  el.modal = $('modal');
  el.modalForm = $('modal-form');
  el.modalTitle = $('modal-title');
  el.modalPrompt = $('modal-prompt');
  el.modalInput = $('modal-input');
  el.modalOk = $('modal-ok');
  el.modalCancel = $('modal-cancel');

  el.btnNew.textContent = T['tb.new'] + ' ▾';
  el.btnPanel.title = T['tip.tabPanel'];
  el.btnView.title = T['tip.viewCycle'];
  el.newMenu.querySelector('[data-kind="shell"]').textContent = T['tb.powershell'];
  el.newMenu.querySelector('[data-kind="custom"]').textContent = T['tb.customCmd'];
  el.tabMenu.querySelector('[data-act="rename"]').textContent = T['menu.rename'];
  el.tabMenu.querySelector('[data-act="close"]').textContent = T['menu.close'];

  el.btnView.addEventListener('click', () => {
    invoke('view_mode_cycle').catch((err) => log(`[tabbar] 切換檢視失敗：${err}`));
  });

  installStripEvents();
  installMenus();
  installPanelResize();

  await listen('tab-state', (e) => {
    state = e.payload;
    render();
  });

  try {
    const s = await invoke('settings_get');
    panelVisible = s.tabPanelVisible;
    panelWidth = s.tabPanelWidth;
  } catch (err) {
    log(`[tabbar] 讀設定失敗，用預設值：${err}`);
  }
  applyPanel();
  render();

  setInterval(refreshTooltips, 30000);
}
