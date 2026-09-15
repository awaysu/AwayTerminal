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

    /// <summary>內建角色在下拉裡的固定順序（其餘使用者自訂的依檔名排在後面）。使用者指定 UI/UX Designer 排在 QA Engineer 上面。
    /// ui-ux-designer（使用者提供，2026-09-14）；已經複製過範本的資料目錄會由 EnsureDefaults 補上缺的檔。</summary>
    public static readonly string[] BuiltInRoles = { "product-manager", "software-engineer", "software-architect", "ui-ux-designer", "qa-engineer" };

    /// <summary>設定視窗格 1～4 的預設角色（和下拉順序分開：格 4 仍預設 QA）。</summary>
    public static readonly string[] DefaultSlotRoles = { "product-manager", "software-engineer", "software-architect", "qa-engineer" };

    public sealed record RoleInfo(string Key, string Title);

    /// <summary>缺檔就從內嵌範本補；使用者沒改過的舊版範本換成新版（改過的不動）。</summary>
    public static void EnsureDefaults() => WriteDefaults(overwrite: false);

    /// <summary>內建範本全部覆寫回預設（使用者自己新增的角色檔不受影響）。</summary>
    public static void RestoreDefaults() => WriteDefaults(overwrite: true);

    // ---------- 範本自動更新（使用者要求，2026-09-15）----------
    // 實錄：加 UI/UX Designer 時改了內嵌的 product-manager.md，但使用者資料目錄裡 9/14 複製出來的舊版（從沒改過）一直沒換，
    // PM 不知道有設計師。原本「檔案存在就不動」分不出「使用者改過」和「只是舊版」。
    // 做法：multiagent\.defaults.json 記每個檔「AwayTerminal 最後寫進去的內容雜湊」；檔案現在的雜湊＝記錄值＝使用者沒改過 → 範本變了就換新版。
    // 還沒有記錄的舊資料目錄（1.2.0 開發期建的）改比對下面這份「以前出過的範本雜湊」（git 歷史算出來的）。
    // 雜湊前先正規化（去 UTF-8 BOM、CRLF→LF）：範本檔在 git 工作目錄裡可能是 LF 或 CRLF，嵌進 exe 的位元組不一定一樣。
    private const string ManifestFile = ".defaults.json";

    /// <summary>以前各版內建範本的正規化 SHA-256（改範本時把「改之前」那版的雜湊加進來）。</summary>
    private static readonly Dictionary<string, string[]> PreviousDefaults = new(StringComparer.OrdinalIgnoreCase)
    {
        ["common.md"] = new[]
        {
            "7faab5e2e9d0ce280a4351512cb616edd93a2dd8246bb0dd95c01b8ff849769f",   // f01fcaa
            "39541db501bff4b61a779a9a8d510f8bca33ba13b5164c09fca06ce3b76081c8",   // 657fa2f
        },
        ["roles/product-manager.md"] = new[]
        {
            "70bcf377e85c1e5d913b413fc2d4456b80b7253a1dff26e5f9189838a839c57d",   // f01fcaa
            "c49fb96726111551c280a3f29010274d0c127e60680275d66d401e11fcb2833f",   // 0565f54（加 UI/UX Designer）
        },
    };

    private static bool _synced;   // 這次執行已經比對過雜湊（EnsureDefaults 很常被叫，之後只補缺檔）

    private static void WriteDefaults(bool overwrite)
    {
        bool sync = overwrite || !_synced;
        var asm = Assembly.GetExecutingAssembly();
        var manifest = sync ? LoadManifest() : null;
        bool manifestChanged = false;
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
                        bool untouched = manifest != null && manifest.TryGetValue(rel, out var written) ? written == cur
                            : PreviousDefaults.TryGetValue(rel, out var olds) && olds.Contains(cur);
                        if (!untouched) continue;   // 使用者改過 → 不動，也不改記錄（之後照樣視為使用者的版本）
                        File.WriteAllBytes(target, data);
                        Diag.Log($"ma role default updated {rel}");
                    }
                }
                else
                {
                    Directory.CreateDirectory(Path.GetDirectoryName(target)!);
                    File.WriteAllBytes(target, data);
                }
                if (manifest != null && (!manifest.TryGetValue(rel, out var old) || old != newHash)) { manifest[rel] = newHash; manifestChanged = true; }
            }
            catch (Exception ex) { Diag.Log($"ma role default {rel}: {ex.Message}"); }
        }
        if (manifest != null && manifestChanged) SaveManifest(manifest);
        if (sync) _synced = true;
    }

    /// <summary>去 UTF-8 BOM、CRLF→LF 之後的 SHA-256（小寫十六進位）。</summary>
    private static string NormalizedHash(byte[] bytes)
    {
        string s = Encoding.UTF8.GetString(bytes);
        if (s.Length > 0 && s[0] == '\uFEFF') s = s.Substring(1);
        s = s.Replace("\r\n", "\n");
        return Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(Encoding.UTF8.GetBytes(s))).ToLowerInvariant();
    }

    /// <summary>讀 .defaults.json；沒有＝空（沒記錄的檔改用 PreviousDefaults 判斷）；壞掉＝null（同樣用 PreviousDefaults，且不覆寫壞檔）。</summary>
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
        catch (Exception ex) { Diag.Log("ma role manifest: " + ex.Message); }
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

        // 1.2.0 實測：Codex（gpt-5.6-sol）內建「你是 /root，可以 spawn_agent 開子代理」的提示詞，使用者說「讓 Agent-12 做…」時
        // 它直接開了名叫 /root/agent_12 的子代理，信箱完全沒用到；--disable multi_agent、agents.max_depth 等設定都拿不掉那些工具
        // （codex exec 問它有哪些工具，照樣列出 collaboration.spawn_agent）→ 只能在這裡講清楚。Claude Code 的 Task 工具同理。
        sb.Append("## Your teammates are separate terminals, not sub-agents\n\n");
        sb.Append("Every other agent listed above is an independent coding-agent session running in its own AwayTerminal pane.\n");
        sb.Append("They are NOT your sub-agents and you cannot reach them with any built-in tool.\n\n");
        sb.Append("- \"Have Agent-xx do something\" / \"let Agent-xx handle it\" always means: write a mailbox message to that agent (below).\n");
        sb.Append("- Do not use built-in sub-agent or collaboration tools in this team session at all: no spawn_agent, followup_task,\n");
        sb.Append("  send_message, wait_agent, list_agents, interrupt_agent, no Task/sub-agent tool, and never create a sub-agent named after a teammate.\n");
        sb.Append("- Do not wait, sleep or poll for replies. End your turn; AwayTerminal types a notice into your terminal when a reply arrives.\n\n");

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
        sb.Append("status: completed        # completed | failed | blocked | stopped | paused | pass | fail  (only for results)\n");
        sb.Append("files_changed:\n  - path/to/file\n");
        sb.Append("---\n(body)\n```\n\n");
        sb.Append($"Never edit or delete an existing message file. Do not write anything else into {AgentMessage.BusRelDir}/.\n");
        sb.Append("Writing the file is the whole act of sending: after writing it, end your turn; AwayTerminal delivers it.\n\n");

        // 使用者回報（2026-09-15）：PM 寄了暫停信就跟使用者說「已通知暫停」，但收件人正在工作、信在排隊，根本沒停
        sb.Append("### Delivery timing\n\n");
        sb.Append("AwayTerminal types a message into the recipient's terminal only when the recipient is idle. While the recipient is\n");
        sb.Append("working, the message waits in a queue and is delivered after it finishes its current work. A message therefore\n");
        sb.Append("cannot interrupt or stop an agent that is working.\n\n");
        sb.Append($"- If the user wants agents to stop right away, tell the user to right-click the team tab and choose \"{Loc.T("ma.menuStop")}\"\n");
        sb.Append("  (it interrupts every agent), or to press Esc in that agent's pane. Do not claim that a message has stopped anyone.\n");
        sb.Append("- When you report a message you sent, say it was sent and will be read when the recipient is idle. Do not say the\n");
        sb.Append("  recipient has received it, been notified, or acted on it until its reply arrives.\n\n");

        sb.Append("## How you receive messages\n\n");
        sb.Append("AwayTerminal types a line like this into your terminal:\n\n");
        string exampleFrom = enabled.FirstOrDefault(x => !ReferenceEquals(x, me))?.AgentId ?? "Agent-" + g.Number + "1";
        sb.Append("    ").Append(string.Format(Loc.T("ma.deliverOne"), 7, exampleFrom, "TASK-001", "TASK",
            $"{AgentMessage.BusRelDir}/0007-{exampleFrom}-to-{me.AgentId}.md")).Append("\n\n");
        sb.Append("Read that file, act according to your role, and reply by writing a new message file.\n");
        sb.Append($"You may read any file in {AgentMessage.BusRelDir}/ for context.\n");
        // D（使用者選，2026-09-15 test3 實錄）：派 TASK-003 的信因為收件人一直忙、排了 23 分鐘才送，工程師早就自己讀信箱做完了，
        // 晚到的投遞又讓它多花一輪重看。
        sb.Append("A message can be delivered long after it was written. If AwayTerminal delivers a message you have already read and\n");
        sb.Append("handled (for example you found it in the mailbox yourself), say so in one line and continue. Do not redo the work and do\n");
        sb.Append("not reply to it again.\n");
        // 1.2.0 實測：PowerShell 5.1 的 Get-Content 預設用系統字碼頁讀，中文訊息變亂碼（Codex 在 Windows 預設 shell 就是它）
        sb.Append("Message files are UTF-8. Read and write them as UTF-8 (in Windows PowerShell use Get-Content -Raw -Encoding UTF8 <file>).\n\n");

        // 使用者要求（2026-09-15）：格 2～4 的任務不管是完成、失敗、被停止、被暫停，一律回報給格 1（下方全寬、使用者對話的那一格）。
        // 「停止任務」打進去的「先停一下然後記錄目前狀態」不是從信箱來的，也要回報——否則 Agent-x1 不知道各格停在哪。
        var lead = g.Slots[0];
        if (!ReferenceEquals(me, lead) && lead.Enabled)
        {
            sb.Append($"## Always report to {lead.AgentId}\n\n");
            sb.Append($"{lead.AgentId} ({lead.RoleTitle}) coordinates this team and is the agent the user talks to.\n");
            sb.Append("Whenever a task you are working on ends for any reason - completed, failed, blocked, stopped, interrupted or paused,\n");
            sb.Append("including when the user or AwayTerminal tells you to stop or pause, and even if the task did not come from the mailbox -\n");
            sb.Append($"write a message to {lead.AgentId} with what happened, the current state and what is left to do.\n");
            sb.Append("Use type TASK_RESULT (status completed or failed) for finished work, BLOCKED when you cannot continue,\n");
            sb.Append("and INFO with status stopped or paused when you were told to stop or pause.\n\n");
        }
        else if (hasWorker)
        {
            sb.Append("## Reports from your teammates\n\n");
            sb.Append("The other agents report to you whenever one of their tasks ends, fails, is blocked, or is stopped or paused.\n");
            sb.Append("Use those reports to keep track of where each agent is.\n\n");
        }

        // 使用者要求（2026-09-15）：有開 UI/UX Designer 時，有畫面的任務一律先找設計師。實錄：使用者對 PM 說「用 C# 寫一個貪食蛇遊戲」，
        // PM 直接派給工程師（它的角色檔是舊版、沒提設計師，而且規則寫「小任務 PM → SE → PM 就夠」）。
        // 寫在執行期脈絡而不是只改 product-manager.md：資料目錄裡的舊角色檔不一定會更新，而且只有真的有開設計師才講。
        var designer = enabled.FirstOrDefault(x => !ReferenceEquals(x, me) && x.Role == "ui-ux-designer");
        if (designer != null && (me.Role == "product-manager" || ReferenceEquals(me, lead)))
        {
            sb.Append("## Design before UI implementation\n\n");
            sb.Append($"{designer.AgentId} (UI/UX Designer) is enabled. Every task that has a user interface - new or changed windows, screens,\n");
            sb.Append($"layouts, controls or visual style - first gets a design from {designer.AgentId}: assign the design task, wait for the result,\n");
            sb.Append("then give the design to the agent that implements it. This applies to small tasks too.\n");
            sb.Append("Skip the design step only when the change has no visible UI or the user asks to skip it.\n\n");
        }

        // B、C（使用者選，2026-09-15 test3 實錄：貪食蛇小遊戲三個 agent 花 45 分鐘以上——工程師的 GUI 驅動測試一直被別的視窗搶走焦點、
        // 失焦自動暫停，14 分鐘都在修測試腳本；三個終端機＋使用者共用同一個 Windows 桌面，這種測試在多代理下注定不穩）
        string askUser = ReferenceEquals(me, lead) || !lead.Enabled ? "ask the user" : $"ask {lead.AgentId} to ask the user";
        sb.Append("## Shared desktop\n\n");
        sb.Append("All agents and the user share one Windows desktop, so any window can lose focus at any moment.\n");
        sb.Append("Do not run checks that need the foreground window or keyboard focus: no GUI automation that sends keystrokes or clicks\n");
        sb.Append("to windows, no bringing windows to the front, no capturing live application windows, no tests that depend on a window\n");
        sb.Append("keeping focus. Prefer command-line and headless checks.\n");
        sb.Append($"If something can only be verified by looking at a window, {askUser} to check it.\n\n");

        string reportStuck = ReferenceEquals(me, lead) || !lead.Enabled
            ? "tell the user what you tried, what you found and what you need"
            : $"report BLOCKED to {lead.AgentId} with what you tried, what you found and what you need";
        sb.Append("## When you are stuck\n\n");
        sb.Append("If the same problem is still unsolved after two different attempts, or after about 10 minutes, stop working on it.\n");
        sb.Append($"Do not build more tools, scripts or test harnesses around it; {reportStuck}.\n\n");

        sb.Append("## Talking to the user\n\n");
        // 1.2.0 實測：「叫 Agent-12 顯示 123」→ worker 只把 123 寫進回信，自己的畫面沒顯示，使用者在那格看不到
        sb.Append($"Only {lead.AgentId} takes requests from the user and asks the user questions; the other agents report to {lead.AgentId}\n");
        sb.Append("with a message file. The user can still see every agent's terminal, so a worker also shows its work in its own terminal:\n");
        sb.Append("when a task asks you to show, print or display something, output it in your terminal reply as well as in your result message.\n");
        sb.Append("Write terminal replies in the user's language.\n");
        return sb.ToString();
    }
}
