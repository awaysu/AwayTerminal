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
}

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
