using System.IO;
using System.Windows;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

public partial class OpenDirDialog : Window
{
    public string SelectedDir { get; private set; } = "";

    public OpenDirDialog()
    {
        InitializeComponent();
        var s = AppSettings.Current;
        DirCombo.ItemsSource = s.DirBookmarks;
        DirCombo.Text = s.LastDir ?? "";
    }

    private void Add_Click(object sender, RoutedEventArgs e)
    {
        var d = new Microsoft.Win32.OpenFolderDialog { Title = "選擇資料夾" };
        if (!string.IsNullOrWhiteSpace(DirCombo.Text) && Directory.Exists(DirCombo.Text))
            d.InitialDirectory = DirCombo.Text;
        if (d.ShowDialog(this) == true)
        {
            var folder = d.FolderName;
            var s = AppSettings.Current;
            if (!s.DirBookmarks.Contains(folder))
            {
                s.DirBookmarks.Insert(0, folder);
                DirCombo.ItemsSource = null;
                DirCombo.ItemsSource = s.DirBookmarks;
            }
            DirCombo.Text = folder;
        }
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        string dir = DirCombo.Text?.Trim() ?? "";
        if (string.IsNullOrWhiteSpace(dir) || !Directory.Exists(dir))
        {
            MessageBox.Show(this, "目錄不存在，請重新選擇。", "AwayTerminal", MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }
        SelectedDir = dir;

        var s = AppSettings.Current;
        s.LastDir = dir;
        if (!s.DirBookmarks.Contains(dir)) s.DirBookmarks.Insert(0, dir);
        s.Save();

        DialogResult = true;
    }
}
