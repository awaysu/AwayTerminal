using System.Collections.Generic;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace AwayTerminal.Sessions;

/// <summary>內建 Telnet 連線（不依賴 Windows telnet 用戶端），含最小 IAC 協商。</summary>
public sealed class TelnetSession : ITerminalSession
{
    private TcpClient? _client;
    private NetworkStream? _stream;
    private readonly CancellationTokenSource _cts = new();
    private bool _disposed;

    public event Action<byte[]>? Output;
    public event Action? Exited;
    public int ProcessId => 0;

    private const byte IAC = 255, DONT = 254, DO = 253, WONT = 252, WILL = 251, SB = 250, SE = 240, NOP = 241;
    private const byte OPT_ECHO = 1, OPT_SGA = 3;

    /// <summary>保持連線：每 N 分鐘送一個 IAC NOP（伺服器會忽略、不影響畫面；0=關）。Start 前設定。</summary>
    public int KeepAliveMins { get; set; }
    private Timer? _keepAlive;

    public void Start(string host, int port)
    {
        _client = new TcpClient { NoDelay = true };
        _client.Connect(host, port); // 連不上會丟例外，由呼叫端處理
        _stream = _client.GetStream();
        _ = Task.Run(ReadLoopAsync);
        if (KeepAliveMins > 0)
        {
            var t = TimeSpan.FromMinutes(KeepAliveMins);
            _keepAlive = new Timer(_ => SendNop(), null, t, t);
        }
    }

    /// <summary>IAC NOP 直接寫串流——不能走 Write()（它會把 0xFF 轉義成資料位元組）。</summary>
    private void SendNop()
    {
        try { _stream?.Write(new byte[] { IAC, NOP }); _stream?.Flush(); } catch { }
    }

    private async Task ReadLoopAsync()
    {
        var buf = new byte[8192];
        try
        {
            while (!_cts.IsCancellationRequested && _stream != null)
            {
                int n = await _stream.ReadAsync(buf.AsMemory(0, buf.Length), _cts.Token).ConfigureAwait(false);
                if (n <= 0) break;
                var data = ProcessIncoming(buf, n);
                if (data.Length > 0) Output?.Invoke(data);
            }
        }
        catch { }
        finally { if (!_disposed) { try { Exited?.Invoke(); } catch { } } }
    }

    private byte[] ProcessIncoming(byte[] buf, int len)
    {
        var data = new List<byte>(len);
        int i = 0;
        while (i < len)
        {
            byte b = buf[i++];
            if (b != IAC) { data.Add(b); continue; }
            if (i >= len) break;
            byte cmd = buf[i++];
            if (cmd == IAC) { data.Add(IAC); continue; } // 轉義的 0xFF
            if (cmd is WILL or WONT or DO or DONT)
            {
                if (i >= len) break;
                RespondOption(cmd, buf[i++]);
            }
            else if (cmd == SB)
            {
                while (i < len)
                {
                    if (buf[i] == IAC && i + 1 < len && buf[i + 1] == SE) { i += 2; break; }
                    i++;
                }
            }
        }
        return data.ToArray();
    }

    private void RespondOption(byte cmd, byte opt)
    {
        byte reply;
        if (cmd == WILL) reply = (opt == OPT_ECHO || opt == OPT_SGA) ? DO : DONT;
        else if (cmd == DO) reply = (opt == OPT_SGA) ? WILL : WONT;
        else return;
        try { _stream?.Write(new byte[] { IAC, reply, opt }); _stream?.Flush(); } catch { }
    }

    public void Write(ReadOnlySpan<byte> data)
    {
        if (_stream == null) return;
        try
        {
            foreach (var b in data)
            {
                if (b == IAC) _stream.WriteByte(IAC); // 轉義
                _stream.WriteByte(b);
            }
            _stream.Flush();
        }
        catch { }
    }

    public void WriteText(string text) => Write(Encoding.UTF8.GetBytes(text));

    public void Resize(int cols, int rows) { /* NAWS 可選，暫略 */ }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        try { _keepAlive?.Dispose(); } catch { }
        try { _cts.Cancel(); } catch { }
        try { _stream?.Dispose(); } catch { }
        try { _client?.Close(); } catch { }
        _cts.Dispose();
    }
}
