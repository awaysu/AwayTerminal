//! 手機閱讀瘦身（搬移舊版 `TelegramRemote.TidyForPhone` 與它的規則表）。
//!
//! 終端機畫面直接丟到手機上是不能讀的：claude／codex 這類 TUI 每秒重繪，畫面上有 spinner、
//! 狀態列、方框、逐格重繪留下的截斷殘影。舊版累積了一整張規則表把這些濾掉，**這裡逐條照抄**
//! （每一條後面的註解就是舊版寫的原因，大多是使用者回報之後加的）。
//!
//! ## 三層規則
//! 1. [`NOISE_LINES`]：一律濾掉的整行（分隔線、狀態列、spinner、空提示符…）
//! 2. [`TUI_FRAGMENTS`]：**只在畫面看起來是 TUI 時**才套用的「碎片」規則。
//!    套在一般 shell 會把合法輸出整行吃掉——`git branch` 的 `* main`、`whoami` 的 `awaysu`、
//!    `echo $?` 的 `0`、`ls` 的單字母檔名。自動推播沒剩內容就整則不送，手機端看起來就是
//!    「遠端沒反應」（舊版註解）。
//! 3. 前綴去重：TUI 逐格重繪會留下同一行不同寬度的截斷殘影
//!    （`● PowerSh` / `● PowerShell(Sta` / 完整行）→ 某行是另一較長行的前綴就刪掉。
//!    **同樣只對 TUI 畫面做**：一般 shell 的 `README` 與 `README.md` 是真的兩行。
//!
//! ## 表格攤平
//! claude 的回答常是 box-drawing 表格，手機字型下 CJK 寬度對不齊＝整團亂，整段刪又會把
//! 「答案就是表格」的內容清光。折衷＝**只有有邊框的真資料表**攤平成 `a | b | c`；
//! 歡迎框／changelog 那種「無邊框、中間一條 │ 分兩欄」的裝飾版面整行丟棄。
//!
//! ⚠️ 舊版 `CLAUDE.md` 的那條雷：**遠端 `/last` 絕不能用原始位元組流去 ANSI**——要拿
//! 「xterm 畫面上看得到的文字」（`a{id}US all` 那條路），位元組流裡的重繪序列會變成一團垃圾。
//! 所以這個模組的輸入一律是**前端給的畫面文字**，見 `docs/TELEGRAM.md`。

use fancy_regex::Regex;
use std::sync::OnceLock;

/// 一律濾掉的整行（照舊版 `NoiseLineRes` 的順序與註解）。
const NOISE_LINES: &[&str] = &[
    r"^[─━═╌╍]{3,}$",                                   // 輸入框分隔線（原寬＝終端機整行）
    r"^⏵⏵ ",                                            // bypass permissions 狀態列
    r"^⧉",                                              // artifact / desktop 提示列
    r"^❯\s*$",                                          // 空的輸入提示符
    r"^❯ (?![0-9]+[.．])",                               // 輸入提示回顯——但保留選單選項（❯ 1. Yes）
    r"^/[A-Za-z]{1,5}$",                                // 狀態列右側斜線碎片（/rc、/eff…單獨成行）
    r"^~[\\/][^\s]*$",                                  // cwd 路徑碎片單獨成行（~\Desktop、~/proj）
    r"^[·✢✣✤✥✳✶✻✽＊*+]+$",                              // spinner 殘影（如 ✢*✶）
    r"^[·✢✣✤✥✳✶✻✽*+\s]*\S{1,30}…\s*(\(.*)?$",           // spinner 動詞行（Inferring… / ✢ Inferring… (40s…）
    r"^\(\d+s( ·.*)?$",                                 // spinner 括號統計殘段（(40s · ↓ 3.3）
    r"^(←\s*)?\d+ agents?$",                            // 狀態列 agents 碎片（← 3 agents）
    r"connecting…",                                     // 連線中提示（/rc connecting…）
    r"^(Mon|Tue|Wed|Thu|Fri|Sat|Sun) (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) +\d", // 狀態列日期
    r"^⎿\s+Tip: ",                                      // 小提示列
    r#"^❯\s*Try ""#,                                    // 輸入框的建議 placeholder
    r"^[◉⏸]\s",                                         // 狀態列右側（◉ xhigh · /effort / ⏸ manual mode）
    r"^You've used \d+% of your (weekly|session) limit", // 用量提示列
    r"^[│┌┐└┘├┤╭╮╰╯─╴╶\s]+$",                           // 方框邊線（歡迎框／選擇題框）
    r"✂.*hidden",                                       // 選擇題摺疊提示（✂ 7 lines hidden）
    r"^Notes: press n",                                 // 選擇題備註提示
    r"^Chat about this$",                               // 選擇題聊天提示
    r"Enter to select|↑/↓ to navigate|Esc to cancel",   // 選單導航列
    r"\(ctrl\+o to expand\)$",                          // ctrl+o 展開提示行
    r"^[☐☑]\s",                                         // 選擇題分頁標頭
    r"^│|│$",                                           // 方框內容行（頭／尾帶 │）
    r"[╭╮╰╯]",                                          // 含方框角字元的行
    r"^Welcome back .{0,30}!$",                         // claude 歡迎行
    r"^[▀-▟]{2,}",                                      // claude 開場 logo banner
    r"^[▀-▟\s]+$",                                      // 方塊字元行（logo 殘影）
    r"^[·✢✣✤✥✳✶✻✽*+\s]*\S+ for (\d+m )?\d+s$",          // 完成計時行（✻ Crunched for 3s）
    r"^\d{1,2}:\d{2}( \|.*)?$",                         // 狀態列時間／用量行（5:03 | Fable 5 | …）
    r"thinking with \S+ effort\)?$",                    // spinner 統計折行尾段
    r"^\(?thought for \d+m? ?\d*s\)?$",                 // 思考計時殘段
    r"↓ [\d.]+k? tokens",                               // spinner token 統計
    r"\| 5h: \d|\| 7d: \d|resets in \d",                // 狀態列用量截斷段
    r"^⎿\s+[a-z]\S{0,14}$",                             // ⎿ 小寫短殘字（保留 ⎿ Added/Read/(No output)）
];

/// **只在 TUI 畫面**才套用的碎片規則（照舊版 `TuiFragmentRes`）。
const TUI_FRAGMENTS: &[&str] = &[
    r"^[·✢✣✤✥✳✶✻✽*+]\s?[A-Za-z()\[\]-]{0,15}$", // spinner 字形＋短英文殘字（✣ Sock-hop）
    r"^[·✢✣✤✥✳✶✻✽*+]\s?\S{1,20}$",              // spinner 字形＋單一短 token
    r"^[a-z][a-z-]{0,5}$",                       // 小寫短碎片（hopp / k-ho / illo）
    r"^[A-Za-z]$",                               // 單一字母殘字
    r"^[一-鿿]$",                                // 單一中文字殘字（選單導航動畫殘影）
    r"^[\d()\[\]{}.,;:'\x22-]{1,3}$",            // 孤立碎片行（) 2 3 …重繪殘留）
];

/// 畫面上有這些字元就算 TUI（提示符 ❯、工具輸出 ⎿、spinner 字形、狀態列 ⏵◉）。
const TUI_GLYPHS: &[char] = &['❯', '⎿', '✻', '✢', '✶', '✽', '⏵', '◉'];

fn noise() -> &'static Vec<Regex> {
    static R: OnceLock<Vec<Regex>> = OnceLock::new();
    R.get_or_init(|| compile(NOISE_LINES))
}

fn fragments() -> &'static Vec<Regex> {
    static R: OnceLock<Vec<Regex>> = OnceLock::new();
    R.get_or_init(|| compile(TUI_FRAGMENTS))
}

fn compile(list: &[&str]) -> Vec<Regex> {
    list.iter()
        .filter_map(|p| match Regex::new(p) {
            Ok(r) => Some(r),
            Err(e) => {
                // 規則表寫錯不該讓遠端整個不能用：略過那一條並講出來
                println!("[AwayTerminal] Telegram 過濾規則編不過（{p}）：{e}");
                None
            }
        })
        .collect()
}

fn matches_any(list: &[Regex], s: &str) -> bool {
    list.iter().any(|r| r.is_match(s).unwrap_or(false))
}

/// 畫面看起來是 claude 一類的 TUI 嗎。
pub fn is_tui_screen(s: &str) -> bool {
    s.chars().any(|c| TUI_GLYPHS.contains(&c))
}

/// 去掉 claude 在框前的標記（`● ` / `⎿ `），供邊框偵測用（舊版 `StripBullet`）。
fn strip_bullet(bare: &str) -> &str {
    let mut cs = bare.chars();
    match (cs.next(), cs.next()) {
        (Some(c), Some(' ')) if c == '●' || c == '⎿' => bare[c.len_utf8() + 1..].trim(),
        _ => bare,
    }
}

fn table_top(s: &str) -> bool {
    re_cached(&TABLE_TOP, r"^[┌╭][─┬┌┐╭╮\s]*[┐╮]$", s)
}
fn table_bottom(s: &str) -> bool {
    re_cached(&TABLE_BOTTOM, r"^[└╰][─┴└┘╰╯\s]*[┘╯]$", s)
}
fn table_sep(s: &str) -> bool {
    re_cached(&TABLE_SEP, r"^[├][─┼├┤\s]*[┤]$", s)
}
/// 選擇題／確認框的內容行：剝邊框後保留（❯ 選中項、「1. 」編號選項、問句）。
fn question_line(s: &str) -> bool {
    re_cached(&QUESTION, r"^❯|^\d{1,2}\.\s|[?？]$", s)
}

static TABLE_TOP: OnceLock<Option<Regex>> = OnceLock::new();
static TABLE_BOTTOM: OnceLock<Option<Regex>> = OnceLock::new();
static TABLE_SEP: OnceLock<Option<Regex>> = OnceLock::new();
static QUESTION: OnceLock<Option<Regex>> = OnceLock::new();

fn re_cached(cell: &OnceLock<Option<Regex>>, pattern: &str, s: &str) -> bool {
    cell.get_or_init(|| Regex::new(pattern).ok())
        .as_ref()
        .and_then(|r| r.is_match(s).ok())
        .unwrap_or(false)
}

/// 把表格內容列（`│ a │ b │ c │`）攤平成 `a | b | c`（舊版 `FlattenRow`）。
fn flatten_row(t: &str) -> String {
    let mut cells: Vec<String> = t
        .split(['│', '｜'])
        .map(|c| c.trim().to_string())
        .collect();
    while cells.first().is_some_and(|c| c.is_empty()) {
        cells.remove(0);
    }
    while cells.last().is_some_and(|c| c.is_empty()) {
        cells.pop();
    }
    cells.join(" | ")
}

/// 空白收斂（比較用的 key）。
fn squeeze(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 手機閱讀瘦身。逐段照舊版 `TidyForPhone`。
pub fn tidy_for_phone(s: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut prev_blank = true;
    let mut in_table = false;
    let tui = is_tui_screen(s);

    for raw in s.replace('\r', "").split('\n') {
        let line = raw.trim_end();
        let bare = line.trim_start();
        let det = strip_bullet(bare);

        // ── 表格（只認有邊框的真資料表）──
        if table_top(det) {
            in_table = true;
            continue;
        }
        if in_table {
            if table_bottom(det) {
                in_table = false;
                continue;
            }
            if table_sep(det) {
                continue;
            }
            if det.contains('│') || det.contains('｜') {
                let fr = flatten_row(det);
                if !fr.is_empty() {
                    out.push(fr);
                    prev_blank = false;
                }
                continue;
            }
            in_table = false; // 沒有底框就離開表格 → 這行照一般規則處理
        }

        // │ 邊框行：內文是問句／選項（claude 選擇題）→ 剝框保留
        if !bare.is_empty()
            && (bare.starts_with('│') || bare.ends_with('│'))
        {
            let inner = bare.trim_matches(|c| c == '│' || c == ' ');
            if !inner.is_empty() && question_line(inner) {
                out.push(format!("  {inner}"));
                prev_blank = false;
                continue;
            }
        }
        // 不在表格內、夾著 │ 的行＝歡迎／changelog 兩欄裝飾框 → 整行丟棄
        if bare.contains('│') {
            continue;
        }

        if !bare.is_empty()
            && (matches_any(noise(), bare) || (tui && matches_any(fragments(), bare)))
        {
            continue;
        }
        let blank = line.is_empty();
        if blank && prev_blank {
            continue;
        }
        out.push(line.to_string());
        prev_blank = blank;
    }

    // 前綴去重（只對 TUI 畫面）
    if tui {
        let mut keys: Vec<String> = out.iter().map(|l| squeeze(l.trim())).collect();
        let mut i = out.len();
        while i > 0 {
            i -= 1;
            if keys[i].chars().count() < 4 {
                continue; // 太短的另有碎片規則
            }
            let mut drop = false;
            for j in 0..out.len() {
                if j == i || keys[j].len() <= keys[i].len() {
                    continue;
                }
                if keys[j].starts_with(&keys[i]) {
                    drop = true;
                    break;
                }
            }
            if drop {
                out.remove(i);
                keys.remove(i);
            }
        }
    }

    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// 取字串尾端最多 `max` 個**字元**（舊版 `Clip` 取的是 UTF-16 單位並避免切半個 surrogate；
/// Rust 這邊按字元切，同樣不會切出壞字）。
pub fn clip_tail(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    s.chars().skip(n - max).collect()
}

/// Telegram 的 HTML parse mode 要 escape 的三個字元（舊版 `EscHtml`）。
pub fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// 一則訊息的字數上限。Telegram 的硬上限是 4096，舊版送 `<pre>` 包起來的內容時取 3500
/// 留給標頭與標籤。
pub const MAX_BODY: usize = 3500;
/// Telegram `sendMessage` 的硬上限。
pub const TELEGRAM_LIMIT: usize = 4096;

/// 超過上限的文字切成好幾則（**照行切**，不要把一行切兩半）。
///
/// 舊版沒有這個：它用 [`clip_tail`] 只送尾端。這是新版加的——使用者要看完整輸出時
/// （`/last 200`）舊版會默默吃掉前面（見 `docs/TELEGRAM.md` 的「刻意不同」）。
pub fn split_message(text: &str, limit: usize) -> Vec<String> {
    if text.chars().count() <= limit {
        return vec![text.to_string()];
    }
    let mut parts = Vec::new();
    let mut cur = String::new();
    for line in text.split('\n') {
        let line_len = line.chars().count();
        let cur_len = cur.chars().count();
        // 單行就超過上限 → 那一行自己再切
        if line_len >= limit {
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            let mut chunk = String::new();
            for c in line.chars() {
                if chunk.chars().count() + 1 > limit {
                    parts.push(std::mem::take(&mut chunk));
                }
                chunk.push(c);
            }
            if !chunk.is_empty() {
                cur = chunk;
            }
            continue;
        }
        if cur_len + 1 + line_len > limit {
            parts.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// 找出 `cur` 相對 `baseline` 的新增輸出（舊版 `DiffNew`）。
///
/// 終端機往下捲：`cur = […基準尾端…][新行]`。錨點＝基準尾端最多 3 個非空行；
/// 因為 prompt 行打字後會「原行變長」（`PS>` → `PS> ls`），最後一行用**前綴**比對。
///
/// - `Some("")`＝完全一樣（沒有新輸出）
/// - `None`＝比對不到錨點（清屏／TUI 大改／首次）→ 呼叫端退回快照模式
///
/// ⚠️ 開頭那段「剝掉兩邊從最底下往上一模一樣的行」是舊版 2026-09-15 使用者回報後加的：
/// Codex 的輸入框與狀態列固定在畫面最底、內容不變，新輸出插在它上面 →
/// 拿「最後 3 行」當錨點會對到最底下那幾行 → 新輸出＝空字串 → 自動推播整則不送。
pub fn diff_new(baseline: &str, cur: &str) -> Option<String> {
    if baseline.trim().is_empty() {
        return None;
    }
    let cur = cur.replace('\r', "");
    let base = baseline.replace('\r', "");
    let mut bl: Vec<&str> = base.split('\n').collect();
    let cl: Vec<&str> = cur.split('\n').collect();

    let mut nb = bl.len();
    let mut nc = cl.len();
    while nb > 0 && bl[nb - 1].trim().is_empty() {
        nb -= 1;
    }
    while nc > 0 && cl[nc - 1].trim().is_empty() {
        nc -= 1;
    }
    let mut common = 0;
    while common < nb && common < nc && bl[nb - 1 - common] == cl[nc - 1 - common] {
        common += 1;
    }
    if common == nb && common == nc {
        return Some(String::new()); // 完全一樣
    }
    let mut cur_owned = cur.clone();
    if common > 0 && common < nb && common < nc {
        bl.truncate(nb - common);
        cur_owned = cl[..(nc - common)].join("\n");
    }

    // 錨點＝基準尾端「連續」的最後 3 行（保留中間空行、只去尾端空行）
    let mut end = bl.len();
    while end > 0 && bl[end - 1].trim().is_empty() {
        end -= 1;
    }
    if end == 0 {
        return Some(cur_owned); // 基準是空畫面 → 全部都是新的
    }
    let anchor: Vec<&str> = bl[end.saturating_sub(3)..end].to_vec();
    let last_old = anchor[anchor.len() - 1];

    // 在 cur 裡找「前幾行完整相等 ＋ 最後一行是前綴」的位置
    let lines: Vec<&str> = cur_owned.split('\n').collect();
    let need = anchor.len();
    let mut found: Option<usize> = None;
    if lines.len() >= need {
        for start in (0..=lines.len() - need).rev() {
            let mut ok = true;
            for k in 0..need - 1 {
                if lines[start + k] != anchor[k] {
                    ok = false;
                    break;
                }
            }
            if !ok {
                continue;
            }
            let hit = lines[start + need - 1];
            if hit == last_old || hit.starts_with(last_old) {
                found = Some(start + need - 1);
                break;
            }
        }
    }
    let at = found?;
    // 錨點行有變化（prompt 變長）→ 把那一行也算進新輸出
    let changed = lines[at] != last_old;
    let from = if changed { at } else { at + 1 };
    if from >= lines.len() {
        return Some(String::new());
    }
    Some(lines[from..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 規則表全部編得過（少一條就會靜靜失效）。
    #[test]
    fn every_rule_compiles() {
        assert_eq!(noise().len(), NOISE_LINES.len(), "有雜訊規則編不過");
        assert_eq!(fragments().len(), TUI_FRAGMENTS.len(), "有碎片規則編不過");
    }

    /// 每一條雜訊規則各給一個例子（照舊版註解裡的樣子）。
    #[test]
    fn filters_every_noise_rule() {
        let cases = [
            "────────────────",
            "⏵⏵ bypass permissions on",
            "⧉ In Desktop",
            "❯",
            "❯ 這是測試",
            "/rc",
            "~\\Desktop",
            "✢*✶",
            "✢ Inferring… (40s · ↓ 3.3k",
            "(40s · ↓ 3.3",
            "← 3 agents",
            "/rc connecting…",
            "Sat Sep 27 16:00",
            "⎿  Tip: press ctrl+o",
            "❯ Try \"fix the bug\"",
            "◉ xhigh · /effort",
            "You've used 51% of your weekly limit",
            "│  ─────  │",
            "✂ 7 lines hidden",
            "Notes: press n to add",
            "Chat about this",
            "Enter to select · ↑/↓ to navigate",
            "some text (ctrl+o to expand)",
            "☐ 遊戲風格",
            "│ 內容",
            "╭─ Claude ─╮",
            "Welcome back awaysu!",
            "▐▛███▜▌ Claude Code",
            "▀▀▀ ▀▀",
            "✻ Crunched for 1m 36s",
            "5:03 | Fable 5 | 5h: 51",
            "…thinking with xhigh effort",
            "(thought for 7s)",
            "· ↓ 3.0k tokens · esc to interrupt",
            "9 | Fable 5 | 5h: 51",
            "⎿  cleaned",
        ];
        for c in cases {
            let got = tidy_for_phone(&format!("❯ x\n{c}\nreal output"));
            assert!(
                !got.contains(c),
                "這一行應該被濾掉：{c:?}\n結果：{got:?}"
            );
        }
    }

    /// 選單選項要**保留**（`❯ 1. Yes` 不是輸入回顯）。
    #[test]
    fn keeps_menu_options() {
        let screen = "Do you want to proceed?\n❯ 1. Yes\n  2. No\nEnter to select · ↑/↓ to navigate";
        let got = tidy_for_phone(screen);
        assert!(got.contains("❯ 1. Yes"), "{got}");
        assert!(got.contains("2. No"), "{got}");
        assert!(!got.contains("to navigate"), "導航列要濾掉：{got}");
    }

    /// **碎片規則只對 TUI 畫面套用**——一般 shell 的合法輸出不能被吃掉。
    #[test]
    fn shell_output_survives() {
        // 沒有 TUI 記號的畫面
        let shell = "* main\n  dev\nawaysu\n0\na\nREADME\nREADME.md";
        let got = tidy_for_phone(shell);
        for keep in ["* main", "  dev", "awaysu", "0", "a", "README", "README.md"] {
            assert!(
                got.lines().any(|l| l == keep),
                "一般 shell 的 {keep:?} 被吃掉了：{got:?}"
            );
        }
        // 同樣的碎片在 TUI 畫面上要被濾掉
        let tui = tidy_for_phone("❯ ask\nhopp\nF\n✻ Sock-\nreal answer");
        assert!(!tui.contains("hopp"), "{tui}");
        assert!(!tui.lines().any(|l| l == "F"), "{tui}");
        assert!(tui.contains("real answer"), "{tui}");
    }

    /// 前綴去重（只對 TUI）：重繪殘影刪掉、完整行留下。
    #[test]
    fn drops_redraw_prefixes_only_in_tui() {
        let tui = tidy_for_phone("❯ x\n● PowerSh\n● PowerShell(Sta\n● PowerShell(Start)");
        assert!(tui.contains("● PowerShell(Start)"), "{tui}");
        assert!(!tui.contains("● PowerSh\n"), "{tui}");
        // 一般 shell：README 與 README.md 都要留
        let shell = tidy_for_phone("README\nREADME.md");
        assert_eq!(shell, "README\nREADME.md");
    }

    /// 有邊框的真資料表攤平成 `a | b | c`。
    #[test]
    fn flattens_real_tables() {
        let table = "┌──────┬──────┐\n│ 項目 │ 值   │\n├──────┼──────┤\n│ 速度 │ 快   │\n└──────┴──────┘";
        let got = tidy_for_phone(table);
        assert_eq!(got, "項目 | 值\n速度 | 快", "{got:?}");
    }

    /// 無邊框的兩欄裝飾框（歡迎框／changelog）整行丟棄，不要當表格攤平。
    #[test]
    fn drops_decorative_two_column_boxes() {
        let welcome = "  Claude Code v2.1 │ /help for help\n  cwd: C:\\x │ model: fable\nreal output";
        let got = tidy_for_phone(welcome);
        assert_eq!(got, "real output", "{got:?}");
    }

    /// 連續空行收斂成一行、尾端空行去掉。
    #[test]
    fn squeezes_blank_lines() {
        assert_eq!(tidy_for_phone("a\n\n\n\nb\n\n\n"), "a\n\nb");
    }

    /// `clip_tail` 取尾端、不切壞多位元字元。
    #[test]
    fn clips_from_the_tail() {
        assert_eq!(clip_tail("abcdef", 3), "def");
        assert_eq!(clip_tail("abc", 10), "abc");
        let cjk = "一二三四五";
        assert_eq!(clip_tail(cjk, 2), "四五");
        assert_eq!(clip_tail("a🙂b", 2), "🙂b", "emoji 不能切一半");
    }

    /// 訊息切段：照行切、每段不超過上限。
    #[test]
    fn splits_long_messages_by_line() {
        let text = (1..=100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let parts = split_message(&text, 100);
        assert!(parts.len() > 1);
        for p in &parts {
            assert!(p.chars().count() <= 100, "有一段超過上限：{}", p.chars().count());
        }
        // 內容不能少、順序不能變
        assert_eq!(parts.join("\n"), text);
        // 沒超過上限就只有一段
        assert_eq!(split_message("short", 100), vec!["short".to_string()]);
    }

    /// 單一行就超過上限時，那一行自己切。
    #[test]
    fn splits_one_very_long_line() {
        let line = "x".repeat(250);
        let parts = split_message(&line, 100);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts.concat(), line);
    }

    /// `diff_new`：只推新輸出。
    ///
    /// 基準是**整個畫面**（呼叫端給 400 行），所以錨點是尾端 3 行、在畫面上唯一。
    #[test]
    fn diffs_only_new_output() {
        let base = "PS C:\\> echo one\none\nPS C:\\> ";
        let cur = "PS C:\\> echo one\none\nPS C:\\> echo two\ntwo\nPS C:\\> ";
        let d = diff_new(base, cur).expect("應該找得到錨點");
        assert!(d.contains("two"), "{d:?}");
        assert!(!d.contains("one"), "舊輸出不該再推一次：{d:?}");
        // 錨點行變長（PS> → PS> echo two）→ 那一行也算新輸出
        assert!(d.starts_with("PS C:\\> echo two"), "{d:?}");
    }

    /// **退化情況**：基準只有一行、而那一行在新畫面上重複出現（shell 的 prompt）。
    ///
    /// 錨點只有一行時只能挑「最後一個符合的位置」，那就是畫面最底的新 prompt →
    /// 它後面沒有東西 → 回 `Some("")`。**舊版 `DiffNew` 的 `LastIndexOf` 行為完全一樣**；
    /// 實際上不會發生，因為呼叫端給的基準是整個畫面（400 行），錨點是尾端 3 行、
    /// 在畫面上唯一。這條測試把界線釘住，免得有人以為它壞了而「修」成單行也往前找。
    #[test]
    fn single_line_anchor_that_repeats_is_ambiguous() {
        let d = diff_new("PS C:\\> ", "PS C:\\> ls\nfile1\nPS C:\\> ");
        assert_eq!(d.as_deref(), Some(""), "和舊版同樣的結果");
    }

    /// 完全一樣＝沒有新輸出（`Some("")`），呼叫端才知道「不要送空訊息」。
    #[test]
    fn same_screen_means_no_new_output() {
        assert_eq!(diff_new("a\nb\n", "a\nb\n").as_deref(), Some(""));
        // 尾端空白列不算差異
        assert_eq!(diff_new("a\nb", "a\nb\n\n").as_deref(), Some(""));
    }

    /// **固定在畫面最底的輸入框**（Codex）：新輸出插在它上面也要找得到。
    ///
    /// 這是舊版 2026-09-15 使用者回報的那個 bug：拿「最後 3 行」當錨點會對到最底下那幾行，
    /// 新輸出算成空字串 → 手機收不到回覆。
    #[test]
    fn finds_new_output_above_a_fixed_input_box() {
        let base = "Q1\nA1\n\n› Ask Codex to do anything\nmodel: gpt · cwd: C:\\x";
        let cur = "Q1\nA1\nQ2\nA2\n\n› Ask Codex to do anything\nmodel: gpt · cwd: C:\\x";
        let d = diff_new(base, cur).expect("應該找得到");
        assert!(d.contains("Q2") && d.contains("A2"), "{d:?}");
        assert!(!d.contains("Ask Codex"), "底部固定那幾行不算新輸出：{d:?}");
    }

    /// 基準是空的／對不上 → `None`（呼叫端退回快照）。
    #[test]
    fn returns_none_when_the_anchor_is_gone() {
        assert_eq!(diff_new("", "anything"), None, "空基準＝沒有基準");
        assert_eq!(
            diff_new("old screen\nthat is gone", "completely different\nscreen here"),
            None,
            "清屏之後對不到錨點"
        );
    }

    /// HTML escape 只動那三個字元。
    #[test]
    fn escapes_html() {
        assert_eq!(esc_html("<a> & 'b'"), "&lt;a&gt; &amp; 'b'");
    }

    /// **真的 claude 畫面**當 fixture：瘦身後只剩答案，沒有 spinner／狀態列／方框。
    #[test]
    fn real_claude_screen_becomes_readable() {
        let screen = include_str!("../../resources/telegram/claude-screen.txt");
        let got = tidy_for_phone(screen);
        // 答案留下來
        assert!(got.contains("4"), "答案不見了：{got:?}");
        // 這些都要不見
        for gone in [
            "▐▛███▜▌",
            "Welcome back",
            "⏵⏵ bypass permissions",
            "✻ Crunched",
            "↓ 3.0k tokens",
            "Enter to select",
            "╭",
            "│",
        ] {
            assert!(!got.contains(gone), "{gone:?} 應該被濾掉：{got:?}");
        }
        // 瘦身後應該短很多
        assert!(
            got.lines().count() < screen.lines().count() / 2,
            "瘦身效果太差：{} → {} 行",
            screen.lines().count(),
            got.lines().count()
        );
    }
}
