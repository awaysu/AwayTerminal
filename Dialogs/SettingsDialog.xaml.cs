using System.Linq;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using AwayTerminal.Localization;
using AwayTerminal.Services;

namespace AwayTerminal.Dialogs;

public partial class SettingsDialog : Window
{
    public SettingsDialog()
    {
        InitializeComponent();
        var s = AppSettings.Current;

        // 語言
        if (Loc.Lang == "en") EnRadio.IsChecked = true; else ZhRadio.IsChecked = true;

        // 字型 / 背景
        FontCombo.ItemsSource = Fonts.SystemFontFamilies.Select(f => f.Source).OrderBy(n => n).ToList();
        FontCombo.Text = s.FontFamily;
        for (int i = 8; i <= 28; i++) SizeCombo.Items.Add(i);
        SizeCombo.Text = s.FontSize.ToString();
        FgBox.Text = s.Foreground;
        BgBox.Text = s.Background;

        // Claude 輸入送出：靜止閘門門檻
        ImeQuietBox.Text = s.ImeQuietMs.ToString();

        // 檔案總管右鍵選單
        ShellMenuCheck.IsChecked = s.ExplorerMenu;

        // 本地化
        Title = Loc.T("settings.title");
        LangGroup.Header = Loc.T("settings.groupLang");
        FontGroup.Header = Loc.T("settings.groupFont");
        FamilyLabel.Text = Loc.T("font.family");
        SizeLabel.Text = Loc.T("font.size");
        FgLabel.Text = Loc.T("font.fg");
        BgLabel.Text = Loc.T("font.bg");
        FgPreview.ToolTip = Loc.T("font.pick");
        BgPreview.ToolTip = Loc.T("font.pick");
        ImeGroup.Header = Loc.T("settings.groupIme");
        ImeQuietLabel.Text = Loc.T("settings.imeQuiet");
        ImeHelpLink.Inlines.Clear();
        ImeHelpLink.Inlines.Add(Loc.T("settings.imeQuietHelpLink"));
        ShellGroup.Header = Loc.T("settings.groupShell");
        ShellMenuCheck.Content = Loc.T("settings.shellMenu");
        ResetBtn.Content = Loc.T("common.reset");
        OkBtn.Content = Loc.T("common.ok");
        CancelBtn.Content = Loc.T("common.cancel");
    }

    private void ImeHelp_Click(object sender, RoutedEventArgs e)
        => MessageBox.Show(this, Loc.T("settings.imeQuietHelp"), Loc.T("settings.imeQuietHelpTitle"),
                           MessageBoxButton.OK, MessageBoxImage.Information);

    private void Fg_Changed(object sender, TextChangedEventArgs e) => UpdatePreview(FgBox, FgPreview);
    private void Bg_Changed(object sender, TextChangedEventArgs e) => UpdatePreview(BgBox, BgPreview);
    private void FgPreview_Click(object sender, MouseButtonEventArgs e) => PickColor(FgBox);
    private void BgPreview_Click(object sender, MouseButtonEventArgs e) => PickColor(BgBox);

    private void PickColor(TextBox box)
    {
        using var cd = new System.Windows.Forms.ColorDialog { FullOpen = true, AnyColor = true };
        try
        {
            var c = (Color)ColorConverter.ConvertFromString(box.Text.Trim());
            cd.Color = System.Drawing.Color.FromArgb(c.R, c.G, c.B);
        }
        catch { }
        if (cd.ShowDialog(Win32Owner.Of(this)) == System.Windows.Forms.DialogResult.OK)
        {
            var c = cd.Color;
            box.Text = $"#{c.R:X2}{c.G:X2}{c.B:X2}";
        }
    }

    private static void UpdatePreview(TextBox box, Border preview)
    {
        try { preview.Background = new SolidColorBrush((Color)ColorConverter.ConvertFromString(box.Text.Trim())); }
        catch { }
    }

    private void Reset_Click(object sender, RoutedEventArgs e)
    {
        FontCombo.Text = "Cascadia Mono";
        SizeCombo.Text = "14";
        FgBox.Text = "#E0E0E0";
        BgBox.Text = "#1E1E1E";
        ImeQuietBox.Text = "20";
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        string lang = EnRadio.IsChecked == true ? "en" : "zh";
        var s = AppSettings.Current;
        s.Language = lang;
        s.FontFamily = string.IsNullOrWhiteSpace(FontCombo.Text) ? "Cascadia Mono" : FontCombo.Text.Trim();
        s.FontSize = int.TryParse(SizeCombo.Text, out int sz) && sz is >= 6 and <= 72 ? sz : 14;
        s.Foreground = ValidColor(FgBox.Text, "#E0E0E0");
        s.Background = ValidColor(BgBox.Text, "#1E1E1E");
        s.ImeQuietMs = int.TryParse(ImeQuietBox.Text, out int q) ? System.Math.Clamp(q, 0, 150) : 20;
        s.ExplorerMenu = ShellMenuCheck.IsChecked == true;
        s.Save();

        Loc.SetLang(lang);
        DialogResult = true;
    }

    private static string ValidColor(string text, string fallback)
    {
        try { _ = (Color)ColorConverter.ConvertFromString(text.Trim()); return text.Trim(); }
        catch { return fallback; }
    }
}
