# CLAUDE.md — AwayTerminal

Windows 原生終端機模擬器。**C# WPF (.NET 9) 外殼 + WebView2 內嵌 xterm.js（終端機渲染）+ ConPTY / 內建 Telnet / 序列埠**。
使用者：awaysu@gmail.com（偏好繁體中文回覆）。名稱：中英文皆為「**AwayTerminal**」（`Loc.T("app.name")`；原中文名「艾維終端機」已全面改為 AwayTerminal）。
GitHub：原始碼 https://github.com/awaysu/AwayTerminal（public）、安裝檔下載 https://github.com/awaysu/Download。

## 建置 / 執行 / 測試
```powershell
dotnet build                                     # 建置（會自動簽章 exe，見「數位簽章」）
.\bin\Debug\net9.0-windows\AwayTerminal.exe      # 執行
```
- GUI 程式，**無法在此環境互動測試**。驗證：`dotnet build` 0 錯誤 + 冒煙啟動
  （`Start-Process` 後檢查子行程 msedgewebview2.exe / powershell.exe / conhost.exe）。
- 從工具環境啟動 exe 會被沙箱擋 → `dangerouslyDisableSandbox: true`。
- 重建前先 `Get-Process AwayTerminal | Stop-Process -Force`，否則 exe 鎖住 build 失敗（MSB3027）。
- UI 疑難時請使用者截圖，用 `System.Drawing` **逐像素取樣**分析（曾靠此找到 xterm 黑帶 #000）。

## 架構
- **WPF 原生 chrome**：工具列（圖上字下、寬 72px；右側直欄模式圖左字右、寬 114px）、分頁列（**在視窗底部**）、各對話框、右鍵選單。
- **單一 WebView2** 載入 `web/index.html`；`terminal.js` 管理多個 xterm 實例；分頁模式只顯示 active，分割/分欄用 CSS grid 全顯。
- **C# 每分頁一個 `ITerminalSession`**：`ConPtySession`(PowerShell / claude.exe / ssh.exe / adb.exe / 自訂 exe)、`TelnetSession`(TCP+IAC)、`SerialSession`(COM，專用 blocking-read 執行緒)。
- **`TermKind`**：PowerShell / Ssh / Telnet / Com / Adb / Claude / Custom。狀態燈：PowerShell=子行程樹+近 1.5s 輸出；Claude/Custom=近 1.2s 輸出；其餘（遠端/COM）=近 0.5s 輸出。

### C# ↔ JS 訊息協定（字串；分隔字元 US = ``）
- JS→C#：`i{id}{US}{text}`(輸入)、`r{id}{US}{cols},{rows}`(尺寸)、`a{id}{US}{kind}{US}{text}`(查詢回覆)、
  `p{id}`(點選 pane)、`k{id1,id2,...}`(拖曳後新順序)、`z{size}`(Ctrl+滾輪縮放後字級)、`ready`
- C#→JS：`o{id}{US}{base64}`(輸出)、`n{id}{US}{title}`(建立)、`t{id}{US}{title}`(改名)、
  `s{id}`/`x{id}`/`c{id}`/`f{id}`(顯示/關閉/清除/聚焦)、`L{tab|split|columns}`(模式)、
  `q{id}{US}{sel|all|text}`(查詢文字；text=遠端用、xterm 渲染後純文字最後 400 行)、`T{json}`(全域字型/顏色)、`P{id}{US}{fg}{US}{bg}`(單一分頁配色；空=回設定預設)、`A{id}`(全選)、`F`(開 Ctrl+F 搜尋列)
- 改協定時 **C# 與 `web/terminal.js` 必須同步**。

## 關鍵檔案
| 檔 | 作用 |
|----|------|
| `MainWindow.xaml(.cs)` | 主視窗：工具列/分頁/狀態輪詢/工作列圓點/New 下拉/紀錄/自訂連線/快速輸入抽屜/login-as/恢復分頁 |
| `App.xaml.cs` | 啟動時清理環境變數（見「踩雷」）＋ TERM/COLORTERM |
| `ConPty/` | ConPTY P/Invoke（含 TerminateProcess、`GracefulExitBytes` 優雅關閉；預設 Ctrl+C ×3）|
| `Sessions/` | ITerminalSession + Telnet/Serial（Serial 用 blocking-read 執行緒，非 DataReceived）|
| `Dialogs/` | CustomConnDialog(自訂連線)、PromptDialog(常用字串+群組)、RemoteDialog(遠端設定)、SettingsDialog、ConnectDialog、ComDialog、ExitDialog、LogDialog、InputDialog… |
| `Services/TelegramRemote.cs` | Telegram 遠端控制（自寫 long polling、指令解析、`IRemoteHost` 介面、`TidyForPhone` 雜訊過濾）|
| `Models/TerminalTab.cs` | 分頁模型（狀態、尺寸、`StartUtc`+已啟動時間 tooltip、PendingCommand、LoginBuffer、Restore）|
| `Services/AppSettings.cs` | 設定（`%LOCALAPPDATA%\AwayTerminal\settings.json`）：CustomConns、History、Prompts(含 Group)、字型/顏色、SavedTabs… |
| `Localization/Loc.cs` | 中/英字串表 + `LocProxy`（XAML 綁定動態換語言）|
| `Logging/` `Macros/` | log 記錄（時間戳 `[yy-MM-dd HH:mm:ss]`）/ TTL 巨集直譯器 |
| `web/` | 前端（`terminal.js` + `vendor/` xterm）；`icon/` 工具列與圖示 PNG（WPF Resource）|
| `tools/adb/` | **內建打包的 adb**（adb.exe + AdbWinApi/AdbWinUsbApi/libwinpthread dll），隨 build 複製到輸出，ADB 一律用它 |
| `installer/` | Inno Setup 安裝腳本 `installer.iss` + 公開憑證 `AwayTerminal.cer`（見「數位簽章 / 安裝檔」）|
| `app.ico` | 由 `icon/app-icon.png` 產生（PNG 內嵌 ICO）|

## 慣例
- **版本**：每交付一次 `<Version>` +0.0.1（「關於」顯示）。目前 **1.0.0**（2026-07-27 由 0.9.87 升版）。「關於」是自訂深色小視窗（非 MessageBox）、內容置左：標題/版本、作者行 `Awaysu (awaysu@gmail.com)` **以執行期渲染的圖片顯示**（`RenderTextImage`，依 DPI 畫、防文字收集）、Source Code 可點連結開 GitHub。
- **i18n**：使用者可見字串走 `Loc.T`；工具列文字：新連接(New) / 紀錄 / 複製 / 貼上 / 複製全部 / 清除畫面 / 分割 / 功能列 / 常用字串 / 遠端設定 / 設定 / 關於。
- **設定持久化**：記住值都進 `AppSettings.Current` + `.Save()`。
- **分頁命名**：`NextName(prefix)` → 「型態(數字)」，例 PowerShell(1)、ADB(1)、自訂用「名稱(數字)」；跳過已存在名稱。
- **狀態圓點**（分頁左側）：綠=可輸入、橘=忙，純活動偵測（已移除 BEL 那套）。**工作列 icon 右下**另有 overlay 圓點：全部閒置=綠、有忙=橘、無分頁=無（`TaskbarItemInfo.Overlay`）。
- **強調色**：`#FDFFB0` 淡黃（作用中分頁框、終端機外框、分割 pane 框、快速輸入面板框）。作用中分頁上方開口與外框融合（tab 往上 -1px 蓋線）。

## 主要功能行為
- **New（新連接）下拉**（工具列最左）：PowerShell → SSH/Telnet → 連接埠 → ADB → 分隔線 → 你的自訂連線 → 「自訂…」(開管理視窗)。項目為圖示+文字。
- **Claude Code**：不再是內建按鈕，已**遷移成一筆自訂連線「ClaudeCode」**（`EnsureDefaults` 只做一次、以 `ClaudeMigratedToCustom` 旗標記住）。**直接以 ConPTY 執行 claude.exe（不經 PowerShell）**：claude 即主行程、一開始就用目前尺寸建立，**已移除舊的「等尺寸才送指令」hack**；勾「使用 PowerShell」或路徑是 .cmd/.bat（npm 版）時才走 PowerShell。
- **自訂連線**（`CustomConnDialog`）：左清單（可收合）+ 底部「自動偵測 / ＋新增 / －刪除」+ 右編輯（圖示下拉 32×32 無字 / 名稱 / 執行檔+瀏覽 / 參數 / 關閉按鍵〔無·Ctrl+C·Ctrl+D〕×〔x1~x5〕/ 隱藏 / 啟動前選擇資料夾 / 使用 PowerShell）+ 「儲存 / 返回」。髒資料追蹤、切換/關閉/刪除前提示。**自動偵測**：在 PATH 與常見位置找 claude/wsl/opencode/gemini/aider 加入。「隱藏」= 不列在 New 下拉。自訂分頁**不做開機還原**。
- **ADB**：**內建 adb**（`AppSettings.BundledAdbPath`，`tools/adb/adb.exe`），不需設路徑。按下先 `adb devices`：0 台提示、1 台直接開、2 台以上跳選單選序號；直接以 ConPTY 跑 `adb shell`。
- **紀錄按鈕**（New 右邊，icon `history.png`）：下拉列最近 10 次連線（圖示+標籤），點選以相同設定重開。每次開連線都記錄（最新在前、去重、上限 20），存於 `AppSettings.History`。開啟邏輯抽成 `OpenPowerShellDirect`/`OpenTelnetDirect`/`OpenSshLoginAs`/`OpenComDirect`/`OpenAdbShell`/`OpenCustom` 共用。
- **常用字串**（`PromptDialog`，原「Prompt」）：風格同自訂連線。**群組（方式 B）**：`PromptItem.Group`；左清單依群組顯示**可收合標題**，群組標題**右鍵可改名/刪群組**（刪群組問：連字串刪 / 移到未分組 / 取消）。編輯區有「群組」可編輯下拉。**備份/載入 XML**（左下兩鈕；載入取代目前清單）。
- **快速輸入抽屜**：工具列（關於那列）**最右端有小箭頭 ▼**（貼近黃框上緣），按下右側浮出約 1/5 寬面板（群組 / 標題 / 內容 / 送出）；再按 ▲ 收合。**放在終端機右側欄、不疊在 WebView2 上**（避免 airspace，見踩雷）。
- **SSH**：無帳號欄。連線後顯示 `login as:`（PuTTY 式，`HandleLoginInput`/`OpenSshLoginAs`），輸入帳號才啟動 `ssh.exe user@host`；主機欄可直接打 `user@host`。
- **關分頁**：背景執行緒送優雅結束鍵（`GracefulExitBytes`，預設 Ctrl+C ×3；SSH=Ctrl+D ×3；自訂可設 無/Ctrl+C/Ctrl+D ×1~5）→ 60ms 後 TerminateProcess → ClosePseudoConsole。不卡 UI。
- **關閉程式**（`ExitDialog`）：兩選項「下次開啟恢復目前分頁」「Claude Code 離開前更新 CLAUDE.md」（**勾選狀態記憶**於 `ExitRestoreTabs`/`ExitUpdateMd`）。確認後存 SavedTab（ps/claude/ssh/telnet/com）→ 快速終止 → **`Environment.Exit(0)`**（跳過 WebView2 冗長 teardown）。
- **啟動**：`ready` 後只恢復上次存下的分頁；**沒有就保持空白（不再硬開預設 PowerShell）**。
- **分頁 tooltip**：`標題 時:分`（已啟動時間，例 `PowerShell(1) 00:23`），狀態輪詢每 0.6s 更新。
- **視窗檢視三態**（`_viewMode`，「分割」鈕循環）：tab → split(grid) → columns(單列橫排) → tab。分割/分欄可拖曳重排、點標題單頁 zoom。
- **功能列位置**（`ToolbarPosition`）：top(圖上字下) ↔ right(右側直欄 114px，圖左字右，附加屬性 `ToolBtnEx.Horizontal` + template trigger)。
- **字型/顏色**：`設定`(SettingsDialog) 用 GroupBox 分「語言 / 字體背景顏色」；**Ctrl+滾輪**縮放字級（全域、記憶、`z` 協定）；**分頁右鍵「配色」**逐分頁套色（5 組預設 + 回設定預設，`P` 協定）。
- **記錄 log**：視窗按鈕「開始記錄」；時間戳 `[yy-MM-dd HH:mm:ss]`。
- **遠端控制（Telegram，v0.9.74~76 新增）**：一台 PC 一個 bot（@BotFather 申請 token），`RemoteDialog` 設定 Bot Token / 允許的 Chat ID（一鍵抓）/ 推播開關（`AppSettings.RemoteEnabled/TelegramBotToken/TelegramChatId/RemoteNotify`）。`TelegramRemote` 自寫 HttpClient long polling（無第三方套件），啟動時 PrimeOffset 跳過舊訊息、只認設定的 chat_id。指令：`/list` `/goto <n>` `/new`（**每種連線只列一個**：PowerShell(桌面)/SSH/Telnet/COM/ADB/未隱藏自訂各一，**不列 History**；回數字開啟+自動附著；PowerShell 與 PickDir 自訂一律以桌面為工作目錄）`/ssh [user@]主機[:埠]`／`/telnet [主機[:埠]]`（不帶參數用上次主機；ssh 帶 user@ 直接連、否則走 login as: 回覆帳號）`/history`（只顯示最近 10 筆；`/history <n>` 才用該筆開新連線——**刻意不吃純數字回覆**，數字留給終端機輸入/選單應答；PickDir 者以桌面開）`/shot`（畫面截圖）純文字=打字+Enter `/key <ctrl-c|ctrl-d|esc|tab|enter|方向>` `/stop` `/last [n]` `/more`（上一則輸出往前翻頁，約 3000 字/頁；緩衝=瘦身後的最近 400 行）`/close [n]`（真正關閉分頁，走優雅結束、不跳 PC 確認框；無參數=關附著的）`/where` `/follow` `/notify` `/exit`（只離開檢視不關分頁；**附著後閒置 10 分鐘自動離開**，9 分鐘先警告，計時只算手機來訊）`/help`；啟動時自動 `setMyCommands` 註冊指令選單（手機有 Menu 鈕）。**附著分頁忙碌≥3秒轉閒 → 自動推最後 30 行輸出**（follow 開時）；其他分頁推一行閒置通知（notify 開時）。輸出來源=**xterm 渲染後文字**（`q…text` 協定），再經 `TidyForPhone` 過濾 TUI 雜訊行、取行數、3500 字截斷、`<pre>` 送出。**選擇題**：claude 跳選單時（轉閒推播會帶出題目+選項，`│` 框內的問句/選項行剝框保留），手機**直接回數字** → 遠端偵測畫面有「❯ N.」選單就自動換算成 ↑/↓ 導航+Enter（claude 選單不吃數字鍵，2026-07-27 實測傳 3 正確選中第 3 項）。**inline 按鈕（v0.9.82）**：`/list`（點按=goto）、`/new`、`/history` 清單附按鈕（每列 2 顆），選擇題推播偵測到選單自動附選項按鈕（callback `opt:{tabId}:{n}`、跨分頁拒答），`/close` 一律先出「✅ 確定關閉／✖ 取消」按鈕；long polling 認 `callback_query`（同樣驗 chat id）→ `answerCallbackQuery` 停轉圈，打字流程全數保留。
- **保持連線（v0.9.83~84）**：SSH/Telnet 連線視窗「保持連線」**下拉** 0/1/3/5/10/15/30/60 分鐘（`AppSettings.KeepAliveMins`，預設 10、記憶；0=關）。SSH=`ssh.exe -o ServerAliveInterval={分×60} -o ServerAliveCountMax=3`（`SshCommand()` 集中組指令，四個啟動點共用）；Telnet=`TelnetSession.KeepAliveMins` 計時器送 IAC NOP（**直接寫串流，勿走 `Write()`——它會把 0xFF 轉義成資料**）。
- **斷線自動重連（v0.9.82）**：SSH/Telnet/COM 連線視窗有「斷線自動重連」勾選（`AppSettings.AutoReconnect`，**預設不勾**（0.9.85 改）、記憶）。session Exited 且分頁還在（非使用者關閉）→ 依 `Restore` 重建 session（同分頁），退避 3,6,9…最多 30 秒，一收到輸出歸零；使用者打 exit 登出也會重連（要停就關分頁）。SSH 重連=重跑 ssh.exe（密碼會再問）。
- **終端機右鍵選單＋Ctrl+F 搜尋（v0.9.82）**：`ContextMenuRequested` 取代 Edge 預設選單 → 剪下(＝複製)/複製/純文字貼上/全選(`A` 協定)/搜尋(`F` 協定)。搜尋列是 **HTML 內的浮動列**（避開 airspace），Ctrl+F 由 `attachCustomKeyEventHandler` 攔截；vendor 無 search addon → 自製 buffer 掃描（translateToString 快篩＋命中行逐 cell 對映欄位處理中文寬字），Enter/Shift+Enter 上下一筆、Esc 關閉、命中行置中＋選取標示。

## 數位簽章 / 安裝檔
- **自簽章（本機用）**：`sign.ps1` build 後自動簽 exe（csproj `SignOutput` target；找不到憑證安靜略過）。憑證 `CN=AwayTerminal (awaysu)` 在 `CurrentUser\My`（含私鑰）。`trust-cert.ps1` 使用者跑一次加入本機信任。
- **安裝檔（Inno Setup）**：`installer/installer.iss`。流程：`dotnet publish -c Release -r win-x64 --self-contained` → 簽章發佈的 exe → 匯出 `AwayTerminal.cer`（公開）→ ISCC 編譯。安裝時（系統管理員）把憑證匯入「受信任的根 + 受信任的發行者」、必要時裝 WebView2（內含 bootstrapper）、建捷徑。
  - ISCC 路徑：`%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe`。
  - **限制**：自簽憑證只在「裝過此憑證」的電腦被信任；**安裝檔本身第一次執行仍會跳 SmartScreen「不明發行者」**（裝完後程式本體才不被擋）。要連安裝檔都免警告需付費 CA（OV/EV）憑證。

## 踩雷紀錄（重要）
- **WebView2 airspace**：WPF 內容**無法可靠疊在 WebView2 上**（複製提示 `CopyPopup` 才用 Popup）。故「快速輸入抽屜」做在終端機**右側欄**（黃框外）、不覆蓋畫面；小箭頭放在 root Grid 工具列右端。
- **COM 輸出慢**：`SerialPort.DataReceived` 有延遲 → 改**專用執行緒 blocking `BaseStream.Read`**（同 ConPTY 讀取迴圈），資料一到立即送畫面。勿改回 DataReceived。
- **WebView2 快取**：改了 `web/` 檔卻「看起來沒變」→ 已用 `WebResourceRequested` 自行伺服＋no-cache 解決；勿移除。
- **環境變數**：從 Claude Code 環境啟動會繼承 `NO_COLOR=1`（claude 全無色）等 → `App.OnStartup` 清除並設 `TERM=xterm-256color`、`COLORTERM=truecolor`。另設 `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`（claude 2.1.x 起 TUI 改用 alternate screen，scrollback 全失效、長回覆只剩最後一頁；此為官方 v2.1.132+ 的 opt-out，改回經典渲染器讓回覆留在 xterm scrollback）。
- **xterm 黑帶**：xterm.css 的 `.xterm-viewport` 預設純黑 → CSS `transparent !important` 蓋掉。
- **xterm `windowsPty` 不可加**：會造成 claude 輸入列殘字（此機 Win10 19045 ConPTY）。勿加回。
- **BEL→綠燈機制別加回**：曾造成卡橘燈與中文回顯破綻（注音「分段多空格」其實是它插隊，非輸入問題）。
- **關閉慢**：WPF 關 WebView2 teardown 慢 → `Environment.Exit(0)`；勿改回 `Close()`。
- **fit 截行**：padding 放在 **xterm 元素本身**（fit addon 會扣除）；放父層最後一行被截 1/3。
- **初始尺寸**：新 session 用 `_lastCols/_lastRows`，勿寫死 80×24。
- **控制字元常值**：`US = ''`、DEL 用 `''`——勿貼不可見原始字元（CS1011）。
- **WinForms**（ColorDialog/FolderBrowserDialog 用）：csproj `Using Remove` System.Windows.Forms/System.Drawing，該命名空間型別一律全名呼叫。
- **清除畫面**（PowerShell/SSH）：Esc → 延遲 ~60ms → Ctrl+L；黏著送會被 PSReadLine 當 escape 序列。
- **DPI 髮絲線**：Window `UseLayoutRounding` + `SnapsToDevicePixels` 已開。
- **深色對話框的 ComboBox 灰字**：視窗層級的灰字 `TextBlock` 隱式樣式會**滲入 ComboBox 模板**（TextBlock 非 Control、隱式樣式穿越 template 邊界），下拉白底配灰字看不清。解法=在 ComboBox 隱式樣式的 `Style.Resources` 放一個黑字 TextBlock 樣式蓋掉（ConnectDialog/ComDialog 已加）；新增深色對話框時記得照做。
- **防毒**：PC-cillin 誤判（ConPTY 開 shell）；專案資料夾已加例外。
- **`.ps1` 有中文必須 UTF-8 BOM**；`.iss` 有中文亦需 UTF-8 BOM（故 installer.iss 用英文）；`.reg` 有中文必須 UTF-16 LE。
- **憑證私鑰不進 git**（在 Windows 憑證存放區）；`settings.json` 在 `%LOCALAPPDATA%`（不在專案內）；`.gitignore` 排除 `bin/ obj/ installer/dist/ installer/MicrosoftEdgeWebview2Setup.exe`。
- **安裝版舊 exe 與開發版共用 settings.json 會「剝欄位」**：Program Files 的 v0.9.73 安裝版一存檔就把它不認識的新欄位（遠端 token/chatId/RemoteEnabled）整組洗掉——2026-07-27 中招，遠端靜默 3 小時才發現（診斷法：對 bot token 打 `getUpdates`，409=有人在 poll、200=服務沒起來）。對策（v0.9.82 起）：`AppSettings` 加 `[JsonExtensionData] ExtraFields` 保留未知欄位、`Save()` 改 tmp+`File.Move` 原子替換（防強殺留半截檔）、`Load()` 解析失敗先備份 `settings.json.bad` 再退預設。**在裝過安裝版的機器測新功能前，先確認沒誤開 Program Files 的舊版**；新功能穩定後盡快重出安裝檔。
- **遠端 /last 絕不能用「原始位元組流去 ANSI」**：claude 等 TUI 用游標定位原地重繪，去 ANSI 直接串接會把幾百次重繪黏成 `Inferring…Inferring…` 洪流；逐格差分重繪還會產生 `oing`/`✣ Bi` 碎片。正解=向 xterm 查渲染後文字（`q…text`→`a…text`，JS `lastPlainText()` 接回 isWrapped 邏輯行），xterm 已把重繪合成完畢。位元組流緩衝（`TerminalTab.RemoteRecent`）僅當 JS 逾時未回的備援。
- **claude inline 渲染器會在 scrollback 留孤兒行**（spinner 片段 `illo`/`· Bill`、歡迎框帶標題長行）——用 `TelegramRemote.NoiseLineRes` 規則逐行過濾；歡迎框上緣「邊線+標題」合併行目前仍會漏掉（待辦）。
- **Telegram bot 測試可全自動**：電腦版 Telegram 用 `tg://resolve?domain=<bot>` 直開對話；打字必用 **Set-Clipboard+Ctrl+V**（SendKeys 會被注音 IME 吃掉）、勿送 ESC（無組字時會關聊天室）；`SetForegroundWindow` 被前景鎖擋時先 `keybd_event` 按一下 ALT。

## 待辦（需使用者輸入）
1. COM「清畫面時送 reset」要送什麼（`SerialSession.SendReset()` 仍為空）。
2. ~~巨集 `elseif`/`break`/`continue`~~ **已做（v0.9.81）**：if..then/elseif/else/endif 分支鏈（`_branchNext`/`_toEndif`）＋ break/continue（`_loopOwner` 取最近一層 for/while；只支援獨立成行，單行式 `if x break` 不支援）。檔案操作/檔案傳輸仍未做，依實際 `.ttl` 需求再擴充。

## 待辦（遠端 Telegram，接續）
1. ~~過濾殘留~~ **已驗收完成（2026-07-27 情境1 tt1 網頁貪食蛇 + 情境2 tt2 C++ 貪食蛇+MSVC 編譯，端到端全通過）**：主回覆完全乾淨。過濾手段=`NoiseLineRes` 規則行過濾（spinner/狀態列/方框/碎片）+ **前綴去重**（空白正規化後某行是較長行前綴 = TUI 截斷殘影）。若日後 claude 換 spinner 字形/statusline 格式再漏，補 `TelegramRemote.NoiseLineRes` 即可。
2. 手機端 adb 截圖驗證：手機有指紋鎖，adb 無法解鎖，**待使用者解鎖後補驗**（`tools\adb\adb.exe shell screencap -p /sdcard/x.png` + pull；`exec-out >` 會被 PowerShell 弄壞二進位）。
3. ~~`/new` / `setMyCommands` / `/shot`~~ **已完成並實測（2026-07-27）**：`/new` v0.9.77 改版=**每種連線只列一個**（不列 History；PowerShell/PickDir 自訂以桌面為工作目錄、不跳資料夾框）、開完自動附著、非 SSH 且 follow 開時 1.5s 後自動推開場畫面；`setMyCommands`=服務啟動時自動註冊（手機出現 Menu 鈕）；`/shot`=WebView2 `CapturePreviewAsync` PNG + sendPhoto（附著分頁未顯示會先切前景）。
3a. **SSH/Telnet 遠端登入（v0.9.77~78）**：`/new` 選 SSH → 開「login as:」分頁並提示回覆帳號；`SendInputToTab` 在 `Session==null && LoginBuffer!=null` 時轉 `HandleLoginInput`（修掉遠端打不進 login as: 的問題），密碼走一般文字輸入（送出後 0.7s 自動回傳畫面）。Telnet 直接開，登入提示由開場畫面推播帶出。0.9.78 加專屬指令 `/ssh [user@]主機[:埠]`、`/telnet [主機[:埠]]`（`ParseHostPort` 解析；不帶參數用 `LastHost`＋`LastSshPort`/`LastTelnetPort`；ssh 帶 user@ 走直啟路徑 `IRemoteHost.OpenSsh`、跳過 login as:）。**尚未實機驗**（需使用者手機測 SSH 帳密流程）。
4. ~~`/close`、`/more`、10 分鐘閒置自動 `/exit`~~ **已做（v0.9.81，未實測）**。規格內尚未做：`/clear`、選配 `/arm` PIN、非附著分頁選擇題通知帶內容。
5. 驗完 commit（v0.9.74~82 遠端功能一整包，含 `q…text` 協定）。
6. **2026-07-27 自動化測試已過**：MacroRunner 單元測試 18/18（elseif/break/continue/巢狀/回歸＋ParseHostPort/MenuOptions/TidyForPhone，散彈槍在 scratchpad mrtest 專案）；Telnet 自動重連 e2e（本機假伺服器，斷線後 3s→6s 退避重連實測）；Telegram e2e（/help、/new inline 按鈕點擊開 PowerShell(桌面)+子行程驗證、文字指令回傳、/history 按鈕、/close 確認按鈕→實關）；UI 截圖驗證（右鍵五項選單、Ctrl+F 搜尋 1/2→2/2→Esc）。**未測**：SSH 帳密實機（需真帳密）、/more 長輸出翻頁、COM 重連（無裝置）、10 分鐘閒置（太久）、選擇題按鈕（需 claude 跳選單）。
