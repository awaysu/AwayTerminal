//! 可選用 Windows Terminal 的新版 ConPTY 主機（`conpty.dll` + `OpenConsole.exe`）。
//!
//! 照搬舊版 `ConPty/ConptyDll.cs` 的行為：
//! 放在 exe 旁的 `conpty\` 子目錄（`conpty.dll` 會在自己旁邊找 `OpenConsole.exe`），
//! 兩個檔任一缺少就靜靜退回 kernel32 的 `CreatePseudoConsole`（Win10 內建 conhost）。
//! 測試／除錯用環境變數：`AWAYTERMINAL_CONPTY_DIR` 指定目錄、`AWAYTERMINAL_CONPTY=inbox` 強制內建。
//!
//! 為什麼要換掉內建 conhost（舊版 `third_party/conpty/README.md`）：Win10 的 2019 年版 conhost
//! 會用絕對定位重繪整個畫面（＝輸入二倍／殘影）、吞掉 bracketed-paste 標記、攤平 alt-screen、
//! 把軟換行變硬換行，還會吃掉 lone ESC 後面的多字元輸入（Claude Code「按過 Esc 再打字整句消失」）。

use std::path::PathBuf;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Console::COORD;

type CreateFn = unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut isize) -> i32;
type ResizeFn = unsafe extern "system" fn(isize, COORD) -> i32;
type ClosePcFn = unsafe extern "system" fn(isize);
type ReleaseFn = unsafe extern "system" fn(isize);

pub struct ConptyHost {
    // 保持 Library 活著；卸載後函式指標就無效了
    _lib: libloading::Library,
    create: CreateFn,
    resize: ResizeFn,
    close: ClosePcFn,
    release: ReleaseFn,
    dir: PathBuf,
}

// 函式指標指向常駐的 dll，可跨執行緒使用
unsafe impl Send for ConptyHost {}
unsafe impl Sync for ConptyHost {}

impl ConptyHost {
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// # Safety
    /// `input` / `output` 必須是有效的管線 handle，且在這條 pseudoconsole 存活期間有效。
    pub unsafe fn create(
        &self,
        size: COORD,
        input: HANDLE,
        output: HANDLE,
        flags: u32,
    ) -> Result<isize, i32> {
        let mut hpc: isize = 0;
        let hr = (self.create)(size, input, output, flags, &mut hpc);
        if hr == 0 {
            Ok(hpc)
        } else {
            Err(hr)
        }
    }

    /// # Safety
    /// `hpc` 必須是本物件 `create` 回傳、且尚未 `close` 的 handle。
    pub unsafe fn resize(&self, hpc: isize, size: COORD) -> i32 {
        (self.resize)(hpc, size)
    }

    /// # Safety
    /// `hpc` 必須是本物件 `create` 回傳、且只 close 一次。
    pub unsafe fn close(&self, hpc: isize) {
        (self.close)(hpc)
    }

    /// 子行程掛上 pseudoconsole 之後呼叫：放掉 conpty.dll 持有的參考 handle，
    /// 之後最後一個 client 離開時 OpenConsole 會自己結束並關閉輸出管線。
    /// （內建 conhost 沒有這個 API，也不會自己關輸出管線——所以行程結束一律靠等
    ///  process handle 偵測，見 `conpty.rs` 的 waiter thread。）
    /// # Safety
    /// `hpc` 必須是本物件 `create` 回傳的 handle，且子行程已經掛上。
    pub unsafe fn release(&self, hpc: isize) {
        (self.release)(hpc)
    }
}

static HOST: OnceLock<Option<ConptyHost>> = OnceLock::new();

/// 已載入新版 conpty.dll（且 OpenConsole.exe 在旁）時回傳它，否則 None＝用內建 conhost。
pub fn host() -> Option<&'static ConptyHost> {
    HOST.get_or_init(load).as_ref()
}

/// 診斷字串，啟動時記進 log。
pub fn backend_name() -> String {
    match host() {
        Some(h) => format!("conpty.dll (OpenConsole) {}", h.dir().display()),
        None => "inbox conhost".to_string(),
    }
}

fn load() -> Option<ConptyHost> {
    if std::env::var("AWAYTERMINAL_CONPTY")
        .map(|v| v.eq_ignore_ascii_case("inbox"))
        .unwrap_or(false)
    {
        return None;
    }

    for dir in candidate_dirs() {
        let dll = dir.join("conpty.dll");
        if !dll.is_file() || !dir.join("OpenConsole.exe").is_file() {
            continue;
        }
        if let Some(h) = load_from(&dll, &dir) {
            return Some(h);
        }
    }
    None
}

fn load_from(dll: &std::path::Path, dir: &std::path::Path) -> Option<ConptyHost> {
    unsafe {
        let lib = libloading::Library::new(dll).ok()?;
        let create = *lib.get::<CreateFn>(b"ConptyCreatePseudoConsole\0").ok()?;
        let resize = *lib.get::<ResizeFn>(b"ConptyResizePseudoConsole\0").ok()?;
        let close = *lib.get::<ClosePcFn>(b"ConptyClosePseudoConsole\0").ok()?;
        let release = *lib.get::<ReleaseFn>(b"ConptyReleasePseudoConsole\0").ok()?;
        Some(ConptyHost {
            _lib: lib,
            create,
            resize,
            close,
            release,
            dir: dir.to_path_buf(),
        })
    }
}

/// 搜尋順序。舊版只看 exe 旁的 `conpty\`；這裡多兩個是因為 Rust 的產出佈局：
/// `cargo run --example` 的 exe 在 `target/<profile>/examples/`，
/// 而 `cargo test` 根本不會產生「exe 旁」的複製。
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(d) = std::env::var("AWAYTERMINAL_CONPTY_DIR") {
        if !d.trim().is_empty() {
            dirs.push(PathBuf::from(d));
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            dirs.push(exe_dir.join("conpty"));
            // target/<profile>/examples/foo.exe → target/<profile>/conpty
            if let Some(up) = exe_dir.parent() {
                dirs.push(up.join("conpty"));
            }
        }
    }

    // dev / cargo test：直接用原始碼樹裡的那份（release 不編這段）
    if cfg!(debug_assertions) {
        dirs.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("conpty"),
        );
    }

    dirs
}
