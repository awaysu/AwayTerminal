//! OpenCode 資料庫（`opencode.db`）的體檢與清理（2.1.8）。
//!
//! 為什麼要有這個：OpenCode 把每次對話的過程以「事件快照」寫進 SQLite 的 `event` 表，
//! **從不清理**（上游 anomalyco/opencode #52936、#46833、#33356、#47729 都是同一件事，
//! 到 1.19.3 仍沒有清理指令）。使用者的檔案 2026-10-19 長到 7.3 GB，其中 `event` 佔 7.1 GB，
//! OpenCode TUI 一啟動就把它讀進記憶體 → 0.9 GB RSS → Bun 段錯誤閃退，代理團隊裡的
//! OpenCode 格看起來就是「一開就 crash」。
//!
//! 做法（照上游 #38362 維護者的說法：`event` 裡的快照是效能用的副本，對話本文在
//! `message`／`part` 表）：
//! 1. 先把 `opencode.db` 複製成 `opencode.db.bak`（同資料夾、每次覆蓋，不會越備越多）。
//! 2. 刪掉 **7 天前**的 session 的事件列，以及沒有對應 session 的孤兒事件；最近 7 天的照留。
//! 3. `VACUUM` 把檔案縮回來。
//!
//! 清理需要獨佔鎖：OpenCode 還在跑（任何分頁、任何團隊格）就會拿不到鎖，
//! 這時**直接告訴使用者先關掉**，不去砍它（CLAUDE.md：絕不砍不是自己啟動的行程）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::i18n::t;

/// 超過這個大小就提醒（1 GiB）。上游回報 1.4 GB 就會 OOM（#38362）。
pub const WARN_BYTES: u64 = 1024 * 1024 * 1024;

/// 清理時保留最近幾天的 session 事件。
const KEEP_DAYS: i64 = 7;

/// 這次啟動已經提醒過了（每次執行程式只提醒一次，免得開四格團隊問四次）。
static WARNED: AtomicBool = AtomicBool::new(false);

fn home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
}

/// OpenCode 的資料庫路徑：`$XDG_DATA_HOME/opencode/opencode.db`，
/// 沒設就是 `~/.local/share/opencode/opencode.db`（三個平台 OpenCode 都放這裡）。
pub fn db_path() -> Option<PathBuf> {
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".local").join("share")))?;
    Some(base.join("opencode").join("opencode.db"))
}

/// 資料庫目前的大小（主檔＋ `-wal`；不存在＝0）。
pub fn db_bytes(path: &Path) -> u64 {
    let main = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let wal = std::fs::metadata(with_suffix(path, "-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    main + wal
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// 人看的大小（`7.3 GB`／`512 MB`／`12 KB`）。
pub fn human(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.0} MB", b / MB)
    } else {
        format!("{:.0} KB", b / 1024.0)
    }
}

/// 設定視窗「系統 → OpenCode 資料庫」顯示用。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbInfo {
    pub exists: bool,
    pub path: String,
    pub bytes: u64,
    pub size: String,
    /// 大到該提醒了（≥ [`WARN_BYTES`]）。
    pub warn: bool,
}

pub fn info() -> DbInfo {
    let path = db_path().unwrap_or_default();
    let exists = path.is_file();
    let bytes = if exists { db_bytes(&path) } else { 0 };
    DbInfo {
        exists,
        path: path.to_string_lossy().to_string(),
        size: human(bytes),
        warn: exists && bytes >= WARN_BYTES,
        bytes,
    }
}

#[tauri::command]
pub fn ocdb_info() -> DbInfo {
    info()
}

/// 開 OpenCode 的連線或團隊格之前問一次：資料庫過大就回大小字串（前端跳問答），
/// 否則 `None`。**每次執行程式只回一次 `Some`**。
pub fn should_warn(conn: &crate::settings::CustomConn) -> Option<String> {
    if crate::agent::adapters::backend_of(conn) != Some(crate::agent::adapters::Backend::OpenCode) {
        return None;
    }
    let i = info();
    if !i.warn {
        return None;
    }
    if WARNED.swap(true, Ordering::Relaxed) {
        return None;
    }
    Some(i.size)
}

/// 前端 `createSession` 開自訂連線／團隊格之前呼叫（`conn`＝自訂連線名稱；`agent`＝團隊格）。
#[tauri::command]
pub fn ocdb_should_warn(
    conn: Option<String>,
    agent: Option<crate::commands::AgentArgs>,
    settings: tauri::State<'_, std::sync::Arc<crate::settings::SettingsStore>>,
    teams: tauri::State<'_, std::sync::Arc<crate::agent::TeamManager>>,
) -> Option<String> {
    let c = if let Some(name) = conn.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        crate::custom::find(&settings, name)?
    } else if let Some(a) = agent {
        crate::agent::plan_slot(&settings, &teams, &a.team, a.index).ok()?.conn
    } else {
        return None;
    };
    should_warn(&c)
}

/// 清理結果。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanResult {
    pub before: String,
    pub after: String,
    pub backup: String,
    /// 刪了幾筆事件。
    pub removed: u64,
}

/// 備份 → 刪舊事件 → VACUUM。拿不到獨佔鎖（OpenCode 還在跑）就回 `err.ocdbLocked`。
pub fn cleanup() -> Result<CleanResult, String> {
    let path = db_path().filter(|p| p.is_file()).ok_or_else(|| t("err.ocdbMissing"))?;
    let before = db_bytes(&path);

    // 先確認拿得到獨佔鎖，再去花時間複製 7 GB（OpenCode 在跑的話複製也是白做）
    let conn = rusqlite::Connection::open(&path).map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    if let Err(e) = conn.execute_batch("BEGIN IMMEDIATE; COMMIT;") {
        return Err(if is_busy(&e) { t("err.ocdbLocked") } else { e.to_string() });
    }
    drop(conn);

    // 1. 備份（含 -wal：還沒寫回主檔的那一段也要）
    let backup = with_suffix(&path, ".bak");
    std::fs::copy(&path, &backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    let wal = with_suffix(&path, "-wal");
    if wal.is_file() {
        let _ = std::fs::copy(&wal, with_suffix(&backup, "-wal"));
    }

    // 2. 刪舊事件
    let conn = rusqlite::Connection::open(&path).map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    let cutoff = chrono::Utc::now().timestamp_millis() - KEEP_DAYS * 24 * 3600 * 1000;
    let removed = (|| -> rusqlite::Result<u64> {
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        let a = conn.execute(
            "DELETE FROM event WHERE aggregate_id IN (SELECT id FROM session WHERE time_updated < ?1)",
            [cutoff],
        )?;
        let b = conn.execute(
            "DELETE FROM event WHERE aggregate_id NOT IN (SELECT id FROM session)",
            [],
        )?;
        conn.execute_batch("COMMIT;")?;
        Ok((a + b) as u64)
    })()
    .map_err(|e| if is_busy(&e) { t("err.ocdbLocked") } else { e.to_string() })?;

    // 3. 壓縮檔案（VACUUM 不能在交易裡）
    conn.execute_batch("VACUUM;").map_err(|e| e.to_string())?;
    // 把 WAL 寫回主檔並截掉，大小才會真的變
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    drop(conn);

    let after = db_bytes(&path);
    println!(
        "[AwayTerminal] OpenCode 資料庫清理：{} → {}，刪 {removed} 筆事件，備份 {}",
        human(before),
        human(after),
        backup.display()
    );
    Ok(CleanResult {
        before: human(before),
        after: human(after),
        backup: backup.to_string_lossy().to_string(),
        removed,
    })
}

fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == rusqlite::ErrorCode::DatabaseBusy || f.code == rusqlite::ErrorCode::DatabaseLocked
    )
}

/// 設定視窗的「清理…」。`async`＝丟到執行緒池，7 GB 的複製與 VACUUM 不卡 UI。
#[tauri::command]
pub async fn ocdb_cleanup() -> Result<CleanResult, String> {
    tauri::async_runtime::spawn_blocking(cleanup)
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes() {
        assert_eq!(human(7_743_602_688), "7.2 GB");
        assert_eq!(human(300 * 1024 * 1024), "300 MB");
        assert_eq!(human(12 * 1024), "12 KB");
    }

    /// 在暫存資料夾造一個長得像 OpenCode 的資料庫：舊 session 的事件要刪、新的要留、孤兒要刪。
    #[test]
    fn cleanup_keeps_recent_sessions() {
        let dir = std::env::temp_dir().join(format!("at-ocdb-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("opencode")).unwrap();
        let db = dir.join("opencode").join("opencode.db");
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch(
                "CREATE TABLE session(id TEXT PRIMARY KEY, time_updated INTEGER NOT NULL);
                 CREATE TABLE event(id TEXT PRIMARY KEY, aggregate_id TEXT NOT NULL, seq INTEGER, type TEXT, data TEXT);",
            )
            .unwrap();
            let now = chrono::Utc::now().timestamp_millis();
            c.execute("INSERT INTO session VALUES ('old', ?1)", [now - 30 * 86_400_000]).unwrap();
            c.execute("INSERT INTO session VALUES ('new', ?1)", [now - 3_600_000]).unwrap();
            for i in 0..50 {
                c.execute(
                    "INSERT INTO event VALUES (?1, 'old', ?2, 'message.part.updated.1', ?3)",
                    rusqlite::params![format!("o{i}"), i, "x".repeat(10_000)],
                )
                .unwrap();
            }
            c.execute("INSERT INTO event VALUES ('n1', 'new', 1, 't', 'keep')", []).unwrap();
            c.execute("INSERT INTO event VALUES ('z1', 'gone', 1, 't', 'orphan')", []).unwrap();
        }
        // 用 XDG_DATA_HOME 把 db_path 指到暫存夾（測試執行緒間不平行改環境變數：這個測試自己跑）
        std::env::set_var("XDG_DATA_HOME", &dir);
        let r = cleanup().expect("cleanup ok");
        std::env::remove_var("XDG_DATA_HOME");
        assert_eq!(r.removed, 51, "50 筆舊的 + 1 筆孤兒");
        let c = rusqlite::Connection::open(&db).unwrap();
        let left: i64 = c.query_row("SELECT count(*) FROM event", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 1, "新 session 的那一筆要留著");
        assert!(dir.join("opencode").join("opencode.db.bak").is_file(), "有備份");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
