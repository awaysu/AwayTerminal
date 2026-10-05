//! 字型的**取得**：自帶字型的檔案內容、下載更多中文等寬字型、匯入本機字型、移除。
//!
//! 掃描與清單在 [`crate::fonts`]；這裡只管「檔案怎麼來的、放哪裡」。
//!
//! | 來源 | 放哪裡 | 誰寫的 |
//! |---|---|---|
//! | 自帶 | Tauri resources 的 `fonts/`（唯讀） | 安裝檔 |
//! | 下載 | `<設定資料夾>/fonts/` | [`font_download`] |
//! | 匯入 | `<設定資料夾>/fonts/` | [`font_import`] |
//!
//! **不安裝到系統、不需要管理員權限**：webview 用 `@font-face` 直接吃檔案內容
//! （前端拿 [`font_face_bytes`] 回的位元組建 `FontFace`），所以字型只對這個程式有效，
//! 解除安裝把資料夾刪掉就乾淨了。
//!
//! ⚠️ **下載只連白名單裡的網址**（[`CATALOG`] 寫死，而且一律 `https://`）：
//! 這是程式裡除了「檢查更新」以外唯一會連外的功能，而且只有使用者自己按下去才會跑。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use crate::fonts;

/// 下載逾時（整個請求）。CJK 字型十幾 MB，慢一點的線路要久一點。
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// 可以下載的中文等寬字型。**全部是官方 repo 的直接檔案連結**（不是壓縮檔，
/// 所以不必為了解壓多帶一個 zip／7z 解碼器）。`bytes` 是寫這份清單時量到的大小，
/// 只拿來顯示；真正的進度用伺服器回的 `Content-Length`。
///
/// ⚠️ Sarasa Mono TC 的 **Bold** 官方只出在 50 MB 的 `.7z` 裡，沒有單檔連結 →
/// 不放進來（要解 7z 得多帶一個解碼器）。自帶的是 Regular，粗體由 webview 合成。
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "cascadia-next-tc",
        family: "Cascadia Next TC",
        file: "CascadiaNextTC.wght.ttf",
        url: "https://github.com/microsoft/cascadia-code/releases/download/cascadia-next/CascadiaNextTC.wght.ttf",
        bytes: 6_833_112,
        license: "SIL Open Font License 1.1",
        by: "Microsoft",
    },
    CatalogEntry {
        id: "lxgw-wenkai-mono-tc",
        family: "LXGW WenKai Mono TC",
        file: "LXGWWenKaiMonoTC-Regular.ttf",
        url: "https://github.com/lxgw/LxgwWenKaiTC/releases/download/v1.522/LXGWWenKaiMonoTC-Regular.ttf",
        bytes: 15_277_228,
        license: "SIL Open Font License 1.1",
        by: "LXGW",
    },
    CatalogEntry {
        id: "noto-sans-mono-cjk-tc",
        family: "Noto Sans Mono CJK TC",
        file: "NotoSansMonoCJKtc-Regular.otf",
        url: "https://raw.githubusercontent.com/notofonts/noto-cjk/Sans2.004/Sans/Mono/NotoSansMonoCJKtc-Regular.otf",
        bytes: 16_392_304,
        license: "SIL Open Font License 1.1",
        by: "Google",
    },
];

/// 清單裡的一筆。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    /// 前端用的代號（也是取消時認的 key）
    pub id: &'static str,
    /// 下載完之後會出現在字型清單裡的家族名
    pub family: &'static str,
    /// 存檔名（放進 `<設定資料夾>/fonts/`）
    pub file: &'static str,
    /// 下載網址（**一定是 https**，`font_download` 會再檢查一次）
    pub url: &'static str,
    /// 大小（寫清單時量的，只拿來顯示）
    pub bytes: u64,
    pub license: &'static str,
    pub by: &'static str,
}

/// 下載進度（`font-download` 事件）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    /// 已經收到幾個位元組
    pub got: u64,
    /// 總共幾個位元組（伺服器沒給 `Content-Length` 就是 0）
    pub total: u64,
    pub done: bool,
    /// 有值＝失敗或取消的原因
    pub error: Option<String>,
}

/// 正在下載的那些（id → 取消旗標）。同一個 id 不會同時跑兩份。
type Running = Mutex<Vec<(String, Arc<AtomicBool>)>>;

fn running() -> &'static Running {
    static R: OnceLock<Running> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Vec::new()))
}
use std::sync::OnceLock;

/// 可以下載的清單（設定視窗的「下載更多中文等寬字型」）。
#[tauri::command]
pub fn font_catalog() -> Vec<CatalogEntry> {
    CATALOG.to_vec()
}

/// 前端要用 `@font-face` 載入的檔案清單。
///
/// 自帶的照 [`fonts::BUILTIN`]；使用者下載／匯入的把 `<設定資料夾>/fonts/` 掃一遍。
/// `weight`／`style` 從檔名猜（我們自己放的檔名都很規矩），猜不到就當 400／normal——
/// 猜錯最多是粗體由 webview 合成，不會出錯。
#[tauri::command]
pub fn font_faces() -> Vec<FaceFile> {
    let mut out = Vec::new();
    if let Some(dir) = fonts::resource_dir() {
        for (file, family) in fonts::BUILTIN {
            let path = dir.join(file);
            if path.is_file() {
                out.push(FaceFile::new(family.to_string(), &path, true));
            }
        }
    }
    if let Some(dir) = fonts::user_dir() {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for ent in rd.flatten() {
                let path = ent.path();
                if !is_font_file(&path) {
                    continue;
                }
                let Ok(data) = std::fs::read(&path) else { continue };
                // 使用者的檔案檔名不可信 → 家族名一律讀 `name` 表；
                // 一個檔只登記第一個家族（`.ttc` 之後再說）
                if let Some((family, _)) = fonts::families_in(&data).into_iter().next() {
                    out.push(FaceFile::new(family, &path, false));
                }
            }
        }
    }
    out
}

/// 一個要載入 webview 的字型檔。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceFile {
    pub family: String,
    /// 完整路徑（前端拿去跟 [`font_face_bytes`] 要內容）
    pub path: String,
    /// `400` / `700`
    pub weight: u32,
    /// `normal` / `italic`
    pub style: String,
    pub builtin: bool,
    pub bytes: u64,
}

impl FaceFile {
    fn new(family: String, path: &Path, builtin: bool) -> Self {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        let weight = if name.contains("bold") { 700 } else { 400 };
        let style = if name.contains("italic") || name.contains("oblique") {
            "italic"
        } else {
            "normal"
        };
        Self {
            family,
            path: path.to_string_lossy().to_string(),
            weight,
            style: style.to_string(),
            builtin,
            bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        }
    }
}

/// 一個字型檔的內容（前端用它建 `FontFace`）。
///
/// **只給自帶與使用者字型資料夾底下的檔案**：路徑要正規化之後真的落在那兩個資料夾裡，
/// 不然這就變成「前端可以叫後端讀任何檔案」。
#[tauri::command]
pub fn font_face_bytes(path: String) -> Result<tauri::ipc::Response, String> {
    let want = std::fs::canonicalize(&path).map_err(|e| format!("{path}：{e}"))?;
    let allowed = [fonts::resource_dir(), fonts::user_dir()]
        .into_iter()
        .flatten()
        .filter_map(|d| std::fs::canonicalize(d).ok())
        .any(|d| want.starts_with(&d));
    if !allowed {
        return Err(crate::i18n::t("font.notOurs"));
    }
    let data = std::fs::read(&want).map_err(|e| format!("{}：{e}", want.display()))?;
    Ok(tauri::ipc::Response::new(data))
}

fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| fonts::FONT_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// 選字型檔（「匯入字型…」）。可以一次選好幾個。
///
/// 篩選器照 [`fonts::FONT_EXTS`]；`None` ＝使用者按了取消。
#[tauri::command]
pub async fn font_pick_files(app: AppHandle, title: Option<String>) -> Option<Vec<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .set_title(title.unwrap_or_else(|| crate::i18n::t("font.pickTitle")))
        .add_filter(crate::i18n::t("font.pickFilter"), fonts::FONT_EXTS)
        .add_filter(crate::i18n::t("dlg.allFiles"), &["*"])
        .pick_files(move |f| {
            let _ = tx.send(f);
        });
    let picked = tokio::task::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .ok()
        .flatten()?;
    Some(picked.into_iter().map(|p| p.to_string()).collect())
}

/// 匯入本機的字型檔（複製到 `<設定資料夾>/fonts/`，**不安裝到系統**）。
///
/// 回傳真的加進來的家族名。認不出家族名（不是字型、或是符號字型）的檔案會被拒絕，
/// 而且**不會留下半個檔案**。
#[tauri::command]
pub fn font_import(paths: Vec<String>) -> Result<Vec<String>, String> {
    let dir = fonts::user_dir().ok_or_else(|| crate::i18n::t("font.noDir"))?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut added: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for p in paths {
        let src = PathBuf::from(&p);
        if !is_font_file(&src) {
            errors.push(crate::i18n::tf("font.badExt", &[&p]));
            continue;
        }
        let data = match std::fs::read(&src) {
            Ok(d) => d,
            Err(e) => {
                errors.push(format!("{p}：{e}"));
                continue;
            }
        };
        let fams = fonts::families_in(&data);
        if fams.is_empty() {
            errors.push(crate::i18n::tf("font.notAFont", &[&p]));
            continue;
        }
        let name = src.file_name().unwrap_or_default().to_string_lossy().to_string();
        let dest = unique_path(dir, &name);
        if let Err(e) = std::fs::write(&dest, &data) {
            errors.push(format!("{p}：{e}"));
            continue;
        }
        for (fam, _) in fams {
            if !added.contains(&fam) {
                added.push(fam);
            }
        }
    }
    fonts::invalidate();
    println!("[AwayTerminal] 匯入字型：{} 套（{}）", added.len(), added.join("、"));
    if added.is_empty() && !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(added)
}

/// 同名檔案就在後面加 `-2`、`-3`…（不要蓋掉使用者已經有的）。
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    if !path.exists() {
        return path;
    }
    let stem = Path::new(name).file_stem().unwrap_or_default().to_string_lossy().to_string();
    let ext = Path::new(name).extension().unwrap_or_default().to_string_lossy().to_string();
    for n in 2..1000 {
        let p = dir.join(format!("{stem}-{n}.{ext}"));
        if !p.exists() {
            return p;
        }
    }
    path
}

/// 下載清單裡的一套字型。進度走 `font-download` 事件（[`Progress`]）。
///
/// 下載到暫存檔，**確認真的是字型（讀得出家族名）才改成正式檔名**——
/// 半途斷線不會在資料夾裡留下一個壞掉的 `.ttf`。
#[tauri::command]
pub async fn font_download(app: AppHandle, id: String) -> Result<String, String> {
    let entry = CATALOG
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| crate::i18n::tf("font.unknownId", &[&id]))?;
    // 白名單之外一律拒絕（清單是寫死的，這裡是第二道）
    if !entry.url.starts_with("https://") {
        return Err(crate::i18n::t("font.notHttps"));
    }
    let dir = fonts::user_dir().ok_or_else(|| crate::i18n::t("font.noDir"))?.to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut r = running().lock().unwrap_or_else(|e| e.into_inner());
        if r.iter().any(|(k, _)| *k == id) {
            return Err(crate::i18n::t("font.busy"));
        }
        r.push((id.clone(), cancel.clone()));
    }

    // 不管怎麼結束（含下載執行緒 panic、`?` 提早回傳）都要把自己從 `running` 拿掉，
    // 否則這個字型之後永遠「下載中」（BUG D10）→ 用 Drop 收尾
    struct Unregister(String);
    impl Drop for Unregister {
        fn drop(&mut self) {
            running()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(k, _)| *k != self.0);
        }
    }
    let _unregister = Unregister(id.clone());

    let result = tokio::task::spawn_blocking(move || download_blocking(&app, entry, &dir, &cancel))
        .await
        .map_err(|e| e.to_string())?;
    fonts::invalidate();
    result
}

/// 一個字型檔最多收這麼多位元組。目錄裡最大的也才幾 MB；`Content-Length` 是對方說了算，
/// 不能照單全收拿去預先配置記憶體（BUG D10）。
const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;

/// 取消正在跑的下載。回 `true`＝真的有一個在跑。
#[tauri::command]
pub fn font_download_cancel(id: String) -> bool {
    let r = running().lock().unwrap_or_else(|e| e.into_inner());
    for (k, flag) in r.iter() {
        if *k == id {
            flag.store(true, Ordering::SeqCst);
            return true;
        }
    }
    false
}

fn download_blocking(
    app: &AppHandle,
    entry: &CatalogEntry,
    dir: &Path,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let emit = |got: u64, total: u64, done: bool, error: Option<String>| {
        let _ = app.emit(
            "font-download",
            Progress { id: entry.id.to_string(), got, total, done, error },
        );
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("AwayTerminal/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let resp = agent.get(entry.url).call().map_err(|e| {
        let msg = describe(&e);
        emit(0, 0, true, Some(msg.clone()));
        msg
    })?;
    let total: u64 = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(entry.bytes);

    let tmp = dir.join(format!("{}.part", entry.file));
    let mut body = resp.into_body().into_reader();
    let mut buf = vec![0u8; 256 * 1024];
    if total > MAX_FONT_BYTES {
        let msg = crate::i18n::tf("font.tooLarge", &[&(MAX_FONT_BYTES / 1024 / 1024).to_string()]);
        emit(0, total, true, Some(msg.clone()));
        return Err(msg);
    }
    let mut data: Vec<u8> = Vec::with_capacity(total as usize);
    let mut last = std::time::Instant::now();
    emit(0, total, false, None);
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(&tmp);
            let msg = crate::i18n::t("font.cancelled");
            emit(data.len() as u64, total, true, Some(msg.clone()));
            return Err(msg);
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                let msg = e.to_string();
                emit(data.len() as u64, total, true, Some(msg.clone()));
                return Err(msg);
            }
        };
        data.extend_from_slice(&buf[..n]);
        // 沒給 Content-Length、或給了卻送得比說的多 → 一樣要有上限
        if data.len() as u64 > MAX_FONT_BYTES {
            let _ = std::fs::remove_file(&tmp);
            let msg = crate::i18n::tf("font.tooLarge", &[&(MAX_FONT_BYTES / 1024 / 1024).to_string()]);
            emit(data.len() as u64, total, true, Some(msg.clone()));
            return Err(msg);
        }
        // 進度事件別太密（每 200ms 一次就夠畫進度條了）
        if last.elapsed() >= std::time::Duration::from_millis(200) {
            last = std::time::Instant::now();
            emit(data.len() as u64, total, false, None);
        }
    }

    // 真的是字型嗎？（連到錯的東西、或被中間人塞了一頁 HTML）
    let fams = fonts::families_in(&data);
    if !fams.iter().any(|(n, _)| n == entry.family) {
        let msg = crate::i18n::tf("font.wrongFont", &[entry.family]);
        emit(data.len() as u64, total, true, Some(msg.clone()));
        return Err(msg);
    }
    std::fs::write(dir.join(entry.file), &data).map_err(|e| {
        let msg = e.to_string();
        emit(data.len() as u64, total, true, Some(msg.clone()));
        msg
    })?;
    emit(data.len() as u64, total, true, None);
    println!(
        "[AwayTerminal] 下載字型完成：{}（{} 位元組）",
        entry.family,
        data.len()
    );
    Ok(entry.family.to_string())
}

/// `ureq` 的錯誤訊息裡**不要有 URL**（同 `telegram::api::describe` 的理由：
/// 那裡是 token，這裡只是乾淨——使用者看到一長串網址也沒有幫助）。
fn describe(e: &ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(code) => format!("HTTP {code}"),
        other => {
            // 變體很多而且會隨版本增加，只取型別名那一段
            let s = other.to_string();
            s.split(':').next().unwrap_or("error").trim().to_string()
        }
    }
}

/// 移除**使用者自己下載／匯入**的字型（自帶的不能刪）。回傳刪掉幾個檔案。
#[tauri::command]
pub fn font_remove(family: String) -> Result<usize, String> {
    let dir = fonts::user_dir().ok_or_else(|| crate::i18n::t("font.noDir"))?;
    let mut removed = 0usize;
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(0) };
    for ent in rd.flatten() {
        let path = ent.path();
        if !is_font_file(&path) {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else { continue };
        if fonts::families_in(&data).iter().any(|(n, _)| *n == family) {
            if let Err(e) = std::fs::remove_file(&path) {
                return Err(format!("{}：{e}", path.display()));
            }
            removed += 1;
        }
    }
    fonts::invalidate();
    println!("[AwayTerminal] 移除字型：{family}（{removed} 個檔案）");
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 清單裡的網址一律 https，而且 id／檔名不重複。
    #[test]
    fn catalog_is_https_and_unique() {
        let mut ids = std::collections::HashSet::new();
        let mut files = std::collections::HashSet::new();
        for e in CATALOG {
            assert!(e.url.starts_with("https://"), "{} 不是 https", e.id);
            assert!(ids.insert(e.id), "id 重複：{}", e.id);
            assert!(files.insert(e.file), "檔名重複：{}", e.file);
            assert!(e.bytes > 0, "{} 沒填大小", e.id);
            assert!(!e.family.is_empty() && !e.license.is_empty());
        }
    }

    /// 可下載的字型不可以和自帶的撞家族名（會分不清哪一個生效）。
    #[test]
    fn catalog_does_not_shadow_builtin() {
        for e in CATALOG {
            assert!(
                !fonts::builtin_families().contains(&e.family),
                "{} 和自帶字型同名",
                e.family
            );
        }
    }

    /// `FaceFile` 從檔名猜粗細／斜體。
    #[test]
    fn weight_and_style_from_file_name() {
        let f = FaceFile::new("X".into(), Path::new("C:/x/JetBrainsMono-Bold.ttf"), true);
        assert_eq!(f.weight, 700);
        assert_eq!(f.style, "normal");
        let r = FaceFile::new("X".into(), Path::new("C:/x/SarasaMonoTC-Regular.ttf"), true);
        assert_eq!(r.weight, 400);
        let i = FaceFile::new("X".into(), Path::new("C:/x/Foo-BoldItalic.otf"), false);
        assert_eq!(i.weight, 700);
        assert_eq!(i.style, "italic");
    }

    /// 同名就往後編號，不會蓋掉已經有的檔案。
    #[test]
    fn unique_path_does_not_overwrite() {
        let dir = std::env::temp_dir().join("awayterm-fontstore-test");
        let _ = std::fs::create_dir_all(&dir);
        let first = dir.join("a.ttf");
        let _ = std::fs::write(&first, b"x");
        let next = unique_path(&dir, "a.ttf");
        assert_ne!(next, first);
        assert_eq!(next.file_name().unwrap(), "a-2.ttf");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 字型資料夾以外的檔案不給讀（不然前端就能叫後端讀任何檔案）。
    #[test]
    fn face_bytes_refuses_paths_outside_the_font_dirs() {
        let outside = std::env::temp_dir().join("awayterm-not-a-font-dir.ttf");
        let _ = std::fs::write(&outside, b"x");
        let r = font_face_bytes(outside.to_string_lossy().to_string());
        assert!(r.is_err(), "字型資料夾以外的檔案不應該讀得到");
        let _ = std::fs::remove_file(&outside);
    }
}
