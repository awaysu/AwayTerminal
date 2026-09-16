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
    private volatile NetworkStream? _stream;   // 背景執行緒連上後才設定；UI 執行緒的 Write 讀它
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

    /// <summary>啟動連線。連線移到背景執行緒——同步 Connect 在主機不通時會阻塞到
    /// TCP 逾時（約 20 秒），從 UI 執行緒呼叫（開連線／自動重連的 DispatcherTimer）會凍結整個視窗。
    /// 連不上改以紅字輸出錯誤並觸發 Exited（自動重連有勾就會接手退避重試）。</summary>
    public void Start(string host, int port)
    {
        _client = new TcpClient { NoDelay = true };
        _ = Task.Run(() => ConnectAndReadAsync(host, port));
    }

    private async Task ConnectAndReadAsync(string host, int port)
    {
        try
        {
            await _client!.ConnectAsync(host, port, _cts.Token).ConfigureAwait(false);
            _stream = _client.GetStream();
            if (KeepAliveMins > 0 && !_disposed)
            {
                var t = TimeSpan.FromMinutes(KeepAliveMins);
                _keepAlive = new Timer(_ => SendNop(), null, t, t);
            }
        }
        catch (Exception ex)
        {
            if (!_disposed)
            {
                try { Output?.Invoke(Encoding.UTF8.GetBytes($"\r\n\x1b[31m{ex.Message}\x1b[0m\r\n")); } catch { }
                try { Exited?.Invoke(); } catch { }
            }
            return;
        }
        await ReadLoopAsync().ConfigureAwait(false);
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

    // IAC 解析狀態要跨 ReadAsync 的 chunk 保留：協商序列（IAC WILL opt／IAC SB … IAC SE／轉義的 FF FF）落在讀取邊界上時，
    // 舊寫法把後半當成資料印到畫面（0xFB 0x01…變亂碼）、協商也沒回應。
    private enum IacState { Data, Iac, Option, Sb, SbIac }
    private IacState _iac = IacState.Data;
    private byte _iacCmd;

    private byte[] ProcessIncoming(byte[] buf, int len)
    {
        var data = new List<byte>(len);
        for (int i = 0; i < len; i++)
        {
            byte b = buf[i];
            switch (_iac)
            {
                case IacState.Data:
                    if (b == IAC) _iac = IacState.Iac; else data.Add(b);
                    break;
                case IacState.Iac:
                    if (b == IAC) { data.Add(IAC); _iac = IacState.Data; }            // 轉義的 0xFF
                    else if (b is WILL or WONT or DO or DONT) { _iacCmd = b; _iac = IacState.Option; }
                    else if (b == SB) _iac = IacState.Sb;
                    else _iac = IacState.Data;                                        // NOP／GA／AYT… 兩位元組指令：丟掉
                    break;
                case IacState.Option:
                    RespondOption(_iacCmd, b);
                    _iac = IacState.Data;
                    break;
                case IacState.Sb:
                    if (b == IAC) _iac = IacState.SbIac;                               // 子協商內容一律略過
                    break;
                case IacState.SbIac:
                    _iac = b == SE ? IacState.Data : IacState.Sb;                      // IAC SE 結束；IAC IAC＝資料裡的 FF，留在子協商
                    break;
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
        var stream = _stream;
        if (stream == null || data.Length == 0) return;
        // 先轉義再一次送出：NoDelay 下逐 byte WriteByte＝每個字一個 TCP 封包、一次 syscall（貼 4KB＝上千個封包，小型 telnet 裝置會掉）
        var buf = new List<byte>(data.Length + 8);
        foreach (var b in data)
        {
            if (b == IAC) buf.Add(IAC); // 轉義
            buf.Add(b);
        }
        try { stream.Write(System.Runtime.InteropServices.CollectionsMarshal.AsSpan(buf)); stream.Flush(); }
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
