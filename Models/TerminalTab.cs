using System.ComponentModel;
using System.Windows;
using System.Windows.Media;
using AwayTerminal.Services;
using AwayTerminal.Sessions;

namespace AwayTerminal.Models;

public enum TermStatus { Ready, Busy }

public enum TermKind { PowerShell, Ssh, Telnet, Com, Adb, Claude, Custom }

/// <summary>一個分頁的資料模型（供分頁列 UI 綁定）。</summary>
public sealed class TerminalTab : INotifyPropertyChanged
{
    public int Id { get; }
    public TermKind Kind { get; }
    public ITerminalSession? Session { get; set; }

    /// <summary>最後一次收到輸出的時間（遠端連線判斷忙碌用）。</summary>
    public DateTime LastOutputUtc { get; set; }

    /// <summary>分頁建立時間（tooltip 顯示已啟動時長用）。</summary>
    public DateTime StartUtc { get; } = DateTime.UtcNow;

    /// <summary>收到終端機鈴聲後、等待使用者輸入中（Claude 完成）→ 強制綠燈。</summary>
    public bool WaitingForUser { get; set; }

    /// <summary>最後回報的終端機尺寸（session 延後啟動時使用）。</summary>
    public int Cols { get; set; } = 80;
    public int Rows { get; set; } = 24;

    /// <summary>待送出的自動指令（如 Claude Code）：等前端回報實際尺寸後才送，避免以 80 欄啟動。</summary>
    public string? PendingCommand { get; set; }

    /// <summary>輸出合批緩衝：爆量小塊聚成一次 write，避免 TUI 畫面撕裂（以 lock 保護）。</summary>
    public System.IO.MemoryStream OutBuf { get; } = new();
    public bool FlushScheduled { get; set; }

    /// <summary>遠端 /last 用：ANSI 去除後的近期可讀文字（有上限，超過從前面截；以 RemoteLock 保護）。</summary>
    public readonly object RemoteLock = new();
    public System.Text.StringBuilder RemoteRecent { get; } = new();

    /// <summary>SSH「login as:」狀態：非 null = 等待使用者輸入帳號後才啟動 ssh。</summary>
    public System.Text.StringBuilder? LoginBuffer { get; set; }
    public string? PendingHost { get; set; }
    public int PendingPort { get; set; }

    /// <summary>重建此分頁所需的連線資訊（關閉時存檔、下次開啟恢復；斷線自動重連也用它）。</summary>
    public SavedTab? Restore { get; set; }

    /// <summary>SSH/Telnet/COM：session 結束時自動重連（開啟時從設定帶入）。</summary>
    public bool AutoReconnect { get; set; }
    /// <summary>連續重連次數（退避延遲用；一收到輸出就歸零）。</summary>
    public int ReconnectAttempt { get; set; }

    /// <summary>此分頁的記錄器（null = 未記錄）。</summary>
    public object? Logger { get; set; }

    /// <summary>此分頁的巨集執行器（null = 未執行）。</summary>
    public object? Macro { get; set; }

    public TerminalTab(int id, TermKind kind, string title)
    {
        Id = id;
        Kind = kind;
        _title = title;
    }

    private string _title;
    public string Title
    {
        get => _title;
        set { if (_title != value) { _title = value; Raise(nameof(Title)); Raise(nameof(ToolTipText)); } }
    }

    // tooltip：標題 + 已啟動時間（時:分），例：PowerShell-1 00:23
    public string ToolTipText
    {
        get
        {
            int mins = (int)(DateTime.UtcNow - StartUtc).TotalMinutes;
            if (mins < 0) mins = 0;
            return $"{_title} {mins / 60:D2}:{mins % 60:D2}";
        }
    }

    /// <summary>供狀態輪詢定期呼叫，更新 tooltip 的已啟動時間。</summary>
    public void RefreshRuntime() => Raise(nameof(ToolTipText));

    // 狀態方塊：Ready=綠(可輸入)、Busy=橘(跑程式)
    private TermStatus _status = TermStatus.Ready;
    public TermStatus Status
    {
        get => _status;
        set { if (_status != value) { _status = value; Raise(nameof(Status)); Raise(nameof(StatusBrush)); } }
    }
    public Brush StatusBrush => _status == TermStatus.Busy ? BusyBrush : ReadyBrush;

    // 記錄 log icon：灰=停、藍=記錄中
    private bool _isLogging;
    public bool IsLogging
    {
        get => _isLogging;
        set { if (_isLogging != value) { _isLogging = value; Raise(nameof(IsLogging)); Raise(nameof(LogBrush)); Raise(nameof(LogOpacity)); } }
    }
    public Brush LogBrush => _isLogging ? LogOnBrush : OffBrush;
    // log icon 圖片：停=淡、記錄中=亮
    public double LogOpacity => _isLogging ? 1.0 : 0.35;

    // 巨集 icon：灰=停、紫=執行中
    private bool _isMacroRunning;
    public bool IsMacroRunning
    {
        get => _isMacroRunning;
        set { if (_isMacroRunning != value) { _isMacroRunning = value; Raise(nameof(IsMacroRunning)); Raise(nameof(MacroBrush)); Raise(nameof(MacroOpacity)); } }
    }
    public Brush MacroBrush => _isMacroRunning ? MacroOnBrush : OffBrush;
    // 巨集 icon 圖片：停=淡、執行中=亮
    public double MacroOpacity => _isMacroRunning ? 1.0 : 0.35;

    private bool _isActive;
    public bool IsActive
    {
        get => _isActive;
        set { if (_isActive != value) { _isActive = value; Raise(nameof(IsActive)); Raise(nameof(TabBackground)); Raise(nameof(TabBorder)); Raise(nameof(TabBorderThickness)); Raise(nameof(TabMargin)); Raise(nameof(TitleBrush)); } }
    }
    public Brush TabBackground => _isActive ? ActiveBg : InactiveBg;
    public Brush TabBorder => _isActive ? ActiveBorder : InactiveBg;
    // 作用中：左/右/下細黃線、上方開口（分頁列在終端機下方，開口朝上相連）；非作用中：上方也不畫線（避免壓在外框黃線下形成暗線）
    public Thickness TabBorderThickness => _isActive ? new Thickness(1, 0, 1, 1) : new Thickness(1, 0, 1, 1);
    // 作用中分頁往上抬 1px，蓋掉終端機外框底邊的黃線 → 形成融合缺口
    public Thickness TabMargin => _isActive ? new Thickness(0, -1, 4, 0) : new Thickness(0, 0, 4, 0);

    // 選中的標題亮、未選的標題淡灰
    public Brush TitleBrush => _isActive ? ActiveTitleBrush : InactiveTitleBrush;

    private static readonly Brush ReadyBrush = Frozen("#4CAF50");
    private static readonly Brush BusyBrush = Frozen("#FF9800");
    private static readonly Brush OffBrush = Frozen("#777777");
    private static readonly Brush LogOnBrush = Frozen("#2196F3");
    private static readonly Brush MacroOnBrush = Frozen("#9C27B0");
    // 選中分頁背景 = 終端機背景(#1E1E1E)，視覺上與下方 terminal 連成一體；藍框標示作用中
    private static readonly Brush ActiveBg = Frozen("#1E1E1E");
    private static readonly Brush InactiveBg = Frozen("#333337");
    private static readonly Brush ActiveBorder = Frozen("#FDFFB0");
    private static readonly Brush ActiveTitleBrush = Frozen("#EDEDED");
    private static readonly Brush InactiveTitleBrush = Frozen("#888888");

    private static Brush Frozen(string hex)
    {
        var b = new SolidColorBrush((Color)ColorConverter.ConvertFromString(hex));
        b.Freeze();
        return b;
    }

    public event PropertyChangedEventHandler? PropertyChanged;
    private void Raise(string name) => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
