using System.ComponentModel;
using System.Windows;
using System.Windows.Media;
using AwayTerminal.Localization;
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

    /// <summary>最後一次「使用者從鍵盤／貼上」送入的時間（只記 JS 的 i 協定，不含程式自動送的指令）。
    /// 用途：打字時每個按鍵的回顯都會更新 LastOutputUtc，分頁會一路被判定為忙，停手後才翻閒——
    /// 若不分辨就會把「打字」誤當成「程式跑完」而推播到手機。見 MainWindow.UpdateStatuses。</summary>
    public DateTime LastInputUtc { get; set; }

    /// <summary>最後一次「送出指令」（按 Enter／遠端 enter=true）的時間。用來把「真的送出跑東西」與
    /// 「只是在輸入框打字」分開：在 App 打字送出、AI 很快回答時，光靠 LastInputUtc 會被誤判成打字回顯而
    /// 不推播（使用者實測「改在 App 發問沒丟給手機」）；只要這段忙碌期間有送出過，就視為真工作、照推。</summary>
    public DateTime LastSubmitUtc { get; set; }

    /// <summary>分頁開啟時間（tooltip 顯示「開啟 HH:mm」用）。可設：恢復分頁時填回原始開啟時間，
    /// 讓重開程式後 tooltip 仍顯示這個分頁「最初開啟」的時刻，而非本次恢復的時刻（1.1.4 使用者要求）。</summary>
    public DateTime StartUtc { get; set; } = DateTime.UtcNow;

    /// <summary>收到終端機鈴聲後、等待使用者輸入中（Claude 完成）→ 強制綠燈。</summary>
    public bool WaitingForUser { get; set; }

    /// <summary>claude 型分頁（Kind=Claude 或執行檔名含 claude）：貼上走 ESC+CR；
    /// 關閉程式時「更新 CLAUDE.md」也以此判斷——自訂連線開的 ClaudeCode 沒有 Restore，
    /// 不能再用 Restore.Type == "claude" 找。</summary>
    public bool ClaudePaste { get; set; }

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

    /// <summary>遠端緩衝的 UTF-8 解碼器：保留跨 chunk 狀態（中文字被讀取邊界切開時不會變 �）。
    /// 只在 session 的輸出執行緒使用（同一時間僅一條）。</summary>
    public System.Text.Decoder RemoteDecoder { get; } = System.Text.Encoding.UTF8.GetDecoder();

    /// <summary>SSH「login as:」狀態：非 null = 等待使用者輸入帳號後才啟動 ssh。</summary>
    public System.Text.StringBuilder? LoginBuffer { get; set; }
    public string? PendingHost { get; set; }
    public int PendingPort { get; set; }

    /// <summary>重建此分頁所需的連線資訊（關閉時存檔、下次開啟恢復；斷線自動重連也用它）。</summary>
    public SavedTab? Restore { get; set; }

    /// <summary>啟動時的工作目錄（視窗標題後援：claude/自訂等沒有提示行的分頁顯示這個）。</summary>
    public string WorkDir { get; set; } = "";

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
        // 分頁列圖示與種類名稱依 Kind 給預設（1.1.2）；自訂連線由 OpenCustom 再覆寫成該連線的圖示／名稱
        (_iconFile, KindKey) = kind switch
        {
            TermKind.PowerShell => ("powershell.png", "kind.powershell"),
            TermKind.Ssh => ("ssh-telnet.png", "kind.ssh"),
            TermKind.Telnet => ("ssh-telnet.png", "kind.telnet"),
            TermKind.Com => ("com.png", "kind.com"),
            TermKind.Adb => ("adb.png", "kind.adb"),
            TermKind.Claude => ("claude-code.png", "kind.claude"),
            _ => ("run.png", "kind.custom"),
        };
    }

    private string _title;
    /// <summary>分頁標題＝完整名稱、不截字（1.1.2 使用者要求：過長只在分頁列顯示層以 CharacterEllipsis 截、
    /// 改名視窗與 tooltip 都看得到全名）。</summary>
    public string Title
    {
        get => _title;
        set { if (_title != value) { _title = value; Raise(nameof(Title)); Raise(nameof(ToolTipText)); } }
    }

    /// <summary>由提示字元行解析到的目前路徑（PowerShell/SSH/Telnet/WSL 等 shell 分頁；tooltip 第二行）。</summary>
    public string CwdPath { get; set; } = "";

    /// <summary>使用者手動「更改名稱」後鎖住：不再依目前目錄自動改名。</summary>
    public bool TitleLocked { get; set; }

    /// <summary>分頁列圖示檔名（icon/ 內的 png；＝New 下拉同一組圖示），依狀態染成綠／紅（StatusIcon）。</summary>
    private string _iconFile;
    public string IconFile
    {
        get => _iconFile;
        set { if (_iconFile != value) { _iconFile = value; Raise(nameof(StatusIcon)); } }
    }

    /// <summary>種類名稱的 Loc key（kind.powershell / kind.ssh / …），圖示 tooltip 用。</summary>
    public string KindKey { get; set; }

    /// <summary>種類補充（主機、COM 埠、自訂連線名稱、工作目錄…）：直接從 Restore（各開啟點都會填）取，
    /// 不必每個開啟點各自設一次；沒有 Restore 時退回啟動目錄。</summary>
    private string KindDetail => Restore switch
    {
        null => WorkDir,
        { Type: "ssh" } r => r.Host,
        { Type: "telnet" } r => $"{r.Host}:{r.Port}",
        { Type: "com" } r => $"{r.ComPort} {r.Baud}",
        { Type: "adb" } r => r.AdbSerial,
        { Type: "custom" } r => string.IsNullOrEmpty(r.Dir) ? r.Name : $"{r.Name}  {r.Dir}",
        var r => r.Dir,   // ps / claude
    };

    /// <summary>圖示 tooltip：「是哪一種連線」＝種類名稱＋補充，例「SSH  user@host」「PowerShell  C:\path」。</summary>
    public string KindTip
    {
        get
        {
            string d = KindDetail;
            return string.IsNullOrEmpty(d) ? Loc.T(KindKey) : $"{Loc.T(KindKey)}  {d}";
        }
    }

    // tooltip：完整名稱 + 開啟時刻（時鐘 HH:mm，本機時區），例：AwayPhotoRawEditor_Swift  開啟 14:12；
    // 第二行＝目前路徑（shell 分頁）；記錄 log／巨集執行中也在這裡註明（1.1.2 起分頁列不再放 log／巨集圖示）。
    // 1.1.4 起改顯示「實際開啟的時間點」而非經過時長（使用者要求）——StartUtc 恢復分頁時會填回原始開啟時間。
    public string ToolTipText
    {
        get
        {
            var sb = new System.Text.StringBuilder($"{_title}  {Loc.T("tip.tabOpened")} {StartUtc.ToLocalTime():HH:mm}");
            if (!string.IsNullOrEmpty(CwdPath) && CwdPath != _title) sb.Append('\n').Append(CwdPath);
            if (_isLogging) sb.Append('\n').Append(Loc.T("tip.tabLogging"));
            if (_isMacroRunning) sb.Append('\n').Append(Loc.T("tip.tabMacroRunning"));
            return sb.ToString();
        }
    }

    /// <summary>視窗標題中括號內顯示的連線標籤（例「ClaudeCode」「PowerShell」「SSH」）：
    /// 自訂連線用連線名稱、其餘用種類名稱。見 MainWindow.SetTitlePath →「AwayTerminal - [標籤] 路徑」。</summary>
    public string TitleTag => Restore is { Type: "custom", Name: var nm } && !string.IsNullOrWhiteSpace(nm)
        ? nm : Loc.T(KindKey);

    /// <summary>供狀態輪詢定期呼叫：更新 tooltip 的開啟時刻文字（語言切換時 tip.tabOpened/KindTip 也靠這裡）。</summary>
    public void RefreshRuntime() { Raise(nameof(ToolTipText)); Raise(nameof(KindTip)); }

    // 狀態：Ready=綠(可輸入)、Busy=紅(跑程式；1.0.42 由橘改紅，與工作列彈跳球同色)。
    // 1.1.2 起不再是圓點，而是把該分頁的連線圖示染色（StatusIcon）。
    private TermStatus _status = TermStatus.Ready;
    public TermStatus Status
    {
        get => _status;
        set { if (_status != value) { _status = value; Raise(nameof(Status)); Raise(nameof(StatusIcon)); } }
    }
    public ImageSource StatusIcon => IconTint.Get(_iconFile, _status == TermStatus.Busy ? BusyColor : ReadyColor);

    // 記錄 log 中（分頁 tooltip 註明；右鍵選單開始/停止）
    private bool _isLogging;
    public bool IsLogging
    {
        get => _isLogging;
        set { if (_isLogging != value) { _isLogging = value; Raise(nameof(IsLogging)); Raise(nameof(ToolTipText)); } }
    }

    // 巨集執行中（分頁 tooltip 註明；右鍵選單執行/停止）
    private bool _isMacroRunning;
    public bool IsMacroRunning
    {
        get => _isMacroRunning;
        set { if (_isMacroRunning != value) { _isMacroRunning = value; Raise(nameof(IsMacroRunning)); Raise(nameof(ToolTipText)); } }
    }

    private bool _isActive;
    public bool IsActive
    {
        get => _isActive;
        set { if (_isActive != value) { _isActive = value; Raise(nameof(IsActive)); Raise(nameof(TabBackground)); Raise(nameof(TabBorder)); Raise(nameof(TitleBrush)); } }
    }
    // 1.1.0：分頁改為右側直列列表框，作用中＝黃框＋終端機同色底、非作用中＝灰底無框（邊框粗細/位移由 XAML 固定，
    // 1.0.x 底部分頁列「上方開口與外框融合」的 TabBorderThickness/TabMargin 已移除）
    public Brush TabBackground => _isActive ? ActiveBg : InactiveBg;
    public Brush TabBorder => _isActive ? ActiveBorder : InactiveBg;

    // 選中的標題亮、未選的標題淡灰
    public Brush TitleBrush => _isActive ? ActiveTitleBrush : InactiveTitleBrush;

    // 1.1.2：分頁列圖示染色改淡綠／淡紅（使用者要求；Material 200 級，原 #4CAF50／#F44336 太濃）。工作列彈跳球仍是 #F44336。
    private static readonly Color ReadyColor = (Color)ColorConverter.ConvertFromString("#A5D6A7");
    private static readonly Color BusyColor = (Color)ColorConverter.ConvertFromString("#EF9A9A");
    // 選中分頁背景 = 終端機背景(#1E1E1E)；黃框標示作用中
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
