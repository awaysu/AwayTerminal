//! AI 聊天室的輪流發言，**不開 GUI、不啟動真的 claude／codex、全程在 `%TEMP%`**。
//!
//! ```text
//! cargo run --example chat_probe
//! ```
//!
//! 驗這幾件事（每一條都是舊版行為表裡的一列）：
//!
//! | # | 驗什麼 |
//! |---|---|
//! | 1 | 聊天室角色檔三層組好了，**主持人那一段只有第 1 位有** |
//! | 2 | 三個假 agent 用真的 ConPTY 起來，角色檔真的以旗標交下去 |
//! | 3 | 給主題 → `transcript.md` 有開頭（主題、回合數、參加者） |
//! | 4 | 第 1 回合三個人**依格號輪流**被問到，每個人寫自己的 `r1-Agent-xx.md` |
//! | 5 | 每個人的發言被接進 `transcript.md`（順序正確） |
//! | 6 | 使用者插話 → 進 `transcript.md`，下一位看得到 |
//! | 7 | 回合數跑完 → 進入寫結論，只問**主持人** |
//! | 8 | 主持人寫 `conclusion.md` → 接進紀錄 → 狀態變成已結束 |
//! | 9 | 「結束討論」可以提前收尾（這一輪結束後就去寫結論） |
//! | 10 | **上一場留下的發言檔不會被當成這一場的**（`read_finished` 的時間判斷） |
//!
//! ⚠️ 安全：專案資料夾是 `%TEMP%\awayterm-chat-probe-<pid>`，結束時整個刪掉；
//! 只關自己開的那幾個 PTY，不按名稱砍任何行程；**完全不碰使用者的 `.ai/`**。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use awayterminal_lib::agent::chat::{self, ChatAction};
use awayterminal_lib::agent::deliver::{agent_ready, Signals};
use awayterminal_lib::agent::roles;
use awayterminal_lib::agent::team::{ChatPhase, GroupKind, Slot, Team};
use awayterminal_lib::pty::{self, SpawnOptions};
use awayterminal_lib::session::{ExitInfo, TerminalSession};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct Pane {
    session: Arc<dyn TerminalSession>,
    text: Arc<Mutex<String>>,
    last_output: Arc<AtomicU64>,
}

impl Pane {
    fn screen(&self) -> String {
        self.text.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

fn spawn_pane(exe: &str, dir: &str, role_file: &str) -> Pane {
    let text = Arc::new(Mutex::new(String::new()));
    let last_output = Arc::new(AtomicU64::new(now_ms()));
    let t2 = text.clone();
    let o2 = last_output.clone();
    let on_output: awayterminal_lib::session::OnOutput = Arc::new(move |bytes: &[u8]| {
        o2.store(now_ms(), Ordering::Relaxed);
        if let Ok(mut g) = t2.lock() {
            g.push_str(&String::from_utf8_lossy(bytes));
        }
    });
    let on_exit: awayterminal_lib::session::OnExit = Arc::new(|_: ExitInfo| {});
    let session = pty::spawn(
        SpawnOptions {
            command_line: format!("\"{exe}\" --append-system-prompt-file \"{role_file}\""),
            cols: 100,
            rows: 30,
            cwd: Some(dir.to_string()),
            graceful_exit_bytes: SpawnOptions::default_graceful_exit_bytes(),
            env: Vec::new(),
            kill_on_close: true,
        },
        on_output,
        on_exit,
    )
    .expect("開不起假 agent 的 PTY");
    Pane {
        session,
        text,
        last_output,
    }
}

fn wait_for(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    f()
}

fn show(ok: bool, name: &str, detail: &str) -> bool {
    println!(
        "  {} {name}{}",
        if ok { "PASS" } else { "FAIL" },
        if detail.is_empty() {
            String::new()
        } else {
            format!("  — {detail}")
        }
    );
    ok
}

/// 測試資料夾，**drop 就刪掉**——`main` panic 時也會刪。
///
/// 為什麼要 Drop：第一次跑這支 probe 時在 i18n 那裡 panic，最後的 `remove_dir_all`
/// 跑不到，`%TEMP%wayterm-chat-probe-<pid>` 就留下來了（和 `roles.rs` 測試同一個坑）。
struct Root(std::path::PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) => println!("已刪掉 {}", self.0.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => println!("⚠️ 刪不掉 {}（{e}）——請手動確認", self.0.display()),
        }
    }
}

fn main() {
    let root = std::env::temp_dir().join(format!("awayterm-chat-probe-{}", std::process::id()));
    let data = root.join("appdata");
    let proj = root.join("project");
    let _ = std::fs::create_dir_all(&data);
    let _ = std::fs::create_dir_all(&proj);
    let _root_guard = Root(root.clone());
    println!("chat_probe：全程在 {}", root.display());
    println!("（不啟動真的 claude／codex，不碰使用者的 .ai/）\n");

    let exe = std::env::current_exe()
        .expect("拿不到自己的路徑")
        .with_file_name(if cfg!(windows) { "fake_agent.exe" } else { "fake_agent" });
    if !exe.is_file() {
        println!("FAIL  找不到 {}，先跑 `cargo build --example fake_agent`", exe.display());
        std::process::exit(1);
    }

    let mut pass = 0;
    let mut fail = 0;
    let mut check = |ok: bool| {
        if ok {
            pass += 1;
        } else {
            fail += 1;
        }
    };

    // ---- 1. 聊天室角色檔 ----
    println!("1) 聊天室角色檔（主持人 ＋ 反方辯論者 ＋ 情報研究員）");
    let dir = proj.to_string_lossy().to_string();
    let mut team = Team::new("chat-probe", 1, dir.clone());
    team.kind = GroupKind::Chat;
    team.work_dir = dir.clone();
    team.rounds = 2;
    for (i, role) in ["host", "devils-advocate", "researcher"].iter().enumerate() {
        let s = &mut team.slots[i];
        s.enabled = true;
        s.role = role.to_string();
        s.role_title = roles::title_of(&roles::CHAT, &data, role);
        s.backend = "claude-code".to_string();
    }
    // 先給資料夾名（角色檔裡寫著它）
    team.chat_folder = chat::new_chat_folder(&dir);
    let mut role_files = Vec::new();
    for i in 1..=3u32 {
        let p = roles::compose(&roles::CHAT, &data, &team, i).expect("角色檔組不出來");
        role_files.push(p.to_string_lossy().to_string());
    }
    let host_text = std::fs::read_to_string(&role_files[0]).unwrap_or_default();
    let other_text = std::fs::read_to_string(&role_files[1]).unwrap_or_default();
    check(show(
        host_text.contains("# AwayTerminal AI 聊天室 共同規則")
            && host_text.contains("# 主持人")
            && host_text.contains("# Runtime Context（AwayTerminal 產生）"),
        "三層都在（共同規則 ＋ 角色 ＋ 執行期脈絡）",
        "",
    ));
    check(show(
        host_text.contains("## 你是主持人") && !other_text.contains("## 你是主持人"),
        "「你是主持人」只有第 1 位有",
        "",
    ));
    check(show(
        team.slots[0].role_title == "主持人"
            && team.slots[1].role_title == "反方辯論者"
            && team.slots[2].role_title == "情報研究員",
        "角色標題取自角色檔的第一個標題",
        &format!(
            "{} / {} / {}",
            team.slots[0].role_title, team.slots[1].role_title, team.slots[2].role_title
        ),
    ));

    // ---- 10. 上一場的紀錄與發言檔都先放好，證明這一場不會用到它們 ----
    // `start_discussion` 的判斷是「這個資料夾已經有 transcript.md」→ 換新資料夾，
    // 所以要兩個檔都寫（只寫發言檔的話資料夾不會換——那種情況靠 `read_finished` 的
    // 時間判斷擋，有單元測試 `read_finished_rejects_stale_and_fresh_files`）。
    let stale_folder = team.chat_folder.clone();
    let stale_turn = chat::chat_path(&team, &chat::turn_file(1, "Agent-11"));
    let _ = std::fs::create_dir_all(stale_turn.parent().unwrap());
    std::fs::write(&stale_turn, "這是上一場留下的舊發言，不該被採用").unwrap();
    chat::write_transcript(&team, "# 上一場的紀錄");

    // ---- 2. 三個假 agent 起來 ----
    println!("\n2) 三個假 agent（真的 ConPTY）");
    let mut panes = Vec::new();
    for rf in &role_files {
        panes.push(spawn_pane(&exe.to_string_lossy(), &dir, rf));
    }
    for (i, p) in panes.iter().enumerate() {
        team.slots[i].tab = Some(10 + i as u32);
        team.slots[i].launched_ms = now_ms() as u128;
        let _ = p;
    }
    // 看短的那一行（長路徑那一行會被 ConPTY 折行，`contains()` 會對不上）
    let started = wait_for(20, || {
        panes
            .iter()
            .all(|p| p.screen().contains("role-check ctx=true mine=true"))
    });
    if !started {
        for (i, p) in panes.iter().enumerate() {
            println!("     pane{i} 畫面：{:?}", p.screen());
        }
    }
    check(show(
        started,
        "三格都啟動、角色檔以旗標交下去",
        &panes
            .iter()
            .enumerate()
            .map(|(i, p)| {
                format!(
                    "pane{i}={}",
                    if p.screen().contains("role-check ctx=true mine=true") {
                        "ok"
                    } else {
                        "?"
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(" "),
    ));

    // tick 要用的兩個 closure：分頁活著嗎、那一格閒下來了嗎
    let alive = |_id: u32| true;
    let ready_of = |panes: &Vec<Pane>, s: &Slot| -> bool {
        let Some(tab) = s.tab else { return false };
        let i = (tab - 10) as usize;
        let Some(p) = panes.get(i) else { return false };
        agent_ready(&Signals {
            now_ms: now_ms(),
            launched_ms: s.launched_ms as u64,
            last_output_ms: p.last_output.load(Ordering::Relaxed),
            last_input_ms: 0,
            last_submit_ms: 0,
            last_delivered_ms: s.last_delivered_ms as u64,
            via_ps: false,
        })
    };

    // ---- 3. 給主題 ----
    println!("\n3) 給主題");
    chat::start_discussion(&mut team, "要不要自己寫 SSH，還是呼叫系統的 ssh.exe？");
    // 換了資料夾之後角色檔要重組（裡面寫著資料夾路徑）
    for i in 1..=3u32 {
        let _ = roles::compose(&roles::CHAT, &data, &team, i);
    }
    let header = chat::transcript_header(&team);
    chat::write_transcript(&team, &header);
    let tr = chat::chat_path(&team, "transcript.md");
    let tr_text = std::fs::read_to_string(&tr).unwrap_or_default();
    check(show(
        tr_text.contains("要不要自己寫 SSH") && tr_text.contains("Agent-11") && tr_text.contains("Agent-13"),
        "transcript.md 有主題、回合數、參加者",
        "",
    ));
    check(show(
        team.phase == ChatPhase::Discussing && team.round == 1 && team.speaker == 0,
        "狀態＝討論中、第 1 回合、第 1 位",
        "",
    ));
    check(show(
        team.chat_folder != stale_folder
            && !chat::chat_path(&team, &chat::turn_file(1, "Agent-11")).is_file()
            && stale_turn.is_file(),
        "換了新資料夾 → 上一場的紀錄與發言檔都留在舊資料夾",
        &format!("{stale_folder} → {}", team.chat_folder),
    ));

    // ---- 4～8. 跑完整場（tick 迴圈）----
    println!("\n4) 輪流發言（2 回合 × 3 人）＋ 插話 ＋ 結論");
    let mut asked: Vec<String> = Vec::new();
    let mut said_once = false;
    let deadline = Instant::now() + Duration::from_secs(150);
    while team.phase != ChatPhase::Done && Instant::now() < deadline {
        let ready = |s: &Slot| ready_of(&panes, s);
        match chat::tick(&mut team, now_ms() as u128, &alive, &ready) {
            ChatAction::Type { tab, text } => {
                let i = (tab - 10) as usize;
                // 和 `send_text_then_enter` 同樣的時序：文字一次、300ms 後單獨送 Enter
                panes[i].session.write(text.as_bytes());
                std::thread::sleep(Duration::from_millis(300));
                panes[i].session.write(b"\r");
                let who = team.slots[i].agent_id();
                let kind = if text.contains("conclusion.md") { "結論" } else { "發言" };
                asked.push(format!("r{}:{who}:{kind}", team.round));
                println!("     → 問 {who}（第 {} 回合，{kind}）", team.round);
                // 第 1 回合第 1 位講完之後插一句話（使用者插話）
                if !said_once && asked.len() == 2 {
                    chat::user_said(&team, "補充一下：舊設備很多，相容性比效能重要。");
                    said_once = true;
                    println!("     → 使用者插話");
                }
            }
            ChatAction::None => std::thread::sleep(Duration::from_millis(200)),
        }
    }
    let tr_text = std::fs::read_to_string(&tr).unwrap_or_default();

    check(show(
        team.phase == ChatPhase::Done,
        "跑到結束（回合數跑完 → 結論 → Done）",
        &format!("phase={:?} round={}", team.phase, team.round),
    ));
    // 順序：r1 三個人、r2 三個人、最後結論只問主持人
    let expect: Vec<String> = vec![
        "r1:Agent-11:發言".into(),
        "r1:Agent-12:發言".into(),
        "r1:Agent-13:發言".into(),
        "r2:Agent-11:發言".into(),
        "r2:Agent-12:發言".into(),
        "r2:Agent-13:發言".into(),
        "r3:Agent-11:結論".into(),
    ];
    check(show(
        asked == expect,
        "依格號輪流、每回合每人一次、結論只問主持人",
        &asked.join(" "),
    ));
    check(show(
        (1..=3).all(|i| chat::chat_path(&team, &chat::turn_file(1, &format!("Agent-1{i}"))).is_file()),
        "第 1 回合三個發言檔都寫出來了",
        "",
    ));
    check(show(
        tr_text.matches("## 第 1 回合").count() == 3 && tr_text.matches("## 第 2 回合").count() == 3,
        "六則發言都接進 transcript.md",
        &format!(
            "r1={} r2={}",
            tr_text.matches("## 第 1 回合").count(),
            tr_text.matches("## 第 2 回合").count()
        ),
    ));
    check(show(
        tr_text.contains("## 使用者") && tr_text.contains("相容性比效能重要"),
        "使用者插話進了紀錄",
        "",
    ));
    check(show(
        tr_text.contains("## 結論") && chat::chat_path(&team, "conclusion.md").is_file(),
        "主持人寫了結論並接進紀錄",
        "",
    ));
    // 插話要在第 1 位的發言之後、第 2 位之前（下一位才看得到）
    let i_user = tr_text.find("## 使用者");
    let i_second = tr_text.find("## 第 1 回合 · Agent-12");
    check(show(
        matches!((i_user, i_second), (Some(u), Some(s)) if u < s),
        "插話的位置在下一位發言之前",
        "",
    ));

    // ---- 9. 「結束討論」提前收尾 ----
    println!("\n9) 「結束討論」提前收尾");
    let mut t2 = Team::new("k2", 2, dir.clone());
    t2.kind = GroupKind::Chat;
    t2.work_dir = dir.clone();
    t2.rounds = 9;
    t2.phase = ChatPhase::Discussing;
    for i in 0..3 {
        t2.slots[i].enabled = true;
        t2.slots[i].tab = Some(10 + i as u32);
    }
    t2.speaker = 1; // 第 1 回合已經有人講過
    t2.end_requested = true;
    chat::advance_turn(&mut t2, 3);
    check(show(
        t2.phase == ChatPhase::Concluding,
        "按了結束 → 這一輪結束後就去寫結論（不再多問一個人）",
        &format!("round={}", t2.round),
    ));

    // ---- 收乾淨 ----
    println!(
        "\n收尾：關掉自己開的 {} 個 PTY（PID {}）",
        panes.len(),
        panes
            .iter()
            .map(|p| p.session.pid().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    for p in &panes {
        p.session.close();
    }
    std::thread::sleep(Duration::from_millis(400));
    // 資料夾由 `_root_guard` 的 Drop 刪（panic 時也會刪）

    println!(
        "\nRESULT: {}",
        if fail == 0 {
            format!("PASS（{pass} 項）")
        } else {
            format!("FAIL（{fail} 項失敗、{pass} 項通過）")
        }
    );
    if fail > 0 {
        std::process::exit(1);
    }
}
