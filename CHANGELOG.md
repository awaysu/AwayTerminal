# 變更紀錄

版本格式 `主.次.修`。這一版是 **AwayTerminal 1.2.8 的跨平台重寫版**，
所以下面整理的是「相對 1.2.8」的差異，不是逐次提交的紀錄。

---

## 2.0.0（未發佈）

**整個程式重寫**：C# WPF + WebView2（僅 Windows）→ **Rust（Tauri 2）+ xterm.js**。
目標是「舊版所有功能都在、行為相容、跑得更快、而且能跨平台」。
目前 **Windows 完成**，macOS／Linux 等機器（`CLAUDE.md` 的平台順序）。

### 相對 1.2.8 的重點

- **安裝檔約 10 MB，不需要 .NET 執行環境。** 舊版是 framework-dependent 的 .NET 9，
  安裝檔要一起帶 .NET Desktop Runtime 的安裝程式。
- **終端機改用 WebGL 繪製**（`@xterm/addon-webgl`）。舊版沒載這個 addon ＝ DOM 繪製。
  載入失敗或 context lost 會自動退回 DOM。
- **輸出走 Tauri 的二進位 channel**，不再是 `o{id}US{base64}` 字串 ＋ JS `atob`。
  實測 50.8 MB/s（舊版那條路 15.1 MB/s；直接回 `Vec<u8>` 只有 7.4，因為會被序列化成
  JSON 數字陣列）。
- **`terminal.js` 幾乎原封不動搬過來**（+61／−5 行、四處修改，每處都標 `// AT2-n` 並
  登記在 `docs/TERMINAL-JS-DIFF.md`）。注音組字、二次輸入、Claude Code 多行貼上這些
  調校過的行為完全沿用。

### 連線

- **SSH 改成內建**（Rust `russh`），不再呼叫系統的 `ssh.exe`。行為照 PuTTY：
  終端機裡問 `login as:`、主機金鑰確認（SHA256 ＋ MD5，金鑰變更一律拒絕）、
  PuTTY 的演算法順序 ＋ 弱演算法警告、`.ppk` 金鑰、Windows Pageant、keepalive、
  斷線自動重連（退避 3/6/9…30 秒，按 Enter 立刻重連）。
  - 主機金鑰存在設定資料夾的 `known_hosts`（OpenSSH 格式），**不碰 `~/.ssh`**；
    PuTTY 是存在登錄檔，那個做法不跨平台。
  - `login as:` 的時機**與舊版不同**：舊版連線前就問（帳號要放進 `ssh.exe` 的命令列），
    這一版連上交握之後才問（PuTTY 的順序）。
- **Telnet 自己實作**（照舊版 `TelnetSession.cs` 逐段翻寫），並**補上舊版沒做的 NAWS**
  （改變視窗大小會通知對方）與 `TTYPE`（回 `xterm`）。
- **連接埠（COM）** 改用 `serialport-rs`。舊版有、但 `serialport` 不支援的組合
  （Mark／Space 同位、1.5 個停止位元、RTS/CTS＋XON/XOFF 並用）會降級並在畫面上警告。
- WSL／ADB 分頁、自訂連線預設集（Claude Code、Codex、Gemini CLI、OpenCode…）與
  「自動偵測」都在。ADB 的裝置清單**多列出離線／未授權的裝置**（灰字），
  舊版是直接濾掉、只說「沒有偵測到裝置」。

### 介面

- 分頁／分割／分欄三態、右側分頁列（拖寬、顯示隱藏、拖曳排序）、狀態燈、改名、
  tooltip 執行時間、關分頁送優雅結束鍵——都照舊版。
- **視窗大小位置與檢視模式會記住**（舊版固定最大化、檢視模式只在記憶體裡）。
- 終端機：複製／純文字貼上／複製全部／存檔、Ctrl+F 搜尋、網址選單、OSC 52、
  Ctrl+滾輪縮放、逐分頁配色、清畫面（確認）。**終端機右鍵多了「全選」**
  （舊版有這個字串但沒有任何呼叫端）。
- 我的最愛、輸入文字視窗、log 記錄（去 ANSI、時間戳、append）、恢復分頁（含畫面倒回）、
  保持連線、更新檢查、關於頁、第三方授權聲明。
- **介面語言八種**（繁體中文、English、简体中文、日本語、한국어、Español、Deutsch、
  Français），舊版只有中／英。首次啟動依系統語言；日期與 log 時間戳的格式**不隨語言變**。
- 對話框全部改成頁內對話框（舊版是 WPF `MessageBox`／`InputDialog`）；
  存檔與選資料夾仍然用系統原生對話框。

### TTL 巨集

- **整個解譯器重寫，以 TeraTerm 的 `ttpmacro/` 原始碼為藍本逐段翻寫**，
  相容度比舊版自寫的 C# 版高：**214 個保留字裡實作了 127 個**（舊版少很多），
  含 `if/elseif/else`、`for/while/until/do`、`break/continue`、`goto/call`、`include`、
  陣列、`sprintf`、時間日期、37 個檔案與資料夾指令。
- **正規表示式**（`strmatch`／`strreplace`／`waitregex`／`regexoption`）改用 `fancy-regex`，
  實測 30 個 Oniguruma 語法裡支援 28 個（含 look-behind 與反向參照——舊裝置的 banner
  真的會用到，所以沒選標準的 `regex` crate）。對照表在 `docs/TTL-REGEX.md`。
- **修掉舊版 C# 版三個與 TeraTerm 不一致的地方**：`and`／`or`／`xor`／`not` 是位元運算
  （不是邏輯運算）、位元運算比比較運算更緊密、整數是 32 位元且會繞回。
  舊的 `.ttl` 檔如果依賴錯誤的行為，結果會變——要請使用者確認手上的檔案。

### 新功能（舊版沒有）

- **沙盒模式**（自訂連線與代理團隊，預設開啟）：每個分頁一棵 `git worktree`、
  `TEMP`／`CARGO_TARGET_DIR` 導到沙盒目錄（**不隔離 `HOME`／`APPDATA`**，那會讓
  Claude Code、Codex 掉登入）、每個分頁一個 Windows Job Object（kill-on-close）、
  自動產生各工具的護欄設定（Claude Code 的 `PreToolUse` hook 會拒絕 `taskkill /IM`、
  `Stop-Process -Name`、砍 repo 外的路徑、`git push --force`）。
  - **這是防呆不是防壞**：agent 和使用者在同一個 Windows 登入工作階段，繞路仍碰得到
    桌面。真正的隔離要靠 VM，選項寫在 `docs/AGENT-SANDBOX.md`。
  - 已知漏洞：用 Microsoft Store 的 app execution alias 啟動的行程（很多機器上的
    `pwsh`）由 AppX 服務建立，不在我們的 Job Object 裡 → 關分頁收不到它。
- **AI 聊天室**（`.ai/chat/`、21 個角色）與**代理團隊**（`.ai/bus/` 信箱、角色檔）都在，
  聊天室的討論紀錄只由 AwayTerminal 寫，每個參加者只寫自己那一則。
- **逐分頁「推播到 Telegram」** 開關（舊版只有全域的 `/notify`）。
- 安裝檔多了授權頁（MIT 全文）與八種語言的語言選擇。

### Telegram 遠端

- 指令、完成推播、雜訊過濾、截圖都照舊版搬（19 條指令全在），**加上超長訊息自動切段**
  （舊版超過 4096 字會被 Telegram 退掉）。
- 訊息**八語**（舊版這些字串是寫死的繁體中文，從來沒被翻譯）。
- **截圖是單色的**：前端把畫面文字畫進一張新的 canvas，不抓 xterm 自己的 canvas
  ——那需要打開 WebGL 的 `preserveDrawingBuffer`，會拖慢渲染。舊版是 WPF 整塊 render，
  每個字有顏色。理由與平台截圖的介面在 `docs/TELEGRAM.md`。
- bot token 仍然是**明文**存在設定檔（照舊版，才能直接匯入舊設定）。降低措施：
  不進 log、錯誤訊息不含 URL（URL 裡有 token）、設定視窗永不回填。
  改成系統金鑰存放區是 mac／Linux 一起處理的事。

### 從舊版升級

- 會讀 `%LOCALAPPDATA%\AwayTerminal\settings.json`（**唯讀，不改動舊檔**）並在第一次
  啟動時問要不要匯入。對照表與 21 條刻意跳過的欄位在 `docs/MIGRATION.md`。
- **不匯入工作階段狀態**（`SavedTabs`／`History`）——那是「上次關程式時的樣子」。
  舊版的「紀錄」與主機歷史由**我的最愛**取代。

### 還沒做

- macOS 與 Linux（等機器）。
- 程式碼簽章、Tauri 自動更新（設定骨架在、公鑰留空 ＝ 功能停用；見 `docs/RELEASE.md`）。
- Telegram 的平台截圖（有顏色的那種）。
- TTL 剩下的 87 個保留字（清單與理由在 `docs/TTL-TODO.md`；`sendbroadcast`／密碼相關
  指令是刻意不做）。
- WSL 的發行版清單（舊版也沒有，是新功能，等使用者決定）。
