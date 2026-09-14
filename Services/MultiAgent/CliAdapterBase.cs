using System.IO;
using AwayTerminal.Dialogs;
using AwayTerminal.Models;

namespace AwayTerminal.Services.MultiAgent;

/// <summary>四家共用：找執行檔、角色指引句。</summary>
internal abstract class CliAdapterBase : ICodingAgentAdapter
{
    public abstract string Key { get; }
    public abstract string DisplayName { get; }

    /// <summary>執行檔名含這個字就算同一種（使用者換過圖示或手動新增的自訂連線）。</summary>
    protected abstract string ExeWord { get; }

    public abstract AgentLaunch BuildLaunch(CustomConn conn, AgentSlot slot, string roleText);

    private static readonly string[] AllKeys = { "claude-code", "codex", "opencode", "geminicli" };

    public CustomConn? Resolve()
    {
        // 1) 自訂連線：同圖示 key 優先；圖示不是四家之一的，再看執行檔名（沒隱藏的優先）
        var conns = AppSettings.Current.CustomConns
            .Where(c => !string.IsNullOrWhiteSpace(c.Path))
            .OrderBy(c => c.Hidden).ToList();
        var hit = conns.FirstOrDefault(c => string.Equals(c.Icon, Key, StringComparison.OrdinalIgnoreCase) && Usable(c))
               ?? conns.FirstOrDefault(c => !AllKeys.Contains(c.Icon, StringComparer.OrdinalIgnoreCase) && ExeMatches(c.Path) && Usable(c));
        if (hit != null)
            return new CustomConn
            {
                Name = hit.Name, Path = hit.Path, Args = hit.Args, Icon = Key,
                CloseKey = hit.CloseKey, CloseCount = hit.CloseCount, ViaPowerShell = hit.ViaPowerShell, PickDir = false
            };

        // 2) 自動偵測（與「自訂… → 自動偵測」同一張表：預設參數也一樣，例 ClaudeCode 的 --dangerously-skip-permissions）
        var tool = CustomConnDialog.KnownTools.FirstOrDefault(t => string.Equals(t.Icon, Key, StringComparison.OrdinalIgnoreCase));
        if (tool == null) return null;
        string path = CustomConnDialog.ResolveTool(tool.ExeNames);
        if (string.IsNullOrEmpty(path)) return null;
        return new CustomConn
        {
            Name = tool.Name, Path = path, Args = tool.Args, Icon = Key,
            ViaPowerShell = path.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase) || path.EndsWith(".bat", StringComparison.OrdinalIgnoreCase)
        };
    }

    private static bool Usable(CustomConn c) => c.ViaPowerShell || File.Exists(c.Path);

    private bool ExeMatches(string path)
    {
        try { return Path.GetFileNameWithoutExtension(path).Contains(ExeWord, StringComparison.OrdinalIgnoreCase); }
        catch { return false; }
    }

    protected static bool RunsViaPowerShell(CustomConn conn) =>
        conn.ViaPowerShell
        || conn.Path.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase)
        || conn.Path.EndsWith(".bat", StringComparison.OrdinalIgnoreCase);

    /// <summary>角色指引（英文、單行、不含引號與 PowerShell／cmd 特殊字元——要能安全地放進命令列，也要能直接打進 TUI）。</summary>
    protected static string Pointer(AgentSlot slot) =>
        $"You are {slot.AgentId} ({slot.RoleTitle}) in an AwayTerminal Multi-Agent team. " +
        $"Before doing anything else, read the file {slot.RoleFile} completely. " +
        "It defines your role, your teammates and how to send and receive messages. Follow it for the whole session.";

    /// <summary>多行文字壓成一行（TOML literal string 不能換行、不能含單引號；命令列不能含雙引號）。</summary>
    protected static string OneLine(string s) =>
        s.Replace("\r", "").Replace("\n", " ").Replace("'", "’").Replace("\"", "”").Trim();
}
