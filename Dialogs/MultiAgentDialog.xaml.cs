using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using AwayTerminal.Localization;
using AwayTerminal.Models;
using AwayTerminal.Services;
using AwayTerminal.Services.MultiAgent;

namespace AwayTerminal.Dialogs;

/// <summary>設定視窗一格的選擇。</summary>
public sealed class AgentSlotSetup
{
    public bool Enabled { get; set; }
    public string Backend { get; set; } = "";
    public string Role { get; set; } = "";
}

/// <summary>設定視窗的結果（新開一組／既有組補開、重新啟動）。</summary>
public sealed class MultiAgentSetup
{
    public string Dir { get; set; } = "";
    public AgentSlotSetup[] Slots { get; set; } = { new(), new(), new(), new() };
    /// <summary>既有組：要（重新）啟動的格號 1～4（新勾啟用的、按了「重新啟動」的）。新開的組不用。</summary>
    public List<int> Launch { get; set; } = new();
}

/// <summary>
/// New → Multi-Agent 的設定視窗（1.2.0）。上方專案資料夾；四格（2×2）各有：啟用（格 1 一定啟用）、Agent ID（程式產生、不可改）、
/// Coding Agent（只列這台電腦找得到的：ClaudeCode／Codex／OpenCode／GeminiCLI）、Agent Role（None＋roles\*.md）。
/// 預設格 1、2 啟用（Product Manager／Software Engineer），格 3、4 停用（Software Architect／QA Engineer）；上次的選擇記在
/// AppSettings.MultiAgentLastSetup。<para>分頁右鍵「Multi-Agent 設定…」開同一個視窗：執行中的格唯讀、沒啟用的格可以勾起來補開、
/// 已結束的格可以改 CLI／角色後「重新啟動」。</para>
/// </summary>
public partial class MultiAgentDialog : Window
{
    private sealed record Choice(string Key, string Text, string? Icon);

    private sealed class SlotUi
    {
        public CheckBox Enable = null!;
        public TextBlock Id = null!;
        public ComboBox Backend = null!;
        public ComboBox Role = null!;
        public TextBlock Status = null!;
        public Button Restart = null!;
        public bool Locked;       // 既有組：執行中＝唯讀
        public bool Exited;       // 既有組：已結束
        public bool WantRestart;  // 已結束的格按了「重新啟動」
    }

    private readonly AgentGroup? _group;
    private readonly SlotUi[] _ui = new SlotUi[4];
    private readonly List<Choice> _backends;

    public MultiAgentSetup? Result { get; private set; }

    public MultiAgentDialog(string? dir, AgentGroup? existing)
    {
        InitializeComponent();
        _group = existing;

        Title = Loc.T("ma.title") + (existing != null ? $" — {ShortName(existing.Dir)}" : "");
        FolderLabel.Text = Loc.T("ma.dlgFolder");
        BrowseBtn.Content = Loc.T("log.browse");
        RestoreRolesBtn.Content = Loc.T("ma.dlgRestoreRoles");
        OpenRolesBtn.Content = Loc.T("ma.dlgOpenRoles");
        OkBtn.Content = Loc.T(existing == null ? "ma.dlgOpen" : "ma.dlgApply");
        CancelBtn.Content = Loc.T("ma.dlgCancel");
        HintText.Text = Loc.T("ma.dlgHint");

        // 這台電腦找得到的 Coding Agent（沿用自訂連線或自動偵測；見 ICodingAgentAdapter.Resolve）
        _backends = AdapterRegistry.All.Where(a => a.Resolve() != null)
            .Select(a => new Choice(a.Key, a.DisplayName, a.Key + ".png")).ToList();

        var saved = LoadLastSetup();
        if (existing != null)
        {
            FolderBox.Text = existing.Dir;
            FolderBox.IsReadOnly = true;
            BrowseBtn.IsEnabled = false;
        }
        else FolderBox.Text = dir ?? saved?.Dir ?? "";

        for (int i = 0; i < 4; i++) SlotGrid.Children.Add(BuildSlot(i, saved));
        FillRoles();
        for (int i = 0; i < 4; i++) ApplyInitial(i, saved);

        if (_backends.Count == 0 && existing == null)
        {
            NoBackendText.Text = Loc.T("ma.dlgNoBackend");
            NoBackendText.Visibility = Visibility.Visible;
            OkBtn.IsEnabled = false;
        }
    }

    private static string ShortName(string dir)
    {
        try { var n = Path.GetFileName(dir.TrimEnd('\\', '/')); return string.IsNullOrEmpty(n) ? dir : n; }
        catch { return dir; }
    }

    private static MultiAgentSetup? LoadLastSetup()
    {
        try
        {
            string json = AppSettings.Current.MultiAgentLastSetup;
            if (string.IsNullOrWhiteSpace(json)) return null;
            var s = JsonSerializer.Deserialize<MultiAgentSetup>(json);
            return s?.Slots is { Length: 4 } ? s : null;
        }
        catch { return null; }
    }

    // ---------- 一格的畫面 ----------
    private FrameworkElement BuildSlot(int i, MultiAgentSetup? saved)
    {
        var slotColor = (Color)ColorConverter.ConvertFromString(SlotColor(i + 1));
        var ui = _ui[i] = new SlotUi();
        var panel = new StackPanel();

        var head = new DockPanel { Margin = new Thickness(0, 0, 0, 8) };
        ui.Enable = new CheckBox { Content = Loc.T("ma.dlgEnable"), Margin = new Thickness(8, 0, 0, 0) };
        DockPanel.SetDock(ui.Enable, Dock.Right);
        head.Children.Add(ui.Enable);
        ui.Id = new TextBlock { FontSize = 15, FontWeight = FontWeights.Bold, Foreground = new SolidColorBrush(slotColor) };
        head.Children.Add(ui.Id);
        panel.Children.Add(head);

        panel.Children.Add(new TextBlock { Text = "Coding Agent", Margin = new Thickness(0, 0, 0, 3) });
        ui.Backend = new ComboBox { Margin = new Thickness(0, 0, 0, 8), ItemTemplate = ChoiceTemplate(withIcon: true) };
        panel.Children.Add(ui.Backend);

        panel.Children.Add(new TextBlock { Text = "Agent Role", Margin = new Thickness(0, 0, 0, 3) });
        ui.Role = new ComboBox { Margin = new Thickness(0, 0, 0, 6), ItemTemplate = ChoiceTemplate(withIcon: false) };
        panel.Children.Add(ui.Role);

        // 狀態列（執行中／已結束＋重新啟動）只有既有的組才有；新開的組不留空白
        var foot = new DockPanel { Height = 26, Visibility = _group == null ? Visibility.Collapsed : Visibility.Visible };
        ui.Restart = new Button { Content = Loc.T("ma.dlgRestart"), Padding = new Thickness(10, 0, 10, 0), Visibility = Visibility.Collapsed };
        DockPanel.SetDock(ui.Restart, Dock.Right);
        int idx = i;
        ui.Restart.Click += (_, _) => ToggleRestart(idx);
        foot.Children.Add(ui.Restart);
        ui.Status = new TextBlock { Foreground = new SolidColorBrush(Color.FromRgb(0x9A, 0x9A, 0x9A)), FontSize = 12 };
        foot.Children.Add(ui.Status);
        panel.Children.Add(foot);

        ui.Enable.Checked += (_, _) => UpdateEnabled(idx);
        ui.Enable.Unchecked += (_, _) => UpdateEnabled(idx);

        return new Border
        {
            BorderBrush = new SolidColorBrush(slotColor), BorderThickness = new Thickness(2), CornerRadius = new CornerRadius(4),
            Background = new SolidColorBrush(Color.FromRgb(0x25, 0x25, 0x28)), Padding = new Thickness(12, 10, 12, 8),
            Margin = new Thickness(4), Child = panel
        };
    }

    private static string SlotColor(int index) => index switch { 1 => "#EF9A9A", 2 => "#90CAF9", 3 => "#A5D6A7", _ => "#CE93D8" };

    /// <summary>下拉項目：[圖示] 文字（角色下拉沒有圖示，不留縮排）。</summary>
    private static DataTemplate ChoiceTemplate(bool withIcon)
    {
        var t = new DataTemplate(typeof(Choice));
        var sp = new FrameworkElementFactory(typeof(StackPanel));
        sp.SetValue(StackPanel.OrientationProperty, Orientation.Horizontal);
        if (withIcon)
        {
            var img = new FrameworkElementFactory(typeof(Image));
            img.SetBinding(Image.SourceProperty, new System.Windows.Data.Binding("Icon") { Converter = IconConverter.Instance });
            img.SetValue(Image.WidthProperty, 20.0);
            img.SetValue(Image.HeightProperty, 20.0);
            img.SetValue(Image.MarginProperty, new Thickness(0, 0, 6, 0));
            sp.AppendChild(img);
        }
        var tb = new FrameworkElementFactory(typeof(TextBlock));
        tb.SetBinding(TextBlock.TextProperty, new System.Windows.Data.Binding("Text"));
        tb.SetValue(TextBlock.VerticalAlignmentProperty, VerticalAlignment.Center);
        sp.AppendChild(tb);
        t.VisualTree = sp;
        return t;
    }

    private sealed class IconConverter : System.Windows.Data.IValueConverter
    {
        public static readonly IconConverter Instance = new();
        public object? Convert(object value, Type targetType, object parameter, System.Globalization.CultureInfo culture)
        {
            if (value is not string file || string.IsNullOrEmpty(file)) return null;
            try { return new System.Windows.Media.Imaging.BitmapImage(new Uri($"pack://application:,,,/icon/{file}")); }
            catch { return null; }
        }
        public object ConvertBack(object value, Type targetType, object parameter, System.Globalization.CultureInfo culture) => System.Windows.Data.Binding.DoNothing;
    }

    private void FillRoles()
    {
        var roles = new List<Choice> { new("", Loc.T("ma.dlgRoleNone"), null) };
        roles.AddRange(RoleLibrary.ListRoles().Select(r => new Choice(r.Key, r.Title, null)));
        for (int i = 0; i < 4; i++)
        {
            string? cur = (_ui[i].Role.SelectedItem as Choice)?.Key;
            _ui[i].Role.ItemsSource = roles;
            if (cur != null) Select(_ui[i].Role, cur);
        }
    }

    private static bool Select(ComboBox box, string key)
    {
        foreach (var o in box.Items)
            if (o is Choice c && string.Equals(c.Key, key, StringComparison.OrdinalIgnoreCase)) { box.SelectedItem = c; return true; }
        return false;
    }

    private void ApplyInitial(int i, MultiAgentSetup? saved)
    {
        var ui = _ui[i];
        int index = i + 1;
        string[] defaultRoles = RoleLibrary.BuiltInRoles;

        if (_group != null)
        {
            var slot = _group.Slots[i];
            ui.Id.Text = slot.AgentId;
            var list = new List<Choice>(_backends);
            if (!string.IsNullOrEmpty(slot.Backend) && !list.Any(c => c.Key == slot.Backend))
                list.Add(new Choice(slot.Backend, slot.BackendName, slot.Backend + ".png"));   // 執行中的 CLI 這台已找不到也照樣顯示
            ui.Backend.ItemsSource = list;
            if (slot.Tab != null)
            {
                Select(ui.Backend, slot.Backend);
                Select(ui.Role, slot.Role);
                ui.Enable.IsChecked = true;
                ui.Enable.IsEnabled = false;
                if (slot.Tab.Session != null)
                {
                    ui.Locked = true;
                    ui.Backend.IsEnabled = ui.Role.IsEnabled = false;
                    ui.Status.Text = Loc.T("ma.dlgRunning");
                }
                else
                {
                    ui.Exited = true;
                    ui.Status.Text = Loc.T("ma.stateExited");
                    ui.Restart.Visibility = Visibility.Visible;
                    ui.Backend.IsEnabled = ui.Role.IsEnabled = false;   // 按「重新啟動」才能改
                }
            }
            else
            {
                if (!Select(ui.Backend, slot.Backend) && list.Count > 0) ui.Backend.SelectedIndex = DefaultBackendIndex(index, list);
                if (!Select(ui.Role, string.IsNullOrEmpty(slot.Role) && !slot.Enabled ? defaultRoles[i] : slot.Role)) Select(ui.Role, defaultRoles[i]);
                ui.Enable.IsChecked = false;
                ui.Status.Text = Loc.T("ma.dlgNotRunning");
                UpdateEnabled(i);
            }
            return;
        }

        // 新開一組：組號按確定時才決定 → 先顯示 Agent-?1
        ui.Id.Text = $"Agent-?{index}";
        ui.Backend.ItemsSource = _backends;
        var ss = saved?.Slots[i];
        if (ss == null || !Select(ui.Backend, ss.Backend))
            if (_backends.Count > 0) ui.Backend.SelectedIndex = DefaultBackendIndex(index, _backends);
        if (ss == null || !Select(ui.Role, ss.Role)) Select(ui.Role, defaultRoles[i]);
        ui.Enable.IsChecked = index == 1 || (ss?.Enabled ?? index == 2);
        if (index == 1) ui.Enable.IsEnabled = false;   // 格 1 一定啟用（下方全寬那一格）
        UpdateEnabled(i);
    }

    /// <summary>預設 CLI：有 ClaudeCode 優先給格 1、有 Codex 優先給格 2，其餘＝清單第一個。</summary>
    private static int DefaultBackendIndex(int index, List<Choice> list)
    {
        string want = index == 1 ? "claude-code" : index == 2 ? "codex" : "";
        int k = list.FindIndex(c => c.Key == want);
        return k >= 0 ? k : 0;
    }

    private void UpdateEnabled(int i)
    {
        var ui = _ui[i];
        if (ui.Locked || ui.Exited) return;
        bool on = ui.Enable.IsChecked == true;
        ui.Backend.IsEnabled = ui.Role.IsEnabled = on;
    }

    private void ToggleRestart(int i)
    {
        var ui = _ui[i];
        ui.WantRestart = !ui.WantRestart;
        ui.Backend.IsEnabled = ui.Role.IsEnabled = ui.WantRestart;
        ui.Restart.Content = ui.WantRestart ? Loc.T("ma.dlgRestartOn") : Loc.T("ma.dlgRestart");
    }

    // ---------- 按鈕 ----------
    private void Browse_Click(object sender, RoutedEventArgs e)
    {
        using var fbd = new System.Windows.Forms.FolderBrowserDialog
        {
            Description = Loc.T("ma.pickDir"), UseDescriptionForTitle = true, ShowNewFolderButton = true
        };
        string start = Directory.Exists(FolderBox.Text) ? FolderBox.Text : AppSettings.Current.LastDir ?? "";
        if (Directory.Exists(start)) fbd.SelectedPath = start;
        // WinForms 對話框一定要傳 owner（踩雷：沒傳會開在主視窗後面＝「點了沒反應」）
        if (fbd.ShowDialog(Win32Owner.Of(this)) != System.Windows.Forms.DialogResult.OK) return;
        FolderBox.Text = fbd.SelectedPath;
        AppSettings.Current.LastDir = fbd.SelectedPath;
        AppSettings.Current.Save();
    }

    private void RestoreRoles_Click(object sender, RoutedEventArgs e)
    {
        if (MessageBox.Show(this, Loc.T("ma.dlgRestoreRolesAsk"), Loc.T("ma.title"), MessageBoxButton.YesNo, MessageBoxImage.Question)
            != MessageBoxResult.Yes) return;
        RoleLibrary.RestoreDefaults();
        FillRoles();
    }

    private void OpenRoles_Click(object sender, RoutedEventArgs e)
    {
        RoleLibrary.EnsureDefaults();
        try { System.Diagnostics.Process.Start("explorer.exe", $"\"{RoleLibrary.RolesDir}\""); } catch { }
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        string dir = FolderBox.Text.Trim();
        if (_group == null)
        {
            if (string.IsNullOrEmpty(dir)) { Warn(Loc.T("ma.dlgNeedFolder")); return; }
            if (!Directory.Exists(dir)) { Warn(string.Format(Loc.T("ma.dlgFolderMissing"), dir)); return; }
        }

        var result = new MultiAgentSetup { Dir = _group?.Dir ?? dir };
        for (int i = 0; i < 4; i++)
        {
            var ui = _ui[i];
            bool enabled = ui.Enable.IsChecked == true;
            string backend = (ui.Backend.SelectedItem as Choice)?.Key ?? "";
            string role = (ui.Role.SelectedItem as Choice)?.Key ?? "";
            bool needsBackend = _group == null ? enabled : (ui.WantRestart || (!ui.Locked && !ui.Exited && enabled));
            if (needsBackend && string.IsNullOrEmpty(backend))
            {
                Warn(string.Format(Loc.T("ma.dlgNeedBackend"), ui.Id.Text));
                return;
            }
            result.Slots[i] = new AgentSlotSetup { Enabled = enabled, Backend = backend, Role = role };
            if (_group != null && (ui.WantRestart || (!ui.Locked && !ui.Exited && enabled))) result.Launch.Add(i + 1);
        }

        if (_group == null)
        {
            try { AppSettings.Current.MultiAgentLastSetup = JsonSerializer.Serialize(result); AppSettings.Current.Save(); } catch { }
        }
        else if (result.Launch.Count == 0) { DialogResult = false; return; }   // 既有組沒有要變動的 → 當作取消

        Result = result;
        DialogResult = true;
    }

    private void Warn(string msg) => MessageBox.Show(this, msg, Loc.T("ma.title"), MessageBoxButton.OK, MessageBoxImage.Warning);
}
