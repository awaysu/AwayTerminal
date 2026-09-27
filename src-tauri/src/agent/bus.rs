//! 信箱監看（搬移舊版 `Services/MultiAgent/MessageBus.cs`）。
//!
//! 專案資料夾的 `.ai/bus/`，一封信一個 `.md`。**寄信＝agent 寫檔**——舊版刻意不用 hook
//!（1.1.11 那版用 Stop hook ＋ curl ＋ localhost HTTP，使用者決定「完全不用 hook」）：
//! 寫檔是四家 CLI 本來就會的事，沒有信任審核、沒有父行程鏈的問題。
//!
//! | 項目 | 舊版 | 這裡 |
//! |---|---|---|
//! | 發現新信 | `FileSystemWatcher`（Created／Changed／Renamed）＋**每 3 秒掃目錄**當備援 | **只用輪詢**（見下），掃描間隔照舊版 3 秒、tick 700ms |
//! | 「寫完了」判斷 | 最後修改時間距今 ≥ **1500ms** 才讀（agent 可能 Write 完再 Edit） | 同 |
//! | 已投遞 | `.delivered`（每行一個檔名）；開組時先讀，已投遞的不重送 | 同 |
//! | 程式關著時寫進來的信 | 下次開啟後投遞 | 同 |
//! | 同一資料夾開兩組 | 各自一個 bus，依收件人的組號各取所需 | 同 |
//! | 讀不到（被鎖） | 不標記，下一輪再試 | 同 |
//!
//! **為什麼只用輪詢**：舊版兩者並用，而且註解寫明「大量寫入、網路磁碟會漏事件」才加的輪詢；
//! 真正決定「什麼時候讀」的是那個 1500ms 穩定期，watcher 只是讓候選名單早一點進來。
//! 700ms 一次的 `read_dir` 對一個只有幾十個檔的資料夾是可忽略的成本，
//! 少一個平台相依的元件（notify crate 在 Windows／mac／Linux 行為不同）反而更穩。
//! 這是**刻意的差異**，寫進 `docs/MULTI-AGENT.md`。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::message::{self, AgentMessage};

/// 檔案最後修改時間距今至少這麼久才讀（agent 可能 Write 完再 Edit）。照舊版 `StableMs`。
pub const STABLE_MS: u128 = 1500;
/// 掃目錄的間隔（照舊版 `ScanEveryMs`）。
pub const SCAN_EVERY_MS: u128 = 3000;
/// 已投遞紀錄的檔名（照舊版）。
pub const DELIVERED_NAME: &str = ".delivered";

/// 一個團隊的信箱。
pub struct MessageBus {
    pub project_dir: PathBuf,
    pub bus_dir: PathBuf,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// 已經交給上層處理過的檔名（同一個檔只觸發一次）。
    raised: HashSet<String>,
    /// `.delivered` 的內容。
    delivered: HashSet<String>,
    /// 看到但還沒穩定的候選。
    candidates: HashSet<String>,
    last_scan_ms: u128,
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn mtime_ms(p: &Path) -> Option<u128> {
    p.metadata()
        .and_then(|m| m.modified())
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis())
}

impl MessageBus {
    pub fn new(project_dir: impl Into<PathBuf>) -> Self {
        let project_dir = project_dir.into();
        let bus_dir = project_dir.join(".ai").join("bus");
        Self {
            project_dir,
            bus_dir,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// 建資料夾、讀 `.delivered`。
    pub fn start(&self) {
        let _ = std::fs::create_dir_all(&self.bus_dir);
        let mut g = self.lock();
        if let Ok(text) = std::fs::read_to_string(self.delivered_path()) {
            for line in text.lines() {
                let t = line.trim();
                if !t.is_empty() {
                    g.delivered.insert(t.to_string());
                }
            }
        }
        g.last_scan_ms = 0; // 第一次 tick 立刻掃（開組前就在的信）
        println!(
            "[AwayTerminal] 代理團隊信箱：{}（已投遞 {} 封）",
            self.bus_dir.display(),
            g.delivered.len()
        );
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn delivered_path(&self) -> PathBuf {
        self.bus_dir.join(DELIVERED_NAME)
    }

    /// 掃一次：回傳**這一輪新發現且已經寫完**的信（照編號／檔名排序）。
    ///
    /// 呼叫端每 700ms 叫一次（同舊版的 timer）。
    pub fn poll(&self) -> Vec<AgentMessage> {
        let now = now_ms();
        let mut ready: Vec<String> = Vec::new();
        {
            let mut g = self.lock();
            if now.saturating_sub(g.last_scan_ms) >= SCAN_EVERY_MS {
                g.last_scan_ms = now;
                if let Ok(rd) = std::fs::read_dir(&self.bus_dir) {
                    for e in rd.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if message::is_message_name(&name) && !g.raised.contains(&name) {
                            g.candidates.insert(name);
                        }
                    }
                }
            }
            for name in g.candidates.clone() {
                if g.raised.contains(&name) || g.delivered.contains(&name) {
                    g.candidates.remove(&name);
                    continue;
                }
                let full = self.bus_dir.join(&name);
                let Some(m) = mtime_ms(&full) else {
                    g.candidates.remove(&name); // 檔不見了
                    continue;
                };
                if now.saturating_sub(m) < STABLE_MS {
                    continue; // 還在寫
                }
                g.candidates.remove(&name);
                g.raised.insert(name.clone());
                ready.push(name);
            }
        }
        ready.sort();
        let mut out = Vec::new();
        for name in ready {
            match message::parse_file(&self.bus_dir.join(&name)) {
                Some(m) => out.push(m),
                None => {
                    // 讀不到（被鎖）→ 拿掉 raised，下一輪再試（照舊版）
                    self.lock().raised.remove(&name);
                }
            }
        }
        out
    }

    /// 這封信已經投遞過了嗎。
    pub fn is_delivered(&self, file_name: &str) -> bool {
        self.lock().delivered.contains(file_name)
    }

    /// 記為已投遞（追加一行到 `.delivered`；同名只記一次）。
    pub fn mark_delivered(&self, file_name: &str) {
        {
            let mut g = self.lock();
            if !g.delivered.insert(file_name.to_string()) {
                return;
            }
        }
        use std::io::Write;
        let path = self.delivered_path();
        match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            Ok(mut f) => {
                let _ = writeln!(f, "{file_name}");
            }
            Err(e) => println!("[AwayTerminal] 代理團隊信箱：寫 .delivered 失敗（{e}）"),
        }
    }

    /// AwayTerminal 自己寄一封信（例：收件人沒啟用）。回傳檔名。
    pub fn write_message(
        &self,
        from: &str,
        to: &str,
        kind: &str,
        task: &str,
        body: &str,
    ) -> Option<String> {
        let _ = std::fs::create_dir_all(&self.bus_dir);
        let name = message::next_file_name(&self.bus_dir, from, to);
        let mut text = String::from("---\n");
        text.push_str(&format!("from: {from}\n"));
        text.push_str(&format!("to: {to}\n"));
        text.push_str(&format!("type: {kind}\n"));
        if !task.trim().is_empty() {
            text.push_str(&format!("task: {task}\n"));
        }
        text.push_str("---\n");
        text.push_str(body.trim_end());
        text.push('\n');
        match std::fs::write(self.bus_dir.join(&name), text.as_bytes()) {
            Ok(()) => Some(name),
            Err(e) => {
                println!("[AwayTerminal] 代理團隊信箱：寫信失敗（{e}）");
                None
            }
        }
    }
}

/// 專案的 `.gitignore` 沒有 `.ai/` 就追加一行（沒有 `.gitignore` 就建立）。照舊版 `EnsureGitIgnore`。
///
/// ⚠️ 這是**唯一**會動使用者 `.gitignore` 的地方，而且照舊版行為：`.ai/` 底下同時放信箱與討論紀錄，
/// 不該進版控。（沙盒那邊相反：`sandbox.rs` 寫 `.git/info/exclude`，因為那是**我們自己**造的目錄。）
pub fn ensure_gitignore(project_dir: &Path) {
    let path = project_dir.join(".gitignore");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            for raw in text.split('\n') {
                let t = raw.trim().trim_end_matches('\r');
                if matches!(t, ".ai" | ".ai/" | "/.ai" | "/.ai/" | ".ai/*" | "/.ai/*") {
                    return;
                }
            }
            let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
            let mut add = String::new();
            if !text.is_empty() && !text.ends_with('\n') {
                add.push_str(nl);
            }
            add.push_str("# AwayTerminal Multi-Agent mailbox");
            add.push_str(nl);
            add.push_str(".ai/");
            add.push_str(nl);
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&path) {
                let _ = f.write_all(add.as_bytes());
                println!("[AwayTerminal] 代理團隊：.gitignore 加了 .ai/（{}）", path.display());
            }
        }
        Err(_) => {
            let _ = std::fs::write(&path, "# AwayTerminal Multi-Agent mailbox\n.ai/\n");
            println!("[AwayTerminal] 代理團隊：建了 .gitignore 並加入 .ai/（{}）", path.display());
        }
    }
}

/// 共用的 `Arc<MessageBus>`。
pub type SharedBus = Arc<MessageBus>;

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("awayterm-bus-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 寫進去的信要等**穩定 1500ms** 之後才會被撈出來。
    #[test]
    fn waits_for_the_file_to_be_stable() {
        let dir = temp("stable");
        let bus = MessageBus::new(&dir);
        bus.start();
        std::fs::write(
            bus.bus_dir.join("0001-Agent-11-to-Agent-12.md"),
            "---\nfrom: Agent-11\nto: Agent-12\ntype: TASK\n---\nhi",
        )
        .unwrap();
        // 剛寫完 → 還不能讀
        assert!(bus.poll().is_empty(), "剛寫完就讀到了（少了穩定期）");
        // 把修改時間往前推（不要真的睡 1.5 秒）
        let p = bus.bus_dir.join("0001-Agent-11-to-Agent-12.md");
        let old = std::time::SystemTime::now() - std::time::Duration::from_millis(3000);
        filetime_set(&p, old);
        let got = bus.poll();
        assert_eq!(got.len(), 1, "穩定之後應該讀到：{got:?}");
        assert_eq!(got[0].from, "Agent-11");
        // 同一封不會再來一次
        assert!(bus.poll().is_empty(), "同一封信被讀到兩次");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `.delivered` 裡的不再投遞（程式重開也一樣）。
    #[test]
    fn skips_already_delivered() {
        let dir = temp("delivered");
        let bus = MessageBus::new(&dir);
        bus.start();
        let name = "0001-Agent-11-to-Agent-12.md";
        let p = bus.bus_dir.join(name);
        std::fs::write(&p, "---\nfrom: Agent-11\nto: Agent-12\n---\nhi").unwrap();
        filetime_set(&p, std::time::SystemTime::now() - std::time::Duration::from_millis(3000));
        bus.mark_delivered(name);
        assert!(bus.poll().is_empty(), "已投遞的又被撈出來");
        // 重開（新的 bus 物件）也要記得
        let bus2 = MessageBus::new(&dir);
        bus2.start();
        assert!(bus2.is_delivered(name), ".delivered 沒有被讀回來");
        assert!(bus2.poll().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 不是信的檔案不理（`.delivered`、`board.md`、`.txt`）。
    #[test]
    fn ignores_non_messages() {
        let dir = temp("ignore");
        let bus = MessageBus::new(&dir);
        bus.start();
        for n in [".delivered", "board.md", "notes.txt"] {
            let p = bus.bus_dir.join(n);
            std::fs::write(&p, "x").unwrap();
            filetime_set(&p, std::time::SystemTime::now() - std::time::Duration::from_millis(3000));
        }
        assert!(bus.poll().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AwayTerminal 自己寄信：編號接續、front matter 格式對。
    #[test]
    fn writes_its_own_message() {
        let dir = temp("write");
        let bus = MessageBus::new(&dir);
        bus.start();
        std::fs::write(bus.bus_dir.join("0004-Agent-11-to-Agent-12.md"), "x").unwrap();
        let name = bus
            .write_message("AwayTerminal", "Agent-11", "INFO", "TASK-001", "沒送到")
            .unwrap();
        assert_eq!(name, "0005-AwayTerminal-to-Agent-11.md");
        let text = std::fs::read_to_string(bus.bus_dir.join(&name)).unwrap();
        assert!(text.starts_with("---\nfrom: AwayTerminal\nto: Agent-11\ntype: INFO\ntask: TASK-001\n---\n"));
        assert!(text.trim_end().ends_with("沒送到"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `.gitignore`：沒有就建、有就追加、已經有 `.ai/` 就不動。
    #[test]
    fn manages_gitignore() {
        let dir = temp("gitignore");
        ensure_gitignore(&dir);
        let text = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(text.contains(".ai/"));

        // 已經有了 → 不重複加
        ensure_gitignore(&dir);
        let again = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(text, again, "重複加了一次");

        // 既有內容（沒有結尾換行）→ 追加
        let dir2 = temp("gitignore2");
        std::fs::write(dir2.join(".gitignore"), "target/").unwrap();
        ensure_gitignore(&dir2);
        let t2 = std::fs::read_to_string(dir2.join(".gitignore")).unwrap();
        assert!(t2.starts_with("target/"), "原本的內容被動到了：{t2:?}");
        assert!(t2.contains(".ai/"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    /// 測試用：把檔案的修改時間往前推（避免測試真的睡 1.5 秒）。
    fn filetime_set(path: &Path, when: std::time::SystemTime) {
        // 不引入 filetime crate：用 `File::set_times`（Rust 1.75+）
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        let times = std::fs::FileTimes::new().set_modified(when);
        f.set_times(times).unwrap();
    }
}
