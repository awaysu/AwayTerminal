using System.IO;
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

/// <summary>設定視窗的結果（新開一組／既有的組改設定）。</summary>
public sealed class MultiAgentSetup
{
    public string Dir { get; set; } = "";
    public AgentSlotSetup[] Slots { get; set; } = { new(), new(), new(), new() };
    /// <summary>既有組：要（重新）啟動的格號 1～4（新勾啟用的、改了 CLI／角色的、按了「重新啟動」的）。新開的組不用。</summary>
    public List<int> Launch { get; set; } = new();
    /// <summary>既有組：取消勾選「啟用」＝要關掉的格號。</summary>
    public List<int> Close { get; set; } = new();
    /// <summary>投遞限制次數（0＝不限）。</summary>
    public int MaxMessages { get; set; } = AgentGroup.DefaultMaxMessages;
    /// <summary>閒置檢查分鐘數（0＝不檢查）。</summary>
    public int IdleCheckMinutes { get; set; } = AgentGroup.DefaultIdleCheckMinutes;
}

/// <summary>
/// New → 代理團隊（Multi-Agent）的設定視窗（1.2.0）。開視窗前已經選好專案資料夾（和其他「啟動前選擇資料夾」的連線一樣先跳資料夾選擇）；
/// 四格（2×2）各有：啟用（格 1 一定啟用）、Agent ID（Agent-x1～x4，組號開組時才決定）、Coding Agent（只列這台電腦找得到的）、
/// Agent Role（None＋roles\*.md）。每次都從預設開始：格 1～4 的 CLI 依序 ClaudeCode／Codex／OpenCode／GeminiCLI（找不到就用清單第一個），
/// 角色依序 PM／SE／Architect／QA，格 1、2 啟用。沒勾「啟用」的格文字一律灰色。
/// <para>分頁右鍵「代理團隊設定…」開同一個視窗改既有的組：勾起沒啟用的格＝啟動；取消勾選執行中的格＝關掉（格 1 不能關）；
/// 改了 CLI 或角色＝重新啟動那一格（CLI 的角色是啟動時注入的）；「重新啟動」鈕＝設定不變也重開。會結束執行中 agent 的變更，按「套用」時先確認。</para>
/// </summary>
public partial class MultiAgentDialog : Window
{
    private sealed record Choice(string Key, string Text, string? Icon);

    private enum SlotState { NotRunning, Running, Exited }
    private enum SlotAction { None, Start, Restart, Close }

    private sealed class SlotUi
    {
        public CheckBox Enable = null!;
        public TextBlock Id = null!;
        public TextBlock CliLabel = null!;
        public TextBlock RoleLabel = null!;
        public ComboBox Backend = null!;
        public ComboBox Role = null!;
        public TextBlock Status = null!;
        public Button Restart = null!;
        public Brush IdBrush = null!;
        public SlotState State;              // 既有組：開視窗當下這格的狀態
        public string OrigBackend = "";      // 既有組：執行中／已結束那格目前的 CLI 與角色（改了＝重新啟動）
        public string OrigRole = "";
        public bool WantRestart;             // 按了「重新啟動」
    }

    /// <summary>每格預設的 Coding Agent（格 1～4）。</summary>
    private static readonly string[] DefaultBackends = { "claude-code", "codex", "opencode", "geminicli" };

    private static readonly Brush NormalText = Frozen(0xE0, 0xE0, 0xE0);
    private static readonly Brush GrayText = Frozen(0x6E, 0x6E, 0x6E);   // 沒勾「啟用」的格
    private static readonly Brush StatusText = Frozen(0x9A, 0x9A, 0x9A);

    private readonly AgentGroup? _group;
    private readonly string _dir;
    private readonly SlotUi[] _ui = new SlotUi[4];
    private readonly List<Choice> _backends;
    private bool _ready;   // 初始化完成前（設 ItemsSource／預選）的 SelectionChanged 不處理

    public MultiAgentSetup? Result { get; private set; }

    public MultiAgentDialog(string dir, AgentGroup? existing)
    {
        InitializeComponent();
        _group = existing;
        _dir = existing?.Dir ?? dir;

        // 資料夾在開視窗前就選好了（New → 代理團隊先跳資料夾選擇），視窗裡不再有資料夾欄位，標題顯示完整路徑
        Title = $"{Loc.T("ma.title")} — {_dir}";
        RestoreRolesBtn.Content = Loc.T("ma.dlgRestoreRoles");
        OpenRolesBtn.Content = Loc.T("ma.dlgOpenRoles");
        OkBtn.Content = Loc.T(existing == null ? "ma.dlgOpen" : "ma.dlgApply");
        CancelBtn.Content = Loc.T("ma.dlgCancel");
        HintText.Text = Loc.T("ma.dlgHint");

        // 投遞限制次數：10／30／50／100／不限；新開的組預設 30，既有的組＝它目前的值（不在選項裡就補一項）
        LimitLabel.Text = Loc.T("ma.dlgLimit");
        LimitHint.Text = Loc.T("ma.dlgLimitHint");
        LimitHint.ToolTip = LimitHint.Text;
        var limits = AgentGroup.LimitChoices.Select(n => new Choice(n.ToString(), n > 0 ? n.ToString() : Loc.T("ma.limitUnlimited"), null)).ToList();
        int curLimit = existing?.MaxMessages ?? AgentGroup.DefaultMaxMessages;
        if (!AgentGroup.LimitChoices.Contains(curLimit)) limits.Insert(0, new Choice(curLimit.ToString(), curLimit.ToString(), null));
        LimitBox.ItemTemplate = ChoiceTemplate(withIcon: false);
        LimitBox.ItemsSource = limits;
        Select(LimitBox, curLimit.ToString());

        // 閒置檢查：15／30／60 分鐘／不檢查；新開的組預設 30，既有的組＝它目前的值
        IdleLabel.Text = Loc.T("ma.dlgIdleCheck");
        IdleHint.Text = Loc.T("ma.dlgIdleHint");
        IdleHint.ToolTip = IdleHint.Text;
        var idles = AgentGroup.IdleCheckChoices
            .Select(n => new Choice(n.ToString(), n > 0 ? string.Format(Loc.T("ma.idleMinutes"), n) : Loc.T("ma.idleOff"), null)).ToList();
        int curIdle = existing?.IdleCheckMinutes ?? AgentGroup.DefaultIdleCheckMinutes;
        if (!AgentGroup.IdleCheckChoices.Contains(curIdle))
            idles.Insert(0, new Choice(curIdle.ToString(), string.Format(Loc.T("ma.idleMinutes"), curIdle), null));
        IdleBox.ItemTemplate = ChoiceTemplate(withIcon: false);
        IdleBox.ItemsSource = idles;
        Select(IdleBox, curIdle.ToString());

        // 這台電腦找得到的 Coding Agent（沿用自訂連線或自動偵測；見 ICodingAgentAdapter.Resolve），順序 ClaudeCode／Codex／OpenCode／GeminiCLI
        _backends = AdapterRegistry.All.Where(a => a.Resolve() != null)
            .Select(a => new Choice(a.Key, a.DisplayName, a.Key + ".png")).ToList();

        for (int i = 0; i < 4; i++) SlotGrid.Children.Add(BuildSlot(i));
        FillRoles();
        for (int i = 0; i < 4; i++) ApplyInitial(i);
        _ready = true;
        for (int i = 0; i < 4; i++) RefreshSlot(i);

        if (_backends.Count == 0 && existing == null)
        {
            NoBackendText.Text = Loc.T("ma.dlgNoBackend");
            NoBackendText.Visibility = Visibility.Visible;
            OkBtn.IsEnabled = false;
        }
    }

    private static SolidColorBrush Frozen(byte r, byte g, byte b)
    {
        var br = new SolidColorBrush(Color.FromRgb(r, g, b));
        br.Freeze();
        return br;
    }

    // ---------- 一格的畫面 ----------
    private FrameworkElement BuildSlot(int i)
    {
        var slotColor = (Color)ColorConverter.ConvertFromString(SlotColor(i + 1));
        var ui = _ui[i] = new SlotUi { IdBrush = new SolidColorBrush(slotColor) };
        var panel = new StackPanel();
        int idx = i;

        var head = new DockPanel { Margin = new Thickness(0, 0, 0, 8) };
        ui.Enable = new CheckBox { Content = Loc.T("ma.dlgEnable"), Margin = new Thickness(8, 0, 0, 0) };
        DockPanel.SetDock(ui.Enable, Dock.Right);
        head.Children.Add(ui.Enable);
        ui.Id = new TextBlock { FontSize = 15, FontWeight = FontWeights.Bold, Foreground = ui.IdBrush };
        head.Children.Add(ui.Id);
        panel.Children.Add(head);

        ui.CliLabel = new TextBlock { Text = Loc.T("ma.dlgAgentType"), Margin = new Thickness(0, 0, 0, 3) };
        panel.Children.Add(ui.CliLabel);
        ui.Backend = new ComboBox { Margin = new Thickness(0, 0, 0, 8), ItemTemplate = ChoiceTemplate(withIcon: true) };
        panel.Children.Add(ui.Backend);

        ui.RoleLabel = new TextBlock { Text = Loc.T("ma.dlgAgentRole"), Margin = new Thickness(0, 0, 0, 3) };
        panel.Children.Add(ui.RoleLabel);
        ui.Role = new ComboBox { Margin = new Thickness(0, 0, 0, 6), ItemTemplate = ChoiceTemplate(withIcon: false) };
        panel.Children.Add(ui.Role);

        // 狀態列（執行中／已結束／未啟用＋套用後會怎樣＋重新啟動）只有既有的組才有；新開的組不留空白
        var foot = new DockPanel { Height = 26, Visibility = _group == null ? Visibility.Collapsed : Visibility.Visible };
        ui.Restart = new Button { Content = Loc.T("ma.dlgRestart"), Padding = new Thickness(10, 0, 10, 0), Visibility = Visibility.Collapsed };
        DockPanel.SetDock(ui.Restart, Dock.Right);
        ui.Restart.Click += (_, _) => { _ui[idx].WantRestart = !_ui[idx].WantRestart; RefreshSlot(idx); };
        foot.Children.Add(ui.Restart);
        ui.Status = new TextBlock { Foreground = StatusText, FontSize = 12 };
        foot.Children.Add(ui.Status);
        panel.Children.Add(foot);

        ui.Enable.Checked += (_, _) => RefreshSlot(idx);
        ui.Enable.Unchecked += (_, _) => RefreshSlot(idx);
        ui.Backend.SelectionChanged += (_, _) => RefreshSlot(idx);
        ui.Role.SelectionChanged += (_, _) => RefreshSlot(idx);

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

    private static string KeyOf(ComboBox box) => (box.SelectedItem as Choice)?.Key ?? "";

    /// <summary>格 index 的預設 CLI；這台找不到就用清單第一個。</summary>
    private static void SelectDefaultBackend(ComboBox box, int index)
    {
        if (!Select(box, DefaultBackends[index - 1]) && box.Items.Count > 0) box.SelectedIndex = 0;
    }

    private void ApplyInitial(int i)
    {
        var ui = _ui[i];
        int index = i + 1;
        string defaultRole = RoleLibrary.DefaultSlotRoles[i];
        ui.Id.Text = $"Agent-x{index}";   // 組號開組時才決定（1～9）；既有的組也照這樣顯示，與新開時一致

        if (_group == null)
        {
            ui.Backend.ItemsSource = _backends;
            SelectDefaultBackend(ui.Backend, index);
            Select(ui.Role, defaultRole);
            ui.Enable.IsChecked = index <= 2;
            ui.Enable.IsEnabled = index != 1;   // 格 1（下方全寬那一格）一定啟用
            return;
        }

        var slot = _group.Slots[i];
        var list = new List<Choice>(_backends);
        if (!string.IsNullOrEmpty(slot.Backend) && !list.Any(c => c.Key == slot.Backend))
            list.Add(new Choice(slot.Backend, slot.BackendName, slot.Backend + ".png"));   // 用過的 CLI 這台已找不到也照樣顯示
        ui.Backend.ItemsSource = list;
        if (slot.Tab != null)
        {
            ui.State = slot.Tab.Session != null ? SlotState.Running : SlotState.Exited;
            ui.OrigBackend = slot.Backend;
            ui.OrigRole = slot.Role;
            Select(ui.Backend, slot.Backend);
            Select(ui.Role, slot.Role);
            ui.Enable.IsChecked = true;
            ui.Enable.IsEnabled = index != 1;   // 格 1 不能關（其他格取消勾選＝套用後關掉那個 agent）
        }
        else
        {
            ui.State = SlotState.NotRunning;
            // 從沒設定過＝預設；之前開過又被關掉的格＝沿用它上次的 CLI／角色
            if (string.IsNullOrEmpty(slot.Backend) || !Select(ui.Backend, slot.Backend)) SelectDefaultBackend(ui.Backend, index);
            if (string.IsNullOrEmpty(slot.Backend) || !Select(ui.Role, slot.Role)) Select(ui.Role, defaultRole);
            ui.Enable.IsChecked = false;
        }
    }

    /// <summary>既有組這格按「套用」後會怎樣。</summary>
    private SlotAction ActionOf(int i)
    {
        var ui = _ui[i];
        bool on = ui.Enable.IsChecked == true;
        if (ui.State == SlotState.NotRunning) return on ? SlotAction.Start : SlotAction.None;
        if (!on) return SlotAction.Close;
        bool changed = !string.Equals(KeyOf(ui.Backend), ui.OrigBackend, StringComparison.OrdinalIgnoreCase)
                    || !string.Equals(KeyOf(ui.Role), ui.OrigRole, StringComparison.OrdinalIgnoreCase);
        return changed || ui.WantRestart ? SlotAction.Restart : SlotAction.None;
    }

    /// <summary>依「啟用」與目前的選擇更新這一格：可不可以改、灰字、狀態列。</summary>
    private void RefreshSlot(int i)
    {
        if (!_ready) return;
        var ui = _ui[i];
        bool on = ui.Enable.IsChecked == true;
        ui.Backend.IsEnabled = ui.Role.IsEnabled = on;
        // 下拉的文字被黑字樣式固定住（見 XAML 註解），停用時改用半透明呈現灰階
        ui.Backend.Opacity = ui.Role.Opacity = on ? 1.0 : 0.45;
        ui.Enable.Foreground = ui.CliLabel.Foreground = ui.RoleLabel.Foreground = on ? NormalText : GrayText;
        ui.Id.Foreground = on ? ui.IdBrush : GrayText;

        if (_group == null) return;
        ui.Restart.Visibility = on && ui.State != SlotState.NotRunning ? Visibility.Visible : Visibility.Collapsed;
        ui.Restart.Content = Loc.T(ui.WantRestart ? "ma.dlgRestartOn" : "ma.dlgRestart");
        string status = ui.State switch
        {
            SlotState.Running => Loc.T("ma.dlgRunning"),
            SlotState.Exited => Loc.T("ma.dlgExited"),
            _ => Loc.T(on ? "ma.dlgNotStarted" : "ma.dlgNotRunning")
        };
        string? will = ActionOf(i) switch
        {
            SlotAction.Start => "ma.dlgWillStart",
            SlotAction.Restart => "ma.dlgWillRestart",
            SlotAction.Close => "ma.dlgWillClose",
            _ => null
        };
        ui.Status.Text = will == null ? status : status + Loc.T(will);
        ui.Status.Foreground = on ? StatusText : GrayText;
    }

    // ---------- 按鈕 ----------
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
        if (_group == null && !Directory.Exists(_dir)) { Warn(string.Format(Loc.T("ma.dlgFolderMissing"), _dir)); return; }   // 開著視窗時資料夾被刪／改名

        var result = new MultiAgentSetup
        {
            Dir = _dir,
            MaxMessages = int.TryParse(KeyOf(LimitBox), out int limit) ? Math.Max(0, limit) : AgentGroup.DefaultMaxMessages,
            IdleCheckMinutes = int.TryParse(KeyOf(IdleBox), out int idle) ? Math.Max(0, idle) : AgentGroup.DefaultIdleCheckMinutes
        };
        var endsConversation = new List<string>();   // 執行中、套用後會關閉或重新啟動的 agent（先確認）
        for (int i = 0; i < 4; i++)
        {
            var ui = _ui[i];
            bool enabled = ui.Enable.IsChecked == true;
            string backend = KeyOf(ui.Backend);
            var act = _group == null ? (enabled ? SlotAction.Start : SlotAction.None) : ActionOf(i);
            if ((act is SlotAction.Start or SlotAction.Restart) && string.IsNullOrEmpty(backend))
            {
                Warn(string.Format(Loc.T("ma.dlgNeedBackend"), ui.Id.Text));
                return;
            }
            result.Slots[i] = new AgentSlotSetup { Enabled = enabled, Backend = backend, Role = KeyOf(ui.Role) };
            if (_group == null) continue;
            if (act is SlotAction.Start or SlotAction.Restart) result.Launch.Add(i + 1);
            if (act == SlotAction.Close) result.Close.Add(i + 1);
            if (ui.State == SlotState.Running && (act is SlotAction.Close or SlotAction.Restart))
                endsConversation.Add(string.Format(Loc.T(act == SlotAction.Close ? "ma.applyClose" : "ma.applyRestart"),
                    _group.Slots[i].Label));   // 用 pane 標題上的全名（Agent-12 · Software Engineer · Codex），對得上是哪一格
        }

        if (_group != null)
        {
            if (result.Launch.Count == 0 && result.Close.Count == 0 && result.MaxMessages == _group.MaxMessages
                && result.IdleCheckMinutes == _group.IdleCheckMinutes)
            { DialogResult = false; return; }   // 沒有要變動的 → 當作取消
            if (endsConversation.Count > 0 &&
                MessageBox.Show(this, string.Format(Loc.T("ma.applyAsk"), string.Join("\n", endsConversation)), Loc.T("ma.title"),
                    MessageBoxButton.YesNo, MessageBoxImage.Warning) != MessageBoxResult.Yes) return;
        }

        Result = result;
        DialogResult = true;
    }

    private void Warn(string msg) => MessageBox.Show(this, msg, Loc.T("ma.title"), MessageBoxButton.OK, MessageBoxImage.Warning);
}
