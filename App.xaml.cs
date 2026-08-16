using System.Runtime.InteropServices;
using System.Windows;

namespace AwayTerminal;

/// <summary>
/// Interaction logic for App.xaml
/// </summary>
public partial class App : Application
{
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetStdHandle(int nStdHandle, IntPtr hHandle);

    protected override void OnStartup(StartupEventArgs e)
    {
        // 從「有重導 stdout/stderr 的環境」（腳本、CI、開發工具）啟動時，本程式會繼承對方的
        // pipe std handle；CreateProcess 對 console 子行程會把這些 handle「值」原樣帶過去，
        // ConPTY 連接只在 std handle 為空時才換成 console handle → powershell/claude 一寫
        // stdout 就打到無效 handle、0.5 秒內死掉（分頁只剩游標）。GUI 程式用不到 std handle，
        // 一律歸零，讓每個 ConPTY 子行程都拿到正確的 console handle。
        SetStdHandle(-10, IntPtr.Zero);  // STD_INPUT_HANDLE
        SetStdHandle(-11, IntPtr.Zero);  // STD_OUTPUT_HANDLE
        SetStdHandle(-12, IntPtr.Zero);  // STD_ERROR_HANDLE

        // 清掉會抑制子行程彩色輸出/干擾行為的繼承環境變數
        // （例如從 Claude Code 或 CI 環境啟動時會帶進 NO_COLOR=1，導致 claude 等工具全部無色）。
        // CLAUDE*/ANTHROPIC* 用字首掃掉——清單式列舉追不上 claude 新增變數的速度
        // （2026-08 已出現 CLAUDE_CODE_EXECPATH、CLAUDE_EFFORT，殘留會影響巢狀 claude 判斷）。
        foreach (var name in new[] { "NO_COLOR", "GIT_TERMINAL_PROMPT" })
        {
            Environment.SetEnvironmentVariable(name, null);
        }
        foreach (System.Collections.DictionaryEntry kv in Environment.GetEnvironmentVariables())
        {
            var k = (string)kv.Key;
            if (k.StartsWith("CLAUDE", StringComparison.OrdinalIgnoreCase) ||
                k.StartsWith("ANTHROPIC", StringComparison.OrdinalIgnoreCase))
                Environment.SetEnvironmentVariable(k, null);
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
