# TTL 巨集（TeraTerm 的 `.ttl`）

`CLAUDE.md` 定的做法：**以 TeraTerm 開源的 `ttpmacro/` 原始碼為藍本逐段翻寫成 Rust**（BSD-3），
相容度要比舊版 AwayTerminal 的 C# 自寫版更完整。舊版 C# 版是**行為下限**：
舊版跑得動的巨集，新版一定要跑得動、結果一樣。

- 藍本：`reference/teraterm/teraterm/ttpmacro/`（`git clone --depth 1`，**不進 commit**；`reference/` 已在 `.gitignore`）
- 授權：BSD-3（見 `THIRD-PARTY-NOTICES.md`）
- 第一批（TASK-012）＝解析／運算式／變數／流程控制／不碰 I/O 的指令。
- 第二批（TASK-013）＝**執行入口、連線輸出入（`send`／`wait`）、對話框、檔案**。
- 正規表示式（`strmatch`／`strreplace`／`waitregex`／`regexoption`）是 **TASK-014**。

---

## 1. `ttpmacro/` → Rust 模組對照

| 原檔 | 行數 | 內容 | Rust |
|---|---|---|---|
| `ttmparse.h` | 403 | `Err*` 錯誤碼、`Rsv*` 保留字編號、`TVariableType` | `src/ttl/error.rs`、`src/ttl/words.rs`、`src/ttl/vars.rs` |
| `errdlg.cpp` | — | `DispErr` 的英文訊息 | `src/ttl/error.rs`（**原字照抄**） |
| `ttmparse.cpp` | 1982 | `CheckReservedWord`（211 個名稱） | `src/ttl/words.rs`（**由原碼抽出來生成**） |
| 同上 | | `GetFirstChar`／`GetIdentifier`／`GetReservedWord`／`GetOperator`／`GetLabelName`／`GetString`／`GetQuotedStr`／`GetCharByCode`／`GetNumber` | `src/ttl/lex.rs` |
| 同上 | | `GetFactor`→`EvalMultiplication`→…→`GetExpression`（11 層） | `src/ttl/expr.rs` |
| 同上 | | `Variables[]`、`CheckVar`／`NewIntVar`／`NewStrVar`／`NewIntAryVar`／`NewStrAryVar`／`NewLabVar`／`DelLabVar`／`SetIntVal`／`SetStrVal`／`GetIntVar`／`GetStrVar`／`GetIndex` | `src/ttl/vars.rs`＋`exec.rs` 的 `var_ref` |
| `ttmbuff.c` | 586 | `GetRawLine`／`GetNewLine`／`RegisterLabels`／`JumpToLabel`／`CallToLabel`／`ReturnFromSub`／`BuffInclude`／`SetForLoop`／`NextLoop`／`SetWhileLoop`／`BackToWhile`／`BreakLoop` | `src/ttl/exec.rs` |
| `ttl.cpp` | 6583 | `Exec`／`ExecCmnd` 的跳過旗標階梯與指令分派 | `src/ttl/exec.rs` |
| 同上 | | `TTLIf`／`TTLFor`／`TTLWhile`／`TTLDo`／`TTLLoop`／`TTLBreak`／`TTLGoto`／`TTLCall`／`TTLReturn`／`TTLInclude`… | `src/ttl/exec.rs` |
| 同上 | | `TTLStrLen`／`TTLStrCompare`／…／`TTLSprintf`／`TTLGetTime`／`BitRotate`… | `src/ttl/cmds.rs` |
| `ttmdlg.cpp`／`msgdlg.cpp`／`inpdlg.cpp`／`ListDlg.cpp`／`statdlg.cpp` | — | 對話框 | `src/ttl/io.rs`（Rust 端）＋`src/macro.js`（前端；**一個元件應付五種**） |
| `ttmdde.c`：`DDEOut`／`DDESend`／`Read1Byte`／`Wait`／`SetWait` | 1093 | 和 `ttermpro` 的 DDE 通訊與 `wait` 的比對 | `src/ttl/host.rs`（`MacroHost`／`RecvBuffer`／`WaitMatcher`）——`CLAUDE.md` 定的「DDE 改成程式內直接呼叫後端」 |
| `ttl.cpp`：`TTLSend`／`TTLWait`／`TTLPause`／`TTLConnect`… | | 會碰外界的指令 | `src/ttl/io.rs` |
| `ttl.cpp`：`TTLFile*`／`TTLFind*`／`TTLFolder*` | | 檔案指令 | `src/ttl/files.rs` |
| `ttmmain.cpp`：`IdTTLWait*` 的狀態機 | 735 | 「等」的驅動 | `src/ttl/io.rs` 的 `pump_until`（每 10ms 檢查中斷與逾時） |
| `MainWindow.MacroAction`（**舊版 AwayTerminal**） | | 執行入口 | `src/ttl/runner.rs`＋`src/macro.js` |
| `ttmdde.c`／`wait4all.c`／`ttmenc2.c` | — | 和 `ttermpro` 的 DDE 通訊、多視窗等待、密碼加密 | **不做**（DDE 改成程式內直接呼叫連線後端，見 `CLAUDE.md`） |
| `ttmlib.c`／`ttmmain.cpp`／`ttmacro.cpp`／`ttl_gui.cpp` | — | Win32 的進入點與輔助 | 不需要（Tauri 這邊自己的入口，第二批做） |

## 2. 執行模型（可暫停的狀態機）

原碼不是「先建 AST 再走一遍」，而是**每次讀一行、執行一行**，流程控制靠幾個「跳過」旗標
加一個小堆疊。我們照抄這個模型，因為：

1. 它就是相容性的來源（跳過的規則、巢狀計數、`ParseAgain` 都會影響行為）；
2. **一步＝一行**，所以第二批的 `wait`／`pause` 只要讓 `Interp::step()` 回「還在等」
   就能掛起，不必把直譯器改成另一種寫法；鍵盤打斷也一樣。

| 原碼全域 | 意思 | Rust |
|---|---|---|
| `IfNest` | 目前在幾層 `if…endif` 裡 | `Interp::if_nest` |
| `ElseFlag` | 往前找 `else`／`elseif`／`endif` | `else_flag` |
| `EndIfFlag` | 往前找 `endif` | `end_if_flag` |
| `EndWhileFlag` | 往前找迴圈結尾 | `end_while_flag` |
| `BreakFlag`／`ContinueFlag` | `break`／`continue` 往前找迴圈結尾 | 同名 |
| `NextFlag` | `next` 跳回 `for` 時的「續跑」標記 | `next_flag` |
| `ParseAgain` | 同一行還沒處理完（單行 `if`） | `parse_again` |
| `PtrStack`／`TypeStack`（`MAXSP` 10） | call／for／while 堆疊 | `stack: Vec<Ctl>` |
| `Buff[]`／`BuffPtr[]`（`MAXNESTLEVEL` 10） | include 的每一層 | `buffers: Vec<Buffer>` |

**一個刻意的簡化**：原碼的跳躍位置是「檔案內的位元組偏移」，我們用**行索引**。
`GetRawLine` 永遠從行開頭讀，兩者等價。

## 3. 語法：照原碼的幾個重點

### 3.1 一行的結構

```
[:標籤]
指令 參數…            ; 分號到行尾是註解
變數 = 運算式          /* 這種註解也可以，而且能跨行 */
```

- 識別字：`[A-Za-z_][A-Za-z0-9_]*`，**超過 31 個字的部分丟掉**（`MaxNameLen` 32）。
- 大小寫**全部不敏感**（指令、變數、標籤都是）。
- 一行最長 1023 位元組（`MaxLineLen`），超過的**丟掉**（不是折到下一行）。
- 字串最長 511 位元組（`MaxStrLen`）。
- **沒有續行符號**。

### 3.2 數字與字串

| 寫法 | 意思 |
|---|---|
| `123` | 十進位 |
| `$ff` | 十六進位（**不是** `0x`；`$` 後面沒有數字＝0） |
| `'abc'` `"abc"` | 字串（**沒有反斜線轉義**——`'a\nb'` 是 4 個字元） |
| `#65` `#$41` | 字元碼（值必須 1～255，否則語法錯誤） |
| `'ab'#33"cd"` | 相鄰的片段會接起來＝`ab!cd`。**中間有空白就不算**（見下） |

⚠️ 片段之間**不能有空白**：`'ab' #33` 是「字串 `ab`」後面再一個參數 `#33`，
因為原碼看的是收尾引號**緊接著**的那個字元。

### 3.3 運算式：11 層優先權（1 最緊）

| 層 | 運算子 | 備註 |
|---|---|---|
| 1 | 常值、變數、`( )`、單元 `+ - ~ !` 與 `not` | |
| 2 | `* / %` | `/` 與 `%` 對 0 都是 `Divide by zero.` |
| 3 | `+ -` | **不是字串相接**（要用 `strconcat`） |
| 4 | `<< >> >>>` | `>>>` ＝邏輯右移 |
| 5 | `&` / `and` | |
| 6 | `^` / `xor` | |
| 7 | `\|` / `or` | |
| 8 | `< > <= >=` | |
| 9 | `= == <> !=` | |
| 10 | `&&` | |
| 11 | `\|\|` | |

⚠️ **三件和 C 不一樣的事**：

1. **位元運算（5～7）比比較運算（8～9）緊**。所以 `0 = 2 & 1` 是「先算 `2&1`＝0，
   再比 `0 = 0`」＝1；照 C 的讀法會是 `(0=2) & 1`＝0。
2. **`and`／`or`／`xor`／`not` 是位元運算**，邏輯運算是 `&& || !`。
   （舊版 C# 版把 `and`／`or` 當邏輯運算，見第 6 節。）
3. 整數是 **32-bit 有號**、溢位環繞；`$FFFFFFFF` ＝ -1。

移位的邊界行為也照原碼那個梯子：位移量是負的就**反向**，`>= 32` 飽和成 0
（算術右移負數是 -1）。

### 3.4 參數是「運算式」——常踩的雷

`strcopy 'abc' 1 -5 t` 裡的 `1 -5` 會被讀成 **`1-5`＝-4**，不是兩個參數。
原碼的 `GetIntVal` 也是讀整個運算式，所以這是 TeraTerm 本來就有的行為。
要傳負數請加括號：`strcopy 'abc' 1 (-5) t`。

### 3.5 變數

- 型別：整數、字串、整數陣列、字串陣列、標籤。**型別不能換**
  （`a = 1` 之後 `a = 'x'` 是 `Type mismatch.`）。
- **標籤和變數共用名稱空間**（原碼是同一張表），所以 `:foo` 之後就不能有變數 `foo`。
- 指令的「目標變數」**不存在就自動建**（原碼 `GetIntVar`／`GetStrVar`），
  所以 `int2str istr i` 不必先宣告 `istr`。
- 陣列要先 `intdim`／`strdim`，大小 <= 0 或重複宣告都是語法錯誤；索引 0 起算，
  超範圍是 `Index out of range.`。
- 系統變數（照 `InitVar`）：`result`、`timeout`、`mtimeout`、`inputstr`、
  `matchstr`、`groupmatchstr1`～`groupmatchstr9`。
  `param1..N`／`paramcnt` 要等第二批的執行入口（要有命令列參數才有意義）。

## 3.6 執行入口（TASK-013）

| 舊版行為 | 出處 | 新版 | 一樣嗎 |
|---|---|---|---|
| 分頁右鍵「執行巨集…」 | `MainWindow.xaml` 的 `MenuMacro_Click` | 同（分頁右鍵選單） | ✅ |
| 檔案選擇：`TeraTerm 巨集 (*.ttl)`／`所有檔案` | `MacroAction` 的 `OpenFileDialog` | 同（`macro_pick_file`） | ✅ |
| 已經在跑 → 問「要停止巨集嗎？」，是 → 停 | 同上 | 同 | ✅ |
| 讀檔失敗 → 跳「無法讀取巨集：」 | 同上 | 同（`macro_run` 回 Err → 前端對話框） | ✅ |
| 巨集在背景執行緒跑 | `Task.Run(RunAsync)` | 每個分頁一條專用執行緒 | ✅ |
| `messagebox`／`yesnobox`／`inputbox` 交給 UI | 三個事件 | `macro-dialog` event → 前端 → `macro_answer` | ✅ |
| 分頁關閉／程式結束 → 停巨集 | `CloseTab`／`OnClosed` | `tab_close` 呼叫 `stop_for_tab` | ✅ |
| 執行中的狀態 | `IsMacroRunning`（只影響 tooltip） | tooltip **＋分頁列一個黃色 `M`**（檔名與目前行號在 tooltip） | ⬜ **新增** |
| 巨集結束 | 靜靜結束 | 畫面上一行灰字「[巨集執行完畢：檔名]」；中斷是「[巨集已中斷：…]」；錯誤是紅字＋對話框 | ⬜ **新增**（舊版只有錯誤對話框） |
| 執行中使用者打字 | **照樣送給連線**（`MacroRunner` 沒有攔鍵盤） | 同（`IoTap::on_input` 一律放行） | ✅ |

中斷：右鍵選單再點一次「執行巨集…」→ 問「要停止巨集嗎？」。正在 `wait`／`pause`
的巨集會在 10ms 內停下來（`ttl_probe` 實測 150ms 內結束，逾時設 30 秒也一樣）。

## 3.7 「等」與接收緩衝

| 項目 | 原碼 | 我們 |
|---|---|---|
| 資料來源 | `ttermpro` 透過 DDE 送過來 | `IoTap::on_output`（TASK-011 留的接縫）→ `RecvBuffer` |
| `wait` 的比對 | 原始位元組，**含 ANSI escape** | **先去掉 ANSI** 再比對 |
| 緩衝上限 | 環形緩衝 | 400KB，滿了砍成 200KB |
| 逾時 | `timeout`×1000 + `mtimeout` 毫秒，0＝永遠等 | 同 |
| 候選數 | 最多 10 個，**同時命中時索引小的贏** | 同（`WaitMatcher`，有測試） |
| `waitln`／`recvln` 的 `inputstr` | `RecvLnBuff`（下一行的第一個位元組才清前一行） | 同 |

⚠️ **「去掉 ANSI」是刻意和原碼不同、照舊版 AwayTerminal**：原碼比對原始位元組，
所以遇到有顏色的提示字元（`[32m$[0m`）會比不到；舊版 C# 版先去 ANSI，
使用者的巨集是照那個行為寫的。`ttl_probe` 有一條就是在驗「有顏色的 `login:` 也比對得到」。

## 4. 指令清單（對照原碼的 211 個保留字）

### 4.1 已實作（121 個）

第二批（TASK-013）新增的：

| 類別 | 指令 |
|---|---|
| 連線輸出入 | `send` `sendln` `dispstr`、`wait` `waitln` `waitn` `recvln` `flushrecv` |
| 暫停 | `pause` `mpause` |
| 終端機 | `beep` `clearscreen` `settitle` `gettitle` |
| log | `logopen` `logwrite` `logclose` |
| 連線控制 | `connect` `disconnect` `testlink` |
| 對話框 | `messagebox` `yesnobox` `inputbox` `passwordbox` `statusbox` `closesbox` `listbox` `filenamebox` `dirnamebox` `setdlgpos` |
| 檔案 | `fileopen` `fileclose` `fileread` `filereadln` `filewrite` `filewriteln` `fileseek` `fileseekback` `filemarkptr` `filestrseek` `filestrseek2` `filetruncate` `filesearch` `filecreate` `filedelete` `filerename` `filecopy` `filestat` |
| 資料夾與搜尋 | `foldercreate` `folderdelete` `foldersearch` `findfirst` `findnext` `findclose` `getdir` `setdir` `changedir` |

第一批（TASK-012）的 60 個：

| 類別 | 指令 |
|---|---|
| 流程控制 | `if` `then` `else` `elseif` `endif`（含**單行式** `if <cond> <指令>`）、`for` `next`、`while` `endwhile`、`until` `enduntil`、`do` `loop`（`do while`／`do until`／`loop while`／`loop until`）、`break` `continue`、`goto` `call` `return`、`end` `exit`、`include`、`ifdefined` |
| 陣列 | `intdim` `strdim` |
| 字串 | `strlen` `strcompare` `strconcat` `strcopy` `strscan` `strinsert` `strremove` `strtrim` `strsplit` `strjoin` `strspecial` `tolower` `toupper` |
| 整數 ↔ 字串 | `int2str` `str2int` `code2str` `str2code` `sprintf` `sprintf2` |
| 數值 | `random` `rotateleft` `rotateright` |
| 時間 | `gettime` `getdate` |
| 環境與路徑 | `getenv` `setenv` `expandenv` `basename` `dirname` `makepath` |
| 其他 | `setexitcode` `getver` |
| 運算子（字詞） | `and` `or` `xor` `not` |

### 4.2 還沒做的

完整清單（每一條都有原因）在 **`docs/TTL-TODO.md`**。摘要：

| 分類 | 代表指令 | 狀態 |
|---|---|---|
| 正規表示式 | `strmatch` `strreplace` `waitregex` `regexoption` | **TASK-014**：原碼用 Oniguruma，換 Rust 的 `regex` 有語法差異（無後向參照），要先出差異表再選引擎 |
| 多視窗廣播 | `sendbroadcast` `sendmulticast` `wait4all`… | 等 PM 定語意（TeraTerm 是多行程，我們是單程式多分頁） |
| 密碼存放 | `setpassword` `getpassword`… | **不做原碼的格式**（`ttmenc2.c` 是弱加密，會給錯誤的安全感）；要做應接 OS 憑證存放區 |
| 檔案傳輸 | `xmodem*` `zmodem*` `kmt*` `scp*` | 舊版也沒有，各自是完整協定 → 等需求 |
| 外部程式 | `exec` `execcmnd` | 等 PM 決定沙盒模式下的規則（能 `exec` 就繞過沙盒了） |
| 終端機／設定 | `setecho` `enablekeyb` `setbaud` `loadkeymap`… | 要對應的後端開關 |
| Windows 專屬細節 | `getspecialfolder` `get/setfileattr` `filelock` | 可以做，等有人要用 |
| 雜項計算 | `crc32` `checksum*` `gethostname`… | 純計算，沒有使用案例先不加 |

**沒實作的指令仍然是保留字**：執行到會回 `Unknown command.`（`ErrNotSupported`），
不會被誤認成變數名，也不會安靜跳過。

### 4.3 已知偏差（都刻意，都有測試釘住）

| 項目 | 原碼 | 我們 | 為什麼 |
|---|---|---|---|
| `sprintf` 的浮點轉換（`%e %f %g %a`） | 丟給 C 的 `snprintf` | **不支援**，回 `result=2` + 語法錯誤 | TTL 沒有浮點型別，參數只能是整數；要模擬 C 的浮點格式化得先決定「整數怎麼變 double」，不想在沒有使用案例時猜 |
| `gettime`／`getdate` 的第三個參數（時區） | 改 `TZ` 環境變數 + `tzset()` | **不支援**，回 `result=2` | 那會影響**整個行程**（我們是多分頁的 app，會弄到其他分頁）。要做得用 `chrono-tz` 之類的函式庫 |
| `%Z`（時區名稱） | C 的時區縮寫 | 用 `+0800` 這種偏移 | 跨平台拿不到一致的縮寫 |
| `strftime` 的 `#` 修飾詞 | MSVC 的「去前導零／長格式」 | 接受但**忽略** | 只有 MSVC 有；影響很小 |
| `random` 的亂數源 | SFMT（Mersenne Twister 族） | xorshift64*（種子取自系統時間） | 巨集不需要密碼學等級的亂數；範圍與含端點的行為一樣 |
| `setenv` | `_putenv_s`（只影響自己的行程） | 同 | ⚠️ 我們是多分頁的 app → 會影響**之後開的分頁** |
| `wait` 的比對對象 | 原始位元組（含 ANSI） | **先去掉 ANSI**（照舊版 AwayTerminal） | 有顏色的提示字元原碼比不到；使用者的巨集是照舊版的行為寫的。見 3.7 |
| `filestat` | 大小／時間／屬性，時間格式可選 | 只給**大小**與 `yyyy-mm-dd hh:mm:ss` 的修改時間 | 其餘欄位沒有使用案例 |
| `testlink` | 2／1／0 三種狀態 | 只有 2（連著）與 0 | 我們沒有「有連線層但沒連上」那個中間狀態 |
| `setdir`／`changedir` | `SetCurrentDirectory`（影響整個行程） | **只改巨集自己的目前目錄** | 多分頁的 app 不能讓一支巨集改掉別人的工作目錄 |
| `logopen` 的 binary／plainText／timestamp 參數 | 各自有效 | 讀掉但**不用** | 我們的 log 一律是「去 ANSI 的文字」，時間戳照設定（見 `logging.rs`） |

## 5. 錯誤

錯誤碼與英文訊息**逐條照 `ttmparse.h` + `errdlg.cpp`**（連 `Label requiered.` 的拼字錯誤
一起保留——那是使用者可能搜過的字串）。每個錯誤帶：行號、檔名、該行原文、
以及 token 的起迄位置（原碼的錯誤對話框就是用這幾個欄位把出錯的那一段標起來的）。
繁中訊息是新版多的（`Err::message_zh`），給之後的對話框用。

## 6. 舊版 C# 版（`Macros/MacroRunner.cs`，764 行）相容清單

舊版支援的東西，新版**全部都有**且行為一致（`tests/ttl/oldversion.ttl` 把舊版
`samples/sample.ttl` 裡不碰連線的部分原樣跑了一次）：

| 舊版支援 | 新版 |
|---|---|
| `:label` `goto` `call` `return` | ✅ |
| 單行式 `if <expr> <cmd>`、區塊式 `if..then/elseif/else/endif` | ✅ |
| `for..next`、`while..endwhile`、`break`、`continue` | ✅（另外還有 `do/loop`、`until/enduntil`） |
| `end` `exit` | ✅ |
| 變數與整數運算式（`+ - * / % & | ^ ~ << >> = <> < <= > >= && ||`） | ✅ |
| 字串 `'x'`／`"x"`、字元碼 `#nn` | ✅（另外支援 `#$hh` 與片段相接） |
| `strlen` `strconcat` `strcopy` `str2int` `int2str` `strcompare` `tolower` `toupper` `sprintf` | ✅（另外多了 12 個 `str*`、`sprintf2`、`code2str`／`str2code`） |
| 系統變數 `result` `inputstr` `timeout` | ✅（另外有 `mtimeout`／`matchstr`／`groupmatchstr1..9`） |
| 通訊／對話框（`send` `wait` `messagebox`…） | 第二批 |

### ⚠️ 三個「舊版不一樣」的地方（新版照 TeraTerm）

| 項目 | 舊版 C# | 新版（＝TeraTerm） | 影響 |
|---|---|---|---|
| `and`／`or` | **邏輯**運算 | **位元**運算 | 兩邊都是 0／1 時結果一樣（比較運算的結果就是 0／1），所以一般巨集看不出差別；`2 and 1` 舊版是 1、新版是 0 |
| 運算子優先權 | `\|\| && \| ^ & = 比較 移位 + - * /`（C 的排法） | 位元運算**比**比較運算緊（見 3.3） | `a = 1 and b = 1` 這種寫法兩版讀法不同。**照 TeraTerm 才是對的**（`CLAUDE.md` 定的基準） |
| 整數寬度 | 64-bit（C# `long`） | **32-bit**（同原碼的 `int`） | 超過 21 億的運算會環繞。舊版巨集若靠 64-bit 會有差 |
| `break`／`continue` 在單行式 `if` 裡 | 舊版 README 說**不支援** | 支援 | 新版比較寬 |

舊版還有一個**自己的限制**：它的 `wait` 只比對純文字、`strmatch` 沒有；新版第二批會照
TeraTerm 做完整的 `waitregex`／`strmatch`。

## 7. 驗證

### 單元測試

`cargo test`：**201 個**（TASK-012 是 162），其中 TTL 相關 125 個。
每個指令至少一例，期望值取自原碼（有些直接引用原碼的條件式寫在註解裡）。

### `ttl_probe`

```
cd src-tauri && cargo run --example ttl_probe
```

跑 `src-tauri/tests/ttl/` 底下的 `.ttl` 檔，逐個變數比對。2026-09-27 的結果：
**27 PASS / 0 FAIL**（124 個變數檢查 + 15 個錯誤案例 + I/O 那幾段）。

| 檔案 | 驗什麼 |
|---|---|
| `expr.ttl` | 42 個運算式（優先權、位元 vs 比較、字詞運算子、移位邊界、32-bit 環繞、字串片段、字元碼、中文位元組數） |
| `flow.ttl` | 26 個流程控制（if/else/elseif、巢狀、單行式、for 遞增／遞減／單圈、巢狀 for、while、until、do/loop 三種、break／continue／巢狀 break、goto 前後跳、call/return） |
| `strings.ttl` | 47 個字串／整數指令 + 陣列 + `ifdefined` + rotate/random + 路徑 |
| `inc_main.ttl` + `inc_lib.ttl` | `include`：變數看得到、被 include 的檔跑完會回來、它的標籤只在它自己那一層 |
| `oldversion.ttl` | 舊版 `samples/sample.ttl` 不碰連線的部分**原樣**跑一次 |
| `errors.ttl` | 錯誤的**行號**與檔名 |
| `sample_full.ttl` | 舊版 `samples/sample.ttl` **整檔**（含 `sendln`／`wait`／`messagebox`），對程式內的 TCP echo server 跑 |
| （I/O 段） | `wait`（含 ANSI 顏色的提示字元）／多候選誰先命中／逾時 `result=0`／`sendln` 真的送出去／`yesnobox`／`inputbox` 的回傳值／**中斷正在 wait 的巨集**（150ms 內停）／沒有連線時回 `Link macro first.` |
| （inline） | 15 個錯誤案例：`")" expected.`／`Divide by zero.`／`Variable not initialized.`／`Type mismatch.`／`Invalid control.`（endif／break／return）／`Label requiered.`／`Label already defined.`／`Syntax error.`／`Index out of range.`／`Unknown command.` |

## 8. 下一批（TASK-014）的接法

1. `wait`／`pause` 這類要等的指令：讓 `Interp::step()` 回「還在等」，
   呼叫端（分頁的巨集執行器）隔一段時間再 `step()`。**直譯器不需要改結構**。
2. 輸出流的比對（`wait`／`waitln`／`waitregex`）走 `src/tap.rs` 的 `IoTap::on_output`；
   `send`／`sendln` 走 `session_write`（`IoTap::on_input` 可以讓巨集執行中吃掉鍵盤）。
3. 對話框：照第一批的做法在前端做頁內對話框，Rust 端等回覆的 command **一定要 `async`**
   （見 `docs/REGRESSION-CHECKLIST.md` 的「隱含契約」）。
4. 正規表示式：先做一張 Oniguruma vs Rust `regex` 的語法差異表再決定用哪個
   （或包一層讓常見寫法一致）。
