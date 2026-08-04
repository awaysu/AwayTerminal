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
- C#→JS：`o{id}{US}{base64}`(輸出)、`n{id}{US}{title}[{US}{flags}]`(建立；flags `c`=claude 貼上走 ESC+CR)、`t{id}{US}{title}`(改名)、
  `s{id}`/`x{id}`/`c{id}`/`f{id}`(顯示/關閉/清除/聚焦)、`L{tab|split|columns}`(模式)、
  `q{id}{US}{sel|selpaste|all|text|file}`(查詢文字；selpaste=同 sel 回選取文字、C# 收到後再貼回原分頁；text=遠端用、渲染後純文字最後 400 行；file=整個 buffer 純文字、回覆後存檔)、`T{json}`(全域字型/顏色)、`P{id}{US}{fg}{US}{bg}`(單一分頁配色；空=回設定預設)、`A{id}`(全選)、`F`(開 Ctrl+F 搜尋列)、`v{id}{US}{base64}`(貼上→JS `doPaste`：一般=`term.paste()`、claude=ESC+CR)
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
| `LICENSE` / `THIRD-PARTY-NOTICES.md` | MIT（`Copyright (c) 2026 Awaysu`）＋第三方聲明。**由 csproj 的 `Content` 複製進發佈輸出**，安裝檔與 MSIX 兩邊自動含入（MIT 要求「所有副本」都附聲明，只放 repo 不算數）|
| `installer/` | Inno Setup 安裝腳本 `installer.iss` + 公開憑證 `AwayTerminal.cer` + 使用者選擇性信任腳本 `trust-publisher.ps1`（見「數位簽章 / 安裝檔」）|
| `msix/` | MSIX 打包（上架 Microsoft Store 用）：`AppxManifest.xml` + `build-msix.ps1` + `make-images.ps1` + `Images/`；`layout/` `out/` 為產出，已 gitignore |
| `app.ico` | 由 `icon/app-icon.png` 產生（PNG 內嵌 ICO）|

## 慣例
- **視窗標題**：`AwayTerminal - 目前路徑`（v1.0.6，1.0.9 擴充）。路徑由**提示字元行解析**（`q…cwd` 查游標所在提示行 → `CwdRes` 六組 regex：PowerShell / cmd / bash-zsh / RHEL `[user@host ~]#` / Android adb `host:/path $` / fish `user@host /path>`），狀態輪詢每 0.6s 更新；解析不到就保留上次值（打字中不閃動）。**沒有提示行的分頁（claude/自訂）用 `TerminalTab.WorkDir`（啟動目錄）墊底**，切分頁以 WorkDir 起始、無分頁時只顯示程式名。**註：不能讀行程 PEB 的 CWD**——PowerShell 的 `Set-Location` 不會同步行程工作目錄，讀到的永遠是啟動目錄。
- **版本**：每交付一次 `<Version>` +0.0.1（「關於」顯示）。目前 **1.0.11**（右鍵「複製且貼上」；1.0.10=claude 分頁貼上改 ESC+CR）（1.0.0=2026-07-27 里程碑、repo 重建＋安裝檔發佈；1.0.4 起「關於」email=weisu.tech@gmail.com、仍為圖片渲染；1.0.5 起最下方顯示「編譯時間: yyyy/M/d HH:mm:ss」＝exe 檔案寫入時間，複製/安裝會保留時間戳）。「關於」是自訂深色小視窗（非 MessageBox）、內容置左：標題/版本、作者行 `Awaysu (awaysu@gmail.com)` **以執行期渲染的圖片顯示**（`RenderTextImage`，依 DPI 畫、防文字收集）、Source Code 可點連結開 GitHub。
- **i18n**：使用者可見字串走 `Loc.T`；工具列文字：新連接(New) / 紀錄 / 複製 / 純文字貼上 / 複製全部 / 清除畫面 / 視窗分割 / 功能列 / 常用字串 / 遠端設定 / 設定 / 關於。
- **設定持久化**：記住值都進 `AppSettings.Current` + `.Save()`。
- **分頁命名**：`NextName(prefix)` → 「型態(數字)」，例 PowerShell(1)、ADB(1)、自訂用「名稱(數字)」；跳過已存在名稱。
- **狀態圓點**（分頁左側）：綠=可輸入、橘=忙，純活動偵測（已移除 BEL 那套）。**工作列 icon 右下**另有 overlay 圓點：全部閒置=綠、有忙=橘、無分頁=無（`TaskbarItemInfo.Overlay`）。
- **強調色**：`#FDFFB0` 淡黃（作用中分頁框、終端機外框、分割 pane 框、快速輸入面板框）。作用中分頁上方開口與外框融合（tab 往上 -1px 蓋線）。

## 主要功能行為
- **New（新連接）下拉**（工具列最左）：PowerShell → SSH/Telnet → 連接埠 → **分隔線** → ADB → 你的自訂連線（ClaudeCode 等）→ 分隔線 → 「自訂…」(開管理視窗)。項目為圖示+文字。
  - **分組原則（v1.0.15 定）**：第一區＝**AwayTerminal 自己實作**的連線（ConPTY／Telnet／序列埠，不依賴外部程式）；第二區＝**依賴機器上已安裝的外部程式**。ADB 自 1.0.13 起不再內建 adb.exe，性質與自訂連線相同，故移入第二區。
  - **ADB 刻意不轉成 `CustomConn`**（與 ClaudeCode 的作法不同）：一般自訂連線只會執行「exe + 參數」，轉過去會失去 `adb devices` 偵測與多裝置選擇，故保留專屬的 `OpenAdb_Click`，只改選單位置。
- **Claude Code**：不再是內建按鈕，已**遷移成一筆自訂連線「ClaudeCode」**（`EnsureDefaults` 只做一次、以 `ClaudeMigratedToCustom` 旗標記住）。**直接以 ConPTY 執行 claude.exe（不經 PowerShell）**：claude 即主行程、一開始就用目前尺寸建立，**已移除舊的「等尺寸才送指令」hack**；勾「使用 PowerShell」或路徑是 .cmd/.bat（npm 版）時才走 PowerShell。
- **自訂連線**（`CustomConnDialog`）：左清單（可收合）+ 底部「自動偵測 / ＋新增 / －刪除」+ 右編輯（圖示下拉 32×32 無字 / 名稱 / 執行檔+瀏覽 / 參數 / 關閉按鍵〔無·Ctrl+C·Ctrl+D〕×〔x1~x5〕/ 隱藏 / 啟動前選擇資料夾 / 使用 PowerShell）+ 「儲存 / 返回」。髒資料追蹤、切換/關閉/刪除前提示。**自動偵測**：在 PATH 與常見位置找 claude/wsl/opencode/gemini/aider 加入。「隱藏」= 不列在 New 下拉。自訂分頁**不做開機還原**。
- **ADB**：**v1.0.13 起不再內建 adb**（授權原因見下），改用 `AppSettings.ResolveAdbPath()` 搜尋使用者既有安裝——順序：設定裡指定的 `AdbPath` → `PATH` → `ANDROID_HOME`/`ANDROID_SDK_ROOT` → Android Studio 預設位置 → **舊版殘留的 `tools\adb\adb.exe`**（1.0.12 以前裝過的機器照樣能用，升級不會突然壞掉）。找不到 → `PromptInstallAdb()` 說明並詢問是否開啟官方下載頁。**設定視窗有「ADB / adb 路徑」欄位**（v1.0.14 補；`SettingsDialog` 第三個 GroupBox，含瀏覽鈕），**留空＝自動偵測**，提示行會即時顯示目前實際會用到哪一支 adb（或找不到），讓「留空」不是黑箱。找到後行為不變：先 `adb devices`，0 台提示、1 台直接開、2 台以上跳選單，再以 ConPTY 跑 `adb shell`。
  - **為何移除**：Android SDK 條款 §3.4 禁止轉散布 SDK，與 §3.5「另有授權的開源元件」的界線不明確；上架 Store 需對散布內容擁有明確權利。且會用 ADB 的人幾乎都已安裝 platform-tools，為少數人背 8.5MB 與法律模糊地帶不划算。**另有一個確定的缺失**：原本只複製了 4 個二進位檔，沒有帶 Google platform-tools 內的 `NOTICE.txt`，那是 Apache-2.0 §4(d) 明文要求的——不散布之後這個義務一併消失。
  - **踩雷**：`CopyToOutputDirectory` **不會刪除已從專案移除的檔案**，`bin\publish` 會留著上一次的 `tools\adb\`。移除打包內容後必須**先刪 `bin\publish` 再重跑 publish**，否則安裝檔照樣把 adb 包進去（2026-08-04 實際踩到）。
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
- **遠端控制（Telegram，v0.9.74~76 新增）**：一台 PC 一個 bot（@BotFather 申請 token），`RemoteDialog` 設定 Bot Token / 允許的 Chat ID（一鍵抓）/ 推播開關（`AppSettings.RemoteEnabled/TelegramBotToken/TelegramChatId/RemoteNotify`）。**多開防護（v1.0.1，雙實例實測過）**：具名 Mutex `Local\AwayTerminal.TelegramRemote` 保證同機只有第一個實例啟動遠端（同 token 兩個 long polling 會 409+搶訊息）；其餘實例跳過並在遠端設定顯示橘字提示，關掉持有者後回來按「儲存」即可接手（AbandonedMutex 也視為取得）。**一個 token 只能一台裝置 poll**：第二台電腦要另申請 bot（chat id 可同一個）。`TelegramRemote` 自寫 HttpClient long polling（無第三方套件），啟動時 PrimeOffset 跳過舊訊息、只認設定的 chat_id。指令：`/list` `/goto <n>` `/new`（**每種連線只列一個**：PowerShell(桌面)/SSH/Telnet/COM/ADB/未隱藏自訂各一，**不列 History**；回數字開啟+自動附著；PowerShell 與 PickDir 自訂一律以桌面為工作目錄）`/ssh [user@]主機[:埠]`／`/telnet [主機[:埠]]`（不帶參數用上次主機；ssh 帶 user@ 直接連、否則走 login as: 回覆帳號）`/history`（只顯示最近 10 筆；`/history <n>` 才用該筆開新連線——**刻意不吃純數字回覆**，數字留給終端機輸入/選單應答；PickDir 者以桌面開）`/shot`（畫面截圖）純文字=打字+Enter `/key <ctrl-c|ctrl-d|esc|tab|enter|方向>` `/stop` `/last [n]` `/more`（上一則輸出往前翻頁，約 3000 字/頁；緩衝=瘦身後的最近 400 行）`/close [n]`（真正關閉分頁，走優雅結束、不跳 PC 確認框；無參數=關附著的）`/where` `/follow` `/notify` `/exit`（只離開檢視不關分頁；**附著後閒置 10 分鐘自動離開**，9 分鐘先警告，計時只算手機來訊）`/help`；啟動時自動 `setMyCommands` 註冊指令選單（手機有 Menu 鈕）。**follow/完成推播＝只推新輸出（v1.0.3，e2e 驗證過）**：`_baseline` 記每分頁上次推播（或 /goto 附著）當下的原始渲染文字，`DiffNew` 用「基準尾端連續 3 行錨點」（前幾行完整比對＋最後一行前綴比對，prompt 打字後原行變長仍能對上；錨點必須連續取、含中間空行）找出新增行；比對不到（清屏/TUI 大改）自動退回「最後 n 行快照」，`/last` 指令固定快照語意。同分頁連打兩個指令，第二次推播不再包含第一次輸出。其他分頁推一行閒置通知（notify 開時）。輸出來源=**xterm 渲染後文字**（`q…text` 協定），再經 `TidyForPhone` 過濾 TUI 雜訊行、取行數、3500 字截斷、`<pre>` 送出。**選擇題**：claude 跳選單時（轉閒推播會帶出題目+選項，`│` 框內的問句/選項行剝框保留），手機**直接回數字** → 遠端偵測畫面有「❯ N.」選單就自動換算成 ↑/↓ 導航+Enter（claude 選單不吃數字鍵，2026-07-27 實測傳 3 正確選中第 3 項）。**inline 按鈕（v0.9.82）**：`/list`（點按=goto）、`/new`、`/history` 清單附按鈕（每列 2 顆），選擇題推播偵測到選單自動附選項按鈕（callback `opt:{tabId}:{n}`、跨分頁拒答），`/close` 一律先出「✅ 確定關閉／✖ 取消」按鈕；long polling 認 `callback_query`（同樣驗 chat id）→ `answerCallbackQuery` 停轉圈，打字流程全數保留。
- **推播噪音修正（v1.0.13）**：使用者回報「在輸入框打字也會推播到手機」。根因＝**打字的按鍵回顯會一直更新 `LastOutputUtc`**，所以打一句話（很容易超過原本的 3 秒忙碌門檻）整段都被判定為忙，停手後 0.5~1.5s 翻閒 → 觸發「完成」推播。修法＝`TerminalTab.LastInputUtc`（只在 JS 的 `i` 協定更新，即真正的鍵盤／貼上），轉閒時若距最後一次按鍵 < 2.5s 就視為打字回顯、不推播；真正的工作是「送出指令後輸出持續數秒」，最後一次按鍵離轉閒必然超過這個距離。**遠端自己送的指令走 `SendInputToTab`、不經 `i` 協定**，所以從手機下指令仍會正常回推結果。第二個噪音來源＝`SendLast` 在增量模式算不出新輸出時仍會送出「只有標題沒有內容」的空訊息 → 新增 `auto` 參數，自動推播（僅 `OnTabIdle`）無新輸出時整則不送；使用者主動要的（/last、送鍵後回傳、開連線推開場）仍會回一句，否則看起來像壞了。
- **保持連線（v0.9.83~84）**：SSH/Telnet 連線視窗「保持連線」**下拉** 0/1/3/5/10/15/30/60 分鐘（`AppSettings.KeepAliveMins`，預設 10、記憶；0=關）。SSH=`ssh.exe -o ServerAliveInterval={分×60} -o ServerAliveCountMax=3`（`SshCommand()` 集中組指令，四個啟動點共用）；Telnet=`TelnetSession.KeepAliveMins` 計時器送 IAC NOP（**直接寫串流，勿走 `Write()`——它會把 0xFF 轉義成資料**）。
- **斷線自動重連（v0.9.82）**：SSH/Telnet/COM 連線視窗有「斷線自動重連」勾選（`AppSettings.AutoReconnect`，**預設不勾**（0.9.85 改）、記憶）。session Exited 且分頁還在（非使用者關閉）→ 依 `Restore` 重建 session（同分頁），退避 3,6,9…最多 30 秒，一收到輸出歸零；使用者打 exit 登出也會重連（要停就關分頁）。SSH 重連=重跑 ssh.exe（密碼會再問）。
- **終端機右鍵選單＋Ctrl+F 搜尋（v0.9.82，1.0.2 改版）**：`ContextMenuRequested` 取代 Edge 預設選單 → **貼上/複製/複製且貼上/複製全部/複製全部存至檔案/分隔線/搜尋**（1.0.11 於「複製」下加入「複製且貼上」＝選取文字進剪貼簿後直接貼回原分頁，走 `q…selpaste`；`q…file` 協定=整個 buffer 純文字→`SaveBufferToFile` 存檔對話框；工具列該鈕仍叫「純文字貼上」、右鍵選單叫「貼上」，行為相同；JS 端 `A`(全選) 協定保留未用）。搜尋列是 **HTML 內的浮動列**（避開 airspace），Ctrl+F 由 `attachCustomKeyEventHandler` 攔截；vendor 無 search addon → 自製 buffer 掃描（translateToString 快篩＋命中行逐 cell 對映欄位處理中文寬字），Enter/Shift+Enter 上下一筆、Esc 關閉、命中行置中＋選取標示。

## 數位簽章 / 安裝檔
- **自簽章（本機用）**：`sign.ps1` build 後自動簽 exe（csproj `SignOutput` target；找不到憑證安靜略過）。憑證 `CN=AwayTerminal (awaysu)` 在 `CurrentUser\My`（含私鑰）。目前指紋 `7B11D2A5062A07C5BDD440A8C6B34FD3D7E5719D`（2026-07-21 ~ 2031-07-21）。
- **私鑰刻意不做任何備份**（2026-08-03 決定）：**不進 git、不放雲端**。理由：(1) 推上 git 不可逆——歷史永久保存、GitHub 未被引用的物件無法可靠刪除；(2) private repo 防的是路人，防不了真正的威脅——本機 gh token（repo 權限、存在 keyring）本身就能 clone，等於在憑證存放區之外多一份可被同一個威脅取得的副本；(3) 這把金鑰是 `AllowPlaintextExport`（可明文匯出），且 1.0.11 以前的安裝檔曾把它塞進使用者的**機器層級**受信任根，一旦外流，用它簽的惡意程式在每台跑過安裝檔的電腦上都自動受信任；(4) 產業方向相反——CA/B Forum 自 2023-06 起要求簽章私鑰must放在 FIPS 認證硬體。
  - **遺失時的重建程序**（成本近乎零，故不需備份）：① 跑 `trust-cert.ps1`（找不到憑證時會自動 `New-SelfSignedCertificate` 建新的）② 重新 build/publish 讓 `sign.ps1` 以新憑證簽章 ③ **重新匯出 `installer/AwayTerminal.cer`**（安裝檔流程裡那一步）④ 更新本檔與下載頁公布的指紋 ⑤ 通知已執行過 `trust-publisher.ps1` 的使用者重跑一次（舊憑證的信任不會自動轉移）。
  - **注意**：舊憑證簽過的已發佈安裝檔不受影響（有 DigiCert 時間戳，簽章仍有效），重建只影響之後的版本。**`trust-cert.ps1` 是開發用**（找不到憑證會**建立新的自簽憑證**，絕不可給使用者跑）；給使用者的是 `installer/trust-publisher.ps1`（只匯入公開 .cer、只寫 `CurrentUser`、支援 `-Remove`、會印指紋供核對）。
- **⚠️ publish 出來的 exe 不會自動簽章**：`SignOutput` target 掛在 `AfterTargets="Build"`，`dotnet publish` 會覆蓋掉已簽的檔案 → 出安裝檔時必須**手動**對 `bin\publish\AwayTerminal.exe` 與編譯完的 `AwayTerminal-Setup-*.exe` 各跑一次 `sign.ps1`。
- **兩條產線，發佈方式不同（v1.0.13 起）**——**目錄不可共用**，否則後跑的會覆蓋前者（`CopyToOutputDirectory` 不刪舊檔，正是 adb 那次踩到的陷阱）：
  | 通路 | 發佈方式 | 輸出目錄 | 大小 |
  |---|---|---|---|
  | Inno 安裝檔 | `--self-contained false`（框架相依） | `bin\publish` | 24 檔 / 3.2 MB |
  | MSIX / Store | `--self-contained true`（自帶） | `bin\publish-msix` | 488 檔 / 171.6 MB |
  - **Store 版必須自帶**：MSIX 無法像 Inno 那樣偵測並安裝 .NET Desktop Runtime，Store 也沒有桌面 .NET 的 framework package。
  - **安裝檔內含 .NET 9 Desktop Runtime 安裝程式**（`installer/windowsdesktop-runtime-win-x64.exe`，58MB，已 gitignore；`DesktopRuntimeMissing` 為真才執行）。**偵測用資料夾掃描不用註冊表**——`HKLM\SOFTWARE\dotnet\Setup\InstalledVersions\x64\sharedfx` 在 Win10 19045 實測**不存在**（即使 runtime 已裝），改掃 `{commonpf64}\dotnet\shared\Microsoft.WindowsDesktop.App\9.*`。
  - **安裝檔反而變大**：61.8MB（自帶版是 55.6MB）——runtime 安裝程式本身已壓縮、Inno 壓不動，而自帶的 172MB 散檔能壓到 55MB。換到的是**安裝後磁碟 184.8MB → 7.4MB**，且 runtime 由微軟經 Windows Update 修補（自帶＝.NET 安全更新變成你的責任）。
  - **`[InstallDelete] Type: filesandordirs; Name: "{app}\*"` 不可移除**：Inno 就地升級**不會刪除舊版裝過、新版已不含的檔案**。沒有這段，從 ≤1.0.12 升級會留下 ~172MB 孤兒 runtime 檔＋`tools\adb`，框架相依省的空間完全沒實現（實測升級後仍是 184.8MB，加了之後降到 7.4MB）。{app} 內無使用者資料（設定在 `%LOCALAPPDATA%`、log 在文件夾），清空安全。
- **安裝檔（Inno Setup）**：`installer/installer.iss`。流程：`dotnet publish -c Release -r win-x64 --self-contained false` → 簽章發佈的 exe → 匯出 `AwayTerminal.cer`（公開）→ ISCC 編譯 → **再簽 setup exe**。安裝時必要時裝 WebView2（內含 bootstrapper）、建捷徑。
  - **v1.0.12 起安裝檔不再碰憑證存放區**（原本 `[Run]` 有兩行 `certutil -addstore -f Root/TrustedPublisher`，`runhidden` + 管理員權限）。移除原因：這個組合等同 **MITRE ATT&CK T1553.004（Install Root Certificate）**，MITRE 頁面舉的惡意程式範例指令幾乎一模一樣，`certutil` 本身又是知名 LOLBin，**極可能是 PC-cillin 誤判的主因**；且它要求每位使用者信任一張私鑰放在開發機上的根憑證（Superfish／eDellRoot 前例）。改為 `AwayTerminal.cer` + `trust-publisher.ps1` 裝進 `{app}`，**使用者自行決定**是否執行。
  - **兩者刻意分屬不同存放區、不會互相干擾**：舊的壞行為寫 `LocalMachine`（全機器、需管理員），新的 opt-in 寫 `CurrentUser`（僅自己、免管理員）。`[Run]` 保留兩行 `-delstore`**只清 LocalMachine**，讓 1.0.11 以前裝過的機器升級時自動清乾淨，不會洗掉使用者自己的選擇。
  - **踩雷**：`Cert:\CurrentUser\Root` 的檢視是「使用者 ∪ 機器」存放區的**聯集**，直接 `Remove-Item` 會刪到機器層級那張並拿到 Access denied（`trust-publisher.ps1 -Remove` 已逐張 try/catch 並分開提示）。
  - ISCC 路徑：`%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe`。
  - **限制（2026-08 查證後大幅修正）**：使用者遇到的其實是**三個獨立機制**——瀏覽器下載警告、SmartScreen 執行警告（前兩者只看**信譽**）、UAC「不明發行者」（只看**簽章身分**）。**憑證只解決第三個。**
    - **「買 OV/EV 就免警告」已不成立**：微軟 2024 年移除了 EV 的 SmartScreen 即時信譽特權（[code-signing-options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)：「EV certificates previously bypassed SmartScreen entirely on first download... That behavior was removed in 2024」），現在 OV 與 EV 同級，**不要買 EV**。SmartScreen **沒有白名單管道**（微軟明文 "no need (or mechanism)"），只能靠下載量累積。
    - **自簽在 SmartScreen 眼中等同未簽**，且信譽**無法跨版本累積**（每個新 hash 從零開始）——四天發十二版的節奏在數學上永遠累積不到。憑證的真正價值是讓信譽掛在同一個發行者身分下累積。
    - **Smart App Control（Win11 22H2+ 全新安裝）直接封鎖**自簽／未簽執行檔，且不限有 MOTW 的檔案，只認受信任根計畫內 CA 簽發的憑證。
    - **選項成本**：個人買不到 OV（需法人），對應產品是 **IV**。台灣可行的最便宜是 Certum 開源憑證雲端版 **€49/年**（免硬體 token）；SSL.com IV $129/年＋eSigner $20/月。**Azure Artifact Signing（$9.99/月）個人限美加，台灣不適用**。SignPath Foundation 對開源免費但憑證掛他們的名字，且要求「可驗證地從原始碼建置」——**原本 `tools/adb/` 的 Google 預編譯檔是疑慮點，v1.0.13 移除後此障礙已消失**（加上 1.0.13 已補 MIT LICENSE，SignPath 的兩項前置條件都滿足了）。**Microsoft Store 是唯一真正零警告**的通路（個人註冊費已取消），但要改包 MSIX。
    - **winget 有效但只限該通路**：社群來源被標記為 Trusted，下載的檔案拿到 `ZoneId=2` 而非 `ZoneId=3`（本機實測），SmartScreen 因此不觸發；但從 GitHub Releases 直載的人不受惠，SAC 與防毒也不受影響。註：本機用 `winget install --manifest` 測試走的是**非信任路徑**，會看到真實使用者不會遇到的警告（別被誤導）。

## MSIX / Microsoft Store（v1.0.12 起）
- **打包**：`msix\build-msix.ps1`（`-Publish` 重跑 publish、`-NoSign` 產生未簽章套件供上架）。產出 `msix\out\AwayTerminal-<四段版本>.msix`，約 76MB（MSIX 壓縮比 Inno 好，安裝檔是 55.6MB 但解開後 180MB）。圖示由 `make-images.ps1` 從 `icon\app-icon.png` 產生 11 種尺寸。
- **必須是全信任桌面應用**：`EntryPoint="Windows.FullTrustApplication"` + `rescap:runFullTrust`。**絕不可做成 UWP/AppContainer**——子行程會繼承容器，ConPTY 開的 powershell/ssh/adb 全部失去檔案系統與網路存取，程式等於報廢。
- **兩大風險已實測排除（2026-08-03，Win10 19045）**：
  1. **ConPTY 在 `C:\Program Files\WindowsApps\...` 長路徑下正常**——側載後子行程樹確實出現 `conhost.exe` + `powershell.exe`（曾擔心 terminal#16860 的 `CreatePseudoConsole` MAX_PATH 崩潰，未重現）。
  2. **內建 `adb.exe` 可執行**（WindowsAppSDK #4651 的 ACCESS_DENIED 未重現）。注意：**外部行程跑 WindowsApps 裡的 adb 會 Access denied**（ACL 只允許套件自己），這是正常的、不是 bug；要驗證得用 `Invoke-CommandInDesktopPackage -PackageFamilyName ... -AppId AwayTerminal` 以套件身分執行。
- **設定不會沿用（重要）**：封裝後 `%LOCALAPPDATA%` 被重導到 `%LOCALAPPDATA%\Packages\<PFN>\LocalCache\Local\AwayTerminal\settings.json`，**沒有 read-through 回退**——實測封裝版讀不到原本的 3 筆自訂連線／16 筆歷史／Telegram token，等於全新設定。要沿用需宣告 `rescap:unvirtualizedResources`（受限能力，上架要向微軟說明理由；Windows Terminal 就是這樣做的）或在程式內做一次性匯入。
- **上架前置（需使用者操作）**：① 到 **`storedeveloper.microsoft.com`** 註冊個人帳號（費用已取消；從 Partner Center 或 VS 進去會走到舊的 19 美元流程）② 保留應用程式名稱 → 取得 `Identity Name` 與 `Publisher=CN=<GUID>`，同步改進 `AppxManifest.xml` ③ 用 `-NoSign` 產生套件（微軟會以你的發行者身分重簽）④ **隱私權政策是硬性要求**（政策 10.5.1，桌面橋接產品必備）⑤ **Telegram 遠端控制是最大的審查風險**（長輪詢外部伺服器＋執行指令＋截圖，形同 RAT），建議預設關閉並在提交表單誠實說明。
- **簽章與 Identity 必須完全一致**：`AppxManifest.xml` 的 `Publisher` 與簽章憑證主體不符時，makeappx 打包會過但安裝會被判 identity 不符而失敗（`build-msix.ps1` 已在簽章前先擋下並印出兩者差異）。

## 踩雷紀錄（重要）
- **多行貼上必須走 `xterm.paste()`（v1.0.7 修）**：舊版把剪貼簿文字直接 `Session.WriteText`，每個換行都被當 Enter 送出 → 前面幾行被執行掉、**只剩最後一行留在輸入框**（claude 尤其明顯）。正解＝新增 `v{id}{US}{base64}` 協定交給 JS 呼叫 `term.paste()`，由它負責 `\r\n`→`\r` 正規化，並在程式啟用 bracketed paste（DECSET 2004）時包上 `ESC[200~/201~`；之後照常經 `onData`→`i…` 回送。工具列「純文字貼上」、右鍵「貼上」、常用字串抽屜、PromptDialog 全部走 `PasteToActive()`。註：Windows PowerShell 5.1 內建的 PSReadLine 較舊、未必啟用 bracketed paste，該情況下多行仍會逐行執行（Windows Terminal 亦同）；vim/bash 則正常。
- **claude 分頁多行貼上「有時候分開貼上」（v1.0.10 修）**：實測（ConPTY harness + stdindump/claude 2.1.220）**Win10 19045 conhost 會把輸入流的 `ESC[200~`/`ESC[201~` 整組丟棄**（位元組數正好少 12；win32-input-mode 序列同樣被吃），claude 收不到 bracketed paste 標記、只能靠「輸入叢發時序」猜；而 conhost 轉譯分塊時序不穩（第一行永遠先單獨到、其餘隔 10~175ms、14KB 拆 7 塊）→ 4KB 以上必分裂（碎片留輸入框+「paste again to expand」）、小貼上看時機。正解＝claude 分頁貼上**不走 bracketed paste，改把每個換行送成 `ESC+CR`**（claude 的 Shift+Enter 軟換行鍵，實測可完整穿透 ConPTY、200 行不裂不誤送）：`n` 協定第三欄 flags `c`（AddTab `claudePaste`；Kind=Claude 自動、Custom/PowerShell 依 `IsClaudeExe` 檔名含 claude），JS `doPaste` 統一入口（`v` 協定與 pane 上 capture 階段攔截的原生 Ctrl+V 都走它）。Win11 conhost 若不吃標記也不受影響（claude 分頁一律 ESC+CR，跨版本行為一致）。測試工具在 scratchpad `pastetest/`（pastehost+stdindump，防毒會擋 Start-Process → 用 .NET Process API + CreateNoWindow）。
- **注音組字閃英文字（v1.0.8 修，只動顯示層）**：微軟注音每鍵先回報原始鍵值（h=ㄏ）再更新成注音，xterm 的 `.composition-view` 忠實畫出就閃英文字。修法＝`MutationObserver` 監看該元素：內容一變先藏 30ms、含英文字母（＝鍵值殘影）持續隱藏。**不碰輸入流**（BEL 那次的教訓）；若日後改用拼音輸入法需拿掉字母過濾（拼音組字本來就是英文字母）。
- **UI 自動化測試的座標會漂移**：`ui.ps1` 用**螢幕絕對座標**，必須每次先截圖取 `GetWindowRect` 原點、再把影像座標加上原點；沿用上一張截圖的座標會誤點到標題列的最小化/關閉鈕（2026-07-30 就這樣誤觸關閉→ExitDialog 被確認→程式結束，一度誤判成防毒問題）。**使用者正在操作電腦時不要跑侵入式點擊**（視窗會被移動/最小化，兩邊互相干擾）。判斷是否為誤觸：`settings.json` 寫入時間若剛好早於程式結束 1 秒＝走了正常關閉流程，不是當機。
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
- **`.ps1` 有中文必須 UTF-8 BOM**；`.iss` 有中文亦需 UTF-8 BOM（故 installer.iss **一律用英文**，檔頭已註明）；`.reg` 有中文必須 UTF-16 LE。**2026-08-04 又踩一次**：在 installer.iss 加了中文註解後用 `Set-Content -Encoding utf8` 改版號，註解變亂碼並與程式行黏在一起（`Source:` 被吃進註解、`function` 宣告被吃掉），ISCC 報 line 106 錯誤。改動 .iss 請用 Edit 工具逐段改，勿整檔重新編碼。
- **`Get-AuthenticodeSignature` 回 `UnknownError` 不一定是壞事**：訊息若為「terminated in a root certificate which is not trusted」，代表自簽根憑證不在信任存放區——**1.0.12 起安裝檔會主動移除機器層級根憑證，所以這是預期結果**，程式照樣能執行。別誤判成防毒攔截或簽章失敗（2026-08-04 一度誤判）。
- **`[Diagnostics.Process]::Start` 在此環境啟動 Program Files 的 exe 會回 "Access is denied"**，但同一支程式用 Bash 工具 `./AwayTerminal.exe &` 或 `cmd /c` 都能正常啟動。這是工具環境的怪癖，不是防毒也不是程式問題——驗證安裝版時請用 Bash 啟動。
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
2. 手機端 adb 截圖驗證：手機有指紋鎖，adb 無法解鎖，**待使用者解鎖後補驗**（`adb shell screencap -p /sdcard/x.png` + pull；`exec-out >` 會被 PowerShell 弄壞二進位。註：1.0.13 起已不內建 adb，測試請用系統上的 platform-tools）。
3. ~~`/new` / `setMyCommands` / `/shot`~~ **已完成並實測（2026-07-27）**：`/new` v0.9.77 改版=**每種連線只列一個**（不列 History；PowerShell/PickDir 自訂以桌面為工作目錄、不跳資料夾框）、開完自動附著、非 SSH 且 follow 開時 1.5s 後自動推開場畫面；`setMyCommands`=服務啟動時自動註冊（手機出現 Menu 鈕）；`/shot`=WebView2 `CapturePreviewAsync` PNG + sendPhoto（附著分頁未顯示會先切前景）。
3a. **SSH/Telnet 遠端登入（v0.9.77~78）**：`/new` 選 SSH → 開「login as:」分頁並提示回覆帳號；`SendInputToTab` 在 `Session==null && LoginBuffer!=null` 時轉 `HandleLoginInput`（修掉遠端打不進 login as: 的問題），密碼走一般文字輸入（送出後 0.7s 自動回傳畫面）。Telnet 直接開，登入提示由開場畫面推播帶出。0.9.78 加專屬指令 `/ssh [user@]主機[:埠]`、`/telnet [主機[:埠]]`（`ParseHostPort` 解析；不帶參數用 `LastHost`＋`LastSshPort`/`LastTelnetPort`；ssh 帶 user@ 走直啟路徑 `IRemoteHost.OpenSsh`、跳過 login as:）。**尚未實機驗**（需使用者手機測 SSH 帳密流程）。
4. ~~`/close`、`/more`、10 分鐘閒置自動 `/exit`~~ **已做（v0.9.81，未實測）**。規格內尚未做：`/clear`、選配 `/arm` PIN、非附著分頁選擇題通知帶內容。
5. 驗完 commit（v0.9.74~82 遠端功能一整包，含 `q…text` 協定）。
6. **2026-07-27 自動化測試已過**：MacroRunner 單元測試 18/18（elseif/break/continue/巢狀/回歸＋ParseHostPort/MenuOptions/TidyForPhone，散彈槍在 scratchpad mrtest 專案）；Telnet 自動重連 e2e（本機假伺服器，斷線後 3s→6s 退避重連實測）；Telegram e2e（/help、/new inline 按鈕點擊開 PowerShell(桌面)+子行程驗證、文字指令回傳、/history 按鈕、/close 確認按鈕→實關）；UI 截圖驗證（右鍵五項選單、Ctrl+F 搜尋 1/2→2/2→Esc）。**未測**：SSH 帳密實機（需真帳密）、/more 長輸出翻頁、COM 重連（無裝置）、10 分鐘閒置（太久）、選擇題按鈕（需 claude 跳選單）。
