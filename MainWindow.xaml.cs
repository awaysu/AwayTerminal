using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using AwayTerminal.ConPty;
using AwayTerminal.Dialogs;
using AwayTerminal.Localization;
using AwayTerminal.Logging;
using AwayTerminal.Macros;
using AwayTerminal.Models;
using AwayTerminal.Services;
using AwayTerminal.Sessions;
using Microsoft.Web.WebView2.Core;

namespace AwayTerminal;

/// <summary>
/// 主視窗：WPF 原生工具列 + 分頁列；單一 WebView2 內以多個 xterm 依分頁切換。
/// </summary>
public partial class MainWindow : Window, IRemoteHost
{
    private const char US = '\u001f';

    public ObservableCollection<TerminalTab> Tabs { get; } = new();
    private TerminalTab? _active;
    private int _nextId = 1;
    private bool _webReady;
    private string _viewMode = "tab"; // tab | split | columns
    private bool _splitMode => _viewMode != "tab"; // 分割或分欄時終端機外框讓給各 pane
    private string _webRoot = "";
    private int _lastCols = 80, _lastRows = 24; // 前端最近回報的終端機尺寸（新分頁初始值用）
    private DispatcherTimer? _statusTimer;
    private DispatcherTimer? _copyPopupTimer;
    private List<PromptItem> _quickItems = new(); // 快速輸入面板目前群組的字串

    // 遠端（Telegram）：服務 + 忙碌起始時間（用來過濾閃爍、只在忙碌≥3秒後推播閒置）
    private TelegramRemote? _remote;
    private readonly Dictionary<int, DateTime> _busySince = new();
    private TaskCompletionSource<string>? _remoteTextTcs; // q…text 查詢的等待中回覆

    /// <summary>依型態產生分頁預設名稱「{prefix}({n})」，跳過已存在名稱。例：PowerShell(1)。</summary>
    private string NextName(string prefix)
    {
        int n = 0;
        string name;
        do { name = $"{prefix}({++n})"; } while (Tabs.Any(t => t.Title == name));
        return name;
    }

    // 在指定按鈕旁邊浮出提示（複製成功等）
    private void ShowCopyFeedback(FrameworkElement target, string msg)
    {
        CopyPopupText.Text = msg;
        CopyPopup.PlacementTarget = target;
        CopyPopup.IsOpen = false;
        CopyPopup.IsOpen = true;
        _copyPopupTimer ??= new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(1200) };
        _copyPopupTimer.Stop();
        _copyPopupTimer.Tick -= HideCopyPopup;
        _copyPopupTimer.Tick += HideCopyPopup;
        _copyPopupTimer.Start();
    }

    private void HideCopyPopup(object? sender, EventArgs e)
    {
        _copyPopupTimer?.Stop();
        CopyPopup.IsOpen = false;
    }

    public MainWindow()
    {
        InitializeComponent();
        Loc.Init(AppSettings.Current.Language);
        Loc.Changed += ApplyLoc;
        ApplyLoc();
        ApplyToolbarPosition();   // 依上次記憶還原功能列位置
        Loaded += OnLoaded;
        Closing += OnClosingAsk;
        Closed += OnClosed;
        // 快速輸入面板開啟時，視窗大小改變則同步調整寬度（1/5）
        SizeChanged += (_, _) => { if (QuickPanel.Visibility == Visibility.Visible) QuickPanel.Width = Math.Max(200, ActualWidth / 5.0); };
    }

    /// <summary>關閉時彈自訂視窗；確認後快速收尾並立即結束（跳過 WebView2 冗長 teardown）。</summary>
    private void OnClosingAsk(object? sender, System.ComponentModel.CancelEventArgs e)
    {
        e.Cancel = true; // 先攔下，改由自訂視窗決定

        var dlg = new ExitDialog { Owner = this };
        dlg.SetClaudeAvailable(Tabs.Any(t => t.Restore?.Type == "claude" && t.Session != null));
        dlg.UpdateAction = UpdateClaudeMdAsync;
        if (dlg.ShowDialog() != true) return; // 取消 → 不關

        // 記住兩個勾選狀態，下次開啟關閉視窗時沿用
        AppSettings.Current.ExitRestoreTabs = dlg.RestoreTabs;
        AppSettings.Current.ExitUpdateMd = dlg.UpdateMd;

        AppSettings.Current.SavedTabs = dlg.RestoreTabs
            ? Tabs.Where(t => t.Restore != null)
                  .Select(t => { var s2 = t.Restore!; s2.Title = t.Title; return s2; }).ToList()
            : new List<SavedTab>();
        AppSettings.Current.Save();

        // 快速收尾：終止子行程（不送 Ctrl+C、不等待），然後立即結束程式
        _statusTimer?.Stop();
        _remote?.Stop();
        foreach (var t in Tabs)
        {
            try { (t.Macro as MacroRunner)?.Stop(); } catch { }
            try { (t.Logger as SessionLogger)?.Dispose(); } catch { }
            try
            {
                if (t.Session is ConPtySession cps) cps.GracefulExitBytes = Array.Empty<byte>();
                t.Session?.Dispose();
            }
            catch { }
        }
        Environment.Exit(0); // 立即終止，msedgewebview2 等子程序會隨之結束
    }

    /// <summary>請每個 Claude Code 分頁更新 CLAUDE.md，並等到它們都閒置（或逾時）。</summary>
    private async Task UpdateClaudeMdAsync()
    {
        var claudeTabs = Tabs.Where(t => t.Restore?.Type == "claude" && t.Session != null).ToList();
        if (claudeTabs.Count == 0) return;

        string prompt = Loc.T("exit.mdPrompt");
        foreach (var t in claudeTabs)
        {
            t.LastOutputUtc = DateTime.UtcNow;      // 重置活動計時
            t.Session!.WriteText(prompt + "\r");
        }

        await Task.Delay(2500);                     // 給 claude 一點時間開始處理
        var deadline = DateTime.UtcNow.AddSeconds(180); // 最多等 3 分鐘
        while (DateTime.UtcNow < deadline)
        {
            // 全部 claude 分頁都超過 4 秒沒新輸出 = 視為更新完成
            if (claudeTabs.All(t => (DateTime.UtcNow - t.LastOutputUtc).TotalMilliseconds > 4000)) break;
            await Task.Delay(500);
        }
    }

    private void ApplyLoc()
    {
        Title = string.IsNullOrEmpty(_titlePath) ? Loc.T("app.name") : $"{Loc.T("app.name")} - {_titlePath}";
        BtnNew.Content = Loc.T("tb.new"); BtnNew.ToolTip = Loc.T("tip.new");
        BtnHistory.Content = Loc.T("tb.history"); BtnHistory.ToolTip = Loc.T("tip.history");
        BtnCopy.Content = Loc.T("tb.copy"); BtnCopy.ToolTip = Loc.T("tip.copy");
        BtnPaste.Content = Loc.T("tb.paste"); BtnPaste.ToolTip = Loc.T("tip.paste");
        BtnCopyAll.Content = Loc.T("tb.copyall"); BtnCopyAll.ToolTip = Loc.T("tip.copyall");
        BtnClear.Content = Loc.T("tb.clear"); BtnClear.ToolTip = Loc.T("tip.clear");
        BtnPrompt.Content = Loc.T("tb.prompt"); BtnPrompt.ToolTip = Loc.T("tip.prompt");
        BtnRemote.Content = Loc.T("tb.remote"); BtnRemote.ToolTip = Loc.T("tip.remote");
        BtnSettings.Content = Loc.T("tb.settings"); BtnSettings.ToolTip = Loc.T("tip.settings");
        BtnAbout.Content = Loc.T("tb.about"); BtnAbout.ToolTip = Loc.T("tip.about");
        BtnTaskbar.Content = Loc.T("tb.taskbar"); BtnTaskbar.ToolTip = Loc.T("tip.taskbar");
        QuickGroupLabel.Text = Loc.T("prompt.group");
        QuickTitleLabel.Text = Loc.T("prompt.header");
        QuickContentLabel.Text = Loc.T("prompt.content");
        QuickSendBtn.Content = Loc.T("prompt.send");
        UpdateSplitButton();
    }

    /// <summary>「New」按鈕：下拉選單（圖示＋文字）。
    /// 內建：PowerShell / SSH-Telnet / 連接埠 / ADB；分隔線後接自訂連線與「自訂…」管理。</summary>
    private void New_Click(object sender, RoutedEventArgs e)
    {
        var menu = new ContextMenu();
        menu.Items.Add(MakeNewItem("tb.powershell", "powershell.png", OpenPowerShell_Click));
        menu.Items.Add(MakeNewItem("tb.ssh", "ssh-telnet.png", OpenSsh_Click));
        menu.Items.Add(MakeNewItem("tb.com", "com.png", OpenCom_Click));
        menu.Items.Add(MakeNewItem("tb.adb", "adb.png", OpenAdb_Click));

        // 自訂連線（未勾「不顯示」者）→ 點擊直接開；有項目時才加分隔線群組
        var customs = AppSettings.Current.CustomConns
            .Where(c => !c.Hidden && !string.IsNullOrWhiteSpace(c.Name)).ToList();
        if (customs.Count > 0)
        {
            menu.Items.Add(new Separator());
            foreach (var c in customs)
            {
                var conn = c;
                string iconFile = string.IsNullOrWhiteSpace(conn.Icon) ? "run.png" : conn.Icon + ".png";
                var mi = MakeNewItemRaw(conn.Name, iconFile);
                mi.Click += (_, _) => OpenCustom(conn);
                menu.Items.Add(mi);
            }
        }
        // 「自訂…」上方一律加分隔線 → 開管理視窗
        menu.Items.Add(new Separator());
        var manage = MakeNewItemRaw(Loc.T("menu.custom"), "settings.png");
        manage.Click += Custom_Click;
        menu.Items.Add(manage);

        bool right = AppSettings.Current.ToolbarPosition == "right";
        menu.PlacementTarget = (UIElement)sender;
        menu.Placement = right
            ? System.Windows.Controls.Primitives.PlacementMode.Right
            : System.Windows.Controls.Primitives.PlacementMode.Bottom;
        menu.IsOpen = true;
    }

    private MenuItem MakeNewItem(string locKey, string iconFile, RoutedEventHandler handler)
    {
        var mi = MakeNewItemRaw(Loc.T(locKey), iconFile);
        mi.Click += handler;
        return mi;
    }

    /// <summary>建立下拉選單項目：圖示放進 Header（非 MenuItem.Icon，避免被固定尺寸圖示欄裁切）；圖左字右。</summary>
    private MenuItem MakeNewItemRaw(string headerText, string iconFile)
    {
        var panel = new StackPanel { Orientation = Orientation.Horizontal };
        try
        {
            panel.Children.Add(new Image
            {
                Source = new System.Windows.Media.Imaging.BitmapImage(
                    new Uri($"pack://application:,,,/icon/{iconFile}")),
                Width = 26,
                Height = 26,
                Margin = new Thickness(0, 0, 10, 0),
                VerticalAlignment = VerticalAlignment.Center
            });
        }
        catch { }
        panel.Children.Add(new TextBlock
        {
            Text = headerText,
            VerticalAlignment = VerticalAlignment.Center,
            FontSize = 11  // 與工具列按鈕文字一致
        });
        return new MenuItem { Header = panel };
    }

    /// <summary>開一筆自訂連線。ViaPowerShell 或 .cmd/.bat → 走 PowerShell；否則直接以 ConPTY 執行。
    /// forcedDir：直接指定工作目錄並略過「啟動前選擇資料夾」（遠端開啟用）。</summary>
    private void OpenCustom(CustomConn conn, string? forcedDir = null)
    {
        if (!_webReady) return;
        string path = conn.Path;
        bool viaPs = conn.ViaPowerShell
            || path.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase)
            || path.EndsWith(".bat", StringComparison.OrdinalIgnoreCase);

        if (!viaPs && !File.Exists(path))
        {
            MessageBox.Show(this, Loc.T("custom.notFound") + "\n" + path, "AwayTerminal",
                MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }

        string? dir = forcedDir;
        if (dir == null && conn.PickDir)
        {
            dir = PickWorkDir(conn.Name);
            if (dir == null) return; // 使用者取消 → 不開
        }

        string args = string.IsNullOrWhiteSpace(conn.Args) ? "" : " " + conn.Args.Trim();
        string title = NextName(string.IsNullOrWhiteSpace(conn.Name) ? "Custom" : conn.Name);

        // 關閉分頁時送的鍵：無 / Ctrl+C(0x03) / Ctrl+D(0x04) × 次數
        byte[] closeBytes;
        if (conn.CloseKey == "none")
            closeBytes = Array.Empty<byte>();
        else
        {
            byte closeByte = conn.CloseKey == "ctrl-d" ? (byte)0x04 : (byte)0x03;
            int closeCount = conn.CloseCount is >= 1 and <= 5 ? conn.CloseCount : 3;
            closeBytes = Enumerable.Repeat(closeByte, closeCount).ToArray();
        }

        if (viaPs)
        {
            var s = new ConPtySession { GracefulExitBytes = closeBytes };
            var tab = StartTab(TermKind.PowerShell, title, s, () => s.Start("powershell.exe", _lastCols, _lastRows, dir),
                claudePaste: IsClaudeExe(path));
            if (tab == null) return;
            if (dir != null) { tab.WorkDir = dir; SetTitlePath(dir); }
            // 等尺寸就緒後才把指令打進 PowerShell（避免以 80 欄啟動）；1 秒後保險送出
            tab.PendingCommand = $"& \"{path}\"{args}";
            var timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
            timer.Tick += (_, _) =>
            {
                timer.Stop();
                if (tab.PendingCommand != null && tab.Session != null)
                {
                    string cmd = tab.PendingCommand; tab.PendingCommand = null;
                    tab.Session.WriteText(cmd + "\r");
                }
            };
            timer.Start();
        }
        else
        {
            var s = new ConPtySession { GracefulExitBytes = closeBytes };
            var tab = StartTab(TermKind.Custom, title, s, () => s.Start($"\"{path}\"{args}", _lastCols, _lastRows, dir),
                claudePaste: IsClaudeExe(path));
            if (tab != null && dir != null) { tab.WorkDir = dir; SetTitlePath(dir); }
        }
        AddHistory(new SavedTab
        {
            Type = "custom", Title = conn.Name, Path = path, Args = conn.Args, Icon = conn.Icon,
            PickDir = conn.PickDir, ViaPowerShell = conn.ViaPowerShell,
            CloseKey = conn.CloseKey, CloseCount = conn.CloseCount
        });
        // 自訂分頁暫不做開機還原（不設 Restore）
    }

    private void Custom_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new CustomConnDialog { Owner = this };
        dlg.ShowDialog(); // New 下拉每次開都讀最新 AppSettings，故不需額外刷新
    }

    // ---------- 常用字串快速輸入（右側抽屜）----------
    private void QuickToggle_Click(object sender, RoutedEventArgs e)
    {
        if (QuickPanel.Visibility == Visibility.Visible)
        {
            QuickPanel.Visibility = Visibility.Collapsed;
            QuickToggle.Content = "▼"; // ▼ 收合（點按展開）
        }
        else
        {
            QuickPanel.Width = Math.Max(200, ActualWidth / 5.0);
            RefreshQuickGroups();
            QuickPanel.Visibility = Visibility.Visible;
            QuickToggle.Content = "▲"; // ▲ 展開中（點按收合）
        }
    }

    private void RefreshQuickGroups()
    {
        var groups = AppSettings.Current.Prompts
            .Select(p => string.IsNullOrWhiteSpace(p.Group) ? Loc.T("prompt.ungrouped") : p.Group)
            .Distinct().OrderBy(g => g).ToList();
        var items = new List<string> { Loc.T("quick.all") };
        items.AddRange(groups);
        string? keep = QuickGroupCombo.SelectedItem as string;
        QuickGroupCombo.ItemsSource = items;
        QuickGroupCombo.SelectedItem = keep != null && items.Contains(keep) ? keep : items[0];
        RefreshQuickTitles();
    }

    private void RefreshQuickTitles()
    {
        string g = QuickGroupCombo.SelectedItem as string ?? Loc.T("quick.all");
        IEnumerable<PromptItem> src = AppSettings.Current.Prompts;
        if (g == Loc.T("prompt.ungrouped")) src = src.Where(p => string.IsNullOrWhiteSpace(p.Group));
        else if (g != Loc.T("quick.all")) src = src.Where(p => p.Group == g);
        _quickItems = src.ToList();
        QuickTitleCombo.ItemsSource = _quickItems.Select(p => p.Title).ToList();
        if (_quickItems.Count > 0) QuickTitleCombo.SelectedIndex = 0;
        else { QuickTitleCombo.SelectedIndex = -1; QuickContentBox.Text = ""; }
    }

    private void QuickGroup_SelectionChanged(object sender, SelectionChangedEventArgs e) => RefreshQuickTitles();

    private void QuickTitle_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        int i = QuickTitleCombo.SelectedIndex;
        QuickContentBox.Text = (i >= 0 && i < _quickItems.Count) ? _quickItems[i].Content : "";
    }

    private void QuickSend_Click(object sender, RoutedEventArgs e)
    {
        string content = QuickContentBox.Text;
        if (string.IsNullOrWhiteSpace(content)) return;
        PasteToActive(content);   // 常用字串常含多行 → 同樣走 bracketed paste
        Web.Focus();
    }

    // ---------- 連線紀錄（「紀錄」按鈕）----------
    /// <summary>記錄一次連線（最新在前、去重、上限 20）。存的是複本，避免與分頁 Restore 共用物件。</summary>
    private void AddHistory(SavedTab e)
    {
        var h = AppSettings.Current.History;
        string key = HistoryKey(e);
        h.RemoveAll(x => HistoryKey(x) == key);
        h.Insert(0, CloneTab(e));
        while (h.Count > 20) h.RemoveAt(h.Count - 1);
        AppSettings.Current.Save();
    }

    private static SavedTab CloneTab(SavedTab s) => new()
    {
        Type = s.Type, Title = s.Title, Dir = s.Dir, Host = s.Host, Port = s.Port,
        ComPort = s.ComPort, Baud = s.Baud, DataBits = s.DataBits, Parity = s.Parity,
        StopBits = s.StopBits, Flow = s.Flow, AdbSerial = s.AdbSerial, Path = s.Path,
        Args = s.Args, Icon = s.Icon, PickDir = s.PickDir, ViaPowerShell = s.ViaPowerShell,
        CloseKey = s.CloseKey, CloseCount = s.CloseCount
    };

    private static string HistoryKey(SavedTab e) => e.Type switch
    {
        "ps" => "ps|" + e.Dir,
        "claude" => "claude|" + e.Dir,
        "ssh" => "ssh|" + e.Host + "|" + e.Port,
        "telnet" => "telnet|" + e.Host + "|" + e.Port,
        "com" => "com|" + e.ComPort + "|" + e.Baud,
        "adb" => "adb|" + e.AdbSerial,
        "custom" => "custom|" + e.Title + "|" + e.Path,
        _ => e.Type + "|" + e.Title
    };

    private static string HistoryLabel(SavedTab e) => e.Type switch
    {
        "ps" => "PowerShell — " + ShortDir(e.Dir),
        "claude" => "ClaudeCode — " + ShortDir(e.Dir),
        "ssh" => e.Host,
        "telnet" => $"{e.Host}:{e.Port}",
        "com" => $"{e.ComPort} {e.Baud}",
        "adb" => "ADB" + (string.IsNullOrEmpty(e.AdbSerial) ? "" : " " + e.AdbSerial),
        "custom" => e.Title,
        _ => e.Title
    };

    private static string HistoryIcon(SavedTab e) => e.Type switch
    {
        "ps" => "powershell.png",
        "claude" => "claude-code.png",
        "ssh" or "telnet" => "ssh-telnet.png",
        "com" => "com.png",
        "adb" => "adb.png",
        "custom" => string.IsNullOrWhiteSpace(e.Icon) ? "run.png" : e.Icon + ".png",
        _ => "new-connecting.png"
    };

    private static string ShortDir(string dir)
    {
        if (string.IsNullOrEmpty(dir)) return "";
        try { var n = Path.GetFileName(dir.TrimEnd('\\', '/')); return string.IsNullOrEmpty(n) ? dir : n; }
        catch { return dir; }
    }

    /// <summary>「紀錄」按鈕：下拉顯示最近 10 次連線（圖示＋標籤），點選重開。</summary>
    private void History_Click(object sender, RoutedEventArgs e)
    {
        var menu = new ContextMenu();
        var list = AppSettings.Current.History.Take(10).ToList();
        if (list.Count == 0)
        {
            menu.Items.Add(new MenuItem { Header = Loc.T("history.empty"), IsEnabled = false });
        }
        else
        {
            foreach (var h in list)
            {
                var entry = h;
                var mi = MakeNewItemRaw(HistoryLabel(entry), HistoryIcon(entry));
                mi.Click += (_, _) => ReopenHistory(entry);
                menu.Items.Add(mi);
            }
        }
        bool right = AppSettings.Current.ToolbarPosition == "right";
        menu.PlacementTarget = (UIElement)sender;
        menu.Placement = right
            ? System.Windows.Controls.Primitives.PlacementMode.Right
            : System.Windows.Controls.Primitives.PlacementMode.Bottom;
        menu.IsOpen = true;
    }

    private void ReopenHistory(SavedTab e)
    {
        if (!_webReady) return;
        string deskDir() => Directory.Exists(e.Dir) ? e.Dir : Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);
        switch (e.Type)
        {
            case "ps": OpenPowerShellDirect(deskDir(), NextName("PowerShell")); break;
            case "claude": OpenClaudeDirect(deskDir(), NextName("Claude")); break;
            case "ssh": OpenSshLoginAs(e.Host, e.Port); break;
            case "telnet": OpenTelnetDirect(e.Host, e.Port); break;
            case "com": OpenComDirect(e.ComPort, e.Baud, e.DataBits, e.Parity, e.StopBits, e.Flow); break;
            case "adb":
            {
                string? adb = AppSettings.ResolveAdbPath();
                if (adb == null) { PromptInstallAdb(); break; }
                OpenAdbShell(adb, string.IsNullOrEmpty(e.AdbSerial) ? null : e.AdbSerial, NextName("ADB"));
                break;
            }
            case "custom":
                OpenCustom(new CustomConn
                {
                    Name = e.Title, Path = e.Path, Args = e.Args, Icon = e.Icon,
                    PickDir = e.PickDir, ViaPowerShell = e.ViaPowerShell,
                    CloseKey = e.CloseKey, CloseCount = e.CloseCount
                }, string.IsNullOrWhiteSpace(e.Dir) ? null : e.Dir);   // 遠端開啟帶桌面目錄、不跳資料夾框
                break;
        }
    }

    /// <summary>切換功能列位置：上方橫排 ↔ 右側直欄（記憶於設定）。</summary>
    private void TaskbarToggle_Click(object sender, RoutedEventArgs e)
    {
        AppSettings.Current.ToolbarPosition = AppSettings.Current.ToolbarPosition == "right" ? "top" : "right";
        AppSettings.Current.Save();
        ApplyToolbarPosition();
    }

    private void ApplyToolbarPosition()
    {
        bool right = AppSettings.Current.ToolbarPosition == "right";
        DockPanel.SetDock(ToolbarBorder, right ? Dock.Right : Dock.Top);
        ToolbarPanel.Orientation = right ? Orientation.Vertical : Orientation.Horizontal;
        // 分隔線隨方向轉向；按鈕內部圖示/文字排版隨方向切換（右側=圖左字右）
        foreach (var child in ToolbarPanel.Children)
        {
            if (child is System.Windows.Shapes.Rectangle sep)
            {
                if (right)
                {
                    sep.Width = double.NaN; sep.Height = 1;
                    sep.HorizontalAlignment = HorizontalAlignment.Stretch;
                    sep.VerticalAlignment = VerticalAlignment.Center;
                    sep.Margin = new Thickness(2, 6, 2, 6);
                }
                else
                {
                    sep.Width = 1; sep.Height = double.NaN;
                    sep.HorizontalAlignment = HorizontalAlignment.Center;
                    sep.VerticalAlignment = VerticalAlignment.Stretch;
                    sep.Margin = new Thickness(6, 2, 6, 2);
                }
            }
            else if (child is Button btn)
            {
                ToolBtnEx.SetHorizontal(btn, right);
                btn.Width = right ? 114 : 72;        // 直欄較寬（容納「圖左字右」）
                btn.HorizontalContentAlignment = right ? HorizontalAlignment.Left : HorizontalAlignment.Center;
            }
        }
    }

    private void ApplyWebDefaultBg()
    {
        try { Web.DefaultBackgroundColor = System.Drawing.ColorTranslator.FromHtml(AppSettings.Current.Background); }
        catch { Web.DefaultBackgroundColor = System.Drawing.Color.FromArgb(0x1E, 0x1E, 0x1E); }
    }

    private void PostTheme()
    {
        var s = AppSettings.Current;
        string family = $"\"{s.FontFamily}\", Consolas, \"Microsoft JhengHei\", \"微軟正黑體\", monospace";
        var json = JsonSerializer.Serialize(new
        {
            fontFamily = family,
            fontSize = s.FontSize,
            foreground = s.Foreground,
            background = s.Background
        });
        PostToWeb("T" + json);
    }

    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        TabStrip.ItemsSource = Tabs;
        ApplyWebDefaultBg(); // 避免 WebView2 內容未畫出前露出白底

        string userData = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "AwayTerminal", "WebView2");
        Directory.CreateDirectory(userData);

        var env = await CoreWebView2Environment.CreateAsync(null, userData);
        await Web.EnsureCoreWebView2Async(env);

        var core = Web.CoreWebView2;
        core.Settings.AreDevToolsEnabled = true;
        core.Settings.IsStatusBarEnabled = false;
        core.Settings.IsSwipeNavigationEnabled = false;

        _webRoot = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "web"));
        core.SetVirtualHostNameToFolderMapping("app", _webRoot, CoreWebView2HostResourceAccessKind.Allow);
        // 自行伺服 web 檔並加 no-cache，確保每次都載入最新前端（不吃 WebView2 快取）
        core.AddWebResourceRequestedFilter("https://app/*", CoreWebView2WebResourceContext.All);
        core.WebResourceRequested += OnWebResourceRequested;
        core.WebMessageReceived += OnWebMessage;
        core.ContextMenuRequested += OnWebContextMenu;   // 終端機右鍵：自訂選單取代 Edge 預設
        core.Navigate("https://app/index.html");

        _statusTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(600) };
        _statusTimer.Tick += UpdateStatuses;
        _statusTimer.Start();

        StartOrRestartRemote(); // 依設定啟動 Telegram 遠端（未設 token/chatId 則不啟動）
    }

    // ---------- WebView2 訊息 ----------
    private void OnWebMessage(object? sender, CoreWebView2WebMessageReceivedEventArgs e)
    {
        string msg;
        try { msg = e.TryGetWebMessageAsString(); }
        catch { return; }
        if (string.IsNullOrEmpty(msg)) return;

        if (msg == "ready")
        {
            _webReady = true;
            PostTheme();
            // 只恢復上次存下的分頁；沒有就保持空白（不再硬開一個預設 PowerShell）
            var saved = AppSettings.Current.SavedTabs;
            if (saved is { Count: > 0 }) RestoreTabs(saved);
            return;
        }

        char kind = msg[0];
        string rest = msg.Substring(1);
        switch (kind)
        {
            case 'i': // 輸入： id US text
            {
                int p = rest.IndexOf(US);
                if (p < 0) break;
                var tab = FindTab(rest.Substring(0, p));
                if (tab != null)
                {
                    string text = rest.Substring(p + 1);
                    tab.LastInputUtc = DateTime.UtcNow;  // 使用者實際按鍵／貼上（遠端推播用來排除打字回顯）
                    if (tab.Session == null && tab.LoginBuffer != null)
                        HandleLoginInput(tab, text); // SSH「login as:」中
                    else
                        tab.Session?.WriteText(text);
                }
                break;
            }
            case 'r': // 尺寸： id US cols,rows
            {
                int p = rest.IndexOf(US);
                if (p < 0) break;
                var tab = FindTab(rest.Substring(0, p));
                var wh = rest.Substring(p + 1).Split(',');
                if (tab != null && wh.Length == 2 &&
                    int.TryParse(wh[0], out int c) && int.TryParse(wh[1], out int r))
                {
                    tab.Cols = c; tab.Rows = r; // 記住尺寸（login as: 階段 session 尚未啟動）
                    _lastCols = c; _lastRows = r;
                    tab.Session?.Resize(c, r);
                    // 有待送出的自動指令（Claude Code）→ 尺寸就緒後才送，寬度才會正確
                    if (tab.PendingCommand != null && tab.Session != null)
                    {
                        string cmd = tab.PendingCommand;
                        tab.PendingCommand = null;
                        tab.Session.WriteText(cmd + "\r");
                    }
                }
                break;
            }
            case 'a': // 查詢回覆： id US kind US text → 剪貼簿
            {
                int p1 = rest.IndexOf(US);
                if (p1 < 0) break;
                int p2 = rest.IndexOf(US, p1 + 1);
                if (p2 < 0) break;
                string qk = rest.Substring(p1 + 1, p2 - p1 - 1);
                string text = rest.Substring(p2 + 1);
                if (qk == "text") { _remoteTextTcs?.TrySetResult(text); _remoteTextTcs = null; break; } // 遠端查詢，不進剪貼簿
                if (qk == "file") { SaveBufferToFile(rest.Substring(0, p1), text); break; }             // 複製全部至檔案
                if (qk == "cwd") { UpdateTitlePath(rest.Substring(0, p1), text); break; }               // 標題列目前路徑
                var target = qk == "all" ? (FrameworkElement)BtnCopyAll : BtnCopy;
                if (string.IsNullOrEmpty(text)) { ShowCopyFeedback(target, Loc.T("toast.noSelection")); break; }
                try { Clipboard.SetText(text); } catch { }
                // 複製且貼上：進剪貼簿後再貼回原分頁（走 v 協定，claude 分頁自動用 ESC+CR）
                if (qk == "selpaste")
                {
                    var srcTab = FindTab(rest.Substring(0, p1));
                    if (srcTab != null) PasteToTab(srcTab.Id, text);
                    ShowCopyFeedback(target, Loc.T("toast.copiedPasted"));
                    break;
                }
                ShowCopyFeedback(target, Loc.T(qk == "all" ? "toast.copiedAll" : "toast.copied"));
                break;
            }
            case 'p': // 使用者在分割模式點了某 pane → 設為 active（不回送避免迴圈）
            {
                var tab = FindTab(rest);
                if (tab != null)
                {
                    _active = tab;
                    foreach (var t in Tabs) t.IsActive = (t == tab);
                }
                break;
            }
            case 'k': // 拖曳後的新順序
                ReorderTabs(rest.Split(','));
                break;
            case 'z': // Ctrl+滾輪縮放：記住新字級（新分頁/重開沿用）
                if (int.TryParse(rest, out int fs) && fs is >= 6 and <= 40)
                {
                    AppSettings.Current.FontSize = fs;
                    AppSettings.Current.Save();
                }
                break;
        }
    }

    private void ReorderTabs(string[] ids)
    {
        int target = 0;
        foreach (var idStr in ids)
        {
            var tab = FindTab(idStr);
            if (tab == null) continue;
            int cur = Tabs.IndexOf(tab);
            if (cur >= 0 && cur != target && target < Tabs.Count) Tabs.Move(cur, target);
            target++;
        }
    }

    // ---------- 分頁管理 ----------
    private TerminalTab? FindTab(string idStr)
        => int.TryParse(idStr, out int id) ? FindTab(id) : null;

    private TerminalTab? FindTab(int id)
    {
        foreach (var t in Tabs) if (t.Id == id) return t;
        return null;
    }

    private TerminalTab AddTab(TermKind kind, string title, bool claudePaste = false)
    {
        int id = _nextId++;
        var tab = new TerminalTab(id, kind, title) { Cols = _lastCols, Rows = _lastRows };
        Tabs.Add(tab);
        // 第三欄 flags：c=claude 分頁 → JS 端多行貼上改送 ESC+CR 軟換行（terminal.js doPaste）
        PostToWeb("n" + id + US + title + US + (claudePaste || kind == TermKind.Claude ? "c" : ""));
        SelectTab(tab);
        return tab;
    }

    /// <summary>執行檔名含 claude → 貼上需走 ESC+CR 軟換行（AddTab 的 claudePaste 旗標）。</summary>
    private static bool IsClaudeExe(string path)
    {
        try { return Path.GetFileNameWithoutExtension(path).Contains("claude", StringComparison.OrdinalIgnoreCase); }
        catch { return false; }
    }

    /// <summary>建立分頁、掛好輸出/結束事件，再啟動連線（先掛事件再 start，避免漏掉開頭輸出）。</summary>
    private TerminalTab? StartTab(TermKind kind, string title, ITerminalSession session, Action start, bool claudePaste = false)
    {
        if (!_webReady) return null;
        var tab = AddTab(kind, title, claudePaste);
        tab.Session = session;
        if (kind is TermKind.Ssh or TermKind.Telnet or TermKind.Com)
            tab.AutoReconnect = AppSettings.Current.AutoReconnect;   // 斷線自動重連（連線視窗勾選）
        session.Output += data => OnSessionOutput(tab, data);
        session.Exited += () => OnSessionExited(tab.Id);
        try { start(); }
        catch (Exception ex)
        {
            RemoveTabSilently(tab);
            MessageBox.Show(this, Loc.T("msg.connectFail") + "\n" + ex.Message, "AwayTerminal",
                MessageBoxButton.OK, MessageBoxImage.Error);
            return null;
        }
        return tab;
    }

    private void OpenPowerShellDirect(string dir, string title)
    {
        var s = new ConPtySession();
        var tab = StartTab(TermKind.PowerShell, title, s, () => s.Start("powershell.exe", _lastCols, _lastRows, dir));
        if (tab != null) { tab.Restore = new SavedTab { Type = "ps", Title = title, Dir = dir }; tab.WorkDir = dir; SetTitlePath(dir); }
        AddHistory(new SavedTab { Type = "ps", Title = title, Dir = dir });
    }

    /// <summary>依上次關閉時儲存的清單重建分頁。</summary>
    private void RestoreTabs(List<SavedTab> saved)
    {
        foreach (var st in saved.ToList())
        {
            try
            {
                switch (st.Type)
                {
                    case "ps":
                        OpenPowerShellDirect(
                            Directory.Exists(st.Dir) ? st.Dir : Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory),
                            string.IsNullOrWhiteSpace(st.Title) ? NextName("PowerShell") : st.Title);
                        break;
                    case "claude":
                        OpenClaudeDirect(
                            Directory.Exists(st.Dir) ? st.Dir : Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory),
                            string.IsNullOrWhiteSpace(st.Title) ? NextName("Claude") : st.Title);
                        break;
                    case "ssh":
                        if (st.Host.Contains('@'))
                        {
                            var s = new ConPtySession { GracefulExitBytes = new byte[] { 0x04, 0x04, 0x04 } }; // SSH：Ctrl+D ×2
                            var tab = StartTab(TermKind.Ssh, st.Host, s,
                                () => s.Start(SshCommand(st.Host, st.Port), _lastCols, _lastRows, null));
                            if (tab != null) tab.Restore = st;
                        }
                        else
                        {
                            var tab = AddTab(TermKind.Ssh, st.Host);
                            tab.PendingHost = st.Host;
                            tab.PendingPort = st.Port;
                            tab.LoginBuffer = new StringBuilder();
                            tab.Restore = st;
                            EchoToTab(tab.Id, "login as: ");
                        }
                        break;
                    case "telnet":
                    {
                        var s = new TelnetSession { KeepAliveMins = AppSettings.Current.KeepAliveMins };
                        var tab = StartTab(TermKind.Telnet, string.IsNullOrWhiteSpace(st.Title) ? $"{st.Host}:{st.Port}" : st.Title,
                            s, () => s.Start(st.Host, st.Port));
                        if (tab != null) tab.Restore = st;
                        break;
                    }
                    case "com":
                    {
                        var s = new SerialSession();
                        var tab = StartTab(TermKind.Com, string.IsNullOrWhiteSpace(st.Title) ? $"{st.ComPort} {st.Baud}" : st.Title,
                            s, () => s.Start(st.ComPort, st.Baud, st.DataBits,
                                Enum.Parse<System.IO.Ports.Parity>(st.Parity),
                                Enum.Parse<System.IO.Ports.StopBits>(st.StopBits),
                                Enum.Parse<System.IO.Ports.Handshake>(st.Flow)));
                        if (tab != null) tab.Restore = st;
                        break;
                    }
                }
            }
            catch { /* 個別分頁恢復失敗就跳過 */ }
        }
    }

    private void SelectTab(TerminalTab tab)
    {
        _active = tab;
        SetTitlePath(tab.WorkDir);   // 先用啟動目錄墊底（claude/自訂沒提示行就顯示它）；有提示行的分頁 0.6s 內被解析值蓋掉
        foreach (var t in Tabs) t.IsActive = (t == tab);
        PostToWeb("s" + tab.Id);
        Web.Focus();
    }

    private void RemoveTabSilently(TerminalTab tab)
    {
        int idx = Tabs.IndexOf(tab);
        PostToWeb("x" + tab.Id);
        try { (tab.Macro as MacroRunner)?.Stop(); } catch { }
        try { (tab.Logger as SessionLogger)?.Dispose(); } catch { }
        // session 收尾（Ctrl+C ×2 + 終止）移到背景執行緒，關分頁不卡 UI
        var session = tab.Session;
        tab.Session = null;
        if (session != null) _ = System.Threading.Tasks.Task.Run(() => { try { session.Dispose(); } catch { } });
        Tabs.Remove(tab);
        if (_active == tab)
        {
            _active = null;
            if (Tabs.Count > 0) SelectTab(Tabs[Math.Min(idx, Tabs.Count - 1)]);
        }
    }

    private void CloseTab(TerminalTab tab)
    {
        var r = MessageBox.Show(this, string.Format(Loc.T("msg.closeTabConfirm"), tab.Title),
            Loc.T("msg.closeTabTitle"), MessageBoxButton.YesNo, MessageBoxImage.Question);
        if (r != MessageBoxResult.Yes) return;
        RemoveTabSilently(tab);
    }

    private void OnSessionOutput(TerminalTab tab, byte[] data)
    {
        tab.LastOutputUtc = DateTime.UtcNow;
        if (tab.ReconnectAttempt != 0) tab.ReconnectAttempt = 0;   // 有輸出=真的連上了 → 重連退避歸零

        // log 記錄
        (tab.Logger as SessionLogger)?.Write(data);

        // 遠端 /last 緩衝（只在遠端啟用時才做）
        if (_remote != null) AppendRemoteRecent(tab, data);

        // 輸出合批：累積到緩衝，同一批 Dispatcher 迴圈只送一次 → 減少 TUI 部分畫格造成的錯亂
        bool schedule = false;
        lock (tab.OutBuf)
        {
            tab.OutBuf.Write(data, 0, data.Length);
            if (!tab.FlushScheduled) { tab.FlushScheduled = true; schedule = true; }
        }
        if (schedule) Dispatcher.InvokeAsync(() => FlushTabOutput(tab));
    }

    private void FlushTabOutput(TerminalTab tab)
    {
        byte[] chunk;
        lock (tab.OutBuf)
        {
            chunk = tab.OutBuf.ToArray();
            tab.OutBuf.SetLength(0);
            tab.FlushScheduled = false;
        }
        if (chunk.Length > 0)
            PostToWeb("o" + tab.Id + US + Convert.ToBase64String(chunk));
    }

    private void OnSessionExited(int id)
    {
        string bye = "o" + id + US + Convert.ToBase64String(
            Encoding.UTF8.GetBytes("\r\n\x1b[90m" + Loc.T("term.exited") + "\x1b[0m\r\n"));
        Dispatcher.InvokeAsync(() =>
        {
            PostToWeb(bye);
            // 斷線自動重連：分頁還在（非使用者關閉）且勾了自動重連 → 排程重連
            var tab = FindTab(id);
            if (tab == null || !tab.AutoReconnect || tab.Restore == null) return;
            if (tab.Kind is not (TermKind.Ssh or TermKind.Telnet or TermKind.Com)) return;
            tab.Session = null;   // 舊 session 已結束
            ScheduleReconnect(tab);
        });
    }

    /// <summary>排程自動重連：退避 3,6,9…最多 30 秒（一收到輸出就歸零）。</summary>
    private void ScheduleReconnect(TerminalTab tab)
    {
        tab.ReconnectAttempt++;
        int delay = Math.Min(30, 3 * tab.ReconnectAttempt);
        EchoToTab(tab.Id, "\r\n\x1b[33m" + string.Format(Loc.T("term.reconnect"), delay) + "\x1b[0m\r\n");
        var timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(delay) };
        timer.Tick += (_, _) =>
        {
            timer.Stop();
            var t = FindTab(tab.Id);
            if (t == null || t.Session != null) return;   // 已被關閉或已重連
            TryReconnect(t);
        };
        timer.Start();
    }

    /// <summary>依 Restore 資訊重建 session（同一個分頁，不另開）。失敗就再排下一次。</summary>
    private void TryReconnect(TerminalTab tab)
    {
        var st = tab.Restore!;
        try
        {
            switch (tab.Kind)
            {
                case TermKind.Ssh:
                {
                    // 登入後 Restore.Host = user@host → 直接重跑 ssh.exe（密碼會再問一次）
                    var s = new ConPtySession { GracefulExitBytes = new byte[] { 0x04, 0x04, 0x04 } };
                    tab.Session = s;
                    s.Output += data => OnSessionOutput(tab, data);
                    s.Exited += () => OnSessionExited(tab.Id);
                    s.Start(SshCommand(st.Host, st.Port), tab.Cols, tab.Rows, null);
                    break;
                }
                case TermKind.Telnet:
                {
                    var s = new TelnetSession { KeepAliveMins = AppSettings.Current.KeepAliveMins };
                    tab.Session = s;
                    s.Output += data => OnSessionOutput(tab, data);
                    s.Exited += () => OnSessionExited(tab.Id);
                    s.Start(st.Host, st.Port);
                    break;
                }
                case TermKind.Com:
                {
                    var s = new SerialSession();
                    tab.Session = s;
                    s.Output += data => OnSessionOutput(tab, data);
                    s.Exited += () => OnSessionExited(tab.Id);
                    s.Start(st.ComPort, st.Baud, st.DataBits,
                        Enum.Parse<System.IO.Ports.Parity>(st.Parity),
                        Enum.Parse<System.IO.Ports.StopBits>(st.StopBits),
                        Enum.Parse<System.IO.Ports.Handshake>(st.Flow));
                    break;
                }
            }
            tab.Session?.Resize(tab.Cols, tab.Rows);
        }
        catch
        {
            tab.Session = null;
            ScheduleReconnect(tab);   // 連不上（埠不存在/主機不通）→ 退避後再試
        }
    }

    private void OnWebResourceRequested(object? sender, CoreWebView2WebResourceRequestedEventArgs e)
    {
        try
        {
            var uri = new Uri(e.Request.Uri);
            string rel = Uri.UnescapeDataString(uri.AbsolutePath).TrimStart('/').Replace('/', Path.DirectorySeparatorChar);
            string file = Path.GetFullPath(Path.Combine(_webRoot, rel));
            if (!file.StartsWith(_webRoot, StringComparison.OrdinalIgnoreCase) || !File.Exists(file)) return;
            var ms = new MemoryStream(File.ReadAllBytes(file));
            string headers = "Content-Type: " + MimeOf(file) + "\r\nCache-Control: no-cache, no-store, must-revalidate";
            e.Response = Web.CoreWebView2.Environment.CreateWebResourceResponse(ms, 200, "OK", headers);
        }
        catch { }
    }

    private static string MimeOf(string path) => Path.GetExtension(path).ToLowerInvariant() switch
    {
        ".html" => "text/html; charset=utf-8",
        ".js" => "text/javascript; charset=utf-8",
        ".css" => "text/css; charset=utf-8",
        ".png" => "image/png",
        ".json" => "application/json; charset=utf-8",
        ".svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };

    private void PostToWeb(string s)
    {
        try { Web.CoreWebView2?.PostWebMessageAsString(s); } catch { }
    }

    // ---------- 工作列 icon 右下狀態圓點 ----------
    private ImageSource? _greenDot, _orangeDot;
    private ImageSource GreenDot => _greenDot ??= MakeDotOverlay(Color.FromRgb(0x4C, 0xAF, 0x50));
    private ImageSource OrangeDot => _orangeDot ??= MakeDotOverlay(Color.FromRgb(0xFF, 0x98, 0x00));

    /// <summary>畫一個帶白邊的實心圓，作為工作列 icon 的 overlay。</summary>
    private static ImageSource MakeDotOverlay(Color color)
    {
        var dv = new DrawingVisual();
        using (var dc = dv.RenderOpen())
        {
            const double size = 16, r = size / 2 - 1.5;
            var c = new Point(size / 2, size / 2);
            dc.DrawEllipse(new SolidColorBrush(color), new Pen(new SolidColorBrush(Colors.White), 1.5), c, r, r);
        }
        var rtb = new RenderTargetBitmap(16, 16, 96, 96, PixelFormats.Pbgra32);
        rtb.Render(dv);
        rtb.Freeze();
        return rtb;
    }

    // ---------- 綠 / 橘狀態輪詢 ----------
    private void UpdateStatuses(object? sender, EventArgs e)
    {
        if (Tabs.Count == 0) { Taskbar.Overlay = null; SetTitlePath(""); return; } // 沒有分頁 → 不顯示圓點、標題回程式名
        // 標題列目前路徑：向作用中分頁查提示字元行（回覆走 a…cwd）
        if (_webReady && _active != null) PostToWeb("q" + _active.Id + US + "cwd");
        HashSet<int> busyParents;
        try { busyParents = ProcessTree.ParentsWithChildren(); }
        catch { return; }

        var now = DateTime.UtcNow;
        foreach (var t in Tabs)
        {
            bool busy;
            if (t.Kind == TermKind.PowerShell)
            {
                // 有外部子行程（claude/編譯等）「且」近 1.5 秒持續有輸出 = 忙(橘)。
                // 停下來等使用者時：行程仍活著但不再送資料 → 綠。
                int pid = t.Session?.ProcessId ?? 0;
                bool hasChild = pid != 0 && busyParents.Contains(pid);
                bool streaming = (now - t.LastOutputUtc).TotalMilliseconds < 1500;
                busy = hasChild && streaming;
            }
            else if (t.Kind == TermKind.Claude || t.Kind == TermKind.Custom)
            {
                // 直接跑的 claude / 自訂 exe：近期持續有輸出 = 忙(橘)，停下等使用者輸入 = 綠
                busy = (now - t.LastOutputUtc).TotalMilliseconds < 1200;
            }
            else
            {
                busy = (now - t.LastOutputUtc).TotalMilliseconds < 500; // 遠端：近期有輸出 = 忙
            }
            var prev = t.Status;
            t.Status = busy ? TermStatus.Busy : TermStatus.Ready;
            t.RefreshRuntime(); // 更新 tooltip 的已啟動時間

            // 遠端：忙碌持續 ≥3 秒後轉閒 → 交給遠端服務（附著分頁推輸出 / 其他分頁推通知；過濾短暫閃爍）
            if (_remote?.IsRunning == true)
            {
                if (busy && prev != TermStatus.Busy) _busySince[t.Id] = now;
                else if (!busy && prev == TermStatus.Busy)
                {
                    // 打字不算「程式做完事」：每個按鍵的回顯都在更新 LastOutputUtc，所以打一句話
                    // （超過 3 秒很常見）整段都會被判定為忙，停手約 0.5~1.5 秒後翻閒 → 舊版就推播了。
                    // 真正的工作是「送出指令後輸出持續數秒」，此時最後一次按鍵離轉閒至少有那段時間；
                    // 打字則永遠只差一個「輸出靜默門檻」。用 2.5 秒把兩者分開。
                    // 註：遠端自己送進去的指令走 SendInputToTab、不經 i 協定，不會更新 LastInputUtc，
                    // 所以從手機下的指令依然會正常回推結果。
                    bool echoFromTyping = (now - t.LastInputUtc).TotalSeconds < 2.5;
                    if (!echoFromTyping &&
                        _busySince.TryGetValue(t.Id, out var since) && (now - since).TotalSeconds >= 3)
                        _remote.OnTabIdle(t.Id, t.Title);
                    _busySince.Remove(t.Id);
                }
            }
        }
        // 工作列 icon 右下圓點：有分頁忙碌=橘、全部閒置=綠
        Taskbar.Overlay = Tabs.Any(t => t.Status == TermStatus.Busy) ? OrangeDot : GreenDot;
    }

    // ---------- 工具列：連線群組 ----------
    /// <summary>資料夾選擇（PowerShell / ClaudeCode 共用）；取消回傳 null。</summary>
    private string? PickWorkDir(string title)
    {
        using var fbd = new System.Windows.Forms.FolderBrowserDialog
        {
            Description = title,
            UseDescriptionForTitle = true,
            ShowNewFolderButton = true
        };
        var last = AppSettings.Current.LastDir;
        if (!string.IsNullOrWhiteSpace(last) && Directory.Exists(last)) fbd.SelectedPath = last;
        if (fbd.ShowDialog() != System.Windows.Forms.DialogResult.OK) return null;
        AppSettings.Current.LastDir = fbd.SelectedPath;
        AppSettings.Current.Save();
        return fbd.SelectedPath;
    }

    private void OpenPowerShell_Click(object sender, RoutedEventArgs e)
    {
        if (!_webReady) return;
        string? dir = PickWorkDir(Loc.T("dlg.pickDirPs"));
        if (dir != null) OpenPowerShellDirect(dir, NextName("PowerShell"));
    }

    /// <summary>開啟 Claude Code 分頁（僅供恢復舊「claude」分頁用；一般 claude 已改走自訂連線）。
    /// 預設直接以 ConPTY 執行 claude.exe（不經 PowerShell）：
    /// claude.exe 即為主行程，一開始就用目前視窗尺寸建立、關閉送 Ctrl+C ×3、離開後分頁即結束。
    /// 若設定勾「透過 PowerShell」或路徑是 .cmd/.bat（npm 版）→ 開 PowerShell 再把指令打進去。</summary>
    private void OpenClaudeDirect(string dir, string title)
    {
        var cfg = AppSettings.Current;
        string path = cfg.ClaudePath;
        string args = string.IsNullOrWhiteSpace(cfg.ClaudeArgs) ? "" : " " + cfg.ClaudeArgs.Trim();

        // .cmd/.bat 無法用 CreateProcess 直接跑 → 強制走 PowerShell
        bool viaPs = cfg.ClaudeViaPowerShell
            || path.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase)
            || path.EndsWith(".bat", StringComparison.OrdinalIgnoreCase);

        if (viaPs)
        {
            var s = new ConPtySession();
            var tab = StartTab(TermKind.PowerShell, title, s, () => s.Start("powershell.exe", _lastCols, _lastRows, dir),
                claudePaste: true);
            if (tab == null) return;
            tab.WorkDir = dir; SetTitlePath(dir);
            tab.Restore = new SavedTab { Type = "claude", Title = title, Dir = dir };
            AddHistory(new SavedTab { Type = "claude", Title = title, Dir = dir });
            // 等尺寸就緒後才把指令打進 PowerShell，避免 claude 以 80 欄啟動；1 秒後保險送出
            tab.PendingCommand = $"& \"{path}\"{args}";
            var timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
            timer.Tick += (_, _) =>
            {
                timer.Stop();
                if (tab.PendingCommand != null && tab.Session != null)
                {
                    string cmd = tab.PendingCommand; tab.PendingCommand = null;
                    tab.Session.WriteText(cmd + "\r");
                }
            };
            timer.Start();
        }
        else
        {
            if (!File.Exists(path)) return; // 直接模式需有效 exe（按鈕流程已先提醒；恢復分頁時靜默略過）
            var s = new ConPtySession(); // 預設 GracefulExitBytes = Ctrl+C ×3
            var tab = StartTab(TermKind.Claude, title, s,
                () => s.Start($"\"{path}\"{args}", _lastCols, _lastRows, dir));
            if (tab != null) { tab.Restore = new SavedTab { Type = "claude", Title = title, Dir = dir }; tab.WorkDir = dir; SetTitlePath(dir); }
            AddHistory(new SavedTab { Type = "claude", Title = title, Dir = dir });
        }
    }

    private void OpenSsh_Click(object sender, RoutedEventArgs e)
    {
        if (!_webReady) return;
        var dlg = new ConnectDialog { Owner = this };
        if (dlg.ShowDialog() != true) return;

        if (dlg.ConnType == "telnet") OpenTelnetDirect(dlg.Host, dlg.Port);
        else OpenSshLoginAs(dlg.Host, dlg.Port);
    }

    private void OpenTelnetDirect(string host, int port)
    {
        var s = new TelnetSession { KeepAliveMins = AppSettings.Current.KeepAliveMins };
        var tab = StartTab(TermKind.Telnet, $"{host}:{port}", s, () => s.Start(host, port));
        if (tab != null) tab.Restore = new SavedTab { Type = "telnet", Host = host, Port = port };
        AddHistory(new SavedTab { Type = "telnet", Host = host, Port = port });
    }

    /// <summary>組 ssh.exe 指令：含保持連線（KeepAliveMins 分→ServerAliveInterval 秒；0=不加）。</summary>
    private static string SshCommand(string host, int port)
    {
        int m = AppSettings.Current.KeepAliveMins;
        string ka = m > 0 ? $" -o ServerAliveInterval={m * 60} -o ServerAliveCountMax=3" : "";
        return $"ssh.exe -p {port}{ka} {host}";
    }

    /// <summary>PuTTY 式：先開分頁顯示「login as:」，輸入帳號後才啟動 ssh。</summary>
    private void OpenSshLoginAs(string host, int port)
    {
        var tab = AddTab(TermKind.Ssh, host);
        tab.PendingHost = host;
        tab.PendingPort = port;
        tab.LoginBuffer = new StringBuilder();
        tab.Restore = new SavedTab { Type = "ssh", Host = host, Port = port };
        AddHistory(new SavedTab { Type = "ssh", Host = host, Port = port });
        EchoToTab(tab.Id, "login as: ");
    }

    private void EchoToTab(int id, string text)
        => PostToWeb("o" + id + US + Convert.ToBase64String(Encoding.UTF8.GetBytes(text)));

    /// <summary>SSH「login as:」輸入處理：Enter 啟動 ssh；Backspace 退格；其餘字元回顯。</summary>
    private void HandleLoginInput(TerminalTab tab, string text)
    {
        foreach (char c in text)
        {
            if (c == '\r' || c == '\n')
            {
                string user = tab.LoginBuffer!.ToString().Trim();
                tab.LoginBuffer = null;
                EchoToTab(tab.Id, "\r\n");

                string host = tab.PendingHost ?? "";
                string target = string.IsNullOrEmpty(user) || host.Contains('@') ? host : $"{user}@{host}";
                tab.Title = target;
                PostToWeb("t" + tab.Id + US + target);
                if (tab.Restore != null) { tab.Restore.Host = target; tab.Restore.Title = target; }

                var s = new ConPtySession { GracefulExitBytes = new byte[] { 0x04, 0x04, 0x04 } }; // SSH：Ctrl+D ×2 登出
                tab.Session = s;
                tab.AutoReconnect = AppSettings.Current.AutoReconnect;   // login as: 路徑不經 StartTab，這裡補旗標
                s.Output += d => OnSessionOutput(tab, d);
                s.Exited += () => OnSessionExited(tab.Id);
                try { s.Start(SshCommand(target, tab.PendingPort), tab.Cols, tab.Rows, null); }
                catch (Exception ex)
                {
                    MessageBox.Show(this, Loc.T("msg.connectFail") + "\n" + ex.Message, "AwayTerminal",
                        MessageBoxButton.OK, MessageBoxImage.Error);
                }
                return;
            }
            if (c == '\u007f' || c == '\b')
            {
                if (tab.LoginBuffer!.Length > 0)
                {
                    tab.LoginBuffer.Length--;
                    EchoToTab(tab.Id, "\b \b");
                }
            }
            else if (!char.IsControl(c))
            {
                tab.LoginBuffer!.Append(c);
                EchoToTab(tab.Id, c.ToString());
            }
        }
    }

    private void OpenCom_Click(object sender, RoutedEventArgs e)
    {
        if (!_webReady) return;
        var dlg = new ComDialog { Owner = this };
        if (dlg.ShowDialog() != true) return;
        OpenComDirect(dlg.PortName, dlg.Baud, dlg.DataBits,
            dlg.Parity.ToString(), dlg.StopBits.ToString(), dlg.Handshake.ToString());
    }

    private void OpenComDirect(string port, int baud, int dataBits, string parity, string stopBits, string flow)
    {
        try
        {
            var s = new SerialSession();
            var tab = StartTab(TermKind.Com, $"{port} {baud}", s,
                () => s.Start(port, baud, dataBits,
                    Enum.Parse<System.IO.Ports.Parity>(parity),
                    Enum.Parse<System.IO.Ports.StopBits>(stopBits),
                    Enum.Parse<System.IO.Ports.Handshake>(flow)));
            var st = new SavedTab
            {
                Type = "com", ComPort = port, Baud = baud, DataBits = dataBits,
                Parity = parity, StopBits = stopBits, Flow = flow
            };
            if (tab != null) tab.Restore = st;
            AddHistory(st);
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, Loc.T("msg.connectFail") + "\n" + ex.Message, "AwayTerminal",
                MessageBoxButton.OK, MessageBoxImage.Error);
        }
    }

    /// <summary>分頁列尾端 ▲：上拉列出所有分頁供選擇。</summary>
    private void TabList_Click(object sender, RoutedEventArgs e)
    {
        if (Tabs.Count == 0) return;
        var menu = new ContextMenu();
        foreach (var t in Tabs)
        {
            var tt = t;
            var mi = new MenuItem
            {
                Header = t.Title,
                FontWeight = t.IsActive ? FontWeights.Bold : FontWeights.Normal
            };
            mi.Click += (_, _) => SelectTab(tt);
            menu.Items.Add(mi);
        }
        menu.PlacementTarget = (UIElement)sender;
        menu.Placement = System.Windows.Controls.Primitives.PlacementMode.Top;
        menu.IsOpen = true;
    }

    private void Split_Click(object sender, RoutedEventArgs e)
    {
        // 三態循環：分頁 → 分割(grid) → 分欄(單列) → 分頁
        _viewMode = _viewMode switch { "tab" => "split", "split" => "columns", _ => "tab" };
        PostToWeb("L" + _viewMode);
        // 分頁模式：終端機外框細黃線；分割/分欄：讓給各 pane 自己的黃框
        TermFrame.BorderBrush = _splitMode
            ? new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0x33, 0x33, 0x33))
            : new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xFD, 0xFF, 0xB0));
        UpdateSplitButton();
    }

    private void UpdateSplitButton()
    {
        // 按鈕文字顯示「下一個」模式（點了會變成的樣子）
        (BtnSplit.Content, BtnSplit.ToolTip) = _viewMode switch
        {
            "tab" => (Loc.T("tb.split"), Loc.T("tip.split")),      // 目前分頁，點→分割
            "split" => (Loc.T("tb.columns"), Loc.T("tip.columns")), // 目前分割，點→分欄
            _ => (Loc.T("tb.tabs"), Loc.T("tip.tabs")),             // 目前分欄，點→分頁
        };
    }

    /// <summary>終端機右鍵選單：取代 WebView2 預設選單（剪下/複製/純文字貼上/全選/搜尋）。</summary>
    private void OnWebContextMenu(object? sender, CoreWebView2ContextMenuRequestedEventArgs e)
    {
        e.Handled = true;   // 擋掉 Edge 預設選單（重新載入/檢視原始檔等一律不顯示）
        var menu = new ContextMenu();
        MenuItem Item(string key, Action act)
        {
            var mi = new MenuItem { Header = Loc.T(key) };
            mi.Click += (_, _) => act();
            return mi;
        }
        menu.Items.Add(Item("ctx.paste", () => Paste_Click(this, new RoutedEventArgs())));
        menu.Items.Add(Item("ctx.copy", () => { if (_active != null) PostToWeb("q" + _active.Id + US + "sel"); }));
        // 複製且貼上：選取的文字進剪貼簿後，直接貼回終端機（省去「複製→再貼上」兩步）
        menu.Items.Add(Item("ctx.copyPaste", () => { if (_active != null) PostToWeb("q" + _active.Id + US + "selpaste"); }));
        menu.Items.Add(Item("tb.copyall", () => { if (_active != null) PostToWeb("q" + _active.Id + US + "all"); }));
        menu.Items.Add(Item("ctx.copyAllFile", () => { if (_active != null) PostToWeb("q" + _active.Id + US + "file"); }));
        menu.Items.Add(new Separator());
        menu.Items.Add(Item("ctx.search", () => PostToWeb("F")));
        menu.PlacementTarget = Web;
        menu.Placement = System.Windows.Controls.Primitives.PlacementMode.MousePoint;
        menu.IsOpen = true;
    }

    // 標題列「程式名稱 - 目前路徑」：由提示字元行解析（PowerShell/cmd/bash-ssh）。
    // 註：PowerShell 的 Set-Location 不會同步行程 CWD，讀 PEB 拿到的會是啟動目錄，故改用提示行解析。
    private static readonly System.Text.RegularExpressions.Regex[] CwdRes =
    {
        new(@"^PS\s+(?<p>[A-Za-z]:\\.*?|/.*?)\s*>"),               // PowerShell：PS C:\path>
        new(@"^(?<p>[A-Za-z]:\\[^>]*?)\s*>"),                       // cmd：C:\path>
        new(@"^[^@\s]+@[^:\s]+:\s*(?<p>[^\s$#%]+)\s*[$#%]"),        // bash/zsh：user@host:~/path$（zsh 提示常用 %）
        new(@"^\[[^@\s\]]+@[^\s\]]+\s+(?<p>[^\]]+)\]\s*[$#]"),      // RHEL/CentOS：[user@host ~]#
        new(@"^[\w.-]+:(?<p>/[^\s$#]*)\s*[$#]"),                    // Android adb：davinci:/data $
        new(@"^[^@\s]+@[^\s:]+\s+(?<p>[/~][^\s>$#%]*)\s*[>$#%]"),   // fish 等：user@host /path>
    };

    private string _titlePath = "";

    /// <summary>依提示字元行更新標題（解析不到就保留上次的，避免打字過程閃動）。</summary>
    private void UpdateTitlePath(string idStr, string promptLine)
    {
        if (_active == null || _active.Id.ToString() != idStr) return;   // 已切換分頁 → 丟棄過期回覆
        foreach (var re in CwdRes)
        {
            var m = re.Match(promptLine);
            if (!m.Success) continue;
            SetTitlePath(m.Groups["p"].Value.Trim());
            return;
        }
    }

    private void SetTitlePath(string path)
    {
        if (_titlePath == path) return;
        _titlePath = path;
        Title = string.IsNullOrEmpty(path) ? Loc.T("app.name") : $"{Loc.T("app.name")} - {path}";
    }

    /// <summary>「複製全部至檔案」：把整個 buffer 的純文字存檔（q…file 的回覆）。</summary>
    private void SaveBufferToFile(string idStr, string text)
    {
        string title = FindTab(idStr)?.Title ?? "AwayTerminal";
        foreach (var c in Path.GetInvalidFileNameChars()) title = title.Replace(c, '_');
        var dlg = new Microsoft.Win32.SaveFileDialog
        {
            FileName = $"{title}-{DateTime.Now:yyyyMMdd-HHmmss}.txt",
            Filter = "Text file (*.txt)|*.txt|All files (*.*)|*.*",
            DefaultExt = ".txt"
        };
        if (dlg.ShowDialog(this) != true) return;
        try { File.WriteAllText(dlg.FileName, text, Encoding.UTF8); }
        catch (Exception ex)
        {
            MessageBox.Show(this, Loc.T("msg.saveFail") + "\n" + ex.Message, "AwayTerminal",
                MessageBoxButton.OK, MessageBoxImage.Error);
        }
    }

    // ---------- 工具列：編輯群組 ----------
    private void Copy_Click(object sender, RoutedEventArgs e)
    {
        if (_active != null) PostToWeb("q" + _active.Id + US + "sel");
    }

    private void CopyAll_Click(object sender, RoutedEventArgs e)
    {
        if (_active != null) PostToWeb("q" + _active.Id + US + "all");
    }

    private void Paste_Click(object sender, RoutedEventArgs e)
    {
        string text = "";
        try { text = Clipboard.GetText(); } catch { }
        PasteToActive(text);
        Web.Focus();
    }

    /// <summary>把文字「貼」進作用中分頁：走 xterm.paste（v 協定）而非直接寫 session——
    /// 多行文字才會依程式的 bracketed paste 設定正確處理，不會被逐行當成 Enter 送出。</summary>
    private void PasteToActive(string text)
    {
        if (_active == null) return;
        PasteToTab(_active.Id, text);
    }

    /// <summary>把文字「貼」進指定分頁（同 PasteToActive，但指定 id——查詢回覆時用，
    /// 避免查詢送出到回覆之間使用者切了分頁而貼錯地方）。</summary>
    private void PasteToTab(int id, string text)
    {
        if (string.IsNullOrEmpty(text) || !_webReady) return;
        PostToWeb("v" + id + US + Convert.ToBase64String(Encoding.UTF8.GetBytes(text)));
    }

    private async void Clear_Click(object sender, RoutedEventArgs e)
    {
        if (_active == null) return;
        var session = _active.Session;
        if (_active.Kind == TermKind.PowerShell || _active.Kind == TermKind.Ssh)
        {
            // 先送 Esc 清掉還沒送出的輸入，隔一小段再送 Ctrl+L 清畫面。
            // 若兩者黏在一起送，PSReadLine 會當成 escape 序列而兩者都失效。
            session?.Write(new byte[] { 0x1B });
            await System.Threading.Tasks.Task.Delay(60);
            session?.Write(new byte[] { 0x0C });
        }
        else
        {
            PostToWeb("c" + _active.Id); // Telnet / COM：直接清 xterm 緩衝
        }
        Web.Focus();
    }

    // ---------- 工具列：設定群組（P3 / P4 補齊）----------
    private void Prompt_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new PromptDialog { Owner = this };
        if (dlg.ShowDialog() == true && dlg.ContentToSend != null)
        {
            PasteToActive(dlg.ContentToSend);
            Web.Focus();
        }
    }

    private void Remote_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new RemoteDialog { Owner = this };
        if (dlg.ShowDialog() != true) return;
        StartOrRestartRemote();
        if (RemoteTakenByOther) Info(Loc.T("remote.takenByOther"));   // 另一個實例持有遠端 → 明講沒啟動
    }

    // ---------- 遠端（Telegram）：服務控制 + IRemoteHost 實作 ----------
    // 多開防護：同一 token 兩個實例同時 long polling 會 409 Conflict、訊息被隨機搶走
    // → 用具名 Mutex 保證同機只有第一個實例啟動遠端；其餘實例跳過並在遠端設定顯示提示。
    private System.Threading.Mutex? _remoteMutex;
    /// <summary>遠端已由本機另一個 AwayTerminal 實例使用中（本實例未啟動遠端）。</summary>
    internal static bool RemoteTakenByOther { get; private set; }

    private bool TryAcquireRemoteLock()
    {
        if (_remoteMutex != null) return true;   // 本實例已持有
        var m = new System.Threading.Mutex(false, @"Local\AwayTerminal.TelegramRemote");
        bool got;
        try { got = m.WaitOne(0); }
        catch (System.Threading.AbandonedMutexException) { got = true; }   // 前實例被強殺 → 直接接手
        if (got) { _remoteMutex = m; return true; }
        m.Dispose();
        return false;
    }

    private void ReleaseRemoteLock()
    {
        try { _remoteMutex?.ReleaseMutex(); } catch { }
        try { _remoteMutex?.Dispose(); } catch { }
        _remoteMutex = null;
    }

    private void StartOrRestartRemote()
    {
        var s = AppSettings.Current;
        _remote ??= new TelegramRemote(this);
        _remote.Stop();
        if (!(s.RemoteEnabled && !string.IsNullOrWhiteSpace(s.TelegramBotToken) && s.TelegramChatId != 0))
        { ReleaseRemoteLock(); RemoteTakenByOther = false; return; }
        if (!TryAcquireRemoteLock()) { RemoteTakenByOther = true; return; }
        RemoteTakenByOther = false;
        _remote.Start(s.TelegramBotToken, s.TelegramChatId, s.RemoteNotify);
    }

    // 去 ANSI：CSI / OSC(BEL) / DCS-PM-APC-SOS(ST) / 其餘 2 字元跳脫
    private static readonly System.Text.RegularExpressions.Regex AnsiRe = new(
        @"\x1b\[[0-9;?<>=]*[@-~]|\x1b\][^\x07]*\x07|\x1b[PX^_].*?\x1b\\|\x1b.",
        System.Text.RegularExpressions.RegexOptions.Compiled | System.Text.RegularExpressions.RegexOptions.Singleline);

    // 游標移動/定位（CUP/HVP/CUU/CUD/CUF/CUB/CNL/CPL/VPA）：TUI「原地重繪」（spinner、狀態列）
    // 全靠這些跳行蓋字。轉成換行，重繪的每一格才會是獨立行，遠端端的雜訊過濾才切得掉。
    private static readonly System.Text.RegularExpressions.Regex CursorMoveRe = new(
        @"\x1b\[[0-9;]*[HfABCDEFd]", System.Text.RegularExpressions.RegexOptions.Compiled);

    /// <summary>把一批輸出去 ANSI、去控制字元後，累積到分頁的遠端近期文字緩衝（有上限）。背景執行緒呼叫。</summary>
    private void AppendRemoteRecent(TerminalTab tab, byte[] data)
    {
        string s;
        try { s = Encoding.UTF8.GetString(data); } catch { return; }
        s = s.Replace("\r\n", "\n");            // 正常換行先收斂，避免下面把 \r 當成重繪換行時翻倍
        s = CursorMoveRe.Replace(s, "\n");
        s = AnsiRe.Replace(s, "");
        var sb = new StringBuilder(s.Length);
        foreach (char c in s)
        {
            if (c == '\r' || c == '\n') { sb.Append('\n'); continue; }  // 單獨 \r = 原地改寫，視為換行
            if (c >= ' ' || c == '\t') sb.Append(c);
        }
        lock (tab.RemoteLock)
        {
            tab.RemoteRecent.Append(sb);
            if (tab.RemoteRecent.Length > 12000)
                tab.RemoteRecent.Remove(0, tab.RemoteRecent.Length - 8000);
        }
    }

    // /new 用：上次列給遠端的連線清單快照（OpenConnection 依此索引開啟）
    private List<SavedTab> _remoteConnList = new();

    IReadOnlyList<string> IRemoteHost.ListConnections()
        => Dispatcher.Invoke(() =>
        {
            // 每種連線只列一個（不列 History，避免同型態重複洗版）；需要工作目錄的預設以「桌面」開啟
            string desk = Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);
            var s = AppSettings.Current;
            var list = new List<SavedTab> { new() { Type = "ps", Dir = desk } };
            var labels = new List<string> { "PowerShell（桌面）" };
            if (!string.IsNullOrWhiteSpace(s.LastHost))
            {
                list.Add(new SavedTab { Type = "ssh", Host = s.LastHost, Port = s.LastSshPort });
                labels.Add($"SSH {s.LastHost}");
                list.Add(new SavedTab { Type = "telnet", Host = s.LastHost, Port = s.LastTelnetPort });
                labels.Add($"Telnet {s.LastHost}:{s.LastTelnetPort}");
            }
            if (!string.IsNullOrWhiteSpace(s.ComPort))
            {
                list.Add(new SavedTab
                {
                    Type = "com", ComPort = s.ComPort, Baud = s.ComBaud, DataBits = s.ComDataBits,
                    Parity = s.ComParity, StopBits = s.ComStopBits, Flow = s.ComFlow
                });
                labels.Add($"{s.ComPort} {s.ComBaud}");
            }
            list.Add(new SavedTab { Type = "adb" });
            labels.Add("ADB");
            foreach (var c in s.CustomConns.Where(c => !c.Hidden && !string.IsNullOrWhiteSpace(c.Name)))
            {
                list.Add(new SavedTab
                {
                    Type = "custom", Title = c.Name, Path = c.Path, Args = c.Args, Icon = c.Icon,
                    Dir = c.PickDir ? desk : "",   // 遠端不能跳資料夾框 → 需選資料夾者以桌面開啟
                    ViaPowerShell = c.ViaPowerShell, CloseKey = c.CloseKey, CloseCount = c.CloseCount,
                });
                labels.Add(c.Name);
            }
            _remoteConnList = list;
            return (IReadOnlyList<string>)labels;
        });

    (int Id, string Title) IRemoteHost.OpenConnection(int index)
        => Dispatcher.Invoke(() =>
        {
            if (index < 1 || index > _remoteConnList.Count || !_webReady) return (0, "");
            int before = Tabs.Count;
            ReopenHistory(_remoteConnList[index - 1]);   // 清單項一律 PickDir=false、Dir=桌面（不會跳對話框）
            var tab = Tabs.Count > before ? Tabs[^1] : null;
            return tab == null ? (0, "") : (tab.Id, tab.Title);
        });

    (int Id, string Title) IRemoteHost.OpenSsh(string host, int port)
        => Dispatcher.Invoke(() =>
        {
            if (!_webReady || string.IsNullOrWhiteSpace(host)) return (0, "");
            int before = Tabs.Count;
            if (host.Contains('@'))
            {
                // 帶 user@ → 直接啟動 ssh.exe（同開機恢復的直啟路徑），密碼照畫面提示輸入
                var s = new ConPtySession { GracefulExitBytes = new byte[] { 0x04, 0x04, 0x04 } };
                var t = StartTab(TermKind.Ssh, host, s, () => s.Start(SshCommand(host, port), _lastCols, _lastRows, null));
                if (t != null) t.Restore = new SavedTab { Type = "ssh", Host = host, Port = port };
                AddHistory(new SavedTab { Type = "ssh", Host = host, Port = port });
            }
            else OpenSshLoginAs(host, port);
            var tab = Tabs.Count > before ? Tabs[^1] : null;
            return tab == null ? (0, "") : (tab.Id, tab.Title);
        });

    (int Id, string Title) IRemoteHost.OpenTelnet(string host, int port)
        => Dispatcher.Invoke(() =>
        {
            if (!_webReady || string.IsNullOrWhiteSpace(host)) return (0, "");
            int before = Tabs.Count;
            OpenTelnetDirect(host, port);
            var tab = Tabs.Count > before ? Tabs[^1] : null;
            return tab == null ? (0, "") : (tab.Id, tab.Title);
        });

    // /history 用：上次列給遠端的紀錄快照（OpenHistory 依此索引重開）
    private List<SavedTab> _remoteHistList = new();

    IReadOnlyList<string> IRemoteHost.ListHistory()
        => Dispatcher.Invoke(() =>
        {
            _remoteHistList = AppSettings.Current.History.Take(10).ToList();
            return (IReadOnlyList<string>)_remoteHistList.Select(e => e.Type switch
            {
                "ssh" => "SSH " + e.Host,          // 手機純文字列表沒有圖示 → ssh/telnet 補型態前綴
                "telnet" => $"Telnet {e.Host}:{e.Port}",
                _ => HistoryLabel(e)
            }).ToList();
        });

    (int Id, string Title) IRemoteHost.OpenHistory(int index)
        => Dispatcher.Invoke(() =>
        {
            if (index < 1 || index > _remoteHistList.Count || !_webReady) return (0, "");
            var e = _remoteHistList[index - 1];
            if (e.PickDir)   // 遠端不能跳資料夾框 → 以桌面為工作目錄
            {
                e = CloneTab(e);
                e.PickDir = false;
                e.Dir = Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);
            }
            int before = Tabs.Count;
            ReopenHistory(e);
            var tab = Tabs.Count > before ? Tabs[^1] : null;
            return tab == null ? (0, "") : (tab.Id, tab.Title);
        });

    async Task<byte[]?> IRemoteHost.CaptureTabPngAsync(int tabId)
    {
        return await await Dispatcher.InvokeAsync(async () =>
        {
            if (Web.CoreWebView2 == null) return null;
            var tab = FindTab(tabId);
            if (tab != null && _active != tab) { SelectTab(tab); await Task.Delay(350); }   // 切到前景再拍
            using var ms = new MemoryStream();
            await Web.CoreWebView2.CapturePreviewAsync(CoreWebView2CapturePreviewImageFormat.Png, ms);
            return ms.ToArray();
        });
    }

    bool IRemoteHost.CloseTab(int tabId)
        => Dispatcher.Invoke(() =>
        {
            var tab = FindTab(tabId);
            if (tab == null) return false;
            RemoveTabSilently(tab);   // 手機端已下指令 → 不再跳 PC 端確認框，直接走優雅結束流程
            return true;
        });

    IReadOnlyList<RemoteTabInfo> IRemoteHost.SnapshotTabs()
        => Dispatcher.Invoke(() => (IReadOnlyList<RemoteTabInfo>)Tabs.Select(
               t => new RemoteTabInfo(t.Id, t.Title, t.Status == TermStatus.Busy, t.Kind.ToString())).ToList());

    bool IRemoteHost.SendInputToTab(int tabId, string text, bool enter)
        => Dispatcher.Invoke(() =>
        {
            var tab = FindTab(tabId);
            if (tab == null) return false;
            string t = enter ? text + "\r" : text;
            if (tab.Session == null && tab.LoginBuffer != null)
            { HandleLoginInput(tab, t); return true; }   // SSH「login as:」：帳號從遠端回覆也能登入
            if (tab.Session == null) return false;
            tab.Session.WriteText(t);
            return true;
        });

    bool IRemoteHost.SendKeyToTab(int tabId, string keyName)
        => Dispatcher.Invoke(() =>
        {
            var tab = FindTab(tabId);
            if (tab?.Session == null) return false;
            byte[]? b = keyName switch
            {
                "ctrl-c" => new byte[] { 0x03 },
                "ctrl-d" => new byte[] { 0x04 },
                "esc" => new byte[] { 0x1b },
                "tab" => new byte[] { 0x09 },
                "enter" => new byte[] { 0x0d },
                "up" => new byte[] { 0x1b, (byte)'[', (byte)'A' },
                "down" => new byte[] { 0x1b, (byte)'[', (byte)'B' },
                "right" => new byte[] { 0x1b, (byte)'[', (byte)'C' },
                "left" => new byte[] { 0x1b, (byte)'[', (byte)'D' },
                _ => null
            };
            if (b == null) return false;
            tab.Session.Write(b);
            return true;
        });

    string IRemoteHost.GetRecentText(int tabId, int lines)
    {
        // 首選：向 xterm 查詢「渲染後」的畫面文字（q…text → a…text）。xterm 已把 TUI 的
        // 原地重繪全部合成完畢，拿到的是乾淨整行，不會有差分重繪碎片（背景執行緒短暫等待）。
        var tcs = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        Dispatcher.Invoke(() =>
        {
            if (FindTab(tabId) == null) { tcs.TrySetResult(""); return; }
            _remoteTextTcs = tcs;
            PostToWeb("q" + tabId + US + "text");
        });
        try
        {
            if (tcs.Task.Wait(1500) && tcs.Task.Result.Length > 0)
            {
                var a = tcs.Task.Result.Replace("\r", "").Split('\n');
                int s0 = Math.Max(0, a.Length - lines);
                return string.Join("\n", a, s0, a.Length - s0);
            }
        }
        catch { }

        // 備援（WebView2 未回覆）：退回原始位元組流緩衝
        return Dispatcher.Invoke(() =>
        {
            var tab = FindTab(tabId);
            if (tab == null) return "";
            string all;
            lock (tab.RemoteLock) all = tab.RemoteRecent.ToString();
            if (all.Length == 0) return "";
            var arr = all.Replace("\r", "").Split('\n');
            int end = arr.Length;
            while (end > 0 && arr[end - 1].Length == 0) end--;   // 去尾端空行
            int start = Math.Max(0, end - lines);
            return string.Join("\n", arr.Skip(start).Take(end - start));
        });
    }

    private void Settings_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new SettingsDialog { Owner = this };
        // 語言切換由 Loc.Changed 自動套用工具列；字型/背景改動於此重新套到終端機
        if (dlg.ShowDialog() == true) { PostTheme(); ApplyWebDefaultBg(); }
    }

    /// <summary>找不到 adb：說明並詢問是否開啟官方 platform-tools 下載頁。</summary>
    private void PromptInstallAdb()
    {
        var r = MessageBox.Show(this, Loc.T("adb.notInstalled"), "AwayTerminal",
            MessageBoxButton.YesNo, MessageBoxImage.Information);
        if (r != MessageBoxResult.Yes) return;
        try
        {
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
            { FileName = AppSettings.AdbDownloadUrl, UseShellExecute = true });
        }
        catch { }
    }

    // ---------- 工具列：ADB ----------
    private void OpenAdb_Click(object sender, RoutedEventArgs e)
    {
        if (!_webReady) return;

        // 搜尋這台電腦上的 adb（PATH / Android SDK 位置 / 設定裡指定的路徑）
        string? adb = AppSettings.ResolveAdbPath();
        if (adb == null) { PromptInstallAdb(); return; }

        List<string> devices;
        try { devices = AdbDevices(adb); }
        catch (Exception ex)
        {
            MessageBox.Show(this, Loc.T("msg.connectFail") + "\n" + ex.Message, "AwayTerminal",
                MessageBoxButton.OK, MessageBoxImage.Error);
            return;
        }

        if (devices.Count == 0)
        {
            MessageBox.Show(this, Loc.T("adb.noDevice"), "AwayTerminal",
                MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }
        if (devices.Count == 1)
        {
            OpenAdbShell(adb, null, NextName("ADB")); // 只有一台 → 直接開
            return;
        }

        // 兩台以上 → 上拉選單讓使用者選裝置
        var menu = new ContextMenu();
        foreach (var d in devices)
        {
            string id = d;
            var mi = new MenuItem { Header = id };
            mi.Click += (_, _) => OpenAdbShell(adb, id, id);
            menu.Items.Add(mi);
        }
        menu.PlacementTarget = BtnNew;
        menu.Placement = System.Windows.Controls.Primitives.PlacementMode.Bottom;
        menu.IsOpen = true;
    }

    /// <summary>執行 `adb devices`（直接跑 adb.exe，不透過 PowerShell），回傳線上裝置序號。</summary>
    private static List<string> AdbDevices(string adb)
    {
        var psi = new ProcessStartInfo
        {
            FileName = adb,
            Arguments = "devices",
            RedirectStandardOutput = true,
            UseShellExecute = false,
            CreateNoWindow = true,
            StandardOutputEncoding = Encoding.UTF8
        };
        var list = new List<string>();
        using var p = Process.Start(psi);
        if (p == null) return list;
        string output = p.StandardOutput.ReadToEnd();
        try { p.WaitForExit(5000); } catch { }
        foreach (var line in output.Split('\n'))
        {
            // 格式：<serial>\tdevice；跳過標頭與 offline / unauthorized
            int tab = line.IndexOf('\t');
            if (tab <= 0) continue;
            string serial = line.Substring(0, tab).Trim();
            string state = line.Substring(tab + 1).Trim();
            if (state == "device" && serial.Length > 0) list.Add(serial);
        }
        return list;
    }

    /// <summary>直接以 ConPTY 執行 adb shell（不經過 PowerShell）；關閉時送 Ctrl+C ×3。</summary>
    private void OpenAdbShell(string adb, string? serial, string title)
    {
        var s = new ConPtySession(); // 預設 GracefulExitBytes = Ctrl+C ×3
        string args = serial == null ? "shell" : $"-s {serial} shell";
        StartTab(TermKind.Adb, title, s, () => s.Start($"\"{adb}\" {args}", _lastCols, _lastRows, null));
        AddHistory(new SavedTab { Type = "adb", Title = title, AdbSerial = serial ?? "" });
        // ADB 分頁不做開機還原（每次都需重新偵測裝置），故不設 Restore
    }

    private void About_Click(object sender, RoutedEventArgs e)
    {
        var ver = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version;
        string v = ver == null ? "0.0.1" : $"{ver.Major}.{ver.Minor}.{ver.Build}";
        var gray = new System.Windows.Media.SolidColorBrush(
            (System.Windows.Media.Color)System.Windows.Media.ColorConverter.ConvertFromString("#E0E0E0"));

        var panel = new StackPanel { Margin = new Thickness(24, 18, 24, 14) };   // 內容一律置左
        panel.Children.Add(new TextBlock
        {
            Text = "AwayTerminal", FontSize = 18, FontWeight = FontWeights.Bold,
            Foreground = System.Windows.Media.Brushes.White
        });
        panel.Children.Add(new TextBlock
        { Text = $"{Loc.T("about.version")} v{v}", Foreground = gray, Margin = new Thickness(0, 6, 0, 0) });

        panel.Children.Add(new TextBlock
        { Text = Loc.T("about.author") + ":", Foreground = gray, Margin = new Thickness(0, 14, 0, 0) });
        // 作者行以圖片顯示（執行期渲染，畫面上沒有可複製/可被爬的 email 文字）
        panel.Children.Add(new Image
        {
            Source = RenderTextImage("Awaysu (weisu.tech" + "@" + "gmail.com)"),
            Stretch = System.Windows.Media.Stretch.None,
            HorizontalAlignment = HorizontalAlignment.Left,
            Margin = new Thickness(0, 2, 0, 0)
        });

        panel.Children.Add(new TextBlock
        { Text = "Source Code:", Foreground = gray, Margin = new Thickness(0, 14, 0, 0) });
        var link = new System.Windows.Documents.Hyperlink(
            new System.Windows.Documents.Run("https://github.com/awaysu/AwayTerminal"))
        { Foreground = new System.Windows.Media.SolidColorBrush(
            (System.Windows.Media.Color)System.Windows.Media.ColorConverter.ConvertFromString("#4FC1FF")) };
        link.Click += (_, _) =>
        {
            try
            {
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(
                    "https://github.com/awaysu/AwayTerminal") { UseShellExecute = true });
            }
            catch { }
        };
        var linkText = new TextBlock { Margin = new Thickness(0, 2, 0, 0) };
        linkText.Inlines.Add(link);
        panel.Children.Add(linkText);

        // 編譯時間＝本 exe 的檔案寫入時間（build 當下寫檔；複製/安裝都會保留原時間戳）
        string built;
        try
        {
            built = File.GetLastWriteTime(Environment.ProcessPath ?? AppContext.BaseDirectory)
                .ToString("yyyy/M/d HH:mm:ss", System.Globalization.CultureInfo.InvariantCulture);
        }
        catch { built = "-"; }
        panel.Children.Add(new TextBlock
        { Text = $"{Loc.T("about.buildTime")}: {built}", Foreground = gray, Margin = new Thickness(0, 14, 0, 0) });

        var ok = new Button
        { Content = "OK", Width = 76, Height = 26, Margin = new Thickness(0, 18, 0, 0), HorizontalAlignment = HorizontalAlignment.Right, IsDefault = true, IsCancel = true };
        panel.Children.Add(ok);

        var win = new Window
        {
            Title = Loc.T("about.title"),
            Owner = this,
            SizeToContent = SizeToContent.WidthAndHeight,
            WindowStartupLocation = WindowStartupLocation.CenterOwner,
            ResizeMode = ResizeMode.NoResize,
            ShowInTaskbar = false,
            Background = new System.Windows.Media.SolidColorBrush(
                (System.Windows.Media.Color)System.Windows.Media.ColorConverter.ConvertFromString("#2D2D30")),
            Content = panel
        };
        ok.Click += (_, _) => win.Close();
        win.ShowDialog();
    }

    /// <summary>把一行文字畫成點陣圖（依目前 DPI 渲染，不會糊）。「關於」的 email 用圖片顯示防收集。</summary>
    private System.Windows.Media.Imaging.BitmapSource RenderTextImage(string text)
    {
        var dpi = System.Windows.Media.VisualTreeHelper.GetDpi(this);
        var ft = new System.Windows.Media.FormattedText(
            text, System.Globalization.CultureInfo.InvariantCulture, FlowDirection.LeftToRight,
            new System.Windows.Media.Typeface(new System.Windows.Media.FontFamily("Segoe UI"),
                FontStyles.Normal, FontWeights.Normal, FontStretches.Normal),
            13, System.Windows.Media.Brushes.White, dpi.PixelsPerDip);
        var dv = new System.Windows.Media.DrawingVisual();
        using (var dc = dv.RenderOpen())
            dc.DrawText(ft, new Point(0, 0));
        var bmp = new System.Windows.Media.Imaging.RenderTargetBitmap(
            (int)Math.Ceiling(ft.Width * dpi.DpiScaleX) + 1,
            (int)Math.Ceiling(ft.Height * dpi.DpiScaleY) + 1,
            dpi.PixelsPerInchX, dpi.PixelsPerInchY,
            System.Windows.Media.PixelFormats.Pbgra32);
        bmp.Render(dv);
        bmp.Freeze();
        return bmp;
    }

    // ---------- 分頁互動 ----------
    private static TerminalTab? TabOf(object sender) => (sender as FrameworkElement)?.DataContext as TerminalTab;

    private void Tab_Click(object sender, MouseButtonEventArgs e)
    {
        var tab = TabOf(sender);
        if (tab != null) SelectTab(tab);
    }

    private void TabClose_Click(object sender, RoutedEventArgs e)
    {
        e.Handled = true;
        var tab = TabOf(sender);
        if (tab != null) CloseTab(tab);
    }

    private void MenuRename_Click(object sender, RoutedEventArgs e)
    {
        var tab = TabOf(sender);
        if (tab == null) return;
        string? name = InputDialog.Show(this, Loc.T("dlg.renameTitle"), Loc.T("dlg.renamePrompt"), tab.Title);
        if (!string.IsNullOrWhiteSpace(name))
        {
            tab.Title = name.Trim();
            PostToWeb("t" + tab.Id + US + tab.Title); // 同步分割模式 pane 標題
        }
    }

    private void MenuLog_Click(object sender, RoutedEventArgs e)
    {
        var tab = TabOf(sender);
        if (tab != null) LogAction(tab);
    }

    private void MenuMacro_Click(object sender, RoutedEventArgs e)
    {
        var tab = TabOf(sender);
        if (tab != null) MacroAction(tab);
    }

    /// <summary>分頁右鍵「配色」：套用該分頁的文字/背景色。Tag="fg|bg"；空 Tag = 回到設定預設顏色。</summary>
    private void MenuColor_Click(object sender, RoutedEventArgs e)
    {
        var tab = TabOf(sender);
        if (tab == null) return;
        string tag = (sender as MenuItem)?.Tag as string ?? "";
        string fg = "", bg = "";
        var parts = tag.Split('|');
        if (parts.Length == 2) { fg = parts[0]; bg = parts[1]; }
        // P{id}US{fg}US{bg}；fg/bg 皆空 → 前端清除覆寫、回到設定預設
        PostToWeb("P" + tab.Id + US + fg + US + bg);
    }

    private void LogIcon_Click(object sender, MouseButtonEventArgs e)
    {
        e.Handled = true;
        var tab = TabOf(sender);
        if (tab == null) return;
        SelectTab(tab);
        LogAction(tab);
    }

    private void MacroIcon_Click(object sender, MouseButtonEventArgs e)
    {
        e.Handled = true;
        var tab = TabOf(sender);
        if (tab == null) return;
        SelectTab(tab);
        MacroAction(tab);
    }

    // ---------- 記錄 log ----------
    private void LogAction(TerminalTab tab)
    {
        if (tab.Logger is SessionLogger)
        {
            if (MessageBox.Show(this, Loc.T("msg.stopLogAsk"), Loc.T("dlg.logTitle"),
                    MessageBoxButton.YesNo, MessageBoxImage.Question) == MessageBoxResult.Yes)
                StopLogging(tab, openFolder: true);
            return;
        }

        var dlg = new LogDialog(tab.Title) { Owner = this };
        if (dlg.ShowDialog() != true) return;
        try
        {
            tab.Logger = new SessionLogger(dlg.LogPath, dlg.Timestamp, dlg.Append);
            tab.IsLogging = true;
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, Loc.T("msg.logFail") + "\n" + ex.Message, "AwayTerminal",
                MessageBoxButton.OK, MessageBoxImage.Error);
        }
    }

    private void StopLogging(TerminalTab tab, bool openFolder)
    {
        if (tab.Logger is not SessionLogger logger) return;
        string path = logger.FilePath;
        try { logger.Dispose(); } catch { }
        tab.Logger = null;
        tab.IsLogging = false;
        if (openFolder)
        {
            try { Process.Start("explorer.exe", $"/select,\"{path}\""); } catch { }
        }
    }

    // ---------- 巨集 ----------
    private void MacroAction(TerminalTab tab)
    {
        if (tab.Macro is MacroRunner running)
        {
            if (MessageBox.Show(this, Loc.T("msg.stopMacroAsk"), Loc.T("dlg.macroTitle"),
                    MessageBoxButton.YesNo, MessageBoxImage.Question) == MessageBoxResult.Yes)
                running.Stop();
            return;
        }
        if (tab.Session == null) return;

        var ofd = new Microsoft.Win32.OpenFileDialog
        {
            Filter = Loc.Lang == "en"
                ? "TeraTerm macro (*.ttl)|*.ttl|All files (*.*)|*.*"
                : "TeraTerm 巨集 (*.ttl)|*.ttl|所有檔案 (*.*)|*.*"
        };
        if (ofd.ShowDialog(this) != true) return;

        string[] lines;
        try { lines = File.ReadAllLines(ofd.FileName); }
        catch (Exception ex)
        {
            MessageBox.Show(this, Loc.T("msg.macroReadFail") + "\n" + ex.Message, "AwayTerminal",
                MessageBoxButton.OK, MessageBoxImage.Error);
            return;
        }

        var runner = new MacroRunner(tab.Session, lines);
        runner.MessageRequested += (m, t) => Dispatcher.Invoke(() =>
            MessageBox.Show(this, m, string.IsNullOrEmpty(t) ? "Macro" : t, MessageBoxButton.OK, MessageBoxImage.Information));
        runner.ConfirmRequested += (m, t) => Dispatcher.Invoke(() =>
            MessageBox.Show(this, m, string.IsNullOrEmpty(t) ? "Macro" : t, MessageBoxButton.YesNo, MessageBoxImage.Question) == MessageBoxResult.Yes);
        runner.InputRequested += (m, t, def) => Dispatcher.Invoke(() =>
            InputDialog.Show(this, string.IsNullOrEmpty(t) ? "Macro" : t, m, def));
        runner.Finished += () => Dispatcher.InvokeAsync(() =>
        {
            tab.IsMacroRunning = false;
            if (ReferenceEquals(tab.Macro, runner)) tab.Macro = null;
        });
        tab.Macro = runner;
        tab.IsMacroRunning = true;
        runner.Start();
    }

    private void MenuClose_Click(object sender, RoutedEventArgs e)
    {
        var tab = TabOf(sender);
        if (tab != null) CloseTab(tab);
    }

    // ---------- 小工具 ----------
    private void Info(string msg)
        => MessageBox.Show(this, msg, "AwayTerminal", MessageBoxButton.OK, MessageBoxImage.Information);

    private static string SafeName(string dir)
    {
        try
        {
            var n = Path.GetFileName(dir.TrimEnd('\\', '/'));
            return string.IsNullOrEmpty(n) ? dir : n;
        }
        catch { return "PowerShell"; }
    }

    private void OnClosed(object? sender, EventArgs e)
    {
        _statusTimer?.Stop();
        var tasks = new List<System.Threading.Tasks.Task>();
        foreach (var t in Tabs)
        {
            try { (t.Macro as MacroRunner)?.Stop(); } catch { }
            try { (t.Logger as SessionLogger)?.Dispose(); } catch { }
            var s = t.Session;
            t.Session = null;
            if (s != null) tasks.Add(System.Threading.Tasks.Task.Run(() => { try { s.Dispose(); } catch { } }));
        }
        // 各分頁平行收尾，最多等 0.8 秒（背景會繼續，程式不卡）
        try { System.Threading.Tasks.Task.WaitAll(tasks.ToArray(), 800); } catch { }
    }
}
