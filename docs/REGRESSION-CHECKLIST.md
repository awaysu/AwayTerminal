# 手動回歸測試清單

對應 `CLAUDE.md` 風險 12：把舊版 `reference/AwayTerminal/CLAUDE.md` 的踩雷紀錄與功能清單
整理成**可逐項勾選**的清單，各平台逐項比對。

## 怎麼用

- 每搬完一個功能，把那一組從頭跑一次；發佈前全部跑一次。
- 平台欄一個平台一欄，填 `PASS` / `FAIL` / `-`（該平台不適用）/ 空白（還沒測）。
- **FAIL 一律連現象一起寫**，別只打 FAIL。
- 標 👤 的項目**代理做不到**（要真人操作鍵盤／看畫面，或需要視窗焦點），
  必須請使用者跑；其餘可以用 dev log（`awayDump` / `awayVerify` / `[diag]`）驗。

| 組別 | 狀態 |
|---|---|
| A. 分頁 | 本次（TASK-004）填好 |
| B. 檢視三態 | 本次（TASK-004）填好 |
| C. 設定檔 | 本次（TASK-004）填好 |
| D. 工具列 | 本次（TASK-005）填好 |
| E. 複製 / 存檔 | 本次（TASK-005）填好 |
| F. 逐分頁配色 | 本次（TASK-005）填好 |
| G. log 記錄 | 本次（TASK-005）填好 |
| H. 輸入 / IME | 待填（TASK-003 已有素材：`docs/TERMINAL-JS-DIFF.md` 第二節、`docs/IME-LAB.md`） |
| I. 貼上 | 待填 |
| J. 輸出 / 渲染 | 待填 |
| K. SSH | 本次（TASK-006）填好第一階段 |
| K2. 其餘連線後端（Telnet / COM / WSL / ADB） | 待填（階段 2） |
| P. 自訂連線 | 本次（TASK-007）填好 |
| Q. 沙盒模式（新功能） | 本次（TASK-007）填好 |
| L. 巨集 / 我的最愛 / 輸入文字視窗 | 待填（階段 3） |
| M. 恢復分頁 / 重連 / 保持連線 | 待填（階段 3） |
| N. 代理團隊 / AI 聊天室 / Telegram | 待填（階段 4） |
| O. 安裝 / 更新 / 簽章 | 待填（階段 5） |

---

## A. 分頁

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| A1 | 開一條新連線後，看那個 pane 有沒有 fit（`awayVerify` 報的欄列數，或畫面有沒有填滿） | pane 的欄列數＝視窗實際大小（不是 80×24）。**這條最容易回歸**：host 建完分頁一定要在 `n{id}` 之後再送 `s{id}`，少了它 `terminal.js` 的 `active` 是 null、`refit()` 直接 return，pane 永遠不 fit、也不回報尺寸 | `MainWindow.xaml.cs` `AddTab` → `SelectTab`（`PostToWeb("s" + tab.Id)`）；TASK-003 實際踩過 | PASS | | |
| A2 | 連開 3 個 PowerShell 分頁 | 三個各自有提示字元、各自的尺寸都正確，彼此輸出不串台 | — | PASS | | |
| A3 | 分頁 id 配發 | 從 1 開始遞增，關掉再開不重用 | `_nextId = 1` | PASS | | |
| A4 | 新 PowerShell 分頁的初始名稱 | `PowerShell(1)`、`PowerShell(2)`… | `NextName("PowerShell")` | PASS | | |
| A5 | 等 0.6 秒後看分頁名稱 | shell 分頁自動改名成**目前目錄的最後一段**（例 `C:\Users\me\Desktop` → `Desktop`） | 1.1.2；`UpdateDirTitle` / `TracksCwdTitle` / `ParseCwd` | PASS | | |
| A6 | 在 shell 裡 `cd` 到別的資料夾 | 名稱跟著換 | 同上 | | | |
| A7 | 在 shell 裡啟動 claude（沒有提示行） | 名稱**停在**啟動 claude 當下的目錄，不亂跳；claude 離開後又跟著更新 | 同上（「claude 跑起來後沒有提示行」） | | | |
| A8 | 👤 右鍵分頁 →「更改名稱」，或雙擊該列 | 改完分頁列與分割模式的 pane 標題都變（`t` 協定），而且**之後不再依目錄自動改名** | `MenuRename_Click` + `TitleLocked` | | | |
| A9 | 👤 拖曳分頁列上下換位置 | 順序變；切到分割／分欄模式時 pane 順序跟著一致（`K` 協定） | 1.1.8 `Tab_DragDrop` | | | |
| A10 | 👤 在分割模式點另一個 pane | 分頁列作用中那一列跟著換（`p` 協定），而且**不會**反覆跳（不回送 `s`） | `case 'p'`「不回送避免迴圈」 | | | |
| A11 | 👤 按分頁列的 ✕ | 先跳「確定要關閉「名稱」？」是／否；按「否」什麼都不會發生 | `CloseTab` 的 MessageBox | | | |
| A12 | 關掉**作用中**的分頁 | 自動選到原位置的那個分頁（沒有就選最後一個），畫面不會變空白 | `RemoveTabSilently`：`Tabs[Math.Min(idx, Count-1)]` | | | |
| A13 | 關分頁之後查行程 | `pwsh` 與對應的 `OpenConsole.exe` 都不見了（先送 Ctrl+C ×3、等 60ms 再強制收尾） | `ConPtySession.Dispose` / `GracefulExitBytes = {0x03,0x03,0x03}` | | | |
| A14 | 關掉整個程式之後查行程 | 沒有殘留的 `OpenConsole.exe` 鎖住資料夾 | 踩雷：「強殺會留殭屍 conhost」；新版 `RunEvent::Exit` → `close_all()` | | | |
| A15 | 分頁列狀態燈：閒置的 shell | 圖示是**淡綠 #A5D6A7** | `TerminalTab.ReadyColor` | PASS | | |
| A16 | 在 shell 裡跑會持續輸出的東西（例 `ping -t`） | 圖示變**淡紅 #EF9A9A**；停下來後 1.5 秒內變回綠 | `UpdateStatuses`：有子行程**且**近 1.5 秒有輸出 | | | |
| A17 | shell 裡**停在等輸入**（例跑完一個指令） | 綠。行程還活著但不再送資料＝不算忙 | 同上（這是「且」的重點） | | | |
| A18 | 直接跑 claude 的分頁 | 近 1.2 秒有輸出＝紅，否則綠（沒有子行程判斷） | `UpdateStatuses` 的 Claude/Custom 分支 | | | |
| A19 | 👤 滑鼠停在分頁列一列上 | tooltip＝`名稱  執行 日:時:分`，第二行是完整路徑（與名稱相同時不重複顯示） | `TerminalTab.ToolTipText` / `ElapsedText`（1.2.5 改回經過時長） | | | |
| A20 | 👤 滑鼠停在狀態燈圖示上 | tooltip＝`種類名稱  補充`，例「PowerShell  C:\Users\me\Desktop」 | `KindTip` | | | |
| A21 | 👤 新分頁 →「自訂指令…」輸入 `claude` | 開得起來，而且該分頁的 flags 有 `c`（dev log 看得到）＝多行貼上走 ESC+CR | `IsClaudeExe`；`n` 協定第三欄 | | | |
| A22 | 👤 新分頁 →「自訂指令…」輸入不存在的指令 | 跳錯誤訊息，**不會**留下一個空的 pane（host 會補送 `x{id}`） | `StartTab` 失敗時 `RemoveTabSilently` | | | |

## B. 檢視三態

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| B1 | 連按工具列的檢視按鈕 | 循環順序＝**分頁 → 分割 → 分欄 → 分頁**；按鈕文字顯示「點了會變成的樣子」 | `Split_Click` / `UpdateSplitButton` | PASS | | |
| B2 | 切到分割、分欄、再切回分頁，每次看各 pane 的欄列數 | 每次切換後**每個** pane 都重新 fit（分割＝grid 對半、分欄＝等寬直條、分頁＝滿版） | `L` 協定 → `terminal.js` `layout()` + `refit()` | PASS | | |
| B3 | 分頁模式時看終端機外框 | 細黃線 `#FDFFB0` | `TermFrame.BorderBrush` | | | |
| B4 | 分割／分欄模式時看終端機外框 | 外框變灰 `#333`，改由各 pane 自己畫黃框標示作用中 | 同上 | | | |
| B5 | 👤 分割模式點 pane 標題列 | zoom（只顯示那一個，再點還原） | `terminal.js` `zoomed` | | | |
| B6 | 👤 分割模式拖曳 pane 標題列到另一個 pane 上 | 兩個位置對調，並把新順序回報 host（`k` 協定） | `onDrop` / `notifyOrder` | | | |
| B7 | 👤 在分割模式縮放視窗 | 所有 pane 都跟著 fit，最後一行不會被截掉 | 踩雷：「fit 截行」——padding 放在 xterm 元素本身 | | | |
| B8 | 分割模式下有 pane 被隱藏過再顯示 | 不會卡在「使用者往上捲」而整格空白 | 踩雷（1.2.0）：`settleBottoms` / `noteScrollIntent` | | | |

## C. 設定檔

`%APPDATA%\com.awaysu.awayterminal\settings.json`（**不是**舊版的
`%LOCALAPPDATA%\AwayTerminal`；舊設定匯入是階段 5 的獨立任務）。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| C1 | 全新安裝（先把 settings.json 刪掉）啟動一次 | 檔案被建出來，內容＝預設值（字型 Cascadia Mono、字級 14、前景 #E0E0E0、背景 #1E1E1E、`imeQuietMs` 20、`restoreBufferLines` 2000、分頁列 220/顯示） | `Services/AppSettings.cs` 的預設值 | PASS | | |
| C2 | 👤 Ctrl+滾輪縮放字級，關掉程式再開 | 字級記住了（`z` 協定 → `fontSize`） | `case 'z'`（只收 6~40） | | | |
| C3 | 👤 Ctrl+滾輪一路快速縮放，同時看 settings.json 的修改時間 | **不是每滾一格寫一次**（寫檔有 600ms 防抖） | 新增（舊版每次都寫） | | | |
| C4 | 切到分欄模式，關掉程式再開 | 還是分欄模式 | 新增（舊版 `_viewMode` 只在記憶體） | | | |
| C5 | 👤 拖曳分頁列左緣改寬度，關掉程式再開 | 寬度記住了；最小 120px | `TabSplitter_DragCompleted` / `TabPanelWidth` | | | |
| C6 | 👤 按工具列右端的 ▲ 隱藏分頁列，關掉程式再開 | 還是隱藏的，按鈕顯示 ▼ | `TabPanelToggle_Click` / `TabPanelVisible` | | | |
| C7 | 👤 改視窗大小／位置，關掉程式再開 | 回到上次的大小與位置；上次是最大化就開成最大化（不會把最大化後的尺寸記成還原尺寸） | 新增（舊版固定 `WindowState="Maximized"`） | | | |
| C8 | 把 settings.json 故意改成壞掉的 JSON 再啟動 | 用預設值開起來，而且**不會覆寫**那個檔（`_suppressSave` 同款行為），dev log 有一行解析失敗 | `AppSettings.Load` 的 `_suppressSave` | | | |
| C9 | 程式跑的時候看 settings.json 旁邊 | 不會留下 `.tmp` 半截檔（先寫 tmp 再原子替換） | `AppSettings.Save()` | | | |
| C10 | 用編輯器開 settings.json | UTF-8、沒有 BOM、中文不是亂碼 | 踩雷：「PS5.1 `Get-Content` 會用 Big5 讀壞 settings.json」——**不要**用 PowerShell 5.1 的 `ConvertFrom-Json`/`ConvertTo-Json` 改這個檔 | | | |

## D. 工具列

按鈕順序照舊版 `MainWindow.xaml`：新連接 ｜ 我的最愛 ‖ 輸入文字 ｜ 複製 ｜ 貼上 ｜ 複製全部 ｜
清畫面 ｜ 翻頁 ‖ 分割 ‖ 遠端 ｜ 其他設定 ‖ 關於。**新版目前只做了**新分頁 ‖ 複製 ｜ 純文字貼上 ｜
複製全部 ｜ 清除畫面 ｜ 翻頁 ‖ 視窗分割——其餘按鈕的功能還沒搬，刻意不放佔位按鈕。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| D1 | 看工具列按鈕文字 | 複製／**純文字貼上**／複製全部／**清除畫面**／翻頁（注意 XAML 的預設文字是「貼上」「清畫面」，**執行時會被 `Loc.T` 換掉**——以執行時的字為準） | `ApplyTexts()` + `Loc.cs` 的 `tb.*` | PASS | | |
| D2 | 👤 滑鼠停在每個按鈕上 | tooltip 照 `tip.copy` / `tip.paste` / `tip.copyall` / `tip.clear` / `tip.page` | 同上 | | | |
| D3 | 關掉所有分頁 | 需要作用中分頁的按鈕（複製／貼上／複製全部／清除畫面／翻頁）變灰停用 | 新增（舊版沒有 disable，按了只是沒反應） | | | |
| D4 | 👤 按「翻頁 ▾」 | 下拉有：上一頁／下一頁／──／移到最上面／移到最下面 | `Page_Click` / `MakeScrollItem` | | | |
| D5 | 在有長輸出的分頁按「上一頁」 | 畫面往上捲一頁，**不會送出任何輸入**（提示字元後面不會多字） | `ScrollActive` → `S` 協定 | PASS | | |
| D6 | 按「移到最上面」再「移到最下面」 | 捲到頭再回到底；回到底之後新輸出會自動跟著捲 | `S` + `terminal.js` `settleBottoms` | PASS | | |
| D7 | 👤 在終端機上按右鍵 | 出現本程式的選單（複製／複製且貼上／純文字貼上／複製全部／複製全部存至檔案／──／搜尋），**不是** webview 預設的「重新載入／檢視原始檔」 | `OnWebContextMenu`（`e.Handled = true`） | | | |
| D8 | 👤 右鍵選單按「搜尋」 | 終端機右上角出現搜尋列（＝Ctrl+F 那一個） | `PostToWeb("F")` | | | |
| D9 | 👤 `echo https://example.com` 之後點那個連結 | 跳「從瀏覽器開啟／複製網址」兩項選單（1.1.10 起先跳選單、不直接開） | `U` 協定 → `ShowUrlMenu` | | | |
| D10 | 👤 選「從瀏覽器開啟」 | 用系統預設瀏覽器開啟。**只放行 http/https**——`file://` 之類的字串點了不會有動作 | `OpenUrlExternal` | | | |
| D11 | 👤 選「複製網址」 | 網址進剪貼簿，畫面出現「已複製網址」提示 | `toast.urlCopied` | | | |
| D12 | 按「清除畫面」 | **先跳「確定要清除「分頁名稱」的畫面嗎？」**是／否；按否什麼都不會發生 | v1.0.27 起一律先問（使用者指定） | | | |
| D13 | 在 PowerShell 分頁確認清除 | 畫面清空但 **scrollback 還在**（用「翻頁」捲得回去）——走的是 Esc → 60ms → Ctrl+L，不是 `term.clear()` | `Clear_Click` 的 PowerShell/SSH 分支 | | | |
| D14 | （之後做 Telnet/COM 時）確認清除 | 走 `c` 協定 → `term.clear()`，**scrollback 會被洗掉**，這就是為什麼一律先問 | 同上的 else 分支 | - | - | - |

## E. 複製 / 存檔

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| E1 | 👤 用滑鼠選一段文字，按「複製」 | 文字進剪貼簿，出現「複製成功」提示 | `Copy_Click` → `q…sel` → `a…sel` | | | |
| E2 | 👤 什麼都不選就按「複製」 | 提示「沒有選取文字」 | `toast.noSelection` | | | |
| E3 | 👤 在**接管滑鼠的程式**裡（例如 claude 全螢幕模式）不選就按「複製」 | 提示改成「沒有選取文字（此程式接管了滑鼠：按住 Shift 再拖曳選取）」 | `m` 協定 → `_selMouseHintId` | | | |
| E4 | 👤 按「複製全部」 | 整個 buffer 進剪貼簿（**含顏色的序列化文字**，不是純文字——這是舊版 `all` 的行為），提示「已複製全部文字」 | `CopyAll_Click` → `ser.serialize()` | | | |
| E5 | 👤 右鍵「複製且貼上」 | 選取文字先進剪貼簿、再貼回同一個分頁（走 `v` 協定，所以 claude 分頁照樣 ESC+CR），提示「已複製並貼上」 | `q…selpaste` → `PasteToTab` | | | |
| E6 | 👤 右鍵「複製全部存至檔案」 | 跳存檔對話框，預設檔名 `{分頁名稱}-{yyyyMMdd-HHmmss}.txt`；存完提示「已存檔」 | `SaveBufferToFile` | | | |
| E7 | 用編輯器或 `xxd` 檢查存出來的 .txt | **UTF-8 with BOM**（`EF BB BF` 開頭）——舊版用 `Encoding.UTF8`，.NET 那個靜態屬性會輸出 BOM | 同上 | | | |
| E8 | 👤 複製一段多行文字後按「純文字貼上」 | 整段進輸入框成為多行，**不會**前幾行被執行掉只剩最後一行 | v1.0.7：必須走 `xterm.paste()` | | | |
| E9 | 在 claude 分頁按「純文字貼上」貼多行 | 換行變 ESC+CR 軟換行、整段一次送（Win10 conhost 會吃掉 bracketed paste 標記） | v1.0.10 / `doPaste` 的 claude 分支 | | | |
| E10 | 自動驗（`--verify`） | `toolbar_paste` 送一段標記字串後，xterm buffer 讀得到它＝`v` → `doPaste` → `xterm.paste()` → PTY 整條路通 | — | PASS | | |

⚠️ **剪貼簿只能半自動驗**：`navigator.clipboard.readText()` 需要視窗有焦點，沒焦點時在
WebView2 上**不是拒絕、是不回應**（實測會把整個驗證流程卡住）。共用桌面不能搶焦點，
所以 E1～E5 一律標 👤；`--verify` 讀不到時會印「剪貼簿讀不到（視窗沒有焦點）→ 需要目視確認」。

## F. 逐分頁配色

色票清單存在 `settings.json` 的 `palette`（全域、可編輯）；**「哪個分頁選了哪一組」只留在記憶體**。
理由＝分頁 id 跨重啟沒有意義，要持久化得等「恢復分頁」把分頁本身存下來（階段 3）。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| F1 | 👤 分頁右鍵 →「配色 ▸」 | 子選單：預設（設定顏色）／──／五組「Aa 範例文字」，每一列用自己的前景背景色顯示 | `MainWindow.xaml` 的配色子選單（五組色票順序照抄） | | | |
| F2 | 👤 選第一組（黃字黑底） | **只有那一個分頁**變色，其他分頁不受影響 | `P{id}US{fg}US{bg}` | | | |
| F3 | 👤 再選「預設（設定顏色）」 | 回到 `settings.json` 的 `foreground`／`background` | `fg`/`bg` 皆空＝清除覆寫 | | | |
| F4 | 自動驗（`--verify`） | 套用後 pane 元素背景＝`rgb(0, 0, 0)`；清除後＝`rgb(30, 30, 30)`（＝`#1E1E1E`） | — | PASS | | |
| F5 | 設好配色後關掉程式再開 | **不會記住**（已知、刻意：見本節開頭） | 舊版同樣不記 | | | |

## G. log 記錄

格式規格與逐條對照寫在 `src-tauri/src/logging.rs` 的檔頭。**這是最容易「看起來對其實差一點」的功能**，
所以下面每一條都可以用檔案內容驗，而且附一份實際樣本。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| G1 | 👤 分頁右鍵 →「記錄 log…」 | 對話框：log 存檔位置（預設 `{我的文件}\AwayTerminalLogs\{分頁名稱}_{yyyyMMdd_HHmmss}.log`）＋「瀏覽…」＋兩個核取方塊（時間戳／append），按鈕是「開始記錄」 | `LogDialog` | | | |
| G2 | 👤 路徑清空後按「開始記錄」 | 顯示「請輸入 log 存檔位置。」且**不關閉對話框** | `LogDialog.Ok_Click` | | | |
| G3 | 開始記錄後看分頁 tooltip | 多一行「● 記錄 log 中」 | `tip.tabLogging` | | | |
| G4 | 用 `xxd` 看 log 檔開頭 | `EF BB BF`（**UTF-8 with BOM**，`new UTF8Encoding(true)`） | `SessionLogger` 建構子 | PASS | | |
| G5 | 檢查換行 | 檔案裡**只有 LF**、沒有 CR（`\r\n` 先被換成 `\n`，`StreamWriter.Write(string)` 不做轉換） | `SessionLogger.Write` | PASS | | |
| G6 | 檢查有沒有殘留 ANSI | 檔案裡**沒有 `0x1B`**（CSI、OSC、兩字元 Fe 三種都被刪掉） | 同上的 `AnsiRegex` | PASS | | |
| G7 | 讓輸出含中文再檢查 | 中文完整、沒有 `�`（跨 chunk 保留 UTF-8 解碼狀態） | 同上的 `_utf8` decoder | PASS | | |
| G8 | 勾了時間戳 | **每一行**開頭 `[yy-MM-dd HH:mm:ss] `（本地時間，共 20 個位元組） | 同上 | PASS | | |
| G9 | 對同一個檔案再記一次、勾 append | 內容接在後面，而且**不會再插一個 BOM**。⚠️ 判斷要看**檔案長度**不是 `stream_position()`——append 模式剛開檔時位置回報 0，用位置判斷會每次都插 BOM（本次實作踩過） | 同上 | PASS | | |
| G10 | 不勾 append、對同一個檔案再記一次 | 檔案被截斷重寫，開頭有 BOM | 同上 | PASS | | |
| G11 | 👤 再按一次「記錄 log…」 | 問「要停止記錄 log 嗎？」；按是 → 停止並在檔案總管中選取那個檔 | `LogAction` / `StopLogging(openFolder: true)` | | | |
| G12 | 記錄中直接關掉那個分頁 | log 正常收尾（不會留半截、不會鎖住檔案） | `RemoveTabSilently` 的 `Logger.Dispose()` | | | |
| G13 | 👤 記錄 claude 這類 TUI 的輸出 | log 裡會有「重繪被攤平」的長行（游標定位被去掉，多次重繪黏在一起）。**這是舊版一樣的行為、不是 bug**——舊版就是因此規定 Telegram `/last` 不能用原始位元組流去 ANSI、要向 xterm 查渲染後文字 | 踩雷：「遠端 /last 絕不能用原始位元組流去 ANSI」 | | | |

### G 的實際樣本（2026-09-26 在 Windows 10 19045 + pwsh 實測）

送進去的指令（`` `e `` 是 PowerShell 7 的 ESC 轉義）：

```text
Write-Host "<ESC>[36m中文測試 AWAY_LOG_OK<ESC>[0m"; $Host.UI.RawUI.WindowTitle='away-log'
```

產出的 log（`xxd` 前 8 個位元組是 `efbb bf5b 3236 2d30`；檔案內只有 LF、沒有 `0x1b`）：

```text
[26-09-26 22:14:35] Write-Host "`e[36m中文測試 AWAY_LOG_OK`e[0m"; $Host.UI.RawUI.WindowTitle='away-log'>
[26-09-26 22:14:35] 中文測試 AWAY_LOG_OK
[26-09-26 22:14:35] PS C:\Users\Awaysu\Desktop>
```

第一行看起來有點亂（結尾多一個 `>`、中間可能重複）是**預期的**：PSReadLine 逐字重繪輸入行，
游標定位序列被去掉之後那些重繪就黏在一起。舊版同一支演算法在同一條流上也是這個結果——
比對新舊版時請比「第 2、3 行」與位元組層面的四個特徵（BOM／LF／無 ESC／時間戳），
不要比第一行的字元數。

### ⚠️ 這台機器的環境雷（2026-09-26 實測）

**預設的 log 資料夾「我的文件\AwayTerminalLogs」在這台機器會讓開檔永遠不返回。**
同一份程式碼寫 `%TEMP%` 只要 1ms。用 `cargo run --example log_probe <路徑>` 幾秒就能重現：
TEMP 路徑印出完整結果，Documents 路徑印完 `path = …` 就停住。

判斷：防毒（PC-cillin 的資料夾保護）擋住剛建置、沒有簽章的 exe 寫入文件資料夾，而且
**不是回一個錯誤、是直接卡住**。這和舊版踩雷「`Process::Start` 剛建置的 exe 回 Access denied
——是 PC-cillin 擋的」是同一族問題。

對策（已做）：`Logger::open_with_timeout` 把開檔丟到背景執行緒、最多等 3 秒，逾時就回一個
講清楚的錯誤（提示可能是防毒、請加例外或換資料夾），不讓 IPC 執行緒整個凍住。
**要真正能寫進文件資料夾，需要使用者把 AwayTerminal 加進防毒例外**；正式版有簽章之後可能就不會了，
發佈前要再確認一次。

## H. 輸入 / IME（待填）

素材已經在 `docs/TERMINAL-JS-DIFF.md` 第二節（25 條踩雷 → 落在哪個函式）與
`docs/IME-LAB.md`（九項固定劇本）。等 Windows 基準跑完再把每一條展開成測試步驟。

## I. 貼上（待填）

## J. 輸出 / 渲染（待填）

## K. SSH

完整的行為對照、演算法現況與「待真機驗證」清單在 `docs/SSH.md`。這裡只列勾選項。
自動驗證：`cd src-tauri && cargo run --example ssh_probe`（**不連任何外部主機**，
用同一支程式裡起的測試 sshd），以及 `--verify` 的 app 端路徑那一步。

| # | 怎麼測 | 預期結果 | 基準 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| K1 | `cargo run --example ssh_probe` | **17 PASS / 0 FAIL** | — | PASS | | |
| K2 | `--verify` 的 SSH 那一步 | 分頁 `kind=ssh`、`backend=russh`；連不上時原因印在終端機；session 正常結束 | 舊版：連不上不可留死分頁 | PASS | | |
| K3 | 👤「新分頁 ▾」→「SSH…」 | 跳完整對話框（欄位表在 `docs/SSH.md` 第 7 節） | 舊版 `ConnectDialog.xaml` | | | |
| K4 | 👤 連一台真設備 | 先出現灰字「連線到 host:port …」，再出現 `login as: ` | PuTTY（**與舊版不同**：舊版連線前就問，見 docs/SSH.md） | | | |
| K5 | 👤 在 `login as:` 打字 | 有回顯；Backspace 退得掉；Enter 送出後分頁標題變 `user@host` | 舊版 `HandleLoginInput` | | | |
| K6 | 👤 密碼提示 | `user@host's password: `，打字**完全不回顯** | PuTTY | | | |
| K7 | 👤 密碼打錯 | 顯示 `Access denied` 並可再試，三次後結束連線 | PuTTY | | | |
| K8 | 👤 **第一次**連某台主機 | 跳「主機金鑰尚未記錄」，顯示演算法／位元數／**SHA256 與 MD5 兩種指紋**，三個選項：接受並儲存／只這次／取消 | PuTTY | | | |
| K9 | 👤 按「取消」 | 連線中止，不進 shell，`known_hosts` 沒有變動 | PuTTY | | | |
| K10 | 👤 按「只這次」 | 連得上，但 `known_hosts` **沒有**新增那一行（下次再問） | PuTTY Connect Once | PASS（probe） | | |
| K11 | 👤 按「接受並儲存」後再連同一台 | **不再詢問**，直接連 | PuTTY | PASS（probe） | | |
| K12 | 手動把 `known_hosts` 那一行的金鑰改掉再連 | 跳**紅框**「⚠ 警告：主機金鑰不符！」，措辭更嚴重，預設焦點在「取消」，並告訴你舊記錄在哪個檔第幾行 | PuTTY 的 POSSIBLE SECURITY BREACH | PASS（probe） | | |
| K13 | 把 `known_hosts` 改成壞掉的內容再連 | 當成「金鑰不符」跳最嚴重的對話框，**不可以**當成「沒記錄」然後覆寫 | 安全預設 | PASS（單元測試） | | |
| K14 | 看 `known_hosts` 檔案 | OpenSSH 格式、一行一台、非預設埠是 `[host]:port`；路徑印在後端 log | — | PASS | | |
| K15 | 確認**沒有**動到 `~/.ssh/known_hosts` | 那是 OpenSSH 的檔，程式不該偷偷寫 | — | PASS（程式沒有這條路） | | |
| K16 | 👤 用 `.ppk` 金鑰連線 | 連得上；金鑰有密碼時在終端機裡問 passphrase（不回顯） | PuTTY | PASS（probe） | | |
| K17 | 👤 Pageant 裡載入金鑰後連線 | 不必打密碼就連上；**沒有** Pageant 時完全不打擾使用者（只記 log） | PuTTY | ⬜ 未實機驗證 | - | - |
| K18 | 👤 改視窗大小 | 遠端跟著換行（window-change） | — | PASS（probe） | | |
| K19 | 👤 關閉 SSH 分頁 | 送 **Ctrl+D ×3**，遠端 session 真的登出 | 舊版 `GracefulExitBytes` | PASS（probe：收到 3 個 `0x04`） | | |
| K20 | 👤 SSH 分頁的狀態燈 | 遠端規則：近 0.5 秒有輸出＝紅，否則綠（**不看**子行程） | 舊版 `UpdateStatuses` 的 else 分支 | | | |
| K21 | 👤 SSH 分頁名稱跟著遠端目錄 | 提示行解析得到就改名（同 shell 分頁） | 舊版 `TracksCwdTitle` 含 `Ssh` | | | |
| K22 | 👤 舊設備（風險 3） | 見 `docs/SSH.md` 第 8 節 S1～S18 | — | ⬜ 等設備清單 | - | - |
| K23 | `ssh_probe` 的「只支援舊演算法的伺服器」那組 | 連得上（kex `group14-sha1`、cipher `aes128-cbc`、MAC `hmac-sha1`、hostkey `ssh-rsa`） | PuTTY 的清單含舊演算法但排最後 | PASS | | |
| K24 | 同上 | **弱演算法警告有跳**（橘框，列出是哪幾項） | PuTTY 的 warn-below-this-line | PASS（probe） | | |
| K25 | 👤 弱演算法警告按「繼續連線」，再連同一台 | **不再問**（記在 `settings.json` 的 `sshWeakAccepted`） | PuTTY | | | |
| K26 | 弱演算法警告按「取消」 | 交握中止、session 結束、**沒有進到 shell** | 安全預設 | PASS（probe） | | |
| K27 | 演算法清單的順序 | 四組都是強→弱，SHA-1 kex 在最後三名、`3des-cbc` 最後、`hmac-sha1` 最後、`ssh-rsa` 最後 | `docs/SSH.md` 第 4 節 | PASS（單元測試） | | |
| K28 | 清單裡的名稱 russh 都認得 | 不然交握時會靜默少一個演算法 | — | PASS（單元測試） | | |
| K29 | 手改 `settings.json` 放一個不存在的演算法名 | 終端機印一行黃字說略過了；**整組都認不出來時退回預設**（不會變成空清單而連不上） | — | PASS（單元測試） | | |
| K30 | 👤 連線閒置超過 `keepAliveMins`（預設 10 分鐘） | 連線沒有被切（`keepalive@openssh.com`） | 舊版 `ServerAliveInterval` | ⬜ 需真設備 | | |
| K31 | 伺服器主動斷線 | session 正常結束，不會掛住 | — | PASS（probe） | | |
| K32 | 👤 伺服器在驗證前送 banner | banner 顯示出來，而且**不是階梯狀**（只有 LF 的 banner 要轉成 CR LF） | PuTTY | ⬜ 需真設備 | | |

### K33～K45：連線對話框（B6，TASK-009）

| # | 怎麼測 | 預期結果 | 基準 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| K33 | 👤 主機欄打 `10.0.0.5:2222` 然後跳到別的欄位 | 自動拆成主機 `10.0.0.5`、埠 `2222` | 舊版主機欄也吃 `host:port` | PASS（`parseHostPort` 單元邏輯 + 目視） | | |
| K34 | 👤 埠欄清空 | 送出時當 22（不可以變成 0 或報錯） | 舊版預設 22 | | | |
| K35 | 👤 主機留白按「連線」 | 不建分頁，欄位旁提示要填主機 | 舊版同樣不讓空主機過 | | | |
| K36 | 👤 帳號留白 | 連上後在終端機問 `login as:`（＝舊版行為） | 舊版沒有帳號欄 | | | |
| K37 | 👤 帳號填好 | **不問** `login as:`，直接進密碼／金鑰驗證，標題一開始就是 `user@host` | PuTTY | | | |
| K38 | 👤 選金鑰檔（`.ppk`） | 路徑顯示在欄位裡；連線用金鑰，不問密碼（金鑰有 passphrase 時在終端機問） | PuTTY | | | |
| K39 | 👤 勾「用 Pageant」 | 沒有 Pageant 時**不要**跳錯誤，退回問密碼 | PuTTY | | | |
| K40 | 👤「進階」展開 | 四組演算法清單，順序同預設；警告線以下的**標紅**並有說明 | `docs/SSH.md` 第 4 節 | | | |
| K41 | 👤 把某組全部取消勾選 | 送出時該組退回預設（不可以變成空清單而連不上） | 同 K29 | PASS（Rust 端單元測試） | | |
| K42 | 👤 拖曳演算法改順序再連 | 交握真的照新順序（後端 log 印出協商結果） | PuTTY 可調順序 | | | |
| K43 | 👤「進階」的環境變數預設 | 預設有 `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`（＝舊版寫死的那條） | 舊版 `SshCommand` 的 `-o SendEnv` | | | |
| K44 | 👤 環境變數打成沒有 `=` 的一行 | 那行被略過，其餘照送（不要整個連線失敗） | — | | | |
| K45 | 👤 對話框按「加到我的最愛」 | 問名字後存起來；**存檔內容沒有密碼欄** | 舊版 `SavedTab` 也沒有密碼 | PASS（單元測試 `favorite_has_no_password_field`） | | |

## TN. Telnet

行為對照表、協商清單、「舊版沒有而 PuTTY 有」的待決清單都在 `docs/TELNET.md`。
自動驗證：`cd src-tauri && cargo run --example telnet_probe`（測試伺服器在同一支程式裡，
**不連任何外部主機**）。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| TN1 | `cargo run --example telnet_probe` | **20 PASS / 0 FAIL** | — | PASS | | |
| TN2 | `--verify` 的 Telnet 那一步 | 分頁 `kind=telnet`、`backend=telnet`、標題 `host:port`；連不上時原因印在終端機 | 舊版 `OpenTelnetDirect` | PASS | | |
| TN3 | 👤 對話框類型切到 Telnet | 埠自動變 23；帳號／金鑰／Pageant／進階四列收起來 | 舊版 `TypeCombo` 切換換埠 | | | |
| TN4 | 👤 連一台真設備 | 登入提示是**遠端**送的（不是我們印的），密碼不回顯 | Telnet 沒有自己的驗證層 | ⬜ 需真設備 | | |
| TN5 | 協商：`WILL ECHO`／`WILL SGA` | 回 `DO`；其餘 `WILL` 回 `DONT` | `RespondOption` | PASS（probe＋單元測試） | | |
| TN6 | 協商：`DO SGA` | 回 `WILL`；其餘 `DO` 回 `WONT` | 同上 | PASS | | |
| TN7 | 協商：`WONT`／`DONT` | **會回**（`WONT x`→`DONT x`、`DONT x`→`WONT x`），但**只在狀態真的改變時**；沒談成過的拒絕不回（否則無限乒乓） | PuTTY／RFC 854（TASK-011 由 PM 決定改掉舊版行為） | PASS（probe＋單元測試） | | |
| TN7b | 協商：`DO TTYPE` → `SB TTYPE SEND` | 回 `WILL TTYPE`，然後 `SB TTYPE IS xterm` | RFC 1091／PuTTY（TASK-011 新增） | PASS（probe＋單元測試） | | |
| TN7c | 伺服器主動 `WILL TTYPE` | 回 `DONT`（那是它要告訴我們它的類型，和上一條是兩件事） | 照舊版 | PASS（單元測試） | | |
| TN7d | 👤 連真設備看畫面 | 遠端知道我們是 xterm → 顏色／功能鍵正常 | TTYPE 的目的 | ⬜ 需真設備 | | |
| TN8 | 協商序列切在封包邊界上 | 後半**不可以**變成畫面上的亂碼（`0xFB` 之類），協商照樣要回 | `TelnetSession.cs` 的 `_iac` 註解（舊版踩過） | PASS（probe＋單元測試） | | |
| TN9 | 送出含 `0xFF` 的資料 | 轉義成 `FF FF`，而且**一次寫出**（不是逐 byte 一個封包） | `Write` 的註解 | PASS（probe） | | |
| TN10 | 收到 `IAC IAC` | 畫面上是一個 `0xFF`，不是指令 | `IacState.Iac` | PASS（probe） | | |
| TN11 | 👤 Enter | 送**單一 CR**（舊版沒有 CR LF／CR NUL 轉換）。**真設備若沒反應，回頭看 `docs/TELNET.md` 第 4 節第二列** | `Write` 原樣送 | PASS（probe） | | |
| TN12 | 重連退避歸零的時機 | **從 socket 讀到第一批位元組**才歸零（不是「有輸出」——我們自己的訊息也走輸出 callback） | 見「隱含契約」的總則 | PASS（probe：連不上時「連上了」次數＝0） | | |
| TN13 | 👤 中文輸出 | UTF-8 設備正常；Big5 設備是亂碼（**舊版也一樣**，要不要做編碼轉換由 PM 決定） | 原樣轉給 xterm | ⬜ 需真設備 | | |
| TN14 | 中文被切在兩個封包之間 | 不裂（我們只拆 IAC、不碰位元組） | — | PASS（probe＋單元測試） | | |
| TN15 | 👤 改視窗大小 | 遠端跟著換行（NAWS）。**這是新增功能**，舊版的 `Resize` 是空的 | `CLAUDE.md`「Telnet 自己實作（加 NAWS）」 | PASS（probe） | | |
| TN16 | 尺寸沒變時 | **不重送** NAWS（前端每次 fit 都會呼叫 resize） | — | PASS（probe） | | |
| TN17 | 👤 不認 NAWS 的老設備 | 只會回 `DONT NAWS`，連線不受影響 | RFC 1073 | ⬜ 需真設備 | | |
| TN18 | 伺服器斷線 | session 結束事件**正好一次**（多一次就會排兩條重連） | — | PASS（probe） | | |
| TN19 | 👤 關閉 Telnet 分頁 | **不送**任何優雅結束鍵，直接關 socket | `Dispose`（舊版沒有 GracefulExitBytes） | | | |
| TN20 | 👤 保持連線（keepalive） | 每 N 分鐘一個 `IAC NOP`，畫面上**看不到東西** | `SendNop` | ⬜ 需真設備（最短 1 分鐘） | | |
| TN21 | 👤 清畫面 | 走 `term.clear()`（Telnet 沒有 shell 可以下 `cls`），會先問確認 | 舊版 `c` 協定那條路 | | | |
| TN22 | 👤 Telnet 分頁的狀態燈 | 遠端規則：近 0.5 秒有輸出＝紅，否則綠 | `UpdateStatuses` 的 else | | | |
| TN23 | 👤 存成我的最愛 → 再從最愛開 | 主機／埠／保持連線／自動重連都照存的；**存檔沒有密碼欄** | 舊版 `SavedTab` | | | |

## CM. 連接埠（COM）

行為對照表、`serialport` 的限制、平台差異都在 `docs/COM.md`。
自動驗證：`cd src-tauri && cargo run --example com_probe`（**不需要硬體**，用同程式內的管線當假裝置）。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| CM1 | `cargo run --example com_probe` | **13 PASS / 0 FAIL** | — | PASS | | |
| CM2 | `--verify` 的連接埠那一步 | 列得出埠與四組選項；開不存在的埠**回錯誤且不留死分頁** | 舊版 `StartTab` 的 catch → `RemoveTabSilently` ＋錯誤視窗 | PASS | | |
| CM3 | 👤「新分頁 ▾ → 連接埠…」 | 跳**獨立**的對話框（不是 SSH/Telnet 那個「類型」下拉），欄位順序 Port / Baud / Data / Parity / Stop / Flow ＋斷線自動重連 ＋「回到預設」 | `Dialogs/ComDialog.xaml` | | | |
| CM4 | 👤 對話框開起來的值 | 是**上次用的**（`settings.json` 的 `comPort`…） | `AppSettings.Com*` | | | |
| CM5 | 👤 按「回到預設」 | COM5 / 115200 / 8 / None / 1 / None；**自動重連的勾選不動** | `Reset_Click` | | | |
| CM6 | 👤 埠下拉 | 列出實際存在的埠；**一定含設定裡那個**（就算現在沒插）；旁邊看得到 USB 描述 | `GetPortNames()` ＋加值 | | | |
| CM7 | 👤 沒有任何埠時 | 提示「偵測不到任何連接埠…」，不是空白 | — | | | |
| CM8 | 👤 插上線後按「重新掃描」 | 新的埠出現在清單裡 | 舊版每次開對話框重新列 | | | |
| CM9 | 選項清單的內容 | **只有函式庫支援的值**（同位 None/Odd/Even、停止位元 1/2、流量控制 None/XON-XOFF/RTS-CTS） | 見 `docs/COM.md` 第 3 節 | PASS（`--verify` 印出來） | | |
| CM10 | 手改 `settings.json` 放 `Mark`／`OnePointFive`／`RequestToSendXOnXOff` | 退成支援的值，**並且在終端機印一行黃字說明**（不可以安靜換掉） | — | PASS（單元測試＋probe） | | |
| CM11 | 分頁標題 | `COM5 115200`（埠＋鮑率） | `OpenComDirect` | PASS（單元測試） | | |
| CM12 | 👤 打字 | 原樣送出，**CR 不轉成 CR LF**、沒有本地回顯 | `SerialSession.Write` | PASS（probe） | | |
| CM13 | 👤 貼上 4KB | 一次寫出（不是逐 byte），不卡住 | `WriteLoop` 的註解 | PASS（probe） | | |
| CM14 | 👤 流量控制擋住時打字 | **不會凍住**（寫入在專用執行緒上，逾時 2 秒後丟掉那一筆） | 舊版同款 | ⬜ 需真設備 | | |
| CM15 | 👤 拔線 | 結束事件正好一次 → 勾了自動重連就退避重連；沒勾就提示按 Enter | `ReadLoop` 的 finally | PASS（probe 的拔線那條） | | |
| CM16 | 重連的「連上了」判斷 | **開埠成功**就算（不能等輸出——序列裝置可能永遠不說話） | 見「隱含契約」總則 | PASS（probe） | | |
| CM17 | 👤 關分頁 | **不送任何優雅結束鍵**，埠馬上釋放（別的程式開得起來） | `Dispose` 只關 port | PASS（probe 驗過不送鍵） | | |
| CM18 | 關分頁的結束事件 | 正好一次，而且**立刻**（不等讀取逾時） | `Dispose` 自己發 `Exited` | PASS（probe；第一版偷懶等逾時被抓到） | | |
| CM19 | 👤 清畫面 | 走 `term.clear()`（沒有 shell 可下 `cls`），先問確認 | 舊版同 | | | |
| CM20 | 👤 COM 分頁的名稱 | **不**跟著遠端目錄改（`TracksCwdTitle` 不含 Com） | 舊版同 | | | |
| CM21 | 👤 存成我的最愛 → 從最愛開 | 埠／鮑率／其餘參數都照存的 | — | | | |
| CM22 | 👤 恢復分頁 | 關程式再開，COM 分頁回來（畫面紀錄也在）；裝置不在了要好好報錯 | 舊版 `case "com"` | | | |
| CM23 | 👤 `SendReset`（清畫面對 COM 送什麼） | **什麼都不送**——舊版的 `SendReset()` 是 TODO，沒有定義過行為，不要自己發明 | `SerialSession.SendReset` | ⬜ 等使用者定義 | | |

## K2. 其餘連線後端：WSL / ADB（待填，階段 2）

## P. 自訂連線

舊版對應 `Dialogs/CustomConnDialog.xaml(.cs)` 與 `MainWindow.OpenCustom`。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| P1 | 全新安裝（`settings.json` 沒有 `customConns`） | 清單是**空的**，「新分頁 ▾」只有 PowerShell／SSH／自訂指令…／自訂連線設定… | v1.0.18 起不自動建立任何自訂連線 | PASS | | |
| P2 | 👤「新分頁 ▾」→「自訂連線設定…」→「自動偵測」 | 把這台機器上有裝的工具加進清單，順序＝ClaudeCode／Codex／OpenCode／GeminiCLI／QwenCode／WSL／Aider／ADB | `KnownTools` 的順序（使用者 2026-09-15 指定） | | | |
| P3 | 再按一次「自動偵測」 | 顯示「沒有找到新的工具」，**不會重複加入** | 同名或**同路徑**都算已存在 | PASS（單元測試） | | |
| P4 | 看 ClaudeCode 那一條的參數 | `--dangerously-skip-permissions` | `KnownTools` | PASS | | |
| P5 | 看 OpenCode 那一條的參數 | `--auto` | 舊版使用者要求 2026-09-15 | PASS | | |
| P6 | 看 Codex／GeminiCLI／QwenCode 的參數 | **空的**（要跳過核准的人自己加） | `KnownTools` 的註解 | PASS | | |
| P7 | 自動偵測加入的 `.cmd` 工具 | 「透過 PowerShell 執行」自動打勾（npm 裝的是 `.cmd`） | `AutoDetect_Click` 的 `viaPs` | PASS | | |
| P8 | 👤 點清單裡一條 → 改參數 → 儲存 → 關掉程式再開 | 改動有留著 | — | | | |
| P9 | 👤「新增」→ 填名稱與執行檔 → 儲存 | 出現在清單與「新分頁 ▾」；沙盒核取方塊**預設是打勾的** | `CLAUDE.md`：沙盒預設開啟 | | | |
| P10 | 👤 勾「隱藏」 | 不出現在「新分頁 ▾」，但還在設定清單裡 | `CustomConn.Hidden` | | | |
| P11 | 👤 勾「啟動前選擇工作目錄」的連線 → 從「新分頁 ▾」開它 | **先跳資料夾選擇**；按取消就不開分頁 | `OpenCustom` 的 `PickDir` | | | |
| P12 | 👤 關閉鍵設 Ctrl+D ×2 的連線 → 開起來再關分頁 | 送兩個 `0x04` | `OpenCustom` 的 `closeBytes` | PASS（單元測試） | | |
| P13 | 👤 關閉鍵設「不送」 | 關分頁時不送任何鍵，直接收掉 | 同上（`none`） | | | |
| P14 | 👤 開一條 claude 的自訂連線 | 分頁 flags 有 `c`（多行貼上走 ESC+CR）；分頁名稱＝工作目錄名 | `IsClaudeExe` / `UsesDirTitle` | | | |
| P15 | 👤 開一條「透過 PowerShell 執行」的連線 | 工具跑起來；**工具結束後仍留在 PowerShell 裡**（不是分頁直接關掉） | 舊版用 `PendingCommand` 打字；新版用 `-NoExit -Command`（見「刻意不同」表） | | | |

## Q. 沙盒模式（新功能）

規格＝`CLAUDE.md`「新增功能 → 沙盒模式」，完整說明在 `docs/AGENT-SANDBOX.md`。
**舊版沒有這個功能**，所以「基準」欄寫的是規格出處。

| # | 怎麼測 | 預期結果 | 基準 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| Q1 | `node scripts/test-sandbox-guard.mjs` | **37 PASS / 0 FAIL**（deny 清單每條至少一例 + 15 個必須放行） | — | PASS | PASS | PASS |
| Q2 | `--verify` 的沙盒那一步 | worktree=true、分支 `sandbox/<名>-<時間>`、`git status` 乾淨、`TEMP` 導到沙盒、worktree 與分支都存在 | — | PASS | | |
| Q3 | `--verify` 的 Job Object 那一步 | 在分頁裡開的子行程 PID，**關分頁前存活=true、關分頁後存活=false** | `CLAUDE.md` 第 2 層 kill-on-close | PASS | - | - |
| Q4 | 👤 開一條沙盒連線（工作目錄在 git repo 裡） | 分頁的工作目錄是 `<repo>/.ai/sandbox/<連線名>`；分頁列有綠色 ⬚ 標記；tooltip 顯示沙盒路徑與分支 | — | | | |
| Q5 | 👤 `git status`（在主工作目錄） | **看不到** `.ai/sandbox/`；而且**使用者的 `.gitignore` 沒有被改**（忽略是寫在 `.git/info/exclude`） | — | PASS | | |
| Q6 | 👤 開一條沙盒連線（工作目錄**不是** git repo） | 沒有 worktree；分頁列的 ⬚ 是**灰色**；tooltip 寫「沙盒（無 worktree，不是 git repo）」；環境變數與 Job Object 照做 | — | | | |
| Q7 | 👤 在沙盒分頁裡 `echo $env:TEMP` | 指到 `<沙盒>/.tmp`，不是系統的 TEMP | — | | | |
| Q8 | 👤 在沙盒分頁裡 `echo $env:APPDATA` / `$env:USERPROFILE` | **和平常一樣**（沒有被改）——改了 agent 會掉登入 | `CLAUDE.md` 明寫 | | | |
| Q9 | 👤 Rust 專案的沙盒分頁 `echo $env:CARGO_TARGET_DIR` | 指到 `<沙盒>/.target`；非 Rust 專案則**沒有**這個變數 | — | | | |
| Q10 | 👤 開一條 **Claude Code** 的沙盒連線，看沙盒目錄 | 產生 `.claude/awayterm-sandbox-guard.mjs` 與 `.claude/settings.local.json`（`PreToolUse`、matcher `Bash`） | `CLAUDE.md` 第 2 層 | ⬜ 未實機驗證 | | |
| Q11 | 👤 在那個 Claude Code 分頁裡叫它跑 `taskkill /IM notepad.exe` | **被拒絕**，訊息說明是沙盒擋的、以及怎麼關掉 | — | ⬜ 未實機驗證 | | |
| Q12 | 👤 叫它跑 `taskkill /PID <某個自己開的 PID>` | **放行** | — | ⬜ 未實機驗證 | | |
| Q13 | 沙盒目錄裡已經有 `.claude/settings.local.json` | hooks **合併**而不是蓋掉；原本的欄位（`permissions` 等）保留；重開分頁不會重複掛 | — | PASS（單元測試） | | |
| Q14 | 👤 Codex / Gemini CLI 的沙盒連線 | 啟動參數多了 `--sandbox workspace-write` / `--sandbox` | `CLAUDE.md` 第 2 層 | ⬜ **參數名未實機驗證** | | |
| Q15 | 👤 分頁右鍵 →「沙盒模式」 | 勾勾反映**連線設定**的值；點了之後提示「下次啟動生效」並提供「重新啟動分頁」 | `CLAUDE.md`：改變在下次啟動生效、需提示 | | | |
| Q16 | 👤 按「重新啟動分頁」 | 舊分頁關掉（走優雅結束鍵）、用新設定開一個新的 | — | | | |
| Q17 | 👤 分頁右鍵 →「清除沙盒…」 | 確認對話框說明「分支會保留」；確認後 worktree 消失、**分支還在**（`git branch --list 'sandbox/*'`） | — | | | |
| Q18 | 👤 沙盒分頁關閉後 | worktree **不會**自動刪（裡面可能有未 commit 的成果） | — | | | |
| Q19 | 👤 WSL／ADB 的自動偵測連線 | 沙盒**預設是關的**（它們是拿來操作機器的工具） | 判斷寫在 `custom.rs` 的 `default_sandbox` | PASS（單元測試） | | |
| Q20 | 👤 沙盒 worktree 是從哪個狀態開的 | **目前 HEAD**——agent 看不到你還沒 commit 的修改。成果要用 `git merge sandbox/…` 拿回來 | `docs/AGENT-SANDBOX.md`「工作流程」 | | | |

## T. TTL 巨集

行為基準、`ttpmacro/` 檔案對照、指令清單、與舊版 C# 版的差異都在 `docs/TTL.md`。
自動驗證：`cd src-tauri && cargo run --example ttl_probe`（跑 `tests/ttl/*.ttl`，比對每個變數）。

| # | 怎麼測 | 預期結果 | 基準 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| T1 | `cargo run --example ttl_probe` | **21 PASS / 0 FAIL**（124 個變數檢查 + 13 個錯誤案例） | — | PASS | | |
| T2 | 運算式優先權 | **位元運算比比較運算緊**（`0 = 2 & 1` ＝ 1）；和 C 相反 | `ttmparse.cpp` 的 11 層 | PASS | | |
| T3 | `and`／`or`／`xor`／`not` | 是**位元**運算（`2 and 1` ＝ 0）；邏輯要用 `&& \|\| !` | `CheckReservedWord` → RsvBAnd… | PASS | | |
| T4 | 整數寬度 | 32-bit 有號、溢位環繞（`$FFFFFFFF` ＝ -1） | 原碼的 `int` | PASS | | |
| T5 | 移位邊界 | 負位移＝反向；`>= 32` 飽和（算術右移負數是 -1）；`>>>` 是邏輯右移 | `EvalBitShift` 的梯子 | PASS | | |
| T6 | `/` 與 `%` 對 0 | 兩個都是 `Divide by zero.` | `Val2==0 && op!=Mul` | PASS | | |
| T7 | 字串常值 | **沒有反斜線轉義**；`'ab'#33"cd"` 相接（中間有空白就不算） | `GetString`／`GetQuotedStr` | PASS | | |
| T8 | 字元碼 | `#65`／`#$41`；值必須 1～255 | `GetCharByCode` | PASS | | |
| T9 | 長度上限 | 識別字 31、字串 511、一行 1023（超過的**丟掉**） | `MaxNameLen`／`MaxStrLen`／`MaxLineLen` | PASS | | |
| T10 | 大小寫 | 指令／變數／標籤全部不敏感 | `_stricmp` | PASS | | |
| T11 | 註解 | `;` 到行尾；`/* */`（可跨行、一行可多段） | `GetFirstChar` | PASS | | |
| T12 | `if` 三種寫法 | 區塊式、`elseif`、單行式 `if <cond> <指令>`（含 `break`／`continue`） | `TTLIf` | PASS | | |
| T13 | 巢狀 if 被跳過時 | 裡面的 `if`／`endif` 也要數層數，不可以配錯 | `ExecCmnd` 的旗標階梯 | PASS | | |
| T14 | `for` 方向 | 起＝迄跑一圈；起 > 迄是**遞減** | `TTLFor` 的 `i<ValEnd`／`i>ValEnd` | PASS | | |
| T15 | `while`／`until`／`do`／`loop` | 四組配對都對，`loop while`／`loop until` 也對 | `TTLWhile`／`TTLDo`／`TTLLoop` | PASS | | |
| T16 | `break`／`continue` | break 跳出**最內層**、continue 跳到迴圈結尾再繼續 | `BreakLoop` | PASS | | |
| T17 | `goto` | 往前、往後都能跳 | `JumpToLabel` | PASS | | |
| T18 | `call`／`return` | 回到 `call` 的下一行；`return` 沒有對應的 call 是 `Invalid control.` | `CallToLabel`／`ReturnFromSub` | PASS | | |
| T19 | 堆疊上限 | call／for／while 疊 10 層以上是 `Stack overflow.` | `MAXSP` 10 | PASS | | |
| T20 | `include` | 變數全域可見；**標籤只在那一層**（被 include 的檔跑完就消失） | `BuffInclude`／`DelLabVar` | PASS | | |
| T21 | 陣列 | `intdim`／`strdim` 先宣告；索引 0 起算；超範圍 `Index out of range.`；重複宣告／大小 0 是語法錯誤 | `TTLDim`／`GetIndex` | PASS | | |
| T22 | 目標變數自動建立 | `int2str istr i` 不必先宣告 `istr` | `GetStrVar`／`GetIntVar` | PASS | | |
| T23 | 型別不能換 | `a = 1` 之後 `a = 'x'` 是 `Type mismatch.` | `ExecCmnd` 的賦值那段 | PASS | | |
| T24 | 標籤與變數同名 | 算重複定義 | 同一張變數表 | PASS | | |
| T25 | 錯誤訊息 | 英文字**逐字**照 `errdlg.cpp`（含 `Label requiered.` 的拼字錯誤），帶行號與檔名 | `DispErr` | PASS | | |
| T26 | 沒實作的指令 | `sendln` 這種第二批的指令回 `Unknown command.`，**不會被當成變數** | `ErrNotSupported` | PASS | | |
| T27 | 舊版巨集 | 舊版 `samples/sample.ttl` 不碰連線的部分原樣跑，結果一樣 | 舊版 `MacroRunner.cs` | PASS（`oldversion.ttl`） | | |
| T28 | 參數是運算式 | `strcopy 'abc' 1 -5 t` 的 `1 -5` 會被讀成 `1-5`（要傳負數得加括號） | `GetIntVal` | PASS | | |
| T29 | 一步＝一行 | `Interp::step()` 一次只跑一行（第二批的 `wait`／`pause` 靠這個掛起） | `Exec()` | PASS | | |
| T30 | 👤 `sprintf` 的浮點（`%f`） | 這一批**不支援**，回 `result=2` ＋語法錯誤（見 `docs/TTL.md` 4.3） | — | PASS（單元測試） | | |
| T31 | 👤 `gettime` 的時區參數 | **不支援**（會改整個行程的 `TZ`），回 `result=2` | 見 `docs/TTL.md` 4.3 | PASS | | |
| T32 | 👤 `setenv` | 只影響本行程，**會影響之後開的分頁**（第二批的 UI 要提醒） | `_putenv_s` | ⬜ 需目視 | | |

### T33～T52：執行入口與 I/O（TASK-013）

| # | 怎麼測 | 預期結果 | 基準 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| T33 | `cargo run --example ttl_probe` | **29 PASS / 0 FAIL**（含 I/O、正規表示式、`exec` 段與舊版 sample.ttl 整檔） | — | PASS | | |
| T34 | `--verify` 的 TTL 那一步 | 在真的 PowerShell 分頁上 `sendln` → `wait` 命中 → 結束提示；分頁狀態有上有下 | — | PASS | | |
| T35 | 👤 分頁右鍵「執行巨集…」 | 跳檔案選擇，篩選器是「TeraTerm 巨集 (*.ttl)」＋「所有檔案」 | 舊版 `MacroAction` | | | |
| T36 | 👤 選一支正常的巨集 | 開始跑；分頁列出現黃色 `M`，tooltip 有檔名與**目前行號** | ⬜ 新增（舊版只有 tooltip 一句） | | | |
| T37 | 👤 執行中再點「執行巨集…」 | 問「要停止巨集嗎？」→ 是 → 停，畫面上一行「[巨集已中斷：檔名]」 | 舊版 `msg.stopMacroAsk` | | | |
| T38 | 👤 巨集正常結束 | 畫面上一行灰字「[巨集執行完畢：檔名]」，`M` 消失 | ⬜ 新增 | | | |
| T39 | 👤 巨集有語法錯誤 | 紅字「[巨集錯誤] … 檔名:行號」＋對話框（訊息／檔名／行號／那一行） | 舊版跳對話框 | | | |
| T40 | 👤 選一個不存在／讀不到的檔 | 「無法讀取巨集：」 | 舊版 `msg.macroReadFail` | | | |
| T41 | 👤 執行中關閉分頁 | 巨集乾淨結束，不留執行緒（後端 log 有「巨集結束」） | 舊版 `CloseTab` 先 `Stop()` | PASS（`tab_close` 呼叫 `stop_for_tab`） | | |
| T42 | 👤 執行中連線斷掉 | `wait` 不會一直等（`connected()` 變 false 就當逾時） | 原碼 `Linked` | PASS（probe） | | |
| T43 | 👤 執行中打字 | 照樣送給連線（**不攔鍵盤**，同舊版） | 舊版 `MacroRunner` 沒攔 | | | |
| T44 | `wait` 比對有顏色的提示字元 | 比對得到（先去 ANSI；**刻意和原碼不同、照舊版**） | 舊版 C# 版 | PASS（probe） | | |
| T45 | `wait` 多個候選 | `result` ＝第幾個（1 起算）；**同時命中時索引小的贏** | `ttmdde.c` 的 `Wait()` | PASS（單元測試＋probe） | | |
| T46 | `wait` 逾時 | `result=0`；逾時＝`timeout`×1000＋`mtimeout` 毫秒，0＝永遠等 | `TTLWait` | PASS | | |
| T47 | `waitln`／`recvln` | `inputstr` ＝那一行（去掉尾端 CR LF） | `GetRecvLnBuff` | PASS | | |
| T48 | `sendln` | 送出的是內容＋**單一 CR**（不是 CR LF） | `TTLSendLn` → `DDEOut1Byte(0x0d)` | PASS（probe 看到 `"hello
"`） | | |
| T49 | `send` 的整數參數 | 送一個位元組（`send 65` ＝ `A`） | `GetParamStrings` 的 `LOBYTE` | PASS（單元測試） | | |
| T50 | 👤 `messagebox`／`yesnobox`／`inputbox`／`passwordbox`／`listbox` | 五種都跳得出來；`yesnobox` 的 `result` 是 1／0、`inputbox` 取消時 `result=0` 且 `inputstr` 空 | `ttmdlg.cpp` 那幾個 | PASS（probe 的假 host）＋👤 目視 | | |
| T51 | 👤 `statusbox`／`closesbox` | 右下角常駐提示，`closesbox` 收掉；**不會擋住巨集** | 原碼是常駐小視窗 | | | |
| T52 | 檔案指令 | `fileopen`／`filereadln`／`filewrite`… 的 `result` 語意照原碼（**1＝碰到檔尾**） | `TTLFileReadln` | PASS（單元測試 11 條） | | |
| T53 | 👤 `connect '<host>:<port> /telnet'` | 在**斷線的**分頁上連得起來；已經連著時 `result=2` | `TTLConnect` | | | |
| T54 | 相對路徑 | 相對於**巨集檔所在的資料夾**（`getdir` 看得到），`setdir` 只改巨集自己的 | `GetAbsPath`／`CurrentDir` | PASS（單元測試） | | |

### T55～T65：正規表示式（TASK-014 A）

引擎決定與**實測**差異表在 `docs/TTL-REGEX.md`。

| # | 怎麼測 | 預期結果 | 基準 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| T55 | `cargo test regex` | 語法矩陣、選項對映、四個指令的行為全過 | `docs/TTL-REGEX.md` 第 2 節（**實測**，不是照記憶） | PASS | | |
| T56 | `strmatch 'abc123' '([a-z]+)([0-9]+)'` | `result`＝**1 起算的位元組位移**、`matchstr`＝整段、`groupmatchstr1/2`＝群組 | `TTLMatchStr` | PASS（單元測試） | | |
| T57 | 沒命中的 `strmatch` | `result=0`，而且 `matchstr`／`groupmatchstr*` **先被清空** | 原碼比對前就清 | PASS | | |
| T58 | 壞掉的 pattern | `strmatch` → `result=0`；`strreplace` → **`result=-1`**（不是錯誤、不中斷巨集） | `onig_new` 失敗那條 | PASS | | |
| T59 | `strreplace` 的取代字串 | **原樣插入**，`\1`／`$1` **不會展開**（要用群組得自己讀 `groupmatchstr*`） | 原碼沒有展開 | PASS | | |
| T60 | `^`／`$` | 是**行**錨點（`^b` 在 `"a\nb"` 命中）——Rust 預設不是，我們**預設開 `m` 旗標**補回來 | Ruby 語法 | PASS | | |
| T61 | `regexoption 'SINGLELINE'` | 關掉行錨點（`^`→`\A`、`$`→`\Z`）；`NEGATE_SINGLELINE` 開回來 | Oniguruma | PASS | | |
| T62 | `regexoption 'MULTILINE'` | 是「**`.` 也吃換行**」（等於 Rust 的 `s`），**不是** Perl 的 `/m` | Oniguruma 的命名陷阱 | PASS | | |
| T63 | `regexoption 'FIND_LONGEST'`／`SYNTAX_*`／`ENCODING_*` | **接受但不生效**，終端機上一行黃字說明；巨集**繼續跑**。認不出來的關鍵字才回語法錯誤 | 刻意（見下方偏差表） | PASS（單元測試） | | |
| T64 | `waitregex` | 逐行比對（收到 LF 才比，資料燒完再補一次）；命中時 `result`＝第幾個、`inputstr`＝**去 ANSI 後的那一行**、`matchstr`／`groupmatchstr*` 也設好；逾時 `result=0` | `ttmdde.c` 的 `FindRegexString` | PASS（probe 7 項） | | |
| T65 | 👤 設備吐 Big5 中文時 `waitregex` | **可能比對不到**（要先解成 UTF-8，無效位元組變 `U+FFFD`）。`wait` 那條路不受影響 | 舊版也是解成字串才比對 → 不是退步 | ⬜ 需真設備 | | |

### T66～T73：`exec` / `execcmnd`（TASK-014 B）

| # | 怎麼測 | 預期結果 | 基準 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| T66 | `exec 'cmd /c exit 7' 'hide' 1` | `result`＝**7**（子行程的 exit code） | `TTLExec` 的 wait 版 | PASS（`--verify`：`exitCode=7`） | | |
| T67 | `exec … 0`（不等） | 馬上回 `result=0`，巨集繼續跑 | 原碼 `CloseHandle` 就不管 | PASS（probe） | | |
| T68 | `execcmnd '<一行 TTL>'` | 那一行**當 TTL 指令執行**（不是開行程） | `TTLExecCmnd` | PASS（probe：`a=42`） | | |
| T69 | 沙盒分頁裡 `exec` | 子行程的 `TEMP`／`TMP`（Rust 專案還有 `CARGO_TARGET_DIR`）**導到沙盒**，工作目錄用沙盒的 | PM 在 TASK-014 定的規則 | PASS（`--verify`：`tempInSandbox=true`） | | |
| T70 | 巨集結束後 | `exec` 開出來的行程（含**孫行程**）被 Job Object 收掉——只按 handle 收，**絕不按名稱砍** | `docs/AGENT-SANDBOX.md` 第 2 層 | PASS（`--verify` 記下 PID，結束後 `aliveAfterMacro=false`） | | |
| T71 | `cargo run --example job_probe` | `PROBE PASS`：`TEMP` 有導過去、關 job 之後 `cmd` 與**孫行程**都不在了 | — | PASS | | |
| T72 | ⚠️ 用 **Store 的 app execution alias** 開的行程（很多機器的 `pwsh` 就是） | **收不到**：真正的行程由 AppX 服務建立，不在我們的 job 裡。`cargo run --example job_probe -- --pwsh` 會印 `PROBE NOTE` | 2026-09-27 實測 | 已知限制 | | |
| T73 | 巨集的 `exec` 有沒有套 agent 的 hook 護欄 | **沒有**（巨集是使用者自己寫的）。沙盒的工作區隔離與 Job Object 照套 | PM 在 TASK-014 定的規則 | — | | |

## CP. 輸入文字視窗（TASK-014 C）

舊版對應 `Dialogs/ComposeDialog` + `MainWindow.SendSnippet`。行為對照表與
「為什麼做成頁內對話框」在 **`docs/COMPOSE.md`**。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| CP1 | `cargo test compose` | 解碼（BOM／UTF-8／Big5）與換行統一 5 條全過 | `LoadFile` | PASS | | |
| CP2 | `--verify` 的輸入文字那一步 | Big5 檔讀回來**內容正確**、換行是 CRLF、送進真的 PowerShell 分頁後**畫面上抓得到那段中文** | — | PASS | | |
| CP3 | 👤 工具列「輸入文字」 | 開頁內對話框，焦點在文字框，游標在最後 | `Compose_Click` | | | |
| CP4 | 👤 沒有分頁時 | 提示「沒有分頁可送」，**不是無聲返回** | `ShowCopyFeedback` | | | |
| CP5 | 👤 用注音打一段中文再按送出 | 組字完全在文字框裡發生（**不經 xterm／ConPTY**），整段一次貼進分頁 | 這個功能的**存在理由** | | | |
| CP6 | 👤 勾「送出後送 Enter」 | 貼上後**等 200ms** 才送 `\r`；勾選狀態記進 `settings.json` | `SendSnippet` 的 `Delay(200)` | | | |
| CP7 | 👤 在 claude 分頁送多行 | 換行變成**軟換行**（ESC+CR）——那是 `terminal.js` 原本就有的邏輯，我們走同一條貼上路徑 | `doPaste` | | | |
| CP8 | 👤 開視窗後切到別的分頁再按送出 | 送到**開視窗那一刻**的分頁 | `_targetTab` | | | |
| CP9 | 👤 打字後按 X／「返回」 | 文字**留著**，下次開還在（送出之後才清空） | `_draft`（static） | | | |
| CP10 | 👤 按「清除」再按「復原」 | 救得回來；**關掉重開也救得回來** | `_lastCleared`（static） | | | |
| CP11 | 👤 Ctrl+Z | 走自己維護的堆疊（程式改過 `value` 之後原生 undo 不可靠） | WPF 原生 undo | | | |
| CP12 | 👤 載入一個 Big5 的 `.txt` | 中文正確；下面提示用哪種編碼解出來的（**新增**：舊版沒說） | BOM → UTF-8 → ANSI(cp950) | PASS（`--verify`） | | |
| CP13 | 👤 載入超過 2MB 的檔 | 「檔案太大（上限 2 MB），未載入。」**現有內容不動** | `LoadMaxMB = 2` | | | |
| CP14 | 👤 按「儲存」 | 存成 **UTF-8 無 BOM** | `SaveFile` | | | |
| CP15 | 👤 Ctrl+Enter／Esc | 送出／關閉 | `PreviewKeyDown` | | | |
| CP16 | 文字框空的時候 | 「送出」是灰的（空白不送；只想送 Enter 請直接在終端機按） | `Send_Click` 的第一行 | PASS（前端邏輯） | | |

## L. 我的最愛

舊版對應 `MainWindow.Favorites.cs` + `Dialogs/FavoritesDialog`。程式在
`src-tauri/src/favorites.rs` + `src/favs.js`。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| L1 | 全新安裝按「我的最愛 ▾」 | 清單是空的，顯示「還沒有我的最愛」 | — | PASS | | |
| L2 | 👤 開一個 PowerShell 分頁 →「加到我的最愛：目前分頁」 | 存成一筆，名稱預設是分頁名稱；**記住工作目錄** | 舊版存 `SavedTab` | | | |
| L3 | 👤 沒有分頁時看選單 | 「加到我的最愛」是**灰的** | 舊版同樣灰掉 | | | |
| L4 | 👤 同一條連線再按一次「加到我的最愛」 | 提示「已經在我的最愛裡了」，**不會存成第二筆** | 舊版一條連線只留一筆 | PASS（`key_of` 單元測試，大小寫不分） | | |
| L5 | 👤 點一筆 PowerShell 最愛 | 在**記下的工作目錄**開新分頁，不再跳資料夾選擇 | 舊版同 | | | |
| L6 | 👤 點一筆 SSH 最愛 | 直接連（主機／埠／帳號／金鑰／保持連線／自動重連都照存的）；**密碼當場問** | 舊版不存密碼 | | | |
| L7 | 👤 點一筆自訂連線最愛 | 用存的連線名稱＋工作目錄開（沙盒開關照該條連線目前的設定） | 舊版同 | | | |
| L8 | 看 `settings.json` 的 `favorites` | **沒有任何密碼欄位** | 舊版 `SavedTab` 逐欄看過也沒有 | PASS（單元測試） | | |
| L9 | 👤「設定…」→ 改名 | 清單跟著改；重名要被擋 | 舊版 `FavoritesDialog` | | | |
| L10 | 👤「設定…」→ 上移／下移 | 順序變了，**下拉選單的順序跟著變**（存進 `settings.json`） | 舊版可排序 | | | |
| L11 | 👤「設定…」→ 刪除 | 先問確認再刪 | 舊版會問 | | | |
| L12 | 👤 重開程式 | 最愛清單與順序都還在 | — | | | |
| L13 | 👤 手改 `settings.json` 把某筆的 `kind` 寫成不認識的字 | 那筆當成 PowerShell（或略過），程式**不可以**開不起來 | — | | | |

## M. 斷線自動重連（SSH / Telnet 共用）

舊版對應 `ScheduleReconnect` / `ManualReconnect` / `TryReconnect`。程式在
`src-tauri/src/ssh/reconnect.rs`；行為對照表在 `docs/SSH.md` 第 5 節。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| M1 | `--verify` 的 SSH 那一步 | 「提示按 Enter 重連＝true」「自動重連有排程＝true」「退避次數 1 → 2」 | — | PASS | | |
| M2 | 退避秒數 | 3、6、9…**上限 30** | `Math.Min(30, 3 * attempt)` | PASS（單元測試釘住 3/6/9/27/30/30） | | |
| M3 | 👤 沒勾「斷線自動重連」的連線斷掉 | 只印灰字「連線已中斷。按 Enter 重新連線。」，**不會**自己重連 | 舊版 `OnSessionExit` 的 else | PASS（`--verify`） | | |
| M4 | 👤 承上，按 Enter | 立刻重連（不用等） | `ManualReconnect` | | | |
| M5 | 👤 勾了自動重連的連線斷掉 | 印「N 秒後自動重連…」，時間到自己連回去 | `ScheduleReconnect` | PASS（`--verify` 排程；probe 驗過重連成功） | | |
| M6 | 👤 等待重連時按 Enter | **不等退避**馬上重連，而且不會變成連兩次 | `ManualReconnect`（我們用 generation 作廢舊排程） | | | |
| M7 | 👤 重連成功後畫面 | 舊畫面**留著**，新輸出接在後面（不清畫面、可以往上捲看斷線前的內容） | 舊版送 `b` 推 scrollback | | | |
| M8 | 👤 重連成功後再斷一次 | 退避從 3 秒重新算（不是接著上次的 30 秒） | 舊版「有輸出就歸零」 | PASS（`OnConnected` 歸零；見 `docs/SSH.md` 第 5 節的刻意不同） | | |
| M9 | 👤 重連連不上 | 退避次數往上加（3→6→9…），訊息每次都印 | 舊版同 | PASS（`--verify` 看到 1→2） | | |
| M10 | 👤 等待重連時自己關分頁 | 不再重連、不留背景排程（後端沒有多餘 log） | 舊版 `_closing` | | | |
| M11 | 👤 重連時的主機金鑰 | **不再問**（已接受過的金鑰沿用） | PuTTY | PASS（probe：伺服器重開後重連，被問 0 次） | | |
| M12 | 👤 重連用的參數 | 主機／埠／帳號／金鑰／演算法都跟第一次一樣；**密碼要重新問** | 舊版不存密碼 | | | |
| M13 | 👤 log 記錄開著的分頁斷線重連 | log 繼續寫同一個檔（不會斷檔或重開一個） | 舊版 log 綁分頁不綁 session | | | |
| M14 | 👤 Telnet 分頁斷線 | 退避、提示、按 Enter 重連的行為與 SSH **完全一樣**（同一套程式） | 舊版兩種共用 `ScheduleReconnect` | PASS（`--verify` 的 Telnet 那步：提示有出來） | | |

## R. 恢復分頁（含畫面紀錄倒回）

舊版 1.0.45 的功能。流程對照表在 `src-tauri/src/restore.rs` 的檔頭註解。
`--verify` 會在**同一次執行裡**走完「存 → 恢復」並檢查順序。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| R1 | 👤 按視窗的 ✕ | 跳離開對話框，勾選項文字是「下次開啟恢復目前分頁（含畫面上的舊訊息）」 | `Dialogs/ExitDialog` + `Loc` 的 `exit.restore` | | | |
| R2 | 👤 對話框按「取消」 | 程式**不關**，分頁都還在 | `OnClosingAsk` 的 `return` | | | |
| R3 | 👤 勾選狀態 | 下次開啟這個對話框時沿用（`settings.json` 的 `exitRestoreTabs`，預設開） | `AppSettings.ExitRestoreTabs` | | | |
| R4 | `--verify` 的恢復那一步 | 「存下 N 個分頁」、「有畫面內容的分頁：第 1 筆」、「存檔不含密碼欄位：true」 | — | PASS | | |
| R5 | 同上 | 「舊畫面有倒回來」「分隔行有出現」 | `b` 協定 + `LoadRestoreBuffer` | PASS | | |
| R6 | 同上（**最重要**） | **`held` 順序正確**：舊畫面 → 分隔行 → 新連線的輸出。這是在驗 `terminal.js` 的 `pendingRestore`／`held`（原檔一字未改的那段） | `applyRestore` | PASS（記號 4 < 分隔行 5 < 提示字元 6） | | |
| R7 | 👤 恢復後往上捲 | 看得到上次關閉前的內容（含顏色） | xterm 序列化含 SGR | | | |
| R8 | 👤 恢復 PowerShell 分頁 | 在**上次的工作目錄**開起來；分頁名稱照上次 | `SavedTab.Dir` / `Title` | | | |
| R9 | 👤 恢復自訂連線分頁（沙盒開著） | worktree 還在就沿用、不在就重開一個；工作目錄是**沙盒之前**的那個（不會在沙盒裡再開一層） | 1.0.30 起也恢復 custom | | | |
| R10 | 👤 恢復 SSH 分頁 | 直接連（帳號已經記在參數裡）；**密碼重問**。舊版是印 `login as:` 等使用者打帳號——**刻意不同** | `RestoreTabs` 的 `case "ssh"` | | | |
| R11 | 👤 恢復 Telnet 分頁 | 直接連，標題照上次 | `case "telnet"` | | | |
| R12 | 👤 某個分頁的工作目錄已經被刪掉 | 那一筆略過或退回桌面，**其餘分頁照開**（不可以整個恢復流程掛掉） | `catch { }` 跳過個別分頁 | | | |
| R13 | 看 `settings.json` 的 `savedTabs` | **沒有任何密碼欄位** | 舊版 `SavedTab` 也沒有 | PASS（單元測試） | | |
| R14 | 看 `{app config dir}/restore/` | `tab1.txt`、`tab2.txt`…，UTF-8 **不含 BOM**；每次關閉**全部重寫**（不累積） | `FinishExitAsync`：先清目錄、`new UTF8Encoding(false)` | PASS（`--verify` 的存檔那步） | | |
| R15 | 👤 沒勾「恢復分頁」就關 | 下次開啟是**一個乾淨的預設分頁**，`savedTabs` 清空 | 同上（`restore=false`） | | | |
| R16 | 👤 `restoreBufferLines` 改成 0 | 分頁照恢復，但**沒有舊畫面** | `AppSettings.RestoreBufferLines` | | | |
| R17 | 👤 `--cmd claude` 啟動 | **不恢復**（那是「這次要開這個」的意思） | 新版的判斷（舊版沒有 CLI 參數） | | | |
| R18 | 👤 關閉程式的速度 | 按了「離開」視窗**馬上消失**，存檔在看不見的狀態下做完（最多 2.5 秒） | 1.2.6 的 `Hide()` + 2500ms 上限 | | | |
| R19 | 👤 前端壞掉時按 ✕ | 對話框沒出來的話，**再按一次 ✕ 就會關**（不會鎖死視窗） | 舊版有 WPF 對話框保證，我們沒有 → 自己加的逃生門 | | | |
| R20 | 👤 恢復後的分頁 tooltip | 執行時長從**最初開啟**算起，不是從恢復算起 | `SavedTab.OpenedUtc`（1.1.4） | | | |

## M2. 保持連線（待填，階段 3）

## N. 代理團隊 / AI 聊天室 / Telegram 遠端（待填，階段 4）

## O. 安裝 / 更新 / 簽章（待填，階段 5）

---

## 刻意與舊版不同的地方

回歸比對時會撞到這些差異，**它們都是故意的**，不要當成搬移沒搬完。

| 項目 | 舊版 | 新版 | 為什麼 |
|---|---|---|---|
| 分頁列狀態燈圖示 | `icon/*.png` 灰階圖，`IconTint` 逐像素染色 | inline SVG + `currentColor` | 同一組顏色（`#A5D6A7`／`#EF9A9A`）、同樣「連線種類圖示染綠紅」的語意，但不必把二進位圖檔搬進 repo，也不必在瀏覽器做 canvas 逐像素處理 |
| 確認／輸入／log 對話框 | WPF `MessageBox`／`InputDialog`／`LogDialog` | 頁內深色對話框 | 不加 dialog plugin 的 JS 端，也不用 `window.confirm`（webview 原生對話框會擋住事件迴圈、樣式也對不起來）。文字、按鈕、驗證行為照舊版 |
| 存檔／選資料夾／選 log 位置 | WPF / WinForms 對話框 | 系統原生對話框（`tauri-plugin-dialog`，從 Rust 端叫） | 這幾個要的就是系統檔案瀏覽器 |
| 視窗大小／位置 | 不存（XAML 固定 `WindowState="Maximized"`） | 存進 `settings.json` | **新增功能、舊版沒有這個行為** |
| 檢視模式（分頁／分割／分欄） | 只在記憶體 | 存進 `settings.json` | 新增；舊版每次開都是分頁模式 |
| 逐分頁配色的色票 | 寫死在 `MainWindow.xaml` | `settings.json` 的 `palette` | 讓使用者能改；「哪個分頁選哪一組」兩邊都不持久化 |
| 「記錄 log 中」提示 | 只在 tooltip（1.1.2 起分頁列不放 log 圖示） | tooltip **＋** 分頁列一個小紅點 | 一個點不占空間又看得出來 |
| 工具列按鈕 | 12 個 | 目前 7 個 | 我的最愛／輸入文字／遠端／其他設定／關於的功能還沒搬，**刻意不放佔位按鈕** |
| 全選（`A` 協定） | 有 `ctx.selectAll` 字串但**沒有呼叫端**（死協定） | 終端機右鍵有「全選」 | **新增、舊版字串存在但沒有呼叫端**。TASK-006 由 PM 決定補上；`terminal.js` 的 `A` 分支本來就能用 |
| log 開檔 | 直接開 | 丟背景執行緒、最多等 3 秒 | 見上面的環境雷 |
| SSH 後端 | 呼叫系統 `ssh.exe` | **內建 `russh`** | `CLAUDE.md` 定案：SSH 內建、行為照 PuTTY。使用流程照舊版，協定層照 PuTTY（`docs/SSH.md`） |
| SSH 的 `login as:` 時機 | **連線前**就問（帳號要放進 `ssh.exe` 命令列） | **連上交握後**才問 | PuTTY 的順序，也是內建 SSH 的自然順序。使用者看到的差別：前面多一行灰字「連線到 host:port …」，主機金鑰對話框會在 `login as:` 之前 |
| SSH 主機金鑰存放 | （舊版沒有，`ssh.exe` 用 `~/.ssh/known_hosts`） | `{app config dir}/known_hosts`，**不碰** `~/.ssh/known_hosts` | 程式不該偷偷寫 OpenSSH 的檔。代價：用 `ssh` 連過的主機這裡仍會問一次 |
| **沙盒模式** | 沒有 | 自訂連線多一個選項，**預設開啟** | **新增功能**，規格＝`CLAUDE.md`「新增功能」一節。說明見 `docs/AGENT-SANDBOX.md` |
| 分頁列的沙盒標記 ⬚ | — | 綠色＝有 worktree、灰色＝只有環境變數與 Job Object | 新增；一個字不占空間又看得出來 |
| 自訂連線「透過 PowerShell 執行」 | 先開互動 PowerShell，**等尺寸就緒後才把指令打進去**（避免以 80 欄啟動） | `pwsh -NoExit -Command "& 'path' args"` | 我們的 PTY 一開始就是前端回報的真實尺寸，沒有 80 欄問題 → 少一套延後打字的機制。使用者看到的結果一樣（工具跑完仍留在 shell 裡）。**⚠️ 若使用者回報某個工具以 80 欄啟動，回頭看這一條** |
| SSH 的 3DES 與 CBC 位置 | PuTTY 把 3DES 放在警告線**之上** | 所有 CBC 與 3DES 都在警告線**之下** | CBC 在 SSH 上有已知攻擊面、3DES 的 64-bit 區塊早就不該當預設。舊設備照樣連得上，只是多一次警告 |
| 沙盒的 git 忽略 | — | 寫 `.git/info/exclude`，**不動使用者的 `.gitignore`** | `.gitignore` 是會進 commit 的檔，程式不該改它 |
| 重連退避「歸零」的觸發點 | **一收到輸出**就歸零（`OnSessionOutput` 第一行） | **shell channel 開成功**才歸零（`OnConnected`） | 舊版的輸出全來自 `ssh.exe`＝一定是遠端的。新版內建 SSH，自己的狀態訊息（「連線到 …」、`login as:`、錯誤訊息）走同一條輸出 callback，照舊版寫會被誤判成「連上了」→ 退避永遠停在 3 秒（`--verify` 抓到）。目的一樣，判斷更精確 |
| SSH 連線對話框的欄位 | 類型／主機／埠／保持連線／自動重連 | 多了帳號、金鑰檔、Pageant、進階（演算法四組 + 環境變數） | 舊版這些只能靠 `ssh.exe` 命令列參數；內建 SSH 之後沒有命令列可下，只能做進對話框。**密碼欄兩邊都沒有**（當場問、不存檔） |
| 我的最愛的主機歷史 | 主機欄是可編輯下拉，記最近連過的主機 | **沒有**（只有我的最愛） | 我的最愛已經涵蓋「常連的」。若使用者回報想要歷史，再補 |
| Telnet 的 TTYPE 與 `WONT`／`DONT` | 不認 TTYPE、不回 `WONT`／`DONT` | 回 `xterm`；`WONT`／`DONT` 在狀態改變時回答 | PM 在 TASK-011 決定照 PuTTY。TTYPE 影響遠端送不送顏色與功能鍵；回答拒絕是 RFC 854 要求的（但只在改變狀態時，否則無限乒乓） |
| COM 的讀取方式 | blocking read（`ReadTimeout = InfiniteTimeout`） | **25ms 短逾時輪詢** | `serialport` 的 `read` 要靠逾時才回得來，否則關分頁時執行緒永遠卡住。**「關分頁」不靠這個逾時**：`close()` 自己發結束事件（同舊版 `Dispose`） |
| COM 的同位 Mark／Space、1.5 停止位元、RTS/CTS+XON/XOFF | `System.IO.Ports` 都有 | **沒有**（`serialport` 不支援）→ 清單不列、設定裡有就降級並印黃字 | 見 `docs/COM.md` 第 3 節。真的需要時要自己用 Win32 DCB 或 fork crate |
| COM 對話框的「加到我的最愛」 | 沒有（只能從分頁加） | 有 | 和 SSH／Telnet 對話框一致 |
| TTL 的 `and`／`or` | 舊版 C# 版當成**邏輯**運算 | **位元**運算 | `CLAUDE.md` 定的基準是 TeraTerm 原碼（`CheckReservedWord` 對映到 RsvBAnd／RsvBOr）。比較運算的結果是 0／1，所以一般巨集看不出差別 |
| TTL 的運算子優先權 | 舊版照 C 的排法（比較比位元緊） | **位元比比較緊**（原碼的 11 層） | 同上。`a = 1 and b = 1` 兩版讀法不同 |
| TTL 的整數寬度 | 舊版是 64-bit（C# `long`） | **32-bit 有號、溢位環繞** | 同原碼的 `int`；`$FFFFFFFF` ＝ -1 |
| TTL 的 `break`／`continue` 在單行式 `if` 裡 | 舊版 README 說不支援 | 支援 | 照原碼，新版比較寬 |
| TTL 的 `wait` 比對對象 | 原碼比**原始位元組**（含 ANSI）；舊版 C# 先去 ANSI | **先去 ANSI**（照舊版） | 有顏色的提示字元（`[32m$[0m`）原碼比不到；使用者的巨集是照舊版行為寫的 |
| 巨集執行中的顯示 | 舊版只有 `IsMacroRunning` 影響 tooltip | tooltip **＋分頁列黃色 `M`**（含目前行號） | 一個字不占空間又看得出來（同 log 紅點、沙盒 ⬚ 的做法） |
| 巨集結束／中斷的提示 | 舊版靜靜結束（只有錯誤跳視窗） | 畫面上一行灰字（完畢／已中斷），錯誤是紅字＋對話框 | 使用者要知道巨集什麼時候跑完；這也是 `--verify` 能自動驗的依據 |
| TTL 的 `setdir`／`changedir` | `SetCurrentDirectory`（整個行程） | 只改**巨集自己的**目前目錄 | 多分頁的 app 不能讓一支巨集改掉別人的工作目錄 |
| TTL 的密碼指令（`setpassword`…） | 原碼用自己的弱加密存進 `.INI` | **不做那個格式** | 弱加密會給使用者錯誤的安全感；要做應接 OS 憑證存放區 |
| Telnet 的視窗大小 | `Resize` 是空的（`// NAWS 可選，暫略`），遠端永遠以為 80×24 | **送 NAWS**（RFC 1073） | `CLAUDE.md` 定案「Telnet 自己實作（加 NAWS）」。`vi`／`top` 才不會畫錯 |
| 恢復 SSH 分頁 | 只印 `login as: ` 等使用者打帳號（帳號要塞進 `ssh.exe` 命令列） | **直接連**（帳號已經記在 `SshConnParams` 裡），沒有帳號才問 | 和第一次連線的行為一致。密碼兩邊都是重問 |
| 離開對話框 | WPF `ExitDialog`，含「恢復分頁」與「更新 CLAUDE.md」兩個勾選 | 頁內對話框，只有「恢復分頁」 | 「更新 CLAUDE.md」是代理團隊的功能（階段 4），那時再補 |
| TTL 的正規表示式引擎 | 舊版**沒有** `strmatch`／`waitregex`；原碼用 Oniguruma | **`fancy-regex`**（差異表：`docs/TTL-REGEX.md`） | 選它是因為後顧與後向參照都要有（`regex` 刻意不支援）。`^`／`$` 用「預設開 `m`」補成原碼的行錨點語意；`\101`／`\cA` 沒有（改寫成 `\x41`／`\x01`） |
| `regexoption` 不支援的關鍵字 | — | **接受、印一行黃字、繼續跑** | 「舊巨集裡的一行 `regexoption` 不該讓整支巨集停掉」，但安靜忽略會讓使用者以為生效了。認不出來的關鍵字才回語法錯誤 |
| TTL 的 `exec` | 原碼 `CreateProcess(NULL, cmdline, …)`，開完不管 | 走 `cmd /C`（其他平台 `sh -c`），並**放進巨集自己的 Job Object**；沙盒分頁還帶沙盒的 `TEMP`／工作目錄 | Rust 沒有「整條命令列」的 API；Job Object 是「只收自己開的那一棵」的正確做法（絕不按名稱砍）。**不套 agent 的 hook 護欄**——巨集是使用者自己寫的（PM 在 TASK-014 定） |
| 輸入文字視窗 | WPF **模態視窗** | 頁內對話框 | 理由與可回退的做法見 `docs/COMPOSE.md` 第 2 節（組字都在 `<textarea>`／WPF `TextBox` 裡，對這個功能沒有差別） |
| 輸入文字送出的換行 | 原樣送（WPF 多行文字本來就是 CRLF） | **明確**轉成 CRLF | `<textarea>` 給的是 `\n`；不轉的話同一份文字新舊版送出的位元組不一樣 |
| 載入文字檔的「系統 ANSI」 | `Encoding.Default`（這台是 cp950） | **固定 Big5** | 跨平台沒有「系統 ANSI」這回事；使用者的舊檔就是 Big5 |

## 隱含契約（最容易回歸的一類）

這些不是功能，是「兩邊必須配合」的規則。改到相關程式碼時請重跑對應項目。

| 規則 | 違反的症狀 | 對應項目 |
|---|---|---|
| host 建完分頁一定要在 `n{id}` 之後再送 `s{id}` | `terminal.js` 的 `active` 留在 null → `refit()` 直接 return → pane 永遠不 fit、也不回報尺寸 | A1 |
| `tab_*`（分頁列按下去）會改模型**並**回送舊協定；`pane_*`（`p`／`k` 從 `terminal.js` 進來）**只改模型、不回送** | 回送就會和前端打乒乓（舊版 `case 'p'` 的註解就是「不回送避免迴圈」）：點 pane 之後作用中分頁反覆跳、拖曳排序抖動 | A9、A10 |
| 多行貼上一定走 `v` 協定／`xterm.paste()`，不可以直接寫 session | 每個換行被當 Enter 送出，只剩最後一行留在輸入框 | E8、E9 |
| 清畫面的 Esc 與 Ctrl+L 不可以黏著送 | PSReadLine 當成 escape 序列，兩者都失效 | D13 |
| 會阻塞的檔案操作不可以在 IPC 執行緒上直接做 | 防毒擋住寫入時整個程式的 IPC 全停（本次實測） | G 的環境雷 |
| **介面文字要照執行時的字，不是照 XAML** | 舊版 `MainWindow.xaml` 的按鈕內容只是設計時的預設值，`ApplyTexts()` 會用 `Loc.T` 全部換掉（例：XAML 寫「貼上」「清畫面」，執行時是「**純文字貼上**」「**清除畫面**」）。照 XAML 抄就會做出使用者沒見過的文字 | D1 |
| `ssh-hostkey` event 一定要回 `ssh_hostkey_answer` | Rust 的 SSH 任務停在交握中間等答案，前端不回就卡到逾時（180 秒）才當成取消——使用者看到的是「連線很久沒反應」 | K8～K12 |
| `ssh-weak-algo` event 也一定要回 `ssh_hostkey_answer`（兩者共用同一個回覆通道） | 同上：不回就卡 180 秒。加新的「交握中間問使用者」的事件時都要記得配一個前端 listener | K24～K26 |
| **舊版靠外部程式（`ssh.exe`、`telnet.exe` 之類）副作用成立的規則，內建實作要重新檢查觸發點** | 照抄會得到「看起來對、其實永遠不成立／永遠成立」的條件。實際案例：舊版「**一收到輸出**就把重連退避歸零」——它的輸出全部來自 `ssh.exe`，所以等於「連上了」；內建 SSH 之後我們自己的狀態訊息（「連線到 …」、`login as:`、錯誤訊息）走同一條輸出 callback，退避永遠停在第一次的 3 秒（`--verify` 抓到）。搬 Telnet／COM／ADB 時每一條「有輸出」「行程結束」類的規則都要重新問一次「這個訊號現在還是原來的意思嗎」 | M8、M9、TN12、CM16 |
| **要收掉自己開的行程，只能用 handle（Job Object／PID），而且那個 PID 必須是自己這次開的** | 按名稱砍（`taskkill /IM`、`Stop-Process -Name`）會把整個團隊連自己一起砍掉——這台機器的團隊就跑在舊版 AwayTerminal 底下，而新版 exe 的名稱和舊版一樣。**已知漏洞**：用 **Store 的 app execution alias** 開的行程（很多機器上的 `pwsh`）由 AppX 服務建立，不在我們的 job 裡 → 收不到（`examples/job_probe.rs` 兩種都實測過） | T70～T73、Q7 |
| **`--verify` 的步驟之間，前一步在畫面上留下的重畫要等它畫完** | 上一步的 Ctrl+C 讓 PSReadLine 重畫（印中斷的那一行＋新的提示字元），**蓋掉**下一步剛用 `writeOutput` 寫進畫面的記號 → 恢復分頁那兩條變成假失敗（TASK-014 實際踩到：`verifyCompose` 送完 Ctrl+C 沒等就跑 `verifyRestore`） | R2、R4、CP2 |
| 會等前端回覆的 tauri command **一定要是 `async`** | tauri 2 的同步 command 跑在**主執行緒**上；擋住主執行緒 webview 的 IPC 就進不來，前端永遠沒機會回答 → 一定逾時。實際案例：`exit_confirm` 要等 `a…save`，第一版寫成同步 → 「存下 2 個分頁」卻一個畫面都沒存到 | R1～R4 |
| 恢復畫面的 `b{id}` 一定要在 `n{id}` 之後、`s{id}` 與啟動連線之前 | 順序錯了就不是「舊訊息在上、新連線在下」：`b` 比 `n` 早＝前端還沒有那個 pane，訊息直接丟掉；比連線晚＝新輸出被舊畫面蓋掉 | R5～R7 |
| `macro-dialog` event 一定要回 `macro_answer` | 巨集的執行緒停在那裡等（每 100ms 檢查中斷）。不回就會一直卡著，使用者看到「巨集不動了」。`statusbox`／`closesbox` 是例外（不等回覆） | T50、T51 |
