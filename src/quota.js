// 工具列右上角的 AI CLI 額度（2.0.11 新增，舊版沒有）。資料來源見 `src-tauri/src/quota.rs`。
//
// 格式是使用者 2026-10-06 定的（一家一行；沒有資料的那家不顯示）：
//   Claude:5h 41%|7d 12%|reset 1h26m
//   Codex:5h 22%|7d 38%|reset 3h46m
// 模型名稱一開始也在這一行，使用者看過之後拿掉了（太寬）→ 只放在滑鼠停上去的說明裡。
// 這一行的字（5h／7d／reset）各語言都一樣；滑鼠停上去的說明才跟著語言走。
// reset＝5 小時區間的倒數。日期時間格式不隨語言變（`CLAUDE.md` 的八語規則）。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { onLangChange } from './i18n.js';
import { log } from './bridge.js';

/** 多久問一次後端（讀 Codex 紀錄檔的尾端＋一個小 JSON，很便宜）。 */
const POLL_MS = 15_000;
/** 數字超過這麼久沒更新就變淡（CLI 沒在用的時候不會回報）。 */
const STALE_SECS = 30 * 60;
/** 已用超過這個百分比就標紅。 */
const HOT_PCT = 80;

let el = null;
/** 最近一次 `quota_get` 的結果，以及拿到它時本機與後端的時間差（秒）。 */
let last = null;
let skew = 0;

function nowSecs() {
  return Math.floor(Date.now() / 1000) + skew;
}

function pad2(n) {
  return String(n).padStart(2, '0');
}

/** 剩幾秒 → `1h26m`／`26m`；已經過了＝`0m`。 */
export function formatLeft(secs) {
  const m = Math.max(0, Math.floor(secs / 60));
  const h = Math.floor(m / 60);
  return h > 0 ? `${h}h${pad2(m % 60)}m` : `${m}m`;
}

/** Unix 秒 → `10/09 18:00`（本機時間；格式不隨語言變）。 */
function formatAt(secs) {
  const d = new Date(secs * 1000);
  return `${pad2(d.getMonth() + 1)}/${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

/** 已用百分比；區間已經重置過了就是 0（CLI 還沒回報新的數字）。 */
function usedOf(w, now) {
  if (!w) return null;
  if (w.resetsAt && w.resetsAt <= now) return 0;
  return Math.round(w.usedPct);
}

function pctSpan(label, used) {
  const s = document.createElement('span');
  s.textContent = `${label} ${used === null ? '—' : `${used}%`}`;
  if (used !== null && used >= HOT_PCT) s.className = 'quota-hot';
  return s;
}

function windowTip(key, w, now) {
  if (!w) return '';
  const used = usedOf(w, now);
  if (!w.resetsAt) return fmt(key, used, '—');
  return fmt(key, used, w.resetsAt <= now ? T['quota.resetDone'] : formatAt(w.resetsAt));
}

function agoText(q, now) {
  if (!q.updatedAt) return '';
  const mins = Math.max(0, Math.floor((now - q.updatedAt) / 60));
  return mins < 1 ? T['quota.updatedNow'] : fmt('quota.updatedAgo', mins);
}

/** 一家一行。`q` 是 `Quota`（見 quota.rs）。 */
function line(name, q, now) {
  const row = document.createElement('div');
  row.className = 'quota-row';
  if (q.updatedAt && now - q.updatedAt > STALE_SECS) row.classList.add('quota-stale');

  const parts = [];
  parts.push(pctSpan('5h', usedOf(q.fiveHour, now)));
  parts.push(pctSpan('7d', usedOf(q.sevenDay, now)));
  if (q.fiveHour && q.fiveHour.resetsAt) {
    const s = document.createElement('span');
    s.textContent = `reset ${formatLeft(q.fiveHour.resetsAt - now)}`;
    parts.push(s);
  }
  const model = [q.model, q.effort].filter(Boolean).join(' ');

  row.append(`${name}:`);
  parts.forEach((p, i) => {
    if (i > 0) row.append('|');
    row.append(p);
  });

  const tip = [
    model ? `${name} · ${model}` : name,
    windowTip('quota.tip5h', q.fiveHour, now),
    windowTip('quota.tip7d', q.sevenDay, now),
    agoText(q, now),
    name === 'Claude' ? T['quota.claudeNote'] : '',
  ].filter(Boolean);
  row.title = tip.join('\n');
  return row;
}

function render() {
  if (!el) return;
  el.textContent = '';
  const now = nowSecs();
  // 還沒有額度數字（Claude Code 剛開、還沒問過問題）＝不顯示那一行，不要一排「—」
  const has = (q) => q && (q.fiveHour || q.sevenDay);
  if (last && has(last.claude)) el.append(line('Claude', last.claude, now));
  if (last && has(last.codex)) el.append(line('Codex', last.codex, now));
  el.hidden = el.childElementCount === 0;
}

async function refresh() {
  try {
    last = await invoke('quota_get');
    skew = last.now - Math.floor(Date.now() / 1000);
  } catch (e) {
    log(`[quota] 讀不到額度：${e}`);
  }
  render();
}

export function initQuota() {
  el = document.getElementById('quota');
  if (!el) return;
  // 語言切換＝重畫說明文字
  onLangChange(render);
  refresh();
  setInterval(refresh, POLL_MS);
}
