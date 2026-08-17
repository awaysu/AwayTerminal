# AwayTerminal — 開發交付說明（v0.0.4）

一個 Windows 原生終端機模擬器：**C# WPF 外殼 + WebView2 內嵌 xterm.js（終端機渲染）+ ConPTY / 內建 Telnet / 序列埠**。

---

## 如何建置與執行

```powershell
cd <專案資料夾>
dotnet build                       # 建置
.\bin\Debug\net9.0-windows\AwayTerminal.exe   # 執行
```

> ⚠️ **防毒**：PC-cillin 等防毒會誤判（因為程式會用 ConPTY 開 PowerShell，且每次改版都是全新
> 雜湊、信譽從零開始）。請把**專案資料夾**加入防毒例外；若重建後又被殺，確認例外還在。
> 註：連 `[Diagnostics.Process]::Start` 啟動剛建置的 exe 都可能被擋（回 "Access is denied"），
> 這時改用 `cmd /c` 或 shell 直接執行即可，不是程式壞了。

- 需求：.NET 9 SDK、WebView2 Runtime（Win10/11 多半已內建，安裝檔亦含 bootstrapper）。
- 設定 / 歷史存於：`%LOCALAPPDATA%\AwayTerminal\settings.json`
- 版本號在 `AwayTerminal.csproj` 的 `<Version>`；每交付一版 +0.0.1（顯示在「關於」）。

---

## 目前完成度（P0–P4 全部完成）

### 工具列（由左至右，分三組 + 關於）
| 群組 | 按鈕 | 行為 |
|------|------|------|
| 連線 | PowerShell | 跳目錄視窗（可選書籤/新增目錄），確定後在該目錄開分頁；記住上次、預設桌面 |
| 連線 | SSH/Telnet | 跳連線視窗（類型 + IP 下拉歷史 + Port）；SSH 走 `ssh.exe`、Telnet 內建 TCP |
| 連線 | COM | 跳序列埠視窗（Port/Baud/Data/Parity/Stop/Flow）；預設 COM5/115200/8/None/1/None |
| 編輯 | 複製 | 複製終端機選取的文字 |
| 編輯 | 貼上 | 把剪貼簿貼進終端機 |
| 編輯 | 複製全部 | 複製整個緩衝區文字到剪貼簿 |
| 編輯 | 清畫面 | 清除該分頁畫面 |
| 設定 | 常用Prompt | Prompt 管理視窗（見下）|
| 設定 | 字體背景 | 設定字型/大小/文字色/背景色，即時套用到所有分頁 |
| 設定 | 遠端 | 保留（依你要求不做）|
| 設定 | 其他設定 | 中文 / English 切換 |
| — | 關於 | 版本 / 編譯時間 / 作者 awaysu@gmail.com（圖片渲染）/ 下載 / Source Code / 授權 © 2026 Chih-Wei Su (Awaysu) / 第三方元件 |

### 分頁
- 開連線後在工具列下方開分頁；標題 = `[狀態方塊][● log][M 巨集] 名稱`。
- **右鍵**：更改名稱 / 記錄 log / 執行巨集 / 關閉（關閉會再確認）。
- 滑鼠停留顯示完整標題 tooltip；標題過長自動省略。

### 狀態方塊（分頁最左小方塊）
- **綠**＝可輸入（閒置）、**橘**＝正在跑程式。
- PowerShell：用「子行程樹」判斷（有外部程式在跑 = 橘）。內建 cmdlet（如 `dir`）不會變橘，這是預期。
- SSH/Telnet/COM：用「近期有無輸出」判斷忙碌。
- **Claude Code**：偵測終端機鈴聲（BEL）——Claude 完成、等你輸入時 → 綠；你一按鍵送出 → 橘。
  （你的 `~/.claude/settings.json` 已設 `preferredNotifChannel: terminal_bell`，會送出 BEL。）

### log 記錄（● icon，灰=停 / 藍=記錄中）
- 點灰 icon 或右鍵「記錄 log」→ 跳視窗（存檔路徑、是否加每行時間戳、檔案存在時 append）→ 確定開始。
- 點藍 icon → 詢問是否停止 → 停止後用檔案總管開啟該 log 檔位置。
- log 為**乾淨純文字**（濾掉 ANSI 顏色碼）。

### 巨集（M icon，灰=停 / 紫=執行中）
- 點灰 icon 或右鍵「執行巨集」→ 選 `*.ttl` → 執行；紫 icon → 詢問是否停止。
- 支援 TeraTerm .ttl 的**常用子集**：`sendln` `send` `wait` `waitln` `pause` `mpause`
  `messagebox` `connect` `disconnect` `end` `exit`。字串 `'...'`/`"..."`，字元碼 `#nn`，`;` 註解。
- 範例檔：`samples\sample.ttl`。

### 常用 prompt 管理視窗
- 上半：標題清單 + 送出 / 刪除（刪除前確認）/ 取消。
- 下半：標題 + 內容（預設唯讀）+ 修改 / 儲存 / 取消清空。
- 「送出」把內容打進目前分頁；資料存在 settings.json。

### 中文支援
- 終端機顯示與輸入（IME）中文正常；全形寬度用 xterm unicode11 addon 處理。

---

## ⚠️ 兩個待你回來確認/補充的項目

1. **COM 的「清畫面時送 reset」**：規格說「com port 也送個 reset」，但沒定義 reset 是什麼
   （切 DTR？送 break？送固定字串？）。目前 `SerialSession.SendReset()` 是空的、清畫面只清畫面。
   告訴我 reset 要送什麼，我補上。
2. **巨集指令範圍**：目前是常用子集。若你的實際 .ttl 用到其他指令（變數、`if/goto`、`inputbox`、
   `getdir`、`setbaud`…），把你的 .ttl 給我，我照著擴充。

---

## 已知限制 / 備註（可日後再調）
- i18n（中/英）目前套用於**工具列、關於、其他設定**；分頁右鍵選單與各連線視窗內部標籤仍為中文
  （之後可補齊）。
- Telnet 為最小實作（IAC 協商 + 字元模式），未做 NAWS 視窗大小通知。
- SSH 用系統 `ssh.exe`，金鑰/密碼/known_hosts 沿用你的 OpenSSH 設定。
- 分頁尚未支援拖曳重排、分割視窗（規格未要求）。
- exe 未簽章 → 防毒可能誤判（見上）。

---

## 專案結構
```
AwayTerminal.csproj        專案（.NET 9 WPF）
app.manifest               DPI 感知
MainWindow.xaml/.cs        主視窗（工具列/分頁/狀態輪詢/各功能接線）
ProcessTree.cs             子行程掃描（綠/橘判斷）
ConPty/                    ConPTY 原生封裝（PowerShell / SSH via ssh.exe）
Sessions/                  ITerminalSession / TelnetSession / SerialSession
Dialogs/                   各設定視窗（目錄/連線/COM/字體/prompt/log/設定/輸入）
Models/TerminalTab.cs      分頁資料模型
Services/AppSettings.cs    設定與歷史（JSON）
Localization/Loc.cs        中/英字串表
Logging/SessionLogger.cs   log 記錄器
Macros/MacroRunner.cs      TTL 巨集引擎
web/                       前端：index.html / terminal.js / vendor(xterm.js+addons)
samples/sample.ttl         範例巨集
```
