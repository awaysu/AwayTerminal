using System.Windows;
using System.Windows.Input;
using AwayTerminal.Localization;

namespace AwayTerminal.Dialogs;

/// <summary>分頁列最左「…」開的輸入框：先在一般 TextBox 把文字（含中文、多行）打好，
/// 按「送出」才整段貼進作用中分頁。用途＝繞過 claude 對逐鍵 IME 輸入的重複／亂碼問題
/// （IME 組字與提交都發生在 WPF TextBox，xterm／ConPTY 只會收到最後一次整段貼上）。
/// 按鈕：清除（整段刪掉，可復原）／復原（還原上一步編輯，含清除；Ctrl+Z 同效）／返回（不送）／送出。
/// <para>草稿保留：打到一半按 X 或「返回」關掉，下次叫回來文字還在（`_draft`，程式存活期間有效）；
/// 「送出」後才清空。「清除」掉的內容另存 `_lastCleared`，即使關掉重開（undo 堆疊已是新的）
/// 按「復原」仍能把它找回來。</para></summary>
public partial class ComposeDialog : Window
{
    private static string _draft = "";        // 未送出的草稿（X／返回 後保留）
    private static string _lastCleared = "";  // 最近一次「清除」掉的內容（跨開關也能復原）

    public string TextToSend => Input.Text;
    public bool SendEnter => SendEnterChk.IsChecked == true;

    public ComposeDialog(bool sendEnter)
    {
        InitializeComponent();
        Title = Loc.T("compose.title");
        HintText.Text = Loc.T("compose.hint");
        SendEnterChk.Content = Loc.T("prompt.sendEnter");
        SendEnterChk.IsChecked = sendEnter;
        ClearBtn.Content = Loc.T("compose.clear");
        UndoBtn.Content = Loc.T("compose.undo");
        BackBtn.Content = Loc.T("compose.back");
        SendBtn.Content = Loc.T("compose.send");

        // 帶回上次的草稿；把 undo 堆疊歸零，免得第一下「復原」把草稿整段退掉
        Input.Text = _draft;
        Input.IsUndoEnabled = false;
        Input.IsUndoEnabled = true;
        Input.CaretIndex = Input.Text.Length;

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

    private void Send_Click(object sender, RoutedEventArgs e) => Send();

    private void Send()
    {
        if (string.IsNullOrEmpty(Input.Text)) return;   // 空白不送（想只送 Enter 請直接在終端機按）
        DialogResult = true;
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
