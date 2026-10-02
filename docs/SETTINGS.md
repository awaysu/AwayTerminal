# 設定視窗、中／英介面、關於（TASK-015）

舊版對應：`Dialogs/SettingsDialog`、`Localization/Loc.cs`、`MainWindow.About_Click`、
`Services/UpdateChecker.cs`。程式在 `src-tauri/src/{prefs,update,i18n}.rs` ＋
`src/{setdlg,about,i18n,strings}.js`。

---

## 1. 設定欄位對照表

### 1.1 舊版設定視窗有的（欄位、順序、預設值照抄）

| 舊版欄位 | 舊版控制項 | 新版 | 預設 | 套用時機 |
|---|---|---|---|---|
| 語言 中文／English | `ZhRadio`／`EnRadio` | **八種語言的下拉**（舊版只有兩個 radio） | 第一次啟動看系統語言 | **按確定**（不必重啟，見第 2 節） |
| 字型 | `FontCombo`（可編輯下拉，`Fonts.SystemFontFamilies`） | `<select>` ＋ `<optgroup>` 四組（內建／已下載匯入／系統等寬／系統其他）＋「自訂…」輸入框，另有下載／匯入／移除三顆按鈕 | `JetBrains Mono`（自帶） | 按確定 → `T{json}` 即時套到所有分頁 |
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
| 新增的自訂連線預設開啟沙盒 | `sandboxDefault` | `false` | **新欄位**（2026-10-02 起預設關，原本是 `true`）；打開之後 WSL／ADB 仍然預設關（`custom::default_sandbox`） |
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

## 2. i18n：八種語言

### 2.1 語言清單

順序＝設定視窗下拉的順序（PM 在 TASK-015 修訂版定的）。選項的文字用**該語言自己的寫法**，
使用者看不懂目前語言時也找得到自己的。

| 代碼 | 語言 | 字串來源 |
|---|---|---|
| `zh-TW` | 繁體中文 | **主語言**：key 的來源，文字照舊版 `Localization/Loc.cs` |
| `en` | English | 舊版 `Loc.cs` 有同名 key 的照抄（連標點），其餘自己寫 |
| `zh-CN` | 简体中文 | 從繁中轉，**術語照大陸慣用**（设置／粘贴／串口／标签页／宏／主机密钥） |
| `ja` | 日本語 | 機械翻譯，術語照 TeraTerm／Windows Terminal 的日文介面（設定／貼り付け／シリアルポート／タブ／マクロ／ホスト鍵） |
| `ko` | 한국어 | 同上（설정／붙여넣기／시리얼 포트／탭／매크로／호스트 키） |
| `es` | Español | 同上（Configuración／Pegar／Puerto serie／Pestaña／Macro／Clave de host） |
| `de` | Deutsch | 同上（Einstellungen／Einfügen／Serieller Port／Tab／Makro／Hostschlüssel） |
| `fr` | Français | 同上（Paramètres／Coller／Port série／Onglet／Macro／Clé d’hôte） |

每個語言檔的檔頭、設定視窗語言下拉的下面、關於頁都有一行「機器翻譯，歡迎修正」
（`settings.langNote`，各語言自己的寫法）。繁中與英文不是機器翻譯，所以那一行說的是
「除了繁體中文與 English 之外」。

### 2.2 檔案與退回鏈

- 一種語言一個檔：`src/lang/<代碼>.js`（key 與註解和 `zh-TW.js` 一一對應）。
- `src/strings.js` 把八個合起來，提供 `T['key']`（Proxy，存取的那一刻才挑語言）與
  `fmt('key', 參數…)`。
- **退回鏈：選的語言 → `en` → `zh-TW` → key 本身**。所以少翻一條不會出現 `undefined`，
  只會看到英文（或繁中）。`scripts/test-i18n.mjs` 仍然當它是「沒做完」。

### 2.3 Rust 端：**翻譯只有一份**

Rust 端**不存八種語言**。前端在啟動與切語言時，把「Rust 會用到的那 129 條」
（`i18n_keys()` 回報）**已經翻好的字**推給後端（`i18n_push`）；Rust 存成一張 runtime 的表。
推之前（啟動最早期）用 `i18n.rs` 內建的繁中／英文當後備。

| | 前端 | Rust 端 |
|---|---|---|
| 檔案 | `src/lang/*.js`（八種） | `src-tauri/src/i18n.rs`（**只有繁中／英文的後備**＋ runtime 表） |
| 取用 | `T['key']`、`fmt('key', …)` | `t("key")`、`tf("key", &[…])` |
| 切換 | `applyLang('ja')` → 每個模組的 `applyTexts()` 重跑 ＋ 推給後端 | `i18n_push` 收到就換（`AtomicU8` ＋ runtime 表，背景執行緒也讀得到） |
| 參數 | `{0}`／`{1}` | 同 |
| 查不到 | 退回鏈（見上） | 推過來的 → 內建後備 → key 本身 |

**為什麼不是 PM 原本說的「Rust 回代碼、前端查表」**：

1. **一部分做不到**：**直接寫進終端機畫面的訊息**（重連倒數、SSH 的「連線到 …」、
   巨集結束提示、COM 降級警告、恢復分頁分隔行）是背景執行緒把**位元組**寫進 pane，
   和 PTY 輸出走同一條路——前端拿到的是終端機內容，沒有機會查表。
2. **回代碼的風險**：漏改一處，使用者就看到 `err.connNotFound` 這種字（等於壞掉）；
   現在的做法漏一條只是退回內建的繁中／英文。
3. **有些訊息帶作業系統原文**（`開啟 COM5 失敗：系統找不到指定的檔案。`），
   前端查表也翻不了後半段。

兩種做法的共同目標「**翻譯只有一份**」都達到了，而且是可以驗的（下一節）。

⚠️ `i18n_push` 之後 Rust 會**重送 `T{json}`**：那包 JSON 裡有搜尋列與代理狀態的字。
不重送的話搜尋列會留在上一個語言（`--verify` 抓到過）。

### 2.4 Rust 端字串清單（哪些翻、哪些不翻）

| 種類 | 翻不翻 | 條數 |
|---|---|---|
| `Err(...)` 回給前端的、寫進終端機畫面的、檔案對話框標題、`T{json}` 的字 | ✅ | **129** |
| TTL 的 22 條錯誤訊息（本來就有 `message()` 英文 ＋ `message_zh()`） | ✅ 照語言挑一個 | 22 |
| `println!("[AwayTerminal] …")` | ❌ 開發診斷（打包後沒有 stdout；`--verify` 與踩雷紀錄都在比對這些字） | 57 |
| 例外（`.git/info/exclude` 的註解、字型 fallback 清單、`--verify` 專用） | ❌ | 12 |
| **沒歸類的** | — | **0** |

### 2.5 怎麼新增一種語言

1. 複製 `src/lang/en.js` 成 `src/lang/<代碼>.js`，翻好（key 與順序不要動）。
2. `src/strings.js`：`import` 它，加進 `LANGS`（顯示名稱用該語言自己的寫法）與 `TABLES`。
3. `src-tauri/src/i18n.rs` 的 `LANGS` 也加一個代碼（設定視窗才存得進去）。
4. `node scripts/test-i18n.mjs` —— 缺的 key、空字串、參數編號不一致都會列出來。
5. 想加系統語言的對映規則（例如 `pt-BR` → `pt`）改 `strings.js` 的 `matchLang()`。

### 2.6 測試

```
node scripts/test-i18n.mjs
```

檢查六件事：八種語言的 key 是否齊（缺的逐條印）、有沒有空字串、有沒有多餘的 key、
`{0}`／`{1}` 的參數編號是否一致、**Rust 端需要的 129 個 key 前端都有**、
工具列文字是否過長（只提醒）。**每個任務都要跑**——新字串沒補齊八語就算沒做完。

2026-09-27：八種語言各 **389** 個 key，缺漏 0、空字串 0、多餘 0、參數不符 0。

### 2.7 預設語言與不跟著語言變的東西

- **第一次啟動**（`settings.language` 是空的）：用系統語言（`sys-locale`）對到這八種，
  對不到用 `en`；然後把選到的存回設定。使用者在設定視窗改過就固定，不再看系統。
  **舊版沒有這個行為**（舊版預設一律繁中）→ 新增。
- 舊設定檔寫的是 `zh`（只有中英兩種的時期）→ 讀進來當 `zh-TW`（`setLang` 有處理，有測試）。
- **日期／時間格式不跟著語言變**：log 的時間戳是舊版的相容格式
  （`[yy-MM-dd HH:mm:ss]`，改了會讓舊的 log 解析不了），分頁 tooltip 的「執行 00d00h00m」
  也是固定格式（`elapsedText`；2026-10-02 由舊版的 `日:時:分` 改成帶單位）。這次只翻文字。

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
| 查哪裡 | `GET https://www.awaysu.cc/software/api.php?action=check_update&app=awayterminal&platform=windows&version=<版本>` | 同一支 API、同一個 `app=awayterminal`（2.0 接手舊版的產品頁），`platform` 依平台（windows／macos／linux） |
| User-Agent | `AwayTerminal/<版本>` | 同 |
| 逾時 | 10 秒 | 同 |
| 版本比較 | 伺服器的 `update_available`；沒帶才自己比（逐段數字、補 0、`-beta` 視為較小） | 同（`update::compare`，9 條測試） |
| 沒有新版 | 按鈕旁一行「已是最新版本 (vX)」 | 同 |
| **失敗（離線／`ok:false`／JSON 壞掉）** | 按鈕旁一行「檢查失敗（請確認網路後再試）」，**不跳錯誤視窗** | 同 |
| 有新版 | 跳視窗：目前／最新版本＋更新內容（可捲動）＋「前往下載頁」 | 同 |
| 下載 | 開軟體頁，讓使用者自己選安裝版／免安裝版 | 同 |
| 自動下載安裝 | 沒有 | **這次也沒有**（Tauri updater 是階段 5） |

2026-10-01 使用者定案：2.0 **沿用舊版的 `awayterminal` 代號與下載頁**，不另開產品頁。

### 3.3 只有這一個功能會連外

`ureq`（MIT OR Apache-2.0，rustls + webpki-roots）只有 `update_check` 用。
`--verify` **不會**打真的網站：`update_verify` 在 127.0.0.1 開一個只回一次的假伺服器，
再對一個沒人聽的 port 打一次證明失敗是安靜的。

---

## 4. 驗證

```
cargo test                          231 passed（i18n 6、prefs 2、update 6）
node scripts/test-i18n.mjs          八種語言各 389 個 key，缺漏 0
node scripts/i18n-audit.mjs         沒歸類的 0 條
npm run tauri dev -- -- -- --verify 1
  [verify] 設定視窗（app 端路徑）
  [verify] 中／英介面切換
  [verify] 八種語言（八個工具列文字互不相同、後端訊息跟著換）
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
