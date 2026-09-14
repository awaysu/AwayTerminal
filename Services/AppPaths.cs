namespace AwayTerminal.Services;

/// <summary>
/// 本機資料目錄（1.2.0，源自 1.1.11 協作版）：預設 %LOCALAPPDATA%\AwayTerminal（settings.json、restore\、WebView2\、diag.log、multiagent\）。
/// <para>測試模式：環境變數 <c>AWAYTERMINAL_DATA_DIR</c> 指定另一個資料夾 → 開發版可以跟使用者正在用的 AwayTerminal
/// 同時開著而互不干擾（設定／恢復分頁／WebView2 快取全部分開）。測試模式下也不登錄檔案總管右鍵選單、不開實例間管線、
/// 不啟動 Telegram 遠端（這三個都是「整台電腦只能有一份」的東西），視窗標題加 [TEST]。
/// 註：<c>Environment.GetFolderPath(LocalApplicationData)</c> 走 shell folder API、不讀 LOCALAPPDATA 環境變數，
/// 所以要有這個專用開關才隔離得了。</para>
/// </summary>
internal static class AppPaths
{
    private static readonly string? Override = Environment.GetEnvironmentVariable("AWAYTERMINAL_DATA_DIR");

    /// <summary>測試模式（有設 AWAYTERMINAL_DATA_DIR）。</summary>
    public static bool IsTestMode => !string.IsNullOrWhiteSpace(Override);

    public static string DataDir { get; } = IsTestMode
        ? System.IO.Path.GetFullPath(Override!.Trim())
        : System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "AwayTerminal");
}
