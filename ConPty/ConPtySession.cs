using System.IO;
using System.Text;
using AwayTerminal.Sessions;

namespace AwayTerminal.ConPty;

/// <summary>
/// 一個 ConPTY 連線：啟動子行程（如 powershell.exe / ssh.exe），
/// 提供輸出事件、輸入寫入、尺寸調整。
/// </summary>
public sealed class ConPtySession : ITerminalSession
{
    private PseudoConsolePipe? _inputPipe;
    private PseudoConsolePipe? _outputPipe;
    private PseudoConsole? _pty;
    private PROCESS_INFORMATION _procInfo;
    private FileStream? _writeStream;
    private FileStream? _readStream;
    private readonly CancellationTokenSource _cts = new();
    private bool _disposed;
    private int _exitRaised;                      // Exited 只發一次（行程等待與管線 EOF 兩條路都可能到）
    private RegisteredWaitHandle? _procWait;      // 行程結束等待（threadpool 登記，不佔執行緒）
    private ManualResetEvent? _procEvent;         // 包住 hProcess 的 WaitHandle（不擁有 handle）

    /// <summary>原始 UTF-8 位元組輸出（於背景執行緒觸發）。</summary>
    public event Action<byte[]>? Output;

    /// <summary>子行程結束時觸發（於背景執行緒）。
    /// 1.0.30 起以「等待行程 handle」偵測——ConPTY 的 conhost 在子行程結束後**不會**關輸出管線
    /// （要等我們 ClosePseudoConsole），所以光靠 ReadLoop 的 EOF 永遠等不到；實測 claude 按 Ctrl+C
    /// 離開、powershell 打 exit 後分頁都毫無反應，就是這個原因（scratchpad ptyprobe 2026-08-20 驗證）。</summary>
    public event Action? Exited;

    public int ProcessId => _procInfo.dwProcessId;

    /// <summary>關閉前送出的「優雅結束」位元組（一次全送出）。
    /// 預設 Ctrl+C ×3（claude 離開方式）；SSH 建議設為 Ctrl+D ×2（登出遠端 shell）。</summary>
    public byte[] GracefulExitBytes { get; set; } = { 0x03, 0x03, 0x03 };

    public void Start(string command, int cols, int rows, string? workingDirectory = null)
    {
        if (cols < 1) cols = 80;
        if (rows < 1) rows = 24;

        _inputPipe = new PseudoConsolePipe();
        _outputPipe = new PseudoConsolePipe();
        _pty = PseudoConsole.Create(_inputPipe.ReadSide, _outputPipe.WriteSide, cols, rows);
        _procInfo = ProcessFactory.Start(command, _pty.Handle, workingDirectory);

        // 我方保留：輸入「寫端」（送鍵盤）、輸出「讀端」（收畫面）
        _writeStream = new FileStream(_inputPipe.WriteSide, FileAccess.Write);
        _readStream = new FileStream(_outputPipe.ReadSide, FileAccess.Read);

        // 另兩端已由 pseudoconsole 複製走，釋放父行程持有的副本
        _inputPipe.ReadSide.Dispose();
        _outputPipe.WriteSide.Dispose();

        _ = Task.Run(ReadLoopAsync);

        // 行程結束偵測（見 Exited 註解）。handle 由 Dispose 的 CloseHandle 負責，這裡不擁有；
        // Dispose 會先 Unregister 再關 handle。觸發後稍等，讓 conhost 把最後一批輸出送完再通知。
        _procEvent = new ManualResetEvent(false);
        _procEvent.SafeWaitHandle = new Microsoft.Win32.SafeHandles.SafeWaitHandle(_procInfo.hProcess, ownsHandle: false);
        _procWait = ThreadPool.RegisterWaitForSingleObject(_procEvent, (_, _) =>
        {
            if (_disposed) return;
            Thread.Sleep(150);
            RaiseExited();
        }, null, Timeout.Infinite, executeOnlyOnce: true);
    }

    private void RaiseExited()
    {
        if (_disposed) return;
        if (Interlocked.Exchange(ref _exitRaised, 1) != 0) return;
        try { Exited?.Invoke(); } catch { }
    }

    private async Task ReadLoopAsync()
    {
        var buffer = new byte[65536];
        try
        {
            while (!_cts.IsCancellationRequested && _readStream != null)
            {
                int n = await _readStream.ReadAsync(buffer.AsMemory(0, buffer.Length), _cts.Token).ConfigureAwait(false);
                if (n <= 0) break;
                var chunk = new byte[n];
                Buffer.BlockCopy(buffer, 0, chunk, 0, n);
                Output?.Invoke(chunk);
            }
        }
        catch (OperationCanceledException) { }
        catch (IOException) { }
        catch (ObjectDisposedException) { }
        finally
        {
            RaiseExited();   // 管線真的關了（例如 conhost 自己掛掉）也算結束；已發過就不重發
        }
    }

    public void Write(ReadOnlySpan<byte> data)
    {
        if (_writeStream == null) return;
        try
        {
            _writeStream.Write(data);
            _writeStream.Flush();
        }
        catch (IOException) { }
        catch (ObjectDisposedException) { }
    }

    public void WriteText(string text) => Write(Encoding.UTF8.GetBytes(text));

    public void Resize(int cols, int rows)
    {
        if (cols < 1 || rows < 1) return;
        _pty?.Resize(cols, rows);
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        // 先取消行程等待（之後 TerminateProcess 不會被當成「自然結束」發 Exited），handle 稍後才關
        try { _procWait?.Unregister(null); } catch { }
        _procWait = null;

        // 先一次送出「優雅結束」按鍵（PowerShell/claude=Ctrl+C×3、SSH=Ctrl+D×2），
        // 只短暫等待就往下強制收尾（快）。此方法在背景執行緒呼叫，不卡 UI。
        try
        {
            if (_writeStream != null && GracefulExitBytes.Length > 0)
            {
                _writeStream.Write(GracefulExitBytes, 0, GracefulExitBytes.Length);
                _writeStream.Flush();
                Thread.Sleep(60);
            }
        }
        catch { }

        try { _cts.Cancel(); } catch { }
        // 先終止子行程，ClosePseudoConsole 與管線關閉才會立即返回（否則會阻塞）
        try { if (_procInfo.hProcess != IntPtr.Zero) NativeMethods.TerminateProcess(_procInfo.hProcess, 0); } catch { }
        try { _pty?.Dispose(); } catch { }
        try { _writeStream?.Dispose(); } catch { }
        try { _readStream?.Dispose(); } catch { }

        try
        {
            if (_procInfo.hThread != IntPtr.Zero) NativeMethods.CloseHandle(_procInfo.hThread);
            if (_procInfo.hProcess != IntPtr.Zero) NativeMethods.CloseHandle(_procInfo.hProcess);
        }
        catch { }

        try { _inputPipe?.Dispose(); } catch { }
        try { _outputPipe?.Dispose(); } catch { }
        try { _procEvent?.Dispose(); } catch { }   // 不擁有 hProcess，Dispose 只釋放包裝
        _cts.Dispose();
    }
}
