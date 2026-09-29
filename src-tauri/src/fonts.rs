//! 字型清單（設定視窗的「字型」下拉）與程式自帶／下載／匯入的字型。
//!
//! 舊版 WPF 用 `Fonts.SystemFontFamilies` 一行就拿到整台機器的字型家族。
//! Tauri／webview 沒有這種 API（`queryLocalFonts` 要權限，WebView2 上不保證有），
//! 所以這裡自己掃：**走各平台的字型目錄，把每個字型檔的 `name` 表讀出來**。
//!
//! | 取什麼 | 從哪裡 | 為什麼 |
//! |---|---|---|
//! | 家族名 | `name` 表的 ID 16（Typographic Family），沒有才退回 ID 1（Family） | ID 1 會把粗細切開（「Roboto Light」自成一族），ID 16 才是使用者心裡的「Roboto」 |
//! | 語言 | 優先 Windows 平台 + 英文（0x0409），再退回任何看得懂的 | CSS `font-family` 吃英文家族名最保險；中文字型的中文名在 WebView2 認得、在 WebKit 不一定 |
//! | 等寬 | `post` 表的 `isFixedPitch` | 比量字寬便宜也準（量字寬還要 `cmap` + `hmtx`，而且中文字型的 ASCII 半寬會誤判） |
//!
//! **三種來源**（[`FontSource`]，下拉就照這個分組）：
//!   * `builtin` — 程式自帶，跟著安裝檔走，**不安裝到系統**（`src-tauri/fonts/`，Tauri resources）
//!   * `user` — 使用者下載或匯入的（`<設定資料夾>/fonts/`）
//!   * `system` — 這台機器裝的
//!
//! **符號字型不列出來**（Wingdings、Webdings、Marlett、Symbol、Segoe MDL2 Assets…）：
//! 它們拿來當終端機字型只會滿畫面小圖，清單裡多 10 幾個雜訊。判斷見 [`has_latin`]。
//!
//! **速度**：Windows 的 `Fonts` 資料夾在開發機上是幾百 MB，整個讀完要半秒多，
//! 所以掃描結果進 [`CACHE`]，而且啟動時就在背景執行緒暖機（[`warm_up`]）。
//! 匯入／下載／移除之後呼叫 [`invalidate`] 讓下一次重掃。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 字型從哪裡來。下拉就照這個分四組。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FontSource {
    /// 程式自帶（安裝檔裡就有，不安裝到系統）
    Builtin,
    /// 使用者下載或匯入的（`<設定資料夾>/fonts/`）
    User,
    /// 這台機器上裝的
    System,
}

/// 一個字型家族。`mono` ＝等寬（下拉裡排前面並分組）。
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FontFamily {
    pub name: String,
    pub mono: bool,
    pub source: FontSource,
}

/// 程式自帶的字型（檔名 → 家族名）。家族名是用 `font_probe` 讀出來的，
/// **必須和 `name` 表裡的一模一樣**，CSS `font-family` 才選得到。
///
/// | 檔案 | 家族 | 大小 | 授權 |
/// |---|---|---|---|
/// | `JetBrainsMono-Regular/Bold.ttf` | JetBrains Mono | 268 + 271 KB | OFL-1.1 |
/// | `CascadiaMono-Regular/Bold.ttf` | Cascadia Mono | 562 + 568 KB | OFL-1.1 |
/// | `SarasaMonoTC-Regular.ttf` | Sarasa Mono TC | 13.5 MB | OFL-1.1 |
///
/// ⚠️ **Sarasa 只帶 Regular**（TASK-033 的取捨）：Bold 再多 13.3 MB，安裝檔會從
/// 約 10 MB 變成約 37 MB。粗體由 webview 合成（終端機的粗體本來就多半是合成的）。
/// 想要真的 Bold 的人可以用「下載更多中文等寬字型」把它抓下來。
pub const BUILTIN: &[(&str, &str)] = &[
    ("JetBrainsMono-Regular.ttf", "JetBrains Mono"),
    ("JetBrainsMono-Bold.ttf", "JetBrains Mono"),
    ("CascadiaMono-Regular.ttf", "Cascadia Mono"),
    ("CascadiaMono-Bold.ttf", "Cascadia Mono"),
    ("SarasaMonoTC-Regular.ttf", "Sarasa Mono TC"),
];

/// 內建字型的家族名（去重、保持上面的順序）。前端要用它做「內建」那一組。
pub fn builtin_families() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (_, fam) in BUILTIN {
        if !out.contains(fam) {
            out.push(fam);
        }
    }
    out
}

/// 掃描結果的快取。`None` ＝還沒掃／已作廢。
static CACHE: Mutex<Option<Vec<FontFamily>>> = Mutex::new(None);

/// 自帶字型的資料夾（Tauri resources 底下的 `fonts/`）。啟動時由 [`init`] 設定。
static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();
/// 使用者下載／匯入的字型資料夾（`<設定資料夾>/fonts/`）。啟動時由 [`init`] 設定。
static USER_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 單一字型檔的大小上限。正常的字型檔最大也就幾十 MB（Noto CJK 全集約 20 MB）；
/// 超過的一律跳過，免得有人在字型資料夾裡放了奇怪的東西就把掃描卡住。
const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;

/// 掃描的目錄深度。Linux 是 `fonts/truetype/<family>/*.ttf` 的形狀，三層夠用。
const MAX_DEPTH: u32 = 3;

/// 認得的副檔名。`.ttc`／`.otc` 是字型集合（一個檔裡好幾套，要逐一取）。
pub const FONT_EXTS: &[&str] = &["ttf", "otf", "ttc", "otc"];

/// 啟動時把兩個資料夾告訴這個模組（`lib.rs` 的 `setup`）。
///
/// `resources` ＝自帶字型（唯讀）；`user` ＝下載／匯入的（會被寫入，這裡順手建好）。
pub fn init(resources: PathBuf, user: PathBuf) {
    let _ = std::fs::create_dir_all(&user);
    let _ = RESOURCE_DIR.set(resources);
    let _ = USER_DIR.set(user);
}

/// 自帶字型資料夾。沒設定過（單元測試）就回 `None`。
pub fn resource_dir() -> Option<&'static Path> {
    RESOURCE_DIR.get().map(|p| p.as_path())
}

/// 使用者下載／匯入的字型資料夾。
pub fn user_dir() -> Option<&'static Path> {
    USER_DIR.get().map(|p| p.as_path())
}

/// 這台機器上可以用的字型家族，**等寬在前、其餘在後，各自依名稱排序**。
///
/// 第一次呼叫（或 [`invalidate`] 之後）會真的去掃，之後都是快取。
pub fn families() -> Vec<FontFamily> {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(scan());
    }
    guard.clone().unwrap_or_default()
}

/// 讓快取作廢（匯入／下載／移除字型之後）。下一次 [`families`] 會重掃。
pub fn invalidate() {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

/// 啟動時在背景執行緒先掃一遍，讓「其他設定」按下去是即時的。
///
/// 掃描只讀檔、不改任何東西，失敗也只是回到「開設定時才掃」。
pub fn warm_up() {
    std::thread::Builder::new()
        .name("font-scan".into())
        .spawn(|| {
            let t0 = std::time::Instant::now();
            let list = families();
            let builtin = list.iter().filter(|f| f.source == FontSource::Builtin).count();
            let user = list.iter().filter(|f| f.source == FontSource::User).count();
            println!(
                "[AwayTerminal] 字型清單：{} 個家族（內建 {builtin}、下載/匯入 {user}），掃描 {} ms",
                list.len(),
                t0.elapsed().as_millis()
            );
        })
        .ok();
}

/// 各平台的系統字型目錄。**使用者自己裝的也要**（Windows 的
/// `%LOCALAPPDATA%\Microsoft\Windows\Fonts` ＝「只為我安裝」的那些）。
fn system_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        dirs.push(PathBuf::from(windir).join("Fonts"));
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        for d in [
            "/System/Library/Fonts",
            "/System/Library/Fonts/Supplemental",
            "/Library/Fonts",
        ] {
            dirs.push(PathBuf::from(d));
        }
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for d in ["/usr/share/fonts", "/usr/local/share/fonts"] {
            dirs.push(PathBuf::from(d));
        }
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(&home).join(".local/share/fonts"));
            dirs.push(PathBuf::from(&home).join(".fonts"));
        }
    }
    dirs
}

/// 真正的掃描。**家族名去重**：同一族的 Regular／Bold／Italic 各是一個檔，
/// 只要有任何一個檔說自己是等寬，整族就算等寬（Regular 會是等寬，Italic 偶爾漏標）。
///
/// 來源的優先序是 builtin > user > system：同名時算前面那個
/// （自帶的 Sarasa 和系統裝的 Sarasa 撞名時，要顯示成「內建」）。
fn scan() -> Vec<FontFamily> {
    let mut found: BTreeMap<String, FontFamily> = BTreeMap::new();

    // 1) 自帶（照 BUILTIN 的表，不用掃目錄——檔案就是我們自己放的）
    if let Some(dir) = resource_dir() {
        for (file, _) in BUILTIN {
            add_file(&dir.join(file), FontSource::Builtin, &mut found);
        }
    }
    // 2) 使用者下載／匯入的
    if let Some(dir) = user_dir() {
        walk(dir, MAX_DEPTH, FontSource::User, &mut found);
    }
    // 3) 系統
    for dir in system_dirs() {
        walk(&dir, MAX_DEPTH, FontSource::System, &mut found);
    }

    let mut out: Vec<FontFamily> = found.into_values().collect();
    // 等寬在前，各組依名稱（不分大小寫）排序
    out.sort_by(|a, b| {
        b.mono
            .cmp(&a.mono)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    if out.is_empty() {
        // 一個都掃不到（權限、容器、剝掉字型的系統…）：給一份通用的，
        // 使用者仍然可以自己打（下拉最後一項是「自訂…」）。
        out = ["Cascadia Mono", "Consolas", "Menlo", "DejaVu Sans Mono", "Courier New", "monospace"]
            .iter()
            .map(|s| FontFamily { name: (*s).to_string(), mono: true, source: FontSource::System })
            .collect();
    }
    out
}

fn walk(dir: &Path, depth: u32, source: FontSource, found: &mut BTreeMap<String, FontFamily>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        let path = ent.path();
        let Ok(ft) = ent.file_type() else { continue };
        if ft.is_dir() {
            if depth > 0 {
                walk(&path, depth - 1, source, found);
            }
            continue;
        }
        add_file(&path, source, found);
    }
}

/// 一個檔案（不是字型就跳過）。
fn add_file(path: &Path, source: FontSource, found: &mut BTreeMap<String, FontFamily>) {
    let is_font = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| FONT_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false);
    if !is_font {
        return;
    }
    if std::fs::metadata(path).map(|m| m.len() > MAX_FONT_BYTES).unwrap_or(true) {
        return;
    }
    let Ok(data) = std::fs::read(path) else { return };
    for (name, mono) in families_in(&data) {
        let key = name.to_lowercase();
        match found.get_mut(&key) {
            // 已經有了：只補「有沒有任何一個檔說自己是等寬」，來源不改（builtin 先掃，優先）
            Some(e) => e.mono |= mono,
            None => {
                found.insert(key, FontFamily { name, mono, source });
            }
        }
    }
}

/// 一個字型檔裡的所有家族（`.ttc` 可能有好幾套）。公開是為了讓 `font_probe` 與測試能用。
///
/// **符號字型直接跳過**（見 [`has_latin`]）。
pub fn families_in(data: &[u8]) -> Vec<(String, bool)> {
    let count = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    let mut out = Vec::new();
    for i in 0..count {
        let Ok(face) = ttf_parser::Face::parse(data, i) else { continue };
        if !has_latin(&face) {
            continue;
        }
        let Some(name) = family_name(&face) else { continue };
        out.push((name, face.is_monospaced()));
    }
    out
}

/// 「這個字型打得出英文字母嗎」——打不出來的就是符號／圖示字型，不列進清單。
///
/// 兩種都要擋（都出現在一般的 Windows 上）：
///   * **舊式符號字型**（Wingdings、Webdings、Symbol、Marlett、MS Reference Specialty…）：
///     cmap 只有 Windows/Symbol（平台 3、編碼 0）那張表，沒有 Unicode 的。
///   * **圖示字型**（Segoe MDL2 Assets、Segoe Fluent Icons…）：有 Unicode cmap，
///     但只對到私用區（U+E000–），`A`／`a` 根本沒有字。
fn has_latin(face: &ttf_parser::Face<'_>) -> bool {
    let Some(cmap) = face.tables().cmap else { return false };
    if !cmap.subtables.into_iter().any(|s| s.is_unicode()) {
        return false;
    }
    face.glyph_index('A').is_some() && face.glyph_index('a').is_some()
}

/// 家族名：先找 Typographic Family（ID 16），沒有才用 Family（ID 1）；
/// 同一個 ID 裡優先取英文的那筆。
fn family_name(face: &ttf_parser::Face<'_>) -> Option<String> {
    const TYPOGRAPHIC_FAMILY: u16 = 16;
    const FAMILY: u16 = 1;
    for want in [TYPOGRAPHIC_FAMILY, FAMILY] {
        let mut fallback: Option<String> = None;
        for name in face.names() {
            if name.name_id != want {
                continue;
            }
            let Some(text) = name.to_string() else { continue };
            let text = text.trim().to_string();
            if text.is_empty() {
                continue;
            }
            // Windows 平台（3）的英文（0x0409）＝最可靠的英文家族名；
            // Macintosh 平台（1）的 language_id 0 也是英文。
            let english = (name.platform_id == ttf_parser::PlatformId::Windows
                && name.language_id == 0x0409)
                || (name.platform_id == ttf_parser::PlatformId::Macintosh && name.language_id == 0);
            if english {
                return Some(text);
            }
            fallback.get_or_insert(text);
        }
        if fallback.is_some() {
            return fallback;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 掃描不會回空的（真的掃不到也有那份通用清單）。
    #[test]
    fn never_empty() {
        assert!(!families().is_empty());
    }

    /// 等寬一定排在非等寬前面，而且同一組內是照名稱排的。
    #[test]
    fn mono_first_then_sorted() {
        let list = families();
        let first_non_mono = list.iter().position(|f| !f.mono).unwrap_or(list.len());
        assert!(list[..first_non_mono].iter().all(|f| f.mono), "等寬那一段裡混進了非等寬");
        assert!(list[first_non_mono..].iter().all(|f| !f.mono), "非等寬那一段裡混進了等寬");
        for part in [&list[..first_non_mono], &list[first_non_mono..]] {
            for w in part.windows(2) {
                assert!(
                    w[0].name.to_lowercase() <= w[1].name.to_lowercase(),
                    "沒照名稱排：{} 在 {} 前面",
                    w[0].name,
                    w[1].name
                );
            }
        }
    }

    /// 家族名不重複（同一族的 Regular／Bold 是不同檔案，只能出現一次）。
    #[test]
    fn families_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for f in families() {
            assert!(seen.insert(f.name.to_lowercase()), "重複的家族名：{}", f.name);
        }
    }

    /// 垃圾資料不會 panic（字型資料夾裡什麼都可能有）。
    #[test]
    fn junk_is_ignored() {
        assert!(families_in(b"not a font at all").is_empty());
        assert!(families_in(&[]).is_empty());
    }

    /// 自帶字型的家族名去重之後就是那三套，順序照 `BUILTIN`。
    #[test]
    fn builtin_families_are_deduped() {
        assert_eq!(builtin_families(), vec!["JetBrains Mono", "Cascadia Mono", "Sarasa Mono TC"]);
    }

    /// 自帶的字型檔真的在 repo 裡，而且 `name` 表寫的家族名和 `BUILTIN` 對得起來。
    /// （寫錯一個字 CSS 就選不到那個字型，而且畫面上只會「看起來沒換」。）
    #[test]
    fn builtin_files_exist_and_match_their_family_names() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts");
        for (file, family) in BUILTIN {
            let path = dir.join(file);
            let data = std::fs::read(&path).unwrap_or_else(|e| panic!("讀不到 {path:?}：{e}"));
            let got = families_in(&data);
            assert!(
                got.iter().any(|(n, _)| n == family),
                "{file} 的家族名是 {got:?}，BUILTIN 寫的是 {family}"
            );
            assert!(got.iter().all(|(_, mono)| *mono), "{file} 不是等寬？{got:?}");
        }
    }

    /// 符號／圖示字型不列進清單（Wingdings 這類拿來當終端機字型只會滿畫面小圖）。
    /// 這台機器沒有那些字型的話就跳過（CI／容器）。
    #[test]
    fn symbol_fonts_are_filtered_out() {
        let names: Vec<String> = families().into_iter().map(|f| f.name.to_lowercase()).collect();
        if names.len() <= 6 {
            return; // 退回那份通用清單＝這台沒有字型目錄，不判定
        }
        for bad in ["wingdings", "webdings", "marlett", "symbol", "segoe mdl2 assets"] {
            assert!(!names.iter().any(|n| n == bad), "符號字型 {bad} 不應該出現在清單裡");
        }
    }

    /// 這台機器上真的有裝字型的話，至少要掃得到幾十套——
    /// 「只看得到少數幾個」正是 TASK-031 要修的症頭。
    #[test]
    fn finds_real_fonts_when_present() {
        let list = families();
        if list.len() <= 6 {
            return;
        }
        assert!(list.len() >= 20, "只掃到 {} 個家族，看起來掃描沒有走到真正的字型目錄", list.len());
    }
}
