using System.Windows;

namespace AwayTerminal.Dialogs;

public partial class InputDialog : Window
{
    public string Value => Input.Text;

    public InputDialog(string title, string prompt, string initial)
    {
        InitializeComponent();
        Title = title;
        PromptText.Text = prompt;
        Input.Text = initial;
        OkBtn.Content = Localization.Loc.T("common.ok");
        CancelBtn.Content = Localization.Loc.T("common.cancel");
        Loaded += (_, _) => { Input.SelectAll(); Input.Focus(); };
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        DialogResult = true;
    }

    /// <summary>顯示對話框；按確定回傳輸入字串，取消回傳 null。</summary>
    public static string? Show(Window owner, string title, string prompt, string initial = "")
    {
        var dlg = new InputDialog(title, prompt, initial) { Owner = owner };
        return dlg.ShowDialog() == true ? dlg.Value : null;
    }
}
