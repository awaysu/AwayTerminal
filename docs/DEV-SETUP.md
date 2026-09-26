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

## 目錄結構

```
AwayTerminal2/
├─ CLAUDE.md               # 專案規格 / 規劃（勿隨意改）
├─ index.html              # Vite 進入點，單一全視窗終端
├─ package.json
├─ vite.config.js          # 固定埠 1420、不自動開瀏覽器
├─ docs/
│  └─ DEV-SETUP.md         # 本文件
├─ src/
│  └─ main.js              # xterm.js 初始化、渲染器選擇、IPC 驗證、loopback echo
├─ src-tauri/
│  ├─ Cargo.toml           # tauri / serde / serde_json / tokio
│  ├─ build.rs
│  ├─ tauri.conf.json      # productName AwayTerminal、identifier com.awaysu.awayterminal、1200x800
│  ├─ icons/               # 由舊版 icon/app-icon.png 用 `npx tauri icon` 產生
│  └─ src/
│     ├─ main.rs           # release 不開主控台視窗
│     └─ lib.rs            # #[tauri::command] ping() / report_renderer()
├─ reference/              # 不進 repo（.gitignore）
│  └─ AwayTerminal/        # 舊版原始碼，行為規格與回歸測試清單來源
└─ .ai/                    # Multi-Agent 信箱，不進 repo
```

## 前端目前的行為（階段 1 骨架）

- xterm.js 單一終端鋪滿視窗，`FitAddon` 隨視窗縮放（resize + ResizeObserver，30ms debounce）。
- 渲染器：先試 `@xterm/addon-webgl`，失敗時 `console.warn` 並退回 DOM 渲染；
  **實際使用的渲染器印在畫面第一行**（`[AwayTerminal2] renderer = WebGL` 或 `DOM`）。
  另外掛了 `onContextLoss`（休眠喚醒 / 驅動重置）→ 卸載 addon 自動退回 DOM。
- `Unicode11Addon` + `unicode.activeVersion = '11'`：全形字寬度用 Unicode 11 規則，比 xterm 內建 v6 準。
- 已載入但尚未接 UI 的 addon：`search`、`serialize`、`web-links`。
- 測試輸出：中文全形字、全形對齊檢查、16 色 / 256 色 / TrueColor。
- `invoke('ping')` 驗證 Tauri IPC，結果印在終端。
- `invoke('report_renderer', { renderer })` 把渲染器名稱印到後端 stdout，
  所以不開 devtools 也能從 `npm run tauri dev` 的啟動 log 看到 `[AwayTerminal] renderer = WebGL`。
  （release build 有 `windows_subsystem = "windows"`，不會有主控台，此行只在 dev 看得到。）
- 鍵盤輸入目前是**前端 loopback echo**，還沒有 PTY。

## 2026-09-26 實測結果

- `npm run build`：通過（dist 446 KB / gzip 113 KB）。
- `npm run tauri build`：通過，產出
  `src-tauri/target/release/AwayTerminal.exe`（3,108,864 bytes ≈ 3.0 MB）、
  `bundle/nsis/AwayTerminal_0.1.0_x64-setup.exe`（≈1.1 MB）、
  `bundle/msi/AwayTerminal_0.1.0_x64_en-US.msi`（≈1.6 MB）。
- `npm run tauri dev`：視窗正常開啟，啟動 log 出現 `[AwayTerminal] renderer = WebGL`
  → **WebGL addon 在 Windows WebView2（153.0.4234.48）成功啟用**，沒有退回 DOM。
- 相依版本：tauri 2.11.6、tauri-build 2.6.3、@xterm/xterm 5.5.0、Vite 7.3.6。

> 注意：`tauri.conf.json` 要設 `mainBinaryName: "AwayTerminal"`，否則執行檔會叫
> `awayterminal.exe`（Cargo package 名），與產品名不一致。

## 尚未做的事（後續任務）

- 搬入舊版 `web/terminal.js`（輸入佇列、IME、貼上等調校）。
- ConPTY / OpenConsole 後端、Tauri 二進位 channel 傳輸。
- SSH / Telnet / COM、分頁 / 分割 / 分欄、log、我的最愛等。
