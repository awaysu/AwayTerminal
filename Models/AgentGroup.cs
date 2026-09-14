using AwayTerminal.Services.MultiAgent;

namespace AwayTerminal.Models;

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

    /// <summary>暫停投遞（使用者右鍵、或訊息數到上限）。暫停中照收信、照排隊，只是不打字。</summary>
    public bool Paused { get; set; }

    /// <summary>本輪已投遞幾則（到 AppSettings.MultiAgentMaxMessages 就暫停；「繼續投遞」歸零）。</summary>
    public int MessageCount { get; set; }

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
