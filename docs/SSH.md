# SSH（內建 `russh`）

舊版的 SSH 是**呼叫系統 `ssh.exe`**，所以「照抄舊版」在協定層不成立。基準分成兩半
（這是 `CLAUDE.md` 定的：SSH 內建、**行為照 PuTTY**）：

- **使用流程**（欄位、`login as:` 互動、關閉鍵、重連、保持連線）→ 照舊版
  `MainWindow.xaml.cs` / `Dialogs/ConnectDialog.xaml`。
- **協定層**（驗證順序、主機金鑰、演算法、指紋、對話框語意）→ 照 PuTTY。
  官方原始碼 `git.tartarus.org/simon/putty.git`；
  ⚠️ `github.com/PuTTY-Terminal-Suite` **是假的**，絕不使用。

程式在 `src-tauri/src/ssh/`：`mod.rs`（連線、驗證、輸入輸出）、
`hostkey.rs`（主機金鑰記錄）、`prompt.rs`（主機金鑰對話框的 Rust↔前端橋）。

---

## 1. 舊版使用流程對照

| 舊版行為 | 出處 | 新版 | 狀態 |
|---|---|---|---|
| 分頁標題先是 `host`，輸入帳號後變 `user@host` | `OpenSshLoginAs` / `HandleLoginInput` | 同（`on_user` 回報帳號 → 改標題 + 送 `t`） | ✅ |
| 終端機裡問 `login as: `（**不是**跳視窗） | 同上 | 同 | ✅ |
| `login as:` 的 Backspace 退格（回顯 `\b \b`）、Enter 送出、其餘字元回顯 | `HandleLoginInput` | 同（`prompt_line`） | ✅ |
| 關分頁送 **Ctrl+D ×3** | `GracefulExitBytes = {0x04,0x04,0x04}` | 同（`ssh::GRACEFUL_EXIT_BYTES`） | ✅ |
| 連不上時不可以留一個「打字全被吞」的死分頁 | `HandleLoginInput` 的 catch（註解寫得很清楚） | 連線失敗會 `on_exit`，錯誤印在畫面上 | ✅ |
| 狀態燈：遠端連線看「近期有輸出」（不看子行程） | `UpdateStatuses` 的 else 分支 | 同（`TabKind::Ssh` 不是 `is_local_shell`） | ✅ |
| 分頁名稱之後跟著遠端目前目錄（提示行解析） | `TracksCwdTitle` 含 `Ssh` | 同（`q…cwd` 對 SSH 分頁也送） | ✅ |
| 連線對話框：類型／IP 主機（可編輯下拉＋歷史）／Port（ssh 22）／保持連線（分鐘，0=關）／斷線自動重連 | `ConnectDialog.xaml` | **只做了 host[:port] 一行輸入** | ⬜ TASK-007 |
| 保持連線 → `ServerAliveInterval = 分鐘×60`、`ServerAliveCountMax=3`，預設 10 分鐘 | `SshCommand` / `AppSettings.KeepAliveMins` | 尚未實作（russh 的 `keepalive_interval` / `keepalive_max` 對得上） | ⬜ TASK-007 |
| 斷線自動重連：退避 3,6,9…最多 30 秒；有輸出就歸零；等待中按 Enter 立刻重連 | `ScheduleReconnect` / `ManualReconnect` | 尚未實作 | ⬜ TASK-007 |
| `-o SendEnv=CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN` | `SshCommand` | 尚未實作（內建 SSH 要改成送 `env` request 或在遠端 shell 設） | ⬜ TASK-007 |

### 與舊版**刻意不同**的一點

舊版在**連線前**就顯示 `login as:`（因為帳號要放進 `ssh.exe` 的命令列）。
新版是**連上、交握完成之後**才問——這是 PuTTY 的行為，也是內建 SSH 的自然順序
（帳號是 SSH 驗證的一部分，不是命令列參數）。使用者看到的差別只有：
`login as:` 前面會先出現一行灰字「連線到 host:port …」，而且主機金鑰對話框（如果有）
會在 `login as:` **之前**跳出來。

---

## 2. PuTTY 行為對照

| 項目 | PuTTY 怎麼做 | 我們怎麼做 | 狀態 |
|---|---|---|---|
| 帳號 | 連上後在終端機問 `login as:` | 同 | ✅ |
| 密碼 | 終端機裡問 `user@host's password: `，**完全不回顯** | 同（`prompt_line(echo=false)`） | ✅ |
| 密碼錯誤 | 顯示 `Access denied` 並可重試 | 同，最多 3 次後結束連線 | ✅ |
| keyboard-interactive | 伺服器給名稱／說明／每一題的 `echo` 旗標，逐題問 | 同 | ✅ |
| publickey | 支援 OpenSSH 格式與 **`.ppk`**；有密碼的金鑰當場問 passphrase | 同（`russh` 的 `decode_secret_key` 看到 `PuTTY-User-Key-File-` 就走 PPK） | ✅ |
| Pageant | 用 Pageant 裡的金鑰，沒有 Pageant 就跳過 | 同（`AgentClient::connect_pageant()`；失敗只記 log 不打擾使用者）。mac/Linux 走 `SSH_AUTH_SOCK` | ✅ 已接，**未實機驗證**（見第 6 節） |
| 驗證順序 | none 探測 → publickey（含 agent）→ keyboard-interactive → password | 同 | ✅ |
| 主機金鑰快取 | 登錄檔 `HKCU\Software\SimonTatham\PuTTY\SshHostKeys` | **檔案** `{app config dir}/known_hosts`（見第 3 節） | ✅ |
| 第一次連線 | 「The server's host key is not cached」+ 指紋 + 三個選項 | 同語意（繁中） | ✅ |
| 金鑰變更 | **「POSSIBLE SECURITY BREACH!」**，更嚴重的警告 | 同語意：紅框紅字標題、預設焦點在「取消」 | ✅ |
| 指紋 | 新版 SHA256、舊版 MD5 | **兩個都顯示**，另加演算法與位元數 | ✅ |
| 三個選項 | Accept（存）／Connect Once（不存）／Cancel | 接受並儲存／只這次／取消 | ✅ |
| 憑證式主機金鑰 | 支援（需設定信任的 CA） | **明確拒絕**並在畫面上說明，不默默放行 | ⬜ 之後 |
| 演算法順序 | 自己的清單，舊演算法排後面；用到弱演算法會警告 | **還是 russh 的安全預設** | ⬜ TASK-007 |
| 每條連線覆蓋演算法 | 有 | 尚未 | ⬜ TASK-007 |

---

## 3. 主機金鑰檔

- **位置**：`{app config dir}/known_hosts`。Windows 是
  `%APPDATA%\com.awaysu.awayterminal\known_hosts`。啟動每條 SSH 連線時會把路徑印在
  後端 log（`[AwayTerminal] known_hosts: …`）。
- **格式**：就是 OpenSSH 的 `known_hosts`——一行一台：

  ```text
  example.test ssh-ed25519 AAAAC3Nza…
  [example.test]:2222 ssh-ed25519 AAAAC3Nza…
  ```

  非預設埠用 `[host]:port`（OpenSSH 慣例，`russh` 的讀取端也認這個）。
- **為什麼不自己定格式**：這個格式已經是「一行一台、人看得懂、能直接刪一行」，
  而 `russh` 自帶讀取與「金鑰換了」的判斷。自創格式只會多一份要維護的解析器。
- **要忘記某一台**：用文字編輯器刪掉那一行即可。
- ⚠️ **我們不讀也不寫 `~/.ssh/known_hosts`**：那是 OpenSSH 的檔案，程式不該偷偷往裡面加東西。
  代價是第一次連某台主機仍會問一次，即使你用 `ssh` 連過。
- **檔案壞掉時**：`check` 會回「金鑰不符」而**不是**「沒記錄」——寧可跳最嚴重的對話框，
  也不要把警告吞掉然後覆寫。

---

## 4. 演算法（現況）

這一段**還沒做**（TASK-007）。現在用 `russh` 的預設清單（`Preferred::DEFAULT`），
那是「安全的那一組」，所以**很舊的設備可能連不上**。

已經確定會用到的 feature 都開了：

- `des` → `3des-cbc`
- `rsa` → `ssh-rsa`（SHA-1 主機金鑰）
- `ring` 當 crypto backend（不是 russh 預設的 `aws-lc-rs`，因為它在 Windows 上要裝 NASM）

TASK-007 要做的：把預設順序改成 PuTTY 的（`curve25519` → `ecdh` → `group-exchange-sha256`
→ … → `group14-sha1` → `group1-sha1` 排最後）、每條連線可覆蓋、用到弱演算法時照 PuTTY 給警告。

---

## 5. 自動驗證：`ssh_probe`

```
cd src-tauri
cargo run --example ssh_probe
```

**完全不靠網路、不連任何外部主機**：用 `russh` 自己的 server 端在同一支程式裡起一台測試
sshd（綁 `127.0.0.1` 的臨時埠），再用我們的 client 連上去。2026-09-26 實測 **11 PASS / 0 FAIL**：

| 驗什麼 | 怎麼驗 |
|---|---|
| 密碼驗證 + `login as:` 互動 | 等 `login as: ` → 送帳號 → 等 `password: ` → 送密碼 → 收到遠端 banner |
| 密碼不回顯 | 整段輸出裡找不到密碼字串 |
| 第一次連線會問使用者 | decider 收到的 verdict 是 `Unknown` |
| `resize` 送出 window-change | 伺服器收到 `100x40` 並回報 |
| 送指令拿到輸出 | 伺服器回 `GOT:echo hello` |
| 關閉送 Ctrl+D | 伺服器數到 **3 個 `0x04`** |
| 已記錄過就不再問 | 第二次連線時故意讓 decider 回「拒絕」，仍然連得上＝根本沒問 |
| **金鑰換掉要被擋下** | 在同一個 `host:port` 記一把不同的金鑰 → verdict 是 `Changed { line: 1 }`、連線結束、**沒有進到 shell** |
| 「只這次」不寫檔 | 連得上，而且 `known_hosts` 內容前後一模一樣 |
| `.ppk`（未加密） | 連得上，伺服器確認走的是 publickey |
| `.ppk`（加密，密碼 `123`） | 同上 |

`log_probe` 那種「環境問題」也一併分離得開：`ssh_probe` 不碰使用者的檔案，
`known_hosts` 寫在 `%TEMP%` 的專屬資料夾並在結束時刪掉。

### app 端路徑

`--verify` 會多連一次 `127.0.0.1:1`（**沒人在聽**的埠），驗
`session_create(kind:"ssh")` → 建分頁（`kind=ssh`、`backend=russh`）→
連不上時把原因印在終端機 → session 正常結束。同樣不碰外部主機。

### 真連線

這台機器**沒有** Windows OpenSSH sshd（`Get-Service sshd` 查不到），所以真連線那一組
自動跳過並在 `ssh_probe` 的輸出印 `SKIP`。要跑的話：裝「OpenSSH 伺服器」選用功能、
啟動 `sshd` 服務，再用 `ssh_probe` 之外的手動步驟連 `127.0.0.1`。

---

## 6. ⚠️ 待真機驗證（要使用者的設備清單）

`CLAUDE.md` 風險 3 的核心：**問題不只演算法**。下面每一項都要在使用者實際會連的設備上勾一次。
Agent-11 拿到設備清單之後逐台填。

| # | 要驗什麼 | 為什麼可能出問題 | 設備 A | 設備 B | 設備 C |
|---|---|---|---|---|---|
| S1 | 連得上、拿到 shell | 最基本 | | | |
| S2 | **kex**：`diffie-hellman-group1-sha1` / `group14-sha1` / `group-exchange-sha1` | 現在用 russh 的安全預設，這些**沒有**在清單裡 → 舊設備會在交握就失敗（TASK-007 要補） | | | |
| S3 | **cipher**：`aes128-cbc` / `aes256-cbc` / `3des-cbc` | 同上；`des` feature 已開，但順序還沒放進去 | | | |
| S4 | **MAC**：`hmac-sha1` | 同上 | | | |
| S5 | **hostkey**：`ssh-rsa`（SHA-1 簽章） | `rsa` feature 已開，`Preferred::DEFAULT` 也含 `Rsa { hash: None }`，但要實測 | | | |
| S6 | **banner**：連線前的公告文字有沒有正確顯示 | 有些設備在驗證前送一大段 banner，換行／編碼可能怪 | | | |
| S7 | **keyboard-interactive**：有些設備用它代替密碼 | 提示文字、`echo` 旗標、多回合（含變更密碼流程） | | | |
| S8 | **不理 ext-info**：舊設備收到 `ext-info-c` 可能直接斷線 | russh 會送，舊設備的容忍度不一 | | | |
| S9 | **strict-kex 不相容**：舊設備不支援 `kex-strict-*` 擴充 | Terrapin 修補之後的相容性問題 | | | |
| S10 | **主機金鑰指紋**與使用者用 PuTTY 看到的**一致** | 兩邊算法要一樣（SHA256／MD5 都對一次） | | | |
| S11 | 中文輸出（Big5 或 UTF-8 的設備） | 遠端可能不是 UTF-8；目前原樣轉給 xterm | | | |
| S12 | 視窗大小改變後遠端跟著換行 | window-change 已驗（測試 sshd），真設備再確認 | | | |
| S13 | 關分頁的 Ctrl+D ×3 真的讓遠端登出（不是留著 session） | 有些設備要 `exit\r` | | | |
| S14 | 斷線之後的行為（拔網路線／設備重開） | 目前**沒有**自動重連（TASK-007） | | | |
| S15 | 連線閒置很久不會被切（保持連線） | 目前**沒有** keepalive（TASK-007） | | | |
| S16 | Pageant：把金鑰載進 Pageant 後不用打密碼就能連 | 程式已接但**沒有實機驗過**（測試 sshd 用的是金鑰檔那條路） | | | |

**請使用者提供**：每台設備的型號／韌體版本、對外的服務（SSH 版本字串，`nc host 22` 看得到）、
慣用的帳號驗證方式（密碼／金鑰／keyboard-interactive）、是否走非 22 埠。
**不要提供密碼**——驗證時由使用者自己輸入。
