using System.Collections.Generic;

namespace AwayTerminal.Localization;

/// <summary>簡易中/英字串表。切換語言時觸發 Changed，讓 UI 重新套用。</summary>
public static class Loc
{
    public static string Lang { get; private set; } = "zh";
    public static event Action? Changed;

    private static readonly Dictionary<string, (string zh, string en)> M = new()
    {
        ["app.name"] = ("AwayTerminal", "AwayTerminal"),
        ["tb.new"] = ("新連接", "New"),
        ["tip.new"] = ("新增連線（選擇類型）", "New connection (choose type)"),
        ["tb.history"] = ("紀錄", "History"),
        ["tip.history"] = ("最近開過的連線（點選重開）", "Recent connections (click to reopen)"),
        ["history.empty"] = ("（尚無紀錄）", "(no history yet)"),
        ["quick.all"] = ("（全部）", "(All)"),
        ["tb.claude"] = ("ClaudeCode", "ClaudeCode"),
        ["tip.claude"] = ("選目錄開啟並執行 Claude Code", "Open folder and run Claude Code"),
        ["tb.adb"] = ("ADB", "ADB"),
        ["tip.adb"] = ("開 ADB shell", "Open ADB shell"),
        ["adb.noPath"] = ("請先到「設定」設定 adb 路徑。", "Please set the adb path in Settings first."),
        ["adb.notInstalled"] = (
            "找不到 adb。AwayTerminal 不再內建 adb，請自行安裝 Android SDK Platform Tools\n" +
            "（或在「設定」中指定 adb.exe 路徑）。\n\n已搜尋：PATH、ANDROID_HOME / ANDROID_SDK_ROOT、Android Studio 預設位置。\n\n" +
            "要開啟官方下載頁嗎？",
            "adb was not found. AwayTerminal no longer bundles adb - please install the Android SDK " +
            "Platform Tools (or set the path to adb.exe in Settings).\n\nSearched: PATH, ANDROID_HOME / " +
            "ANDROID_SDK_ROOT, and the default Android Studio location.\n\nOpen the official download page?"),
        ["adb.noDevice"] = ("沒有偵測到 adb 裝置。", "No adb device detected."),
        ["adb.pickDevice"] = ("選擇裝置", "Pick a device"),
        ["settings.adbPath"] = ("adb 路徑", "adb path"),
        ["settings.font"] = ("字型", "Font"),
        ["settings.claudePath"] = ("Claude 路徑", "Claude path"),
        ["settings.claudeArgs"] = ("Claude 參數", "Claude arguments"),
        ["settings.claudeViaPs"] = ("透過 PowerShell 執行（npm 版 claude 請勾）", "Run via PowerShell (check for npm claude)"),
        ["claude.noPath"] = ("請先到「設定」設定 Claude 路徑（找不到 claude 執行檔）。", "Please set the Claude path in Settings first (claude executable not found)."),
        // 設定 GroupBox 標題 / 啟用勾選
        ["settings.groupLang"] = ("語言", "Language"),
        ["settings.groupFont"] = ("字體背景顏色", "Font & colors"),
        ["settings.groupClaude"] = ("Claude", "Claude"),
        ["settings.groupAdb"] = ("adb", "adb"),
        ["settings.claudeEnable"] = ("使用 Claude（工具列顯示按鈕）", "Use Claude (show toolbar button)"),
        // Claude 輸入送出：靜止閘門（1.0.43）
        ["settings.groupIme"] = ("Claude 輸入送出", "Claude input timing"),
        ["settings.imeQuiet"] = ("送出前等待靜止 (ms)", "Wait for quiet before send (ms)"),
        ["settings.imeQuietHelpLink"] = ("這是什麼？", "What is this?"),
        ["settings.imeQuietHelpTitle"] = ("送出前等待靜止 (ms)", "Wait for quiet before send (ms)"),
        ["settings.imeQuietHelp"] = (
            "此設定只作用於 Claude Code 分頁。\n\n" +
            "打注音（整段送出）、貼上、或按 Backspace 時，若 Claude 正在重繪畫面（執行中、" +
            "建議文字在跳），直接送出偶爾會讓 Claude 把剛輸入的字重複顯示成兩份，或在全形/半形" +
            "混合時把游標位置算錯、少一格或留殘影。\n\n" +
            "開啟後，這幾類輸入會等 Claude 畫面靜止「這麼多毫秒」才送出，避開重繪空檔、降低上述問題。\n\n" +
            "• 只有 Claude 忙碌重繪時才會有這點延遲；停在提示列打字時 0 延遲。\n" +
            "• 一般英數打字、Enter、Ctrl 鍵不受影響。\n" +
            "• 數字越大越保守（較不易出錯，忙碌時延遲略增）；越小反應越快、保護越弱。\n" +
            "• 設 0 = 關閉此功能（立即送出）。\n\n" +
            "預設 20。這是降低問題頻率的緩解措施；根本原因在 Claude Code 端的畫面重繪。",
            "This setting only affects Claude Code tabs.\n\n" +
            "When you commit IME (Zhuyin) text, paste, or press Backspace while Claude is repainting " +
            "(running, or the suggestion text is updating), sending immediately can occasionally make " +
            "Claude echo the just-typed text twice, or miscompute the cursor column when full-width and " +
            "half-width characters are mixed (a column short, or a leftover ghost).\n\n" +
            "When enabled, these inputs wait until Claude's output has been quiet for this many " +
            "milliseconds before being sent, avoiding the repaint window and reducing those problems.\n\n" +
            "• The delay only applies while Claude is busy repainting; typing at an idle prompt has 0 delay.\n" +
            "• Normal letters/digits, Enter and Ctrl keys are unaffected.\n" +
            "• Higher = more conservative (fewer glitches, slightly more delay when busy); lower = snappier, weaker protection.\n" +
            "• Set 0 to turn this off (send immediately).\n\n" +
            "Default is 20. This is a mitigation that lowers the frequency; the root cause is Claude Code's own screen repaint."),
        // 註：ADB 的設定字串（groupAdb / adbEnable / adbPath）自 v1.0.18 起在設定視窗已不使用——
        // ADB 改由「新連接 → 自訂…」管理。保留鍵值以免其他地方漏改時直接 KeyNotFound。
        // 分頁右鍵：配色
        ["menu.color"] = ("配色", "Colors"),
        ["menu.colorDefault"] = ("預設（設定顏色）", "Default (settings colors)"),

        // 自訂新連接
        ["menu.custom"] = ("自訂…", "Custom…"),
        ["custom.title"] = ("自訂新連接", "Connections"),
        ["custom.icon"] = ("圖示", "Icon"),
        ["custom.name"] = ("名稱", "Name"),
        ["custom.path"] = ("執行檔", "Executable"),
        ["custom.args"] = ("參數", "Arguments"),
        ["custom.pickDir"] = ("啟動前選擇資料夾", "Choose folder before launch"),
        ["custom.hidden"] = ("隱藏", "Hidden"),
        ["custom.viaPs"] = ("使用 PowerShell", "Use PowerShell"),
        ["custom.closeKey"] = ("關閉按鍵", "Close key"),
        ["custom.closeNone"] = ("無", "None"),
        ["custom.add"] = ("＋ 新增", "＋ New"),
        ["custom.delBtn"] = ("－ 刪除", "－ Delete"),
        ["custom.save"] = ("儲存", "Save"),
        ["custom.delete"] = ("刪除", "Delete"),
        ["custom.back"] = ("返回", "Back"),
        ["custom.detect"] = ("自動偵測", "Auto-detect"),
        ["custom.detectNone"] = ("沒有偵測到可加入的工具（claude / opencode 等，或都已在清單中）。",
                                  "No new tools detected (claude / opencode, etc., or all already listed)."),
        ["custom.detectDone"] = ("已加入：{0}", "Added: {0}"),
        ["custom.delConfirm"] = ("確定要刪除「{0}」？", "Delete \"{0}\"?"),
        ["custom.untitled"] = ("新項目", "New item"),
        ["custom.needName"] = ("請輸入名稱。", "Please enter a name."),
        ["custom.notFound"] = ("找不到執行檔：", "Executable not found:"),
        ["custom.unsavedAsk"] = ("尚未儲存，是否儲存變更？", "You have unsaved changes. Save them?"),
        ["tb.powershell"] = ("PowerShell", "PowerShell"),
        ["tb.ssh"] = ("SSH/Telnet", "SSH/Telnet"),
        ["tb.com"] = ("連接埠", "COM"),
        ["tb.copy"] = ("複製", "Copy"),
        ["tb.paste"] = ("純文字貼上", "Paste as text"),
        ["tb.copyall"] = ("複製全部", "Copy All"),
        ["tb.clear"] = ("清除畫面", "Clear"),
        ["tb.page"] = ("翻頁", "Scroll"),
        ["page.up"] = ("上一頁", "Page up"),
        ["page.down"] = ("下一頁", "Page down"),
        ["page.top"] = ("移到最上面", "Go to top"),
        ["page.bottom"] = ("移到最下面", "Go to bottom"),
        ["tb.prompt"] = ("常用字串", "Snippets"),
        ["tb.font"] = ("字體背景", "Font/BG"),
        ["tb.remote"] = ("遠端設定", "Remote"),
        ["tb.settings"] = ("設定", "Settings"),
        ["tb.about"] = ("關於", "About"),
        ["tb.split"] = ("視窗分割", "Split"),
        ["tb.tabs"] = ("視窗分頁", "Tabs"),
        ["tb.columns"] = ("視窗分欄", "Columns"),
        ["tip.split"] = ("點按循環：分頁 → 分割 → 分欄", "Click to cycle: Tabs → Split → Columns"),
        ["tip.tabs"] = ("點按循環：分頁 → 分割 → 分欄", "Click to cycle: Tabs → Split → Columns"),
        ["tip.columns"] = ("點按循環：分頁 → 分割 → 分欄", "Click to cycle: Tabs → Split → Columns"),

        ["tip.powershell"] = ("開 PowerShell", "Open PowerShell"),
        ["tip.ssh"] = ("開 SSH / Telnet", "Open SSH / Telnet"),
        ["tip.com"] = ("開 COM 埠", "Open COM port"),
        ["tip.copy"] = ("複製選取的文字", "Copy selection"),
        ["tip.paste"] = ("把剪貼簿內容以純文字貼進終端機", "Paste clipboard as plain text"),
        ["tip.copyall"] = ("複製全部緩衝文字", "Copy all buffer text"),
        ["tip.clear"] = ("清除畫面", "Clear screen"),
        ["tip.page"] = ("捲動畫面（上/下一頁、最上/最下面）", "Scroll the view (page up/down, top/bottom)"),
        ["tip.prompt"] = ("常用字串", "Common strings"),
        ["tip.font"] = ("字體 / 背景設定", "Font / background"),
        ["tip.remote"] = ("遠端控制設定 (Telegram)", "Remote control (Telegram)"),
        ["tip.settings"] = ("設定", "Settings"),
        ["tip.about"] = ("關於", "About"),

        ["about.title"] = ("關於 AwayTerminal", "About AwayTerminal"),
        ["about.author"] = ("作者", "Author"),
        ["about.version"] = ("版本", "Version"),
        ["about.buildTime"] = ("編譯時間", "Build time"),
        ["about.download"] = ("下載", "Download"),
        ["about.license"] = ("授權", "License"),
        ["about.thirdParty"] = ("第三方元件", "Third-party components"),
        // 檢查更新（「關於」視窗底部按鈕；資料來自 awaysu.cc/software 的 check_update API）
        ["update.check"] = ("檢查更新", "Check for updates"),
        ["update.checking"] = ("檢查中…", "Checking..."),
        ["update.latest"] = ("已是最新版本", "You are up to date"),
        ["update.failed"] = ("檢查失敗（請確認網路後再試）", "Check failed (check your connection and try again)"),
        ["update.title"] = ("檢查更新", "Check for updates"),
        ["update.found"] = ("有新版本可用", "A new version is available"),
        ["update.current"] = ("目前版本", "Current version"),
        ["update.latestVer"] = ("最新版本", "Latest version"),
        ["update.notes"] = ("更新內容", "What's new"),
        ["update.goDownload"] = ("前往下載頁", "Open download page"),
        ["update.close"] = ("關閉", "Close"),

        ["settings.title"] = ("設定", "Settings"),
        ["settings.language"] = ("語言 / Language", "語言 / Language"),
        ["common.ok"] = ("確定", "OK"),
        ["common.cancel"] = ("取消", "Cancel"),
        ["common.save"] = ("儲存", "Save"),

        // 遠端設定 (Telegram)
        ["remote.title"] = ("遠端設定 (Telegram)", "Remote (Telegram)"),
        ["remote.enable"] = ("啟用遠端控制", "Enable remote control"),
        ["remote.token"] = ("Bot Token", "Bot Token"),
        ["remote.chatId"] = ("允許的 Chat ID", "Allowed Chat ID"),
        ["remote.getChatId"] = ("取得 chat id", "Get chat id"),
        ["remote.notify"] = ("其他（未進入的）分頁完成也推播通知", "Also notify for tabs you haven't entered"),
        ["remote.hint"] = ("向 @BotFather 申請 bot 取得 token；先用手機傳一則訊息給你的 bot，再按「取得 chat id」。",
                            "Create a bot via @BotFather for the token; send your bot a message first, then click \"Get chat id\"."),
        ["remote.needToken"] = ("請先填入 Bot Token。", "Please enter the Bot Token first."),
        ["remote.noUpdates"] = ("找不到訊息。請先用手機傳一則訊息給你的 bot，再試一次。",
                                 "No message found. Send your bot a message from your phone first, then try again."),
        ["remote.gotChatId"] = ("已取得 chat id：{0}", "Got chat id: {0}"),

        ["msg.willAddP4"] = ("此功能將在 P4 加入。", "This feature will be added in P4."),

        // 分頁右鍵選單 / 分頁列
        ["menu.rename"] = ("更改名稱", "Rename"),
        ["menu.log"] = ("記錄 log…", "Record log…"),
        ["menu.macro"] = ("執行巨集…", "Run macro…"),
        ["menu.close"] = ("關閉", "Close"),
        // 1.1.2：分頁列的 log／巨集圖示移除，改在分頁 tooltip 註明狀態
        ["tip.tabLogging"] = ("● 記錄 log 中", "● Recording log"),
        ["tip.tabMacroRunning"] = ("● 巨集執行中", "● Macro running"),
        ["tip.tabClose"] = ("關閉", "Close"),
        // 1.1.2：分頁列連線圖示（綠=閒、紅=忙）的 tooltip＝是哪一種連線
        ["kind.powershell"] = ("PowerShell", "PowerShell"),
        ["kind.ssh"] = ("SSH", "SSH"),
        ["kind.telnet"] = ("Telnet", "Telnet"),
        ["kind.com"] = ("連接埠 (COM)", "Serial port (COM)"),
        ["kind.adb"] = ("ADB", "ADB"),
        ["kind.claude"] = ("Claude Code", "Claude Code"),
        ["kind.custom"] = ("自訂連線", "Custom connection"),
        ["tip.tabPanel"] = ("顯示／隱藏分頁列表", "Show / hide the tab list"),

        // 分頁列最左「…」輸入框（先打好再送出）
        // 1.0.46：工具列「輸入文字」鈕（取代分頁列「…」與右側快速輸入抽屜），視窗上方多常用字串列
        ["tb.compose"] = ("輸入文字", "Compose"),
        ["tip.compose"] = ("先打好文字或選常用字串，再送到目前分頁", "Compose text or pick a snippet, then send to the current tab"),
        ["compose.insert"] = ("插入", "Insert"),
        ["compose.sendNow"] = ("直接送出", "Send now"),
        ["compose.sent"] = ("✓ 已送出", "✓ Sent"),
        ["compose.title"] = ("輸入文字", "Compose"),
        ["compose.hint"] = ("在此輸入要送到目前分頁的文字（可多行；Ctrl+Enter 送出）", "Type the text to send to the current tab (multi-line OK; Ctrl+Enter sends)"),
        ["compose.send"] = ("送出", "Send"),
        ["compose.back"] = ("返回", "Back"),
        ["compose.clear"] = ("清除", "Clear"),
        ["compose.undo"] = ("復原", "Undo"),
        ["compose.noTab"] = ("沒有分頁可送", "No tab to send to"),

        // 浮動提示
        ["toast.copied"] = ("複製成功", "Copied"),
        ["toast.copiedPasted"] = ("已複製並貼上", "Copied and pasted"),
        ["toast.copiedAll"] = ("已複製全部文字", "All text copied"),
        ["toast.noSelection"] = ("沒有選取文字", "Nothing selected"),

        // MessageBox
        ["msg.closeTabConfirm"] = ("確定要關閉「{0}」？", "Close \"{0}\"?"),
        ["msg.closeTabTitle"] = ("關閉分頁", "Close Tab"),
        ["msg.exitedCloseAsk"] = ("「{0}」的連線已結束（程式離開或斷線）。\n\n要關閉這個分頁嗎？", "\"{0}\" has ended (the program exited or the connection dropped).\n\nClose this tab?"),
        ["msg.exitedTitle"] = ("連線已結束", "Session Ended"),
        ["msg.clearConfirm"] = ("確定要清除「{0}」的畫面嗎？", "Clear the screen of \"{0}\"?"),
        ["msg.clearTitle"] = ("清除畫面", "Clear Screen"),
        ["msg.connectFail"] = ("連線 / 啟動失敗：", "Connection / launch failed:"),
        ["msg.restoreAsk"] = ("下次開啟時要恢復目前的分頁嗎？", "Restore current tabs next time?"),
        ["msg.exitAsk"] = ("確定要關閉 AwayTerminal 嗎？", "Close AwayTerminal?"),
        ["msg.exitTitle"] = ("關閉程式", "Exit"),
        ["exit.title"] = ("關閉 AwayTerminal", "Close AwayTerminal"),
        ["exit.msg"] = ("確定要關閉 AwayTerminal 嗎？", "Close AwayTerminal?"),
        ["exit.restore"] = ("下次開啟恢復目前分頁（含畫面上的舊訊息）", "Restore current tabs next time (with scrollback)"),
        ["exit.updateMd"] = ("Claude Code 離開前更新 CLAUDE.md", "Update CLAUDE.md before Claude Code exits"),
        ["exit.updating"] = ("正在請 Claude Code 更新 CLAUDE.md，請稍候…", "Asking Claude Code to update CLAUDE.md, please wait…"),
        ["exit.ok"] = ("確定離開", "Exit"),
        ["exit.mdPrompt"] = ("請更新 CLAUDE.md，把這次工作的重點與變更記錄進去。", "Please update CLAUDE.md to record this session's key changes."),
        ["msg.stopLogAsk"] = ("要停止記錄 log 嗎？", "Stop logging?"),
        ["msg.stopMacroAsk"] = ("要停止巨集嗎？", "Stop the macro?"),
        ["msg.logFail"] = ("無法開始記錄：", "Cannot start logging:"),
        ["msg.macroReadFail"] = ("無法讀取巨集：", "Cannot read macro:"),
        ["dlg.logTitle"] = ("記錄 log", "Record Log"),
        ["dlg.macroTitle"] = ("執行巨集", "Run Macro"),
        ["dlg.renameTitle"] = ("更改名稱", "Rename"),
        ["dlg.renamePrompt"] = ("分頁名稱：", "Tab name:"),
        ["dlg.pickDirPs"] = ("選擇 PowerShell 工作目錄（可在此按「建立新資料夾」）", "Choose the PowerShell working folder"),
        ["dlg.pickDirClaude"] = ("選擇 Claude Code 工作目錄（可在此按「建立新資料夾」）", "Choose the Claude Code working folder"),
        ["term.exited"] = ("[連線已結束]", "[session ended]"),
        ["term.reconnect"] = ("[連線中斷，{0} 秒後自動重連…（關閉分頁可停止）]", "[Disconnected. Reconnecting in {0}s… (close tab to stop)]"),
        // 1.0.45：SSH/Telnet/COM 沒勾自動重連時，session 結束後在同一分頁按 Enter 重連（舊訊息留在 scrollback）
        ["term.exitedEnter"] = ("[按 Enter 在此分頁重新連線]", "[Press Enter to reconnect in this tab]"),
        // 1.0.45：恢復分頁時倒回舊訊息後的分隔行（{0}＝上次關閉時間）
        ["term.restoredSep"] = ("──── 以上為上次關閉前的紀錄（{0}）────", "──── previous session, saved {0} ────"),
        // 1.0.45：檔案總管右鍵選單
        ["shell.menuText"] = ("用 AwayTerminal 開啟", "Open in AwayTerminal"),
        ["shell.dirMissing"] = ("找不到資料夾：\n{0}", "Folder not found:\n{0}"),
        ["settings.groupShell"] = ("檔案總管", "File Explorer"),
        ["settings.shellMenu"] = ("資料夾右鍵選單加入「用 AwayTerminal 開啟」（在該資料夾開 PowerShell 分頁）",
                                  "Add \"Open in AwayTerminal\" to the folder context menu (opens a PowerShell tab there)"),
        ["conn.autoReconnect"] = ("斷線自動重連", "Auto-reconnect on disconnect"),
        ["conn.keepAlive"] = ("保持連線（分鐘，0=關）", "Keep-alive (min, 0=off)"),

        ["remote.takenByOther"] = (
            "遠端已由另一個 AwayTerminal 視窗使用中，此視窗不啟動遠端（同一組 Bot Token 同時只能一個視窗連線）。關閉另一個視窗後，回到此視窗的遠端設定按「儲存」即可接手。",
            "Remote is already running in another AwayTerminal window; this window will not start it (one bot token allows only one active connection). Close the other window, then press Save in this window's Remote settings to take over."),

        // 終端機右鍵選單
        ["ctx.cut"] = ("剪下", "Cut"),
        ["ctx.copy"] = ("複製", "Copy"),
        ["ctx.copyPaste"] = ("複製且貼上", "Copy and paste"),
        ["ctx.paste"] = ("貼上", "Paste"),
        ["ctx.selectAll"] = ("全選", "Select all"),
        ["ctx.search"] = ("搜尋", "Find"),
        ["ctx.copyAllFile"] = ("複製全部存至檔案", "Copy all to file"),
        ["msg.saveFail"] = ("存檔失敗", "Save failed"),

        // 連線視窗
        ["conn.title"] = ("開 SSH / Telnet", "Open SSH / Telnet"),
        ["conn.type"] = ("類型", "Type"),
        ["conn.host"] = ("IP / 主機", "IP / Host"),
        ["conn.connect"] = ("連線", "Connect"),
        ["conn.needHost"] = ("請輸入 IP / 主機。", "Please enter an IP / host."),

        // COM 視窗
        ["com.title"] = ("開連接埠", "Open COM Port"),
        ["com.open"] = ("開啟", "Open"),
        ["common.reset"] = ("回到預設", "Reset to default"),

        // 字體視窗
        ["font.title"] = ("字體 / 背景設定", "Font / Background"),
        ["font.family"] = ("字型", "Font"),
        ["font.size"] = ("大小", "Size"),
        ["font.fg"] = ("文字顏色", "Text color"),
        ["font.bg"] = ("背景顏色", "Background color"),
        ["font.pick"] = ("點我選顏色", "Click to pick a color"),

        // Prompt 視窗
        ["prompt.title"] = ("常用字串", "Snippets"),
        ["prompt.send"] = ("送出", "Send"),
        ["prompt.delete"] = ("刪除", "Delete"),
        ["prompt.edit"] = ("修改", "Edit"),
        ["prompt.save"] = ("儲存", "Save"),
        ["prompt.new"] = ("新增", "New"),
        ["prompt.clear"] = ("取消/清空", "Clear"),
        ["prompt.header"] = ("標題", "Title"),
        ["prompt.content"] = ("內容", "Content"),
        ["prompt.sendEnter"] = ("送出後送 Enter", "Send Enter after submit"),
        ["prompt.group"] = ("群組", "Group"),
        ["prompt.backup"] = ("備份", "Backup"),
        ["prompt.load"] = ("載入", "Load"),
        ["prompt.loadReplace"] = ("載入會取代目前的常用字串清單，確定？", "Loading replaces the current snippet list. Continue?"),
        ["prompt.ungrouped"] = ("未分組", "Ungrouped"),
        ["prompt.groupRename"] = ("群組改名", "Rename group"),
        ["prompt.groupRenamePrompt"] = ("新群組名稱：", "New group name:"),
        ["prompt.groupDelete"] = ("刪除群組", "Delete group"),
        ["prompt.groupDeleteAsk"] = ("刪除群組「{0}」？\n是＝連同字串一起刪除；否＝把字串移到未分組；取消＝不動作。",
                                      "Delete group \"{0}\"?\nYes = delete its snippets; No = move them to Ungrouped; Cancel = nothing."),
        ["prompt.noContent"] = ("沒有內容可送出。", "Nothing to send."),
        ["prompt.delConfirm"] = ("確定要刪除「{0}」？", "Delete \"{0}\"?"),
        ["prompt.needTitle"] = ("請輸入標題。", "Please enter a title."),

        // Log 視窗
        ["log.path"] = ("log 存檔位置：", "Log file path:"),
        ["log.browse"] = ("瀏覽…", "Browse…"),
        ["log.timestamp"] = ("每行前面加時間戳 [yy-MM-dd HH:mm:ss]", "Prefix each line with [yy-MM-dd HH:mm:ss]"),
        ["log.append"] = ("檔案已存在時附加（append）", "Append if the file exists"),
        ["log.start"] = ("開始記錄", "Start logging"),
        ["log.needPath"] = ("請輸入 log 存檔位置。", "Please enter the log file path."),

        // 其他設定
        ["settings.claudeCmd"] = ("Claude Code 指令", "Claude Code command"),

        // 分頁預設名稱
        ["tab.newConn"] = ("新連線{0}", "New Tab {0}"),
    };

    public static string T(string key)
        => M.TryGetValue(key, out var v) ? (Lang == "en" ? v.en : v.zh) : key;

    public static void SetLang(string lang)
    {
        var l = lang == "en" ? "en" : "zh";
        if (l == Lang) return;
        Lang = l;
        Changed?.Invoke();
    }

    public static void Init(string lang)
    {
        Lang = lang == "en" ? "en" : "zh";
    }
}

/// <summary>供 XAML 綁定用的字串代理（DataTemplate 內的選單/工具提示可動態換語言）。
/// 用法：Header="{Binding Path=[menu.rename], Source={x:Static loc:LocProxy.Instance}}"</summary>
public sealed class LocProxy : System.ComponentModel.INotifyPropertyChanged
{
    public static LocProxy Instance { get; } = new();
    private LocProxy()
    {
        Loc.Changed += () => PropertyChanged?.Invoke(this,
            new System.ComponentModel.PropertyChangedEventArgs("Item[]"));
    }
    public string this[string key] => Loc.T(key);
    public event System.ComponentModel.PropertyChangedEventHandler? PropertyChanged;
}
