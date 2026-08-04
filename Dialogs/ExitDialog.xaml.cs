using System.Threading.Tasks;
using System.Windows;
using AwayTerminal.Localization;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

public partial class ExitDialog : Window
{
    public bool RestoreTabs => RestoreCheck.IsChecked == true;
    public bool UpdateMd => UpdateMdCheck.IsChecked == true;

    /// <summary>更新 CLAUDE.md 的實作（由 MainWindow 提供）。</summary>
    public Func<Task>? UpdateAction { get; set; }

    private bool _hasClaude;

    public ExitDialog()
    {
        InitializeComponent();
        Title = Loc.T("exit.title");
        MsgText.Text = Loc.T("exit.msg");
        RestoreCheck.Content = Loc.T("exit.restore");
        UpdateMdCheck.Content = Loc.T("exit.updateMd");
        BusyText.Text = Loc.T("exit.updating");
        OkBtn.Content = Loc.T("exit.ok");
        CancelBtn.Content = Loc.T("common.cancel");

        // 還原上次關閉時兩個勾選的狀態
        RestoreCheck.IsChecked = AppSettings.Current.ExitRestoreTabs;
        UpdateMdCheck.IsChecked = AppSettings.Current.ExitUpdateMd;
    }

    /// <summary>沒有 Claude Code 分頁時，停用「更新 CLAUDE.md」選項。</summary>
    public void SetClaudeAvailable(bool available)
    {
        _hasClaude = available;
        UpdateMdCheck.IsEnabled = available;
        if (!available) UpdateMdCheck.IsChecked = false;
    }

    private async void Ok_Click(object sender, RoutedEventArgs e)
    {
        if (UpdateMd && _hasClaude && UpdateAction != null)
        {
            // 進入等待狀態：顯示進度條、鎖住按鈕
            BusyPanel.Visibility = Visibility.Visible;
            OkBtn.IsEnabled = false;
            CancelBtn.IsEnabled = false;
            RestoreCheck.IsEnabled = false;
            UpdateMdCheck.IsEnabled = false;
            try { await UpdateAction(); } catch { }
        }
        // 等待期間使用者可能按標題列 X 關掉本視窗（Cancel 鈕雖停用、X 沒擋）——
        // 視窗已離開 ShowDialog 狀態時設 DialogResult 會丟 InvalidOperationException，
        // 在 async void 裡等於整個程式 crash，所以先確認還開著。
        if (IsVisible) DialogResult = true;
    }
}
