// 介面文字（中／英）與分頁圖示。
//
// 每一筆是 `'key': [繁中, English]`。呼叫端用 `T['key']`，語言由 `setLang()` 決定
//（見下面的 Proxy）。key 名稱刻意和舊版 `Localization/Loc.cs` 一樣，對照得起來。
// Rust 端有自己的一份表（`src-tauri/src/i18n.rs`），理由寫在那個檔的最上面。

const STR = {
  // 工具列（舊版 tb.*）
  'tb.new': ['新分頁', 'New tab'],
  'tb.powershell': ['PowerShell', 'PowerShell'],
  'tb.customCmd': ['自訂指令…', 'Custom command…'],
  'tb.split': ['視窗分割', 'Split'],
  'tb.tabs': ['視窗分頁', 'Tabs'],
  'tb.columns': ['視窗分欄', 'Columns'],
  // 舊版三個 tip 的文字一樣，照抄
  'tip.viewCycle': ['點按循環：分頁 → 分割 → 分欄', 'Click to cycle: tabs → split → columns'],
  'tip.tabPanel': ['顯示／隱藏分頁列表', 'Show / hide the tab list'],
  'tip.tabClose': ['關閉', 'Close'],

  // 分頁右鍵選單（舊版 menu.*）
  'menu.rename': ['更改名稱', 'Rename'],
  'menu.close': ['關閉', 'Close'],

  // 分頁 tooltip（舊版 tip.tabElapsed）
  'tip.tabElapsed': ['執行', 'running'],

  // 對話框（舊版 dlg.* / msg.*）
  'dlg.renameTitle': ['更改名稱', 'Rename'],
  'dlg.renamePrompt': ['分頁名稱：', 'Tab name:'],
  'dlg.customTitle': ['自訂指令', 'Custom command'],
  'dlg.customPrompt': ['要執行的指令（例：claude、codex、wsl）：', 'Command to run (e.g. claude, codex, wsl):'],
  'msg.closeTabTitle': ['關閉分頁', 'Close Tab'],
  // {0} = 分頁名稱
  'msg.closeTabConfirm': ['確定要關閉「{0}」？', 'Close "{0}"?'],
  'dlg.ok': ['確定', 'OK'],
  'dlg.cancel': ['取消', 'Cancel'],
  'dlg.yes': ['是', 'Yes'],
  'dlg.no': ['否', 'No'],

  // 連線種類（舊版 kind.*；後端也會送 kindLabel，這份是前端的後備）
  'kind.powershell': ['PowerShell', 'PowerShell'],
  'kind.claude': ['Claude Code', 'Claude Code'],
  'kind.custom': ['自訂連線', 'Custom connection'],

  'msg.connectFail': ['連線失敗', 'Connection failed'],
  'msg.noTabs': ['沒有分頁。按「新分頁」開一個。', 'No tabs. Click "New tab" to open one.'],

  // 工具列：編輯群組（舊版 tb.* / tip.*）
  'tb.copy': ['複製', 'Copy'],
  'tb.paste': ['純文字貼上', 'Paste as text'],
  'tb.copyall': ['複製全部', 'Copy All'],
  'tb.clear': ['清除畫面', 'Clear'],
  'tb.page': ['翻頁', 'Scroll'],
  'tip.copy': ['複製選取的文字', 'Copy selection'],
  'tip.paste': ['把剪貼簿內容以純文字貼進終端機', 'Paste clipboard as plain text'],
  'tip.copyall': ['複製全部緩衝文字', 'Copy all buffer text'],
  'tip.clear': ['清除畫面', 'Clear screen'],
  'tip.page': ['捲動畫面（上/下一頁、最上/最下面）', 'Scroll the view (page up/down, top/bottom)'],

  // 翻頁下拉（舊版 page.*）
  'page.up': ['上一頁', 'Page up'],
  'page.down': ['下一頁', 'Page down'],
  'page.top': ['移到最上面', 'Go to top'],
  'page.bottom': ['移到最下面', 'Go to bottom'],

  // 終端機右鍵選單（舊版 ctx.*）
  'ctx.copy': ['複製', 'Copy'],
  'ctx.copyPaste': ['複製且貼上', 'Copy and paste'],
  'ctx.copyAllFile': ['複製全部存至檔案', 'Copy all to file'],
  'ctx.search': ['搜尋', 'Find'],
  'ctx.openUrl': ['從瀏覽器開啟', 'Open in browser'],
  'ctx.copyUrl': ['複製網址', 'Copy URL'],

  // 分頁右鍵：配色與 log（舊版 menu.*）
  'menu.color': ['配色', 'Colors'],
  'menu.colorDefault': ['預設（設定顏色）', 'Default (settings colors)'],
  'menu.colorSample': ['Aa 範例文字', 'Aa sample text'],
  'menu.log': ['記錄 log…', 'Record log…'],

  // toast（舊版 toast.*，等效 ShowCopyFeedback）
  'toast.copied': ['複製成功', 'Copied'],
  'toast.copiedPasted': ['已複製並貼上', 'Copied and pasted'],
  'toast.copiedAll': ['已複製全部文字', 'All text copied'],
  'toast.noSelection': ['沒有選取文字', 'Nothing selected'],
  'toast.noSelectionMouse': ['沒有選取文字（此程式接管了滑鼠：按住 Shift 再拖曳選取）', 'Nothing selected (this program captures the mouse: hold Shift while dragging to select)'],
  'toast.urlCopied': ['已複製網址', 'URL copied'],
  'toast.saved': ['已存檔', 'Saved'],

  // 清除畫面（舊版 msg.clear*）
  'msg.clearTitle': ['清除畫面', 'Clear Screen'],
  'msg.clearConfirm': ['確定要清除「{0}」的畫面嗎？', 'Clear the screen of "{0}"?'],

  // log（舊版 dlg.logTitle / log.* / msg.*）
  'dlg.logTitle': ['記錄 log', 'Record Log'],
  'log.path': ['log 存檔位置：', 'Log file path:'],
  'log.browse': ['瀏覽…', 'Browse…'],
  'log.timestamp': ['每行前面加時間戳 [yy-MM-dd HH:mm:ss]', 'Prefix each line with [yy-MM-dd HH:mm:ss]'],
  'log.append': ['檔案已存在時附加（append）', 'Append if the file exists'],
  'log.start': ['開始記錄', 'Start logging'],
  'log.needPath': ['請輸入 log 存檔位置。', 'Please enter the log file path.'],
  'msg.stopLogAsk': ['要停止記錄 log 嗎？', 'Stop logging?'],
  'msg.logFail': ['無法開始記錄：', 'Cannot start logging:'],
  'msg.saveFail': ['存檔失敗', 'Save failed'],
  'tip.tabLogging': ['● 記錄 log 中', '● Recording log'],

  // 選工作目錄（舊版 dlg.pickDir*）
  'dlg.pickDirPs': ['選擇 PowerShell 工作目錄（可在此按「建立新資料夾」）', 'Choose the PowerShell working folder'],
  'dlg.pickDirCustom': ['選擇工作目錄（可在此按「建立新資料夾」）', 'Choose the working folder (you can create a new folder here)'],

  // 全選（舊版有這個字串但沒有呼叫端，TASK-006 當新功能補上）
  'ctx.selectAll': ['全選', 'Select all'],

  // ---- SSH（TASK-006）----
  // 舊版的字面：`tb.ssh` = SSH/Telnet、`tip.ssh` = 開 SSH / Telnet（一個入口、對話框裡選類型）
  'tb.ssh': ['SSH/Telnet', 'SSH/Telnet'],
  'dlg.sshTitle': ['開 SSH', 'Open SSH'],
  'dlg.telnetTitle': ['開 Telnet', 'Open Telnet'],
  'dlg.telnetPrompt': ['主機（可加 :埠，預設 23）：', 'Host (add :port; default 23):'],
  // 舊版 ConnectDialog 是「類型／IP 主機／Port／保持連線／斷線自動重連」五個欄位；
  // 完整對話框是 TASK-007，這裡先用一行 host[:port]
  'dlg.sshPrompt': ['主機（可加 :埠，預設 22）：', 'Host (add :port; default 22):'],

  // 主機金鑰確認。語意照 PuTTY，文字翻成繁中。
  'hk.titleUnknown': ['主機金鑰尚未記錄', 'The host key is not recorded yet'],
  'hk.titleChanged': ['⚠ 警告：主機金鑰不符！', '⚠ Warning: the host key does not match!'],
  'hk.bodyUnknown': [
    '這台伺服器的主機金鑰沒有記錄在本程式的快取裡，無法確定它就是你想連的那一台。\n' +
      '請用其他可信的方式核對下面的指紋。',
    "This host's key is not in this program's cache, so there is no way to be sure it is the machine " +
      'you meant to reach.\nPlease verify the fingerprint below by some other trustworthy means.',
  ],
  'hk.bodyChanged': [
    '可能有安全問題！這台伺服器的主機金鑰與本程式記錄的不同。\n' +
      '這代表：伺服器的管理者換過金鑰，或者這條連線被冒充（中間人攻擊）。\n' +
      '如果不確定，請按「取消」，並先向伺服器管理者確認新的指紋。',
    'This may be a security problem! This host’s key differs from the one this program recorded.\n' +
      'That means either the administrator changed the key, or this connection is being impersonated ' +
      '(a man-in-the-middle).\nIf you are not sure, press "Cancel" and confirm the new fingerprint ' +
      'with the server administrator first.',
  ],
  'hk.noteUnknown': ['按「接受並儲存」會把這把金鑰記下來，以後連同一台就不再詢問。', '"Accept and store" remembers this key, so the same host will not be asked about again.'],
  // {0} = known_hosts 路徑、{1} = 行號
  'hk.noteChanged': ['舊記錄在 {0} 第 {1} 行。要改用新金鑰請先刪掉那一行，或按「接受並儲存」覆蓋。', 'The old record is on line {1} of {0}. To switch to the new key, delete that line first, or press "Accept and store" to overwrite it.'],
  'hk.store': ['接受並儲存', 'Accept and store'],
  'hk.once': ['只這次', 'Just this once'],
  'hk.cancel': ['取消', 'Cancel'],
  'hk.host': ['主機', 'Host'],
  'hk.alg': ['演算法', 'Algorithm'],

  // 弱演算法警告（PuTTY 的 warn-below-this-line；TASK-008 B4）
  'wa.title': ['這條連線使用較舊的加密演算法', 'This connection uses older cryptographic algorithms'],
  // {0} = host[:port]
  'wa.body': [
    '{0} 只支援（或優先選用）下面這些演算法。它們仍然可以用，但已經被認為較弱——\n' +
      '很舊的網路設備通常只有這些，一般的伺服器不該用到。',
    '{0} only supports (or preferred) the algorithms below. They still work, but they are considered ' +
      'weak—very old network devices often have nothing else, while an ordinary server should not ' +
      'be using them.',
  ],
  'wa.note': ['按「繼續連線」之後，這台主機就不會再問（記在設定裡）。', 'After "Continue", this host will not be asked about again (it is remembered in the settings).'],
  'wa.go': ['繼續連線', 'Continue'],
  'wa.cancel': ['取消', 'Cancel'],

  // ---- 連線對話框（TASK-009 B6；TASK-010 加 Telnet 類型）----
  // 對話框標題逐字照舊版 `conn.title`
  'sd.title': ['開 SSH / Telnet', 'Open SSH / Telnet'],
  'sd.type': ['類型', 'Type'],
  'sd.host': ['IP / 主機', 'IP / host'],
  'sd.port': ['Port', 'Port'],
  'sd.user': ['帳號', 'User'],
  'sd.userHint': ['留空＝連上後在終端機問 login as:', 'Leave empty = ask login as: in the terminal after connecting'],
  'sd.key': ['金鑰檔', 'Key file'],
  'sd.keep': ['保持連線', 'Keep-alive'],
  'sd.keepHint': ['分鐘，0＝關閉', 'minutes, 0 = off'],
  'sd.agent': ['也試 Pageant／ssh-agent 裡的金鑰', 'Also try keys from Pageant / ssh-agent'],
  'sd.reconnect': ['斷線自動重連', 'Auto-reconnect on disconnect'],
  'sd.adv': ['進階：演算法與環境變數', 'Advanced: algorithms and environment variables'],
  'sd.advNote': ['都不選＝用預設順序（先強後弱）。⚠ 是警告線以下的舊演算法，協商到時會先問你。', 'Select none = use the default order (strongest first). ⚠ marks algorithms below the warning line; you will be asked before one is used.'],
  'sd.kex': ['金鑰交換', 'Key exchange'],
  'sd.hostKey': ['主機金鑰', 'Host key'],
  'sd.cipher': ['加密', 'Cipher'],
  'sd.mac': ['訊息驗證', 'MAC'],
  'sd.env': ['送出環境變數（一行一個 KEY=VALUE）', 'Send environment variables (one KEY=VALUE per line)'],
  'sd.envHint': ['伺服器多半設了 AcceptEnv 白名單，沒放行只會印一行灰字、不影響連線。', 'Most servers have an AcceptEnv allow-list; anything not allowed only prints a grey line and does not affect the connection.'],
  'sd.connect': ['連線', 'Connect'],
  'sd.addFav': ['加到我的最愛', 'Add to favorites'],
  'sd.needHost': ['請輸入主機。', 'Please enter a host.'],

  // ---- 連接埠（TASK-011；舊版 Dialogs/ComDialog + Loc 的 com.*）----
  'tb.com': ['連接埠', 'COM'],
  'tip.com': ['開 COM 埠', 'Open COM port'],
  'kind.com': ['連接埠 (COM)', 'Serial port (COM)'],
  'com.title': ['開連接埠', 'Open COM Port'],
  'com.open': ['開啟', 'Open'],
  'common.reset': ['回到預設', 'Reset to default'],
  // 欄位名稱照舊版 XAML 的英文（使用者看到的就是這些字）
  'cd.port': ['Port', 'Port'],
  'cd.baud': ['Baud rate', 'Baud rate'],
  'cd.data': ['Data bits', 'Data bits'],
  'cd.parity': ['Parity', 'Parity'],
  'cd.stop': ['Stop bits', 'Stop bits'],
  'cd.flow': ['Flow control', 'Flow control'],
  'cd.rescan': ['重新掃描', 'Rescan'],
  'cd.needPort': ['請選擇或輸入連接埠。', 'Please choose or type a port.'],
  'cd.noPorts': ['目前偵測不到任何連接埠（USB 轉序列線插上後按「重新掃描」）。', 'No serial port detected (plug in a USB-to-serial adapter, then press "Rescan").'],

  // ---- TTL 巨集（TASK-013；舊版 Loc 的 menu.macro／msg.stopMacroAsk／dlg.macroTitle）----
  'menu.macro': ['執行巨集…', 'Run macro…'],
  'macro.title': ['執行巨集', 'Run macro'],
  'macro.stopAsk': ['要停止巨集嗎？', 'Stop the macro?'],
  'macro.readFail': ['無法讀取巨集：', 'Could not read the macro:'],
  'macro.pick': ['選擇 TTL 巨集', 'Choose a TTL macro'],
  'macro.errorTitle': ['巨集錯誤', 'Macro error'],
  // {0}=訊息 {1}=檔名 {2}=行號 {3}=那一行的內容
  'macro.errorBody': ['{0}\n\n{1} 第 {2} 行：\n{3}', '{0}\n\n{1} line {2}:\n{3}'],
  // 分頁 tooltip 多一行（新增；舊版只有「巨集執行中」）
  'tip.tabMacro': ['● 巨集執行中：{0}（第 {1} 行）', '● Macro running: {0} (line {1})'],

  // ---- 輸入文字（TASK-014；舊版 Loc 的 compose.*，文字逐字照抄）----
  'tb.compose': ['輸入文字', 'Compose'],
  'tip.compose': ['先打好文字再送到目前分頁（中文用輸入法打在這裡，不會被逐鍵送出）', 'Compose the text first, then send it to the current tab (type Chinese with your IME here; it is not sent key by key)'],
  'compose.title': ['輸入文字', 'Compose'],
  'compose.placeholder': ['在此輸入要送出的文字（可多行，Ctrl+Enter 送出）', 'Type the text to send (multi-line OK; Ctrl+Enter sends)'],
  'compose.send': ['送出', 'Send'],
  'compose.back': ['返回', 'Back'],
  'compose.clear': ['清除', 'Clear'],
  'compose.save': ['儲存', 'Save'],
  'compose.undo': ['復原', 'Undo'],
  'compose.sendEnter': ['送出後送 Enter', 'Send Enter after submit'],
  'compose.noTab': ['沒有分頁可送', 'No tab to send to'],
  'compose.loadFile': ['載入文字檔', 'Load file'],
  // {0} = 用哪種編碼解出來的（新增：舊版沒有告訴使用者）
  'compose.loaded': ['已載入（{0}）', 'Loaded ({0})'],
  // {0} = 存到哪裡
  'compose.saved': ['已儲存：{0}', 'Saved: {0}'],

  // ---- 離開程式與恢復分頁（TASK-010；舊版 Dialogs/ExitDialog + 1.0.45）----
  'exit.title': ['離開 AwayTerminal', 'Quit AwayTerminal'],
  'exit.body': ['要關閉程式嗎？', 'Quit the program?'],
  // 文字逐字照舊版 Loc 的 exit.restore
  'exit.restore': ['下次開啟恢復目前分頁（含畫面上的舊訊息）', 'Restore current tabs next time (with scrollback)'],
  'exit.go': ['離開', 'Quit'],
  // {0} = 分頁數
  'restore.done': ['已恢復 {0} 個分頁', 'Restored {0} tab(s)'],
  'restore.failed': ['有 {0} 個分頁恢復失敗（詳情見後端 log）', '{0} tab(s) could not be restored (see the backend log)'],
  'sd.quick': ['快速連線（host[:port]）…', 'Quick connect (host[:port])…'],

  // ---- 我的最愛（TASK-009 C；舊版 fav.* / tb.favorites / tip.favorites）----
  'tb.favorites': ['我的最愛', 'Favorites'],
  'tip.favorites': ['我的最愛：點選開啟，或把目前分頁加進來', 'Favorites: open one, or add the current tab'],
  'fav.empty': ['（還沒有我的最愛）', '(no favorites yet)'],
  'fav.add': ['加到我的最愛', 'Add to favorites'],
  'fav.addNamed': ['加到我的最愛：{0}', 'Add to favorites: {0}'],
  'fav.settings': ['設定…', 'Settings…'],
  'fav.added': ['已加入我的最愛：{0}', 'Added to favorites: {0}'],
  'fav.deleteAsk': ['確定要從我的最愛刪除「{0}」？', 'Remove "{0}" from favorites?'],
  'fav.nameTitle': ['我的最愛', 'Favorites'],
  'fav.namePrompt': ['名稱：', 'Name:'],
  'fav.up': ['上移', 'Move up'],
  'fav.down': ['下移', 'Move down'],

  // ---- 自訂連線（TASK-007，舊版 custom.*）----
  'conn.title': ['自訂連線', 'Custom connections'],
  'conn.detect': ['自動偵測', 'Auto-detect'],
  'conn.new': ['新增', 'New'],
  'conn.save': ['儲存', 'Save'],
  'conn.delete': ['刪除', 'Delete'],
  'conn.empty': ['清單是空的。按「自動偵測」找出這台機器上裝了哪些工具。', 'The list is empty. Press "Auto-detect" to find the tools installed on this machine.'],
  'conn.newHint': ['新增一條連線：填名稱與執行檔路徑後按「儲存」。', 'Add a connection: fill in the name and the path to the executable, then press "Save".'],
  'conn.saved': ['已儲存。', 'Saved.'],
  'conn.deleted': ['已刪除。', 'Deleted.'],
  'conn.detectNone': ['沒有找到新的工具（可能都已經在清單裡，或都沒安裝）。', 'No new tools found (they may already be in the list, or none are installed).'],
  'conn.detectDone': ['已加入：', 'Added:'],
  'conn.sandboxOn': ['沙盒', 'Sandbox'],
  'conn.sandboxOff': ['無沙盒', 'No sandbox'],
  'conn.hiddenTag': ['隱藏', 'Hidden'],
  'dlg.close': ['關閉', 'Close'],
  'tb.manageConns': ['自訂連線設定…', 'Custom connections…'],

  // ---- 沙盒模式（新功能）----
  'sb.menu': ['沙盒模式', 'Sandbox mode'],
  'sb.clear': ['清除沙盒…', 'Remove sandbox…'],
  'sb.tipOn': ['沙盒：{0}', 'Sandbox: {0}'],
  'sb.tipBranch': ['沙盒分支：{0}', 'Sandbox branch: {0}'],
  'sb.tipNoWorktree': ['沙盒（無 worktree，不是 git repo）', 'Sandbox (no worktree; not a git repository)'],
  'sb.tipOff': ['沙盒：關閉', 'Sandbox: off'],
  'sb.changedTitle': ['沙盒模式', 'Sandbox mode'],
  // {0} = 連線名稱、{1} = 開啟/關閉
  'sb.changedBody': ['「{0}」的沙盒模式已{1}。\n這個改變要等**下次啟動這個分頁**才生效。\n要現在就重新啟動這個分頁嗎？（會關掉目前的連線）', 'Sandbox mode for "{0}" is now {1}.\nThe change takes effect **the next time this tab starts**.\nRestart this tab now? (the current connection will be closed)'],
  'sb.on': ['開啟', 'on'],
  'sb.off': ['關閉', 'off'],
  'sb.restartNow': ['重新啟動分頁', 'Restart tab'],
  'sb.later': ['稍後', 'Later'],
  'sb.clearTitle': ['清除沙盒', 'Remove sandbox'],
  // {0} = worktree 路徑、{1} = 分支
  'sb.clearBody': ['要移除這個沙盒的 worktree 嗎？\n\n{0}\n\n分支 {1} 會**保留**——裡面若有還沒合併的成果，之後仍然可以用 git merge 取回（做法見 docs/AGENT-SANDBOX.md）。', 'Remove this sandbox\'s worktree?\n\n{0}\n\nThe branch {1} is **kept** - if it holds work that has not been merged, you can still get it back with git merge (see docs/AGENT-SANDBOX.md).'],
  'sb.cleared': ['沙盒已移除（分支保留）。', 'The sandbox was removed (the branch was kept).'],
  'sb.noSandbox': ['這個分頁沒有沙盒。', 'This tab has no sandbox.'],
  // ---- 設定視窗（TASK-015 A；舊版 Dialogs/SettingsDialog + Loc 的 settings.* / font.*）----
  'tb.settings': ['其他設定', 'Settings'],
  'tip.settings': ['字型、顏色、語言與其他設定', 'Font, colors, language and other settings'],
  'settings.title': ['設定', 'Settings'],
  'settings.groupLang': ['語言', 'Language'],
  'settings.groupFont': ['字體背景顏色', 'Font & colors'],
  'font.family': ['字型', 'Font'],
  'font.size': ['大小', 'Size'],
  'font.fg': ['文字顏色', 'Text color'],
  'font.bg': ['背景顏色', 'Background color'],
  'font.pick': ['點我選顏色', 'Click to pick a color'],
  'settings.groupIme': ['Claude 輸入送出', 'Claude input timing'],
  'settings.imeQuiet': ['送出前等待靜止 (ms)', 'Wait for quiet before send (ms)'],
  'settings.imeQuietHelpLink': ['這是什麼？', 'What is this?'],
  'settings.imeQuietHelpTitle': ['送出前等待靜止 (ms)', 'Wait for quiet before send (ms)'],
  // 逐字照舊版（很長，是使用者真的會讀的說明）
  'settings.imeQuietHelp': [
    '此設定只作用於 Claude Code 分頁。\n\n' +
      '打注音（整段送出）、貼上、或按 Backspace 時，若 Claude 正在重繪畫面（執行中、' +
      '建議文字在跳），直接送出偶爾會讓 Claude 把剛輸入的字重複顯示成兩份，或在全形/半形' +
      '混合時把游標位置算錯、少一格或留殘影。\n\n' +
      '開啟後，這幾類輸入會等 Claude 畫面靜止「這麼多毫秒」才送出，避開重繪空檔、降低上述問題。\n\n' +
      '• 只有 Claude 忙碌重繪時才會有這點延遲；停在提示列打字時 0 延遲。\n' +
      '• 一般英數打字、Enter、Ctrl 鍵不受影響。\n' +
      '• 數字越大越保守（較不易出錯，忙碌時延遲略增）；越小反應越快、保護越弱。\n' +
      '• 設 0 = 關閉此功能（立即送出）。\n\n' +
      '預設 20。這是降低問題頻率的緩解措施；根本原因在 Claude Code 端的畫面重繪。',
    'This setting only affects Claude Code tabs.\n\n' +
      'When you commit IME (Zhuyin) text, paste, or press Backspace while Claude is repainting ' +
      '(running, or the suggestion text is updating), sending immediately can occasionally make ' +
      'Claude echo the just-typed text twice, or miscompute the cursor column when full-width and ' +
      'half-width characters are mixed (a column short, or a leftover ghost).\n\n' +
      "When enabled, these inputs wait until Claude's output has been quiet for this many " +
      'milliseconds before being sent, avoiding the repaint window and reducing those problems.\n\n' +
      '• The delay only applies while Claude is busy repainting; typing at an idle prompt has 0 delay.\n' +
      '• Normal letters/digits, Enter and Ctrl keys are unaffected.\n' +
      '• Higher = more conservative (fewer glitches, slightly more delay when busy); lower = snappier, weaker protection.\n' +
      '• Set 0 to turn this off (send immediately).\n\n' +
      "Default is 20. This is a mitigation that lowers the frequency; the root cause is Claude Code's own screen repaint.",
  ],
  // 檔案總管右鍵選單：**還沒做**（Windows 登錄檔那一段是 TASK-016），所以灰掉並註明
  'settings.groupShell': ['檔案總管', 'File Explorer'],
  'settings.shellMenu': [
    '資料夾右鍵選單加入「用 AwayTerminal 開啟」（在該資料夾開 PowerShell 分頁）',
    'Add "Open in AwayTerminal" to the folder context menu (opens a PowerShell tab there)',
  ],
  'settings.todo': ['（這項還沒搬過來）', '(not ported yet)'],

  // ---- 新版多的設定（舊版只能手改 settings.json）----
  'settings.groupMore': ['其他', 'Other'],
  'settings.restoreLines': ['恢復分頁保留的行數', 'Scrollback lines kept for tab restore'],
  'settings.restoreLinesHint': ['0＝不保留畫面紀錄', '0 = do not keep the screen record'],
  'settings.exitRestore': ['關閉程式時預設勾「恢復分頁」', 'Tick "restore tabs" by default when quitting'],
  'settings.keepAlive': ['保持連線（分鐘，0＝關）', 'Keep-alive (min, 0 = off)'],
  'settings.autoReconnect': ['新連線預設開啟斷線自動重連', 'New connections auto-reconnect by default'],
  'settings.logDir': ['log 預設資料夾', 'Default folder for logs'],
  'settings.browse': ['瀏覽…', 'Browse…'],
  'settings.logTimestamp': ['log 每行加時間戳', 'Prefix each log line with a timestamp'],
  'settings.logAppend': ['log 檔已存在時附加在後面', 'Append to the log file if it exists'],
  'settings.groupSandbox': ['沙盒模式', 'Sandbox mode'],
  'settings.sandboxDefault': ['新增的自訂連線預設開啟沙盒', 'New custom connections start with the sandbox on'],
  'settings.sandboxNote': [
    '沙盒是防呆不是防壞（說明見 docs/AGENT-SANDBOX.md）。改這裡不會動到已經存在的連線。',
    'The sandbox guards against mistakes, not against malice (see docs/AGENT-SANDBOX.md). Changing this does not affect existing connections.',
  ],
  'settings.weakClear': ['清除已接受的弱演算法記錄', 'Clear the list of accepted weak algorithms'],
  // {0} = 幾筆
  'settings.weakCount': ['目前記了 {0} 台主機', '{0} host(s) currently remembered'],
  'settings.weakCleared': ['已清除。', 'Cleared.'],
  'settings.needRestart': ['（下次啟動分頁才生效）', '(takes effect the next time a tab starts)'],
  'common.reset': ['回到預設', 'Reset to default'],
  'common.ok': ['確定', 'OK'],
  'common.cancel': ['取消', 'Cancel'],

  // ---- 關於（TASK-015 C；舊版 About_Click）----
  'tb.about': ['關於', 'About'],
  'tip.about': ['版本、授權與檢查更新', 'Version, license and update check'],
  'about.title': ['關於 AwayTerminal', 'About AwayTerminal'],
  'about.version': ['版本', 'Version'],
  'about.buildTime': ['編譯時間', 'Build time'],
  'about.author': ['作者', 'Author'],
  'about.download': ['下載', 'Download'],
  'about.license': ['授權', 'License'],
  'about.thirdParty': ['第三方元件', 'Third-party components'],
  'about.noticesLink': ['完整第三方授權聲明', 'Full third-party notices'],
  'about.noticesFail': ['讀不到 THIRD-PARTY-NOTICES.md', 'Could not read THIRD-PARTY-NOTICES.md'],
  'about.close': ['關閉', 'Close'],

  // ---- 檢查更新（照舊版：只有按下去才查，失敗只在按鈕旁顯示一行）----
  'update.check': ['檢查更新', 'Check for updates'],
  'update.checking': ['檢查中…', 'Checking...'],
  'update.latest': ['已是最新版本', 'You are up to date'],
  'update.failed': ['檢查失敗（請確認網路後再試）', 'Check failed (check your connection and try again)'],
  'update.title': ['檢查更新', 'Check for updates'],
  'update.found': ['有新版本可用', 'A new version is available'],
  'update.current': ['目前版本', 'Current version'],
  'update.latestVer': ['最新版本', 'Latest version'],
  'update.notes': ['更新內容', "What's new"],
  'update.goDownload': ['前往下載頁', 'Open download page'],
  'update.close': ['關閉', 'Close'],
};

// ---------------------------------------------------------------- 語言
//
// 每個值是 `[繁中, English]`。呼叫端**一律用 `T['key']`**（和只有中文的時候一樣），
// 由下面的 Proxy 在存取的那一刻挑語言 —— 所以切語言之後重畫一次就全換掉了，
// 不必到 9 個檔案去改呼叫方式。
//
// 英文字串**優先照舊版 `Localization/Loc.cs` 的同名 key**（連標點與大小寫）；
// 舊版沒有的（內建 SSH／Telnet／COM／TTL／沙盒的對話框都是新版才有）才自己寫。

let lang = 'zh';

/** 設定語言（`'zh'`／`'en'`；其他值一律當 `zh`，同舊版 `Loc.SetLang`）。 */
export function setLang(code) {
  lang = code === 'en' ? 'en' : 'zh';
}

/** 目前語言。 */
export function getLang() {
  return lang;
}

/** 這個 key 在表裡嗎（設定視窗的自我檢查用）。 */
export function hasKey(key) {
  return Object.prototype.hasOwnProperty.call(STR, key);
}

/** 所有 key（i18n 稽核用）。 */
export function allKeys() {
  return Object.keys(STR);
}

function pick(entry) {
  if (entry === undefined) return undefined;
  // 還沒補英文的（理論上沒有，`npm run i18n:check` 會擋）→ 退回中文，不要顯示 undefined
  if (Array.isArray(entry)) return (lang === 'en' ? entry[1] : entry[0]) ?? entry[0];
  return entry;
}

/**
 * 介面文字。用起來和普通物件一樣（`T['tb.new']`），但值是**現在**這個語言的。
 * 查不到的 key 回 key 本身（fail-soft，和 Rust 端的 `i18n::t` 一致）。
 */
export const T = new Proxy(
  {},
  {
    get: (_t, key) => (typeof key === 'string' ? (pick(STR[key]) ?? key) : undefined),
    has: (_t, key) => typeof key === 'string' && key in STR,
    ownKeys: () => Object.keys(STR),
    getOwnPropertyDescriptor: (_t, key) => ({
      value: pick(STR[key]),
      enumerable: true,
      configurable: true,
    }),
  },
);

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
