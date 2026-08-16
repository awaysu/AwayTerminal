# AwayTerminal —— 選擇性信任發行者憑證（**不需要**執行這個程式也能正常使用）
#
# 用途：AwayTerminal 使用自簽憑證簽章，Windows 預設不認得，因此執行時 UAC 會顯示
#       「不明的發行者」。跑過這個腳本之後，UAC 會改為顯示「Awaysu」。
#
# 這個腳本只匯入「公開憑證」（不含私鑰），而且只寫進**你自己的**憑證存放區
# （CurrentUser），不影響這台電腦的其他使用者，也不需要系統管理員權限。
#
# 用法：在這個資料夾按右鍵 →「在終端機中開啟」，然後執行
#           powershell -ExecutionPolicy Bypass -File .\trust-publisher.ps1
#
# 要復原（移除信任）：
#           powershell -ExecutionPolicy Bypass -File .\trust-publisher.ps1 -Remove

param([switch]$Remove)

$ErrorActionPreference = 'Stop'
# 2026-08-13 起改用跨程式共用的「Awaysu」憑證（舊安裝檔簽的 AwayTerminal 憑證仍可由 -Remove 段清除）
$subject = 'CN=Awaysu, O=Awaysu, C=TW'
$stores  = @('Cert:\CurrentUser\Root', 'Cert:\CurrentUser\TrustedPublisher')

if ($Remove) {
    # 注意：Cert:\CurrentUser\Root 的檢視是「使用者存放區 ∪ 機器存放區」的聯集，
    # 因此這裡會列到 1.0.11 以前的安裝檔寫進 LocalMachine 的那張。刪機器層級的
    # 需要系統管理員權限，一般使用者會拿到 Access denied——逐張處理並分開說明。
    $removed = 0; $machine = 0
    foreach ($s in $stores) {
        foreach ($c in @(Get-ChildItem $s | Where-Object { $_.Subject -eq $subject })) {
            try { Remove-Item $c.PSPath -Force -ErrorAction Stop; $removed++ }
            catch { $machine++ }
        }
    }
    Write-Host "已從你的個人憑證存放區移除 $removed 張。"
    if ($machine -gt 0) {
        Write-Host ""
        Write-Host "另有 $machine 張是「機器層級」憑證，由 1.0.11 以前的安裝檔寫入，移除需要系統管理員權限。" -ForegroundColor Yellow
        Write-Host "解除安裝 AwayTerminal 會自動清掉，或以系統管理員身分執行："
        Write-Host '    certutil -delstore Root "AwayTerminal (awaysu)"'
        Write-Host '    certutil -delstore TrustedPublisher "AwayTerminal (awaysu)"'
    }
    return
}

$cer = Join-Path $PSScriptRoot 'AwayTerminal.cer'
if (-not (Test-Path $cer)) {
    Write-Host "找不到 AwayTerminal.cer（應與本腳本放在同一個資料夾）。" -ForegroundColor Red
    exit 1
}

# 印出憑證指紋供你自行核對。請與下載頁公布的指紋比對後再決定要不要信任：
#   https://github.com/awaysu/Download/blob/main/README.AwayTerminal.md
$c = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 $cer
Write-Host "憑證主體： $($c.Subject)"
Write-Host "指紋(SHA1)：$($c.Thumbprint)"
Write-Host "有效期限： $($c.NotBefore.ToString('yyyy-MM-dd')) ~ $($c.NotAfter.ToString('yyyy-MM-dd'))"
Write-Host ""

if ($c.Subject -ne $subject) {
    Write-Host "憑證主體與預期不符，已中止。" -ForegroundColor Red
    exit 1
}

foreach ($s in $stores) { Import-Certificate -FilePath $cer -CertStoreLocation $s | Out-Null }

Write-Host "完成。之後執行 AwayTerminal 時，UAC 會顯示「Awaysu」而非「不明的發行者」。"
Write-Host "注意：這不會消除 SmartScreen 的「Windows 已保護你的電腦」提示——那取決於下載量累積的信譽，與憑證無關。"
