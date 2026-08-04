using System.IO.Ports;
using System.Text;
using System.Threading;

namespace AwayTerminal.Sessions;

/// <summary>序列埠（COM）連線，使用 System.IO.Ports。</summary>
public sealed class SerialSession : ITerminalSession
{
    private SerialPort? _port;
    private Thread? _reader;
    private volatile bool _disposed;

    public event Action<byte[]>? Output;
    public event Action? Exited;
    public int ProcessId => 0;

    public void Start(string portName, int baud, int dataBits, Parity parity, StopBits stopBits, Handshake flow)
    {
        _port = new SerialPort(portName, baud, parity, dataBits, stopBits)
        {
            Handshake = flow,
            ReadBufferSize = 65536,
            WriteTimeout = 2000,
            ReadTimeout = SerialPort.InfiniteTimeout, // blocking read：有資料就立即返回
        };
        _port.Open(); // 開不了會丟例外，由呼叫端處理

        // 注意：當 Handshake 為 RTS 類型時，設定 RtsEnable 會丟例外，故有條件設定
        try { _port.DtrEnable = true; } catch { }
        if (flow == Handshake.None || flow == Handshake.XOnXOff)
        {
            try { _port.RtsEnable = true; } catch { }
        }

        // 專用執行緒直接 blocking read（比 DataReceived 事件即時、無延遲）
        _reader = new Thread(ReadLoop) { IsBackground = true, Name = "COM-read" };
        _reader.Start();
    }

    private void ReadLoop()
    {
        var stream = _port?.BaseStream;
        if (stream == null) return;
        var buffer = new byte[8192];
        try
        {
            while (!_disposed)
            {
                int n = stream.Read(buffer, 0, buffer.Length); // 阻塞到至少 1 byte
                if (n <= 0) break;
                var chunk = new byte[n];
                Buffer.BlockCopy(buffer, 0, chunk, 0, n);
                Output?.Invoke(chunk);
            }
        }
        catch { /* 關閉 port 時 Read 會丟例外 → 正常結束 */ }
        finally
        {
            // 非使用者關閉（拔線／裝置消失）也要發 Exited，自動重連才會接手；
            // 使用者關閉走 Dispose（_disposed=true），由 Dispose 自己發。
            if (!_disposed) { try { Exited?.Invoke(); } catch { } }
        }
    }

    public void Write(ReadOnlySpan<byte> data)
    {
        try
        {
            var arr = data.ToArray();
            _port?.Write(arr, 0, arr.Length);
        }
        catch { }
    }

    public void WriteText(string text) => Write(Encoding.UTF8.GetBytes(text));

    public void Resize(int cols, int rows) { }

    /// <summary>清畫面時對 COM 送的 reset。TODO：待使用者定義要送什麼（目前不送）。</summary>
    public void SendReset()
    {
        // 待定義：可能是切換 DTR、送 break、或送固定字串。
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        try
        {
            if (_port != null)
            {
                if (_port.IsOpen) _port.Close(); // 會讓 blocking Read 丟例外而結束讀取迴圈
                _port.Dispose();
            }
        }
        catch { }
        try { Exited?.Invoke(); } catch { }
    }
}
