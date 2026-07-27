using System.Windows;

namespace AwayTerminal;

/// <summary>
/// Interaction logic for App.xaml
/// </summary>
public partial class App : Application
{
    protected override void OnStartup(StartupEventArgs e)
    {
        // 清掉會抑制子行程彩色輸出/干擾行為的繼承環境變數
        // （例如從 Claude Code 或 CI 環境啟動時會帶進 NO_COLOR=1，導致 claude 等工具全部無色）
        foreach (var name in new[]
        {
            "NO_COLOR", "CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_ENTRYPOINT",
            "CLAUDE_CODE_SESSION_ID", "CLAUDE_PID", "GIT_TERMINAL_PROMPT",
        })
        {
            Environment.SetEnvironmentVariable(name, null);
        }

        // 告知子行程本終端機支援 256 色 / 全彩（xterm.js 前端）
        Environment.SetEnvironmentVariable("TERM", "xterm-256color");
        Environment.SetEnvironmentVariable("COLORTERM", "truecolor");

        // Claude Code 2.1.x 起 TUI 改用 alternate screen（全螢幕渲染），
        // 終端機原生 scrollback 完全失效——長回覆只看得到最後一頁。
        // 官方 opt-out（v2.1.132+）：改回經典渲染器，回覆留在 xterm.js 的 50000 行 scrollback。
        Environment.SetEnvironmentVariable("CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN", "1");

        base.OnStartup(e);
    }
}
