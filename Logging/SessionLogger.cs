using System.IO;
using System.Text;
using System.Text.RegularExpressions;

namespace AwayTerminal.Logging;

/// <summary>把分頁輸出寫成純文字 log（濾掉 ANSI 控制碼，可選每行加時間戳）。</summary>
public sealed partial class SessionLogger : IDisposable
{
    private readonly StreamWriter _writer;
    private readonly bool _timestamp;
    private bool _atLineStart = true;
    private readonly object _lock = new();
    private bool _disposed;
    // 跨 chunk 保留 UTF-8 狀態：中文字被讀取邊界切開時，一次性 GetString 會寫出 �
    private readonly Decoder _utf8 = Encoding.UTF8.GetDecoder();
    // 跨 chunk 保留「還沒收完的 ESC 序列」：ESC[3 | 2m 被讀取邊界切開時，regex 兩半都對不到，log 裡就留下 [32m 之類的碎片
    private string _escCarry = "";

    public string FilePath { get; }

    public SessionLogger(string path, bool timestamp, bool append)
    {
        FilePath = path;
        var dir = Path.GetDirectoryName(path);
        if (!string.IsNullOrEmpty(dir)) Directory.CreateDirectory(dir);
        _writer = new StreamWriter(path, append, new UTF8Encoding(true));   // 每個 chunk 手動 Flush 一次（AutoFlush＋逐字 Write＝每個字一次 WriteFile）
        _timestamp = timestamp;
    }

    /// <summary>把尾端「還沒收完」的 ESC 序列切下來留到下一個 chunk（上限 4K，超過就當作不是序列照寫）。</summary>
    internal static string SplitIncompleteEscape(ref string text, Regex ansi)
    {
        int esc = text.LastIndexOf('\x1b');
        if (esc < 0 || text.Length - esc > 4096) return "";
        // OSC（ESC ] … BEL／ESC \）要看到終止符才算收完：`ESC ]` 本身也符合 regex 的兩字元 Fe 分支，會被誤判成完整、把 `]0;title` 留在 log 裡
        bool osc = esc + 1 < text.Length && text[esc + 1] == ']';
        bool oscOpen = osc && text.IndexOf('\x07', esc) < 0 && text.IndexOf("\x1b\\", esc, StringComparison.Ordinal) < 0;
        if (!oscOpen)
        {
            var m = ansi.Match(text, esc);
            if (m.Success && m.Index == esc) return "";   // 最後一個 ESC 序列是完整的
        }
        string tail = text.Substring(esc);
        text = text.Substring(0, esc);
        return tail;
    }

    [GeneratedRegex(@"\x1b\][\s\S]*?(?:\x07|\x1b\\)|\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]")]
    private static partial Regex AnsiRegex();

    [GeneratedRegex(@"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")]
    private static partial Regex CtrlRegex();

    public void Write(byte[] data)
    {
        if (_disposed) return;
        var chars = new char[_utf8.GetCharCount(data, 0, data.Length, false)];
        _utf8.GetChars(data, 0, data.Length, chars, 0, false);
        string text = new string(chars);

        lock (_lock)
        {
            try
            {
                text = _escCarry + text;
                _escCarry = SplitIncompleteEscape(ref text, AnsiRegex());
                text = AnsiRegex().Replace(text, "");
                text = text.Replace("\r\n", "\n");
                text = CtrlRegex().Replace(text, ""); // 去掉殘餘控制碼（保留 \n \t）
                if (text.Length == 0) return;

                if (!_timestamp)
                {
                    _writer.Write(text);
                }
                else
                {
                    // 整個 chunk 先組好再寫一次（claude 串流每秒幾十 KB，逐字寫＝每秒上萬次 WriteFile、還在餵畫面的那條執行緒上）
                    var sb = new StringBuilder(text.Length + 64);
                    foreach (char c in text)
                    {
                        if (_atLineStart)
                        {
                            sb.Append('[').Append(DateTime.Now.ToString("yy-MM-dd HH:mm:ss")).Append("] ");
                            _atLineStart = false;
                        }
                        sb.Append(c);
                        if (c == '\n') _atLineStart = true;
                    }
                    _writer.Write(sb.ToString());
                }
                _writer.Flush();
            }
            catch { }
        }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        lock (_lock)
        {
            try { _writer.Flush(); _writer.Dispose(); } catch { }
        }
    }
}
