using AwayTerminal.Models;

namespace AwayTerminal.Services.MultiAgent;

/// <summary>
/// 一種 Coding Agent CLI（ClaudeCode／Codex／OpenCode／GeminiCLI）的差異全部關在這裡（1.2.0）。
/// 上層（MainWindow.MultiAgent）只做兩件會碰到 CLI 的事：<b>啟動它</b>（自訂連線＋額外參數，走既有 OpenCustom）、
/// <b>往它的終端機打一行字</b>（既有 SendTextThenEnter）；所以介面刻意很小。
/// </summary>
internal interface ICodingAgentAdapter
{
    /// <summary>＝圖示 key：claude-code / codex / opencode / geminicli。</summary>
    string Key { get; }

    string DisplayName { get; }

    /// <summary>要啟動哪一支：優先沿用使用者「自訂連線」清單裡同圖示（或執行檔名）的那筆（路徑／參數／PowerShell／關閉鍵），
    /// 沒有才自動偵測（CustomConnDialog.ResolveTool＋KnownTools 的預設參數）。找不到＝null（設定視窗不列）。</summary>
    CustomConn? Resolve();

    /// <summary>組這個 agent 的啟動方式：附加參數（只用在這次啟動，不存進恢復資訊）＋要不要在 CLI 第一次閒置時打「請先讀角色檔」。</summary>
    AgentLaunch BuildLaunch(CustomConn conn, AgentSlot slot, string roleText);
}

/// <param name="ExtraArgs">附加在自訂連線參數後面（前面自帶空白）。</param>
/// <param name="FirstMessage">非 null＝CLI 沒有可靠的「每個 session 系統提示」管道，第一次閒置時由 AwayTerminal 打這一句（保底注入）。</param>
internal sealed record AgentLaunch(string ExtraArgs, string? FirstMessage);
