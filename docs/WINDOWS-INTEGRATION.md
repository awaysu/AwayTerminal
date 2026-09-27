# Windows 專屬整合：WSL／ADB／檔案總管右鍵選單／單一執行個體（TASK-016）

舊版對應：`Dialogs/CustomConnDialog.KnownTools`（WSL／ADB）、`MainWindow.OpenAdbFlow`、
`Services/ShellIntegration.cs`、`Services/IpcPipe.cs`、`App.xaml.cs` 的 `--open-dir`。
程式在 `src-tauri/src/{adb,shellmenu}.rs` ＋ `src/adb.js`、`src/setdlg.js`、`src/main.js`。

---

## 1. WSL：**舊版就已經是「自訂連線」**，不是內建選單項目

⚠️ TASK-016 的任務書寫「『新分頁 ▾』加 WSL 子選單列發行版」，但**舊版沒有這個行為**。
我照 `CLAUDE.md` 的規矩去看原始碼：

| 事實 | 出處 |
|---|---|
| WSL 是 `KnownTools` 裡的一筆預設集：`new("WSL", new[]{ "wsl.exe" }, "", "wsl", false)` | `Dialogs/CustomConnDialog.xaml.cs:49` |
| 開它就是開 `wsl.exe`（沒有參數、不選工作目錄） | 同上（args 空、pickDir false） |
| **整個舊版原始碼沒有 `wsl -l`／`-l -v`／`distribution` 這些字** | `grep -rn "\-l -v\|distribution\|distro" reference/AwayTerminal` → 0 筆 |
| 分頁名稱跟著提示行的目前目錄（和 PowerShell／SSH 一樣） | `MainWindow.TracksCwdTitle` 的註解列了「PowerShell / SSH / Telnet / 自訂 shell（WSL、docker bash…）」 |

所以新版**已經有 WSL 了**（TASK-007 搬 `KNOWN_TOOLS` 時就進來了）：
「新分頁 ▾」→ 自訂連線那一區 →`WSL`，或「自訂連線設定…」→「自動偵測」把它加進來。
我的最愛、恢復分頁、狀態燈、關閉鍵都走自訂連線那條共用的路。

### 「列發行版」要不要做？

**舊版沒有，所以不算搬移，是新功能** → 我沒有自己加上去（`CLAUDE.md`：行為相容為優先，
新功能要使用者定案）。真的要做的話這樣做最小：
`wsl.exe -l -q` 拿發行版清單（UTF-16LE 輸出要注意），子選單各開 `wsl.exe -d <名稱>`；
「沒有 WSL 或沒有發行版時不顯示」也照舊版自訂連線那套（偵測不到執行檔就不加入清單）。

| WSL 的行為 | 舊版 | 新版 |
|---|---|---|
| 怎麼列 | **不列**（一個項目，開預設發行版） | 同 |
| 開什麼 | `wsl.exe`（無參數） | 同 |
| 工作目錄 | 不選（`PickDir = false`） | 同 |
| 關閉鍵 | Ctrl+C ×3（`CustomConn` 的預設） | 同 |
| 沙盒 | 沒有沙盒這個功能 | **預設關**（`custom::default_sandbox` 把 WSL／ADB 排除） |
| 分頁名稱 | 跟著提示行的目前目錄 | 同 |
| 沒裝 WSL | 自動偵測找不到 `wsl.exe` → 不加進清單 | 同 |

---

## 2. ADB

和 WSL 一樣是「自訂連線」（v1.0.18 起），**但開的時候走裝置流程**——
`OpenCustom` 看到執行檔叫 `adb` 就轉去 `OpenAdbFlow`，因為直接跑 `adb shell`
在接了兩台以上時只會噴錯。

### 行為對照表

| 項目 | 舊版 | 新版 |
|---|---|---|
| 判斷「這是 adb」 | `IsAdbExe`：**看檔名**（`Path.GetFileNameWithoutExtension == "adb"`），而且 `!ViaPowerShell` | 同（`adb::is_adb_exe` ＋ `src/adb.js` 的 `isAdbConn`） |
| adb 路徑搜尋順序 | 自訂連線填的路徑 → PATH → `ANDROID_HOME`／`ANDROID_SDK_ROOT` → Android Studio 預設位置 → 舊版殘留的 `tools\adb\adb.exe` | 同（`adb::resolve_path`／`candidates`；非 Windows 另找 `~/Library/Android/sdk`、`~/Android/Sdk`） |
| 是否打包 adb | **不打包**（Android SDK 條款 §3.4 禁止轉散布） | 同 |
| 列裝置 | `adb devices`，逾時 5 秒，`CreateNoWindow` | 同（`adb::list_devices`；逾時到就砍**自己開的那個** PID） |
| 解析 | `<序號>\t<狀態>`，**只留 `device`** | 解析一樣，但**全部回傳**（含 `offline`／`unauthorized`）→ 見下面「刻意不同」 |
| 找不到 adb | 說明 ＋ 問「要開啟官方下載頁嗎？」 | 同（`adb.notInstalled` ＋ `platform-tools` 網址） |
| 0 台 | 「沒有偵測到 adb 裝置。」 | 同（外加把 offline／unauthorized 的列出來） |
| 1 台 | **直接開**，分頁名稱 `ADB` | 同 |
| 2 台以上 | 「新分頁」按鈕底下的選單選序號，分頁名稱＝**序號** | 頁內清單對話框（要能顯示不能選的項目），名稱一樣＝序號 |
| 開什麼 | `adb shell`／`adb -s <序號> shell` | 同（`adb::command_line`） |
| 關閉鍵 | Ctrl+C ×3 | 同 |
| 恢復分頁 | 記 `adb.exe` 路徑與序號，**不再跑 `adb devices`** | 同（`SavedTab.adb_path`／`adb_serial`） |

### 刻意不同

| 項目 | 舊版 | 新版 | 為什麼 |
|---|---|---|---|
| `offline`／`unauthorized` 的裝置 | 濾掉 → 畫面顯示「沒有偵測到 adb 裝置」 | **列出來但不能選**，旁邊寫狀態 | 插了沒授權的手機時，舊版的訊息會讓人以為線沒插好。這是**加資訊**，可用的裝置行為完全一樣 |
| 選裝置的 UI | 按鈕底下的 ContextMenu | 頁內清單對話框 | 要能顯示灰掉的項目；也和其他頁內對話框一致 |

⚠️ **已知限制**：這台機器的 `adb` 是 **Microsoft Store 的 app execution alias**
（`%LOCALAPPDATA%\Microsoft\WindowsApps\adb.exe`）。那種行程由 AppX 服務建立，
**不會進分頁的 Job Object**（同 `docs/AGENT-SANDBOX.md` 記的 `pwsh` 那條）→
關分頁不一定收得掉 `adb.exe`。要避開就在自訂連線裡把路徑填成 platform-tools 裡的真檔案。

---

## 3. 檔案總管右鍵選單「用 AwayTerminal 開啟」

### 登錄檔（**只碰 `HKCU`**，免管理員）

| key | 值 |
|---|---|
| `HKCU\Software\Classes\Directory\shell\AwayTerminal` | `(Default)` ＝選單文字（跟著語言）、`Icon` ＝ `"<exe>",0` |
| `HKCU\Software\Classes\Directory\shell\AwayTerminal\command` | `(Default)` ＝ `"<exe>" --open-dir "%V"` |
| `HKCU\Software\Classes\Directory\Background\shell\AwayTerminal` | 同上（在資料夾**內空白處**按右鍵） |
| `HKCU\Software\Classes\Directory\Background\shell\AwayTerminal\command` | 同上 |

逐項照舊版 `ShellIntegration.cs`：兩個位置、`%V`（不是 `%1`——`%V` 在兩種情形都給得出路徑）、
`Icon` 指向 exe 的第 0 個圖示、參數名 **`--open-dir`**（⚠️ 任務書寫的是 `--dir`，
但舊版與已經裝在使用者機器上的右鍵選單用的是 `--open-dir`，照舊版才不會壞）。

- **不碰 `HKLM`**，也不碰別人的 key。移除＝刪掉我們這兩個子樹。
- 勾選狀態**讀登錄檔的實際狀態**，不存進 `settings.json`（使用者可能用別的方式刪過）。
- 路徑永遠指向**目前這支 exe**（搬家／升級後選單不會指到舊位置）。

### ⚠️ 這裡我踩了兩次坑（都已修，寫成回歸項目）

使用者的機器上**本來就有這個 key**（舊版 v1.2.8 每次啟動都重新登錄它）：

1. **單元測試**第一版直接 register/unregister 真的 key → 把選單指到**測試執行檔**
   （`awayterminal_lib-<hash>.exe`）。
2. **`--verify`** 第一版用「重新登錄」實作「復原」→ 把選單指到 **dev 的 exe**。

現在測試與 `--verify` 都用**測試專用的 key 名稱** `AwayTerminal_UnitTest`
（`shellmenu::key_name(sandbox)`），而且 `--verify` 會另外印一行
「使用者的 key 沒被動過」比對 command 字串。兩次都有把使用者的 key 手動復原成
`"C:\Program Files\AwayTerminal\AwayTerminal.exe" --open-dir "%V"`。

### MSIX

舊版就記著：MSIX 版寫的登錄檔會被虛擬化、檔案總管看不到（要 COM 擴充）→
這個功能只對安裝檔／開發版有效。新版一樣。

---

## 4. 單一執行個體與 `--open-dir`

| 舊版 | 新版 |
|---|---|
| Named Pipe `AwayTerminal.OpenDir`（`CurrentUserOnly`，只有同一使用者連得上） | `tauri-plugin-single-instance`（Windows 底層是 mutex ＋ 視窗訊息） |
| 第二個實例連得上就送一行 `open-dir\t<路徑>` 然後**自己結束**、不開第二個視窗 | 同（plugin 把 `argv` 轉給第一個實例，第二個實例自己結束） |
| 連不上（沒有實例在跑）→ 自己正常啟動，在 ready 之後開那個資料夾 | 同（`--open-dir` 走 `LaunchArgs`，`terminal.js` 載完才開，讓它成為作用中分頁） |
| 也接受「單一個存在的資料夾路徑」＝把資料夾拖到 exe 上 | 同（`cli.rs` 的裸參數分支） |
| 資料夾不存在 → 提示 | 同（`dir_exists` ＋ `shell.dirMissing`） |
| 收到之後把視窗拉到前面 | 同（`unminimize` ＋ `show` ＋ `set_focus`） |

mac／Linux 的 Unix domain socket 版本之後再做（`CLAUDE.md` 的平台差異清單裡有這一條）。
Finder Quick Action／Nautilus 腳本也還沒做，只在這裡記著。

---

## 5. 驗證

```
cargo test                        247 passed（adb 5、shellmenu 2、migrate 9）
npm run tauri dev -- -- -- --verify 1
  [verify] ADB（app 端路徑）
  [verify] 檔案總管右鍵選單（只碰 HKCU，而且用測試專用的 key）
  [verify] 匯入舊版設定
```

`--verify` 的實際輸出（2026-09-27，這台機器沒有接手機）：

```
[verify] adb 路徑：C:\Users\Awaysu\AppData\Local\Microsoft\WindowsApps\adb.exe
[verify] 裝置 0 台（可用 0）：(沒有)
[verify] 指定不存在的路徑會退回自動搜尋：true
[verify] 使用者真的那個 key：已登錄=true（"C:\Program Files\AwayTerminal\AwayTerminal.exe" --open-dir "%V"）（只讀）
[verify] 登錄後讀回：已登錄=true、command="…\target\debug\awayterminal.exe" --open-dir "%V"（要有 --open-dir 與 %V：true）
[verify] 移除後讀回：已登錄=false（要 false）
[verify] 測試專用的 key 已清掉：true
[verify] 使用者的 key 沒被動過：true
```

**沒有真機可驗的**：接一台手機（0／1／2 台的三種流程）、右鍵選單真的在檔案總管出現、
第二個實例把資料夾轉交給第一個。這些列進 `docs/REGRESSION-CHECKLIST.md` 的 👤 項目。
