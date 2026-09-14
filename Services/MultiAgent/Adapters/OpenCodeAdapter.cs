using AwayTerminal.Models;

namespace AwayTerminal.Services.MultiAgent;

/// <summary>OpenCode：沒有「附加系統提示」的每 session 參數（自訂 agent 的 prompt 會不會取代內建提示未查證）→ 使用者選的保底：
/// CLI 第一次閒置時由 AwayTerminal 打「請先讀角色檔、讀完回覆 READY」。</summary>
internal sealed class OpenCodeAdapter : CliAdapterBase
{
    public override string Key => "opencode";
    public override string DisplayName => "OpenCode";
    protected override string ExeWord => "opencode";

    public override AgentLaunch BuildLaunch(CustomConn conn, AgentSlot slot, string roleText)
        => new("", Pointer(slot) + " After reading it, reply only with READY.");
}
