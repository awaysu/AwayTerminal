//! Windows 的視窗圖示與工作列身分（TASK-035）。
//!
//! 使用者回報「工作列上是 Windows 預設的空白圖示」。exe 內嵌的檔案圖示是對的
//!（檔案總管看得到），所以問題只會出在**執行中的那個視窗**：
//!
//! | 誰決定工作列上的圖示 | 怎麼問 | 誰設的 |
//! |---|---|---|
//! | 視窗圖示（大／小） | `WM_GETICON` + `GetClassLongPtr(GCLP_HICON)` | `WM_SETICON`，或視窗類別 |
//! | 工作列把哪些視窗歸成一堆 | `GetCurrentProcessExplicitAppUserModelID` | `SetCurrentProcessExplicitAppUserModelID` |
//!
//! `WM_GETICON` 回 0 而且類別圖示也是 0 → Windows 只好用預設的空白圖示。
//!
//! **AppUserModelID 一定要和 1.x 不一樣**：兩版的 exe 都叫 `AwayTerminal.exe`，
//! 沒有明確的 AUMID 時 Windows 會拿 exe 路徑去猜，開發版（`target\debug\`）、
//! 安裝版與 1.x 有機會被歸成同一堆，圖示與跳躍清單就會互相蓋。
//! 我們設 `com.awaysu.awayterminal2`（2.1.0 起 repo 改名回 AwayTerminal，這個 ID **不跟著改**：改了已釘選在工作列的圖示會失效。**結尾的 2 是刻意的**——`tauri.conf.json` 的
//! `identifier` 是 `com.awaysu.awayterminal`，但那是 bundle id，不是工作列身分；
//! 1.x 若曾用過同名，分組還是會撞）。

#[cfg(windows)]
use tauri::Manager;
use tauri::WebviewWindow;

/// 這一版的工作列身分。**不要和 1.x 相同**（見模組說明）。
pub const APP_USER_MODEL_ID: &str = "com.awaysu.awayterminal2";

/// `--verify` 用的量測結果。數字用字串回（HICON 是指標，JSON 的 number 放不下）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconProbe {
    /// `WM_GETICON` / `ICON_BIG`
    pub icon_big: String,
    /// `WM_GETICON` / `ICON_SMALL`
    pub icon_small: String,
    /// `WM_GETICON` / `ICON_SMALL2`（Windows 自己從大圖縮的那一份）
    pub icon_small2: String,
    /// 視窗類別的圖示（`GCLP_HICON`／`GCLP_HICONSM`）——視窗自己沒設時工作列用這個
    pub class_icon: String,
    pub class_icon_small: String,
    /// `GetCurrentProcessExplicitAppUserModelID`（沒設過就是空字串）
    pub app_user_model_id: String,
    /// 期望的 AUMID（比對用）
    pub want_app_user_model_id: String,
    pub hwnd: String,
}

/// 行程的 AppUserModelID。**要在任何視窗建立之前呼叫**（Windows 只認第一次）。
///
/// 失敗不影響啟動：最多是工作列把開發版與安裝版歸在一起。
#[cfg(windows)]
pub fn set_app_user_model_id() {
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    let wide: Vec<u16> = APP_USER_MODEL_ID.encode_utf16().chain(std::iter::once(0)).collect();
    let hr = unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };
    if hr != 0 {
        println!("[AwayTerminal] 設定 AppUserModelID 失敗：HRESULT 0x{hr:08X}");
    }
}

/// 把 exe 內嵌的圖示掛到視窗上（`WM_SETICON`，大小各一份）。
///
/// **為什麼需要這一步**：Tauri 只有在 `default_window_icon` 真的被嵌進去時才會設視窗圖示，
/// 而那份圖示來自 `tauri.conf.json` 的 `bundle.icon` 裡的 **PNG**——
/// 它是**單一尺寸**的點陣圖，Windows 拿去當 `ICON_BIG` 時會再縮放一次，
/// 而且 `cargo run`（dev）的路徑上不一定會被套用。
///
/// 這裡改成直接用 **exe 自己的 `.ico` 資源**（`LoadImageW` + `IMAGE_ICON`，
/// 分別要 `SM_CXICON`／`SM_CXSMICON` 的尺寸），所以：
///   * 拿到的是 `.ico` 裡**針對該尺寸做好的那一張**（16/24/32/48/64/256），不是縮出來的；
///   * dev 與 release 走同一條路（兩邊的 exe 都有 `icon.ico` 這份資源，`tauri-build` 放的）。
///
/// 回傳 `(big, small)` 是不是都掛上去了。
#[cfg(windows)]
pub fn apply_window_icon(window: &WebviewWindow) -> (bool, bool) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR,
        SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_SETICON,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics;

    let Ok(hwnd) = window.hwnd() else { return (false, false) };
    let hwnd = hwnd.0 as HWND;
    // exe 的第一個圖示資源。`tauri-build` 用 `IDI_ICON`＝1 把 icon.ico 放進去，
    // 但保險起見兩個常見的 id 都試（0 是「第一個」的慣例寫法用不了，改用 1 與 2）。
    let module = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) };
    let load = |id: u16, cx: i32, cy: i32| -> isize {
        unsafe {
            LoadImageW(module, id as usize as *const u16, IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR) as isize
        }
    };
    let (cx_big, cy_big) = unsafe { (GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CYICON)) };
    let (cx_sm, cy_sm) = unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };

    let mut big = 0isize;
    let mut small = 0isize;
    for id in [1u16, 2, 32512] {
        if big == 0 {
            big = load(id, cx_big, cy_big);
        }
        if small == 0 {
            small = load(id, cx_sm, cy_sm);
        }
        if big != 0 && small != 0 {
            break;
        }
    }
    unsafe {
        if big != 0 {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big);
        }
        if small != 0 {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small);
        }
    }
    (big != 0, small != 0)
}

/// 量現在的狀態（`--verify` 與人工排查用）。**只讀，不改任何東西。**
#[cfg(windows)]
pub fn probe(window: &WebviewWindow) -> IconProbe {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassLongPtrW, SendMessageW, GCLP_HICON, GCLP_HICONSM, ICON_BIG, ICON_SMALL,
        ICON_SMALL2, WM_GETICON,
    };
    let hwnd = window.hwnd().map(|h| h.0 as HWND).unwrap_or(std::ptr::null_mut());
    let hex = |v: isize| format!("0x{v:X}");
    if hwnd.is_null() {
        return IconProbe {
            icon_big: "0x0".into(),
            icon_small: "0x0".into(),
            icon_small2: "0x0".into(),
            class_icon: "0x0".into(),
            class_icon_small: "0x0".into(),
            app_user_model_id: current_app_user_model_id(),
            want_app_user_model_id: APP_USER_MODEL_ID.into(),
            hwnd: "0x0".into(),
        };
    }
    unsafe {
        IconProbe {
            icon_big: hex(SendMessageW(hwnd, WM_GETICON, ICON_BIG as usize, 0)),
            icon_small: hex(SendMessageW(hwnd, WM_GETICON, ICON_SMALL as usize, 0)),
            icon_small2: hex(SendMessageW(hwnd, WM_GETICON, ICON_SMALL2 as usize, 0)),
            class_icon: hex(GetClassLongPtrW(hwnd, GCLP_HICON) as isize),
            class_icon_small: hex(GetClassLongPtrW(hwnd, GCLP_HICONSM) as isize),
            app_user_model_id: current_app_user_model_id(),
            want_app_user_model_id: APP_USER_MODEL_ID.into(),
            hwnd: hex(hwnd as isize),
        }
    }
}

/// 目前行程的 AppUserModelID（沒設過就是空字串）。
#[cfg(windows)]
fn current_app_user_model_id() -> String {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::GetCurrentProcessExplicitAppUserModelID;
    let mut ptr: *mut u16 = std::ptr::null_mut();
    let hr = unsafe { GetCurrentProcessExplicitAppUserModelID(&mut ptr) };
    if hr != 0 || ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) });
    unsafe { CoTaskMemFree(ptr as *mut _) };
    s
}

/// `--verify` 用：量主視窗的圖示與工作列身分。
#[tauri::command]
#[cfg(windows)]
pub fn window_icon_probe(app: tauri::AppHandle) -> Option<IconProbe> {
    app.get_webview_window("main").map(|w| probe(&w))
}

// ---------------------------------------------------------------- 非 Windows
//
// 工作列圖示這件事只有 Windows 有（mac 的 Dock 用 .icns、Linux 用 .desktop 檔），
// 所以這幾個在別的平台是空操作，`--verify` 那一段會直接跳過。

#[cfg(not(windows))]
pub fn set_app_user_model_id() {}

#[cfg(not(windows))]
pub fn apply_window_icon(_window: &WebviewWindow) -> (bool, bool) {
    (true, true) // mac/Linux 由 Tauri 的 default_window_icon 處理
}

#[tauri::command]
#[cfg(not(windows))]
pub fn window_icon_probe(_app: tauri::AppHandle) -> Option<IconProbe> {
    None
}
