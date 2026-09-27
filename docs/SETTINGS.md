# 設定視窗、中／英介面、關於（TASK-015）

舊版對應：`Dialogs/SettingsDialog`、`Localization/Loc.cs`、`MainWindow.About_Click`、
`Services/UpdateChecker.cs`。程式在 `src-tauri/src/{prefs,update,i18n}.rs` ＋
`src/{setdlg,about,i18n,strings}.js`。

---

## 1. 設定欄位對照表

### 1.1 舊版設定視窗有的（欄位、順序、預設值照抄）

| 舊版欄位 | 舊版控制項 | 新版 | 預設 | 套用時機 |
|---|---|---|---|---|
| 語言 中文／English | `ZhRadio`／`EnRadio` | 同（radio） | `zh` | **按確定**（不必重啟，見第 2 節） |
| 字型 | `FontCombo`（可編輯下拉，`Fonts.SystemFontFamilies`） | `<input list>` ＋ `font_list()` | `Cascadia Mono` | 按確定 → `T{json}` 即時套到所有分頁 |
| 大小 | `SizeCombo`（8～28，可自己打） | `<input type=number>` 6～40 | `14` | 同上 |
| 文字顏色 | `FgBox` ＋色塊（`ColorDialog`） | 文字框 ＋ `<input type=color>` | `#E0E0E0` | 同上 |
| 背景顏色 | `BgBox` ＋色塊 | 同上 | `#1E1E1E` | 同上 |
| 送出前等待靜止 (ms) | `ImeQuietBox`（0～150） | 同 | `20` | 同上（`terminal.js` 的 `QUIET_MS`） |
| 「這是什麼？」說明 | `MessageBox` | 頁內對話框（文字**逐字照抄**） | — | — |
| 資料夾右鍵選單「用 AwayTerminal 開啟」 | `ShellMenuCheck` | **有欄位但灰掉**，旁邊寫「（這項還沒搬過來）」 | — | 見第 5 節 |
| 回到預設 | `ResetBtn` | 同（**只重設字型／大小／前景／背景／imeQuiet**，和舊版一樣不動語言） | — | — |
| 確定／取消 | `OkBtn`／`CancelBtn` | 同（**取消＝什麼都不動**，不做即時預覽） | — | — |

### 1.2 新版多的（舊版只能手改 `settings.json`）

PM 在 TASK-015 A2 要求「`settings.json` 已有的欄位全部要能從這裡改」。

| 欄位 | `settings.json` 的 key | 預設 | 備註 |
|---|---|---|---|
| 恢復分頁保留的行數 | `restoreBufferLines` | `2000` | 0＝不保留畫面紀錄 |
| 關閉程式時預設勾「恢復分頁」 | `exitRestoreTabs` | `true` | 離開對話框的勾選初值 |
| 保持連線（分鐘，0＝關） | `keepAliveMins` | `10` | 新連線的預設值 |
| 新連線預設開啟斷線自動重連 | `autoReconnect` | `false` | 同上 |
| log 預設資料夾 | `logDir` | 我的文件\AwayTerminalLogs | 空白時由後端補 |
| log 每行加時間戳 | `logTimestamp` | `true` | |
| log 檔已存在時附加在後面 | `logAppend` | `true` | |
| 新增的自訂連線預設開啟沙盒 | `sandboxDefault` | `true` | **新欄位**；WSL／ADB 仍然預設關（`custom::default_sandbox`） |
| 清除已接受的弱演算法記錄 | `sshWeakAccepted` | — | 只給一個**清除鈕**（記錄型欄位不給編輯 UI，PM 定） |

### 1.3 刻意不放進設定視窗的

| `settings.json` 的 key | 為什麼 |
|---|---|
| `window`（大小位置）、`viewMode`、`tabPanelVisible`／`tabPanelWidth`、`palette` | 使用者用滑鼠操作就會存，不必有欄位 |
| `favorites`、`customConns`、`savedTabs` | 各自有自己的對話框（我的最愛／自訂連線／恢復分頁） |
| `comPort`／`comBaud`…、`lastDir`、`composeSendEnter` | 「上次用的值」——由對應的對話框自己記 |
| `sshWeakAccepted` 的內容 | 見上（只給清除） |

### 1.4 顏色驗證

舊版 `ValidColor` 用 WPF 的 `ColorConverter`，連 `Red` 這種名稱都吃。
新版**只收 `#RGB`／`#RRGGBB`**，認不出來就退回預設——`#` 開頭才保證在 CSS 與
xterm.js 兩邊一致（`prefs::valid_color`，有測試）。

---

## 2. i18n 的做法與 Rust 端字串清單

### 2.1 兩邊各有一份字串表

| | 前端 | Rust 端 |
|---|---|---|
| 檔案 | `src/strings.js`（`'key': [繁中, English]`） | `src-tauri/src/i18n.rs`（`(key, 繁中, English)`） |
| 取用 | `T['key']`（Proxy，存取時才挑語言）、`fmt('key', 參數)` | `t("key")`、`tf("key", &[參數])` |
| 切換 | `applyLang('en')` → 每個模組的 `applyTexts()` 重跑一次 | `i18n::set_lang("en")`（`AtomicU8`，背景執行緒也讀得到） |
| 參數 | `{0}`／`{1}` | 同 |
| 查不到 key | 回 key 本身 | 回 key 本身（debug build 會先 `debug_assert` 叫） |

**為什麼 Rust 端不用「回代碼給前端查表」**（PM 要求二選一並寫理由）：

1. **漏掉的代價**：回代碼漏改一處，使用者會看到 `err.connNotFound` 這種字（等於壞掉）；
   Rust 端自己查表漏改一處只是那一句留在繁中（難看但看得懂）。
2. **不必動協定**：錯誤字串是 `Result<_, String>` 的 `Err`、終端機訊息是直接寫進 pane 的
   位元組。改成代碼要動 `bridge.js`、每個對話框、`--verify` 的比對——改動面更大。
3. **有些訊息翻不動**：它們帶作業系統的原文（`開啟 COM5 失敗：系統找不到指定的檔案。`），
   前端查表也翻不了後半段。
4. `ttl/error.rs` 本來就有 `message()`（英文，照 `errdlg.cpp`）與 `message_zh()` 兩份，
   只要多一個 `message_for_lang()` 就好，不必把 22 個錯誤碼送到前端。

### 2.2 Rust 端字串清單（哪些翻、哪些不翻）

| 種類 | 翻不翻 | 數量 | 為什麼 |
|---|---|---|---|
| `Err(...)` 回給前端的（對話框／提示） | ✅ | — | 使用者直接看到 |
| 寫進終端機畫面的（重連倒數、SSH 狀態、巨集結束、COM 降級警告） | ✅ | — | 同上 |
| 檔案選擇／存檔對話框的標題與篩選器 | ✅ | — | 同上 |
| `T{json}`（搜尋列、代理狀態標籤） | ✅ | — | 前端直接顯示 |
| TTL 的 22 條錯誤訊息 | ✅（本來就有兩份） | 22 | `errdlg.cpp` 的英文＋新版的繁中 |
| `println!("[AwayTerminal] …")` | ❌ | 57 | **開發診斷**：打包後沒有 stdout；`--verify` 與踩雷紀錄都在比對這些字串 |
| 例外（寫進 `.git/info/exclude` 的註解、字型 fallback 清單、`--verify` 專用訊息） | ❌ | 12 | 不是介面文字 |

**這張表是可以重新產生的**：`node scripts/i18n-audit.mjs` 會掃過
`src-tauri/src/**/*.rs`，把每一條含中文的字串字面歸類，**出現沒歸類的就 exit 1**。
2026-09-27 的結果：字串表 130 條、TTL 錯誤 22 條、`println!` 57 條、例外 12 條、
**沒歸類的 0 條**。

### 2.3 前端字串

`src/strings.js` 共 **265** 個 key。英文的來源：

- **93 個**舊版 `Loc.cs` 有同名 key → 英文**照抄**（連標點與大小寫）。
  其中 5 個的中文和舊版不一樣（例：舊版 `tb.new` 是「新連接」，我們是「新分頁」），
  英文照我們的中文調整。
- **其餘**是新版才有的東西（內建 SSH／Telnet／COM 對話框、TTL 巨集、沙盒、
  恢復分頁、設定、關於）→ 自己寫。

切語言**不必重啟**（同舊版）：`applyLang()` 會叫每個模組的 `applyTexts()`；
Rust 端的 `set_lang()` 同時換掉後端訊息與 `T{json}` 裡的字。

---

## 3. 關於頁與更新檢查

### 3.1 關於頁

| 舊版 | 新版 |
|---|---|
| 標題 AwayTerminal、版本、編譯時間 | 同（編譯時間＝**exe 的檔案寫入時間**，同舊版） |
| 作者：名字**畫成圖片**（避免 email 被爬） | 同（canvas 畫，DOM 裡沒有可選取的 email 文字） |
| 下載連結、Source Code 連結 | 同（Source Code 改成 `awaysu/AwayTerminal2`） |
| 授權：MIT © 2026 Chih-Wei Su (Awaysu) | 同 |
| 第三方元件：`xterm.js 6.0.0 (MIT)`、`.NET 9／WebView2` | xterm.js **實際版本**（見下）、Tauri、russh、serialport-rs |
| — | **新增**：可展開的「完整第三方授權聲明」＝直接讀 `THIRD-PARTY-NOTICES.md`（不複製一份） |
| 「檢查更新」按鈕 ＋ 狀態文字 | 同 |

**xterm.js 的版本是 build 時從 `node_modules/@xterm/xterm/package.json` 讀的**
（`build.rs` → `env!("XTERM_VERSION")`）。舊版把它寫死成 5.5.0 而實際是 6.0.0，
`CLAUDE.md` 記著這條雷——寫死就一定會過期。

### 3.2 更新檢查行為表

| 項目 | 舊版 | 新版 |
|---|---|---|
| 什麼時候查 | **只有按下「檢查更新」**（啟動時不自動查） | 同 |
| 查哪裡 | `GET https://www.awaysu.cc/software/api.php?action=check_update&app=awayterminal&platform=windows&version=<版本>` | 同一支 API，`app=`**`awayterminal2`**、`platform` 依平台（windows／macos／linux） |
| User-Agent | `AwayTerminal/<版本>` | 同 |
| 逾時 | 10 秒 | 同 |
| 版本比較 | 伺服器的 `update_available`；沒帶才自己比（逐段數字、補 0、`-beta` 視為較小） | 同（`update::compare`，9 條測試） |
| 沒有新版 | 按鈕旁一行「已是最新版本 (vX)」 | 同 |
| **失敗（離線／`ok:false`／JSON 壞掉）** | 按鈕旁一行「檢查失敗（請確認網路後再試）」，**不跳錯誤視窗** | 同 |
| 有新版 | 跳視窗：目前／最新版本＋更新內容（可捲動）＋「前往下載頁」 | 同 |
| 下載 | 開軟體頁，讓使用者自己選安裝版／免安裝版 | 同 |
| 自動下載安裝 | 沒有 | **這次也沒有**（Tauri updater 是階段 5） |

⚠️ **要請使用者確認**：`app=awayterminal2` 這個「參數代號」要先在
`awaysu.cc/software` 後台建好，否則 API 會回 `ok:false` → 畫面顯示「檢查失敗」。
新版的下載頁網址也還沒定（目前先用舊版那頁）。

### 3.3 只有這一個功能會連外

`ureq`（MIT OR Apache-2.0，rustls + webpki-roots）只有 `update_check` 用。
`--verify` **不會**打真的網站：`update_verify` 在 127.0.0.1 開一個只回一次的假伺服器，
再對一個沒人聽的 port 打一次證明失敗是安靜的。

---

## 4. 驗證

```
cargo test                          229 passed（i18n 4、prefs 2、update 6）
node scripts/i18n-audit.mjs         沒歸類的 0 條
npm run tauri dev -- -- -- --verify 1
  [verify] 設定視窗（app 端路徑）
  [verify] 中／英介面切換
  [verify] 檢查更新
```

⚠️ **`--verify` 驗不到的一項**：「改字級之後畫面上的字真的變大、欄數跟著變少」。
原因是這個環境跑到那一步時 pane 的方框是 **0×0**（量不到尺寸），
xterm 的 `FitAddon.fit()` 因此是 no-op、`term.cols` 停在啟動時量到的值——
**改字級之前就已經是 0×0**，所以不是設定那條路的問題。
改成驗「`T{json}` 有到 `terminal.js`」＋「`applyTheme()` 真的跑了（pane 背景變了）」，
畫面那一項列進 `docs/REGRESSION-CHECKLIST.md` 的 👤 目視清單（ST9）。

---

## 5. 還沒做的

| 項目 | 為什麼 |
|---|---|
| 檔案總管「用 AwayTerminal 開啟」 | 要寫 HKCU 登錄檔（舊版 `ShellIntegration`），屬於 Windows 專屬整合；欄位先放著並灰掉，TASK-016 再做 |
| Telegram 遠端的設定欄位 | 功能本體是階段 4，欄位一起那時再加（現在連空欄位都不放，免得使用者以為能用） |
| Claude 路徑／參數、adb 路徑 | 舊版 v1.0.18 起就從設定視窗移到「自訂連線」了，新版一開始就是自訂連線 |
| 「檔案太大」等少數後端訊息的英文 | 已經在 `i18n.rs` 裡有兩種語言；剩下的是 `println!` 診斷（刻意不翻） |
| Tauri updater（自動下載安裝、簽章） | 階段 5 |
