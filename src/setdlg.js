// 設定視窗（搬移舊版 `Dialogs/SettingsDialog`）。
//
// 舊版的欄位、分組、順序、預設值都照抄；**按「確定」才套用**（取消＝什麼都不動），
// 這一點和舊版一樣，不做「即時預覽」——使用者改壞顏色時還能按取消跑掉。
//
// 舊版只有 5 組欄位（語言／字體背景顏色／Claude 輸入送出／檔案總管 ＋ 回到預設）。
// PM 在 TASK-015 要求「`settings.json` 已有的欄位全部要能從這裡改」，所以多了
// 「其他」與「沙盒模式」兩組——哪個是舊版就有的寫在 docs/SETTINGS.md 的對照表。

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { T, fmt, LANGS, getLang as currentLang } from './strings.js';
import { setToolLabel } from './icons.js';
import { applyLang, onLangChange } from './i18n.js';
import { log } from './bridge.js';
import { loadAppFonts } from './appfonts.js';

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
  syncFontSelect(s.fontFamily);
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
  el.sandboxDefault.checked = !!s.sandboxDefault;
  el.askModel.checked = !!s.askModelOnOpen;
  el.modelsNote.textContent = '';
  syncModelsButton();
  el.weakCount.textContent = fmt('settings.weakCount', (s.sshWeakAccepted || []).length);
  // Telegram 遠端：狀態從後端問（**token 不回傳**，只回「有沒有設定」）
  el.renderer.value = ['auto', 'webgl', 'canvas', 'dom'].includes(s.renderer)
    ? s.renderer
    : 'auto';
  el.renderNote.textContent = fmt(
    'settings.rendererNote',
    (window.AwayActiveRenderer && window.AwayActiveRenderer()) || '-'
  );
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
  syncFontSelect('Cascadia Mono');
  el.size.value = '14';
  el.fg.value = '#E0E0E0';
  el.bg.value = '#1E1E1E';
  el.imeQuiet.value = '20';
  syncPicker(el.fg, el.fgPick);
  syncPicker(el.bg, el.bgPick);
}

/** 「自訂…」那一項的 value。字型不可能叫這個名字（前後都有底線，不是合法的家族名）。 */
const CUSTOM_FONT = '__custom__';

/**
 * 字型下拉。後端 `font_list` 回的是**這台機器實際裝的所有家族**
 * （`{ name, mono }`，等寬已經排在前面）。
 *
 * ⚠️ **不可以用 `<input list>` ＋ `<datalist>`**（TASK-031 用了，TASK-032 使用者回報
 * 「只看得到一個字型」）：Chromium 會拿**輸入框目前的值**去過濾候選，而設定一開啟
 * 輸入框就填著目前字型（例 `Cascadia Mono`），所以下拉只剩符合的那一個。
 * 改成真正的 `<select>` ＋ `<optgroup>`：點開一定是全部，而且鍵盤（上下鍵、Enter、
 * Esc、首字母跳選）原生就能用。清單以外的字型走最後一項「自訂…」。
 *
 * 每一項用**該字型本身**畫自己的名字（所見即所得）；`monospace` 當後備，
 * 這樣萬一家族名拼不出來也還看得見字。
 *
 * @param {string} current 目前生效的字型（標上「（目前使用）」並選起來，下拉一打開就看得到）
 */
async function fillFonts(current) {
  let fonts = [];
  try {
    fonts = await invoke('font_list');
  } catch (e) {
    log(`[settings] 讀字型清單失敗：${e}`);
  }
  el.familySel.textContent = '';
  fontSources = new Map();
  // 四組（TASK-033）：自帶的排最前面（一定在、跨平台一樣），再來是使用者自己
  // 下載／匯入的，最後才是這台機器裝的（等寬優先）。
  const groups = [
    ['font.groupBuiltin', fonts.filter((f) => f.source === 'builtin')],
    ['font.groupUser', fonts.filter((f) => f.source === 'user')],
    ['font.groupMono', fonts.filter((f) => f.source === 'system' && f.mono)],
    ['font.groupOther', fonts.filter((f) => f.source === 'system' && !f.mono)],
  ];
  let total = 0;
  for (const [label, list] of groups) {
    if (!list.length) continue;
    const g = document.createElement('optgroup');
    g.label = T[label] || label;
    g.dataset.k = label; // 換語言時 `applyTexts` 要用這個重貼標題
    for (const f of list) {
      // 後端舊版回的是純字串；相容性上拿到字串也要能用
      const name = typeof f === 'string' ? f : f.name;
      if (!name) continue;
      const opt = document.createElement('option');
      opt.value = name;
      fontSources.set(name, typeof f === 'string' ? 'system' : f.source);
      if (name === current) {
        opt.textContent = fmt('font.current', name);
        opt.dataset.current = '1';
      } else {
        opt.textContent = name;
      }
      // 家族名不會含雙引號（`name` 表裡沒看過），保險起見還是拿掉
      opt.style.fontFamily = `"${name.replace(/"/g, '')}", monospace`;
      g.appendChild(opt);
      total++;
    }
    el.familySel.appendChild(g);
  }
  // 最後一項：打清單以外的字型名稱
  const custom = document.createElement('option');
  custom.value = CUSTOM_FONT;
  custom.textContent = T['font.custom'];
  custom.dataset.k = 'font.custom';
  el.familySel.appendChild(custom);
  log(`[settings] 字型清單 ${total} 個家族（內建/下載匯入/系統等寬/系統其他 = ${groups.map(([, l]) => l.length).join(' / ')}）`);
}

/**
 * 下拉與輸入框對齊。`el.family`（輸入框）**永遠是真正的值**——`save()` 只讀它，
 * 所以「從下拉選」與「自己打」兩條路最後都收斂到同一個地方。
 */
function syncFontSelect(name) {
  const known = [...el.familySel.options].some((o) => o.value === name);
  el.familySel.value = known ? name : CUSTOM_FONT;
  el.family.hidden = known;
  updateFontButtons();
}

/**
 * 換語言時把字型下拉的**組標題／「自訂…」／「（目前使用）」**重貼一次。
 * 字型名稱本身不翻譯（那是家族名，翻了就選不到字型）。
 * 對話框沒開過時 `<select>` 是空的，這裡自然什麼都不做。
 */
function relabelFonts() {
  if (!el.familySel) return;
  for (const g of el.familySel.querySelectorAll('optgroup[data-k]')) {
    g.label = T[g.dataset.k] || g.label;
  }
  const custom = el.familySel.querySelector('option[data-k]');
  if (custom) custom.textContent = T[custom.dataset.k] || custom.textContent;
  const cur = el.familySel.querySelector('option[data-current]');
  if (cur) cur.textContent = fmt('font.current', cur.value);
}

/** 目前選到的家族來源（`builtin`／`user`／`system`）——「移除」只對 `user` 開放。 */
let fontSources = new Map();

/** 正在下載的那一套（`font-download` 事件的 id）。 */
let downloading = null;
/** 正在下載的那一套的家族名（進度文字要顯示這個，不是目錄 id；BUG-AUDIT B13）。 */
let downloadingFamily = '';

/** 下拉重畫＋維持目前選擇（匯入／下載／移除之後用）。 */
async function refreshFonts(keep) {
  // `''` 是「退回預設」（移除了正在選的字型），不是「沒指定」——只有 undefined 才沿用目前的值
  //（BUG-AUDIT B10：原本 `keep || …` 把空字串當沒指定，已移除的名字留在輸入框被存檔）
  const want = keep === undefined ? el.family.value : keep;
  await fillFonts(opened ? opened.fontFamily : want);
  syncFontSelect(want);
  el.family.value = want;
  updateFontButtons();
}

/** 「移除」只有在選到使用者自己下載／匯入的字型時才能按。 */
function updateFontButtons() {
  const src = fontSources.get(el.familySel.value);
  el.fontRemove.disabled = src !== 'user';
}

/** 位元組數寫成人看的（下載清單與進度都用）。 */
function mb(n) {
  return `${(Number(n) / 1048576).toFixed(1)} MB`;
}

/** 「下載更多中文等寬字型…」 */
async function downloadFont() {
  let list = [];
  try {
    list = await invoke('font_catalog');
  } catch (e) {
    el.fontNote.textContent = String(e);
    return;
  }
  const have = new Set(fontSources.keys());
  const items = list.map((c) => ({
    value: c.id,
    label: `${c.family}　${mb(c.bytes)}　${c.by}　${c.license}${have.has(c.family) ? `　${T['font.already']}` : ''}`,
    disabled: have.has(c.family),
  }));
  const id = await hooks.askFromList?.(T['font.downloadTitle'], items);
  if (!id) return;
  const entry = list.find((c) => c.id === id);
  downloading = id;
  downloadingFamily = entry ? entry.family : id;
  el.fontDownload.disabled = true;
  el.fontImport.disabled = true;
  el.fontCancel.hidden = false;
  el.fontNote.textContent = fmt('font.downloading', entry.family, '0', mb(entry.bytes));
  try {
    const family = await invoke('font_download', { id });
    el.fontNote.textContent = fmt('font.downloaded', family);
    // 新字型也要載進 webview，不然下拉裡看得到、畫面上卻是 fallback
    await loadAppFonts();
    await refreshFonts(family);
  } catch (e) {
    el.fontNote.textContent = String(e);
  } finally {
    downloading = null;
    el.fontDownload.disabled = false;
    el.fontImport.disabled = false;
    el.fontCancel.hidden = true;
  }
}

/** 「匯入字型…」：複製到設定資料夾底下的 `fonts/`，不安裝到系統。 */
async function importFonts() {
  let paths = null;
  try {
    paths = await invoke('font_pick_files', { title: T['font.importTitle'] });
  } catch (e) {
    el.fontNote.textContent = String(e);
    return;
  }
  if (!paths || !paths.length) return;
  el.fontImport.disabled = true;
  try {
    const added = await invoke('font_import', { paths });
    el.fontNote.textContent = added.length
      ? fmt('font.imported', added.length, added.join('、'))
      : T['font.importedNone'];
    await loadAppFonts();
    await refreshFonts(added[0] || el.family.value);
  } catch (e) {
    el.fontNote.textContent = String(e);
  } finally {
    el.fontImport.disabled = false;
  }
}

/** 「移除」：只刪使用者自己下載／匯入的（自帶的刪不掉，按鈕會是灰的）。 */
async function removeFont() {
  const family = el.familySel.value;
  if (fontSources.get(family) !== 'user') return;
  const ok = await hooks.askYesNo?.(T['settings.title'], fmt('font.removeAsk', family));
  if (!ok) return;
  try {
    const n = await invoke('font_remove', { family });
    el.fontNote.textContent = fmt('font.removed', family, n);
    // 移掉的正好是選著的那一套 → 退回預設，不要留一個選不到的名字
    const next = el.family.value === family ? '' : el.family.value;
    await refreshFonts(next);
  } catch (e) {
    el.fontNote.textContent = String(e);
  }
}

export async function openSettings() {
  try {
    opened = await invoke('settings_get');
  } catch (e) {
    hooks.showInfo?.(T['settings.title'], String(e));
    return;
  }
  await fillFonts(opened.fontFamily);
  el.fontNote.textContent = '';
  fill(opened);
  updateFontButtons();
  el.root.hidden = false;
  el.familySel.focus();
}

function close() {
  el.root.hidden = true;
}

/** 「更新模型清單」只有在「開啟時選模型」勾著的時候才能按（沒勾＝根本不會用到模型清單）。 */
function syncModelsButton() {
  el.modelsRefresh.disabled = !el.askModel.checked;
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
    askModelOnOpen: el.askModel.checked,
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
  // 註：Telegram 遠端搬回舊版的獨立「遠端設定」視窗了（`remotedlg.js`，TASK-029）。
  // 兩個地方改同一組欄位會互相蓋，所以這裡**不要**再放一份。
  // 介面文字：前端自己換（Rust 那邊有自己的一份表，見 src-tauri/src/i18n.rs）
  applyLang(after.language);
  // BUG D6：settings.json 壞掉（解析失敗）時這次改的只留在記憶體、重開就不見——要說出來，視窗不關
  const readonly = await invoke('settings_readonly_reason').catch(() => null);
  if (readonly) {
    el.note.textContent = readonly;
    return;
  }
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
  el.fontDownload.textContent = T['font.download'];
  el.fontImport.textContent = T['font.import'];
  el.fontRemove.textContent = T['font.remove'];
  el.fontCancel.textContent = T['dlg.cancel'];
  relabelFonts();
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
  el.lModels.textContent = T['settings.groupModels'];
  el.lAskModel.textContent = T['settings.askModel'];
  el.lAskModel.parentElement.title = T['settings.askModelNote'];
  el.modelsRefresh.textContent = T['settings.modelsRefresh'];
  // 說明放在 tooltip：旁邊那一格要留給「更新了幾個」的結果，再多一行字設定視窗就要捲了
  el.modelsRefresh.title = T['settings.modelsNote'];
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
  el.familySel = $('st-family-sel');
  el.fontDownload = $('st-font-download');
  el.fontImport = $('st-font-import');
  el.fontRemove = $('st-font-remove');
  el.fontCancel = $('st-font-cancel');
  el.fontNote = $('st-font-note');
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
  el.lModels = $('st-l-models');
  el.askModel = $('st-askmodel');
  el.lAskModel = $('st-l-askmodel');
  el.modelsRefresh = $('st-models-refresh');
  el.modelsNote = $('st-models-note');
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
    // 頁內 #modal（字型清單、確認框…）開著時 Esc 只關 modal，不連底下這個視窗一起關（BUG-AUDIT B3）
    if (e.key === 'Escape' && !el.root.hidden && document.getElementById('modal').hidden) close();
  });

  // 字型下拉：選「自訂…」才把輸入框叫出來；選真的字型就把值收回輸入框
  //（`save()` 只讀 `el.family`，兩條路收斂在同一個地方）
  el.fontDownload.addEventListener('click', downloadFont);
  el.fontImport.addEventListener('click', importFonts);
  el.fontRemove.addEventListener('click', removeFont);
  el.fontCancel.addEventListener('click', () => {
    if (downloading) invoke('font_download_cancel', { id: downloading }).catch(() => {});
  });
  // 下載進度（Rust 每 200ms 送一次）
  listen('font-download', (e) => {
    const p = e.payload || {};
    if (!downloading || p.id !== downloading) return;
    if (p.done) return; // 結束的訊息由 downloadFont 的 then/catch 寫
    const pct = p.total ? Math.round((p.got / p.total) * 100) : 0;
    el.fontNote.textContent = fmt('font.downloading', downloadingFamily || p.id, String(pct), mb(p.total || 0));
  }).catch((e) => log(`[settings] 掛下載進度 listener 失敗：${e}`));

  el.familySel.addEventListener('change', () => {
    updateFontButtons();
    const v = el.familySel.value;
    if (v === CUSTOM_FONT) {
      el.family.hidden = false;
      el.family.focus();
      el.family.select();
      return;
    }
    el.family.value = v;
    el.family.hidden = true;
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

  // 「開啟時選模型」沒勾＝用不到模型清單 →「更新模型清單」跟著不能按
  el.askModel.addEventListener('change', syncModelsButton);

  // 更新模型清單（2.0.3）：不等 10 分鐘的快取，立刻重新向這台電腦上每一家 AI CLI 問一次
  el.modelsRefresh.addEventListener('click', async () => {
    el.modelsRefresh.disabled = true;
    el.modelsNote.textContent = T['settings.modelsBusy'];
    try {
      const opts = await invoke('agent_setup_options', { kind: 'team' });
      const backends = opts.backends.map((b) => b.key);
      if (backends.length === 0) {
        el.modelsNote.textContent = T['settings.modelsNone'];
        return;
      }
      const lists = await invoke('cli_models', { backends, refresh: true });
      // 每一家各有幾個；問不到清單的那一家標「—」
      const parts = lists.map((l) => `${l.backendName} ${l.models.length > 0 ? l.models.length : '—'}`);
      el.modelsNote.textContent = fmt('settings.modelsDone', parts.join('、'));
    } catch (e) {
      el.modelsNote.textContent = fmt('settings.modelsFail', String(e));
    } finally {
      syncModelsButton();
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
