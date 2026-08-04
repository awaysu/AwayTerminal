using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Linq;
using System.Windows;
using System.Windows.Controls;
using AwayTerminal.Localization;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

/// <summary>
/// 自訂新連接管理。左清單＋下方「＋新增 / 刪除」，右詳細設定，右下「儲存 / 返回」。
/// 儲存自動判斷新增或修改；刪除僅在選到既有項目時啟用；儲存僅在有變更時啟用；切換/關閉若有未存變更會提示。
/// </summary>
public partial class CustomConnDialog : Window
{
    // 可選的圖示（對應 icon\{key}.png）
    private static readonly string[] IconKeys =
        { "powershell", "ssh-telnet", "adb", "wsl", "git", "docker", "claude-code", "opencode", "python", "run", "none" };
    private const string DefaultIcon = "run";

    /// <summary>自動偵測用的已知工具（在 PATH 與常見安裝位置尋找）。</summary>
    private sealed class KnownTool
    {
        public string Name;
        public string[] ExeNames;
        public string Args;
        public string Icon;
        public bool PickDir;
        public KnownTool(string name, string[] exeNames, string args, string icon, bool pickDir)
        { Name = name; ExeNames = exeNames; Args = args; Icon = icon; PickDir = pickDir; }
    }

    private static readonly KnownTool[] KnownTools =
    {
        new("ClaudeCode", new[] { "claude.exe", "claude.cmd" }, "--dangerously-skip-permissions", "claude-code", true),
        new("WSL",        new[] { "wsl.exe" }, "", "wsl", false),
        new("OpenCode",   new[] { "opencode.exe", "opencode.cmd" }, "", "opencode", true),
        new("Gemini",     new[] { "gemini.exe", "gemini.cmd" }, "", "run", true),
        new("Aider",      new[] { "aider.exe", "aider.cmd" }, "", "run", true),
        // ADB：v1.0.18 起不再是內建選單項目，改成一般自訂連線。參數固定 shell；
        // 若機器上接了兩台以上裝置，MainWindow.OpenCustom 會偵測到執行檔是 adb 而
        // 先跑 adb devices 讓使用者選序號（見 IsAdbExe），所以裝置選單不會流失。
        new("ADB",        new[] { "adb.exe" }, "shell", "adb", false),
    };

    /// <summary>圖示下拉的資料項（用 DataTemplate 顯示，避免直接把 UIElement 當內容導致共享視覺問題）。</summary>
    private sealed class IconChoice
    {
        public string Key { get; }
        public System.Windows.Media.ImageSource? Image { get; }
        public IconChoice(string key)
        {
            Key = key;
            try { Image = new System.Windows.Media.Imaging.BitmapImage(new Uri($"pack://application:,,,/icon/{key}.png")); }
            catch { }
        }
    }

    private readonly ObservableCollection<CustomConn> _items = new();
    private CustomConn? _editing;  // 目前編輯的既有項目（null＝新增模式）
    private bool _isNew;           // 新增模式（編輯區是一筆尚未加入清單的新連線）
    private bool _dirty;           // 編輯區與載入值不同
    private bool _suppress;        // 載入編輯區時抑制 dirty
    private bool _switching;       // 還原選取時避免重入

    public CustomConnDialog()
    {
        InitializeComponent();

        // 本地化
        Title = Loc.T("custom.title");
        IconLabel.Text = Loc.T("custom.icon");
        NameLabel.Text = Loc.T("custom.name");
        PathLabel.Text = Loc.T("custom.path");
        ArgsLabel.Text = Loc.T("custom.args");
        CloseKeyLabel.Text = Loc.T("custom.closeKey");
        PickDirCheck.Content = Loc.T("custom.pickDir");
        HiddenCheck.Content = Loc.T("custom.hidden");
        ViaPsCheck.Content = Loc.T("custom.viaPs");
        BrowseBtn.Content = Loc.T("log.browse");
        AddBtn.Content = Loc.T("custom.add");
        DeleteBtn.Content = Loc.T("custom.delBtn");
        AutoDetectBtn.Content = Loc.T("custom.detect");
        SaveBtn.Content = Loc.T("custom.save");
        BackBtn.Content = Loc.T("custom.back");

        // 圖示下拉：只顯示 32x32 圖示（無文字）
        var iconTemplate = new DataTemplate(typeof(IconChoice));
        var imgFactory = new FrameworkElementFactory(typeof(Image));
        imgFactory.SetBinding(Image.SourceProperty, new System.Windows.Data.Binding("Image"));
        imgFactory.SetValue(Image.WidthProperty, 32.0);
        imgFactory.SetValue(Image.HeightProperty, 32.0);
        imgFactory.SetValue(Image.StretchProperty, System.Windows.Media.Stretch.Uniform);
        iconTemplate.VisualTree = imgFactory;
        IconCombo.ItemTemplate = iconTemplate;
        IconCombo.ItemsSource = IconKeys.Select(k => new IconChoice(k)).ToList();
        IconCombo.SelectionChanged += (_, _) => MarkDirty();

        // 關閉按鍵（無 / Ctrl+C / Ctrl+D）/ 次數（x1~x5）
        CloseKeyCombo.Items.Add(new ComboBoxItem { Content = Loc.T("custom.closeNone"), Tag = "none" });
        CloseKeyCombo.Items.Add(new ComboBoxItem { Content = "Ctrl+C", Tag = "ctrl-c" });
        CloseKeyCombo.Items.Add(new ComboBoxItem { Content = "Ctrl+D", Tag = "ctrl-d" });
        CloseKeyCombo.SelectionChanged += (_, _) => { MarkDirty(); CloseCountCombo.IsEnabled = SelectedCloseKey() != "none"; };
        for (int i = 1; i <= 5; i++)
            CloseCountCombo.Items.Add(new ComboBoxItem { Content = "x" + i, Tag = i });
        CloseCountCombo.SelectionChanged += (_, _) => MarkDirty();

        // 編輯區變更 → 標記 dirty
        NameBox.TextChanged += (_, _) => MarkDirty();
        PathBox.TextChanged += (_, _) => MarkDirty();
        ArgsBox.TextChanged += (_, _) => MarkDirty();
        HiddenCheck.Checked += (_, _) => MarkDirty(); HiddenCheck.Unchecked += (_, _) => MarkDirty();
        PickDirCheck.Checked += (_, _) => MarkDirty(); PickDirCheck.Unchecked += (_, _) => MarkDirty();
        ViaPsCheck.Checked += (_, _) => MarkDirty(); ViaPsCheck.Unchecked += (_, _) => MarkDirty();

        // 載入現有清單的副本（未按儲存前不動到設定）
        foreach (var c in AppSettings.Current.CustomConns)
            _items.Add(Clone(c));
        List.ItemsSource = _items;

        if (_items.Count > 0) List.SelectedIndex = 0; // 觸發載入第一筆
        else EnterNewMode();
    }

    private static CustomConn Clone(CustomConn c) => new()
    {
        Name = c.Name, Path = c.Path, Args = c.Args, Icon = c.Icon,
        CloseKey = c.CloseKey, CloseCount = c.CloseCount,
        PickDir = c.PickDir, Hidden = c.Hidden, ViaPowerShell = c.ViaPowerShell
    };

    // ---------- 圖示 / 關閉鍵 下拉選取 ----------
    private void SelectIcon(string? key)
    {
        string want = string.IsNullOrWhiteSpace(key) ? DefaultIcon : key;
        foreach (IconChoice ic in IconCombo.Items)
            if (ic.Key == want) { IconCombo.SelectedItem = ic; return; }
        foreach (IconChoice ic in IconCombo.Items)
            if (ic.Key == DefaultIcon) { IconCombo.SelectedItem = ic; return; }
    }
    private string SelectedIconKey() => (IconCombo.SelectedItem as IconChoice)?.Key ?? DefaultIcon;

    private void SelectCloseKey(string? key)
    {
        string want = key switch { "ctrl-d" => "ctrl-d", "none" => "none", _ => "ctrl-c" };
        foreach (ComboBoxItem it in CloseKeyCombo.Items)
            if ((string)it.Tag == want) { CloseKeyCombo.SelectedItem = it; return; }
    }
    private string SelectedCloseKey() => (CloseKeyCombo.SelectedItem as ComboBoxItem)?.Tag as string ?? "ctrl-c";

    private void SelectCloseCount(int n)
    {
        int want = n is >= 1 and <= 5 ? n : 3;
        foreach (ComboBoxItem it in CloseCountCombo.Items)
            if ((int)it.Tag == want) { CloseCountCombo.SelectedItem = it; return; }
    }
    private int SelectedCloseCount() => (CloseCountCombo.SelectedItem as ComboBoxItem)?.Tag is int n ? n : 3;

    // ---------- 編輯區載入 / 讀取 ----------
    private void LoadEditor(CustomConn? c)
    {
        _suppress = true;
        Editor.IsEnabled = true;
        SelectIcon(c?.Icon);
        NameBox.Text = c?.Name ?? "";
        PathBox.Text = c?.Path ?? "";
        ArgsBox.Text = c?.Args ?? "";
        SelectCloseKey(c?.CloseKey);
        SelectCloseCount(c?.CloseCount ?? 3);
        HiddenCheck.IsChecked = c?.Hidden ?? false;
        PickDirCheck.IsChecked = c?.PickDir ?? false;
        ViaPsCheck.IsChecked = c?.ViaPowerShell ?? false;
        _suppress = false;
        _dirty = false;
        UpdateButtons();
    }

    private void ApplyToItem(CustomConn c)
    {
        c.Name = NameBox.Text.Trim();
        c.Path = PathBox.Text.Trim();
        c.Args = ArgsBox.Text.Trim();
        c.Icon = SelectedIconKey();
        c.CloseKey = SelectedCloseKey();
        c.CloseCount = SelectedCloseCount();
        c.Hidden = HiddenCheck.IsChecked == true;
        c.PickDir = PickDirCheck.IsChecked == true;
        c.ViaPowerShell = ViaPsCheck.IsChecked == true;
    }

    private void EnterNewMode()
    {
        _switching = true;
        List.SelectedItem = null;
        _switching = false;
        _isNew = true;
        _editing = null;
        LoadEditor(null);
        NameBox.Focus();
    }

    private void MarkDirty()
    {
        if (_suppress) return;
        _dirty = true;
        UpdateButtons();
    }

    private void UpdateButtons()
    {
        SaveBtn.IsEnabled = _dirty && !string.IsNullOrWhiteSpace(NameBox.Text);
        DeleteBtn.IsEnabled = !_isNew && _editing != null; // 只在既有項目啟用
    }

    // ---------- 清單選取切換 ----------
    private void List_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_switching) return;
        var newSel = List.SelectedItem as CustomConn;

        if (_dirty)
        {
            var r = MessageBox.Show(this, Loc.T("custom.unsavedAsk"), Loc.T("custom.title"),
                MessageBoxButton.YesNoCancel, MessageBoxImage.Question);
            if (r == MessageBoxResult.Cancel) { RevertSelection(e); return; }
            if (r == MessageBoxResult.Yes && !DoSave()) { RevertSelection(e); return; }
            // No → 放棄變更
        }

        // DoSave（新增模式）可能改了選取；統一回到使用者點選的 newSel
        if (!ReferenceEquals(List.SelectedItem, newSel))
        {
            _switching = true;
            List.SelectedItem = newSel;
            _switching = false;
        }
        _isNew = false;
        _editing = newSel;
        LoadEditor(newSel);
    }

    private void RevertSelection(SelectionChangedEventArgs e)
    {
        _switching = true;
        if (_isNew) List.SelectedItem = null;
        else List.SelectedItem = e.RemovedItems.Count > 0 ? e.RemovedItems[0] : null;
        _switching = false;
    }

    // ---------- 按鈕 ----------
    private void Add_Click(object sender, RoutedEventArgs e)
    {
        if (_dirty)
        {
            var r = MessageBox.Show(this, Loc.T("custom.unsavedAsk"), Loc.T("custom.title"),
                MessageBoxButton.YesNoCancel, MessageBoxImage.Question);
            if (r == MessageBoxResult.Cancel) return;
            if (r == MessageBoxResult.Yes && !DoSave()) return;
        }
        EnterNewMode();
    }

    private void Browse_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new Microsoft.Win32.OpenFileDialog
        {
            Filter = (Loc.Lang == "en" ? "Executable" : "執行檔") + " (*.exe;*.cmd;*.bat)|*.exe;*.cmd;*.bat|"
                     + (Loc.Lang == "en" ? "All files" : "所有檔案") + " (*.*)|*.*",
            Title = Loc.T("custom.path")
        };
        if (dlg.ShowDialog(this) == true)
        {
            PathBox.Text = dlg.FileName;
            if (string.IsNullOrWhiteSpace(NameBox.Text))
                NameBox.Text = System.IO.Path.GetFileNameWithoutExtension(dlg.FileName);
        }
    }

    private void Save_Click(object sender, RoutedEventArgs e) => DoSave();
    private void Back_Click(object sender, RoutedEventArgs e) => Close();

    /// <summary>自動偵測已知工具（claude / opencode 等），把找到且尚未在清單中的加入並存檔。</summary>
    private void AutoDetect_Click(object sender, RoutedEventArgs e)
    {
        var added = new System.Collections.Generic.List<string>();
        foreach (var tool in KnownTools)
        {
            if (_items.Any(c => string.Equals(c.Name, tool.Name, StringComparison.OrdinalIgnoreCase))) continue; // 已存在
            string path = ResolveTool(tool.ExeNames);
            if (string.IsNullOrEmpty(path)) continue;

            bool viaPs = path.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase)
                      || path.EndsWith(".bat", StringComparison.OrdinalIgnoreCase);
            _items.Add(new CustomConn
            {
                Name = tool.Name, Path = path, Args = tool.Args, Icon = tool.Icon,
                PickDir = tool.PickDir, ViaPowerShell = viaPs
            });
            added.Add(tool.Name);
        }

        if (added.Count == 0)
        {
            MessageBox.Show(this, Loc.T("custom.detectNone"), Loc.T("custom.title"),
                MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }

        List.Items.Refresh();
        Commit();
        MessageBox.Show(this, string.Format(Loc.T("custom.detectDone"), string.Join("、", added)),
            Loc.T("custom.title"), MessageBoxButton.OK, MessageBoxImage.Information);
    }

    /// <summary>在 PATH 與常見安裝位置尋找工具，回傳第一個存在的完整路徑；找不到回空字串。</summary>
    private static string ResolveTool(string[] exeNames)
    {
        string[] extraDirs =
        {
            System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".local", "bin"),
            System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "npm"),
            System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs"),
        };
        foreach (var exe in exeNames)
        {
            var p = AppSettings.TryResolveOnPath(exe);
            if (!string.IsNullOrEmpty(p)) return p;
            foreach (var d in extraDirs)
            {
                try
                {
                    var full = System.IO.Path.Combine(d, exe);
                    if (System.IO.File.Exists(full)) return full;
                }
                catch { }
            }
        }
        return "";
    }

    /// <summary>儲存目前編輯內容（自動判斷新增或修改），並持久化。成功回傳 true。</summary>
    private bool DoSave()
    {
        if (string.IsNullOrWhiteSpace(NameBox.Text)) { Warn("custom.needName"); return false; }

        bool wasNew = _isNew;
        CustomConn target;
        if (_isNew)
        {
            target = new CustomConn();
            _items.Add(target);
            _isNew = false;
        }
        else if (_editing != null) target = _editing;
        else return false;

        ApplyToItem(target);
        _editing = target;
        List.Items.Refresh();

        if (wasNew) // 新增後在清單選取它（既有項目維持原選取，交由呼叫端處理）
        {
            _switching = true;
            List.SelectedItem = target;
            _switching = false;
        }

        Commit();
        _dirty = false;
        UpdateButtons();
        return true;
    }

    private void Delete_Click(object sender, RoutedEventArgs e)
    {
        if (_isNew || _editing == null) return;
        var r = MessageBox.Show(this, string.Format(Loc.T("custom.delConfirm"), _editing.Name), Loc.T("custom.delete"),
            MessageBoxButton.YesNo, MessageBoxImage.Question);
        if (r != MessageBoxResult.Yes) return;
        int idx = _items.IndexOf(_editing);
        _items.Remove(_editing);
        _editing = null;
        Commit();

        if (_items.Count > 0)
        {
            _switching = true;
            List.SelectedIndex = System.Math.Min(idx, _items.Count - 1);
            _switching = false;
            _editing = List.SelectedItem as CustomConn;
            _isNew = false;
            LoadEditor(_editing);
        }
        else EnterNewMode();
    }

    private void Commit()
    {
        AppSettings.Current.CustomConns = _items
            .Where(c => !string.IsNullOrWhiteSpace(c.Name))
            .ToList();
        AppSettings.Current.Save();
    }

    protected override void OnClosing(CancelEventArgs e)
    {
        if (_dirty)
        {
            var r = MessageBox.Show(this, Loc.T("custom.unsavedAsk"), Loc.T("custom.title"),
                MessageBoxButton.YesNoCancel, MessageBoxImage.Question);
            if (r == MessageBoxResult.Cancel) { e.Cancel = true; return; }
            if (r == MessageBoxResult.Yes && !DoSave()) { e.Cancel = true; return; }
        }
        base.OnClosing(e);
    }

    private void Warn(string key)
        => MessageBox.Show(this, Loc.T(key), Loc.T("custom.title"), MessageBoxButton.OK, MessageBoxImage.Warning);
}
