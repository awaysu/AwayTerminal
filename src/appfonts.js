// 程式自帶／下載／匯入的字型：載進 webview，**不安裝到系統**。
//
// 為什麼要有這一層（TASK-033）：
//   * 舊版只能用「這台機器裝了什麼」，所以在別人的新電腦上中文常常變成非等寬，
//     xterm 的欄位對位整片跑掉；
//   * 三個平台各自預設不同的字型，畫面長得不一樣。
// 現在安裝檔自帶 JetBrains Mono／Cascadia Mono／Sarasa Mono TC，三個平台一致。
//
// 做法：Rust 的 `font_faces` 回「有哪些檔案、家族名／粗細／斜體是什麼」，
// `font_face_bytes` 回檔案內容（raw bytes，不經 JSON），前端用 `FontFace` 加進
// `document.fonts`。**不用 asset 協定**——那要設 scope，而且開發與安裝後的路徑不一樣；
// 位元組這條路在 dev 與 release 行為完全相同。
//
// ⚠️ **一定要等字型真的載完才讓 xterm 量字寬**：xterm 是用「量一個字有多寬」決定每一格的
// 大小，字型還沒到就會用 fallback 量出錯的寬度，然後整個畫面的欄位都對不準
//（而且之後不會自己重量）。所以 `main.js` 在載入 `terminal.js` **之前** await 這裡。

import { invoke } from '@tauri-apps/api/core';

import { log } from './bridge.js';

/** 已經加進 `document.fonts` 的檔案路徑（重新整理清單時不要重複載）。 */
const loaded = new Set();

/**
 * 把自帶與使用者的字型載進 webview。
 *
 * 失敗**不會**讓啟動失敗：載不到就退回系統字型（畫面還是能用，只是中文可能不等寬）。
 * 回傳成功載入的家族名（去重）。
 */
export async function loadAppFonts() {
  let faces = [];
  try {
    faces = await invoke('font_faces');
  } catch (e) {
    log(`[fonts] 讀自帶字型清單失敗：${e}`);
    return [];
  }
  const families = new Set();
  const t0 = performance.now();
  await Promise.all(
    faces.map(async (f) => {
      families.add(f.family);
      if (loaded.has(f.path)) return;
      try {
        // raw bytes（`tauri::ipc::Response`）→ ArrayBuffer，不經 base64／JSON
        const buf = await invoke('font_face_bytes', { path: f.path });
        const face = new FontFace(f.family, buf, {
          weight: String(f.weight),
          style: f.style,
          // 字型很大（Sarasa 13 MB），但終端機**第一個畫面就要用它** → 不要 swap
          display: 'block',
        });
        await face.load();
        document.fonts.add(face);
        loaded.add(f.path);
      } catch (e) {
        families.delete(f.family);
        log(`[fonts] 載入失敗 ${f.family}（${f.path}）：${e}`);
      }
    })
  );
  // `document.fonts.ready` ＝瀏覽器把所有待處理的字型都處理完了
  try {
    await document.fonts.ready;
  } catch {
    /* 沒有這個 API 就算了 */
  }
  const ms = Math.round(performance.now() - t0);
  log(`[fonts] 自帶／使用者字型：${faces.length} 個檔、${families.size} 個家族，載入 ${ms} ms`);
  return [...families];
}

/**
 * 某個家族**真的可以畫字了嗎**（`--verify` 與設定視窗用）。
 *
 * `document.fonts.check` 要帶字級，而且家族名有空白時要加引號。
 */
export function fontReady(family, size = '14px') {
  try {
    return document.fonts.check(`${size} "${String(family).replace(/"/g, '')}"`);
  } catch {
    return false;
  }
}

/** 已經載進來的家族（給設定視窗標「內建」用）。 */
export async function builtinFamilies() {
  try {
    const faces = await invoke('font_faces');
    return [...new Set(faces.filter((f) => f.builtin).map((f) => f.family))];
  } catch {
    return [];
  }
}
