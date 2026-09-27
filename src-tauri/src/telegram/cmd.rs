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

/// `/help` 的內容。**逐字照舊版**，只拿掉這一版還沒做的那幾條（`/new`／`/ssh`／`/telnet`／
/// `/history`，見 `docs/TELEGRAM.md` 的「還沒做」）。
pub fn help_text() -> String {
    crate::i18n::t("tg.help")
}

/// 註冊給 Telegram 的原生指令選單（`setMyCommands`）。
pub fn command_menu() -> Vec<(&'static str, String)> {
    vec![
        ("goto", crate::i18n::t("tg.cmdGoto")),
        ("last", crate::i18n::t("tg.cmdLast")),
        ("more", crate::i18n::t("tg.cmdMore")),
        ("shot", crate::i18n::t("tg.cmdShot")),
        ("key", crate::i18n::t("tg.cmdKey")),
        ("stop", crate::i18n::t("tg.cmdStop")),
        ("where", crate::i18n::t("tg.cmdWhere")),
        ("close", crate::i18n::t("tg.cmdClose")),
        ("follow", crate::i18n::t("tg.cmdFollow")),
        ("notify", crate::i18n::t("tg.cmdNotify")),
        ("plain", crate::i18n::t("tg.cmdPlain")),
        ("exit", crate::i18n::t("tg.cmdExit")),
        ("help", crate::i18n::t("tg.cmdHelp")),
    ]
}

/// `/plain` 開啟時附加在提問後面的提示（舊版 `PlainSuffix`）。
///
/// 只加在「較長的提問」上（≥8 字、非純數字）——短答與選單數字不加，免得干擾。
pub fn plain_suffix() -> String {
    crate::i18n::t("tg.plainSuffix")
}

/// 這一句要不要加 `/plain` 的提示。
pub fn wants_plain_suffix(text: &str) -> bool {
    let t = text.trim();
    t.chars().count() >= 8 && t.parse::<i64>().is_err()
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

    /// `/plain` 的提示只加在較長的提問上。
    #[test]
    fn plain_suffix_only_for_real_questions() {
        assert!(wants_plain_suffix("這個專案的架構是什麼？"));
        assert!(!wants_plain_suffix("好"), "短答不加");
        assert!(!wants_plain_suffix("2"), "選單數字不加");
        assert!(!wants_plain_suffix("12345678"), "純數字不加");
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
