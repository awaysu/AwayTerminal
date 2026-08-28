using Microsoft.Win32;

namespace AwayTerminal.Services;

/// <summary>
/// 檔案總管右鍵選單「用 AwayTerminal 開啟」（1.0.45）。
/// 登錄在 HKCU\Software\Classes（只影響目前使用者、免管理員）：
///   Directory\shell\AwayTerminal            → 在資料夾上按右鍵
///   Directory\Background\shell\AwayTerminal → 在資料夾內空白處按右鍵
/// command = "AwayTerminal.exe" --open-dir "%V"（%V＝該資料夾；VS Code 同款寫法，兩種位置都適用）。
/// 每次啟動依 AppSettings.ExplorerMenu 重新套用：路徑永遠指向目前這支 exe（搬家／升級後自動更新），
/// 文字跟隨目前語言。MSIX 版寫的登錄檔會被虛擬化、檔案總管看不到（需 COM 擴充），此功能只對安裝檔／開發版有效。
/// 解除安裝：installer.iss 的 [UninstallRun] 用 reg.exe 刪掉這兩個 key。
/// </summary>
internal static class ShellIntegration
{
    private const string KeyName = "AwayTerminal";
    private static readonly string[] Roots =
    {
        @"Software\Classes\Directory\shell\",
        @"Software\Classes\Directory\Background\shell\",
    };

    /// <summary>目前執行檔完整路徑（apphost exe，不是 dll）。</summary>
    public static string ExePath => Environment.ProcessPath ?? "";

    /// <summary>依設定登錄或移除；任何失敗只記 diag.log，不影響啟動。</summary>
    public static void Apply(bool enable, string menuText)
    {
        try
        {
            if (enable) Register(menuText);
            else Unregister();
        }
        catch (Exception ex)
        {
            Diag.Log($"shell menu apply({enable}) failed: {ex.Message}");
        }
    }

    private static void Register(string menuText)
    {
        string exe = ExePath;
        if (string.IsNullOrEmpty(exe)) return;
        string command = $"\"{exe}\" --open-dir \"%V\"";
        foreach (var root in Roots)
        {
            using var k = Registry.CurrentUser.CreateSubKey(root + KeyName);
            if (k == null) continue;
            k.SetValue("", menuText);
            k.SetValue("Icon", $"\"{exe}\",0");
            using var c = k.CreateSubKey("command");
            c?.SetValue("", command);
        }
    }

    private static void Unregister()
    {
        foreach (var root in Roots)
        {
            try { Registry.CurrentUser.DeleteSubKeyTree(root + KeyName, throwOnMissingSubKey: false); } catch { }
        }
    }
}
