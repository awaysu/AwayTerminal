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

import { T, fmt, iconSvg, elapsedText } from './strings.js';
import { createSession, log } from './bridge.js';

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

/** 分頁 tooltip：完整名稱 + 執行了多久（日:時:分），第二行＝目前路徑。同舊版 `ToolTipText`。 */
function tooltipFor(tab) {
  let s = `${tab.title}  ${T['tip.tabElapsed']} ${elapsedText(tab.startedAt)}`;
  if (tab.cwdPath && tab.cwdPath !== tab.title) s += `\n${tab.cwdPath}`;
  if (tab.logging) s += `\n${T['tip.tabLogging']}`;
  return s;
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

/** 文字輸入對話框。回傳 Promise<string|null>（取消＝null）。 */
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
function askYesNo(title, prompt) {
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

/** 只有一個「確定」的訊息框（錯誤訊息用）。 */
function showInfo(title, prompt) {
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
  el.btnView.textContent =
    state.viewMode === 'tab'
      ? T['tb.split']
      : state.viewMode === 'split'
        ? T['tb.columns']
        : T['tb.tabs'];
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
    // 記錄 log 中：舊版 1.1.2 起分頁列不再放 log 圖示、只在 tooltip 註明，
    // 但一個小紅點不占空間又看得出來，所以這裡多一個（刻意不同，見文件）
    if (tab.logging) {
      const dot = document.createElement('span');
      dot.className = 'tab-logdot';
      dot.textContent = '●';
      dot.title = T['tip.tabLogging'];
      row.insertBefore(dot, close);
    }
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
  for (const m of [el.tabMenu, el.newMenu, el.pageMenu, el.termMenu, el.urlMenu]) m.hidden = true;
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
    if (item.dataset.act === 'color') return; // 有子選單，點父項不動作
    hideMenus();
    if (item.dataset.act === 'rename') renameTab(id);
    else if (item.dataset.act === 'log') logAction(id);
    else if (item.dataset.act === 'close') closeTab(id);
  });

  el.btnNew.addEventListener('click', (e) => {
    e.stopPropagation();
    const show = el.newMenu.hidden;
    hideMenus();
    if (show) showMenuUnder(el.newMenu, el.btnNew);
  });

  el.newMenu.addEventListener('click', async (e) => {
    const item = e.target.closest('[data-kind]');
    if (!item) return;
    hideMenus();
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
      case 'search':
        invoke('toolbar_search');
        break;
    }
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
      if (!el.modal.hidden) el.modalCancel.click();
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
  });
  el.btnCopyAll.addEventListener('click', () => {
    const id = activeId();
    if (id !== null) invoke('toolbar_copy_all', { id });
  });
  el.btnPaste.addEventListener('click', () => {
    const id = activeId();
    if (id !== null) pasteFromClipboard(id);
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

// ------------------------------------------------------------------ 啟動

export async function initTabBar() {
  el.btnNew = $('btn-new');
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
  el.toast = $('toast');
  el.modal = $('modal');
  el.modalForm = $('modal-form');
  el.modalTitle = $('modal-title');
  el.modalPrompt = $('modal-prompt');
  el.modalInput = $('modal-input');
  el.modalExtra = $('modal-extra');
  el.modalBrowse = $('modal-browse');
  el.modalCheck1 = $('modal-check1');
  el.modalCheck2 = $('modal-check2');
  el.modalCheck1Text = $('modal-check1-text');
  el.modalCheck2Text = $('modal-check2-text');
  el.modalOk = $('modal-ok');
  el.modalCancel = $('modal-cancel');

  // 介面文字一律從 strings.js 來（之後要做中英切換）
  el.btnNew.textContent = T['tb.new'] + ' ▾';
  el.btnCopy.textContent = T['tb.copy'];
  el.btnCopy.title = T['tip.copy'];
  el.btnPaste.textContent = T['tb.paste'];
  el.btnPaste.title = T['tip.paste'];
  el.btnCopyAll.textContent = T['tb.copyall'];
  el.btnCopyAll.title = T['tip.copyall'];
  el.btnClear.textContent = T['tb.clear'];
  el.btnClear.title = T['tip.clear'];
  el.btnPage.textContent = T['tb.page'] + ' ▾';
  el.btnPage.title = T['tip.page'];
  el.btnPanel.title = T['tip.tabPanel'];
  el.btnView.title = T['tip.viewCycle'];
  setText(el.newMenu, '[data-kind="shell"]', T['tb.powershell']);
  setText(el.newMenu, '[data-kind="custom"]', T['tb.customCmd']);
  setText(el.tabMenu, '[data-act="rename"]', T['menu.rename']);
  setText(el.tabMenu, '[data-act="log"]', T['menu.log']);
  setText(el.tabMenu, '[data-act="close"]', T['menu.close']);
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
    ['[data-term="copy"]', 'ctx.copy'],
    ['[data-term="copyPaste"]', 'ctx.copyPaste'],
    ['[data-term="paste"]', 'tb.paste'],
    ['[data-term="copyall"]', 'tb.copyall'],
    ['[data-term="copyAllFile"]', 'ctx.copyAllFile'],
    ['[data-term="search"]', 'ctx.search'],
  ]) {
    setText(el.termMenu, sel, T[key]);
  }
  setText(el.urlMenu, '[data-url="open"]', T['ctx.openUrl']);
  setText(el.urlMenu, '[data-url="copy"]', T['ctx.copyUrl']);
  // 配色父項的文字要保留子選單的箭頭，所以只改第一個文字節點
  el.tabMenu.querySelector('[data-act="color"]').firstChild.nodeValue = T['menu.color'];

  installToolbar();
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
  if (node) node.textContent = text;
}
