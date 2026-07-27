using System.Windows;
using AwayTerminal.Localization;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

public partial class ConnectDialog : Window
{
    /// <summary>ssh 或 telnet。</summary>
    public string ConnType { get; private set; } = "ssh";
    public string Host { get; private set; } = "";
    public int Port { get; private set; } = 22;

    private bool _init;

    public ConnectDialog()
    {
        InitializeComponent();
        Title = Loc.T("conn.title");
        TypeLabel.Text = Loc.T("conn.type");
        HostLabel.Text = Loc.T("conn.host");
        ConnectBtn.Content = Loc.T("conn.connect");
        CancelBtn.Content = Loc.T("common.cancel");
        ReconnectBox.Content = Loc.T("conn.autoReconnect");
        KeepAliveLabel.Text = Loc.T("conn.keepAlive");
        var s = AppSettings.Current;
        ReconnectBox.IsChecked = s.AutoReconnect;
        foreach (var m in new[] { 0, 1, 3, 5, 10, 15, 30, 60 }) KeepAliveCombo.Items.Add(m);
        KeepAliveCombo.SelectedItem = KeepAliveCombo.Items.Contains(s.KeepAliveMins) ? s.KeepAliveMins : 10;

        TypeCombo.Items.Add("SSH");
        TypeCombo.Items.Add("Telnet");
        TypeCombo.SelectedItem = s.LastConnType == "telnet" ? "Telnet" : "SSH";

        HostCombo.ItemsSource = s.HostHistory;
        HostCombo.Text = s.LastHost;
        PortBox.Text = (s.LastConnType == "telnet" ? s.LastTelnetPort : s.LastSshPort).ToString();

        _init = true;
    }

    private bool IsTelnet => (TypeCombo.SelectedItem as string) == "Telnet";

    private void TypeCombo_Changed(object sender, System.Windows.Controls.SelectionChangedEventArgs e)
    {
        if (!_init) return;
        if (int.TryParse(PortBox.Text, out int cur))
        {
            if (IsTelnet && cur == 22) PortBox.Text = "23";
            else if (!IsTelnet && cur == 23) PortBox.Text = "22";
        }
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        string host = HostCombo.Text?.Trim() ?? "";
        if (string.IsNullOrWhiteSpace(host))
        {
            MessageBox.Show(this, Loc.T("conn.needHost"), "AwayTerminal", MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }
        int port = int.TryParse(PortBox.Text, out int p) ? p : (IsTelnet ? 23 : 22);

        ConnType = IsTelnet ? "telnet" : "ssh";
        Host = host;
        Port = port;

        var s = AppSettings.Current;
        s.AutoReconnect = ReconnectBox.IsChecked == true;
        s.KeepAliveMins = KeepAliveCombo.SelectedItem is int ka ? ka : 10;
        s.LastConnType = ConnType;
        s.LastHost = host;
        if (ConnType == "telnet") s.LastTelnetPort = port; else s.LastSshPort = port;
        s.HostHistory.Remove(host);
        s.HostHistory.Insert(0, host);
        if (s.HostHistory.Count > 20) s.HostHistory.RemoveRange(20, s.HostHistory.Count - 20);
        s.Save();

        DialogResult = true;
    }
}
