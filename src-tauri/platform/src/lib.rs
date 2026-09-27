//! AwayTerminal 的**平台相依原始碼**：PTY、子行程樹、行程群組、PATH 搜尋、
//! 序列埠裝置路徑、Linux 的 WebKitGTK 環境變數。
//!
//! # 為什麼要獨立成一個 crate
//!
//! 這些程式碼是為 mac／Linux 寫的，但**開發機是 Windows**，所以「它到底編不編得過」
//! 是最容易被忽略的風險——`cargo check` 在 Windows 上完全不看 `#[cfg(unix)]` 的內容。
//!
//! 主 crate 沒辦法跨 target `check`：它依賴 tauri，而 tauri 在 Linux 會拉進
//! `glib-sys`／`gdk-sys`／`webkit2gtk-sys` 等一整排 `-sys`（build script 需要
//! `pkg-config` 與 GTK 的開發標頭檔），在 mac 會拉進 `objc2-exception-helper`
//! （build script 要用 clang 編一段 Objective-C，需要 Apple SDK）。兩者在 Windows 上
//! 都不存在，於是**第三方的 build script 先失敗**，我們自己的程式碼一行都沒被看過。
//!
//! 所以把平台相依的部分搬到這裡，**只依賴 `libc`**（純 Rust 綁定，build script 不需要
//! 任何外部東西），就真的可以：
//!
//! ```text
//! cargo check  -p awayterm-platform --target x86_64-unknown-linux-gnu
//! cargo check  -p awayterm-platform --target aarch64-apple-darwin
//! cargo check  -p awayterm-platform --target x86_64-apple-darwin
//! cargo clippy -p awayterm-platform --target <同上>  -- -D warnings
//! ```
//!
//! 主 crate 那一側的 `#[cfg(unix)]` 只剩很薄的接線（把這裡的型別包成
//! `TerminalSession`），風險小得多。
//!
//! # 什麼該放進來
//!
//! | 放 | 不放 |
//! |---|---|
//! | 系統呼叫、`/proc` 與 `libproc`、裝置路徑規則、PATH 搜尋順序 | 任何 tauri 型別、i18n、設定檔 |
//! | 可以在任何平台跑的純邏輯（`linuxenv` 的判斷、路徑比對） | 需要視窗／webview 的東西 |
//!
//! # ⚠️ 全部標「待真機驗證」
//!
//! 這個 crate 目前**只被編譯器看過**，沒有在真的 mac／Linux 上跑過。
//! 逐項清單在 `docs/PLATFORM-UNIX.md`。

pub mod cmdline;
pub mod linuxenv;
pub mod proctree;
pub mod serial;
pub mod uptime;
pub mod which;

#[cfg(unix)]
pub mod pgroup;
#[cfg(unix)]
pub mod pty;

/// 這個 crate 是為哪個平台編的（診斷字串）。
pub fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(windows) {
        "windows"
    } else {
        "unix"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn platform_name_is_known() {
        assert!(!super::platform_name().is_empty());
    }
}
