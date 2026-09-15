using System.IO;
using System.Text;
using System.Windows;
using AwayTerminal.Dialogs;
using AwayTerminal.Localization;
using AwayTerminal.Models;
using AwayTerminal.Services;
using AwayTerminal.Services.MultiAgent;

namespace AwayTerminal;

/// <summary>
/// Multi-Agent 分頁（1.2.0）：一個分頁裡 2～4 個互動 AI CLI（ClaudeCode／Codex／OpenCode／GeminiCLI）各帶角色分工。
/// <para>畫面：每個 agent 是一個獨立分頁（沿用 1.1.11 協作分頁「綁組」的做法），分頁列只顯示一列，前端把 pane 排成「下一上 N−1」（g 協定）。</para>
/// <para>溝通：agent 寫信到專案的 <c>.ai/bus/</c>（<see cref="MessageBus"/> 發現）→ 這裡依收件人 ID 排進那一格的佇列 →
/// 收件人閒置時打一行「請讀 …」＋隔 300ms 單獨送 Enter（<see cref="SendTextThenEnter"/>）。沒有 hook、沒有 HTTP。</para>
/// <para>防失控：每組投遞 AgentGroup.MaxMessages 則（設定視窗最上面選，預設 30、可不限）就暫停，右鍵「投遞」選次數＝繼續並歸零。不支援巨集與 Telegram 遠端。</para>
/// </summary>
public partial class MainWindow
{
    private readonly List<AgentGroup> _agentGroups = new();
    private System.Windows.Data.ListCollectionView? _stripView;
    /// <summary>關閉整組／重新啟動某格時設成那一組：RemoveTabSilently 不要逐格重排或拆組（做完再一起處理）。</summary>
    private AgentGroup? _suspendRelink;

    // ---------- 分頁列：一組一列 ----------
    /// <summary>分頁列要不要列這個分頁：一般分頁都列；Multi-Agent 只列那一組的代表列。</summary>
    private static bool IsStripRow(TerminalTab t) => t.Agent == null || ReferenceEquals(t.Agent.Group.RowTab, t);

    /// <summary>分頁列上代表這個分頁的那一列（Multi-Agent＝整組的代表列）。</summary>
    private static TerminalTab RowOf(TerminalTab t) => t.Agent?.Group.RowTab ?? t;

    /// <summary>作用中分頁的那一列亮黃框；Multi-Agent 記住最後點的那一格。</summary>
    private void MarkActiveRow(TerminalTab active)
    {
        var row = RowOf(active);
        foreach (var t in Tabs) t.IsActive = ReferenceEquals(t, row);
        if (active.Agent != null) active.Agent.Group.LastFocused = active;
    }

    /// <summary>點分頁列那一列要切到哪個分頁：Multi-Agent＝最後點過的那一格（還在組裡的話）。</summary>
    private static TerminalTab FocusTargetOf(TerminalTab tab)
    {
        var g = tab.Agent?.Group;
        return g?.LastFocused is { Agent: { } a } lf && ReferenceEquals(a.Group, g) ? lf : tab;
    }

    /// <summary>分頁右鍵選單要作用的分頁（記錄 log、配色）：Multi-Agent＝最後點的那一格。</summary>
    private static TerminalTab? MenuTargetOf(object sender)
    {
        var tab = TabOf(sender);
        return tab == null ? null : FocusTargetOf(tab);
    }

    /// <summary>同一組的各格在 Tabs 裡一律緊接在代表列後面、依格號排（分頁列拖曳、恢復分頁、存檔順序都靠這個保持成組）。</summary>
    private void NormalizeAgentOrder()
    {
        foreach (var g in _agentGroups)
        {
            var tabs = g.Running.Select(s => s.Tab!).ToList();
            for (int k = 1; k < tabs.Count; k++)
            {
                int anchor = Tabs.IndexOf(tabs[k - 1]), cur = Tabs.IndexOf(tabs[k]);
                if (anchor < 0 || cur < 0) continue;
                int target = cur > anchor ? anchor + 1 : anchor;   // 在前面的先移走、anchor 會往前一格
                if (cur != target) Tabs.Move(cur, target);
            }
        }
    }

    // ---------- 開一組 ----------
    private void OpenMultiAgent_Click(object sender, RoutedEventArgs e) => OpenMultiAgent(null);

    /// <summary>New → 代理團隊：先選專案資料夾（和其他「啟動前選擇資料夾」的連線一樣）→ 設定視窗。dir 給定＝紀錄重開，直接開設定視窗。</summary>
    private void OpenMultiAgent(string? dir)
    {
        if (DeferUntilWebReady(() => OpenMultiAgent(dir), "OpenMultiAgent")) return;
        if (AgentGroup.NextFreeNumber(_agentGroups) == 0) { Info(Loc.T("ma.tooMany")); return; }
        dir ??= PickWorkDir(Loc.T("ma.pickDir"));
        if (dir == null) { Web.Focus(); return; }
        var dlg = new MultiAgentDialog(dir, null) { Owner = this };
        if (dlg.ShowDialog() != true || dlg.Result is not { } setup) { Web.Focus(); return; }
        if (OpenAgentGroup(setup) == null) return;
        AddHistory(new SavedTab { Type = "multiagent", Title = Loc.T("ma.title"), Dir = setup.Dir });
    }

    /// <summary>依設定開一組：組號 → .gitignore → 組合每格角色檔（名單要完整，先全部組好）→ 依格號啟動 → 綁組 → 開始監看信箱。
    /// restore（恢復分頁）＝格號 → 上次存的 SavedTab（執行檔／參數／scrollback／開啟時間）。</summary>
    private AgentGroup? OpenAgentGroup(MultiAgentSetup setup, string? key = null, int preferredNumber = 0, double ratio = 0.5,
                                       Dictionary<int, SavedTab>? restore = null, string? title = null)
    {
        int number = AgentGroup.NextFreeNumber(_agentGroups, preferredNumber);
        if (number == 0) { Info(Loc.T("ma.tooMany")); return null; }
        var g = new AgentGroup(key ?? Guid.NewGuid().ToString("N"), number, setup.Dir)
        {
            Ratio = AgentGroup.ClampRatio(ratio), MaxMessages = Math.Max(0, setup.MaxMessages)
        };
        for (int i = 0; i < 4; i++)
        {
            var s = g.Slots[i];
            var ss = setup.Slots[i] ?? new AgentSlotSetup();
            s.Enabled = ss.Enabled || (restore == null && i == 0);   // 新開的組格 1 一定啟用
            s.Backend = ss.Backend;
            s.Role = ss.Role;
            s.RoleTitle = RoleLibrary.TitleOf(ss.Role);
        }
        g.Title = !string.IsNullOrWhiteSpace(title) && !Tabs.Any(t => t.Title == title) ? title : DirTabName(setup.Dir, Loc.T("ma.title"));

        MessageBus.EnsureGitIgnore(setup.Dir);
        RoleLibrary.ClearSession(number);
        foreach (var s in g.Slots.Where(x => x.Enabled))
        {
            try { RoleLibrary.Compose(g, s); }
            catch (Exception ex) { Diag.Log($"ma compose {s.AgentId}: {ex.Message}"); }
        }

        _agentGroups.Add(g);
        foreach (var s in g.Slots.Where(x => x.Enabled))
            LaunchSlot(g, s, restore != null && restore.TryGetValue(s.Index, out var st) ? st : null);
        if (!g.Running.Any())
        {
            _agentGroups.Remove(g);
            Info(Loc.T("ma.openFail"));
            return null;
        }
        LinkAgentGroup(g);
        StartBus(g);
        if (g.RowTab != null) SelectTab(g.RowTab);
        Diag.Log($"ma open team {g.Number} dir={g.Dir} agents={string.Join(",", g.Running.Select(s => $"{s.AgentId}:{s.Backend}:{s.Role}"))}");
        return g;
    }

    /// <summary>啟動一格：Coding Agent 的自訂連線（恢復分頁時用上次存的）＋ adapter 的附加參數，走既有 OpenCustom（不記紀錄）。</summary>
    private TerminalTab? LaunchSlot(AgentGroup g, AgentSlot s, SavedTab? saved = null)
    {
        var adapter = AdapterRegistry.ByKey(s.Backend);
        if (adapter == null) { Diag.Log($"ma launch {s.AgentId}: unknown backend '{s.Backend}'"); return null; }

        CustomConn? conn = null;
        if (saved != null && !string.IsNullOrWhiteSpace(saved.Path) && (saved.ViaPowerShell || File.Exists(saved.Path)))
            conn = new CustomConn
            {
                Name = string.IsNullOrWhiteSpace(saved.Name) ? adapter.DisplayName : saved.Name,
                Path = saved.Path, Args = saved.Args, Icon = s.Backend, ViaPowerShell = saved.ViaPowerShell,
                CloseKey = saved.CloseKey, CloseCount = saved.CloseCount
            };
        conn ??= adapter.Resolve();
        if (conn == null)
        {
            Diag.Log($"ma launch {s.AgentId}: {adapter.DisplayName} not found");
            MessageBox.Show(this, string.Format(Loc.T("ma.backendMissing"), adapter.DisplayName, s.AgentId), Loc.T("ma.title"),
                MessageBoxButton.OK, MessageBoxImage.Warning);
            return null;
        }

        if (string.IsNullOrEmpty(s.RoleFile) || !File.Exists(s.RoleFile))
        {
            try { RoleLibrary.Compose(g, s); } catch (Exception ex) { Diag.Log($"ma compose {s.AgentId}: {ex.Message}"); }
        }
        string roleText = "";
        try { roleText = File.ReadAllText(s.RoleFile, Encoding.UTF8); } catch { }
        var launch = adapter.BuildLaunch(conn, s, roleText);

        _restoreBufferForNextTab = saved != null ? LoadRestoreBuffer(saved) : null;
        _restoreOpenedForNextTab = saved == null || saved.OpenedUtc == default ? null : saved.OpenedUtc;
        TerminalTab? tab;
        try { tab = OpenCustom(conn, g.Dir, s.AgentId, addHistory: false, extraArgs: launch.ExtraArgs); }
        catch (Exception ex) { Diag.Log($"ma launch {s.AgentId}: {ex.Message}"); tab = null; }
        finally { _restoreBufferForNextTab = null; _restoreOpenedForNextTab = null; }
        if (tab == null) return null;

        s.Tab = tab;
        tab.Agent = s;
        s.LaunchedUtc = DateTime.UtcNow;
        s.PendingFirstMessage = launch.FirstMessage;
        s.RoleInjected = launch.FirstMessage == null;
        s.LastDeliveredUtc = default;
        s.DeliveryChecked = true;
        s.PostedState = -1;
        Diag.Log($"ma launch {s.AgentId} backend={s.Backend} role={s.Role} path={conn.Path} viaPs={conn.ViaPowerShell} " +
                 $"role-inject={(launch.FirstMessage == null ? "flag" : "typed")} args+={launch.ExtraArgs.Length}");
        return tab;
    }

    /// <summary>綁組／重綁：順序整理、標題（代表列＝組名、其餘＝Agent ID）、分頁列只留一列、前端排版（g 協定）。</summary>
    private void LinkAgentGroup(AgentGroup g)
    {
        NormalizeAgentOrder();
        var running = g.Running.ToList();
        if (running.Count == 0) return;
        foreach (var s in running)
        {
            var tab = s.Tab!;
            string want = ReferenceEquals(s, running[0]) ? g.Title : s.AgentId;
            if (tab.Title != want) { tab.Title = want; PostToWeb("t" + tab.Id + US + want); }
            tab.RaiseAgent();
            s.PostedState = -1;   // 重排後 pane 的狀態標籤重送
        }
        _stripView?.Refresh();
        PostAgentGroup(g);
        if (_active != null) MarkActiveRow(_active);
    }

    /// <summary>g{下方 pane id}US{上列比例}US{上列 id,…}US{標籤|…}US{外框顏色,…}（標籤與顏色的順序＝下方、上列由左到右）。</summary>
    private void PostAgentGroup(AgentGroup g)
    {
        var run = g.Running.ToList();
        if (run.Count == 0) return;
        PostToWeb("g" + run[0].Tab!.Id + US + g.Ratio.ToString("0.###", System.Globalization.CultureInfo.InvariantCulture) + US +
                  string.Join(",", run.Skip(1).Select(s => s.Tab!.Id)) + US +
                  string.Join("|", run.Select(s => s.Label.Replace("|", "/").Replace(US, ' '))) + US +
                  string.Join(",", run.Select(s => s.Color)));
    }

    /// <summary>整組已經沒有分頁：停止監看信箱、移出清單。</summary>
    private void DisbandAgentGroup(AgentGroup g)
    {
        _agentGroups.Remove(g);
        try { g.Bus?.Dispose(); } catch { }
        g.Bus = null;
        foreach (var s in g.Slots) s.Queue.Clear();
        _stripView?.Refresh();
        Diag.Log($"ma close team {g.Number}");
    }

    /// <summary>關閉整組（分頁列只有一列＝一起關；ask＝先確認）。</summary>
    private void CloseAgentGroup(AgentGroup g, bool ask)
    {
        if (ask && MessageBox.Show(this, string.Format(Loc.T("ma.closeConfirm"), g.Title, g.Running.Count()),
                Loc.T("msg.closeTabTitle"), MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
        _suspendRelink = g;
        try { foreach (var s in g.Running.Reverse().ToList()) RemoveTabSilently(s.Tab!); }
        finally { _suspendRelink = null; }
        if (_agentGroups.Contains(g)) DisbandAgentGroup(g);
    }

    /// <summary>RemoveTabSilently 的 Multi-Agent 收尾（分頁已從 Tabs 移除之後呼叫）：組裡還有別格＝重綁，沒有＝拆組。</summary>
    private void AfterAgentTabRemoved(AgentGroup g)
    {
        if (ReferenceEquals(g, _suspendRelink) || !_agentGroups.Contains(g)) return;
        if (!g.Running.Any()) DisbandAgentGroup(g);
        else LinkAgentGroup(g);
    }

    // ---------- 信箱 ----------
    private void StartBus(AgentGroup g)
    {
        try
        {
            var bus = new MessageBus(g.Dir);
            bus.MessageArrived += m => Dispatcher.InvokeAsync(() => OnAgentMessage(g, m));
            g.Bus = bus;
            bus.Start();
        }
        catch (Exception ex) { Diag.Log("ma bus start: " + ex.Message); }
    }

    /// <summary>「Agent-11  Product Manager (ClaudeCode), …」（AwayTerminal 寫給 agent 的通知信用）。</summary>
    private static string Roster(AgentGroup g) =>
        string.Join(", ", g.Slots.Where(s => s.Enabled).Select(s => $"{s.AgentId} {s.RoleTitle} ({s.BackendName})"));

    /// <summary>信箱發現一封新信（UI 執行緒）。只收「收件人是本組」的（all＝寄件人是本組）；同資料夾的其他組各自處理自己的。</summary>
    private void OnAgentMessage(AgentGroup g, AgentMessage m)
    {
        if (!_agentGroups.Contains(g) || g.Bus == null || g.Bus.IsDelivered(m.FileName)) return;
        string prefix = "Agent-" + g.Number;
        bool Mine(string id) => id.Length == prefix.Length + 1 && id.StartsWith(prefix, StringComparison.OrdinalIgnoreCase);
        if (m.IsBroadcast ? !Mine(m.From) : !Mine(m.To))
        {
            Diag.Log($"ma skip {m.FileName} from={m.From} to={m.To} (not team {g.Number})");
            return;
        }
        Diag.Log($"ma msg {m.FileName} from={m.From} to={m.To} type={m.Type} task={m.Task}{(m.HeaderWarning ? " header-warn" : "")}");

        if (m.IsBroadcast)
        {
            var targets = g.Running.Where(s => !string.Equals(s.AgentId, m.From, StringComparison.OrdinalIgnoreCase)).ToList();
            if (targets.Count == 0) g.Bus.MarkDelivered(m.FileName);
            foreach (var s in targets) s.Queue.Enqueue(m);
        }
        else
        {
            var slot = g.SlotById(m.To);
            if (slot == null || !slot.Enabled || slot.Tab == null)
            {
                g.Bus.MarkDelivered(m.FileName);
                Diag.Log($"ma msg {m.FileName}: {m.To} is not running in team {g.Number}");
                if (g.SlotById(m.From) is { Tab: not null } sender)   // 寄件人是本組 agent 才回通知（AwayTerminal 自己的信不回，免得打轉）
                    g.Bus.Write("AwayTerminal", sender.AgentId, "INFO", m.Task,
                        $"Your message {m.RelPath} was not delivered: {m.To} is not running in this team.\n\n" +
                        $"Running agents: {string.Join(", ", g.Running.Select(s => $"{s.AgentId} {s.RoleTitle} ({s.BackendName})"))}.\n" +
                        "Send it to one of them instead, or tell the user that this agent needs to be enabled.");
                return;
            }
            slot.Queue.Enqueue(m);
        }
        PostAgentState(g);
        g.RowTab?.RaiseAgentState();
    }

    // ---------- 投遞（狀態輪詢每 0.6 秒呼叫）----------
    private void MultiAgentTick()
    {
        if (_agentGroups.Count == 0) return;
        var now = DateTime.UtcNow;
        foreach (var g in _agentGroups.ToList())
        {
            foreach (var s in g.Running.ToList())
            {
                var tab = s.Tab!;
                if (tab.Session == null) continue;

                // Enter 補送：打完那一行 10 秒後，收件人除了打字回顯之外沒有再輸出＝Enter 可能沒送出去
                // （claude 把整塊當貼上、CR 變換行，1.1.10 實測過）→ 單獨再送一次 Enter。沒有重打整行，不會重複投遞。
                if (!s.DeliveryChecked && (now - s.LastDeliveredUtc).TotalSeconds >= 10)
                {
                    s.DeliveryChecked = true;
                    if ((tab.LastOutputUtc - s.LastDeliveredUtc).TotalSeconds < 2)
                    {
                        tab.Session.WriteText("\r");
                        Diag.Log($"ma resend-enter {s.AgentId}");
                    }
                }

                if (!AgentReady(s, now)) continue;

                if (s.PendingFirstMessage != null)   // OpenCode／Gemini：角色靠第一句打進去（保底注入）
                {
                    SendTextThenEnter(tab, s.PendingFirstMessage);
                    s.PendingFirstMessage = null;
                    s.RoleInjected = true;
                    MarkTyped(s, now);
                    s.DeliveryChecked = true;   // 預期它只回一句 READY、輸出很短——不做「Enter 沒送出」補送（實測會誤判）
                    Diag.Log($"ma role-inject typed {s.AgentId}");
                    continue;
                }
                if (s.Queue.Count == 0 || g.Paused || !s.RoleInjected) continue;
                if (g.LimitReached) { PauseAgentGroup(g, limit: true); continue; }
                DeliverQueued(g, s, now);
            }
            PostAgentState(g);
            g.RowTab?.RaiseAgentState();
        }
    }

    /// <summary>這一格現在可以打字給它嗎：CLI 啟動後有畫過東西、已經靜止一陣子、剛打過字的等它開始工作、使用者沒在打字。
    /// 經 PowerShell 啟動的（npm 版 .cmd）多等一點：PowerShell 提示行出來之後 node 還要載入 CLI，那段安靜期打的字會被吃掉。</summary>
    private static bool AgentReady(AgentSlot s, DateTime now)
    {
        var tab = s.Tab!;
        bool viaPs = tab.Kind == TermKind.PowerShell;
        return tab.PendingCommand == null
            && (now - s.LaunchedUtc).TotalSeconds >= (viaPs ? 10 : 5)
            && tab.LastOutputUtc > s.LaunchedUtc
            && (now - tab.LastOutputUtc).TotalMilliseconds >= (viaPs ? 3000 : 2000)
            && (now - s.LastDeliveredUtc).TotalSeconds >= 3
            && (now - tab.LastInputUtc).TotalSeconds >= 3
            && (now - tab.LastSubmitUtc).TotalSeconds >= 3;   // 遠端（Telegram）剛送出訊息給 Agent-x1：別在 Enter 送達前把信打進去
    }

    private static void MarkTyped(AgentSlot s, DateTime now)
    {
        s.LastDeliveredUtc = now;
        s.DeliveryChecked = false;
    }

    /// <summary>把這格佇列裡的信一次送出：一封＝「訊息 #n from … 請讀 …」；多封＝「你有 k 則新訊息：請依序讀 …」。每封都記進 .delivered、計數。</summary>
    private void DeliverQueued(AgentGroup g, AgentSlot s, DateTime now)
    {
        int take = g.MaxMessages > 0 ? Math.Min(s.Queue.Count, g.MaxMessages - g.MessageCount) : s.Queue.Count;
        var batch = new List<AgentMessage>();
        while (batch.Count < take && s.Queue.Count > 0) batch.Add(s.Queue.Dequeue());
        if (batch.Count == 0) return;

        string line;
        if (batch.Count == 1)
        {
            var m = batch[0];
            g.DeliverySeq++;
            line = string.Equals(m.From, "AwayTerminal", StringComparison.OrdinalIgnoreCase)
                ? string.Format(Loc.T("ma.deliverInfo"), g.DeliverySeq, m.RelPath)
                : string.Format(Loc.T("ma.deliverOne"), g.DeliverySeq, m.From, string.IsNullOrWhiteSpace(m.Task) ? "—" : m.Task, m.Type, m.RelPath);
        }
        else
        {
            g.DeliverySeq += batch.Count;
            line = string.Format(Loc.T("ma.deliverMany"), batch.Count, string.Join(Loc.Lang == "en" ? ", " : "、", batch.Select(b => b.RelPath)));
        }
        SendTextThenEnter(s.Tab!, line);
        foreach (var m in batch) g.Bus?.MarkDelivered(m.FileName);
        g.MessageCount += batch.Count;
        MarkTyped(s, now);
        Diag.Log($"ma deliver #{g.DeliverySeq} -> {s.AgentId} ({string.Join(", ", batch.Select(b => b.FileName))}) count={g.MessageCount}/{g.LimitText}");
        if (g.LimitReached) PauseAgentGroup(g, limit: true);
    }

    private void PauseAgentGroup(AgentGroup g, bool limit)
    {
        if (g.Paused) return;
        g.Paused = true;
        g.PausedByLimit = limit;
        Diag.Log($"ma pause team {g.Number}{(limit ? $" (limit {g.MessageCount})" : "")} pending={g.PendingCount}");
        g.RowTab?.RaiseAgentState();
        if (limit) FlashIfInactive();
    }

    /// <summary>pane 標題的狀態標籤（E 協定；只在變了才送）：0 閒置、1 忙碌、2 有信待投遞、3 已結束、4 忙碌且有信待投遞。
    /// 4 是使用者回報後補的：信只在閒置時投遞，原本忙碌蓋掉「有信待送」，PM 寄了暫停信、對方沒停也看不出信還在排隊。</summary>
    private void PostAgentState(AgentGroup g)
    {
        if (!_webReady) return;
        foreach (var s in g.Running)
        {
            var tab = s.Tab!;
            bool queued = s.Queue.Count > 0;
            int st = tab.Session == null ? 3 : tab.Status == TermStatus.Busy ? (queued ? 4 : 1) : queued ? 2 : 0;
            if (st == s.PostedState) continue;
            s.PostedState = st;
            PostToWeb("E" + tab.Id + US + st);
        }
    }

    /// <summary>送一行文字進分頁、隔 RemoteEnterDelayMs 再單獨送 Enter（claude 分頁文字走 JS doPaste）。遠端送指令與 Multi-Agent 投遞共用。
    /// 文字＋CR 一次寫入時 claude 會當成貼上、CR 變換行而不送出（1.1.10 實測），所以一律分兩次。</summary>
    private void SendTextThenEnter(TerminalTab tab, string text, bool enter = true)
    {
        var session = tab.Session;
        if (session == null) return;
        if (tab.ClaudePaste && _webReady) PasteToTab(tab.Id, text);
        else if (text.Length > 0) session.WriteText(text);
        if (!enter) return;
        tab.LastSubmitUtc = DateTime.UtcNow;
        var timer = new System.Windows.Threading.DispatcherTimer { Interval = TimeSpan.FromMilliseconds(RemoteEnterDelayMs) };
        timer.Tick += (_, _) => { timer.Stop(); if (tab.Session == session) session.WriteText("\r"); };
        timer.Start();
    }

    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    private struct FLASHWINFO { public uint cbSize; public IntPtr hwnd; public uint dwFlags; public uint uCount; public uint dwTimeout; }
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool FlashWindowEx(ref FLASHWINFO pwfi);

    /// <summary>投遞到上限暫停時，視窗不在前景就閃工作列按鈕提醒使用者（切回來自動停）。</summary>
    private void FlashIfInactive()
    {
        try
        {
            if (IsActive) return;
            var fi = new FLASHWINFO
            {
                cbSize = (uint)System.Runtime.InteropServices.Marshal.SizeOf<FLASHWINFO>(),
                hwnd = new System.Windows.Interop.WindowInteropHelper(this).Handle,
                dwFlags = 0x2 | 0xC   // FLASHW_TRAY | FLASHW_TIMERNOFG
            };
            FlashWindowEx(ref fi);
        }
        catch { }
    }

    // ---------- 分頁右鍵 ----------
    /// <summary>「代理團隊設定…」：啟動沒啟用的格、關掉取消勾選的格、改了 CLI／角色（或按「重新啟動」）的格重新啟動。
    /// 會結束執行中 agent 的變更，設定視窗按「套用」時已經確認過。</summary>
    private void AgentSetup_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Agent?.Group is not { } g) return;
        var dlg = new MultiAgentDialog(g.Dir, g) { Owner = this };
        if (dlg.ShowDialog() != true || dlg.Result is not { } r) { Web.Focus(); return; }
        ApplyAgentSetup(g, r);
    }

    private void ApplyAgentSetup(AgentGroup g, MultiAgentSetup r)
    {
        if (!_agentGroups.Contains(g)) return;
        if (r.MaxMessages != g.MaxMessages)
        {
            g.MaxMessages = Math.Max(0, r.MaxMessages);
            if (g.Paused && g.PausedByLimit && !g.LimitReached) g.Paused = g.PausedByLimit = false;   // 因為到上限而暫停、上限調高了＝接著送（計數照舊）
            Diag.Log($"ma setup team {g.Number}: limit={g.LimitText} count={g.MessageCount} paused={g.Paused}");
            g.RowTab?.RaiseAgentState();
        }
        var close = r.Close.Where(i => i is >= 1 and <= 4).Distinct().ToList();
        var launch = r.Launch.Where(i => i is >= 1 and <= 4 && !close.Contains(i)).Distinct().OrderBy(i => i).ToList();
        bool wasActive = _active?.Agent?.Group == g;
        var fresh = new List<AgentSlot>();
        var closed = new List<string>();
        bool rosterChanged = false;
        _suspendRelink = g;
        try
        {
            foreach (int i in close)
            {
                var s = g.Slots[i - 1];
                if (s.Tab != null) RemoveTabSilently(s.Tab);   // 執行中＝結束那個 CLI；已結束＝收掉那一格
                rosterChanged |= s.Enabled;
                s.Enabled = false;
                s.Queue.Clear();
                closed.Add(s.AgentId);
            }
            foreach (int i in launch)
            {
                var s = g.Slots[i - 1];
                var ss = r.Slots[i - 1];
                if (s.Tab != null)
                {
                    RemoveTabSilently(s.Tab);   // 執行中或已結束：關掉舊分頁、同一格用新設定重開（角色是啟動時注入的，改角色也得重開）
                    rosterChanged |= s.Backend != ss.Backend || s.Role != ss.Role;
                }
                else rosterChanged = true;
                s.Enabled = true;
                s.Backend = ss.Backend;
                s.Role = ss.Role;
                s.RoleTitle = RoleLibrary.TitleOf(ss.Role);
                s.Queue.Clear();
                fresh.Add(s);
            }
        }
        finally { _suspendRelink = null; }

        // 名單變了 → 每格的角色檔（隊友名單）都重組；新開的格用新檔啟動
        foreach (var s in g.Slots.Where(x => x.Enabled))
        {
            try { RoleLibrary.Compose(g, s); } catch (Exception ex) { Diag.Log($"ma compose {s.AgentId}: {ex.Message}"); }
        }
        foreach (var s in fresh) LaunchSlot(g, s);
        if (!g.Running.Any()) { DisbandAgentGroup(g); return; }
        LinkAgentGroup(g);
        // 關掉的那格若是作用中分頁，RemoveTabSilently 會跳到清單裡的下一個分頁（可能是別的分頁）→ 拉回這一組
        if (wasActive && _active?.Agent?.Group != g && g.RowTab != null) SelectTab(FocusTargetOf(g.RowTab));

        // 通知 PM（沒有 PM 角色就是代表列那一格）：隊友名單變了、角色檔已更新（它是啟動時讀的，要它重讀 Runtime Context）
        if (rosterChanged && g.Bus != null)
        {
            var pm = g.Running.FirstOrDefault(s => s.Role == "product-manager" && !fresh.Contains(s))
                  ?? g.Running.FirstOrDefault(s => !fresh.Contains(s));
            if (pm != null)
                g.Bus.Write("AwayTerminal", pm.AgentId, "INFO", "",
                    $"The team roster changed. Enabled agents now: {Roster(g)}.\n\n" +
                    $"Your role file {pm.RoleFile} has been regenerated. Re-read its Runtime Context section before assigning more work.");
        }
        Diag.Log($"ma setup team {g.Number}: launched {string.Join(",", fresh.Select(s => s.AgentId))} closed {string.Join(",", closed)}");
    }

    /// <summary>右鍵「投遞」子選單打開：勾目前的狀態——暫停中＝勾「暫停」，否則勾目前的次數。</summary>
    private void AgentDelivery_SubmenuOpened(object sender, RoutedEventArgs e)
    {
        if (!ReferenceEquals(e.OriginalSource, sender) || sender is not System.Windows.Controls.MenuItem parent) return;
        if (TabOf(sender)?.Agent?.Group is not { } g) return;
        foreach (var o in parent.Items)
            if (o is System.Windows.Controls.MenuItem mi)
                mi.IsChecked = g.Paused ? (string?)mi.Tag == "pause" : (string?)mi.Tag == g.MaxMessages.ToString();
    }

    /// <summary>右鍵「投遞」→ 10／30／50／100／不限／暫停。選次數＝改成這個上限並繼續投遞（暫停中就解除、本輪計數歸零，補送暫停期間收到的信）；
    /// 沒有暫停時只改上限、計數照舊。</summary>
    private void AgentDelivery_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Agent?.Group is not { } g || sender is not System.Windows.Controls.MenuItem { Tag: string tag }) return;
        if (tag == "pause") { PauseAgentGroup(g, limit: false); return; }
        if (!int.TryParse(tag, out int max)) return;
        g.MaxMessages = Math.Max(0, max);
        if (g.Paused)
        {
            g.Paused = g.PausedByLimit = false;
            g.MessageCount = 0;
        }
        Diag.Log($"ma delivery team {g.Number}: limit={g.LimitText} count={g.MessageCount} pending={g.PendingCount}");
        g.RowTab?.RaiseAgentState();
    }

    private const int StopClearDelayMs = 1000, StopPromptDelayMs = 1500;

    /// <summary>右鍵「停止任務」（使用者要求）：整組每一格送 Esc 中斷正在做的事 → 1 秒後 Ctrl+U 清輸入框 → 1.5 秒時打「先停一下然後記錄目前狀態」＋Enter。
    /// 信只在收件人閒置時投遞，PM 寄暫停信攔不住正在工作的 agent，所以要有直接中斷的入口。
    /// <para>Ctrl+U 是 probe 實測補的：claude 還在思考、沒輸出就被 Esc 中斷時，會把剛才那則訊息放回輸入框，
    /// 不清掉的話停止句會接在後面、合成一則重新送出（＝它繼續做原本的事）。codex 同流程（閒置／思考中／輸出中）實測 Ctrl+U 無副作用。
    /// 閒置的格收到 Esc 也無害（claude／codex 閒置時實測照樣收下停止句並回報狀態）。</para></summary>
    private void AgentStop_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Agent?.Group is not { } g) return;
        var targets = g.Running.Where(s => s.Tab!.Session != null).Select(s => (Slot: s, Tab: s.Tab!, Session: s.Tab!.Session!)).ToList();
        if (targets.Count == 0) return;
        var now = DateTime.UtcNow;
        foreach (var t in targets)
        {
            t.Session.WriteText("\x1b");
            MarkTyped(t.Slot, now);   // 停止流程跑完前不投遞佇列裡的信（AgentReady 要距上次打字 ≥3 秒）
        }
        string prompt = Loc.T("ma.stopPrompt");
        bool cleared = false;
        var timer = new System.Windows.Threading.DispatcherTimer { Interval = TimeSpan.FromMilliseconds(StopClearDelayMs) };
        timer.Tick += (_, _) =>
        {
            if (!cleared)
            {
                cleared = true;
                foreach (var t in targets) if (t.Tab.Session == t.Session) t.Session.WriteText("\x15");   // Ctrl+U
                timer.Interval = TimeSpan.FromMilliseconds(StopPromptDelayMs - StopClearDelayMs);
                return;
            }
            timer.Stop();
            foreach (var t in targets)
            {
                if (t.Tab.Session != t.Session) continue;   // 這 1.5 秒內那格被重開或結束
                SendTextThenEnter(t.Tab, prompt);
                MarkTyped(t.Slot, DateTime.UtcNow);
            }
        };
        timer.Start();
        Diag.Log($"ma stop team {g.Number}: esc+prompt -> {string.Join(",", targets.Select(t => t.Slot.AgentId))}");
    }

    /// <summary>「開啟訊息資料夾」：檔案總管開專案的 .ai\bus。</summary>
    private void AgentOpenBus_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Agent?.Group is not { } g) return;
        string dir = Path.Combine(g.Dir, ".ai", "bus");
        try { Directory.CreateDirectory(dir); System.Diagnostics.Process.Start("explorer.exe", $"\"{dir}\""); }
        catch (Exception ex) { Diag.Log("ma open bus: " + ex.Message); }
    }

    // ---------- 恢復分頁 ----------
    /// <summary>上次關閉時存的一組（同一個 AgentKey 的各格）→ 同資料夾、同組號（沒被占用的話）、同比例重開；
    /// 每格照上次的執行檔／參數、scrollback 倒回去。角色檔以目前的 roles\ 重新組合、CLI 是新 session（OpenCode／Gemini 重新打第一句）。</summary>
    private void RestoreAgentGroup(List<SavedTab> entries)
    {
        var first = entries.OrderBy(x => x.AgentIndex).First();
        if (string.IsNullOrWhiteSpace(first.Dir) || !Directory.Exists(first.Dir))
        {
            Diag.Log($"ma restore skipped: folder missing '{first.Dir}'");
            return;
        }
        var setup = new MultiAgentSetup { Dir = first.Dir, MaxMessages = first.AgentMaxMessages };
        var bySlot = new Dictionary<int, SavedTab>();
        foreach (var st in entries)
        {
            if (st.AgentIndex is < 1 or > 4 || bySlot.ContainsKey(st.AgentIndex)) continue;
            bySlot[st.AgentIndex] = st;
            setup.Slots[st.AgentIndex - 1] = new AgentSlotSetup { Enabled = true, Backend = st.AgentBackend, Role = st.AgentRole };
        }
        if (bySlot.Count == 0) return;
        OpenAgentGroup(setup, first.AgentKey, first.AgentGroupNumber, first.AgentRatio, bySlot, first.Title);
    }
}
