//! Linux 的 WebKitGTK 啟動環境（`CLAUDE.md` 風險 2 的解法 b）。
//!
//! WebKitGTK 有兩個「不設就會壞」的環境變數，而且症狀都是**整片白畫面**，
//! 使用者完全看不出原因：
//!
//! | 變數 | 為什麼 |
//! |---|---|
//! | `WEBKIT_DISABLE_DMABUF_RENDERER=1` | NVIDIA 的專有驅動 ＋ WebKitGTK 2.4x 的 DMA-BUF renderer 會給出白畫面。這是 Linux 上最常見的 Tauri／WebKitGTK 問題 |
//! | `WEBKIT_DISABLE_COMPOSITING_MODE=1` | 舊版 WebKitGTK 與部分 Intel／虛擬機的驅動下，合成模式會讓畫面不更新或閃爍 |
//!
//! 還有一個**不設值、只提示**的：
//!
//! | 變數 | 為什麼只提示 |
//! |---|---|
//! | `GTK_IM_MODULE` | 沒設的話中文輸入法（fcitx5／ibus）可能完全不能用。但值要看使用者裝的是哪一套，**猜錯會把本來好的弄壞**，所以只在畫面上提示一次，不替使用者決定 |
//!
//! # 原則：只在「使用者沒設」時才設
//!
//! 使用者自己 export 過就照他的（他可能正是為了測試才設成 0）。這裡的函式都是
//! **純邏輯**（吃現在的值、回要不要設），所以在 Windows 上也編得過也測得到——
//! 真正呼叫 `std::env::set_var` 的地方在主 crate 的啟動流程，而且只在 `cfg(linux)`。
//!
//! # ⚠️ 一定要在建立 webview 之前設
//!
//! WebKitGTK 在第一次初始化時才讀這些變數，晚了就沒有作用。所以呼叫點在
//! `run()` 的最前面，不是 `.setup()` 裡面（`.setup()` 已經在視窗建好之後）。

/// 要設成 `1` 的變數（只有在目前沒設、或設成空字串時才設）。
pub const FORCE_ONE: &[&str] = &[
    "WEBKIT_DISABLE_DMABUF_RENDERER",
    "WEBKIT_DISABLE_COMPOSITING_MODE",
];

/// 一項要不要動的決定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// 設成這個值。
    Set(&'static str, &'static str),
    /// 使用者已經設了，不動（附目前的值，給診斷行用）。
    Keep(&'static str, String),
}

/// 算出這一次啟動要對 [`FORCE_ONE`] 做什麼。
///
/// `lookup` 是「讀環境變數」的函式（測試時餵假的，正式時餵 `std::env::var`）。
pub fn plan<F: Fn(&str) -> Option<String>>(lookup: F) -> Vec<Action> {
    FORCE_ONE
        .iter()
        .map(|&k| match lookup(k) {
            Some(v) if !v.trim().is_empty() => Action::Keep(k, v),
            _ => Action::Set(k, "1"),
        })
        .collect()
}

/// 要不要提示使用者設定 `GTK_IM_MODULE`。
///
/// 已經設了就不提示。**不猜值**：裝 fcitx5 的人要 `fcitx`、裝 ibus 的要 `ibus`，
/// 猜錯會把本來好的輸入法弄壞。
pub fn should_hint_im_module<F: Fn(&str) -> Option<String>>(lookup: F) -> bool {
    match lookup("GTK_IM_MODULE") {
        Some(v) => v.trim().is_empty(),
        None => true,
    }
}

/// 目前的桌面看起來是 Wayland 嗎（診斷行與文件用；Wayland 下不能截別人的視窗，
/// 見 `src/telegram/shot.rs`）。
pub fn is_wayland<F: Fn(&str) -> Option<String>>(lookup: F) -> bool {
    if let Some(t) = lookup("XDG_SESSION_TYPE") {
        if t.eq_ignore_ascii_case("wayland") {
            return true;
        }
    }
    lookup("WAYLAND_DISPLAY").is_some_and(|v| !v.trim().is_empty())
}

/// 啟動時要印的那一行（診斷用，不是介面文字，所以不進 i18n）。
pub fn describe(actions: &[Action]) -> String {
    let mut parts = Vec::new();
    for a in actions {
        match a {
            Action::Set(k, v) => parts.push(format!("{k}={v}（本程式設的）")),
            Action::Keep(k, v) => parts.push(format!("{k}={v}（使用者已設，不動）")),
        }
    }
    parts.join("　")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    /// 都沒設 → 兩個都設成 1。
    #[test]
    fn sets_both_when_unset() {
        let p = plan(none);
        assert_eq!(
            p,
            vec![
                Action::Set("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
                Action::Set("WEBKIT_DISABLE_COMPOSITING_MODE", "1"),
            ]
        );
    }

    /// 使用者設過（就算設成 0）就不動——他可能正是為了測試才設的。
    #[test]
    fn keeps_what_the_user_set() {
        let p = plan(|k| {
            if k == "WEBKIT_DISABLE_DMABUF_RENDERER" {
                Some("0".to_string())
            } else {
                None
            }
        });
        assert_eq!(p[0], Action::Keep("WEBKIT_DISABLE_DMABUF_RENDERER", "0".to_string()));
        assert_eq!(p[1], Action::Set("WEBKIT_DISABLE_COMPOSITING_MODE", "1"));
    }

    /// 設成空字串當成沒設（`export FOO=` 的情況）。
    #[test]
    fn an_empty_value_counts_as_unset() {
        let p = plan(|_| Some("  ".to_string()));
        assert!(p.iter().all(|a| matches!(a, Action::Set(..))));
    }

    #[test]
    fn im_module_hint_only_when_missing() {
        assert!(should_hint_im_module(none));
        assert!(should_hint_im_module(|_| Some(String::new())));
        assert!(!should_hint_im_module(|_| Some("fcitx".to_string())));
    }

    #[test]
    fn detects_wayland_from_either_variable() {
        assert!(is_wayland(|k| (k == "XDG_SESSION_TYPE").then(|| "wayland".to_string())));
        assert!(is_wayland(|k| (k == "XDG_SESSION_TYPE").then(|| "Wayland".to_string())));
        assert!(is_wayland(|k| (k == "WAYLAND_DISPLAY").then(|| "wayland-0".to_string())));
        assert!(!is_wayland(|k| (k == "XDG_SESSION_TYPE").then(|| "x11".to_string())));
        assert!(!is_wayland(none));
    }

    /// 診斷行要分得出「我們設的」與「使用者設的」。
    #[test]
    fn describe_says_who_set_it() {
        let s = describe(&plan(none));
        assert!(s.contains("本程式設的"));
        let s2 = describe(&plan(|_| Some("0".to_string())));
        assert!(s2.contains("使用者已設"));
    }
}
