using System.Collections.Generic;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace AwayTerminal.Services;

/// <summary>
/// 把灰階工具列圖示（icon/*.png，淺色填充＋深色描邊）染成指定顏色（1.1.2 分頁列狀態圖示：閒置綠、忙碌紅）。
/// 做法＝每個像素取亮度（gamma 0.7 稍微提亮）再乘上色調：亮部＝色調色、描邊維持深色，圖形細節保留；alpha 原樣。
/// 依（檔名, 顏色）快取一份凍結的 BitmapSource，全部分頁共用。
/// </summary>
public static class IconTint
{
    private static readonly Dictionary<string, BitmapSource> Cache = new();

    public static BitmapSource Get(string file, Color tint)
    {
        string key = file + "|" + tint;
        lock (Cache)
        {
            if (Cache.TryGetValue(key, out var hit)) return hit;
        }
        BitmapSource bmp;
        try { bmp = Tint(file, tint); }
        catch
        {
            // 圖示檔不存在（自訂連線的 Icon key 不合法）→ 退回通用「run」圖示；連它都失敗才給空圖
            try { bmp = Tint("run.png", tint); }
            catch { bmp = BitmapSource.Create(1, 1, 96, 96, PixelFormats.Bgra32, null, new byte[4], 4); bmp.Freeze(); }
        }
        lock (Cache) { Cache[key] = bmp; }
        return bmp;
    }

    private static BitmapSource Tint(string file, Color tint)
    {
        var src = new BitmapImage(new Uri($"pack://application:,,,/icon/{file}", UriKind.Absolute));
        var conv = new FormatConvertedBitmap(src, PixelFormats.Bgra32, null, 0);
        int w = conv.PixelWidth, h = conv.PixelHeight, stride = w * 4;
        var px = new byte[stride * h];
        conv.CopyPixels(px, stride, 0);
        for (int i = 0; i < px.Length; i += 4)
        {
            double lum = (0.299 * px[i + 2] + 0.587 * px[i + 1] + 0.114 * px[i]) / 255.0;
            lum = Math.Pow(lum, 0.7);
            px[i] = (byte)(tint.B * lum);
            px[i + 1] = (byte)(tint.G * lum);
            px[i + 2] = (byte)(tint.R * lum);
        }
        var bmp = BitmapSource.Create(w, h, 96, 96, PixelFormats.Bgra32, null, px, stride);
        bmp.Freeze();
        return bmp;
    }
}
