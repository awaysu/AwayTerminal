using System.IO;
using System.Text;
using AwayTerminal.Models;

namespace AwayTerminal.Services.MultiAgent;

/// <summary>
/// Multi-Agent 信箱監看（1.2.0）：專案資料夾 <c>.ai/bus/</c>，一封信一個 .md。
/// <para>取代 1.1.11 協作分頁的 Stop hook＋curl＋localhost HTTP（使用者決定「完全不用 hook」）：寄信＝agent 用它本來就會的「寫檔」；
/// 沒有 hook、沒有信任審核、沒有父行程鏈，四家 CLI 通用。這裡只負責「發現新信」，路由／投遞在 MainWindow.MultiAgent。</para>
/// <para>發現：FileSystemWatcher（Created／Changed／Renamed）＋每 3 秒掃目錄當備援（大量寫入、網路磁碟會漏事件）。
/// 最後修改時間距今 ≥ <see cref="StableMs"/> 才讀（agent 可能 Write 後再 Edit）。
/// 已投遞紀錄 <c>.delivered</c>（每行一個檔名）：開組／恢復時先讀，已投遞的不重送；程式關著時寫進來的信在下次開啟後投遞。</para>
/// <para>同一個資料夾開兩組也可以：每組各自一個 MessageBus，依收件人 ID 的組號（Agent-1x／Agent-2x）各取所需（見 MainWindow.OnAgentMessage）。</para>
/// </summary>
internal sealed class MessageBus : IDisposable
{
    public const int StableMs = 1500;
    private const int ScanEveryMs = 3000;
    private const string DeliveredName = ".delivered";

    public string ProjectDir { get; }
    public string BusDir { get; }
    private string DeliveredPath => Path.Combine(BusDir, DeliveredName);

    /// <summary>發現一封新信（背景執行緒觸發；同一個檔名只觸發一次）。</summary>
    public event Action<AgentMessage>? MessageArrived;

    private readonly object _lock = new();
    private readonly HashSet<string> _raised = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<string> _delivered = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<string> _candidates = new(StringComparer.OrdinalIgnoreCase);
    private FileSystemWatcher? _fsw;
    private Timer? _timer;
    private DateTime _lastScanUtc;
    private bool _disposed;

    /// <summary>.delivered 由同一資料夾的多個組共用，寫入時跨實例鎖同一個物件。</summary>
    private static readonly Dictionary<string, object> FileLocks = new(StringComparer.OrdinalIgnoreCase);

    public MessageBus(string projectDir)
    {
        ProjectDir = projectDir;
        BusDir = Path.Combine(projectDir, ".ai", "bus");
    }

    public void Start()
    {
        Directory.CreateDirectory(BusDir);
        try
        {
            if (File.Exists(DeliveredPath))
                foreach (var line in File.ReadAllLines(DeliveredPath, Encoding.UTF8))
                    if (line.Trim().Length > 0) _delivered.Add(line.Trim());
        }
        catch (Exception ex) { Diag.Log("ma bus read .delivered: " + ex.Message); }

        try
        {
            _fsw = new FileSystemWatcher(BusDir, "*.md") { NotifyFilter = NotifyFilters.FileName | NotifyFilters.LastWrite | NotifyFilters.Size };
            _fsw.Created += (_, e) => AddCandidate(e.Name);
            _fsw.Changed += (_, e) => AddCandidate(e.Name);
            _fsw.Renamed += (_, e) => AddCandidate(e.Name);
            _fsw.Error += (_, e) => Diag.Log("ma bus watcher error: " + e.GetException().Message);
            _fsw.EnableRaisingEvents = true;
        }
        catch (Exception ex) { Diag.Log("ma bus watcher: " + ex.Message + " (polling only)"); }

        _lastScanUtc = DateTime.MinValue;   // 第一次 tick 立刻掃（開組前就在的信）
        _timer = new Timer(_ => Tick(), null, 300, 700);
        Diag.Log($"ma bus start {BusDir} delivered={_delivered.Count}");
    }

    private void AddCandidate(string? name)
    {
        if (string.IsNullOrEmpty(name) || !IsMessageName(name)) return;
        lock (_lock) { if (!_raised.Contains(name)) _candidates.Add(name); }
    }

    private static bool IsMessageName(string name) =>
        name.EndsWith(".md", StringComparison.OrdinalIgnoreCase) && !name.StartsWith('.')
        && !string.Equals(name, "board.md", StringComparison.OrdinalIgnoreCase);

    private int _ticking;   // Timer 回呼可能重疊（上一次還在讀檔）→ 跳過這次

    private void Tick()
    {
        if (_disposed || Interlocked.Exchange(ref _ticking, 1) != 0) return;
        try { TickCore(); }
        finally { Interlocked.Exchange(ref _ticking, 0); }
    }

    private void TickCore()
    {
        var now = DateTime.UtcNow;
        List<string> ready = new();
        try
        {
            if ((now - _lastScanUtc).TotalMilliseconds >= ScanEveryMs)
            {
                _lastScanUtc = now;
                foreach (var f in Directory.EnumerateFiles(BusDir, "*.md")) AddCandidate(Path.GetFileName(f));
            }
            lock (_lock)
            {
                foreach (var name in _candidates.ToList())
                {
                    if (_raised.Contains(name) || _delivered.Contains(name)) { _candidates.Remove(name); continue; }
                    string full = Path.Combine(BusDir, name);
                    if (!File.Exists(full)) { _candidates.Remove(name); continue; }
                    if ((now - File.GetLastWriteTimeUtc(full)).TotalMilliseconds < StableMs) continue;   // 還在寫
                    _candidates.Remove(name);
                    _raised.Add(name);
                    ready.Add(full);
                }
            }
        }
        catch (Exception ex) { Diag.Log("ma bus tick: " + ex.Message); }

        foreach (var full in ready.OrderBy(f => f, StringComparer.OrdinalIgnoreCase))
        {
            var msg = AgentMessage.Parse(full);
            if (msg == null)
            {
                lock (_lock) _raised.Remove(Path.GetFileName(full));   // 讀不到（被鎖）→ 下次再試
                continue;
            }
            try { MessageArrived?.Invoke(msg); } catch (Exception ex) { Diag.Log("ma bus handler: " + ex.Message); }
        }
    }

    public bool IsDelivered(string fileName) { lock (_lock) return _delivered.Contains(fileName); }

    /// <summary>記為已投遞（追加一行到 .delivered；同名只記一次）。</summary>
    public void MarkDelivered(string fileName)
    {
        lock (_lock) { if (!_delivered.Add(fileName)) return; }
        object fl;
        lock (FileLocks) { if (!FileLocks.TryGetValue(DeliveredPath, out fl!)) FileLocks[DeliveredPath] = fl = new object(); }
        lock (fl)
        {
            try { File.AppendAllText(DeliveredPath, fileName + "\n", new UTF8Encoding(false)); }
            catch (Exception ex) { Diag.Log("ma bus write .delivered: " + ex.Message); }
        }
    }

    /// <summary>AwayTerminal 自己寄一封信（例：收件人沒啟用、隊友加入），寄件人 AwayTerminal。回傳檔名。</summary>
    public string? Write(string from, string to, string type, string task, string body)
    {
        try
        {
            Directory.CreateDirectory(BusDir);
            string name = AgentMessage.NextFileName(BusDir, from, to);
            var sb = new StringBuilder();
            sb.Append("---\n").Append($"from: {from}\n").Append($"to: {to}\n").Append($"type: {type}\n");
            if (!string.IsNullOrWhiteSpace(task)) sb.Append($"task: {task}\n");
            sb.Append("---\n").Append(body.TrimEnd()).Append('\n');
            File.WriteAllText(Path.Combine(BusDir, name), sb.ToString(), new UTF8Encoding(false));
            return name;
        }
        catch (Exception ex) { Diag.Log("ma bus write message: " + ex.Message); return null; }
    }

    /// <summary>專案的 .gitignore 沒有 .ai/ 就追加一行（沒有 .gitignore 就建立）。</summary>
    public static void EnsureGitIgnore(string projectDir)
    {
        try
        {
            string path = Path.Combine(projectDir, ".gitignore");
            if (File.Exists(path))
            {
                string text = File.ReadAllText(path, Encoding.UTF8);
                foreach (var raw in text.Split('\n'))
                {
                    string t = raw.Trim().TrimEnd('\r');
                    if (t is ".ai" or ".ai/" or "/.ai" or "/.ai/" or ".ai/*" or "/.ai/*") return;
                }
                string nl = text.Contains("\r\n") ? "\r\n" : "\n";
                string add = (text.Length > 0 && !text.EndsWith('\n') ? nl : "") + "# AwayTerminal Multi-Agent mailbox" + nl + ".ai/" + nl;
                File.AppendAllText(path, add, new UTF8Encoding(false));
            }
            else
            {
                File.WriteAllText(path, "# AwayTerminal Multi-Agent mailbox\n.ai/\n", new UTF8Encoding(false));
            }
            Diag.Log("ma gitignore: added .ai/ to " + path);
        }
        catch (Exception ex) { Diag.Log("ma gitignore: " + ex.Message); }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        try { _timer?.Dispose(); } catch { }
        try { if (_fsw != null) { _fsw.EnableRaisingEvents = false; _fsw.Dispose(); } } catch { }
        Diag.Log($"ma bus stop {BusDir}");
    }
}
