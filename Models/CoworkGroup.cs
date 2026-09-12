namespace AwayTerminal.Models;

/// <summary>
/// Claude+Codex 協作分頁（1.1.11）：兩個獨立的分頁（各自的 session、xterm、log、恢復畫面）綁成一組，
/// 右側分頁列只顯示 <see cref="First"/> 那一列、終端機區左右（或上下）並排兩半。
/// 刻意不另做「一個分頁兩個 session」的新模型：既有的輸出／輸入／狀態／恢復／記錄都以單一分頁為單位，綁組就不用重寫。
/// 使用者決定：這種分頁不支援巨集與 Telegram 遠端。
/// </summary>
public sealed class CoworkGroup
{
    public CoworkGroup(string key, TerminalTab first, TerminalTab second)
    {
        Key = key;
        First = first;
        Second = second;
    }

    /// <summary>穩定代號（存進 SavedTab.CoworkKey，恢復分頁時靠它把兩半重新綁回來）。</summary>
    public string Key { get; }

    /// <summary>左／上半＝Claude Code（分頁列顯示的那一列）。</summary>
    public TerminalTab First { get; }

    /// <summary>右／下半＝Codex。</summary>
    public TerminalTab Second { get; }

    /// <summary>false＝左右並排、true＝上下（分頁列那一列的切換鈕）。</summary>
    public bool Vertical { get; set; }

    /// <summary>第一半占的比例（中間分隔線拖曳；0.15~0.85）。</summary>
    public double Ratio { get; set; } = 0.5;

    /// <summary>最後點過的那一半：點分頁列這一列時回到它。</summary>
    public TerminalTab? LastFocused { get; set; }

    // ---------- 自動交棒狀態（1.1.11，見 MainWindow.Cowork.cs；程式重開不保留＝兩邊 session 也是新開的）----------
    /// <summary>已交棒幾輪（每送出一次「請讀…」＋1）。</summary>
    public int Round { get; set; }
    /// <summary>暫停交棒（使用者右鍵、達到上限、對方已結束）。</summary>
    public bool Paused { get; set; }
    /// <summary>交棒檔寫了 STATUS: DONE。</summary>
    public bool Done { get; set; }
    /// <summary>暫停期間收到、還沒送出的交棒（繼續時補送）：哪一半寫的。</summary>
    public TerminalTab? PendingFrom { get; set; }
    /// <summary>已處理過的交棒檔修改時間（同一個版本不重送）。</summary>
    public DateTime HandledToCodexUtc { get; set; }
    public DateTime HandledToClaudeUtc { get; set; }
    /// <summary>正在等對方閒置後送出（避免同時送兩次）。</summary>
    public bool Delivering { get; set; }

    public TerminalTab Other(TerminalTab t) => ReferenceEquals(t, First) ? Second : First;

    public static double ClampRatio(double r) => double.IsNaN(r) ? 0.5 : Math.Clamp(r, 0.15, 0.85);
}
