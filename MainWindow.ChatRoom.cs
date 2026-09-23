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
/// 差別在於不走 <c>.ai/bus</c> 信箱，而是由 AwayTerminal 主持：輪到誰就在那一格打一行「第 N 回合輪到你」，
/// 它把發言寫成檔案，AwayTerminal 接進共用的 <c>.ai/chat/&lt;時間&gt;/transcript.md</c>，再換下一位。</para>
/// <para>回合數跑完（或右鍵「結束討論」）→ 請主持人（第 1 位）讀完紀錄寫 <c>conclusion.md</c>。
/// 某位超過 <see cref="AgentGroup.TurnTimeoutMinutes"/> 分鐘沒發言就跳過他這一回合並記進紀錄，不讓整場停住。</para>
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

    /// <summary>問主題（可以按取消，之後右鍵「開始討論…」再給）。給了就寫討論紀錄的開頭並開始第 1 回合。</summary>
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
        // 這個資料夾已經有一場討論（換主題、或恢復分頁接回來的那場）→ 開新的資料夾，舊的發言檔／結論不會被當成這一場的
        if (File.Exists(ChatPath(g, "transcript.md"))) g.ChatFolder = NewChatFolder(g.Dir);
        g.Topic = topic;
        g.Round = 1;
        g.Speaker = 0;
        g.EndRequested = false;
        g.TurnAskedUtc = default;
        g.AskedAgentId = "";
        g.TurnStartedUtc = DateTime.UtcNow;
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

    /// <summary>參加者（依格號；第 1 位＝主持人）。session 已結束的格也算在名單裡（索引才不會跳動），輪到他時直接跳過、不等 5 分鐘。</summary>
    private static IEnumerable<AgentSlot> ChatSpeakers(AgentGroup g) => g.Running;

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
            if (host.Tab!.Session == null)   // 主持人已結束：沒人能寫結論，直接收場並在紀錄註明
            {
                WriteTranscript(g, string.Format(Loc.T("chat.trHostGone"), host.AgentId));
                FinishChat(g, timedOut: true);
                return;
            }
            if (g.TurnAskedUtc == default)
            {
                if (!AgentReady(host, now))
                {
                    if (g.TurnStartedUtc != default && (now - g.TurnStartedUtc).TotalMinutes >= AgentGroup.TurnTimeoutMinutes * 2)
                        FinishChat(g, timedOut: true);   // 主持人一直忙到問不了：收場（同結論逾時）
                    return;
                }
                SendTextThenEnter(host.Tab!, string.Format(Loc.T("chat.conclusionPrompt"),
                    g.Round - 1, ChatRelPath(g, "transcript.md"), ChatRelPath(g, "conclusion.md")));
                MarkTyped(host, now);
                g.TurnAskedUtc = now;
                g.AskedAgentId = host.AgentId;
                Diag.Log($"chat CHAT-{g.Number}: asked {host.AgentId} for conclusion");
                return;
            }
            if (ReadFinished(ChatPath(g, "conclusion.md"), g.TurnAskedUtc, now) is not { } conclusion)
            {
                if ((now - g.TurnAskedUtc).TotalMinutes >= AgentGroup.TurnTimeoutMinutes * 2) FinishChat(g, timedOut: true);
                return;
            }
            WriteTranscript(g, $"## {Loc.T("chat.trConclusion")}（{host.AgentId} {host.RoleTitle}）\n\n{conclusion}");
            FinishChat(g, timedOut: false);
            return;
        }

        // 討論中。使用者按了「結束討論」而下一位還沒被問到 → 不再多問一個人，直接去寫結論
        if (g.TurnAskedUtc == default && g.EndRequested) { ConcludeChat(g, "user"); return; }
        AgentSlot slot;
        if (g.TurnAskedUtc != default)
        {
            // 問過某位、等他發言：一律先用 Agent ID 找回他（期間名單變了索引會跑掉——這要在「索引超出範圍」的判斷之前做，
            // 否則最後一位在等發言時前面有人被關掉，他的那一輪會被整個略過）；他自己被關掉了就換下一位
            int idx = speakers.FindIndex(x => x.AgentId == g.AskedAgentId);
            if (idx < 0) { AdvanceChatTurn(g, speakers.Count); return; }
            g.Speaker = idx;
            slot = speakers[idx];
        }
        else
        {
            if (g.Speaker >= speakers.Count) { AdvanceChatTurn(g, speakers.Count); return; }
            slot = speakers[g.Speaker];
        }
        string file = $"r{g.Round}-{slot.AgentId}.md";
        if (slot.Tab!.Session == null)   // 這一格已結束（問之前或問之後都一樣）：不等 5 分鐘，跳過並在紀錄註明
        {
            WriteTranscript(g, string.Format(Loc.T("chat.trEnded"), g.Round, slot.AgentId));
            Diag.Log($"chat CHAT-{g.Number}: {slot.AgentId} has exited, skipped on round {g.Round}");
            AdvanceChatTurn(g, speakers.Count);
            return;
        }
        if (g.TurnAskedUtc == default)
        {
            if (!AgentReady(slot, now))
            {
                // 一直忙碌（畫面不停重繪之類）連問都問不到：等滿逾時一樣跳過並註明，5 分鐘逾時才不會只保護「問了以後」
                if (g.TurnStartedUtc != default && (now - g.TurnStartedUtc).TotalMinutes >= AgentGroup.TurnTimeoutMinutes)
                {
                    WriteTranscript(g, string.Format(Loc.T("chat.trSkipped"), g.Round, slot.AgentId, AgentGroup.TurnTimeoutMinutes));
                    Diag.Log($"chat CHAT-{g.Number}: {slot.AgentId} never became idle on round {g.Round}, skipped");
                    AdvanceChatTurn(g, speakers.Count);
                }
                return;
            }
            SendTextThenEnter(slot.Tab!, string.Format(Loc.T("chat.turnPrompt"), g.Round, g.Rounds, slot.RoleTitle,
                ChatRelPath(g, "transcript.md"), ChatRelPath(g, file)));
            MarkTyped(slot, now);
            g.TurnAskedUtc = now;
            g.AskedAgentId = slot.AgentId;
            Diag.Log($"chat CHAT-{g.Number}: round {g.Round}/{g.Rounds} -> {slot.AgentId} ({slot.RoleTitle})");
            g.RowTab?.RaiseAgentState();
            return;
        }
        if (ReadFinished(ChatPath(g, file), g.TurnAskedUtc, now) is { } said)
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

    /// <summary>換下一位；一回合走完換下一回合；回合數跑完（或使用者按了結束）就去寫結論。</summary>
    private void AdvanceChatTurn(AgentGroup g, int speakerCount)
    {
        g.TurnAskedUtc = default;
        g.AskedAgentId = "";
        g.TurnStartedUtc = DateTime.UtcNow;
        g.Speaker++;
        if (g.Speaker < speakerCount && !g.EndRequested) { g.RowTab?.RaiseAgentState(); return; }
        g.Speaker = 0;
        g.Round++;
        if (g.EndRequested || g.Round > g.Rounds) ConcludeChat(g, g.EndRequested ? "user" : "rounds");
        else g.RowTab?.RaiseAgentState();
    }

    /// <summary>進入「寫結論」階段。Round 在這之後＝「跑完的回合數＋1」（結論提示用 Round−1 說共幾回合）。</summary>
    private void ConcludeChat(AgentGroup g, string why)
    {
        g.TurnAskedUtc = default;
        g.AskedAgentId = "";
        g.TurnStartedUtc = DateTime.UtcNow;
        if (g.Phase == ChatPhase.Discussing && g.Speaker > 0) g.Round++;   // 這一回合已經有人講過＝算一回合（從 AdvanceChatTurn 來的已經加過）
        g.Speaker = 0;
        g.Phase = ChatPhase.Concluding;
        Diag.Log($"chat CHAT-{g.Number}: discussion ended（{why}）→ conclusion");
        g.RowTab?.RaiseAgentState();
    }

    /// <summary>讀「寫完了」的檔案：一定要是 askedUtc（我們開口問）之後才寫的——恢復分頁或同一個資料夾再開一場時，上一場留下的舊檔不能當成這一場的發言；
    /// 而且最後修改 ≥1 秒前才算寫完（避免讀到一半）。null＝還沒有／還在寫／是舊檔。</summary>
    private static string? ReadFinished(string path, DateTime askedUtc, DateTime now)
    {
        try
        {
            if (!File.Exists(path)) return null;
            var written = File.GetLastWriteTimeUtc(path);
            if (written < askedUtc.AddSeconds(-2)) return null;   // 舊檔（FAT／網路磁碟的 mtime 只有 2 秒解析度，留餘裕）
            if ((now - written).TotalMilliseconds < 1000) return null;
            string text = File.ReadAllText(path, Encoding.UTF8).Trim();
            return text.Length == 0 ? null : text;
        }
        catch { return null; }
    }

    private void FinishChat(AgentGroup g, bool timedOut)
    {
        g.Phase = ChatPhase.Done;
        g.TurnAskedUtc = default;
        g.AskedAgentId = "";
        Diag.Log($"chat CHAT-{g.Number}: finished{(timedOut ? " (no conclusion)" : "")}");
        g.RowTab?.RaiseAgentState();
        FlashIfInactive();
    }

    // ---------- 分頁右鍵 ----------
    /// <summary>「開始討論…／換主題…」：給主題（討論中會先確認）。</summary>
    private void ChatTopic_Click(object sender, RoutedEventArgs e)
    {
        if (TabOf(sender)?.Agent?.Group is not { } g || !g.IsChat) return;
        if (g.Phase is ChatPhase.Discussing or ChatPhase.Concluding &&
            MessageBox.Show(this, Loc.T("chat.newTopicAsk"), Loc.T("chat.title"), MessageBoxButton.YesNo, MessageBoxImage.Question)
            != MessageBoxResult.Yes) return;
        // 新的討論紀錄資料夾由 StartChatDiscussion 在「真的給了主題」之後才開——這裡先換，使用者按取消就會讓進行中的那場指到空資料夾
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
        // 還沒有主題時插話會先建出 transcript.md，之後「開始討論」看到檔案就換新資料夾＝那句話誰也看不到；結束後插話也沒人會讀
        if (g.Phase is not (ChatPhase.Discussing or ChatPhase.Concluding)) { Info(Loc.T("chat.sayNotNow")); return; }
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

    /// <summary>這場討論的資料夾名＝現在的時間；同一分鐘內已經有一場（同資料夾開兩間、連續換主題）就補 -2、-3，不共用。</summary>
    internal static string NewChatFolder(string projectDir)
    {
        string stamp = DateTime.Now.ToString("yyyyMMdd-HHmm"), name = stamp;
        try
        {
            for (int n = 2; Directory.Exists(Path.Combine(projectDir, ".ai", "chat", name)); n++) name = $"{stamp}-{n}";
        }
        catch { }
        return name;
    }
}
