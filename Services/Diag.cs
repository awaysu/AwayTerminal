namespace AwayTerminal.Services;

/// <summary>
/// 輕量診斷紀錄：專追「有時候才發生、事後採證不到」的問題（例如資料夾對話框沒蹦出來）。
/// 寫入 %LOCALAPPDATA%\AwayTerminal\diag.log；超過 256KB 砍掉前半保留後段；任何失敗一律吞掉。
/// </summary>
internal static class Diag
{
    private static readonly object Lock = new();
    private static readonly string LogPath =
        System.IO.Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "AwayTerminal", "diag.log");

    public static void Log(string msg)
    {
        try
        {
            lock (Lock)
            {
                System.IO.Directory.CreateDirectory(System.IO.Path.GetDirectoryName(LogPath)!);
                var fi = new System.IO.FileInfo(LogPath);
                if (fi.Exists && fi.Length > 256 * 1024)
                {
                    string all = System.IO.File.ReadAllText(LogPath);
                    System.IO.File.WriteAllText(LogPath, all[(all.Length / 2)..]);
                }
                System.IO.File.AppendAllText(LogPath,
                    $"[{DateTime.Now:yy-MM-dd HH:mm:ss.fff}] {msg}\r\n");
            }
        }
        catch { }
    }
}
