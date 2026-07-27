param([string]$Path)
# build 後自動簽章用（由 csproj 的 SignOutput target 呼叫）。找不到憑證就安靜略過。
if (-not $Path -or -not (Test-Path $Path)) { exit 0 }
$subject = "CN=AwayTerminal (awaysu), O=AwayTerminal, C=TW"
$cert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object { $_.Subject -eq $subject -and $_.HasPrivateKey } |
    Select-Object -First 1
if (-not $cert) { Write-Host "[sign] 找不到憑證，略過（可執行 trust-cert.ps1 建立/信任）"; exit 0 }
try {
    Set-AuthenticodeSignature -FilePath $Path -Certificate $cert -HashAlgorithm SHA256 `
        -TimestampServer "http://timestamp.digicert.com" -ErrorAction Stop | Out-Null
    Write-Host "[sign] 已簽章 $Path"
} catch {
    try {
        Set-AuthenticodeSignature -FilePath $Path -Certificate $cert -HashAlgorithm SHA256 -ErrorAction Stop | Out-Null
        Write-Host "[sign] 已簽章（無時間戳）"
    } catch { Write-Host "[sign] 失敗: $_" }
}
exit 0
