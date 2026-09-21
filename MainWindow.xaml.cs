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

    /// <summary>WebView2 ready 前使用者點的開連線動作（ready 後補執行；一格、後點的蓋前面的）。</summary>
    private Action? _pendingReadyAction;

    /// <summary>WebView2 尚未 ready 時把動作排隊並回傳 true（呼叫端直接 return）。
    /// 啟動後頭幾秒點「New→…」原本被 `if (!_webReady) return` 靜默吞掉＝「點了沒反應、
    /// 對話框沒蹦出來」（diag.log 2026-08-11 實錄 webReady=False）；排隊後 ready 一到就補執行。</summary>
    private bool DeferUntilWebReady(Action action, string what)
    {
        if (_webReady) return false;
        Diag.Log($"defer until web ready: {what}");
        _pendingReadyAction = action;
        return true;
    }
    private bool _exiting;            // OnClosingAsk 確認離開後為 true：session Exited 不再跳詢問框
    private string _viewMode = "tab"; // tab | split | columns
    private bool _splitMode => _viewMode != "tab"; // 分割或分欄時終端機外框讓給各 pane
    private string _webRoot = "";
    private int _lastCols = 80, _lastRows = 24; // 前端最近回報的終端機尺寸（新分頁初始值用）
    private DispatcherTimer? _statusTimer;
    private DispatcherTimer? _copyPopupTimer;

    // 遠端（Telegram）：服務 + 忙碌起始時間（用來過濾閃爍、只在忙碌≥3秒後推播閒置）
    private TelegramRemote? _remote;
    private readonly Dictionary<int, DateTime> _busySince = new();
    // q…text 查詢的等待者（依分頁 id；只在 UI 執行緒存取）。1.1.10 起遠端在執行緒池跑、指令與完成推播可能同時查——
    // 舊版單一欄位會被後一個查詢蓋掉，前一個必逾時退回劣化備援；改成同分頁的等待者一起用同一份回覆完成。
    private readonly Dictionary<int, List<TaskCompletionSource<string>>> _remoteTextWaiters = new();
    private string? _selMouseHintId;   // 1.1.10：JS 回報「選取是空的、因為程式接管了滑鼠」（m 協定）→ 下一個空選取提示改說按住 Shift

    /// <summary>RestoreTabs 逐筆設定：下一個 AddTab 要先倒回的 scrollback（內容, 分隔行）；AddTab 用掉就清（1.0.45）。</summary>
    private (string buf, string sep)? _restoreBufferForNextTab;
    private DateTime? _restoreOpenedForNextTab;   // 1.1.4：恢復分頁的原始開啟時間，AddTab 消化（同 _restoreBufferForNextTab 機制）
    /// <summary>關閉程式時等前端回傳各分頁 scrollback（a…save）的等待表（1.0.45）。</summary>
    private readonly Dictionary<int, TaskCompletionSource<string>> _saveBufTcs = new();

    /// <summary>依型態產生分頁預設名稱「{prefix}({n})」，跳過已存在名稱。例：PowerShell(1)。</summary>
    private string NextName(string prefix)
    {
        int n = 0;
        string name;
        do { name = $"{prefix}({++n})"; } while (Tabs.Any(t => t.Title == name));
        return name;
    }

    /// <summary>分頁名稱最長字數（ClaudeCode / OpenCode 以資料夾命名時用）。</summary>
    /// <summary>ClaudeCode / OpenCode / PowerShell 分頁名稱＝工作目錄名稱（1.1.2 起不截字：過長只在分頁列
    /// 顯示層以 CharacterEllipsis 截、改名與 tooltip 看得到全名；1.0.44~1.1.2 前取前 15 字）。
    /// 例：C:\Users\me\Desktop\WORKSPACE2\AwayTerminal → 「AwayTerminal」。
    /// 同一資料夾再開一個 → 補「(2)」「(3)」；取不到目錄名（沒選資料夾）→ 退回 NextName(prefix)。</summary>
    private string DirTabName(string? dir, string prefix)
    {
        string name = ShortDir(dir ?? "");
        if (string.IsNullOrWhiteSpace(name))
            return NextName(string.IsNullOrWhiteSpace(prefix) ? "Custom" : prefix);
        if (!Tabs.Any(t => t.Title == name)) return name;
        int n = 1;
        string dup;
        do { dup = $"{name}({++n})"; } while (Tabs.Any(t => t.Title == dup));
        return dup;
    }

    /// <summary>這條連線是不是 ClaudeCode / Codex / OpenCode / GeminiCLI / QwenCode（分頁改用資料夾名稱，見 DirTabName）。
    /// 先看圖示 key（「自動偵測」加入的就是 claude-code / codex / opencode / geminicli），使用者換過圖示或
    /// 手動新增的則看執行檔名。其餘連線（PowerShell / WSL / ADB / Aider…）維持原本命名。</summary>
    private static bool UsesDirTitle(string path, string icon)
    {
        if (icon is "claude-code" or "opencode" or "codex" or "geminicli") return true;
        try
        {
            string exe = Path.GetFileNameWithoutExtension(path);
            return exe.Contains("claude", StringComparison.OrdinalIgnoreCase)
                || exe.Contains("codex", StringComparison.OrdinalIgnoreCase)
                || exe.Contains("opencode", StringComparison.OrdinalIgnoreCase)
                || exe.Contains("gemini", StringComparison.OrdinalIgnoreCase)
                || exe.Contains("qwen", StringComparison.OrdinalIgnoreCase);
        }
        catch { return false; }
    }

    // 在指定按鈕旁邊浮出提示（複製成功等）；atMouse＝浮在滑鼠位置（網址選單「複製網址」用，按鈕不在附近）
    private void ShowCopyFeedback(FrameworkElement target, string msg, bool atMouse = false)
    {
        CopyPopupText.Text = msg;
        CopyPopup.PlacementTarget = target;
        CopyPopup.Placement = atMouse ? System.Windows.Controls.Primitives.PlacementMode.MousePoint
                                      : System.Windows.Controls.Primitives.PlacementMode.Bottom;
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
        Loaded += OnLoaded;
        Closing += OnClosingAsk;
        Closed += OnClosed;
    }

    /// <summary>關閉時彈自訂視窗；確認後交給 FinishExitAsync（先向前端要 scrollback 存檔，再快速收尾並立即結束）。</summary>
    private void OnClosingAsk(object? sender, System.ComponentModel.CancelEventArgs e)
    {
        e.Cancel = true; // 先攔下，改由自訂視窗決定
        if (_exiting) return;   // 已確認離開、正在等前端回傳 scrollback（最多 2.5 秒）→ 重複的關閉請求忽略

        var dlg = new ExitDialog { Owner = this };
        // 以 ClaudePaste 找 claude 分頁——自訂連線開的 ClaudeCode 沒有 Restore，
        // 用 Restore.Type == "claude" 只會找到舊紀錄恢復的 legacy 分頁（v1.0.18 後等於永遠 false）
        // Multi-Agent 的 claude 不算（1.2.0：好幾個 agent 同時改同一份 CLAUDE.md 會互相覆蓋）
        dlg.SetClaudeAvailable(Tabs.Any(t => t.ClaudePaste && t.Session != null && t.Agent == null));
        dlg.UpdateAction = UpdateClaudeMdAsync;
        if (dlg.ShowDialog() != true) return; // 取消 → 不關

        // 記住兩個勾選狀態，下次開啟關閉視窗時沿用
        AppSettings.Current.ExitRestoreTabs = dlg.RestoreTabs;
        AppSettings.Current.ExitUpdateMd = dlg.UpdateMd;

        _exiting = true;   // 之後 session 的 Exited 不再跳「要關閉分頁嗎」；X 再按也不再問
        _ = FinishExitAsync(dlg.RestoreTabs);
    }

    /// <summary>確認離開後的收尾（1.0.45 改為非同步）：勾「恢復分頁」時先向前端要每個分頁的 scrollback
    /// （q…save，xterm 序列化、含顏色）存到 AppSettings.RestoreDir、SavedTab.BufferFile 記檔名——
    /// 下次開啟 RestoreTabs 會先把它倒回分頁再啟動連線（舊訊息在上、新連線在下）。
    /// 之後存設定、終止子行程（不送 Ctrl+C、不等待）、Environment.Exit(0)（跳過 WebView2 冗長 teardown）。</summary>
    private async Task FinishExitAsync(bool restore)
    {
        var sw = System.Diagnostics.Stopwatch.StartNew();
        long tCapture, tSave, tNotify;
        // 1.2.6：確認離開就先把視窗藏起來＝使用者看到的是「立刻關掉」；存 scrollback／設定、Telegram 離線通知、
        // 收 session、WebView2 teardown（實測 Environment.Exit 本身就要約 0.4 秒）都在看不見的狀態下做完。
        // 藏起來的 WebView2 照樣處理訊息（q…save 回得來，實測）。
        try { Hide(); } catch { }
        var tabs = restore ? Tabs.Where(t => t.Restore != null).ToList() : new List<TerminalTab>();
        var bufs = new Dictionary<int, string>();
        if (tabs.Count > 0 && AppSettings.Current.RestoreBufferLines > 0)
        {
            try { bufs = await CaptureBuffersAsync(tabs, 2500); } catch (Exception ex) { Diag.Log("capture buffers: " + ex.Message); }
        }

        tCapture = sw.ElapsedMilliseconds;

        // 暫存目錄每次重寫：舊檔全清，不累積
        string dir = AppSettings.RestoreDir;
        try
        {
            Directory.CreateDirectory(dir);
            foreach (var f in Directory.GetFiles(dir)) { try { File.Delete(f); } catch { } }
        }
        catch { }

        var saved = new List<SavedTab>();
        int n = 0;
        foreach (var t in tabs)
        {
            var s2 = t.Restore!;
            s2.Title = t.Title;
            s2.OpenedUtc = t.StartUtc;   // 1.1.4：存原始開啟時間，恢復後 tooltip 的執行時長（1.2.5 日:時:分）接著算、不歸零
            // 1.2.0 Multi-Agent：各格存同一個組代號＋格號／組號／角色／CLI／比例，恢復時整組重開
            var ag = t.Agent;
            s2.AgentKey = ag?.Group.Key ?? "";
            s2.AgentIndex = ag?.Index ?? 0;
            s2.AgentGroupNumber = ag?.Group.Number ?? 0;
            s2.AgentRole = ag?.Role ?? "";
            s2.AgentBackend = ag?.Backend ?? "";
            s2.AgentRatio = ag?.Group.Ratio ?? 0.5;
            s2.AgentMaxMessages = ag?.Group.MaxMessages ?? AgentGroup.DefaultMaxMessages;
            s2.AgentIdleCheck = ag?.Group.IdleCheckMinutes ?? AgentGroup.DefaultIdleCheckMinutes;
            s2.AgentMode = ag?.Group.IsChat == true ? 1 : 0;                       // 1.2.3：聊天室要連討論進度一起回來
            s2.AgentRounds = ag?.Group.Rounds ?? AgentGroup.DefaultRounds;
            s2.AgentChatFolder = ag?.Group.ChatFolder ?? "";
            s2.BufferFile = "";
            if (bufs.TryGetValue(t.Id, out var text) && !string.IsNullOrEmpty(text))
            {
                string name = $"tab{++n}.txt";
                try { File.WriteAllText(Path.Combine(dir, name), text, new UTF8Encoding(false)); s2.BufferFile = name; }
                catch (Exception ex) { Diag.Log($"save buffer {name}: {ex.Message}"); }
            }
            saved.Add(s2);
        }
        AppSettings.Current.SavedTabs = saved;
        AppSettings.Current.Save();
        tSave = sw.ElapsedMilliseconds;

        // 快速收尾：終止子行程（不送 Ctrl+C、不等待），然後立即結束程式
        _statusTimer?.Stop();
        _bounceTimer?.Stop();
        _remote?.NotifyOfflineBlocking();   // 關閉前先告知手機「遠端離線」（使用者選項；只有遠端在跑才送）
        _remote?.Stop();
        tNotify = sw.ElapsedMilliseconds;
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
        Diag.Log($"exit: tabs={Tabs.Count} capture={tCapture}ms save={tSave - tCapture}ms notify={tNotify - tSave}ms dispose={sw.ElapsedMilliseconds - tNotify}ms");
        // 看門狗：Environment.Exit 要跑 finalizer／DLL 卸載／WebView2 收尾，偶爾會拖很久甚至卡死（視窗已藏、行程卻留著）；
        // 該存的都存完了，1.5 秒還沒走完就直接砍掉自己。
        new Thread(() =>
        {
            Thread.Sleep(1500);
            try { System.Diagnostics.Process.GetCurrentProcess().Kill(); } catch { }
        }) { IsBackground = true }.Start();
        Environment.Exit(0); // 立即終止，msedgewebview2 等子程序會隨之結束
    }

    /// <summary>向前端要各分頁 scrollback 序列化（q…save → a…save）；逾時就只拿已回的那些。</summary>
    private async Task<Dictionary<int, string>> CaptureBuffersAsync(List<TerminalTab> tabs, int timeoutMs)
    {
        var result = new Dictionary<int, string>();
        if (!_webReady) return result;
        _saveBufTcs.Clear();
        var waits = new List<Task>();
        foreach (var t in tabs)
        {
            var tcs = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
            _saveBufTcs[t.Id] = tcs;
            waits.Add(tcs.Task);
            PostToWeb("q" + t.Id + US + "save");
        }
        await Task.WhenAny(Task.WhenAll(waits), Task.Delay(timeoutMs));
        foreach (var t in tabs)
            if (_saveBufTcs.TryGetValue(t.Id, out var tcs) && tcs.Task.IsCompletedSuccessfully) result[t.Id] = tcs.Task.Result;
        _saveBufTcs.Clear();
        return result;
    }

    /// <summary>請每個 Claude Code 分頁更新 CLAUDE.md，並等到它們都閒置（或逾時）。</summary>
    private async Task UpdateClaudeMdAsync()
    {
        var claudeTabs = Tabs.Where(t => t.ClaudePaste && t.Session != null && t.Agent == null).ToList();
        if (claudeTabs.Count == 0) return;

        string prompt = Loc.T("exit.mdPrompt");
        foreach (var t in claudeTabs)
        {
            t.LastOutputUtc = DateTime.UtcNow;      // 重置活動計時
            t.Session!.WriteText(prompt);
        }
        // 文字與 Enter 一定分開送（1.1.10 實測：一次寫入「文字＋CR」claude 會當成貼上、CR 變成輸入框換行、沒送出）
        await Task.Delay(RemoteEnterDelayMs);
        foreach (var t in claudeTabs) t.Session?.WriteText("\r");

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
        Title = ComposeTitle();
        BtnNew.Content = Loc.T("tb.new"); BtnNew.ToolTip = Loc.T("tip.new");
        BtnFavorites.Content = Loc.T("tb.favorites"); BtnFavorites.ToolTip = Loc.T("tip.favorites");
        BtnCompose.Content = Loc.T("tb.compose"); BtnCompose.ToolTip = Loc.T("tip.compose");
        BtnCopy.Content = Loc.T("tb.copy"); BtnCopy.ToolTip = Loc.T("tip.copy");
        BtnPaste.Content = Loc.T("tb.paste"); BtnPaste.ToolTip = Loc.T("tip.paste");
        BtnCopyAll.Content = Loc.T("tb.copyall"); BtnCopyAll.ToolTip = Loc.T("tip.copyall");
        BtnClear.Content = Loc.T("tb.clear"); BtnClear.ToolTip = Loc.T("tip.clear");
        BtnPage.Content = Loc.T("tb.page"); BtnPage.ToolTip = Loc.T("tip.page");
        BtnPrompt.Content = Loc.T("tb.prompt"); BtnPrompt.ToolTip = Loc.T("tip.prompt");
        BtnRemote.Content = Loc.T("tb.remote"); BtnRemote.ToolTip = Loc.T("tip.remote");
        BtnSettings.Content = Loc.T("tb.settings"); BtnSettings.ToolTip = Loc.T("tip.settings");
        BtnAbout.Content = Loc.T("tb.about"); BtnAbout.ToolTip = Loc.T("tip.about");
        UpdateSplitButton();
        if (_webReady) PostTheme();   // 1.2.0：Multi-Agent pane 狀態標籤的文字隨語言（T 協定 agentStates）
    }

    /// <summary>「New」按鈕：下拉選單（圖示＋文字）。
    /// 預設區＝AwayTerminal 自己實作的連線：PowerShell / SSH-Telnet / 連接埠。
    /// 分隔線後＝使用者自訂的連線（ClaudeCode / ADB / WSL…，全新安裝為空），
    /// 再一條分隔線接「自訂…」管理視窗。</summary>
    private void New_Click(object sender, RoutedEventArgs e)
    {
        var menu = new ContextMenu();
        menu.Items.Add(MakeNewItem("tb.powershell", "powershell.png", OpenPowerShell_Click));
        menu.Items.Add(MakeNewItem("tb.ssh", "ssh-telnet.png", OpenSsh_Click));
        menu.Items.Add(MakeNewItem("tb.com", "com.png", OpenCom_Click));

        // 分隔線後＝使用者自己管理的自訂連線。ADB 也在其中（v1.0.18 起不再內建），
        // 所以刪光自訂連線後這一區就是空的。
        menu.Items.Add(new Separator());

        // 自訂連線（未勾「不顯示」者）→ 點擊直接開
        var customs = AppSettings.Current.CustomConns
            .Where(c => !c.Hidden && !string.IsNullOrWhiteSpace(c.Name)).ToList();
        foreach (var c in customs)
        {
            var conn = c;
            string iconFile = CustomIconFile(conn.Icon);
            var mi = MakeNewItemRaw(conn.Name, iconFile);
            mi.Click += (_, _) => OpenCustom(conn);
            menu.Items.Add(mi);
        }

        // 註：ADB 自 v1.0.18 起**不再是內建項目**，改由自訂連線決定（自訂視窗的
        // 「自動偵測」可一鍵加入）。刪光自訂連線後這一區就是空的，符合預期。
        // 代理團隊（1.2.0）與 AI 聊天室（1.2.3）一律列出（沒有可用的 CLI 時設定視窗會說明）；
        // 使用者指定：AI聊天室在代理團隊下面，它與「自訂…」之間再一條分隔線
        menu.Items.Add(new Separator());
        menu.Items.Add(MakeNewItem("ma.title", "multi-agent.png", OpenMultiAgent_Click));
        menu.Items.Add(MakeNewItem("chat.title", "chatroom.png", OpenChatRoom_Click));
        menu.Items.Add(new Separator());
        var manage = MakeNewItemRaw(Loc.T("menu.custom"), "settings.png");
        manage.Click += Custom_Click;
        menu.Items.Add(manage);

        menu.PlacementTarget = (UIElement)sender;
        menu.Placement = System.Windows.Controls.Primitives.PlacementMode.Bottom;
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
    /// <summary>執行檔是不是 adb（自訂連線指向 adb 時要走裝置偵測流程，而非直接啟動）。</summary>
    private static bool IsAdbExe(string path)
    {
        try { return string.Equals(Path.GetFileNameWithoutExtension(path), "adb", StringComparison.OrdinalIgnoreCase); }
        catch { return false; }
    }

    /// <param name="forcedDir">指定工作目錄（遠端/紀錄/恢復用）；null 且 PickDir 時跳資料夾框。</param>
    /// <param name="restoreTitle">恢復分頁時沿用上次的分頁標題（名稱已被占用時退回 NextName）。</param>
    /// <param name="addHistory">false＝不記進「紀錄」（Multi-Agent 的各格另記一筆 multiagent）。</param>
    /// <param name="extraArgs">附加在連線參數後面、只用在這次啟動（1.2.0 Multi-Agent 注入角色檔；不存進恢復資訊／紀錄）。</param>
    /// <returns>開出來的分頁；沒開成（取消、找不到執行檔、web 未 ready 排隊中、adb 流程）回 null。</returns>
    private TerminalTab? OpenCustom(CustomConn conn, string? forcedDir = null, string? restoreTitle = null,
                                    bool addHistory = true, string extraArgs = "")
    {
        // 診斷點：與 PickWorkDir 的 log 對照可分辨「點了沒進 handler」vs「進了卡在哪一步」
        Diag.Log($"OpenCustom '{conn.Name}' pickDir={conn.PickDir} webReady={_webReady}");
        if (DeferUntilWebReady(() => OpenCustom(conn, forcedDir, restoreTitle, addHistory, extraArgs), $"OpenCustom {conn.Name}")) return null;
        string path = conn.Path;

        // 指向 adb 的自訂連線改走專屬流程：先 adb devices，0 台提示、1 台直接開、
        // 2 台以上跳選單選序號。否則會直接跑「adb shell」，接多台時只會噴錯。
        if (!conn.ViaPowerShell && IsAdbExe(path)) { OpenAdbFlow(path); return null; }

        bool viaPs = conn.ViaPowerShell
            || path.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase)
            || path.EndsWith(".bat", StringComparison.OrdinalIgnoreCase);

        if (!viaPs && !File.Exists(path))
        {
            MessageBox.Show(this, Loc.T("custom.notFound") + "\n" + path, "AwayTerminal",
                MessageBoxButton.OK, MessageBoxImage.Warning);
            return null;
        }

        string? dir = forcedDir;
        if (dir == null && conn.PickDir)
        {
            dir = PickWorkDir(conn.Name);
            if (dir == null) return null; // 使用者取消 → 不開
        }

        string args = (string.IsNullOrWhiteSpace(conn.Args) ? "" : " " + conn.Args.Trim()) + extraArgs;
        // Codex 一律關掉裝飾動畫（使用者回報，2026-09-15）：gpt-6-astra 閒置時輸入框背景「星星閃爍」每秒重畫 6～7 次（probe 實錄 ~8KB/s），
        // 自訂分頁的忙閒＝近 1.2 秒有沒有輸出 → 做完了分頁圖示還是一直紅色；代理團隊則是畫面永不靜止、信送不出去。
        // 使用者自己在參數寫了 tui.whimsy（例如想留動畫）就不動。只在這次啟動加，不寫回連線設定。
        if (IsCodexExe(path) && !args.Contains("tui.whimsy", StringComparison.OrdinalIgnoreCase)) args += " -c tui.whimsy=false";
        // ClaudeCode / Codex / OpenCode → 分頁名稱用工作目錄名稱（例：AwayTerminal），其餘連線照舊「名稱(1)」。
        // 恢復分頁時 restoreTitle 優先（沿用上次看到的名稱）。
        string title = !string.IsNullOrWhiteSpace(restoreTitle) && !Tabs.Any(t => t.Title == restoreTitle)
            ? restoreTitle
            : UsesDirTitle(path, conn.Icon)
                ? DirTabName(dir, conn.Name)
                : NextName(string.IsNullOrWhiteSpace(conn.Name) ? "Custom" : conn.Name);

        // 開機恢復用的資訊（1.0.30 起自訂分頁也恢復）：Dir 存實際工作目錄，恢復時直接用、不再跳資料夾框；
        // Name 存連線名稱（Title 在關閉時會被改成分頁標題，見 SavedTab.Name 註解）
        var restore = new SavedTab
        {
            Type = "custom", Name = conn.Name, Title = title, Dir = dir ?? "",
            Path = path, Args = conn.Args, Icon = conn.Icon,
            PickDir = conn.PickDir, ViaPowerShell = conn.ViaPowerShell,
            CloseKey = conn.CloseKey, CloseCount = conn.CloseCount
        };

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

        TerminalTab? opened;
        if (viaPs)
        {
            var s = new ConPtySession { GracefulExitBytes = closeBytes };
            var tab = StartTab(TermKind.PowerShell, title, s, () => s.Start("powershell.exe", _lastCols, _lastRows, dir),
                claudePaste: IsClaudeExe(path));
            if (tab == null) return null;
            opened = tab;
            tab.Restore = restore;
            tab.IconFile = CustomIconFile(conn.Icon); tab.KindKey = "kind.custom";   // 分頁列圖示＝這條自訂連線的圖示（1.1.2）
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
            if (tab == null) return null;
            opened = tab;
            tab.Restore = restore;
            tab.IconFile = CustomIconFile(conn.Icon);   // 分頁列圖示＝這條自訂連線的圖示（1.1.2；KindKey 預設即 kind.custom）
            if (dir != null) { tab.WorkDir = dir; SetTitlePath(dir); }
        }
        if (addHistory) AddHistory(new SavedTab
        {
            Type = "custom", Name = conn.Name, Title = conn.Name, Path = path, Args = conn.Args, Icon = conn.Icon,
            PickDir = conn.PickDir, ViaPowerShell = conn.ViaPowerShell,
            CloseKey = conn.CloseKey, CloseCount = conn.CloseCount
        });
        return opened;
    }

    private void Custom_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new CustomConnDialog { Owner = this };
        dlg.ShowDialog(); // New 下拉每次開都讀最新 AppSettings，故不需額外刷新
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
        Type = s.Type, Title = s.Title, Name = s.Name, Dir = s.Dir, Host = s.Host, Port = s.Port,
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
        "multiagent" => "multiagent|" + e.Dir,
        "chatroom" => "chatroom|" + e.Dir,
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
        "multiagent" => Loc.T("ma.title") + " — " + ShortDir(e.Dir),
        "chatroom" => Loc.T("chat.title") + " — " + ShortDir(e.Dir),
        _ => e.Title
    };

    private static string HistoryIcon(SavedTab e) => e.Type switch
    {
        "ps" => "powershell.png",
        "multiagent" => "multi-agent.png",
        "chatroom" => "chatroom.png",
        "claude" => "claude-code.png",
        "ssh" or "telnet" => "ssh-telnet.png",
        "com" => "com.png",
        "adb" => "adb.png",
        "custom" => CustomIconFile(e.Icon),
        _ => "new-connecting.png"
    };

    /// <summary>自訂連線的圖示 key → icon/ 檔名（New 下拉、紀錄、分頁列三處同一組）。</summary>
    private static string CustomIconFile(string? icon) => string.IsNullOrWhiteSpace(icon) ? "run.png" : icon + ".png";

    private static string ShortDir(string dir)
    {
        if (string.IsNullOrEmpty(dir)) return "";
        try { var n = Path.GetFileName(dir.TrimEnd('\\', '/')); return string.IsNullOrEmpty(n) ? dir : n; }
        catch { return dir; }
    }

    // 註：工具列「紀錄」按鈕 2026-09-16 改成「我的最愛」（MainWindow.Favorites.cs）。連線紀錄照記，Telegram /history 仍用它。

    private void ReopenHistory(SavedTab e)
    {
        if (DeferUntilWebReady(() => ReopenHistory(e), $"ReopenHistory {e.Type}")) return;
        string deskDir() => Directory.Exists(e.Dir) ? e.Dir : Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);
        switch (e.Type)
        {
            case "ps": OpenPowerShellDirect(deskDir(), NextName("PowerShell")); break;
            case "claude": { string d = deskDir(); OpenClaudeDirect(d, DirTabName(d, "Claude")); break; }
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
                    Name = string.IsNullOrWhiteSpace(e.Name) ? e.Title : e.Name, Path = e.Path, Args = e.Args, Icon = e.Icon,
                    PickDir = e.PickDir, ViaPowerShell = e.ViaPowerShell,
                    CloseKey = e.CloseKey, CloseCount = e.CloseCount
                }, string.IsNullOrWhiteSpace(e.Dir) ? null : e.Dir);   // 遠端開啟帶桌面目錄、不跳資料夾框
                break;
            case "multiagent":   // 1.2.0：用上次的資料夾直接開設定視窗（資料夾不在了＝先跳資料夾選擇）
                OpenMultiAgent(Directory.Exists(e.Dir) ? e.Dir : null);
                break;
            case "chatroom":     // 1.2.3：AI 聊天室，同上
                OpenChatRoom(Directory.Exists(e.Dir) ? e.Dir : null);
                break;
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
            background = s.Background,
            imeQuietMs = s.ImeQuietMs,   // claude 分頁靜止閘門門檻（見 AppSettings.ImeQuietMs / terminal.js）
            restoreLines = s.RestoreBufferLines,   // 關閉時每個分頁保留的 scrollback 行數（q…save；1.0.45）
            // 1.2.0 Multi-Agent pane 狀態標籤（E 協定 0～3）
            agentStates = new[] { Loc.T("ma.stateIdle"), Loc.T("ma.stateBusy"), Loc.T("ma.stateQueued"), Loc.T("ma.stateExited"), Loc.T("ma.stateBusyQueued") },
            // Ctrl+F 搜尋列的文字（index.html 預設中文；隨語言）
            search = new { placeholder = Loc.T("search.placeholder"), prev = Loc.T("search.prev"), next = Loc.T("search.next"), close = Loc.T("search.close") }
        });
        PostToWeb("T" + json);
    }

    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        // 分頁列綁「過濾後的檢視」（1.2.0）：Multi-Agent 分頁的每個 agent 都是獨立分頁，但整組只顯示一列（見 IsStripRow）
        _stripView = new System.Windows.Data.ListCollectionView(Tabs) { Filter = o => o is TerminalTab t && IsStripRow(t) };
        TabStrip.ItemsSource = _stripView;
        ApplyTabPanel();     // 右側分頁列表框：依記憶的顯示狀態與寬度
        ApplyWebDefaultBg(); // 避免 WebView2 內容未畫出前露出白底

        string userData = Path.Combine(AppPaths.DataDir, "WebView2");   // 測試模式（AWAYTERMINAL_DATA_DIR）與正式版分開
        try
        {
            long tLoaded = SinceProcessStartMs();
            Directory.CreateDirectory(userData);
            var env = await CoreWebView2Environment.CreateAsync(null, userData);
            await Web.EnsureCoreWebView2Async(env);
            // 啟動耗時（自行程啟動起算，1.2.6）：之後再有人說「開很慢」，看 diag.log 就知道慢在視窗、WebView2 還是前端（web ready 那行）
            Diag.Log($"startup: window loaded +{tLoaded}ms, webview2 ready +{SinceProcessStartMs()}ms");
        }
        catch (Exception ex)
        {
            // async void 裡丟出來＝直接跳 .NET 的當機框。最常見＝免安裝版在沒有 WebView2 Runtime 的電腦上
            // （安裝檔會順便裝，zip 不會）→ 說清楚缺什麼、給下載頁，然後結束（沒有 WebView2 這個程式什麼都做不了）。
            Diag.Log("WebView2 init failed: " + ex);
            MessageBox.Show(this, Loc.T("msg.webview2Fail") + "\n\n" + ex.Message, "AwayTerminal", MessageBoxButton.OK, MessageBoxImage.Error);
            Environment.Exit(1);
            return;
        }

        var core = Web.CoreWebView2;
        core.Settings.AreDevToolsEnabled = true;
        core.Settings.IsStatusBarEnabled = false;
        core.Settings.IsSwipeNavigationEnabled = false;
        // 1.2.6 啟動加速：關掉 SmartScreen 信譽檢查。只載入本機的 https://app/（虛擬主機），外部連結一律交給系統瀏覽器；
        // 開著的話每次啟動 Navigate 後要等它查完才 commit——實測（公司網路）index.html 19ms 就回應、卻卡到 2020ms 才開始解析，
        // 每次都剛好 2 秒（逾時）。關掉後 Navigate → web ready 由約 2.1 秒降到約 0.1 秒。舊版 Runtime 沒這個屬性 → 吞掉。
        try { core.Settings.IsReputationCheckingRequired = false; } catch { }

        _webRoot = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "web"));
        core.SetVirtualHostNameToFolderMapping("app", _webRoot, CoreWebView2HostResourceAccessKind.Allow);
        // 自行伺服 web 檔並加 no-cache，確保每次都載入最新前端（不吃 WebView2 快取）
        core.AddWebResourceRequestedFilter("https://app/*", CoreWebView2WebResourceContext.All);
        core.WebResourceRequested += OnWebResourceRequested;
        core.WebMessageReceived += OnWebMessage;
        // 安全網（1.1.6）：任何 window.open / target=_blank 一律用系統預設瀏覽器開、不開內嵌 WebView2 視窗
        core.NewWindowRequested += (_, ev) => { ev.Handled = true; OpenUrlExternal(ev.Uri); };
        core.ContextMenuRequested += OnWebContextMenu;   // 終端機右鍵：自訂選單取代 Edge 預設
        core.Navigate("https://app/index.html");

        _statusTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(600) };
        _statusTimer.Tick += UpdateStatuses;
        _statusTimer.Start();

        // 測試模式（AWAYTERMINAL_DATA_DIR）：遠端、檔案總管右鍵選單、實例間管線都是整台電腦只有一份的東西，
        // 開發版一律不碰，免得搶走／改寫使用者正在用的 AwayTerminal
        if (AppPaths.IsTestMode) { Diag.Log("test mode: data dir " + AppPaths.DataDir); return; }

        StartOrRestartRemote(); // 依設定啟動 Telegram 遠端（未設 token/chatId 則不啟動）

        // 檔案總管右鍵「用 AwayTerminal 開啟」（1.0.45）：依設定登錄/移除 HKCU 選單（路徑指向目前 exe、文字隨語言），
        // 並開 IPC 管線——之後從右鍵再啟動的 AwayTerminal.exe 會把資料夾轉交過來、由這個實例開分頁
        ShellIntegration.Apply(AppSettings.Current.ExplorerMenu, Loc.T("shell.menuText"));
        IpcPipe.StartServer(line => Dispatcher.InvokeAsync(() => HandleIpcLine(line)));
    }

    private static long SinceProcessStartMs()
    {
        try { return (long)(DateTime.Now - System.Diagnostics.Process.GetCurrentProcess().StartTime).TotalMilliseconds; }
        catch { return -1; }
    }

    // ---------- 檔案總管右鍵「用 AwayTerminal 開啟」（1.0.45）----------
    private void HandleIpcLine(string line)
    {
        int t = line.IndexOf('\t');
        string cmd = t < 0 ? line : line.Substring(0, t);
        string arg = t < 0 ? "" : line.Substring(t + 1);
        Diag.Log($"ipc: {cmd} {arg}");
        if (cmd == "open-dir") OpenDirFromShell(arg);
    }

    /// <summary>在指定資料夾開 PowerShell 分頁（分頁名＝資料夾名，同 ClaudeCode 的命名），並把視窗拉到前景。
    /// WebView2 未 ready 時排隊（DeferUntilWebReady）。</summary>
    private void OpenDirFromShell(string dir)
    {
        BringToFront();
        if (DeferUntilWebReady(() => OpenDirFromShell(dir), "OpenDirFromShell")) return;
        if (string.IsNullOrWhiteSpace(dir) || !Directory.Exists(dir))
        {
            MessageBox.Show(this, string.Format(Loc.T("shell.dirMissing"), dir), "AwayTerminal",
                MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }
        OpenPowerShellDirect(dir, DirTabName(dir, "PowerShell"));
    }

    /// <summary>把主視窗拉到前景（最小化先還原；Topmost 開關一下繞過前景鎖）。</summary>
    private void BringToFront()
    {
        try
        {
            if (WindowState == WindowState.Minimized) WindowState = WindowState.Normal;
            Activate();
            Topmost = true; Topmost = false;
        }
        catch { }
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
            Diag.Log($"web ready +{SinceProcessStartMs()}ms");
            PostTheme();
            // 只恢復上次存下的分頁；沒有就保持空白（不再硬開一個預設 PowerShell）
            var saved = AppSettings.Current.SavedTabs;
            if (saved is { Count: > 0 }) RestoreTabs(saved);
            // ready 前被排隊的使用者動作（見 DeferUntilWebReady）→ 現在補執行
            var pending = _pendingReadyAction;
            _pendingReadyAction = null;
            if (pending != null) { Diag.Log("running deferred action"); pending(); }
            // 命令列 --open-dir（檔案總管右鍵，啟動時沒有既有實例可轉交）→ 恢復分頁之後再開，讓它成為作用中分頁
            if (App.PendingOpenDir != null) { string d = App.PendingOpenDir; App.PendingOpenDir = null; OpenDirFromShell(d); }
            return;
        }

        char kind = msg[0];
        string rest = msg.Substring(1);
        switch (kind)
        {
            case 'D': // JS 端 IME 診斷（terminal.js IMEDBG）
                Diag.Log("js" + rest);
                return;
            case 'U': // 點終端機裡的連結（rest=URL）→ 1.1.10 起先跳選單「從瀏覽器開啟／複製網址」（1.1.6~1.1.9 是直接開瀏覽器）
                ShowUrlMenu(rest);
                return;
            case 'y': // 1.1.10：程式以 OSC 52 要求寫入剪貼簿（claude 全螢幕介面接管滑鼠、自己畫選取後用它複製）：id US text
            {
                int p = rest.IndexOf(US);
                if (p < 0) break;
                try { Clipboard.SetText(rest.Substring(p + 1)); }
                catch (Exception ex) { Diag.Log($"osc52 clipboard: {ex.Message}"); }
                return;
            }
            case 'm': // 1.1.10：接下來那個空的選取回覆是因為程式接管了滑鼠（xterm 選取停用）→ 提示按住 Shift 拖曳
                _selMouseHintId = rest;
                return;
            case 'i': // 輸入： id US text
            {
                int p = rest.IndexOf(US);
                if (p < 0) break;
                var tab = FindTab(rest.Substring(0, p));
                if (tab != null)
                {
                    string text = rest.Substring(p + 1);
                    tab.LastInputUtc = DateTime.UtcNow;  // 使用者實際按鍵／貼上（遠端推播用來排除打字回顯）
                    if (text.IndexOf('\r') >= 0 || text.IndexOf('\n') >= 0)
                        tab.LastSubmitUtc = DateTime.UtcNow;   // 含 Enter＝送出指令 → 完成後應推播
                    if (tab.Session == null && tab.LoginBuffer != null)
                        HandleLoginInput(tab, text); // SSH「login as:」中
                    else if (tab.Session == null && tab.Restore != null && tab.Kind is (TermKind.Ssh or TermKind.Telnet or TermKind.Com))
                    {
                        // 連線已結束的遠端分頁：按 Enter 在同一分頁重連（1.0.45，舊訊息留在 scrollback）；其他按鍵忽略
                        if (text.IndexOf('\r') >= 0 || text.IndexOf('\n') >= 0) ManualReconnect(tab);
                    }
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
                    if (tab.Agent == null) { _lastCols = c; _lastRows = r; }   // Multi-Agent 的 pane 只占一部分，別拿來當新分頁的初始尺寸
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
                if (qk == "save")   // 關閉程式：scrollback 序列化回覆（FinishExitAsync 等待中）
                {
                    if (int.TryParse(rest.Substring(0, p1), out int sid) && _saveBufTcs.TryGetValue(sid, out var stcs)) stcs.TrySetResult(text);
                    break;
                }
                if (qk == "text")   // 遠端查詢，不進剪貼簿
                {
                    if (int.TryParse(rest.Substring(0, p1), out int tid) && _remoteTextWaiters.Remove(tid, out var waiters))
                        foreach (var w in waiters) w.TrySetResult(text);
                    break;
                }
                if (qk == "file") { SaveBufferToFile(rest.Substring(0, p1), text); break; }             // 複製全部至檔案
                if (qk == "cwd") { UpdateTitlePath(rest.Substring(0, p1), text); UpdateDirTitle(rest.Substring(0, p1), text); break; } // 標題列目前路徑／SSH-Telnet 分頁名
                var target = qk == "all" ? (FrameworkElement)BtnCopyAll : BtnCopy;
                bool mouseOwned = _selMouseHintId == rest.Substring(0, p1);
                _selMouseHintId = null;
                if (string.IsNullOrEmpty(text))
                {
                    ShowCopyFeedback(target, Loc.T(mouseOwned ? "toast.noSelectionMouse" : "toast.noSelection"));
                    break;
                }
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
                    MarkActiveRow(tab);   // Multi-Agent 點了另一格（1.2.0）：同一列、記住最後點的那一格
                    if (tab.Agent != null) SetTitlePath(tab.WorkDir);   // 標題的 [Agent-12 Codex] 跟著換
                }
                break;
            }
            case 'G': // 1.2.0：Multi-Agent 上下分隔線拖完的新比例：下方 pane id US 上列比例
            {
                int p = rest.IndexOf(US);
                if (p < 0) break;
                if (FindTab(rest.Substring(0, p))?.Agent?.Group is { } g &&
                    double.TryParse(rest.Substring(p + 1), System.Globalization.NumberStyles.Float,
                                    System.Globalization.CultureInfo.InvariantCulture, out double ratio))
                    g.Ratio = AgentGroup.ClampRatio(ratio);
                break;
            }
            case 'k': // 拖曳後的新順序
                ReorderTabs(rest.Split(','));
                NormalizeAgentOrder();   // Multi-Agent 各格保持相鄰（JS 回報時已相鄰，保險）
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
        var tab = new TerminalTab(id, kind, title)
        { Cols = _lastCols, Rows = _lastRows, ClaudePaste = claudePaste || kind == TermKind.Claude };
        // 恢復分頁：填回原始開啟時間（1.1.4），讓 tooltip 顯示最初開啟時刻而非本次恢復時刻
        if (_restoreOpenedForNextTab is { } opened) { tab.StartUtc = opened; _restoreOpenedForNextTab = null; }
        Tabs.Add(tab);
        // 第三欄 flags：c=claude 分頁 → JS 端多行貼上改送 ESC+CR 軟換行（terminal.js doPaste）
        PostToWeb("n" + id + US + title + US + (tab.ClaudePaste ? "c" : ""));
        // 恢復分頁（1.0.45）：上次存的 scrollback 先倒回這個 xterm（b 協定；JS 端等 fit 到最終寬度才寫、期間的輸出先扣住），
        // 呼叫端接著才啟動連線 → 舊訊息在上、分隔行、新連線在下
        if (_restoreBufferForNextTab != null)
        {
            var (buf, sep) = _restoreBufferForNextTab.Value;
            _restoreBufferForNextTab = null;
            PostToWeb("b" + id + US + Convert.ToBase64String(Encoding.UTF8.GetBytes(buf))
                          + US + Convert.ToBase64String(Encoding.UTF8.GetBytes(sep)));
        }
        SelectTab(tab);
        return tab;
    }

    /// <summary>讀回上次關閉時存的 scrollback（SavedTab.BufferFile）；沒有或讀不到回 null。
    /// 回傳 (內容, 分隔行)：內容末尾補 SGR 重置；分隔行灰字帶存檔時間，JS 端寫在舊內容之後、推進 scrollback 之前。</summary>
    private static (string buf, string sep)? LoadRestoreBuffer(SavedTab st)
    {
        if (string.IsNullOrWhiteSpace(st.BufferFile)) return null;
        try
        {
            string path = Path.Combine(AppSettings.RestoreDir, st.BufferFile);
            if (!File.Exists(path)) return null;
            string text = File.ReadAllText(path, Encoding.UTF8);
            if (string.IsNullOrEmpty(text)) return null;
            string when = File.GetLastWriteTime(path).ToString("yyyy-MM-dd HH:mm");
            string sep = "\x1b[90m" + string.Format(Loc.T("term.restoredSep"), when) + "\x1b[0m";
            return (text + "\x1b[0m", sep);
        }
        catch (Exception ex) { Diag.Log($"load buffer {st.BufferFile}: {ex.Message}"); return null; }
    }

    /// <summary>執行檔名含 claude → 貼上需走 ESC+CR 軟換行（AddTab 的 claudePaste 旗標）。</summary>
    private static bool IsClaudeExe(string path)
    {
        try { return Path.GetFileNameWithoutExtension(path).Contains("claude", StringComparison.OrdinalIgnoreCase); }
        catch { return false; }
    }

    /// <summary>執行檔是 Codex CLI（codex.exe／npm 版 codex.cmd）。</summary>
    private static bool IsCodexExe(string path)
    {
        try { return string.Equals(Path.GetFileNameWithoutExtension(path), "codex", StringComparison.OrdinalIgnoreCase); }
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
        session.Exited += () => OnSessionExited(tab, session);
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
        var agentKeysDone = new HashSet<string>();
        foreach (var st in saved.ToList())
        {
            // 1.2.0 Multi-Agent：遇到一組的第一格就整組一起重開（各格的 scrollback 在 LaunchSlot 裡倒回），後面同組的略過
            if (!string.IsNullOrEmpty(st.AgentKey))
            {
                if (agentKeysDone.Add(st.AgentKey))
                {
                    try { RestoreAgentGroup(saved.Where(x => x.AgentKey == st.AgentKey).ToList()); }
                    catch (Exception ex) { Diag.Log("ma restore: " + ex.Message); }
                }
                continue;
            }
            _restoreBufferForNextTab = LoadRestoreBuffer(st);   // 有存 scrollback 就交給 AddTab 先倒回去（1.0.45）
            _restoreOpenedForNextTab = st.OpenedUtc == default ? null : st.OpenedUtc;   // 1.1.4：原始開啟時間（舊檔沒有＝用當下）
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
                    {
                        string cdir = Directory.Exists(st.Dir) ? st.Dir : Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);
                        OpenClaudeDirect(cdir, string.IsNullOrWhiteSpace(st.Title) ? DirTabName(cdir, "Claude") : st.Title);
                        break;
                    }
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
                    case "custom":
                    {
                        // 自訂連線（ClaudeCode / opencode / wsl…）：1.0.30 起也恢復。
                        // 工作目錄直接用上次的（不跳資料夾框卡住啟動）；目錄不在了且原本要選目錄 → 退回桌面。
                        string? dir = null;
                        if (!string.IsNullOrWhiteSpace(st.Dir) && Directory.Exists(st.Dir)) dir = st.Dir;
                        else if (st.PickDir) dir = Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);
                        OpenCustom(new CustomConn
                        {
                            Name = string.IsNullOrWhiteSpace(st.Name) ? st.Title : st.Name,
                            Path = st.Path, Args = st.Args, Icon = st.Icon,
                            PickDir = st.PickDir, ViaPowerShell = st.ViaPowerShell,
                            CloseKey = st.CloseKey, CloseCount = st.CloseCount
                        }, dir, st.Title);
                        break;
                    }
                    case "adb":
                    {
                        // ADB：直接用上次的 adb.exe 與序號重開 adb shell（不再先跑 adb devices 選裝置——
                        // 啟動時不能跳選單）。裝置不在時 adb 會自己報錯結束，交給「連線已結束→要關閉分頁嗎」處理。
                        string? adb = !string.IsNullOrWhiteSpace(st.Path) && File.Exists(st.Path) ? st.Path : AppSettings.ResolveAdbPath();
                        if (adb == null) break;
                        string title = string.IsNullOrWhiteSpace(st.Title) || Tabs.Any(t => t.Title == st.Title) ? NextName("ADB") : st.Title;
                        OpenAdbShell(adb, string.IsNullOrEmpty(st.AdbSerial) ? null : st.AdbSerial, title);
                        break;
                    }
                }
            }
            catch { /* 個別分頁恢復失敗就跳過 */ }
            finally { _restoreBufferForNextTab = null; _restoreOpenedForNextTab = null; }   // 這筆沒開成分頁（adb 找不到、使用者取消）→ 別留給下一筆
        }
    }

    private void SelectTab(TerminalTab tab)
    {
        _active = tab;
        SetTitlePath(tab.WorkDir);   // 先用啟動目錄墊底（claude/自訂沒提示行就顯示它）；有提示行的分頁 0.6s 內被解析值蓋掉
        MarkActiveRow(tab);          // Multi-Agent（1.2.0）：亮的是整組那一列
        PostToWeb("s" + tab.Id);
        Web.Focus();
    }

    private void RemoveTabSilently(TerminalTab tab)
    {
        // Multi-Agent（1.2.0）：先把這格從組裡摘掉（組的代表列可能換人），分頁移除後再重綁或拆組
        var agentGroup = tab.Agent?.Group;
        if (tab.Agent is { } slot) { slot.Tab = null; tab.Agent = null; }
        int idx = Tabs.IndexOf(tab);
        PostToWeb("x" + tab.Id);
        try { (tab.Macro as MacroRunner)?.Stop(); } catch { }
        try { (tab.Logger as SessionLogger)?.Dispose(); } catch { }
        tab.ReconnectTimer?.Stop();
        tab.ReconnectTimer = null;
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
        if (agentGroup != null) AfterAgentTabRemoved(agentGroup);
    }

    private void CloseTab(TerminalTab tab)
    {
        if (tab.Agent?.Group is { } g) { CloseAgentGroup(g, ask: true); return; }   // Multi-Agent：分頁列只有一列＝整組一起關
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

    private void OnSessionExited(TerminalTab tab, ITerminalSession session)
    {
        Dispatcher.InvokeAsync(() =>
        {
            // 過期事件防護：分頁已被使用者關閉、或已重連成新 session（SerialSession.Dispose
            // 會再發一次 Exited）→ 一律忽略，否則會把活的新 session 誤判成已結束。
            if (FindTab(tab.Id) == null || !ReferenceEquals(tab.Session, session)) return;

            // 舊 session 一定要 Dispose：自動重連只換新不釋放的話，Telnet 的 keepalive
            // Timer/TcpClient 與 ConPTY 的 HPCON/行程 handle 會隨每次斷線累積。
            tab.Session = null;
            _ = System.Threading.Tasks.Task.Run(() => { try { session.Dispose(); } catch { } });

            bool remoteKind = tab.Kind is (TermKind.Ssh or TermKind.Telnet or TermKind.Com);
            bool willAuto = tab.AutoReconnect && tab.Restore != null && remoteKind;
            // 沒勾自動重連的 SSH/Telnet/COM：提示按 Enter 在同一分頁重連（1.0.45；舊訊息留在 scrollback，見 ManualReconnect）
            string hint = (!willAuto && !_exiting && remoteKind && tab.Restore != null)
                ? " \x1b[33m" + Loc.T("term.exitedEnter") + "\x1b[0m" : "";
            PostToWeb("o" + tab.Id + US + Convert.ToBase64String(
                Encoding.UTF8.GetBytes("\r\n\x1b[90m" + Loc.T("term.exited") + "\x1b[0m" + hint + "\r\n")));

            // 斷線自動重連：勾了自動重連才排程
            if (willAuto)
            {
                ScheduleReconnect(tab);
                return;
            }

            // claude / adb / 自訂 exe 結束（斷線、崩潰、/exit）→ 分頁只剩一行灰字沒人看得到，
            // 改為跳出提示並詢問是否順手關掉（1.0.30，使用者要求）。程式關閉中不問。
            // Multi-Agent 的格不問（1.2.0）：分頁留著、pane 標籤顯示「已結束」，右鍵「Multi-Agent 設定…」可以重新啟動那一格
            if (tab.Agent is { } exitedSlot)
            {
                Diag.Log($"ma exited {exitedSlot.AgentId}");
                exitedSlot.Group.RowTab?.RaiseAgentState();
                return;
            }
            if (!_exiting && tab.Kind is (TermKind.Claude or TermKind.Custom or TermKind.Adb))
                AskCloseExitedTab(tab);
        });
    }

    /// <summary>session 結束後詢問是否關閉該分頁（Yes 才關；對話框期間分頁若已被關就略過）。</summary>
    private void AskCloseExitedTab(TerminalTab tab)
    {
        var r = MessageBox.Show(this, string.Format(Loc.T("msg.exitedCloseAsk"), tab.Title),
            Loc.T("msg.exitedTitle"), MessageBoxButton.YesNo, MessageBoxImage.Question);
        if (r != MessageBoxResult.Yes) return;
        if (FindTab(tab.Id) == null) return;   // 期間被遠端 /close 等關掉了
        RemoveTabSilently(tab);
    }

    /// <summary>排程自動重連：退避 3,6,9…最多 30 秒（一收到輸出就歸零）。</summary>
    private void ScheduleReconnect(TerminalTab tab)
    {
        tab.ReconnectAttempt++;
        int delay = Math.Min(30, 3 * tab.ReconnectAttempt);
        EchoToTab(tab.Id, "\r\n\x1b[33m" + string.Format(Loc.T("term.reconnect"), delay) + "\x1b[0m\r\n");
        // 一個分頁同時只能有一條重連鏈：舊的計時器先停掉（否則手動 Enter 一次就多一條鏈，各自倒數、各自印「n 秒後重連」）
        tab.ReconnectTimer?.Stop();
        var timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(delay) };
        tab.ReconnectTimer = timer;
        timer.Tick += (_, _) =>
        {
            timer.Stop();
            if (ReferenceEquals(tab.ReconnectTimer, timer)) tab.ReconnectTimer = null;
            var t = FindTab(tab.Id);
            if (t == null || t.Session != null) return;   // 已被關閉或已重連
            TryReconnect(t);
        };
        timer.Start();
    }

    /// <summary>連線已結束的 SSH/Telnet/COM 分頁按 Enter → 在同一分頁重連（1.0.45）：舊訊息留在 scrollback。
    /// SSH 尚未輸入過帳號（Host 沒有 user@）→ 回到「login as:」。自動重連等待中按 Enter＝不等退避、立刻連。</summary>
    private void ManualReconnect(TerminalTab tab)
    {
        var st = tab.Restore;
        if (st == null || tab.Session != null) return;
        tab.ReconnectAttempt = 0;
        tab.ReconnectTimer?.Stop();   // 自動重連的倒數作廢，改成現在立刻連（失敗再由 TryReconnect 重新排一條）
        tab.ReconnectTimer = null;
        if (tab.Kind == TermKind.Ssh && !st.Host.Contains('@'))
        {
            PostToWeb("b" + tab.Id + US + US);   // 舊畫面先推進 scrollback（同 TryReconnect）
            tab.PendingHost = st.Host;
            tab.PendingPort = st.Port;
            tab.LoginBuffer = new StringBuilder();
            EchoToTab(tab.Id, "login as: ");
            return;
        }
        TryReconnect(tab);
    }

    /// <summary>依 Restore 資訊重建 session（同一個分頁，不另開）。失敗：自動重連者再排下一次、手動者印錯誤再提示按 Enter。</summary>
    private void TryReconnect(TerminalTab tab)
    {
        var st = tab.Restore!;
        // 1.0.45：先把舊畫面整頁推進 scrollback、游標歸位（b 協定，見 terminal.js applyRestore）——
        // ConPTY 新 session 的第一幀一定送 ESC[2J 清可視區，不先推的話「最後一頁」舊訊息會憑空消失。
        PostToWeb("b" + tab.Id + US + US);
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
                    s.Exited += () => OnSessionExited(tab, s);
                    s.Start(SshCommand(st.Host, st.Port), tab.Cols, tab.Rows, null);
                    break;
                }
                case TermKind.Telnet:
                {
                    var s = new TelnetSession { KeepAliveMins = AppSettings.Current.KeepAliveMins };
                    tab.Session = s;
                    s.Output += data => OnSessionOutput(tab, data);
                    s.Exited += () => OnSessionExited(tab, s);
                    s.Start(st.Host, st.Port);
                    break;
                }
                case TermKind.Com:
                {
                    var s = new SerialSession();
                    tab.Session = s;
                    s.Output += data => OnSessionOutput(tab, data);
                    s.Exited += () => OnSessionExited(tab, s);
                    s.Start(st.ComPort, st.Baud, st.DataBits,
                        Enum.Parse<System.IO.Ports.Parity>(st.Parity),
                        Enum.Parse<System.IO.Ports.StopBits>(st.StopBits),
                        Enum.Parse<System.IO.Ports.Handshake>(st.Flow));
                    break;
                }
            }
            tab.Session?.Resize(tab.Cols, tab.Rows);
        }
        catch (Exception ex)
        {
            // 開失敗的 session 也要釋放（先取下再 Dispose，Serial 的 Exited 重發會被過期防護擋掉）
            var dead = tab.Session;
            tab.Session = null;
            if (dead != null) _ = System.Threading.Tasks.Task.Run(() => { try { dead.Dispose(); } catch { } });
            if (tab.AutoReconnect)
                ScheduleReconnect(tab);   // 連不上（埠不存在/主機不通）→ 退避後再試
            else
                EchoToTab(tab.Id, "\r\n\x1b[31m" + ex.Message + "\x1b[0m \x1b[33m" + Loc.T("term.exitedEnter") + "\x1b[0m\r\n");
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

    // ---------- 工作列 icon 右下狀態球（1.0.30：閒置不顯示；有分頁忙碌＝紅球上下跳動）----------
    // 原本綠=閒/橘=忙兩顆靜態圓點。改成「沒事就乾淨、有事才跳」：overlay 只能放 ImageSource，
    // 動畫＝預先畫好一輪彈跳影格，忙碌時用計時器輪播；閒置或沒分頁就把 overlay 清掉、計時器停掉。
    private const int BounceFrameCount = 12;
    private ImageSource[]? _bounceFrames;
    private int _bounceIdx;
    private DispatcherTimer? _bounceTimer;

    private ImageSource[] BounceFrames => _bounceFrames ??= Enumerable.Range(0, BounceFrameCount).Select(MakeBounceFrame).ToArray();

    /// <summary>第 k 格：紅球由底部彈到頂再落下（sin 曲線），落地附近略壓扁、頂點略拉長。
    /// 1.0.41：球放大到接近填滿徽章（半徑占比 4/16→13/40）＝視覺上約 1.7 倍大（工作列 overlay 徽章本身
    /// 由 Windows 固定成小尺寸、大點陣圖會被縮下去，故無法真的三倍，這是徽章內能放到的最大）。畫布 40×40
    /// 高解析度讓縮放後仍平滑。</summary>
    private static ImageSource MakeBounceFrame(int k)
    {
        const double size = 40, r = 16.0, top = 18.0, bottom = 23.0;
        double phase = Math.Sin(Math.PI * k / BounceFrameCount);   // 0→1→0：底 → 頂 → 底
        double cy = bottom - (bottom - top) * phase;
        double squash = 1 + 0.15 * Math.Max(0, 1 - phase * 5);   // 只在貼地那幾格壓扁（0.15 保證壓扁+描邊仍不出畫布）
        var dv = new DrawingVisual();
        using (var dc = dv.RenderOpen())
        {
            var fill = new SolidColorBrush(Color.FromRgb(0xF4, 0x43, 0x36));
            var pen = new Pen(new SolidColorBrush(Colors.White), 1.8);
            dc.DrawEllipse(fill, pen, new Point(size / 2, cy), r * squash, r / squash);
        }
        var rtb = new RenderTargetBitmap((int)size, (int)size, 96, 96, PixelFormats.Pbgra32);
        rtb.Render(dv);
        rtb.Freeze();
        return rtb;
    }

    /// <summary>忙碌 → 開始（或維持）紅球跳動；閒置 → 清掉 overlay、停計時器。</summary>
    private void SetTaskbarBusy(bool busy)
    {
        if (!busy)
        {
            _bounceTimer?.Stop();
            if (Taskbar.Overlay != null) Taskbar.Overlay = null;
            return;
        }
        if (_bounceTimer == null)
        {
            _bounceTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(60) };   // 12 格 × 60ms ≈ 0.7s 一跳
            _bounceTimer.Tick += (_, _) =>
            {
                var frames = BounceFrames;
                _bounceIdx = (_bounceIdx + 1) % frames.Length;
                Taskbar.Overlay = frames[_bounceIdx];
            };
        }
        if (!_bounceTimer.IsEnabled)
        {
            _bounceIdx = 0;
            Taskbar.Overlay = BounceFrames[0];
            _bounceTimer.Start();
        }
    }

    // ---------- 綠 / 橘狀態輪詢 ----------
    private void UpdateStatuses(object? sender, EventArgs e)
    {
        if (Tabs.Count == 0) { SetTaskbarBusy(false); SetTitlePath(""); return; } // 沒有分頁 → 不顯示狀態球、標題回程式名
        // 標題列目前路徑：向作用中分頁查提示字元行（回覆走 a…cwd）。
        // 1.1.2：shell 分頁（PowerShell/SSH/Telnet/WSL 等自訂 shell）名稱＝目前目錄名稱，所以已連線、未手動改名的
        // 分頁（含非作用中）也一起查，回覆進 UpdateDirTitle（claude 跑起來後沒有提示行 → 名稱停在啟動 claude 時的目錄）
        if (_webReady)
        {
            if (_active != null) PostToWeb("q" + _active.Id + US + "cwd");
            foreach (var t in Tabs)
                if (t != _active && TracksCwdTitle(t)) PostToWeb("q" + t.Id + US + "cwd");
        }
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
                else if (!busy && prev == TermStatus.Busy && _busySince.TryGetValue(t.Id, out var since))
                {
                    // 何時推播「完成」？區分「真的送出指令跑東西」與「只是在輸入框打字」。
                    // 舊法只看「最後按鍵距轉閒 <2.5s＝打字回顯」，但在 App 打字送出、AI 很快回答時，
                    // 轉閒也在 2.5s 內 → 被誤判成打字、不推（使用者實測「改在 App 發問沒丟給手機」）。
                    // 改用「這段忙碌期間有沒有送出過（按 Enter / 遠端 enter=true → LastSubmitUtc）」判斷：
                    //   有送出＝真工作 → 忙 ≥0.8s 就推（含 App 打字送出、遠端送出、快答）。
                    //   沒送出＝純打字 → 維持 2.5s 打字回顯抑制 + 忙 ≥3s 門檻（擋輸入框打字噪音）。
                    // 送出時間允許比忙碌起點早 2 秒（送出→開始輸出有延遲，尤其遠端），才不會漏判。
                    double busyDur = (now - since).TotalSeconds;
                    bool submittedThisBusy = t.LastSubmitUtc >= since.AddSeconds(-2);
                    bool echoFromTyping = !submittedThisBusy && (now - t.LastInputUtc).TotalSeconds < 2.5;
                    bool longEnough = submittedThisBusy ? busyDur >= 0.8 : busyDur >= 3;
                    if (!echoFromTyping && longEnough && RemoteVisible(t))   // 代理團隊只有 Agent-x1 會推播
                        _remote.OnTabIdle(t.Id, RemoteTitle(t));
                    _busySince.Remove(t.Id);
                }
            }
        }
        // 工作列 icon 右下：有分頁忙碌=紅球跳動、全部閒置=不顯示
        SetTaskbarBusy(Tabs.Any(t => t.Status == TermStatus.Busy));
        // Multi-Agent（1.2.0）：忙閒剛更新完 → 看哪一格閒下來、有信要送
        try { MultiAgentTick(); } catch (Exception ex) { Diag.Log("ma tick: " + ex.Message); }
    }

    // ---------- 工具列：連線群組 ----------
    /// <summary>資料夾選擇（PowerShell / ClaudeCode 等 PickDir 自訂連線共用）；取消回傳 null。</summary>
    private string? PickWorkDir(string title)
    {
        // ⚠️ 這裡絕不可等待 idle 類優先權（1.0.22 曾加 Dispatcher.Invoke(空, ContextIdle) 想等
        // 選單收合，1.0.23 期實際害死一次）：ContextIdle 沒有完成時間上限，使用者「怎麼沒反應？」
        // 時會晃滑鼠/連點，輸入訊息讓佇列一直不閒，Invoke 永不返回＝對話框永遠不開，
        // 而 UI 其他部分照常運作、完全看不出卡住。對話框有 owner、z 順序有保障，不需要那步。
        Diag.Log($"PickWorkDir enter '{title}'");
        Activate(); // 拉回前景，讓 owned 對話框跟著到最前面

        using var fbd = new System.Windows.Forms.FolderBrowserDialog
        {
            Description = title,
            UseDescriptionForTitle = true,
            ShowNewFolderButton = true
        };
        // LastDir 在休眠中的硬碟或斷線的網路磁碟上時，Directory.Exists 會把 UI 卡住好幾秒，
        // 對話框遲遲蹦不出來（使用者等到放棄＝「沒有出來」）。改丟背景執行緒查、最多等 300ms；
        // 逾時就不預選資料夾，讓對話框立刻開（代價只是這次從預設位置開始瀏覽）。
        var last = AppSettings.Current.LastDir;
        if (!string.IsNullOrWhiteSpace(last))
        {
            var swProbe = Stopwatch.StartNew();
            var probe = Task.Run(() =>
            { try { return Directory.Exists(last); } catch { return false; } });
            bool hit = probe.Wait(300) && probe.Result;
            Diag.Log($"PickWorkDir probe={(hit ? "ok" : "skip")} {swProbe.ElapsedMilliseconds}ms '{last}'");
            if (hit) fbd.SelectedPath = last;
        }

        // 必須指定擁有者，否則對話框可能開在主視窗後面＝「點了沒反應」（見 Win32Owner 註解）
        var swDlg = Stopwatch.StartNew();
        var result = fbd.ShowDialog(Win32Owner.Of(this));
        Diag.Log($"PickWorkDir ShowDialog={result} {swDlg.ElapsedMilliseconds}ms");
        if (result != System.Windows.Forms.DialogResult.OK) return null;
        AppSettings.Current.LastDir = fbd.SelectedPath;
        AppSettings.Current.Save();
        return fbd.SelectedPath;
    }

    private void OpenPowerShell_Click(object sender, RoutedEventArgs e)
    {
        if (DeferUntilWebReady(() => OpenPowerShell_Click(sender, e), "OpenPowerShell")) return;
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
            tab.IconFile = "claude-code.png"; tab.KindKey = "kind.claude";   // 經 PowerShell 跑的 claude 分頁列圖示仍是 claude（1.1.2）
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
        if (DeferUntilWebReady(() => OpenSsh_Click(sender, e), "OpenSsh")) return;
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

    /// <summary>
    /// 組 ssh.exe 指令：含保持連線（KeepAliveMins 分→ServerAliveInterval 秒；0=不加）。
    /// 1.1.1：帶 SendEnv 把 CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN 送到遠端——遠端跑 claude 時才不會走
    /// alternate screen 把對話從 scrollback 洗掉（本機是 App.OnStartup 設的，SSH 不會自動帶過去）。
    /// 遠端 sshd 要在 AcceptEnv 允許這個名字才收，沒允許就靜默忽略、無副作用；不收的機器請在遠端 rc 檔 export。
    /// </summary>
    private static string SshCommand(string host, int port)
    {
        int m = AppSettings.Current.KeepAliveMins;
        string ka = m > 0 ? $" -o ServerAliveInterval={m * 60} -o ServerAliveCountMax=3" : "";
        return $"ssh.exe -p {port}{ka} -o SendEnv=CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN {host}";
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

    /// <summary>主機帶 user@ → 直接啟動 ssh.exe（同開機恢復的直啟路徑），密碼照畫面提示輸入。遠端 /ssh 與我的最愛共用。</summary>
    private void OpenSshUserAtHost(string host, int port)
    {
        var s = new ConPtySession { GracefulExitBytes = new byte[] { 0x04, 0x04, 0x04 } };   // SSH：Ctrl+D ×3
        var t = StartTab(TermKind.Ssh, host, s, () => s.Start(SshCommand(host, port), _lastCols, _lastRows, null));
        if (t != null) t.Restore = new SavedTab { Type = "ssh", Host = host, Port = port };
        AddHistory(new SavedTab { Type = "ssh", Host = host, Port = port });
    }

    private void EchoToTab(int id, string text)
        => PostToWeb("o" + id + US + Convert.ToBase64String(Encoding.UTF8.GetBytes(text)));

    /// <summary>1.1.10：點終端機裡的連結 → 在滑鼠位置跳選單「從瀏覽器開啟」「複製網址」（使用者要求：點了不要馬上開瀏覽器）。
    /// 選單同終端機右鍵選單，是 WPF ContextMenu（獨立 popup 視窗，不受 WebView2 airspace 影響）。</summary>
    private void ShowUrlMenu(string url)
    {
        if (string.IsNullOrWhiteSpace(url)) return;
        var menu = new ContextMenu();
        var open = new MenuItem { Header = Loc.T("ctx.openUrl") };
        open.Click += (_, _) => OpenUrlExternal(url);
        var copy = new MenuItem { Header = Loc.T("ctx.copyUrl") };
        copy.Click += (_, _) =>
        {
            try { Clipboard.SetText(url); } catch { }
            ShowCopyFeedback(Web, Loc.T("toast.urlCopied"), atMouse: true);
        };
        menu.Items.Add(open);
        menu.Items.Add(copy);
        menu.PlacementTarget = Web;
        menu.Placement = System.Windows.Controls.Primitives.PlacementMode.MousePoint;
        menu.IsOpen = true;
    }

    /// <summary>1.1.6：終端機裡的連結（xterm web-links 套件偵測）用系統預設瀏覽器開。
    /// 只放行 http/https（擋掉 file:、javascript: 等，避免點到終端機輸出的怪字串觸發本機動作）。</summary>
    private void OpenUrlExternal(string url)
    {
        if (string.IsNullOrWhiteSpace(url)) return;
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri)) return;
        if (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps) return;
        try { System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(uri.AbsoluteUri) { UseShellExecute = true }); }
        catch (Exception ex) { Diag.Log($"open url: {ex.Message}"); }
    }

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
                s.Exited += () => OnSessionExited(tab, s);
                try { s.Start(SshCommand(target, tab.PendingPort), tab.Cols, tab.Rows, null); }
                catch (Exception ex)
                {
                    // 開失敗（ssh.exe 不在 PATH 等）的 session 不能留在分頁上：Start 丟例外時讀取迴圈沒起來、Exited 永遠不會發，
                    // 之後所有打字都被 Session?.WriteText 無聲吞掉、Enter 也到不了 ManualReconnect＝分頁變磚。
                    // 同 TryReconnect 的收尾：取下、背景 Dispose，然後回到「login as:」讓使用者重試（主機／埠都還在）。
                    tab.Session = null;
                    _ = System.Threading.Tasks.Task.Run(() => { try { s.Dispose(); } catch { } });
                    MessageBox.Show(this, Loc.T("msg.connectFail") + "\n" + ex.Message, "AwayTerminal",
                        MessageBoxButton.OK, MessageBoxImage.Error);
                    if (FindTab(tab.Id) != null && tab.Session == null)
                    {
                        tab.LoginBuffer = new StringBuilder();
                        EchoToTab(tab.Id, "\r\nlogin as: ");
                    }
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
        if (DeferUntilWebReady(() => OpenCom_Click(sender, e), "OpenCom")) return;
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
    /// <summary>工具列「輸入文字」（1.0.30 為分頁列最左「…」，1.0.46 移到工具列並併入常用字串）：
    /// 開輸入框先把文字打好（IME 在一般 TextBox 裡組字、不經 xterm/ConPTY），按「送出」才整段貼進作用中分頁——
    /// 繞過 claude 逐鍵解析造成的重複／亂碼。視窗上方可選常用字串「插入」文字框或「直接送出」（視窗留著可連送，
    /// 依該字串的 SendEnter）。都走 SendSnippet（claude 分頁換行→ESC+CR 軟換行；送 Enter 則 200ms 後補 CR）。</summary>
    private void Compose_Click(object sender, RoutedEventArgs e)
    {
        if (_active == null)
        {
            ShowCopyFeedback((FrameworkElement)sender, Loc.T("compose.noTab"));   // 不無聲返回
            return;
        }
        var dlg = new ComposeDialog(AppSettings.Current.ComposeSendEnter, AppSettings.Current.Prompts,
                                    (content, enter) => SendSnippet(content, enter)) { Owner = this };
        if (dlg.ShowDialog() != true) { Web.Focus(); return; }
        AppSettings.Current.ComposeSendEnter = dlg.SendEnter;
        AppSettings.Current.Save();
        SendSnippet(dlg.TextToSend, dlg.SendEnter);
        Web.Focus();
    }

    // ---------- 右側分頁列表框（1.1.0）----------
    private const double TabPanelMinWidth = 120;

    /// <summary>右上 ▼/▲：顯示／隱藏分頁列表框（狀態記憶於 AppSettings.TabPanelVisible）。</summary>
    private void TabPanelToggle_Click(object sender, RoutedEventArgs e)
    {
        AppSettings.Current.TabPanelVisible = !AppSettings.Current.TabPanelVisible;
        AppSettings.Current.Save();
        ApplyTabPanel();
    }

    /// <summary>依設定套用列表框顯示／寬度：隱藏＝欄寬 0、拖曳區也收起；顯示＝欄寬回設定值（不小於最小寬）。</summary>
    private void ApplyTabPanel()
    {
        var s = AppSettings.Current;
        bool show = s.TabPanelVisible;
        TabPanel.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
        TabSplitter.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
        TabPanelCol.MinWidth = show ? TabPanelMinWidth : 0;
        TabPanelCol.Width = show ? new GridLength(Math.Max(TabPanelMinWidth, s.TabPanelWidth)) : new GridLength(0);
        TabPanelToggle.Content = show ? "▲" : "▼";
    }

    /// <summary>拖完列表框左緣 → 記住新寬度。</summary>
    private void TabSplitter_DragCompleted(object sender, System.Windows.Controls.Primitives.DragCompletedEventArgs e)
    {
        AppSettings.Current.TabPanelWidth = Math.Round(TabPanelCol.ActualWidth);
        AppSettings.Current.Save();
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
        new(@"^[\w.-]+:(?<p>[^\s]+)\s+[^\s]+[$#%]"),                // macOS bash 預設：host:資料夾名 user$（\h:\W \u$，只有 basename）
        new(@"^[^@\s]+@\S+\s+(?<p>\S+)\s+[%$#]"),                   // macOS zsh 預設：user@host 資料夾名 %（空格分隔、cwd 可為純 basename，非 /~ 開頭；1.1.7）
    };

    private string _titlePath = "";

    /// <summary>從提示字元行解析目前路徑（CwdRes 八組 regex）；解析不到回 null。</summary>
    private static string? ParseCwd(string promptLine)
    {
        foreach (var re in CwdRes)
        {
            var m = re.Match(promptLine);
            if (m.Success) return m.Groups["p"].Value.Trim();
        }
        return null;
    }

    /// <summary>依提示字元行更新標題（解析不到就保留上次的，避免打字過程閃動）。</summary>
    private void UpdateTitlePath(string idStr, string promptLine)
    {
        if (_active == null || _active.Id.ToString() != idStr) return;   // 已切換分頁 → 丟棄過期回覆
        var path = ParseCwd(promptLine);
        if (path != null) SetTitlePath(path);
    }

    /// <summary>哪些分頁依提示行的目前目錄自動命名：PowerShell / SSH / Telnet / 自訂 shell（WSL、docker bash…；
    /// 提示行解析不到的自訂程式如 python REPL 自然不會動），已連線、未手動改名。Claude 直跑與 ADB 不算。</summary>
    private static bool TracksCwdTitle(TerminalTab t)
        => t.Session != null && !t.TitleLocked && t.Agent == null   // Multi-Agent 的格名稱由組決定（1.2.0）
           && t.Kind is TermKind.PowerShell or TermKind.Ssh or TermKind.Telnet or TermKind.Custom;

    /// <summary>1.1.2：shell 分頁名稱＝目前目錄名稱（例 ~/workspace1/AwayPhotoRawEditor_Swift → AwayPhotoRawEditor_Swift）。
    /// 名稱保留全文（過長只在分頁列顯示層截），tooltip 第二行是完整路徑；使用者手動改過名（TitleLocked）就不再動。
    /// 在 shell 裡啟動 claude 後沒有提示行 → 名稱停在啟動 claude 當下的目錄；claude 離開、提示行回來再跟著更新。
    /// 同一資料夾開第二個分頁時 DirTabName 補的「(2)」保留（同目錄不重新命名）。登入前（login as: / 密碼提示）
    /// 解析不到提示行，SSH 分頁維持主機名。</summary>
    private void UpdateDirTitle(string idStr, string promptLine)
    {
        var tab = FindTab(idStr);
        if (tab == null || !TracksCwdTitle(tab)) return;
        var path = ParseCwd(promptLine);
        if (path == null || path == tab.CwdPath) return;
        tab.CwdPath = path;
        string name = DirNameOf(path);
        if (string.IsNullOrEmpty(name) || tab.Title == name) return;
        if (tab.Title.StartsWith(name, StringComparison.Ordinal) && tab.Title.Length > name.Length
            && tab.Title[name.Length] == '(' && tab.Title.EndsWith(')')) return;   // 「名稱(2)」＝同目錄的第二個分頁，保留
        tab.Title = name;
        PostToWeb("t" + tab.Id + US + tab.Title);   // 同步分割模式 pane 標題
    }

    /// <summary>路徑最後一段：`~/a/b/` → b、`/` → /、`~` → ~、`C:\Users\me` → me、`C:\` → C:。</summary>
    private static string DirNameOf(string path)
    {
        string t = path.TrimEnd('/', '\\');
        if (t.Length == 0) return path;   // 只有根「/」
        int i = t.LastIndexOfAny(new[] { '/', '\\' });
        string last = i < 0 ? t : t.Substring(i + 1);
        return last.Length == 0 ? path : last;   // `C:\` 去尾後剩 `C:`→取整段
    }

    private string _titleTag = "";

    private void SetTitlePath(string path)
    {
        // 1.1.4：標題格式改「AwayTerminal - [連線標籤] 路徑」（標籤＝作用中分頁的 TitleTag）
        string tag = _active?.TitleTag ?? "";
        if (_titlePath == path && _titleTag == tag) return;
        _titlePath = path;
        _titleTag = tag;
        Title = ComposeTitle();
    }

    /// <summary>視窗標題字串：「AwayTerminal」／「AwayTerminal - [標籤] 路徑」（沒路徑就只有程式名；語言切換沿用）。</summary>
    private string ComposeTitle()
    {
        string app = Loc.T("app.name") + (AppPaths.IsTestMode ? " [TEST]" : "");   // 測試模式一眼分得出不是正式那一個
        if (string.IsNullOrEmpty(_titlePath)) return app;
        string prefix = string.IsNullOrEmpty(_titleTag) ? "" : $"[{_titleTag}] ";
        return $"{app} - {prefix}{_titlePath}";
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

    /// <summary>送出常用字串；sendEnter=true 時內容貼上後補送 Enter 直接執行。
    /// Enter 不能併進貼上內容（claude 分頁會把內容裡的換行轉成 ESC+CR 軟換行、bracketed paste
    /// 下換行也只是插入，都不會送出）→ 等貼上經 JS 往返寫進 session 後，再直接對 session 送 CR。</summary>
    private void SendSnippet(string content, bool sendEnter)
    {
        var tab = _active;
        if (tab == null || string.IsNullOrEmpty(content)) return;
        PasteToTab(tab.Id, content);
        if (!sendEnter) return;
        var timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(200) };
        timer.Tick += (_, _) =>
        {
            timer.Stop();
            tab.Session?.WriteText("\r"); // 固定送到當初的分頁，期間切分頁也不會送錯
        };
        timer.Start();
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

        // 先確認再清：誤按的代價在 Telnet / COM 特別高——那條路走 term.clear()，
        // 整個 scrollback 會被洗掉、救不回來（PowerShell / SSH 只是 shell 重畫，捲得回去）。
        var tab = _active;
        if (MessageBox.Show(this, string.Format(Loc.T("msg.clearConfirm"), tab.Title),
                Loc.T("msg.clearTitle"), MessageBoxButton.YesNo, MessageBoxImage.Question)
            != MessageBoxResult.Yes)
        {
            Web.Focus();
            return;
        }
        // 對話框期間可能被切分頁 → 一律清當初按下時的那一頁
        var session = tab.Session;
        if (tab.Kind == TermKind.PowerShell || tab.Kind == TermKind.Ssh)
        {
            // 先送 Esc 清掉還沒送出的輸入，隔一小段再送 Ctrl+L 清畫面。
            // 若兩者黏在一起送，PSReadLine 會當成 escape 序列而兩者都失效。
            session?.Write(new byte[] { 0x1B });
            await System.Threading.Tasks.Task.Delay(60);
            session?.Write(new byte[] { 0x0C });
        }
        else
        {
            PostToWeb("c" + tab.Id); // Telnet / COM：直接清 xterm 緩衝
        }
        Web.Focus();
    }

    /// <summary>「翻頁」按鈕：下拉選單捲動作用中分頁的畫面（走 S 協定交給 xterm，不動輸入流）。</summary>
    private void Page_Click(object sender, RoutedEventArgs e)
    {
        var menu = new ContextMenu();
        menu.Items.Add(MakeScrollItem("page.up", "up"));
        menu.Items.Add(MakeScrollItem("page.down", "down"));
        menu.Items.Add(new Separator());
        menu.Items.Add(MakeScrollItem("page.top", "top"));
        menu.Items.Add(MakeScrollItem("page.bottom", "bottom"));

        menu.PlacementTarget = (UIElement)sender;
        menu.Placement = System.Windows.Controls.Primitives.PlacementMode.Bottom;
        menu.IsOpen = true;
    }

    private MenuItem MakeScrollItem(string locKey, string action)
    {
        var mi = new MenuItem
        {
            Header = new TextBlock { Text = Loc.T(locKey), FontSize = 11 }, // 字級同 New 下拉
        };
        mi.Click += (_, _) => ScrollActive(action);
        return mi;
    }

    /// <summary>捲動作用中分頁的檢視。捲完把焦點還給終端機，讓使用者能直接繼續打字。</summary>
    private void ScrollActive(string action)
    {
        if (_active == null) return;
        PostToWeb("S" + _active.Id + US + action);
        Web.Focus();
    }

    // ---------- 工具列：設定群組（P3 / P4 補齊）----------
    private void Prompt_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new PromptDialog { Owner = this };
        if (dlg.ShowDialog() == true && dlg.ContentToSend != null)
        {
            SendSnippet(dlg.ContentToSend, dlg.SendEnterToSend);
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
        if (AppPaths.IsTestMode) return;   // 測試模式不啟動遠端（同一個 bot 只能有一個程式在 poll）
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
        try
        {
            // 用分頁自己的 Decoder 保留跨 chunk 狀態：中文字被 64KB 讀取邊界切開時，
            // 一次性 GetString 會把前後半各解成 �（畫面沒事——xterm 有 streaming decoder）
            var chars = new char[tab.RemoteDecoder.GetCharCount(data, 0, data.Length, false)];
            tab.RemoteDecoder.GetChars(data, 0, data.Length, chars, 0, false);
            s = new string(chars);
        }
        catch { return; }
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
            if (host.Contains('@')) OpenSshUserAtHost(host, port);
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
            // 代理團隊／AI 聊天室不列（1.2.0／1.2.3）：重開要跳設定視窗（modal），從手機觸發會把輪詢執行緒卡在 Dispatcher.Invoke 直到有人在電腦前按掉
            _remoteHistList = AppSettings.Current.History.Where(h => h.Type is not ("multiagent" or "chatroom")).Take(10).ToList();
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
            if (tab == null || !RemoteVisible(tab)) return false;
            // 手機端已下指令（TelegramRemote 先出過確認按鈕）→ 不再跳 PC 端確認框，直接走優雅結束流程。
            // 代理團隊的 Agent-x1＝整組一起關（分頁列上本來就是一組一列、✕ 也是整組）
            if (tab.Agent?.Group is { } g) CloseAgentGroup(g, ask: false);
            else RemoveTabSilently(tab);
            return true;
        });

    // 代理團隊的遠端（使用者要求，2026-09-15；1.2.0 起原本整組不支援遠端）：只和 Agent-x1（格 1、下方全寬、使用者對話的那一格）溝通——
    // 手機的分頁清單一組只列 Agent-x1 一項（標題帶組名與 Agent ID），打字／按鍵／完成推播都只對它；其他格不列、送不進去。
    // 巨集仍不支援；/history 也不列代理團隊（重開要跳資料夾與設定視窗，手機做不到）。
    private static bool RemoteVisible(TerminalTab t) => t.Agent == null || t.Agent.Index == 1;

    /// <summary>手機上看到的分頁名稱：代理團隊＝「組名（代理團隊 Agent-11）」。</summary>
    private static string RemoteTitle(TerminalTab t) =>
        t.Agent is { } a ? $"{a.Group.Title}（{Loc.T(a.Group.IsChat ? "chat.title" : "ma.title")} {a.AgentId}）" : t.Title;

    IReadOnlyList<RemoteTabInfo> IRemoteHost.SnapshotTabs()
        => Dispatcher.Invoke(() => (IReadOnlyList<RemoteTabInfo>)Tabs.Where(RemoteVisible).Select(
               t => new RemoteTabInfo(t.Id, RemoteTitle(t), t.Status == TermStatus.Busy,
                                      t.Agent != null ? RemoteTabInfo.MultiAgentKind : t.Kind.ToString())).ToList());

    DateTime IRemoteHost.GetTabActivityUtc(int tabId)
        => Dispatcher.Invoke(() =>
        {
            var tab = FindTab(tabId);
            if (tab == null) return DateTime.MinValue;
            return tab.LastOutputUtc > tab.LastInputUtc ? tab.LastOutputUtc : tab.LastInputUtc;
        });

    bool IRemoteHost.SendInputToTab(int tabId, string text, bool enter)
        => Dispatcher.Invoke(() =>
        {
            var tab = FindTab(tabId);
            if (tab == null || !RemoteVisible(tab)) return false;   // 代理團隊只能送給 Agent-x1
            if (enter) tab.LastSubmitUtc = DateTime.UtcNow;   // 遠端送出的指令也算「送出」，完成後照推（代理團隊的投遞也會等它 3 秒，免得信和手機訊息打在一起）
            if (tab.Session == null && tab.LoginBuffer != null)
            { HandleLoginInput(tab, enter ? text + "\r" : text); return true; }   // SSH「login as:」：帳號從遠端回覆也能登入
            if (tab.Session == null) return false;
            // 1.1.10：文字與 Enter 分兩次送（同「輸入文字」視窗的 SendSnippet）。1.1.9 以前「文字＋CR」一次寫入：
            // claude 會把整塊當成貼上、CR 變成輸入框裡的換行而不是送出——手機傳來的訊息停在輸入框、看起來「沒有動作」。
            // 實測（scratchpad probe，claude 2.1.269＋OpenConsole，同一段 95 字訊息 A/B）：一次寫入＝沒送出、文字＋300ms 後單獨 CR＝送出；83 字一次寫入也沒送出。
            // 短訊息（53 字）一次寫入則有送出，所以是「有時候」。claude 分頁的文字走 JS doPaste（多行轉 ESC+CR 軟換行、
            // 先 ESC[I 吸收懸置狀態、等 claude 靜止）；Enter 一律延後對「當初那個 session」直接送，期間切分頁也不會送錯。
            // 1.2.0：與 Multi-Agent 投遞共用 SendTextThenEnter（MainWindow.MultiAgent.cs）
            SendTextThenEnter(tab, text, enter);
            return true;
        });

    /// <summary>遠端送文字後隔多久才送 Enter（ms）。要比 JS doPaste 最壞的延遲（靜止閘門上限 150＋ESC[I 間隔 25）長，
    /// 並超過 claude 的貼上判定窗；SendSnippet 用 200 已實測可行，遠端取 300 多留餘裕（手機端感覺不到）。</summary>
    private const int RemoteEnterDelayMs = 300;

    bool IRemoteHost.SendKeyToTab(int tabId, string keyName)
        => Dispatcher.Invoke(() =>
        {
            var tab = FindTab(tabId);
            if (tab?.Session == null || !RemoteVisible(tab)) return false;   // 代理團隊只能送給 Agent-x1
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
            if (!_remoteTextWaiters.TryGetValue(tabId, out var list)) _remoteTextWaiters[tabId] = list = new();
            list.Add(tcs);
            PostToWeb("q" + tabId + US + "text");
        });
        try
        {
            bool got = tcs.Task.Wait(1500);
            if (!got)   // 逾時：把自己從等待者移除，免得殘留
                Dispatcher.Invoke(() =>
                {
                    if (_remoteTextWaiters.TryGetValue(tabId, out var list) && list.Remove(tcs) && list.Count == 0)
                        _remoteTextWaiters.Remove(tabId);
                });
            if (got && tcs.Task.Result.Length > 0)
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
        // 語言切換由 Loc.Changed 自動套用工具列；字型/背景改動於此重新套到終端機；檔案總管右鍵選單依勾選登錄/移除（文字隨語言）
        if (dlg.ShowDialog() == true)
        {
            PostTheme(); ApplyWebDefaultBg();
            if (!AppPaths.IsTestMode) ShellIntegration.Apply(AppSettings.Current.ExplorerMenu, Loc.T("shell.menuText"));   // 測試模式不改 HKCU
        }
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

    // ---------- ADB（由指向 adb 的自訂連線觸發；1.0.18 起不再是內建選單項目）----------
    /// <summary>ADB 開啟流程：adb devices → 0 台提示 / 1 台直接開 / 2 台以上選裝置。</summary>
    private void OpenAdbFlow(string? adbPath = null)
    {
        if (DeferUntilWebReady(() => OpenAdbFlow(adbPath), "OpenAdbFlow")) return;

        // 自訂連線指定的路徑優先；沒有就搜尋這台電腦（PATH / Android SDK 位置）
        string? adb = !string.IsNullOrWhiteSpace(adbPath) && File.Exists(adbPath)
            ? adbPath : AppSettings.ResolveAdbPath();
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
        var tab = StartTab(TermKind.Adb, title, s, () => s.Start($"\"{adb}\" {args}", _lastCols, _lastRows, null));
        // 1.0.30 起 ADB 也做開機恢復：記住 adb.exe 與序號，RestoreTabs 直接重開（不再跑 adb devices）
        if (tab != null) tab.Restore = new SavedTab { Type = "adb", Title = title, AdbSerial = serial ?? "", Path = adb };
        AddHistory(new SavedTab { Type = "adb", Title = title, AdbSerial = serial ?? "", Path = adb });
    }

    private void About_Click(object sender, RoutedEventArgs e)
    {
        var ver = System.Reflection.Assembly.GetExecutingAssembly().GetName().Version;
        string v = ver == null ? "0.0.1" : $"{ver.Major}.{ver.Minor}.{ver.Build}";
        var gray = new System.Windows.Media.SolidColorBrush(
            (System.Windows.Media.Color)System.Windows.Media.ColorConverter.ConvertFromString("#E0E0E0"));

        // 編譯時間＝本 exe 的檔案寫入時間（build 當下寫檔；複製/安裝都會保留原時間戳）
        string built;
        try
        {
            built = File.GetLastWriteTime(Environment.ProcessPath ?? AppContext.BaseDirectory)
                .ToString("yyyy/M/d HH:mm:ss", System.Globalization.CultureInfo.InvariantCulture);
        }
        catch { built = "-"; }

        // 可點連結（開預設瀏覽器）
        TextBlock MakeLink(string url)
        {
            var link = new System.Windows.Documents.Hyperlink(new System.Windows.Documents.Run(url))
            { Foreground = new System.Windows.Media.SolidColorBrush(
                (System.Windows.Media.Color)System.Windows.Media.ColorConverter.ConvertFromString("#4FC1FF")) };
            link.Click += (_, _) =>
            {
                try
                {
                    System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(url)
                    { UseShellExecute = true });
                }
                catch { }
            };
            var tb = new TextBlock { Margin = new Thickness(0, 2, 0, 0) };
            tb.Inlines.Add(link);
            return tb;
        }

        // 版式與 AwayPhotoRawEditor 的「關於」一致：
        // 標題 / 版本 / 編譯時間 / 作者 / 下載 / Source Code / 授權 / 第三方元件
        //（無「二次開發說明」——本專案為 MIT、無 LGPL 元件，沒有替換重連結義務要告知）
        var panel = new StackPanel { Margin = new Thickness(24, 18, 24, 14) };   // 內容一律置左
        panel.Children.Add(new TextBlock
        {
            Text = "AwayTerminal", FontSize = 18, FontWeight = FontWeights.Bold,
            Foreground = System.Windows.Media.Brushes.White
        });
        panel.Children.Add(new TextBlock
        { Text = $"{Loc.T("about.version")}: v{v}", Foreground = gray, Margin = new Thickness(0, 8, 0, 0) });
        panel.Children.Add(new TextBlock
        { Text = $"{Loc.T("about.buildTime")}: {built}", Foreground = gray, Margin = new Thickness(0, 4, 0, 0) });

        // 作者：標籤與名字同一行；名字以圖片顯示（執行期渲染，畫面上沒有可複製/可被爬的 email 文字）
        var authorRow = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 4, 0, 0) };
        authorRow.Children.Add(new TextBlock
        { Text = Loc.T("about.author") + ": ", Foreground = gray, VerticalAlignment = VerticalAlignment.Center });
        authorRow.Children.Add(new Image
        {
            Source = RenderTextImage("Awaysu (awaysu" + "@" + "gmail.com)"),
            Stretch = System.Windows.Media.Stretch.None,
            VerticalAlignment = VerticalAlignment.Center
        });
        panel.Children.Add(authorRow);

        panel.Children.Add(new TextBlock
        { Text = Loc.T("about.download") + ":", Foreground = gray, Margin = new Thickness(0, 14, 0, 0) });
        panel.Children.Add(MakeLink("https://www.awaysu.cc/software/awayterminal"));

        panel.Children.Add(new TextBlock
        { Text = "Source Code:", Foreground = gray, Margin = new Thickness(0, 14, 0, 0) });
        panel.Children.Add(MakeLink("https://github.com/awaysu/AwayTerminal"));

        panel.Children.Add(new TextBlock
        { Text = $"{Loc.T("about.license")}: MIT　© 2026 Chih-Wei Su (Awaysu)", Foreground = gray, Margin = new Thickness(0, 14, 0, 0) });

        panel.Children.Add(new TextBlock
        { Text = Loc.T("about.thirdParty") + ":", Foreground = gray, Margin = new Thickness(0, 14, 0, 0) });
        panel.Children.Add(new TextBlock
        { Text = "xterm.js 6.0.0 (MIT)", Foreground = gray, Margin = new Thickness(0, 2, 0, 0) });   // web/vendor/xterm.js＝@xterm/xterm@6.0.0
        panel.Children.Add(new TextBlock
        { Text = ".NET Runtime 9 (MIT)／Microsoft Edge WebView2", Foreground = gray, Margin = new Thickness(0, 2, 0, 0) });

        // 底部按鈕列：狀態文字（就地顯示檢查結果）＋「檢查更新」＋ OK
        var status = new TextBlock
        { Foreground = gray, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(0, 0, 10, 0) };
        var check = new Button
        { Content = Loc.T("update.check"), MinWidth = 96, Height = 26, Padding = new Thickness(8, 0, 8, 0), Margin = new Thickness(0, 0, 8, 0) };
        var ok = new Button
        { Content = "OK", Width = 76, Height = 26, IsDefault = true, IsCancel = true };
        var btnRow = new StackPanel
        {
            Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right,
            Margin = new Thickness(0, 18, 0, 0)
        };
        btnRow.Children.Add(status);
        btnRow.Children.Add(check);
        btnRow.Children.Add(ok);
        panel.Children.Add(btnRow);

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
        // 等 API 回來的這幾秒使用者可能已經把「關於」關掉 → 那時要改掛主視窗，
        // 否則 Owner 指向已關閉的視窗會丟例外（async void 沒人接＝直接掛掉）。
        bool aboutOpen = true;
        win.Closed += (_, _) => aboutOpen = false;
        // 檢查更新：呼叫網站 API（10 秒逾時），沒新版就在按鈕旁顯示一行結果，有新版才跳視窗
        check.Click += async (_, _) =>
        {
            check.IsEnabled = false;
            status.Text = Loc.T("update.checking");
            var info = await UpdateChecker.CheckAsync(v);
            check.IsEnabled = true;
            if (info == null) { status.Text = Loc.T("update.failed"); return; }
            if (!info.UpdateAvailable) { status.Text = $"{Loc.T("update.latest")} (v{info.LatestVersion})"; return; }
            status.Text = "";
            ShowUpdateDialog(aboutOpen ? win : this, v, info);
        };
        win.ShowDialog();
    }

    /// <summary>有新版時跳出：目前/最新版本、更新內容（可捲動），按鈕開軟體頁讓使用者自己選安裝版或免安裝版。</summary>
    private static void ShowUpdateDialog(Window owner, string current, UpdateInfo info)
    {
        var gray = new System.Windows.Media.SolidColorBrush(
            (System.Windows.Media.Color)System.Windows.Media.ColorConverter.ConvertFromString("#E0E0E0"));
        var panel = new StackPanel { Margin = new Thickness(24, 18, 24, 14), MaxWidth = 520 };
        panel.Children.Add(new TextBlock
        {
            Text = Loc.T("update.found"), FontSize = 16, FontWeight = FontWeights.Bold,
            Foreground = System.Windows.Media.Brushes.White
        });
        panel.Children.Add(new TextBlock
        { Text = $"{Loc.T("update.current")}: v{current}", Foreground = gray, Margin = new Thickness(0, 10, 0, 0) });
        panel.Children.Add(new TextBlock
        { Text = $"{Loc.T("update.latestVer")}: v{info.LatestVersion}", Foreground = gray, Margin = new Thickness(0, 4, 0, 0) });

        if (!string.IsNullOrWhiteSpace(info.ReleaseNotes))
        {
            panel.Children.Add(new TextBlock
            { Text = Loc.T("update.notes") + ":", Foreground = gray, Margin = new Thickness(0, 14, 0, 0) });
            panel.Children.Add(new ScrollViewer
            {
                MaxHeight = 260, Margin = new Thickness(0, 4, 0, 0),
                VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
                HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
                Content = new TextBlock
                {
                    Text = info.ReleaseNotes.Trim(), Foreground = gray,
                    TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 0, 8, 0)
                }
            });
        }

        var go = new Button
        { Content = Loc.T("update.goDownload"), MinWidth = 110, Height = 26, Padding = new Thickness(8, 0, 8, 0), Margin = new Thickness(0, 0, 8, 0), IsDefault = true };
        var close = new Button
        { Content = Loc.T("update.close"), Width = 76, Height = 26, IsCancel = true };
        var row = new StackPanel
        {
            Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right,
            Margin = new Thickness(0, 18, 0, 0)
        };
        row.Children.Add(go);
        row.Children.Add(close);
        panel.Children.Add(row);

        var win = new Window
        {
            Title = Loc.T("update.title"),
            SizeToContent = SizeToContent.WidthAndHeight,
            WindowStartupLocation = WindowStartupLocation.CenterOwner,
            ResizeMode = ResizeMode.NoResize,
            ShowInTaskbar = false,
            Background = new System.Windows.Media.SolidColorBrush(
                (System.Windows.Media.Color)System.Windows.Media.ColorConverter.ConvertFromString("#2D2D30")),
            Content = panel
        };
        try { win.Owner = owner; }
        catch { win.WindowStartupLocation = WindowStartupLocation.CenterScreen; }
        go.Click += (_, _) =>
        {
            try
            {
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(info.PageUrl)
                { UseShellExecute = true });
            }
            catch { }
            win.Close();
        };
        close.Click += (_, _) => win.Close();
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
        if (_tabDragging) { _tabDragging = false; return; }   // 剛結束拖曳 → 這次放開不當選取
        var tab = TabOf(sender);
        if (tab != null) SelectTab(FocusTargetOf(tab));   // Multi-Agent：回到最後點的那一格
    }

    // ---------- 右側分頁列拖曳排序（1.1.8）----------
    private System.Windows.Point _tabDragStart;
    private TerminalTab? _tabDragItem;
    private bool _tabDragging;   // 這次按下有無真的拖曳（避免拖完那下放開被當成選取）

    private void Tab_DragDown(object sender, MouseButtonEventArgs e)
    {
        _tabDragging = false;   // 每次重新按下先歸零，拖曳後的下一次點選不會被吃掉
        _tabDragStart = e.GetPosition(null);
        _tabDragItem = TabOf(sender);   // 記住候選；超過門檻才真的起拖（否則就是一般點選）
    }

    private void Tab_DragMove(object sender, MouseEventArgs e)
    {
        if (e.LeftButton != MouseButtonState.Pressed || _tabDragItem == null) return;
        var pos = e.GetPosition(null);
        if (Math.Abs(pos.X - _tabDragStart.X) < SystemParameters.MinimumHorizontalDragDistance &&
            Math.Abs(pos.Y - _tabDragStart.Y) < SystemParameters.MinimumVerticalDragDistance) return;
        var item = _tabDragItem;
        _tabDragItem = null;
        _tabDragging = true;
        try { DragDrop.DoDragDrop((DependencyObject)sender, new DataObject("AwayTab", item), DragDropEffects.Move); }
        catch { }
    }

    private void Tab_DragOver(object sender, DragEventArgs e)
    {
        bool ok = e.Data.GetDataPresent("AwayTab");
        e.Effects = ok ? DragDropEffects.Move : DragDropEffects.None;
        e.Handled = true;
        if (ok && sender is Border b && TabOf(b) != (e.Data.GetData("AwayTab") as TerminalTab)) b.Opacity = 0.55;
    }

    private void Tab_DragLeave(object sender, DragEventArgs e)
    {
        if (sender is Border b) b.Opacity = 1.0;
    }

    private void Tab_DragDrop(object sender, DragEventArgs e)
    {
        if (sender is Border b) b.Opacity = 1.0;
        if (e.Data.GetData("AwayTab") is not TerminalTab dragged) return;
        var target = TabOf(sender);
        if (target == null || target == dragged) return;
        int from = Tabs.IndexOf(dragged), to = Tabs.IndexOf(target);
        if (from < 0 || to < 0 || from == to) return;
        // Multi-Agent（1.2.0）：分頁列一列＝好幾個分頁。往下拖到一組上時落在那組最後一格之後，免得插進組中間
        if (from < to && target.Agent?.Group is { } tg && tg.Running.LastOrDefault()?.Tab is { } lastTab) to = Tabs.IndexOf(lastTab);
        Tabs.Move(from, to);
        NormalizeAgentOrder();   // 拖的若是一組（代表列），其他格跟上
        // 分割/分欄模式的 pane 順序同步（K 協定），並存新順序（下次開機恢復照此序）
        if (_webReady) PostToWeb("K" + string.Join(",", Tabs.Select(t => t.Id)));
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
            if (tab.Agent?.Group is { } ag) ag.Title = tab.Title;   // Multi-Agent：改的是組名（代表列換人時沿用）
            tab.TitleLocked = true;   // 手動改名後不再依目前目錄自動改名（1.1.2）
            PostToWeb("t" + tab.Id + US + tab.Title); // 同步分割模式 pane 標題
        }
    }

    private void MenuLog_Click(object sender, RoutedEventArgs e)
    {
        var tab = MenuTargetOf(sender);   // Multi-Agent：最後點的那一格
        if (tab != null) LogAction(tab);
    }

    private void MenuMacro_Click(object sender, RoutedEventArgs e)
    {
        var tab = TabOf(sender);
        if (tab != null && tab.Agent == null) MacroAction(tab);   // Multi-Agent 不支援巨集（1.2.0；選單項目也藏起來了）
    }

    /// <summary>分頁右鍵「配色」：套用該分頁的文字/背景色。Tag="fg|bg"；空 Tag = 回到設定預設顏色。</summary>
    private void MenuColor_Click(object sender, RoutedEventArgs e)
    {
        var tab = MenuTargetOf(sender);   // Multi-Agent：最後點的那一格
        if (tab == null) return;
        string tag = (sender as MenuItem)?.Tag as string ?? "";
        string fg = "", bg = "";
        var parts = tag.Split('|');
        if (parts.Length == 2) { fg = parts[0]; bg = parts[1]; }
        // P{id}US{fg}US{bg}；fg/bg 皆空 → 前端清除覆寫、回到設定預設
        PostToWeb("P" + tab.Id + US + fg + US + bg);
    }

    // ---------- 記錄 log ----------（1.1.2 起分頁列不再放 log／巨集圖示，只從右鍵選單進入）
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
