using Microsoft.Win32.SafeHandles;

namespace AwayTerminal.ConPty;

/// <summary>ConPTY 虛擬主控台（HPCON）。1.1.3 起後端二選一：exe 旁 <c>conpty\conpty.dll</c>（Windows Terminal 的
/// 新版 OpenConsole，見 <see cref="ConptyDll"/>）有就用它、沒有就走 kernel32 內建 conhost。</summary>
internal sealed class PseudoConsole : IDisposable
{
    public IntPtr Handle { get; }
    private readonly bool _dll;

    private PseudoConsole(IntPtr handle, bool dll) { Handle = handle; _dll = dll; }

    /// <summary>目前這條 pseudoconsole 是不是跑在新版 OpenConsole 上。</summary>
    public bool IsOpenConsole => _dll;

    public static PseudoConsole Create(SafeFileHandle inputReadSide, SafeFileHandle outputWriteSide, int cols, int rows)
    {
        var size = new COORD { X = (short)cols, Y = (short)rows };
        int hr;
        IntPtr hPC;
        bool dll = ConptyDll.Available;
        if (dll)
            hr = ConptyDll.Create(size, inputReadSide.DangerousGetHandle(), outputWriteSide.DangerousGetHandle(), 0, out hPC);
        else
            hr = NativeMethods.CreatePseudoConsole(size, inputReadSide, outputWriteSide, 0, out hPC);
        if (hr != 0)
            throw new InvalidOperationException($"CreatePseudoConsole 失敗 (HRESULT 0x{hr:X8}{(dll ? ", conpty.dll" : "")})");
        return new PseudoConsole(hPC, dll);
    }

    /// <summary>子行程掛上之後放掉參考 handle（只有 conpty.dll 後端需要；client 全離開時 OpenConsole 才會自己結束）。</summary>
    public void Release()
    {
        if (_dll && Handle != IntPtr.Zero) ConptyDll.Release(Handle);
    }

    public void Resize(int cols, int rows)
    {
        var size = new COORD { X = (short)cols, Y = (short)rows };
        if (_dll) ConptyDll.Resize(Handle, size);
        else NativeMethods.ResizePseudoConsole(Handle, size);
    }

    public void Dispose()
    {
        if (Handle == IntPtr.Zero) return;
        if (_dll) ConptyDll.Close(Handle);
        else NativeMethods.ClosePseudoConsole(Handle);
    }
}
