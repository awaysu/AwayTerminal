using System.IO;
using System.Linq;
using System.Windows;
using AwayTerminal.Localization;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

public partial class LogDialog : Window
{
    public string LogPath { get; private set; } = "";
    public bool Timestamp { get; private set; }
    public bool Append { get; private set; }

    public LogDialog(string tabTitle)
    {
        InitializeComponent();
        Title = Loc.T("dlg.logTitle");
        PathLabel.Text = Loc.T("log.path");
        BrowseBtn.Content = Loc.T("log.browse");
        TsCheck.Content = Loc.T("log.timestamp");
        AppendCheck.Content = Loc.T("log.append");
        StartBtn.Content = Loc.T("log.start");
        CancelBtn.Content = Loc.T("common.cancel");
        var s = AppSettings.Current;

        string safe = new string((tabTitle ?? "log").Select(c => Path.GetInvalidFileNameChars().Contains(c) ? '_' : c).ToArray());
        string file = $"{safe}_{DateTime.Now:yyyyMMdd_HHmmss}.log";
        PathBox.Text = Path.Combine(s.LogDir, file);
        TsCheck.IsChecked = s.LogTimestamp;
        AppendCheck.IsChecked = s.LogAppend;
    }

    private void Browse_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new Microsoft.Win32.SaveFileDialog
        {
            Filter = Loc.Lang == "en" ? "Log file (*.log)|*.log|All files (*.*)|*.*" : "Log 檔 (*.log)|*.log|所有檔案 (*.*)|*.*",
            FileName = Path.GetFileName(PathBox.Text),
            DefaultExt = ".log"
        };
        try
        {
            var dir = Path.GetDirectoryName(PathBox.Text);
            if (!string.IsNullOrEmpty(dir) && Directory.Exists(dir)) dlg.InitialDirectory = dir;
        }
        catch { }
        if (dlg.ShowDialog(this) == true) PathBox.Text = dlg.FileName;
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        string path = PathBox.Text?.Trim() ?? "";
        if (string.IsNullOrWhiteSpace(path))
        {
            MessageBox.Show(this, Loc.T("log.needPath"), "AwayTerminal", MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }
        LogPath = path;
        Timestamp = TsCheck.IsChecked == true;
        Append = AppendCheck.IsChecked == true;

        var s = AppSettings.Current;
        try { var d = Path.GetDirectoryName(path); if (!string.IsNullOrEmpty(d)) s.LogDir = d; } catch { }
        s.LogTimestamp = Timestamp;
        s.LogAppend = Append;
        s.Save();

        DialogResult = true;
    }
}
