// 「關於」與「檢查更新」（搬移舊版 `MainWindow.About_Click` / `ShowUpdateDialog`）。
//
// 版式照舊版：標題／版本／編譯時間／作者／下載／Source Code／授權／第三方元件，
// 底部是「狀態文字 ＋ 檢查更新 ＋ 關閉」。
//
// 照舊版的三個重點：
//   1. **只有按下「檢查更新」才連外**，啟動時不自動查。
//   2. 檢查失敗（離線／伺服器沒回 ok）只在按鈕旁顯示一行字，**不跳錯誤視窗**。
//   3. 有新版才跳第二個視窗，按鈕開「軟體頁」讓使用者自己選安裝版／免安裝版
//      （自動下載安裝是 Tauri updater，屬於階段 5）。
//
// 第三方授權聲明**不複製一份**：後端讀 repo／安裝目錄裡的 `THIRD-PARTY-NOTICES.md`。

import { invoke } from '@tauri-apps/api/core';

import { T, LANGS, getLang } from './strings.js';
import { setToolLabel } from './icons.js';
import { onLangChange } from './i18n.js';

const el = {};
let hooks = {};
/** `about_info` 的結果（開一次就夠，版本不會變）。 */
let info = null;
/** 讀進來的 THIRD-PARTY-NOTICES.md（第一次展開才讀）。 */
let notices = null;

function $(id) {
  return document.getElementById(id);
}

/** 一列「標籤：值」。`link` 為真時值是可點的連結。 */
function row(label, value, link) {
  const div = document.createElement('div');
  div.className = 'about-row';
  const b = document.createElement('span');
  b.className = 'about-label';
  b.textContent = label ? `${label}: ` : '';
  div.append(b);
  if (link) {
    const a = document.createElement('button');
    a.type = 'button';
    a.className = 'link-btn';
    a.textContent = value;
    a.addEventListener('click', () => {
      invoke('open_url', { url: value }).catch(() => {});
    });
    div.append(a);
  } else {
    const s = document.createElement('span');
    s.textContent = value;
    div.append(s);
  }
  return div;
}

/**
 * 作者那一行的 email **畫成圖**（同舊版 `RenderTextImage` 的用意：
 * 畫面上沒有可以被爬蟲抓走的 email 文字）。
 * 這裡用 canvas；字級跟著介面，所以在高 DPI 螢幕上也不會糊。
 */
function authorImage(parts) {
  const text = parts.join('');
  const scale = window.devicePixelRatio || 1;
  const canvas = document.createElement('canvas');
  const ctx = canvas.getContext('2d');
  const font = '13px "Segoe UI", system-ui, sans-serif';
  ctx.font = font;
  const w = Math.ceil(ctx.measureText(text).width) + 2;
  const h = 18;
  canvas.width = Math.ceil(w * scale);
  canvas.height = Math.ceil(h * scale);
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  ctx.scale(scale, scale);
  ctx.font = font;
  ctx.fillStyle = '#E0E0E0';
  ctx.textBaseline = 'middle';
  ctx.fillText(text, 1, h / 2);
  canvas.className = 'about-author';
  return canvas;
}

/** 目前語言的自稱（關於頁顯示）。 */
function langName() {
  const hit = LANGS.find((l) => l.code === getLang());
  return hit ? `${hit.name} (${hit.code})` : getLang();
}

function render() {
  el.rows.textContent = '';
  el.rows.append(row(T['about.version'], `v${info.version}`));
  el.rows.append(row(T['about.buildTime'], info.buildTime));

  const author = document.createElement('div');
  author.className = 'about-row';
  const al = document.createElement('span');
  al.className = 'about-label';
  al.textContent = `${T['about.author']}: `;
  author.append(al, authorImage(info.authorParts));
  el.rows.append(author);

  el.rows.append(row(T['about.download'], info.downloadUrl, true));
  el.rows.append(row('Source Code', info.sourceUrl, true));
  el.rows.append(row(T['about.license'], 'MIT　© 2026 Chih-Wei Su (Awaysu)'));
  // 目前實際在用的終端機渲染器（WebGL／canvas／DOM）。
  // Linux 的 WebKitGTK 上 WebGL 可能被停用而靜靜退回 DOM ——那會慢很多，
  // 但畫面看起來一樣，所以要有地方看得到（`CLAUDE.md` 風險 2 解法 c）。
  const rend = (window.AwayActiveRenderer && window.AwayActiveRenderer()) || '-';
  el.rows.append(row(T['about.renderer'], rend));
  // 介面語言：目前語言 ＋「機器翻譯，歡迎修正」（PM 在 TASK-015 修訂版要求關於頁也放一行）
  el.rows.append(row(T['settings.groupLang'], `${langName()}　${T['settings.langNote']}`));
  // 第三方元件：xterm.js 的版本是 build 時從 node_modules 讀的**實際**版本
  //（舊版寫死成 5.5.0，實際是 6.0.0 —— CLAUDE.md 記著這條雷）
  el.rows.append(
    row(
      T['about.thirdParty'],
      `xterm.js ${info.xtermVersion} (MIT)、Tauri ${info.tauriVersion} (MIT/Apache-2.0)、` +
        `russh (Apache-2.0)、serialport-rs (MPL-2.0)`,
    ),
  );
}

export async function openAbout() {
  if (!info) {
    try {
      info = await invoke('about_info');
    } catch (e) {
      hooks.showInfo?.(T['about.title'], String(e));
      return;
    }
  }
  render();
  el.status.textContent = '';
  el.check.disabled = false;
  el.notices.open = false;
  el.root.hidden = false;
  el.close.focus();
}

async function loadNotices() {
  if (notices !== null) return;
  try {
    notices = await invoke('third_party_notices');
  } catch (e) {
    notices = String(e);
  }
  el.noticesText.textContent = notices;
}

async function checkUpdate() {
  el.check.disabled = true;
  el.status.textContent = T['update.checking'];
  let r = null;
  try {
    r = await invoke('update_check', { current: info.version });
  } catch {
    r = null;
  }
  el.check.disabled = false;
  if (!r) {
    // 離線／伺服器沒回 ok：只顯示一行（舊版刻意不跳視窗）
    el.status.textContent = T['update.failed'];
    return;
  }
  if (!r.updateAvailable) {
    el.status.textContent = `${T['update.latest']} (v${r.latestVersion})`;
    return;
  }
  el.status.textContent = '';
  showUpdate(r);
}

function showUpdate(r) {
  el.upTitle.textContent = T['update.found'];
  el.upRows.textContent = '';
  el.upRows.append(row(T['update.current'], `v${info.version}`));
  el.upRows.append(row(T['update.latestVer'], `v${r.latestVersion}`));
  const notes = (r.releaseNotes || '').trim();
  el.upNotesLabel.textContent = notes ? `${T['update.notes']}:` : '';
  el.upNotes.textContent = notes;
  el.upNotes.hidden = !notes;
  el.upGo.textContent = T['update.goDownload'];
  el.upClose.textContent = T['update.close'];
  el.upGo.onclick = () => {
    invoke('open_url', { url: r.pageUrl }).catch(() => {});
    el.upRoot.hidden = true;
  };
  el.upRoot.hidden = false;
  el.upGo.focus();
}

/** 把介面文字重設一次（切語言時會被叫）。 */
function applyTexts() {
  setToolLabel(el.btn, T['tb.about']);
  el.btn.title = T['tip.about'];
  el.noticesLabel.textContent = T['about.noticesLink'];
  el.check.textContent = T['update.check'];
  el.close.textContent = T['about.close'];
  el.upTitle.textContent = T['update.found'];
  el.upGo.textContent = T['update.goDownload'];
  el.upClose.textContent = T['update.close'];
  if (info && !el.root.hidden) render();
}

export function initAbout(injected) {
  hooks = injected || {};
  el.root = $('aboutdlg');
  el.btn = $('btn-about');
  el.rows = $('about-rows');
  el.notices = $('about-notices');
  el.noticesLabel = $('about-notices-label');
  el.noticesText = $('about-notices-text');
  el.status = $('about-status');
  el.check = $('about-check');
  el.close = $('about-close');
  el.upRoot = $('updatedlg');
  el.upTitle = $('up-title');
  el.upRows = $('up-rows');
  el.upNotesLabel = $('up-notes-label');
  el.upNotes = $('up-notes');
  el.upGo = $('up-go');
  el.upClose = $('up-close');

  onLangChange(applyTexts);

  el.btn.addEventListener('click', () => {
    hooks.hideMenus?.();
    openAbout();
  });
  el.close.addEventListener('click', () => {
    el.root.hidden = true;
  });
  el.check.addEventListener('click', checkUpdate);
  el.notices.addEventListener('toggle', () => {
    if (el.notices.open) loadNotices();
  });
  el.upClose.addEventListener('click', () => {
    el.upRoot.hidden = true;
  });
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    if (!el.upRoot.hidden) el.upRoot.hidden = true;
    else if (!el.root.hidden) el.root.hidden = true;
  });

  return { openAbout };
}
