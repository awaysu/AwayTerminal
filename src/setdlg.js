// 設定視窗（搬移舊版 `Dialogs/SettingsDialog`）。
//
// 舊版的欄位、分組、順序、預設值都照抄；**按「確定」才套用**（取消＝什麼都不動），
// 這一點和舊版一樣，不做「即時預覽」——使用者改壞顏色時還能按取消跑掉。
//
// 舊版只有 5 組欄位（語言／字體背景顏色／Claude 輸入送出／檔案總管 ＋ 回到預設）。
// PM 在 TASK-015 要求「`settings.json` 已有的欄位全部要能從這裡改」，所以多了
// 「其他」與「沙盒模式」兩組——哪個是舊版就有的寫在 docs/SETTINGS.md 的對照表。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt, LANGS, getLang as currentLang } from './strings.js';
import { setToolLabel } from './icons.js';
import { applyLang, onLangChange } from './i18n.js';
import { log } from './bridge.js';

const el = {};
let hooks = {};
/** 開啟時的設定（按「取消」要能原樣回去，也用來判斷哪些欄位真的被改了）。 */
let opened = null;
/** 開啟時「檔案總管右鍵選單」是不是已經登錄（只在勾選變了才動登錄檔）。 */
let shellMenuBefore = false;

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
  // 舊設定檔可能是只有中英兩種時期的 `zh` → 當成 zh-TW
  const lang = s.language === 'zh' ? 'zh-TW' : s.language || currentLang();
  el.lang.value = LANGS.some((l) => l.code === lang) ? lang : currentLang();
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
  // Telegram 遠端：狀態從後端問（**token 不回傳**，只回「有沒有設定」）
  el.renderer.value = ['auto', 'webgl', 'canvas', 'dom'].includes(s.renderer)
    ? s.renderer
    : 'auto';
  el.renderNote.textContent = fmt(
    'settings.rendererNote',
    (window.AwayActiveRenderer && window.AwayActiveRenderer()) || '-'
  );
  el.tgEnabled.checked = !!s.remoteEnabled;
  el.tgChat.value = s.telegramChatId ? String(s.telegramChatId) : '';
  el.tgNotify.checked = !!s.remoteNotify;
  el.tgToken.value = '';
  el.tgToken.placeholder = '';
  el.tgNote.textContent = '';
  invoke('telegram_state')
    .then((st) => {
      el.tgToken.placeholder = T[st.hasToken ? 'settings.tgTokenSet' : 'settings.tgTokenNone'];
      el.tgNote.textContent = T[st.running ? 'settings.tgRunning' : 'settings.tgStopped'];
    })
    .catch(() => {});
  el.note.textContent = '';
  // 檔案總管右鍵選單：狀態直接讀登錄檔（不是讀設定——使用者可能用別的方式刪過）
  shellMenuBefore = false;
  el.shellMenu.disabled = true;
  invoke('shell_menu_state')
    .then((st) => {
      shellMenuBefore = !!st.enabled;
      el.shellMenu.checked = shellMenuBefore;
      el.shellMenu.disabled = false;
      el.shellNote.textContent = st.enabled ? st.command : T['settings.shellMenuHint'];
    })
    .catch(() => {
      // 非 Windows：這個功能沒有（command 也不存在）→ 維持灰掉並註明
      el.shellMenu.checked = false;
      el.shellMenu.disabled = true;
        });
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
  const lang = el.lang.value;
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
    // 渲染器：改了要重開分頁才生效（addon 在建 pane 時掛）
    renderer: el.renderer.value,
  };
  let after;
  try {
    // Rust 端寫檔、夾好範圍、重送 `T{json}`（字型／字級／顏色因此即時套到所有分頁）
    after = await invoke('settings_apply', { patch });
  } catch (err) {
    el.note.textContent = String(err);
    return;
  }
  // 檔案總管右鍵選單：勾＝寫入、取消＝刪除（只在勾選狀態真的變了才動登錄檔）
  if (!el.shellMenu.disabled && el.shellMenu.checked !== shellMenuBefore) {
    try {
      const st = await invoke('shell_menu_apply', {
        enable: el.shellMenu.checked,
        // 選單文字跟著語言（同舊版 `Loc.T("shell.menuText")`）
        text: T['shell.menuText'],
      });
      log(`[settings] 檔案總管右鍵選單：${st.enabled ? '已登錄' : '已移除'}${st.command ? '（' + st.command + '）' : ''}`);
    } catch (err) {
      el.note.textContent = String(err);
      return;
    }
  }
  // Telegram 遠端：token 留空＝不動（設定視窗永遠不回填，留空就不能當成「清掉」）
  try {
    const st = await invoke('telegram_apply', {
      enabled: el.tgEnabled.checked,
      token: el.tgToken.value.trim() ? el.tgToken.value.trim() : null,
      chatId: Number(el.tgChat.value.trim()) || 0,
      notify: el.tgNotify.checked,
    });
    // ⚠️ 這一行**不可以**印 token（後端也不回傳）
    log(`[settings] Telegram 遠端：${st.running ? '已啟動' : '未啟動'} chat=${st.chatId} token=${st.hasToken ? '已設定' : '未設定'}`);
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
  el.lLangPick.textContent = T['settings.groupLang'];
  el.langNote.textContent = T['settings.langNote'];
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
  el.lMigrate.textContent = T['settings.groupMigrate'];
  el.migrate.textContent = T['migrate.button'];
  el.lShell.textContent = T['settings.groupShell'];
  el.lShellMenu.textContent = T['settings.shellMenu'];
  el.lRender.textContent = T['settings.groupRender'];
  el.lRenderPick.textContent = T['settings.renderer'];
  for (const o of el.renderer.options) {
    o.textContent = T[`settings.renderer.${o.value}`] || o.value;
  }
  el.renderNote.textContent = fmt(
    'settings.rendererNote',
    (window.AwayActiveRenderer && window.AwayActiveRenderer()) || '-'
  );
  el.lTg.textContent = T['settings.groupTg'];
  el.lTgEnabled.textContent = T['settings.tgEnabled'];
  el.lTgToken.textContent = T['settings.tgToken'];
  el.lTgChat.textContent = T['settings.tgChat'];
  el.lTgNotify.textContent = T['settings.tgNotify'];
  el.reset.textContent = T['common.reset'];
  el.ok.textContent = T['common.ok'];
  el.cancel.textContent = T['common.cancel'];
  setToolLabel(el.btn, T['tb.settings']);
  el.btn.title = T['tip.settings'];
}

export function initSettings(injected) {
  hooks = injected || {};
  el.root = $('setdlg');
  el.form = $('setdlg-box');
  el.title = $('setdlg-title');
  el.btn = $('btn-settings');
  el.lang = $('st-lang');
  el.lLang = $('st-l-lang');
  el.lLangPick = $('st-l-langpick');
  el.langNote = $('st-lang-note');
  // 選項的文字用**該語言自己的寫法**（使用者看不懂目前語言時也找得到自己的）
  el.lang.innerHTML = LANGS.map((l) => `<option value="${l.code}">${l.name}</option>`).join('');
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
  el.lMigrate = $('st-l-migrate');
  el.migrate = $('st-migrate');
  el.migrateNote = $('st-migrate-note');
  el.lShell = $('st-l-shell');
  el.lShellMenu = $('st-l-shellmenu');
  // ⚠️ 這一行從 TASK-016 就漏掉了（TASK-028 才發現）：`fill()` 第一件事就是
  // `el.shellMenu.disabled = true`，沒有它整個 `openSettings()` 在
  // `el.root.hidden = false` **之前**就丟例外 → 按「其他設定」完全沒反應。
  el.shellMenu = $('st-shellmenu');
  el.shellNote = $('st-shell-note');
  el.lRender = $('st-l-render');
  el.lRenderPick = $('st-l-renderpick');
  el.renderer = $('st-renderer');
  el.renderNote = $('st-render-note');
  el.lTg = $('st-l-tg');
  el.lTgEnabled = $('st-l-tgenabled');
  el.lTgToken = $('st-l-tgtoken');
  el.lTgChat = $('st-l-tgchat');
  el.lTgNotify = $('st-l-tgnotify');
  el.tgEnabled = $('st-tg-enabled');
  el.tgToken = $('st-tg-token');
  el.tgChat = $('st-tg-chat');
  el.tgNotify = $('st-tg-notify');
  el.tgNote = $('st-tg-note');
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

  // 匯入舊版設定（選檔；第一次啟動的自動提示在 main.js）
  el.migrate.addEventListener('click', async () => {
    el.migrateNote.textContent = '';
    try {
      const file = await invoke('migrate_pick_file');
      if (!file) return;
      const r = await invoke('migrate_import', { path: file });
      el.migrateNote.textContent = fmt('migrate.done', r.applied, r.conns, r.favorites);
      // 匯進來的值要立刻反映在畫面上
      opened = await invoke('settings_get');
      fill(opened);
      applyLang(opened.language);
      if (r.warnings.length) el.note.textContent = r.warnings.join('　');
    } catch (e) {
      el.migrateNote.textContent = String(e);
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

  return { openSettings, currentLang };
}
