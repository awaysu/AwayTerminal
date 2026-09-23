using System.Globalization;
using System.IO;
using System.Text;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using AwayTerminal.Localization;

namespace AwayTerminal.Dialogs;

/// <summary>工具列「輸入文字」開的視窗（1.0.30 為分頁列最左「…」，1.0.46 移到工具列）：
/// 先在一般 TextBox 把文字（含中文、多行）打好，按「送出」才整段貼進作用中分頁。
/// 用途＝繞過 claude 對逐鍵 IME 輸入的重複／亂碼問題（IME 組字與提交都發生在 WPF TextBox，
/// xterm／ConPTY 只會收到最後一次整段貼上）。
/// <para>1.2.7（使用者要求）：常用字串功能整個移除（工具列按鈕、管理視窗、這裡的常用字串列都拿掉）。
/// 上方文字連結：載入文字檔（1.1.10；選檔案、內容取代文字框，可復原）／清除（整段刪掉，可復原）／
/// 復原（還原上一步編輯，含清除；Ctrl+Z 同效）／儲存（另存成文字檔，會問存哪裡）。
/// 下方：送出後送 Enter／返回（不送）／送出（Ctrl+Enter 同效）。
/// 草稿保留：打到一半按 X 或「返回」關掉，下次叫回來文字還在（`_draft`，程式存活期間有效）；
/// 「送出」後才清空。「清除」掉的內容另存 `_lastCleared`，即使關掉重開（undo 堆疊已是新的）
/// 按「復原」仍能把它找回來。</para></summary>
public partial class ComposeDialog : Window
{
    private static string _draft = "";        // 未送出的草稿（X／返回 後保留）
    private static string _lastCleared = "";  // 最近一次「清除」掉的內容（跨開關也能復原）
    private static string? _lastLoadDir;      // 上次「載入文字檔」的資料夾（程式存活期間）
    private static string? _lastSaveDir;      // 上次「儲存」的資料夾（程式存活期間）
    private const int LoadMaxMB = 2;          // 載入文字檔上限：文字框放太大的內容會卡、也不適合整段送進終端機

    public string TextToSend => Input.Text;
    public bool SendEnter => SendEnterChk.IsChecked == true;

    public ComposeDialog(bool sendEnter)
    {
        InitializeComponent();

        Title = Loc.T("compose.title");
        Placeholder.Text = Loc.T("compose.placeholder");
        SendEnterChk.Content = Loc.T("compose.sendEnter");
        SendEnterChk.IsChecked = sendEnter;
        LoadFileBtn.Content = Loc.T("compose.loadFile");
        ClearBtn.Content = Loc.T("compose.clear");
        UndoBtn.Content = Loc.T("compose.undo");
        SaveBtn.Content = Loc.T("compose.save");
        BackBtn.Content = Loc.T("compose.back");
        SendBtn.Content = Loc.T("compose.send");

        // 帶回上次的草稿；把 undo 堆疊歸零，免得第一下「復原」把草稿整段退掉
        Input.Text = _draft;
        Input.IsUndoEnabled = false;
        Input.IsUndoEnabled = true;
        Input.CaretIndex = Input.Text.Length;
        Input_TextChanged(Input, null!);   // 空草稿不會觸發 TextChanged：補一次，定提示字／送出鈕狀態

        Loaded += (_, _) => Input.Focus();
        Input.PreviewKeyDown += (_, e) =>
        {
            if (e.Key == Key.Enter && (Keyboard.Modifiers & ModifierKeys.Control) != 0)
            {
                e.Handled = true;
                Send();
            }
        };
        // 關閉時記草稿：送出＝清空；X／返回＝保留目前內容
        Closing += (_, _) => _draft = DialogResult == true ? "" : Input.Text;
    }

    // ---------- 文字框 ----------
    private void Input_TextChanged(object sender, TextChangedEventArgs e)
    {
        Placeholder.Visibility = Input.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        SendBtn.IsEnabled = Input.Text.Length > 0;
    }

    private void Send_Click(object sender, RoutedEventArgs e) => Send();

    private void Send()
    {
        if (string.IsNullOrEmpty(Input.Text)) return;   // 空白不送（想只送 Enter 請直接在終端機按）
        DialogResult = true;
    }

    /// <summary>儲存（1.2.7）：把文字框內容另存成文字檔（UTF-8、無 BOM；換行保持 TextBox 的 \r\n），會問存哪裡。</summary>
    private void Save_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new Microsoft.Win32.SaveFileDialog
        {
            Title = Loc.T("compose.save"),
            FileName = "compose.txt",
            DefaultExt = ".txt",
            Filter = Loc.T("compose.loadFilter") + " (*.txt)|*.txt|Markdown (*.md)|*.md|"
                     + (Loc.Lang == "en" ? "All files" : "所有檔案") + " (*.*)|*.*"
        };
        if (_lastSaveDir != null && Directory.Exists(_lastSaveDir)) dlg.InitialDirectory = _lastSaveDir;
        else if (_lastLoadDir != null && Directory.Exists(_lastLoadDir)) dlg.InitialDirectory = _lastLoadDir;
        if (dlg.ShowDialog(this) != true) { Input.Focus(); return; }
        _lastSaveDir = Path.GetDirectoryName(dlg.FileName);
        try
        {
            File.WriteAllText(dlg.FileName, Input.Text, new UTF8Encoding(false));
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, Loc.T("compose.saveFail") + "\n" + ex.Message, Title,
                MessageBoxButton.OK, MessageBoxImage.Error);
        }
        Input.Focus();
    }

    /// <summary>載入文字檔（1.1.10）：選一個檔案，內容取代文字框。同「清除」走全選＋取代選取（進 undo 堆疊，「復原」救得回來），
    /// 原內容也記進 _lastCleared（關掉重開後照樣救得回來）。換行統一成 TextBox 自己用的 \r\n（送出時 claude 分頁會轉 ESC+CR 軟換行）。</summary>
    private void LoadFile_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new Microsoft.Win32.OpenFileDialog
        {
            Title = Loc.T("compose.loadFile"),
            Filter = Loc.T("compose.loadFilter") + " (*.txt;*.md;*.log;*.json;*.csv;*.xml;*.yaml;*.yml)|*.txt;*.md;*.log;*.json;*.csv;*.xml;*.yaml;*.yml|"
                     + (Loc.Lang == "en" ? "All files" : "所有檔案") + " (*.*)|*.*"
        };
        if (_lastLoadDir != null && Directory.Exists(_lastLoadDir)) dlg.InitialDirectory = _lastLoadDir;
        if (dlg.ShowDialog(this) != true) { Input.Focus(); return; }
        _lastLoadDir = Path.GetDirectoryName(dlg.FileName);

        string text;
        try
        {
            if (new FileInfo(dlg.FileName).Length > LoadMaxMB * 1024L * 1024L)
            {
                MessageBox.Show(this, string.Format(Loc.T("compose.loadTooBig"), LoadMaxMB), Title,
                    MessageBoxButton.OK, MessageBoxImage.Warning);
                return;
            }
            text = DecodeText(File.ReadAllBytes(dlg.FileName));
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, Loc.T("compose.loadFail") + "\n" + ex.Message, Title,
                MessageBoxButton.OK, MessageBoxImage.Error);
            return;
        }

        text = text.Replace("\r\n", "\n").Replace('\r', '\n').Replace("\n", "\r\n");
        if (Input.Text.Length > 0) _lastCleared = Input.Text;
        Input.SelectAll();
        Input.SelectedText = text;
        Input.CaretIndex = Input.Text.Length;
        Input.Focus();
    }

    /// <summary>文字檔解碼：有 BOM 照 BOM（UTF-8／UTF-16 LE／BE）；沒有 BOM 先試嚴格 UTF-8，
    /// 不合法（例如舊的 Big5 記事本檔）才退回系統 ANSI 字碼頁（繁中 Windows＝950），避免中文變亂碼。</summary>
    private static string DecodeText(byte[] b)
    {
        if (b.Length >= 3 && b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF) return Encoding.UTF8.GetString(b, 3, b.Length - 3);
        if (b.Length >= 2 && b[0] == 0xFF && b[1] == 0xFE) return Encoding.Unicode.GetString(b, 2, b.Length - 2);
        if (b.Length >= 2 && b[0] == 0xFE && b[1] == 0xFF) return Encoding.BigEndianUnicode.GetString(b, 2, b.Length - 2);
        try { return new UTF8Encoding(false, true).GetString(b); }
        catch (DecoderFallbackException) { }
        try
        {
            Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);
            return Encoding.GetEncoding(CultureInfo.CurrentCulture.TextInfo.ANSICodePage).GetString(b);
        }
        catch { return Encoding.UTF8.GetString(b); }
    }

    /// <summary>清除：整段刪掉。走「全選＋取代選取」而非直接設 Text，確保進 undo 堆疊，Ctrl+Z／復原救得回來。</summary>
    private void Clear_Click(object sender, RoutedEventArgs e)
    {
        if (Input.Text.Length > 0)
        {
            _lastCleared = Input.Text;
            Input.SelectAll();
            Input.SelectedText = "";
        }
        Input.Focus();
    }

    /// <summary>復原：先走 TextBox 自己的 undo（逐步還原，含清除）；堆疊空了（例如清除後關掉再開）
    /// 而框是空的 → 把最近一次清除的內容整段放回。</summary>
    private void Undo_Click(object sender, RoutedEventArgs e)
    {
        if (Input.CanUndo) Input.Undo();
        else if (Input.Text.Length == 0 && _lastCleared.Length > 0) Input.Text = _lastCleared;
        Input.CaretIndex = Input.Text.Length;
        Input.Focus();
    }
}
