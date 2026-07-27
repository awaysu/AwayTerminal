using Microsoft.Win32.SafeHandles;

namespace AwayTerminal.ConPty;

/// <summary>ConPTY 虛擬主控台（HPCON）。</summary>
internal sealed class PseudoConsole : IDisposable
{
    public IntPtr Handle { get; }

    private PseudoConsole(IntPtr handle) => Handle = handle;

    public static PseudoConsole Create(SafeFileHandle inputReadSide, SafeFileHandle outputWriteSide, int cols, int rows)
    {
        var size = new COORD { X = (short)cols, Y = (short)rows };
        int hr = NativeMethods.CreatePseudoConsole(size, inputReadSide, outputWriteSide, 0, out IntPtr hPC);
        if (hr != 0)
            throw new InvalidOperationException($"CreatePseudoConsole 失敗 (HRESULT 0x{hr:X8})");
        return new PseudoConsole(hPC);
    }

    public void Resize(int cols, int rows)
    {
        var size = new COORD { X = (short)cols, Y = (short)rows };
        NativeMethods.ResizePseudoConsole(Handle, size);
    }

    public void Dispose()
    {
        if (Handle != IntPtr.Zero)
            NativeMethods.ClosePseudoConsole(Handle);
    }
}
