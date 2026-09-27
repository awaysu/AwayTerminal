//! 指令解析與回覆文字（搬移舊版 `TelegramRemote.HandleCommand` 的判斷部分）。
//!
//! 這裡**只做純邏輯**：一行文字進來 → 決定「要做什麼」。真正去動分頁、去打 HTTP 的部分在
//! [`super::remote`]，這樣每一條規則都能單獨測（不需要真的 Telegram、也不需要真的分頁）。

/// 一行來訊要做的事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// `/help`、`/start`
    Help,
    /// `/list`、`/status`
    TabList,
    /// `/where`
    Where,
    /// `/goto [n]`（`None`＝沒帶編號，列分頁清單）
    Goto(Option<usize>),
    /// `/exit`
    Exit,
    /// `/last [n]`（預設 20）
    Last(usize),
    /// `/more`
    More,
    /// `/shot`
    Shot,
    /// `/close [n]`（`None`＝關目前附著的）
    Close(Option<u32>),
    /// `/key <名稱>`
    Key(String),
    /// `/stop`＝`/key ctrl-c`
    Stop,
    /// `/notify on|off`
    Notify(bool),
    /// `/follow on|off`
    Follow(bool),
    /// `/plain on|off`
    Plain(bool),
    /// 純文字 → 打進目前附著的分頁
    Send(String),
    /// 純數字、而且畫面上是選單 → 換算成 ↑／↓＋Enter（呼叫端再確認是不是選單）
    MenuAnswer(u32),
    /// `/new [n]`（`None`＝列出可開的連線）
    New(Option<usize>),
    /// `/ssh [user@]主機[:埠]`（空字串＝用我的最愛裡第一條 SSH）
    Ssh(String),
    /// `/telnet [主機[:埠]]`（空字串＝用我的最愛裡第一條 Telnet）
    Telnet(String),
    /// `/history [n]`（`None`＝列出清單）
    History(Option<usize>),
    /// 認不出來的 `/指令`
    Unknown,
}

/// 開關參數：`on`／`1` ＝開；其他＝關。`/follow` 的預設相反（見舊版：`!off && != "0"`）。
fn on_off(arg: &str, default_on: bool) -> bool {
    let a = arg.trim().to_ascii_lowercase();
    if a.is_empty() {
        return default_on;
    }
    if default_on {
        a != "off" && a != "0"
    } else {
        a == "on" || a == "1"
    }
}

/// 解析一行來訊。
///
/// `attached`＝目前有沒有附著分頁（沒有的話純文字要回「尚未選擇分頁」，由呼叫端處理）。
pub fn parse(text: &str) -> Action {
    let text = text.trim();
    if text.is_empty() {
        return Action::Unknown;
    }
    if !text.starts_with('/') {
        // 1～9 的純數字：可能是選單應答（呼叫端看畫面決定；不是選單就當普通文字送出）
        if let Ok(n) = text.parse::<u32>() {
            if (1..=9).contains(&n) {
                return Action::MenuAnswer(n);
            }
        }
        return Action::Send(text.to_string());
    }
    let mut it = text.splitn(2, ' ');
    let cmd = it
        .next()
        .unwrap_or("")
        .trim_start_matches('/')
        .to_ascii_lowercase();
    let arg = it.next().unwrap_or("").trim().to_string();
    match cmd.as_str() {
        "help" | "start" => Action::Help,
        "list" | "status" => Action::TabList,
        "where" => Action::Where,
        "goto" => Action::Goto(arg.parse::<usize>().ok().filter(|n| *n >= 1)),
        "exit" => Action::Exit,
        "last" => Action::Last(arg.parse::<usize>().ok().filter(|n| *n > 0).unwrap_or(20)),
        "more" => Action::More,
        "shot" => Action::Shot,
        "close" => Action::Close(arg.parse::<u32>().ok()),
        "key" => Action::Key(arg.to_ascii_lowercase()),
        "stop" => Action::Stop,
        // `/notify` 預設關、`/follow` 預設開（照舊版）
        "notify" => Action::Notify(on_off(&arg, false)),
        "follow" => Action::Follow(on_off(&arg, true)),
        "plain" => Action::Plain(on_off(&arg, false)),
        "new" => Action::New(arg.parse::<usize>().ok().filter(|n| *n >= 1)),
        "ssh" => Action::Ssh(arg.clone()),
        "telnet" => Action::Telnet(arg.clone()),
        "history" => Action::History(arg.parse::<usize>().ok().filter(|n| *n >= 1)),
        _ => Action::Unknown,
    }
}

/// 可以送的控制鍵 → 要寫進 PTY 的位元組（照舊版 `SendKeyToTab` 認的那幾個）。
pub fn key_bytes(name: &str) -> Option<&'static [u8]> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "ctrl-c" => b"\x03",
        "ctrl-d" => b"\x04",
        "esc" => b"\x1b",
        "tab" => b"\t",
        "enter" => b"\r",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        _ => return None,
    })
}

/// `/help` 的內容。**逐字照舊版**（19 條全在）。
pub fn help_text() -> String {
    crate::i18n::t("tg.help")
}

/// 註冊給 Telegram 的原生指令選單（`setMyCommands`）。
pub fn command_menu() -> Vec<(&'static str, String)> {
    // 順序照舊版 `RegisterCommandsAsync`（goto、new、history、ssh、telnet 在前）
    vec![
        ("goto", crate::i18n::t("tg.cmdGoto")),
        ("new", crate::i18n::t("tg.cmdNew")),
        ("history", crate::i18n::t("tg.cmdHistory")),
        ("ssh", crate::i18n::t("tg.cmdSsh")),
        ("telnet", crate::i18n::t("tg.cmdTelnet")),
        ("last", crate::i18n::t("tg.cmdLast")),
        ("more", crate::i18n::t("tg.cmdMore")),
        ("close", crate::i18n::t("tg.cmdClose")),
        ("key", crate::i18n::t("tg.cmdKey")),
        ("stop", crate::i18n::t("tg.cmdStop")),
        ("shot", crate::i18n::t("tg.cmdShot")),
        ("where", crate::i18n::t("tg.cmdWhere")),
        ("follow", crate::i18n::t("tg.cmdFollow")),
        ("notify", crate::i18n::t("tg.cmdNotify")),
        ("plain", crate::i18n::t("tg.cmdPlain")),
        ("exit", crate::i18n::t("tg.cmdExit")),
        ("help", crate::i18n::t("tg.cmdHelp")),
    ]
}

/// 解析「[user@]主機[:埠]」。照舊版 `ParseHostPort`：沒寫 `:埠` 就不動埠，**不支援 IPv6**。
///
/// 回 `(主機, 埠)`；`埠 = 0` ＝沒指定，由呼叫端填預設（SSH 22／Telnet 23）。
pub fn parse_host_port(arg: &str) -> (String, u16) {
    let host = arg.trim();
    if let Some(i) = host.rfind(':') {
        if i > 0 {
            if let Ok(p) = host[i + 1..].parse::<u16>() {
                if p > 0 {
                    return (host[..i].to_string(), p);
                }
            }
        }
    }
    (host.to_string(), 0)
}

/// `/plain` 開啟時附加在提問後面的提示（舊版 `PlainSuffix`）。
///
/// 只加在「較長的提問」上（≥8 字、非純數字）——短答與選單數字不加，免得干擾。
pub fn plain_suffix() -> String {
    crate::i18n::t("tg.plainSuffix")
}

/// 這一句要不要加 `/plain` 的提示。
///
/// 舊版只看長度（≥8 字、非純數字）。這一版**多一個條件 `talks_to_ai`：分頁要是會跟 AI
/// 對話的那種**（`TabKind::Claude`／`Custom`——自訂連線與代理團隊的格子都是這兩種）。
///
/// 為什麼：`/plain` 是「請 AI 不要用表格」，加在 shell／SSH／Telnet／COM 分頁上只會
/// 把那一行弄壞。最糟的情況是**從手機打 SSH 密碼**——8 個字以上的密碼會被接上一整句
/// 中文提示，登入失敗而且看不出原因（密碼不回顯，畫面上只會出現 `Access denied`）。
pub fn wants_plain_suffix(text: &str, talks_to_ai: bool) -> bool {
    let t = text.trim();
    talks_to_ai && t.chars().count() >= 8 && t.parse::<i64>().is_err()
}

/// 從畫面文字找出選單選項（`❯ N. 文字` / `  N. 文字`）。
///
/// `signature_source`＝**瘦身前**的文字：必須有導航列（`to navigate` / `Enter to select`）
/// 才算選單。瘦身會把導航列濾掉，所以偵測要看原始畫面（舊版註解）。
pub fn menu_options(tidied: &str, signature_source: &str) -> Vec<(u32, String)> {
    if !(signature_source.contains("to navigate") || signature_source.contains("Enter to select")) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in tidied.lines() {
        let t = line.trim_start_matches(['❯', ' ']).trim();
        let Some(dot) = t.find(['.', '．']) else { continue };
        let (num, rest) = t.split_at(dot);
        if let Ok(n) = num.trim().parse::<u32>() {
            if (1..=9).contains(&n) {
                let label = rest
                    .trim_start_matches(['.', '．'])
                    .trim()
                    .chars()
                    .take(14)
                    .collect::<String>();
                if !label.is_empty() && !out.iter().any(|(m, _)| *m == n) {
                    out.push((n, label));
                }
            }
        }
    }
    out
}

/// 畫面上目前選到第幾項（最後一個 `❯ N.`；舊版取最後一個＝最下方的選單）。
pub fn current_menu_index(screen: &str) -> Option<u32> {
    let mut last = None;
    for line in screen.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('❯') {
            let rest = rest.trim();
            if let Some(dot) = rest.find(['.', '．']) {
                if let Ok(n) = rest[..dot].trim().parse::<u32>() {
                    last = Some(n);
                }
            }
        }
    }
    last
}

/// 畫面看起來是選單嗎（有 `❯ N.` ＋導航列）。
pub fn is_menu_screen(screen: &str) -> bool {
    current_menu_index(screen).is_some()
        && (screen.contains("to navigate") || screen.contains("Enter to select"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_command() {
        assert_eq!(parse("/help"), Action::Help);
        assert_eq!(parse("/start"), Action::Help);
        assert_eq!(parse("/list"), Action::TabList);
        assert_eq!(parse("/status"), Action::TabList);
        assert_eq!(parse("/where"), Action::Where);
        assert_eq!(parse("/exit"), Action::Exit);
        assert_eq!(parse("/more"), Action::More);
        assert_eq!(parse("/shot"), Action::Shot);
        assert_eq!(parse("/stop"), Action::Stop);
        assert_eq!(parse("/nope"), Action::Unknown);
    }

    /// 大小寫與前後空白不影響（手機鍵盤常自動大寫）。
    #[test]
    fn commands_are_case_insensitive() {
        assert_eq!(parse("/HELP"), Action::Help);
        assert_eq!(parse("  /Goto 2  "), Action::Goto(Some(2)));
    }

    #[test]
    fn parses_arguments() {
        assert_eq!(parse("/goto 3"), Action::Goto(Some(3)));
        assert_eq!(parse("/goto"), Action::Goto(None), "不帶編號＝列清單");
        assert_eq!(parse("/goto 0"), Action::Goto(None), "0 不是有效編號");
        assert_eq!(parse("/goto abc"), Action::Goto(None));
        assert_eq!(parse("/last"), Action::Last(20), "預設 20 行");
        assert_eq!(parse("/last 5"), Action::Last(5));
        assert_eq!(parse("/last 0"), Action::Last(20), "0 → 預設");
        assert_eq!(parse("/close"), Action::Close(None));
        assert_eq!(parse("/close 7"), Action::Close(Some(7)));
        assert_eq!(parse("/key ctrl-c"), Action::Key("ctrl-c".into()));
        assert_eq!(parse("/key UP"), Action::Key("up".into()));
    }

    /// 開關的預設方向照舊版：`/notify` 預設關、`/follow` 預設開。
    #[test]
    fn switch_defaults_follow_v1() {
        assert_eq!(parse("/notify"), Action::Notify(false));
        assert_eq!(parse("/notify on"), Action::Notify(true));
        assert_eq!(parse("/notify 1"), Action::Notify(true));
        assert_eq!(parse("/notify off"), Action::Notify(false));
        assert_eq!(parse("/follow"), Action::Follow(true));
        assert_eq!(parse("/follow off"), Action::Follow(false));
        assert_eq!(parse("/follow 0"), Action::Follow(false));
        assert_eq!(parse("/follow on"), Action::Follow(true));
        assert_eq!(parse("/plain"), Action::Plain(false));
        assert_eq!(parse("/plain on"), Action::Plain(true));
    }

    /// 純文字送進分頁；1～9 的純數字可能是選單應答。
    #[test]
    fn plain_text_goes_to_the_tab() {
        assert_eq!(parse("hello"), Action::Send("hello".into()));
        assert_eq!(parse("2 + 2"), Action::Send("2 + 2".into()));
        assert_eq!(parse("1"), Action::MenuAnswer(1));
        assert_eq!(parse("9"), Action::MenuAnswer(9));
        assert_eq!(parse("10"), Action::Send("10".into()), "10 不在 1～9");
        assert_eq!(parse("0"), Action::Send("0".into()));
    }

    #[test]
    fn maps_control_keys() {
        assert_eq!(key_bytes("ctrl-c"), Some(&b"\x03"[..]));
        assert_eq!(key_bytes("CTRL-D"), Some(&b"\x04"[..]));
        assert_eq!(key_bytes("esc"), Some(&b"\x1b"[..]));
        assert_eq!(key_bytes("up"), Some(&b"\x1b[A"[..]));
        assert_eq!(key_bytes("down"), Some(&b"\x1b[B"[..]));
        assert_eq!(key_bytes("right"), Some(&b"\x1b[C"[..]));
        assert_eq!(key_bytes("left"), Some(&b"\x1b[D"[..]));
        assert_eq!(key_bytes("enter"), Some(&b"\r"[..]));
        assert_eq!(key_bytes("tab"), Some(&b"\t"[..]));
        assert_eq!(key_bytes("f13"), None);
    }

    /// `/plain` 的提示只加在會跟 AI 對話的分頁上、而且是較長的提問。
    #[test]
    fn plain_suffix_only_for_real_questions() {
        assert!(wants_plain_suffix("這個專案的架構是什麼？", true));
        assert!(!wants_plain_suffix("好", true), "短答不加");
        assert!(!wants_plain_suffix("2", true), "選單數字不加");
        assert!(!wants_plain_suffix("12345678", true), "純數字不加");
    }

    /// **不是 AI 分頁就一律不加**——最要緊的是從手機打進 SSH 的密碼：
    /// 8 個字以上的密碼被接上一句中文提示會登入失敗，而且密碼不回顯、看不出原因。
    #[test]
    fn plain_suffix_never_touches_a_password() {
        assert!(!wants_plain_suffix("hunter2hunter2", false));
        assert!(!wants_plain_suffix("這個專案的架構是什麼？", false));
        assert!(!wants_plain_suffix("ls -la /var/log", false), "shell 指令也不加");
    }

    /// `/new`／`/ssh`／`/telnet`／`/history` 的解析。
    #[test]
    fn parses_the_open_connection_commands() {
        assert_eq!(parse("/new"), Action::New(None));
        assert_eq!(parse("/new 3"), Action::New(Some(3)));
        assert_eq!(parse("/new 0"), Action::New(None), "0 當成沒帶編號");
        assert_eq!(parse("/history"), Action::History(None));
        assert_eq!(parse("/history 2"), Action::History(Some(2)));
        assert_eq!(parse("/ssh"), Action::Ssh(String::new()));
        assert_eq!(parse("/ssh me@10.0.0.1:2222"), Action::Ssh("me@10.0.0.1:2222".to_string()));
        assert_eq!(parse("/telnet"), Action::Telnet(String::new()));
        assert_eq!(parse("/telnet 10.0.0.9"), Action::Telnet("10.0.0.9".to_string()));
    }

    /// `[user@]主機[:埠]`，照舊版 `ParseHostPort`：沒寫埠就回 0（呼叫端填預設）。
    #[test]
    fn parses_host_and_port() {
        assert_eq!(parse_host_port("10.0.0.1"), ("10.0.0.1".to_string(), 0));
        assert_eq!(parse_host_port("10.0.0.1:2222"), ("10.0.0.1".to_string(), 2222));
        assert_eq!(parse_host_port("me@host:23"), ("me@host".to_string(), 23));
        assert_eq!(parse_host_port(" host "), ("host".to_string(), 0));
        // 埠不是數字＝整段都當主機名（同舊版的 TryParse 失敗路徑）
        assert_eq!(parse_host_port("host:ssh"), ("host:ssh".to_string(), 0));
        // **IPv6 真的會被切壞**，而且舊版一模一樣（`LastIndexOf(':')` ＋ `TryParse`）：
        // `::1` → 主機 `:`、埠 1。舊版註解就寫明「IPv6 不支援」，這裡把這個限制釘住，
        // 之後誰要支援 IPv6 就會看到這條測試失敗、知道連舊版一起改。
        assert_eq!(parse_host_port("::1"), (":".to_string(), 1));
    }

    /// `/help` 的清單要含舊版全部 19 條（TASK-020 漏了四條，TASK-021 補回）。
    #[test]
    fn help_lists_every_command() {
        let h = help_text();
        for c in [
            "/goto", "/new", "/ssh", "/telnet", "/history", "/shot", "/key", "/stop", "/last",
            "/more", "/close", "/where", "/follow", "/notify", "/plain", "/exit",
        ] {
            assert!(h.contains(c), "/help 少了 {c}");
        }
    }

    /// 指令選單的順序照舊版 `RegisterCommandsAsync`，而且每一條都有說明。
    #[test]
    fn command_menu_matches_v1_order() {
        let m = command_menu();
        let names: Vec<&str> = m.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec![
                "goto", "new", "history", "ssh", "telnet", "last", "more", "close", "key", "stop",
                "shot", "where", "follow", "notify", "plain", "exit", "help"
            ]
        );
        assert!(m.iter().all(|(_, d)| !d.is_empty()), "有指令沒有說明");
    }

    /// 選單偵測：**要有導航列**才算選單（瘦身會把它濾掉，所以看原始畫面）。
    #[test]
    fn menu_needs_the_navigation_line() {
        let tidied = "Do you want to proceed?\n❯ 1. Yes\n  2. No";
        let raw = "Do you want to proceed?\n❯ 1. Yes\n  2. No\nEnter to select · ↑/↓ to navigate";
        let opts = menu_options(tidied, raw);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0], (1, "Yes".to_string()));
        assert_eq!(opts[1], (2, "No".to_string()));
        // 沒有導航列 → 不是選單（一般輸出裡的「1. 」不要變成按鈕）
        assert!(menu_options(tidied, tidied).is_empty());
    }

    /// 選項標籤太長要截短（Telegram 按鈕放不下）。
    #[test]
    fn truncates_long_option_labels() {
        let tidied = "❯ 1. 這是一個非常非常長的選項名稱會被截斷\n  2. 短的";
        let opts = menu_options(tidied, "to navigate");
        assert_eq!(opts[0].1.chars().count(), 14);
    }

    /// 目前選到第幾項＝最後一個 `❯ N.`（畫面最底的那個選單）。
    #[test]
    fn finds_the_current_menu_index() {
        let screen = "❯ 1. old\n\n❯ 3. Yes\n  4. No\nEnter to select";
        assert_eq!(current_menu_index(screen), Some(3));
        assert!(is_menu_screen(screen));
        assert_eq!(current_menu_index("no menu here"), None);
        assert!(!is_menu_screen("❯ 2. Yes"), "沒有導航列就不是選單");
    }
}
