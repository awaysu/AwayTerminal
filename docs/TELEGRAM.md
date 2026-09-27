# Telegram 遠端

舊版 `Services/TelegramRemote.cs`（約 1000 行）＋ `Dialogs/RemoteDialog` 的搬移。
用手機的 Telegram 看電腦上的分頁、下指令、收「做完了」的推播。

**實作**：`src-tauri/src/telegram/`（`api` / `cmd` / `tidy` / `screen` / `shot` / `remote` / `probe`）。

---

## 0. 一分鐘版

1. 在 Telegram 找 `@BotFather` → `/newbot` → 拿到 bot token。
2. 跟自己的 bot 說一句話，再開 `https://api.telegram.org/bot<token>/getUpdates` 看 `chat.id`。
3. AwayTerminal → 設定 → **Telegram 遠端** → 勾「開啟」、貼 token、填 chat id → 確定。
4. 手機上 `/goto` 選分頁 → 直接打字＝把那行打進分頁並按 Enter → 做完會自動推回來。

---

## A1. 設定

| 項目 | 值 |
|---|---|
| 設定檔欄位 | `remote_enabled`、`telegram_bot_token`、`telegram_chat_id`、`remote_notify` |
| token 存法 | **明文存在 `settings.json`**——照舊版（`AppSettings.TelegramBotToken` 也是明文）。沿用才能直接匯入舊版設定 |
| 設定視窗 | 舊版是獨立的「遠端」對話框；v2 併成設定視窗裡的一個區塊 |
| token 欄位 | `type=password`、**永遠不回填**。後端只回 `hasToken`（布林），不回 token 本身。留空按確定＝**不變更**（不是清空） |
| 授權範圍 | **只認設定裡那一個 chat id**。其他人的訊息一律不理也不回（連「你沒有權限」都不回，免得洩漏這個 bot 有人在用） |
| 連線方式 | long polling（`getUpdates?timeout=30`），**沒有 webhook**（不用開對外的埠） |
| 啟動 | 程式啟動時 `remote_enabled` 開著就拉起輪詢；設定視窗按確定＝重開輪詢（換 token／chat id 才會生效） |
| 輪詢失敗 | 退避 3 秒後重試，**永遠不會就此停掉**。第 1 次與每 20 次記一行診斷，恢復時再記一行 |
| 啟動第一件事 | `getUpdates?offset=-1` 把 offset 推到最新。⚠️ 一定要 `-1`：預設一次最多回 100 則，關機累積超過 100 則時 offset 會停在第 101 則，第一次輪詢就把舊訊息當現在的指令全部重播 |
| 上線／離線 | 啟動送「🟢 已開啟」，關閉前送「🔴 已關閉」（`notify_offline`，在 `app.exit` 之前）。強殺／當機收不到屬預期 |

### 舊版沒有翻譯

舊版這些訊息是**寫死的繁體中文**（沒走 `Loc.T`）。v2 補齊八語：
Rust 的 `i18n.rs` 有 `tg.*` 共 49 條，前端 `src/lang/*.js` 八份都有。

---

## A2. 指令

### 分頁與畫面

| 指令 | 行為 |
|---|---|
| `/goto [n]` | 進第 n 個分頁；不帶編號＝列出分頁＋inline 按鈕。**進入即定基準**，之後只推新輸出 |
| `/where` | 我在哪個分頁（含 🟢閒／🟠忙） |
| `/exit` | 離開分頁檢視（分頁照跑） |
| 直接打字 | 送出該行 ＋ Enter。**文字與 Enter 分兩次送**（同代理團隊的 `deliver`） |
| 回一個數字 | 畫面是選擇題＝選該項（換算成 ↑／↓＋Enter，每 120ms 一次）；不是選單＝當普通文字送出 |
| `/key <名稱>` | `ctrl-c`／`ctrl-d`／`esc`／`tab`／`enter`／`up`／`down`／`left`／`right` |
| `/stop` | 等於 `/key ctrl-c` |
| `/last [n]` | 最後 n 行輸出（預設 20） |
| `/more` | 上一則輸出再往前翻一頁 |
| `/shot` | 畫面截圖（見 A5） |
| `/close [n]` | **先出確認按鈕**才真的關（誤觸保險）。代理團隊的一格會提醒「整組一起關」 |
| `/follow on\|off` | 完成時自動回傳輸出（預設**開**） |
| `/notify on\|off` | 其他（未進入的）分頁完成也推播（預設**關**） |
| `/plain on\|off` | 提問時附加「請用純文字、不要表格」（預設**關**） |
| `/help`、`/start` | 指令一覽 |

### 從手機開新連線

| 指令 | 行為 |
|---|---|
| `/new [n]` | 不帶編號＝列出可開的連線＋按鈕；帶編號＝開那一條並**自動附著** |
| `/ssh [user@]主機[:埠]` | 埠省略＝22。帶 `user@` 就不必在畫面上回帳號 |
| `/telnet [主機[:埠]]` | 埠省略＝23 |
| `/history [n]` | 列最近可重開的連線；**帶編號才開**（純數字要留給終端機輸入與選單應答，舊版註解明講） |

開完之後的提示照舊版分兩種：SSH 回「`login as:` → 直接回覆帳號，接著照畫面提示回覆密碼」，
其餘回「直接打字送指令，`/last` 看輸出」並在 `/follow` 開著時 1.5 秒後推開場畫面。

#### 可開的清單是怎麼來的

舊版 `ListConnections` 列的是「PowerShell（桌面）＋ `LastHost` 的 SSH／Telnet ＋ 連接埠 ＋ ADB ＋ 自訂連線」。
**v2 沒有 `LastHost`／`History`**——TASK-009 決定用**我的最愛**取代主機歷史
（`docs/MIGRATION.md` 的跳過表有記），所以這一版列的是：

1. PowerShell（桌面）
2. **我的最愛**（SSH／Telnet／連接埠／自訂連線都在裡面，含帳號、金鑰、演算法等完整參數）
3. 設定裡上次用的連接埠
4. ADB
5. 自訂連線（不含隱藏的）

`/ssh`／`/telnet` 不帶參數時用「我的最愛裡第一條同型態的」——整條拿來用，不只是主機名，
所以金鑰與演算法覆寫都會生效。都沒有就回用法說明。

需要選資料夾的自訂連線以**桌面**為工作目錄開啟（手機上不能跳資料夾對話框，同舊版）。

#### 從手機打密碼安全嗎

SSH 的密碼提示走 `prompt_line(..., echo_input: false)`＝**不回顯**，所以畫面上不會有密碼，
`/last` 與完成推播也就不可能把它送出去。傳輸端是 Telegram 的 HTTPS。

⚠️ 一個這一版特別處理過的坑：`/plain` 開著時，舊版會把「請用純文字回答」接在**任何**
≥8 字的訊息後面——**包括密碼**，登入會失敗而且看不出原因（密碼不回顯，畫面上只有
`Access denied`）。這一版多一個條件：只有「會跟 AI 對話的分頁」（`TabKind::Claude`／
`Custom`）才加後綴，單元測試 `plain_suffix_never_touches_a_password` 守著。

#### 開分頁要繞前端（隱含契約）

`session_create` 需要一個輸出用的 `Channel`，那是前端 invoke 才有的東西，Rust 生不出來。
所以遠端 emit 一個 `telegram-open` 事件、前端呼叫 `createSession`，再用
`telegram_opened` 把分頁 id 交回來（同 `ssh-hostkey` → `ssh_hostkey_answer`）。
**前端一定要回**，不回就等到 8 秒逾時、使用者在手機上看到「開啟失敗」。

### 訊息長度

| 項目 | 值 |
|---|---|
| Telegram 硬上限 | 4096 字 |
| 一則畫面的內容上限 | 3500 字（`MAX_BODY`，留空間給標頭與 `<pre>`） |
| 超過怎麼辦 | 畫面內容**取尾端**（`clip_tail`，切點落在代理對中間就往後一格，免得開頭出現 `�`）；一般文字**切段連送**（`split_message`，優先在換行處切）|
| 格式 | 畫面內容用 `parse_mode=HTML` 的 `<pre>`（`&`／`<`／`>` 先轉義）。純文字回覆不帶 parse mode |

`split_message` 是 v2 加的：舊版長文字直接送會被 Telegram 退掉。

---

## A3. 完成推播

「分頁忙 → 閒」的那一刻（`status.rs` 的 600ms 輪詢算出來）：

| 情況 | 條件 |
|---|---|
| 這段忙碌期間**有送出過**（按 Enter／遠端送出 → `last_submit`） | 忙 ≥ **0.8 秒**就推 |
| 沒送出（只是在輸入框打字） | 距最後輸入 ≥2.5 秒（打字回顯抑制）**且**忙 ≥ **3 秒** |
| 送出時間允許比忙碌起點早 2 秒 | 送出→開始輸出有延遲（尤其遠端），不然會漏判 |

為什麼要分兩種：舊版只看「最後按鍵距轉閒 <2.5s＝打字回顯」，但**在電腦上打字送出、AI 很快回答**時轉閒也在 2.5 秒內 → 被誤判成打字、不推（舊版使用者實測「改在 App 發問沒丟給手機」）。

推的內容：

| 項目 | 行為 |
|---|---|
| 附著中的分頁 ＋ `/follow` 開 | 推「基準之後的新輸出」（`diff_new`），對不到基準就退回整個畫面快照 |
| 其他分頁 | 只有 `/notify` 開才推一行「🟢 xxx 閒置（完成）」 |
| 逐分頁關掉推播 | 分頁右鍵「推播到 Telegram」取消勾選＝這個分頁永遠不推（**v2 多的**，只留在記憶體） |
| 沒有新輸出 | **整則不送**（舊版會送只有標頭的空訊息，是手機端噪音的主因） |
| 去重 | 8 秒內同內容（空白正規化後比對）不重送——忙→閒可能連兩次觸發 |
| 不立刻回推 | 送出後**不做「700ms 後回推」**。舊版拿掉了：固定延遲常抓到半成品，又會和完成推播重複（使用者實測一次 `hi` 收到三則） |
| 附著分頁完成＝算活動 | 重置閒置計時，在電腦上持續工作時手機會一直收到，不會被「手機 10 分鐘沒動作」判成閒置而離開 |

閒置離開：附著後 **9 分鐘**警告一次、**10 分鐘**靜默離開分頁檢視（分頁不關）。
閒置取「手機來訊」與「該分頁最近動作」的較晚者。

---

## A4. 雜訊過濾（`tidy.rs`）

claude／codex 的畫面對手機來說大半是雜訊。`tidy_for_phone` 逐條照舊版翻寫（**37 條**規則，每條一個單元測試，另外有一段真的 claude 畫面當 fixture：`resources/telegram/claude-screen.txt`）：

| 類別 | 例 |
|---|---|
| 進行中的動畫 | spinner（`✻ Crunched…`）、`esc to interrupt`、token 計數 |
| 邊框與分隔線 | `╭──╮`、`╰──╯`、`───`、`═══` |
| 輸入框與狀態列 | `❯`／`>` 的空輸入行、`⏵⏵ bypass permissions on`、`◉ xhigh · /effort  5:03 | Fable 5` |
| 開場 | logo banner、welcome box、tips box |
| 表格 | 上框／下框／分隔列丟掉，內容列**攤平**成 `欄1 · 欄2 · 欄3`（手機上表格一定跑掉） |
| 項目符號 | `⏺`／`●`／`⎿` 之類前綴拿掉，保留文字 |

**shell 的輸出不會被動到**（有一條測試專門守這件事：`ls`／`git status` 之類的輸出原樣通過）。

`is_tui_screen` 判斷「整個畫面是 TUI」（vim／top／選單），這種畫面不做表格攤平——它的「表格」就是內容。

---

## A5. 截圖

| 項目 | 做法 |
|---|---|
| 目前（B(a)） | 前端（`bridge.js` 的 `shotPng`）拿畫面文字**自己畫一張新的 `<canvas>`** → `toDataURL('image/png')` → Rust 解 base64 → `sendPhoto` |
| 為什麼不抓 xterm 的 canvas | WebGL 的 drawing buffer 畫完就可以丟（`preserveDrawingBuffer` 預設 false，`toDataURL` 多半全黑）；打開它每一格都多留一份 buffer、**拖慢渲染**——而渲染速度正是這一版的重點 |
| 代價 | **單色**（前景／背景），舊版 WPF 是整塊 render、每個字有顏色。逐字畫在格子上，所以中文與框線字的排版和終端機一致 |
| 取樣 | 2 倍（手機上看才不糊；Telegram 會再壓一次） |
| 之後（B(b)） | `shot::platform::capture` 的介面已經留好，三個平台目前都回 `None` → 一律走前端那條路。Windows `PrintWindow`、macOS `CGWindowListCreateImage`（要螢幕錄製權限）、Linux X11 `XGetImage`（Wayland 下一般程式不能截別人的視窗） |
| ⚠️ | 平台截圖需要視窗在前景，而**團隊規則不准碰前景視窗** → B(b) 只能在使用者自己的機器上驗，不進 `--verify` |

---

## 「畫面上看得到的文字」怎麼拿

**絕不能用原始位元組流去 ANSI。** 位元組流裡是 claude／codex 逐格重繪的控制序列，去掉 ANSI 得到的是一團重複的垃圾（同一行十幾個不同寬度的版本）。這是舊版 `CLAUDE.md` 明文寫的一條雷。

要走的是 `q{id}US text` → `a{id}US text US<內容>`：`terminal.js` 的 `lastPlainText()`（`buffer.getLine().translateToString()`，自動接回 `isWrapped` 的折行）。`screen.rs` 是 Rust 這一端的信箱：送 `q`、等 `a`、逾時放棄。

**逾時之後不做任何「退回位元組流」的備援**——舊版有，而且它正是那條雷的來源（UI 執行緒卡住時退回劣化的位元組流，選單偵測對不到、使用者看到亂碼）。v2 的輪詢在自己的執行緒上、不會卡住 webview，所以逾時代表前端真的有問題，這時候寧可回一句「畫面尚未就緒」。

---

## 安全

| 項目 | 做法 |
|---|---|
| token 不進 log | 這個模組沒有任何一行印出 token。`api.rs` 的 `describe()` **只留錯誤型別與 HTTP 狀態，不含 URL**（URL 裡有 token）——有一條單元測試 `errors_never_leak_the_url` 守著 |
| token 不進介面 | 後端回給前端的是 `hasToken` 布林。設定視窗的 log 行印 `token=已設定`，不印值 |
| token 不進 verify | `telegram_probe` 用寫死的假 token，並有一條檢查確認它不出現在任何一行輸出裡 |
| 只認一個 chat | 見 A1 |
| `/close` 要確認 | 手機上按錯一個鍵就關掉跑了一小時的 claude 是真的會發生 |

---

## 驗證

| 層 | 內容 |
|---|---|
| `cargo test --lib telegram` | **49 個**單元測試：`tidy` 19（每條規則一例＋真畫面 fixture）、`api` 6（含不洩漏 URL）、`cmd` 10、`screen` 3、`shot` 6、`remote` 3、`probe` 2 |
| `npm run verify` 的 Telegram 區段 | `telegram_probe`：起一個**只聽 127.0.0.1 的假 Bot API**，把遠端接上去，驗 prime offset／指令選單／上線通知／非授權 chat 被忽略／各指令回覆／輪詢失敗退避恢復／打字進真分頁／`/last` 的 `<pre>`／PNG 有效／切段／完成推播只一則／`/close` 先確認／token 不洩漏 |
| **不做** | 不打真的 Telegram、不讀使用者設定裡的 token、不啟動真的 claude |

手動要看的（`docs/REGRESSION-CHECKLIST.md` 的 TG 那幾列）：真的 bot 收發、手機上表格攤平好不好讀、截圖看不看得懂。
