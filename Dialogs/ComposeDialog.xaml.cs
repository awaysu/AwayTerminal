using System.Collections.Generic;
using System.Linq;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Threading;
using AwayTerminal.Localization;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

/// <summary>工具列「輸入文字」開的視窗（1.0.30 為分頁列最左「…」，1.0.46 移到工具列並併入常用字串）：
/// 先在一般 TextBox 把文字（含中文、多行）打好，按「送出」才整段貼進作用中分頁。
/// 用途＝繞過 claude 對逐鍵 IME 輸入的重複／亂碼問題（IME 組字與提交都發生在 WPF TextBox，
/// xterm／ConPTY 只會收到最後一次整段貼上）。
/// <para>上方常用字串列（取代原本工具列右端的快速輸入抽屜）：群組／標題下拉 →「插入」把該字串插進文字框
/// 游標處（可再修改）；「直接送出」不經文字框、依該字串自己的「送出後送 Enter」設定立刻送到目前分頁，
/// 視窗留著可以連續送好幾條。下拉列下方的唯讀 TextBox 顯示所選字串內容（可選取複製、不可改），
/// 與輸入框以 2:3 分高度。上次選的群組／標題記在 static，再開視窗直接按「直接送出」就能重送。</para>
/// <para>按鈕：清除（整段刪掉，可復原）／復原（還原上一步編輯，含清除；Ctrl+Z 同效）／返回（不送）／送出。
/// 草稿保留：打到一半按 X 或「返回」關掉，下次叫回來文字還在（`_draft`，程式存活期間有效）；
/// 「送出」後才清空。「清除」掉的內容另存 `_lastCleared`，即使關掉重開（undo 堆疊已是新的）
/// 按「復原」仍能把它找回來。</para></summary>
public partial class ComposeDialog : Window
{
    private static string _draft = "";        // 未送出的草稿（X／返回 後保留）
    private static string _lastCleared = "";  // 最近一次「清除」掉的內容（跨開關也能復原）
    private static string? _lastGroup;        // 上次選的常用字串群組／標題（程式存活期間）
    private static string? _lastTitle;

    private readonly IReadOnlyList<PromptItem> _all;
    private readonly Action<string, bool> _sendNow;   // 直接送出：內容, 送出後送 Enter（由 MainWindow.SendSnippet 實作）
    private List<PromptItem> _items = new();          // 目前群組篩出的字串（與 TitleCombo 同序）
    private DispatcherTimer? _sentTimer;

    public string TextToSend => Input.Text;
    public bool SendEnter => SendEnterChk.IsChecked == true;

    public ComposeDialog(bool sendEnter, IReadOnlyList<PromptItem> snippets, Action<string, bool> sendNow)
    {
        InitializeComponent();
        _all = snippets;
        _sendNow = sendNow;

        Title = Loc.T("compose.title");
        HintText.Text = Loc.T("compose.hint");
        SendEnterChk.Content = Loc.T("prompt.sendEnter");
        SendEnterChk.IsChecked = sendEnter;
        ClearBtn.Content = Loc.T("compose.clear");
        UndoBtn.Content = Loc.T("compose.undo");
        BackBtn.Content = Loc.T("compose.back");
        SendBtn.Content = Loc.T("compose.send");
        GroupLabel.Text = Loc.T("prompt.group");
        TitleLabel.Text = Loc.T("prompt.header");
        InsertBtn.Content = Loc.T("compose.insert");
        SendNowBtn.Content = Loc.T("compose.sendNow");
        RefreshGroups();

        // 帶回上次的草稿；把 undo 堆疊歸零，免得第一下「復原」把草稿整段退掉
        Input.Text = _draft;
        Input.IsUndoEnabled = false;
        Input.IsUndoEnabled = true;
        Input.CaretIndex = Input.Text.Length;

        Loaded += (_, _) => Input.Focus();
        Input.PreviewKeyDown += (_, e) =>
        {
            if (e.Key == Key.Enter && (Keyboard.Modifiers & ModifierKeys.Control) != 0)
            {
                e.Handled = true;
                Send();
            }
        };
        // 關閉時記草稿：送出＝清空；X／返回＝保留目前內容
        Closing += (_, _) => { _draft = DialogResult == true ? "" : Input.Text; _sentTimer?.Stop(); };
    }

    // ---------- 常用字串列 ----------
    private void RefreshGroups()
    {
        var groups = _all
            .Select(p => string.IsNullOrWhiteSpace(p.Group) ? Loc.T("prompt.ungrouped") : p.Group)
            .Distinct().OrderBy(g => g).ToList();
        var items = new List<string> { Loc.T("quick.all") };
        items.AddRange(groups);
        GroupCombo.ItemsSource = items;
        GroupCombo.SelectedItem = _lastGroup != null && items.Contains(_lastGroup) ? _lastGroup : items[0];
        // SelectedItem 變更會觸發 Group_Changed → RefreshTitles；同值（第一次）也補呼叫一次
        RefreshTitles();
    }

    private void RefreshTitles()
    {
        string g = GroupCombo.SelectedItem as string ?? Loc.T("quick.all");
        IEnumerable<PromptItem> src = _all;
        if (g == Loc.T("prompt.ungrouped")) src = src.Where(p => string.IsNullOrWhiteSpace(p.Group));
        else if (g != Loc.T("quick.all")) src = src.Where(p => p.Group == g);
        _items = src.ToList();
        TitleCombo.ItemsSource = _items.Select(p => p.Title).ToList();
        int idx = _lastTitle != null ? _items.FindIndex(p => p.Title == _lastTitle) : -1;
        TitleCombo.SelectedIndex = _items.Count == 0 ? -1 : (idx >= 0 ? idx : 0);
        InsertBtn.IsEnabled = SendNowBtn.IsEnabled = _items.Count > 0;
        UpdatePreview();
    }

    private PromptItem? Selected
        => TitleCombo.SelectedIndex >= 0 && TitleCombo.SelectedIndex < _items.Count ? _items[TitleCombo.SelectedIndex] : null;

    /// <summary>唯讀預覽框＝所選字串的完整內容，不用先插入就知道會送什麼。</summary>
    private void UpdatePreview()
    {
        var p = Selected;
        PreviewBox.Text = p?.Content ?? "";
    }

    private void Group_Changed(object sender, SelectionChangedEventArgs e)
    {
        _lastGroup = GroupCombo.SelectedItem as string;
        RefreshTitles();
    }

    private void Title_Changed(object sender, SelectionChangedEventArgs e)
    {
        var p = Selected;
        if (p != null) _lastTitle = p.Title;
        UpdatePreview();
    }

    /// <summary>插入：把所選字串放進文字框游標處（有選取則取代）。走 SelectedText 所以進 undo 堆疊、Ctrl+Z 救得回來。</summary>
    private void Insert_Click(object sender, RoutedEventArgs e)
    {
        var p = Selected;
        if (p == null) return;
        Input.SelectedText = p.Content;
        Input.CaretIndex = Input.SelectionStart + Input.SelectionLength;
        Input.SelectionLength = 0;
        Input.Focus();
    }

    /// <summary>直接送出：所選字串不經文字框、依它自己的「送出後送 Enter」立刻送到目前分頁；視窗留著可連送。
    /// 按鈕短暫變成「✓ 已送出」當回饋。</summary>
    private void SendNow_Click(object sender, RoutedEventArgs e)
    {
        var p = Selected;
        if (p == null || string.IsNullOrEmpty(p.Content)) return;
        _sendNow(p.Content, p.SendEnter);

        SendNowBtn.Content = Loc.T("compose.sent");
        SendNowBtn.IsEnabled = false;
        _sentTimer ??= new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(900) };
        _sentTimer.Tick -= SentTimer_Tick;
        _sentTimer.Tick += SentTimer_Tick;
        _sentTimer.Stop();
        _sentTimer.Start();
    }

    private void SentTimer_Tick(object? sender, EventArgs e)
    {
        _sentTimer?.Stop();
        SendNowBtn.Content = Loc.T("compose.sendNow");
        SendNowBtn.IsEnabled = _items.Count > 0;
    }

    // ---------- 文字框 ----------
    private void Send_Click(object sender, RoutedEventArgs e) => Send();

    private void Send()
    {
        if (string.IsNullOrEmpty(Input.Text)) return;   // 空白不送（想只送 Enter 請直接在終端機按）
        DialogResult = true;
    }

    /// <summary>清除：整段刪掉。走「全選＋取代選取」而非直接設 Text，確保進 undo 堆疊，Ctrl+Z／復原救得回來。</summary>
    private void Clear_Click(object sender, RoutedEventArgs e)
    {
        if (Input.Text.Length > 0)
        {
            _lastCleared = Input.Text;
            Input.SelectAll();
            Input.SelectedText = "";
        }
        Input.Focus();
    }

    /// <summary>復原：先走 TextBox 自己的 undo（逐步還原，含清除）；堆疊空了（例如清除後關掉再開）
    /// 而框是空的 → 把最近一次清除的內容整段放回。</summary>
    private void Undo_Click(object sender, RoutedEventArgs e)
    {
        if (Input.CanUndo) Input.Undo();
        else if (Input.Text.Length == 0 && _lastCleared.Length > 0) Input.Text = _lastCleared;
        Input.CaretIndex = Input.Text.Length;
        Input.Focus();
    }
}
