//! 命令列字串 → `execvp` 的參數陣列。
//!
//! Windows 的 `CreateProcess` 吃**一整條字串**，Unix 的 `execvp` 吃**陣列**。
//! 主 crate 的 `SpawnOptions.command_line` 是前者的形狀（舊版就是這樣存的），
//! 所以 Unix 這邊要切開。
//!
//! 放在這個 crate（而不是 `src/pty/unix.rs`）的理由：它是純邏輯，放在這裡**所有平台
//! 都編得過也測得到**——放在 `#[cfg(not(windows))]` 的檔案裡，它的測試在 Windows 上
//! 永遠不會跑，等於沒有測試。

/// 切開命令列。支援用雙引號包住含空白的路徑。
///
/// **不做 shell 展開**（`~`、`$VAR`、`*` 都原樣傳過去）——我們是直接 `execvp`，
/// 和 Windows 端直接 `CreateProcess` 對稱。要展開就得經過 shell，那是另一件事。
pub fn split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut had_quote = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                in_quote = !in_quote;
                had_quote = true;
            }
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() || had_quote {
                    out.push(std::mem::take(&mut cur));
                    had_quote = false;
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() || had_quote {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn splits_plain_commands() {
        assert_eq!(split("/bin/zsh"), vec!["/bin/zsh"]);
        assert_eq!(split("/usr/bin/pwsh -NoLogo"), vec!["/usr/bin/pwsh", "-NoLogo"]);
        assert_eq!(split("  a   b  "), vec!["a", "b"]);
        assert_eq!(split(""), Vec::<String>::new());
    }

    /// 引號包住含空白的路徑（mac 的 `/Applications/…` 常常有空白）。
    #[test]
    fn honours_quotes() {
        assert_eq!(
            split("\"/Applications/My App/bin/tool\" --flag"),
            vec!["/Applications/My App/bin/tool", "--flag"]
        );
        assert_eq!(split("\"a b\" \"c d\""), vec!["a b", "c d"]);
    }

    /// 空引號 `""` 是一個**空參數**，不可以被吃掉（`tool "" x` 有三個參數）。
    #[test]
    fn an_empty_quoted_argument_survives() {
        assert_eq!(split("tool \"\" x"), vec!["tool", "", "x"]);
    }

    /// **不做** shell 展開——和 Windows 端對稱。
    #[test]
    fn does_not_expand_like_a_shell() {
        assert_eq!(split("~/bin/x"), vec!["~/bin/x"]);
        assert_eq!(split("$SHELL"), vec!["$SHELL"]);
        assert_eq!(split("ls *.txt"), vec!["ls", "*.txt"]);
    }
}
