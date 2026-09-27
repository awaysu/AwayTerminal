// 設定視窗（搬移舊版 `Dialogs/SettingsDialog`）。
//
// 舊版的欄位、分組、順序、預設值都照抄；**按「確定」才套用**（取消＝什麼都不動），
// 這一點和舊版一樣，不做「即時預覽」——使用者改壞顏色時還能按取消跑掉。
//
// 舊版只有 5 組欄位（語言／字體背景顏色／Claude 輸入送出／檔案總管 ＋ 回到預設）。
// PM 在 TASK-015 要求「`settings.json` 已有的欄位全部要能從這裡改」，所以多了
// 「其他」與「沙盒模式」兩組——哪個是舊版就有的寫在 docs/SETTINGS.md 的對照表。

import { invoke } from '@tauri-apps/api/core';

import { T } from './strings.js';
import { fmt } from './strings.js';
import { applyLang, getLang } from './i18n.js';
import { onLangChange } from './i18n.js';
import { log } from './bridge.js';

const el = {};
let hooks = {};
/** 開啟時的設定（按「取消」要能原樣回去，也用來判斷哪些欄位真的被改了）。 */
let opened = null;

function $(id) {
  return document.getElementById(id);
}

/** 顏色輸入框旁邊的小色塊（舊版是 `Border` + `ColorDialog`，這裡用原生 `<input type=color>`）。 */
function syncPicker(text, picker) {
  const v = text.value.trim();
  if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(v)) picker.value = expand(v);
}

function expand(hex) {
  if (hex.length === 4) return '#' + [...hex.slice(1)].map((c) => c + c).join('');
  return hex.toLowerCase();
}

function fill(s) {
  (s.language === 'en' ? el.langEn : el.langZh).checked = true;
  el.family.value = s.fontFamily;
  el.size.value = String(s.fontSize);
  el.fg.value = s.foreground;
  el.bg.value = s.background;
  syncPicker(el.fg, el.fgPick);
  syncPicker(el.bg, el.bgPick);
  el.imeQuiet.value = String(s.imeQuietMs);
  el.restoreLines.value = String(s.restoreBufferLines);
  el.keepAlive.value = String(s.keepAliveMins);
  el.logDir.value = s.logDir;
  el.exitRestore.checked = s.exitRestoreTabs !== false;
  el.autoReconnect.checked = !!s.autoReconnect;
  el.logTs.checked = s.logTimestamp !== false;
  el.logAppend.checked = s.logAppend !== false;
  el.sandboxDefault.checked = s.sandboxDefault !== false;
  el.weakCount.textContent = fmt('settings.weakCount', (s.sshWeakAccepted || []).length);
  el.note.textContent = '';
}

/** 「回到預設」：只重設**舊版那個按鈕會重設的欄位**（語言與其他組不動，同舊版）。 */
function resetDefaults() {
  el.family.value = 'Cascadia Mono';
  el.size.value = '14';
  el.fg.value = '#E0E0E0';
  el.bg.value = '#1E1E1E';
  el.imeQuiet.value = '20';
  syncPicker(el.fg, el.fgPick);
  syncPicker(el.bg, el.bgPick);
}

export async function openSettings() {
  try {
    opened = await invoke('settings_get');
  } catch (e) {
    hooks.showInfo?.(T['settings.title'], String(e));
    return;
  }
  // 字型清單（後端只回這台機器真的有的；輸入框是 datalist，可以自己打）
  try {
    const fonts = await invoke('font_list');
    el.fonts.innerHTML = fonts.map((f) => `<option value="${f}"></option>`).join('');
  } catch {
    el.fonts.innerHTML = '';
  }
  fill(opened);
  el.root.hidden = false;
  el.family.focus();
}

function close() {
  el.root.hidden = true;
}

async function save(e) {
  e?.preventDefault();
  const lang = el.langEn.checked ? 'en' : 'zh';
  const patch = {
    language: lang,
    fontFamily: el.family.value,
    fontSize: Number(el.size.value) || opened.fontSize,
    foreground: el.fg.value,
    background: el.bg.value,
    imeQuietMs: Number(el.imeQuiet.value) || 0,
    restoreBufferLines: Number(el.restoreLines.value) || 0,
    keepAliveMins: Number(el.keepAlive.value) || 0,
    autoReconnect: el.autoReconnect.checked,
    logDir: el.logDir.value,
    logTimestamp: el.logTs.checked,
    logAppend: el.logAppend.checked,
    exitRestoreTabs: el.exitRestore.checked,
    sandboxDefault: el.sandboxDefault.checked,
  };
  let after;
  try {
    // Rust 端寫檔、夾好範圍、重送 `T{json}`（字型／字級／顏色因此即時套到所有分頁）
    after = await invoke('settings_apply', { patch });
  } catch (err) {
    el.note.textContent = String(err);
    return;
  }
  // 介面文字：前端自己換（Rust 那邊有自己的一份表，見 src-tauri/src/i18n.rs）
  applyLang(after.language);
  close();
  log(
    `[settings] 已套用：語言=${after.language} 字型=${after.fontFamily} ${after.fontSize}px ` +
      `前景=${after.foreground} 背景=${after.background} imeQuiet=${after.imeQuietMs}ms`,
  );
}

/** 把介面文字重設一次（切語言時會被叫；註冊在 `i18n.js`）。 */
function applyTexts() {
  el.title.textContent = T['settings.title'];
  el.lLang.textContent = T['settings.groupLang'];
  el.lFont.textContent = T['settings.groupFont'];
  el.lFamily.textContent = T['font.family'];
  el.lSize.textContent = T['font.size'];
  el.lFg.textContent = T['font.fg'];
  el.lBg.textContent = T['font.bg'];
  el.fgPick.title = T['font.pick'];
  el.bgPick.title = T['font.pick'];
  el.lIme.textContent = T['settings.groupIme'];
  el.lImeQuiet.textContent = T['settings.imeQuiet'];
  el.imeHelp.textContent = T['settings.imeQuietHelpLink'];
  el.lMore.textContent = T['settings.groupMore'];
  el.lRestoreLines.textContent = T['settings.restoreLines'];
  el.restoreLinesHint.textContent = T['settings.restoreLinesHint'];
  el.lKeepAlive.textContent = T['settings.keepAlive'];
  el.lLogDir.textContent = T['settings.logDir'];
  el.logDirPick.textContent = T['settings.browse'];
  el.lExitRestore.textContent = T['settings.exitRestore'];
  el.lAutoReconnect.textContent = T['settings.autoReconnect'];
  el.lLogTs.textContent = T['settings.logTimestamp'];
  el.lLogAppend.textContent = T['settings.logAppend'];
  el.lSandbox.textContent = T['settings.groupSandbox'];
  el.lSandboxDefault.textContent = T['settings.sandboxDefault'];
  el.sandboxNote.textContent = T['settings.sandboxNote'];
  el.weakClear.textContent = T['settings.weakClear'];
  el.lShell.textContent = T['settings.groupShell'];
  el.lShellMenu.textContent = T['settings.shellMenu'];
  el.shellNote.textContent = T['settings.todo'];
  el.reset.textContent = T['common.reset'];
  el.ok.textContent = T['common.ok'];
  el.cancel.textContent = T['common.cancel'];
  el.btn.textContent = T['tb.settings'];
  el.btn.title = T['tip.settings'];
}

export function initSettings(injected) {
  hooks = injected || {};
  el.root = $('setdlg');
  el.form = $('setdlg-box');
  el.title = $('setdlg-title');
  el.btn = $('btn-settings');
  el.langZh = $('st-lang-zh');
  el.langEn = $('st-lang-en');
  el.lLang = $('st-l-lang');
  el.lFont = $('st-l-font');
  el.lFamily = $('st-l-family');
  el.lSize = $('st-l-size');
  el.lFg = $('st-l-fg');
  el.lBg = $('st-l-bg');
  el.family = $('st-family');
  el.fonts = $('st-fonts');
  el.size = $('st-size');
  el.fg = $('st-fg');
  el.bg = $('st-bg');
  el.fgPick = $('st-fg-pick');
  el.bgPick = $('st-bg-pick');
  el.lIme = $('st-l-ime');
  el.lImeQuiet = $('st-l-imequiet');
  el.imeQuiet = $('st-imequiet');
  el.imeHelp = $('st-imehelp');
  el.lMore = $('st-l-more');
  el.lRestoreLines = $('st-l-restorelines');
  el.restoreLines = $('st-restorelines');
  el.restoreLinesHint = $('st-restorelines-hint');
  el.lKeepAlive = $('st-l-keepalive');
  el.keepAlive = $('st-keepalive');
  el.lLogDir = $('st-l-logdir');
  el.logDir = $('st-logdir');
  el.logDirPick = $('st-logdir-pick');
  el.lExitRestore = $('st-l-exitrestore');
  el.exitRestore = $('st-exitrestore');
  el.lAutoReconnect = $('st-l-autoreconnect');
  el.autoReconnect = $('st-autoreconnect');
  el.lLogTs = $('st-l-logts');
  el.logTs = $('st-logts');
  el.lLogAppend = $('st-l-logappend');
  el.logAppend = $('st-logappend');
  el.lSandbox = $('st-l-sandbox');
  el.lSandboxDefault = $('st-l-sandboxdefault');
  el.sandboxDefault = $('st-sandboxdefault');
  el.sandboxNote = $('st-sandbox-note');
  el.weakClear = $('st-weak-clear');
  el.weakCount = $('st-weak-count');
  el.lShell = $('st-l-shell');
  el.lShellMenu = $('st-l-shellmenu');
  el.shellNote = $('st-shell-note');
  el.note = $('st-note');
  el.reset = $('st-reset');
  el.ok = $('st-ok');
  el.cancel = $('st-cancel');

  onLangChange(applyTexts);

  el.btn.addEventListener('click', () => {
    hooks.hideMenus?.();
    openSettings();
  });
  el.form.addEventListener('submit', save);
  el.cancel.addEventListener('click', close);
  el.reset.addEventListener('click', resetDefaults);
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !el.root.hidden) close();
  });

  el.fg.addEventListener('input', () => syncPicker(el.fg, el.fgPick));
  el.bg.addEventListener('input', () => syncPicker(el.bg, el.bgPick));
  el.fgPick.addEventListener('input', () => {
    el.fg.value = el.fgPick.value.toUpperCase();
  });
  el.bgPick.addEventListener('input', () => {
    el.bg.value = el.bgPick.value.toUpperCase();
  });

  // 「這是什麼？」：舊版是一個 MessageBox，內容逐字照抄
  el.imeHelp.addEventListener('click', () => {
    hooks.showInfo?.(T['settings.imeQuietHelpTitle'], T['settings.imeQuietHelp']);
  });

  el.logDirPick.addEventListener('click', async () => {
    try {
      const dir = await invoke('pick_work_dir', { title: T['settings.logDir'] });
      if (dir) el.logDir.value = dir;
    } catch (e) {
      el.note.textContent = String(e);
    }
  });

  el.weakClear.addEventListener('click', async () => {
    try {
      const n = await invoke('ssh_weak_clear');
      el.weakCount.textContent = fmt('settings.weakCount', 0);
      el.note.textContent = `${T['settings.weakCleared']}（${n}）`;
    } catch (e) {
      el.note.textContent = String(e);
    }
  });

  return { openSettings, currentLang: getLang };
}
