using System.IO;
using System.Reflection;
using System.Text;
using AwayTerminal.Localization;
using AwayTerminal.Models;

namespace AwayTerminal.Services.ChatRoom;

/// <summary>
/// AI 聊天室的角色檔（1.2.3，使用者要求：和代理團隊的角色分開）。組合方式同代理團隊：
/// ① <c>common.md</c>（聊天室共同規則）＋ ② <c>roles\&lt;角色&gt;.md</c> ＋ ③ 執行期脈絡（參加者名單、討論紀錄與發言檔的路徑、發言規則）。
/// <para>範本內嵌在程式（csproj EmbeddedResource，LogicalName 前綴 <c>ChatRoom/</c>），第一次用到複製到
/// <c>%LOCALAPPDATA%\AwayTerminal\chatroom\</c>；使用者沒改過的舊版會自動換新（同 RoleLibrary 的 .defaults.json 機制）。</para>
/// </summary>
internal static class ChatRoleLibrary
{
    private const string ResPrefix = "ChatRoom/";
    private const string ManifestFile = ".defaults.json";

    public static string Root => Path.Combine(AppPaths.DataDir, "chatroom");
    public static string RolesDir => Path.Combine(Root, "roles");
    public static string CommonPath => Path.Combine(Root, "common.md");
    public static string SessionDir(int groupNumber) => Path.Combine(Root, "sessions", groupNumber.ToString());

    /// <summary>下拉裡的固定順序：主持人、反方辯論者、情報研究員在前，其餘依使用者給的清單；使用者自己加的排最後。</summary>
    public static readonly string[] BuiltInRoles =
    {
        "host", "devils-advocate", "researcher",
        "software-engineer", "embedded-engineer", "rf-engineer", "wireless-expert", "hardware-engineer",
        "network-engineer", "cloud-engineer", "security-expert", "legal-advisor", "investment-analyst",
        "marketing-expert", "health-advisor", "education-advisor", "career-advisor", "psychology-advisor",
        "travel-planner", "creative-designer", "relationship-advisor",
    };

    /// <summary>設定視窗第 1～4 位的預設角色（第 1 位＝主持人）。</summary>
    public static readonly string[] DefaultSlotRoles = { "host", "devils-advocate", "researcher", "software-engineer" };

    private static bool _synced;

    public static void EnsureDefaults() => WriteDefaults(overwrite: false);
    public static void RestoreDefaults() => WriteDefaults(overwrite: true);

    /// <summary>缺檔就補；使用者沒改過的舊版範本換新版（比對 .defaults.json 記的雜湊）。見 RoleLibrary 同名方法的說明。</summary>
    private static void WriteDefaults(bool overwrite)
    {
        bool sync = overwrite || !_synced;
        var asm = Assembly.GetExecutingAssembly();
        var manifest = sync ? LoadManifest() : null;
        bool changed = false;
        foreach (var name in asm.GetManifestResourceNames())
        {
            if (!name.StartsWith(ResPrefix, StringComparison.Ordinal)) continue;
            string rel = name.Substring(ResPrefix.Length).Replace('\\', '/');
            string target = Path.Combine(Root, rel.Replace('/', Path.DirectorySeparatorChar));
            try
            {
                bool exists = File.Exists(target);
                if (exists && !sync) continue;
                byte[] data;
                using (var s = asm.GetManifestResourceStream(name))
                {
                    if (s == null) continue;
                    using var ms = new MemoryStream();
                    s.CopyTo(ms);
                    data = ms.ToArray();
                }
                string newHash = NormalizedHash(data);
                if (exists && !overwrite)
                {
                    string cur = NormalizedHash(File.ReadAllBytes(target));
                    if (cur != newHash)
                    {
                        if (manifest == null || !manifest.TryGetValue(rel, out var written) || written != cur) continue;   // 使用者改過 → 不動
                        File.WriteAllBytes(target, data);
                        Diag.Log($"chat role default updated {rel}");
                    }
                }
                else
                {
                    Directory.CreateDirectory(Path.GetDirectoryName(target)!);
                    File.WriteAllBytes(target, data);
                }
                if (manifest != null && (!manifest.TryGetValue(rel, out var old) || old != newHash)) { manifest[rel] = newHash; changed = true; }
            }
            catch (Exception ex) { Diag.Log($"chat role default {rel}: {ex.Message}"); }
        }
        if (manifest != null && changed) SaveManifest(manifest);
        if (sync) _synced = true;
    }

    private static string NormalizedHash(byte[] bytes)
    {
        string s = Encoding.UTF8.GetString(bytes);
        if (s.Length > 0 && s[0] == '\uFEFF') s = s.Substring(1);
        s = s.Replace("\r\n", "\n");
        return Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(Encoding.UTF8.GetBytes(s))).ToLowerInvariant();
    }

    private static Dictionary<string, string>? LoadManifest()
    {
        try
        {
            string path = Path.Combine(Root, ManifestFile);
            if (!File.Exists(path)) return new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            var d = System.Text.Json.JsonSerializer.Deserialize<Dictionary<string, string>>(File.ReadAllText(path, Encoding.UTF8));
            return d == null ? null : new Dictionary<string, string>(d, StringComparer.OrdinalIgnoreCase);
        }
        catch { return null; }
    }

    private static void SaveManifest(Dictionary<string, string> manifest)
    {
        try
        {
            Directory.CreateDirectory(Root);
            File.WriteAllText(Path.Combine(Root, ManifestFile),
                System.Text.Json.JsonSerializer.Serialize(manifest, new System.Text.Json.JsonSerializerOptions { WriteIndented = true }),
                new UTF8Encoding(false));
        }
        catch (Exception ex) { Diag.Log("chat role manifest: " + ex.Message); }
    }

    /// <summary>角色下拉的清單（沿用代理團隊的 <see cref="MultiAgent.RoleLibrary.RoleInfo"/> 型別）。</summary>
    public static List<MultiAgent.RoleLibrary.RoleInfo> ListRoles()
    {
        EnsureDefaults();
        var list = new List<MultiAgent.RoleLibrary.RoleInfo>();
        try
        {
            var keys = Directory.EnumerateFiles(RolesDir, "*.md")
                .Select(Path.GetFileNameWithoutExtension)
                .OrderBy(k => Array.IndexOf(BuiltInRoles, k) is int i && i >= 0 ? i : 100)
                .ThenBy(k => k, StringComparer.OrdinalIgnoreCase);
            foreach (var k in keys) list.Add(new MultiAgent.RoleLibrary.RoleInfo(k!, TitleOf(k)));
        }
        catch (Exception ex) { Diag.Log("chat list roles: " + ex.Message); }
        return list;
    }

    /// <summary>角色標題＝角色檔第一個「# 」標題；沒有就用檔名。</summary>
    public static string TitleOf(string? key)
    {
        if (string.IsNullOrWhiteSpace(key)) return "-";
        try
        {
            EnsureDefaults();
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
        return key;
    }

    /// <summary>開聊天室時清掉這個編號上次留下的角色檔。</summary>
    public static void ClearSession(int groupNumber)
    {
        try
        {
            string dir = SessionDir(groupNumber);
            if (Directory.Exists(dir)) foreach (var f in Directory.GetFiles(dir)) { try { File.Delete(f); } catch { } }
        }
        catch { }
    }

    /// <summary>組合一位參加者的角色檔（UTF-8 無 BOM）→ 絕對路徑（同時寫進 slot.RoleFile）。</summary>
    public static string Compose(AgentGroup g, AgentSlot s)
    {
        EnsureDefaults();
        var sb = new StringBuilder();
        sb.Append(ReadOrEmpty(CommonPath).TrimEnd()).Append("\n\n");
        if (!string.IsNullOrWhiteSpace(s.Role))
        {
            string roleText = ReadOrEmpty(Path.Combine(RolesDir, s.Role + ".md")).TrimEnd();
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

    /// <summary>第三層：這場聊天室的執行期脈絡（參加者、檔案位置、發言規則）。</summary>
    private static string RuntimeContext(AgentGroup g, AgentSlot me)
    {
        var joined = g.Slots.Where(x => x.Enabled).ToList();
        int w1 = joined.Max(x => x.AgentId.Length), w2 = joined.Max(x => x.RoleTitle.Length);
        var host = joined[0];
        var sb = new StringBuilder();
        sb.Append("# Runtime Context（AwayTerminal 產生）\n\n");
        sb.Append($"你的代號：{me.AgentId}\n");
        sb.Append($"你的角色：{me.RoleTitle}\n");
        sb.Append($"你用的 AI：{me.BackendName}\n");
        sb.Append($"聊天室編號：CHAT-{g.Number}\n");
        sb.Append($"專案資料夾：{g.Dir}\n");
        sb.Append($"討論迴數：{g.Rounds} 迴（一迴＝每個人各發言一次；使用者可以提前結束）\n");
        sb.Append("參加者：\n");
        foreach (var x in joined)
            sb.Append($"  - {x.AgentId.PadRight(w1)}  {x.RoleTitle.PadRight(w2)}  ({x.BackendName}){(ReferenceEquals(x, me) ? "   ← 你" : "")}\n");
        sb.Append($"主持人：{host.AgentId}（{host.RoleTitle}）\n\n");

        sb.Append("## 討論紀錄與你的發言\n\n");
        sb.Append($"- 共用討論紀錄：{AgentGroup.ChatRelDir}/{g.ChatFolder}/transcript.md（所有人的發言，AwayTerminal 依序接進去）\n");
        sb.Append($"- 你的發言檔：{AgentGroup.ChatRelDir}/{g.ChatFolder}/r{{迴數}}-{me.AgentId}.md（例：r1-{me.AgentId}.md）\n");
        sb.Append("- 路徑都是相對於專案資料夾。檔案一律 UTF-8（Windows PowerShell 讀檔請用 Get-Content -Raw -Encoding UTF8）。\n\n");
        sb.Append("輪到你時，AwayTerminal 會在你的畫面打一行字，告訴你第幾迴、要讀哪一份紀錄、發言要寫到哪個檔案。\n");
        sb.Append("**檔案路徑一律以那一行給的為準**（使用者換主題時，討論紀錄會換到新的資料夾，上面寫的路徑就過期了）。照著做：\n");
        sb.Append("讀紀錄 → 想清楚你要回應誰 → 把發言寫進上面那個檔案（300 字以內）→ 結束這一輪。\n");
        sb.Append("不要修改別人的發言檔，也不要改 transcript.md。\n\n");

        if (ReferenceEquals(me, host))
        {
            sb.Append("## 你是主持人\n\n");
            sb.Append($"- 討論結束時 AwayTerminal 會叫你寫結論，寫到 {AgentGroup.ChatRelDir}/{g.ChatFolder}/conclusion.md（600 字以內），\n");
            sb.Append("  並在你自己的畫面上顯示摘要給使用者看。\n");
            sb.Append("- 使用者只跟你說話；其他參加者只透過討論紀錄互動。\n\n");
        }

        if (!string.IsNullOrWhiteSpace(g.Topic))
            sb.Append($"## 這次的主題\n\n{g.Topic}\n");
        return sb.ToString();
    }
}
