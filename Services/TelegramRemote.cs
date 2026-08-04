using System.Net.Http;
using System.Text;
using System.Text.Json;

namespace AwayTerminal.Services;

/// <summary>遠端主機橋接：Telegram 服務透過此介面操作分頁。實作端（MainWindow）須自行 marshal 到 UI 執行緒。</summary>
public interface IRemoteHost
{
    /// <summary>依分頁列順序回傳目前分頁快照。</summary>
    IReadOnlyList<RemoteTabInfo> SnapshotTabs();
    /// <summary>把文字送進指定分頁（enter=true 時附帶 Enter）。分頁不存在/未就緒回 false。</summary>
    bool SendInputToTab(int tabId, string text, bool enter);
    /// <summary>送控制鍵（ctrl-c/ctrl-d/esc/tab/enter/up/down/left/right）。</summary>
    bool SendKeyToTab(int tabId, string keyName);
    /// <summary>取指定分頁最後 n 行的近期可讀輸出（已去 ANSI）。</summary>
    string GetRecentText(int tabId, int lines);
    /// <summary>/new 用：列出可開啟的連線標籤（每種一個：PowerShell(桌面)/SSH/Telnet/COM/ADB + 未隱藏的自訂連線）。</summary>
    IReadOnlyList<string> ListConnections();
    /// <summary>依 ListConnections 的 1-based 編號開啟連線；回 (新分頁 id, 標題)，失敗 (0, "")。</summary>
    (int Id, string Title) OpenConnection(int index);
    /// <summary>/ssh 用：host 含 user@ 直接啟動 ssh.exe；否則走「login as:」。回 (id, 標題)，失敗 (0, "")。</summary>
    (int Id, string Title) OpenSsh(string host, int port);
    /// <summary>/telnet 用：直接連線。回 (id, 標題)，失敗 (0, "")。</summary>
    (int Id, string Title) OpenTelnet(string host, int port);
    /// <summary>/history 用：最近連線標籤（最新在前，最多 10 筆）。</summary>
    IReadOnlyList<string> ListHistory();
    /// <summary>依 ListHistory 的 1-based 編號重開；回 (id, 標題)，失敗 (0, "")。</summary>
    (int Id, string Title) OpenHistory(int index);
    /// <summary>/shot 用：抓取終端機畫面 PNG（若指定分頁未顯示會先切到前景）。</summary>
    Task<byte[]?> CaptureTabPngAsync(int tabId);
    /// <summary>/close 用：真正關閉分頁（走既有優雅結束流程；不跳 PC 端確認框）。分頁不存在回 false。</summary>
    bool CloseTab(int tabId);
}

/// <summary>分頁快照（給遠端列表/狀態用）。</summary>
public sealed record RemoteTabInfo(int Id, string Title, bool Busy, string Kind);

/// <summary>
/// Telegram 遠端控制。自寫 HttpClient long polling（getUpdates/sendMessage），無第三方相依。
/// 一台 PC 一個 bot：只認設定裡「允許的 chat id」，其他人一律忽略。
/// </summary>
public sealed class TelegramRemote
{
    private readonly IRemoteHost _host;
    private readonly HttpClient _http = new() { Timeout = TimeSpan.FromSeconds(50) };
    private CancellationTokenSource? _cts;
    private long _offset;

    private string _token = "";
    private long _chatId;
    private volatile bool _notify;

    // 目前附著的分頁 id（/goto 設定；0=未選）；_follow：送指令後與分頁完成時自動回傳輸出
    private int _currentTabId;
    private bool _follow = true;
    private int _newPendingCount;   // /new 列表待選中（>0 = 下一個純數字是選連線，值=清單長度）

    // /more 翻頁：上次 SendLast 的瘦身後全文＋「已顯示區段」起點（往前翻頁用）
    private string _moreBuf = "";
    private int _morePos;
    private int _moreTabId;

    // 「只推新輸出」基準：每個分頁上次推播（或附著）當下的原始渲染文字；
    // follow/完成推播只推基準之後的新行，比對不到（清屏/TUI 重繪）退回快照。
    private readonly Dictionary<int, string> _baseline = new();

    // 10 分鐘閒置自動離開分頁檢視（只離開檢視，分頁照跑；9 分鐘先警告一次）
    private DateTime _lastUserMsgUtc = DateTime.UtcNow;
    private bool _idleWarned;

    public TelegramRemote(IRemoteHost host) => _host = host;

    public bool IsRunning => _cts != null;

    public void Start(string token, long chatId, bool notify)
    {
        Stop();
        _token = token?.Trim() ?? "";
        _chatId = chatId;
        _notify = notify;
        _lastUserMsgUtc = DateTime.UtcNow;
        if (string.IsNullOrEmpty(_token) || _chatId == 0) return;
        _cts = new CancellationTokenSource();
        _ = PollLoop(_cts.Token);
    }

    public void Stop()
    {
        try { _cts?.Cancel(); } catch { }
        _cts = null;
    }

    /// <summary>分頁忙→閒事件（由 UI 執行緒呼叫，fire-and-forget）。
    /// 附著中的分頁：follow 開啟時直接推最終輸出（例如 claude 做完的回覆）；
    /// 其他分頁：notify 開啟時推一行閒置通知。</summary>
    public void OnTabIdle(int tabId, string title)
    {
        if (_cts == null) return;
        // auto:true → 算不出新輸出時整則不送（自動推播不需要「（尚無新輸出）」這種空訊息）
        // 註：一定要丟到執行緒池——SendLast 在第一個 await 前會同步呼叫 GetRecentText，
        // 它內部以 tcs.Task.Wait 等 WebView2 的 a…text 回覆；本方法由 UI 執行緒（狀態輪詢）呼叫，
        // 直接跑會把 UI 卡住 1.5 秒、回覆又需要 UI 執行緒處理 → 必逾時退回劣化的位元組流備援。
        if (tabId == _currentTabId && _follow)
        { _ = Task.Run(() => SendLast(30, $"🟢 {title} 完成", incremental: true, auto: true)); return; }
        if (_notify) _ = SendAsync($"🟢 {title} 閒置（完成）");
    }

    // ---------- long polling ----------
    private async Task PollLoop(CancellationToken ct)
    {
        await PrimeOffset(ct);                       // 跳過啟動前累積的舊訊息，避免重播
        await RegisterCommandsAsync(ct);             // 註冊原生 / 指令選單
        await SendAsync("AwayTerminal 遠端已連線 ✅  /help 看指令");
        while (!ct.IsCancellationRequested)
        {
            try
            {
                await CheckIdleAsync();   // getUpdates 最多阻塞 30s → 每輪至少檢查一次閒置
                string url = $"https://api.telegram.org/bot{_token}/getUpdates?timeout=30&offset={_offset}";
                using var resp = await _http.GetAsync(url, ct);
                if (!resp.IsSuccessStatusCode) { await Task.Delay(3000, ct); continue; }
                string json = await resp.Content.ReadAsStringAsync(ct);
                using var doc = JsonDocument.Parse(json);
                if (!doc.RootElement.TryGetProperty("result", out var result)) continue;
                foreach (var upd in result.EnumerateArray())
                {
                    if (upd.TryGetProperty("update_id", out var uid)) _offset = uid.GetInt64() + 1;
                    HandleUpdate(upd);
                }
            }
            catch (OperationCanceledException) { break; }
            catch { try { await Task.Delay(3000, ct); } catch { break; } }
        }
    }

    /// <summary>啟動時先把既有累積訊息的 offset 推到最新，避免一開機就重播舊指令。</summary>
    private async Task PrimeOffset(CancellationToken ct)
    {
        try
        {
            using var resp = await _http.GetAsync($"https://api.telegram.org/bot{_token}/getUpdates?timeout=0", ct);
            if (!resp.IsSuccessStatusCode) return;
            string json = await resp.Content.ReadAsStringAsync(ct);
            using var doc = JsonDocument.Parse(json);
            if (doc.RootElement.TryGetProperty("result", out var result))
                foreach (var upd in result.EnumerateArray())
                    if (upd.TryGetProperty("update_id", out var uid)) _offset = uid.GetInt64() + 1;
        }
        catch { }
    }

    private void HandleUpdate(JsonElement upd)
    {
        // inline 按鈕點擊
        if (upd.TryGetProperty("callback_query", out var cq))
        {
            long cbChat = 0;
            if (cq.TryGetProperty("message", out var cm) &&
                cm.TryGetProperty("chat", out var cchat) && cchat.TryGetProperty("id", out var ccid))
                cbChat = ccid.GetInt64();
            if (cbChat != _chatId) return;           // 安全：只認允許的 chat id
            string cbId = cq.TryGetProperty("id", out var idEl) ? idEl.GetString() ?? "" : "";
            string data = cq.TryGetProperty("data", out var dEl) ? dEl.GetString() ?? "" : "";
            _ = HandleCallback(cbId, data);
            return;
        }

        if (!upd.TryGetProperty("message", out var m)) return;
        if (!m.TryGetProperty("text", out var t)) return;
        long chatId = 0;
        if (m.TryGetProperty("chat", out var chat) && chat.TryGetProperty("id", out var cid))
            chatId = cid.GetInt64();
        if (chatId != _chatId) return;               // 安全：只認允許的 chat id
        string text = (t.GetString() ?? "").Trim();
        _ = HandleCommand(text);
    }

    /// <summary>inline 按鈕分派。data 格式：goto:n / new:n / hist:n / opt:{tabId}:{n} / close:{tabId} / noop</summary>
    private async Task HandleCallback(string cbId, string data)
    {
        try
        {
            _lastUserMsgUtc = DateTime.UtcNow;   // 按鈕也算活動
            _idleWarned = false;
            if (data == "noop") { await AnswerCallback(cbId, "已取消"); return; }
            await AnswerCallback(cbId);

            var p = data.Split(':', 2);
            string kind = p[0];
            string arg = p.Length > 1 ? p[1] : "";
            switch (kind)
            {
                case "goto": await DoGoto(arg); break;
                case "new":
                {
                    var labels = _host.ListConnections();   // 重新快照，確保索引對得上
                    if (int.TryParse(arg, out int ni)) await OpenFromList(ni, labels.Count);
                    break;
                }
                case "hist":
                {
                    var labels = _host.ListHistory();
                    if (int.TryParse(arg, out int hi)) await OpenFromHistory(hi, labels.Count);
                    break;
                }
                case "opt":
                {
                    var q = arg.Split(':');
                    if (q.Length == 2 && int.TryParse(q[0], out int tid) && int.TryParse(q[1], out int on))
                    {
                        if (tid != _currentTabId) { await SendAsync("已不在該分頁，先 /goto 回去再選。"); break; }
                        if (!await TryAnswerMenu(on)) await SendAsync("畫面上已沒有選單（可能已經回答過了）。");
                    }
                    break;
                }
                case "close":
                {
                    if (!int.TryParse(arg, out int cid2)) break;
                    var t2 = _host.SnapshotTabs().FirstOrDefault(x => x.Id == cid2);
                    if (t2 == null || !_host.CloseTab(cid2)) { await SendAsync("分頁已不存在。"); break; }
                    if (cid2 == _currentTabId) _currentTabId = 0;
                    await SendAsync($"已關閉 [{t2.Title}]。");
                    break;
                }
            }
        }
        catch { }
    }

    private async Task HandleCommand(string text)
    {
        try
        {
            if (text.Length == 0) return;
            _lastUserMsgUtc = DateTime.UtcNow;   // 任何來訊都算活動（閒置計時歸零）
            _idleWarned = false;

            if (!text.StartsWith("/"))
            {
                // /new 列表待選：下一個純數字 = 選連線開啟（優先於一切，含選單應答）
                if (_newPendingCount > 0 && int.TryParse(text, out int ni))
                { await OpenFromList(ni, _newPendingCount); return; }
                _newPendingCount = 0;

                if (_currentTabId == 0) { await SendAsync("尚未選擇分頁。先 /list 看分頁，再 /goto <編號>，或 /new 開新連線。"); return; }

                // 選擇題應答：畫面上是「❯ N. 選項」選單 → 純數字自動換算成 ↑/↓+Enter（也可點推播附的按鈕）
                if (int.TryParse(text, out int optNum) && optNum >= 1 && optNum <= 9 &&
                    await TryAnswerMenu(optNum))
                    return;

                // 純文字 = 送到目前分頁
                if (!_host.SendInputToTab(_currentTabId, text, true))
                { await SendAsync("分頁已關閉或尚未就緒，請重新 /goto。"); _currentTabId = 0; return; }
                if (_follow) { await Task.Delay(700); await SendLast(15, incremental: true); }
                return;
            }

            var parts = text.Split(new[] { ' ' }, 2, StringSplitOptions.RemoveEmptyEntries);
            string cmd = parts[0].TrimStart('/').ToLowerInvariant();
            string arg = parts.Length > 1 ? parts[1].Trim() : "";
            if (cmd != "new") _newPendingCount = 0;

            switch (cmd)
            {
                case "help":
                case "start": await SendAsync(HelpText()); break;
                case "list":
                case "status": await SendTabList(); break;
                case "where": await SendWhere(); break;
                case "goto": await DoGoto(arg); break;
                case "exit": _currentTabId = 0; await SendAsync("已離開分頁檢視（分頁仍在執行）。"); break;
                case "new": await DoNew(arg); break;
                case "history": await DoHistory(arg); break;
                case "ssh": await DoSsh(arg); break;
                case "telnet": await DoTelnet(arg); break;
                case "shot": await DoShot(); break;
                case "last": await SendLast(int.TryParse(arg, out int n) && n > 0 ? n : 20); break;
                case "more": await DoMore(); break;
                case "close": await DoClose(arg); break;
                case "key": await DoKey(arg); break;
                case "stop": await DoKey("ctrl-c"); break;
                case "notify":
                    _notify = arg.Equals("on", StringComparison.OrdinalIgnoreCase) || arg == "1";
                    await SendAsync("通知：" + (_notify ? "開" : "關")); break;
                case "follow":
                    _follow = !arg.Equals("off", StringComparison.OrdinalIgnoreCase) && arg != "0";
                    await SendAsync("自動回傳輸出：" + (_follow ? "開" : "關")); break;
                default: await SendAsync("未知指令。/help 看全部。"); break;
            }
        }
        catch { /* 遠端為 best-effort，單一指令失敗不影響輪詢 */ }
    }

    private async Task DoNew(string arg)
    {
        var labels = _host.ListConnections();
        if (labels.Count == 0) { await SendAsync("沒有可用的連線。"); return; }
        if (int.TryParse(arg, out int idx)) { await OpenFromList(idx, labels.Count); return; }
        var sb = new StringBuilder("選擇要開啟的連線（點按鈕，或回覆數字）：\n");
        var btns = new List<(string Text, string Data)>();
        for (int i = 0; i < labels.Count; i++)
        {
            sb.Append($"{i + 1}: {labels[i]}\n");
            btns.Add(($"{i + 1} {Trunc(labels[i], 14)}", $"new:{i + 1}"));
        }
        _newPendingCount = labels.Count;
        await SendAsync(sb.ToString(), buttons: BtnRows(btns, 2));
    }

    /// <summary>/ssh [user@]主機[:埠]。不帶參數用上次主機；含 user@ 直接連（跳過 login as:）。</summary>
    private async Task DoSsh(string arg)
    {
        var s = AppSettings.Current;
        string host = arg; int port = 0;
        ParseHostPort(ref host, ref port);
        if (string.IsNullOrWhiteSpace(host)) host = s.LastHost;
        if (port <= 0) port = s.LastSshPort > 0 ? s.LastSshPort : 22;
        if (string.IsNullOrWhiteSpace(host))
        { await SendAsync("用法：/ssh [user@]主機[:埠]（不帶參數時用上次連過的主機）"); return; }

        var (id, title) = _host.OpenSsh(host, port);
        if (id == 0) { await SendAsync("開啟失敗。"); return; }
        _currentTabId = id;   // 開完直接附著
        if (!host.Contains('@'))
        { await SendAsync($"已開啟並進入 [{title}]。login as: → 直接回覆帳號，接著照畫面提示回覆密碼。"); return; }
        await SendAsync($"已開啟並進入 [{title}]。照畫面提示回覆密碼。");
        if (_follow) { await Task.Delay(1500); await SendLast(12, incremental: true); }
    }

    /// <summary>/telnet 主機[:埠]。不帶參數用上次主機。</summary>
    private async Task DoTelnet(string arg)
    {
        var s = AppSettings.Current;
        string host = arg; int port = 0;
        ParseHostPort(ref host, ref port);
        if (string.IsNullOrWhiteSpace(host)) host = s.LastHost;
        if (port <= 0) port = s.LastTelnetPort > 0 ? s.LastTelnetPort : 23;
        if (string.IsNullOrWhiteSpace(host))
        { await SendAsync("用法：/telnet 主機[:埠]（不帶參數時用上次連過的主機）"); return; }

        var (id, title) = _host.OpenTelnet(host, port);
        if (id == 0) { await SendAsync("開啟失敗。"); return; }
        _currentTabId = id;   // 開完直接附著
        await SendAsync($"已開啟並進入 [{title}]。");
        if (_follow) { await Task.Delay(1500); await SendLast(12, incremental: true); }   // 推開場畫面（伺服器登入提示）
    }

    /// <summary>解析「[user@]host[:port]」；沒寫 :port 時不動 port（IPv6 不支援）。</summary>
    private static void ParseHostPort(ref string host, ref int port)
    {
        host = (host ?? "").Trim();
        int i = host.LastIndexOf(':');
        if (i > 0 && int.TryParse(host[(i + 1)..], out int p) && p > 0) { port = p; host = host[..i]; }
    }

    private async Task OpenFromList(int idx, int max)
    {
        _newPendingCount = 0;
        if (idx < 1 || idx > max) { await SendAsync($"編號要在 1~{max} 之間。"); return; }
        var (id, title) = _host.OpenConnection(idx);
        if (id == 0) { await SendAsync("開啟失敗。"); return; }
        await AttachAndReport(id, title);
    }

    /// <summary>/history：只顯示最近連線（最新在前）；/history <編號> 才用該筆開新連線。
    /// 刻意不做「回覆數字選取」——純數字要留給終端機輸入與選單應答。</summary>
    private async Task DoHistory(string arg)
    {
        var labels = _host.ListHistory();
        if (labels.Count == 0) { await SendAsync("還沒有連線紀錄。"); return; }
        if (int.TryParse(arg, out int idx)) { await OpenFromHistory(idx, labels.Count); return; }
        var sb = new StringBuilder("最近連線（點按鈕或 /history <編號> 用該筆開新連線）：\n");
        var btns = new List<(string Text, string Data)>();
        for (int i = 0; i < labels.Count; i++)
        {
            sb.Append($"{i + 1}: {labels[i]}\n");
            btns.Add(($"{i + 1} {Trunc(labels[i], 14)}", $"hist:{i + 1}"));
        }
        await SendAsync(sb.ToString(), buttons: BtnRows(btns, 2));
    }

    private async Task OpenFromHistory(int idx, int max)
    {
        if (idx < 1 || idx > max) { await SendAsync($"編號要在 1~{max} 之間。"); return; }
        var (id, title) = _host.OpenHistory(idx);
        if (id == 0) { await SendAsync("開啟失敗。"); return; }
        await AttachAndReport(id, title);
    }

    /// <summary>開完連線的共同收尾：附著、SSH 提示回帳號、其餘推開場畫面。</summary>
    private async Task AttachAndReport(int id, string title)
    {
        _currentTabId = id;   // 開完直接附著

        // SSH 剛開啟（login as: 階段）→ 提示直接回覆帳號（之後照畫面提示回覆密碼）
        if (_host.SnapshotTabs().FirstOrDefault(t => t.Id == id)?.Kind == "Ssh")
        { await SendAsync($"已開啟並進入 [{title}]。login as: → 直接回覆帳號，接著照畫面提示回覆密碼。"); return; }

        await SendAsync($"已開啟並進入 [{title}]。直接打字送指令，/last 看輸出。");
        if (_follow) { await Task.Delay(1500); await SendLast(12, incremental: true); }   // 推開場畫面（telnet/COM 登入提示、shell 提示列）
    }

    private async Task DoShot()
    {
        var png = await _host.CaptureTabPngAsync(_currentTabId);
        if (png == null || png.Length == 0) { await SendAsync("截圖失敗（畫面尚未就緒）。"); return; }
        await SendPhotoAsync(png);
    }

    private async Task DoGoto(string arg)
    {
        var tabs = _host.SnapshotTabs();
        if (tabs.Count == 0) { await SendAsync("目前沒有開啟的分頁。"); return; }
        if (!int.TryParse(arg, out int idx) || idx < 1 || idx > tabs.Count)
        { await SendAsync("用法：/goto <編號>\n\n" + TabListText()); return; }
        var tab = tabs[idx - 1];
        _currentTabId = tab.Id;
        _baseline[tab.Id] = _host.GetRecentText(tab.Id, 400);   // 附著即定基準：之後只推新輸出
        await SendAsync($"已進入 [{idx}] {tab.Title}。直接打字送指令，/last 看輸出，/exit 離開。");
    }

    private async Task DoKey(string arg)
    {
        if (_currentTabId == 0) { await SendAsync("尚未選擇分頁。先 /goto。"); return; }
        if (string.IsNullOrWhiteSpace(arg))
        { await SendAsync("用法：/key <ctrl-c|ctrl-d|esc|tab|enter|up|down|left|right>"); return; }
        if (!_host.SendKeyToTab(_currentTabId, arg.ToLowerInvariant()))
        { await SendAsync("送鍵失敗（分頁關閉或按鍵名稱錯誤）。"); return; }
        if (_follow) { await Task.Delay(500); await SendLast(15, incremental: true); }
    }

    /// <summary>推畫面給手機。incremental=true（follow/完成推播）：只推「上次推播基準」之後的新輸出，
    /// 比對不到基準（清屏/TUI 大改/首次）自動退回快照；false（/last 指令）：固定「最後 n 行」快照。
    /// 兩種模式推完都把基準更新到現在，之後的推播不會重複舊內容。</summary>
    /// <param name="auto">true＝自動推播（忙轉閒），沒有新輸出就完全不送，避免手機收到空訊息。
    /// false＝使用者主動要（/last、送鍵後回傳），即使沒有新輸出也要回一句，否則看起來像壞了。</param>
    private async Task SendLast(int lines, string? header = null, bool incremental = false, bool auto = false)
    {
        if (_currentTabId == 0)
        {
            if (!auto) await SendAsync("尚未選擇分頁。先 /goto。");
            return;
        }
        string raw = _host.GetRecentText(_currentTabId, 400);
        string? diff = incremental && _baseline.TryGetValue(_currentTabId, out var b) ? DiffNew(b, raw) : null;
        _baseline[_currentTabId] = raw;

        // 先抓一大段、瘦身去雜訊後才取行，行數額度才不會被 spinner 雜訊吃掉
        string allFull = TidyForPhone(raw);
        string body = diff != null ? TidyForPhone(diff) : allFull;
        if (string.IsNullOrWhiteSpace(body))
        {
            // 自動推播（忙轉閒）沒有新輸出 → 整則不送。舊版會送出只有「🟢 xxx 完成」
            // 而沒有內容的空訊息，是手機端噪音的主要來源之一。
            if (auto) return;
            if (diff != null) { await SendAsync(header ?? "（尚無新輸出）"); return; }
            await SendAsync(header ?? "（暫無輸出）"); _moreBuf = ""; return;
        }
        string txt;
        if (diff != null) txt = body;   // 增量：整段新輸出（超過 3500 字由 Clip 取尾）
        else
        {
            var arr = body.Split('\n');
            int start = Math.Max(0, arr.Length - lines);
            txt = string.Join("\n", arr, start, arr.Length - start);
        }
        string sent = Clip(txt, 3500);
        // /more 翻頁狀態：以全文為緩衝；sent 是全文尾端（增量的瘦身結果與全文瘦身可能微差 → 取 max 保護）
        _moreBuf = allFull; _moreTabId = _currentTabId; _morePos = Math.Max(0, allFull.Length - sent.Length);
        string head = header == null ? "" : EscHtml(header) + "\n";
        // 畫面是選擇題（❯ N. 選單）→ 附上選項按鈕，點了直接選
        var opts = MenuOptions(sent);
        var btns = opts.Count >= 2
            ? BtnRows(opts.Select(o => (o.Label, $"opt:{_currentTabId}:{o.Num}")), 3)
            : null;
        await SendAsync(head + "<pre>" + EscHtml(sent) + "</pre>", html: true, buttons: btns);
    }

    /// <summary>找出 cur 相對 baseline 的新增輸出（終端機往下捲：cur = […基準尾端…][新行]）。
    /// 錨點=基準尾端最多 3 個非空行；因 prompt 行打字後會「原行變長」（PS&gt; → PS&gt; ls），
    /// 先用「前幾行完整比對＋最後一行前綴比對」定位，錨點行有變化就把該行算進新輸出。
    /// 完全比對不到回 null（呼叫端退回快照模式）。</summary>
    private static string? DiffNew(string baseline, string cur)
    {
        if (string.IsNullOrWhiteSpace(baseline)) return null;
        cur = cur.Replace("\r", "");
        var bl = baseline.Replace("\r", "").Split('\n');
        // 錨點=基準尾端「連續」的最後 3 行（保留中間空行、只去尾端空行）——
        // 跳行拼接會與實際畫面的相鄰關係不符，多行比對就永遠對不上。
        int end = bl.Length;
        while (end > 0 && bl[end - 1].Trim().Length == 0) end--;
        if (end == 0) return cur;   // 基準是空畫面 → 全部都是新的
        var anchor = new List<string>();
        for (int i = Math.Max(0, end - 3); i < end; i++) anchor.Add(bl[i]);
        string lastOld = anchor[^1];

        int lineStart = -1;
        if (anchor.Count >= 2)
        {
            // 多行錨點：前幾行完整出現、且下一行以 lastOld 開頭（prompt 打了指令仍能對上）
            string headBlock = string.Join("\n", anchor.GetRange(0, anchor.Count - 1)) + "\n";
            int from = cur.Length - 1;
            while (from >= 0)
            {
                int h = cur.LastIndexOf(headBlock, from, StringComparison.Ordinal);
                if (h < 0) break;
                int ls = h + headBlock.Length;
                if (cur.Length - ls >= lastOld.Length &&
                    string.CompareOrdinal(cur, ls, lastOld, 0, lastOld.Length) == 0)
                { lineStart = ls; break; }
                from = h - 1;
            }
        }
        if (lineStart < 0)
        {
            // 單行錨點退路：最後一個「行首以 lastOld 開頭」的位置
            int p = cur.LastIndexOf("\n" + lastOld, StringComparison.Ordinal);
            if (p >= 0) lineStart = p + 1;
            else if (cur.StartsWith(lastOld, StringComparison.Ordinal)) lineStart = 0;
            else return null;
        }
        int lineEnd = cur.IndexOf('\n', lineStart);
        string lineNow = lineEnd < 0 ? cur[lineStart..] : cur[lineStart..lineEnd];
        if (lineNow == lastOld)   // 錨點行沒變 → 新輸出從下一行開始
            return lineEnd < 0 ? "" : cur[(lineEnd + 1)..];
        return cur[lineStart..];  // 行變長（prompt 上打了指令）→ 含此行一起推
    }

    /// <summary>從畫面文字解析選擇題選項（「N. 文字」行；需有選單導航列才視為選單）。</summary>
    private static List<(int Num, string Label)> MenuOptions(string screen)
    {
        var res = new List<(int, string)>();
        if (!System.Text.RegularExpressions.Regex.IsMatch(screen, @"to navigate|Enter to select")) return res;
        var seen = new HashSet<int>();
        foreach (System.Text.RegularExpressions.Match m in
                 System.Text.RegularExpressions.Regex.Matches(screen, @"(?m)^\s*(?:❯\s*)?(\d{1,2})\.\s+(.+)$"))
        {
            int n = int.Parse(m.Groups[1].Value);
            if (n is < 1 or > 12 || !seen.Add(n)) continue;
            res.Add((n, $"{n} {Trunc(m.Groups[2].Value.Trim(), 12)}"));
        }
        return res;
    }

    /// <summary>/more：把上次 /last（含自動推播）沒顯示到的更早內容往前翻一頁（約 3000 字/頁）。</summary>
    private async Task DoMore()
    {
        if (_moreBuf.Length == 0 || _moreTabId != _currentTabId)
        { await SendAsync("先 /last 拿到最新輸出，再用 /more 往前翻。"); return; }
        if (_morePos <= 0)
        { await SendAsync("已經到最舊的內容了（畫面緩衝約 400 行）。"); return; }
        int end = _morePos;
        int start = Math.Max(0, end - 3000);
        if (start > 0)
        {
            int nl = _moreBuf.IndexOf('\n', start);
            if (nl >= 0 && nl < end) start = nl + 1;   // 對齊行首，避免頁面從半行開始
        }
        _morePos = start;
        string tag = start == 0 ? "（已到最舊）" : "（/more 繼續往前）";
        await SendAsync($"…往前一頁 {tag}\n<pre>" + EscHtml(_moreBuf[start..end].TrimEnd('\n')) + "</pre>", html: true);
    }

    /// <summary>/close：真正關閉分頁（優雅結束）。無參數=關目前附著的分頁；/close <編號>=關 /list 的第 n 個。</summary>
    private async Task DoClose(string arg)
    {
        var tabs = _host.SnapshotTabs();
        int targetId; string title;
        if (int.TryParse(arg, out int idx))
        {
            if (tabs.Count == 0) { await SendAsync("目前沒有開啟的分頁。"); return; }
            if (idx < 1 || idx > tabs.Count) { await SendAsync($"編號要在 1~{tabs.Count} 之間。/list 看編號。"); return; }
            targetId = tabs[idx - 1].Id; title = tabs[idx - 1].Title;
        }
        else
        {
            if (_currentTabId == 0) { await SendAsync("尚未附著分頁。/close <編號> 指定要關哪個，或先 /goto。"); return; }
            var cur = tabs.FirstOrDefault(t => t.Id == _currentTabId);
            if (cur == null) { _currentTabId = 0; await SendAsync("分頁已不存在。"); return; }
            targetId = cur.Id; title = cur.Title;
        }
        // 誤觸保險：先出確認按鈕，點「確定關閉」（callback close:{id}）才真的關
        await SendAsync($"確定要關閉 [{title}] 嗎？（會結束該分頁執行中的程式）",
            buttons: new List<List<(string Text, string Data)>>
            { new() { ("✅ 確定關閉", $"close:{targetId}"), ("✖ 取消", "noop") } });
    }

    /// <summary>附著中且 10 分鐘沒收到任何訊息 → 自動離開分頁檢視（分頁照跑）；9 分鐘先警告一次。</summary>
    private async Task CheckIdleAsync()
    {
        if (_currentTabId == 0) return;
        var idle = DateTime.UtcNow - _lastUserMsgUtc;
        if (idle >= TimeSpan.FromMinutes(10))
        {
            _currentTabId = 0; _idleWarned = false;
            await SendAsync("閒置 10 分鐘，已自動離開分頁檢視（分頁仍在執行）。/list 或 /goto 可再進入。");
        }
        else if (idle >= TimeSpan.FromMinutes(9) && !_idleWarned)
        {
            _idleWarned = true;
            await SendAsync("⏳ 已閒置 9 分鐘，再 1 分鐘沒動作將自動離開分頁檢視（分頁不會被關閉）。");
        }
    }

    // 手機閱讀瘦身：去 TUI 雜訊（分隔線、狀態列、spinner、空提示符）、去行尾空白、收斂連續空行
    private static readonly System.Text.RegularExpressions.Regex[] NoiseLineRes =
    {
        new(@"^[─━═╌╍]{3,}$"),                                   // 輸入框分隔線（原寬 = 終端機整行）
        new(@"^⏵⏵ "),                                            // bypass permissions 狀態列
        new(@"^⧉"),                                              // artifact / desktop 提示列
        new(@"^❯\s*$"),                                          // 空的輸入提示符
        new(@"^[·✢✣✤✥✳✶✻✽＊*+]+$"),                              // spinner 殘影（如 ✢*✶）
        new(@"^[·✢✣✤✥✳✶✻✽*+\s]*\S{1,30}…\s*(\(.*)?$"),           // spinner 動詞行（Inferring… / ✢ Inferring… (40s · ↓ 3.3…）
        new(@"^\(\d+s( ·.*)?$"),                                 // spinner 括號統計殘段（(40s · ↓ 3.3）
        new(@"^[·✢✣✤✥✳✶✻✽*+]\s?[A-Za-z()\[\]-]{0,15}$"),         // spinner 字形+短英文殘字（✣ Sock-hop / ✻ Sock- / * B）
        new(@"^[a-z][a-z-]{0,5}$"),                              // 小寫短碎片（hopp / k-ho / illo / oing）
        new(@"^(Mon|Tue|Wed|Thu|Fri|Sat|Sun) (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) +\d"), // 狀態列日期
        new(@"^⎿\s+Tip: "),                                      // 小提示列
        new(@"^❯\s*Try """),                                     // 輸入框的建議 placeholder
        new(@"^[◉⏸]\s"),                                         // 狀態列右側（◉ xhigh · /effort / ⏸ manual mode…）
        new(@"^You've used \d+% of your (weekly|session) limit"), // 用量提示列
        new(@"^[│┌┐└┘├┤╭╮╰╯─╴╶\s]+$"),                           // 方框邊線（claude 歡迎框/選擇題框等）
        new(@"✂.*hidden"),                                       // 選擇題摺疊提示（✂ 7 lines hidden）
        new(@"^Notes: press n"),                                 // 選擇題備註提示
        new(@"^Chat about this$"),                               // 選擇題聊天提示
        new(@"Enter to select|↑/↓ to navigate|Esc to cancel"),   // 選單導航列
        new(@"\(ctrl\+o to expand\)$"),                          // ctrl+o 展開提示行
        new(@"^[☐☑]\s"),                                         // 選擇題分頁標頭（☐ 遊戲風格）
        new(@"^│|│$"),                                           // 方框內容行（頭/尾帶 │ 邊框）
        new(@"[╭╮╰╯]"),                                          // 含方框角字元的行（歡迎框標題+邊線合併行）
        new(@"^Welcome back .{0,30}!$"),                         // claude 歡迎行
        new(@"^[A-Za-z]$"),                                      // 單一字母殘字（F）
        new(@"^[一-鿿]$"),                               // 單一中文字殘字（選單導航動畫殘影）
        new(@"^[▀-▟\s]+$"),                            // 方塊字元行（claude logo 殘影）
        new(@"^[·✢✣✤✥✳✶✻✽*+\s]*\S+ for (\d+m )?\d+s$"),          // 完成計時行（✻ Crunched for 3s / 1m 36s）
        new(@"^\d{1,2}:\d{2}( \|.*)?$"),                         // 狀態列時間/用量行（5:03 / 2:04 | Fable 5 | …）
        new(@"thinking with \S+ effort\)?$"),                    // spinner 統計折行尾段（…thinking with xhigh effort）
        new(@"^\(?thought for \d+m? ?\d*s\)?$"),                 // 思考計時殘段（thought for 7s)）
        new(@"↓ [\d.]+k? tokens"),                               // spinner token 統計（· ↓ 3.0k tokens …）
        new(@"\| 5h: \d|\| 7d: \d|resets in \d"),                // 狀態列用量截斷段（9 | Fable 5 | 5h: 51）
        new(@"^⎿\s+[a-z]\S{0,14}$"),                             // ⎿ 小寫短殘字（⎿ cleaned；保留 ⎿ Added/Read/(No output)）
        new(@"^[\d()\[\]{}.,;:'""-]{1,3}$"),                     // 孤立碎片行（) 2 3 …重繪殘留）
    };

    // 選擇題/確認框的內容行：剝邊框後保留（❯ 選中項、「1. 」編號選項、問句）
    private static readonly System.Text.RegularExpressions.Regex QuestionLineRe =
        new(@"^❯|^\d{1,2}\.\s|[?？]$", System.Text.RegularExpressions.RegexOptions.Compiled);

    private static string TidyForPhone(string s)
    {
        var outLines = new List<string>();
        bool prevBlank = true;
        foreach (var raw in s.Replace("\r", "").Split('\n'))
        {
            string line = raw.TrimEnd();
            string bare = line.TrimStart();
            // │ 邊框行：若內文是「問句/選項」（claude 選擇題）→ 剝框保留；其他（歡迎框等）走既有規則丟棄
            if (bare.Length > 0 && (bare[0] == '│' || bare[^1] == '│'))
            {
                string inner = bare.Trim('│', ' ');
                if (inner.Length > 0 && QuestionLineRe.IsMatch(inner))
                { outLines.Add("  " + inner); prevBlank = false; continue; }
            }
            if (bare.Length > 0 && Array.Exists(NoiseLineRes, re => re.IsMatch(bare))) continue;
            bool blank = line.Length == 0;
            if (blank && prevBlank) continue;
            outLines.Add(line);
            prevBlank = blank;
        }
        // 前綴去重：TUI 逐格重繪會留下同一行在不同寬度的截斷殘影（● PowerSh / ● PowerShell(Sta / 完整行）。
        // 以「空白收斂」後比較，某行是另一較長行的前綴 → 殘影，刪掉。
        var keys = new List<string>(outLines.Count);
        foreach (var l in outLines)
            keys.Add(System.Text.RegularExpressions.Regex.Replace(l.Trim(), @"\s+", " "));
        for (int i = outLines.Count - 1; i >= 0; i--)
        {
            if (keys[i].Length < 4) continue;                    // 太短的另有碎片規則
            for (int j = 0; j < outLines.Count; j++)
            {
                if (j == i || keys[j].Length <= keys[i].Length) continue;
                if (keys[j].StartsWith(keys[i], StringComparison.Ordinal))
                { outLines.RemoveAt(i); keys.RemoveAt(i); break; }
            }
        }
        while (outLines.Count > 0 && outLines[^1].Length == 0) outLines.RemoveAt(outLines.Count - 1);
        return string.Join("\n", outLines);
    }

    private static string EscHtml(string s) =>
        s.Replace("&", "&amp;").Replace("<", "&lt;").Replace(">", "&gt;");

    private async Task SendWhere()
    {
        if (_currentTabId == 0) { await SendAsync("目前未選擇分頁。/list 看分頁。"); return; }
        var tabs = _host.SnapshotTabs();
        for (int i = 0; i < tabs.Count; i++)
            if (tabs[i].Id == _currentTabId)
            { await SendAsync($"目前在 [{i + 1}] {tabs[i].Title} {(tabs[i].Busy ? "🟠忙" : "🟢閒")}"); return; }
        _currentTabId = 0;
        await SendAsync("分頁已關閉。/list 重新選。");
    }

    /// <summary>畫面上是「❯ N.」選單時把選擇換算成 ↑/↓+Enter（claude 選單不吃數字鍵）。不是選單回 false。</summary>
    private async Task<bool> TryAnswerMenu(int optNum)
    {
        if (_currentTabId == 0) return false;
        string screen = _host.GetRecentText(_currentTabId, 60);
        var marks = System.Text.RegularExpressions.Regex.Matches(screen, @"❯\s*(\d{1,2})\.");
        if (marks.Count == 0 ||
            !System.Text.RegularExpressions.Regex.IsMatch(screen, @"to navigate|Enter to select"))
            return false;
        int cur = int.Parse(marks[^1].Groups[1].Value);   // 取最後一個 ❯（最下方=目前的選單）
        int delta = optNum - cur;
        for (int i = 0; i < Math.Abs(delta); i++)
        { _host.SendKeyToTab(_currentTabId, delta > 0 ? "down" : "up"); await Task.Delay(120); }
        await Task.Delay(150);
        _host.SendKeyToTab(_currentTabId, "enter");
        await SendAsync($"已選擇 {optNum}。");
        if (_follow) { await Task.Delay(900); await SendLast(15, incremental: true); }
        return true;
    }

    /// <summary>/list：分頁清單＋每個分頁一顆 goto 按鈕。</summary>
    private async Task SendTabList()
    {
        var tabs = _host.SnapshotTabs();
        if (tabs.Count == 0) { await SendAsync("目前沒有開啟的分頁。/new 開新連線。"); return; }
        var sb = new StringBuilder("目前分頁（點按鈕進入）：\n");
        var btns = new List<(string Text, string Data)>();
        for (int i = 0; i < tabs.Count; i++)
        {
            sb.Append($"{i + 1}: {tabs[i].Title} {(tabs[i].Busy ? "🟠" : "🟢")}");
            if (tabs[i].Id == _currentTabId) sb.Append("  ← 目前");
            sb.Append('\n');
            btns.Add(($"{i + 1} {Trunc(tabs[i].Title, 14)} {(tabs[i].Busy ? "🟠" : "🟢")}", $"goto:{i + 1}"));
        }
        await SendAsync(sb.ToString(), buttons: BtnRows(btns, 2));
    }

    private string TabListText()
    {
        var tabs = _host.SnapshotTabs();
        if (tabs.Count == 0) return "目前沒有開啟的分頁。";
        var sb = new StringBuilder("目前分頁：\n");
        for (int i = 0; i < tabs.Count; i++)
        {
            sb.Append($"{i + 1}: {tabs[i].Title} {(tabs[i].Busy ? "🟠" : "🟢")}");
            if (tabs[i].Id == _currentTabId) sb.Append("  ← 目前");
            sb.Append('\n');
        }
        sb.Append("\n/goto <編號> 進入，/help 看全部指令");
        return sb.ToString();
    }

    private static string HelpText() =>
        "AwayTerminal 遠端指令\n" +
        "/list  列出分頁（🟢閒 🟠忙）\n" +
        "/goto <n>  進入第 n 個分頁\n" +
        "/new  開新連線（每種連線列一個，回覆數字開啟；SSH 開啟後回覆帳號、密碼登入）\n" +
        "/ssh [user@]主機[:埠]  開 SSH（不帶參數用上次主機；帶 user@ 直接連、免回帳號）\n" +
        "/telnet [主機[:埠]]  開 Telnet（不帶參數用上次主機）\n" +
        "/history [編號]  最近連線紀錄；帶編號=用該筆開新連線\n" +
        "/shot  終端機畫面截圖\n" +
        "(直接打字)  送出該行 + Enter；畫面是選擇題時回數字或點訊息附的按鈕=選該選項\n" +
        "/key <名稱>  送控制鍵 ctrl-c/ctrl-d/esc/tab/enter/up/down/left/right\n" +
        "/stop  送 Ctrl+C\n" +
        "/last [n]  最後 n 行輸出（預設 20）\n" +
        "/more  上一則輸出再往前翻一頁\n" +
        "/close [n]  真正關閉分頁（無參數=關目前附著的；/list 看編號）\n" +
        "/where  我在哪個分頁\n" +
        "/follow on|off  送完自動回傳輸出\n" +
        "/notify on|off  忙→閒推播\n" +
        "/exit  離開分頁檢視（不關分頁；附著後閒置 10 分鐘也會自動離開）";

    private static string Clip(string s, int max) => s.Length <= max ? s : s.Substring(s.Length - max);

    private async Task SendPhotoAsync(byte[] png)
    {
        try
        {
            using var form = new MultipartFormDataContent();
            form.Add(new StringContent(_chatId.ToString()), "chat_id");
            var bc = new ByteArrayContent(png);
            bc.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue("image/png");
            form.Add(bc, "photo", "terminal.png");
            using var _ = await _http.PostAsync($"https://api.telegram.org/bot{_token}/sendPhoto", form);
        }
        catch { }
    }

    /// <summary>向 BotFather API 註冊原生指令選單（手機打 / 會跳出可點選清單）。</summary>
    private async Task RegisterCommandsAsync(CancellationToken ct)
    {
        try
        {
            var cmds = new[]
            {
                new { command = "list",   description = "列出分頁（🟢閒 🟠忙）" },
                new { command = "goto",   description = "進入第 n 個分頁" },
                new { command = "new",    description = "開新連線（每種列一個，選數字）" },
                new { command = "history", description = "最近連線紀錄（/history n 開新連線）" },
                new { command = "ssh",    description = "開 SSH（可帶 user@主機:埠）" },
                new { command = "telnet", description = "開 Telnet（可帶 主機:埠）" },
                new { command = "last",   description = "最後 n 行輸出（預設 20）" },
                new { command = "more",   description = "上一則輸出往前翻一頁" },
                new { command = "close",  description = "真正關閉分頁（可帶編號）" },
                new { command = "key",    description = "送控制鍵 ctrl-c/esc/tab/enter/方向" },
                new { command = "stop",   description = "送 Ctrl+C" },
                new { command = "shot",   description = "終端機畫面截圖" },
                new { command = "where",  description = "我在哪個分頁" },
                new { command = "follow", description = "完成自動回傳輸出 on/off" },
                new { command = "notify", description = "忙轉閒推播 on/off" },
                new { command = "exit",   description = "離開分頁檢視（不關分頁）" },
                new { command = "help",   description = "完整指令說明" },
            };
            string body = JsonSerializer.Serialize(new { commands = cmds });
            using var content = new StringContent(body, Encoding.UTF8, "application/json");
            using var _ = await _http.PostAsync($"https://api.telegram.org/bot{_token}/setMyCommands", content, ct);
        }
        catch { }
    }

    private async Task SendAsync(string text, bool html = false, List<List<(string Text, string Data)>>? buttons = null)
    {
        try
        {
            var payload = new Dictionary<string, object> { ["chat_id"] = _chatId, ["text"] = text };
            if (html) payload["parse_mode"] = "HTML";
            if (buttons is { Count: > 0 })
                payload["reply_markup"] = new Dictionary<string, object>
                {
                    ["inline_keyboard"] = buttons.Select(row =>
                        row.Select(b => new Dictionary<string, string> { ["text"] = b.Text, ["callback_data"] = b.Data }).ToList()).ToList()
                };
            string body = JsonSerializer.Serialize(payload);
            using var content = new StringContent(body, Encoding.UTF8, "application/json");
            using var _ = await _http.PostAsync($"https://api.telegram.org/bot{_token}/sendMessage", content);
        }
        catch { }
    }

    /// <summary>回應 inline 按鈕點擊（讓按鈕停止轉圈；text 會以小型 toast 顯示）。</summary>
    private async Task AnswerCallback(string callbackId, string? text = null)
    {
        try
        {
            var payload = new Dictionary<string, object> { ["callback_query_id"] = callbackId };
            if (text != null) payload["text"] = text;
            string body = JsonSerializer.Serialize(payload);
            using var content = new StringContent(body, Encoding.UTF8, "application/json");
            using var _ = await _http.PostAsync($"https://api.telegram.org/bot{_token}/answerCallbackQuery", content);
        }
        catch { }
    }

    /// <summary>把一串按鈕折成每列 perRow 顆的鍵盤。</summary>
    private static List<List<(string Text, string Data)>> BtnRows(IEnumerable<(string Text, string Data)> btns, int perRow)
        => btns.Chunk(perRow).Select(c => c.ToList()).ToList();

    private static string Trunc(string s, int n) => s.Length <= n ? s : s[..(n - 1)] + "…";

    /// <summary>設定用：抓最近一則訊息的 chat id（讓使用者免手查）。找不到回 null。</summary>
    public static async Task<long?> TryGetLatestChatId(string token)
    {
        try
        {
            using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(10) };
            string json = await http.GetStringAsync($"https://api.telegram.org/bot{token.Trim()}/getUpdates");
            using var doc = JsonDocument.Parse(json);
            if (!doc.RootElement.TryGetProperty("result", out var result)) return null;
            long? last = null;
            foreach (var upd in result.EnumerateArray())
                if (upd.TryGetProperty("message", out var m) &&
                    m.TryGetProperty("chat", out var chat) &&
                    chat.TryGetProperty("id", out var cid))
                    last = cid.GetInt64();
            return last;
        }
        catch { return null; }
    }
}
