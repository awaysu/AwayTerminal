# AwayTerminal2 開發環境設定

本文件記錄階段 1 技術驗證骨架的開發環境、建置方式與目錄結構。

## 工具鏈版本（Windows 開發機，2026-09-26 實測）

| 工具 | 版本 | 安裝方式 |
|---|---|---|
| Node.js | v24.15.0 | 已預裝 |
| npm | 11.12.1 | 隨 Node |
| git | 2.55.0.windows.3 | 已預裝 |
| rustup | 1.29.1 | `winget install Rustlang.Rustup` |
| rustc | 1.98.1 (48a229cea 2026-09-01) | `rustup default stable-x86_64-pc-windows-msvc` |
| cargo | 1.98.1 (797e8a9bc 2026-08-05) | 同上 |
| MSVC C++ 建置工具 + Windows SDK | Visual Studio 18 Community 的 `Microsoft.VisualStudio.Workload.NativeDesktop` | 見下 |
| Tauri CLI | `@tauri-apps/cli` ^2（專案相依，不全域安裝） | `npm install` |

### MSVC / Windows SDK 安裝指令

Rust 的 `x86_64-pc-windows-msvc` target 需要 MSVC 連結器與 Windows SDK。
本機已有 Visual Studio 18 Community，但沒有 C++ 工作負載，用安裝器補裝：

```powershell
# 注意：--passive / --quiet 必須「一開始就以管理員身分」執行，否則安裝器直接回 exit code 5007
Start-Process -Verb RunAs -Wait `
  "C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe" `
  -ArgumentList 'modify','--installPath','"C:\Program Files\Microsoft Visual Studio\18\Community"',
                '--add','Microsoft.VisualStudio.Workload.NativeDesktop','--includeRecommended',
                '--passive','--norestart'
```

沒有 Visual Studio 的機器可改用：

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools `
  --override "--passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

驗證：

```powershell
& "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" `
  -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
cargo --version
rustc --version
```

## 建置 / 開發

```powershell
npm install            # 安裝前端與 Tauri CLI
npm run dev            # 只跑 Vite 開發伺服器（http://localhost:1420，瀏覽器內看不到 IPC）
npm run tauri dev      # 開發模式：啟動 Vite + Tauri 視窗（有 IPC）
npm run build          # 只建置前端到 dist/
npm run tauri build    # 產生 release 執行檔與安裝檔
```

`npm run tauri build` 產出位置：

- 執行檔：`src-tauri/target/release/AwayTerminal.exe`
- 安裝檔：`src-tauri/target/release/bundle/nsis/`、`src-tauri/target/release/bundle/msi/`

## `.ps1` 含中文一定要存成 UTF-8 **with BOM**

（舊版踩雷紀錄第 8、47 條，舊版踩過**兩次**。TASK-024 發現我們也踩了。）

**Windows PowerShell 5.1**（不是 PowerShell 7）讀 `.ps1` 時，**沒有 BOM 就用系統 ANSI
代碼頁解碼**——這台機器是 Big5，所以 UTF-8 的中文會變成亂碼：

- 中文只在**註解**裡 → 註解變亂碼（難看但還能跑）
- 中文在**字串或參數**裡 → **行為錯誤**（檔名／訊息全錯），而且錯在資料不在語法，
  所以不會有語法錯誤幫你抓

```powershell
# 檢查 repo 裡所有含非 ASCII 的 .ps1 有沒有 BOM
node scripts/audit-pitfalls.mjs        # 它的第二節就在做這件事
```

修法：用 UTF-8 **with BOM** 存檔（`utf-8-sig`）。PowerShell 7 兩種都讀得對，
但我們不能假設使用者的機器上跑的是 7。

⚠️ **相關但不同的一條**（舊版第 51 條）：不要用
`Get-Content | ConvertFrom-Json … | ConvertTo-Json | Out-File` 去改 `settings.json`
——PowerShell 5.1 的 `ConvertTo-Json` 預設深度只有 2，巢狀結構會被截成字串。

---

## 目錄結構

```
AwayTerminal2/
├─ CLAUDE.md               # 專案規格 / 規劃（勿隨意改）
├─ LICENSE                 # MIT
├─ THIRD-PARTY-NOTICES.md  # xterm.js、Windows Terminal(conpty.dll/OpenConsole)、Tauri…
├─ index.html              # Vite 進入點，單一全視窗終端
├─ package.json
├─ vite.config.js          # 固定埠 1420、不自動開瀏覽器
├─ docs/
│  ├─ DEV-SETUP.md         # 本文件
│  └─ IPC-BENCH.md         # Tauri IPC 四條路的實測與採用理由（CLAUDE.md 風險 5）
├─ src/
│  └─ main.js              # xterm.js 初始化、渲染器選擇、session 接線、IPC bench
├─ src-tauri/
│  ├─ Cargo.toml           # tauri / serde / serde_json / tokio / windows-sys / libloading
│  ├─ build.rs             # tauri_build + 把 resources/conpty 複製到 target/<profile>/conpty
│  ├─ tauri.conf.json      # productName AwayTerminal、mainBinaryName、bundle.resources
│  ├─ icons/               # 由舊版 icon/app-icon.png 用 `npx tauri icon` 產生
│  ├─ resources/conpty/    # Windows Terminal 的 conpty.dll + OpenConsole.exe（MIT，微軟簽章）
│  ├─ examples/
│  │  └─ pty_probe.rs      # 不開視窗的 PTY 層驗證（cargo run --example pty_probe）
│  └─ src/
│     ├─ main.rs           # release 不開主控台視窗
│     ├─ lib.rs            # tauri Builder、指令註冊、結束時收掉所有 session
│     ├─ startup.rs        # 環境變數清理 / TERM / COLORTERM（照搬舊版 App.xaml.cs）
│     ├─ session.rs        # trait TerminalSession + SessionManager
│     ├─ output.rs         # PTY 輸出批次合併後送 channel（OutputPump）
│     ├─ commands.rs       # session_create / write / resize / close、ping、log_line
│     ├─ bench.rs          # IPC 傳輸量測用的四個指令
│     └─ pty/
│        ├─ mod.rs         # 平台切換
│        ├─ conpty.rs      # Windows ConPTY 本體（照舊版 ConPty/*.cs 翻寫）
│        ├─ conpty_host.rs # 載入 conpty.dll（OpenConsole），缺檔退回內建 conhost
│        ├─ shell.rs       # 找 pwsh / powershell
│        └─ unix.rs        # mac / Linux stub
├─ reference/              # 不進 repo（.gitignore）
│  └─ AwayTerminal/        # 舊版原始碼，行為規格與回歸測試清單來源
└─ .ai/                    # Multi-Agent 信箱，不進 repo
```

## ConPTY 後端

`src-tauri/resources/conpty/` 放 Windows Terminal 的 `conpty.dll` + `OpenConsole.exe`
（MIT、微軟 Authenticode 簽章，取自 `node-pty@1.1.0`，SHA-256 記在
`resources/conpty/README.md` 與 `THIRD-PARTY-NOTICES.md`）。兩個檔會到兩個地方：

| 情境 | 由誰複製 | 位置 |
|---|---|---|
| `cargo build` / `cargo run --example` / `npm run tauri dev` / 直接跑 release exe | `build.rs` | `src-tauri/target/<profile>/conpty/` |
| MSI / NSIS 安裝後 | `tauri.conf.json` 的 `bundle.resources` | 安裝目錄的 `conpty\`（exe 旁） |

> `bundle.resources` **只**影響安裝檔，不會放到 `target/release/` 的 exe 旁邊，
> 所以 `build.rs` 那份複製是必要的（否則直接跑 release exe 會靜靜退回 Win10 內建 conhost）。

載入失敗或缺檔時自動退回 kernel32 的 `CreatePseudoConsole`（內建 conhost），行為同舊版。
環境變數（同舊版）：

- `AWAYTERMINAL_CONPTY=inbox` — 強制用內建 conhost，用來對照舊行為。
- `AWAYTERMINAL_CONPTY_DIR=<dir>` — 指定另一份 conpty.dll / OpenConsole.exe。

實際使用哪一個會印在啟動 log：`[AwayTerminal] ConPTY backend: ...`，
前端也可以 `invoke('conpty_backend')` 問，並印在終端第二行。

## 最低版本

- **Rust 1.89**（`Cargo.toml` 的 `rust-version`）。`russh` 0.63.3 要求它。
- Node 20+（Vite 7 與沙盒護欄腳本用）。

## ⚠️ dev 第一次啟動偶發失敗（重跑就好）

Rust 剛重新編譯之後的第一次 `npm run tauri dev`，有時候視窗根本不載入，log 只有：

```
[0926/235302.181:ERROR:ui\gfx\win\window_impl.cc:172] Failed to unregister class Chrome_WidgetWin_0. Error = 1411
error: process didn't exit successfully: `target\debug\awayterminal.exe` (exit code: 143)
```

**這是 WebView2 啟動的暫時性失敗，不是程式的 bug——直接重跑一次就正常。**
2026-09-26～27 的驗證過程遇到三次，每次重跑都成功。

⚠️ **不要為了這件事去找行程砍。** 這個開發團隊自己就跑在舊版 AwayTerminal 底下
（`.ai/bus/0014`），而新版 exe 的名稱（`awayterminal.exe`）和舊版（`AwayTerminal.exe`）
在 Windows 上不分大小寫——按名稱砍等於把團隊連自己一起砍掉。要確認有沒有殘留，用

```powershell
Get-CimInstance Win32_Process -Filter "Name='awayterminal.exe'" | Select-Object ProcessId, CommandLine
```

看**命令列**：`C:\Program Files\AwayTerminal\...` 是使用者的舊版，絕對不能動。

## 驗證用工具（都不需要視窗焦點）

```powershell
cargo run --example pty_probe    # PTY 層：PowerShell exit code / cmd exit 偵測 / resize
cargo run --example ssh_probe    # SSH：同一支程式裡起測試 sshd，不連任何外部主機
cargo run --example log_probe    # log 格式（BOM / LF / 去 ANSI / 時間戳）
cargo test                       # 單元測試
cargo clippy --all-targets -- -D warnings
node ..\scripts\test-sandbox-guard.mjs   # 沙盒護欄的 deny/allow 清單
```

整套 `--verify`（會開視窗、跑完自己關掉）**一律走這支包裝**，不要自己用 `timeout` 包：

```powershell
npm run verify           # ＝ node scripts/dev-verify.mjs 2 420（分頁數、逾時秒）
npm run verify 3 600
```

⚠️ **為什麼一定要用它**：直接 `timeout ... npx tauri dev` 逾時只砍最外層，留下
`npx → @tauri-apps/cli → vite（佔著 1420）` 與 `target\debug\awayterminal.exe → OpenConsole.exe`
一整串孤兒；那個 `awayterminal.exe` 抓著 `target\debug`，下一次 `cargo build` 就會
`os error 32`（TASK-017 留了一隻，把 PM 的建置擋住）。`dev-verify.mjs` 收尾時用
`taskkill /PID <pid> /T /F` **依 PID 收整棵樹**（絕不依名稱——依名稱會把使用者的
AwayTerminal 和正在跑的代理團隊一起砍掉），並清掉 `%TEMP%` 底下的 `awayterm-verify-team-*`。

在 devtools 裡（`npm run tauri dev`）可用：

```js
awayBench(1, 10)      // IPC 四條路 × 1MB × 10 次，結果同時進後端 log
awayBenchSmall(200)   // channel 的 512B / 2048B（1024 門檻兩邊）
awayDump(6)           // 把 xterm buffer 尾端印到後端 log，驗證輸出真的進到畫面
```

## 前端目前的行為（階段 1 骨架）

- xterm.js 單一終端鋪滿視窗，`FitAddon` 隨視窗縮放（resize + ResizeObserver，30ms debounce）。
- 渲染器：先試 `@xterm/addon-webgl`，失敗時 `console.warn` 並退回 DOM 渲染；
  **實際使用的渲染器印在畫面第一行**（`[AwayTerminal2] renderer = WebGL` 或 `DOM`）。
  另外掛了 `onContextLoss`（休眠喚醒 / 驅動重置）→ 卸載 addon 自動退回 DOM。
- `Unicode11Addon` + `unicode.activeVersion = '11'`：全形字寬度用 Unicode 11 規則，比 xterm 內建 v6 準。
- 已載入但尚未接 UI 的 addon：`search`、`serialize`、`web-links`。
- `invoke('ping')` 驗證 Tauri IPC；`invoke('conpty_backend')` 印出實際的 ConPTY 主機。
- `invoke('report_renderer', { renderer })` / `invoke('log_line', { msg })` 把訊息印到後端 stdout，
  所以不開 devtools 也能從 `npm run tauri dev` 的啟動 log 拿到資料。
  （release build 有 `windows_subsystem = "windows"`，不會有主控台，這些只在 dev 看得到。）
- **啟動即開一條 ConPTY PowerShell**：輸出走 channel 的原始 bytes（`term.write(Uint8Array)`，
  不先 decode 成字串）；`onData` → `session_write_text`、`onBinary` → `session_write`；
  視窗縮放 → `session_resize`（欄列沒變就不送）；收到結束事件印
  `[行程已結束，exit code N]`。
- 輸入處理刻意保持最簡單。舊版 `web/terminal.js` 的輸入佇列 / IME / 貼上調校**還沒搬**。

## 從舊版照搬的踩雷修正（TASK-002）

這幾條是舊版流血換來的，新版都保留了，位置寫在括號裡：

1. **子行程結束一定要等 process handle**，不能等輸出管線 EOF。ConPTY 的 conhost 在子行程結束後
   不會關輸出管線（要等我們 `ClosePseudoConsole`）→ 光靠讀取迴圈的 EOF 永遠等不到，
   舊版實測 claude 按 Ctrl+C 離開、powershell 打 `exit` 後分頁毫無反應就是這個原因。
   （`pty/conpty.rs` 的 `spawn_waiter`；結束後再等 150ms 讓最後一批輸出送完，
   結束事件用 atomic 保證只發一次。）
2. **`CreateProcess` 的那一刻 std handle 必須是空的**。`CreateProcess` 會把父行程的 std handle
   「值」原樣帶給 console 子行程，而 ConPTY 只在 std handle 為空時才換成 pseudoconsole 的 handle。
   父行程若有 stdout（腳本／CI／Claude Code 工具環境的 pipe，或 `cargo run` 的主控台），
   子行程會寫到那裡而不是 PTY。**本專案的 `pty_probe` 第一次執行就重現了**：
   `AWAY_OK` 印在 probe 自己的主控台、PTY 只收到 conhost 的開場序列 `ESC[1t ESC[c …`。
   舊版是在啟動時全域歸零；新版改成只包住 `CreateProcess` 那一瞬間再還原
   （`pty/conpty.rs` 的 `with_null_std_handles`），這樣不會弄壞自己的 stdout。
3. **輸入管線要加鎖**：打字、巨集、遠端指令、關閉時的 Ctrl+C 可能同時寫，沒鎖會互相蓋掉
   位元組（按鍵消失）。（`ConPtySession.write_lock`）
4. **子行程掛上後要 `ConptyReleasePseudoConsole`**，最後一個 client 離開時 OpenConsole 才會
   自己收掉；否則強殺會留殭屍 OpenConsole 鎖住資料夾。程式結束時也走
   `RunEvent::Exit` → `close_all()`。（`pty/conpty.rs`、`lib.rs`）
5. **關閉先送優雅結束鍵**（PowerShell / Claude Code = Ctrl+C ×3），等 60ms 再
   `TerminateProcess` → `ClosePseudoConsole`；順序反了會阻塞。（`ConPtySession::close`）
6. **不要用 `PSEUDOCONSOLE_INHERIT_CURSOR`**：會讓 conhost 發 DSR，時機不對會把
   `ESC[r;cR` 漏進子行程的輸入。
7. **環境變數**：清掉 `NO_COLOR`、`GIT_TERMINAL_PROMPT` 與所有 `CLAUDE*` / `ANTHROPIC*`
   （字首掃，清單追不上 claude 新增變數的速度），設 `TERM=xterm-256color`、
   `COLORTERM=truecolor`、`CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`。（`startup.rs`）
8. **不要加 xterm 的 `windowsPty` 選項**：舊版實測會造成 claude 輸入列殘字；
   OpenConsole 後端本身折行正確，也不需要它。

## 2026-09-26 實測結果

- `cargo clippy --all-targets`：無警告（只有一個無害的 `linker stdout` 提示，MSVC 中文版輸出）。
- `cargo test`：1 passed。
- `cargo run --example pty_probe`：3/3 PASS
  （pwsh `echo AWAY_OK; exit 7` → 輸出含 AWAY_OK + exit code 7 + 3.4 秒內結束；
  `cmd /c echo x` → exit code 0；`resize(120,40)` → `$Host.UI.RawUI.WindowSize` = 120x40）。
- `npm run tauri dev`：視窗開啟，啟動 log 出現
  `[AwayTerminal] ConPTY backend: conpty.dll (OpenConsole) …` 與
  `[AwayTerminal] renderer = WebGL`
  → **WebGL addon 在 Windows WebView2（153.0.4234.48）成功啟用**，沒有退回 DOM。
  子行程樹 `awayterminal.exe → OpenConsole.exe + pwsh.exe`；`awayDump()` 讀到的
  xterm buffer 尾端就是真的 `PS C:\…\src-tauri>` 提示字元（端到端不靠視窗即可驗證）。
- IPC 傳輸：見 `IPC-BENCH.md`。結論＝channel raw 50.8 MB/s、`Vec<u8>` 只有 7.4 MB/s
  （確實變成 JSON 數字陣列）、舊版的 base64 是 15.1 MB/s。
- 相依版本：tauri 2.11.6、tauri-build 2.6.3、@xterm/xterm 5.5.0、Vite 7.3.6、
  windows-sys 0.61、libloading 0.8.9。

> 注意：`tauri.conf.json` 要設 `mainBinaryName: "AwayTerminal"`，否則執行檔會叫
> `awayterminal.exe`（Cargo package 名），與產品名不一致。

## 行尾規則（`.gitattributes`）

**結論：不用管。** 用你習慣的編輯器、用腳本改檔案都可以，git 會處理。

### 規則

`repo 裡一律存 LF，簽出時照平台`（Windows 拿到 CRLF）。`.gitattributes` 定的，
所以不依賴每個人的 `core.autocrlf` 設定。

固定行尾的例外：

| 型態 | 行尾 | 為什麼 |
|---|---|---|
| `.ps1`／`.bat`／`.cmd`／`.nsh`／`.nsi` | **CRLF** | Windows 工具鏈的地盤；makensis 對 CRLF 最沒有意外 |
| `.sh`／`.desktop` | **LF** | CRLF 會讓 `#!/usr/bin/env bash` 變成 `bash\r`，Linux 直接報 `bad interpreter` |
| 測試 fixture（`claude-screen.txt`、`runtime-context.txt`、`resources/multiagent/**`、`resources/chatroom/**`、`tests/ttl/*.ttl`） | **LF** | 要**逐字比對**的固定資料，在每個平台都必須完全一樣。角色範本還被 `include_str!` 嵌進 exe、`roles.rs` 有雜湊檢查 |
| 二進位（png／ico／icns／exe／dll／msi／字型／**`.ppk`**） | 不處理 | 當成文字去「正規化」會**直接損壞檔案**。`.ppk` 雖然是文字，但那是測試向量，一個 byte 都不准動 |

### 為什麼要特別立這條規則（TASK-024 的教訓）

沒有 `.gitattributes` 的時候，`core.autocrlf=true` 只管「簽出時轉 CRLF」，
**不保證 blob 是 LF**。當時 `docs/REGRESSION-CHECKLIST.md` 的 blob 是 CRLF，
而我用 python 腳本重寫它時寫進 LF ——那個 commit 就顯示 **2264 行變動，真正的內容
只差 36 行**。

後果不是「檔案壞了」，是**沒辦法 review**：PM 打開 diff 看到整份檔案都紅綠相間，
沒辦法判斷我到底改了什麼。真正的變更藏在雜訊裡，比沒有變更更危險。

要看一份被行尾雜訊淹掉的 diff：

```powershell
git diff --ignore-cr-at-eol <ref1> <ref2> -- <檔案>
```

### 用腳本改檔案時

Python 的 `io.open(path, encoding='utf-8')` 讀的時候會把 `\r\n` 變成 `\n`
（universal newlines），寫的時候預設又轉回 `os.linesep`——所以**在 Windows 上
來回一趟會保持 CRLF**。有了 `.gitattributes` 之後這件事已經不重要了
（blob 一律是 LF），但如果你想讓工作目錄乾淨一致，寫檔時明確指定：

```python
io.open(path, "w", encoding="utf-8", newline="\n")   # 一律寫 LF
```

⚠️ 含中文的 `.ps1` **另外**還要 UTF-8 **BOM**（編碼，和行尾是兩件事）——
見上面那一節，`scripts/audit-pitfalls.mjs` 會檢查。

---

## ⚠️ 不要對這個 repo 跑 prettier 或任何格式化工具

這個 repo **刻意沒有** `.prettierrc`，也**不要加**（PM 決定，2026-09-27）：

1. `src/terminal.js` 是從舊版逐字搬過來的，靠「diff 只有那幾行」來證明行為沒變
   （`docs/TERMINAL-JS-DIFF.md`）。格式化工具一跑就全毀。
2. 其餘檔案的風格本來就不統一，沒有一個設定能同時符合。

沒有設定檔時 `npx prettier --write` 會用它自己的預設值（雙引號、80 欄）把**整個檔**重排：
TASK-017 我在 5 個檔上跑了一次，diff 從約 800 行變成 5740 行，只能 `git checkout` 還原再重做。
要調格式就手改那幾行。

## 尚未做的事（後續任務）

- 搬入舊版 `web/terminal.js`（輸入佇列、IME、貼上等調校）。
- mac / Linux 的 forkpty 後端（`pty/unix.rs` 目前是 stub）。
- SSH / Telnet / COM、分頁 / 分割 / 分欄、log、我的最愛等。
- 量舊版基準（啟動時間、`cat` 大檔、記憶體）再和新版對比。
