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
| A15 | 分頁列狀態燈：閒置的 shell | 圖示是**淡綠 #A5D6A7**（同一組 PNG，用 `index.html` 的 SVG filter 染色：`tint × 亮度^0.7`，公式同舊版 `IconTint`） | `TerminalTab.ReadyColor` | PASS | | |
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
| C6 | `cargo test --lib settings` | ①**不認識的欄位原樣保留**（降版或安裝版／開發版共用設定檔時不會洗掉 Telegram token——舊版踩雷第 52 條）②解析失敗時**整個不寫回**，原檔不動 | 舊版踩雷第 52 條 | PASS | — | — |

| C6 | 👤 按工具列右端的 ▲ 隱藏分頁列，關掉程式再開 | 還是隱藏的，按鈕顯示 ▼ | `TabPanelToggle_Click` / `TabPanelVisible` | | | |
| C7 | 👤 改視窗大小／位置，關掉程式再開 | 回到上次的大小與位置；上次是最大化就開成最大化（不會把最大化後的尺寸記成還原尺寸） | 新增（舊版固定 `WindowState="Maximized"`） | | | |
| C8 | 把 settings.json 故意改成壞掉的 JSON 再啟動 | 用預設值開起來，而且**不會覆寫**那個檔（`_suppressSave` 同款行為），dev log 有一行解析失敗 | `AppSettings.Load` 的 `_suppressSave` | | | |
| C9 | 程式跑的時候看 settings.json 旁邊 | 不會留下 `.tmp` 半截檔（先寫 tmp 再原子替換） | `AppSettings.Save()` | | | |
| C10 | 用編輯器開 settings.json | UTF-8、沒有 BOM、中文不是亂碼 | 踩雷：「PS5.1 `Get-Content` 會用 Big5 讀壞 settings.json」——**不要**用 PowerShell 5.1 的 `ConvertFrom-Json`/`ConvertTo-Json` 改這個檔 | | | |
| C11 | 👤 **最小化之後從工作列右鍵關掉**程式 → 再開 | 視窗**在螢幕上**（回到最小化前的位置，不是開在看不見的地方）。雷：Windows 最小化時把視窗移到實體座標 `(-32000,-32000)`，還是會發 `Moved`／`Resized`——存下去之後下次啟動照套就開在螢幕外，重開也救不回來（TASK-026 使用者實際回報「工作列有 但沒辦法放大」） | 新增（舊版不記位置） | PASS（2026-09-28 實測，見下方註） | | |
| C12 | 👤 把視窗拖到外接螢幕、關掉程式 → **拔掉外接螢幕**再開 | 視窗置中在剩下的那台螢幕上；dev log 有一行「記住的視窗位置 (x,y) 不在任何螢幕上，改為置中」 | 舊版 `MainWindow.xaml` 的 `WindowStartupLocation="CenterScreen"`（舊版不記位置，所以一定看得到；新版記了位置也不能退步） | | | |
| C13 | `cargo test --lib winpos` | 「矩形有沒有一塊落在某台螢幕上」的判斷：全在螢幕內／完全在外／跨兩台／`-25600` 那個假座標／負座標但在左側副螢幕內／只露一條邊／標題列在畫面上緣外／沒有螢幕清單，共 10 個測試 | 新增（TASK-026） | PASS | — | — |

### C11 的實測（2026-09-28，TASK-026）

整條「最小化 → 從工作列關掉 → 再開」要真人跑（共用桌面的規則：不用會搶焦點的自動化）。
**能自動驗的那一半**已經驗過了——用 `SW_SHOWMINNOACTIVE` 把 dev 視窗最小化（不搶焦點、不送鍵盤滑鼠），再用唯讀的 `GetWindowRect`／`IsIconic` 看狀態：

| 起始 `window` | 最小化時的 `GetWindowRect` | 最小化 3.8 秒後的 `settings.json` |
|---|---|---|
| 修好的版本 `{x:200,y:150}` | `-25600,-25600`、`IsIconic=True` | `{x:200,y:150}`（**沒變**） |
| 把 `is_minimized()` 那一段拿掉的對照組 `{x:200,y:150}` | 同上 | `{x:-25600,y:-25600}`（**就是使用者回報的那一份**） |
| 修好的版本，起始塞 `{x:-25600,y:-25600}` | — | 視窗**置中**開在螢幕上（`rect=-7,-4,1543,834`），log 有「不在任何螢幕上，改為置中」 |

`GetWindowRect` 讀到 −25600 而不是 −32000，是因為查詢用的 PowerShell 不是 DPI-aware，座標被 Windows 除以 1.25 虛擬化過——和使用者設定檔裡那個數字同一個來源。

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
| D15 | 👤 **和舊版並排看工具列** | 每顆按鈕都是**圖上字下**：圖 26×26、字 11px、按鈕寬 72（繁中／簡中的字都短於 72，所以整排和舊版一樣寬）、圓角 4、底 #3A3A3D、滑鼠移上去 #50505A。圖就是舊版那一組 PNG（`new-connecting`／`favorite`／`compose`／`copy`／`paste`／`copy-all`／`clear`／`page-scroll`／`arrange`／`settings`／`about`） | `MainWindow.xaml` 的 `ToolBtn` 樣式（第 16～49 行）與各 Button 的 `Tag` | | | |
| D16 | `npm run verify` 看 `[verify] 圖示` 那幾行 | 工具列「沒圖 0、沒字 0、圖載不到 0」；下拉選單、分頁列、自訂連線挑選器都「載不到 0」；分頁列圖示有 `#tint-ready`／`#tint-busy` 的 filter | 新增（TASK-027） | PASS | | |
| D17 | 切成德文／法文／日文（`Bildschirm löschen`、`Effacer l’écran`、`テキストとして貼り付け`） | 放不下 72 的按鈕會**變寬**（不是截字、也不是換行），工具列放不下時可以橫向捲；圖示照樣置中、整排高度一致 | 刻意與舊版不同（見下方表；舊版固定 72、長字會溢出蓋到隔壁） | | | |
| D18 | 👤 按「新分頁 ▾」／「我的最愛 ▾」 | 每一項都是**圖左字右**（圖 26×26、右邊距 10）：PowerShell／SSH／連接埠／代理團隊／AI聊天室／自訂連線各自的圖／自訂連線設定…（齒輪） | `MakeNewItemRaw`、`New_Click`、`Favorites_Click` | | | |
| D19 | 👤 自訂連線設定 → 看「圖示」那一列 | 舊版 `IconKeys` 那 14 個圖（powershell／ssh-telnet／adb／wsl／git／docker／claude-code／codex／opencode／geminicli／qwen／python／run／none），點一個會框起來，存檔後「新分頁 ▾」與分頁列都換成那個圖 | `CustomConnDialog.IconKeys`／`DefaultIcon = run` | | | |

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
| CM17 | `cargo test com` ＋ 讀 `com/mod.rs` | 輸出走**專屬執行緒的 blocking read**（同 ConPTY 的讀取迴圈），**不可改回事件式**——舊版 `SerialPort.DataReceived` 有延遲，資料要一到就送畫面（舊版踩雷第 29 條） | `SerialSession.cs` | PASS | — | — |

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

## ST. 設定視窗（TASK-015 A）

舊版對應 `Dialogs/SettingsDialog`。欄位對照表、哪些是新版多的、顏色驗證的規則都在
**`docs/SETTINGS.md`**。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| ST1 | `cargo test prefs` | 顏色驗證與字型清單的測試全過 | `ValidColor` | PASS | | |
| ST2 | `--verify` 的設定那一步 | 字級／背景／imeQuiet 存得進去、`T{json}` 有重送、pane 背景跟著變、`'Red'` 退回預設、字級 999 被忽略 | — | PASS | | |
| ST3 | 👤 工具列「其他設定」 | 開頁內對話框，欄位順序＝語言／字體背景顏色／Claude 輸入送出／其他／沙盒模式／檔案總管 | `SettingsDialog.xaml` | | | |
| ST4 | 👤 對話框開起來的值 | 是**目前的設定**（不是預設值） | 建構子讀 `AppSettings.Current` | | | |
| ST5 | 👤 按「取消」 | **什麼都不動**（連剛改的顏色也不套） | `IsCancel` | | | |
| ST6 | 👤 按「回到預設」 | 字型 Cascadia Mono／大小 14／前景 #E0E0E0／背景 #1E1E1E／imeQuiet 20；**語言與其他組不動** | `Reset_Click` | | | |
| ST7 | 👤 顏色欄位旁的小色塊 | 點了開系統選色器；打字改十六進位色碼時色塊跟著變 | `ColorDialog` ＋ `Border` | | | |
| ST8 | 👤 打一個不合法的顏色（例 `Red`、`#12345`）按確定 | 退回預設色，**不跳錯誤** | `ValidColor` | PASS（單元測試＋`--verify`） | | |
| ST9 | 👤 **改字級按確定** | 所有分頁的字**立刻變大／變小**，而且欄數跟著變（`vi`／`top` 不會畫錯） | `PostTheme()` → `applyTheme()` | ⬜ **只能目視**（見下） | | |
| ST10 | 👤 Ctrl+滾輪縮放 | 同 ST9（走同一條 `applyTheme`） | 舊版同 | ⬜ 只能目視 | | |
| ST11 | 👤 改字型成「Consolas」 | 立刻換字型；下拉列得出這台機器有的等寬字型，也可以自己打沒列到的 | `Fonts.SystemFontFamilies` | | | |
| ST12 | 👤 「送出前等待靜止 (ms)」旁的「這是什麼？」 | 跳說明，文字**逐字**和舊版一樣 | `settings.imeQuietHelp` | | | |
| ST13 | 👤 改 imeQuiet 之後在 Claude 分頁打注音 | 行為跟著改（0＝立刻送） | `terminal.js` 的 `QUIET_MS` | | | |
| ST14 | 👤 改 log 預設資料夾 → 開 log | 新位置生效 | `AppSettings.LogDir` | | | |
| ST15 | 👤 按「清除已接受的弱演算法記錄」 | 旁邊的筆數變 0；下次連那台舊設備會**再問一次** | ⬜ 新增 | PASS（`--verify`） | | |
| ST16 | 👤 深色的設定視窗裡點開任一個下拉（語言、渲染器） | 選項**看得清楚**（不是白底灰字）。舊版踩雷第 45 條是 WPF 的隱式樣式滲進 ComboBox；新版是 CSS，症狀可能一樣 | 舊版踩雷第 45 條 | ⬜ | — | — |

| ST16 | 👤 關掉「新增的自訂連線預設開啟沙盒」→ 自訂連線「自動偵測」 | 新加進來的連線沙盒是**關**的；**已存在的不受影響** | ⬜ 新增 | PASS（單元測試） | | |
| ST17 | 👤 檔案總管那一組 | 勾選框是**灰的**，旁邊寫「（這項還沒搬過來）」 | — | | | |
| ST18 | 改設定後重開程式 | 值還在（`settings.json`） | `Save()` | | | |

⚠️ **ST9／ST10 為什麼只能目視**：`--verify` 跑到設定那一步時 pane 的方框是 **0×0**
（那個環境量不到尺寸），`FitAddon.fit()` 因此是 no-op、`term.cols` 停在啟動時的值——
**改字級之前就已經 0×0**，所以不是設定那條路的問題。自動驗證改成驗
「`T{json}` 有到 `terminal.js`」＋「`applyTheme()` 真的跑了（pane 背景變了）」。

## LG. 介面語言（八種；TASK-015 B 修訂版）

清單、退回鏈、Rust 端怎麼拿到字串、怎麼新增一種語言都在 **`docs/SETTINGS.md` 第 2 節**。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| LG1 | `node scripts/test-i18n.mjs` | 八種語言各 **389** 個 key，缺漏／空字串／多餘／參數不符**全部 0**；Rust 端需要的 129 個 key 前端都有 | ⬜ 新增（舊版只有中英） | PASS | | |
| LG2 | `node scripts/i18n-audit.mjs` | 沒歸類的 0 條（Rust 的中文字串都落在字串表／TTL 錯誤表／`println!` 診斷／有理由的例外） | — | PASS | | |
| LG3 | `--verify` 的「八種語言」那一步 | 八個工具列文字**互不相同**、沒有 `undefined`、**後端訊息跟著換**（`sandbox_clear` 的回覆八種語言各一句） | — | PASS | | |
| LG4 | `--verify` 的「中／英介面切換」那一步 | 工具列「複製」→「Copy」、Rust 錯誤訊息跟著換、**搜尋列的字**也跟著換（`T{json}` 有重送） | `Loc.Changed` ＋ `PostTheme()` | PASS | | |
| LG5 | 👤 設定 → 每一種語言各切一次 | **不必重啟**：工具列、選單、右鍵、所有對話框立刻變；**沒有殘留的其他語言**、沒有 `undefined` | `Loc.SetLang` ＋ `ApplyTexts()` | | | |
| LG6 | 👤 切到 **Deutsch**／**Français** | 工具列按鈕不會被截字或擠掉最右邊的分頁列箭頭（文字長 → 按鈕變寬，工具列可**橫向捲**） | ⬜ 新增（舊版沒有這兩種語言） | | | |
| LG7 | 👤 切到 **日本語**／**한국어** | 中日韓字型正常（不會變成方框）；`Aa 範例文字` 那一列看得懂 | — | | | |
| LG8 | 👤 切到 **简体中文** | 用的是大陸慣用詞（设置／粘贴／串口／标签页／宏／主机密钥），不是繁簡字面轉換 | PM 在修訂版指定 | | | |
| LG9 | 👤 切到 **Español** | 對話框（SSH／連接埠／我的最愛／自訂連線／輸入文字／設定／關於）全部是西班牙文 | — | | | |
| LG10 | 👤 任一非中文語言下讓連線失敗（連 127.0.0.1:1） | 終端機裡的錯誤訊息是**那個語言**（不是中文、也不是 `err.xxx`） | — | PASS（`--verify` LG3 每種語言各驗一次） | | |
| LG11 | 👤 任一非中文語言下跑一支有錯的巨集 | 錯誤對話框的訊息是英文（TTL 的 22 條錯誤只有繁中／英文兩份，非中文語言走英文） | `Err::message()` | | | |
| LG12 | 👤 切語言後 Ctrl+F | 搜尋列的提示字與按鈕 tooltip 跟著換（走 `T{json}`） | `PostTheme()` 的 `search` | PASS（LG4） | | |
| LG13 | 👤 第一次啟動（把 `settings.json` 的 `language` 清成 `""` 再開） | 用**系統語言**對到八種之一，對不到用 `en`，並把選到的存回設定 | ⬜ 新增（舊版預設一律繁中） | | | |
| LG14 | 手改 `settings.json` 的 `language` 成 `zh`（舊版寫法） | 當成 `zh-TW`，不會變成英文 | 相容舊設定檔 | PASS（`setLang` 有測試；`--verify` 也走過這條） | | |
| LG15 | 手改成不存在的代碼（例 `xx`） | 退回 `en`，**不會壞掉** | — | PASS（`setLang` 有測試） | | |
| LG16 | 👤 log 的時間戳與分頁 tooltip 的「執行 日:時:分」 | **不跟著語言變**（log 時間戳是相容格式，改了舊 log 就解析不了） | PM 在修訂版指定 | | | |
| LG17 | 👤 語言下拉本身 | 八個選項用**各自語言**的寫法（繁體中文／English／简体中文／日本語／한국어／Español／Deutsch／Français），下面一行「機器翻譯，歡迎修正」 | ⬜ 新增 | | | |
| LG18 | 👤 關於頁 | 有一行「目前語言 ＋ 機器翻譯，歡迎修正」 | ⬜ 新增（PM 在修訂版要求） | | | |
| LG19 | 重開程式 | 記得上次的語言 | `AppSettings.Language` | | | |

## AB. 關於與檢查更新（TASK-015 C）

行為表在 `docs/SETTINGS.md` 第 3 節。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| AB1 | `cargo test update` | URL 參數、解析、壞回應、版本比較共 6 條全過 | `UpdateChecker` | PASS | | |
| AB2 | `--verify` 的更新那一步 | 請求帶 `app=awayterminal2` 與 User-Agent、解析得出版本、**連不上時安靜回 null**；關於頁的 xterm.js 版本＝`package.json` 的版本 | — | PASS（**打 127.0.0.1 的假伺服器，不連真網站**） | | |
| AB3 | 👤 工具列「關於」 | 版式照舊版：名稱／版本／編譯時間／作者／下載／Source Code／授權／第三方元件 | `About_Click` | | | |
| AB4 | 👤 作者那一行 | email 是**圖片**（選不到、複製不到文字） | `RenderTextImage` | | | |
| AB5 | 👤 第三方元件 | xterm.js 的版本是**實際**版本（不是寫死的；舊版寫 5.5.0 而實際 6.0.0） | `CLAUDE.md` 的雷 | PASS（`--verify`） | | |
| AB6 | 👤 展開「完整第三方授權聲明」 | 顯示 `THIRD-PARTY-NOTICES.md` 的內容（**不是複製品**） | ⬜ 新增 | PASS（`--verify` 讀到 11397 字） | | |
| AB7 | 👤 點「下載」「Source Code」 | 用系統瀏覽器開（`awaysu.cc` / `github.com/awaysu/AwayTerminal2`） | `MakeLink` | | | |
| AB8 | 👤 按「檢查更新」（**有網路**） | 按鈕旁顯示「檢查中…」→「已是最新版本 (vX)」或跳「有新版本可用」 | `check.Click` | ⬜ 需網路 ＋ 網站要有 `awayterminal2` 這個代號 | | |
| AB9 | 👤 按「檢查更新」（**拔網路**） | 只顯示一行「檢查失敗（請確認網路後再試）」，**不跳錯誤視窗** | 舊版刻意如此 | PASS（`--verify` 的離線路徑） | | |
| AB10 | 👤 有新版時 | 跳視窗：目前／最新版本＋更新內容，按「前往下載頁」開軟體頁 | `ShowUpdateDialog` | | | |
| AB11 | 啟動程式 | **不會自動查更新**（舊版也不會） | — | PASS（程式裡只有按鈕那條路） | | |

## WS. WSL 分頁（TASK-016 A）

⚠️ **WSL 在舊版就已經是「自訂連線」**，不是內建選單項目，**舊版也不列發行版**
（證據見 `docs/WINDOWS-INTEGRATION.md` 第 1 節）。所以這一組驗的是「自訂連線那條路對 WSL 也成立」。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| WS1 | 「自訂連線設定…」→「自動偵測」 | 這台機器有 `wsl.exe` 就會出現 `WSL` 這一筆（圖示 wsl、參數空、不選工作目錄） | `KnownTools` 第 49 行 | PASS（單元測試 `auto_detect_*`） | — | — |
| WS2 | 👤 沒裝 WSL 的機器 | 自動偵測**不會**加 WSL（找不到執行檔就不加） | 同 | | — | — |
| WS3 | 👤「新分頁 ▾」→ WSL | 開一個 WSL 分頁，拿到 WSL 的提示字元 | `OpenCustom` | | — | — |
| WS4 | 👤 WSL 分頁的名稱 | 跟著提示行的目前目錄（和 PowerShell／SSH 一樣） | `TracksCwdTitle` 的註解列了 WSL | | — | — |
| WS5 | 👤 關 WSL 分頁 | 送 Ctrl+C ×3（`CustomConn` 的預設關閉鍵） | 同 | | — | — |
| WS6 | WSL 的沙盒預設 | **關**（和 ADB 一樣：使用者拿它操作機器，開沙盒只會莫名進到 worktree） | ⬜ 新增（舊版沒有沙盒） | PASS（單元測試） | — | — |
| WS7 | 👤 WSL 分頁存成我的最愛 → 從最愛開 | 照 `conn` 那條路重開 | 舊版同 | | — | — |
| WS8 | 👤 勾了「恢復分頁」關程式 → 重開 | WSL 分頁回來（`kind=conn`） | 舊版同 | | — | — |

## AD. ADB 分頁（TASK-016 B）

行為對照表在 `docs/WINDOWS-INTEGRATION.md` 第 2 節。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| AD1 | `cargo test adb` | 5 條全過（檔名判斷、`adb devices` 解析含 offline／unauthorized、空輸出、命令列格式、候選清單） | `IsAdbExe`／`AdbDevices`／`OpenAdbShell` | PASS | — | — |
| AD2 | `--verify` 的 ADB 那一步 | 印出 adb 路徑與裝置數；**指定不存在的路徑會退回自動搜尋** | `ResolveAdbPath` | PASS | | |
| AD3 | 👤 沒裝 adb 的機器點 ADB | 說明「找不到 adb…」＋問要不要開官方下載頁 | `PromptInstallAdb` | | | |
| AD4 | 👤 有 adb、**沒接手機** | 提示「沒有偵測到 adb 裝置。」，**不開分頁** | `adb.noDevice` | PASS（`--verify` 這台機器就是 0 台） | | |
| AD5 | 👤 接**一台**手機 | **直接開**，分頁名稱 `ADB`，跑的是 `adb shell` | `OpenAdbFlow` 的 1 台分支 | ⬜ 需真機 | | |
| AD6 | 👤 接**兩台以上** | 跳清單選序號，分頁名稱＝**序號**，跑的是 `adb -s <序號> shell` | 同上的 2 台以上分支 | ⬜ 需真機 | | |
| AD7 | 👤 接一台**沒授權**的手機（螢幕上還沒按「允許」） | 清單裡看得到它、**灰的不能選**，旁邊寫 `unauthorized`（**舊版會說「沒有偵測到裝置」**） | 刻意不同 | ⬜ 需真機 | | |
| AD8 | 👤 開著 ADB 分頁把線拔掉 | 分頁不會卡住（`adb shell` 自己結束 → 走 session 結束那條路） | 舊版同 | ⬜ 需真機 | | |
| AD9 | 👤 關 ADB 分頁 | 送 Ctrl+C ×3 | `ConPtySession` 預設 | | | |
| AD10 | 👤 勾「恢復分頁」關程式 → 重開 | ADB 分頁回來，**不再跑 `adb devices`**（用存下來的路徑與序號） | 舊版 1.0.30 | | | |
| AD11 | ⚠️ adb 是 Store 別名時 | 關分頁不一定收得掉 `adb.exe`（AppX 服務開的行程不在 Job Object 裡） | 同 `pwsh` 那條已知限制 | 已知限制 | | |

## EX. 檔案總管右鍵選單與單一執行個體（TASK-016 C）

登錄檔 key 清單在 `docs/WINDOWS-INTEGRATION.md` 第 3 節。**只碰 `HKCU`。**

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| EX1 | `cargo test shellmenu` | 2 條全過；寫入→讀回→刪除**都在測試專用的 key**（`AwayTerminal_UnitTest`） | `ShellIntegration` | PASS | — | — |
| EX2 | `--verify` 的右鍵選單那一步 | 登錄→讀回（有 `--open-dir` 與 `%V`、指向目前的 exe）→移除；結尾印「**使用者的 key 沒被動過**」 | — | PASS | — | — |
| EX3 | 👤 設定 → 檔案總管 → 勾起來 → 確定 | 資料夾右鍵與資料夾內空白處右鍵都出現「用 AwayTerminal 開啟」 | 兩個 `Roots` | | — | — |
| EX4 | 👤 點那個選單項目（**程式沒開**） | 程式啟動並在那個資料夾開一個 PowerShell 分頁 | `--open-dir` ＋ ready 之後開 | | — | — |
| EX5 | 👤 點那個選單項目（**程式已經開著**） | **不開第二個視窗**：現有視窗跳到前面並多一個分頁 | `IpcPipe` → 新版 single-instance plugin | | — | — |
| EX6 | 👤 取消勾選 → 確定 | 右鍵選單消失（兩個位置都刪掉） | `Unregister` | | — | — |
| EX7 | 👤 把資料夾拖到 exe 上 | 同 EX4（裸參數也當成 `--open-dir`） | `App.xaml.cs` 的裸路徑分支 | | — | — |
| EX8 | 👤 選單項目指向被刪掉的資料夾 | 提示找不到資料夾，不開分頁 | `shell.dirMissing` | | — | — |
| EX9 | 登錄檔範圍 | **只有** `HKCU\Software\Classes\Directory\{shell,Background\shell}\AwayTerminal`；`HKLM` 一個都沒碰 | 同舊版 | PASS（測試＋`--verify` 都比對過） | — | — |
| EX10 | 👤 MSIX 版 | 右鍵選單**不會出現**（登錄檔被虛擬化，要 COM 擴充）——舊版就有這個限制 | 舊版註解 | ⬜ 階段 5 | — | — |

## MG. 匯入舊版設定（TASK-016 D）

完整欄位對照表在 **`docs/MIGRATION.md`**。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| MG1 | `cargo test migrate` | 9 條全過（含中文路徑、`Mark` 同位降級、代理團隊的最愛跳過、壞檔不改壞設定、BOM） | — | PASS | — | — |
| MG2 | `--verify` 的匯入那一步 | 用臨時的舊版格式 JSON 走一次：欄位、語言 `zh`→`zh-TW`、中文路徑、COM 降級＋3 個提醒、Telegram token、SSH 最愛的參數、`SavedTabs` 沒匯入 | — | PASS | — | — |
| MG3 | 👤 **第一次啟動**（新版還沒有 settings.json）且舊檔存在 | 問一次「要匯入嗎？」，訊息裡有舊檔路徑與「{n} 條自訂連線、{m} 筆我的最愛」 | ⬜ 新增 | | — | — |
| MG4 | 👤 按「不要」 | 什麼都不匯入，而且**不再問**（新版的 settings.json 一存在就不是第一次啟動了） | ⬜ 新增 | | — | — |
| MG5 | 👤 匯入之後 | 字型／顏色立刻套用、自訂連線出現在「新分頁 ▾」、我的最愛出現在下拉 | ⬜ 新增 | | — | — |
| MG6 | 👤 設定 → 舊版設定 → 匯入舊版設定… | 檔案對話框**預設開在舊版的資料夾**；選了之後畫面上有摘要 | ⬜ 新增 | | — | — |
| MG7 | 👤 匯入兩次 | 第二次不會把自訂連線／我的最愛變成兩份（同名不重複加） | ⬜ 新增 | PASS（單元測試） | — | — |
| MG8 | **舊檔有沒有被動到** | 匯入前後舊檔的內容與修改時間完全一樣（只讀） | PM 指定 | PASS（程式只有 `read_to_string`） | — | — |
| MG9 | 👤 匯入含 `Mark` 同位的舊檔 | 降級成 `None` **並且畫面上看得到提醒** | `docs/COM.md` 第 3 節 | PASS（`--verify` 驗到 3 個提醒） | — | — |
| MG10 | 👤 舊檔裡有代理團隊的我的最愛 | 跳過並在摘要裡列出名稱（階段 4 才有功能） | ⬜ 新增 | PASS（單元測試＋`--verify`） | — | — |

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

## MA. 代理團隊（TASK-017）

行為對照與「為什麼」在 **`docs/MULTI-AGENT.md`**。
👤＝要人看畫面的（`--verify` 驗不到）。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| MA1 | `cargo test agent::` | 全過（含**執行期脈絡與舊版模板逐字比對**） | `RoleLibrary` 等 | PASS | — | — |
| MA2 | `cargo run --example agent_probe` | 27/27 PASS，結束時把 `%TEMP%` 的專案刪掉 | — | PASS | — | — |
| MA3 | `--verify` 的代理團隊那一段 | 建團隊→兩格啟動→角色檔送到→一封信來回→節流→停止→關閉，全部 true | — | PASS | — | — |
| MA4 | 角色檔三層 | `common.md` ＋ `roles/<角色>.md` ＋ `# Runtime Context (generated by AwayTerminal)`，層與層之間一條 `---` | `RoleLibrary.Compose` | PASS | — | — |
| MA5 | 執行期脈絡的每一段 | 身分／`not sub-agents`／`How to send`／`Delivery timing`／`How you receive`／`Always report to`／`Shared desktop`／`When you are stuck`／`Talking to the user` 都在 | `RoleLibrary.RuntimeContext` | PASS（fixture 逐字） | — | — |
| MA6 | 名單欄位對齊 | `  - Agent-11  Product Manager    (ClaudeCode)   <- you`（`PadRight`，依字元數） | 同上 | PASS | — | — |
| MA7 | Solo Mode | 只啟用一格 → 多 `You are the only enabled agent (Solo Mode).`，**沒有**「Always report to」那段，卡住時「tell the user」 | 同上 | PASS | — | — |
| MA8 | 有開 UI/UX Designer | PM／格 1 的角色檔多 `## Design before UI implementation`；設計師自己和其他 worker **沒有** | 同上 | PASS | — | — |
| MA9 | 角色檔自動更新 | 使用者沒改過（雜湊＝`.defaults.json` 的記錄）→ 範本變了就換新版；改過的**不動** | `PreviousDefaults`／`.defaults.json` | PASS | — | — |
| MA10 | CRLF／BOM 不算「改過」 | 只差換行或 BOM 的檔不會被判定成使用者編輯 | `NormalizedHash` | PASS | — | — |
| MA11 | 「還原角色檔預設」 | 內建六個檔回到預設；**自己新增的角色檔不受影響** | `RestoreDefaults` | PASS（單元測試） | — | — |
| MA12 | 信箱檔名編號 | `NNNN-<from>-to-<to>.md`，`NNNN`＝最大值＋1、補到 4 位 | `AgentMessage` | PASS | — | — |
| MA13 | front matter 寬鬆解析 | 缺 `from`／`to` 用檔名補、缺 `type` 當 `INFO`、`agent-12`／`Agent12` → `Agent-12`、壞掉**照樣投遞** | 同上 | PASS | — | — |
| MA14 | 穩定判斷 | 檔案要**穩定 1500ms**（大小與 mtime 沒變）才交出來 | `StableMs` | PASS | — | — |
| MA15 | 不重送 | `.delivered` 記過的檔不會再交出來；重開程式也不會 | `IsDelivered` | PASS | — | — |
| MA16 | 不是本組的信 | 收件人不屬於本組（`Agent-2x`）→ 略過，log 一行 | `Mine()` | PASS | — | — |
| MA17 | 收件人沒在跑 | 記成已投遞，**AwayTerminal 自己寄一封 INFO 回寄件人**（附在跑的名單）；AwayTerminal 自己的信不回 | `OnAgentMessage` | PASS | — | — |
| MA18 | `to: all` | 排進每個收件人的佇列；**最後一個拿到才**記已投遞 | `DeliverQueued` | PASS（單元測試邏輯） | — | — |
| MA19 | 閒置判定（直接跑 exe） | 啟動 ≥5 秒、啟動後有輸出、靜止 ≥2000ms、距投遞／使用者打字／送出各 ≥3 秒 | `AgentReady` | PASS | — | — |
| MA20 | 閒置判定（經 PowerShell） | 改成 ≥10 秒與 ≥3000ms | 同上 | PASS | — | — |
| MA21 | 投遞那一行（一封） | `[AwayTerminal] 訊息 #n from X (task, type)：請讀 …，依你的角色處理，完成後回信給 X。` | `ma.deliverOne` | PASS | — | — |
| MA22 | 投遞那一行（多封） | `你有 k 則新訊息：請依序讀 …`，分隔符中文「、」英文 `, ` | `ma.deliverMany` | PASS | — | — |
| MA23 | 系統通知 | 寄件人是 AwayTerminal → `通知 #n：…（不需要回信）` | `ma.deliverInfo` | PASS | — | — |
| MA24 | 沒有 task 欄位 | 那一行顯示 `(—, TYPE)` | `DeliverQueued` | PASS | — | — |
| MA25 | 文字與 Enter 分開送 | 文字先進去，**300ms 後**才單獨送 `\r`（claude 會把「文字＋CR」當貼上、CR 變軟換行不送出） | `SendTextThenEnter` | PASS | — | — |
| MA26 | Enter 補送 | 打完 10 秒後、對方 2 秒內沒有新輸出 → **只**再送一個 Enter（不重打整行） | `ResendEnterIfSwallowed` | PASS | — | — |
| MA27 | 角色注入那一句不補 Enter | OpenCode／Gemini 的第一句打完就設 `delivery_checked`（舊版實測補送會誤判） | `MultiAgentTick` | PASS（程式碼對照） | — | — |
| MA28 | 節流 | 投遞到上限自動暫停；暫停中**照收信照排隊、不打字** | `PauseAgentGroup` | PASS | — | — |
| MA29 | 右鍵「投遞」選次數 | 改上限；**暫停中**才歸零並繼續（沒暫停只改上限、計數照舊） | `AgentDelivery_Click` | PASS | — | — |
| MA30 | 右鍵「投遞 → 暫停」 | 使用者自己暫停；之後調高上限**不會**自動解除 | `PausedByLimit` | PASS | — | — |
| MA31 | 停止任務 | 每格 `Esc` → **1 秒後 `Ctrl+U`** → **1.5 秒時**打停止句＋Enter | `AgentStop_Click` | PASS | — | — |
| MA32 | 停止流程中不投遞 | 三段都 `MarkTyped`，所以佇列裡的信要等 3 秒後才可能投 | 同上 | PASS（程式碼對照） | — | — |
| MA33 | 閒置檢查 | ≥2 格在跑、都不忙、沒排隊、沒暫停，連續閒置 N 分鐘 → 打一句給 Agent-x1；**不算投遞則數** | `CheckTeamIdle` | PASS（程式碼對照） | — | — |
| MA34 | pane 狀態標籤 | `E{id}US{0..4}`：閒置／忙碌／有信待送／已結束／**忙碌且有信待送**；只在變了才送 | `PostAgentState` | PASS | — | — |
| MA35 | `g` 協定 | `g{下方id}US{比例}US{上列id,…}US{標籤\|…}US{顏色,…}`；標籤＝`Agent-12 · Software Engineer · Codex`，顏色照格號 | `PostAgentGroup` | PASS | — | — |
| MA36 | `G` 協定 | 拖分隔線 → 比例記進團隊（clamp 0.15～0.85），雙擊回 0.5 | `case 'G'` | PASS（`agent_ratio`） | — | — |
| MA37 | 分頁列一組一列 | 只列代表列（最小格號、有分頁那格）；點它回到**最後點過的那一格** | `IsStripRow`／`FocusTargetOf` | PASS（單元測試） | — | — |
| MA38 | 關閉整組 | 分頁列的 ✕ 與右鍵「關閉」都先確認「（N 個 agent 一起關閉）」，然後一起關 | `CloseAgentGroup` | PASS | — | — |
| MA39 | 關掉其中一格 | 組裡還有別格 → 重排（`g` 重送）；最後一格 → 拆組 | `AfterAgentTabRemoved` | PASS | — | — |
| MA40 | 沙盒（團隊） | **一個團隊一棵 worktree**，`.ai/bus/` 在裡面，護欄每家 CLI 寫一次 | `CLAUDE.md` | PASS | — | — |
| MA41 | 一格啟動失敗 | 那一格從名單拿掉、**其餘照開**，剩下幾格的角色檔重組；全失敗＝不開團隊 | `LaunchSlot` 回 null | PASS（程式碼對照） | — | — |
| MA42 | 👤 建團隊視窗 | 欄位：投遞限制／閒置檢查／沙盒（預設開）＋2×2 四格（啟用、Agent ID 帶色、代理人類型、代理人角色）＋還原／開啟角色檔 | `MultiAgentDialog` | ⬜ | — | — |
| MA43 | 👤 格 1 的「啟用」 | 永遠勾著而且**不能取消** | `ui.Enable.IsEnabled = index != 1` | ⬜ | — | — |
| MA44 | 👤 這台沒裝任何 CLI | 視窗顯示「找不到可用的代理人類型…」並停用「開啟」 | `ma.dlgNoBackend` | ⬜ | — | — |
| MA45 | 👤 pane 排版 | 下方全寬＝Agent-x1，上列 x2…x4 平分；外框顏色 1 紅 2 藍 3 綠 4 紫；標題有狀態小標 | `applyGroup` | ⬜ | — | — |
| MA46 | 👤 真的 Claude Code 團隊 | 角色真的生效（它知道自己是誰、知道隊友、會寫信到 `.ai/bus/`） | — | ⬜ | — | — |
| MA47 | 👤 真的 Codex／OpenCode／Gemini | Codex 的 `developer_instructions` 吃得下；OpenCode／Gemini 的第一句打進去後回 READY | — | ⬜ | — | — |
| MA48 | 👤 八語 | 切每一種語言，建團隊視窗、右鍵選單、投遞那一行都跟著換 | — | ⬜ | — | — |
| MA49 | 👤 離開前更新 CLAUDE.md | 有一般 claude 分頁才能勾；勾了會停在對話框顯示「正在請 Claude Code 更新…」；**代理團隊的格不算** | `ExitDialog` | ⬜ | — | — |
| MA50 | 右鍵「代理團隊設定…」→ 改投遞上限 | 換上限；**因為到上限而暫停**且新上限還沒到 → 自動解除（計數**不歸零**） | `ApplyAgentSetup` | PASS | — | — |
| MA51 | 設定…→ 改閒置檢查 | 換值並把「從什麼時候開始閒置」歸零 | 同上 | PASS | — | — |
| MA52 | 設定…→ 勾掉某一格 | 那個分頁關掉、從名單移除、佇列清空 | 同上 | PASS（`--verify`） | — | — |
| MA53 | 設定…→ 加一格 | 用新設定啟動；其餘格不受影響 | 同上 | PASS（`--verify`） | — | — |
| MA54 | 設定…→ 換 CLI 或換角色 | **關掉重開**（角色是啟動時注入的） | 同上 | PASS（`--verify`：格 2 換成 QA 後重讀角色檔） | — | — |
| MA55 | 設定…→ 按「重新啟動」 | 設定沒變也重開 | `WantRestart` | PASS（單元測試 `action_for`） | — | — |
| MA56 | 設定…→ 什麼都沒改按套用 | 當作取消，不重開任何東西 | 同上 | PASS（`changed = false`） | — | — |
| MA57 | 設定…→ 名單變了 | 每格角色檔重組 ＋ 寄一封 INFO 給 PM 要它重讀 Runtime Context | 同上 | PASS（`--verify`） | — | — |
| MA58 | 設定…的過程中不重排 | 關好幾格再開好幾格，中途不拆組（`suspend_relink`） | `_suspendRelink` | PASS | — | — |
| MA59 | 👤 設定…的狀態列 | 每格顯示「執行中／已結束／未執行／未啟用」＋「· 套用後啟動／重新啟動／關閉」 | `RefreshSlot` | ⬜ | — | — |
| MA60 | 👤 設定…會結束對話時先確認 | 「套用後：・Agent-12 · … 會重新啟動」＋「目前的對話就結束了。要套用嗎？」 | `ma.applyAsk` | ⬜ | — | — |
| MA61 | 👤 設定…的沙盒勾選 | 顯示目前狀態但**不能改**（worktree 是建團隊時開的） | 新版限制 | ⬜ | — | — |
| MA62 | 恢復代理團隊分頁 | 同資料夾、**同組號**（沒被占用）、同比例、同上限；兩格都回來 | `RestoreAgentGroup` | PASS（`--verify`） | — | — |
| MA63 | 恢復：Agent ID 沿用 | 角色檔重新組好，裡面的 `Agent ID:` 和上次一樣 | 同上 | PASS（`--verify`） | — | — |
| MA64 | 恢復：畫面倒回 | 上次那一格的 scrollback 回到新 pane（`b` 協定） | 同上 | PASS（`--verify`） | — | — |
| MA65 | 恢復：執行檔沿用 | 照上次的執行檔／參數；**絕對路徑不存在就重新偵測** | `LaunchSlot` 的 `saved` 分支 | PASS（程式碼對照） | — | — |
| MA66 | 恢復：資料夾不見了 | 這一組不恢復、log 一行，其餘分頁照開 | 同上 | PASS | — | — |
| MA67 | 恢復：不重投上次的信 | `.delivered` 在工作區裡，重開後不會再投一次 | `.delivered` | PASS（`--verify` 印出筆數） | — | — |
| MA68 | 👤 恢復後的排版 | `g` 重綁、比例和上次一樣、外框顏色照格號 | 同上 | ⬜ | — | — |

## CH. AI 聊天室（TASK-019）

行為對照與「為什麼」在 **`docs/CHATROOM.md`**。畫面／分頁／沙盒／停止／恢復和代理團隊共用，
所以那些條目看 MA 章節；這裡只列聊天室自己的。👤＝要人看畫面的。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| CH1 | `cargo test agent::chat` | 全過（檔名／輪替／結論時機／`read_finished`／資料夾不撞名／執行期脈絡） | `ChatRoleLibrary`／`ChatRoomTick` | PASS | — | — |
| CH2 | `cargo run --example chat_probe` | 15/15 PASS，結束時把 `%TEMP%` 的專案刪掉 | — | PASS | — | — |
| CH3 | `--verify` 的聊天室那一段 | 建聊天室→三格啟動→給主題→2 回合×3 人→插話→結論→改名→關閉，全部 true | — | PASS | — | — |
| CH4 | 21 個角色 | `chatroom/roles/` 有 21 個，下拉順序＝主持人、反方辯論者、情報研究員、…、感情顧問 | `ChatRoleLibrary.BuiltInRoles` | PASS | — | — |
| CH5 | 角色標題 | 取角色檔第一個 `# ` 標題（主持人／反方辯論者／情報研究員…）；沒有角色＝`-`（**不是** `None`） | `ChatRoleLibrary.TitleOf` | PASS | — | — |
| CH6 | 三層角色檔 | `AI 聊天室 共同規則` ＋ `roles/<角色>.md` ＋ `# Runtime Context（AwayTerminal 產生）` | `ChatRoleLibrary.Compose` | PASS | — | — |
| CH7 | 執行期脈絡是**中文**、和代理團隊完全不同 | 代號／角色／用的 AI／聊天室編號／回合數／參加者／主持人／發言檔路徑／「路徑以那一行為準」 | 同上 | PASS | — | — |
| CH8 | 「你是主持人」那一段 | **只有第 1 位**有（要寫 `conclusion.md`、使用者只跟他說話） | 同上 | PASS | — | — |
| CH9 | 主題寫在脈絡最後 | 有主題才有 `## 這次的主題` | 同上 | PASS | — | — |
| CH10 | 第 1 位固定主持人 | 設定視窗那一格的角色下拉停用、tooltip 是 `chat.hostFixed`；後端也會強制 | `_chat && i == 0` | PASS（後端）／⬜（👤 看畫面） | — | — |
| CH11 | 少於兩位 | 擋下來並顯示「AI 聊天室至少要兩位參加者。」 | `chat.dlgNeedTwo` | PASS | — | — |
| CH12 | 討論紀錄資料夾 | `.ai/chat/<yyyyMMdd-HHmm>/`；同一分鐘再開一場補 `-2` | `NewChatFolder` | PASS | — | — |
| CH13 | 換主題 | 這個資料夾已經有 `transcript.md` → **開新資料夾**，舊的原封不動 | `StartChatDiscussion` | PASS | — | — |
| CH14 | 討論紀錄開頭 | 標題、主題、回合數、參加者名單 | 同上 | PASS | — | — |
| CH15 | 輪流順序 | **依格號**，一回合每人各一次；`r{回合}-Agent-xx.md` | `ChatRoomTick` | PASS | — | — |
| CH16 | 什麼時候打字 | 和代理團隊同一個 `agent_ready`（6 個閘門） | `AgentReady` | PASS | — | — |
| CH17 | 發言接進紀錄 | `## 第 n 回合 · Agent-xx（角色）` ＋ 內容，順序正確 | `WriteTranscript` | PASS | — | — |
| CH18 | 舊檔不算 | mtime 早於「我們開口問 −2 秒」的檔**不採用**（上一場留下的） | `ReadFinished` | PASS | — | — |
| CH19 | 還在寫不算 | mtime 不到 1 秒前的檔不讀（避免讀一半） | 同上 | PASS | — | — |
| CH20 | 逾時跳過 | 5 分鐘沒回應 → 跳過並在紀錄註明；「連問都問不到」從輪到他就開始算 | `TurnTimeoutMinutes` | PASS（程式碼對照） | — | — |
| CH21 | 某格已結束 | **不等 5 分鐘**直接跳過並註明 | 同上 | PASS（程式碼對照） | — | — |
| CH22 | 等發言期間名單變了 | 用 **Agent ID** 找回他，不是索引 | `AskedAgentId` | PASS（程式碼對照） | — | — |
| CH23 | 使用者插話 | 接進紀錄（`## 使用者`），**下一位看得到**；只有討論中／寫結論中能插 | `ChatSay_Click` | PASS | — | — |
| CH24 | 結束條件 | 回合數跑完，或「結束討論」（這一輪結束後就去寫結論，不多問一個人） | `AdvanceChatTurn` | PASS | — | — |
| CH25 | 結論 | 只問主持人；寫 `conclusion.md` → 接進紀錄 → 狀態已結束；逾時 10 分鐘 | `ConcludeChat`／`FinishChat` | PASS | — | — |
| CH26 | 主持人已結束 | 在紀錄註明「沒有結論」並收場 | `chat.trHostGone` | PASS（程式碼對照） | — | — |
| CH27 | 沙盒 | **一間聊天室一棵 worktree**，討論紀錄在裡面 | `CLAUDE.md` | PASS | — | — |
| CH28 | 恢復聊天室分頁 | 同資料夾、同編號、同回合數；**進度不回來**（停在等主題），沿用上次的紀錄資料夾指標 | `RestoreAgentGroup` ＋ 聊天室註解 | PASS（欄位＋程式碼對照） | — | — |
| CH29 | 改名（團隊與聊天室共用） | 組名換掉、代表列分頁標題跟著換、**重綁不會被蓋回去** | `AgentGroup.Title` | PASS（`--verify`） | — | — |
| CH30 | 👤 建聊天室視窗 | 第一列是「討論回合」、**沒有**閒置檢查那一列、欄位名是「使用的 AI／角色」 | `MultiAgentDialog(GroupMode.Chat)` | ⬜ | — | — |
| CH31 | 👤 開好之後問主題 | 多行輸入框（Ctrl+Enter 確定）；按取消也能開，之後右鍵「開始討論…」再給 | `AskChatTopic` | ⬜ | — | — |
| CH32 | 👤 分頁列那一列 | 小標記顯示「人數·第 n/N 回合」；tooltip 顯示進度 | 新增 | ⬜ | — | — |
| CH33 | 👤 pane 排版 | 下方全寬＝主持人，上列是其他人；外框顏色照格號 | `applyGroup` | ⬜ | — | — |
| CH34 | 👤 右鍵選單 | 設定…／開始討論／換主題…／插話…／結束討論／開啟討論紀錄資料夾；插話與結束會依階段變灰 | `chat.menu*` | ⬜ | — | — |
| CH35 | 👤 **真的** AI 聊天室 | 三個真的 CLI 讀完角色檔後真的輪流發言、互相回應、主持人寫出像樣的結論 | — | ⬜ | — | — |
| CH36 | 👤 八語 | 切每一種語言，建聊天室視窗、右鍵選單、打進畫面那一行都跟著換 | — | ⬜ | — | — |

## N. Telegram 遠端

行為對照與「為什麼」在 **`docs/TELEGRAM.md`**。👤＝要人看畫面（或看手機）的。
自動那幾條的來源：`cargo test --lib telegram`（49 個單元測試）與 `npm run verify`
的 Telegram 區段（`telegram_probe`，假 Bot API 在 127.0.0.1，**不連外**）。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| TG1 | `cargo test --lib telegram` | 49 個全過 | `TelegramRemote.cs` | PASS | — | — |
| TG2 | probe：啟動 | 先 `getUpdates?offset=-1`（prime offset）→ `setMyCommands` → 送上線通知 | `PollLoop` | PASS | — | — |
| TG3 | probe：非授權 chat 發指令 | **沒有任何回覆**（sendMessage 次數不變） | `HandleUpdate` 的 chat id 檢查 | PASS | — | — |
| TG4 | probe：`/help` | 回指令一覽（含 `/goto`） | `HelpText` | PASS | — | — |
| TG5 | probe：沒附著就 `/last` | 提示先 `/goto` | `SendLast` | PASS | — | — |
| TG6 | probe：`/goto` 不帶編號 | 列分頁＋inline 按鈕（`goto:n`） | `SendTabList` | PASS | — | — |
| TG7 | probe：按 `goto:1` 按鈕 | 進那個分頁、回「…/exit 離開」 | callback `goto:` | PASS | — | — |
| TG8 | probe：手機打一行字 | 真的進到分頁（畫面上找得到記號） | `SendTextThenEnter` | PASS | — | — |
| TG9 | probe：送出之後 | **不立刻回推**（只靠完成推播，不會一則變三則） | 舊版 1.0.3x 拿掉的固定延遲回推 | PASS | — | — |
| TG10 | probe：`/last 10` | 回 HTML 的 `<pre>` 畫面文字 | `SendLast` | PASS | — | — |
| TG11 | probe：截圖 | 是有效的 PNG（檔頭對、>1KB） | `SendPhotoAsync` | PASS | — | — |
| TG12 | probe：`/shot` | 走 `sendPhoto` | 同上 | PASS | — | — |
| TG13 | probe：5000 字的訊息 | 切成多段、每段 ≤4096 | **v2 新增**（舊版會被 Telegram 退掉） | PASS | — | — |
| TG14 | probe：連續兩次忙→閒、同內容 | 只推一則（8 秒去重） | `OnTabIdle` | PASS | — | — |
| TG15 | probe：`/close` | **先出確認按鈕**，按取消分頁還在 | `HandleClose` | PASS | — | — |
| TG16 | probe：輪詢連續失敗 | 退避 3 秒後恢復，**不會永久停掉** | 1.1.10 的 `TaskCanceledException` 教訓 | PASS | — | — |
| TG17 | probe：token | 不出現在任何一行輸出裡 | 新增的自我檢查 | PASS | — | — |
| TG18 | `cargo test telegram::tidy` | 37 條雜訊規則每條一例都過，且 shell 輸出**原樣通過** | `TidyForPhone` | PASS | — | — |
| TG19 | `cargo test telegram::api::tests::errors_never_leak_the_url` | 錯誤訊息不含 URL（URL 裡有 token） | 新增 | PASS | — | — |
| TG20 | 👤 真的 bot：設定 | 設定視窗貼 token＋chat id → 手機收到「🟢 已開啟」 | `RemoteDialog` | ⬜ | — | — |
| TG21 | 👤 真的 bot：token 欄位 | 重開設定視窗**不回填** token，顯示「（已設定，留空＝不變更）」；留空按確定 token 不變 | 新增（舊版會回填） | ⬜ | — | — |
| TG22 | 👤 真的 bot：關程式 | 手機收到「🔴 已關閉」 | `NotifyOfflineBlocking` | ⬜ | — | — |
| TG23 | 👤 手機問 claude 一句話 | 做完自動推回答案，**只一則**，表格已攤平、讀得懂 | `OnTabIdle`＋`TidyForPhone` | ⬜ | — | — |
| TG24 | 👤 手機回數字選 claude 的選單 | 真的選到那一項（↑／↓＋Enter） | `TryAnswerMenu` | ⬜ | — | — |
| TG25 | 👤 `/shot` | 圖看得懂、中文與框線對齊（**單色**，見下面的差異表） | `SendPhotoAsync` | ⬜ | — | — |
| TG26 | 👤 附著後放 10 分鐘 | 9 分鐘一則警告、10 分鐘靜默離開，分頁沒被關 | `CheckIdleAsync` | ⬜ | — | — |
| TG27 | 👤 在電腦上持續用那個分頁 | 手機一直收得到完成推播（不會因為手機沒動作被自動離開） | `OnTabIdle` 重置閒置計時 | ⬜ | — | — |
| TG28 | 👤 分頁右鍵「推播到 Telegram」 | 取消勾選之後那個分頁不再推（**v2 新增**；遠端沒開時整項隱藏） | 新增 | ⬜ | — | — |
| TG29 | 👤 八語 | 切每一種語言，`/help` 與所有回覆都跟著換（舊版這些是寫死的繁中） | 新增 | ⬜ | — | — |
| TG30 | 👤 代理團隊那一列 | 分頁清單顯示「組名（代理團隊 Agent-11）」，只有代表列會推播 | `RemoteTitle`／`RemoteVisible` | ⬜ | — | — |
| TG31 | probe：`/new` | 列可開的連線＋按鈕，清單含「PowerShell（桌面）」 | `ListConnections` | PASS | — | — |

| TG32 | probe：`/history` | 列清單＋按鈕（v2 ＝我的最愛，舊版是 `History`） | `ListHistory` | PASS | — | — |

| TG33 | probe：`/new 999` | 只回「1~n」提示，**不開任何分頁** | `OpenFromList` | PASS | — | — |

| TG34 | probe：`/new 1` | 真的開了分頁（PowerShell 桌面）並自動附著 | `AttachAndReport` | PASS | — | — |

| TG35 | probe：`/ssh`／`/telnet` 不帶參數且沒有對應的我的最愛 | 回用法說明，**不連任何主機** | `DoSsh`／`DoTelnet` | PASS | — | — |

| TG36 | `cargo test telegram::cmd` | `[user@]主機[:埠]` 解析（含「IPv6 會被切壞」這個舊版限制） | `ParseHostPort` | PASS | — | — |

| TG37 | `cargo test telegram::cmd::tests::help_lists_every_command` | `/help` 有舊版全部 19 條 | `HelpText` | PASS | — | — |

| TG38 | `cargo test ...command_menu_matches_v1_order` | 指令選單順序照舊版（goto/new/history/ssh/telnet 在前） | `RegisterCommandsAsync` | PASS | — | — |

| TG39 | `cargo test ...plain_suffix_never_touches_a_password` | `/plain` 的後綴**不會**加到 SSH 密碼或 shell 指令上 | **v2 新增的防護** | PASS | — | — |

| TG40 | 👤 真的 bot：`/ssh` 開一台真的設備 | 手機上回帳號 → 回密碼 → 進到 shell；**密碼不出現在任何推播裡** | `DoSsh` | ⬜ | — | — |

| TG41 | 👤 真的 bot：`/new` 開一條自訂連線 | 開起來、自動附著、`/follow` 開著時 1.5 秒後推開場畫面 | `AttachAndReport` | ⬜ | — | — |
| TG42 | 👤 真的 bot：看 claude 開場畫面的推播 | 歡迎框的邊線與 spinner 片段都被濾掉。**已知缺口**：歡迎框上緣「邊線＋標題」合併成一行時舊版會漏，新版沿用同一組規則所以可能一樣（舊版踩雷第 54 條標為待辦） | 舊版踩雷第 54 條 | ⬜ | — | — |



## O. 安裝 / 更新 / 簽章（待填，階段 5）

---

## DV. 開發環境與 repo 衛生（自動）

不是產品功能，但**壞了會讓別的驗證變得不可信**，所以列進清單。

| # | 怎麼測 | 預期結果 | 舊版出處 | Win | mac | Linux |
|---|---|---|---|---|---|---|
| DV1 | `node scripts/audit-pitfalls.mjs` | ①舊版踩雷條數還是 55（變了＝舊版更新過，要重新稽核）②每一條都有人工判斷 ③**活體檢查**：含非 ASCII 的 `.ps1` 都有 UTF-8 BOM | 舊版踩雷第 8、47 條 | PASS | — | — |
| DV2 | `node scripts/test-bridge-args.mjs` | `session_create` 的每個資料參數，`bridge.js` 的 `createSession` 都有傳出去 | TASK-011（`com`）、TASK-021（`adb`） | PASS | — | — |
| DV3 | `node scripts/make-manual-plan.mjs --check` | `docs/MANUAL-TEST-PLAN.md` 是最新的（清單改了就要重新產生） | 新增 | PASS | — | — |
| DV4 | `cd src-tauri && cargo deny check` | advisories／bans／licenses／sources 全 ok | 階段 5 的授權義務 | PASS | — | — |
| DV5 | `npm audit --omit=dev` | 0 vulnerabilities | 新增 | PASS | — | — |
| DV6 | `node scripts/test-i18n.mjs` | 八語都沒有缺漏／空字串／參數不符 | 新增（舊版只有中英） | PASS | — | — |
| DV7 | `node scripts/i18n-audit.mjs` | Rust 裡沒有「使用者看得到但沒進字串表」的中文字面 | 新增 | PASS | — | — |
| DV8 | `cargo test --lib version_tests` | 三處版本一致、NOTICES 與 conpty 有進 bundle、八種安裝語言、updater 公鑰仍是空的 | 新增 | PASS | — | — |

---

## 踩雷紀錄覆蓋稽核（`CLAUDE.md` 風險 12 的收尾）

舊版 `reference/AwayTerminal/CLAUDE.md` 的「踩雷紀錄」那一節（第 186–247 行，
**55 條**頂層條目、26 個圈號子項）逐條對照這份清單。TASK-023 做的。

| 結果 | 條數 |
|---|---|
| ✅ 已覆蓋（清單、`docs/TERMINAL-JS-DIFF.md` 第二節、或「隱含契約」表裡有） | **31** |
| ➖ 不適用於新版（技術不同就不存在了） | **17** |
| ➕ **漏掉 → 補上** | **7** |

> **TASK-024 修正**：原本報 33／17／5。第 8 與第 47 條（`.ps1` 含中文要 UTF-8 BOM）
> 我當時判成「已覆蓋在 `docs/DEV-SETUP.md`」，但那份文件其實**只在 log 格式的段落提到
> BOM**，沒有這條踩雷。而且 `scripts/gen-bigfile.ps1` 有 263 個中文字、**沒有 BOM**
> ——我們自己正踩著它。已補文件、加 BOM，並在 `scripts/audit-pitfalls.mjs` 加一節
> 「活體檢查」自動掃所有含非 ASCII 的 `.ps1`。
>
> 這次誤判的原因是**粗篩命中就當覆蓋**：DEV-SETUP 裡的 `BOM` 是 log 格式那一句，
> 關鍵字對上了但講的是別的事。稽核腳本現在把「標成覆蓋但關鍵字找不到」列成警告，
> 而關鍵字也改成更具體的句子（不再是 `BOM` 這種會誤中的字）。

### ➕ 漏掉、這次補上的五條

| 舊版第幾條 | 內容 | 補在哪 |
|---|---|---|
| 29 | **COM 的輸出要用專屬執行緒 blocking read**，不可改回事件式（舊版 `SerialPort.DataReceived` 有延遲） | 新增 CM17（下面 COM 章節） |
| 45 | 深色對話框的下拉選單**灰字配白底看不清**（舊版是 WPF 隱式樣式滲進 ComboBox；新版換成 CSS，但症狀可能一樣） | 新增 ST16 |
| 48 | `Get-AuthenticodeSignature` 回 `UnknownError` 且訊息是「root certificate which is not trusted」**是預期結果**，不是簽章失敗 | `docs/RELEASE.md` 第 3 節 |
| 52 | **舊 exe 會把不認識的設定欄位整組洗掉**（舊版 2026-07-27 中招，Telegram token 被洗掉、遠端靜默 3 小時） | **程式已修**（`AppSettings.extra` ＋ 四條單元測試）＋ 新增 C6 |
| 54 | claude 的 inline 渲染器會在 scrollback 留孤兒行；**歡迎框上緣「邊線＋標題」合併行舊版仍會漏掉**（舊版標為待辦） | 新增 TG42（已知缺口） |
| 8、47 | **`.ps1` 含中文要存成 UTF-8 with BOM**（PowerShell 5.1 沒有 BOM 就用系統 ANSI＝Big5 解碼）。舊版記了兩次，我們**自己正踩著**：`scripts/gen-bigfile.ps1` 263 個中文字、沒有 BOM | `docs/DEV-SETUP.md` 新增一節 ＋ 加上 BOM ＋ `audit-pitfalls.mjs` 的活體檢查 ＋ 新增 DV1 |

第 52 條是這次稽核最有價值的發現——**它對新版一樣成立而且會掉資料**：
使用者降版、或安裝版與開發版共用同一個 `settings.json` 時，舊的 exe 存檔就會把新欄位
（含 Telegram token）洗掉。舊版用 `[JsonExtensionData]` 修，新版現在用
`#[serde(flatten)] extra` 對應，測試 `unknown_fields_survive_a_round_trip` 守著。

順便確認新版在這一條上有兩個地方**比舊版好**：①寫檔是 tmp ＋ 原子替換（舊版 v0.9.82
才加）；②**解析失敗時整個不寫回**（`writable = false`，原檔完全不動），舊版是先備份
`settings.json.bad` 再退預設——我們的做法不會產生「使用者以為設定還在、其實已經是預設值」
的狀態。測試 `a_broken_file_is_never_overwritten` 守著。

### ➖ 不適用的十七條（技術不同）

| 舊版第幾條 | 為什麼不適用 |
|---|---|
| 10 | 「從 UI 執行緒 `_ = SomeAsyncLoop()`」是 WPF SynchronizationContext 的問題。新版對應的雷是「會等前端回覆的 command 一定要 `async`」，已在「隱含契約」表 |
| 17、21、26、27 | 都是**侵入式 UI 自動化**（螢幕座標點擊、`SendKeys`）的踩雷。新版的 `--verify` 完全不搶前景、不點擊（見「隱含契約」的「`--verify` 驗不到需要視窗尺寸的東西」），這一整類不存在 |
| 20 | 「claude 輸入列第一字後空一格」是舊版自己的顯示殘影，新版沒有那段程式 |
| 28 | WebView2 airspace（WPF 疊在 WebView2 上）。新版所有對話框都是頁內 DOM，見「刻意與舊版不同」 |
| 30 | WebView2 快取（改了 `web/` 看起來沒變）。新版 dev 走 Vite HMR、release 是內嵌資源，沒有自己伺服檔案那一層 |
| 34、35、38、39、44 | xterm 黑帶／`windowsPty`／fit 截行／初始尺寸／DPI 髮絲線——都是舊版的 WPF 版面與 xterm 選項組合，新版的版面是 CSS grid ＋ `FitAddon`，這幾條的觸發條件不存在 |
| 41、42 | WinForms（對話框 owner、`ShowDialog` 猜主視窗）。新版沒有 WinForms。**但 42 最後那一條教訓**（「不要等沒有完成時間上限的佇列」）已一般化進「隱含契約」的 async command 那一條 |
| 36 | 「BEL→綠燈機制別加回」＝舊版拿掉的實作，新版從來沒有 |
| 50 | 「憑證私鑰不進 git」——新版的對應規則寫在 `docs/RELEASE.md`（updater 私鑰與簽章憑證都不進 repo），比清單更適合放那裡 |

### ✅ 已覆蓋的三十三條（抽樣對照）

| 舊版第幾條 | 覆蓋在哪 |
|---|---|
| 1（①～⑪，全程式碼自審） | ①SSH 死 session → K 章＋M 章；②`/history` 排除代理團隊／聊天室 → TG32 的說明；③文字＋CR 要分開送 → MA19、TG8；④啟動失敗要有訊息 → AB 章；⑤重連按 Enter 只能一條鏈 → M9；⑥關於頁的 xterm 版本要是真的 → AB3（build 時從 `node_modules` 讀）；⑦選單偵測要看瘦身前的文字 → `menu_needs_the_navigation_line` 測試＋TG24；⑧碎片規則不可吃掉 shell 輸出 → `shell_output_survives` 測試＋TG18；⑨前綴去重只對 TUI 畫面 → 同上；⑩`PrimeOffset` 要 `offset=-1` → TG2；⑪設定讀取失敗要分「讀不到」和「壞掉」→ C 章＋新增的 C6 |
| 2 | 底部固定輸入框讓增量算成「沒有新輸出」→ `finds_new_output_above_a_fixed_input_box` 測試 ＋ TG23 |
| 3、4、5 | claude／Codex 的輸入時序與 `tui.whimsy` → MA19、MA20 |
| 6 | 「Codex 少一行」未重現、舊版也沒修 → 列在 MA 章的已知現象 |
| 7 | 使用者往上捲導致整格看起來空白 → A 章的捲動項 ＋ `S` 協定 |
| 51 | 不要用 `ConvertFrom-Json`／`ConvertTo-Json` 改 `settings.json`（PowerShell 5.1 的預設深度只有 2）→ `docs/DEV-SETUP.md` ＋「隱含契約」。**第 8、47 條見上面的「漏掉」表** |
| 9 | `catch (OperationCanceledException)` 吃掉 HttpClient 逾時 → TG16（輪詢失敗要退避並恢復，不可永久停掉） |
| 11 | 對 claude 送「文字＋CR」一次寫入可能不送出 → MA19、TG8 |
| 12 | `/tui` 重啟丟掉環境變數 → MA 章的已知現象 |
| 13、14、18、19、22、31、32、33 | ConPTY 那一批（OpenConsole 後端、第一幀 `ESC[2J`、std handle 值傳播、逐字節流、殭屍 conhost 鎖資料夾、Win10 不轉送 alt-screen、環境變數、折行是硬換行）→ `docs/PROTOCOL.md`、`pty/conpty.rs` 的註解、A 章與「隱含契約」 |
| 15、23、24、25 | IME／貼上那一批 → **`docs/TERMINAL-JS-DIFF.md` 第二節**逐條對照 ＋ `docs/MANUAL-TEST-PLAN.md` 的 **P0-IME 12 條** |
| 16 | 恢復 scrollback 要在 fit 到最終寬度之後 → R5～R7 ＋「隱含契約」 |
| 29(部分)、37、40、43、46、49 | COM 輸出、關閉慢、控制字元常值、清除畫面要分開送 Esc 與 Ctrl+L、防毒鎖檔 → D13、G 章、`docs/DEV-SETUP.md` |
| 48 | 見「漏掉」表（這次補進 `docs/RELEASE.md`） |
| 53 | 遠端 `/last` 絕不能用原始位元組流去 ANSI → 「隱含契約」有一條專門寫它 ＋ TG8、TG10 |
| 55 | Telegram bot 測試可全自動 → `telegram_probe`（假 Bot API），TG1～TG19 |

> **方法**：先用關鍵字粗篩（45/55 有命中），再逐條人工判斷；沒命中的 10 條全部讀原文
> 才分類。稽核腳本不留在 repo 裡——它是一次性的工具，判斷本身在上面這三張表。

---

## 刻意與舊版不同的地方

回歸比對時會撞到這些差異，**它們都是故意的**，不要當成搬移沒搬完。

| 項目 | 舊版 | 新版 | 為什麼 |
|---|---|---|---|
| 分頁列狀態燈圖示 | `icon/*.png`，`IconTint` 逐像素 `tint × 亮度^0.7` | **同一組 PNG**（`public/icon/`），用 SVG filter 做同一條公式 | TASK-027 改回舊版那組圖（本來自畫 inline SVG，外觀對不起來、使用者一眼看出）。**不能用 CSS mask 染色**：這組圖的 alpha 是整塊圓角底圖、圖形畫在顏色裡，mask 會變成一塊純色方塊 |
| 工具列按鈕寬度 | 固定 72，不換行也不截字（長字直接溢出蓋到隔壁） | `min-width: 72px`，長字把按鈕撐寬，工具列可橫向捲 | 繁中／簡中的字都短於 72＝整排和舊版一模一樣；英文的 `Paste as text`、日文的 `テキストとして貼り付け`、德文的 `Bildschirm löschen` 這幾顆才會變寬。舊版只有中／英兩種語言，碰不到這個問題，而「溢出蓋到隔壁」在網頁上只會更糟 |
| 自訂連線的圖示挑選 | ComboBox 下拉（圖＋key） | 一排可點的圖示鈕 | HTML 的 `<select>` 不能畫圖；key 與順序照舊版 `IconKeys` |
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
| Telegram 截圖 | WPF 把 pane 整塊 render 成點陣圖（**每個字有顏色**） | 前端把畫面文字畫進一張**新的** `<canvas>`（**單色**：前景／背景） | 抓 xterm 自己的 canvas 要打開 WebGL 的 `preserveDrawingBuffer`，每一格都多留一份 buffer、拖慢渲染——而渲染速度正是這一版的重點。平台截圖（`PrintWindow` 之類）需要視窗在前景，團隊規則不准碰前景視窗 → 介面留在 `shot::platform`，階段 5 再評估 |
| Telegram 的介面語言 | 訊息**寫死繁體中文**（沒走 `Loc.T`） | 八語（`tg.*` 共 49 條） | v2 的八語規則對所有使用者看得到的字都成立 |
| Telegram 逐分頁推播 | 沒有（只有全域的 `/notify`） | 分頁右鍵「推播到 Telegram」可單獨關掉 | **新增功能**；只留在記憶體、不進 `settings.json`（分頁 id 跨重啟沒有意義，同逐分頁配色） |
| Telegram 超長訊息 | 直接送（>4096 會被 Telegram 退掉） | 自動切段連送（`split_message`） | 舊版的漏洞，不是刻意行為 |
| Telegram `/new`／`/history` 列出來的東西 | `LastHost` 的 SSH／Telnet ＋ `AppSettings.History`（最多 10 筆） | **我的最愛** | v2 沒有 `LastHost`／`History`——TASK-009 就決定用我的最愛取代主機歷史（`docs/MIGRATION.md` 的跳過表）。指令本身、提示文字、`/history` 不吃純數字都照舊版 |
| SSH 主機金鑰存放 | （舊版沒有，`ssh.exe` 用 `~/.ssh/known_hosts`） | `{app config dir}/known_hosts`，**不碰** `~/.ssh/known_hosts` | 程式不該偷偷寫 OpenSSH 的檔。代價：用 `ssh` 連過的主機這裡仍會問一次 |
| **沙盒模式** | 沒有 | 自訂連線多一個選項，**預設開啟** | **新增功能**，規格＝`CLAUDE.md`「新增功能」一節。說明見 `docs/AGENT-SANDBOX.md` |
| 分頁列的沙盒標記 ⬚ | — | 綠色＝有 worktree、灰色＝只有環境變數與 Job Object | 新增；一個字不占空間又看得出來 |
| 自訂連線「透過 PowerShell 執行」 | 先開互動 PowerShell，**等尺寸就緒後才把指令打進去**（避免以 80 欄啟動） | `pwsh -NoExit -Command "& 'path' args"` | 我們的 PTY 一開始就是前端回報的真實尺寸，沒有 80 欄問題 → 少一套延後打字的機制。使用者看到的結果一樣（工具跑完仍留在 shell 裡）。**⚠️ 若使用者回報某個工具以 80 欄啟動，回頭看這一條** |
| SSH 的 3DES 與 CBC 位置 | PuTTY 把 3DES 放在警告線**之上** | 所有 CBC 與 3DES 都在警告線**之下** | CBC 在 SSH 上有已知攻擊面、3DES 的 64-bit 區塊早就不該當預設。舊設備照樣連得上，只是多一次警告 |
| 代理團隊的信箱監看 | FileSystemWatcher ＋ 輪詢雙保險 | **只輪詢**（3 秒掃一次，穩定判斷仍是 1.5 秒） | agent 寫檔是「整份寫完」；3 秒相對於「等收件人閒置」的秒級等待可以忽略，而且少一個在 mac/Linux 行為差很多的元件 |
| 代理團隊的 Job Object | `CLAUDE.md` 寫「一個團隊一個」 | **一個分頁一個** | 一個團隊一個的話，關掉某一格時它的子孫行程要等整組關掉才收。一個分頁一個同樣保證「整組關掉全部收乾淨」，還多了「關一格就收一格」 |
| 「經 PowerShell 啟動」的判斷 | `tab.Kind == TermKind.PowerShell` | 啟動時把連線的 `via_powershell` 記在那一格 | 我們的自訂連線分頁不會是 PowerShell 這個種類（`kind` 是 claude／custom），照抄會讓「多等一點」永遠不成立 |
| 代理團隊那一列的資訊 | 只有 tooltip | 多一個小標記（agent 數／`✉待投遞`，暫停時變色） | 同沙盒小標記的作法：不占空間又看得出來 |
| 拆組（`u` 協定） | JS 有 `ungroupAll()`，C# 端**沒有呼叫者**（死協定，同 `A` 全選） | 解散團隊時**還有分頁活著**才送 | 否則那些 pane 會卡在一個沒有團隊的 `.agents` 外框裡，還掛著 agent 標籤與狀態小標 |
| 組角色檔失敗 | 只記 log，照樣開 | 建團隊直接失敗 | 角色檔空的話 agent 根本不知道自己是誰，開起來只會白花使用者的額度 |
| 執行期脈絡的「共用桌面」那句 | 寫死 `one Windows desktop` | 依平台換字 | 跨平台 |
| `common.md` 的沙盒段 | 沒有（舊版沒有沙盒） | 多一段 `## Sandbox Mode`（只 `git add` 自己的檔、禁止 `-A`／`-a`／`stash`、`index.lock` 重試一次） | 團隊共用一棵 worktree 是新版才有的狀況。**規則檔其餘一字不動**，那一段有註解標明是新版加的 |
| 既有團隊的沙盒勾選 | （舊版沒有沙盒） | 顯示目前狀態但**不能改** | worktree 是建團隊時開的；中途換掉會讓已經在跑的 agent 的工作目錄和團隊對不上 |
| 聊天室的主題／插話輸入框 | WPF `InputDialog(multiline: true)`，按「確定」送出 | 頁內 `<textarea>`，**Ctrl+Enter 送出** | 頁內對話框沒有「預設按鈕」的概念，而多行輸入不能把 Enter 當送出 |
| 兩套角色庫 | `RoleLibrary` 與 `ChatRoleLibrary` 兩個類別，雜湊機制／三層組合／標題取法各寫一遍 | 一份實作 ＋ `roles::Library` 的 `TEAM`／`CHAT` 兩個常數 | 那兩個類別除了資料夾名、範本、角色順序、沒有角色時的標題之外一模一樣 |
| 聊天室那一列的資訊 | 只有 tooltip | 多一個小標記（人數·第 n/N 回合） | 同代理團隊的作法 |
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
| 設定視窗的欄位 | 只有語言／字型／大小／前景／背景／imeQuiet／檔案總管 | **多一組「其他」與「沙盒模式」** | PM 在 TASK-015 要求「`settings.json` 已有的欄位全部要能從這裡改」。對照表見 `docs/SETTINGS.md` |
| 顏色欄位接受的寫法 | WPF `ColorConverter`（連 `Red` 這種名稱都吃） | **只收 `#RGB`／`#RRGGBB`** | `#` 開頭才保證在 CSS 與 xterm.js 兩邊一致；認不出來退回預設（同舊版的「退回預設」行為） |
| 字型下拉的來源 | `Fonts.SystemFontFamilies`（WPF 列得出全部字型） | **候選清單 ∩ `%WINDIR%\Fonts`**，而且是可自己打的 `<input list>` | 瀏覽器沒有「列出系統字型」的標準做法（`queryLocalFonts` 要權限、WebView2 上不一定有） |
| 關於頁的 xterm.js 版本 | 寫死字串（結果一直印 5.5.0，實際 6.0.0） | **build 時從 `node_modules` 讀** | `CLAUDE.md` 記著這條雷；寫死一定會過期 |
| 關於頁的第三方授權 | 只列幾行元件名稱 | 多一個可展開的區塊，**直接讀 `THIRD-PARTY-NOTICES.md`** | 不想維護兩份；安裝檔本來就要附那個檔 |
| 檢查更新的 `app=` 參數 | `awayterminal` | **`awayterminal2`** | 新版是另一個產品頁。⚠️ 網站後台要先建好這個代號 |
| 介面語言 | 中文／English 兩種（radio） | **八種**（下拉：繁中／English／简中／日本語／한국어／Español／Deutsch／Français） | 使用者在 TASK-015 期間定案。除了繁中與英文之外都是機器翻譯，各語言檔頭、設定視窗、關於頁都註明 |
| 預設語言 | 一律繁中 | **第一次啟動看系統語言**（`sys-locale`），對不到八種就用 `en`；改過就固定 | 新增；舊版沒有偵測 |
| 語言字串的位置 | `Localization/Loc.cs`（一個檔、兩種語言） | `src/lang/<代碼>.js`（一種語言一個檔）＋ `strings.js` 合併 | 八種語言放一個檔會變成幾千行；一檔一語言好改也好加 |
| Rust 端的字串 | 同一個 `Loc.cs` | **前端在啟動與切語言時推 129 條過去**（`i18n_push`），Rust 只留繁中／英文後備 | 翻譯只有一份。寫進終端機畫面的訊息是背景執行緒產生的，沒辦法回代碼讓前端查表（見 `docs/SETTINGS.md` 2.3） |
| ADB 的 `offline`／`unauthorized` 裝置 | 濾掉（畫面說「沒有偵測到 adb 裝置」） | **列出來但灰掉不能選**，旁邊寫狀態 | 插了沒授權的手機時舊版的訊息會讓人以為線沒插好；可用的裝置行為完全一樣 |
| ADB 選裝置的 UI | 「新分頁」按鈕底下的 ContextMenu | 頁內清單對話框 | 要能顯示灰掉的項目，也和其他頁內對話框一致 |
| WSL「列發行版」 | **舊版沒有這個功能**（WSL 是一筆自訂連線，開預設發行版） | 也沒有 | 任務書寫了要列，但那不是搬移而是新功能；`CLAUDE.md` 的規矩是行為相容優先、新功能要使用者定案（`docs/WINDOWS-INTEGRATION.md` 第 1 節有證據與做法） |
| 右鍵選單的勾選狀態 | 存在 `settings.json` 的 `ExplorerMenu` | **直接讀登錄檔的實際狀態** | 使用者可能用別的方式刪過那個 key；讀實際狀態才不會顯示錯 |
| 匯入舊版設定 | 舊版沒有這件事 | 第一次啟動問一次 ＋ 設定視窗一個按鈕 | `CLAUDE.md` 的搬移清單有「匯入舊版 settings.json」 |
| 連接埠對話框的欄位標籤 | 中文介面下也是英文（`Port`／`Baud rate`…） | 繁中／英文照舊；**其他六種語言用該語言的說法**（波特率／ボーレート／Baudrate…） | 舊版那幾個英文標籤是既有行為，不動；新語言沒有「舊行為」要照，就用當地說法 |
| 更新的 `platform=` 參數 | 固定 `windows` | 依平台（`windows`／`macos`／`linux`） | 舊版只有 Windows |

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
| **測試與 `--verify` 不可以動使用者真的在用的系統狀態**（登錄檔、選單、服務…） | 使用者的機器上**已經有**舊版登錄的右鍵選單 key（v1.2.8 每次啟動都重新登錄）。TASK-016 我踩兩次：①單元測試直接 register/unregister 真的 key → 選單指到**測試執行檔**；②`--verify` 用「重新登錄」實作「復原」→ 指到 **dev 的 exe**。現在兩者都走**測試專用的 key 名稱**（`AwayTerminal_UnitTest`），`--verify` 另外印一行比對「使用者的 key 沒被動過」 | EX1、EX2、EX9 |
| **要收掉自己開的行程，只能用 handle（Job Object／PID），而且那個 PID 必須是自己這次開的** | 按名稱砍（`taskkill /IM`、`Stop-Process -Name`）會把整個團隊連自己一起砍掉——這台機器的團隊就跑在舊版 AwayTerminal 底下，而新版 exe 的名稱和舊版一樣。**已知漏洞**：用 **Store 的 app execution alias** 開的行程（很多機器上的 `pwsh`）由 AppX 服務建立，不在我們的 job 裡 → 收不到（`examples/job_probe.rs` 兩種都實測過） | T70～T73、Q7 |
| **切語言之後要把字串推給 Rust，而且 Rust 收到要重送 `T{json}`** | 後端的錯誤訊息與終端機畫面上的字會留在上一個語言（它們不在前端手上）；搜尋列與代理狀態的字也會留著——那包 JSON 是 `theme_json()` 用 `t()` 取的。`--verify` 抓到過：切成英文之後搜尋列還是「搜尋」 | LG3、LG4、LG12 |
| **介面文字改了之後要讓每個模組重套一次**（`i18n.js` 的 `onLangChange`） | 舊版的對話框是「每次開啟才建立」的，所以建構子讀 `Loc.T` 就自動是新語言；我們的對話框是**一直存在的 DOM**，只在 init 設一次文字 → 切語言之後那個對話框永遠是舊語言。加新的頁內對話框時一定要 `onLangChange(applyTexts)` | LG3、LG4 |
| **`--verify` 驗不到「需要視窗有尺寸」的東西** | 跑到後面幾步時 pane 的方框是 `0×0`，`FitAddon.fit()` 變成 no-op、`term.cols` 停在啟動時的值。拿 cols 當判斷就會得到「永遠沒變」的假失敗（TASK-015 花了好幾輪才確認**改字級之前就已經 0×0**）。這類項目要列進 👤 目視清單 | ST9、ST10 |
| **`--verify` 的步驟之間，前一步在畫面上留下的重畫要等它畫完** | 上一步的 Ctrl+C 讓 PSReadLine 重畫（印中斷的那一行＋新的提示字元），**蓋掉**下一步剛用 `writeOutput` 寫進畫面的記號 → 恢復分頁那兩條變成假失敗（TASK-014 實際踩到：`verifyCompose` 送完 Ctrl+C 沒等就跑 `verifyRestore`） | R2、R4、CP2 |
| 會等前端回覆的 tauri command **一定要是 `async`** | tauri 2 的同步 command 跑在**主執行緒**上；擋住主執行緒 webview 的 IPC 就進不來，前端永遠沒機會回答 → 一定逾時。實際案例：`exit_confirm` 要等 `a…save`，第一版寫成同步 → 「存下 2 個分頁」卻一個畫面都沒存到 | R1～R4 |
| 恢復畫面的 `b{id}` 一定要在 `n{id}` 之後、`s{id}` 與啟動連線之前 | 順序錯了就不是「舊訊息在上、新連線在下」：`b` 比 `n` 早＝前端還沒有那個 pane，訊息直接丟掉；比連線晚＝新輸出被舊畫面蓋掉 | R5～R7 |
| **投遞只能在收件人閒置時做**，而且「閒置」是 6 個條件同時成立 | 少一條就會把字打進正在工作的 CLI：被當成它自己訊息的一部分、或整段被吃掉。舊版實測抓到的兩個極端：①Codex 的星星閃爍動畫讓畫面永遠不靜止（→ `tui.whimsy=false`）②經 PowerShell 啟動時提示行出來後 node 還在載入（→ 多等一點） | MA19、MA20 |
| **代理團隊的測試一律用假 agent，而且不碰使用者的資料目錄** | ①啟動真的 claude／codex 會花掉使用者的額度、還會碰到真的登入狀態；②角色檔寫在 `<設定資料夾>/multiagent/sessions/<組號>/`，`--verify` 建團隊時會 `clear_session()`——**使用者此刻正開著同組號的團隊就會被刪掉**（第一次跑 `--verify` 實際寫進了真的資料目錄，之後改成 `%TEMP%` 覆寫）。`agent_verify_begin` 同時覆寫「要跑哪支 exe」與「資料目錄」，`agent_verify_end` 兩個都清掉 | MA2、MA3 |
| **假 agent 要從角色檔知道自己是誰** | 給它寫死的預設 ID，兩格都會以為自己是 `Agent-11` → 回信寄錯人（`0002-Agent-11-to-Agent-11.md`）。真的 agent 也是從角色檔的 `Agent ID:` 那一行知道的，測試替身要照做（第一次 `--verify` 抓到） | MA3 |
| **判斷「這是哪一套」不能用 `std::ptr::eq` 比對 `const` 的位址** | Rust 的 `const` 是**每個使用點各自 inline 一份**，`&CHAT` 在不同地方可能是不同位址 → 比對隨機失敗。實際案例：`roles::compose` 用 `ptr::eq(lib, &CHAT)` 決定要組哪一種執行期脈絡，結果聊天室的角色檔被組成代理團隊的英文脈絡（`chat_probe` 抓到）。改成 `Library` 上的明確旗標 `chat: bool` | CH6、CH7 |
| **兩種角色檔的欄位名不一樣，凡是「讀角色檔」的程式都要認兩種** | 代理團隊是英文（`Agent ID:`／`Role:`／`# Runtime Context (generated by AwayTerminal)`），聊天室是中文（`你的代號：`／`你的角色：`／`# Runtime Context（AwayTerminal 產生）`）。只認一種的後果：①假 agent 三格都以為自己是 `Agent-11`（回信／發言寄錯人）②`--verify` 的角色檔檢查全部空白。`fake_agent.rs` 與 `agent_verify_state` 都已改成兩種都認 | CH3、MA3 |
| **改了分頁標題要送 `tab-state`，不只是 `t{id}`** | `t{id}` 只更新 `terminal.js` 的 pane 標題；我們的分頁列讀的是 `tab-state` event。改名之後沒送＝那一列還是舊名字（`--verify` 抓到：組名已經是「verify 聊天室」，分頁列還寫著資料夾名） | CH29 |
| **`--verify` 每一段要有自己的 `%TEMP%` 子資料夾** | 本來照行程 id 取名，所以幾段共用同一個資料夾：前一段結束時刪掉、後一段再建回來。順序一改（或多加一段）就會互相踩。TASK-019 起是 `awayterm-verify-team-<pid>\<段名>` | MA3、CH3 |
| **`--verify` 一定要走 `npm run verify`（`scripts/dev-verify.mjs`）** | 直接用 `timeout` 包 `npx tauri dev`，逾時只砍最外層，留下 `npx → cli → vite（佔著 1420）` 與 `target\debug\awayterminal.exe → OpenConsole.exe` 一整串孤兒；那個 `awayterminal.exe` 抓著 `target\debug`，**下一次 `cargo build` 就 `os error 32`**（TASK-017 留了一隻，PM 的建置直接跑不動）。包裝收尾時用 `taskkill /PID <pid> /T /F` 依 PID 收整棵樹——**絕不依名稱**，依名稱會把使用者的 AwayTerminal 和正在跑的代理團隊一起砍掉 | MA2、MA3 |
| **測試的 `%TEMP%` 資料夾要用 Drop 守衛刪，不是在最後一行刪** | assert 失敗時那一行跑不到，資料夾就留著（TASK-017 留下兩個 `awayterm-roles-compose-*`）。`roles.rs` 的測試改用 `struct TempDir` + `impl Drop` | MA1 |
| **剛關掉的 PTY 還占著資料夾** | 優雅結束鍵 60ms ＋ 收行程的執行緒還在跑 → 馬上刪 `%TEMP%` 會拿到 `os error 32`。要等一下並重試（`agent_verify_end` 重試 10 次 × 400ms） | MA3 |
| `macro-dialog` event 一定要回 `macro_answer` | 巨集的執行緒停在那裡等（每 100ms 檢查中斷）。不回就會一直卡著，使用者看到「巨集不動了」。`statusbox`／`closesbox` 是例外（不等回覆） | T50、T51 |
| **遠端要畫面上的文字只能走 `q…text`，不可以拿位元組流去 ANSI** | 位元組流裡是 claude／codex 逐格重繪的控制序列，去掉 ANSI 得到一團重複的垃圾（同一行十幾個不同寬度的版本）＝選單偵測對不到、手機看到亂碼。舊版 `CLAUDE.md` 明文寫過這條雷，而它的來源正是舊版自己的「UI 執行緒卡住就退回位元組流」備援 → v2 **不做那個備援**，逾時就回一句「畫面尚未就緒」 | TG8、TG10 |
| **含 token 的 URL 絕不可以進錯誤訊息** | `ureq::Error` 的 `to_string()` 在某些變體會帶上完整 URL，而 Bot API 的 URL 就是 `…/bot<token>/method` → 一次輪詢失敗就把 token 印進 log。`api.rs` 的 `describe()` 只留錯誤型別與 HTTP 狀態，有單元測試守著 | TG17、TG19 |
| **`std::sync::Mutex` 的 `lock()` 在同一個運算式裡只能出現一次** | Rust 的臨時值活到**整條敘述結束**，所以 `f(g.lock().a, g.lock().b)`、`g.lock().x == 0 && g.lock().y > 0` 都是拿著鎖再去搶同一個鎖＝當場鎖死，而且**編譯器不會警告**。實際案例：`telegram_probe` 的兩行檢查這樣寫 → probe 抓著假 Bot API 的狀態鎖不放 → 每個連線都卡在 `lock()` → 每次 `getUpdates` 都逾時、整段 `--verify` 卡到逾時才收工（看起來像「假伺服器壞了」，其實是呼叫端）。要先把值綁進區域變數，或加只鎖一次的小 getter | TG2、TG16 |
| **`telegram-open` 事件一定要回 `telegram_opened`** | 遠端的輪詢執行緒停在那裡等分頁 id（`session_create` 需要前端才有的 `Channel`，Rust 生不出來）。不回就等到 8 秒逾時，使用者在手機上看到「開啟失敗」。和 `ssh-hostkey` → `ssh_hostkey_answer`、`macro-dialog` → `macro_answer` 同一類 | TG34 |
| **`session_create` 加了參數，`bridge.js` 的 `createSession` 也要傳** | Rust 收到 `None` 會安靜地走預設值，**不會報錯**，所以型別與編譯器都抓不到。已經發生兩次：TASK-011 漏 `com`（選了別的埠沒有作用）、TASK-021 漏 `adb`（多台裝置時選好的序號被丟掉、`adb shell` 失敗）。`scripts/test-bridge-args.mjs` 現在會比對兩邊 | AD3、CM1 |
| **`--verify` 的每一段要各自包 try/catch** | 一段丟例外會讓**後面整批不跑**，而且畫面上看不出來（那幾段的 `[verify]` 行根本不存在），看起來像「跑完了、都沒問題」。TASK-021 在 release exe 上踩到：代理團隊／聊天室／Telegram 三段完全沒跑。現在每段各自包起來、最後印一行「幾段丟例外」 | 全部 |
| **視窗位置存檔前一定要先問 `is_minimized()`** | Windows 最小化時把視窗移到實體座標 `(-32000,-32000)` 並照樣發 `Moved`／`Resized`（125% DPI 下換算成 −25600）。存進去之後下次啟動 `set_position` 到螢幕外 → 又發 `Moved` → 再存一次同樣的座標，**自我延續，使用者重開也救不回來**（工作列有圖示、點了沒畫面）。同理 `maximized` 這時候也讀不準，所以最小化時整個不記。啟動端要再驗一次「這個矩形還在某台螢幕上嗎」，不然拔掉外接螢幕也會中 | C11、C12、C13 |
| **假伺服器的「等某個呼叫出現」一定要有起點** | 從第 0 筆開始掃會match到**很久以前**的訊息。`telegram_probe` 的 `/new 1` 檢查等含 `/last` 的回覆，結果立刻match到前面 `goto:1` 送的「已進入 …/last 看輸出」→ 分頁還沒建好就回傳（換個順序就會變成**假通過**）。`FakeBot::wait` 現在強制要傳 `from` | TG34 |
