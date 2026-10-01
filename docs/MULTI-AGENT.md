# 代理團隊（Multi-Agent）

2～4 個互動 AI CLI（ClaudeCode／Codex／OpenCode／GeminiCLI）各帶一個角色，用專案裡的
`.ai/bus/` 資料夾互寄信，AwayTerminal 看到新信就在**收件人閒置時**把「請讀 …」那一行
打進它的終端機。

搬移自舊版 v1.2.8 的 `Services/MultiAgent/`、`Models/Agent*`、`MainWindow.MultiAgent.cs`
與 `Dialogs/MultiAgentDialog`。**這份文件只寫行為與刻意的差異**；每一段的「為什麼」都在
對應的 Rust 原始碼註解裡（大多是舊版某一次實測踩出來的）。

---

## 1. 一個團隊＝N 個獨立分頁

刻意沿用舊版的做法：**不另做「一個分頁多個 session」的模型**。每個 agent 就是一個一般分頁
（session／xterm／log／恢復畫面都照一般分頁走），綁組只是薄薄一層：

- 前端用 `g` 協定把它們的 pane 排成「**下一上 N−1**」：下方全寬那格＝Agent-x1（使用者對話
  的那一格），上列由左到右是 x2…x4，中間的分隔線可上下拖（拖完 `G` 協定回報比例、雙擊回 0.5）。
- 右側分頁列**一組只顯示一列**（代表列＝格號最小、有分頁的那格）。點那一列會回到
  「最後點過的那一格」。
- 外框顏色固定：格 1 淡紅 `#EF9A9A`、2 淡藍 `#90CAF9`、3 淡綠 `#A5D6A7`、4 淡紫 `#CE93D8`。
- pane 標題＝`Agent-12 · Software Engineer · Codex`。

Agent ID ＝ `Agent-{組號}{格號}`，組號 1～9（最多同時 9 組），開組時取目前沒用到的最小號。

## 2. 建團隊

「新分頁 ▾ → 代理團隊…」→ **先選專案資料夾**（同其他「啟動前選擇資料夾」的連線）→ 設定視窗：

| 欄位 | 預設 | 說明 |
|---|---|---|
| 投遞限制次數 | 50 | 投遞這麼多則就暫停（10／30／50／100／不限）。防 agent 互踢無限迴圈燒 token。舊版預設 30，2026-10-01 使用者改成 50 |
| 閒置檢查 | 30 分鐘 | 整組閒置這麼久就請 Agent-x1 問大家狀況（15／30／60／不檢查） |
| **沙盒** | **開** | 新版才有，見第 6 節 |
| 每格：啟用 | 格 1、2 | 格 1 一定啟用（使用者就是要跟它說話），不能取消 |
| 每格：代理人類型 | 第一個找得到的 | ClaudeCode／Codex／OpenCode／GeminiCLI；**這台沒裝的不列** |
| 每格：代理人角色 | PM／SE／Architect／QA | `roles/*.md` 的檔名，可自己加 |

底下兩個按鈕：**還原角色檔預設**、**開啟角色檔資料夾**。

### 啟動流程（為什麼要前端配合）

每個 agent 的 PTY 輸出走各自的 tauri `Channel`，而 `Channel` **只能由前端建**，所以：

1. `agent_team_create(setup)`：Rust 決定組號 → 開沙盒 → 寫護欄 → `.gitignore` →
   **組好每一格的角色檔**（名單要完整才組得對，所以先全部填好再一次組）→ 回傳啟動計畫。
2. 前端照計畫逐格 `session_create({ kind: 'agent', agent: { team, index } })`。
   連線、參數、環境變數、Job Object 都是 Rust 依計畫決定的。
3. `agent_team_ready(key)`：綁組、送 `g`、開始監看 `.ai/bus/`。

一格啟動失敗（CLI 找不到）→ `agent_slot_failed`：那一格從名單裡拿掉、**其餘照開**，
並把剩下幾格的角色檔重組（隊友名單要對）。全部失敗＝不開團隊。

## 3. 角色檔（三層）

每個 agent 拿到**一份檔**（`<設定資料夾>/multiagent/sessions/<組號>/Agent-xx.md`）：

1. `common.md`（所有人都有）
2. `roles/<角色>.md`（None＝略過）
3. **執行期脈絡**（AwayTerminal 產生）

範本嵌在執行檔裡，第一次用到時複製到 `<設定資料夾>/multiagent/` 讓使用者自己改。
`.defaults.json` 記「AwayTerminal 最後寫進去的內容雜湊」（去 BOM、CRLF→LF 之後的 SHA-256）：

- 現在的雜湊＝記錄值 → 使用者沒改過 → 範本變了就**自動換新版**
- 不一樣 → 使用者改過 → **不動**

（舊版實錄：加 UI/UX Designer 時改了內嵌的 `product-manager.md`，但使用者資料目錄裡那份
從沒改過的舊版一直沒換，PM 不知道有設計師。）

角色檔與 `common.md` **不套八語**——那是給 agent 讀的，不是 UI。UI 文字才八語。

### 執行期脈絡

`roles.rs` 的 `runtime_context()`，**逐字照舊版 `RoleLibrary.RuntimeContext`**，只換執行時的值。
它有這幾段（每一段都是舊版某一次實測加上去的，註解裡寫了原因）：

| 段 | 為什麼有它 |
|---|---|
| Agent ID／角色／provider／團隊／專案目錄／啟用名單／Mailbox | 基本身分 |
| `## Your teammates are separate terminals, not sub-agents` | Codex 內建 `spawn_agent`，使用者說「讓 Agent-12 做…」時它直接開子代理、信箱完全沒用到 |
| `## How to send a message` | 檔名編號規則、front matter、「寫完檔就結束回合」 |
| `### Delivery timing` | PM 寄了暫停信就跟使用者說「已通知暫停」，但收件人正在工作、信在排隊 |
| `## How you receive messages` | 通知那一行的樣子、**同一封晚到不要重做**、訊息檔是 UTF-8（PowerShell 5.1 讀中文會亂碼） |
| `## Always report to Agent-x1` / `## Reports from your teammates` | worker 的任務不管怎麼結束都要回報，否則 PM 不知道它停在哪 |
| `## Design before UI implementation` | **只在有開 UI/UX Designer 且自己是 PM／格 1 時才有** |
| `## Shared desktop` | 三個終端機＋使用者共用一個桌面，GUI 焦點測試注定不穩 |
| `## When you are stuck` | 兩種做法都失敗或超過 10 分鐘就停手回報 |
| `## Talking to the user` | worker 只把結果寫進回信、自己畫面沒顯示，使用者在那格看不到 |
| Solo Mode | 只有一格啟用時多一行 |

**回歸測試**：`resources/multiagent/runtime-context.txt` 是**舊版真的產生出來的那一份**
（從 v1.2.8 執行中的 `Agent-12.md` 抓出來的），`roles.rs` 的
`runtime_context_matches_the_v1_template` 逐字比對它。改壞了測試會紅。

## 4. 角色怎麼交給 CLI

| CLI | 做法 | 為什麼 |
|---|---|---|
| ClaudeCode | `--append-system-prompt-file "<角色檔>"` | 附加在預設系統提示後（舊版 1.1.11 實跑驗證） |
| Codex | `-c tui.whimsy=false -c "developer_instructions='…整份角色檔…'"` | 只有字串版、沒有檔案版（官方 issue #12926 not-planned）。超過 8000 字或經 PowerShell 啟動（cmd.exe 命令列上限 8191）→ 改帶一句「先讀角色檔」 |
| OpenCode | `--auto` ＋ **第一次閒置時打**「先讀角色檔，讀完回 READY」 | 沒有每 session 附加系統提示的參數；`--auto` 是因為一跳權限詢問就卡住 |
| GeminiCLI | 同上（只打字） | `GEMINI_SYSTEM_MD` 會**整份取代**內建提示 |

`tui.whimsy=false` 的理由（舊版 probe 實錄）：gpt-6-astra 閒置時輸入框有星星閃爍動畫，
每秒重畫 6～7 次、約 8KB/s → **畫面永遠不會靜止 2 秒 → `agent_ready` 永遠 false → 信一直卡在佇列**。

要跑哪一支：**使用者「自訂連線」清單裡同圖示（或同執行檔名）的那筆優先**（路徑／參數／
PowerShell／關閉鍵都照使用者設的），沒有才自動偵測。

## 5. 信箱與投遞

`<工作區>/.ai/bus/NNNN-<from>-to-<to>.md`，front matter：

```
---
from: Agent-11
to: Agent-12          # 或 all（廣播給組內除寄件人以外的每個已啟動 agent）
type: TASK            # TASK | TASK_RESULT | QUESTION | ANSWER | REVIEW_REQUEST | REVIEW_RESULT | BLOCKED | INFO
task: TASK-001
status: completed     # 只有結果類才有
files_changed:
  - path/to/file
---
（正文，Markdown）
```

解析**刻意寬鬆**（照舊版）：`from`／`to` 缺了就用檔名補、`type` 缺了當 `INFO`、
`agent-12`／`Agent12` 都正規化成 `Agent-12`、front matter 壞掉**照樣投遞**（只多記一筆警告）。

### 監看

- **輪詢**（每 3 秒掃一次目錄），**沒有** FileSystemWatcher。舊版是 watcher ＋ 輪詢雙保險；
  這裡只留輪詢，理由寫在 `bus.rs` 的模組註解（agent 寫檔是「整份寫完」，3 秒的延遲相對於
  「等收件人閒置」的秒級等待可以忽略，少一個跨平台差異很大的元件）。
- 一個檔要**穩定 1500 毫秒**（大小與修改時間都沒變）才交出來——agent 可能正在寫。
- `.delivered`（信箱裡的一個檔）記已投遞的檔名，**同一封不重送**。
- 只收「收件人是本組」的（`all`＝寄件人是本組）；同一個資料夾開兩組互不干擾。
- 收件人沒在跑 → 記成已投遞，並且**由 AwayTerminal 自己寄一封 INFO 回寄件人**
  （附目前在跑的名單）。AwayTerminal 自己的信不回通知，免得打轉。

### 什麼時候可以打字（`agent_ready`）

**這是最容易錯的一條**，逐條照舊版：

| 條件 | 直接跑 exe | 經 PowerShell（npm 的 `.cmd`） |
|---|---|---|
| 啟動後至少過了 | 5 秒 | 10 秒 |
| CLI 啟動後**有畫過東西** | 必須 | 必須 |
| 最後一次輸出之後靜止 | 2000 ms | 3000 ms |
| 距上次投遞 | ≥3 秒 | ≥3 秒 |
| 距使用者最後一次打字 | ≥3 秒 | ≥3 秒 |
| 距最後一次送出 | ≥3 秒 | ≥3 秒 |

「經 PowerShell 多等一點」：PowerShell 提示行出來之後 node 還要載入 CLI，那段安靜期打的字會被吃掉。

### 打進去的那一行（逐字照舊版，隨介面語言）

- 一封、寄件人是 agent：
  `[AwayTerminal] 訊息 #{n} from {寄件人} ({task}, {type})：請讀 {路徑}，依你的角色處理，完成後回信給 {寄件人}。`
- 一封、寄件人是 AwayTerminal：
  `[AwayTerminal] 通知 #{n}：請讀 {路徑}（AwayTerminal 的系統通知，不需要回信）。`
- 多封（同一格佇列裡有好幾封）：
  `[AwayTerminal] 你有 {k} 則新訊息：請依序讀 {路徑、路徑…}，各自依你的角色處理並回信給寄件人。`
  （分隔符：英文 `, `、其餘語言 `、`）

沒有 `task` 欄位時顯示 `—`。

### 怎麼打進去

`send_text_then_enter`：**文字一次、Enter 隔 300 毫秒再單獨送**。
⚠️ 文字＋CR 一次寫入時 claude 會當成貼上、CR 變成軟換行而**不送出**（舊版 1.1.10 實測）。
claude 分頁的文字走前端的 `v` 協定（＝和使用者按 Ctrl+V 完全一樣：bracketed paste、
ESC+CR 軟換行都照原樣），其餘直接寫進 PTY。

**Enter 補送**：打完那一行 10 秒後，對方除了打字回顯之外沒有再輸出（2 秒內）＝Enter 可能
被吞了 → **單獨再送一次 Enter**（不重打整行，所以不會重複投遞）。角色注入那一句不做補送
（預期它只回一句 READY、輸出很短，舊版實測會誤判）。

### 廣播（`to: all`）

同一封信會排進每個收件人的佇列。要等**最後一個**收件人也拿到才記「已投遞」——否則中途
關程式（`.delivered` 已寫）重開後還沒收到的人永遠收不到。

## 6. 沙盒（新增，舊版沒有）

照 `CLAUDE.md`：**一個團隊一個沙盒**，所有 agent 同一棵 worktree。

- 專案是 git repo → 自動開 `git worktree` 到 `<repo>/.ai/sandbox/team-<組名>-<hash>/`，
  分支 `sandbox/team-…-<日期>`；不是 repo → 只建目錄。
- `TEMP`／`TMP` 導到沙盒底下；Rust 專案多設 `CARGO_TARGET_DIR`。
  **不隔離 `HOME`／`APPDATA`／`USERPROFILE`**——那會讓 Claude Code、Codex 掉登入。
- **`.ai/bus/` 在 worktree 裡**（agent 在裡面工作就在裡面收信）。
- 護欄設定在團隊的 worktree 裡產生一次（Claude Code 的 `PreToolUse` hook、Codex／Gemini
  的 `--sandbox` 參數）。同一家 CLI 只寫一次。
- `common.md` 的 `## Sandbox Mode` 那一段告訴 agent 這件事：**只 `git add` 自己改的檔**
  （禁止 `git add -A`／`git commit -a`／`git stash`——會把隊友做一半的東西一起提交或丟掉）、
  撞到 `index.lock` 就重試一次、不要切分支／reset／rebase／刪 worktree、成果留在
  `sandbox/...` 分支由使用者合併。**那是這份規則檔裡唯一一段新版加的內容**，其餘一字不動。
- Job Object：**一個分頁一個**（見下方「刻意與舊版／規格不同」）。

團隊名是中文時路徑會變 `<sanitized>-<hash>`（和自訂連線的沙盒同一條規則）。

## 7. 節流、停止、閒置檢查

| 功能 | 行為 |
|---|---|
| **節流** | 每組投遞 `maxMessages` 則就暫停（預設 50）。暫停中**照收信、照排隊，只是不打字**。右鍵「投遞」選一個次數＝改上限**並繼續**（計數歸零、補送暫停期間收到的信）；選「暫停」＝使用者自己暫停（調高上限不會自動解除）。 |
| **停止任務** | 右鍵「停止任務」：整組每一格送 `Esc` → **1 秒後 `Ctrl+U`** → **1.5 秒時打「先停一下然後記錄目前狀態」＋Enter**。Ctrl+U 是舊版 probe 實測補的：claude 還在思考、沒輸出就被 Esc 中斷時會把剛才那則訊息放回輸入框，不清掉的話停止句會接在後面、合成一則重新送出（＝它繼續做原本的事）。 |
| **閒置檢查** | ≥2 格在跑、都不忙、沒有信在排隊、沒暫停，連續閒置 N 分鐘 → 打一句話給 Agent-x1 要它逐一問大家狀況。**不算進投遞則數**（這是 AwayTerminal 自己問的）。 |

「信只在收件人閒置時投遞」是為什麼「寄一封暫停信」攔不住正在工作的 agent——所以要有
「停止任務」這個直接中斷的入口。執行期脈絡的 `### Delivery timing` 就是在對 agent 講這件事。

## 8. pane 狀態標籤（`E` 協定）

`0` 閒置、`1` 忙碌、`2` 有信待投遞、`3` 已結束、`4` 忙碌且有信待投遞。
**只在標籤變了才送**。`4` 是舊版使用者回報後補的：信只在閒置時投遞，原本忙碌會蓋掉
「有信待送」，PM 寄了暫停信、對方沒停也看不出信還在排隊。

## 9. Solo Mode

只有一格啟用時，執行期脈絡多一行 `You are the only enabled agent (Solo Mode).`，
而且不會有「回報給 Agent-x1」那段——PM 自己做，然後直接跟使用者講。

## 10. 離開程式時更新 `CLAUDE.md`

離開對話框的第二個勾選（舊版 `ExitDialog` 的 `UpdateMdCheck`）：對每個**一般的**
Claude Code 分頁打「請更新 CLAUDE.md…」，等到它們都安靜 4 秒（最多 3 分鐘）才真的離開。

**代理團隊的格不算**——好幾個 agent 同時改同一份 `CLAUDE.md` 會互相覆蓋（舊版 1.2.0 的註解）。
沒有可以請的分頁時那個勾選是停用的。

---

## 11. 既有團隊的「代理團隊設定…」（TASK-018）

分頁右鍵「代理團隊設定…」開同一個視窗，只是變成「既有團隊」模式：每格多一行狀態
（執行中／已結束／未執行／未啟用 ＋「· 套用後啟動／重新啟動／關閉」）和一個「重新啟動」按鈕。

改得動的東西與**改了之後對已在跑的 agent 做什麼**（逐項照舊版 `ApplyAgentSetup`）：

| 改什麼 | 對執行中的 agent 做什麼 |
|---|---|
| 投遞上限 | 不動任何 agent。如果目前是「**因為到上限而暫停**」而且新上限還沒到 → **自動解除暫停**（計數照舊、**不歸零**；歸零只發生在右鍵「投遞」選次數那條路） |
| 閒置檢查 | 不動任何 agent；「整組從什麼時候開始閒置」歸零重算 |
| 沙盒 | **不能改**（worktree 是建團隊時開的）→ 顯示目前狀態並停用 |
| 勾掉某一格 | 關掉那個分頁（執行中＝結束那個 CLI）、`enabled = false`、清掉它的佇列 |
| 加一格 | 用新設定啟動那一格 |
| 換 CLI 或換角色 | **關掉舊分頁、同一格用新設定重開**——角色是**啟動時注入**的（`--append-system-prompt-file`／`developer_instructions`），不重開它讀到的還是舊角色 |
| 按「重新啟動」 | 設定沒變也重開（舊版 `WantRestart`） |
| 格 1 | 「啟用」永遠勾著、點不動——使用者就是要跟它說話 |

其餘規則：

- **會結束執行中 agent 的變更要先確認**：「套用後：・Agent-12 · Software Engineer · Codex 會重新啟動…
  執行中的 agent 關閉或重新啟動後，它目前的對話就結束了。要套用嗎？」（用 pane 標題上的全名，
  才對得出是哪一格）
- **什麼都沒變＝當作取消**，不重開任何東西。
- 關／開的過程中 `suspend_relink` 打開：`tab_close` 不逐格重排也不拆組，全部做完才一次
  重綁（否則關掉倒數第二格時會先拆組，正要開的那幾格就沒有組可以回）。
- **名單變了 → 每一格的角色檔都重組**（隊友清單要對），而且寄一封 INFO 給 PM：
  「The team roster changed. Enabled agents now: … Your role file … has been regenerated.
  Re-read its Runtime Context section before assigning more work.」
  收件人＝有 PM 角色、而且**不是這一輪剛啟動**的那一格（剛啟動的本來就讀的是新檔），
  沒有 PM 角色就寄給代表列那一格。
- 全部格都失敗／都關掉 → 拆組。
- 關掉的那格如果是作用中分頁，`tab_close` 會跳到別的分頁 → 收尾時把焦點拉回這一組。

## 12. 恢復代理團隊分頁（TASK-018）

關閉程式時每一格都存一筆 `kind = "agent"` 的紀錄（`SavedTab` 的 `agent*` 欄位；
一組的設定每一格都存一份，恢復時取第一格的）：

| 欄位 | 用途 |
|---|---|
| `agentKey` | 同一組的各格共用 → 恢復時靠它把它們綁回一組 |
| `agentIndex` | 格號 1～4 |
| `agentGroupNumber` | 上次的組號，**沒被占用就沿用** → Agent ID 不變 |
| `agentBackend`／`agentRole` | CLI 與角色 |
| `agentRatio` | 上下列比例 |
| `agentMaxMessages`／`agentIdleCheck` | 投遞上限與閒置檢查 |
| `agentSandbox` | 那一組有沒有開沙盒 |
| `connName`（既有欄位） | 上次跑的是哪一條連線 → 恢復時**優先沿用它的執行檔／參數** |

行為（照舊版 `RestoreAgentGroup`）：

- 資料夾不見了 → **這一組不恢復**，log 一行，其餘分頁照開。
- 同資料夾、同組號（沒被占用才沿用）、同比例、同上限、同閒置檢查。
- 每格照上次的執行檔／參數：**絕對路徑一律要存在**才算數（npm 版的 `.cmd` 也一樣——
  CLI 移除了還開 PowerShell 分頁跑不存在的 `.cmd`，那一格會顯示「執行中」、信打進 shell），
  只有「靠 PATH 找」的裸名才交給 PowerShell 解析。查不到就重新偵測一次。
- **角色檔以目前的 `roles/` 重新組合**——CLI 是新 session，OpenCode／Gemini 會重打第一句。
- scrollback 照一般分頁倒回（`b` 協定，在 `n` 之後、`s` 之前）。
- 沙盒團隊：`prepare()` 看到 worktree 還在就沿用。
- 投遞計數從 0 開始（本次執行內的序號），但 `.delivered` 在工作區裡，所以**上次已經投遞過的信
  不會再投一次**。

## 刻意與舊版／規格不同

| # | 舊版／規格 | 這裡 | 為什麼 |
|---|---|---|---|
| 1 | 信箱用 FileSystemWatcher ＋ 輪詢雙保險 | **只輪詢**（3 秒一次，穩定判斷 1.5 秒） | agent 寫檔是整份寫完；3 秒相對於「等收件人閒置」可以忽略；少一個跨平台差異大的元件 |
| 2 | `CLAUDE.md`：「Job Object 一個團隊一個」 | **一個分頁一個** | 一個團隊一個的話，關掉某一格的分頁時它的子孫行程要等整組關掉才收。一個分頁一個同樣保證「整組關掉全部收乾淨」，而且多了「關一格就收一格」 |
| 3 | `tab.Kind == PowerShell` 判斷「經 PowerShell 啟動」 | 啟動時把連線的 `via_powershell` 記在那一格 | 我們的自訂連線分頁不會是 PowerShell 這個種類；直接記下來更準 |
| 4 | 分頁列那一列只有 tooltip | 多一個小標記（agent 數／`✉待投遞`，暫停時變色） | 不占空間又看得出來（同沙盒小標記的作法） |
| 5 | `u{id}`（拆組）JS 有、C# 端沒有呼叫者 | 解散團隊時**還有分頁活著**才送 | 否則那些 pane 會卡在沒有團隊的外框裡（同 `A` 全選的情況） |
| 6 | 組角色檔失敗只記 log | 建團隊直接失敗 | 角色檔空的話 agent 根本不知道自己是誰，開起來只會浪費使用者的額度 |
| 7 | 舊版寫死 `one Windows desktop` | 依平台換字（Windows／macOS／Linux） | 跨平台 |

## 還沒做

| 項目 | 舊版對應 | 說明 |
|---|---|---|
| 改團隊名稱（右鍵「更改名稱」） | 分頁改名 | 現在改的是代表列那個分頁的標題，重綁時會被組名蓋回去。 |

## 怎麼驗

```bash
cargo test --lib                          # agent::* 的單元測試（含執行期脈絡逐字比對）
cargo build --example fake_agent
cargo run   --example agent_probe         # 信箱往返，全程在 %TEMP%，不啟動真的 CLI
npm run verify                            # ＝ --verify 2；最後兩段是代理團隊的整條路與恢復
```

整套 `--verify` **要走 `npm run verify`**（`scripts/dev-verify.mjs`）：它在逾時或被中斷時用
`taskkill /PID <pid> /T /F` 依 PID 收掉整棵行程樹，並清掉 `%TEMP%` 的驗證資料夾。
直接用 `timeout` 包 `npx tauri dev` 會留下抓著 `target\debug` 的孤兒，下一次 `cargo build`
就會 `os error 32`（TASK-017 實際踩到，見 `docs/DEV-SETUP.md`）。

`agent_probe` 與 `--verify` 都**不啟動真的 claude／codex**（用 `examples/fake_agent.rs`）、
**不動使用者的自訂連線清單**（`agent_verify_begin` 的覆寫只活在記憶體裡）、
**不碰使用者的 `.ai/`**（專案資料夾在 `%TEMP%`，驗完整個刪掉）。
