# 打包 AwayTerminal 成 MSIX。
#
#   .\build-msix.ps1                 # 用現有的 bin\publish 打包並簽章（快）
#   .\build-msix.ps1 -Publish        # 先重跑 dotnet publish 再打包
#   .\build-msix.ps1 -NoSign         # 只產生未簽章 .msix（上架用：微軟會自己重簽）
#
# 產出： msix\out\AwayTerminal-<版本>.msix
#
# 注意：AppxManifest.xml 的 Publisher 必須與簽章憑證主體完全一致，否則裝不起來。
# 上架時 Identity 要換成 Partner Center 配發的值，並用 -NoSign 產生未簽章套件。

param(
    [switch]$Publish,
    [switch]$NoSign,
    [string]$CertSubject = 'CN=AwayTerminal (awaysu), O=AwayTerminal, C=TW'
)

$ErrorActionPreference = 'Stop'
$root      = Resolve-Path (Join-Path $PSScriptRoot '..')
# MSIX 用「自帶執行環境」的獨立輸出目錄，與 Inno 安裝檔的框架相依輸出（bin\publish）分開。
# MSIX 無法像 Inno 那樣偵測並安裝 .NET Desktop Runtime，Store 端也沒有桌面 .NET 的
# framework package，所以 Store 版必須自帶。兩者共用同一個目錄會互相覆蓋。
$publishIn = Join-Path $root 'bin\publish-msix'
$layout    = Join-Path $PSScriptRoot 'layout'
$outDir    = Join-Path $PSScriptRoot 'out'

# ---- 找 Windows SDK 工具（取版本號最大的） ----
$sdkRoot = 'C:\Program Files (x86)\Windows Kits\10\bin'
$sdkDir = Get-ChildItem $sdkRoot -Directory |
    Where-Object { Test-Path (Join-Path $_.FullName 'x64\makeappx.exe') } |
    Sort-Object { try { [version]$_.Name } catch { [version]'0.0.0.0' } } -Descending |
    Select-Object -First 1
if (-not $sdkDir) { throw "找不到 makeappx.exe，請安裝 Windows SDK。" }
$makeappx = Join-Path $sdkDir.FullName 'x64\makeappx.exe'
$signtool = Join-Path $sdkDir.FullName 'x64\signtool.exe'
Write-Host "SDK: $($sdkDir.Name)"

# ---- 1. publish ----
if ($Publish -or -not (Test-Path (Join-Path $publishIn 'AwayTerminal.exe'))) {
    Write-Host "`n[1/5] dotnet publish（自帶執行環境）..."
    # 先清空：CopyToOutputDirectory 不會刪除已從專案移除的檔案，殘留物會被打包進去
    if (Test-Path $publishIn) { Remove-Item $publishIn -Recurse -Force }
    & dotnet publish (Join-Path $root 'AwayTerminal.csproj') -c Release -r win-x64 --self-contained true `
        -o $publishIn -v q --nologo
    if ($LASTEXITCODE -ne 0) { throw "dotnet publish 失敗" }
} else {
    Write-Host "`n[1/5] 沿用現有 bin\publish-msix（要重建請加 -Publish）"
}

# ---- 2. 組 layout ----
Write-Host "[2/5] 組裝套件內容 ..."
if (Test-Path $layout) { Remove-Item $layout -Recurse -Force }
New-Item -ItemType Directory -Force $layout | Out-Null
Copy-Item (Join-Path $publishIn '*') $layout -Recurse -Force
Copy-Item (Join-Path $PSScriptRoot 'AppxManifest.xml') $layout -Force
Copy-Item (Join-Path $PSScriptRoot 'Images') $layout -Recurse -Force

# 安裝檔專用、MSIX 不需要的東西（MSIX 由 Store 管理信任與更新）
foreach ($junk in 'AwayTerminal.cer', 'trust-publisher.ps1') {
    $p = Join-Path $layout $junk
    if (Test-Path $p) { Remove-Item $p -Force }
}

# 版本號：由 exe 取，補足成 MSIX 要求的四段且最後一段必須是 0
$ver = [version]((Get-Item (Join-Path $layout 'AwayTerminal.exe')).VersionInfo.FileVersion)
$pkgVer = '{0}.{1}.{2}.0' -f $ver.Major, $ver.Minor, $ver.Build
$manifestPath = Join-Path $layout 'AppxManifest.xml'
$xml = [xml](Get-Content $manifestPath)
$xml.Package.Identity.Version = $pkgVer
$xml.Save($manifestPath)
Write-Host "      套件版本 $pkgVer"

$files = Get-ChildItem $layout -Recurse -File
Write-Host "      $($files.Count) 檔 / $([math]::Round(($files | Measure-Object Length -Sum).Sum/1MB,1)) MB"

# ---- 3. 打包 ----
Write-Host "[3/5] makeappx pack ..."
New-Item -ItemType Directory -Force $outDir | Out-Null
$msix = Join-Path $outDir "AwayTerminal-$pkgVer.msix"
if (Test-Path $msix) { Remove-Item $msix -Force }
# /o 覆寫、/l 保留大小寫長路徑檔名
& $makeappx pack /d $layout /p $msix /o /l | ForEach-Object { if ($_ -match 'error|fail') { Write-Host $_ -ForegroundColor Red } }
if ($LASTEXITCODE -ne 0) { throw "makeappx 失敗（exit $LASTEXITCODE）" }

# ---- 4. 簽章 ----
if ($NoSign) {
    Write-Host "[4/5] 略過簽章（-NoSign）—— 上架用，微軟會以你的發行者身分重新簽章"
} else {
    Write-Host "[4/5] signtool sign ..."
    $cert = Get-ChildItem Cert:\CurrentUser\My |
        Where-Object { $_.Subject -eq $CertSubject -and $_.HasPrivateKey } | Select-Object -First 1
    if (-not $cert) { throw "找不到憑證 $CertSubject（可執行 trust-cert.ps1 建立）" }
    # MSIX 的 Publisher 必須與憑證主體完全相同，否則安裝時會被判 identity 不符
    $manifestPublisher = ([xml](Get-Content (Join-Path $PSScriptRoot 'AppxManifest.xml'))).Package.Identity.Publisher
    if ($manifestPublisher -ne $cert.Subject) {
        throw "資訊清單 Publisher 與憑證主體不符：`n  manifest: $manifestPublisher`n  cert    : $($cert.Subject)"
    }
    & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint /tr http://timestamp.digicert.com /td SHA256 $msix
    if ($LASTEXITCODE -ne 0) { throw "signtool 失敗（exit $LASTEXITCODE）" }
}

# ---- 5. 結果 ----
Write-Host "[5/5] 完成"
$f = Get-Item $msix
Write-Host ""
Write-Host "  $($f.FullName)"
Write-Host "  $([math]::Round($f.Length/1MB,1)) MB"
if (-not $NoSign) { Write-Host "  簽章: $((Get-AuthenticodeSignature $msix).Status)" }
Write-Host ""
Write-Host "側載測試： Add-AppxPackage '$($f.FullName)'"
Write-Host "移除：     Get-AppxPackage *AwayTerminal* | Remove-AppxPackage"
