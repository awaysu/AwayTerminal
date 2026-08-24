using System.Net.Http;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace AwayTerminal.Services;

/// <summary>check_update 回來的資訊（只留程式用得到的欄位）。</summary>
public sealed class UpdateInfo
{
    public string LatestVersion = "";
    public bool UpdateAvailable;
    /// <summary>最新一版的更新說明（可能多行；沒有就是空字串）。</summary>
    public string ReleaseNotes = "";
    /// <summary>軟體頁（下載按鈕開這個，讓使用者自己挑安裝版/免安裝版）。</summary>
    public string PageUrl = "";
}

/// <summary>awaysu.cc/software 的「檢查更新」：GET api.php?action=check_update
/// （公開 API，不需密碼；規格見 software-web 的 readme_for_program.txt）。
/// 版本比較交給伺服器（PHP version_compare），程式端只看 update_available。
/// 任何失敗（沒網路、逾時、回 ok:false、JSON 壞掉）一律回 null，由呼叫端決定要不要提示。</summary>
public static class UpdateChecker
{
    private const string Api = "https://www.awaysu.cc/software/api.php";
    private const string AppSlug = "awayterminal";           // 網站上的「參數代號」
    private const string FallbackPage = "https://www.awaysu.cc/software/awayterminal";

    /// <param name="currentVersion">目前版本，例如 1.0.43（不必帶 v）。</param>
    public static async Task<UpdateInfo?> CheckAsync(string currentVersion, CancellationToken ct = default)
    {
        try
        {
            string url = $"{Api}?action=check_update&app={AppSlug}&platform=windows" +
                         $"&version={Uri.EscapeDataString(currentVersion)}";
            using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(10) };
            http.DefaultRequestHeaders.UserAgent.ParseAdd("AwayTerminal/" + currentVersion);
            string json = await http.GetStringAsync(url, ct).ConfigureAwait(false);

            using var doc = JsonDocument.Parse(json);
            var r = doc.RootElement;
            if (r.ValueKind != JsonValueKind.Object) return null;
            if (!r.TryGetProperty("ok", out var ok) || ok.ValueKind != JsonValueKind.True) return null;

            var info = new UpdateInfo
            {
                LatestVersion = Str(r, "latest_version"),
                ReleaseNotes = Str(r, "release_notes"),
                PageUrl = Str(r, "page_url")
            };
            if (string.IsNullOrWhiteSpace(info.PageUrl)) info.PageUrl = FallbackPage;
            // update_available 由伺服器算好；沒帶（舊版網站）就自己比一次
            info.UpdateAvailable = r.TryGetProperty("update_available", out var ua) && ua.ValueKind != JsonValueKind.Null
                ? ua.ValueKind == JsonValueKind.True
                : Compare(info.LatestVersion, currentVersion) > 0;
            return string.IsNullOrWhiteSpace(info.LatestVersion) ? null : info;
        }
        catch { return null; }   // 網路失敗不吵使用者，呼叫端顯示「檢查失敗」即可
    }

    private static string Str(JsonElement o, string name)
        => o.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? (v.GetString() ?? "") : "";

    /// <summary>版本比較（a&gt;b 回正數）：逐段比數字，段數不同時補 0；-beta 之類後綴視為比正式版小。</summary>
    private static int Compare(string a, string b)
    {
        (int[] nums, bool pre) Parse(string s)
        {
            s = (s ?? "").Trim().TrimStart('v', 'V');
            int cut = s.IndexOfAny(new[] { '-', '+' });
            bool pre = cut >= 0;
            if (cut >= 0) s = s.Substring(0, cut);
            var nums = s.Split('.')
                .Select(x => int.TryParse(x, out int n) ? n : 0).ToArray();
            return (nums, pre);
        }
        var (na, pa) = Parse(a);
        var (nb, pb) = Parse(b);
        for (int i = 0; i < Math.Max(na.Length, nb.Length); i++)
        {
            int x = i < na.Length ? na[i] : 0, y = i < nb.Length ? nb[i] : 0;
            if (x != y) return x.CompareTo(y);
        }
        return pa == pb ? 0 : (pa ? -1 : 1);
    }
}
