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
| K1 | `cargo run --example ssh_probe` | **11 PASS / 0 FAIL** | — | PASS | | |
| K2 | `--verify` 的 SSH 那一步 | 分頁 `kind=ssh`、`backend=russh`；連不上時原因印在終端機；session 正常結束 | 舊版：連不上不可留死分頁 | PASS | | |
| K3 | 👤「新分頁 ▾」→「SSH…」 | 問「主機（可加 :埠，預設 22）」 | ⬜ 完整對話框是 TASK-007 | | | |
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
| K22 | 👤 舊設備（風險 3） | 見 `docs/SSH.md` 第 6 節 S1～S16 | — | ⬜ 等設備清單 | - | - |

## K2. 其餘連線後端：Telnet / COM / WSL / ADB（待填，階段 2）

## L. 巨集 / 我的最愛 / 輸入文字視窗（待填，階段 3）

## M. 恢復分頁 / 斷線重連 / 保持連線（待填，階段 3）

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
