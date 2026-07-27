using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace AwayTerminal.ConPty;

/// <summary>一對匿名管線（讀端 / 寫端）。</summary>
internal sealed class PseudoConsolePipe : IDisposable
{
    public SafeFileHandle ReadSide { get; }
    public SafeFileHandle WriteSide { get; }

    public PseudoConsolePipe()
    {
        if (!NativeMethods.CreatePipe(out var read, out var write, IntPtr.Zero, 0))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "CreatePipe 失敗");
        ReadSide = read;
        WriteSide = write;
    }

    public void Dispose()
    {
        ReadSide?.Dispose();
        WriteSide?.Dispose();
    }
}
