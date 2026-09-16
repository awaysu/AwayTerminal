using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using AwayTerminal.Dialogs;
using AwayTerminal.Localization;
using AwayTerminal.Models;
using AwayTerminal.Services;

namespace AwayTerminal;

/// <summary>
/// 工具列「我的最愛」（使用者要求，2026-09-16；取代「紀錄」按鈕，圖示 icon/favorite.png）。
/// 下拉＝各筆最愛（圖示＋名稱，點了直接開）→ 分隔線 →「加到我的最愛：目前分頁」→「設定…」（改名稱、刪除）。
/// <para>一筆最愛＝分頁的恢復資訊（SavedTab）複本：PowerShell 記目前所在目錄（提示行解析到的 CwdPath）、自訂連線記實際工作目錄
/// （重開不再跳資料夾選擇）、SSH 登入後記 user@host（重開直接連）。代理團隊記整組設定（MultiAgentSetup JSON），重開不跳設定視窗。</para>
/// <para>連線紀錄（AppSettings.History）照記，Telegram /history 仍用它。</para>
/// </summary>
public partial class MainWindow
{
    private void Favorites_Click(object sender, RoutedEventArgs e)
    {
        var menu = new ContextMenu();
        var favs = AppSettings.Current.Favorites.ToList();
        if (favs.Count == 0) menu.Items.Add(new MenuItem { Header = Loc.T("fav.empty"), IsEnabled = false });
        foreach (var f in favs)
        {
            var item = f;
            var mi = MakeNewItemRaw(item.Name, FavoriteIcon(item));
            mi.ToolTip = FavoriteDetail(item);
            mi.Click += (_, _) => OpenFavorite(item);
            menu.Items.Add(mi);
        }
        menu.Items.Add(new Separator());

        var candidate = FavoriteFromTab(_active);
        var add = MakeNewItemRaw(candidate == null ? Loc.T("fav.add") : string.Format(Loc.T("fav.addNamed"), candidate.Name), "favorite.png");
        add.IsEnabled = candidate != null;   // 沒有分頁、或這個分頁沒有可重開的資訊 → 灰掉
        if (candidate != null) add.Click += (_, _) => AddFavorite(candidate);
        menu.Items.Add(add);

        var settings = MakeNewItemRaw(Loc.T("fav.settings"), "settings.png");
        settings.Click += (_, _) => OpenFavoritesSettings();
        menu.Items.Add(settings);

        menu.PlacementTarget = (UIElement)sender;
        menu.Placement = System.Windows.Controls.Primitives.PlacementMode.Bottom;
        menu.IsOpen = true;
    }

    /// <summary>目前分頁 → 一筆最愛（沒有可重開的資訊回 null）。代理團隊＝整組。</summary>
    private FavoriteItem? FavoriteFromTab(TerminalTab? tab)
    {
        if (tab == null) return null;
        if (tab.Agent?.Group is { } g)
        {
            var setup = new MultiAgentSetup { Dir = g.Dir, MaxMessages = g.MaxMessages, IdleCheckMinutes = g.IdleCheckMinutes };
            for (int i = 0; i < 4; i++)
            {
                var s = g.Slots[i];
                setup.Slots[i] = new AgentSlotSetup { Enabled = s.Enabled && !string.IsNullOrEmpty(s.Backend), Backend = s.Backend, Role = s.Role };
            }
            return new FavoriteItem
            {
                Name = g.Title,
                Tab = new SavedTab { Type = "multiagent", Title = g.Title, Dir = g.Dir },
                TeamSetup = JsonSerializer.Serialize(setup)
            };
        }
        if (tab.Restore is not { } r) return null;
        var t = CloneTab(r);
        // PowerShell：記「現在所在」的目錄（使用者 cd 過就是 cd 之後的），不是當初開分頁的目錄
        if (t.Type == "ps" && !string.IsNullOrEmpty(tab.CwdPath) && Directory.Exists(tab.CwdPath)) t.Dir = tab.CwdPath;
        // 自訂連線有實際工作目錄 → 重開直接用它，不再跳資料夾選擇
        if (t.Type == "custom" && !string.IsNullOrEmpty(t.Dir)) t.PickDir = false;
        return new FavoriteItem { Name = tab.Title, Tab = t };
    }

    private void AddFavorite(FavoriteItem candidate)
    {
        var favs = AppSettings.Current.Favorites;
        string key = FavoriteKey(candidate);
        if (favs.FirstOrDefault(x => FavoriteKey(x) == key) is { } exist)
        {
            ShowCopyFeedback(BtnFavorites, string.Format(Loc.T("fav.exists"), exist.Name));
            return;
        }
        // 名稱重複（例如兩個資料夾都叫 AwayTerminal 的不同連線）→ 後面補 (2)、(3)，下拉裡才分得出來
        string name = string.IsNullOrWhiteSpace(candidate.Name) ? Loc.T("tb.favorites") : candidate.Name.Trim();
        string unique = name;
        for (int n = 2; favs.Any(x => x.Name == unique); n++) unique = $"{name} ({n})";
        candidate.Name = unique;
        favs.Add(candidate);
        AppSettings.Current.Save();
        Diag.Log($"favorite add {candidate.Tab.Type} '{candidate.Name}'");
        ShowCopyFeedback(BtnFavorites, string.Format(Loc.T("fav.added"), candidate.Name));
    }

    /// <summary>同一個連線（種類＋主機／路徑＋目錄）只收一筆。</summary>
    private static string FavoriteKey(FavoriteItem f)
    {
        if (!string.IsNullOrEmpty(f.TeamSetup)) return "team|" + f.Tab.Dir + "|" + f.TeamSetup;
        var e = f.Tab;
        return e.Type switch
        {
            "custom" => $"custom|{e.Path}|{e.Args}|{e.Dir}",
            "ssh" or "telnet" => $"{e.Type}|{e.Host}|{e.Port}",
            _ => HistoryKey(e)
        };
    }

    internal static string FavoriteIcon(FavoriteItem f) =>
        !string.IsNullOrEmpty(f.TeamSetup) ? "multi-agent.png" : HistoryIcon(f.Tab);

    /// <summary>下拉 tooltip／設定視窗第二行：連線種類＋位置。</summary>
    internal static string FavoriteDetail(FavoriteItem f)
    {
        var e = f.Tab;
        if (!string.IsNullOrEmpty(f.TeamSetup))
        {
            string members = "";
            try
            {
                if (JsonSerializer.Deserialize<MultiAgentSetup>(f.TeamSetup) is { } s)
                    members = string.Join("／", s.Slots.Where(x => x is { Enabled: true }).Select(x => AwayTerminal.Services.MultiAgent.AdapterRegistry.ByKey(x.Backend)?.DisplayName ?? x.Backend));
            }
            catch { }
            return $"{Loc.T("ma.title")}  {e.Dir}" + (members.Length > 0 ? $"  ({members})" : "");
        }
        return e.Type switch
        {
            "ps" => $"{Loc.T("kind.powershell")}  {e.Dir}",
            "claude" => $"{Loc.T("kind.claude")}  {e.Dir}",
            "ssh" => $"{Loc.T("kind.ssh")}  {e.Host}" + (e.Port is 0 or 22 ? "" : $":{e.Port}"),
            "telnet" => $"{Loc.T("kind.telnet")}  {e.Host}:{e.Port}",
            "com" => $"{Loc.T("kind.com")}  {e.ComPort} {e.Baud}",
            "adb" => $"{Loc.T("kind.adb")}" + (string.IsNullOrEmpty(e.AdbSerial) ? "" : $"  {e.AdbSerial}"),
            "custom" => $"{(string.IsNullOrWhiteSpace(e.Name) ? e.Title : e.Name)}  {e.Dir}".TrimEnd(),
            _ => e.Title
        };
    }

    private void OpenFavorite(FavoriteItem f)
    {
        if (DeferUntilWebReady(() => OpenFavorite(f), $"OpenFavorite {f.Tab.Type}")) return;
        Diag.Log($"favorite open {f.Tab.Type} '{f.Name}'");
        if (!string.IsNullOrEmpty(f.TeamSetup))
        {
            MultiAgentSetup? setup = null;
            try { setup = JsonSerializer.Deserialize<MultiAgentSetup>(f.TeamSetup); } catch { }
            if (setup?.Slots is not { Length: 4 }) { Info(Loc.T("fav.teamBroken")); return; }
            if (!Directory.Exists(setup.Dir)) { Info(string.Format(Loc.T("ma.dlgFolderMissing"), setup.Dir)); return; }
            if (AgentGroup.NextFreeNumber(_agentGroups) == 0) { Info(Loc.T("ma.tooMany")); return; }
            if (OpenAgentGroup(setup, title: f.Name) != null)
                AddHistory(new SavedTab { Type = "multiagent", Title = Loc.T("ma.title"), Dir = setup.Dir });
            return;
        }
        var e = CloneTab(f.Tab);
        if (e.Type == "ssh" && e.Host.Contains('@')) { OpenSshUserAtHost(e.Host, e.Port); return; }
        ReopenHistory(e);
    }

    private void OpenFavoritesSettings()
    {
        var dlg = new FavoritesDialog(AppSettings.Current.Favorites, FavoriteIcon, FavoriteDetail) { Owner = this };
        dlg.ShowDialog();
        Web.Focus();
    }
}
