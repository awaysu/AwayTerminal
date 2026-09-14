using AwayTerminal.Models;

namespace AwayTerminal.Services.MultiAgent;

/// <summary>Codex CLI：<c>-c "developer_instructions='…'"</c>（只有字串版、沒有檔案版——官方 issue #12926 not-planned；1.1.11 實跑驗證）。
/// 值先當 TOML 解析：單引號＝literal string，裡面不能再有單引號、雙引號、換行（OneLine 處理）。
/// 整份角色檔超過 <see cref="MaxInline"/> 字，或經 PowerShell 啟動（npm 版 codex.cmd，cmd.exe 命令列上限 8191）時，
/// 改帶一句「先讀角色檔」的短指引——仍是原生 developer instructions，不必等 CLI 閒置再打字（Codex 第一次進資料夾會先問要不要信任，
/// 那時打字會打進信任選單）。</summary>
internal sealed class CodexAdapter : CliAdapterBase
{
    public override string Key => "codex";
    public override string DisplayName => "Codex";
    protected override string ExeWord => "codex";

    private const int MaxInline = 8000;

    public override AgentLaunch BuildLaunch(CustomConn conn, AgentSlot slot, string roleText)
    {
        string full = OneLine(roleText);
        string text = !RunsViaPowerShell(conn) && full.Length <= MaxInline ? full : OneLine(Pointer(slot));
        // tui.whimsy=false：gpt-6-astra 閒置時輸入框背景有「星星閃爍」動畫，每秒重畫 6～7 次、約 8KB/s（probe 實錄；gpt-5.6-sol 閒置 0 byte）
        // → 畫面永遠不會靜止 2 秒，AgentReady 永遠 false、信一直卡在佇列（使用者中途 /model 換成 astra 後 PM 收不到信）。
        // 只關裝飾動畫；tui.animations=false 也有效，但可能連「Working」這類忙碌指示一起關，忙閒判斷會失準，所以不用它。
        return new($" -c tui.whimsy=false -c \"developer_instructions='{text}'\"", null);
    }
}
