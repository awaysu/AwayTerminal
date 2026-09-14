using System.IO;
using System.Text;
using System.Text.RegularExpressions;

namespace AwayTerminal.Models;

/// <summary>
/// Multi-Agent 信箱裡的一封信（1.2.0）：專案資料夾 <c>.ai/bus/NNNN-&lt;寄件人&gt;-to-&lt;收件人&gt;.md</c>，
/// 開頭一段 YAML front matter（from／to／type／task／status／files_changed…），之後是 Markdown 內文。
/// <para>解析刻意寬鬆（agent 不一定照格式寫）：缺 from／to 用檔名；缺 type 當 INFO；front matter 壞掉仍照檔名投遞。
/// 不引入 YAML 函式庫——只認 <c>key: value</c> 與緊接的 <c>- item</c> 清單。</para>
/// </summary>
public sealed class AgentMessage
{
    public string FileName { get; init; } = "";
    public string FullPath { get; init; } = "";
    /// <summary>檔名開頭的編號（排序用；解析不到＝int.MaxValue）。</summary>
    public int Seq { get; init; } = int.MaxValue;
    public string From { get; set; } = "";
    public string To { get; set; } = "";
    public string Type { get; set; } = "INFO";
    public string Task { get; set; } = "";
    public string Status { get; set; } = "";
    public List<string> FilesChanged { get; } = new();
    public Dictionary<string, string> Fields { get; } = new(StringComparer.OrdinalIgnoreCase);
    public string Body { get; set; } = "";
    /// <summary>front matter 有問題（缺 --- 結尾、缺 from/to）——仍照檔名投遞，另記 diag。</summary>
    public bool HeaderWarning { get; set; }
    public DateTime CreatedUtc { get; init; }

    /// <summary>收件人是 all（投給組內除寄件人以外的每個已啟動 agent）。</summary>
    public bool IsBroadcast => string.Equals(To, "all", StringComparison.OrdinalIgnoreCase);

    /// <summary>信箱資料夾相對專案根目錄的路徑（投遞那一行裡用 / 分隔）。</summary>
    public const string BusRelDir = ".ai/bus";

    public string RelPath => BusRelDir + "/" + FileName;

    private static readonly Regex NameRe = new(@"^(?<n>\d{1,9})-(?<from>.+?)-to-(?<to>.+?)\.md$", RegexOptions.IgnoreCase);
    private static readonly Regex IdRe = new(@"^agent[-_ ]?(?<d>\d{2})$", RegexOptions.IgnoreCase);

    /// <summary>「agent-12」「Agent12」→「Agent-12」；all 與其他名字（AwayTerminal、user）原樣（all 轉小寫）。</summary>
    public static string NormalizeId(string s)
    {
        s = (s ?? "").Trim().Trim('"', '\'');
        if (string.Equals(s, "all", StringComparison.OrdinalIgnoreCase)) return "all";
        var m = IdRe.Match(s);
        return m.Success ? "Agent-" + m.Groups["d"].Value : s;
    }

    /// <summary>讀檔並解析；讀不到（被鎖、已刪）回 null。</summary>
    public static AgentMessage? Parse(string path)
    {
        string text;
        DateTime created;
        try
        {
            using var fs = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            using var sr = new StreamReader(fs, new UTF8Encoding(false), detectEncodingFromByteOrderMarks: true);
            text = sr.ReadToEnd();
            created = File.GetCreationTimeUtc(path);
        }
        catch { return null; }

        string name = Path.GetFileName(path);
        var nm = NameRe.Match(name);
        var msg = new AgentMessage
        {
            FileName = name, FullPath = path, CreatedUtc = created,
            Seq = nm.Success && int.TryParse(nm.Groups["n"].Value, out int n) ? n : int.MaxValue,
        };
        ParseText(msg, text);
        if (string.IsNullOrWhiteSpace(msg.From) && nm.Success) msg.From = nm.Groups["from"].Value;
        if (string.IsNullOrWhiteSpace(msg.To) && nm.Success) msg.To = nm.Groups["to"].Value;
        msg.From = NormalizeId(msg.From);
        msg.To = NormalizeId(msg.To);
        if (string.IsNullOrWhiteSpace(msg.Type)) msg.Type = "INFO";
        msg.Type = msg.Type.Trim().ToUpperInvariant();
        return msg;
    }

    /// <summary>解析 front matter＋內文（Parse 的核心；也供測試直接餵字串）。</summary>
    public static void ParseText(AgentMessage msg, string text)
    {
        var lines = text.Replace("\r\n", "\n").Replace('\r', '\n').Split('\n');
        int i = 0;
        while (i < lines.Length && lines[i].Trim().Length == 0) i++;   // 開頭空行容忍
        if (i >= lines.Length || lines[i].Trim() != "---")
        {
            msg.Body = text;
            msg.HeaderWarning = true;
            return;
        }
        int end = -1;
        for (int j = i + 1; j < lines.Length; j++)
            if (lines[j].Trim() == "---") { end = j; break; }
        if (end < 0) { msg.HeaderWarning = true; end = lines.Length; }

        string? listKey = null;
        for (int j = i + 1; j < end; j++)
        {
            string raw = lines[j];
            string t = raw.Trim();
            if (t.Length == 0 || t.StartsWith('#')) continue;
            if (t.StartsWith("- ") || t == "-")
            {
                if (listKey != null && string.Equals(listKey, "files_changed", StringComparison.OrdinalIgnoreCase))
                {
                    string item = StripComment(t.Length > 1 ? t.Substring(2) : "");
                    if (item.Length > 0) msg.FilesChanged.Add(item);
                }
                continue;
            }
            int c = t.IndexOf(':');
            if (c <= 0) continue;
            string key = t.Substring(0, c).Trim();
            string val = StripComment(t.Substring(c + 1));
            listKey = val.Length == 0 ? key : null;
            if (val.Length == 0) continue;
            msg.Fields[key] = val;
            switch (key.ToLowerInvariant())
            {
                case "from": msg.From = val; break;
                case "to": msg.To = val; break;
                case "type": msg.Type = val; break;
                case "task": case "task_id": msg.Task = val; break;
                case "status": msg.Status = val; break;
            }
        }
        msg.Body = end + 1 < lines.Length ? string.Join("\n", lines, end + 1, lines.Length - end - 1).Trim() : "";
    }

    /// <summary>去掉值尾端的 # 註解與成對引號（值本身含 # 的 URL 之類很少見，寬鬆為先）。</summary>
    private static string StripComment(string v)
    {
        int h = v.IndexOf(" #", StringComparison.Ordinal);
        if (h >= 0) v = v.Substring(0, h);
        v = v.Trim();
        if (v.Length >= 2 && ((v[0] == '"' && v[^1] == '"') || (v[0] == '\'' && v[^1] == '\''))) v = v[1..^1];
        return v.Trim();
    }

    /// <summary>下一個檔名：資料夾裡最大的 NNNN＋1（4 位數補零），例 0008-AwayTerminal-to-Agent-12.md。</summary>
    public static string NextFileName(string busDir, string from, string to)
    {
        int max = 0;
        try
        {
            foreach (var f in Directory.EnumerateFiles(busDir, "*.md"))
            {
                var m = NameRe.Match(Path.GetFileName(f));
                if (m.Success && int.TryParse(m.Groups["n"].Value, out int n) && n > max) max = n;
            }
        }
        catch { }
        return $"{max + 1:D4}-{from}-to-{to}.md";
    }
}
