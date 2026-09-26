# 舊版 vs 新版 基準量測

CLAUDE.md：「**動工前先量舊版基準**：啟動時間、`cat` 大檔時間、記憶體用量；新版做完再對比。」
本文件是量測步驟與待填表格。**數字還沒量**——`docs/IPC-BENCH.md` 是 IPC 層的微觀量測，
這裡要量的是使用者感受到的端到端。

## 受測對象

| | 路徑 |
|---|---|
| 舊版 v1.2.x（C# WPF + WebView2） | `C:\Program Files\AwayTerminal\AwayTerminal.exe` |
| 新版 v2.0.0（Tauri 2 + xterm.js） | `src-tauri\target\release\AwayTerminal.exe`（**一定要用 release**） |

兩邊都要：關掉其他吃 CPU 的程式、接電源（筆電不要省電模式）、同一個顯示器與解析度、
同樣的視窗大小（欄×列會影響 xterm 的成本）。每項量 3 次取中位數。

## 0. 準備測試檔

```powershell
pwsh -File scripts\gen-bigfile.ps1
# 產生 %TEMP%\awayterm-bench\big-5mb.txt 與 big-50mb.txt
# 內容混 ASCII + 中文全形字 + ANSI 256 色，每行約 120 欄
```

## 1. 啟動時間（到提示字元出現）

**這一項必須由使用者做**，原因：舊版 `AwayTerminal.exe` 正在跑這個代理團隊，
代理不能重啟它；而「提示字元出現」也只有看畫面才判斷得準。

做法二選一：

- **手機碼表**：按下捷徑到看到 `PS C:\…>` 出現。粗但夠用，差距夠大時最直觀。
- **`Measure-Command` + 目視**：

  ```powershell
  # 只量到行程可以回應（不含畫面畫完），會低估，但可重複性好
  Measure-Command { $p = Start-Process 'C:\Program Files\AwayTerminal\AwayTerminal.exe' -PassThru; $p.WaitForInputIdle(20000) }
  ```

  `WaitForInputIdle` 對 Tauri 視窗也有效，但兩邊「idle」的定義不完全一樣，
  所以**碼表那組才是主要數字**，這個當輔助。

| 量測 | 舊版 | 新版 | 備註 |
|---|---|---|---|
| 冷啟動（開機後第一次） | | | |
| 熱啟動（關掉再開） | | | |
| 到提示字元出現 | | | 碼表 |

## 2. `cat` 大檔（輸出吞吐）

在終端機裡跑，`Measure-Command` 的時間由 shell 自己回報，不需要碼表：

```powershell
# 5MB 暖機一次（不記錄），再量 50MB
Measure-Command { Get-Content -Raw "$env:TEMP\awayterm-bench\big-5mb.txt" | Out-Host }
Measure-Command { Get-Content -Raw "$env:TEMP\awayterm-bench\big-50mb.txt" | Out-Host }

# 對照組：cmd 的 type（走不同的寫入路徑，不經 PowerShell 的格式化）
Measure-Command { cmd /c type "$env:TEMP\awayterm-bench\big-50mb.txt" }
```

> 注意：`Measure-Command` 量的是「shell 把資料寫完」，不等於「畫面畫完」。
> xterm.js 的 write 是排隊非同步處理的，所以**也要記下「指令回來之後畫面還要多久才停止滾動」**
> （目視即可，或在 devtools 打 `awayDump()` 看最後一行是不是已經到檔尾）。

| 量測 | 舊版（DOM 渲染） | 新版（WebGL） | 備註 |
|---|---|---|---|
| 5MB `Get-Content \| Out-Host` | | | 暖機 |
| 50MB `Get-Content \| Out-Host` | | | |
| 50MB `cmd /c type` | | | |
| 指令回來後畫面續滾時間 | | | 目視 |

若要拆開「IPC 傳輸」與「xterm 渲染」兩段成本，新版可在 devtools 用
`awayBench(50, 3)`（純 IPC，不畫）和上面的實測相減。

## 3. 記憶體（工作集）

新版的行程樹比舊版多一層 webview 子行程，**一定要加總整族**，只看主行程會嚴重低估：

```powershell
# 舊版：AwayTerminal.exe + msedgewebview2.exe（WebView2）+ OpenConsole.exe + pwsh.exe
# 新版：awayterminal.exe / AwayTerminal.exe + msedgewebview2.exe + OpenConsole.exe + pwsh.exe
function Get-TreeWS([int] $rootPid) {
    $all = Get-CimInstance Win32_Process
    $ids = New-Object System.Collections.Generic.HashSet[int]
    $ids.Add($rootPid) | Out-Null
    $changed = $true
    while ($changed) {
        $changed = $false
        foreach ($p in $all) {
            if ($ids.Contains([int]$p.ParentProcessId) -and -not $ids.Contains([int]$p.ProcessId)) {
                $ids.Add([int]$p.ProcessId) | Out-Null; $changed = $true
            }
        }
    }
    $rows = $all | Where-Object { $ids.Contains([int]$_.ProcessId) }
    $rows | Select-Object ProcessId, Name, @{n='WS_MB';e={[math]::Round($_.WorkingSetSize/1MB,1)}} | Format-Table -AutoSize
    '總計 {0:N1} MB（{1} 個行程）' -f (($rows | Measure-Object WorkingSetSize -Sum).Sum / 1MB), $rows.Count
}
Get-TreeWS <主行程PID>
```

三個時間點各量一次：

| 量測 | 舊版 | 新版 | 備註 |
|---|---|---|---|
| 啟動後閒置 30 秒 | | | 一個 PowerShell 分頁 |
| `cat` 50MB 之後 | | | scrollback 吃掉的量 |
| 清畫面並閒置 30 秒後 | | | 有沒有回收 |

> 新版 scrollback 設 50000 行（`terminal.js` `makeTerm`），與舊版相同，所以這項可以直接比。

## 4. 安裝檔 / 磁碟

已經有數字，列在這裡當對照：

| | 舊版 | 新版 v2.0.0 |
|---|---|---|
| 安裝檔 | （待填，舊版 installer） | NSIS 1,666,444 bytes / MSI 2,318,336 bytes |
| 主執行檔 | （待填） | 3,154,944 bytes |
| 需要 .NET runtime | 是 | 否 |
| 需要 WebView2 Runtime | 是 | 是 |

## 5. 已知的預期差異（解讀數字時要記得）

- **Windows 上啟動時間的改善有限**（CLAUDE.md 風險 4）：新版還是用 WebView2，
  省掉的是 .NET + WPF 那兩層。mac / Linux 的改善才明顯。
- **`cat` 大檔的改善應該最明顯**：舊版是 DOM 渲染 + base64 字串 IPC，
  新版是 WebGL + 二進位 channel（IPC 層實測快 3.4 倍，見 `IPC-BENCH.md`）。
- **記憶體**：少了 .NET runtime，但多了 Rust 端的執行緒與緩衝；實際差多少要量。
- 新版目前**只有一個分頁、沒有分頁列 / 狀態燈輪詢**，舊版有。
  舊版的狀態燈每 0.6 秒查子行程樹，本身就有成本——這對舊版不公平，
  所以正式對比時要註明「新版功能還不完整」，不要當成最終結論。

## 填完之後

把數字填進上面的表格，並在 `CLAUDE.md`「速度改善點」旁註明實測結果
（CLAUDE.md 由 PM 決定何時更新）。
