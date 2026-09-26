//! **只給 `--verify` 用**：驗巨集的 `exec` 在沙盒分頁裡的三件事。
//!
//! 1. `exec … 1`（有等）→ `result` 是子行程的 **exit code**
//! 2. `exec` 出來的子行程**帶到沙盒的 `TEMP`**（PM 在 TASK-014 定的規則）
//! 3. 子行程（含孫行程）在**巨集結束時被收掉**（進了巨集的 Job Object）
//!
//! 巨集檔由這裡產生——TTL 的字串沒有反斜線轉義，引號要很小心，在 JS 那邊拼容易出錯。
//!
//! ⚠️ 這裡刻意用 **`powershell.exe`（System32 的真檔案）而不是 `pwsh`**：
//! `pwsh` 在很多機器上是 **Microsoft Store 的 app execution alias**，真正的行程由
//! AppX 啟動服務開出來，**不在我們的 Job Object 裡**，收不掉（`examples/job_probe.rs`
//! 兩種都跑過，證據在那支 probe 的註解）。`-ExecutionPolicy Bypass` 是因為
//! Windows PowerShell 預設的 `Restricted` 會拒跑 `.ps1`。

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::tabs::TabManager;

/// 產生巨集、跑完、讀結果。
#[tauri::command]
pub async fn exec_verify(
    app: AppHandle,
    id: u32,
    tabs: State<'_, Arc<TabManager>>,
) -> Result<serde_json::Value, String> {
    let tmp = std::env::temp_dir();
    let ttl = tmp.join("awayterm-verify-exec.ttl");
    let temp_out = tmp.join("awayterm-exec-temp.txt");
    let pid_out = tmp.join("awayterm-exec-pid.txt");
    let code_out = tmp.join("awayterm-exec-code.txt");
    for f in [&temp_out, &pid_out, &code_out] {
        let _ = std::fs::remove_file(f);
    }

    // 巨集內容：
    //   1. 開一個會活 30 秒的孫行程（pwsh），讓它把 `$PID` 與 `%TEMP%` 寫出來；**不等它**
    //   2. 再開一個馬上結束的（exit 7），**等它** → `result` 應該是 7
    //   3. 用 TTL 的檔案指令把 `result` 寫出來（順手驗檔案那組在 app 裡也能用）
    //
    // ⚠️ TTL 的字串**沒有反斜線轉義、也不能在單引號裡放單引號**（第一版就是這樣踩到
    // `Variable not initialized.`：`'…'C:\…'…'` 在第一個內層引號就結束了）。
    // 所以 PowerShell 的部分寫成獨立的 .ps1，TTL 那一行只剩「路徑」，
    // 再用 `#34`（雙引號的字元碼）把路徑包起來——片段要**緊貼**才會接成一個字串。
    let ps1 = tmp.join("awayterm-verify-exec.ps1");
    let ps_body = format!(
        "$PID | Out-File -Encoding ascii '{}'\r\n\
         $env:TEMP | Out-File -Encoding ascii '{}'\r\n\
         Start-Sleep 30\r\n",
        pid_out.display(),
        temp_out.display()
    );
    std::fs::write(&ps1, ps_body.as_bytes()).map_err(|e| format!("寫 ps1 失敗：{e}"))?;

    // TTL 那一行（`#34` 是雙引號的字元碼，片段緊貼才會接成一個字串）：
    //   exec "pwsh -NoLogo -NoProfile -File "#34"<路徑>"#34 'hide' 0
    let exec_line = format!(
        "exec {q}powershell -NoLogo -NoProfile -ExecutionPolicy Bypass \
         -File {q}#34{q}{ps1}{q}#34 'hide' 0",
        q = '"',
        ps1 = ps1.display()
    );
    let src = format!(
        "{exec_line}\n\
         mpause 1500\n\
         exec 'cmd /c exit 7' 'hide' 1\n\
         code = result\n\
         fileopen fh {q}{code}{q} 0\n\
         int2str s code\n\
         filewriteln fh s\n\
         fileclose fh\n\
         end\n",
        q = '"',
        code = code_out.display()
    );
    std::fs::write(&ttl, src.as_bytes()).map_err(|e| format!("寫巨集檔失敗：{e}"))?;

    // 跑（同 `macro_verify`：開始 → 等它自己結束）
    super::runner::macro_run(app.clone(), id, ttl.to_string_lossy().into_owned()).await?;
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < 30 {
        if tabs.macro_of(id).is_none() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    if tabs.macro_of(id).is_some() {
        super::runner::stop_for_tab(&app, id);
        return Err("巨集 30 秒還沒結束（已中斷）".to_string());
    }

    // 巨集結束了 → Job Object 應該已經 drop，孫行程要不見了
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let pid: u32 = std::fs::read_to_string(&pid_out)
        .map_err(|e| format!("讀不到 PID 檔（子行程沒開起來？）：{e}"))?
        .trim()
        .parse()
        .map_err(|e| format!("PID 檔內容怪怪的：{e}"))?;
    let alive_after = crate::status::pid_exists(pid);

    let child_temp = std::fs::read_to_string(&temp_out).unwrap_or_default().trim().to_string();
    let sandbox_root = tabs.sandbox_of(id).map(|s| s.root).unwrap_or_default();
    // ⚠️ 兩邊都要正規化：沙盒 root 可能是「`C:/…/AwayTerminal2\.ai\sandbox\…`」這種
    // 混著兩種分隔符的字串（repo 路徑是 `/`、後面接的是 `\`），只 replace 一邊會假失敗。
    let norm = |s: &str| s.to_lowercase().replace('/', "\\");
    let temp_in_sandbox =
        !sandbox_root.is_empty() && norm(&child_temp).starts_with(&norm(&sandbox_root));

    let code = std::fs::read_to_string(&code_out)
        .unwrap_or_default()
        .trim()
        .parse::<i32>()
        .unwrap_or(-999);

    for f in [&ttl, &ps1, &temp_out, &pid_out, &code_out] {
        let _ = std::fs::remove_file(f);
    }

    Ok(serde_json::json!({
        "note": format!("巨集跑完（{:?}）", start.elapsed()),
        "exitCode": code,
        "pid": pid,
        "aliveAfterMacro": alive_after,
        "childTemp": child_temp,
        "sandboxRoot": sandbox_root,
        "tempInSandbox": temp_in_sandbox,
    }))
}
