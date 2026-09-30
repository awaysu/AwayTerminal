; AwayTerminal 的 NSIS hook（`tauri.conf.json` → bundle.windows.nsis.installerHooks）
;
; 只做一件事：**解除安裝時把「用 AwayTerminal 開啟」的兩個 HKCU key 刪掉。**
;
; 為什麼是在解除安裝、而不是安裝時寫：
;   舊版（1.2.8）的 Inno 安裝檔也是這樣——安裝時**不寫**選單（`installer.iss` 的
;   `[Files]`／`[Icons]` 都沒有），選單是在設定視窗裡由使用者自己勾（`shellmenu.rs`
;   寫 HKCU），`[UninstallRun]` 負責在移除程式時把它刪掉，不然選單會指向一個已經
;   被刪掉的 exe。安裝時硬寫會有兩個問題：
;     1. 和設定視窗那個勾選打對台（使用者取消勾選，下次升級又被寫回來）。
;     2. HKCU 是「目前使用者」，但 per-machine 安裝跑在提權的環境下，寫進去的可能是
;        另一個帳號的 HKCU。
;
; key 名稱必須和 `src-tauri/src/shellmenu.rs` 的 `key_name(false)` 一致。
; 缺 key 時 `DeleteRegKey` 不會失敗（NSIS 只是不做事），所以乾淨安裝也沒問題。
;
; ⚠️ 測試專用的 key（`AwayTerminal_UnitTest`）**刻意不刪**：那是 `--verify` 自己收尾的
; 東西，解除安裝程式不該去碰使用者機器上任何不是自己寫的東西。
;
; ⚠️ 已知限制（2026-09-30 稽核 I8）：上面第 2 點同樣適用在**這裡**。perMachine 的解除安裝
; 跑在提權環境，下面的 HKCU 是**提權那個帳號**的：一般使用者在 UAC 輸入另一個管理員帳密時，
; 刪到的是管理員的 HKCU，使用者自己的選單會留著（指向已刪除的 exe）；其他帳號的也刪不到。
; 使用者本身是管理員（最常見）時沒問題。改成 `HKU\<SID>` 要取得「非提權使用者」的 SID
; 並處理 hive 沒載入的情況，寫錯會刪到別人的登錄檔 → 暫不做，說明在 docs/RELEASE.md §2。

; ---------------------------------------------------------------------------
; 安裝前：**先移除 AwayTerminal 1.x**（2026-10-01 使用者定案）
;
; 1.x 是 Inno Setup 裝的，和 2.0 同一個目錄（Program Files\AwayTerminal）。不先移除的話，
; 2.0 的 exe 會直接蓋掉 1.x 的 exe、資料夾裡留一堆 .NET 檔案，「新增或移除程式」還會
; 留著 1.x 的項目——之後有人移除 1.x，會連 2.0 的 exe 一起刪掉。
;
; 1.x 的 AppId 固定是 {A8F5C3B1-9D2E-4F6A-B7C8-1234567890AB}（installer.iss），Inno 把
; 解除安裝資訊寫在 HKLM\...\Uninstall\<AppId>_is1（64 位元檢視；保險起見 32 位元也找）。
; 設定檔在 %LOCALAPPDATA%\AwayTerminal、log 在「我的文件」，1.x 的解除安裝**不會刪**，
; 所以 2.0 第一次啟動照樣能匯入舊設定。
;
; Inno 的 unins000.exe 會把自己複製到 %TEMP% 再跑，原本那支馬上結束——ExecWait 等不到
; 真正做完。所以另外等「unins000.exe 被刪掉」（它是最後才刪的），最多 60 秒。
!macro NSIS_HOOK_PREINSTALL
  ; 暫存器只用 $R6～$R9：下面的 CheckIfAppIsRunning 會用掉 $R0～$R3
  SetRegView 64
  ReadRegStr $R6 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\{A8F5C3B1-9D2E-4F6A-B7C8-1234567890AB}_is1" "UninstallString"
  ${If} $R6 == ""
    SetRegView 32
    ReadRegStr $R6 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\{A8F5C3B1-9D2E-4F6A-B7C8-1234567890AB}_is1" "UninstallString"
    SetRegView 64
  ${EndIf}
  ${If} $R6 != ""
    ; Tauri 樣板的「程式是否開著」檢查排在這個 hook **之後**；1.x 的 exe 也叫 AwayTerminal.exe，
    ; 開著的話 Inno 的靜默解除安裝會刪不掉被占用的檔案。所以移除之前先問一次（同一個 macro：
    ; 請使用者同意關閉，取消就中止安裝）。
    !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    DetailPrint "Removing AwayTerminal 1.x ($R6)..."
    ; 去掉前後的引號，拿來等檔案消失
    StrCpy $R7 $R6 1
    ${If} $R7 == '"'
      StrCpy $R7 $R6 "" 1
      StrLen $R8 $R7
      IntOp $R8 $R8 - 1
      StrCpy $R7 $R7 $R8
    ${Else}
      StrCpy $R7 $R6
    ${EndIf}
    ExecWait '$R6 /VERYSILENT /SUPPRESSMSGBOXES /NORESTART' $R8
    DetailPrint "AwayTerminal 1.x uninstaller exit code: $R8"
    StrCpy $R9 0
    ${DoWhile} ${FileExists} "$R7"
      ${If} $R9 >= 120
        DetailPrint "AwayTerminal 1.x uninstaller did not finish within 60s; continuing."
        ${Break}
      ${EndIf}
      Sleep 500
      IntOp $R9 $R9 + 1
    ${Loop}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DetailPrint "Removing the 'Open in AwayTerminal' shell menu (HKCU)..."
  DeleteRegKey HKCU "Software\Classes\Directory\shell\AwayTerminal"
  DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\AwayTerminal"
!macroend
