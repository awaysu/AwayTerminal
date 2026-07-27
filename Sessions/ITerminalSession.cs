namespace AwayTerminal.Sessions;

/// <summary>終端機連線抽象：ConPTY(本機/SSH)、Telnet(TCP)、序列埠 共用。</summary>
public interface ITerminalSession : IDisposable
{
    /// <summary>原始位元組輸出（背景執行緒觸發）。</summary>
    event Action<byte[]>? Output;

    /// <summary>連線結束時觸發。</summary>
    event Action? Exited;

    /// <summary>行程 PID；非行程型連線(Telnet/序列埠)回傳 0。</summary>
    int ProcessId { get; }

    void Write(ReadOnlySpan<byte> data);
    void WriteText(string text);
    void Resize(int cols, int rows);
}
