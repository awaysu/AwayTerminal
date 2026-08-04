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

        // ADB 路徑（留空＝自動偵測）。提示行直接顯示目前實際會用到哪一支，
        // 使用者不必猜自動偵測有沒有找到東西。
        AdbShowChk.IsChecked = s.AdbEnabled;
        AdbBox.Text = s.AdbPath;
        RefreshAdbHint();

        // 本地化
        Title = Loc.T("settings.title");
        LangGroup.Header = Loc.T("settings.groupLang");
        FontGroup.Header = Loc.T("settings.groupFont");
        AdbGroup.Header = Loc.T("settings.groupAdb");
        AdbShowChk.Content = Loc.T("settings.adbEnable");
        AdbLabel.Text = Loc.T("settings.adbPath");
        AdbBrowseBtn.ToolTip = Loc.T("common.browse");
        FamilyLabel.Text = Loc.T("font.family");
        SizeLabel.Text = Loc.T("font.size");
        FgLabel.Text = Loc.T("font.fg");
        BgLabel.Text = Loc.T("font.bg");
        FgPreview.ToolTip = Loc.T("font.pick");
        BgPreview.ToolTip = Loc.T("font.pick");
        ResetBtn.Content = Loc.T("common.reset");
        OkBtn.Content = Loc.T("common.ok");
        CancelBtn.Content = Loc.T("common.cancel");
    }

    /// <summary>提示行：顯示自動偵測到的 adb（或找不到），讓「留空」不是黑箱。</summary>
    private void RefreshAdbHint()
    {
        string? found = AppSettings.ResolveAdbPath();
        AdbHint.Text = found == null
            ? Loc.T("settings.adbNotFound")
            : Loc.T("settings.adbUsing") + " " + found;
    }

    private void AdbBrowse_Click(object sender, RoutedEventArgs e)
    {
        using var d = new System.Windows.Forms.OpenFileDialog
        {
            Title = Loc.T("settings.adbPath"),
            Filter = "adb.exe|adb.exe|*.exe|*.exe",
            CheckFileExists = true
        };
        if (!string.IsNullOrWhiteSpace(AdbBox.Text))
        {
            try { d.InitialDirectory = System.IO.Path.GetDirectoryName(AdbBox.Text.Trim()); } catch { }
        }
        // 指定擁有者，否則對話框可能開在後面（見 Win32Owner）
        if (d.ShowDialog(Win32Owner.Of(this)) == System.Windows.Forms.DialogResult.OK) AdbBox.Text = d.FileName;
    }

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
        AdbBox.Text = "";       // 回到自動偵測
        RefreshAdbHint();
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
        s.AdbEnabled = AdbShowChk.IsChecked == true;
        s.AdbPath = AdbBox.Text.Trim();   // 空字串＝自動偵測
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
