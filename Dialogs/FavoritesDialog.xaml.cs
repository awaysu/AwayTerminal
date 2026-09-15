using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using AwayTerminal.Localization;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

/// <summary>
/// 工具列「我的最愛 → 設定…」（使用者要求，2026-09-16）：修改名稱、刪除。直接改 AppSettings.Current.Favorites 並立即存檔。
/// 新增是在下拉的「加到我的最愛」（從目前分頁），這裡不做。
/// </summary>
public partial class FavoritesDialog : Window
{
    private readonly List<FavoriteItem> _items;
    private readonly Func<FavoriteItem, string> _iconOf;
    private readonly Func<FavoriteItem, string> _detailOf;

    public FavoritesDialog(List<FavoriteItem> items, Func<FavoriteItem, string> iconOf, Func<FavoriteItem, string> detailOf)
    {
        InitializeComponent();
        _items = items;
        _iconOf = iconOf;
        _detailOf = detailOf;

        Title = Loc.T("fav.title");
        NameLabel.Text = Loc.T("fav.name");
        RenameBtn.Content = Loc.T("fav.rename");
        DeleteBtn.Content = Loc.T("fav.delete");
        CloseBtn.Content = Loc.T("fav.close");
        EmptyText.Text = Loc.T("fav.none");

        Refill(0);
    }

    /// <summary>重建清單並選 index 那一筆（超出範圍就選最後一筆）。</summary>
    private void Refill(int select)
    {
        List.Items.Clear();
        foreach (var f in _items) List.Items.Add(BuildRow(f));
        bool any = _items.Count > 0;
        EmptyText.Visibility = any ? Visibility.Collapsed : Visibility.Visible;
        List.Visibility = any ? Visibility.Visible : Visibility.Hidden;
        if (any) List.SelectedIndex = Math.Clamp(select, 0, _items.Count - 1);
        UpdateEditor();
    }

    private FrameworkElement BuildRow(FavoriteItem f)
    {
        var row = new DockPanel { Margin = new Thickness(4, 5, 4, 5), Tag = f };
        try
        {
            var img = new Image
            {
                Source = new System.Windows.Media.Imaging.BitmapImage(new Uri($"pack://application:,,,/icon/{_iconOf(f)}")),
                Width = 26, Height = 26, Margin = new Thickness(0, 0, 10, 0), VerticalAlignment = VerticalAlignment.Center
            };
            DockPanel.SetDock(img, Dock.Left);
            row.Children.Add(img);
        }
        catch { }
        var text = new StackPanel();
        text.Children.Add(new TextBlock { Text = f.Name, FontSize = 14, TextTrimming = TextTrimming.CharacterEllipsis });
        text.Children.Add(new TextBlock
        {
            Text = _detailOf(f), FontSize = 11, Foreground = new SolidColorBrush(Color.FromRgb(0x9A, 0x9A, 0x9A)),
            TextTrimming = TextTrimming.CharacterEllipsis
        });
        row.Children.Add(text);
        row.ToolTip = _detailOf(f);
        return row;
    }

    private FavoriteItem? Selected => (List.SelectedItem as FrameworkElement)?.Tag as FavoriteItem;

    private void List_SelectionChanged(object sender, SelectionChangedEventArgs e) => UpdateEditor();

    private void UpdateEditor()
    {
        var f = Selected;
        NameBox.Text = f?.Name ?? "";
        NameBox.IsEnabled = RenameBtn.IsEnabled = DeleteBtn.IsEnabled = f != null;
    }

    private void Rename_Click(object sender, RoutedEventArgs e)
    {
        if (Selected is not { } f) return;
        string name = NameBox.Text.Trim();
        if (name.Length == 0 || name == f.Name) return;
        int idx = _items.IndexOf(f);
        f.Name = name;
        AppSettings.Current.Save();
        Refill(idx);
    }

    private void Delete_Click(object sender, RoutedEventArgs e)
    {
        if (Selected is not { } f) return;
        if (MessageBox.Show(this, string.Format(Loc.T("fav.deleteAsk"), f.Name), Loc.T("fav.title"),
                MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
        int idx = _items.IndexOf(f);
        _items.Remove(f);
        AppSettings.Current.Save();
        Refill(idx);
    }
}
