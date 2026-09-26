// 介面文字（繁體中文）與分頁圖示。
//
// 中英切換是之後的任務，所以現在只有一份 zh。文字**集中在這裡**，
// 之後加 en 只要把每個值改成 { zh, en } 再由 settings 的 language 挑一個。
// 來源＝舊版 `Localization/Loc.cs` 的同名 key，key 名稱刻意保留，之後對照得起來。

export const T = {
  // 工具列（舊版 tb.*）
  'tb.new': '新分頁',
  'tb.powershell': 'PowerShell',
  'tb.customCmd': '自訂指令…',
  'tb.split': '視窗分割',
  'tb.tabs': '視窗分頁',
  'tb.columns': '視窗分欄',
  // 舊版三個 tip 的文字一樣，照抄
  'tip.viewCycle': '點按循環：分頁 → 分割 → 分欄',
  'tip.tabPanel': '顯示／隱藏分頁列表',
  'tip.tabClose': '關閉',

  // 分頁右鍵選單（舊版 menu.*）
  'menu.rename': '更改名稱',
  'menu.close': '關閉',

  // 分頁 tooltip（舊版 tip.tabElapsed）
  'tip.tabElapsed': '執行',

  // 對話框（舊版 dlg.* / msg.*）
  'dlg.renameTitle': '更改名稱',
  'dlg.renamePrompt': '分頁名稱：',
  'dlg.customTitle': '自訂指令',
  'dlg.customPrompt': '要執行的指令（例：claude、codex、wsl）：',
  'msg.closeTabTitle': '關閉分頁',
  // {0} = 分頁名稱
  'msg.closeTabConfirm': '確定要關閉「{0}」？',
  'dlg.ok': '確定',
  'dlg.cancel': '取消',
  'dlg.yes': '是',
  'dlg.no': '否',

  // 連線種類（舊版 kind.*；後端也會送 kindLabel，這份是前端的後備）
  'kind.powershell': 'PowerShell',
  'kind.claude': 'Claude Code',
  'kind.custom': '自訂連線',

  'msg.connectFail': '連線失敗',
  'msg.noTabs': '沒有分頁。按「新分頁」開一個。',

  // 工具列：編輯群組（舊版 tb.* / tip.*）
  'tb.copy': '複製',
  'tb.paste': '純文字貼上',
  'tb.copyall': '複製全部',
  'tb.clear': '清除畫面',
  'tb.page': '翻頁',
  'tip.copy': '複製選取的文字',
  'tip.paste': '把剪貼簿內容以純文字貼進終端機',
  'tip.copyall': '複製全部緩衝文字',
  'tip.clear': '清除畫面',
  'tip.page': '捲動畫面（上/下一頁、最上/最下面）',

  // 翻頁下拉（舊版 page.*）
  'page.up': '上一頁',
  'page.down': '下一頁',
  'page.top': '移到最上面',
  'page.bottom': '移到最下面',

  // 終端機右鍵選單（舊版 ctx.*）
  'ctx.copy': '複製',
  'ctx.copyPaste': '複製且貼上',
  'ctx.copyAllFile': '複製全部存至檔案',
  'ctx.search': '搜尋',
  'ctx.openUrl': '從瀏覽器開啟',
  'ctx.copyUrl': '複製網址',

  // 分頁右鍵：配色與 log（舊版 menu.*）
  'menu.color': '配色',
  'menu.colorDefault': '預設（設定顏色）',
  'menu.colorSample': 'Aa 範例文字',
  'menu.log': '記錄 log…',

  // toast（舊版 toast.*，等效 ShowCopyFeedback）
  'toast.copied': '複製成功',
  'toast.copiedPasted': '已複製並貼上',
  'toast.copiedAll': '已複製全部文字',
  'toast.noSelection': '沒有選取文字',
  'toast.noSelectionMouse': '沒有選取文字（此程式接管了滑鼠：按住 Shift 再拖曳選取）',
  'toast.urlCopied': '已複製網址',
  'toast.saved': '已存檔',

  // 清除畫面（舊版 msg.clear*）
  'msg.clearTitle': '清除畫面',
  'msg.clearConfirm': '確定要清除「{0}」的畫面嗎？',

  // log（舊版 dlg.logTitle / log.* / msg.*）
  'dlg.logTitle': '記錄 log',
  'log.path': 'log 存檔位置：',
  'log.browse': '瀏覽…',
  'log.timestamp': '每行前面加時間戳 [yy-MM-dd HH:mm:ss]',
  'log.append': '檔案已存在時附加（append）',
  'log.start': '開始記錄',
  'log.needPath': '請輸入 log 存檔位置。',
  'msg.stopLogAsk': '要停止記錄 log 嗎？',
  'msg.logFail': '無法開始記錄：',
  'msg.saveFail': '存檔失敗',
  'tip.tabLogging': '● 記錄 log 中',

  // 選工作目錄（舊版 dlg.pickDir*）
  'dlg.pickDirPs': '選擇 PowerShell 工作目錄（可在此按「建立新資料夾」）',
  'dlg.pickDirCustom': '選擇工作目錄（可在此按「建立新資料夾」）',

  // 全選（舊版有這個字串但沒有呼叫端，TASK-006 當新功能補上）
  'ctx.selectAll': '全選',

  // ---- SSH（TASK-006）----
  // 舊版的字面：`tb.ssh` = SSH/Telnet、`tip.ssh` = 開 SSH / Telnet（一個入口、對話框裡選類型）
  'tb.ssh': 'SSH/Telnet',
  'dlg.sshTitle': '開 SSH',
  'dlg.telnetTitle': '開 Telnet',
  'dlg.telnetPrompt': '主機（可加 :埠，預設 23）：',
  // 舊版 ConnectDialog 是「類型／IP 主機／Port／保持連線／斷線自動重連」五個欄位；
  // 完整對話框是 TASK-007，這裡先用一行 host[:port]
  'dlg.sshPrompt': '主機（可加 :埠，預設 22）：',

  // 主機金鑰確認。語意照 PuTTY，文字翻成繁中。
  'hk.titleUnknown': '主機金鑰尚未記錄',
  'hk.titleChanged': '⚠ 警告：主機金鑰不符！',
  'hk.bodyUnknown':
    '這台伺服器的主機金鑰沒有記錄在本程式的快取裡，無法確定它就是你想連的那一台。\n' +
    '請用其他可信的方式核對下面的指紋。',
  'hk.bodyChanged':
    '可能有安全問題！這台伺服器的主機金鑰與本程式記錄的不同。\n' +
    '這代表：伺服器的管理者換過金鑰，或者這條連線被冒充（中間人攻擊）。\n' +
    '如果不確定，請按「取消」，並先向伺服器管理者確認新的指紋。',
  'hk.noteUnknown': '按「接受並儲存」會把這把金鑰記下來，以後連同一台就不再詢問。',
  // {0} = known_hosts 路徑、{1} = 行號
  'hk.noteChanged': '舊記錄在 {0} 第 {1} 行。要改用新金鑰請先刪掉那一行，或按「接受並儲存」覆蓋。',
  'hk.store': '接受並儲存',
  'hk.once': '只這次',
  'hk.cancel': '取消',
  'hk.host': '主機',
  'hk.alg': '演算法',

  // 弱演算法警告（PuTTY 的 warn-below-this-line；TASK-008 B4）
  'wa.title': '這條連線使用較舊的加密演算法',
  // {0} = host[:port]
  'wa.body':
    '{0} 只支援（或優先選用）下面這些演算法。它們仍然可以用，但已經被認為較弱——\n' +
    '很舊的網路設備通常只有這些，一般的伺服器不該用到。',
  'wa.note': '按「繼續連線」之後，這台主機就不會再問（記在設定裡）。',
  'wa.go': '繼續連線',
  'wa.cancel': '取消',

  // ---- 連線對話框（TASK-009 B6；TASK-010 加 Telnet 類型）----
  // 對話框標題逐字照舊版 `conn.title`
  'sd.title': '開 SSH / Telnet',
  'sd.type': '類型',
  'sd.host': 'IP / 主機',
  'sd.port': 'Port',
  'sd.user': '帳號',
  'sd.userHint': '留空＝連上後在終端機問 login as:',
  'sd.key': '金鑰檔',
  'sd.keep': '保持連線',
  'sd.keepHint': '分鐘，0＝關閉',
  'sd.agent': '也試 Pageant／ssh-agent 裡的金鑰',
  'sd.reconnect': '斷線自動重連',
  'sd.adv': '進階：演算法與環境變數',
  'sd.advNote': '都不選＝用預設順序（先強後弱）。⚠ 是警告線以下的舊演算法，協商到時會先問你。',
  'sd.kex': '金鑰交換',
  'sd.hostKey': '主機金鑰',
  'sd.cipher': '加密',
  'sd.mac': '訊息驗證',
  'sd.env': '送出環境變數（一行一個 KEY=VALUE）',
  'sd.envHint': '伺服器多半設了 AcceptEnv 白名單，沒放行只會印一行灰字、不影響連線。',
  'sd.connect': '連線',
  'sd.addFav': '加到我的最愛',
  'sd.needHost': '請輸入主機。',

  // ---- 連接埠（TASK-011；舊版 Dialogs/ComDialog + Loc 的 com.*）----
  'tb.com': '連接埠',
  'tip.com': '開 COM 埠',
  'kind.com': '連接埠 (COM)',
  'com.title': '開連接埠',
  'com.open': '開啟',
  'common.reset': '回到預設',
  // 欄位名稱照舊版 XAML 的英文（使用者看到的就是這些字）
  'cd.port': 'Port',
  'cd.baud': 'Baud rate',
  'cd.data': 'Data bits',
  'cd.parity': 'Parity',
  'cd.stop': 'Stop bits',
  'cd.flow': 'Flow control',
  'cd.rescan': '重新掃描',
  'cd.needPort': '請選擇或輸入連接埠。',
  'cd.noPorts': '目前偵測不到任何連接埠（USB 轉序列線插上後按「重新掃描」）。',

  // ---- TTL 巨集（TASK-013；舊版 Loc 的 menu.macro／msg.stopMacroAsk／dlg.macroTitle）----
  'menu.macro': '執行巨集…',
  'macro.title': '執行巨集',
  'macro.stopAsk': '要停止巨集嗎？',
  'macro.readFail': '無法讀取巨集：',
  'macro.pick': '選擇 TTL 巨集',
  'macro.errorTitle': '巨集錯誤',
  // {0}=訊息 {1}=檔名 {2}=行號 {3}=那一行的內容
  'macro.errorBody': '{0}\n\n{1} 第 {2} 行：\n{3}',
  // 分頁 tooltip 多一行（新增；舊版只有「巨集執行中」）
  'tip.tabMacro': '● 巨集執行中：{0}（第 {1} 行）',

  // ---- 離開程式與恢復分頁（TASK-010；舊版 Dialogs/ExitDialog + 1.0.45）----
  'exit.title': '離開 AwayTerminal',
  'exit.body': '要關閉程式嗎？',
  // 文字逐字照舊版 Loc 的 exit.restore
  'exit.restore': '下次開啟恢復目前分頁（含畫面上的舊訊息）',
  'exit.go': '離開',
  // {0} = 分頁數
  'restore.done': '已恢復 {0} 個分頁',
  'restore.failed': '有 {0} 個分頁恢復失敗（詳情見後端 log）',
  'sd.quick': '快速連線（host[:port]）…',

  // ---- 我的最愛（TASK-009 C；舊版 fav.* / tb.favorites / tip.favorites）----
  'tb.favorites': '我的最愛',
  'tip.favorites': '我的最愛：點選開啟，或把目前分頁加進來',
  'fav.empty': '（還沒有我的最愛）',
  'fav.add': '加到我的最愛',
  'fav.addNamed': '加到我的最愛：{0}',
  'fav.settings': '設定…',
  'fav.added': '已加入我的最愛：{0}',
  'fav.deleteAsk': '確定要從我的最愛刪除「{0}」？',
  'fav.nameTitle': '我的最愛',
  'fav.namePrompt': '名稱：',
  'fav.up': '上移',
  'fav.down': '下移',

  // ---- 自訂連線（TASK-007，舊版 custom.*）----
  'conn.title': '自訂連線',
  'conn.detect': '自動偵測',
  'conn.new': '新增',
  'conn.save': '儲存',
  'conn.delete': '刪除',
  'conn.empty': '清單是空的。按「自動偵測」找出這台機器上裝了哪些工具。',
  'conn.newHint': '新增一條連線：填名稱與執行檔路徑後按「儲存」。',
  'conn.saved': '已儲存。',
  'conn.deleted': '已刪除。',
  'conn.detectNone': '沒有找到新的工具（可能都已經在清單裡，或都沒安裝）。',
  'conn.detectDone': '已加入：',
  'conn.sandboxOn': '沙盒',
  'conn.sandboxOff': '無沙盒',
  'conn.hiddenTag': '隱藏',
  'dlg.close': '關閉',
  'tb.manageConns': '自訂連線設定…',

  // ---- 沙盒模式（新功能）----
  'sb.menu': '沙盒模式',
  'sb.clear': '清除沙盒…',
  'sb.tipOn': '沙盒：{0}',
  'sb.tipBranch': '沙盒分支：{0}',
  'sb.tipNoWorktree': '沙盒（無 worktree，不是 git repo）',
  'sb.tipOff': '沙盒：關閉',
  'sb.changedTitle': '沙盒模式',
  // {0} = 連線名稱、{1} = 開啟/關閉
  'sb.changedBody': '「{0}」的沙盒模式已{1}。\n這個改變要等**下次啟動這個分頁**才生效。\n要現在就重新啟動這個分頁嗎？（會關掉目前的連線）',
  'sb.on': '開啟',
  'sb.off': '關閉',
  'sb.restartNow': '重新啟動分頁',
  'sb.later': '稍後',
  'sb.clearTitle': '清除沙盒',
  // {0} = worktree 路徑、{1} = 分支
  'sb.clearBody': '要移除這個沙盒的 worktree 嗎？\n\n{0}\n\n分支 {1} 會**保留**——裡面若有還沒合併的成果，之後仍然可以用 git merge 取回（做法見 docs/AGENT-SANDBOX.md）。',
  'sb.cleared': '沙盒已移除（分支保留）。',
  'sb.noSandbox': '這個分頁沒有沙盒。',
};

/** `msg.closeTabConfirm` 這類帶 {0} 的字串。 */
export function fmt(key, ...args) {
  return String(T[key] ?? key).replace(/\{(\d+)\}/g, (m, i) => (args[i] ?? m));
}

// 分頁列圖示。
//
// 舊版用 `icon/*.png` 的灰階圖，以 `IconTint` 逐像素染成綠／紅。新版改成 inline SVG、
// 用 `currentColor` 染色——效果一樣（同一組顏色），但不必把二進位圖檔搬進這個 repo，
// 也不用在瀏覽器裡做 canvas 逐像素處理。種類對得起來就好。
const ICONS = {
  powershell:
    '<path d="M3 4h18a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zm0 2v12h18V6H3zm3.7 2.3 4 3a.9.9 0 0 1 0 1.4l-4 3-1.1-1.4L8.7 12 5.6 9.7l1.1-1.4zM12.5 14H18v1.8h-5.5V14z"/>',
  claude:
    '<path d="M12 2.2 14.4 9l6.8 2.4-6.8 2.4L12 20.6 9.6 13.8 2.8 11.4 9.6 9 12 2.2z"/>',
  custom:
    '<path d="M8 5v14l11-7L8 5zm-4 0h2v14H4V5z"/>',
  ssh: '<path d="M4 6h16v12H4V6zm2 2v8h12V8H6zm1.5 1.5 3 2.5-3 2.5V9.5zM12 14h5v1.5h-5V14z"/>',
  telnet: '<path d="M4 6h16v12H4V6zm2 2v8h12V8H6zm1.5 1.5 3 2.5-3 2.5V9.5zM12 14h5v1.5h-5V14z"/>',
  com: '<path d="M5 8h14a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-6a2 2 0 0 1 2-2zm1 3v4h2v-4H6zm4 0v4h2v-4h-2zm4 0v4h2v-4h-2z"/>',
  adb: '<path d="M7 9h10v7a3 3 0 0 1-3 3h-4a3 3 0 0 1-3-3V9zm1.6-4.6 1.2 2M15.4 4.4l-1.2 2M4 11h2v5H4zm14 0h2v5h-2z"/>',
};

/** 連線種類 → inline SVG（`fill: currentColor`，顏色由 CSS 的狀態 class 決定）。 */
export function iconSvg(kind) {
  const path = ICONS[kind] || ICONS.custom;
  return `<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">${path}</svg>`;
}

/**
 * 執行時長：格式「日:時:分」（日不補零、時分兩位）。
 * 逐字照抄舊版 `TerminalTab.ElapsedText`——未來時間／時鐘倒退視為 0。
 */
export function elapsedText(startedAtMs) {
  let mins = Math.floor((Date.now() - startedAtMs) / 60000);
  if (!(mins >= 0)) mins = 0;
  const pad = (n) => String(n).padStart(2, '0');
  return `${Math.floor(mins / 1440)}:${pad(Math.floor(mins / 60) % 24)}:${pad(mins % 60)}`;
}
