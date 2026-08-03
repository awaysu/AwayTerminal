# 由 icon\app-icon.png（256x256）產生 MSIX 需要的各尺寸圖示到 msix\Images\。
# 非正方形的圖磚（Wide310x150 / SplashScreen）維持等比縮放後置中，不變形。
# 用法： powershell -ExecutionPolicy Bypass -File .\make-images.ps1

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$src = Join-Path $PSScriptRoot '..\icon\app-icon.png' | Resolve-Path
$out = Join-Path $PSScriptRoot 'Images'
New-Item -ItemType Directory -Force $out | Out-Null

# name = 寬x高；圖示本身佔畫布的比例（Windows 建議圖磚留白，故非 1.0）
$targets = @(
    @{ n = 'Square44x44Logo';               w = 44;  h = 44;  fill = 1.00 }
    @{ n = 'Square44x44Logo.targetsize-24'; w = 24;  h = 24;  fill = 1.00 }
    @{ n = 'Square44x44Logo.targetsize-32'; w = 32;  h = 32;  fill = 1.00 }
    @{ n = 'Square44x44Logo.targetsize-48'; w = 48;  h = 48;  fill = 1.00 }
    @{ n = 'Square44x44Logo.targetsize-256';w = 256; h = 256; fill = 1.00 }
    @{ n = 'Square71x71Logo';               w = 71;  h = 71;  fill = 0.66 }
    @{ n = 'Square150x150Logo';             w = 150; h = 150; fill = 0.66 }
    @{ n = 'Square310x310Logo';             w = 310; h = 310; fill = 0.66 }
    @{ n = 'Wide310x150Logo';               w = 310; h = 150; fill = 0.66 }
    @{ n = 'StoreLogo';                     w = 50;  h = 50;  fill = 1.00 }
    @{ n = 'SplashScreen';                  w = 620; h = 300; fill = 0.50 }
)

$img = [System.Drawing.Image]::FromFile($src)
try {
    foreach ($t in $targets) {
        $bmp = New-Object System.Drawing.Bitmap($t.w, $t.h, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        try {
            $g.Clear([System.Drawing.Color]::Transparent)
            $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $g.PixelOffsetMode   = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
            # 等比縮放：以較短邊為準乘上 fill，維持正方形圖示不變形
            $side = [int]([Math]::Min($t.w, $t.h) * $t.fill)
            $x = [int](($t.w - $side) / 2)
            $y = [int](($t.h - $side) / 2)
            $g.DrawImage($img, $x, $y, $side, $side)
        } finally { $g.Dispose() }
        $bmp.Save((Join-Path $out "$($t.n).png"), [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        Write-Host ("  {0,-34} {1}x{2}" -f $t.n, $t.w, $t.h)
    }
} finally { $img.Dispose() }

Write-Host ""
Write-Host "完成，共 $($targets.Count) 個檔案 → $out"
