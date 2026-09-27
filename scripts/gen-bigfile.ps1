# 產生基準測試用的大檔（舊版 vs 新版「cat 大檔」對比用）。
#
#   pwsh -File scripts\gen-bigfile.ps1
#
# 產出（預設在 %TEMP%\awayterm-bench\）：
#   big-5mb.txt   暖機 / 快速比較用
#   big-50mb.txt  正式比較用
#
# 內容刻意混了三種東西，因為它們走 xterm.js 裡不同的成本路徑：
#   * ASCII                 基本吞吐
#   * 中文全形字            寬度計算 + 字型 fallback + WebGL 字型 atlas
#   * ANSI SGR 顏色序列     parser 狀態機 + 每格屬性
# 每行約 120 個顯示欄位，接近終端機實際寬度，才不會整段都在測換行。

[CmdletBinding()]
param(
    [string] $OutDir = (Join-Path $env:TEMP 'awayterm-bench'),
    [int[]]  $SizesMb = @(5, 50)
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}

# 96 種前景色（SGR 38;5;n）輪流用，避免整段同色被 xterm 的屬性比較最佳化掉
$colors = 16..111
$cjk = '測試中文全形字寬度與字型後備繁體字'

function New-Line([int] $i) {
    $c1 = $colors[$i % $colors.Count]
    $c2 = $colors[($i * 7 + 13) % $colors.Count]
    $tag = '{0:D8}' -f $i
    $cjkPart = $cjk.Substring(0, 6 + ($i % 12))          # 6~17 個全形字＝12~34 欄
    $ascii = 'abcdefghijklmnopqrstuvwxyz0123456789'
    $pad = $ascii * 3
    # ESC = [char]27
    $e = [char]27
    $line = "$e[38;5;${c1}m$tag$e[0m $e[38;5;${c2}m$cjkPart$e[0m $pad"
    # 修到約 120 欄（全形算 2 欄；這裡用近似值就夠，目的是行長一致）
    $visible = $tag.Length + 1 + ($cjkPart.Length * 2) + 1 + $pad.Length
    if ($visible -gt 120) {
        $cut = $visible - 120
        $line = "$e[38;5;${c1}m$tag$e[0m $e[38;5;${c2}m$cjkPart$e[0m " + $pad.Substring(0, [Math]::Max(0, $pad.Length - $cut))
    }
    return $line
}

foreach ($mb in $SizesMb) {
    $path = Join-Path $OutDir "big-${mb}mb.txt"
    $target = $mb * 1MB
    Write-Host "產生 $path（目標 $mb MB）..."

    # 明確用 UTF-8 無 BOM 寫出：PowerShell 5.1 的 > / Out-File 會用系統 ANSI（本機 Big5）
    # 把中文寫壞（舊版 CLAUDE.md 踩雷）。StreamWriter 一次開好，逐行寫才不會吃光記憶體。
    $enc = New-Object System.Text.UTF8Encoding($false)
    $sw = New-Object System.IO.StreamWriter($path, $false, $enc)
    try {
        $i = 0
        $written = 0L
        # 先產生一批樣板行重複使用，比每行重算快很多（50MB 大約 40 萬行）
        $templates = @(0..255 | ForEach-Object { New-Line $_ })
        while ($written -lt $target) {
            $line = $templates[$i % $templates.Count]
            $sw.WriteLine($line)
            $written += $line.Length + 2
            $i++
        }
        Write-Host ("  完成：{0:N0} 行" -f $i)
    }
    finally {
        $sw.Dispose()
    }

    $actual = (Get-Item $path).Length
    Write-Host ("  檔案大小：{0:N0} bytes（{1:N1} MB）" -f $actual, ($actual / 1MB))
}

Write-Host ''
Write-Host "輸出目錄：$OutDir"
Write-Host '量測步驟見 docs\BENCHMARK.md'
