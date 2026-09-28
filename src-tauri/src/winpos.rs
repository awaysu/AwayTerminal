//! 視窗位置的合理性判斷（`lib.rs` 的 `apply_window_bounds` / `remember_window_bounds` 用）。
//!
//! 全部是純函式、不碰 Tauri 也不碰 Win32，所以任何平台都跑得到單元測試。
//! 進來的座標一律是**邏輯像素**，而且和 `set_position(LogicalPosition)` 用同一個
//! 換算基準（主視窗當下的 `scale_factor`）——螢幕矩形在呼叫端就先換算好。
//!
//! 為什麼需要這一層（TASK-026）：Windows 把最小化的視窗擺到實體座標 `(-32000,-32000)`，
//! 那時候照樣會發 `Moved`／`Resized`。存下去之後下次啟動就 `set_position` 到螢幕外，
//! 又觸發 `Moved` 再存一次同樣的座標——使用者重開也救不回來（工作列有圖示、點了沒畫面）。

/// 邏輯像素的矩形；左上角 `(x, y)`，寬高保證 ≥ 0。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width: width.max(0),
            height: height.max(0),
        }
    }

    pub fn right(&self) -> i32 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(&self) -> i32 {
        self.y.saturating_add(self.height)
    }

    /// 兩個矩形重疊的部分；沒重疊時寬或高是 0。
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Rect::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
    }
}

/// 要算「看得見」，和某個螢幕重疊的部分至少要這麼大（邏輯像素）。
pub const MIN_VISIBLE: i32 = 100;

/// 標題列（滑鼠能抓住視窗的那一條）大約這麼高。
///
/// 重疊的部分必須**含到標題列那一帶**，不然視窗雖然露出一角，使用者仍然搬不動它
/// （例：整個視窗被推到螢幕上緣之外，只剩下半截露在畫面裡）。
pub const TITLE_BAR: i32 = 48;

/// 座標絕對值超過這個就當成不是真的位置。
///
/// Windows 最小化時給的是 `-32000` 實體像素（125% DPI 下換算成 −25600 邏輯像素）；
/// 真實桌面就算接一排螢幕也到不了兩萬。這是 `is_minimized()` 之外的第二道保險——
/// 萬一某個平台在視窗已經最小化之前就先送 `Moved`，這條會擋下來。
pub const MAX_PLAUSIBLE: i32 = 20_000;

/// 這個座標像不像真的桌面座標。
pub fn plausible_position(x: i32, y: i32) -> bool {
    x.abs() < MAX_PLAUSIBLE && y.abs() < MAX_PLAUSIBLE
}

/// 這個視窗矩形是不是至少有一塊看得見、而且抓得到。
///
/// 條件（對**任一**螢幕成立即可）：
/// 1. 重疊的部分寬、高各 ≥ [`MIN_VISIBLE`]；
/// 2. 重疊的部分要碰到視窗頂端那條 [`TITLE_BAR`] 帶（抓得住才搬得動）。
///
/// `monitors` 空的（列舉失敗）時回 `false`——寧可置中，也不要開在看不到的地方。
pub fn is_visible_on(window: Rect, monitors: &[Rect]) -> bool {
    if !plausible_position(window.x, window.y) {
        return false;
    }
    monitors.iter().any(|m| {
        let inter = window.intersect(m);
        inter.width >= MIN_VISIBLE
            && inter.height >= MIN_VISIBLE
            && inter.y <= window.y.saturating_add(TITLE_BAR)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 這台機器（TASK-026 回報的環境）：單螢幕 1536×864 邏輯像素（實體 1920×1080 @125%）。
    fn one_screen() -> Vec<Rect> {
        vec![Rect::new(0, 0, 1536, 864)]
    }

    /// 左邊接一台副螢幕：主螢幕 (0,0)，副螢幕在負座標。
    fn two_screens() -> Vec<Rect> {
        vec![Rect::new(0, 0, 1536, 864), Rect::new(-1920, 0, 1920, 1080)]
    }

    #[test]
    fn fully_inside_is_visible() {
        assert!(is_visible_on(Rect::new(102, 26, 1200, 800), &one_screen()));
    }

    #[test]
    fn fully_outside_is_not_visible() {
        // 右邊、下面都超出去（外接螢幕拔掉之後的典型情形）
        assert!(!is_visible_on(Rect::new(2600, 100, 1200, 800), &one_screen()));
        assert!(!is_visible_on(Rect::new(100, 2000, 1200, 800), &one_screen()));
    }

    #[test]
    fn straddling_two_screens_is_visible() {
        // 跨在兩台中間（左邊那台只露 100px 寬，右邊那台 1100px）→ 看得見
        assert!(is_visible_on(Rect::new(-100, 50, 1200, 800), &two_screens()));
        // 只露 99px 就不算（門檻是 >=，不是 >）
        assert!(!is_visible_on(Rect::new(-1920 - 1200 + 99, 50, 1200, 800), &[
            Rect::new(-1920, 0, 1920, 1080)
        ]));
    }

    #[test]
    fn minimized_sentinel_is_not_visible() {
        // 實際案例：Windows 最小化時的 (-32000,-32000) 實體 → 125% DPI 下的 −25600
        let bogus = Rect::new(-25600, -25600, 1536, 801);
        assert!(!is_visible_on(bogus, &one_screen()));
        assert!(!is_visible_on(bogus, &two_screens()));
        assert!(!plausible_position(-25600, -25600));
        // 就算真的有一台螢幕在那裡（不可能，但別靠螢幕清單擋），也還是不接受
        assert!(!is_visible_on(bogus, &[Rect::new(-25600, -25600, 1536, 864)]));
    }

    #[test]
    fn negative_but_on_left_secondary_is_visible() {
        assert!(is_visible_on(Rect::new(-1800, 60, 1200, 800), &two_screens()));
        // 同一個座標，副螢幕拔掉之後就不算
        assert!(!is_visible_on(Rect::new(-1800, 60, 1200, 800), &one_screen()));
    }

    #[test]
    fn title_bar_above_screen_is_not_visible() {
        // 露出來的是視窗下半部，標題列在畫面上緣之外 → 抓不到、不算看得見
        assert!(!is_visible_on(Rect::new(100, -500, 1200, 900), &one_screen()));
        // 只掉出去一點點（標題列還在畫面裡）就可以
        assert!(is_visible_on(Rect::new(100, -20, 1200, 900), &one_screen()));
    }

    #[test]
    fn sliver_on_screen_is_not_visible() {
        // 只剩 60px 寬掉在畫面裡 → 當成看不見
        assert!(!is_visible_on(Rect::new(1476, 100, 1200, 800), &one_screen()));
        // 高度只剩 60px 也一樣
        assert!(!is_visible_on(Rect::new(100, 804, 1200, 800), &one_screen()));
    }

    #[test]
    fn no_monitor_info_is_not_visible() {
        assert!(!is_visible_on(Rect::new(102, 26, 1200, 800), &[]));
    }

    #[test]
    fn intersect_handles_no_overlap_and_extremes() {
        let a = Rect::new(0, 0, 100, 100);
        assert_eq!(a.intersect(&Rect::new(200, 200, 100, 100)), Rect::new(200, 200, 0, 0));
        assert_eq!(a.intersect(&Rect::new(50, 50, 100, 100)), Rect::new(50, 50, 50, 50));
        // 不能 overflow（i32::MAX 附近的寬度）
        let huge = Rect::new(i32::MAX - 10, 0, i32::MAX, 100);
        let _ = huge.intersect(&Rect::new(0, 0, 1536, 864));
    }

    #[test]
    fn plausible_position_bounds() {
        assert!(plausible_position(0, 0));
        assert!(plausible_position(-1920, 1080));
        assert!(!plausible_position(-32000, -32000));
        assert!(!plausible_position(0, 25_000));
    }
}
