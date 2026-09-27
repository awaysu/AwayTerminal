// ADB 分頁的裝置流程（搬移舊版 `MainWindow.OpenAdbFlow`）。
//
// 舊版 v1.0.18 起 **ADB 不是內建選單項目**，而是一般的「自訂連線」（`KnownTools` 裡的 `ADB`，
// 參數固定 `shell`）。但**開它的時候不直接跑 `adb shell`**：`OpenCustom` 看到執行檔叫 adb
// 就轉去 `OpenAdbFlow`，先問 `adb devices`——接了兩台以上直接跑會噴錯。
//
// 流程（逐項照舊版）：
//   找不到 adb  → 說明 ＋ 問要不要開官方下載頁（`adb.notInstalled`）
//   0 台        → 提示（`adb.noDevice`）
//   1 台        → **直接開**，分頁名稱 `ADB`
//   2 台以上    → 選單選序號，分頁名稱＝**序號**
//
// ⚠️ 和舊版的一個差異：舊版 `AdbDevices` 只留 `state == "device"` 的，所以插了
// 沒授權的手機時會顯示「沒有偵測到 adb 裝置」，讓人以為線沒插好。我們**把
// offline／unauthorized 也列出來**（灰掉、不能選、旁邊寫狀態），這樣看得出問題在哪。

import { invoke } from '@tauri-apps/api/core';

import { T, fmt } from './strings.js';
import { log } from './bridge.js';

let hooks = {};

export function initAdb(injected) {
  hooks = injected || {};
}

/**
 * 開 ADB 分頁。`adbPath`＝自訂連線裡填的路徑（優先用它，同舊版）。
 * @returns {Promise<boolean>} 有沒有真的開出分頁
 */
export async function openAdb(adbPath) {
  let info;
  try {
    info = await invoke('adb_devices', { adbPath: adbPath || null });
  } catch (e) {
    log(`[adb] ${T['msg.connectFail']}：${e}`);
    await hooks.showInfo?.(T['msg.connectFail'], String(e));
    return false;
  }

  // 找不到 adb → 說明 ＋ 問要不要開下載頁（同舊版 `PromptInstallAdb`）
  if (!info.adb) {
    const go = await hooks.askYesNo?.(T['adb.notInstalled']);
    if (go) await invoke('open_url', { url: info.downloadUrl }).catch(() => {});
    return false;
  }

  const usable = (info.devices || []).filter((d) => d.state === 'device');
  const others = (info.devices || []).filter((d) => d.state !== 'device');

  if (usable.length === 0) {
    // 沒有能用的：如果有 offline／unauthorized 的，把狀態一起講出來（比舊版多的資訊）
    const extra = others.length
      ? '\n\n' + others.map((d) => `${d.serial}　${d.state}`).join('\n')
      : '';
    await hooks.showInfo?.(T['adb.pickDevice'], T['adb.noDevice'] + extra);
    return false;
  }

  if (usable.length === 1) {
    // 一台 → 直接開（分頁名稱 ADB，同舊版 `NextName("ADB")`）
    return await open(info.adb, null);
  }

  // 兩台以上 → 選裝置
  const serial = await hooks.pickFromList?.(
    T['adb.pickDevice'],
    usable.map((d) => ({ value: d.serial, label: d.serial })),
    others.map((d) => ({ value: '', label: `${d.serial}　${d.state}`, disabled: true })),
  );
  if (!serial) return false;
  return await open(info.adb, serial);
}

async function open(adb, serial) {
  try {
    await hooks.createSession({ kind: 'adb', adb: { path: adb, serial: serial || null } });
    log(`[adb] 開了 ADB 分頁：${serial ? fmt('adb.opened', serial) : 'ADB'}`);
    return true;
  } catch (e) {
    log(`[adb] ${T['msg.connectFail']}：${e}`);
    await hooks.showInfo?.(T['msg.connectFail'], String(e));
    return false;
  }
}

/** 這條自訂連線指向 adb 嗎（同舊版 `IsAdbExe`：看檔名）。 */
export function isAdbConn(conn) {
  if (!conn || conn.viaPowerShell) return false; // 舊版：走 PowerShell 的不轉（`!conn.ViaPowerShell &&`）
  const base = String(conn.path || '')
    .split(/[\\/]/)
    .pop()
    .toLowerCase();
  return base === 'adb' || base === 'adb.exe';
}
