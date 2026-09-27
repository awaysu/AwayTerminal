# macOS／Linux：已經寫好的東西與待真機驗證的清單

TASK-022 的產出。目的是**等機器到手時，剩下的工作是「測試與修 bug」，不是「從空殼開始寫」**。

> ⚠️ **這份文件裡標「已寫」的每一項，都只被編譯器看過，沒有在真的 mac／Linux 上跑過。**
> 開發機是 Windows。

---

## 0. 為什麼有 `src-tauri/platform/` 這個 crate

**主 crate 沒辦法在 Windows 上跨 target `cargo check`**：

| target | 卡在哪 |
|---|---|
| `x86_64-unknown-linux-gnu` | `glib-sys`／`gdk-sys`／`gio-sys`／`pango-sys`／`atk-sys`／`cairo-sys-rs`／`gdk-pixbuf-sys`／`javascriptcore-rs-sys`／`soup3-sys` 的 build script 要 `pkg-config` ＋ GTK 開發標頭檔 |
| `aarch64-apple-darwin`／`x86_64-apple-darwin` | `objc2-exception-helper` 的 build script 要用 clang 編一段 Objective-C（需要 Apple SDK） |

兩邊都是**第三方 build script 先失敗**，我們自己的程式碼一行都沒被看過。
`cargo check` 不會跳過 build script，所以沒有辦法繞。

於是把平台相依的部分搬進 `src-tauri/platform/`（crate 名 `awayterm-platform`），
**只依賴 `libc`**（純 Rust 綁定，build script 不需要任何外部東西）。這樣真的可以：

```powershell
cd src-tauri\platform
cargo check  --target x86_64-unknown-linux-gnu
cargo check  --target aarch64-apple-darwin
cargo check  --target x86_64-apple-darwin
cargo clippy --all-targets --target <同上> -- -D warnings
```

主 crate 那一側的 `#[cfg(unix)]` 只剩很薄的接線（把這裡的型別包成 `TerminalSession`）。

**所以「編得過」的保證範圍是**：`platform` crate 的全部，加上 Windows 上編得到的
共用邏輯。**沒有**保證的是：主 crate 裡 `#[cfg(unix)]` 的那幾十行接線
（`src/pty/unix.rs`、`src/status.rs`／`src/sandbox.rs`／`src/com/mod.rs`／
`src/ttl/runner.rs`／`src/startup.rs` 的 unix 分支）。那幾處第一次在真機上 build
**很可能會有編譯錯誤**，都是小錯（型別、import），但要預留時間。

---

## 1. 已經寫好的（對照 `CLAUDE.md` 的平台差異表）

| 項目 | 在哪 | 做法 | 待驗證 |
|---|---|---|---|
| **PTY** | `platform/src/pty.rs` ＋ `src/pty/unix.rs` | `openpty` ＋ `fork`／`execvp`；子行程 `setsid` ＋ `TIOCSCTTY`；resize 用 `TIOCSWINSZ`；讀到 `EIO` 當成結束（Linux 的行為） | 真的開得起來、大小同步正確、`$SHELL` 是 zsh／bash 時提示字元正常 |
| **預設 shell** | `platform/src/pty.rs::default_shell`、`src/pty/shell.rs::local_shell` | `$SHELL` → 沒設就 mac `/bin/zsh`／Linux `/bin/bash`。**刻意不加 `-l`**（GUI 啟動、和 Windows 端對稱） | 不加 `-l` 會不會讓使用者的 PATH 少東西（mac 上很可能會，見下面的 `which`） |
| **PowerShell** | `src/pty/shell.rs::powershell` | **有裝 `pwsh` 才提供**（`CLAUDE.md` 明訂）。不是預設 | `pwsh` 在 Homebrew 裝的位置找得到 |
| **關閉流程** | `src/pty/unix.rs::close` | 送優雅結束鍵 → 等 60ms → 關 fd → `SIGHUP` → 等 400ms → `SIGKILL`（順序照 Windows 端） | claude／codex 收到 SIGHUP 會不會存好狀態 |
| **Job Object 的對應** | `platform/src/pgroup.rs` | 子行程 `setsid` 自己當群組 leader；收掉用 `killpg(SIGHUP)` → `killpg(SIGKILL)` | 沙盒分頁關掉時整棵真的收乾淨（對應 Windows 的 `job_probe`） |
| **狀態燈子行程樹** | `platform/src/proctree.rs` | Linux 掃 `/proc/*/stat` 的第 4 欄；mac `proc_listpids` ＋ `proc_pidinfo(PROC_PIDTBSDINFO)`（**自己宣告 FFI，不用 `libproc` crate**——那個要編 C） | mac 上拿不到別人的行程資訊時的行為；`/proc` 解析在行程名含括號時正確（已有單元測試） |
| **`pid_exists`** | `platform/src/proctree.rs` | `kill(pid, 0)`（mac 沒有 `/proc`，所以不看檔案系統） | — |
| **`which()`** | `platform/src/which.rs` | `PATH` ＋ `~/.local/bin`、`/opt/homebrew/bin`（Apple Silicon）、`/usr/local/bin`、`~/.cargo/bin`、`~/.npm-global/bin`、`~/go/bin`、`~/.nvm/versions/node/*/bin`；檢查**執行權限位元** | **mac 的 GUI 程式拿不到使用者的 PATH**（launchd 環境，不跑 `.zshrc`）——這幾個補充目錄夠不夠？使用者自訂 npm prefix 的情況 |
| **序列埠** | `platform/src/serial.rs` ＋ `src/com/mod.rs` | 列舉後過濾（mac `cu.*`／`tty.*` 且濾掉 Bluetooth；Linux `ttyUSB*`／`ttyACM*`／`ttyS*`／`ttyAMA*`）；**mac 開埠自動把 `tty.` 換成 `cu.`**（`tty.` 會等 DCD 卡住）；Linux 權限不足時提示 `sudo usermod -aG dialout <你>` | 真的插一條 USB 轉序列埠線 |
| **Linux 的 WebKitGTK 環境** | `platform/src/linuxenv.rs` ＋ `src/startup.rs` | 在 `run()` 第一行、建 webview **之前**設 `WEBKIT_DISABLE_DMABUF_RENDERER=1`／`WEBKIT_DISABLE_COMPOSITING_MODE=1`（**只在使用者沒設時**）；偵測 Wayland；`GTK_IM_MODULE` 沒設就提示（不猜值） | 白畫面到底是哪一個變數解決的；Wayland 與 X11 各跑一次 |
| **渲染器三段退回** | `src/main.js`、`settings.renderer` | WebGL → canvas → DOM，`auto` 會照順序試；設定視窗可手動選；關於頁顯示目前用哪個 | **canvas 那一段目前不會啟用**，見下面第 3 節 |
| **IME adapter** | `src/ime/detect.js`、`ime-chromium.js`、`ime-webkit.js` | 執行時偵測引擎（特徵優先於 UA），Chromium＝no-op、WebKit＝**空的**（等錄影） | 整個第 2 節 |
| **字型 fallback** | `src-tauri/src/settings.rs::font_fallback` | mac `Menlo → SF Mono → PingFang TC → Heiti TC`；Linux `DejaVu Sans Mono → Liberation Mono → Noto Sans Mono CJK TC` | 全形字寬、中文缺字 |
| **命令列切開** | `platform/src/cmdline.rs` | Windows 是一整條字串、Unix 要陣列；支援引號，**不做 shell 展開** | — |
| **TTL `exec`** | `src/ttl/runner.rs` | `sh -c` ＋ `process_group(0)`（Unix 的 Job Object 對應） | 巨集停止時整棵收掉 |
| **`getspecialfolder`** | `src/ttl/words.rs` | 本來就是 `Word::Unsupported`（Windows 專屬的 TTL 指令，兩個平台都回「不支援」） | — |
| **「用 AwayTerminal 開啟」** | `platform/linux/*`、`platform/macos/README.md` | Linux：Nautilus 腳本 ＋ Dolphin ServiceMenu（`.desktop`）；mac：Automator Quick Action 的建立步驟。都是**使用者自己裝**，安裝檔不寫（同 Windows） | 兩個桌面各試一次；mac 的 `open -a --args` 在 app 已在跑時參數會被丟掉 |
| **單一執行個體** | `tauri-plugin-single-instance` | 跨平台都由 plugin 處理（Windows 是 mutex ＋ 訊息、Unix 是 socket） | 第二個實例真的把 `--open-dir` 交過去、自己結束 |

### 設定視窗在 Unix 上要隱藏的東西

`shell_menu_state` 這個 command 只有 Windows 有，前端本來就 `.catch()` 之後把勾選灰掉
（`src/setdlg.js` 的 `fill()`）。**Unix 上會是灰的、附一行提示指向 `platform/` 的檔案**
——這一項待真機確認長相。

---

## 2. 第一天要跑什麼

### 共同（兩個平台都做）

```bash
# 1. 先確認 build 得起來（預期會有幾個小編譯錯誤，見第 0 節）
cd src-tauri && cargo build
# 2. 平台 crate 的單元測試（proc 解析、裝置路徑、PATH 順序、命令列切開）
cargo test -p awayterm-platform
# 3. 連線引擎的 probe（都只連 127.0.0.1／in-process，不碰外部主機）
cargo run --example pty_probe
cargo run --example ssh_probe
cargo run --example telnet_probe
cargo run --example com_probe
cargo run --example ttl_probe
# 4. 端到端
cd .. && npm run verify
```

`pty_probe` 是最重要的一支——它證明 `openpty` ＋ fork 那條路真的通。

### macOS 特有

| # | 做什麼 | 看什麼 |
|---|---|---|
| M1 | 啟動，看 stdout | `renderer = WebGL`（WKWebView 上 WebGL 應該穩定）、`webview 引擎＝webkit（wkwebview）` |
| M2 | 開 `public/ime-lab.html`，跑 `docs/IME-LAB.md` 的九項劇本 | **錄下事件序列**，和 `docs/ime-baseline/`（Windows）做差異表 → 寫進 `docs/IME-LAB.md`，再照表寫 `src/ime/ime-webkit.js` |
| M3 | 注音打一句 ＋ Enter | **有沒有多送一個 Enter**（`CLAUDE.md` 風險 1 的主要嫌疑） |
| M4 | 開一個 shell 分頁 | 是 `$SHELL`（zsh）；提示字元正常；分頁名跟著工作目錄變 |
| M5 | `which` 找得到 Homebrew 裝的東西嗎 | 從 **Dock／Finder 啟動**（不是從終端機！）之後，新分頁選單裡 node／pwsh／adb 的偵測結果 |
| M6 | 關分頁 | 子行程真的收掉（`ps` 看不到殘留） |
| M7 | 插 USB 轉序列埠線 | 列舉出 `cu.*`；開得起來（`tty.*` 不會卡住） |
| M8 | Cmd+C／Cmd+V | 走 webview 預設；貼上進 `terminal.js` 既有的 `paste` 攔截（**舊版沒有視窗層級快捷鍵，新版也不加**） |
| M9 | 沙盒分頁關掉 | 行程群組整棵收乾淨 |

### Linux 特有（Ubuntu 22.04 ＋ 24.04 × X11／Wayland × fcitx5／ibus ＝ 8 種組合）

| # | 做什麼 | 看什麼 |
|---|---|---|
| L1 | 啟動，看 stdout | `WebKitGTK 環境：…` 那一行；`renderer =` 是 WebGL 還是退回了 |
| L2 | **白畫面** | 有的話把兩個 `WEBKIT_DISABLE_*` 逐一試，記下**哪一個**解決的（現在是兩個都設） |
| L3 | WebGL 開不開 | 開不了就試 `settings.renderer = canvas`——**canvas addon 目前沒裝**（見第 3 節），所以這一步會退到 DOM；量一下 DOM 有多慢（`docs/BENCHMARK.md` 的方法） |
| L4 | fcitx5 打注音 | 打得出來嗎；`GTK_IM_MODULE` 沒設時的提示有出現嗎 |
| L5 | ibus 打注音 | 同上 |
| L6 | IME 錄影頁 | 同 M2（WebKitGTK 的事件順序可能和 WKWebView 又不一樣） |
| L7 | Wayland 下 | 視窗、輸入法、截圖（`/shot` 在 Wayland 下不能抓別的視窗——確認退回前端畫的那張） |
| L8 | 連接埠 | `/dev/ttyUSB0` 列得出來；**不在 dialout 群組時**的錯誤訊息是那句提示 |
| L9 | Nautilus／Dolphin 右鍵 | `platform/linux/` 的兩個檔裝起來、點了會開在那個資料夾 |
| L10 | 沙盒分頁關掉 | 行程群組整棵收乾淨 |

---

## 3. 已知的缺口與決定

### canvas 渲染器**沒有裝**（刻意）

`@xterm/addon-canvas` 已發佈的版本（穩定 **0.7.0**、預覽 **0.8.0-beta.48**）宣告的
peer 都是 `@xterm/xterm ^5.0.0`，我們用的是 **6.0.0**。硬裝的話：

- npm 會有 peer 衝突（要 `--legacy-peer-deps`）；
- renderer 是 xterm 的**內部介面**，5 → 6 之間改過，很可能在執行時就壞；
- 而且那條路**在 Windows 上永遠不會被執行到**，也就是壞了我們不會知道。

所以三段退回的**結構**做好了、位置留好了（`window.AwayCanvasAddon`），
但**沒有**加這個依賴。要啟用的話：裝 addon → 在 `main.js` 設
`window.AwayCanvasAddon = { CanvasAddon }` → 一行就接上。

**決定的時機**：等 Linux 真機測出「WebGL 不能用而 DOM 太慢」時再做，那時候也才有
環境可以驗。在那之前多裝一個宣告不相容的依賴沒有好處。

### `ime-webkit.js` 是空的（刻意）

`CLAUDE.md` 風險 1 的推測（WebKit 的 `compositionend` 在 Enter 之前 → 多一個 Enter）
**還沒有實測資料**。沒有錄影就寫修正有兩個風險：修錯地方，或更糟——把 Chromium 上
本來好的行為弄壞而我們在 Windows 上不會注意到（那個檔案在 Chromium 上不會被載入）。

流程寫在 `src/ime/ime-webkit.js` 的檔頭：錄影 → 差異表 → 一項差異一個函式 →
每寫一條回 Windows 跑 `npm run verify` ＋ P0-IME 那 12 條。

### 主 crate 的 unix 接線沒有編譯保證

見第 0 節最後一段。第一次在真機 `cargo build` 預留半天。

---

## 4. 需要的機器與工具

| 平台 | 需要 |
|---|---|
| macOS | 一台 mac（Apple Silicon 較好，順便驗 `/opt/homebrew`）、Xcode Command Line Tools、**Apple Developer ID**（年費已付）＋ `codesign`／`notarytool`（發佈時）、一條 USB 轉序列埠線 |
| Linux | Ubuntu 22.04 與 24.04（VM 可以）、各自 X11 與 Wayland、fcitx5 與 ibus、`libwebkit2gtk-4.1-dev`／`libgtk-3-dev`／`pkg-config`／`build-essential`（build 用）、`dialout` 群組、一條 USB 轉序列埠線 |
| 共同 | 使用者常連的 SSH 設備（`docs/SSH.md` 的「待真機驗證」清單） |

Linux 的 build 相依（Ubuntu 24.04）：

```bash
sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config
```

---

## 5. 發佈（等真機之後，不在這次範圍）

| 平台 | 要做 |
|---|---|
| macOS | universal binary（arm64 ＋ x64）、`codesign` ＋ notarization、`.dmg`。`docs/RELEASE.md` 的 Windows 流程可以照抄結構 |
| Linux | AppImage ＋ `.deb`（不用簽章）。AppImage 要確認 `WEBKIT_DISABLE_*` 有進 AppRun |
