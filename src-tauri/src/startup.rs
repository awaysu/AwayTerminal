//! 啟動時的環境整備。整段照搬舊版 `App.xaml.cs OnStartup`（見舊版 CLAUDE.md「踩雷紀錄」）。
//!
//! 必須在**建立任何執行緒 / 任何 ConPTY 子行程之前**呼叫一次：
//! `set_var` / `remove_var` 在多執行緒下不安全，所以要趕在任何執行緒起來之前做完。

/// 做兩件事：清掉會干擾子行程的繼承環境變數、設定終端機能力變數。
///
/// 舊版在這裡還會一次性歸零 std handle；新版把那件事移到真正需要的地方
/// （`pty::conpty::with_null_std_handles`，只包住 `CreateProcess` 那一瞬間），
/// 這樣不會弄壞自己的 stdout。原因與踩雷細節寫在那個函式的註解。
pub fn prepare_process_environment() {
    clean_inherited_env();
    set_terminal_env();
    prepare_webkit_env();
}

/// Linux 的 WebKitGTK 環境變數（`CLAUDE.md` 風險 2 的解法 b）。
///
/// 兩個「不設就會壞」的變數，症狀都是**整片白畫面**、使用者完全看不出原因：
///
/// | 變數 | 為什麼 |
/// |---|---|
/// | `WEBKIT_DISABLE_DMABUF_RENDERER` | NVIDIA 專有驅動 ＋ WebKitGTK 2.4x 的 DMA-BUF renderer ＝ 白畫面。Linux 上最常見的 Tauri 問題 |
/// | `WEBKIT_DISABLE_COMPOSITING_MODE` | 舊 WebKitGTK ／部分 Intel、虛擬機驅動下合成模式會不更新或閃爍 |
///
/// **只在使用者沒設時才設**（他可能正是為了測試才設成 0），判斷邏輯在
/// `awayterm_platform::linuxenv`（純函式，在 Windows 上也測得到）。
///
/// ⚠️ 一定要在**建立 webview 之前**——WebKitGTK 只在第一次初始化時讀這些變數。
/// 這個函式在 `run()` 的第一行被呼叫，比 `.setup()` 早得多。
///
/// `GTK_IM_MODULE` **刻意不設**，只在畫面上提示一次：值要看使用者裝 fcitx5 還是 ibus，
/// 猜錯會把本來好的輸入法弄壞。
#[cfg(target_os = "linux")]
fn prepare_webkit_env() {
    use awayterm_platform::linuxenv::{self, Action};
    let plan = linuxenv::plan(|k| std::env::var(k).ok());
    for a in &plan {
        if let Action::Set(k, v) = a {
            std::env::set_var(k, v);
        }
    }
    println!("[AwayTerminal] WebKitGTK 環境：{}", linuxenv::describe(&plan));
    if linuxenv::is_wayland(|k| std::env::var(k).ok()) {
        println!("[AwayTerminal] 桌面是 Wayland（截圖不能抓別的視窗，見 docs/TELEGRAM.md）");
    }
    if linuxenv::should_hint_im_module(|k| std::env::var(k).ok()) {
        // 前端啟動後會看這個旗標決定要不要提示一次（八語的 `warn.gtkImModule`）
        std::env::set_var("AWAYTERM_HINT_GTK_IM_MODULE", "1");
        println!("[AwayTerminal] GTK_IM_MODULE 沒設——中文輸入法可能不能用，會在畫面上提示");
    }
}

/// 其他平台沒有這件事。
#[cfg(not(target_os = "linux"))]
fn prepare_webkit_env() {}

/// 清掉會抑制子行程彩色輸出 / 干擾行為的繼承環境變數。
///
/// 舊版踩雷：從 Claude Code 或 CI 環境啟動時會帶進 `NO_COLOR=1`，導致 claude 等工具全無色。
/// `CLAUDE*` / `ANTHROPIC*` 用**字首**掃掉——清單式列舉追不上 claude 新增變數的速度
/// （2026-08 已出現 `CLAUDE_CODE_EXECPATH`、`CLAUDE_EFFORT`，殘留會影響巢狀 claude 判斷）。
fn clean_inherited_env() {
    for name in ["NO_COLOR", "GIT_TERMINAL_PROMPT"] {
        std::env::remove_var(name);
    }
    let doomed: Vec<String> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| {
            let u = k.to_ascii_uppercase();
            u.starts_with("CLAUDE") || u.starts_with("ANTHROPIC")
        })
        .collect();
    for k in doomed {
        std::env::remove_var(k);
    }
}

/// 告知子行程本終端機的能力，並關掉 Claude Code 的 alternate screen。
fn set_terminal_env() {
    // xterm.js 前端支援 256 色 / 全彩
    std::env::set_var("TERM", "xterm-256color");
    std::env::set_var("COLORTERM", "truecolor");

    // Claude Code 2.1.x 起 TUI 改用 alternate screen（全螢幕渲染），終端機原生 scrollback
    // 完全失效——長回覆只看得到最後一頁。這是官方 opt-out（v2.1.132+），改回經典渲染器，
    // 讓回覆留在 xterm.js 的 scrollback 裡。
    std::env::set_var("CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN", "1");
}
