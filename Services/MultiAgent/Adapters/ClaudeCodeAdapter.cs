using AwayTerminal.Models;

namespace AwayTerminal.Services.MultiAgent;

/// <summary>Claude Code：<c>--append-system-prompt-file</c>（附加在預設系統提示後；1.1.11 協作版實跑驗證）。</summary>
internal sealed class ClaudeCodeAdapter : CliAdapterBase
{
    public override string Key => "claude-code";
    public override string DisplayName => "ClaudeCode";
    protected override string ExeWord => "claude";

    public override AgentLaunch BuildLaunch(CustomConn conn, AgentSlot slot, string roleText)
        => new($" --append-system-prompt-file \"{slot.RoleFile}\"", null);
}
