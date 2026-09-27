//! 信箱裡的一封信（搬移舊版 `Models/AgentMessage.cs`）。
//!
//! 檔案在專案的 `.ai/bus/NNNN-<寄件人>-to-<收件人>.md`：開頭一段 YAML front matter
//!（`from`／`to`／`type`／`task`／`status`／`files_changed`…），之後是 Markdown 內文。
//!
//! **解析刻意寬鬆**（agent 不一定照格式寫，照舊版）：
//!
//! | 情況 | 舊版怎麼做 | 這裡 |
//! |---|---|---|
//! | 缺 `from`／`to` | 用檔名補 | 同 |
//! | 缺 `type` | 當 `INFO` | 同 |
//! | front matter 壞掉（沒有結尾 `---`） | 標 `header_warning`，**仍照檔名投遞** | 同 |
//! | `agent-12`／`Agent12` | 正規化成 `Agent-12` | 同（[`normalize_id`]） |
//! | 值尾端的 ` #註解`、成對引號 | 去掉 | 同 |
//! | YAML 函式庫 | **不引入**（只認 `key: value` 與緊接的 `- item`） | 同 |
//!
//! 不引入 YAML crate 的理由照舊版：agent 寫出來的 front matter 常常不是合法 YAML
//!（缺引號、中文冒號、tab 混用），嚴格解析會整封讀不到；寬鬆解析＋檔名補值才收得到信。

use std::collections::HashMap;
use std::path::Path;

/// 信箱資料夾相對專案根目錄的路徑（投遞那一行裡用 `/` 分隔，照舊版）。
pub const BUS_REL_DIR: &str = ".ai/bus";

/// 一封信。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    pub file_name: String,
    /// 檔名開頭的編號（排序用；解析不到＝`u32::MAX`）。
    pub seq: u32,
    pub from: String,
    pub to: String,
    /// 大寫（`TASK`／`TASK_RESULT`／`INFO`…）。
    pub kind: String,
    pub task: String,
    pub status: String,
    pub files_changed: Vec<String>,
    pub fields: HashMap<String, String>,
    pub body: String,
    /// front matter 有問題（缺結尾 `---`、缺 from／to）——**仍然要投遞**，只是另記一筆。
    pub header_warning: bool,
}

impl AgentMessage {
    /// 收件人是 `all`（投給組內除寄件人以外的每個已啟動 agent）。
    pub fn is_broadcast(&self) -> bool {
        self.to.eq_ignore_ascii_case("all")
    }

    /// 投遞那一行裡用的相對路徑。
    pub fn rel_path(&self) -> String {
        format!("{BUS_REL_DIR}/{}", self.file_name)
    }
}

/// 「agent-12」「Agent12」→「Agent-12」；`all` 轉小寫；其他名字（`AwayTerminal`、`user`）原樣。
pub fn normalize_id(s: &str) -> String {
    let t = s.trim().trim_matches(['"', '\'']).trim();
    if t.eq_ignore_ascii_case("all") {
        return "all".to_string();
    }
    // `agent` ＋ 可省的 `-`／`_`／空白 ＋ 剛好兩位數字
    let lower = t.to_ascii_lowercase();
    let rest = lower.strip_prefix("agent").map(|r| r.trim_start_matches(['-', '_', ' ']));
    match rest {
        Some(d) if d.len() == 2 && d.chars().all(|c| c.is_ascii_digit()) => format!("Agent-{d}"),
        _ => t.to_string(),
    }
}

/// 拆檔名 `NNNN-<from>-to-<to>.md`（大小寫不分；不合格式回 `None`）。
pub fn parse_file_name(name: &str) -> Option<(u32, String, String)> {
    let stem = name.strip_suffix(".md").or_else(|| name.strip_suffix(".MD"))?;
    let (num, rest) = stem.split_once('-')?;
    if num.is_empty() || num.len() > 9 || !num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // 「-to-」可能出現在名字裡（`Agent-11-to-Agent-12`）→ 取**最後一個**
    let lower = rest.to_ascii_lowercase();
    let at = lower.rfind("-to-")?;
    let from = &rest[..at];
    let to = &rest[at + 4..];
    if from.is_empty() || to.is_empty() {
        return None;
    }
    Some((num.parse().unwrap_or(u32::MAX), from.to_string(), to.to_string()))
}

/// 這個檔名算一封信嗎（照舊版 `IsMessageName`：`.md`、不以 `.` 開頭、不是 `board.md`）。
pub fn is_message_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") && !name.starts_with('.') && lower != "board.md"
}

/// 去掉值尾端的 ` #註解` 與成對引號（照舊版 `StripComment`）。
fn strip_comment(v: &str) -> String {
    let mut s = match v.find(" #") {
        Some(h) => &v[..h],
        None => v,
    }
    .trim()
    .to_string();
    let b = s.as_bytes();
    if s.len() >= 2 && ((b[0] == b'"' && b[s.len() - 1] == b'"') || (b[0] == b'\'' && b[s.len() - 1] == b'\'')) {
        s = s[1..s.len() - 1].to_string();
    }
    s.trim().to_string()
}

/// 解析內容（front matter ＋ 內文）。`parse_text` 拆出來是為了測試能直接餵字串（同舊版）。
pub fn parse_text(msg: &mut AgentMessage, text: &str) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut i = 0;
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1; // 開頭空行容忍
    }
    if i >= lines.len() || lines[i].trim() != "---" {
        msg.body = text.to_string();
        msg.header_warning = true;
        return;
    }
    let mut end = None;
    for (j, l) in lines.iter().enumerate().skip(i + 1) {
        if l.trim() == "---" {
            end = Some(j);
            break;
        }
    }
    let end = match end {
        Some(e) => e,
        None => {
            msg.header_warning = true;
            lines.len()
        }
    };

    let mut list_key: Option<String> = None;
    for l in lines.iter().take(end).skip(i + 1) {
        let t = l.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t == "-" || t.starts_with("- ") {
            if list_key.as_deref().is_some_and(|k| k.eq_ignore_ascii_case("files_changed")) {
                let item = strip_comment(t.strip_prefix("- ").unwrap_or(""));
                if !item.is_empty() {
                    msg.files_changed.push(item);
                }
            }
            continue;
        }
        let Some(c) = t.find(':') else { continue };
        if c == 0 {
            continue;
        }
        let key = t[..c].trim().to_string();
        let val = strip_comment(&t[c + 1..]);
        list_key = if val.is_empty() { Some(key.clone()) } else { None };
        if val.is_empty() {
            continue;
        }
        msg.fields.insert(key.clone(), val.clone());
        match key.to_ascii_lowercase().as_str() {
            "from" => msg.from = val,
            "to" => msg.to = val,
            "type" => msg.kind = val,
            "task" | "task_id" => msg.task = val,
            "status" => msg.status = val,
            _ => {}
        }
    }
    msg.body = if end + 1 < lines.len() {
        lines[end + 1..].join("\n").trim().to_string()
    } else {
        String::new()
    };
}

/// 讀檔並解析；讀不到（被鎖、已刪）回 `None`（照舊版：下一輪再試）。
pub fn parse_file(path: &Path) -> Option<AgentMessage> {
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
    let name = path.file_name()?.to_string_lossy().to_string();
    let parsed_name = parse_file_name(&name);
    let mut msg = AgentMessage {
        file_name: name,
        seq: parsed_name.as_ref().map(|(n, _, _)| *n).unwrap_or(u32::MAX),
        ..Default::default()
    };
    parse_text(&mut msg, &text);
    if let Some((_, from, to)) = parsed_name {
        if msg.from.trim().is_empty() {
            msg.from = from;
        }
        if msg.to.trim().is_empty() {
            msg.to = to;
        }
    }
    msg.from = normalize_id(&msg.from);
    msg.to = normalize_id(&msg.to);
    if msg.kind.trim().is_empty() {
        msg.kind = "INFO".to_string();
    }
    msg.kind = msg.kind.trim().to_ascii_uppercase();
    Some(msg)
}

/// 下一個檔名：資料夾裡最大的 `NNNN` ＋1（補到 4 位數），例 `0008-AwayTerminal-to-Agent-12.md`。
pub fn next_file_name(bus_dir: &Path, from: &str, to: &str) -> String {
    let mut max = 0u32;
    if let Ok(rd) = std::fs::read_dir(bus_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some((n, _, _)) = parse_file_name(&name) {
                if n != u32::MAX && n > max {
                    max = n;
                }
            }
        }
    }
    format!("{:04}-{from}-to-{to}.md", max + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> AgentMessage {
        let mut m = AgentMessage::default();
        parse_text(&mut m, text);
        m
    }

    /// 正常的一封信。
    #[test]
    fn parses_normal_message() {
        let m = parse(
            "---\nfrom: Agent-11\nto: Agent-12\ntype: TASK\ntask: TASK-001\n---\n# 標題\n\n內文",
        );
        assert_eq!(m.from, "Agent-11");
        assert_eq!(m.to, "Agent-12");
        assert_eq!(m.kind, "TASK");
        assert_eq!(m.task, "TASK-001");
        assert_eq!(m.body, "# 標題\n\n內文");
        assert!(!m.header_warning);
    }

    /// `files_changed` 的清單（緊接在 key 後面的 `- item`）。
    #[test]
    fn parses_files_changed_list() {
        let m = parse(
            "---\nfrom: Agent-12\nto: Agent-11\ntype: TASK_RESULT\nstatus: completed\n\
             files_changed:\n  - src/a.rs\n  - src/b.rs   # 順手改的\n---\nok",
        );
        assert_eq!(m.status, "completed");
        assert_eq!(m.files_changed, vec!["src/a.rs", "src/b.rs"]);
    }

    /// 寬鬆：沒有 front matter、缺結尾 `---`、缺 from／to。
    #[test]
    fn is_lenient_about_broken_front_matter() {
        let m = parse("沒有 front matter 的信");
        assert!(m.header_warning);
        assert_eq!(m.body, "沒有 front matter 的信");
        assert_eq!(m.kind, "", "parse_text 不填預設值（parse_file 才填 INFO）");

        let m2 = parse("---\nfrom: Agent-11\nto: Agent-12\n內文沒有結尾的三個減號");
        assert!(m2.header_warning);
        assert_eq!(m2.from, "Agent-11");
    }

    /// ID 正規化。
    #[test]
    fn normalizes_ids() {
        for s in ["agent-12", "Agent12", "AGENT_12", "agent 12", " \"Agent-12\" "] {
            assert_eq!(normalize_id(s), "Agent-12", "{s}");
        }
        assert_eq!(normalize_id("ALL"), "all");
        assert_eq!(normalize_id("AwayTerminal"), "AwayTerminal");
        assert_eq!(normalize_id("user"), "user");
        // 三位數字不是 agent id（組號只有 1～9、格號 1～4）
        assert_eq!(normalize_id("agent-123"), "agent-123");
    }

    /// 檔名解析：`-to-` 出現在名字裡也要對（`Agent-11-to-Agent-12`）。
    #[test]
    fn parses_file_names() {
        let (n, from, to) = parse_file_name("0007-Agent-11-to-Agent-12.md").unwrap();
        assert_eq!((n, from.as_str(), to.as_str()), (7, "Agent-11", "Agent-12"));
        let (n2, from2, to2) = parse_file_name("0012-AwayTerminal-to-all.md").unwrap();
        assert_eq!((n2, from2.as_str(), to2.as_str()), (12, "AwayTerminal", "all"));
        assert!(parse_file_name("board.md").is_none());
        assert!(parse_file_name("0007-Agent-11.md").is_none(), "沒有 -to-");
        assert!(parse_file_name("abcd-a-to-b.md").is_none(), "編號不是數字");
    }

    /// 哪些檔名算信。
    #[test]
    fn filters_message_names() {
        assert!(is_message_name("0001-a-to-b.md"));
        assert!(!is_message_name(".delivered"));
        assert!(!is_message_name("board.md"));
        assert!(!is_message_name("notes.txt"));
    }

    /// 下一個編號：最大值＋1，補 4 位；空資料夾＝0001。
    #[test]
    fn computes_next_file_name() {
        let dir = std::env::temp_dir().join("awayterm-bus-name-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(next_file_name(&dir, "A", "B"), "0001-A-to-B.md");
        std::fs::write(dir.join("0007-Agent-11-to-Agent-12.md"), "x").unwrap();
        std::fs::write(dir.join("0012-Agent-12-to-Agent-11.md"), "x").unwrap();
        std::fs::write(dir.join("board.md"), "x").unwrap();
        assert_eq!(next_file_name(&dir, "AwayTerminal", "Agent-12"), "0013-AwayTerminal-to-Agent-12.md");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 讀檔：缺 from／to 用檔名補、type 預設 INFO、BOM 容忍。
    #[test]
    fn parse_file_fills_from_name() {
        let dir = std::env::temp_dir().join("awayterm-bus-parse-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("0003-agent-11-to-agent-12.md");
        std::fs::write(&p, "\u{feff}---\ntask: TASK-009\n---\n內文").unwrap();
        let m = parse_file(&p).unwrap();
        assert_eq!(m.from, "Agent-11", "from 用檔名補並正規化");
        assert_eq!(m.to, "Agent-12");
        assert_eq!(m.kind, "INFO", "缺 type 當 INFO");
        assert_eq!(m.task, "TASK-009");
        assert_eq!(m.seq, 3);
        assert_eq!(m.rel_path(), ".ai/bus/0003-agent-11-to-agent-12.md");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `to: all` 是廣播。
    #[test]
    fn detects_broadcast() {
        let mut m = AgentMessage::default();
        parse_text(&mut m, "---\nfrom: Agent-11\nto: all\ntype: INFO\n---\nx");
        m.to = normalize_id(&m.to);
        assert!(m.is_broadcast());
    }
}
