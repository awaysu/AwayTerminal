using System.IO;
using System.Reflection;
using System.Text;
using AwayTerminal.Localization;
using AwayTerminal.Models;

namespace AwayTerminal.Services.MultiAgent;

/// <summary>
/// Multi-Agent 角色檔（1.2.0）。三層組合成每個 agent 一份檔：
/// ① <c>common.md</c>（所有人）＋ ② <c>roles\&lt;角色&gt;.md</c>（依角色；None＝略過）＋ ③ 執行期脈絡（AwayTerminal 產生：Agent ID、隊友名單、信箱格式…）。
/// <para>範本內嵌在程式（csproj EmbeddedResource，LogicalName 前綴 <c>MultiAgent/</c>），第一次用到（或檔案不見）時複製到
/// <c>%LOCALAPPDATA%\AwayTerminal\multiagent\</c> 讓使用者自己改；設定視窗有「還原角色檔預設」。使用者丟進 roles\ 的 .md 就是新角色。</para>
/// <para>角色檔完全不提工具名稱（寫檔、列目錄、跑 build 每家 CLI 都有），同一份可以給 ClaudeCode／Codex／OpenCode／Gemini CLI 用。</para>
/// </summary>
internal static class RoleLibrary
{
    private const string ResPrefix = "MultiAgent/";

    public static string Root => Path.Combine(AppPaths.DataDir, "multiagent");
    public static string RolesDir => Path.Combine(Root, "roles");
    public static string CommonPath => Path.Combine(Root, "common.md");
    public static string SessionDir(int groupNumber) => Path.Combine(Root, "sessions", groupNumber.ToString());

    /// <summary>內建四個角色在下拉裡的固定順序（其餘使用者自訂的依檔名排在後面）。</summary>
    public static readonly string[] BuiltInRoles = { "product-manager", "software-engineer", "software-architect", "qa-engineer" };

    public sealed record RoleInfo(string Key, string Title);

    /// <summary>缺檔才從內嵌範本補（使用者改過的不動）。</summary>
    public static void EnsureDefaults() => WriteDefaults(overwrite: false);

    /// <summary>內建範本全部覆寫回預設（使用者自己新增的角色檔不受影響）。</summary>
    public static void RestoreDefaults() => WriteDefaults(overwrite: true);

    private static void WriteDefaults(bool overwrite)
    {
        var asm = Assembly.GetExecutingAssembly();
        foreach (var name in asm.GetManifestResourceNames())
        {
            if (!name.StartsWith(ResPrefix, StringComparison.Ordinal)) continue;
            string rel = name.Substring(ResPrefix.Length).Replace('\\', '/');
            string target = Path.Combine(Root, rel.Replace('/', Path.DirectorySeparatorChar));
            try
            {
                if (!overwrite && File.Exists(target)) continue;
                Directory.CreateDirectory(Path.GetDirectoryName(target)!);
                using var s = asm.GetManifestResourceStream(name);
                if (s == null) continue;
                using var fs = File.Create(target);
                s.CopyTo(fs);
            }
            catch (Exception ex) { Diag.Log($"ma role default {rel}: {ex.Message}"); }
        }
    }

    /// <summary>roles\*.md → (key, 標題)。內建四個在前、其餘依檔名。</summary>
    public static List<RoleInfo> ListRoles()
    {
        EnsureDefaults();
        var list = new List<RoleInfo>();
        try
        {
            var keys = Directory.EnumerateFiles(RolesDir, "*.md")
                .Select(f => Path.GetFileNameWithoutExtension(f))
                .OrderBy(k => Array.IndexOf(BuiltInRoles, k) is int i && i >= 0 ? i : 100)
                .ThenBy(k => k, StringComparer.OrdinalIgnoreCase);
            foreach (var k in keys) list.Add(new RoleInfo(k, TitleOf(k)));
        }
        catch (Exception ex) { Diag.Log("ma list roles: " + ex.Message); }
        return list;
    }

    /// <summary>角色標題＝角色檔第一個「# 」標題；沒有就把檔名轉成 Title Case（software-engineer → Software Engineer）。空 key＝None。</summary>
    public static string TitleOf(string? key)
    {
        if (string.IsNullOrWhiteSpace(key)) return "None";
        try
        {
            EnsureDefaults();   // 第一次用（範本還沒複製出來）時不能退回檔名轉換——「qa-engineer」會變「Qa Engineer」
            string path = Path.Combine(RolesDir, key + ".md");
            if (File.Exists(path))
                foreach (var line in File.ReadLines(path, Encoding.UTF8))
                {
                    string t = line.Trim();
                    if (t.StartsWith("# ")) return t.Substring(2).Trim().Replace("|", "/");
                    if (t.Length > 0) break;
                }
        }
        catch { }
        return string.Join(" ", key.Split('-', '_', ' ').Where(w => w.Length > 0)
            .Select(w => char.ToUpperInvariant(w[0]) + w.Substring(1)));
    }

    /// <summary>開組時清空這個組號的成品資料夾（上次同組號留下的舊檔）。</summary>
    public static void ClearSession(int groupNumber)
    {
        string dir = SessionDir(groupNumber);
        try
        {
            if (Directory.Exists(dir))
                foreach (var f in Directory.GetFiles(dir)) { try { File.Delete(f); } catch { } }
        }
        catch { }
    }

    /// <summary>組合一個 agent 的角色檔（UTF-8 無 BOM）→ 回傳絕對路徑（同時寫進 slot.RoleFile）。隊友名單＝組內「已啟用」的格。</summary>
    public static string Compose(AgentGroup g, AgentSlot s)
    {
        EnsureDefaults();
        var sb = new StringBuilder();
        sb.Append(ReadOrEmpty(CommonPath).TrimEnd()).Append("\n\n");
        if (!string.IsNullOrWhiteSpace(s.Role))
        {
            string rolePath = Path.Combine(RolesDir, s.Role + ".md");
            string roleText = ReadOrEmpty(rolePath).TrimEnd();
            if (roleText.Length > 0) sb.Append("---\n\n").Append(roleText).Append("\n\n");
        }
        sb.Append("---\n\n").Append(RuntimeContext(g, s));

        string dir = SessionDir(g.Number);
        Directory.CreateDirectory(dir);
        string path = Path.Combine(dir, s.AgentId + ".md");
        File.WriteAllText(path, sb.ToString(), new UTF8Encoding(false));
        s.RoleFile = path;
        return path;
    }

    private static string ReadOrEmpty(string path)
    {
        try { return File.Exists(path) ? File.ReadAllText(path, Encoding.UTF8) : ""; }
        catch { return ""; }
    }

    /// <summary>第三層：執行期脈絡（英文；投遞那一行的範例用目前語言，與實際打進終端機的一致）。</summary>
    private static string RuntimeContext(AgentGroup g, AgentSlot me)
    {
        var enabled = g.Slots.Where(x => x.Enabled).ToList();
        bool hasWorker = enabled.Any(x => !ReferenceEquals(x, me));
        int w1 = enabled.Max(x => x.AgentId.Length), w2 = enabled.Max(x => x.RoleTitle.Length);
        var sb = new StringBuilder();
        sb.Append("# Runtime Context (generated by AwayTerminal)\n\n");
        sb.Append($"Agent ID: {me.AgentId}\n");
        sb.Append($"Role: {(string.IsNullOrWhiteSpace(me.Role) ? "None (general assistant)" : me.RoleTitle)}\n");
        sb.Append($"Provider: {me.BackendName}\n");
        sb.Append($"Team session: MAS-{g.Number}\n");
        sb.Append($"Project directory: {g.Dir}\n");
        sb.Append("Enabled agents:\n");
        foreach (var x in enabled)
            sb.Append($"  - {x.AgentId.PadRight(w1)}  {x.RoleTitle.PadRight(w2)}  ({x.BackendName}){(ReferenceEquals(x, me) ? "   <- you" : "")}\n");
        if (!hasWorker) sb.Append("You are the only enabled agent (Solo Mode).\n");
        sb.Append($"Mailbox: {AgentMessage.BusRelDir}/  (relative to the project directory)\n\n");

        sb.Append("## How to send a message\n\n");
        sb.Append($"Write ONE new file {AgentMessage.BusRelDir}/NNNN-{me.AgentId}-to-<recipient id>.md where NNNN is\n");
        sb.Append("(the highest existing NNNN in that folder) + 1, zero-padded to 4 digits. Start the file with a\n");
        sb.Append("YAML front matter block, then write the body in Markdown:\n\n");
        sb.Append("```\n---\n");
        sb.Append($"from: {me.AgentId}\n");
        string sampleTo = enabled.FirstOrDefault(x => !ReferenceEquals(x, me))?.AgentId ?? me.AgentId;
        sb.Append($"to: {sampleTo}\n");
        sb.Append("type: TASK_RESULT        # TASK | TASK_RESULT | QUESTION | ANSWER | REVIEW_REQUEST | REVIEW_RESULT | BLOCKED | INFO\n");
        sb.Append("task: TASK-001\n");
        sb.Append("status: completed        # completed | failed | blocked | pass | fail  (only for results)\n");
        sb.Append("files_changed:\n  - path/to/file\n");
        sb.Append("---\n(body)\n```\n\n");
        sb.Append($"Never edit or delete an existing message file. Do not write anything else into {AgentMessage.BusRelDir}/.\n");
        sb.Append("Writing the file is the whole act of sending: after writing it, end your turn; AwayTerminal delivers it.\n\n");

        sb.Append("## How you receive messages\n\n");
        sb.Append("AwayTerminal types a line like this into your terminal:\n\n");
        string exampleFrom = enabled.FirstOrDefault(x => !ReferenceEquals(x, me))?.AgentId ?? "Agent-" + g.Number + "1";
        sb.Append("    ").Append(string.Format(Loc.T("ma.deliverOne"), 7, exampleFrom, "TASK-001", "TASK",
            $"{AgentMessage.BusRelDir}/0007-{exampleFrom}-to-{me.AgentId}.md")).Append("\n\n");
        sb.Append("Read that file, act according to your role, and reply by writing a new message file.\n");
        sb.Append($"You may read any file in {AgentMessage.BusRelDir}/ for context.\n");
        sb.Append("Only the Product Manager talks to the user. Workers reply to the Product Manager.\n");
        return sb.ToString();
    }
}
