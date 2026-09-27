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

!macro NSIS_HOOK_POSTUNINSTALL
  DetailPrint "Removing the 'Open in AwayTerminal' shell menu (HKCU)..."
  DeleteRegKey HKCU "Software\Classes\Directory\shell\AwayTerminal"
  DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\AwayTerminal"
!macroend
