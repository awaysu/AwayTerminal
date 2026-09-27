//! 命令列參數。
//!
//! TASK-004 的驗證得暫時改 `tauri.conf.json` 的 `devUrl` 才能帶 `?verify=2`，很難用。
//! 這裡改成真正的 CLI 參數，URL 參數仍然可用（URL 優先，方便在 devtools 直接換）：
//!
//! ```text
//! AwayTerminal.exe --cmd claude          第一個分頁用這個指令開（等同 ?cmd=claude）
//! AwayTerminal.exe --verify 2            多開 2 個 shell 分頁並把驗證結果印到 stdout
//! AwayTerminal.exe --bench               啟動時跑一次 IPC bench（等同 ?bench=1）
//! ```
//!
//! `--cmd=claude` 這種等號寫法也吃。dev 下要傳給程式本身要多一個 `--`：
//! `npm run tauri dev -- -- --verify 2`。

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchArgs {
    /// `--cmd <指令>`：第一個分頁改用自訂指令開。
    pub cmd: Option<String>,
    /// `--verify <N>`：多開 N 個 shell 分頁，跑完把結果印到後端 stdout。
    pub verify: u32,
    /// `--bench`：啟動時跑一次 IPC bench。
    pub bench: bool,
    /// `--open-dir <路徑>`：檔案總管右鍵「用 AwayTerminal 開啟」送來的資料夾
    /// （**參數名照舊版**，不是 `--dir`）。也接受「單一個存在的資料夾路徑」＝把資料夾拖到 exe 上。
    pub open_dir: Option<String>,
}

impl LaunchArgs {
    pub fn from_env() -> Self {
        Self::parse(std::env::args().skip(1))
    }

    /// 解析一串參數（`from_env` 與單一執行個體的 callback 都用它）。
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut out = Self::default();
        let mut it = args.into_iter().peekable();
        while let Some(arg) = it.next() {
            // tauri/cargo 會塞一個裸的 `--` 分隔符，跳過
            if arg == "--" {
                continue;
            }
            let (name, inline) = match arg.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (arg, None),
            };
            let mut value = || inline.clone().or_else(|| it.next());
            match name.as_str() {
                "--cmd" => {
                    out.cmd = value().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
                }
                "--verify" => {
                    out.verify = value().and_then(|v| v.trim().parse().ok()).unwrap_or(1);
                }
                "--bench" => out.bench = true,
                "--open-dir" => {
                    out.open_dir = value().map(|v| v.trim().trim_matches('"').to_string()).filter(|v| !v.is_empty());
                }
                // 裸參數：是一個存在的資料夾就當成 --open-dir（把資料夾拖到 exe 上，同舊版）
                other if !other.starts_with('-') => {
                    let p = std::path::Path::new(other);
                    if out.open_dir.is_none() && p.is_dir() {
                        out.open_dir = Some(other.to_string());
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// 前端啟動時讀一次；URL 參數優先於這裡的值。
#[tauri::command]
pub fn launch_args(args: tauri::State<'_, LaunchArgs>) -> LaunchArgs {
    args.inner().clone()
}

#[cfg(test)]
mod tests {
    use super::LaunchArgs;

    fn parse(s: &[&str]) -> LaunchArgs {
        LaunchArgs::parse(s.iter().map(|x| x.to_string()))
    }

    #[test]
    fn parses_space_and_equals_forms() {
        assert_eq!(parse(&["--cmd", "claude"]).cmd.as_deref(), Some("claude"));
        assert_eq!(parse(&["--cmd=codex"]).cmd.as_deref(), Some("codex"));
        assert_eq!(parse(&["--verify", "3"]).verify, 3);
        assert_eq!(parse(&["--verify=2"]).verify, 2);
        assert!(parse(&["--bench"]).bench);
    }

    #[test]
    fn tolerates_separators_and_junk() {
        let a = parse(&["--", "--verify", "2", "--unknown", "x"]);
        assert_eq!(a.verify, 2);
        assert!(a.cmd.is_none());
        // `--verify` 沒帶數字＝預設 1（「多開一個」）
        assert_eq!(parse(&["--verify"]).verify, 1);
        // 空的 --cmd 當成沒給
        assert!(parse(&["--cmd", "  "]).cmd.is_none());
        assert_eq!(parse(&[]).verify, 0);
    }
}
