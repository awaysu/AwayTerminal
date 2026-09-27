//! 代理團隊的信箱往返，**不開 GUI、不啟動真的 claude／codex、全程在 `%TEMP%`**。
//!
//! ```text
//! cargo run --example agent_probe
//! ```
//!
//! 驗這幾件事（每一條都是舊版行為表裡的一列）：
//!
//! | # | 驗什麼 |
//! |---|---|
//! | 1 | 角色檔三層組好了，**執行期脈絡**在裡面，Agent ID 是自己的 |
//! | 2 | 兩個假 agent 用真的 ConPTY 起來，角色檔真的被旗標交下去 |
//! | 3 | `agent_ready`：CLI 剛啟動時 false、靜止之後 true |
//! | 4 | PM 寫一封信 → 信箱**穩定 1.5 秒後**才交出來（`STABLE_MS`） |
//! | 5 | 投遞那一行逐字照舊版，打進 worker 的終端機 |
//! | 6 | worker 回信 → 下一輪偵測到 → 打進 PM |
//! | 7 | 同一封信不重送（`.delivered`） |
//! | 8 | 節流：上限 1 → 投遞一封就暫停，恢復後補送 |
//! | 9 | 停止流程的三段（Esc → Ctrl+U → 停止句）真的到得了 CLI |
//! | 10 | 收件人沒在跑 → AwayTerminal 自己回一封 INFO 給寄件人 |
//!
//! ⚠️ 安全：專案資料夾是 `%TEMP%\awayterm-agent-probe-<pid>`，結束時整個刪掉；
//! 只砍自己開的那兩個 PTY（`session.close()`），不按名稱砍任何行程；
//! **完全不碰使用者的 `.ai/`**。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use awayterminal_lib::agent::bus::{MessageBus, SharedBus};
use awayterminal_lib::agent::deliver::{agent_ready, batch_size, delivery_line, Signals};
use awayterminal_lib::agent::message::AgentMessage;
use awayterminal_lib::agent::roles;
use awayterminal_lib::agent::team::Team;
use awayterminal_lib::pty::{self, SpawnOptions};
use awayterminal_lib::session::{ExitInfo, TerminalSession};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 一格：PTY ＋ 收到的畫面內容 ＋ 最後輸出時間。
struct Pane {
    id: u32,
    session: Arc<dyn TerminalSession>,
    text: Arc<Mutex<String>>,
    last_output: Arc<AtomicU64>,
    launched: u64,
    last_delivered: u64,
    delivered_once: bool,
}

impl Pane {
    fn screen(&self) -> String {
        self.text.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn signals(&self) -> Signals {
        Signals {
            now_ms: now_ms(),
            launched_ms: self.launched,
            last_output_ms: self.last_output.load(Ordering::Relaxed),
            // 沒有使用者在打字，也沒有別的地方送出過
            last_input_ms: 0,
            last_submit_ms: 0,
            last_delivered_ms: self.last_delivered,
            via_ps: false,
        }
    }
}

fn spawn_pane(exe: &str, dir: &str, agent_id: &str, role_file: &str, id: u32) -> Pane {
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
    // Claude Code 的注入方式（`--append-system-prompt-file`）＝最常用的那一條，所以用它
    let command_line = format!("\"{exe}\" --id {agent_id} --append-system-prompt-file \"{role_file}\"");
    let session = pty::spawn(
        SpawnOptions {
            command_line,
            cols: 100,
            rows: 30,
            cwd: Some(dir.to_string()),
            graceful_exit_bytes: SpawnOptions::default_graceful_exit_bytes(),
            env: Vec::new(),
            // 沙盒第 2 層：一格一個 kill-on-close 的 Job Object
            kill_on_close: true,
        },
        on_output,
        on_exit,
    )
    .expect("開不起假 agent 的 PTY");
    Pane {
        id,
        session,
        text,
        last_output,
        launched: now_ms(),
        last_delivered: 0,
        delivered_once: true,
    }
}

/// 送一行進去，300ms 之後單獨送 Enter（＝`send_text_then_enter` 的時序）。
fn type_line(pane: &mut Pane, line: &str) {
    pane.session.write(line.as_bytes());
    std::thread::sleep(Duration::from_millis(300));
    pane.session.write(b"\r");
    pane.last_delivered = now_ms();
    pane.delivered_once = false;
}

/// 等一個條件成立（每 100ms 看一次）。
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
    println!("  {} {name}{}", if ok { "PASS" } else { "FAIL" }, if detail.is_empty() { String::new() } else { format!("  — {detail}") });
    ok
}

fn main() {
    let root = std::env::temp_dir().join(format!("awayterm-agent-probe-{}", std::process::id()));
    let data = root.join("appdata");
    let proj = root.join("project");
    let _ = std::fs::create_dir_all(&data);
    let _ = std::fs::create_dir_all(&proj);
    println!("agent_probe：全程在 {}", root.display());
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

    // ---- 1. 團隊與角色檔 ----
    println!("1) 角色檔三層組合");
    let mut team = Team::new("probe", 1, proj.to_string_lossy().to_string());
    for (i, (role, title)) in [
        ("product-manager", "Product Manager"),
        ("software-engineer", "Software Engineer"),
    ]
    .iter()
    .enumerate()
    {
        let s = &mut team.slots[i];
        s.enabled = true;
        s.role = role.to_string();
        s.role_title = title.to_string();
        s.backend = "claude-code".to_string();
    }
    let mut role_files = Vec::new();
    for i in 0..2u32 {
        let p = roles::compose(&data, &team, i + 1).expect("角色檔組不出來");
        role_files.push(p.to_string_lossy().to_string());
    }
    let pm_text = std::fs::read_to_string(&role_files[0]).unwrap_or_default();
    let se_text = std::fs::read_to_string(&role_files[1]).unwrap_or_default();
    check(show(
        pm_text.contains("# Runtime Context (generated by AwayTerminal)"),
        "執行期脈絡在角色檔裡",
        "",
    ));
    check(show(
        pm_text.contains("Agent ID: Agent-11") && se_text.contains("Agent ID: Agent-12"),
        "各格的 Agent ID 正確",
        "",
    ));
    check(show(
        se_text.contains("## Always report to Agent-11"),
        "worker 有「一律回報給 Agent-11」那段",
        "",
    ));
    check(show(
        pm_text.contains("Mailbox: .ai/bus/"),
        "信箱路徑寫在脈絡裡",
        "",
    ));

    // ---- 2. 兩個假 agent 起來 ----
    println!("\n2) 兩個假 agent（真的 ConPTY）");
    let dir = proj.to_string_lossy().to_string();
    let mut pm = spawn_pane(&exe.to_string_lossy(), &dir, "Agent-11", &role_files[0], 1);
    let mut se = spawn_pane(&exe.to_string_lossy(), &dir, "Agent-12", &role_files[1], 2);
    team.slots[0].tab = Some(pm.id);
    team.slots[1].tab = Some(se.id);
    let started = wait_for(10, || {
        pm.screen().contains("Agent-11 ready") && se.screen().contains("Agent-12 ready")
    });
    check(show(started, "兩格都啟動了", ""));
    check(show(
        pm.screen().contains("runtime-context=true my-id=true"),
        "角色檔真的以旗標交下去（CLI 讀到了）",
        "",
    ));

    // ---- 3. agent_ready ----
    println!("\n3) 閒置判定（agent_ready）");
    check(show(
        !agent_ready(&pm.signals()),
        "剛啟動不能打字（要等 5 秒＋靜止 2 秒）",
        "",
    ));
    let ready = wait_for(15, || agent_ready(&pm.signals()) && agent_ready(&se.signals()));
    check(show(ready, "靜下來之後可以打字", ""));

    // ---- 4. 信箱：穩定 1.5 秒才交出來 ----
    println!("\n4) 信箱監看（STABLE_MS = 1500）");
    let bus: SharedBus = Arc::new(MessageBus::new(&proj));
    bus.start();
    let first = bus
        .write_message("Agent-11", "Agent-12", "TASK", "TASK-001", "請做一件事並回報。")
        .expect("寫不出第一封信");
    let immediate = bus.poll();
    check(show(
        immediate.is_empty(),
        "剛寫好的信不會馬上交出來",
        &format!("poll={} 筆", immediate.len()),
    ));
    let mut seen: Vec<AgentMessage> = Vec::new();
    let got = wait_for(8, || {
        seen.extend(bus.poll());
        seen.iter().any(|m| m.file_name == first)
    });
    check(show(got, "穩定之後交出來了", &first));
    let msg = seen.iter().find(|m| m.file_name == first).cloned().unwrap_or_default();
    check(show(
        msg.from == "Agent-11" && msg.to == "Agent-12" && msg.kind == "TASK",
        "front matter 解析正確",
        &format!("{} → {} ({})", msg.from, msg.to, msg.kind),
    ));

    // ---- 5. 投遞 ----
    println!("\n5) 投遞進 worker 的終端機");
    let take = batch_size(1, 30, 0);
    let line = delivery_line(std::slice::from_ref(&msg), 1);
    println!("     打進去的那一行：{line}");
    check(show(take == 1, "一次送一封", ""));
    check(show(
        line.contains(&format!(".ai/bus/{first}")) && line.starts_with("[AwayTerminal] "),
        "投遞文字含信件路徑",
        "",
    ));
    wait_for(15, || agent_ready(&se.signals()));
    type_line(&mut se, &line);
    let delivered = wait_for(10, || se.screen().contains("got:"));
    check(show(delivered, "worker 收到那一行", ""));
    bus.mark_delivered(&first);

    // ---- 6. worker 回信 → 投遞回 PM ----
    println!("\n6) 回信 → 偵測 → 投遞回 PM");
    let replied = wait_for(10, || se.screen().contains("replied "));
    check(show(replied, "worker 寫了回信", ""));
    let mut reply: Option<AgentMessage> = None;
    let found = wait_for(10, || {
        for m in bus.poll() {
            if m.from == "Agent-12" && m.to == "Agent-11" {
                reply = Some(m);
                return true;
            }
        }
        false
    });
    check(show(found, "信箱偵測到回信", ""));
    if let Some(r) = reply.clone() {
        check(show(
            r.kind == "TASK_RESULT" && r.status == "completed",
            "回信的 type／status 解析正確",
            &format!("{} / {}", r.kind, r.status),
        ));
        let back = delivery_line(std::slice::from_ref(&r), 2);
        wait_for(15, || agent_ready(&pm.signals()));
        type_line(&mut pm, &back);
        check(show(
            wait_for(10, || pm.screen().contains("got:")),
            "PM 收到回信通知",
            "",
        ));
        bus.mark_delivered(&r.file_name);

        // ---- 7. 不重送 ----
        println!("\n7) 同一封不重送");
        check(show(bus.is_delivered(&r.file_name), ".delivered 記下來了", ""));
        let again = bus.poll();
        check(show(
            !again.iter().any(|m| m.file_name == r.file_name),
            "再 poll 不會又拿到同一封",
            "",
        ));
    } else {
        check(show(false, "回信的欄位", "沒有回信可以檢查"));
    }

    // ---- 8. 節流 ----
    println!("\n8) 節流（上限 1）");
    check(show(batch_size(3, 1, 0) == 1, "上限 1 → 一次只送一封", ""));
    check(show(batch_size(3, 1, 1) == 0, "額度用完 → 送 0 封（暫停）", ""));
    check(show(batch_size(3, 0, 99) == 3, "不限 → 全部送", ""));
    // 恢復＝把上限調高並歸零（右鍵「投遞」選次數）
    check(show(batch_size(3, 30, 0) == 3, "恢復後補送暫停期間的信", ""));

    // ---- 9. 停止流程 ----
    println!("\n9) 停止任務（Esc → 1s Ctrl+U → 1.5s 停止句）");
    let before = se.screen().len();
    se.session.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(1000));
    se.session.write(b"\x15");
    std::thread::sleep(Duration::from_millis(500));
    let prompt = "先停一下然後記錄目前狀態";
    type_line(&mut se, prompt);
    let stopped = wait_for(10, || se.screen()[before.min(se.screen().len())..].contains(prompt));
    check(show(stopped, "停止句打到 CLI 了", ""));

    // ---- 10. 收件人沒在跑 ----
    println!("\n10) 收件人沒在跑 → AwayTerminal 回一封 INFO");
    let orphan = bus
        .write_message("Agent-11", "Agent-14", "TASK", "TASK-002", "給沒啟用的那格。")
        .expect("寫不出第二封");
    let notice = bus.write_message(
        "AwayTerminal",
        "Agent-11",
        "INFO",
        "TASK-002",
        &format!("Your message .ai/bus/{orphan} was not delivered: Agent-14 is not running in this team."),
    );
    check(show(notice.is_some(), "系統通知寫出來了", notice.as_deref().unwrap_or("")));
    if let Some(n) = notice {
        let mut sys: Option<AgentMessage> = None;
        wait_for(8, || {
            for m in bus.poll() {
                if m.file_name == n {
                    sys = Some(m);
                    return true;
                }
            }
            false
        });
        match sys {
            Some(m) => {
                let l = delivery_line(&[m], 3);
                println!("     通知那一行：{l}");
                check(show(
                    l.contains("AwayTerminal") && !l.contains("from AwayTerminal"),
                    "走 ma.deliverInfo（不是 ma.deliverOne）",
                    "",
                ));
            }
            None => check(show(false, "通知被偵測到", "")),
        }
    }

    // ---- 收乾淨（只砍自己開的那兩個）----
    println!("\n收尾：關掉自己開的兩個 PTY（PID {} / {}）", pm.session.pid(), se.session.pid());
    pm.session.close();
    se.session.close();
    std::thread::sleep(Duration::from_millis(300));
    match std::fs::remove_dir_all(&root) {
        Ok(()) => println!("已刪掉 {}", root.display()),
        Err(e) => println!("⚠️ 刪不掉 {}（{e}）——請手動確認", root.display()),
    }

    println!("\nRESULT: {}", if fail == 0 { format!("PASS（{pass} 項）") } else { format!("FAIL（{fail} 項失敗、{pass} 項通過）") });
    if fail > 0 {
        std::process::exit(1);
    }
}
