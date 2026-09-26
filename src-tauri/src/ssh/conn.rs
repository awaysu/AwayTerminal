//! SSH 的連線參數與「建一條 session」。
//!
//! 重連、退避、提示訊息那些**不在這裡**——那是兩種後端共用的，在 [`crate::reconnect`]。
//! 這個檔只有「SSH 專屬的部分」：參數欄位、主機金鑰／演算法的準備、帳號回報。
//!
//! 檔案是 TASK-009 的 `ssh/reconnect.rs` 拆出來的（TASK-010 加 Telnet 時泛化）。

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::host::emit_host;
use crate::session::{OnExit, OnOutput, TerminalSession};
use crate::settings::SettingsStore;
use crate::tabs::{self, TabManager};

/// 一條 SSH 連線的完整參數。
///
/// 這個結構同時是：分頁層重連要記住的東西、連線對話框的欄位、**我的最愛**存的內容、
/// 以及**恢復分頁**存的內容。
/// **刻意沒有密碼欄位**——舊版的 `SavedTab` 也沒有，密碼一律當場問。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SshConnParams {
    pub host: String,
    pub port: u16,
    /// 空＝連上後在終端機問 `login as:`（PuTTY 式）。
    pub user: String,
    /// 私鑰檔（OpenSSH 或 `.ppk`）。
    pub key_path: String,
    /// 要不要試 Pageant／ssh-agent。
    pub use_agent: bool,
    /// 保持連線的間隔（分鐘），0＝關。
    pub keepalive_mins: u32,
    /// 斷線自動重連（舊版連線視窗的勾選）。
    pub auto_reconnect: bool,
    pub algos: super::algos::AlgoOverride,
    /// 要送給遠端的環境變數（SSH `env` request）。
    /// 舊版是 `ssh.exe` 的 `-o SendEnv=…`；伺服器拒絕時只印一行灰字，不擋連線。
    pub env: Vec<(String, String)>,
}

/// 建一條 SSH session（輸出／結束管線由 [`crate::reconnect`] 給，重連才能沿用同一條）。
pub fn start(
    app: &AppHandle,
    id: u32,
    params: &SshConnParams,
    parts: &tabs::SessionParts,
    on_output: OnOutput,
    on_exit: OnExit,
    on_connected: Arc<dyn Fn() + Send + Sync>,
) -> Result<Arc<dyn TerminalSession>, String> {
    let settings = app
        .try_state::<Arc<SettingsStore>>()
        .ok_or_else(|| "設定還沒準備好".to_string())?;
    let tabs = app
        .try_state::<Arc<TabManager>>()
        .ok_or_else(|| "分頁清單還沒準備好".to_string())?;

    let on_user: super::OnUser = {
        let app = app.clone();
        let tabs = tabs.inner().clone();
        let host = params.host.clone();
        Arc::new(move |user: &str| {
            let target = format!("{user}@{host}");
            if tabs.set_title(id, &target, false) {
                emit_host(&app, format!("t{id}\x1f{target}"));
                tabs::emit_state(&app, &tabs);
            }
            // 記住帳號：重連就不必再問 login as:（同舊版把 Restore.Host 改成 user@host）
            tabs.set_ssh_user(id, user);
        })
    };

    let store = Arc::new(super::hostkey::HostKeyStore::new(
        settings.dir().join("known_hosts"),
    ));
    let decider = Arc::new(super::prompt::AppDecider::new(
        app.clone(),
        id,
        store.path().to_string_lossy().to_string(),
        (*settings).clone(),
    ));

    let session = super::spawn(
        super::SshOptions {
            host: params.host.clone(),
            port: params.port,
            user: (!params.user.trim().is_empty()).then(|| params.user.clone()),
            cols: parts.cols,
            rows: parts.rows,
            auth: super::SshAuth {
                key_path: (!params.key_path.trim().is_empty()).then(|| params.key_path.clone()),
                key_passphrase: None, // 有密碼的金鑰當場問（同 PuTTY）
                use_agent: params.use_agent,
            },
            algos: params.algos.clone(),
            keepalive_mins: params.keepalive_mins,
            env: params.env.clone(),
        },
        store,
        decider,
        on_output,
        on_exit,
        Some(on_user),
        Some(on_connected as super::OnConnected),
    );
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 參數裡不可以出現密碼類欄位（我的最愛／恢復分頁都存這個結構）。
    #[test]
    fn conn_params_have_no_password_field() {
        let json = serde_json::to_string(&SshConnParams::default()).unwrap();
        for bad in ["password", "passwd", "passphrase", "secret"] {
            assert!(!json.contains(bad), "連線參數不可以有 {bad} 欄位：{json}");
        }
    }
}
