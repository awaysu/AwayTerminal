using System.Collections.Generic;
using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using AwayTerminal.Sessions;

namespace AwayTerminal.Macros;

/// <summary>
/// TeraTerm .ttl 巨集直譯器（擴充子集）。參考：
/// https://teratermproject.github.io/manual/5/en/macro/command/index.html
///
/// 支援：
///   通訊  sendln send sendbreak wait waitln waitn flushrecv pause mpause
///   流程  :label goto call return  if&lt;expr&gt;&lt;cmd&gt;  if..then/elseif/else/endif  for..next  while..endwhile  break continue  end exit
///   變數  a = 5 / b = 'text' / c = a + 1 ；整數運算式 + - * / % & | ^ ~ &lt;&lt; &gt;&gt; = &lt;&gt; &lt; &lt;= &gt; &gt;= && ||，字串 'x'/"x"，字元碼 #nn
///   對話  messagebox yesnobox inputbox statusbox(顯示為 messagebox)
///   字串  strlen strconcat strcopy str2int int2str strcompare tolower toupper sprintf
///   系統變數  result inputstr timeout
/// 未支援（會安靜略過）：檔案操作、檔案傳輸、大部分 set* 通訊設定。
/// 注意：break/continue 只支援獨立成行（單行式 if x break 不支援，請用區塊 if）。
/// </summary>
public sealed partial class MacroRunner
{
    private readonly ITerminalSession _session;
    private readonly string[] _lines;
    private readonly CancellationTokenSource _cts = new();
    private readonly StringBuilder _buf = new();
    private readonly object _bufLock = new();

    private readonly Dictionary<string, int> _labels = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, object> _vars = new(StringComparer.OrdinalIgnoreCase);
    private readonly Stack<int> _callStack = new();

    // 區塊配對（前處理建立）
    private readonly Dictionary<int, int> _branchNext = new();   // if/elseif → 下一個分支行（elseif/else/endif）
    private readonly Dictionary<int, int> _toEndif = new();      // elseif/else →（true 分支落下時跳）endif
    private readonly Dictionary<int, int> _loopOwner = new();    // break/continue 行 → 所屬 for/while 行
    private readonly Dictionary<int, int> _whileToEnd = new();
    private readonly Dictionary<int, int> _endToWhile = new();
    private readonly Dictionary<int, int> _forToNext = new();
    private readonly Dictionary<int, int> _nextToFor = new();
    private readonly Dictionary<int, long> _forLast = new();

    private int _pc;
    private bool _advance;
    private bool _stop;
    private bool _evalElseif;   // 執行到 elseif 行時要評估其條件（由前一個為假的分支跳過來）

    public event Action? Finished;
    public event Action<string, string>? MessageRequested;         // messagebox / statusbox
    public event Func<string, string, bool>? ConfirmRequested;     // yesnobox → true=Yes
    public event Func<string, string, string, string?>? InputRequested; // inputbox → 文字 or null(取消)

    public MacroRunner(ITerminalSession session, string[] lines)
    {
        _session = session;
        _lines = lines;
        _session.Output += OnOutput;
        _vars["result"] = 0L;
        _vars["inputstr"] = "";
        _vars["timeout"] = 0L;
    }

    [GeneratedRegex(@"\x1b\][\s\S]*?(?:\x07|\x1b\\)|\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]")]
    private static partial Regex AnsiRegex();

    private void OnOutput(byte[] data)
    {
        string t = AnsiRegex().Replace(Encoding.UTF8.GetString(data), "");
        lock (_bufLock)
        {
            _buf.Append(t);
            if (_buf.Length > 400_000) _buf.Remove(0, _buf.Length - 200_000);
        }
    }

    public void Start() => _ = Task.Run(RunAsync);
    public void Stop() { try { _cts.Cancel(); } catch { } }

    // ================= 主迴圈 =================
    private async Task RunAsync()
    {
        try
        {
            Preprocess();
            while (_pc >= 0 && _pc < _lines.Length && !_cts.IsCancellationRequested && !_stop)
            {
                _advance = true;
                await ExecLine(_lines[_pc].Trim()).ConfigureAwait(false);
                if (_advance) _pc++;
            }
        }
        catch (OperationCanceledException) { }
        catch { }
        finally
        {
            _session.Output -= OnOutput;
            try { Finished?.Invoke(); } catch { }
        }
    }

    // 前處理：收集 label 與 if/elseif/else/endif 分支鏈、for/next、while/endwhile 配對、break/continue 所屬迴圈
    private void Preprocess()
    {
        var ifStack = new Stack<List<int>>();   // 每個 if 區塊的分支行清單（if、elseif…、else）
        var forStack = new Stack<int>();
        var whileStack = new Stack<int>();
        var loopStack = new Stack<int>();       // for/while 共用巢狀堆疊（break/continue 取最近一層）

        for (int i = 0; i < _lines.Length; i++)
        {
            string line = _lines[i].Trim();
            if (line.Length == 0 || line[0] == ';') continue;
            if (line[0] == ':') { _labels[line.Substring(1).Trim()] = i; continue; }

            var (cmd, _) = SplitCmd(line);
            switch (cmd)
            {
                case "if":
                    if (line.EndsWith(" then", StringComparison.OrdinalIgnoreCase) || line.Equals("if then", StringComparison.OrdinalIgnoreCase))
                        ifStack.Push(new List<int> { i });
                    break;
                case "elseif":
                case "else":
                    if (ifStack.Count > 0)
                    {
                        var branches = ifStack.Peek();
                        _branchNext[branches[^1]] = i;
                        branches.Add(i);
                    }
                    break;
                case "endif":
                    if (ifStack.Count > 0)
                    {
                        var branches = ifStack.Pop();
                        _branchNext[branches[^1]] = i;
                        for (int b = 1; b < branches.Count; b++) _toEndif[branches[b]] = i;  // elseif/else 落下 → 跳 endif
                    }
                    break;
                case "for": forStack.Push(i); loopStack.Push(i); break;
                case "next":
                    if (forStack.Count > 0) { int f = forStack.Pop(); _forToNext[f] = i; _nextToFor[i] = f; }
                    if (loopStack.Count > 0) loopStack.Pop();
                    break;
                case "while": whileStack.Push(i); loopStack.Push(i); break;
                case "endwhile":
                    if (whileStack.Count > 0) { int w = whileStack.Pop(); _whileToEnd[w] = i; _endToWhile[i] = w; }
                    if (loopStack.Count > 0) loopStack.Pop();
                    break;
                case "break":
                case "continue":
                    if (loopStack.Count > 0) _loopOwner[i] = loopStack.Peek();
                    break;
            }
        }
    }

    // ================= 執行一行 =================
    private async Task ExecLine(string line)
    {
        if (line.Length == 0 || line[0] == ';' || line[0] == ':') return;

        // 指定：  name = ...
        var asg = Regex.Match(line, @"^([A-Za-z_]\w*)\s*=(?!=)(.*)$");
        if (asg.Success && !IsKeyword(asg.Groups[1].Value))
        {
            DoAssign(asg.Groups[1].Value, asg.Groups[2].Value.Trim());
            return;
        }

        var (cmd, rest) = SplitCmd(line);
        switch (cmd)
        {
            case "sendln": _session.Write(SendBytes(rest, true)); break;
            case "send": _session.Write(SendBytes(rest, false)); break;
            case "sendbreak": break; // 無操作（多數連線無 break 概念）

            case "pause":
                await Delay((int)EvalInt(rest) * 1000).ConfigureAwait(false); break;
            case "mpause":
                await Delay((int)EvalInt(rest)).ConfigureAwait(false); break;

            case "wait":
            case "waitln":
                _vars["result"] = (long)await WaitForAny(StrList(rest)).ConfigureAwait(false); break;
            case "waitn":
                await WaitForBytes((int)EvalInt(rest)).ConfigureAwait(false); break;
            case "flushrecv":
                lock (_bufLock) _buf.Clear(); break;

            case "messagebox":
            case "statusbox":
            {
                var a = StrList(rest);
                MessageRequested?.Invoke(a.Count > 0 ? a[0] : "", a.Count > 1 ? a[1] : "Macro");
                break;
            }
            case "yesnobox":
            {
                var a = StrList(rest);
                bool yes = ConfirmRequested?.Invoke(a.Count > 0 ? a[0] : "", a.Count > 1 ? a[1] : "Macro") ?? false;
                _vars["result"] = yes ? 1L : 0L;
                break;
            }
            case "inputbox":
            {
                var a = StrList(rest);
                string? r = InputRequested?.Invoke(a.Count > 0 ? a[0] : "", a.Count > 1 ? a[1] : "Macro", a.Count > 2 ? a[2] : "");
                _vars["inputstr"] = r ?? "";
                _vars["result"] = r == null ? 0L : 1L;
                break;
            }

            case "goto": Jump(FirstToken(rest)); break;
            case "call": _callStack.Push(_pc + 1); Jump(FirstToken(rest)); break;
            case "return": if (_callStack.Count > 0) { _pc = _callStack.Pop(); _advance = false; } break;

            case "if": await DoIf(rest, line).ConfigureAwait(false); break;
            case "elseif":
                if (_evalElseif)
                {
                    // 由前一個為假的分支跳過來 → 評估本分支條件
                    _evalElseif = false;
                    string ex = rest;
                    if (ex.EndsWith("then", StringComparison.OrdinalIgnoreCase)) ex = ex[..^4];
                    if (EvalInt(ex.Trim()) == 0) JumpNextBranch(_pc);
                    // 條件為真 → _advance 進入本體
                }
                else if (_toEndif.TryGetValue(_pc, out int endAfterElseif)) { _pc = endAfterElseif; } // true 分支落下 → 跳 endif
                break;
            case "else": // 走到 else（來自 true 分支落下）→ 跳到 endif
                if (_toEndif.TryGetValue(_pc, out int endAfterElse)) { _pc = endAfterElse; }
                break;
            case "endif": break;

            case "for": DoFor(rest); break;
            case "next": DoNext(); break;
            case "while": DoWhile(rest); break;
            case "endwhile":
                if (_endToWhile.TryGetValue(_pc, out int wpc)) { _pc = wpc; _advance = false; } break;

            case "break":
            case "continue":
                if (_loopOwner.TryGetValue(_pc, out int loopPc))
                {
                    var (loopKind, _) = SplitCmd(_lines[loopPc].Trim());
                    if (cmd == "break")
                    {
                        // 跳到 next/endwhile 行（_advance → 其下一行），整個迴圈結束
                        if (loopKind == "for" && _forToNext.TryGetValue(loopPc, out int nx)) _pc = nx;
                        else if (loopKind == "while" && _whileToEnd.TryGetValue(loopPc, out int ew)) _pc = ew;
                    }
                    else if (loopKind == "for")
                    { if (_forToNext.TryGetValue(loopPc, out int nx)) { _pc = nx; _advance = false; } } // 執行 next：遞增+回圈
                    else { _pc = loopPc; _advance = false; }   // while → 回 while 行重新評估
                }
                break;

            case "end":
            case "exit":
                _stop = true; break;

            // 字串
            case "strlen": { var a = StrList(rest); _vars["result"] = (long)(a.Count > 0 ? a[0].Length : 0); break; }
            case "strconcat": DoStrConcat(rest); break;
            case "strcopy": DoStrCopy(rest); break;
            case "int2str": DoInt2Str(rest); break;
            case "str2int": DoStr2Int(rest); break;
            case "strcompare": DoStrCompare(rest); break;
            case "tolower": DoStrCase(rest, false); break;
            case "toupper": DoStrCase(rest, true); break;
            case "sprintf": DoSprintf(rest); break;

            default: break; // 未支援指令略過
        }
    }

    // ================= 指定與運算 =================
    private void DoAssign(string name, string rhs)
    {
        rhs = rhs.Trim();
        if (rhs.Length > 0 && (rhs[0] == '\'' || rhs[0] == '"'))
        {
            _vars[name] = ParseFirstQuoted(rhs);
        }
        else if (IsIdent(rhs) && _vars.TryGetValue(rhs, out var v) && v is string sv)
        {
            _vars[name] = sv;
        }
        else
        {
            _vars[name] = EvalInt(rhs);
        }
    }

    private async Task DoIf(string rest, string fullLine)
    {
        // 區塊式：if <expr> then
        if (fullLine.EndsWith(" then", StringComparison.OrdinalIgnoreCase))
        {
            string exprPart = rest;
            int t = exprPart.LastIndexOf("then", StringComparison.OrdinalIgnoreCase);
            if (t >= 0) exprPart = exprPart.Substring(0, t);
            bool cond = EvalInt(exprPart.Trim()) != 0;
            if (cond) return; // 進入 if 主體（_advance=true → 下一行）
            JumpNextBranch(_pc); // 條件為假 → 下一個分支（elseif 重新評估 / else 進本體 / endif 跳出）
            return;
        }

        // 單行式：if <expr> <command>
        var (condStr, cmdStr) = SplitCondCmd(rest);
        if (EvalInt(condStr) != 0 && cmdStr.Length > 0)
            await ExecLine(cmdStr).ConfigureAwait(false);
    }

    /// <summary>if/elseif 條件為假時跳到下一個分支：elseif=停在該行重新評估、else=進其本體、endif=跳出區塊。</summary>
    private void JumpNextBranch(int branchPc)
    {
        if (!_branchNext.TryGetValue(branchPc, out int nx)) return;
        var (cmd, _) = SplitCmd(_lines[nx].Trim());
        if (cmd == "elseif") { _pc = nx; _advance = false; _evalElseif = true; }
        else { _pc = nx; }   // else → _advance 進 else 本體；endif → _advance 跳出
    }

    private void DoFor(string rest)
    {
        var toks = rest.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
        if (toks.Length < 3) return;
        string v = toks[0];
        long first = EvalInt(toks[1]);
        long last = EvalInt(toks[2]);
        _vars[v] = first;
        _forLast[_pc] = last;
        if (first > last && _forToNext.TryGetValue(_pc, out int nx)) { _pc = nx; } // 跳過整個 for
    }

    private void DoNext()
    {
        if (!_nextToFor.TryGetValue(_pc, out int forPc)) return;
        var toks = _lines[forPc].Trim().Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
        if (toks.Length < 2) return;
        string v = toks[1];
        long cur = GetInt(v) + 1;
        _vars[v] = cur;
        long last = _forLast.TryGetValue(forPc, out var l) ? l : cur;
        if (cur <= last) { _pc = forPc + 1; _advance = false; } // 回圈體
    }

    private void DoWhile(string rest)
    {
        if (EvalInt(rest) == 0 && _whileToEnd.TryGetValue(_pc, out int end)) { _pc = end; } // 跳出 → endwhile（+1 由主迴圈）
    }

    // ================= 字串指令 =================
    private void DoStrConcat(string rest)
    {
        var (a, b) = TwoArgs(rest);
        _vars[a] = GetStr(a) + StrOfArg(b);
    }
    private void DoStrCopy(string rest)
    {
        // strcopy <src> <pos> <len> <destvar>  或  strcopy <destvar> <src>
        var toks = SplitArgs(rest);
        if (toks.Count == 2) { _vars[toks[1]] = StrOfArg(toks[0]); }
        else if (toks.Count >= 4)
        {
            string src = StrOfArg(toks[0]);
            int pos = (int)EvalInt(toks[1]); int len = (int)EvalInt(toks[2]);
            pos = Math.Max(1, pos);
            int start = Math.Min(src.Length, pos - 1);
            len = Math.Max(0, Math.Min(len, src.Length - start));
            _vars[toks[3]] = src.Substring(start, len);
        }
    }
    private void DoInt2Str(string rest)
    {
        var (dest, val) = TwoArgs(rest);
        _vars[dest] = EvalInt(val).ToString(CultureInfo.InvariantCulture);
    }
    private void DoStr2Int(string rest)
    {
        var (dest, val) = TwoArgs(rest);
        _vars[dest] = long.TryParse(StrOfArg(val).Trim(), out long n) ? n : 0L;
    }
    private void DoStrCompare(string rest)
    {
        var (a, b) = TwoArgs(rest);
        _vars["result"] = (long)string.CompareOrdinal(StrOfArg(a), StrOfArg(b));
    }
    private void DoStrCase(string rest, bool upper)
    {
        var (dest, src) = TwoArgs(rest);
        string s = StrOfArg(src);
        _vars[dest] = upper ? s.ToUpperInvariant() : s.ToLowerInvariant();
    }
    private void DoSprintf(string rest)
    {
        // sprintf <format> <args...>  → inputstr
        var toks = SplitArgs(rest);
        if (toks.Count == 0) return;
        string fmt = StrOfArg(toks[0]);
        var sb = new StringBuilder();
        int ai = 1;
        for (int i = 0; i < fmt.Length; i++)
        {
            if (fmt[i] == '%' && i + 1 < fmt.Length)
            {
                char c = fmt[++i];
                if (c == '%') { sb.Append('%'); continue; }
                string arg = ai < toks.Count ? toks[ai++] : "";
                if (c == 'd' || c == 'i' || c == 'u') sb.Append(EvalInt(arg));
                else if (c == 'x') sb.Append(EvalInt(arg).ToString("x"));
                else if (c == 'X') sb.Append(EvalInt(arg).ToString("X"));
                else if (c == 's') sb.Append(StrOfArg(arg));
                else { sb.Append('%'); sb.Append(c); }
            }
            else sb.Append(fmt[i]);
        }
        _vars["inputstr"] = sb.ToString();
    }

    // ================= 等待 =================
    private async Task<int> WaitForAny(List<string> targets)
    {
        if (targets.Count == 0) return 0;
        int start; lock (_bufLock) start = _buf.Length;
        long secs = GetInt("timeout");
        var deadline = DateTime.UtcNow.AddSeconds(secs > 0 ? secs : 300);
        while (!_cts.IsCancellationRequested && DateTime.UtcNow < deadline)
        {
            string chunk;
            lock (_bufLock) chunk = start <= _buf.Length ? _buf.ToString(start, _buf.Length - start) : _buf.ToString();
            for (int i = 0; i < targets.Count; i++)
                if (!string.IsNullOrEmpty(targets[i]) && chunk.Contains(targets[i])) return i + 1;
            await Delay(50).ConfigureAwait(false);
        }
        return 0;
    }
    private async Task WaitForBytes(int n)
    {
        int start; lock (_bufLock) start = _buf.Length;
        long secs = GetInt("timeout");
        var deadline = DateTime.UtcNow.AddSeconds(secs > 0 ? secs : 300);
        while (!_cts.IsCancellationRequested && DateTime.UtcNow < deadline)
        {
            int have; lock (_bufLock) have = _buf.Length - start;
            if (have >= n) return;
            await Delay(50).ConfigureAwait(false);
        }
    }
    private Task Delay(int ms) => Task.Delay(Math.Max(0, ms), _cts.Token);

    // ================= 運算式（整數）=================
    private long EvalInt(string expr)
    {
        var toks = TokenizeExpr(expr);
        int pos = 0;
        long v = ParseExpr(toks, ref pos, 0);
        return v;
    }

    private static readonly string[][] Ops =
    {
        new[] { "||", "or" },
        new[] { "&&", "and" },
        new[] { "|" },
        new[] { "^" },
        new[] { "&" },
        new[] { "=", "==", "<>", "!=" },
        new[] { "<", "<=", ">", ">=" },
        new[] { "<<", ">>" },
        new[] { "+", "-" },
        new[] { "*", "/", "%" },
    };

    private long ParseExpr(List<string> toks, ref int pos, int level)
    {
        if (level >= Ops.Length) return ParseUnary(toks, ref pos);
        long left = ParseExpr(toks, ref pos, level + 1);
        while (pos < toks.Count && Array.IndexOf(Ops[level], toks[pos].ToLowerInvariant()) >= 0)
        {
            string op = toks[pos++].ToLowerInvariant();
            long right = ParseExpr(toks, ref pos, level + 1);
            left = Apply(op, left, right);
        }
        return left;
    }

    private long ParseUnary(List<string> toks, ref int pos)
    {
        if (pos < toks.Count && (toks[pos] == "-" || toks[pos] == "!" || toks[pos] == "~" || toks[pos].Equals("not", StringComparison.OrdinalIgnoreCase)))
        {
            string op = toks[pos++];
            long v = ParseUnary(toks, ref pos);
            return op == "-" ? -v : (op == "~" ? ~v : (v == 0 ? 1 : 0));
        }
        return ParseAtom(toks, ref pos);
    }

    private long ParseAtom(List<string> toks, ref int pos)
    {
        if (pos >= toks.Count) return 0;
        string t = toks[pos++];
        if (t == "(")
        {
            long v = ParseExpr(toks, ref pos, 0);
            if (pos < toks.Count && toks[pos] == ")") pos++;
            return v;
        }
        if (t.StartsWith("0x", StringComparison.OrdinalIgnoreCase) && long.TryParse(t.Substring(2), NumberStyles.HexNumber, CultureInfo.InvariantCulture, out long hx)) return hx;
        if (t.StartsWith("$") && long.TryParse(t.Substring(1), NumberStyles.HexNumber, CultureInfo.InvariantCulture, out long hx2)) return hx2;
        if (long.TryParse(t, out long n)) return n;
        return GetInt(t); // 變數
    }

    private static long Apply(string op, long a, long b) => op switch
    {
        "||" or "or" => (a != 0 || b != 0) ? 1 : 0,
        "&&" or "and" => (a != 0 && b != 0) ? 1 : 0,
        "|" => a | b,
        "^" => a ^ b,
        "&" => a & b,
        "=" or "==" => a == b ? 1 : 0,
        "<>" or "!=" => a != b ? 1 : 0,
        "<" => a < b ? 1 : 0,
        "<=" => a <= b ? 1 : 0,
        ">" => a > b ? 1 : 0,
        ">=" => a >= b ? 1 : 0,
        "<<" => a << (int)b,
        ">>" => a >> (int)b,
        "+" => a + b,
        "-" => a - b,
        "*" => a * b,
        "/" => b == 0 ? 0 : a / b,
        "%" => b == 0 ? 0 : a % b,
        _ => 0,
    };

    private static List<string> TokenizeExpr(string s)
    {
        var list = new List<string>();
        int i = 0;
        while (i < s.Length)
        {
            char c = s[i];
            if (char.IsWhiteSpace(c)) { i++; continue; }
            if (char.IsLetter(c) || c == '_')
            {
                int j = i; while (j < s.Length && (char.IsLetterOrDigit(s[j]) || s[j] == '_')) j++;
                list.Add(s.Substring(i, j - i)); i = j; continue;
            }
            if (char.IsDigit(c) || (c == '$'))
            {
                int j = i + (c == '$' ? 1 : 0);
                if (i + 1 < s.Length && c == '0' && (s[i + 1] == 'x' || s[i + 1] == 'X')) j = i + 2;
                while (j < s.Length && (char.IsLetterOrDigit(s[j]))) j++;
                list.Add(s.Substring(i, j - i)); i = j; continue;
            }
            // 多字元運算子
            string two = i + 1 < s.Length ? s.Substring(i, 2) : "";
            if (two == "||" || two == "&&" || two == "<<" || two == ">>" || two == "<=" || two == ">=" || two == "==" || two == "<>" || two == "!=")
            { list.Add(two); i += 2; continue; }
            list.Add(c.ToString()); i++;
        }
        return list;
    }

    // ================= 參數解析 =================
    private enum TokKind { Str, Hash, Num, Var }
    private readonly struct ArgTok
    {
        public readonly TokKind Kind; public readonly string Str; public readonly long Num;
        public ArgTok(TokKind k, string s, long n) { Kind = k; Str = s; Num = n; }
    }

    private List<ArgTok> TokenizeArgs(string s)
    {
        var list = new List<ArgTok>();
        int i = 0;
        while (i < s.Length)
        {
            char c = s[i];
            if (char.IsWhiteSpace(c)) { i++; continue; }
            if (c == '\'' || c == '"')
            {
                char q = c; i++; var sb = new StringBuilder();
                while (i < s.Length && s[i] != q) sb.Append(s[i++]);
                if (i < s.Length) i++;
                list.Add(new ArgTok(TokKind.Str, sb.ToString(), 0));
            }
            else if (c == '#')
            {
                i++; var num = new StringBuilder();
                while (i < s.Length && char.IsDigit(s[i])) num.Append(s[i++]);
                long.TryParse(num.ToString(), out long code);
                list.Add(new ArgTok(TokKind.Hash, "", code));
            }
            else if (char.IsDigit(c))
            {
                var num = new StringBuilder();
                while (i < s.Length && char.IsLetterOrDigit(s[i])) num.Append(s[i++]);
                long.TryParse(num.ToString(), out long v);
                list.Add(new ArgTok(TokKind.Num, num.ToString(), v));
            }
            else
            {
                var id = new StringBuilder();
                while (i < s.Length && !char.IsWhiteSpace(s[i])) id.Append(s[i++]);
                list.Add(new ArgTok(TokKind.Var, id.ToString(), 0));
            }
        }
        return list;
    }

    private byte[] SendBytes(string rest, bool newline)
    {
        var ms = new List<byte>();
        foreach (var t in TokenizeArgs(rest))
        {
            switch (t.Kind)
            {
                case TokKind.Str: ms.AddRange(Encoding.UTF8.GetBytes(t.Str)); break;
                case TokKind.Hash: case TokKind.Num: ms.Add((byte)(t.Num & 0xFF)); break;
                case TokKind.Var:
                    var v = GetVar(t.Str);
                    if (v is string vs) ms.AddRange(Encoding.UTF8.GetBytes(vs));
                    else ms.Add((byte)(ToLong(v) & 0xFF));
                    break;
            }
        }
        if (newline) ms.Add((byte)'\r');
        return ms.ToArray();
    }

    private List<string> StrList(string rest)
    {
        var list = new List<string>();
        foreach (var t in TokenizeArgs(rest))
        {
            switch (t.Kind)
            {
                case TokKind.Str: list.Add(t.Str); break;
                case TokKind.Hash: list.Add(((char)t.Num).ToString()); break;
                case TokKind.Num: list.Add(t.Str); break;
                case TokKind.Var:
                    var v = GetVar(t.Str);
                    list.Add(v is string vs ? vs : ToLong(v).ToString(CultureInfo.InvariantCulture));
                    break;
            }
        }
        return list;
    }

    private List<string> SplitArgs(string rest) => StrList(rest); // 供字串指令用（每個 token 一個字串）
    private string StrOfArg(string token)
    {
        // token 可能是 'literal' / 變數名 / 數字
        var t = TokenizeArgs(token);
        return t.Count > 0 ? StrList(token)[0] : "";
    }
    private (string, string) TwoArgs(string rest)
    {
        var toks = SplitRawTokens(rest);
        return (toks.Count > 0 ? toks[0] : "", toks.Count > 1 ? string.Join(" ", toks.GetRange(1, toks.Count - 1)) : "");
    }
    private (string, string) SplitCondCmd(string rest)
    {
        rest = rest.Trim();
        if (rest.StartsWith("("))
        {
            int depth = 0, i = 0;
            for (; i < rest.Length; i++) { if (rest[i] == '(') depth++; else if (rest[i] == ')') { depth--; if (depth == 0) { i++; break; } } }
            return (rest.Substring(0, i), rest.Substring(i).Trim());
        }
        int sp = rest.IndexOf(' ');
        return sp < 0 ? (rest, "") : (rest.Substring(0, sp), rest.Substring(sp + 1).Trim());
    }
    private static List<string> SplitRawTokens(string s)
        => new(s.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries));

    // ================= 小工具 =================
    private static (string cmd, string rest) SplitCmd(string line)
    {
        int sp = line.IndexOfAny(new[] { ' ', '\t' });
        return sp < 0 ? (line.ToLowerInvariant(), "") : (line.Substring(0, sp).ToLowerInvariant(), line.Substring(sp + 1).Trim());
    }
    private void Jump(string label)
    {
        if (_labels.TryGetValue(label, out int idx)) { _pc = idx; _advance = false; }
    }
    private static string FirstToken(string s)
    {
        s = s.Trim();
        int sp = s.IndexOf(' ');
        return sp < 0 ? s : s.Substring(0, sp);
    }
    private static string ParseFirstQuoted(string s)
    {
        char q = s[0]; var sb = new StringBuilder();
        for (int i = 1; i < s.Length && s[i] != q; i++) sb.Append(s[i]);
        return sb.ToString();
    }
    private static bool IsIdent(string s) => Regex.IsMatch(s, @"^[A-Za-z_]\w*$");

    private static readonly HashSet<string> Keywords = new(StringComparer.OrdinalIgnoreCase)
    { "if","elseif","else","endif","for","next","while","endwhile","break","continue","goto","call","return","end","exit","then" };
    private static bool IsKeyword(string s) => Keywords.Contains(s);

    private object? GetVar(string name) => _vars.TryGetValue(name, out var v) ? v : null;
    private long GetInt(string name) => ToLong(GetVar(name));
    private string GetStr(string name) { var v = GetVar(name); return v is string s ? s : (v == null ? "" : ToLong(v).ToString(CultureInfo.InvariantCulture)); }
    private static long ToLong(object? v) => v switch { long l => l, int i => i, string s => long.TryParse(s, out var n) ? n : 0, _ => 0 };
}
