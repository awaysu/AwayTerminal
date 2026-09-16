using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;

namespace AwayTerminal.Dialogs;

public partial class InputDialog : Window
{
    public string Value => Input.Text;

    /// <param name="multiline">多行模式（AI 聊天室的主題／插話：一段話往往好幾行）：
    /// 輸入框可換行、可捲動、視窗可調大小，Enter＝換行、**Ctrl+Enter＝確定**（同 ComposeDialog）。</param>
    public InputDialog(string title, string prompt, string initial, bool multiline = false)
    {
        InitializeComponent();
        Title = title;
        PromptText.Text = prompt;
        Input.Text = initial;
        OkBtn.Content = Localization.Loc.T("common.ok");
        CancelBtn.Content = Localization.Loc.T("common.cancel");
        if (multiline)
        {
            SizeToContent = SizeToContent.Manual;   // 高度固定、可拉大（輸入框那一列是 *，會跟著長高）
            Width = 560;
            Height = 380;
            MinWidth = 380;
            MinHeight = 260;
            ResizeMode = ResizeMode.CanResize;
            Input.AcceptsReturn = true;
            Input.TextWrapping = TextWrapping.Wrap;
            Input.VerticalScrollBarVisibility = ScrollBarVisibility.Auto;
            Input.MinHeight = 120;
            Input.VerticalContentAlignment = VerticalAlignment.Top;
            OkBtn.IsDefault = false;              // Enter 要留給換行
            HintText.Text = Localization.Loc.T("common.ctrlEnter");
            HintText.Visibility = Visibility.Visible;
            Input.PreviewKeyDown += (_, e) =>
            {
                if (e.Key == Key.Enter && (Keyboard.Modifiers & ModifierKeys.Control) == ModifierKeys.Control)
                { e.Handled = true; DialogResult = true; }
            };
            Loaded += (_, _) => { Input.CaretIndex = Input.Text.Length; Input.Focus(); };
            return;
        }
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
