# 執行一次即可：把 AwayTerminal 自簽憑證加入「你這台的信任根」，
# 讓 exe 的數位簽章顯示為「有效／受信任」。
# 執行時可能跳一個 Windows 安全性警告視窗 —— 請按「是(Yes)」。
#
# 用法（在 PowerShell）：  .\trust-cert.ps1
# 若沒有憑證，會自動先建立一張自簽憑證。

$subject = "CN=AwayTerminal (awaysu), O=AwayTerminal, C=TW"
$cert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object { $_.Subject -eq $subject -and $_.HasPrivateKey } |
    Select-Object -First 1

if (-not $cert) {
    Write-Host "建立自簽憑證..."
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $subject `
        -CertStoreLocation Cert:\CurrentUser\My -KeyUsage DigitalSignature `
        -KeyExportPolicy Exportable -NotAfter (Get-Date).AddYears(5)
}

$tmp = Join-Path $env:TEMP "awayterminal-codesign.cer"
Export-Certificate -Cert $cert -FilePath $tmp | Out-Null

Write-Host "將憑證加入 信任根(Root) 與 受信任的發行者(TrustedPublisher)..."
Write-Host "若跳出安全性警告，請按『是』。"
Import-Certificate -FilePath $tmp -CertStoreLocation Cert:\CurrentUser\Root
Import-Certificate -FilePath $tmp -CertStoreLocation Cert:\CurrentUser\TrustedPublisher

Write-Host ""
Write-Host "完成。驗證方式："
Write-Host '  Get-AuthenticodeSignature ".\bin\Debug\net9.0-windows\AwayTerminal.exe" | Select Status'
Write-Host "（應顯示 Valid）"
