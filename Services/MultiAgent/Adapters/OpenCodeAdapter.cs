using AwayTerminal.Models;

namespace AwayTerminal.Services.MultiAgent;

/// <summary>OpenCode：沒有「附加系統提示」的每 session 參數（自訂 agent 的 prompt 會不會取代內建提示未查證）→ 使用者選的保底：
/// CLI 第一次閒置時由 AwayTerminal 打「請先讀角色檔、讀完回覆 READY」。
/// <para>代理團隊裡一律帶 <c>--auto</c>（使用者要求，2026-09-15；opencode 1.18.31 起有：沒被明確禁止的權限自動核准）：
/// 團隊裡的 agent 一跳權限詢問就停在那等人按，後面的信也送不出去。連線參數裡已經有 --auto 就不重複加。一般 OpenCode 分頁照連線設定。</para></summary>
internal sealed class OpenCodeAdapter : CliAdapterBase
{
    public override string Key => "opencode";
    public override string DisplayName => "OpenCode";
    protected override string ExeWord => "opencode";

    public override AgentLaunch BuildLaunch(CustomConn conn, AgentSlot slot, string roleText)
    {
        bool hasAuto = System.Text.RegularExpressions.Regex.IsMatch(conn.Args ?? "", @"(^|\s)--auto(\s|$)");
        return new(hasAuto ? "" : " --auto", Pointer(slot) + " After reading it, reply only with READY.");
    }
}
