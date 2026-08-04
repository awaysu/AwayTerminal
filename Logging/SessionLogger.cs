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

    public string FilePath { get; }

    public SessionLogger(string path, bool timestamp, bool append)
    {
        FilePath = path;
        var dir = Path.GetDirectoryName(path);
        if (!string.IsNullOrEmpty(dir)) Directory.CreateDirectory(dir);
        _writer = new StreamWriter(path, append, new UTF8Encoding(true)) { AutoFlush = true };
        _timestamp = timestamp;
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
        text = AnsiRegex().Replace(text, "");
        text = text.Replace("\r\n", "\n");
        text = CtrlRegex().Replace(text, ""); // 去掉殘餘控制碼（保留 \n \t）
        if (text.Length == 0) return;

        lock (_lock)
        {
            try
            {
                if (!_timestamp)
                {
                    _writer.Write(text);
                    return;
                }
                foreach (char c in text)
                {
                    if (_atLineStart)
                    {
                        _writer.Write($"[{DateTime.Now:yy-MM-dd HH:mm:ss}] ");
                        _atLineStart = false;
                    }
                    _writer.Write(c);
                    if (c == '\n') _atLineStart = true;
                }
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
