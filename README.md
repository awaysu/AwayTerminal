# AwayTerminal 2

分頁式終端機：本機 shell、SSH、Telnet、連接埠（序列埠）、TTL 巨集、AI 代理團隊、Telegram 遠端。

[English](README.en.md)　|　授權 MIT

**這是 [AwayTerminal 1.x](https://github.com/awaysu/AwayTerminal)（C# WPF + WebView2，僅 Windows）
的跨平台重寫版。** 目標是「舊版所有功能都在、行為相容、跑得更快、而且能跨平台」。

| | 舊版 1.2.8 | 這一版 |
|---|---|---|
| 技術 | .NET 9 ＋ WPF ＋ WebView2 | **Rust（Tauri 2）** ＋ xterm.js |
| 平台 | 只有 Windows | Windows（**完成**）、macOS（**真機自動測試全過，手動項目待驗**）、Linux（**程式寫好了，等機器實測**） |
| 終端機繪製 | DOM | **WebGL**（載不起來自動退回 DOM） |
| 需要 .NET 執行環境 | 是 | **不需要** |
| 安裝檔 | 含 .NET ＋ WebView2 安裝程式 | 約 5.8 MB（含 WebView2 bootstrapper） |
| 介面語言 | 中／英 | **八種** |

前端的終端機部分（`src/terminal.js`）是**舊版原檔**，只有四處必要修改
（+61／−5 行，每處都標 `// AT2-n` 並登記在 [docs/TERMINAL-JS-DIFF.md](docs/TERMINAL-JS-DIFF.md)）
——注音組字、二次輸入、Claude Code 多行貼上這些調校過很久的行為完全沿用。

---

## 狀態

| 平台 | 狀態 |
|---|---|
| **Windows 10／11 x64** | 功能完成，自動與手動測試都跑過 |
| **macOS** | **Apple Silicon 真機（macOS 26.6）跑過第一批**：`cargo build` 零錯誤、`cargo test` 全過、5 支 probe 全過、`npm run verify` 25 段全過（2026-10-05）。IME／序列埠／沙盒收程序等手動項目待驗，見 [docs/PLATFORM-UNIX.md](docs/PLATFORM-UNIX.md) 第 7 節 |
| **Linux** | 同上（`x86_64-unknown-linux-gnu`），**還沒在真機跑過** |

還沒做：程式碼簽章、自動更新（設定骨架在、公鑰留空 ＝ 功能停用）、mac／Linux 的實測與發佈。
細節見 [docs/PLATFORM-UNIX.md](docs/PLATFORM-UNIX.md) 與 [CHANGELOG.md](CHANGELOG.md)。

---

## 功能

### 連線

| | 狀態 |
|---|---|
| 本機 shell（Windows PowerShell／`pwsh`；mac `$SHELL`＝zsh；Linux `$SHELL`＝bash） | ✅ |
| **SSH（內建，不呼叫系統 `ssh.exe`）**：終端機裡問 `login as:`、主機金鑰確認（SHA256＋MD5）、PuTTY 的演算法順序與弱演算法警告、`.ppk`、Windows Pageant、keepalive、斷線自動重連 | ✅ |
| **Telnet（內建）**：選項協商、IAC 轉義、**NAWS**（舊版沒有）、`TTYPE` | ✅ |
| **連接埠（序列埠）**：USB 友善名稱、鮑率／位元／同位／流控 | ✅ |
| WSL、ADB（`adb devices` 選裝置） | ✅ |
| 自訂連線（Claude Code、Codex CLI、Gemini CLI、OpenCode、QwenCode、Antigravity CLI…含「自動偵測」） | ✅ |

### 終端機與介面

分頁／分割／分欄三態（可拖曳重排）、右側分頁列（拖寬、顯示隱藏、拖曳排序、狀態燈、
改名、tooltip 顯示執行時間）、複製／純文字貼上／複製全部／存檔、Ctrl+F 搜尋、網址選單、
OSC 52 剪貼簿、Ctrl+滾輪縮放、逐分頁配色、清畫面（確認）、輸入文字視窗（UTF-8／Big5）、
log 記錄（去 ANSI、時間戳、append）、我的最愛、恢復分頁（**含畫面紀錄倒回**）、
保持連線、更新檢查、關於頁。

介面語言八種：繁體中文、English、简体中文、日本語、한국어、Español、Deutsch、Français。

### TTL 巨集

以 TeraTerm 開源的 `ttpmacro/` 原始碼為藍本**逐段翻寫成 Rust**，
**214 個保留字裡實作了 139 個**（舊版自寫的 C# 版少很多）。
含 `if/elseif/else`、`for/while/until/do`、`break/continue`、`goto/call`、`include`、陣列、
`sprintf`、時間日期、37 個檔案／資料夾指令、正規表示式（`fancy-regex`，
[實測 30 個 Oniguruma 語法支援 28 個](docs/TTL-REGEX.md)）、CRC／checksum。

⚠️ **修掉舊版 C# 版三個與 TeraTerm 不一致的地方**：`and`／`or`／`xor`／`not` 是**位元**運算
（不是邏輯運算）、位元運算比比較運算更緊密、整數是 32 位元且會繞回。
舊的 `.ttl` 檔如果依賴錯誤的行為，結果會變。

### 新功能（舊版沒有）

- **沙盒模式**（自訂連線與代理團隊的選項，預設關閉）：每個分頁一棵 `git worktree`、
  `TEMP`／`CARGO_TARGET_DIR` 導到沙盒目錄（**不隔離 `HOME`／`APPDATA`**——那會讓
  Claude Code、Codex 掉登入）、Windows Job Object kill-on-close（Unix 用行程群組）、
  自動產生各工具的護欄設定（Claude Code 的 `PreToolUse` hook 會拒絕 `taskkill /IM`、
  砍 repo 外的路徑、`git push --force`）。
  **這是防呆不是防壞**——agent 和使用者在同一個登入工作階段，繞路仍碰得到桌面
  （[docs/AGENT-SANDBOX.md](docs/AGENT-SANDBOX.md) 講清楚了）。
- **代理團隊**（Multi-Agent）：2～4 個 AI CLI 各帶一個角色，靠 `.ai/bus/` 信箱互相寫信，
  只在對方閒置時才打字進去。
- **AI 聊天室**：21 個角色，輪流發言、主持人寫結論，紀錄在 `.ai/chat/`。
- **Telegram 遠端**：用手機看分頁、下指令、截圖、完成推播、雜訊過濾（37 條規則）。
- 逐分頁「推播到 Telegram」開關、視窗大小位置記憶、終端機右鍵「全選」。

---

## 下載

請到 <https://www.awaysu.cc/software/awayterminal> 下載。
Windows 有安裝檔（`.exe`）與免安裝版（`.zip`）。

⚠️ 目前**還沒有程式碼簽章**，Windows 的 SmartScreen 會出現警告
（[docs/RELEASE.md](docs/RELEASE.md) 說明原因與計畫）。

### 從舊版升級

第一次啟動會問要不要匯入 `%LOCALAPPDATA%\AwayTerminal\settings.json`（**唯讀，不改動舊檔**）。
對照表與 21 條刻意跳過的欄位見 [docs/MIGRATION.md](docs/MIGRATION.md)。
兩個版本可以並存（設定放在不同地方）。

---

## 從原始碼建置

需要 Rust（1.89+）、Node 20+、以及各平台的 webview 開發套件。
完整步驟、常見問題與目錄結構在 **[docs/DEV-SETUP.md](docs/DEV-SETUP.md)**。

```bash
npm ci
npm run tauri dev      # 開發
npm run tauri build    # 產生安裝檔
```

常用的檢查（發佈前全部要過，清單在 [docs/RELEASE.md](docs/RELEASE.md)）：

```bash
cd src-tauri && cargo test --lib && cargo clippy --all-targets -- -D warnings
cd .. && node scripts/test-i18n.mjs        # 八語沒有缺漏
node scripts/i18n-audit.mjs                # 沒有漏掉的中文字串
node scripts/test-bridge-args.mjs          # session_create 的參數都有傳到
npm run verify                             # 端到端（開視窗、跑完自己關掉）
npm run verify:release                     # 同上，但用 release 的 exe
```

---

## 文件

### 規格與架構

| 文件 | 內容 |
|---|---|
| [CLAUDE.md](CLAUDE.md) | **這個專案的規格書**：目標、技術選型、平台差異、風險、階段計畫 |
| [docs/DEV-SETUP.md](docs/DEV-SETUP.md) | 開發環境、建置、目錄結構、踩過的環境雷、**行尾規則** |
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | `terminal.js` ↔ host 的 31 個舊協定字串（31/31 都接上了） |
| [docs/TERMINAL-JS-DIFF.md](docs/TERMINAL-JS-DIFF.md) | `terminal.js` 與舊版的**四處**差異，以及沿用的 IME／貼上踩雷修正 |
| [docs/SETTINGS.md](docs/SETTINGS.md) | `settings.json` 每個欄位、哪些是舊版就有的 |

### 功能

| 文件 | 內容 |
|---|---|
| [docs/SSH.md](docs/SSH.md) | 內建 SSH：PuTTY 行為對照、演算法順序、待真機驗證的設備清單 |
| [docs/TELNET.md](docs/TELNET.md) | Telnet 的選項協商與 NAWS |
| [docs/COM.md](docs/COM.md) | 連接埠；`serialport` 不支援的組合怎麼降級 |
| [docs/TTL.md](docs/TTL.md) | TTL 巨集：已實作的指令、與 TeraTerm 原碼的對照 |
| [docs/TTL-TODO.md](docs/TTL-TODO.md) | 還沒做的保留字，**每一條都寫為什麼** |
| [docs/TTL-REGEX.md](docs/TTL-REGEX.md) | Oniguruma vs `fancy-regex` 的實測對照表 |
| [docs/COMPOSE.md](docs/COMPOSE.md) | 輸入文字視窗（Big5 載入、送出後 Enter） |
| [docs/MULTI-AGENT.md](docs/MULTI-AGENT.md) | 代理團隊：信箱格式、投遞時機、節流 |
| [docs/CHATROOM.md](docs/CHATROOM.md) | AI 聊天室：21 個角色、討論流程、紀錄格式 |
| [docs/AGENT-SANDBOX.md](docs/AGENT-SANDBOX.md) | 沙盒模式三層、**已知漏洞**、桌面隔離的選項 |
| [docs/TELEGRAM.md](docs/TELEGRAM.md) | Telegram 遠端：指令、完成推播規則、雜訊過濾、截圖 |
| [docs/WINDOWS-INTEGRATION.md](docs/WINDOWS-INTEGRATION.md) | WSL／ADB 分頁、檔案總管右鍵選單、單一執行個體 |
| [docs/MIGRATION.md](docs/MIGRATION.md) | 匯入舊版設定的對照表與跳過的欄位 |

### 平台

| 文件 | 內容 |
|---|---|
| [docs/PLATFORM-UNIX.md](docs/PLATFORM-UNIX.md) | **mac／Linux：已寫好什麼、待真機驗證什麼、第一天要跑什麼** |
| [docs/IME-LAB.md](docs/IME-LAB.md) | IME 事件錄影頁與九項劇本（跨引擎比對用） |
| [platform/macos/README.md](platform/macos/README.md) | Finder Quick Action 的建立步驟 |
| [platform/linux/](platform/linux/) | Nautilus 腳本與 Dolphin ServiceMenu |

### 測試與發佈

| 文件 | 內容 |
|---|---|
| [docs/SECURITY.md](docs/SECURITY.md) | **安全說明**（給使用者）：token 明文存放、沙盒是防呆不是防壞、SSH 的 Ed25519 建議、回報漏洞的方式 |
| [docs/REGRESSION-CHECKLIST.md](docs/REGRESSION-CHECKLIST.md) | **逐項回歸清單**（自動與 👤 手動），含「刻意與舊版不同」與「隱含契約」兩張表 |
| [docs/MANUAL-TEST-PLAN.md](docs/MANUAL-TEST-PLAN.md) | 上面那份的 👤 條目排成 P0／P1／P2，每節 20～30 分鐘（**從清單產生**） |
| [docs/RELEASE.md](docs/RELEASE.md) | 發佈流程、簽章、updater、檢查清單 |
| [docs/BENCHMARK.md](docs/BENCHMARK.md) | 新舊版怎麼量（啟動、吞吐、記憶體） |
| [docs/IPC-BENCH.md](docs/IPC-BENCH.md) | Tauri IPC 三種傳法的實測（二進位 channel 快 3.4 倍） |
| [CHANGELOG.md](CHANGELOG.md) | 2.0.0 相對 1.2.8 的變更 |

---

## 授權

本專案採 **MIT**（見 [LICENSE](LICENSE)）。

用到的第三方元件包含 MIT、Apache-2.0、BSD-3-Clause、MPL-2.0 等授權
（`russh` 是 Apache-2.0、`serialport-rs` 是 MPL-2.0、TeraTerm 的 `ttpmacro` 是 BSD-3-Clause），
全部的授權全文與出處在 **[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)**，
安裝檔也會把它裝進程式目錄。

行為參考的來源：[PuTTY](https://www.chiark.greenend.org.uk/~sgtatham/putty/)（SSH 的使用流程）、
[TeraTerm](https://github.com/TeraTermProject/teraterm)（TTL 巨集）、
[microsoft/terminal](https://github.com/microsoft/terminal)（ConPTY 與 OpenConsole）。
