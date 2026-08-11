using System.Windows;
using AwayTerminal.Localization;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

public partial class RemoteDialog : Window
{
    public RemoteDialog()
    {
        InitializeComponent();
        var s = AppSettings.Current;
        EnableChk.IsChecked = s.RemoteEnabled;
        TokenBox.Text = s.TelegramBotToken;
        ChatBox.Text = s.TelegramChatId == 0 ? "" : s.TelegramChatId.ToString();
        NotifyChk.IsChecked = s.RemoteNotify;

        Title = Loc.T("remote.title");
        TakenHint.Text = Loc.T("remote.takenByOther");
        TakenHint.Visibility = AwayTerminal.MainWindow.RemoteTakenByOther ? Visibility.Visible : Visibility.Collapsed;
        EnableChk.Content = Loc.T("remote.enable");
        TokenLabel.Text = Loc.T("remote.token");
        ChatLabel.Text = Loc.T("remote.chatId");
        GetIdBtn.Content = Loc.T("remote.getChatId");
        NotifyChk.Content = Loc.T("remote.notify");
        HintText.Text = Loc.T("remote.hint");
        OkBtn.Content = Loc.T("common.save");
        CancelBtn.Content = Loc.T("common.cancel");

        // 未勾「啟用」時下面的設定全部鎖住（灰色不可改）
        EnableChk.Checked += (_, _) => UpdateEnabledUI();
        EnableChk.Unchecked += (_, _) => UpdateEnabledUI();
        UpdateEnabledUI();
    }

    private void UpdateEnabledUI()
    {
        bool on = EnableChk.IsChecked == true;
        TokenBox.IsEnabled = ChatBox.IsEnabled = GetIdBtn.IsEnabled = NotifyChk.IsEnabled = on;
        // TextBlock 不是 Control、IsEnabled 不會讓它變灰；CheckBox 的深色前景是寫死的 #E0E0E0，
        // 停用觸發器也蓋不掉 → 一律用透明度呈現停用感
        double op = on ? 1.0 : 0.45;
        TokenLabel.Opacity = ChatLabel.Opacity = HintText.Opacity = NotifyChk.Opacity = op;
    }

    private async void GetId_Click(object sender, RoutedEventArgs e)
    {
        string token = TokenBox.Text.Trim();
        if (string.IsNullOrEmpty(token)) { MessageBox.Show(this, Loc.T("remote.needToken"), "AwayTerminal"); return; }
        GetIdBtn.IsEnabled = false;
        try
        {
            var id = await TelegramRemote.TryGetLatestChatId(token);
            if (id == null) { MessageBox.Show(this, Loc.T("remote.noUpdates"), "AwayTerminal"); return; }
            ChatBox.Text = id.Value.ToString();
            MessageBox.Show(this, string.Format(Loc.T("remote.gotChatId"), id.Value), "AwayTerminal");
        }
        finally { GetIdBtn.IsEnabled = EnableChk.IsChecked == true; } // 查詢期間若取消勾選，仍要維持灰色
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        var s = AppSettings.Current;
        s.RemoteEnabled = EnableChk.IsChecked == true;
        s.TelegramBotToken = TokenBox.Text.Trim();
        s.TelegramChatId = long.TryParse(ChatBox.Text.Trim(), out long id) ? id : 0;
        s.RemoteNotify = NotifyChk.IsChecked == true;
        s.Save();
        DialogResult = true;
    }
}
