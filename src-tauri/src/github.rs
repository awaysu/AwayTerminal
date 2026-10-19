//! 工具列「GitHub」（2.1.4 使用者要求；2.1.4 在我的最愛下拉，2.1.5 拉到工具列並改成下拉選單）。
//!
//! 下拉選單上面是最常用的 repo（最多 [`RECENT_SHOWN`] 個，[`github_recent`]），最後是「設定…」。
//! 設定視窗上面列出 GitHub 帳號的全部 repo，下面設定帳號、token（選填）、暫存目錄（預設桌面）
//! 與開啟方式（[`GithubConfig::agent`]）。
//! 點一個 repo：
//!
//! - 暫存目錄裡**沒有**這個 repo → 直接 `git clone`（最新版）。
//! - **已經有** → `git fetch` 後比對上游；落後就由前端問要不要更新（`git pull --ff-only`）。
//!
//! 做完前端在那個目錄開一個分頁：預設是「新分頁 ▾」裡第一個 AI agent（2.1.5），可改成指定的或 PowerShell。
//!
//! ## 為什麼這樣做
//!
//! - **列 repo 用 GitHub REST API**（`ureq`，同 `update.rs`）：沒填 token 只看得到公開的
//!   （`/users/{帳號}/repos`），有 token 才連私人的與組織的一起列（`/user/repos`）。
//! - **下載／更新用系統的 `git`**：不自己實作 git 協定；找不到 git 就明講。
//! - **token 不寫進 `.git/config`、也不放命令列**：用 `GIT_CONFIG_COUNT`／`GIT_CONFIG_KEY_0`
//!   環境變數臨時加 `http.https://github.com/.extraheader`（git 2.31+），只對這一次的 git 有效。
//!   沒有 token 時交給使用者自己的 git 認證（Git Credential Manager 之類）。
//! - `GIT_TERMINAL_PROMPT=0`：git 沒有終端機可問帳密，不讓它卡住等輸入。
//!
//! ⚠️ token 是機密：不進 log（`println!` 只印 repo 名與路徑）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager, State};

use crate::i18n::{t, tf};
use crate::settings::{GithubRecent, SettingsStore};

const API: &str = "https://api.github.com";
const TIMEOUT: Duration = Duration::from_secs(20);
/// 一頁最多 100 筆（GitHub 的上限）；再多就翻頁，最多翻到這裡為止。
const MAX_PAGES: usize = 20;
/// 下拉選單最多列幾個常用 repo（使用者要求「不超過 10 個」）。
pub const RECENT_SHOWN: usize = 10;
/// 設定裡最多記幾個（超過就丟掉最少用的；比顯示的多一些，排名才不會一掉出去就歸零）。
const RECENT_KEPT: usize = 50;

/// 視窗下面那三個設定。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubConfig {
    pub user: String,
    pub token: String,
    /// 暫存目錄。設定裡空的＝用 [`default_dir`]（[`config_of`] 會填好，這裡永遠是實際路徑）。
    pub dir: String,
    /// 開啟方式：空＝自動（新分頁第一個 AI agent）、`@shell`＝PowerShell／終端機、其他＝自訂連線名稱。
    #[serde(default)]
    pub agent: String,
}

/// 清單上的一個 repo。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub name: String,
    pub full_name: String,
    pub clone_url: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub fork: bool,
}

/// 點了某個 repo 之後的本機狀態。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    /// 這個 repo 在暫存目錄裡的完整路徑。
    pub path: String,
    /// 已經下載過了嗎（有 `.git`）。
    pub exists: bool,
    /// 比上游落後幾個 commit（沒有上游＝0）。
    pub behind: u32,
    /// 比上游多幾個 commit（本機有自己的 commit）。
    pub ahead: u32,
}

/// 沒設定暫存目錄時用的位置：**桌面**（使用者要求）。系統找不到桌面才退回暫存資料夾。
pub fn default_dir(app: &AppHandle) -> PathBuf {
    app.path().desktop_dir().unwrap_or_else(|_| std::env::temp_dir())
}

fn base_dir(cfg: &GithubConfig) -> PathBuf {
    PathBuf::from(cfg.dir.trim())
}

/// repo 名稱只能當成一層資料夾名（擋掉 `..`、斜線，避免寫到暫存目錄外面）。
fn repo_path(cfg: &GithubConfig, name: &str) -> Result<PathBuf, String> {
    let ok = !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !ok {
        return Err(tf("err.githubBadName", &[name]));
    }
    Ok(base_dir(cfg).join(name))
}

/// 目前的設定；暫存目錄沒設就填成 [`default_dir`]。
fn config_of(settings: &SettingsStore, app: &AppHandle) -> GithubConfig {
    let s = settings.get();
    let dir = if s.github_dir.trim().is_empty() {
        default_dir(app).to_string_lossy().to_string()
    } else {
        s.github_dir
    };
    GithubConfig { user: s.github_user, token: s.github_token, dir, agent: s.github_agent }
}

// ------------------------------------------------------------------ 設定

/// 讀設定（`dir` 沒設時是預設路徑，讓輸入框直接顯示實際會用的位置）。
#[tauri::command]
pub fn github_get_config(app: AppHandle, settings: State<'_, Arc<SettingsStore>>) -> GithubConfig {
    config_of(&settings, &app)
}

#[tauri::command]
pub fn github_set_config(settings: State<'_, Arc<SettingsStore>>, config: GithubConfig) {
    settings.update(|s| {
        s.github_user = config.user.trim().to_string();
        s.github_token = config.token.trim().to_string();
        s.github_dir = config.dir.trim().to_string();
        s.github_agent = config.agent.trim().to_string();
    });
}

// ------------------------------------------------------------------ 常用 repo

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 次數多的排前面；一樣多時最近用的排前面。
fn sort_recent(list: &mut [GithubRecent]) {
    list.sort_by(|a, b| b.count.cmp(&a.count).then(b.last_used.cmp(&a.last_used)));
}

/// 記一次「點了這個 repo」（`repo` 是清單或下拉選單上的那一筆）。
fn record_use(list: &mut Vec<GithubRecent>, repo: &RepoInfo, now: u64) {
    if let Some(r) = list.iter_mut().find(|r| r.full_name.eq_ignore_ascii_case(&repo.full_name)) {
        r.count = r.count.saturating_add(1);
        r.last_used = now;
        // 改名或轉成私人：以最新的為準
        r.name = repo.name.clone();
        r.clone_url = repo.clone_url.clone();
        r.private = repo.private;
    } else {
        list.push(GithubRecent {
            name: repo.name.clone(),
            full_name: repo.full_name.clone(),
            clone_url: repo.clone_url.clone(),
            private: repo.private,
            count: 1,
            last_used: now,
        });
    }
    sort_recent(list);
    list.truncate(RECENT_KEPT);
}

/// 下拉選單用：最常用的前 [`RECENT_SHOWN`] 個。
#[tauri::command]
pub fn github_recent(settings: State<'_, Arc<SettingsStore>>) -> Vec<GithubRecent> {
    let mut list = settings.get().github_recent;
    sort_recent(&mut list);
    list.truncate(RECENT_SHOWN);
    list
}

#[tauri::command]
pub fn github_record_use(settings: State<'_, Arc<SettingsStore>>, repo: RepoInfo) {
    let now = now_secs();
    settings.update(|s| record_use(&mut s.github_recent, &repo, now));
}

// ------------------------------------------------------------------ 列 repo

#[derive(serde::Deserialize)]
struct ApiRepo {
    name: String,
    full_name: String,
    clone_url: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    fork: bool,
}

/// 列出帳號的全部 repo（照最近更新排序）。
#[tauri::command]
pub async fn github_list_repos(
    app: AppHandle,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<Vec<RepoInfo>, String> {
    let cfg = config_of(&settings, &app);
    tokio::task::spawn_blocking(move || list_repos(&cfg))
        .await
        .map_err(|e| e.to_string())?
}

fn list_repos(cfg: &GithubConfig) -> Result<Vec<RepoInfo>, String> {
    let user = cfg.user.trim();
    let token = cfg.token.trim();
    if user.is_empty() && token.is_empty() {
        return Err(t("err.githubNoUser"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(format!("AwayTerminal/{}", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .build()
        .into();
    let mut out = Vec::new();
    for page in 1..=MAX_PAGES {
        // 有 token：自己的（含私人）＋協作＋組織的；沒有：只有這個帳號公開的
        let url = if token.is_empty() {
            format!("{API}/users/{user}/repos?type=owner&sort=updated&per_page=100&page={page}")
        } else {
            format!("{API}/user/repos?sort=updated&per_page=100&page={page}")
        };
        let mut req = agent
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if !token.is_empty() {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        let mut resp = req
            .call()
            .map_err(|e| tf("err.githubNet", &[&e.to_string()]))?;
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| tf("err.githubNet", &[&e.to_string()]))?;
        match status {
            200 => {}
            401 => return Err(t("err.githubToken")),
            404 => return Err(tf("err.githubNoSuchUser", &[user])),
            _ => return Err(tf("err.githubHttp", &[&status.to_string(), &api_message(&body)])),
        }
        let repos: Vec<ApiRepo> =
            serde_json::from_str(&body).map_err(|e| tf("err.githubNet", &[&e.to_string()]))?;
        let n = repos.len();
        out.extend(repos.into_iter().map(|r| RepoInfo {
            name: r.name,
            full_name: r.full_name,
            clone_url: r.clone_url,
            description: r.description.unwrap_or_default(),
            private: r.private,
            fork: r.fork,
        }));
        if n < 100 {
            break;
        }
    }
    println!("[AwayTerminal] GitHub：列出 {} 個 repo", out.len());
    Ok(out)
}

/// GitHub 錯誤回應裡的 `message`（例如 rate limit）；不是 JSON 就回空字串。
fn api_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_string))
        .unwrap_or_default()
}

// ------------------------------------------------------------------ git

/// 組一個 `git` 指令：不閃黑窗、不等終端機輸入；有 token 就臨時帶上認證標頭。
fn git(cfg: &GithubConfig, cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new("git");
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    let token = cfg.token.trim();
    if !token.is_empty() {
        let basic = crate::b64::encode(format!("x-access-token:{token}").as_bytes());
        cmd.env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.https://github.com/.extraheader")
            .env("GIT_CONFIG_VALUE_0", format!("AUTHORIZATION: basic {basic}"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW); // 不要閃一個黑窗
    }
    cmd
}

/// 跑一個 git 指令，成功回 stdout；失敗回 stderr（找不到 git 另外講）。`what` 是錯誤訊息裡的子指令名。
fn run(mut cmd: Command, what: &str) -> Result<String, String> {
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            t("err.gitNotFound")
        } else {
            tf("err.gitFailed", &[what, &e.to_string()])
        }
    })?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(tf("err.gitFailed", &[what, &err]))
    }
}

/// 看這個 repo 在暫存目錄裡的狀態；已經下載過就先 `git fetch` 再和上游比。
#[tauri::command]
pub async fn github_local_state(
    app: AppHandle,
    settings: State<'_, Arc<SettingsStore>>,
    name: String,
) -> Result<LocalState, String> {
    let cfg = config_of(&settings, &app);
    tokio::task::spawn_blocking(move || local_state(&cfg, &name))
        .await
        .map_err(|e| e.to_string())?
}

fn local_state(cfg: &GithubConfig, name: &str) -> Result<LocalState, String> {
    let path = repo_path(cfg, name)?;
    let shown = path.to_string_lossy().to_string();
    if !path.exists() {
        return Ok(LocalState { path: shown, exists: false, behind: 0, ahead: 0 });
    }
    if !path.join(".git").exists() {
        // 資料夾在、但不是 git repo：不去動它（可能是使用者自己的東西）
        return Err(tf("err.githubNotRepo", &[&shown]));
    }
    let mut fetch = git(cfg, Some(&path));
    fetch.args(["fetch", "--quiet"]);
    run(fetch, "fetch")?;
    // 沒有上游（detached HEAD、本機分支）＝沒得比，當成最新
    let mut up = git(cfg, Some(&path));
    up.args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]);
    if run(up, "rev-parse").is_err() {
        return Ok(LocalState { path: shown, exists: true, behind: 0, ahead: 0 });
    }
    let mut count = git(cfg, Some(&path));
    count.args(["rev-list", "--left-right", "--count", "HEAD...@{u}"]);
    let text = run(count, "rev-list")?;
    let mut it = text.split_whitespace().map(|s| s.parse::<u32>().unwrap_or(0));
    let ahead = it.next().unwrap_or(0);
    let behind = it.next().unwrap_or(0);
    println!("[AwayTerminal] GitHub：{name} 落後 {behind}、超前 {ahead}");
    Ok(LocalState { path: shown, exists: true, behind, ahead })
}

/// 下載（`git clone`）到暫存目錄，回完整路徑。
#[tauri::command]
pub async fn github_clone(
    app: AppHandle,
    settings: State<'_, Arc<SettingsStore>>,
    name: String,
    clone_url: String,
) -> Result<String, String> {
    let cfg = config_of(&settings, &app);
    tokio::task::spawn_blocking(move || {
        if !clone_url.starts_with("https://github.com/") {
            return Err(tf("err.githubBadName", &[&clone_url]));
        }
        let path = repo_path(&cfg, &name)?;
        let base = base_dir(&cfg);
        std::fs::create_dir_all(&base).map_err(|e| tf("err.gitFailed", &["clone", &e.to_string()]))?;
        let mut cmd = git(&cfg, Some(&base));
        cmd.arg("clone").arg(&clone_url).arg(&path);
        run(cmd, "clone")?;
        println!("[AwayTerminal] GitHub：clone {name} → {}", path.display());
        Ok(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 更新到最新（`git pull --ff-only`：本機有改動或分岔就失敗、不合併，錯誤原文給使用者看）。
#[tauri::command]
pub async fn github_pull(
    app: AppHandle,
    settings: State<'_, Arc<SettingsStore>>,
    name: String,
) -> Result<(), String> {
    let cfg = config_of(&settings, &app);
    tokio::task::spawn_blocking(move || {
        let path = repo_path(&cfg, &name)?;
        let mut cmd = git(&cfg, Some(&path));
        cmd.args(["pull", "--ff-only"]);
        run(cmd, "pull")?;
        println!("[AwayTerminal] GitHub：pull {name}");
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_name_cannot_escape_base_dir() {
        let cfg = GithubConfig { dir: "base".into(), ..Default::default() };
        assert!(repo_path(&cfg, "AwayTerminal").is_ok());
        assert!(repo_path(&cfg, "my.repo-1_x").is_ok());
        for bad in ["", ".", "..", "../x", "a/b", "a\\b", "C:"] {
            assert!(repo_path(&cfg, bad).is_err(), "{bad} 應該被擋");
        }
    }

    /// 實際連 GitHub：列 repo → clone → 狀態（最新）→ 退一個 commit → 狀態（落後）→ pull → 最新。
    /// 要網路與 git：`cargo test --lib github -- --ignored`
    #[test]
    #[ignore]
    fn live_list_clone_and_update() {
        let dir = std::env::temp_dir().join(format!("awt-gh-test-{}", std::process::id()));
        let cfg = GithubConfig { user: "awaysu".into(), dir: dir.to_string_lossy().into(), ..Default::default() };
        let repos = list_repos(&cfg).expect("list");
        assert!(repos.iter().any(|r| r.name == "AwayTerminal"), "列不到 AwayTerminal");
        let repo = repos.iter().min_by_key(|r| r.name.len()).unwrap();
        let st = local_state(&cfg, &repo.name).unwrap();
        assert!(!st.exists);
        let path = repo_path(&cfg, &repo.name).unwrap();
        let mut c = git(&cfg, None);
        c.arg("clone").arg(&repo.clone_url).arg(&path);
        run(c, "clone").unwrap();
        let st = local_state(&cfg, &repo.name).unwrap();
        assert!(st.exists && st.behind == 0, "{st:?}");
        let mut reset = git(&cfg, Some(&path));
        reset.args(["reset", "--hard", "HEAD~1"]);
        if run(reset, "reset").is_ok() {
            let st = local_state(&cfg, &repo.name).unwrap();
            assert_eq!(st.behind, 1, "{st:?}");
            let mut pull = git(&cfg, Some(&path));
            pull.args(["pull", "--ff-only"]);
            run(pull, "pull").unwrap();
            assert_eq!(local_state(&cfg, &repo.name).unwrap().behind, 0);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_counts_and_orders_by_use() {
        let repo = |n: &str| RepoInfo {
            name: n.into(),
            full_name: format!("awaysu/{n}"),
            clone_url: format!("https://github.com/awaysu/{n}.git"),
            description: String::new(),
            private: false,
            fork: false,
        };
        let mut list = Vec::new();
        record_use(&mut list, &repo("a"), 1);
        record_use(&mut list, &repo("b"), 2);
        record_use(&mut list, &repo("b"), 3);
        record_use(&mut list, &repo("c"), 4);
        let names: Vec<&str> = list.iter().map(|r| r.name.as_str()).collect();
        // b 用了兩次排第一；a、c 都一次 → 最近用的 c 在前
        assert_eq!(names, ["b", "c", "a"]);
        assert_eq!(list[0].count, 2);
        for i in 0..(RECENT_KEPT + 5) {
            record_use(&mut list, &repo(&format!("r{i}")), 10 + i as u64);
        }
        assert_eq!(list.len(), RECENT_KEPT);
        assert_eq!(list[0].name, "b", "最常用的不會因為記滿被擠掉");
    }

    #[test]
    fn api_message_reads_json_or_empty() {
        assert_eq!(api_message(r#"{"message":"API rate limit exceeded"}"#), "API rate limit exceeded");
        assert_eq!(api_message("<html>"), "");
    }
}
