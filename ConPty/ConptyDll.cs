using System.IO;
using System.Runtime.InteropServices;

namespace AwayTerminal.ConPty;

/// <summary>
/// 1.1.3：可選用 Windows Terminal 的新版 ConPTY 主機（`conpty.dll` + `OpenConsole.exe`，MIT、微軟簽章；
/// node-pty / VS Code 也是這樣隨附），取代 Win10 內建的 2019 年 conhost.exe。
/// 放在 exe 旁的 <c>conpty\</c> 子目錄（conpty.dll 會在自己旁邊找 OpenConsole.exe；找不到就退回 system32 conhost）。
/// 沒有這兩個檔＝維持 kernel32 的 CreatePseudoConsole，行為與 1.1.2 以前相同。
/// 測試／除錯：環境變數 <c>AWAYTERMINAL_CONPTY_DIR</c> 指定目錄、<c>AWAYTERMINAL_CONPTY=inbox</c> 強制用內建 conhost。
/// </summary>
internal static class ConptyDll
{
    private delegate int CreateFn(COORD size, IntPtr hInput, IntPtr hOutput, uint dwFlags, out IntPtr phPC);
    private delegate int ResizeFn(IntPtr hPC, COORD size);
    private delegate void ClosePcFn(IntPtr hPC);
    private delegate void ReleaseFn(IntPtr hPC);

    private static readonly CreateFn? _create;
    private static readonly ResizeFn? _resize;
    private static readonly ClosePcFn? _close;
    private static readonly ReleaseFn? _release;

    /// <summary>已載入新版 conpty.dll（且 OpenConsole.exe 在旁）。</summary>
    public static bool Available => _create != null;

    /// <summary>實際使用的目錄（診斷用）；null＝內建 conhost。</summary>
    public static string? Dir { get; }

    static ConptyDll()
    {
        try
        {
            if (string.Equals(Environment.GetEnvironmentVariable("AWAYTERMINAL_CONPTY"), "inbox", StringComparison.OrdinalIgnoreCase))
                return;
            string dir = Environment.GetEnvironmentVariable("AWAYTERMINAL_CONPTY_DIR")
                         ?? Path.Combine(AppContext.BaseDirectory, "conpty");
            string dll = Path.Combine(dir, "conpty.dll");
            if (!File.Exists(dll) || !File.Exists(Path.Combine(dir, "OpenConsole.exe"))) return;
            IntPtr lib = NativeLibrary.Load(dll);
            _create = Marshal.GetDelegateForFunctionPointer<CreateFn>(NativeLibrary.GetExport(lib, "ConptyCreatePseudoConsole"));
            _resize = Marshal.GetDelegateForFunctionPointer<ResizeFn>(NativeLibrary.GetExport(lib, "ConptyResizePseudoConsole"));
            _close = Marshal.GetDelegateForFunctionPointer<ClosePcFn>(NativeLibrary.GetExport(lib, "ConptyClosePseudoConsole"));
            _release = Marshal.GetDelegateForFunctionPointer<ReleaseFn>(NativeLibrary.GetExport(lib, "ConptyReleasePseudoConsole"));
            Dir = dir;
        }
        catch
        {
            _create = null; _resize = null; _close = null; _release = null; Dir = null;
        }
    }

    public static int Create(COORD size, IntPtr hInput, IntPtr hOutput, uint flags, out IntPtr hPC)
        => _create!(size, hInput, hOutput, flags, out hPC);

    public static int Resize(IntPtr hPC, COORD size) => _resize!(hPC, size);

    public static void Close(IntPtr hPC) => _close!(hPC);

    /// <summary>子行程掛上 pseudoconsole 之後呼叫：放掉 conpty.dll 持有的參考 handle，
    /// 之後最後一個 client 離開時 OpenConsole 會自己結束並關閉輸出管線（內建 conhost 不會，見 ConPtySession.Exited）。</summary>
    public static void Release(IntPtr hPC) => _release!(hPC);
}
