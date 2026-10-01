//! 工作列圖示右下的忙碌指示：**只要有分頁是紅色（忙碌），就有一顆紅球上下跳動**；
//! 全部閒置或沒有分頁就不顯示。
//!
//! 搬移舊版 `MainWindow.SetTaskbarBusy`（1.0.30「沒事就乾淨、有事才跳」；1.0.41 把球放大）。
//! overlay 只能放一張圖，所以動畫＝預先畫好一輪彈跳影格、忙碌時輪播。數字照舊版：
//!
//! | 項目 | 值 |
//! |---|---|
//! | 影格 | 12 格、每格 60ms（約 0.7 秒一跳） |
//! | 畫布 | 40×40（overlay 徽章由 Windows 固定成小尺寸，畫大再縮下去才平滑） |
//! | 球 | 半徑 16、`#F44336`、白色描邊 1.8；圓心 y 在 23（貼地）～18（頂點）之間走 sin 曲線 |
//! | 壓扁 | 只有貼地那幾格（`1 + 0.15·max(0, 1 − 5·phase)`，保證壓扁＋描邊仍不出畫布） |
//!
//! 舊版用 WPF 的 `DrawingVisual` 畫；這裡只有一顆橢圓，不值得為它拉一個繪圖 crate，
//! 自己用 4×4 超取樣算覆蓋率。
//!
//! 誰來叫：狀態燈輪詢（`status::tick`，600ms）每一輪算完忙閒就呼叫 [`set_busy`]，
//! 和分頁列圖示染紅是同一份結果。
//!
//! 工作列 overlay 是 Windows 才有的東西（`ITaskbarList3::SetOverlayIcon`，Tauri 包成
//! `set_overlay_icon`）；mac／Linux 是空操作。

/// 一輪彈跳的影格數（舊版 `BounceFrameCount`）。
#[cfg(any(windows, test))]
const FRAME_COUNT: usize = 12;
/// 畫布邊長（像素）。
#[cfg(any(windows, test))]
const SIZE: u32 = 40;

/// 第 `k` 格的 RGBA（非預乘、由上到下）：紅球由底部彈到頂再落下，落地附近略壓扁。
/// 舊版 `MakeBounceFrame`。
#[cfg(any(windows, test))]
fn bounce_frame(k: usize) -> Vec<u8> {
    const R: f64 = 16.0;
    const TOP: f64 = 18.0;
    const BOTTOM: f64 = 23.0;
    const PEN: f64 = 1.8;
    const FILL: [f64; 3] = [244.0, 67.0, 54.0]; // #F44336
    /// 每個像素每邊取幾個子樣本
    const SS: u32 = 4;

    // 0→1→0：底 → 頂 → 底
    let phase = (std::f64::consts::PI * k as f64 / FRAME_COUNT as f64).sin();
    let cx = SIZE as f64 / 2.0;
    let cy = BOTTOM - (BOTTOM - TOP) * phase;
    let squash = 1.0 + 0.15 * (1.0 - phase * 5.0).max(0.0);
    let (rx, ry) = (R * squash, R / squash);

    let mut out = vec![0u8; (SIZE * SIZE * 4) as usize];
    for py in 0..SIZE {
        for px in 0..SIZE {
            let (mut red, mut white) = (0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f64 + (sx as f64 + 0.5) / SS as f64 - cx;
                    let y = py as f64 + (sy as f64 + 0.5) / SS as f64 - cy;
                    // 到橢圓邊界的距離，沿著從圓心出去的那條線量（壓扁最多 15%，
                    // 和真正的垂直距離差不到一個子樣本）。描邊以邊界為中心，內外各半
                    let scale = ((x / rx).powi(2) + (y / ry).powi(2)).sqrt();
                    let dist = if scale == 0.0 {
                        -ry
                    } else {
                        (x * x + y * y).sqrt() * (1.0 - 1.0 / scale)
                    };
                    if dist <= -PEN / 2.0 {
                        red += 1;
                    } else if dist <= PEN / 2.0 {
                        white += 1;
                    }
                }
            }
            let covered = red + white;
            if covered == 0 {
                continue;
            }
            let i = ((py * SIZE + px) * 4) as usize;
            for (c, fill) in FILL.iter().enumerate() {
                out[i + c] =
                    ((fill * red as f64 + 255.0 * white as f64) / covered as f64).round() as u8;
            }
            out[i + 3] = (255 * covered / (SS * SS)) as u8;
        }
    }
    out
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;
    use std::time::Duration;

    use tauri::{AppHandle, Manager};

    use super::{bounce_frame, FRAME_COUNT, SIZE};

    /// 每格停多久（舊版 `_bounceTimer` 的間隔）。
    const FRAME_INTERVAL: Duration = Duration::from_millis(60);

    /// 現在有沒有分頁在忙（狀態燈輪詢每 600ms 寫一次）。
    static BUSY: AtomicBool = AtomicBool::new(false);
    /// 輪播執行緒是不是開著（只開一條）。
    static ANIMATING: AtomicBool = AtomicBool::new(false);
    static FRAMES: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

    pub fn set_busy(app: &AppHandle, busy: bool) {
        BUSY.store(busy, Ordering::SeqCst);
        // 轉閒：輪播執行緒自己會看到、清掉 overlay 再結束
        if !busy || ANIMATING.swap(true, Ordering::SeqCst) {
            return;
        }
        let app = app.clone();
        std::thread::spawn(move || animate(app));
    }

    fn animate(app: AppHandle) {
        let frames = FRAMES.get_or_init(|| (0..FRAME_COUNT).map(bounce_frame).collect());
        let mut idx = 0;
        while BUSY.load(Ordering::SeqCst) {
            show(&app, Some(&frames[idx]));
            idx = (idx + 1) % frames.len();
            std::thread::sleep(FRAME_INTERVAL);
        }
        show(&app, None);
        ANIMATING.store(false, Ordering::SeqCst);
        // 正要收尾時又轉忙：那一次 `set_busy(true)` 看到 ANIMATING 還是 true 就沒開新的 → 這裡補開
        if BUSY.load(Ordering::SeqCst) {
            set_busy(&app, true);
        }
    }

    /// `None`＝清掉 overlay。視窗已經不在（正在結束）就什麼都不做。
    fn show(app: &AppHandle, frame: Option<&[u8]>) {
        let Some(win) = app.get_webview_window("main") else {
            return;
        };
        let icon = frame.map(|rgba| tauri::image::Image::new(rgba, SIZE, SIZE));
        // 失敗只是少一格動畫，不值得洗 log（忙碌時每秒十幾次）
        let _ = win.set_overlay_icon(icon);
    }
}

/// 有分頁忙碌 → 開始（或維持）紅球跳動；全部閒置／沒有分頁 → 清掉 overlay。
/// 可以每一輪輪詢都叫，狀態沒變時不做事。
#[cfg(windows)]
pub fn set_busy(app: &tauri::AppHandle, busy: bool) {
    imp::set_busy(app, busy);
}

#[cfg(not(windows))]
pub fn set_busy(_app: &tauri::AppHandle, _busy: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(frame: &[u8], x: u32, y: u32) -> u8 {
        frame[((y * SIZE + x) * 4 + 3) as usize]
    }

    /// 最上面那一列有東西的 y。
    fn top_row(frame: &[u8]) -> u32 {
        (0..SIZE)
            .find(|&y| (0..SIZE).any(|x| alpha(frame, x, y) > 0))
            .expect("影格不可以是空的")
    }

    /// 有東西的最左一欄到最右一欄的寬度。
    fn width(frame: &[u8]) -> u32 {
        let cols: Vec<u32> = (0..SIZE)
            .filter(|&x| (0..SIZE).any(|y| alpha(frame, x, y) > 0))
            .collect();
        cols.last().unwrap() - cols.first().unwrap() + 1
    }

    #[test]
    fn frames_are_a_red_ball_with_a_white_rim() {
        for k in 0..FRAME_COUNT {
            let f = bounce_frame(k);
            assert_eq!(f.len(), (SIZE * SIZE * 4) as usize);
            // 球心：不透明的 #F44336（圓心 y 在 18～23，(20, 20) 一定在球裡）
            let i = ((20 * SIZE + 20) * 4) as usize;
            assert_eq!(&f[i..i + 4], &[244, 67, 54, 255], "第 {k} 格球心");
            // 四個角落在球外
            for (x, y) in [(0, 0), (SIZE - 1, 0), (0, SIZE - 1), (SIZE - 1, SIZE - 1)] {
                assert_eq!(alpha(&f, x, y), 0, "第 {k} 格角落 ({x},{y})");
            }
            // 上下都留在畫布裡（最上、最下一列是空的）
            for x in 0..SIZE {
                assert_eq!(alpha(&f, x, 0), 0, "第 {k} 格頂到畫布上緣");
                assert_eq!(alpha(&f, x, SIZE - 1), 0, "第 {k} 格頂到畫布下緣");
            }
            // 描邊是白的：球頂那一列的正中間
            let y = top_row(&f);
            let i = ((y * SIZE + 20) * 4) as usize;
            assert_eq!(&f[i..i + 3], &[255, 255, 255], "第 {k} 格描邊");
        }
    }

    /// 真的有在跳：頂點那一格比貼地那一格高，而且只有貼地時壓扁（變寬）。
    #[test]
    fn the_ball_bounces_and_squashes_on_the_ground() {
        let ground = bounce_frame(0);
        let apex = bounce_frame(FRAME_COUNT / 2);
        assert!(top_row(&apex) < top_row(&ground), "頂點要比貼地高");
        assert!(width(&ground) > width(&apex), "貼地那一格要比較寬（壓扁）");
        // 上去和下來對稱（sin 曲線）
        assert_eq!(bounce_frame(3), bounce_frame(FRAME_COUNT - 3));
    }
}
