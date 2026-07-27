using System.Windows;

namespace AwayTerminal;

/// <summary>工具列按鈕的附加屬性。Horizontal=True 時，按鈕內圖示在左、文字在右（右側直欄用）。</summary>
public static class ToolBtnEx
{
    public static readonly DependencyProperty HorizontalProperty =
        DependencyProperty.RegisterAttached(
            "Horizontal", typeof(bool), typeof(ToolBtnEx), new PropertyMetadata(false));

    public static void SetHorizontal(DependencyObject o, bool value) => o.SetValue(HorizontalProperty, value);
    public static bool GetHorizontal(DependencyObject o) => (bool)o.GetValue(HorizontalProperty);
}
