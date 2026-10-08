# 安全說明

*[English below](#security-notes-english)*

這份文件寫給**使用這個程式的人**，不是給開發者的（開發面的細節在
`src-tauri/deny.toml`、`THIRD-PARTY-NOTICES.md`、`docs/SSH.md`）。

發佈之後 GitHub repo 的 **Security policy 就指這一份**。

---

## 1. 回報漏洞

**請不要開公開的 issue。** 用 GitHub 的私下回報：

> repo 頁面 → **Security** → **Report a vulnerability**
> （<https://github.com/awaysu/AwayTerminal/security/advisories/new>）

那條路只有維護者看得到，可以在修好之後才公開。沒辦法用 GitHub 的話，
用 repo 上的作者信箱，主旨請寫 `AwayTerminal security`。

回報請盡量附：影響哪一版、作業系統、重現步驟、以及你認為的影響範圍。

---

## 2. Telegram 遠端的 bot token 是**明文**存的

| | |
|---|---|
| 存在哪 | `settings.json` 的 `telegramBotToken`（Windows：`%APPDATA%\com.awaysu.awayterminal\settings.json`） |
| 為什麼 | **舊版 1.x 就是明文存**，沿用才能直接匯入舊設定；改成加密要 Windows DPAPI／macOS keychain／Linux libsecret 三套，而 Linux 上還可能沒有 keyring |

**這代表什麼**：同一台電腦上、以你的身分執行的任何程式都讀得到它——包括你自己開的
AI agent。拿到 token 的人可以操作那個 bot（讀你發過的訊息、發訊息、看你的 chat id）。
**但拿到 token 不等於能操作你的電腦**：遠端只認設定裡那一個 chat id，別人的訊息一律
不理也不回。

**我們做了什麼降低風險**：

- token **不進 log、不進任何回報、不進 `--verify` 的輸出**
- 錯誤訊息**不含 URL**——Bot API 的網址本身就是 `…/bot<token>/method`，而
  `ureq` 的錯誤在某些情況會把完整網址帶出來，一次輪詢失敗就會把 token 寫進 log。
  `api.rs` 的 `describe()` 只留錯誤型別與 HTTP 狀態，有單元測試
  （`errors_never_leak_the_url`）守著
- 設定視窗**永遠不回填** token（只顯示「已設定／未設定」），所以截圖或旁人看螢幕都拍不到
- 遠端預設**關閉**；不開就完全沒有這件事

**你可以做的**：不用遠端就別開它。用了而覺得 token 可能外流 → 到 Telegram 的
`@BotFather` 撤銷並重發（`/revoke`），再貼新的進設定視窗。

---

## 3. 沙盒模式是**防呆不是防壞**

代理團隊與 AI 工具的分頁可以開「沙盒」（**預設是關的**，要自己勾選）。它做的事：

- 每個分頁一棵 `git worktree`（agent 改的檔案不會直接動到你的工作目錄）
- `TEMP`／`CARGO_TARGET_DIR` 導到沙盒目錄
- Windows 用 Job Object（**關分頁就把整棵子行程收掉**），mac／Linux 用行程群組
- 自動產生各工具的護欄設定——Claude Code 的 `PreToolUse` hook 會拒絕
  `taskkill /IM`、`Stop-Process -Name`、刪 repo 以外的路徑、`git push --force`

它**不**做的事，請務必知道：

| | |
|---|---|
| **不隔離 `HOME`／`APPDATA`／`USERPROFILE`** | 那會讓 Claude Code、Codex 掉登入。所以 agent 讀得到你那些目錄下的東西（含各種工具的設定與憑證） |
| **不是作業系統層級的沙箱** | agent 和你在**同一個登入工作階段**，繞過護欄仍然碰得到你的桌面、視窗、其他行程 |
| **已知漏洞**：Store 別名逃出 Job Object | 用 Microsoft Store 的 *app execution alias* 啟動的行程（很多機器上的 `pwsh` 就是）由 AppX 啟動服務建立，**不會進我們的 Job Object** → 關分頁收不掉那一棵。程式已經把 `WindowsApps` 下的別名排到搜尋順序最後，但只要有東西是那樣啟動的就收不到（`examples/job_probe.rs` 兩種都實測過） |
| 護欄是**名單式**的 | 它擋得住寫成一般形式的危險指令，擋不住刻意繞路（改個寫法、包一層腳本）。它的目的是防止 agent **不小心**做壞事，不是防止有意的行為 |

**真正的隔離要靠虛擬機**（Windows Sandbox／Hyper-V／WSL2）。選項與取捨寫在
`docs/AGENT-SANDBOX.md`。

**你可以做的**：把真的敏感的東西放在不同的 Windows 使用者帳號或虛擬機裡跑 agent；
不要在放著生產憑證的機器上讓 agent 自由跑。

---

## 4. SSH

### 建議用 Ed25519 金鑰登入

```bash
ssh-keygen -t ed25519
```

我們的 SSH 後端依賴的 `rsa` crate 有一個**沒有修補版本**的已知問題
（`RUSTSEC-2023-0071`，Marvin Attack：RSA 私鑰運算不是常數時間）。

- **驗證伺服器的主機金鑰不受影響**——那是公開金鑰運算，過程裡沒有你的秘密
- **用 RSA 私鑰登入**才有理論上的曝險，而且要攻擊者①控制你連上的伺服器、
  ②在大量連線中量測時間差
- **改用 Ed25519（或密碼登入）就完全繞開它**

完整分析在 `docs/SSH.md` 的第 4a 節；決定的紀錄在 `src-tauri/deny.toml`。
上游修好之後我們會升版並更新這一段。

### 主機金鑰存在哪

`{設定資料夾}/known_hosts`，OpenSSH 格式。Windows 是
`%APPDATA%\com.awaysu.awayterminal\known_hosts`。

- **不會碰你的 `~/.ssh`**（那是 `ssh` 指令的地盤，我們不動它）
- PuTTY 是存在登錄檔，我們用檔案是為了跨平台
- **金鑰變更一律拒絕連線**（不是警告後讓你繼續）——那是中間人攻擊最常見的徵兆。
  真的換過機器的話，把 `known_hosts` 裡那一行刪掉再連

### 密碼與金鑰密語

- **不存密碼。** 「我的最愛」沒有密碼欄位（單元測試 `favorite_has_no_password_field`
  釘住這件事），舊版也沒有
- 密碼在終端機裡問、**不回顯**，所以畫面上沒有密碼，Telegram 的 `/last` 與完成推播
  也不可能把它送出去
- 金鑰密語同樣不存，每次連線時問

---

## 5. 其他值得知道的

| | |
|---|---|
| **log 檔** | 記錄的是終端機畫面的內容——**你在裡面打的密碼如果會回顯就會被記下來**。log 預設關閉，開了要注意放哪裡（預設在「我的文件\AwayTerminalLogs」） |
| **恢復分頁** | 關程式時會把畫面內容存到設定資料夾底下（`restore/tab<n>.txt`），下次開啟倒回去。**內容包含畫面上的一切**。不想留就在離開對話框取消勾選「下次開啟恢復目前分頁」 |
| **代理團隊的信箱** | `.ai/bus/` 是**純文字**、在你的專案資料夾裡。agent 之間講的話都在那裡，會被 git 看到（除非你 ignore 它） |
| **更新檢查** | 只問 `awaysu.cc` 有沒有新版、**不會自動下載安裝**。自動更新（Tauri updater）目前**停用**（沒有簽章金鑰） |
| **程式碼簽章** | 目前**還沒有**，所以 Windows SmartScreen 會警告。計畫在 `docs/RELEASE.md` |
| **對外連線** | 只有三種：你自己開的連線（SSH／Telnet）、更新檢查、以及開著遠端時的 Telegram long polling。沒有任何遙測 |

---

<a name="security-notes-english"></a>

# Security notes (English)

This file is for **people using the program**. It is what the GitHub repository's
Security policy points at.

## Reporting a vulnerability

**Please do not open a public issue.** Use GitHub's private reporting:
repository → **Security** → **Report a vulnerability**
(<https://github.com/awaysu/AwayTerminal/security/advisories/new>). If you cannot use
GitHub, email the author address on the repository with the subject
`AwayTerminal security`.

## The Telegram bot token is stored in plain text

It lives in `settings.json` (`telegramBotToken`), exactly as version 1.x stored it —
keeping the format is what lets the old settings be imported. Encrypting it would mean
three different platform key stores, and Linux may not have a keyring at all.

Any program running as you can therefore read it, **including AI agents you start
yourself**. Holding the token lets someone operate that bot; it does **not** let them
operate your computer, because the remote only accepts the one chat id in your settings
and silently ignores everybody else.

Mitigations in place: the token never reaches a log, a report or `--verify` output;
error messages never contain the URL (the Bot API URL *is* the token, and `ureq` errors
can leak the full URL — `api.rs::describe()` strips it, pinned by a unit test); the
settings dialog never fills the field back in; and the remote is off by default.

If you think the token leaked, revoke it with `@BotFather` (`/revoke`) and paste the new
one in.

## Sandbox mode is a guard rail, not a sandbox

Agent and AI-tool tabs get a `git worktree` of their own, redirected `TEMP`, a Windows
Job Object with kill-on-close (process groups on Unix), and generated deny rules for each
tool (Claude Code's `PreToolUse` hook refuses `taskkill /IM`, deleting paths outside the
repo, `git push --force`).

What it does **not** do:

- **`HOME`/`APPDATA`/`USERPROFILE` are deliberately not isolated** — isolating them logs
  Claude Code and Codex out. Agents can read what lives there, including other tools'
  credentials.
- It is **not** an OS-level sandbox. Agents share your login session and can reach your
  desktop if they go around the guard rails.
- **Known hole**: processes started through a Microsoft Store *app execution alias*
  (`pwsh` on many machines) are created by the AppX activation service and **never join
  our Job Object**, so closing the tab does not reap them.
- The deny rules are a list. They stop accidents, not intent.

Real isolation needs a VM — the options are in `docs/AGENT-SANDBOX.md`.

## SSH

**Prefer Ed25519 keys** (`ssh-keygen -t ed25519`). The `rsa` crate that our SSH backend
depends on has an unpatched advisory (`RUSTSEC-2023-0071`, Marvin Attack: RSA private-key
operations are not constant-time).

- **Verifying a server's host key is unaffected** — that is a public-key operation with
  no secret of yours involved.
- Only **logging in with an RSA private key** is theoretically exposed, and an attacker
  would need to control the server you connect to *and* time many connections.
- Ed25519 (or password authentication) avoids the code path entirely.

Host keys are cached in `{config dir}/known_hosts` in OpenSSH format — **never in your
`~/.ssh`**. A **changed** host key **always refuses the connection** rather than warning;
delete the line if you genuinely replaced the machine.

Passwords and key passphrases are **never stored** (favourites have no password field,
pinned by a unit test) and password input is **not echoed**, so it cannot appear in a
screenshot, a log or a Telegram push.

## Other things worth knowing

Session logs record what is on screen, so a password that *does* echo would be recorded;
logging is off by default. Session restore saves screen contents under the config
directory — untick "restore tabs next time" if you would rather it did not. The agent
mailbox `.ai/bus/` is plain text inside your project folder. The update check only asks
whether a newer version exists and never downloads anything; auto-update is currently
**disabled** (no signing key). The binaries are **not code-signed yet**. The only
outbound connections are the ones you open (SSH/Telnet), the update check, and Telegram
long polling while the remote is enabled. There is no telemetry.
