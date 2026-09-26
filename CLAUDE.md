# CLAUDE.md — AwayTerminal2

AwayTerminal（https://github.com/awaysu/AwayTerminal ，C# WPF + WebView2 + xterm.js，僅 Windows）的**跨平台重寫版**。
目前狀態（2026-09-26）：**規劃完成，尚未動工**。下一步＝階段 1 技術驗證。

## 目標
1. **所有原本的功能都要有**，而且**行為相容**——舊版使用上沒有問題（尤其二次輸入文字、注音組字、Claude Code 多行貼上等已調好的行為），新版不可退步。
2. **跨平台**：Windows + macOS + Linux。
3. **執行速度更快**（使用者沒有指定特定瓶頸，是整體要更快）。

## 各功能的 base（參考來源）
| 功能 | 參考 | 做法 |
|---|---|---|
| PowerShell / shell | microsoft/terminal（MIT） | Windows 用 ConPTY + 打包的 OpenConsole（conpty.dll/OpenConsole.exe，同舊版）；mac/Linux 用 forkpty，預設開使用者的 `$SHELL`，有裝 `pwsh` 才開 PowerShell |
| SSH / Telnet | PuTTY（MIT） | **SSH 內建**（不再呼叫系統 ssh.exe）：協定層用 Rust `russh`，**行為照 PuTTY**：`login as:`、主機金鑰確認視窗、keepalive、斷線重連、`.ppk` 金鑰、Windows 支援 Pageant。Telnet 自己實作（加 NAWS） |
| 連接埠 / 巨集 | TeraTermProject（BSD-3） | 連接埠用 `serialport-rs`；TTL 巨集解譯器**以 TeraTerm 開源的 `ttpmacro/` 原始碼（github.com/TeraTermProject/teraterm）為藍本逐段翻寫成 Rust**：解析、運算式、變數、流程控制、各指令行為照原碼；DDE（ttpmacro↔ttermpro）改為程式內直接呼叫連線後端，Win32 對話框／檔案 API 改 Tauri／Rust 標準庫。相容度要比舊版 C# 自寫版更完整 |

- 這三個都是完整應用程式、不是函式庫，microsoft/terminal 和 TeraTerm 綁 Win32 無法跨平台 → 定位是**行為參考與零件來源**，不是直接拿整個程式當 base。
- 整個程式只能有**一套**終端機模擬／渲染（xterm.js），各連線只是不同後端。
- ⚠️ **`github.com/PuTTY-Terminal-Suite` 是假的**（2026-09-15 建立、只有 SEO README、下載連到不相干的 github.io），絕不使用。PuTTY 官方：https://www.chiark.greenend.org.uk/~sgtatham/putty/ ，原始碼 `git.tartarus.org/simon/putty.git`。

## 其他參考（非 base，看做法／產品點子）
| 專案 | 技術 / 授權 | 參考什麼 | 限制 |
|---|---|---|---|
| **Tabby**（github.com/Eugeny/tabby） | Electron + xterm.js，MIT，跨平台 | **架構最接近**：xterm.js 前端 + SSH/Telnet/序列埠/本機 shell 同一程式。前端配置、xterm.js 在 mac/Linux 的坑、WebGL addon、SSH 分頁 UI | 它是 Electron，我們用 Tauri 應更快 |
| **WezTerm**（github.com/wezterm/wezterm） | Rust，MIT，跨平台 | **Rust 後端範本**：內建 SSH、序列埠、多工器；`portable-pty` 即出自 WezTerm | MIT，可借程式碼，寫進 THIRD-PARTY-NOTICES |
| **Ghostty**（github.com/ghostty-org/ghostty） | Zig，原生渲染，MIT | mac／Linux GTK 的**輸入法組字處理**（做 Linux fcitx5/ibus 時回來看）、shell integration（OSC 133）、字型 fallback、Kitty 鍵盤協定 | 渲染自寫，只參考做法 |
| **iTerm2** | Obj-C/Swift，僅 mac，**GPL-3** | tmux 整合、Triggers（類似 TTL `wait`）、Instant Replay（對照「恢復分頁含畫面紀錄倒回」）、hotkey 視窗、inline 圖片 | ⚠️ **GPL，絕不抄程式碼**，只看功能行為 |
| **Warp** | Rust，主程式**非開源** | Blocks（指令＋輸出成塊）、內建 AI、編輯器式輸入框、Workflows；可對照代理團隊／AI 聊天室的體驗 | 只參考產品設計 |

## 技術選型（已定案方向）
- **Tauri 2（Rust 後端）+ xterm.js 前端**。
- 前端**沿用舊版 `web/terminal.js`**（輸入佇列、IME、貼上等調校幾乎原封不動）；舊版 C# 端的輸入處理（claude 輸入佇列、靜止閘門等）照原邏輯改寫成 Rust。
- 不選全原生渲染：注音、全形字、組字畫面要全部重踩，違反「行為相容」。
- Rust crate 候選：`portable-pty`（或自寫 ConPTY 以載入 OpenConsole）、`russh`、`serialport`、`tokio`。

## 速度改善點（舊版實際看到的瓶頸）
1. 舊版 xterm 用 **DOM 渲染**（沒載 WebGL addon）→ 新版改 **`@xterm/addon-webgl`**。
2. 舊版輸出走 `o{id}{US}{base64}` 字串 → `PostWebMessageAsString` → JS `atob` → 新版用 **Tauri 二進位 channel** 直接傳 bytes。
3. 舊版 .NET + WPF + WebView2 三層啟動 → Rust 原生 + 系統 webview，啟動快、記憶體少、安裝檔約 10MB。
- **動工前先量舊版基準**：啟動時間、`cat` 大檔時間、記憶體用量；新版做完再對比。

## 平台差異重點
| | macOS | Linux |
|---|---|---|
| Webview | WKWebView，WebGL 穩定 | WebKitGTK，版本不一，效能／WebGL 較不穩 |
| 中文輸入法 | 系統輸入法，一致 | fcitx5 / ibus 行為各異，**二次輸入類問題最可能在此重現** |
| 發佈 | Developer ID 簽章 + notarization（**使用者已付 Apple 年費**），.dmg，universal binary（arm64+x64） | 不需簽章；先做 AppImage + .deb |
| 連接埠 | `/dev/tty.usbserial-*`、`/dev/cu.*` | `/dev/ttyUSB*`、`/dev/ttyACM*`，需 `dialout` 群組權限 |
| 快捷鍵 | Cmd 取代 Ctrl（終端機內 Ctrl+C 照送） | 同 Windows |
| 「用 AwayTerminal 開啟」 | Finder Quick Action | Nautilus / Dolphin 腳本 |

其他只在 Windows 的東西要換掉：單一執行個體（Named Pipe → Unix domain socket）、狀態燈子行程樹（Linux `/proc`、mac `libproc`）、WSL 項目（mac/Linux 隱藏）、ADB 路徑搜尋、中文字型 fallback。

**平台順序：Windows（先和舊版逐項比對行為）→ macOS → Linux（先支援 Ubuntu X11/Wayland + fcitx5/ibus）。**

## 需要搬移的功能清單（來自舊版）
- 連線：PowerShell、SSH（`login as:`）、Telnet、COM、WSL、ADB（`adb devices` 選裝置）、自訂連線（Claude Code、Codex CLI、Gemini CLI、OpenCode…，含「自動偵測」）
- 分頁：右側分頁列（拖寬、顯示／隱藏、拖曳排序）、狀態燈（連線圖示染綠／紅）、改名、tooltip 顯示執行時間、關分頁送優雅結束鍵
- 檢視：分頁 / 分割 / 分欄三態，可拖曳重排、點標題 zoom
- 終端機：複製／純文字貼上／複製全部／存檔、Ctrl+F 搜尋、網址選單、OSC 52 剪貼簿、Ctrl+滾輪縮放、逐分頁配色、清畫面（確認）
- 輸入文字視窗（載入 UTF-8/Big5、清除、復原、儲存）
- log 記錄（去 ANSI、時間戳、append）
- TTL 巨集（含 if/elseif/else、for/while、break/continue 等舊版已支援的指令）
- 我的最愛
- 恢復分頁（含畫面紀錄倒回）、斷線自動重連、保持連線
- 代理團隊（Multi-Agent，`.ai/bus/` 信箱、角色檔）、AI 聊天室（`.ai/chat/`、21 個角色）
- Telegram 遠端（檢視、下指令、截圖、完成推播、雜訊過濾）
- 中文 / 英文介面、更新檢查、關於頁、第三方授權聲明
- 匯入舊版 `%LOCALAPPDATA%\AwayTerminal\settings.json`

## 階段計畫
1. **技術驗證**：先量舊版基準；Tauri + xterm.js WebGL + ConPTY/OpenConsole，搬入 `terminal.js`；Windows 跑 PowerShell 與 Claude Code，驗證注音、多行貼上、二次輸入與舊版一致，並做速度比較（確認二進位 channel 真的沒走 JSON）。**同一階段就要做**（對應下方風險 1–3）：
   - mac 跑同一版，測注音組字（WKWebView 事件順序）
   - Ubuntu（fcitx5 + ibus）smoke test：注音能不能打、WebGL 能不能開
   - 用 `russh` 實測使用者常連的舊設備（先向使用者要設備清單）
2. **連線後端**：shell → SSH（PuTTY 行為）/ Telnet → 連接埠 + TTL
3. **介面功能**：分頁／分割／分欄、log、搜尋、字型配色、我的最愛、自訂連線、恢復分頁
4. **進階功能**：代理團隊、AI 聊天室、Telegram 遠端
5. **發佈**：Windows 安裝檔／MSIX、mac dmg + 公證、Linux AppImage/.deb；settings.json 匯入

## 慣例 / 注意
- 舊版原始碼與其 CLAUDE.md（185KB，含大量踩雷紀錄）是**行為規格與回歸測試清單**：每搬完一個功能就對照一次。舊版 repo：https://github.com/awaysu/AwayTerminal
- 舊版的 Win10 conhost / WebView2 特有的雷在 mac/Linux 不會出現，但 Unix PTY 下 Claude Code 的輸入時序、alt-screen 行為要重新驗證。
- 授權：本專案 MIT；PuTTY（MIT）、TeraTerm（BSD-3）、microsoft/terminal（MIT）、xterm.js（MIT）都相容，引用的部分要寫進 THIRD-PARTY-NOTICES。
- 對使用者一律用繁體中文。

## 風險 / 待驗證
**高風險（可能推翻做法，階段 1 就驗證）**
1. **`terminal.js` 在 WebKit 可能不能原封不動**：舊版調校基於 WebView2（Chromium）；WKWebView / WebKitGTK 的 `keydown`／`compositionend`／`input` 事件順序不同，注音二次輸入、組字殘留可能重現。最可能元凶（待實測）：Enter 確認組字時 Chromium 先 `keydown`(229, isComposing) 再 `compositionend`；WebKit 先 `compositionend` 再送 isComposing=false 的 Enter → 多送一個 Enter。
   - **解法**：(a) 先做純 HTML「IME 事件錄影頁」，在 WebView2 / WKWebView / WebKitGTK 跑同一組操作（注音+Enter、組字中 Backspace、Esc、候選字選字、中英切換、Claude Code 多行貼上）列出差異表；(b) 引擎差異隔離成 adapter 層（`ime-chromium.js` / `ime-webkit.js`），`terminal.js` 主體不動，**執行時偵測引擎**而非依 OS；(c) WebKit 多出的 Enter：`compositionend` 後同一事件迴圈內的 Enter keydown 吞掉。無備案（mac 只有 WKWebView），但問題在 JS 層可修。
2. **Linux WebKitGTK 輸入法偏弱**：fcitx5/ibus 在 WebKitGTK textarea 有已知問題、WebGL 可能被停用或很慢。若走不通要重想 Linux 架構，不能拖到最後才發現。
   - **解法**：(a) 一天 smoke test：Ubuntu 22.04 + 24.04 VM × X11/Wayland × fcitx5/ibus，測試頁只放 xterm.js + WebGL addon；(b) 已知環境變數做進啟動程式：`WEBKIT_DISABLE_COMPOSITING_MODE=1`、`WEBKIT_DISABLE_DMABUF_RENDERER=1`（NVIDIA 白畫面），`GTK_IM_MODULE=fcitx|ibus` 沒設就提示；(c) 渲染器三段退回 WebGL → `@xterm/addon-canvas` → DOM，啟動時實測決定，並開放手動選；(d) **備案**：Linux 單獨出 Electron 版（Chromium，與 WebView2 行為一致），前端不改，只包一次 IPC 層。Tauri Verso 仍實驗性，不考慮。
3. **`russh` 連舊設備**（**已降為中風險**，2026-09-26 查證 russh 0.63.3）：舊演算法已齊——kex `diffie-hellman-group1-sha1`/`group14-sha1`/`group-exchange-sha1`、cipher `aes*-cbc`/`3des-cbc`（需開 `des` feature）、MAC `hmac-sha1`、hostkey `ssh-rsa`(SHA-1)；**`.ppk` 格式與 Windows Pageant 內建**。russh 現由 Warp 團隊維護。
   - **仍要做**：(a) 實測使用者設備（問題不只演算法：banner、keyboard-interactive、不理 ext-info、strict-kex 不相容）；(b) 預設演算法順序照 PuTTY、每條連線可覆蓋、加 PuTTY 式「弱演算法警告」；(c) **備案**：第二後端 `ssh2` crate（libssh2），最後手段才呼叫系統 `ssh`。

**中風險（調整預期或多做功）**
4. Windows 上 Tauri 仍用 WebView2，啟動改善只有少掉 .NET + WPF 層；主要收益在 WebGL 與二進位傳輸。mac/Linux 改善較明顯。
5. Tauri 二進位 channel：直接回 `Vec<u8>` 可能被序列化成 JSON 數字陣列（比 base64 更慢），要用 raw response 並實測。
6. WebGL addon：全形字、中文字型 fallback、context lost（休眠喚醒）可能出問題，要能自動退回 DOM/canvas 渲染。
7. 舊版為 Win10 conhost 加的 Claude Code 貼上／輸入時序處理，在 Unix PTY 可能多餘甚至有害 → 做成**依平台開關**。
8. PuTTY 相容零件：`.ppk` 解析與 Pageant 用戶端 **russh 已內建**（見風險 3），只剩主機金鑰存放位置（PuTTY 用登錄檔）要改跨平台檔案。
9. Telegram 截圖：舊版用 WPF，新版改 xterm.js canvas 匯出或各平台截圖 API。

**小事**
10. Tauri updater 簽章金鑰保管；舊版使用者升級到新版的路徑。
11. Windows 程式碼簽章（SmartScreen）；舊版若有簽要延續。
12. 把舊版 CLAUDE.md 的踩雷紀錄整理成**可逐項勾選的手動回歸測試清單**，各平台逐項比對。

## 待決定
- repo 形式：新開 `AwayTerminal2` repo，還是之後取代原本 `awaysu/AwayTerminal`（例如 v2 分支）。
