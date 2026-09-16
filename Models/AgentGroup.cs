using AwayTerminal.Services.MultiAgent;

namespace AwayTerminal.Models;

/// <summary>一組的用途：代理團隊（信箱分工）或 AI 聊天室（輪流討論）。</summary>
public enum GroupMode { Team, Chat }

/// <summary>AI 聊天室的進行階段。</summary>
public enum ChatPhase
{
    /// <summary>等使用者給主題。</summary>
    NeedTopic,
    /// <summary>輪流發言中。</summary>
    Discussing,
    /// <summary>已請主持人寫結論。</summary>
    Concluding,
    /// <summary>結論寫完了。</summary>
    Done,
}

/// <summary>
/// 一個 Multi-Agent 分頁（1.2.0）：2～4 個 agent（各自是一個獨立的 <see cref="TerminalTab"/>，session／xterm／log／恢復畫面都照一般分頁）
/// 綁成一組。右側分頁列只顯示 <see cref="RowTab"/> 那一列；終端機區「下一上 N−1」（下＝最小格號、通常是 Agent-x1）。
/// 刻意沿用 1.1.11 協作分頁的做法：不另做「一個分頁多個 session」的新模型，綁組只是薄薄一層。
/// agent 之間用專案裡的 <c>.ai/bus/</c> 資料夾互寄信（<see cref="MessageBus"/>），AwayTerminal 看到新信就在收件人的終端機打一行「請讀 …」。
/// 使用者決定：不支援巨集與 Telegram 遠端。
/// </summary>
public sealed class AgentGroup
{
    public AgentGroup(string key, int number, string dir)
    {
        Key = key;
        Number = number;
        Dir = dir;
        for (int i = 0; i < Slots.Length; i++) Slots[i] = new AgentSlot(this, i + 1);
    }

    /// <summary>穩定代號（GUID，存進 SavedTab.AgentKey；恢復分頁時靠它把各格重新綁回來）。</summary>
    public string Key { get; }

    /// <summary>組號 1～9：Agent ID ＝ Agent-{組號}{格號}。開組時取「目前開著的組」裡最小的空號；恢復時優先沿用上次的號碼。</summary>
    public int Number { get; set; }

    /// <summary>專案資料夾（四個 agent 共用；開組時選一次）。</summary>
    public string Dir { get; }

    /// <summary>組名＝分頁列那一列的標題（預設資料夾名；右鍵「更改名稱」改這個）。</summary>
    public string Title { get; set; } = "";

    /// <summary>格 1～4（索引 0～3）。未啟用／尚未啟動的格 Tab＝null。</summary>
    public AgentSlot[] Slots { get; } = new AgentSlot[4];

    /// <summary>上列（格 2～4）占的高度比例（中間分隔線拖曳；0.15～0.85）。只有一格時沒有上列。</summary>
    public double Ratio { get; set; } = 0.5;

    /// <summary>暫停投遞（使用者右鍵「投遞 → 暫停」、或訊息數到上限）。暫停中照收信、照排隊，只是不打字。</summary>
    public bool Paused { get; set; }

    /// <summary>這次暫停是訊息數到上限造成的（設定視窗把上限調高時自動解除；使用者自己按的暫停不解除）。</summary>
    public bool PausedByLimit { get; set; }

    /// <summary>本輪已投遞幾則（到 <see cref="MaxMessages"/> 就暫停；右鍵「投遞」選一個次數＝繼續並歸零）。</summary>
    public int MessageCount { get; set; }

    /// <summary>投遞限制次數（每組各自設；設定視窗最上面、右鍵「投遞」）：投遞這麼多則就暫停，防 agent 互踢無限迴圈燒 token。0＝不限。</summary>
    public int MaxMessages { get; set; } = DefaultMaxMessages;

    public const int DefaultMaxMessages = 30;

    /// <summary>設定視窗與右鍵選單可選的次數（0＝不限）。</summary>
    public static readonly int[] LimitChoices = { 10, 30, 50, 100, 0 };

    /// <summary>已經投遞到上限（不限＝永遠 false）。</summary>
    public bool LimitReached => MaxMessages > 0 && MessageCount >= MaxMessages;

    /// <summary>上限的顯示文字（分頁列小字、tooltip）：數字或 ∞。</summary>
    public string LimitText => MaxMessages > 0 ? MaxMessages.ToString() : "∞";

    // ---------- AI 聊天室（1.2.3；沿用同一個 AgentGroup／pane 排版，只是不走信箱投遞，改由 AwayTerminal 主持輪流發言）----------
    /// <summary>這一組是代理團隊還是 AI 聊天室。</summary>
    public GroupMode Mode { get; set; } = GroupMode.Team;

    public bool IsChat => Mode == GroupMode.Chat;

    /// <summary>聊天室：討論迴數（一迴＝每個人各發言一次）。</summary>
    public int Rounds { get; set; } = DefaultRounds;

    public const int DefaultRounds = 5;

    /// <summary>設定視窗可選的迴數。</summary>
    public static readonly int[] RoundChoices = { 3, 5, 8, 10 };

    /// <summary>聊天室：某一位超過這麼多分鐘沒發言就跳過他這一迴（在紀錄註明）。</summary>
    public const int TurnTimeoutMinutes = 5;

    /// <summary>討論紀錄資料夾（相對專案資料夾）。</summary>
    public const string ChatRelDir = ".ai/chat";

    /// <summary>這場討論的資料夾名（開聊天室時的時間，例 20260916-1152）。</summary>
    public string ChatFolder { get; set; } = "";

    /// <summary>使用者給的主題（還沒給＝空）。</summary>
    public string Topic { get; set; } = "";

    /// <summary>目前第幾迴（1 起）。</summary>
    public int Round { get; set; } = 1;

    /// <summary>這一迴輪到參加者清單裡的第幾位（0 起）。</summary>
    public int Speaker { get; set; }

    /// <summary>聊天室進行到哪個階段。</summary>
    public ChatPhase Phase { get; set; } = ChatPhase.NeedTopic;

    /// <summary>目前這一輪是什麼時候請他發言的（用來判斷逾時跳過；default＝還沒請）。</summary>
    public DateTime TurnAskedUtc { get; set; }

    /// <summary>使用者按了「結束討論」：這一輪結束後就去寫結論。</summary>
    public bool EndRequested { get; set; }

    /// <summary>閒置檢查（使用者要求，2026-09-16：格 2～4 有時會停著）：整組閒置這麼多分鐘，就請 Agent-x1 問大家目前的狀況。0＝不檢查。</summary>
    public int IdleCheckMinutes { get; set; } = DefaultIdleCheckMinutes;

    public const int DefaultIdleCheckMinutes = 30;

    /// <summary>設定視窗可選的分鐘數（0＝不檢查）。</summary>
    public static readonly int[] IdleCheckChoices = { 15, 30, 60, 0 };

    /// <summary>整組從什麼時候開始全部閒置（default＝現在不是全閒置）。</summary>
    public DateTime AllIdleSinceUtc { get; set; }

    /// <summary>本次程式執行內的投遞序號（「訊息 #n」用，從 1 起）。</summary>
    public int DeliverySeq { get; set; }

    /// <summary>信箱監看（開組時建立、關組時停止）。</summary>
    internal MessageBus? Bus { get; set; }

    /// <summary>最後點過的那一格：點分頁列這一列時回到它。</summary>
    public TerminalTab? LastFocused { get; set; }

    /// <summary>已啟動（有分頁）的格，依格號排序。</summary>
    public IEnumerable<AgentSlot> Running => Slots.Where(s => s.Tab != null);

    /// <summary>分頁列上代表整組的那一列＝格號最小、有分頁的那格（通常是格 1）。</summary>
    public TerminalTab? RowTab => Running.FirstOrDefault()?.Tab;

    /// <summary>有任一格忙碌（分頁列那一列的圖示染忙碌色）。</summary>
    public bool AnyBusy => Running.Any(s => s.Tab!.Status == TermStatus.Busy);

    /// <summary>還沒送出的信（tooltip「待投遞 n」）。</summary>
    public int PendingCount => Slots.Sum(s => s.Queue.Count);

    public AgentSlot? SlotById(string agentId) =>
        Slots.FirstOrDefault(s => string.Equals(s.AgentId, agentId, StringComparison.OrdinalIgnoreCase));

    public static double ClampRatio(double r) => double.IsNaN(r) ? 0.5 : Math.Clamp(r, 0.15, 0.85);

    /// <summary>目前開著的組沒用到的最小組號（1～9；全滿才回 0）。preferred 沒被占用就優先用它（恢復分頁沿用上次號碼）。</summary>
    public static int NextFreeNumber(IEnumerable<AgentGroup> open, int preferred = 0)
    {
        var used = open.Select(g => g.Number).ToHashSet();
        if (preferred is >= 1 and <= 9 && !used.Contains(preferred)) return preferred;
        for (int n = 1; n <= 9; n++) if (!used.Contains(n)) return n;
        return 0;
    }
}
