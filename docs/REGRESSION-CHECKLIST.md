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
| D. 輸入 / IME | 待填（TASK-003 已有素材：`docs/TERMINAL-JS-DIFF.md` 第二節、`docs/IME-LAB.md`） |
| E. 貼上 | 待填 |
| F. 輸出 / 渲染 | 待填 |
| G. 連線後端（SSH / Telnet / COM / WSL / ADB） | 待填（階段 2） |
| H. log / 巨集 / 我的最愛 | 待填（階段 3） |
| I. 恢復分頁 / 重連 / 保持連線 | 待填（階段 3） |
| J. 代理團隊 / AI 聊天室 / Telegram | 待填（階段 4） |
| K. 安裝 / 更新 / 簽章 | 待填（階段 5） |

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

## D. 輸入 / IME（待填）

素材已經在 `docs/TERMINAL-JS-DIFF.md` 第二節（25 條踩雷 → 落在哪個函式）與
`docs/IME-LAB.md`（九項固定劇本）。等 Windows 基準跑完再把每一條展開成測試步驟。

## E. 貼上（待填）

## F. 輸出 / 渲染（待填）

## G. 連線後端：SSH / Telnet / COM / WSL / ADB（待填，階段 2）

## H. log / 巨集 / 我的最愛（待填，階段 3）

## I. 恢復分頁 / 斷線重連 / 保持連線（待填，階段 3）

## J. 代理團隊 / AI 聊天室 / Telegram 遠端（待填，階段 4）

## K. 安裝 / 更新 / 簽章（待填，階段 5）
