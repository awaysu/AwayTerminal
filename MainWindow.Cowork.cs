using System.IO;
using System.Windows;
using AwayTerminal.Localization;
using AwayTerminal.Models;
using AwayTerminal.Services;

namespace AwayTerminal;

/// <summary>
/// Claude+Codex 協作分頁的自動交棒（1.1.11）。分頁本身（綁組、左右／上下、恢復）在 MainWindow.xaml.cs 的「協作分頁」區。
/// <para>流程：某一半的 agent 一輪回覆結束 → 它的 Stop hook 打 CoworkBridge → 這裡找出是哪一組哪一半 →
/// 看它負責寫的交棒檔（Claude＝.ai/handoff/to-codex.md、Codex＝to-claude.md）這個版本處理過沒有 →
/// 新的就等另一半閒置（2 秒沒輸出）、打一行「請讀 …（第 N 輪）」＋隔 300ms 單獨送 Enter。
/// 交棒檔寫了 STATUS: DONE ＝停止；超過上限輪數／使用者暫停＝暫停（右鍵「繼續交棒」補送暫停期間的那一份）。</para>
/// <para>「做完」一律以 Stop hook 為準、不看 AwayTerminal 的忙／閒：停在權限詢問畫面時輸出也會停，拿忙／閒當完成會送錯時機；
/// 忙／閒只用來判斷「現在能不能打字給對方」。</para>
/// </summary>
public partial class MainWindow
{
    private CoworkBridge? _coworkBridge;

    /// <summary>取得（必要時啟動）交棒訊號接收端；啟動失敗回 null（協作分頁照開，只是不會自動交棒）。</summary>
    private CoworkBridge? EnsureCoworkBridge()
    {
        if (_coworkBridge != null) return _coworkBridge;
        var b = new CoworkBridge();
        if (!b.Start(AppSettings.Current.CoworkPort)) return null;
        if (AppSettings.Current.CoworkPort != b.Port) { AppSettings.Current.CoworkPort = b.Port; AppSettings.Current.Save(); }
        b.StopReceived += (agent, cwd, pid) => Dispatcher.InvokeAsync(() => OnCoworkStop(agent, cwd, pid));
        _coworkBridge = b;
        return b;
    }

    /// <summary>協作分頁某一半的額外啟動參數（注入 Stop hook＋交棒規則）；看連線是 Claude 還是 Codex。</summary>
    private string CoworkExtraArgs(string icon, string path)
    {
        var b = EnsureCoworkBridge();
        if (b == null) return "";
        var probe = new CustomConn { Icon = icon, Path = path };
        if (ConnIs(probe, "claude-code", "claude")) return b.ClaudeArgs();
        if (ConnIs(probe, "codex", "codex")) return b.CodexArgs();
        return "";
    }

    private void OnCoworkStop(string agent, string cwd, int clientPid)
    {
        var tab = FindCoworkTabByProcess(clientPid) ?? FindCoworkTabByCwd(agent, cwd);
        Diag.Log($"cowork stop agent={agent} pid={clientPid} tab={tab?.Id.ToString() ?? "?"} cwd={cwd}");
        if (tab?.Cowork is { } g) CheckHandoff(g, tab);
    }

    /// <summary>從 curl 的 PID 沿父行程往上，找到某個協作分頁 session 的行程（經 PowerShell 跑的也會在鏈上）。</summary>
    private TerminalTab? FindCoworkTabByProcess(int pid)
    {
        if (pid <= 0) return null;
        Dictionary<int, int> parents;
        try { parents = ProcessTree.ParentMap(); } catch { return null; }
        var bySession = Tabs.Where(t => t.Cowork != null && t.Session != null && t.Session.ProcessId != 0)
                            .GroupBy(t => t.Session!.ProcessId).ToDictionary(g => g.Key, g => g.First());
        for (int i = 0, cur = pid; i < 24 && cur > 0; i++)
        {
            if (bySession.TryGetValue(cur, out var t)) return t;
            if (!parents.TryGetValue(cur, out int parent) || parent == cur) break;
            cur = parent;
        }
        return null;
    }

    /// <summary>退路：agent 種類（claude＝第一半、codex＝第二半）＋工作目錄；剛好一個才算數（同資料夾開兩組就不猜）。</summary>
    private TerminalTab? FindCoworkTabByCwd(string agent, string cwd)
    {
        if (string.IsNullOrWhiteSpace(cwd)) return null;
        string norm(string p) { try { return Path.GetFullPath(p).TrimEnd('\\', '/'); } catch { return p; } }
        string want = norm(cwd);
        var hits = Tabs.Where(t => t.Cowork is { } g && ReferenceEquals(agent == "claude" ? g.First : g.Second, t)
                                   && string.Equals(norm(t.WorkDir), want, StringComparison.OrdinalIgnoreCase)).ToList();
        return hits.Count == 1 ? hits[0] : null;
    }

    private static string HandoffFileName(CoworkGroup g, TerminalTab writer) => ReferenceEquals(writer, g.First) ? "to-codex.md" : "to-claude.md";

    /// <summary>某一半一輪結束：它負責的交棒檔有新版本就交棒（或依狀態暫停／完成）。</summary>
    private void CheckHandoff(CoworkGroup g, TerminalTab writer)
    {
        if (!ReferenceEquals(writer.Cowork, g)) return;
        bool fromClaude = ReferenceEquals(writer, g.First);
        string name = HandoffFileName(g, writer);
        string path = Path.Combine(writer.WorkDir, ".ai", "handoff", name);
        if (!File.Exists(path)) return;   // 這一輪沒要交棒（例如在問使用者問題）
        DateTime mtime;
        string text;
        try
        {
            mtime = File.GetLastWriteTimeUtc(path);
            using var fs = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            using var sr = new StreamReader(fs, System.Text.Encoding.UTF8, detectEncodingFromByteOrderMarks: true);
            text = sr.ReadToEnd();
        }
        catch (Exception ex) { Diag.Log($"cowork read {name}: {ex.Message}"); return; }
        if (mtime <= (fromClaude ? g.HandledToCodexUtc : g.HandledToClaudeUtc)) return;   // 這個版本處理過了
        if (fromClaude) g.HandledToCodexUtc = mtime; else g.HandledToClaudeUtc = mtime;

        if (System.Text.RegularExpressions.Regex.IsMatch(text, @"^\s*STATUS:\s*DONE\b",
                System.Text.RegularExpressions.RegexOptions.Multiline | System.Text.RegularExpressions.RegexOptions.IgnoreCase))
        {
            g.Done = true; g.Paused = true; g.PendingFrom = null;
            Diag.Log($"cowork {g.Key}: STATUS DONE in {name}");
            g.First.RaiseCoworkState();
            FlashIfInactive();
            return;
        }
        if (g.Paused || g.Round >= AppSettings.Current.CoworkMaxRounds)
        {
            g.Paused = true;
            g.PendingFrom = writer;   // 繼續交棒時補送
            Diag.Log($"cowork {g.Key}: paused, pending {name} (round {g.Round})");
            g.First.RaiseCoworkState();
            FlashIfInactive();
            return;
        }
        _ = DeliverHandoffAsync(g, writer);
    }

    /// <summary>等對方那一半閒置（連續 2 秒沒輸出）再打「請讀 …」＋Enter；最多等 10 分鐘，對方結束或拆組就放棄並暫停。</summary>
    private async Task DeliverHandoffAsync(CoworkGroup g, TerminalTab writer)
    {
        if (g.Delivering) return;
        g.Delivering = true;
        try
        {
            var target = g.Other(writer);
            string readFile = ".ai/handoff/" + HandoffFileName(g, writer);
            string writeFile = ".ai/handoff/" + HandoffFileName(g, target);
            var deadline = DateTime.UtcNow.AddMinutes(10);
            while (true)
            {
                if (!ReferenceEquals(g.First.Cowork, g) || target.Session == null) { g.Paused = true; g.PendingFrom = writer; g.First.RaiseCoworkState(); return; }
                if (g.Paused) { g.PendingFrom = writer; g.First.RaiseCoworkState(); return; }   // 等待期間被暫停
                if ((DateTime.UtcNow - target.LastOutputUtc).TotalMilliseconds >= 2000) break;
                if (DateTime.UtcNow > deadline) { g.Paused = true; g.PendingFrom = writer; g.First.RaiseCoworkState(); Diag.Log($"cowork {g.Key}: target never idle"); return; }
                await Task.Delay(500);
            }
            g.Round++;
            string msg = string.Format(Loc.T("cowork.readMsg"), readFile, writeFile, g.Round);
            Diag.Log($"cowork {g.Key}: round {g.Round} → tab {target.Id} ({readFile})");
            SendTextThenEnter(target, msg);
            g.First.RaiseCoworkState();
        }
        finally { g.Delivering = false; }
    }

    /// <summary>送一行文字進分頁、隔 RemoteEnterDelayMs 再單獨送 Enter（claude 分頁文字走 JS doPaste）。遠端送指令與協作交棒共用。
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

    /// <summary>交棒完成／暫停時，視窗不在前景就閃工作列按鈕提醒使用者（切回來自動停）。</summary>
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
    private void CoworkPause_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Cowork is not { } g) return;
        if (!g.Paused) { g.Paused = true; g.First.RaiseCoworkState(); return; }
        // 繼續：到上限就再給一輪；完成過就清掉完成狀態；暫停期間有待送的就補送
        g.Paused = false;
        g.Done = false;
        if (g.Round >= AppSettings.Current.CoworkMaxRounds) g.Round = AppSettings.Current.CoworkMaxRounds - 1;
        var pending = g.PendingFrom;
        g.PendingFrom = null;
        g.First.RaiseCoworkState();
        if (pending != null && ReferenceEquals(pending.Cowork, g)) _ = DeliverHandoffAsync(g, pending);
    }

    private void CoworkReset_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Cowork is not { } g) return;
        g.Round = 0;
        g.Done = false;
        g.First.RaiseCoworkState();
    }
}
