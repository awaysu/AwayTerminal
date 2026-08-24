using System.Collections.Generic;
using System.IO;
using System.Text.Json;

namespace AwayTerminal.Services;

public sealed class PromptItem
{
    public string Title { get; set; } = "";
    public string Content { get; set; } = "";
    public string Group { get; set; } = "";   // 群組名稱（空＝未分組）
    public bool SendEnter { get; set; }        // 送出後補送 Enter（指令直接執行；新項目預設不勾）
}

/// <summary>一筆自訂新連接（New 下拉的自訂項目 / 自訂管理視窗用）。</summary>
public sealed class CustomConn
{
    public string Name { get; set; } = "";
    public string Path { get; set; } = "";           // 執行檔路徑
    public string Args { get; set; } = "";           // 執行檔參數
    public string Icon { get; set; } = "run";         // 圖示 key（對應 icon\{key}.png；可為 none）
    public string CloseKey { get; set; } = "ctrl-c";  // 關閉分頁送的鍵：ctrl-c(0x03) / ctrl-d(0x04)
    public int CloseCount { get; set; } = 3;           // 關閉鍵送幾次（1~5）
    public bool PickDir { get; set; } = false;        // 啟動前選擇資料夾（工作目錄）
    public bool Hidden { get; set; } = false;         // 隱藏（不列在 New 下拉）
    public bool ViaPowerShell { get; set; } = false;  // 透過 PowerShell 執行（.cmd/需 shell 時用）
}

/// <summary>關閉時儲存的分頁（下次開啟恢復用），同時作為「紀錄」歷史項目。</summary>
public sealed class SavedTab
{
    public string Type { get; set; } = "ps"; // ps | claude | ssh | telnet | com | adb | custom
    public string Title { get; set; } = "";
    public string Dir { get; set; } = "";
    public string Host { get; set; } = "";
    public int Port { get; set; }
    public string ComPort { get; set; } = "";
    public int Baud { get; set; } = 115200;
    public int DataBits { get; set; } = 8;
    public string Parity { get; set; } = "None";
    public string StopBits { get; set; } = "One";
    public string Flow { get; set; } = "None";
    // adb / custom 重開用
    public string AdbSerial { get; set; } = "";
    /// <summary>自訂連線的「連線名稱」（Title 在關閉存檔時會被改成分頁標題「名稱(1)」，恢復時
    /// 不能再拿它當名稱——否則會變成「名稱(1)(1)」）。舊紀錄沒有此欄 → 退回 Title。</summary>
    public string Name { get; set; } = "";
    public string Path { get; set; } = "";             // custom：執行檔；adb：當初用的 adb.exe
    public string Args { get; set; } = "";
    public string Icon { get; set; } = "";
    public bool PickDir { get; set; }
    public bool ViaPowerShell { get; set; }
    public string CloseKey { get; set; } = "ctrl-c";
    public int CloseCount { get; set; } = 3;
}

/// <summary>整個程式的設定與歷史，存成一個 JSON（%LOCALAPPDATA%\AwayTerminal\settings.json）。</summary>
public sealed class AppSettings
{
    // PowerShell 目錄
    public string? LastDir { get; set; }
    public List<string> DirBookmarks { get; set; } = new();

    // SSH / Telnet
    public string LastConnType { get; set; } = "ssh"; // ssh | telnet
    public string LastUser { get; set; } = "";
    public string LastHost { get; set; } = "";
    public int LastSshPort { get; set; } = 22;
    public int LastTelnetPort { get; set; } = 23;
    public List<string> HostHistory { get; set; } = new();
    // SSH/Telnet/COM 斷線自動重連（連線視窗勾選、預設不勾）
    public bool AutoReconnect { get; set; } = false;
    // SSH/Telnet 保持連線間隔（分鐘；0=關；下拉 0/1/3/5/10/15/30/60）：SSH=ServerAliveInterval、Telnet=IAC NOP
    public int KeepAliveMins { get; set; } = 10;

    // COM（第一次預設 COM5 / 115200 / 8 / none / 1 / none）
    public string ComPort { get; set; } = "COM5";
    public int ComBaud { get; set; } = 115200;
    public int ComDataBits { get; set; } = 8;
    public string ComParity { get; set; } = "None";  // None/Odd/Even/Mark/Space
    public string ComStopBits { get; set; } = "One"; // One/Two/OnePointFive
    public string ComFlow { get; set; } = "None";    // None/XOnXOff/RequestToSend/RequestToSendXOnXOff

    // 常用 prompt
    public List<PromptItem> Prompts { get; set; } = new();

    // 字體 / 背景
    public string FontFamily { get; set; } = "Cascadia Mono";
    public int FontSize { get; set; } = 14;
    public string Foreground { get; set; } = "#E0E0E0";
    public string Background { get; set; } = "#1E1E1E";

    /// <summary>claude 分頁「注音整段／貼上／backspace」送出前，等 claude 輸出靜止的門檻（ms；1.0.43）。
    /// 目的：別在 claude 正在重繪上一筆輸入時插入下一筆，降低「二倍字串」與 backspace 游標偏／殘影。
    /// 只在 claude 忙碌重繪時才會延遲；閒置打字時 0 延遲。0=關閉閘門（立即送）。合理範圍 0~150。</summary>
    public int ImeQuietMs { get; set; } = 20;

    // Log
    public string LogDir { get; set; } = "";
    public bool LogTimestamp { get; set; } = true;
    public bool LogAppend { get; set; } = true;

    // 語言
    public string Language { get; set; } = "zh"; // zh | en

    // 功能列位置：top（上方橫排）| right（右側直欄）
    public string ToolbarPosition { get; set; } = "top";

    // Claude 路徑 / 參數（ClaudeCode 按鈕選完目錄後自動執行）
    public const string DefaultClaudeCommand = "claude.exe --dangerously-skip-permissions"; // 舊版相容用
    public string ClaudeCommand { get; set; } = DefaultClaudeCommand;                        // 已停用，保留避免舊 JSON 出錯
    public const string DefaultClaudeArgs = "--dangerously-skip-permissions";
    public string ClaudePath { get; set; } = "";              // claude 執行檔完整路徑（設定裡挑）
    public string ClaudeArgs { get; set; } = DefaultClaudeArgs; // 附加參數
    // 透過 PowerShell 執行 claude（npm 版 claude.cmd 或需要 shell 的指令時勾）；預設不勾＝直接跑 exe
    public bool ClaudeViaPowerShell { get; set; } = false;
    // 啟用 Claude 按鈕（勾了工具列才顯示 ClaudeCode）；預設不勾
    public bool ClaudeEnabled { get; set; } = false;

    // adb.exe 路徑：使用者在設定裡指定時優先採用（空=自動搜尋，見 ResolveAdbPath）
    public string AdbPath { get; set; } = "";
    /// <summary>New 下拉是否顯示 ADB（設定 → ADB 的勾選框）。
    /// ADB 是內建項目、不是自訂連線，刪光自訂連線也不會消失，所以要有這個開關才能隱藏它。
    /// 註：1.0.16 以前這個旗標存在但**選單完全沒理會**、設定也沒有 UI，等同無效欄位；
    /// 1.0.17 起接上並改為預設顯示（原本預設 false，若沿用會讓全新安裝看不到 ADB）。</summary>
    public bool AdbEnabled { get; set; } = true;

    /// <summary>官方 platform-tools 下載頁（找不到 adb 時提示使用者自行安裝）。</summary>
    public const string AdbDownloadUrl = "https://developer.android.com/tools/releases/platform-tools";

    /// <summary>找出這台電腦上的 adb.exe，找不到回 null。
    /// v1.0.13 起**不再隨程式打包 Google 的 adb**——Android SDK 條款 §3.4 禁止轉散布 SDK，
    /// 與 §3.5「另有授權的開源元件」的界線並不明確，且上架 Store 需對散布內容擁有明確權利。
    /// 會用到 ADB 的人幾乎都已安裝 platform-tools，故改為搜尋既有安裝。
    /// 順序：使用者指定 → PATH → 常見 SDK 位置 → 舊版殘留的 tools\adb（升級者不會突然壞掉）。</summary>
    public static string? ResolveAdbPath()
    {
        var cfg = Current;
        if (!string.IsNullOrWhiteSpace(cfg.AdbPath) && File.Exists(cfg.AdbPath)) return cfg.AdbPath;

        foreach (var p in AdbCandidates())
            if (!string.IsNullOrWhiteSpace(p) && File.Exists(p)) return p;

        return null;
    }

    private static IEnumerable<string> AdbCandidates()
    {
        // PATH
        string pathEnv = Environment.GetEnvironmentVariable("PATH") ?? "";
        foreach (var dir in pathEnv.Split(Path.PathSeparator))
        {
            string d = dir.Trim().Trim('"');
            if (d.Length > 0) yield return Path.Combine(d, "adb.exe");
        }

        // Android SDK 環境變數（Android Studio / CI 常設）
        foreach (var varName in new[] { "ANDROID_HOME", "ANDROID_SDK_ROOT" })
        {
            string? root = Environment.GetEnvironmentVariable(varName);
            if (!string.IsNullOrWhiteSpace(root)) yield return Path.Combine(root, "platform-tools", "adb.exe");
        }

        // Android Studio 預設安裝位置
        string localApp = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        yield return Path.Combine(localApp, "Android", "Sdk", "platform-tools", "adb.exe");
        foreach (var pf in new[] { Environment.SpecialFolder.ProgramFiles, Environment.SpecialFolder.ProgramFilesX86 })
            yield return Path.Combine(Environment.GetFolderPath(pf), "Android", "android-sdk", "platform-tools", "adb.exe");

        // 1.0.12 以前的安裝會在程式目錄留下 tools\adb\adb.exe——已存在於使用者電腦上的
        // 副本可以照用（我們不再散布它），升級者的 ADB 功能不會突然失效。
        yield return Path.Combine(AppContext.BaseDirectory, "tools", "adb", "adb.exe");
    }

    // 自訂新連接清單（New 下拉的自訂項目）
    public List<CustomConn> CustomConns { get; set; } = new();
    // 舊 ClaudeCode 設定是否已遷移成自訂連線（v1.0.18 起不再使用，保留以相容舊 JSON）
    public bool ClaudeMigratedToCustom { get; set; } = false;

    /// <summary>常用字串／資料夾書籤的範例是否已放過（只做一次；刪光不會再長回來）。</summary>
    public bool SeededSamples { get; set; } = false;

    // 關閉視窗兩個勾選的記憶
    public bool ExitRestoreTabs { get; set; } = true;
    public bool ExitUpdateMd { get; set; } = false;

    // 分頁列最左「…」輸入框：「送出後送 Enter」勾選的記憶
    public bool ComposeSendEnter { get; set; } = true;

    // 關閉時儲存的分頁（下次開啟恢復）
    public List<SavedTab> SavedTabs { get; set; } = new();

    // 連線紀錄（最近開過的連線，「紀錄」按鈕下拉用；最新在前）
    public List<SavedTab> History { get; set; } = new();

    // 遠端控制（Telegram；一台 PC 一個 bot）
    public bool RemoteEnabled { get; set; } = false;
    public string TelegramBotToken { get; set; } = "";  // 向 @BotFather 申請
    public long TelegramChatId { get; set; } = 0;        // 允許的 chat id（0=未設，服務不啟動）
    public bool RemoteNotify { get; set; } = false;      // 其他（未 /goto 進入的）分頁完成也推播；預設關＝「goto 後才收訊息」（1.0.33）

    /// <summary>未知欄位保留區：讀到「更新版本寫的設定」時，不認識的欄位存這裡、存檔時原樣寫回。
    /// 防「舊版存檔把新版欄位剝掉」——2026-07-27 安裝版 0.9.73 存檔就把遠端 token 整組洗掉過。</summary>
    [System.Text.Json.Serialization.JsonExtensionData]
    public Dictionary<string, JsonElement>? ExtraFields { get; set; }

    // ---------- 載入 / 儲存 ----------
    private static readonly string Dir =
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "AwayTerminal");
    private static readonly string FilePath = Path.Combine(Dir, "settings.json");
    private static readonly JsonSerializerOptions JsonOpts = new() { WriteIndented = true };

    private static AppSettings? _current;
    public static AppSettings Current => _current ??= Load();

    public static AppSettings Load()
    {
        try
        {
            if (File.Exists(FilePath))
            {
                var s = JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(FilePath));
                if (s != null) { s.EnsureDefaults(); return s; }
            }
        }
        catch
        {
            // 檔案存在但解析失敗（截斷/損毀）→ 先留一份 .bad 備份，別讓底下的預設值 Save 直接蓋掉使用者設定
            try { File.Copy(FilePath, FilePath + ".bad", true); } catch { }
        }
        var def = new AppSettings();
        def.EnsureDefaults();
        return def;
    }

    private void EnsureDefaults()
    {
        if (string.IsNullOrWhiteSpace(LogDir))
            LogDir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "AwayTerminalLogs");

        // 常用字串 / 資料夾書籤：**只在第一次建立設定檔時**放入範例。
        // 舊寫法是 `if (Count == 0)`，等於「清單一空就補回來」——使用者把範例全刪掉，
        // 下次啟動又長回來（與自訂連線那個困擾同一類）。改用一次性旗標後刪掉就是刪掉。
        if (!SeededSamples)
        {
            SeededSamples = true;

            if (Prompts.Count == 0)
            {
                Prompts.Add(new PromptItem { Group = "Linux", Title = "看檔案目錄", Content = "ls", SendEnter = true });
                Prompts.Add(new PromptItem { Group = "Linux", Title = "看linux版本", Content = "cat /proc/version", SendEnter = true });
                Prompts.Add(new PromptItem { Group = "常用Prompt", Title = "編譯器位置說明", Content = @"NDK/SDK編譯器在C:\CrossCompilerWin" });
                // 註：此為作者指定的範例內容，主機／帳號／密碼皆為虛構的示範值（非真實憑證）。
                Prompts.Add(new PromptItem { Group = "常用Prompt", Title = "ssh登入", Content = "ssh ip是192.168.50.100,帳號是user1,密碼是1234" });
            }

            if (DirBookmarks.Count == 0)
            {
                DirBookmarks.Add(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory));
                DirBookmarks.Add(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile));
            }

            Save();   // 立即持久化旗標，避免下次啟動又判定成「第一次」
        }

        if (string.IsNullOrWhiteSpace(LastDir))
            LastDir = Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);

        // 自動在 PATH 找 claude（先 exe、再 npm 的 cmd）
        if (string.IsNullOrWhiteSpace(ClaudePath))
        {
            ClaudePath = TryResolveOnPath("claude.exe");
            if (string.IsNullOrWhiteSpace(ClaudePath)) ClaudePath = TryResolveOnPath("claude.cmd");
        }

        // v1.0.18 起**不再自動建立任何自訂連線**：全新安裝的自訂清單是空的，
        // 使用者自己用「新連接 → 自訂… → 自動偵測」一鍵加入想要的工具（ClaudeCode / ADB / WSL…）。
        // 舊的 ClaudeCode 自動遷移已移除——它會讓「我明明全部刪掉了」的清單又冒出東西。
        // ClaudeMigratedToCustom 旗標保留於 JSON（相容舊檔），不再使用。
    }

    /// <summary>在 PATH 各目錄尋找可執行檔的完整路徑；找不到回傳空字串。</summary>
    public static string TryResolveOnPath(string fileName)
    {
        try
        {
            var paths = (Environment.GetEnvironmentVariable("PATH") ?? "").Split(Path.PathSeparator);
            foreach (var p in paths)
            {
                if (string.IsNullOrWhiteSpace(p)) continue;
                string full;
                try { full = Path.Combine(p.Trim(), fileName); } catch { continue; }
                if (File.Exists(full)) return full;
            }
        }
        catch { }
        return "";
    }

    public void Save()
    {
        try
        {
            Directory.CreateDirectory(Dir);
            // 先寫暫存檔再原子替換：中途被強制結束不會留下半截 JSON（半截檔會讓下次載入退回預設值）
            string tmp = FilePath + ".tmp";
            File.WriteAllText(tmp, JsonSerializer.Serialize(this, JsonOpts));
            File.Move(tmp, FilePath, overwrite: true);
        }
        catch { }
    }
}
