using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Linq;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using AwayTerminal.Localization;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

/// <summary>群組標題顯示：空群組顯示「未分組」。</summary>
public sealed class GroupHeaderConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, System.Globalization.CultureInfo culture)
        => string.IsNullOrWhiteSpace(value as string) ? Loc.T("prompt.ungrouped") : value!;
    public object ConvertBack(object value, Type targetType, object parameter, System.Globalization.CultureInfo culture)
        => Binding.DoNothing;
}

/// <summary>
/// 常用字串（分組版）。左側依群組顯示可收合的清單，群組標題右鍵可改名/刪群組；
/// 右側編輯區含「群組」可編輯下拉。互動同「自訂新連接」。
/// </summary>
public partial class PromptDialog : Window
{
    private readonly ObservableCollection<PromptItem> _items = new();
    private PromptItem? _editing;
    private bool _isNew;
    private bool _dirty;
    private bool _suppress;
    private bool _switching;

    /// <summary>按「送出」後要送到分頁的內容（null = 未送出）。</summary>
    public string? ContentToSend { get; private set; }

    public PromptDialog()
    {
        InitializeComponent();

        // 本地化
        Title = Loc.T("prompt.title");
        GroupLabel.Text = Loc.T("prompt.group");
        TitleLabel.Text = Loc.T("prompt.header");
        ContentLabel.Text = Loc.T("prompt.content");
        AddBtn.Content = Loc.T("custom.add");
        DeleteBtn.Content = Loc.T("custom.delBtn");
        BackupBtn.Content = Loc.T("prompt.backup");
        LoadBtn.Content = Loc.T("prompt.load");
        SendBtn.Content = Loc.T("prompt.send");
        SaveBtn.Content = Loc.T("prompt.save");
        BackBtn.Content = Loc.T("custom.back");

        // 編輯區變更 → 標記 dirty
        TitleBox.TextChanged += (_, _) => MarkDirty();
        ContentBox.TextChanged += (_, _) => { MarkDirty(); UpdateButtons(); };
        GroupCombo.AddHandler(System.Windows.Controls.Primitives.TextBoxBase.TextChangedEvent,
            new TextChangedEventHandler((_, _) => MarkDirty()));

        // 載入現有清單的副本（未按儲存前不動到設定）
        foreach (var p in AppSettings.Current.Prompts)
            _items.Add(new PromptItem { Title = p.Title, Content = p.Content, Group = p.Group });

        // 依群組分組顯示
        var view = CollectionViewSource.GetDefaultView(_items);
        view.GroupDescriptions.Add(new PropertyGroupDescription("Group"));
        TitleList.ItemsSource = _items;

        RefreshGroupChoices();
        if (_items.Count > 0) TitleList.SelectedIndex = 0;
        else EnterNewMode();
    }

    private void RefreshView() => CollectionViewSource.GetDefaultView(_items).Refresh();

    /// <summary>更新「群組」下拉的可選清單（現有群組名）。</summary>
    private void RefreshGroupChoices()
    {
        string cur = GroupCombo.Text;
        GroupCombo.ItemsSource = _items
            .Select(p => p.Group).Where(g => !string.IsNullOrWhiteSpace(g))
            .Distinct().OrderBy(g => g).ToList();
        GroupCombo.Text = cur;
    }

    // ---------- 編輯區載入 / 讀取 ----------
    private void LoadEditor(PromptItem? p)
    {
        _suppress = true;
        GroupCombo.Text = p?.Group ?? "";
        TitleBox.Text = p?.Title ?? "";
        ContentBox.Text = p?.Content ?? "";
        _suppress = false;
        _dirty = false;
        UpdateButtons();
    }

    private void ApplyToItem(PromptItem p)
    {
        p.Group = (GroupCombo.Text ?? "").Trim();
        p.Title = TitleBox.Text.Trim();
        p.Content = ContentBox.Text;
    }

    private void EnterNewMode()
    {
        _switching = true;
        TitleList.SelectedItem = null;
        _switching = false;
        _isNew = true;
        _editing = null;
        LoadEditor(null);
        TitleBox.Focus();
    }

    private void MarkDirty()
    {
        if (_suppress) return;
        _dirty = true;
        UpdateButtons();
    }

    private void UpdateButtons()
    {
        SaveBtn.IsEnabled = _dirty && !string.IsNullOrWhiteSpace(TitleBox.Text);
        DeleteBtn.IsEnabled = !_isNew && _editing != null;
        SendBtn.IsEnabled = !string.IsNullOrWhiteSpace(ContentBox.Text);
    }

    // ---------- 清單選取切換 ----------
    private void List_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_switching) return;
        var newSel = TitleList.SelectedItem as PromptItem;

        if (_dirty)
        {
            var r = MessageBox.Show(this, Loc.T("custom.unsavedAsk"), Loc.T("prompt.title"),
                MessageBoxButton.YesNoCancel, MessageBoxImage.Question);
            if (r == MessageBoxResult.Cancel) { RevertSelection(e); return; }
            if (r == MessageBoxResult.Yes && !DoSave()) { RevertSelection(e); return; }
        }

        if (!ReferenceEquals(TitleList.SelectedItem, newSel))
        {
            _switching = true;
            TitleList.SelectedItem = newSel;
            _switching = false;
        }
        _isNew = false;
        _editing = newSel;
        LoadEditor(newSel);
    }

    private void RevertSelection(SelectionChangedEventArgs e)
    {
        _switching = true;
        if (_isNew) TitleList.SelectedItem = null;
        else TitleList.SelectedItem = e.RemovedItems.Count > 0 ? e.RemovedItems[0] : null;
        _switching = false;
    }

    // ---------- 按鈕 ----------
    private void Add_Click(object sender, RoutedEventArgs e)
    {
        if (!ConfirmDiscardOrSave()) return;
        EnterNewMode();
    }

    private void Send_Click(object sender, RoutedEventArgs e)
    {
        string content = ContentBox.Text;
        if (string.IsNullOrWhiteSpace(content))
        {
            MessageBox.Show(this, Loc.T("prompt.noContent"), "AwayTerminal", MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }
        ContentToSend = content;
        DialogResult = true;
    }

    private void Save_Click(object sender, RoutedEventArgs e) => DoSave();
    private void Back_Click(object sender, RoutedEventArgs e) => Close();

    // ---------- 備份 / 載入（XML）----------
    private void Backup_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new Microsoft.Win32.SaveFileDialog
        {
            Filter = "XML (*.xml)|*.xml",
            FileName = "AwayTerminal-snippets.xml",
            Title = Loc.T("prompt.backup")
        };
        if (dlg.ShowDialog(this) != true) return;
        try
        {
            var list = _items.Where(p => !string.IsNullOrWhiteSpace(p.Title))
                .Select(p => new PromptItem { Title = p.Title, Content = p.Content, Group = p.Group })
                .ToList();
            var ser = new System.Xml.Serialization.XmlSerializer(typeof(System.Collections.Generic.List<PromptItem>));
            using var fs = System.IO.File.Create(dlg.FileName);
            ser.Serialize(fs, list);
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, Loc.T("prompt.title"), MessageBoxButton.OK, MessageBoxImage.Error);
        }
    }

    private void Load_Click(object sender, RoutedEventArgs e)
    {
        if (!ConfirmDiscardOrSave()) return;
        var dlg = new Microsoft.Win32.OpenFileDialog
        {
            Filter = "XML (*.xml)|*.xml",
            Title = Loc.T("prompt.load")
        };
        if (dlg.ShowDialog(this) != true) return;

        System.Collections.Generic.List<PromptItem>? loaded;
        try
        {
            var ser = new System.Xml.Serialization.XmlSerializer(typeof(System.Collections.Generic.List<PromptItem>));
            using var fs = System.IO.File.OpenRead(dlg.FileName);
            loaded = ser.Deserialize(fs) as System.Collections.Generic.List<PromptItem>;
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, Loc.T("prompt.title"), MessageBoxButton.OK, MessageBoxImage.Error);
            return;
        }
        if (loaded == null) return;

        if (MessageBox.Show(this, Loc.T("prompt.loadReplace"), Loc.T("prompt.load"),
                MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;

        _items.Clear();
        foreach (var p in loaded)
            _items.Add(new PromptItem { Title = p.Title ?? "", Content = p.Content ?? "", Group = p.Group ?? "" });
        _editing = null;
        RefreshView();
        RefreshGroupChoices();
        Persist();

        if (_items.Count > 0)
        {
            _switching = true;
            TitleList.SelectedIndex = 0;
            _switching = false;
            _editing = TitleList.SelectedItem as PromptItem;
            _isNew = false;
            LoadEditor(_editing);
        }
        else EnterNewMode();
    }

    /// <summary>儲存目前編輯內容（自動判斷新增或修改），並持久化。成功回傳 true。</summary>
    private bool DoSave()
    {
        if (string.IsNullOrWhiteSpace(TitleBox.Text)) { Warn("prompt.needTitle"); return false; }

        PromptItem target;
        if (_isNew)
        {
            target = new PromptItem();
            _items.Add(target);
            _isNew = false;
        }
        else if (_editing != null) target = _editing;
        else return false;

        ApplyToItem(target);
        _editing = target;
        RefreshView();
        RefreshGroupChoices();

        // Refresh 後重新選回該筆（含新增；群組可能改變位置）
        _switching = true;
        TitleList.SelectedItem = target;
        _switching = false;

        Persist();
        _dirty = false;
        UpdateButtons();
        return true;
    }

    private void Delete_Click(object sender, RoutedEventArgs e)
    {
        if (_isNew || _editing == null) return;
        var r = MessageBox.Show(this, string.Format(Loc.T("prompt.delConfirm"), _editing.Title), Loc.T("prompt.delete"),
            MessageBoxButton.YesNo, MessageBoxImage.Question);
        if (r != MessageBoxResult.Yes) return;

        _items.Remove(_editing);
        _editing = null;
        RefreshView();
        RefreshGroupChoices();
        Persist();

        if (_items.Count > 0)
        {
            _switching = true;
            TitleList.SelectedIndex = 0;
            _switching = false;
            _editing = TitleList.SelectedItem as PromptItem;
            _isNew = false;
            LoadEditor(_editing);
        }
        else EnterNewMode();
    }

    // ---------- 群組管理（清單群組標題右鍵）----------
    private void GroupRename_Click(object sender, RoutedEventArgs e)
    {
        string? oldName = GroupNameFromMenu(sender);
        if (oldName == null || !ConfirmDiscardOrSave()) return;

        string? newName = InputDialog.Show(this, Loc.T("prompt.groupRename"), Loc.T("prompt.groupRenamePrompt"), oldName);
        if (newName == null) return;
        newName = newName.Trim();
        foreach (var p in _items) if (p.Group == oldName) p.Group = newName;

        RefreshView();
        RefreshGroupChoices();
        Persist();
        ReselectCurrent();
    }

    private void GroupDelete_Click(object sender, RoutedEventArgs e)
    {
        string? name = GroupNameFromMenu(sender);
        if (name == null || !ConfirmDiscardOrSave()) return;

        string shown = string.IsNullOrEmpty(name) ? Loc.T("prompt.ungrouped") : name;
        var r = MessageBox.Show(this, string.Format(Loc.T("prompt.groupDeleteAsk"), shown), Loc.T("prompt.groupDelete"),
            MessageBoxButton.YesNoCancel, MessageBoxImage.Question);
        if (r == MessageBoxResult.Cancel) return;

        var inGroup = _items.Where(p => p.Group == name).ToList();
        if (r == MessageBoxResult.Yes)
            foreach (var p in inGroup) _items.Remove(p);       // 連同字串刪除
        else
            foreach (var p in inGroup) p.Group = "";            // 移到未分組

        _editing = null;
        RefreshView();
        RefreshGroupChoices();
        Persist();

        if (_items.Count > 0)
        {
            _switching = true;
            TitleList.SelectedIndex = 0;
            _switching = false;
            _editing = TitleList.SelectedItem as PromptItem;
            _isNew = false;
            LoadEditor(_editing);
        }
        else EnterNewMode();
    }

    /// <summary>從右鍵選單項目取回所屬群組名稱。</summary>
    private static string? GroupNameFromMenu(object sender)
    {
        if (sender is MenuItem mi && mi.Parent is ContextMenu cm
            && cm.PlacementTarget is FrameworkElement fe
            && fe.DataContext is CollectionViewGroup g)
            return g.Name as string;
        return null;
    }

    private void ReselectCurrent()
    {
        var sel = TitleList.SelectedItem as PromptItem;
        _isNew = sel == null && _isNew;
        _editing = sel;
        LoadEditor(sel);
    }

    /// <summary>有未存變更時先問；回傳 false 表示使用者取消。</summary>
    private bool ConfirmDiscardOrSave()
    {
        if (!_dirty) return true;
        var r = MessageBox.Show(this, Loc.T("custom.unsavedAsk"), Loc.T("prompt.title"),
            MessageBoxButton.YesNoCancel, MessageBoxImage.Question);
        if (r == MessageBoxResult.Cancel) return false;
        if (r == MessageBoxResult.Yes) return DoSave();
        _dirty = false; // No → 放棄變更
        return true;
    }

    private void Persist()
    {
        AppSettings.Current.Prompts = _items
            .Where(p => !string.IsNullOrWhiteSpace(p.Title))
            .Select(p => new PromptItem { Title = p.Title, Content = p.Content, Group = p.Group })
            .ToList();
        AppSettings.Current.Save();
    }

    protected override void OnClosing(CancelEventArgs e)
    {
        if (_dirty && DialogResult != true)
        {
            var r = MessageBox.Show(this, Loc.T("custom.unsavedAsk"), Loc.T("prompt.title"),
                MessageBoxButton.YesNoCancel, MessageBoxImage.Question);
            if (r == MessageBoxResult.Cancel) { e.Cancel = true; return; }
            if (r == MessageBoxResult.Yes && !DoSave()) { e.Cancel = true; return; }
        }
        base.OnClosing(e);
    }

    private void Warn(string key)
        => MessageBox.Show(this, Loc.T(key), Loc.T("prompt.title"), MessageBoxButton.OK, MessageBoxImage.Warning);
}
