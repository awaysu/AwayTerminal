//! 序列埠的裝置路徑（`CLAUDE.md` 平台差異表的「連接埠」一列）。
//!
//! | 平台 | 裝置 | 權限 |
//! |---|---|---|
//! | Windows | `COM1`…`COM256` | 不用特別設定 |
//! | macOS | `/dev/tty.usbserial-*`、`/dev/cu.*` | 不用特別設定 |
//! | Linux | `/dev/ttyUSB*`、`/dev/ttyACM*` | **要在 `dialout` 群組**（不然 open 會 `EACCES`） |
//!
//! 列舉本身由 `serialport` crate 做（主 crate 的 `src/com/mod.rs`），這裡負責
//! **平台特有的判斷**：哪些路徑要列、`cu` 與 `tty` 差在哪、沒權限時要提示什麼。
//!
//! # mac 的 `tty.*` 與 `cu.*`
//!
//! 同一個裝置有兩個節點：`/dev/tty.xxx`（**開的時候會等 DCD**，給「打進來的」連線用）
//! 與 `/dev/cu.xxx`（call-unit，**不等**）。程式要主動連出去就得用 `cu.*`——用 `tty.*`
//! 在沒有接線或對方沒拉 DCD 時 `open()` 會直接卡住。所以 mac 上**優先列 `cu.*`**。
//!
//! 待真機驗證：實際插一條 USB 轉序列埠線，確認 `serialport` 列出來的名稱形狀與
//! [`prefer_call_unit`] 的判斷一致。

/// 這個裝置路徑看起來是序列埠嗎（列舉結果的後備過濾，`serialport` 有時會列出藍牙節點）。
pub fn looks_like_serial(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    if cfg!(target_os = "macos") {
        // 藍牙的 `/dev/tty.Bluetooth-Incoming-Port` 不是使用者要的
        if name.contains("Bluetooth") {
            return false;
        }
        return name.starts_with("cu.") || name.starts_with("tty.");
    }
    if cfg!(target_os = "linux") {
        return name.starts_with("ttyUSB")
            || name.starts_with("ttyACM")
            || name.starts_with("ttyS")
            || name.starts_with("ttyAMA");
    }
    // Windows：`COM<n>`
    let up = name.to_ascii_uppercase();
    up.starts_with("COM") && up[3..].chars().all(|c| c.is_ascii_digit()) && up.len() > 3
}

/// mac 上要用的那一個節點：`tty.xxx` → `cu.xxx`（見檔頭說明；其他平台原樣回傳）。
pub fn prefer_call_unit(path: &str) -> String {
    if !cfg!(target_os = "macos") {
        return path.to_string();
    }
    match path.rsplit_once('/') {
        Some((dir, name)) => match name.strip_prefix("tty.") {
            Some(rest) => format!("{dir}/cu.{rest}"),
            None => path.to_string(),
        },
        None => path.to_string(),
    }
}

/// 開埠失敗的原因是「權限不足」嗎（Linux 沒進 `dialout` 群組的典型情況）。
///
/// 主 crate 拿這個決定要不要在畫面上提示加入群組（提示文字是八語的
/// `err.comDialoutGroup`，在 `src-tauri/src/i18n.rs`）。
pub fn is_permission_denied(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::PermissionDenied
}

/// Linux 上要提示的指令（**只組字串，不執行**——改群組要登出才生效，而且是使用者的機器）。
pub fn dialout_hint(user: &str) -> String {
    format!("sudo usermod -aG dialout {user}")
}

/// 這個平台需要「加入群組」這種前置設定嗎。
pub fn needs_group_membership() -> bool {
    cfg!(target_os = "linux")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_this_platforms_devices() {
        if cfg!(target_os = "macos") {
            assert!(looks_like_serial("/dev/cu.usbserial-1420"));
            assert!(looks_like_serial("/dev/tty.usbmodem14201"));
            assert!(!looks_like_serial("/dev/tty.Bluetooth-Incoming-Port"));
            assert!(!looks_like_serial("/dev/random"));
        } else if cfg!(target_os = "linux") {
            assert!(looks_like_serial("/dev/ttyUSB0"));
            assert!(looks_like_serial("/dev/ttyACM1"));
            assert!(!looks_like_serial("/dev/tty.usbserial-1420"));
            assert!(!looks_like_serial("/dev/null"));
        } else {
            assert!(looks_like_serial("COM5"));
            assert!(looks_like_serial("COM256"));
            assert!(!looks_like_serial("COM"));
            assert!(!looks_like_serial("/dev/ttyUSB0"));
        }
    }

    /// mac 上 `tty.*` 要換成 `cu.*`（不然 open 會等 DCD 卡住）；其他平台不動。
    #[test]
    fn maps_tty_to_cu_on_mac_only() {
        let got = prefer_call_unit("/dev/tty.usbserial-1420");
        if cfg!(target_os = "macos") {
            assert_eq!(got, "/dev/cu.usbserial-1420");
            // 已經是 cu 的不要再換
            assert_eq!(prefer_call_unit("/dev/cu.x"), "/dev/cu.x");
        } else {
            assert_eq!(got, "/dev/tty.usbserial-1420");
        }
        // 沒有斜線的（Windows 的 COM5）原樣
        assert_eq!(prefer_call_unit("COM5"), "COM5");
    }

    #[test]
    fn permission_denied_is_detected() {
        let e = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "x");
        assert!(is_permission_denied(&e));
        let e2 = std::io::Error::new(std::io::ErrorKind::NotFound, "x");
        assert!(!is_permission_denied(&e2));
    }

    #[test]
    fn dialout_hint_names_the_user() {
        assert_eq!(dialout_hint("awaysu"), "sudo usermod -aG dialout awaysu");
        assert_eq!(needs_group_membership(), cfg!(target_os = "linux"));
    }
}
