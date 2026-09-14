using AwayTerminal.Services.MultiAgent;

namespace AwayTerminal.Models;

/// <summary>Multi-Agent 分頁裡的一格（一個 agent）。</summary>
public sealed class AgentSlot
{
    public AgentSlot(AgentGroup group, int index) { Group = group; Index = index; }

    public AgentGroup Group { get; }

    /// <summary>格號 1～4（決定外框顏色：1 淡紅、2 淡藍、3 淡綠、4 淡紫）。</summary>
    public int Index { get; }

    public string AgentId => $"Agent-{Group.Number}{Index}";

    /// <summary>角色檔名（roles\*.md 去副檔名，例 software-engineer）；空＝None。</summary>
    public string Role { get; set; } = "";

    /// <summary>角色標題（角色檔第一個 # 標題；None＝「None」），pane 標題與執行期脈絡用。</summary>
    public string RoleTitle { get; set; } = "None";

    /// <summary>Coding Agent 種類＝圖示 key：claude-code / codex / opencode / geminicli。</summary>
    public string Backend { get; set; } = "";

    public string BackendName => AdapterRegistry.ByKey(Backend)?.DisplayName ?? Backend;

    /// <summary>設定視窗勾了「啟用」。</summary>
    public bool Enabled { get; set; }

    /// <summary>這格的分頁（未啟動＝null）。</summary>
    public TerminalTab? Tab { get; set; }

    /// <summary>這次啟動的時間（判斷「CLI 已經畫完開場、可以打字」用；恢復分頁時 TerminalTab.StartUtc 會是原始開啟時間，不能拿來判斷）。</summary>
    public DateTime LaunchedUtc { get; set; }

    /// <summary>角色已經交給這個 CLI（啟動參數注入＝一開始就 true；OpenCode／Gemini 要等第一次閒置時打「請先讀角色檔」才算）。</summary>
    public bool RoleInjected { get; set; }

    /// <summary>還沒打給 CLI 的「請先讀角色檔」那一句（null＝不需要或已送）。</summary>
    public string? PendingFirstMessage { get; set; }

    /// <summary>組合好的角色檔（sessions\組號\Agent-xx.md 絕對路徑）。</summary>
    public string RoleFile { get; set; } = "";

    /// <summary>待投遞的信（FIFO）。</summary>
    public Queue<AgentMessage> Queue { get; } = new();

    /// <summary>上一次打字給這格的時間（投遞後要等它開始工作、再閒下來才送下一封；也用來判斷 Enter 有沒有送出去）。</summary>
    public DateTime LastDeliveredUtc { get; set; }

    /// <summary>上一次投遞後是否已經檢查過「Enter 沒送出」（只補送一次 Enter）。</summary>
    public bool DeliveryChecked { get; set; } = true;

    /// <summary>上次送給前端的 pane 狀態標籤（E 協定；-1＝要重送）。</summary>
    public int PostedState { get; set; } = -1;

    /// <summary>pane 標題：Agent-12 · Software Engineer · Codex。</summary>
    public string Label => $"{AgentId} · {RoleTitle} · {BackendName}";

    /// <summary>角色縮寫（視窗標題［］與分頁 tooltip 用）。</summary>
    public string ShortLabel => $"{AgentId} {RoleTitle}";

    /// <summary>外框顏色（Material 200 級，與分頁列狀態圖示同一系列）。</summary>
    public string Color => Index switch { 1 => "#EF9A9A", 2 => "#90CAF9", 3 => "#A5D6A7", _ => "#CE93D8" };
}
