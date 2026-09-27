# 匯入舊版設定：欄位對照表（TASK-016 D）

舊版：`%LOCALAPPDATA%\AwayTerminal\settings.json`（PascalCase、.NET 寫的，可能帶 BOM）。
新版：`%APPDATA%\com.awaysu.awayterminal\settings.json`（camelCase）。
程式在 `src-tauri/src/migrate.rs`。

## 原則

1. **絕對不動舊檔**（只讀）。使用者可能還在用舊版——這個開發團隊本身就跑在舊版底下。
2. **只匯入對得上的欄位**，對不上的列進報告（畫面上看得到，下面也有完整表）。
3. **不匯入「工作階段狀態」**（`SavedTabs`／`History`）：那是「上次關程式時的樣子」，
   匯進來會在第一次啟動就莫名開一堆分頁。
4. **同名的不覆蓋**：使用者已經自己加過的自訂連線／我的最愛保持原樣。

## 什麼時候問

- **第一次啟動**（新版還沒有 `settings.json`，由 `migrate::FirstRun` 在 `setup` 最開頭記下來）
  而且舊檔存在 → 問一次。
- 設定視窗的「舊版設定 → 匯入舊版設定…」隨時可以做，也可以選別台電腦拷過來的檔。

---

## 1. 一樣的欄位（只是大小寫不同）

| 舊版 | 新版 | 備註 |
|---|---|---|
| `FontFamily` | `fontFamily` | |
| `FontSize` | `fontSize` | 超出 6～40 就忽略（新版的 `z` 協定範圍） |
| `Foreground`／`Background` | `foreground`／`background` | 不是 `#RGB`／`#RRGGBB` 就退回預設（新版收窄，見 `docs/SETTINGS.md` 1.4） |
| `ImeQuietMs` | `imeQuietMs` | 夾在 0～150（同舊版） |
| `LogDir`／`LogTimestamp`／`LogAppend` | `logDir`／`logTimestamp`／`logAppend` | |
| `AutoReconnect`／`KeepAliveMins` | `autoReconnect`／`keepAliveMins` | |
| `LastDir` | `lastDir` | |
| `ComPort`／`ComBaud`／`ComDataBits` | `comPort`／`comBaud`／`comDataBits` | |
| `TabPanelVisible`／`TabPanelWidth` | `tabPanelVisible`／`tabPanelWidth` | |
| `ExitRestoreTabs` | `exitRestoreTabs` | |
| `ComposeSendEnter` | `composeSendEnter` | |
| `RestoreBufferLines` | `restoreBufferLines` | |
| `RemoteEnabled`／`TelegramBotToken`／`TelegramChatId`／`RemoteNotify` | 同名 camelCase | **功能是階段 4**，但先存著——不能把使用者的 token 弄丟（PM 指定）。⚠️ token 不進 log、不進 tooltip、不進恢復分頁 |

## 2. 要轉換的

| 舊版 | 新版 | 怎麼轉 |
|---|---|---|
| `Language: "zh"` | `language: "zh-TW"` | 舊版只有中英兩種時的寫法 |
| `Language: "en"` | `language: "en"` | |
| `ComParity: "Mark"`／`"Space"` | `comParity: "None"` | `serialport` 不支援 → **降級並在匯入報告裡提醒** |
| `ComStopBits: "OnePointFive"` | `comStopBits: "One"` | 同上 |
| `ComFlow: "RequestToSendXOnXOff"` | `comFlow: "RequestToSend"` | 同上 |
| `CustomConns[]` | `customConns[]` | 欄位一一對應（`Name`／`Path`／`Args`／`Icon`／`CloseKey`／`CloseCount`／`PickDir`／`Hidden`／`ViaPowerShell`）＋**新版的 `sandbox`**：依新版規則（AI agent 開、WSL／ADB 關） |
| `Favorites[].Tab.Type = "ps"` | `favorites[].kind = "shell"` | |
| `Favorites[].Tab.Type = "claude"`／`"custom"` | `favorites[].kind = "conn"`＋`connName` | 舊版把它們都當自訂連線 |
| `Favorites[].Tab.Type = "ssh"` | `favorites[].kind = "ssh"`＋`ssh{host,port,user}` | keepalive／自動重連用新版目前的設定值填 |
| `Favorites[].Tab.Type = "telnet"` | `favorites[].kind = "telnet"`＋`telnet{host,port}` | 同上 |
| `Favorites[].Tab.Type = "com"` | `favorites[].kind = "com"`＋`com{...}` | 同上 |

## 3. 刻意不匯入（匯入報告會逐條列出原因）

| 舊版欄位 | 為什麼 |
|---|---|
| `SavedTabs` | 上次關程式時開著的分頁＝工作階段狀態，匯進來會莫名開一堆分頁 |
| `History` | 舊版的「紀錄」清單（新版改用我的最愛，TASK-009 決定） |
| `ExplorerMenu` | 新版**直接看登錄檔的實際狀態**，不存這個旗標 |
| `ClaudePath`／`ClaudeArgs`／`ClaudeEnabled`／`ClaudeCommand` | 舊版 v1.0.18 起 Claude 已經是「自訂連線」，會跟著 `CustomConns` 進來 |
| `AdbPath`／`AdbEnabled` | 同上（ADB 也是自訂連線） |
| `Agent*`（10 個欄位） | 代理團隊＝階段 4 |
| `AgentChatFolder` | 同上 |
| `DirBookmarks` | 新版還沒有資料夾書籤 |
| `HostHistory` | 新版用我的最愛取代主機歷史（TASK-009 決定） |
| `LastConnType`／`LastUser`／`LastHost`／`LastSshPort`／`LastTelnetPort` | 「上次連的」由連線對話框自己記；匯進來意義不大（而且 `LastHost` 帶的是內網 IP，不想在第一次啟動就預填） |
| `SeededSamples`／`ClaudeMigratedToCustom` | 舊版自己的旗標 |
| `ExitUpdateMd` | 離開時更新 CLAUDE.md＝代理團隊（階段 4） |
| `ExtraFields` | 舊版自己的「未知欄位保留區」 |
| 代理團隊的我的最愛（`TeamSetup` 非空） | 階段 4 才有對應功能（逐筆列出名稱） |
| `Favorites[].Tab.Type = "adb"` | 新版的我的最愛還沒支援 adb（逐筆列出名稱） |

## 4. 新版才有、舊檔沒有的

`viewMode`、`window`（大小位置）、`palette`、`sshWeakAccepted`、`sandboxDefault`、
`customConns[].sandbox`、`favorites[].kind` 的 `com`／`conn` 細分 —— 都用新版的預設值。

---

## 5. 驗證

`cargo test migrate`（9 條）用一份手寫的舊版 JSON（含中文路徑、`Mark` 同位、
代理團隊的最愛、壞掉的型別）比對結果：

| 測試 | 驗什麼 |
|---|---|
| `imports_matching_fields` | 對得上的欄位都套過去（含 `D:\紀錄\AwayTerminal` 這種中文路徑） |
| `degrades_unsupported_com_values_with_warnings` | 三個 COM 值降級**並且提醒**（不可以安靜改掉） |
| `imports_custom_connections` | 欄位一一對應；沙盒依新版規則（agent 開、WSL 關） |
| `imports_favorites_and_reports_skips` | ps／ssh／com 進來；代理團隊與 adb 跳過並記原因 |
| `lists_intentional_skips` | `SavedTabs`／`History`／`ExplorerMenu`／`ClaudePath`／`AdbPath`／`HostHistory`／`Agent*` 都列進報告 |
| `keeps_telegram_settings` | token 有存下來 |
| `does_not_duplicate_existing_entries` | 同名不重複加、也不覆蓋 |
| `empty_or_garbage_json_changes_nothing` | 空的／型別不對的舊檔不會把設定改壞 |
| `reads_bom_prefixed_file` | 帶 BOM 的檔讀得進來（舊版是 .NET 寫的） |

`--verify` 會用一份臨時的舊版格式 JSON 走一次完整流程（不碰使用者真的舊檔），
驗完把設定還原、把驗證用的自訂連線與我的最愛刪掉。
