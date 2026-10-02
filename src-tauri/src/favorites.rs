//! 我的最愛（搬移舊版 `MainWindow.Favorites.cs` + `Dialogs/FavoritesDialog`）。
//!
//! 舊版的結構是 `FavoriteItem { Name, Tab: SavedTab, TeamSetup }`——一筆最愛＝
//! **分頁的恢復資訊複本**。我們照同一個想法，但 `SavedTab` 那個大雜燴拆成
//! 「種類 + 該種類需要的欄位」，因為新版的 SSH 參數已經有自己的結構（`SshConnParams`）。
//!
//! | 舊版行為 | 這裡 |
//! |---|---|
//! | 下拉＝各筆最愛（點了直接開）→ 分隔線 →「加到我的最愛：目前分頁」→「設定…」 | 同（在 `src/favs.js`） |
//! | PowerShell 記**目前所在**目錄（提示行解析到的 `CwdPath`），不是開分頁時的 | [`from_tab`] |
//! | 自訂連線有實際工作目錄 → 重開直接用，不再跳資料夾選擇 | [`from_tab`]（`pick_dir` 不再套用） |
//! | SSH 登入後記 `user@host`，重開直接連 | 同（`SshConnParams.user` 在登入後被填上） |
//! | 同一個連線（種類＋主機／路徑＋目錄）只收一筆 | [`key_of`] |
//! | 名稱重複 → 後面補 ` (2)`、` (3)` | [`add`] |
//! | 代理團隊記整組設定（`MultiAgentSetup`），重開不跳設定視窗 | 同（`team`，2.0.2；含每格的模型） |
//! | —（舊版沒有） | 自訂連線記啟動時選的**模型**（`model`，2.0.2）；模型不在清單裡時前端會再問一次 |
//! | Telnet／COM | ⬜ 欄位留在模型裡，UI 不顯示 |
//!
//! ## 密碼
//! **不存。** 舊版的 `SavedTab` 也沒有密碼欄位（我去逐欄看過），SSH 密碼一律在終端機當場問。
//! 這一點連 `SshConnParams`／`TelnetParams` 都有單元測試釘住
//! （`reconnect.rs` 的 `conn_params_have_no_password_field`）。

use crate::i18n::{t, tf};
use std::sync::Arc;

use tauri::State;

use crate::settings::SettingsStore;
use crate::ssh::conn::SshConnParams;
use crate::com::ComParams;
use crate::telnet::TelnetParams;

/// 一筆最愛。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FavoriteItem {
    /// 下拉裡顯示的名稱（可改）。
    pub name: String,
    /// `shell`｜`ssh`｜`telnet`｜`com`｜`conn`（自訂連線）｜`team`（代理團隊）｜`chat`（AI 聊天室）。
    pub kind: String,
    /// `shell`：工作目錄。`conn`：實際工作目錄（有值就不再跳資料夾選擇）。
    /// `team`／`chat`：專案資料夾。
    pub dir: String,
    /// `conn`：自訂連線的名稱。
    pub conn_name: String,
    /// `conn`：啟動時選的模型（空＝預設）。
    pub model: String,
    /// `team`／`chat`：整組設定（每格的 CLI／角色／模型、投遞上限、沙盒…）。
    pub team: Option<crate::agent::TeamSetup>,
    /// `ssh`：完整連線參數（**不含密碼**）。
    pub ssh: Option<SshConnParams>,
    /// `telnet`：完整連線參數（Telnet 本來就沒有帳號密碼欄）。
    pub telnet: Option<TelnetParams>,
    /// `com`：埠與鮑率等（序列埠沒有帳號密碼）。
    pub com: Option<ComParams>,
}

impl Default for FavoriteItem {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: "shell".to_string(),
            dir: String::new(),
            conn_name: String::new(),
            model: String::new(),
            team: None,
            ssh: None,
            telnet: None,
            com: None,
        }
    }
}

/// 同一個連線只收一筆的判斷鍵（舊版 `FavoriteKey`：種類＋主機／路徑＋目錄）。
pub fn key_of(f: &FavoriteItem) -> String {
    match f.kind.as_str() {
        "ssh" => {
            let s = f.ssh.clone().unwrap_or_default();
            format!("ssh|{}|{}|{}", s.host.to_lowercase(), s.port, s.user)
        }
        "telnet" => {
            let t = f.telnet.clone().unwrap_or_default();
            format!("telnet|{}|{}", t.host.to_lowercase(), t.port)
        }
        "com" => {
            let c = f.com.clone().unwrap_or_default();
            format!("com|{}|{}", c.port.to_lowercase(), c.baud)
        }
        // 同一條連線、同一個目錄但模型不同＝不同的最愛（一個開 opus、一個開 sonnet）
        "conn" => format!(
            "conn|{}|{}|{}",
            f.conn_name.to_lowercase(),
            f.dir.to_lowercase(),
            f.model
        ),
        // 代理團隊／聊天室：同一個資料夾只收一筆（`team|目錄`、`chat|目錄`）
        _ => format!("{}|{}", f.kind, f.dir.to_lowercase()),
    }
}

/// 目前這個分頁能不能存成最愛？能的話回一筆候選（舊版 `FavoriteFromTab`）。
///
/// `teams` 給了而且這個分頁是代理團隊／聊天室的一格 → 存的是**整組**。
pub fn from_tab(
    tabs: &crate::tabs::TabManager,
    teams: Option<&crate::agent::TeamManager>,
    id: u32,
) -> Option<FavoriteItem> {
    if let Some((title, setup)) = teams.and_then(|t| crate::agent::favorite_setup(t, id)) {
        return Some(FavoriteItem {
            name: title,
            kind: if setup.kind == crate::agent::team::GroupKind::Chat { "chat" } else { "team" }
                .to_string(),
            dir: setup.dir.clone(),
            team: Some(setup),
            ..FavoriteItem::default()
        });
    }
    let view = tabs.state_with(&[]).tabs.into_iter().find(|t| t.id == id)?;
    match view.kind {
        crate::tabs::TabKind::Ssh => {
            let ssh = tabs.ssh_params_of(id)?;
            Some(FavoriteItem {
                name: view.title.clone(),
                kind: "ssh".to_string(),
                ssh: Some(ssh),
                ..FavoriteItem::default()
            })
        }
        crate::tabs::TabKind::Telnet => {
            let telnet = tabs.telnet_params_of(id)?;
            Some(FavoriteItem {
                name: view.title.clone(),
                kind: "telnet".to_string(),
                telnet: Some(telnet),
                ..FavoriteItem::default()
            })
        }
        crate::tabs::TabKind::Com => {
            let com = tabs.com_params_of(id)?;
            Some(FavoriteItem {
                name: view.title.clone(),
                kind: "com".to_string(),
                com: Some(com),
                ..FavoriteItem::default()
            })
        }
        crate::tabs::TabKind::PowerShell => Some(FavoriteItem {
            name: view.title.clone(),
            kind: "shell".to_string(),
            // 舊版：記「現在所在」的目錄（使用者 cd 過就是 cd 之後的）
            dir: view.cwd_path.clone(),
            ..FavoriteItem::default()
        }),
        crate::tabs::TabKind::Claude | crate::tabs::TabKind::Custom => {
            let conn = view.conn_name.clone()?;
            Some(FavoriteItem {
                name: view.title.clone(),
                kind: "conn".to_string(),
                dir: view.cwd_path.clone(),
                conn_name: conn,
                model: view.model.clone(),
                ..FavoriteItem::default()
            })
        }
        // Telnet / COM / ADB：欄位留在模型裡，但還沒有後端
        _ => None,
    }
}

// ------------------------------------------------------------------ commands

#[tauri::command]
pub fn fav_list(settings: State<'_, Arc<SettingsStore>>) -> Vec<FavoriteItem> {
    settings.get().favorites
}

/// 目前分頁的候選最愛（`None`＝這個分頁沒有可重開的資訊，按鈕要灰掉）。
#[tauri::command]
pub fn fav_candidate(
    id: u32,
    tabs_state: State<'_, Arc<crate::tabs::TabManager>>,
    teams: State<'_, Arc<crate::agent::TeamManager>>,
) -> Option<FavoriteItem> {
    from_tab(&tabs_state, Some(&teams), id)
}

/// 加一筆。回傳實際存下來的名稱（可能被補了 ` (2)`），或 `Err` 說明為什麼沒加。
#[tauri::command]
pub fn fav_add(
    mut item: FavoriteItem,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<String, String> {
    let key = key_of(&item);
    let existing = settings.get().favorites;
    if let Some(dup) = existing.iter().find(|f| key_of(f) == key) {
        // 舊版：同一個連線只收一筆，已經有了就講出來（不是錯誤）
        return Err(tf("fav.exists", &[&dup.name]));
    }
    let base = if item.name.trim().is_empty() {
        t("fav.title").to_string()
    } else {
        item.name.trim().to_string()
    };
    let mut name = base.clone();
    let mut n = 2;
    while existing.iter().any(|f| f.name == name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    item.name = name.clone();
    settings.update(|s| s.favorites.push(item));
    println!("[AwayTerminal] 我的最愛新增：{name}");
    Ok(name)
}

#[tauri::command]
pub fn fav_delete(name: String, settings: State<'_, Arc<SettingsStore>>) {
    settings.update(|s| s.favorites.retain(|f| f.name != name));
}

#[tauri::command]
pub fn fav_rename(
    name: String,
    new_name: String,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<(), String> {
    let new_name = new_name.trim().to_string();
    if new_name.is_empty() {
        return Err(t("err.needName").to_string());
    }
    let taken = settings
        .get()
        .favorites
        .iter()
        .any(|f| f.name == new_name && f.name != name);
    if taken {
        return Err(tf("fav.nameTaken", &[&new_name]));
    }
    settings.update(|s| {
        if let Some(f) = s.favorites.iter_mut().find(|f| f.name == name) {
            f.name = new_name.clone();
        }
    });
    Ok(())
}

/// 上移／下移一筆（舊版設定視窗可以排序）。`delta` 是 -1 或 +1。
#[tauri::command]
pub fn fav_move(name: String, delta: i32, settings: State<'_, Arc<SettingsStore>>) {
    settings.update(|s| {
        let Some(i) = s.favorites.iter().position(|f| f.name == name) else {
            return;
        };
        let j = i as i32 + delta;
        if j < 0 || j as usize >= s.favorites.len() {
            return;
        }
        s.favorites.swap(i, j as usize);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_treats_same_connection_as_one() {
        let a = FavoriteItem {
            kind: "ssh".into(),
            ssh: Some(SshConnParams {
                host: "Example.TEST".into(),
                port: 22,
                user: "root".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let b = FavoriteItem {
            name: "另一個名字".into(),
            ..a.clone()
        };
        assert_eq!(key_of(&a), key_of(&b), "名稱不同但同一個連線 → 同一個鍵");

        // 主機大小寫不同也算同一個
        let mut c = a.clone();
        c.ssh.as_mut().unwrap().host = "example.test".into();
        assert_eq!(key_of(&a), key_of(&c));

        // 埠或帳號不同就是不同連線
        let mut d = a.clone();
        d.ssh.as_mut().unwrap().port = 2222;
        assert_ne!(key_of(&a), key_of(&d));
        let mut e = a.clone();
        e.ssh.as_mut().unwrap().user = "admin".into();
        assert_ne!(key_of(&a), key_of(&e));
    }

    #[test]
    fn shell_and_conn_keys_use_dir() {
        let a = FavoriteItem {
            kind: "shell".into(),
            dir: "C:\\Work".into(),
            ..Default::default()
        };
        let b = FavoriteItem {
            kind: "shell".into(),
            dir: "c:\\work".into(),
            ..Default::default()
        };
        assert_eq!(key_of(&a), key_of(&b));
        let c = FavoriteItem {
            kind: "conn".into(),
            conn_name: "ClaudeCode".into(),
            dir: "C:\\Work".into(),
            ..Default::default()
        };
        assert_ne!(key_of(&a), key_of(&c));
    }

    /// 同一條連線、同一個目錄，模型不同＝兩筆不同的最愛；代理團隊同一個資料夾只收一筆。
    #[test]
    fn model_and_team_keys() {
        let opus = FavoriteItem {
            kind: "conn".into(),
            conn_name: "ClaudeCode".into(),
            dir: "C:\\Work".into(),
            model: "opus".into(),
            ..Default::default()
        };
        let sonnet = FavoriteItem { model: "sonnet".into(), ..opus.clone() };
        assert_ne!(key_of(&opus), key_of(&sonnet));
        assert_eq!(key_of(&opus), key_of(&FavoriteItem { name: "x".into(), ..opus.clone() }));

        let team = FavoriteItem { kind: "team".into(), dir: "C:\\Work".into(), ..Default::default() };
        let chat = FavoriteItem { kind: "chat".into(), ..team.clone() };
        assert_ne!(key_of(&team), key_of(&chat), "同一個資料夾的團隊與聊天室是兩筆");
        assert_eq!(key_of(&team), key_of(&FavoriteItem { dir: "c:\\work".into(), ..team.clone() }));
    }

    /// 代理團隊的最愛存整組設定（含每格的模型），而且舊的設定檔（沒有 `model`／`team`）讀得進來。
    #[test]
    fn team_favorite_round_trips_and_old_items_still_load() {
        let json = r#"{"name":"proj","kind":"team","dir":"C:\\p","team":{"dir":"C:\\p","slots":[
            {"enabled":true,"backend":"codex","role":"product-manager","model":"gpt-6-sol"},
            {"enabled":true,"backend":"claude-code","role":"software-engineer"}
        ],"maxMessages":50,"idleCheckMinutes":30,"sandbox":false,"kind":"team","rounds":3}}"#;
        let f: FavoriteItem = serde_json::from_str(json).expect("讀得進來");
        let t = f.team.as_ref().expect("有整組設定");
        assert_eq!(t.slots[0].model, "gpt-6-sol");
        assert_eq!(t.slots[1].model, "", "沒寫模型＝預設");
        let back: FavoriteItem = serde_json::from_str(&serde_json::to_string(&f).unwrap()).unwrap();
        assert_eq!(back.team.unwrap().slots[0].model, "gpt-6-sol");

        // 2.0.1 以前存的自訂連線最愛沒有 model／team 欄位
        let old: FavoriteItem =
            serde_json::from_str(r#"{"name":"cc","kind":"conn","dir":"C:\\p","connName":"ClaudeCode"}"#).unwrap();
        assert_eq!(old.model, "");
        assert!(old.team.is_none());
    }

    /// 我的最愛存的東西裡不可以有密碼（舊版 `SavedTab` 也沒有）。
    #[test]
    fn favorite_has_no_password_field() {
        let json = serde_json::to_string(&FavoriteItem {
            kind: "ssh".into(),
            ssh: Some(SshConnParams::default()),
            ..Default::default()
        })
        .unwrap();
        for bad in ["password", "passwd", "passphrase", "secret"] {
            assert!(!json.contains(bad), "最愛不可以有 {bad} 欄位：{json}");
        }
    }
}
