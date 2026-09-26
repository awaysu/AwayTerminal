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
| 連線對話框：類型／IP 主機（可編輯下拉＋歷史）／Port（ssh 22）／保持連線（分鐘，0=關）／斷線自動重連 | `ConnectDialog.xaml` | 同（`src/sshdlg.js`，欄位表在第 7 節）＋多了帳號／金鑰／進階演算法／環境變數 | ✅ B6 |
| 保持連線 → `ServerAliveInterval = 分鐘×60`、`ServerAliveCountMax=3`，預設 10 分鐘 | `SshCommand` / `AppSettings.KeepAliveMins` | `keepalive_interval` + `keepalive_max = 3`，間隔取自 `keepAliveMins`（預設 10） | ✅ |
| 斷線自動重連：退避 3,6,9…最多 30 秒；有輸出就歸零；等待中按 Enter 立刻重連 | `ScheduleReconnect` / `ManualReconnect` | 同（`ssh/reconnect.rs`，對照表在第 5 節）；「歸零」的觸發點刻意不同，見下 | ✅ B5 |
| `-o SendEnv=CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN` | `SshCommand` | 同（對話框「進階」的環境變數清單 → `channel.set_env`，`request_shell` 之前逐條送） | ✅ B6 |

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
| 伺服器 banner | 驗證前顯示出來 | 同（`auth_banner`；只有 LF 的 banner 轉成 CR LF 才不會階梯狀） | ✅ |
| 演算法順序 | 自己的清單，舊演算法排後面；用到弱演算法會警告 | 同（四組清單見第 4 節；警告在 `kex_done` 比對實際協商結果） | ✅ |
| 每條連線覆蓋演算法 | 有 | `AlgoOverride` 已做，**但還沒有 UI**（B6） | 🟡 |

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

## 4. 演算法（B4，TASK-008 完成）

順序**照 PuTTY**：先強後弱，而且 PuTTY 有一條「warn below this line」——
線以下的演算法仍然在清單裡（舊設備連得上），但實際協商到它們時會跳警告。
程式在 `src-tauri/src/ssh/algos.rs`，四組清單都有單元測試釘住順序。

### 金鑰交換（kex）

| # | 名稱 | |
|---|---|---|
| 1 | `mlkem768x25519-sha256` | 後量子 hybrid（PuTTY 用 NTRU，語意相同） |
| 2 | `curve25519-sha256` | |
| 3 | `curve25519-sha256@libssh.org` | |
| 4-6 | `ecdh-sha2-nistp256` / `-384` / `-521` | |
| 7-10 | `diffie-hellman-group18-sha512` / `group17` / `group16` / `group15` | |
| 11 | `diffie-hellman-group-exchange-sha256` | |
| 12 | `diffie-hellman-group14-sha256` | |
| — | **↓ 以下是警告線（協商到就跳對話框）↓** | |
| 13 | `diffie-hellman-group14-sha1` | 舊設備常用 |
| 14 | `diffie-hellman-group-exchange-sha1` | |
| 15 | `diffie-hellman-group1-sha1` | 最舊 |

### 主機金鑰

`ssh-ed25519` → `ecdsa-sha2-nistp256/384/521` → `rsa-sha2-512` → `rsa-sha2-256`
→ **↓警告線↓** → `ssh-rsa`（RSA + SHA-1，很多舊網路設備只有這個）

### 加密（cipher）

`chacha20-poly1305@openssh.com` → `aes256-gcm@openssh.com` → `aes128-gcm@openssh.com`
→ `aes256-ctr` → `aes192-ctr` → `aes128-ctr`
→ **↓警告線↓** → `aes256-cbc` → `aes192-cbc` → `aes128-cbc` → `3des-cbc`

⚠️ **與 PuTTY 的差異**：PuTTY 的預設清單把 3DES 放在警告線**之上**（歷史原因）。
我們把所有 CBC 與 3DES 都放在線下——CBC 模式在 SSH 上有已知的攻擊面，3DES 的
64-bit 區塊也早就不該當預設。舊設備照樣連得上，只是會看到一次警告。

### 訊息驗證（MAC）

`hmac-sha2-256-etm@openssh.com` → `hmac-sha2-512-etm@openssh.com`
→ `hmac-sha2-256` → `hmac-sha2-512`
→ **↓警告線↓** → `hmac-sha1-etm@openssh.com` → `hmac-sha1`

（ETM＝encrypt-then-MAC，排在同演算法的非 ETM 之前，同 PuTTY 與 OpenSSH。）

### ⚠️ russh 缺哪些 PuTTY 有的演算法

**不是我們拿掉的，是 `russh` 沒有實作**：

| 類別 | 缺的 | 影響 |
|---|---|---|
| kex | NTRU Prime hybrid（`ntru-curve25519-sha512@tinyssh.org`） | 沒差，我們有 `mlkem768x25519-sha256`（後量子的標準版） |
| 主機金鑰 | **Ed448**、**`ssh-dss`（DSA）** | 只支援 DSA 的很舊設備**會連不上**。russh 有 `dsa` feature，要開才有（見下） |
| cipher | Blowfish、單 DES（`des-cbc`）、Arcfour | 只有 20 年前的設備需要；PuTTY 也把它們放在警告線下 |
| MAC | `hmac-md5`、`hmac-sha1-96` | 少數舊設備只有這兩個 → **可能連不上** |

`ssh-dss` 的取捨：russh 的 `dsa` feature 可以開，但 DSA（1024-bit）在 OpenSSH 8.8 就被
完全移除了。**先不開**，等使用者的設備清單真的出現只支援 DSA 的機器再說——
到時候只要在 `Cargo.toml` 的 russh features 加 `"dsa"`，再把 `Algorithm::Dsa` 排到最後。

### 每條連線可覆蓋

`AlgoOverride`（kex / hostKey / cipher / mac 四個字串陣列）存在連線設定裡，
空的就是用上面的預設順序。認不出來的名稱會被跳過並在終端機印一行黃字，
**一整組都認不出來時退回預設**（空清單會讓交握直接失敗，那不是使用者想要的）。

UI 在連線對話框的「進階」區（見第 7 節）；也可以手改 `settings.json`。

### 弱演算法警告

交握完成（`kex_done`）時比對**實際協商到**的四個名稱。有任何一個在警告線以下 → 跳對話框
（橘框、列出哪幾項、兩個選項：繼續連線／取消）。

- 使用者按「繼續連線」→ 記進 `settings.json` 的 `sshWeakAccepted`（`host:port`），**這台主機不再問**。
- 按「取消」→ 交握中止、連線結束（不會進到 shell）。
- 同一條連線只問一次（rekey 也會觸發 `kex_done`）。
- 前端沒回答時逾時 180 秒 → 當成取消（安全預設）。

## 5. keepalive 與斷線自動重連（B5）

### keepalive

`russh` 的 `keepalive_interval` + `keepalive_max = 3`，送的是 `keepalive@openssh.com`
global request（PuTTY 預設也是這條）。間隔取自 `settings.json` 的 `keepAliveMins`
（預設 **10 分鐘**，同舊版 `AppSettings.KeepAliveMins`；0＝關閉），
也可以由每條連線覆寫。

舊版是把它翻成 `ssh.exe` 的 `-o ServerAliveInterval=分鐘×60 -o ServerAliveCountMax=3`，
語意一樣。

### 斷線自動重連

程式在 `src-tauri/src/ssh/reconnect.rs`。分頁層記住 `SshConnParams`（**不含密碼**，見第 7 節），session 結束時排下一次重連。

| 舊版行為 | 出處（`MainWindow.xaml.cs`） | 新版 | 一樣嗎 |
|---|---|---|---|
| 退避 3、6、9…**上限 30 秒** | `ScheduleReconnect`：`Math.Min(30, 3 * attempt)` | `backoff_secs(attempt)`，同公式（單元測試釘住 3/6/9/27/30/30） | ✅ 一樣 |
| 沒勾「斷線自動重連」時只在畫面上提示，**按 Enter 才重連** | `OnSessionExit` 的 else 分支 | 同（灰字「連線已中斷。按 Enter 重新連線。」；`session_write_text` 收到含換行的輸入就觸發） | ✅ 一樣 |
| 等待重連時按 Enter **立刻**重連（不等退避跑完） | `ManualReconnect` | 同（`manual()` 會把排程的 generation 作廢再馬上連） | ✅ 一樣 |
| 重連前把舊畫面留著，接在同一個 buffer 後面 | 不清畫面 | 同（重連前先送 `b{id}` 把 scrollback 推上去，畫面不會被清） | ✅ 一樣 |
| 重連成功後退避次數歸零 | `OnSessionOutput` 第一行：**一收到輸出**就歸零 | **改成「shell channel 開成功」才歸零**（`OnConnected`） | ⚠️ 刻意不同 |
| 使用者自己關分頁 → 不重連 | `_closing` 旗標 | 同（分頁移除後 `on_exit` 找不到分頁就不排程） | ✅ 一樣 |
| 重連中又斷 → 次數繼續往上加 | 同上 | 同（實測退避次數 1 → 2） | ✅ 一樣 |

**為什麼「歸零」的觸發點要改**：舊版的輸出全部來自 `ssh.exe`，所以「有輸出」等於
「連上了」。新版是內建 SSH，**我們自己的狀態訊息**（「連線到 host:port …」、`login as:`、
錯誤訊息）走的是同一條輸出 callback ——照舊版寫會被誤判成「連上了」，退避永遠停在
第一次的 3 秒。這個是 `--verify` 實際抓出來的（`退避次數` 一直是 1），改成
「`request_shell` 成功」之後才變成 1 → 2。語意更精確，目的（連成功過就不要繼續拉長退避）一樣。

## 6. 自動驗證：`ssh_probe`

```
cd src-tauri
cargo run --example ssh_probe
```

**完全不靠網路、不連任何外部主機**：用 `russh` 自己的 server 端在同一支程式裡起一台測試
sshd（綁 `127.0.0.1` 的臨時埠），再用我們的 client 連上去。2026-09-27 實測 **16 PASS / 0 FAIL**：

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
| **只支援舊演算法的伺服器連得上**（B4） | 另起一台**只**接受 `group14-sha1` + `aes128-cbc` + `hmac-sha1` + `ssh-rsa` 的 sshd → 連得上（用 russh 預設清單的 client 會在交握就失敗） |
| **弱演算法警告有跳** | 同一次連線 decider 的 `accept_weak` 被呼叫 ≥1 次 |
| 舊演算法伺服器第二次連線 | 主機金鑰已記錄 → 被問 0 次，仍然連得上 |
| **弱演算法按取消 → 連線中止** | `accept_weak` 回 false → 交握中止、`on_exit` 有來、**沒有進到 shell** |
| **伺服器斷線 → session 正常結束** | 用 `RunningServerHandle::shutdown` 主動關掉伺服器 → client 的結束事件有來（不會掛住） |

`log_probe` 那種「環境問題」也一併分離得開：`ssh_probe` 不碰使用者的檔案，
`known_hosts` 寫在 `%TEMP%` 的專屬資料夾並在結束時刪掉。

### app 端路徑

`--verify` 會多連一次 `127.0.0.1:1`（**沒人在聽**的埠），驗
`session_create(kind:"ssh")` → 建分頁（`kind=ssh`、`backend=russh`）→
連不上時把原因印在終端機 → session 正常結束 → **重連排程**。同樣不碰外部主機。

2026-09-27 的實際輸出：

```
[verify] 分頁 3 建立：backend=russh title=127.0.0.1
[verify] 連不上時有把原因印在終端機：true
[verify] 分頁 kind=ssh（應為 ssh）
[verify] 提示按 Enter 重連：true
[verify] 自動重連有排程：true
[verify] 退避次數=1（第一次應為 1）
[verify] 退避次數變成 2（應 ≥2，代表重試過）
```

最後一行是重點：退避次數會往上加，代表重試真的發生了。第一次跑這段時它一直是 1，
挖出來的原因就是第 5 節寫的「歸零觸發點」。

### 舊演算法那一組的驗證缺口

測試 sshd 也是 `russh`，所以**只驗得到 russh 兩端都支援的舊演算法**。
`ssh-dss`、`hmac-md5`、`hmac-sha1-96`、Blowfish、單 DES、Arcfour 這些 russh 根本沒有，
所以**是 client-only 的缺口**：程式碼裡沒有它們，也無法用這個 probe 驗。
真的遇到只支援那些的設備時會連不上，那時要回頭看第 4 節的「russh 缺哪些」。

### 真連線

這台機器**沒有** Windows OpenSSH sshd（`Get-Service sshd` 查不到），所以真連線那一組
自動跳過並在 `ssh_probe` 的輸出印 `SKIP`。要跑的話：裝「OpenSSH 伺服器」選用功能、
啟動 `sshd` 服務，再用 `ssh_probe` 之外的手動步驟連 `127.0.0.1`。

---

## 7. 連線對話框（B6）

`src/sshdlg.js`。「新分頁 ▾ → SSH…」打開。上半照舊版 `ConnectDialog.xaml` 的欄位，
下半的「進階」是舊版沒有的（舊版這些只能靠 `ssh.exe` 的命令列參數）。

| 欄位 | 預設 | 對應 | 舊版有嗎 |
|---|---|---|---|
| 主機（可輸入 `host` 或 `host:port`，自動拆開） | 空 | `host` / `port` | ✅ 有（可編輯下拉＋歷史） |
| 埠 | 22 | `port` | ✅ 有 |
| 帳號（留白＝連上後在終端機問 `login as:`） | 空 | `user` | ⬜ 舊版沒有（帳號一律在終端機問） |
| 金鑰檔（`.ppk` 或 OpenSSH，按鈕選檔） | 空 | `keyPath` | ⬜ 舊版沒有 |
| 用 Pageant／ssh-agent | 關 | `useAgent` | ⬜ 舊版沒有 |
| 保持連線（分鐘，0＝關） | 10 | `keepaliveMins` | ✅ 有 |
| 斷線自動重連 | 跟著 `settings.json` 的 `autoReconnect` | `autoReconnect` | ✅ 有 |
| 進階 → 演算法四組（kex／主機金鑰／cipher／MAC），勾選、可拖曳排序，警告線以下標紅 | 全勾、順序同預設 | `algos` | ⬜ 舊版沒有 |
| 進階 → 環境變數（`名稱=值`，一行一條） | `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` | `env` | ✅ 有（固定寫死那一條） |
| 「加到我的最愛」 | — | `fav_add`（**不含密碼**） | ✅ 有 |

**密碼欄位刻意沒有**：舊版的連線對話框也沒有密碼欄（`ssh.exe` 自己問），
密碼一律在終端機當場問、不存檔。我的最愛存的是上表的欄位，沒有密碼。

演算法清單來自 `algo_catalog()`（四組 + 哪些在警告線下），對話框不自己寫死名稱。

### 還沒做的

| 項 | 內容 | 為什麼 |
|---|---|---|
| 主機歷史下拉 | 舊版主機欄是可編輯下拉，記最近連過的主機 | 我的最愛已經涵蓋「常連的」；歷史清單等使用者說要不要 |
| Telnet | 對話框的「類型」目前只有 SSH | Telnet 後端還沒做（階段 2 的後半） |

## 8. ⚠️ 待真機驗證（要使用者的設備清單）

`CLAUDE.md` 風險 3 的核心：**問題不只演算法**。下面每一項都要在使用者實際會連的設備上勾一次。
Agent-11 拿到設備清單之後逐台填。

| # | 要驗什麼 | 為什麼可能出問題 | 設備 A | 設備 B | 設備 C |
|---|---|---|---|---|---|
| S1 | 連得上、拿到 shell | 最基本 | | | |
| S2 | **kex**：`diffie-hellman-group1-sha1` / `group14-sha1` / `group-exchange-sha1` | ✅ TASK-008 已放進清單（警告線下），`ssh_probe` 用 `group14-sha1` 的伺服器驗過。真設備要再確認 `group1-sha1` 與 `group-exchange-sha1` | | | |
| S3 | **cipher**：`aes128-cbc` / `aes256-cbc` / `3des-cbc` | ✅ 已在清單（警告線下），`aes128-cbc` 驗過。`3des-cbc` 只有 `des` feature 提供，真設備要確認 | | | |
| S4 | **MAC**：`hmac-sha1` | ✅ 已在清單（警告線下），驗過 | | | |
| S5 | **hostkey**：`ssh-rsa`（SHA-1 簽章） | ✅ 已在清單（警告線下），驗過 | | | |
| S5b | **hostkey**：`ssh-dss`（DSA） | ⚠️ **russh 要開 `dsa` feature 才有，目前沒開** → 只支援 DSA 的設備會連不上。有這種設備請告訴我 | | | |
| S5c | **MAC**：`hmac-md5` / `hmac-sha1-96` | ⚠️ **russh 沒有實作** → 只支援這兩個的設備會連不上 | | | |
| S6 | **banner**：連線前的公告文字有沒有正確顯示 | 有些設備在驗證前送一大段 banner，換行／編碼可能怪 | | | |
| S7 | **keyboard-interactive**：有些設備用它代替密碼 | 提示文字、`echo` 旗標、多回合（含變更密碼流程） | | | |
| S8 | **不理 ext-info**：舊設備收到 `ext-info-c` 可能直接斷線 | russh 會送，舊設備的容忍度不一 | | | |
| S9 | **strict-kex 不相容**：舊設備不支援 `kex-strict-*` 擴充 | Terrapin 修補之後的相容性問題 | | | |
| S10 | **主機金鑰指紋**與使用者用 PuTTY 看到的**一致** | 兩邊算法要一樣（SHA256／MD5 都對一次） | | | |
| S11 | 中文輸出（Big5 或 UTF-8 的設備） | 遠端可能不是 UTF-8；目前原樣轉給 xterm | | | |
| S12 | 視窗大小改變後遠端跟著換行 | window-change 已驗（測試 sshd），真設備再確認 | | | |
| S13 | 關分頁的 Ctrl+D ×3 真的讓遠端登出（不是留著 session） | 有些設備要 `exit\r` | | | |
| S14 | 斷線之後的行為（拔網路線／設備重開） | ✅ 自動重連已做（退避 3/6/9…30 秒，等待中按 Enter 立刻重連）。**真設備要確認**：拔網路線時 russh 多久才發現斷線（沒有 keepalive 的話可能要等 TCP 超時） | | | |
| S15 | 連線閒置很久不會被切（保持連線） | ✅ keepalive 已做（`keepalive@openssh.com`，預設 10 分鐘）。**真設備要確認它認這條 global request**——有些舊設備不認，那時要改用 null packet | | | |
| S17 | 弱演算法警告的文案與時機 | 連舊設備時應該跳一次橘框，接受後同一台不再問 | | | |
| S18 | 伺服器的 banner 有正確顯示（含換行） | TASK-008 補了 `auth_banner`，只有 LF 的 banner 會被轉成 CR LF | | | |
| S16 | Pageant：把金鑰載進 Pageant 後不用打密碼就能連 | 程式已接但**沒有實機驗過**（測試 sshd 用的是金鑰檔那條路） | | | |

**請使用者提供**：每台設備的型號／韌體版本、對外的服務（SSH 版本字串，`nc host 22` 看得到）、
慣用的帳號驗證方式（密碼／金鑰／keyboard-interactive）、是否走非 22 埠。
**不要提供密碼**——驗證時由使用者自己輸入。
