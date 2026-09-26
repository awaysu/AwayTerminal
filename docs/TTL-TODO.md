# TTL 巨集：還沒做的指令（TASK-013 之後的狀態）

已實作的 121 個在 `docs/TTL.md` 第 4.1 節。這裡只列**還沒做的**，每一條都寫原因，
免得之後有人以為是漏掉的。

## 1. TASK-014（正規表示式）

| 指令 | 為什麼還沒做 |
|---|---|
| `strmatch` `strreplace` `waitregex` `regexoption` | 原碼用 **Oniguruma**；換成 Rust 的 `regex` crate 會有語法差異（後向參照、`\G`、POSIX 類別、可切換貪婪度…），Rust 的 `regex` 刻意不支援後向參照（為了線性時間保證）。**要先做一張差異表再選引擎**（`regex` vs `fancy-regex`），硬塞只會做出「看起來會動」的東西 |

## 2. 要等其他功能才有意義

| 指令 | 等什麼 |
|---|---|
| `sendbroadcast` `sendlnbroadcast` `sendmulticast` `sendlnmulticast` `setmulticastname` `wait4all` | TeraTerm 是「一個視窗一個行程」，這些是行程之間的廣播；我們是單一程式多分頁，**語意要重新定義**（要不要對所有分頁送？只對同一組？）→ 等 PM 決定 |
| `clipb2var` `var2clipb` | 剪貼簿：前端有 API，但要決定「巨集偷讀剪貼簿」是否要問使用者 |
| `callmenu` | 要有完整的選單 ID 表（舊版也沒有） |
| `setpassword` `getpassword` `ispassword` `delpassword`（含 `*2`） | 原碼把密碼加密存在 `.INI`（`ttmenc2.c` 是它自己的 XOR 式加密）。**不做那個格式**：弱加密會給使用者錯誤的安全感；要做應該接 OS 的憑證存放區（Windows Credential Manager），那是獨立的一個任務 |
| `loadkeymap` `restoresetup` `setdebug` `show` `showtt` `closett` `getttdir` `getttpos` `setdlgpos`（座標） | 都對映到 TeraTerm 的視窗／設定檔結構，我們沒有對應的東西（`setdlgpos` 已接但**忽略座標**——我們的對話框是頁內置中的） |
| `logstart` `logpause` `loginfo` `logrotate` `logautoclosemode` | 我們的 log 功能沒有「暫停／輪替」的概念（舊版也沒有）。`logopen`／`logwrite`／`logclose` 已經接上既有的 log |
| `setsync` `setecho` `enablekeyb` `sendbreak` `sendkcode` `setdate` `settime` `changedir`(已做) `beep`(已做) | `setecho`／`enablekeyb`／`setsync` 要終端機層的開關（我們沒有那些設定）；`sendbreak` 只有序列埠有意義（COM 的 break 還沒做）；`setdate`／`settime` 是**改系統時間**，不做 |
| `setbaud`／`setspeed` `setrts` `setdtr` `setflowctrl` `setserialdelaychar` `setserialdelayline` | 要 COM 後端多開幾個「連線中改設定」的入口（`serialport` 支援，但要想清楚和對話框設定的關係） |
| `exec` `execcmnd` | 跑外部程式。**要先決定沙盒模式下的規則**（`CLAUDE.md` 的沙盒是防呆，巨集能 `exec` 就繞過了）→ 等 PM 決定 |
| `getspecialfolder` `getfileattr` `setfileattr` `filelock` `fileunlock` `fileconcat` | `getspecialfolder` 是 Windows 的 CSIDL 表；`get/setfileattr` 是 Windows 的屬性位元；`filelock`／`fileunlock` 是 `LockFile` 那種區段鎖（跨平台語意不同）。都可以做，但都是 Windows 專屬的細節，等有人真的要用 |
| `crc16` `crc32` `checksum8/16/32`（含 `*file`） | 純計算，隨時可加；沒有使用案例就先不加（多一組要維護的表） |
| `gethostname` `getipv4addr` `getipv6addr` `uptime` `getmodemstatus` | 系統資訊，同上 |
| `bringupbox` | 把 statusbox 拉到最前面；我們的 statusbox 是右下角的常駐提示，不需要 |

## 3. 不做（有明確理由）

| 指令 | 為什麼不做 |
|---|---|
| `xmodemrecv/send` `ymodemrecv/send` `zmodemrecv/send` `kmtget/recv/send/finish` `bplusrecv/send` `quickvanrecv/send` | 老式檔案傳輸協定。**舊版 AwayTerminal 也沒有**，而且各自是一個完整的協定實作（zmodem 尤其）。等使用者說真的要用才做 |
| `scprecv` `scpsend` | SSH 的 SCP：`russh` 要自己實作 SCP 協定。等有需求 |
| `recvfile` `sendfile` | 同上（`sendfile` 是把檔案內容當鍵盤輸入送出去，倒是容易做——等需求） |
| `cygconnect` | Cygwin 的 `cygterm` 橋接，只有 Windows + Cygwin 有意義 |
| `unlink` `testlink`(已做) | `unlink` 是斷開 DDE 連結（我們沒有 DDE） |
| `setexitcode`(已做) `getver`(已做) | — |
| `ttmenc2.c` 的密碼加密檔格式 | 見上面的 `setpassword` |
| `dispstr` 的 Kanji 模式參數 | 原碼有 SJIS／EUC 的模式；我們一律 UTF-8（和整個程式一致） |

## 4. 已實作但有偏差的（完整表在 `docs/TTL.md` 4.3）

- `sprintf` 的浮點轉換 → `result=2` + 語法錯誤
- `gettime` 的時區參數 → `result=2`（會影響整個行程）
- `%Z` → 用數字偏移
- `random` → xorshift64* 而不是 SFMT
- `filestat` → 只給大小與修改時間
- `logopen` 的 binary／plainText／timestamp 參數 → 讀掉但不用（我們的 log 一律是去 ANSI 的文字，時間戳照設定）
- `clearscreen`／`beep` 的參數 → 讀掉但不用
- `setdlgpos` → 座標忽略
- `testlink` → 只回 2（連著）或 0（沒連著），沒有「有連線層但沒連上」那個中間狀態
- `setdir`／`changedir` → 只改**巨集自己的**目前目錄，不動行程的工作目錄
