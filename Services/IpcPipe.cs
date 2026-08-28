using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Threading.Tasks;

namespace AwayTerminal.Services;

/// <summary>
/// 實例間的具名管線（1.0.45）：檔案總管右鍵「用 AwayTerminal 開啟」會再啟動一個 AwayTerminal.exe，
/// 它先試著把「開啟資料夾」交給已經在跑的實例（連得上就送一行後結束、不開第二個視窗）；
/// 連不上（沒有實例在跑）才自己正常啟動並在 ready 後開該資料夾。
/// 訊息＝一行 "open-dir\t{路徑}"。CurrentUserOnly：只有同一位使用者的行程能連。
/// 第一個實例當伺服端；使用者自己多開時第二個實例建不出管線（MaxNumberOfServerInstances=1）就安靜放棄，
/// 右鍵開啟一律交給第一個實例。
/// </summary>
internal static class IpcPipe
{
    public const string PipeName = "AwayTerminal.OpenDir";

    /// <summary>把一行指令送給已在執行的實例；成功回 true（呼叫端應直接結束）。</summary>
    public static bool TrySend(string line, int timeoutMs = 700)
    {
        try
        {
            using var c = new NamedPipeClientStream(".", PipeName, PipeDirection.Out, PipeOptions.CurrentUserOnly);
            c.Connect(timeoutMs);
            using var w = new StreamWriter(c, new UTF8Encoding(false));
            w.Write(line + "\n");
            w.Flush();
            return true;
        }
        catch (Exception ex)
        {
            Diag.Log($"ipc send skipped: {ex.GetType().Name} {ex.Message}");
            return false;
        }
    }

    /// <summary>伺服端迴圈（背景執行緒）：每個連線讀到 EOF，逐行交給 onLine（呼叫端自己 Dispatcher 到 UI）。</summary>
    public static void StartServer(Action<string> onLine)
    {
        _ = Task.Run(async () =>
        {
            while (true)
            {
                NamedPipeServerStream? s;
                try
                {
                    s = new NamedPipeServerStream(PipeName, PipeDirection.In, 1,
                        PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
                }
                catch (Exception ex)
                {
                    Diag.Log($"ipc server not started (another instance owns it?): {ex.Message}");
                    return;
                }
                try
                {
                    await s.WaitForConnectionAsync().ConfigureAwait(false);
                    using var r = new StreamReader(s, Encoding.UTF8);
                    string text = await r.ReadToEndAsync().ConfigureAwait(false);
                    foreach (var raw in text.Split('\n'))
                    {
                        string line = raw.Trim('\r', ' ');
                        if (line.Length > 0) { try { onLine(line); } catch { } }
                    }
                }
                catch (Exception ex) { Diag.Log($"ipc server: {ex.Message}"); }
                finally { try { s.Dispose(); } catch { } }
            }
        });
    }
}
