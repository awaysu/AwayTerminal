using System.IO;
using System.Text;
using System.Windows;
using AwayTerminal.Dialogs;
using AwayTerminal.Localization;
using AwayTerminal.Models;
using AwayTerminal.Services;
using AwayTerminal.Services.ChatRoom;

namespace AwayTerminal;

/// <summary>
/// AI 聊天室（1.2.3，使用者設計）：2～4 個 AI（ClaudeCode／Codex／OpenCode／GeminiCLI）各帶一個角色，針對使用者給的主題輪流討論。
/// <para>畫面與分頁沿用代理團隊那一套（<see cref="AgentGroup"/>、下一上 N−1 的 pane、分頁列一組一列、恢復分頁、我的最愛）；
/// 差別在於不走 <c>.ai/bus</c> 信箱，而是由 AwayTerminal 主持：輪到誰就在那一格打一行「第 N 迴輪到你」，
/// 它把發言寫成檔案，AwayTerminal 接進共用的 <c>.ai/chat/&lt;時間&gt;/transcript.md</c>，再換下一位。</para>
/// <para>迴數跑完（或右鍵「結束討論」）→ 請主持人（第 1 位）讀完紀錄寫 <c>conclusion.md</c>。
/// 某位超過 <see cref="AgentGroup.TurnTimeoutMinutes"/> 分鐘沒發言就跳過他這一迴並記進紀錄，不讓整場停住。</para>
/// </summary>
public partial class MainWindow
{
    // ---------- 開一間聊天室 ----------
    private void OpenChatRoom_Click(object sender, RoutedEventArgs e) => OpenChatRoom(null);

    /// <summary>New → AI 聊天室：先選資料夾（同其他啟動前選資料夾的連線）→ 設定視窗 → 開好之後問主題。</summary>
    private void OpenChatRoom(string? dir)
    {
        if (DeferUntilWebReady(() => OpenChatRoom(dir), "OpenChatRoom")) return;
        if (AgentGroup.NextFreeNumber(_agentGroups) == 0) { Info(Loc.T("ma.tooMany")); return; }
        dir ??= PickWorkDir(Loc.T("chat.pickDir"));
        if (dir == null) { Web.Focus(); return; }
        var dlg = new MultiAgentDialog(dir, null, GroupMode.Chat) { Owner = this };
        if (dlg.ShowDialog() != true || dlg.Result is not { } setup) { Web.Focus(); return; }
        var g = OpenAgentGroup(setup, mode: GroupMode.Chat);
        if (g == null) return;
        AddHistory(new SavedTab { Type = "chatroom", Title = Loc.T("chat.title"), Dir = setup.Dir });
        AskChatTopic(g);
    }

    /// <summary>問主題（可以按取消，之後右鍵「開始討論…」再給）。給了就寫討論紀錄的開頭並開始第 1 迴。</summary>
    private void AskChatTopic(AgentGroup g)
    {
        // 主題常常是一整段（背景、限制、想要的結論）→ 多行輸入框（使用者要求，2026-09-16）
        var dlg = new InputDialog(Loc.T("chat.title"), Loc.T("chat.topicPrompt"), g.Topic, multiline: true) { Owner = this };
        if (dlg.ShowDialog() != true || string.IsNullOrWhiteSpace(dlg.Value)) { Web.Focus(); return; }
        StartChatDiscussion(g, dlg.Value.Trim());
        Web.Focus();
    }

    private void StartChatDiscussion(AgentGroup g, string topic)
    {
        g.Topic = topic;
        g.Round = 1;
        g.Speaker = 0;
        g.EndRequested = false;
        g.TurnAskedUtc = default;
        g.Phase = ChatPhase.Discussing;
        foreach (var s in g.Slots.Where(x => x.Enabled))
        {
            try { ChatRoleLibrary.Compose(g, s); } catch (Exception ex) { Diag.Log($"chat compose {s.AgentId}: {ex.Message}"); }
        }
        var sb = new StringBuilder();
        sb.Append("# ").Append(Loc.T("chat.title")).Append(" CHAT-").Append(g.Number).Append('\n');
        sb.Append(string.Format(Loc.T("chat.trTopic"), topic)).Append('\n');
        sb.Append(string.Format(Loc.T("chat.trRounds"), g.Rounds)).Append('\n');
        foreach (var s in ChatSpeakers(g)) sb.Append($"- {s.AgentId}  {s.RoleTitle}  ({s.BackendName})\n");
        WriteTranscript(g, sb.ToString());
        Diag.Log($"chat start CHAT-{g.Number} rounds={g.Rounds} dir={g.Dir} folder={g.ChatFolder} speakers={ChatSpeakers(g).Count()}");
        g.RowTab?.RaiseAgentState();
    }

    /// <summary>參加者（依格號；第 1 位＝主持人）。</summary>
    private static IEnumerable<AgentSlot> ChatSpeakers(AgentGroup g) => g.Running.Where(s => s.Tab!.Session != null);

    private static string ChatPath(AgentGroup g, string file) =>
        Path.Combine(g.Dir, ".ai", "chat", g.ChatFolder, file);

    private static string ChatRelPath(AgentGroup g, string file) => $"{AgentGroup.ChatRelDir}/{g.ChatFolder}/{file}";

    private void WriteTranscript(AgentGroup g, string text)
    {
        try
        {
            string path = ChatPath(g, "transcript.md");
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.AppendAllText(path, text.TrimEnd() + "\n\n", new UTF8Encoding(false));
        }
        catch (Exception ex) { Diag.Log("chat transcript: " + ex.Message); }
    }

    // ---------- 主持（狀態輪詢每 0.6 秒呼叫）----------
    private void ChatRoomTick(AgentGroup g, DateTime now)
    {
        if (g.Phase is ChatPhase.NeedTopic or ChatPhase.Done || g.Paused) return;
        var speakers = ChatSpeakers(g).ToList();
        if (speakers.Count == 0) return;

        if (g.Phase == ChatPhase.Concluding)
        {
            var host = speakers[0];
            if (g.TurnAskedUtc == default)
            {
                if (!AgentReady(host, now)) return;
                SendTextThenEnter(host.Tab!, string.Format(Loc.T("chat.conclusionPrompt"),
                    g.Round - 1, ChatRelPath(g, "transcript.md"), ChatRelPath(g, "conclusion.md")));
                MarkTyped(host, now);
                g.TurnAskedUtc = now;
                Diag.Log($"chat CHAT-{g.Number}: asked {host.AgentId} for conclusion");
                return;
            }
            if (ReadFinished(ChatPath(g, "conclusion.md"), now) is not { } conclusion)
            {
                if ((now - g.TurnAskedUtc).TotalMinutes >= AgentGroup.TurnTimeoutMinutes * 2) FinishChat(g, timedOut: true);
                return;
            }
            WriteTranscript(g, $"## {Loc.T("chat.trConclusion")}（{speakers[0].AgentId} {speakers[0].RoleTitle}）\n\n{conclusion}");
            FinishChat(g, timedOut: false);
            return;
        }

        // 討論中：g.Speaker 這一位還沒發言 → 請他發言 → 等他的檔案
        if (g.Speaker >= speakers.Count) { AdvanceChatTurn(g, speakers.Count); return; }
        var slot = speakers[g.Speaker];
        string file = $"r{g.Round}-{slot.AgentId}.md";
        if (g.TurnAskedUtc == default)
        {
            if (!AgentReady(slot, now)) return;
            SendTextThenEnter(slot.Tab!, string.Format(Loc.T("chat.turnPrompt"), g.Round, g.Rounds, slot.RoleTitle,
                ChatRelPath(g, "transcript.md"), ChatRelPath(g, file)));
            MarkTyped(slot, now);
            g.TurnAskedUtc = now;
            Diag.Log($"chat CHAT-{g.Number}: round {g.Round}/{g.Rounds} -> {slot.AgentId} ({slot.RoleTitle})");
            g.RowTab?.RaiseAgentState();
            return;
        }
        if (ReadFinished(ChatPath(g, file), now) is { } said)
        {
            WriteTranscript(g, $"## {string.Format(Loc.T("chat.trTurn"), g.Round, slot.AgentId, slot.RoleTitle)}\n\n{said}");
            AdvanceChatTurn(g, speakers.Count);
            return;
        }
        if ((now - g.TurnAskedUtc).TotalMinutes >= AgentGroup.TurnTimeoutMinutes)
        {
            WriteTranscript(g, string.Format(Loc.T("chat.trSkipped"), g.Round, slot.AgentId, AgentGroup.TurnTimeoutMinutes));
            Diag.Log($"chat CHAT-{g.Number}: {slot.AgentId} timed out on round {g.Round}");
            AdvanceChatTurn(g, speakers.Count);
        }
    }

    /// <summary>換下一位；一迴走完換下一迴；迴數跑完（或使用者按了結束）就去寫結論。</summary>
    private void AdvanceChatTurn(AgentGroup g, int speakerCount)
    {
        g.TurnAskedUtc = default;
        g.Speaker++;
        if (g.Speaker < speakerCount && !g.EndRequested) { g.RowTab?.RaiseAgentState(); return; }
        g.Speaker = 0;
        g.Round++;
        if (g.EndRequested || g.Round > g.Rounds)
        {
            g.Phase = ChatPhase.Concluding;
            Diag.Log($"chat CHAT-{g.Number}: discussion ended（{(g.EndRequested ? "user" : "rounds")}）→ conclusion");
        }
        g.RowTab?.RaiseAgentState();
    }

    /// <summary>讀「寫完了」的檔案（最後修改 ≥1 秒前才算寫完，避免讀到一半），讀完刪不掉也沒關係。null＝還沒有／還在寫。</summary>
    private static string? ReadFinished(string path, DateTime now)
    {
        try
        {
            if (!File.Exists(path)) return null;
            if ((now - File.GetLastWriteTimeUtc(path)).TotalMilliseconds < 1000) return null;
            string text = File.ReadAllText(path, Encoding.UTF8).Trim();
            return text.Length == 0 ? null : text;
        }
        catch { return null; }
    }

    private void FinishChat(AgentGroup g, bool timedOut)
    {
        g.Phase = ChatPhase.Done;
        g.TurnAskedUtc = default;
        Diag.Log($"chat CHAT-{g.Number}: finished{(timedOut ? " (conclusion timed out)" : "")}");
        g.RowTab?.RaiseAgentState();
        FlashIfInactive();
    }

    // ---------- 分頁右鍵 ----------
    /// <summary>「開始討論…／換主題…」：給主題（討論中會先確認）。</summary>
    private void ChatTopic_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Agent?.Group is not { } g || !g.IsChat) return;
        if (g.Phase == ChatPhase.Discussing &&
            MessageBox.Show(this, Loc.T("chat.newTopicAsk"), Loc.T("chat.title"), MessageBoxButton.YesNo, MessageBoxImage.Question)
            != MessageBoxResult.Yes) return;
        if (g.Phase is ChatPhase.Discussing or ChatPhase.Concluding or ChatPhase.Done)
            g.ChatFolder = NewChatFolder();   // 換主題＝開新的討論紀錄
        AskChatTopic(g);
    }

    /// <summary>「結束討論」：這一輪結束後就請主持人寫結論。</summary>
    private void ChatEnd_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Agent?.Group is not { } g || !g.IsChat) return;
        if (g.Phase != ChatPhase.Discussing) return;
        g.EndRequested = true;
        WriteTranscript(g, Loc.T("chat.trUserEnd"));
        Diag.Log($"chat CHAT-{g.Number}: end requested by user at round {g.Round}");
        g.RowTab?.RaiseAgentState();
    }

    /// <summary>「插話…」：把使用者的一段話接進討論紀錄，下一位發言的人就看得到。</summary>
    private void ChatSay_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Agent?.Group is not { } g || !g.IsChat) return;
        var dlg = new InputDialog(Loc.T("chat.title"), Loc.T("chat.sayPrompt"), "", multiline: true) { Owner = this };
        if (dlg.ShowDialog() != true || string.IsNullOrWhiteSpace(dlg.Value)) { Web.Focus(); return; }
        WriteTranscript(g, $"## {Loc.T("chat.trUserSaid")}\n\n{dlg.Value.Trim()}");
        Diag.Log($"chat CHAT-{g.Number}: user comment added");
        Web.Focus();
    }

    /// <summary>「開啟討論紀錄資料夾」。</summary>
    private void ChatOpenFolder_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Agent?.Group is not { } g || !g.IsChat) return;
        string dir = Path.Combine(g.Dir, ".ai", "chat", g.ChatFolder);
        try { Directory.CreateDirectory(dir); System.Diagnostics.Process.Start("explorer.exe", $"\"{dir}\""); }
        catch (Exception ex) { Diag.Log("chat open folder: " + ex.Message); }
    }

    internal static string NewChatFolder() => DateTime.Now.ToString("yyyyMMdd-HHmm");
}
