using AwayTerminal.Models;

namespace AwayTerminal.Services.MultiAgent;

/// <summary>Gemini CLI：<c>GEMINI_SYSTEM_MD</c> 會「整份取代」內建系統提示（不適合注入角色）→ 同 OpenCode 走保底打字。</summary>
internal sealed class GeminiCliAdapter : CliAdapterBase
{
    public override string Key => "geminicli";
    public override string DisplayName => "GeminiCLI";
    protected override string ExeWord => "gemini";

    public override AgentLaunch BuildLaunch(CustomConn conn, AgentSlot slot, string roleText)
        => new("", Pointer(slot) + " After reading it, reply only with READY.");
}
