# TTL 的正規表示式：Oniguruma vs Rust（差異表與引擎決定）

TeraTerm 的 `strmatch`／`strreplace`／`waitregex`／`regexoption` 用 **Oniguruma**
（預設 `ONIG_SYNTAX_RUBY`、UTF-8）。這份表是**實測**出來的（不是照記憶寫的）：
把每個語法丟給 `fancy-regex` 0.19.2 編譯並比對一個樣本字串，記下結果。
表裡的每一條都在 `src-tauri/src/ttl/regex.rs` 的單元測試 `oniguruma_feature_matrix`
裡釘住，改版本或換引擎時測試會先叫。

---

## 1. 引擎決定：**`fancy-regex`**

| 引擎 | 後向參照 `\1` | 後顧 `(?<=)` | 原子群組／佔有量詞 | `\G` `\K` | 條件式／遞迴 | 保證線性時間 |
|---|---|---|---|---|---|---|
| `regex` 1.13 | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ |
| **`fancy-regex` 0.19**（選這個） | ✅ | ✅（含變長） | ✅ | ✅ | ✅ | ❌（會回溯） |
| Oniguruma（原碼） | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |

理由（和 PM 的傾向一致）：

1. **設備 banner 的比對真的會用到後顧與後向參照**，`regex` 刻意不支援（為了線性時間保證），
   少了就等於「使用者原本能跑的 pattern 不能跑」——違反「舊版是行為下限」。
2. 回溯的風險（病態 pattern 吃 CPU）在這裡**可以接受**：巨集是使用者自己寫的、跑在自己的
   分頁執行緒上、而且隨時可以中斷（右鍵「執行巨集…」→ 停）。我們不是對外服務。
3. `fancy-regex` 是 MIT，內部就是 `regex` + 回溯層，沒有 C 依賴（Oniguruma 要編 C）。

**沒有反對理由**。唯一要留意的是下面第 4 節那兩個「Rust 語意不同」的地方——
其中 `^`／`$` 那一條我們**用預設開 `m` 旗標補回來**，所以使用者不會踩到。

## 2. 語法支援（實測，✅＝編譯過且命中）

| 語法 | 例 | fancy-regex |
|---|---|---|
| 群組、選擇、量詞 | `(\d+)-(\d+)` | ✅ |
| 懶惰量詞 | `a.*?b` | ✅ |
| POSIX 字元類 | `[[:alpha:]]` `[[:word:]]` | ✅ |
| 速記類 | `\d` `\w` `\s` | ✅ |
| Oniguruma 的 `\h`（十六進位數字） | `\h+` | ✅ |
| 換行類 `\R` | `a\Rb` | ✅ |
| 錨點 | `\A` `\z` `\Z` `\b` | ✅ |
| 非捕捉群組 | `(?:ab)+` | ✅ |
| 具名群組（兩種拼法） | `(?<n>…)` `(?P<n>…)` | ✅ |
| 行內旗標 | `(?i)` `(?m)` `(?s)` `(?x)` 與 `(?i:…)` | ✅ |
| **後向參照** | `(ab)\1`、`\k<x>` | ✅ |
| **前瞻／負前瞻** | `foo(?=bar)` `foo(?!bar)` | ✅ |
| **後顧／負後顧（含變長）** | `(?<=foo)bar` `(?<!foo)bar` `(?<=a+)b` | ✅ |
| **原子群組** | `(?>ab\|a)b` | ✅ |
| **佔有量詞** | `a++b` | ✅ |
| `\G`（上次結束的位置） | `\Gabc` | ✅ |
| `\K`（丟掉前面的比對） | `foo\Kbar` | ✅ |
| 條件式 | `(a)?(?(1)b\|c)` | ✅ |
| 遞迴 | `a\g<0>?` | ✅ |
| 缺席運算子 | `(?~abc)` | ✅ |
| Unicode 屬性／指令碼 | `\p{L}` `\p{Han}` | ✅ |
| 註解 | `a(?#comment)b` | ✅ |
| 十六進位轉義 | `\x41` | ✅ |
| **八進位轉義** | `\101` | ❌ 被當成「第 101 組的後向參照」 |
| **控制字元轉義** | `\cA` | ❌ `Invalid escape: \c` |

兩個不支援的都有替代寫法：`\101` → `\x41`、`\cA` → `\x01`。
兩者在設備比對裡極少見（要送控制字元通常是用 TTL 的 `#1` 字元碼，不是寫在 pattern 裡）。

## 3. `regexoption` 的對映

原碼可以設**編碼／語法／選項**三類。我們的對映：

| `regexoption` 的值 | Oniguruma 的意思 | 我們 |
|---|---|---|
| `IGNORECASE` | 忽略大小寫 | ✅ → Rust 的 `i` |
| `EXTEND` | 忽略空白與 `#` 註解 | ✅ → Rust 的 `x` |
| `MULTILINE` | **`.` 也吃換行** | ✅ → Rust 的 `s`（⚠️ 名字容易誤會，見下） |
| `SINGLELINE` | `^`→`\A`、`$`→`\Z`（**關掉**行錨點） | ✅ → 把 Rust 的 `m` 關掉 |
| `NEGATE_SINGLELINE` | 取消 SINGLELINE | ✅ → 把 `m` 開回來 |
| `FIND_NOT_EMPTY` | 不要回空比對 | ✅（自己實作：命中空字串就往後找） |
| `OPTION_NONE` | 全部清掉 | ✅（回到預設：`m` 開、其餘關） |
| `FIND_LONGEST` | 最長比對 | ❌ **不支援**：Rust 是 leftmost-first；`regex` 的 leftmost-longest 沒有透過 `fancy-regex` 開出來 |
| `DONT_CAPTURE_GROUP`／`CAPTURE_GROUP` | `(…)` 要不要捕捉 | ❌ 不支援 |
| `SYNTAX_*`（`POSIX_BASIC`／`EMACS`／`GREP`／`JAVA`／`PERL`…） | 換一整套語法 | ❌ 只有 Ruby／Perl 那一套（`fancy-regex` 的語法） |
| `ENCODING_*`（`SJIS`／`BIG5`／`EUC_*`／`UTF16`…） | 換比對用的編碼 | ❌ 只有 **UTF-8**（與 `ASCII`） |

**不支援的值不會讓巨集掛掉**：我們接受那個關鍵字、在終端機印一行黃字說「這個選項沒有支援」，
然後維持目前設定。理由是「舊巨集裡的一行 `regexoption` 不該讓整支巨集停掉」，
但也不能安靜忽略（使用者會以為生效了）。**認不出來的關鍵字**照原碼回語法錯誤。

⚠️ 名字的陷阱：Oniguruma 的 `MULTILINE` 是「`.` 吃換行」（等於 Perl 的 `/s`），
**不是** Perl 的 `/m`。行錨點在 Ruby 語法裡是**預設就開**的——所以我們預設開 Rust 的 `m`（見下）。

## 4. 語意不同的地方（各有一條測試釘住）

| 項目 | Oniguruma（Ruby 語法） | Rust 原本 | 我們怎麼處理 |
|---|---|---|---|
| **`^` `$`** | 一律是**行**錨點（`^b` 在 `"a\nb"` 命中） | 是**整段文字**的錨點（除非 `(?m)`） | **預設就開 `m`**，所以和原碼一樣。`regexoption 'SINGLELINE'` 會關掉它（等於原碼的語意） |
| `$` 在結尾換行前 | 命中（`"abc\n" =~ /c$/`） | 不命中 | 同上：開 `m` 之後 `$` 在 `\n` 前命中 |
| 比對策略 | leftmost-first（預設）／可切 longest | leftmost-first | 一樣；`FIND_LONGEST` 不支援 |
| 比對單位 | 位元組（可換編碼） | **字元（UTF-8）** | TTL 的字串是位元組；我們用 `String::from_utf8_lossy` 解成 UTF-8 再比對。**純 UTF-8 的內容完全一樣**；設備吐 Big5 之類的位元組時比對結果可能不同（見下） |
| 回傳位置 | 位元組位移（`strmatch` 是 1 起算） | 字元索引 | 我們回**位元組位移**（用 `Match::start()`，那本來就是位元組索引），所以和原碼一致 |
| 無效的 pattern | `onig_new` 失敗 → `-1` | 編譯錯誤 | 照原碼：`strmatch` 回 `result=0`、`strreplace` 回 `result=-1` |

### 非 UTF-8 位元組

TTL 的字串是位元組（`char[512]`）。設備吐 Big5／SJIS 時，`wait` 那條路（純位元組比對）
沒問題，但**正規表示式**要先解成 UTF-8：無效的位元組會變成 `U+FFFD`，所以
`waitregex` 對非 UTF-8 的中文**可能比對不到**。

- 舊版 AwayTerminal 的 `wait` 也是解成 .NET 的 UTF-16 字串（同樣的限制），
  所以**這不是退步**——而且舊版根本沒有 `waitregex`。
- 要真的支援，得換成位元組導向的引擎（`regex::bytes` 沒有後顧／後向參照）
  或自己做編碼轉換層。等使用者真的有 Big5 設備再處理（`docs/COM.md` T2／`docs/TELNET.md` T13
  已經有「Big5 設備」這一條待確認）。

## 5. 四個指令的行為（照原碼）

| 指令 | 語法 | 行為 |
|---|---|---|
| `strmatch` | `strmatch <字串> <pattern>` | `result` ＝命中位置（**1 起算的位元組位移**），沒命中 0，pattern 壞掉 0。`matchstr` ＝整個命中，`groupmatchstr1..9` ＝第 1～9 組（**比對前先清空**） |
| `strreplace` | `strreplace <字串變數> <從第幾個字> <pattern> <新字串>` | 從 `pos`（1 起算）之後找；`result` ＝ 1 換了／0 沒找到或 pos 超範圍／**-1 pattern 壞掉**。⚠️ **新字串是原樣插入的**（原碼沒有 `\1`／`$1` 展開），要用群組得自己讀 `groupmatchstr*` |
| `waitregex` | `waitregex <pattern…>`（最多 10 個） | 和 `wait` 一樣會等，但比對是**逐行**做的（收到 LF 時拿整行去比，同原碼 `FindRegexString`）。命中：`result` ＝第幾個 pattern（1 起算）、`inputstr` ＝那一行、`matchstr`／`groupmatchstr*` 也會設。逾時 `result=0` |
| `regexoption` | `regexoption <關鍵字…>` | 見第 3 節 |

`waitregex` 逐行比對是**原碼的行為**，不是我們簡化的：`Wait()` 只在收到 `0x0a` 時
呼叫 `FindRegexString()`，所以沒有換行的資料流不會觸發正規表示式的比對
（原碼在資料燒完之後會再試一次，我們也照做）。

## 6. 驗證

- `src-tauri/src/ttl/regex.rs` 的單元測試：語法矩陣（第 2 節每一條）、選項對映、
  四個指令的行為、`^`／`$` 的行錨點、非 UTF-8 的行為。
- `ttl_probe` 的正規表示式段：`waitregex` 對程式內 TCP server 跑一次（含群組與逾時）。
