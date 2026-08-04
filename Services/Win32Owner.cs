namespace AwayTerminal.Services;

/// <summary>
/// 把 WPF 視窗包成 WinForms 的 <see cref="System.Windows.Forms.IWin32Window"/>，
/// 讓 WinForms 對話框（FolderBrowserDialog / OpenFileDialog / ColorDialog）有正確的擁有者。
///
/// 為什麼一定要傳 owner：不傳時 WinForms 會用 <c>GetActiveWindow()</c> 去猜擁有者，
/// 在 WPF 程式裡不一定猜得到主視窗——猜錯的話對話框會開在主視窗**後面**，
/// 使用者看到的現象就是「點了沒反應」。從 ContextMenu（例如 New 下拉）觸發時特別容易中，
/// 因為選單剛關閉的瞬間作用中視窗未必是主視窗。
/// </summary>
internal sealed class Win32Owner : System.Windows.Forms.IWin32Window
{
    private Win32Owner(IntPtr handle) => Handle = handle;

    public IntPtr Handle { get; }

    /// <summary>取得某個 WPF 視窗的擁有者包裝。</summary>
    public static Win32Owner Of(System.Windows.Window window) =>
        new(new System.Windows.Interop.WindowInteropHelper(window).Handle);
}
