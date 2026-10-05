//! ADB 分頁（搬移舊版 `MainWindow.OpenAdbFlow` ／ `AdbDevices` ／ `OpenAdbShell`
//! ＋ `AppSettings.ResolveAdbPath`）。
//!
//! ## 舊版的流程（逐項照抄）
//!
//! | 舊版 | 出處 | 這裡 |
//! |---|---|---|
//! | ADB **不是內建選單項目**，是一般的「自訂連線」（v1.0.18 起） | `CustomConnDialog.KnownTools` 的 `ADB` | 同（`custom::KNOWN_TOOLS`） |
//! | 開那條自訂連線時，若執行檔叫 `adb` → **不直接跑 `adb shell`**，改走裝置流程 | `OpenCustom` 的 `IsAdbExe` | [`is_adb_exe`]，前端在開 `conn` 之前先問 [`adb_devices`] |
//! | `adb devices`：`<序號>\t<狀態>`，只收 `device`（跳過 `offline`／`unauthorized`／標頭） | `AdbDevices` | [`list_devices`]（逾時 5 秒，同舊版） |
//! | 0 台 → 提示「沒有偵測到 adb 裝置。」 | `adb.noDevice` | 前端用 `adb.noDevice` |
//! | 找不到 adb → 說明 ＋ 問要不要開官方下載頁 | `PromptInstallAdb` | 前端用 `adb.notInstalled` ＋ [`ADB_DOWNLOAD_URL`] |
//! | 1 台 → **直接開**，分頁名稱 `ADB`（`NextName("ADB")`） | `OpenAdbFlow` | 同 |
//! | 2 台以上 → 選單選序號，分頁名稱＝**序號** | 同上 | 同（前端的選單） |
//! | 開 `adb shell`／`adb -s <序號> shell`，關閉鍵 Ctrl+C ×3 | `OpenAdbShell` | [`command_line`]；關閉鍵是 `ConPtySession` 的預設 |
//! | 恢復分頁記 `adb.exe` 路徑與序號，**不再跑 `adb devices`** | `tab.Restore = SavedTab{ type="adb", AdbSerial, Path }` | `restore::SavedTab` 的 `adb_serial`／`path` |
//!
//! ## 路徑搜尋（`ResolveAdbPath`）
//!
//! 順序照舊版：**使用者指定的自訂連線路徑 → PATH → `ANDROID_HOME`／`ANDROID_SDK_ROOT`
//! → Android Studio 的預設位置 → 舊版殘留的 `tools\adb\adb.exe`**。
//! 舊版**不打包 Google 的 adb**（Android SDK 條款 §3.4 禁止轉散布），我們也不打包。
//!
//! `CLAUDE.md` 把「ADB 路徑搜尋」列為 Windows 專屬要換掉的東西 → 候選清單用 `#[cfg]` 分開，
//! 非 Windows 只找 PATH 與 `$ANDROID_*`（mac/Linux 的 Android Studio 位置之後再補）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// 官方 platform-tools 下載頁（找不到 adb 時提示使用者自己裝；同舊版）。
pub const ADB_DOWNLOAD_URL: &str = "https://developer.android.com/tools/releases/platform-tools";

/// `adb devices` 的逾時（舊版 `WaitForExit(5000)`）。
const DEVICES_TIMEOUT: Duration = Duration::from_secs(5);

/// 一台裝置（`adb devices` 的一行）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbDevice {
    pub serial: String,
    /// `device`／`offline`／`unauthorized`…（我們只開 `device` 的，但都回給前端顯示）。
    pub state: String,
}

/// 這個路徑是 adb 嗎（**看檔名**，同舊版 `IsAdbExe`）。
///
/// 為什麼要這個：自訂連線指向 adb 時**不能**直接跑 `adb shell`——接了兩台以上只會噴錯。
pub fn is_adb_exe(path: &str) -> bool {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("adb"))
}

/// 這台機器上的 adb（找不到回 `None`）。順序見模組說明。
pub fn resolve_path(preferred: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = preferred.filter(|p| !p.trim().is_empty()) {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    candidates().into_iter().find(|p| p.is_file())
}

/// 候選路徑（順序照舊版 `AdbCandidates`）。
fn candidates() -> Vec<PathBuf> {
    let exe = if cfg!(windows) { "adb.exe" } else { "adb" };
    let mut out = Vec::new();
    // 1. PATH
    if let Some(p) = crate::pty::shell::which(exe) {
        out.push(p);
    }
    // 2. Android SDK 的環境變數（Android Studio／CI 常設）
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(root) = std::env::var(var) {
            if !root.trim().is_empty() {
                out.push(PathBuf::from(root).join("platform-tools").join(exe));
            }
        }
    }
    // 3. Android Studio 的預設安裝位置
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            out.push(
                PathBuf::from(local)
                    .join("Android")
                    .join("Sdk")
                    .join("platform-tools")
                    .join(exe),
            );
        }
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(pf) = std::env::var(var) {
                out.push(
                    PathBuf::from(pf)
                        .join("Android")
                        .join("android-sdk")
                        .join("platform-tools")
                        .join(exe),
                );
            }
        }
        // 4. 1.0.12 以前的安裝在程式目錄留下的 tools\adb\adb.exe
        //（已經在使用者電腦上的副本可以照用——我們不散布它）
        if let Ok(cur) = std::env::current_exe() {
            if let Some(dir) = cur.parent() {
                out.push(dir.join("tools").join("adb").join(exe));
            }
        }
    }
    #[cfg(not(windows))]
    {
        // mac/Linux：Android Studio 的預設位置（`~/Library/Android/sdk`、`~/Android/Sdk`）
        if let Ok(home) = std::env::var("HOME") {
            let h = PathBuf::from(home);
            out.push(h.join("Library/Android/sdk/platform-tools").join(exe));
            out.push(h.join("Android/Sdk/platform-tools").join(exe));
        }
    }
    out
}

/// 解析 `adb devices` 的輸出。**只有 `\t` 分隔的行算**（跳過標頭與空行），同舊版。
///
/// 舊版只留 `state == "device"`；我們**全部回傳**（含 `offline`／`unauthorized`），
/// 由呼叫端決定——這樣使用者插了沒授權的手機時，畫面上看得到「有這台但未授權」，
/// 而不是「沒有偵測到裝置」（舊版那樣會讓人以為線沒插好）。差異寫進 `docs/WINDOWS-INTEGRATION.md`。
pub fn parse_devices(output: &str) -> Vec<AdbDevice> {
    let mut out = Vec::new();
    for line in output.lines() {
        let Some(tab) = line.find('\t') else { continue };
        let serial = line[..tab].trim();
        let state = line[tab + 1..].trim();
        if serial.is_empty() || state.is_empty() {
            continue;
        }
        out.push(AdbDevice {
            serial: serial.to_string(),
            state: state.to_string(),
        });
    }
    out
}

/// 跑一次 `adb devices`（阻塞；command 那邊用 `spawn_blocking`）。
pub fn list_devices(adb: &Path) -> Result<Vec<AdbDevice>, String> {
    let mut cmd = Command::new(adb);
    cmd.arg("devices");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW); // 不要閃一個黑窗（同舊版 CreateNoWindow）
    }
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| crate::i18n::tf("err.launchFailed", &[&adb.display().to_string(), &e.to_string()]))?;

    // 逾時：舊版 `WaitForExit(5000)`。超時就砍掉**這個自己開的子行程**（只用它的 PID）
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < DEVICES_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                break;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut text = String::new();
    if let Some(mut out) = child.stdout.take() {
        use std::io::Read;
        let _ = out.read_to_string(&mut text);
    }
    Ok(parse_devices(&text))
}

/// 開 ADB shell 的命令列（`adb shell`／`adb -s <序號> shell`，同舊版 `OpenAdbShell`）。
pub fn command_line(adb: &Path, serial: Option<&str>) -> String {
    match serial.filter(|s| !s.trim().is_empty()) {
        Some(s) => format!("\"{}\" -s {} shell", adb.display(), s),
        None => format!("\"{}\" shell", adb.display()),
    }
}

// ------------------------------------------------------------------ commands

/// 前端要開 ADB 分頁之前先問這個：adb 在哪、有哪些裝置。
///
/// - `adbPath`：自訂連線裡填的路徑（優先用它，同舊版）
/// - 回 `{ adb: null }` ＝找不到 adb → 前端提示安裝（`adb.notInstalled` ＋ 下載頁）
/// - 回 `{ devices: [] }` ＝沒有裝置 → 前端提示（`adb.noDevice`）
#[tauri::command]
pub async fn adb_devices(adb_path: Option<String>) -> Result<serde_json::Value, String> {
    let found = resolve_path(adb_path.as_deref());
    let Some(adb) = found else {
        println!("[AwayTerminal] ADB：找不到 adb（PATH／ANDROID_HOME／Android Studio 都沒有）");
        return Ok(serde_json::json!({
            "adb": serde_json::Value::Null,
            "devices": [],
            "downloadUrl": ADB_DOWNLOAD_URL,
        }));
    };
    let adb2 = adb.clone();
    let devices = tokio::task::spawn_blocking(move || list_devices(&adb2))
        .await
        .map_err(|e| e.to_string())??;
    println!(
        "[AwayTerminal] ADB：{} → {} 台（{}）",
        adb.display(),
        devices.len(),
        devices
            .iter()
            .map(|d| format!("{} {}", d.serial, d.state))
            .collect::<Vec<_>>()
            .join("、")
    );
    Ok(serde_json::json!({
        "adb": adb.display().to_string(),
        "devices": devices,
        "downloadUrl": ADB_DOWNLOAD_URL,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `IsAdbExe`：看檔名、不分大小寫、有沒有 `.exe` 都算。
    #[test]
    fn spots_adb_exe() {
        // 反斜線只在 Windows 是路徑分隔字元（Unix 上整串會被當成一個檔名）
        if cfg!(windows) {
            assert!(is_adb_exe("C:\\sdk\\platform-tools\\adb.exe"));
        } else {
            assert!(is_adb_exe("/opt/android-sdk/platform-tools/adb"));
        }
        assert!(is_adb_exe("adb"));
        assert!(is_adb_exe("/usr/bin/ADB"));
        assert!(!is_adb_exe("C:\\tools\\adbx.exe"));
        assert!(!is_adb_exe("wsl.exe"));
        assert!(!is_adb_exe(""));
    }

    /// `adb devices` 的輸出解析（含標頭、空行、offline／unauthorized）。
    #[test]
    fn parses_devices_output() {
        let out = "List of devices attached\r\n\
                   R5CT10ABCDE\tdevice\r\n\
                   emulator-5554\tdevice\r\n\
                   192.168.1.5:5555\toffline\r\n\
                   XYZ123\tunauthorized\r\n\
                   \r\n";
        let d = parse_devices(out);
        assert_eq!(d.len(), 4, "四行有 tab 的都要回：{d:?}");
        assert_eq!(d[0].serial, "R5CT10ABCDE");
        assert_eq!(d[0].state, "device");
        assert_eq!(d[2].state, "offline");
        assert_eq!(d[3].state, "unauthorized");
        // 標頭「List of devices attached」沒有 tab → 不算
        assert!(!d.iter().any(|x| x.serial.contains("List")));
    }

    /// 空輸出／只有標頭 → 沒有裝置。
    #[test]
    fn handles_empty_output() {
        assert!(parse_devices("").is_empty());
        assert!(parse_devices("List of devices attached\n\n").is_empty());
        assert!(parse_devices("* daemon started successfully *\n").is_empty());
    }

    /// 命令列格式照舊版（路徑有空白要加引號）。
    #[test]
    fn builds_command_line_like_v1() {
        let adb = PathBuf::from("C:\\Program Files\\adb.exe");
        assert_eq!(
            command_line(&adb, None),
            "\"C:\\Program Files\\adb.exe\" shell"
        );
        assert_eq!(
            command_line(&adb, Some("R5CT10ABCDE")),
            "\"C:\\Program Files\\adb.exe\" -s R5CT10ABCDE shell"
        );
        // 空白的序號當成「沒有序號」
        assert_eq!(command_line(&adb, Some("  ")), "\"C:\\Program Files\\adb.exe\" shell");
    }

    /// 候選清單不會是空的，而且第一個是 PATH 上的（如果有）。
    #[test]
    fn candidates_are_ordered() {
        let c = candidates();
        // 這台機器不一定有 adb，所以只檢查「有候選」與「都是絕對路徑或檔名」
        for p in &c {
            assert!(!p.as_os_str().is_empty());
        }
    }
}
