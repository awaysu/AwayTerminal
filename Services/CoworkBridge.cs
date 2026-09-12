using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

namespace AwayTerminal.Services;

/// <summary>
/// Claude+Codex 協作分頁的「交棒訊號」接收端（1.1.11）。
/// <para>做法：開協作分頁時，只替那兩個 session 在啟動參數注入 Stop hook（Claude：<c>--settings 檔案</c>；Codex：<c>-c hooks.Stop=…</c>），
/// 不改使用者全域的 ~/.claude、~/.codex 設定。每一輪回覆結束時 hook 跑
/// <c>curl.exe -s -m 5 -X POST http://127.0.0.1:埠/cowork/stop/{claude|codex} --data-binary @-</c>（stdin 的 hook JSON 帶 cwd）。
/// 選 curl：同一個指令字串在 Claude 的 Git Bash、Codex 的 cmd／PowerShell 都能跑（實測 Claude hook 走 bash.exe → curl.exe）。</para>
/// <para>指令字串刻意「每次都一樣」（固定埠、不帶分頁代號或 token）：Codex 會要求使用者審核新的／變更過的 hook，
/// 字串每次變就每次都要審；固定字串只要第一次選「Trust all and continue」。</para>
/// <para>是哪個分頁送的：先查這條 TCP 連線的用戶端 PID（GetExtendedTcpTable），沿父行程往上找到某個協作分頁的 session 行程
/// （實測鏈：curl ← bash ← bash ← claude.exe ← AwayTerminal；Codex CLI 的代理跑在分頁自己的 codex.exe 裡，不是桌面版共用 daemon）；
/// 找不到才退回「agent 種類＋cwd」比對。只綁 127.0.0.1；收到訊號只會讓 AwayTerminal 去看交棒檔、打一行固定的「請讀…」，
/// 不接受任意文字，所以不另設 token。</para>
/// </summary>
public sealed class CoworkBridge
{
    public int Port { get; private set; }

    /// <summary>收到 Stop 訊號（背景執行緒觸發）：agent＝claude|codex、cwd＝hook JSON 的 cwd（可能空）、clientPid＝curl 的 PID（查不到＝0）。</summary>
    public event Action<string, string, int>? StopReceived;

    private TcpListener? _listener;

    public string Dir => Path.Combine(AppPaths.DataDir, "cowork");
    public string ClaudeSettingsPath => Path.Combine(Dir, "claude-hooks.json");
    public string ClaudeProtocolPath => Path.Combine(Dir, "protocol-claude.md");

    /// <summary>啟動（偏好上次的埠，被占用就另找並回報新埠，呼叫端存設定）。失敗回 false（協作分頁照開、只是不會自動交棒）。</summary>
    public bool Start(int preferredPort)
    {
        if (_listener != null) return true;
        var rnd = new Random();
        for (int attempt = 0; attempt < 30; attempt++)
        {
            int port = attempt == 0 && preferredPort is >= 1024 and <= 65535 ? preferredPort : rnd.Next(47100, 48900);
            try
            {
                var l = new TcpListener(IPAddress.Loopback, port);
                l.Start();
                _listener = l;
                Port = port;
                WriteArtifacts();
                _ = Task.Run(AcceptLoop);
                Diag.Log($"cowork bridge listening 127.0.0.1:{port}");
                return true;
            }
            catch (SocketException) { }
        }
        Diag.Log("cowork bridge: no free port");
        return false;
    }

    /// <summary>hook 要跑的指令（兩邊只差最後的 agent 名稱）。</summary>
    public string StopCommand(string agent) =>
        $"curl.exe -s -m 5 -X POST http://127.0.0.1:{Port}/cowork/stop/{agent} --data-binary @-";

    /// <summary>ClaudeCode 半邊的額外啟動參數：注入 Stop hook ＋ 附加交棒規則（系統提示）。</summary>
    public string ClaudeArgs() => $" --settings \"{ClaudeSettingsPath}\" --append-system-prompt-file \"{ClaudeProtocolPath}\"";

    /// <summary>Codex 半邊的額外啟動參數（-c 的值以 TOML 解析：單引號＝literal string，裡面不能再有單引號／雙引號）。</summary>
    public string CodexArgs() =>
        $" -c \"hooks.Stop=[{{hooks=[{{type='command',command='{StopCommand("codex")}',timeout=10}}]}}]\"" +
        $" -c \"developer_instructions='{OneLine(Protocol("Codex", "Claude Code", "to-claude.md", "to-codex.md"))}'\"";

    private void WriteArtifacts()
    {
        Directory.CreateDirectory(Dir);
        var settings = new
        {
            hooks = new
            {
                Stop = new[] { new { hooks = new[] { new { type = "command", command = StopCommand("claude"), timeout = 10 } } } }
            }
        };
        File.WriteAllText(ClaudeSettingsPath, JsonSerializer.Serialize(settings, new JsonSerializerOptions { WriteIndented = true }), new UTF8Encoding(false));
        File.WriteAllText(ClaudeProtocolPath, Protocol("Claude Code", "Codex", "to-codex.md", "to-claude.md"), new UTF8Encoding(false));
    }

    /// <summary>交棒規則（附加在系統提示／developer instructions；只存在協作分頁的這兩個 session）。</summary>
    private static string Protocol(string me, string partner, string outFile, string inFile) =>
        $"# AwayTerminal Claude+Codex 協作\n" +
        $"你（{me}）在 AwayTerminal 的「Claude+Codex」協作分頁裡：畫面另一半是 {partner}（另一個 AI 程式助理），和你在同一個專案資料夾工作，看不到你的對話。\n" +
        $"- 需要 {partner} 接手時：把交辦內容（背景、要做的事、限制、怎麼驗收）整份寫進專案根目錄的 .ai/handoff/{outFile}（覆寫；資料夾不存在就建立），然後結束這一輪回覆。這一輪結束後 AwayTerminal 會自動請 {partner} 去讀。\n" +
        $"- {partner} 做完會把結果寫進 .ai/handoff/{inFile}，你會收到「請讀 .ai/handoff/{inFile}…」的訊息，讀完再決定下一步。\n" +
        $"- 整個任務已完成、不需要再交棒時：在交棒檔第一行寫 STATUS: DONE，AwayTerminal 看到就停止交棒。\n" +
        $"- 只有真的要 {partner} 接手時才寫交棒檔；要問使用者問題、或這一輪只是回報時，不要改交棒檔。\n" +
        $"- 交棒檔要能獨立閱讀（{partner} 看不到你的對話），寫清楚檔案路徑與目前狀態。\n";

    private static string OneLine(string s) =>
        s.Replace("\r", "").Replace("\n", " ").Replace("'", "’").Replace("\"", "”").Trim();

    // ---------- 極簡 HTTP（只收 POST /cowork/stop/{agent}）----------
    private async Task AcceptLoop()
    {
        while (_listener != null)
        {
            TcpClient client;
            try { client = await _listener.AcceptTcpClientAsync().ConfigureAwait(false); }
            catch { break; }
            _ = Task.Run(() => HandleClient(client));
        }
    }

    private async Task HandleClient(TcpClient client)
    {
        string agent = "", cwd = "";
        int pid = 0;
        try
        {
            using (client)
            {
                var remote = (IPEndPoint)client.Client.RemoteEndPoint!;
                pid = FindClientPid(remote.Port, Port);   // 連線還開著時查，才查得到
                client.ReceiveTimeout = 3000;
                var stream = client.GetStream();
                var (path, body) = await ReadRequest(stream).ConfigureAwait(false);
                const string prefix = "/cowork/stop/";
                if (path.StartsWith(prefix, StringComparison.Ordinal)) agent = path[prefix.Length..].Trim('/').ToLowerInvariant();
                try
                {
                    using var doc = JsonDocument.Parse(body.Length == 0 ? "{}" : body);
                    if (doc.RootElement.TryGetProperty("cwd", out var c) && c.ValueKind == JsonValueKind.String) cwd = c.GetString() ?? "";
                }
                catch { }
                // 立刻回 {}：hook 等回應才結束（Codex 的 Stop hook 輸出必須是 JSON；空物件＝不做任何決定）
                byte[] resp = Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
                await stream.WriteAsync(resp).ConfigureAwait(false);
            }
        }
        catch (Exception ex) { Diag.Log("cowork bridge request: " + ex.Message); return; }
        if (agent is "claude" or "codex") StopReceived?.Invoke(agent, cwd, pid);
    }

    private static async Task<(string Path, string Body)> ReadRequest(NetworkStream stream)
    {
        var buf = new MemoryStream();
        var chunk = new byte[4096];
        int headerEnd = -1;
        while (headerEnd < 0 && buf.Length < 32 * 1024)
        {
            int n = await stream.ReadAsync(chunk).ConfigureAwait(false);
            if (n <= 0) break;
            buf.Write(chunk, 0, n);
            headerEnd = IndexOf(buf.GetBuffer(), (int)buf.Length, "\r\n\r\n"u8);
        }
        if (headerEnd < 0) return ("", "");
        string head = Encoding.ASCII.GetString(buf.GetBuffer(), 0, headerEnd);
        var lines = head.Split("\r\n");
        var first = lines[0].Split(' ');
        string path = first.Length > 1 && first[0] == "POST" ? first[1] : "";
        int len = 0;
        foreach (var l in lines)
            if (l.StartsWith("Content-Length:", StringComparison.OrdinalIgnoreCase)) int.TryParse(l[15..].Trim(), out len);
        len = Math.Clamp(len, 0, 1024 * 1024);
        int have = (int)buf.Length - (headerEnd + 4);
        var body = new MemoryStream();
        body.Write(buf.GetBuffer(), headerEnd + 4, Math.Max(0, Math.Min(have, len)));
        while (body.Length < len)
        {
            int n = await stream.ReadAsync(chunk.AsMemory(0, (int)Math.Min(chunk.Length, len - body.Length))).ConfigureAwait(false);
            if (n <= 0) break;
            body.Write(chunk, 0, n);
        }
        return (path, Encoding.UTF8.GetString(body.GetBuffer(), 0, (int)body.Length));
    }

    private static int IndexOf(byte[] data, int length, ReadOnlySpan<byte> pattern) => data.AsSpan(0, length).IndexOf(pattern);

    // ---------- 連線的用戶端 PID ----------
    [DllImport("iphlpapi.dll", SetLastError = true)]
    private static extern uint GetExtendedTcpTable(IntPtr pTcpTable, ref int pdwSize, bool bOrder, int ulAf, int tableClass, uint reserved);

    [StructLayout(LayoutKind.Sequential)]
    private struct MIB_TCPROW_OWNER_PID { public uint State, LocalAddr, LocalPort, RemoteAddr, RemotePort, OwningPid; }

    private static int NetPort(uint p) => (int)(((p & 0xFF) << 8) | ((p >> 8) & 0xFF));

    /// <summary>找「本機埠＝clientPort、對方埠＝serverPort」那條 IPv4 連線的擁有者 PID（＝發 POST 的 curl）；找不到回 0。</summary>
    private static int FindClientPid(int clientPort, int serverPort)
    {
        const int AF_INET = 2, TCP_TABLE_OWNER_PID_ALL = 5;
        int size = 0;
        GetExtendedTcpTable(IntPtr.Zero, ref size, false, AF_INET, TCP_TABLE_OWNER_PID_ALL, 0);
        IntPtr buf = Marshal.AllocHGlobal(size);
        try
        {
            if (GetExtendedTcpTable(buf, ref size, false, AF_INET, TCP_TABLE_OWNER_PID_ALL, 0) != 0) return 0;
            int count = Marshal.ReadInt32(buf);
            int rowSize = Marshal.SizeOf<MIB_TCPROW_OWNER_PID>();
            for (int i = 0; i < count; i++)
            {
                var row = Marshal.PtrToStructure<MIB_TCPROW_OWNER_PID>(buf + 4 + i * rowSize);
                if (NetPort(row.LocalPort) == clientPort && NetPort(row.RemotePort) == serverPort) return (int)row.OwningPid;
            }
        }
        catch { }
        finally { Marshal.FreeHGlobal(buf); }
        return 0;
    }
}
