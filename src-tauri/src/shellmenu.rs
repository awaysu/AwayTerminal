//! 檔案總管右鍵選單「用 AwayTerminal 開啟」（搬移舊版 `Services/ShellIntegration.cs`）。
//!
//! ## 登錄檔（**只碰 `HKCU`**，免管理員）
//!
//! | key | 值 |
//! |---|---|
//! | `HKCU\Software\Classes\Directory\shell\AwayTerminal` | `(Default)` ＝選單文字、`Icon` ＝ `"<exe>",0` |
//! | `HKCU\Software\Classes\Directory\shell\AwayTerminal\command` | `(Default)` ＝ `"<exe>" --open-dir "%V"` |
//! | `HKCU\Software\Classes\Directory\Background\shell\AwayTerminal` | 同上（在資料夾**內空白處**按右鍵） |
//! | `HKCU\Software\Classes\Directory\Background\shell\AwayTerminal\command` | 同上 |
//!
//! 逐項照舊版：兩個位置、`%V`（不是 `%1`——`%V` 在「資料夾上」與「資料夾內空白處」
//! 兩種情形都給得出路徑，VS Code 也是這樣寫）、`Icon` 指向 exe 的第 0 個圖示、
//! 參數名 **`--open-dir`**（不是 `--dir`）、每次啟動依設定重新套用（**路徑永遠指向目前這支 exe**，
//! 所以搬家／升級之後選單不會指到舊位置）、文字跟著語言。
//!
//! ⚠️ **不碰 `HKLM`**，也不碰別人的 key；移除就是把我們這兩個子樹刪掉。
//! MSIX 版的登錄檔會被虛擬化、檔案總管看不到（要 COM 擴充）——舊版就有這個限制，照記。

#![cfg(windows)]

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

/// 我們的 key 名稱（兩個位置都用這個）。
///
/// ⚠️ **測試絕對不能用這個名稱**：使用者可能（很可能）已經有這個 key ——
/// 舊版 v1.2.8 每次啟動都會重新登錄它。我在 TASK-016 的第一版測試就是直接
/// register/unregister 真的 key，結果把使用者的選單指到**測試執行檔**上
///（`awayterminal_lib-<hash>.exe`）。測試改用 [`key_name`] 回的測試專用名稱。
#[cfg_attr(test, allow(dead_code))] // 測試走 TEST_KEY_NAME，這個常數在 test build 下沒人用
const KEY_NAME: &str = "AwayTerminal";


/// 測試與 `--verify` 專用的 key 名稱（不會和使用者的撞）。
pub const SANDBOX_KEY_NAME: &str = "AwayTerminal_UnitTest";

/// 這次要用哪個 key 名稱。
///
/// `sandbox = true`＝用**測試專用**的名稱：單元測試與 `--verify` 都走這個，
/// 所以永遠不會動到使用者真的那個右鍵選單
///（我在 TASK-016 踩過兩次：第一次單元測試把它指到測試執行檔，
///  第二次 `--verify` 的「復原」用重新登錄實作 → 指到 dev 的 exe）。
fn key_name(sandbox: bool) -> &'static str {
    if sandbox || cfg!(test) {
        SANDBOX_KEY_NAME
    } else {
        KEY_NAME
    }
}

/// 兩個位置（同舊版 `Roots`）。
const ROOTS: &[&str] = &[
    r"Software\Classes\Directory\shell",
    r"Software\Classes\Directory\Background\shell",
];

/// 目前這支執行檔的真實路徑。
///
/// PM 特別提到「不是 Store 別名那類」——`current_exe()` 回的就是真實路徑
///（Store 別名是**啟動**時的替身，行程自己看到的是真檔案）。
pub fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

/// 右鍵選單的 `command` 值。
pub fn command_line(exe: &str) -> String {
    format!("\"{exe}\" --open-dir \"%V\"")
}

/// 依設定登錄或移除。**失敗只回 `Err`，呼叫端記 log 就好**（不該影響啟動，同舊版）。
pub fn apply(enable: bool, menu_text: &str, sandbox: bool) -> Result<(), String> {
    if enable {
        register(menu_text, sandbox)
    } else {
        unregister(sandbox)
    }
}

/// 寫入兩個位置。
pub fn register(menu_text: &str, sandbox: bool) -> Result<(), String> {
    let exe = exe_path();
    if exe.is_empty() {
        return Err(crate::i18n::t("err.noExePath"));
    }
    let command = command_line(&exe);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for root in ROOTS {
        let path = format!(r"{root}\{}", key_name(sandbox));
        let (key, _) = hkcu
            .create_subkey(&path)
            .map_err(|e| crate::i18n::tf("err.registryWrite", &[&path, &e.to_string()]))?;
        key.set_value("", &menu_text.to_string())
            .map_err(|e| crate::i18n::tf("err.registryWrite", &[&path, &e.to_string()]))?;
        // 圖示：exe 的第 0 個（同舊版）
        key.set_value("Icon", &format!("\"{exe}\",0"))
            .map_err(|e| crate::i18n::tf("err.registryWrite", &[&format!("{path} (Icon)"), &e.to_string()]))?;
        let (cmd, _) = key.create_subkey("command").map_err(|e| {
            crate::i18n::tf("err.registryWrite", &[&format!("{path} (command)"), &e.to_string()])
        })?;
        cmd.set_value("", &command).map_err(|e| {
            crate::i18n::tf("err.registryWrite", &[&format!("{path} (command)"), &e.to_string()])
        })?;
    }
    println!("[AwayTerminal] 檔案總管右鍵選單：已登錄（{command}）");
    Ok(())
}

/// 把我們的兩個子樹刪掉（不存在不算錯，同舊版 `throwOnMissingSubKey: false`）。
pub fn unregister(sandbox: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for root in ROOTS {
        let path = format!(r"{root}\{}", key_name(sandbox));
        match hkcu.delete_subkey_all(&path) {
            Ok(()) => println!("[AwayTerminal] 檔案總管右鍵選單：已移除 {path}"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(crate::i18n::tf("err.registryDelete", &[&path, &e.to_string()])),
        }
    }
    Ok(())
}

/// 目前登錄的狀態（給 `--verify` 與設定視窗自我檢查用）。
///
/// 回 `(有沒有登錄, command 的值)`。
pub fn current(sandbox: bool) -> (bool, String) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for root in ROOTS {
        let path = format!(r"{root}\{}\command", key_name(sandbox));
        if let Ok(k) = hkcu.open_subkey_with_flags(&path, KEY_READ) {
            if let Ok(v) = k.get_value::<String, _>("") {
                return (true, v);
            }
        }
    }
    (false, String::new())
}

// ------------------------------------------------------------------ commands

/// 設定視窗的勾選：勾＝寫入、取消＝刪除。回傳套用後的狀態。
/// `sandbox = true` 只給 `--verify` 用（走測試專用的 key，碰不到使用者的選單）。
#[tauri::command]
pub fn shell_menu_apply(
    enable: bool,
    text: String,
    sandbox: Option<bool>,
) -> Result<serde_json::Value, String> {
    let sb = sandbox.unwrap_or(false);
    apply(enable, &text, sb)?;
    let (on, command) = current(sb);
    Ok(serde_json::json!({ "enabled": on, "command": command, "sandbox": sb }))
}

/// 目前狀態（設定視窗開啟時讀一次）。
#[tauri::command]
pub fn shell_menu_state(sandbox: Option<bool>) -> serde_json::Value {
    let sb = sandbox.unwrap_or(false);
    let (on, command) = current(sb);
    serde_json::json!({ "enabled": on, "command": command, "exe": exe_path(), "sandbox": sb })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `command` 的格式逐字照舊版（`%V`、引號、`--open-dir`）。
    #[test]
    fn command_matches_v1() {
        assert_eq!(
            command_line("C:\\Program Files\\AwayTerminal\\AwayTerminal.exe"),
            "\"C:\\Program Files\\AwayTerminal\\AwayTerminal.exe\" --open-dir \"%V\""
        );
    }

    /// 寫入 → 讀回 → 刪除，**全程只在 `HKCU`**，而且刪完要查不到。
    ///
    /// ⚠️ 這個測試會真的寫 `HKCU`，但用的是**測試專用的 key 名稱**
    /// （`AwayTerminal_UnitTest`，見 `key_name()`），所以碰不到使用者那個真的選單。
    #[test]
    fn registers_and_unregisters_in_hkcu_only() {
        register("測試用（AwayTerminal 單元測試）", true).expect("登錄應該成功");
        let (on, command) = current(true);
        assert!(on, "登錄之後應該查得到");
        assert!(command.contains("--open-dir"), "command 不對：{command}");
        assert!(command.contains("%V"), "command 少了 %V：{command}");

        // 兩個位置都要有
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for root in ROOTS {
            let path = format!(r"{root}\{}", key_name(true));
            assert!(
                hkcu.open_subkey_with_flags(&path, KEY_READ).is_ok(),
                "{path} 應該存在"
            );
        }

        unregister(true).expect("移除應該成功");
        assert!(!current(true).0, "移除之後不該查得到");
        for root in ROOTS {
            let path = format!(r"{root}\{}", key_name(true));
            assert!(
                hkcu.open_subkey_with_flags(&path, KEY_READ).is_err(),
                "{path} 應該被刪掉"
            );
        }
        // 這個測試用的是測試專用 key，使用者的選單完全沒碰到
    }
}
