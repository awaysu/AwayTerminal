using System.IO.Ports;
using System.Linq;
using System.Windows;
using AwayTerminal.Localization;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

public partial class ComDialog : Window
{
    public string PortName { get; private set; } = "COM5";
    public int Baud { get; private set; } = 115200;
    public int DataBits { get; private set; } = 8;
    public Parity Parity { get; private set; } = Parity.None;
    public StopBits StopBits { get; private set; } = StopBits.One;
    public Handshake Handshake { get; private set; } = Handshake.None;

    private static readonly int[] Bauds = { 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600 };

    public ComDialog()
    {
        InitializeComponent();
        Title = Loc.T("com.title");
        ResetBtn.Content = Loc.T("common.reset");
        OpenBtn.Content = Loc.T("com.open");
        CancelBtn.Content = Loc.T("common.cancel");
        ReconnectBox.Content = Loc.T("conn.autoReconnect");
        var s = AppSettings.Current;
        ReconnectBox.IsChecked = s.AutoReconnect;

        // Port：列出實際存在的 COM，並保證含設定裡的預設值
        var ports = SerialPort.GetPortNames().OrderBy(p => p).ToList();
        if (!ports.Contains(s.ComPort)) ports.Insert(0, s.ComPort);
        PortCombo.ItemsSource = ports;
        PortCombo.Text = s.ComPort;

        foreach (var b in Bauds) BaudCombo.Items.Add(b);
        BaudCombo.Text = s.ComBaud.ToString();

        foreach (var d in new[] { "5", "6", "7", "8" }) DataCombo.Items.Add(d);
        DataCombo.SelectedItem = s.ComDataBits.ToString();
        if (DataCombo.SelectedItem == null) DataCombo.SelectedItem = "8";

        foreach (var p in Enum.GetNames<Parity>()) ParityCombo.Items.Add(p);
        ParityCombo.SelectedItem = s.ComParity;
        if (ParityCombo.SelectedItem == null) ParityCombo.SelectedItem = "None";

        // 顯示友善名稱，Tag = 列舉名
        AddTagged(StopCombo, ("1", "One"), ("1.5", "OnePointFive"), ("2", "Two"));
        SelectByTag(StopCombo, s.ComStopBits, "One");

        AddTagged(FlowCombo, ("None", "None"), ("XON/XOFF", "XOnXOff"),
            ("RTS/CTS", "RequestToSend"), ("RTS/CTS+XON/XOFF", "RequestToSendXOnXOff"));
        SelectByTag(FlowCombo, s.ComFlow, "None");
    }

    private static void AddTagged(System.Windows.Controls.ComboBox cb, params (string disp, string tag)[] items)
    {
        foreach (var (disp, tag) in items)
            cb.Items.Add(new System.Windows.Controls.ComboBoxItem { Content = disp, Tag = tag });
    }

    private static void SelectByTag(System.Windows.Controls.ComboBox cb, string tag, string fallback)
    {
        foreach (System.Windows.Controls.ComboBoxItem it in cb.Items)
        {
            if ((string?)it.Tag == tag) { cb.SelectedItem = it; return; }
        }
        foreach (System.Windows.Controls.ComboBoxItem it in cb.Items)
        {
            if ((string?)it.Tag == fallback) { cb.SelectedItem = it; return; }
        }
    }

    private static string TagOf(System.Windows.Controls.ComboBox cb, string fallback)
        => (cb.SelectedItem as System.Windows.Controls.ComboBoxItem)?.Tag as string ?? fallback;

    private void Reset_Click(object sender, RoutedEventArgs e)
    {
        // 回到第一次的預設值：COM5 / 115200 / 8 / None / 1 / None
        PortCombo.Text = "COM5";
        BaudCombo.Text = "115200";
        DataCombo.SelectedItem = "8";
        ParityCombo.SelectedItem = "None";
        SelectByTag(StopCombo, "One", "One");
        SelectByTag(FlowCombo, "None", "None");
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        PortName = string.IsNullOrWhiteSpace(PortCombo.Text) ? "COM1" : PortCombo.Text.Trim();
        Baud = int.TryParse(BaudCombo.Text, out int b) ? b : 115200;
        DataBits = int.TryParse(DataCombo.SelectedItem as string, out int di) ? di : 8;
        Parity = Enum.TryParse<Parity>(ParityCombo.SelectedItem as string, out var pa) ? pa : Parity.None;
        StopBits = Enum.TryParse<StopBits>(TagOf(StopCombo, "One"), out var sb) ? sb : StopBits.One;
        Handshake = Enum.TryParse<Handshake>(TagOf(FlowCombo, "None"), out var hs) ? hs : Handshake.None;

        var s = AppSettings.Current;
        s.AutoReconnect = ReconnectBox.IsChecked == true;
        s.ComPort = PortName; s.ComBaud = Baud; s.ComDataBits = DataBits;
        s.ComParity = Parity.ToString(); s.ComStopBits = StopBits.ToString(); s.ComFlow = Handshake.ToString();
        s.Save();

        DialogResult = true;
    }
}
