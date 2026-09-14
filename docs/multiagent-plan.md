# AwayTerminal Multi-Agent 實作計畫（v1.2.0）

> 交接文件。由設計 session（Fable 5.1，2026-09-14）寫給實作 session。
> **先讀 `CLAUDE.md`**（專案慣例、建置、協定、踩雷），再照本文件的步驟做。
> 完整分析與理由在提案頁：https://claude.ai/code/artifact/fd87ab73-80fb-4d62-8970-81dc945af40a
> 本文件裡的決策都是使用者已經拍板的，**不要重新提案**（尤其：不要再用 hook／curl／localhost HTTP，不要整合第三方 multi-agent 框架）。

---

## 實作結果（Phase 1 已完成，2026-09-14，v1.2.0）

步驟 0～12 已做完，行為與驗證紀錄在 `CLAUDE.md`（版本 1.2.0 條目、「主要功能行為 → Multi-Agent 分頁」、協定表、踩雷「xterm 視窗卡在『使用者往上捲』」）。和本計畫細節不同的地方（都是實作時依既有慣例決定的細節，決策本身沒變）：

| 計畫寫的 | 實作 | 為什麼 |
|---|---|---|
| OpenCode／Gemini 優先用 `--prompt`／`-i` 第一句旗標，失敗再打字 | 一律打字（CLI 第一次閒置時） | 使用者選的是「保底」；兩個旗標只有文件、沒實測，偵測「旗標有沒有生效」要讀畫面 |
| Codex 角色檔 >8000 字 → 打字 FirstMessage | 改帶一句「先讀角色檔」的**原生** `developer_instructions`（經 PowerShell 的 .cmd 版也一律用短指引） | Codex 第一次進資料夾會先問信任，打字會打進信任選單；cmd.exe 命令列 8191 上限 |
| 投遞後 10 秒收件人沒輸出 → 整行重送一次 | 只補送一個 Enter | 打字本身就有回顯，「沒輸出」判不出來；真正的失敗是「文字在輸入框、Enter 沒送出」，重打整行會重複投遞 |
| pane 狀態 閒置／忙碌／等你 | 閒置／忙碌／**有信待送**／已結束 | 沒有可靠的「等使用者」訊號（BEL 機制已移除）；有信排隊比較有用 |
| `LaunchSpec`（Exe/Args/ViaPowerShell/PsPrefix/…）、`OpenCustom` 加 `psPrefix` | `AgentLaunch(ExtraArgs, FirstMessage)`；`OpenCustom` 只加 `addHistory`／`extraArgs`、改回傳分頁 | 啟動全走既有 OpenCustom（執行檔、PowerShell、關閉鍵都由 CustomConn 決定），psPrefix 沒人用 |
| `SavedTab.AgentSlot` | `AgentIndex`＋多存 `AgentGroupNumber` | 避免與型別 `AgentSlot` 同名；恢復時沿用組號，舊信的收件人 ID 才對得上 |
| 圖示複製 arrange.png | 另畫一張「上三下一」同風格暫代圖 `icon/multi-agent.png` | 一眼看得出是這個版面；使用者之後給正式圖直接覆蓋 |
| （沒寫） | 投遞前也等使用者 3 秒沒打字；經 PowerShell 啟動的 CLI 等 10 秒／靜止 3 秒 | 避免插進使用者打到一半的輸入、避免在 node 還沒載入 CLI 時打字 |
| （沒寫） | 關閉程式「更新 CLAUDE.md」不含 agent 分頁 | 多個 agent 同時改同一份 CLAUDE.md 會互相覆蓋 |
| （沒寫） | 設定視窗提示「第一次在新資料夾先回答每一格的信任提示」 | 同 Codex 那一條，Claude 也會問 |
| （沒寫） | `terminal.js settleBottoms` 修 xterm 捲動卡住（一般分頁也受益） | 恢復分頁時某一格偶爾整片空白，CDP 實錄 viewportY≠baseY |

**未驗證**：分頁右鍵「Multi-Agent 設定…／暫停投遞／繼續投遞／開啟訊息資料夾」（WPF ContextMenu 要真滑鼠，自動化打不開；暫停只驗到「到上限自動暫停」）、真正的 Claude／Codex／OpenCode／Gemini CLI（沒發 prompt）。

---

## 0. 一句話

在「新連接」加一個 **Multi-Agent**：一個分頁裡放 2～4 個真的互動 CLI（ClaudeCode／Codex／OpenCode／GeminiCLI），各自帶角色（PM／工程師／架構師／QA）。agent 之間用專案裡的 **`.ai/bus/` 資料夾當信箱**寫 .md 互傳；AwayTerminal 盯資料夾，看到新信就在**收件人的終端機打一行「請讀 <檔案>」＋Enter**。沒有 hook、沒有 HTTP。

---

## 1. 已定案的決策（使用者選的，直接照做）

| 項目 | 決定 |
|---|---|
| 通訊 | 檔案信箱 `.ai/bus/`＋`FileSystemWatcher`（＋輪詢備援）。**完全不用 hook。** |
| 投遞 | 一律打一行「請讀 <檔案>」，300 ms 後單獨送 Enter（沿用 `SendTextThenEnter`）。不貼內文。 |
| 信箱位置 | 專案根目錄 `.ai/bus/`；開組時自動把 `.ai/` 加進專案 `.gitignore`（沒有就建）。 |
| 角色檔 | 使用者提供的 `C:\Users\AwayWork\Desktop\tmp.txt`（common.md＋四個角色），依 §3.2 補充後內嵌進程式。 |
| 角色注入 | ClaudeCode：`--append-system-prompt-file`；Codex：`-c developer_instructions=`；OpenCode／Gemini：保底（啟動第一句「請先讀 …」，見 §3.7）。 |
| 畫面 | 撿 `AwayTerminal_ORG` 的協作分頁群組程式碼（terminal.js `.cowork`）泛化成 N 個 pane：**下一上 N−1**，外框顏色 Agent1 淡紅／2 淡藍／3 淡綠／4 淡紫。 |
| 拓樸 | v1 只有 PM ↔ worker；worker 之間經 PM（角色檔規定；信箱本身不限制 `to:`）。 |
| 防呆 | 每組訊息上限預設 30 則（設定可改）到了就暫停並提示；右鍵 暫停／繼續。 |
| 不支援 | Telegram 遠端、巨集（同協作分頁的排除方式）。 |
| 版本 | **1.2.0**（電腦上裝的是未推上去的 1.1.12，要跳過它）。 |
| 留底 | `AwayTerminal_ORG` 的三個 commit 推成 GitHub `cowork` 分支。 |
| 圖示 | 先用現成的頂替（`icon/arrange.png` 複製成 `icon/multi-agent.png`），使用者之後會給正式的。 |
| Agent id | `Agent-{組號}{格號}`：第一組 Agent-11～14、第二組 Agent-21～24。組號＝目前開著的組裡最小的空號（1～9）。 |
| 預設 | 格 1、2 啟用（PM／Software Engineer），格 3、4 停用（Software Architect／QA Engineer）；四格都在同一個資料夾（開組時選一次）。 |

---

## 2. 硬性限制

1. **不要碰使用者正在用的 AwayTerminal**（Program Files 的正式版也叫 `AwayTerminal.exe`）。
   - 開發版一律用測試模式啟動：環境變數 `AWAYTERMINAL_DATA_DIR=<另一個資料夾>`（`Services/AppPaths.cs`，從 `_ORG` 搬，見步驟 1）。
   - 重建前只關 `bin\Debug` 那個實例；**不要** `Get-Process AwayTerminal | Stop-Process`。
   - 從工具環境啟動 exe 會被沙箱擋：Bash `(AWAYTERMINAL_DATA_DIR='C:\…\testdata' ./bin/Debug/net9.0-windows/AwayTerminal.exe &)`；PowerShell `Start-Process` 常被 PC-cillin 擋。
2. **不要對真的 Claude／Codex 發 prompt**，除非使用者當次明講可以（他有週額度顧慮）。功能驗證用假 agent（步驟 11）。
3. 版本、CLAUDE.md、README 依專案慣例更新；commit 訊息中文、格式 `v1.2.0: …`，結尾照系統給的 attribution 行。**不要發佈到 awaysu.cc、不要安裝到電腦**，除非使用者說。
4. C# ↔ JS 協定改動時 `MainWindow*.cs` 與 `web/terminal.js` 必須同步，並更新 CLAUDE.md 的協定表。
5. 不改使用者全域的 `~/.claude`、`~/.codex`、`~/.gemini`、`~/.config/opencode`。所有注入都只作用在那一個 session。

---

## 3. 規格

### 3.1 名詞
- **組（AgentGroup）**：一個 Multi-Agent 分頁。有組號（1～9）、專案目錄、2～4 個格、版面比例、暫停旗標、訊息計數、GUID key（存檔用）。
- **格（AgentSlot）**：格號 1～4、`AgentId`、Role（角色檔名或 None）、Backend（adapter key）、Enabled、對應的 `TerminalTab`（未啟動＝null）、待投遞佇列。
- **Backend key** ＝ 現有圖示 key：`claude-code`、`codex`、`opencode`、`geminicli`。

### 3.2 角色檔（三層組合）
資料位置（測試模式跟著 `AppPaths.DataDir`）：
```
%LOCALAPPDATA%\AwayTerminal\multiagent\
  common.md
  roles\product-manager.md
  roles\software-engineer.md
  roles\software-architect.md
  roles\qa-engineer.md
  sessions\<組號>\Agent-<id>.md     ← 開組時組合出來的成品（絕對路徑，給 adapter 用）
```
- 範本內嵌在程式：`Resources/MultiAgent/common.md`、`Resources/MultiAgent/roles/*.md`（csproj `EmbeddedResource`）。第一次啟動、或檔案不在時複製到上面的資料夾；設定視窗有「還原預設」（覆寫）。使用者可自己加 `roles/xxx.md`，下拉會列出來。
- 內容來源：`C:\Users\AwayWork\Desktop\tmp.txt`。它是一份聊天貼文，**要整理成乾淨的 Markdown**（tmp.txt 前三份沒有 `#` 標題階層、第四五份有；統一成 `# 標題`／`## 小節`／條列）。不改語意。
- **common.md 要補三段**（放在 General Rules 之後）：
  1. *Solo mode*：「If AwayTerminal reports that no worker agent is enabled, the Product Manager performs the implementation itself and still reports in the standard result format.」
  2. *Language*：「Talk to the user in the user's language (the language of the user's messages). Messages between agents may be in English.」
  3. *Throttling*：「AwayTerminal enforces a per-team message limit. When the limit is reached, delivery pauses and the user must resume it. Do not spam messages; batch related information into one message.」
- **第三層（執行期脈絡）由 `RoleLibrary.Compose(slot)` 產生**，附在成品最後，內容：
  ```
  # Runtime Context (generated by AwayTerminal)
  Agent ID: Agent-12
  Role: Software Engineer
  Provider: Codex
  Team session: MAS-1
  Project directory: C:\Users\…\project
  Enabled agents:
    - Agent-11  Product Manager   (ClaudeCode)
    - Agent-12  Software Engineer (Codex)      ← you
    - Agent-14  QA Engineer       (GeminiCLI)
  Mailbox: .ai/bus/  (relative to the project directory)

  ## How to send a message
  Write ONE file .ai/bus/NNNN-<your id>-to-<recipient id>.md where NNNN is
  (highest existing NNNN in the folder) + 1, zero-padded to 4 digits. Start the
  file with a YAML front matter block, then the body in Markdown:
  ---
  from: Agent-12
  to: Agent-11
  type: TASK_RESULT        # TASK | TASK_RESULT | QUESTION | ANSWER | REVIEW_REQUEST | REVIEW_RESULT | BLOCKED | INFO
  task: TASK-001
  status: completed        # completed | failed | blocked | pass | fail  (only for results)
  ---
  Never edit or delete an existing message file. Do not write anything else into .ai/bus/.

  ## How you receive messages
  AwayTerminal types a line like
  [AwayTerminal] 訊息 #7 from Agent-11 (TASK-001, TASK)：請讀 .ai/bus/0007-Agent-11-to-Agent-12.md，依你的角色處理，完成後回信給 Agent-11。
  into your terminal. Read that file, act according to your role, and reply by
  writing a new message file. You may read any file in .ai/bus/ for context.
  Only the Product Manager talks to the user. Workers reply to the Product Manager.
  ```
  （`Enabled agents` 要標出 `← you`；角色 None 時 Role 寫 `None (general assistant)`。）

### 3.3 信箱（`.ai/bus/`）
- 檔名 `NNNN-<from>-to-<to>.md`；`to` 可為 `all`。同號不同寄件人視為兩封（依檔案建立時間排序）。
- 解析：YAML front matter（`---` 到 `---`，只認 `key: value` 與 `- item` 清單，不引入 YAML 函式庫）。**寬鬆**：缺 `to` 用檔名；缺 `from` 用檔名；缺 `type` 當 `INFO`；front matter 壞掉仍投遞給檔名收件人，並記 diag `ma parse warn`。
- 已投遞紀錄：`.ai/bus/.delivered`（每行一個檔名，UTF-8）。開組／恢復時先讀它，已投遞的不重送；程式關著時寫進來的新信會在恢復後投遞。
- 穩定判定：檔案最後修改時間距現在 ≥ 1.5 s 才算完成（agent 可能 Write 後再 Edit）。
- 備援：除了 `FileSystemWatcher`，每 3 s 掃一次目錄（`EnumerateFiles("*.md")` 減 `.delivered` 集合）。
- 收件人不在組內（例：`to: Agent-13` 但格 3 未啟用）→ 不投遞、記 diag、把一封 `INFO` 回給寄件人：「Agent-13 is not enabled in this team.」（AwayTerminal 自己寫信，`from: AwayTerminal`）。

### 3.4 投遞
- 佇列：每個格一個 FIFO。收件人可投遞的條件：`tab.Session != null`、`tab.Status == Ready`、`now − tab.LastOutputUtc ≥ 2000 ms`、目前沒有另一封正在投遞、組沒有暫停、未達上限。
- 投遞內容（單行，實際路徑相對專案根、用 `/`）：
  `[AwayTerminal] 訊息 #{n} from {from} ({task 或 —}, {type})：請讀 {path}，依你的角色處理，完成後回信給 {from}。`
  同一收件人佇列有多封時合併成一行：`[AwayTerminal] 你有 {k} 則新訊息：請依序讀 {path1}、{path2}…，各自依你的角色處理並回信給寄件人。`
- 送法：`SendTextThenEnter(tab, line)`（`_ORG MainWindow.Cowork.cs`）：claude 分頁走 `PasteToTab`（JS doPaste，ESC+CR 處理），其他直接 `WriteText`；`RemoteEnterDelayMs`（300）後對同一 session 單獨送 `\r`。
- 重送：投遞後 10 s 內收件人 `LastOutputUtc` 沒前進 → 再送一次（只一次），記 diag `ma resend`。
- 編號 `#n`＝該組本次程式執行內投遞序號（從 1 起）；同時累計進「訊息計數」。
- 上限：計數達 `AppSettings.MultiAgentMaxMessages`（預設 30）→ `Paused=true`、視窗不在前景就閃工作列（`FlashIfInactive`，_ORG 有）、分頁列那列顯示 `⏸`；右鍵「繼續投遞」→ 計數歸零、`Paused=false`、補送佇列。
- 暫停中：信照收、照解析、照排隊，只是不打字。
- `to: all`：投給組內除寄件人外的所有已啟動格（各自算一則）。
- 等待上限：收件人 10 分鐘都不閒置 → 留在佇列，tooltip 顯示「待投遞 n」，不丟。

### 3.5 畫面
- **版面**：下一格＝格 1（全寬），上列＝已啟動的格 2～4 由左到右平分。只有格 1 → 單格全滿。上下分隔線可拖（比例 0.15～0.85，預設 0.5，雙擊回 0.5），拖完 JS 以 `G` 協定回報、C# 存進組（恢復用）。上列各欄 v1 不做拖寬。
- **外框顏色**（2 px）：格 1 `#EF9A9A`、格 2 `#90CAF9`、格 3 `#A5D6A7`、格 4 `#CE93D8`。作用中 pane 用既有的 header 亮色（`.active-pane .pane-header`），外框顏色不變。
- **pane 標題**：`Agent-12 · Software Engineer · Codex`＋狀態 pill（閒置／忙碌／等你）。狀態來自既有 `TermStatus` 與 `WaitingForUser`，C# 以新協定 `E` 推給 JS（§3.6）。
- **分頁列（右側）那一列**：只顯示組的第一格那一列（沿用 `_stripView` Filter）。圖示 `multi-agent.png`，任一格忙碌→染忙碌色；標題＝資料夾名（`DirTabName`）；列尾小字 `✉3/30`、暫停時 `⏸3/30`；tooltip：`Multi-Agent  <dir>`＋每格「Agent-12 SE Codex 忙碌」＋訊息數／暫停／待投遞。
- **右鍵（只在組那一列）**：`Multi-Agent 設定…`、`暫停投遞`／`繼續投遞`、`開啟訊息資料夾`（explorer `.ai/bus`）、`關閉整組`。既有「關閉」對組＝關閉整組（確認框）。「巨集」項對組隱藏（沿用 `NotCoworkVisibility` 的做法）。
- **分割／分欄模式**：整組當一個顯示單位（_ORG 已做）；放大＝整組。
- **設定視窗 `MultiAgentDialog`**：
  - 上方：資料夾（瀏覽；既有組再開時唯讀）。
  - 四個區塊（2×2）：`啟用` 勾選（格 1 永遠勾）、`Agent ID`（唯讀，開組前顯示 `Agent-?1`，組號在按確定時決定）、`Coding Agent` 下拉（只列 `adapter.Detect()` 找得到的；一家都沒有→整個視窗顯示提示並停用確定）、`Agent Role` 下拉（`None`＋`roles\*.md` 的檔名轉成標題）。
  - 預設：格 1 PM、格 2 SE、格 3 Architect、格 4 QA；Backend 預設＝找到的第一家（有 ClaudeCode 優先給格 1、有 Codex 優先給格 2）。上次的選擇存 `AppSettings.MultiAgentLastSetup`（JSON 字串即可）下次帶入。
  - 既有組再開（右鍵設定）：已啟動的格唯讀（灰）、只能勾啟用未啟動的格→按確定就地啟動；已結束（session exited）的格顯示「重新啟動」按鈕。
  - 底部：`還原角色檔預設`、`開啟角色檔資料夾`、`確定`、`取消`。
  - 確定前檢查：至少格 1 啟用；資料夾存在。

### 3.6 C# ↔ JS 協定（新增／修改；US ＝ `\x1f`）
- C#→JS `g{bottomId}{US}{ratio}{US}{topIds 逗號}{US}{labels 以 | 分隔，順序＝bottom,top…}{US}{colors 逗號，同順序}`：建立或更新一組（取代 _ORG 的 a/b 版）。id 不在 terms 裡就忽略那個。
- C#→JS `u{anyIdInGroup}`：拆組，所有 pane 回一般分頁。
- C#→JS `E{id}{US}{0|1|2}`：pane 狀態 pill（0 閒置、1 忙碌、2 等你）。
- JS→C# `G{bottomId}{US}{ratio}`：拖完分隔線。
- 其餘沿用（`n`、`o`、`t`、`K` 重排以組為單位、`k` 回報順序時組內 id 相鄰，bottom 先）。

### 3.7 Adapter（`Services/MultiAgent/`）
```csharp
interface ICodingAgentAdapter {
    string Key { get; }            // "claude-code" | "codex" | "opencode" | "geminicli"
    string DisplayName { get; }    // ClaudeCode | Codex | OpenCode | GeminiCLI
    string? Detect();              // 呼叫 CustomConnDialog.ResolveTool(exeNames)；null＝沒裝
    LaunchSpec BuildLaunch(AgentSlot slot, string workDir, string roleFilePath);
    RoleInjection Injection { get; }   // NativeFlag | FirstMessage
    bool PasteNeedsEscCr { get; }      // 只有 ClaudeCode true
}
record LaunchSpec(string Exe, string Args, bool ViaPowerShell, string? PsPrefix, string? FirstMessage, byte[] CloseBytes);
```
執行檔與參數優先沿用使用者「自訂連線」清單裡同 icon key 的那筆（Path／Args／ViaPowerShell／CloseKey）；沒有那筆才用 `Detect()`＋空參數。各家：

| | ClaudeCode | Codex | OpenCode | GeminiCLI |
|---|---|---|---|---|
| 額外參數 | ` --append-system-prompt-file "<roleFile>"` | ` -c "developer_instructions='<單行內容>'"`（沿用 _ORG `CoworkBridge.OneLine`：去換行、單雙引號換成 ’ ”）；內容 > 8,000 字元→改走 FirstMessage | 無 | 無 |
| FirstMessage | 無 | 無（超長時：`請先完整閱讀 <roleFile>，那是你的角色與協作規則；讀完只回覆 READY。`） | 同左句，優先用旗標 ` --prompt "<句>"`（文件載明、未實測）；若啟動後 15 s 內沒看到任何輸出變化再由 AwayTerminal 打同一句 | 同左句，優先用旗標 ` -i "<句>"`（文件載明、未實測）；同樣有打字退路 |
| 環境變數 | 既有程式層級 `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` 已夠 | — | — | — |

FirstMessage 的「打字退路」：session 第一次閒置（`Status==Ready` 且 2 s 無輸出）時由 `MultiAgentManager` 用 `SendTextThenEnter` 送；只送一次；記 diag `ma role-inject typed`。

---

## 4. 步驟

每一步：改哪些檔 → 做什麼 → 驗收。建議每完成 2～3 步就 `dotnet build` 一次。

### 步驟 0　留底分支
```bash
cd /c/Users/AwayWork/Desktop/WORKSPACE2/AwayTerminal_ORG
git branch cowork 275006d
git push https://github.com/awaysu/AwayTerminal.git cowork:cowork
```
驗收：GitHub 有 `cowork` 分支、含 0d6465c／ea2f1e2／275006d；main 不動。

### 步驟 1　從 `AwayTerminal_ORG` 搬基礎設施（不含交棒）
`_ORG` 路徑：`C:\Users\AwayWork\Desktop\WORKSPACE2\AwayTerminal_ORG`（看 `git -C … diff fe06799 275006d -- <檔>`）。搬這些、**不搬** `Services/CoworkBridge.cs`、`MainWindow.Cowork.cs` 的交棒部分：
1. `Services/AppPaths.cs`（整檔）＋ `App.xaml.cs`／`AppSettings.cs`（`Dir = AppPaths.DataDir`）＋ `MainWindow.xaml.cs` 裡「測試模式不登錄檔案總管、不開 IPC、不啟動 Telegram、標題加 [TEST]」的幾處（在 diff 裡搜 `IsTestMode`）。
2. `MainWindow.xaml.cs OpenCustom`：加 `bool addHistory = true, string extraArgs = ""` 兩個參數（照 _ORG diff）；再多加 `string? psPrefix = null`（viaPs 時放在 `PendingCommand` 前：`$env:X='…'; & "<path>"…`，v1 沒人用到也先留）。
3. `SendTextThenEnter`、`FlashIfInactive`（從 `_ORG MainWindow.Cowork.cs` 搬進新的 `MainWindow.MultiAgent.cs`）。
4. `web/terminal.js` 的群組段（`unitEl`／`unitList`／`applyGroup`／`ungroup`／`setupDivider`／`layout` 以顯示單位為準／`onDrop`／`notifyOrder`／`refit` 對隱藏群組 `measureHidden`）＋ `web/index.html` 的 `.cowork` CSS。先原樣搬、能 build；步驟 9 再泛化。
5. `MainWindow.xaml.cs` 的協作分頁區（`_stripView` Filter、`RowOf`、`MarkActiveRow`、`NormalizeCoworkOrder`、`SelectTab` 回最後點的那半、拖曳排序整組移動、`r` 尺寸回報不拿組內 pane 當初始尺寸、關閉時拆組、遠端與巨集排除）＋ `MainWindow.xaml` 的列（第二圖示、狀態小字、巨集隱藏；**⇆／⇅ 切換鈕不要**，Multi-Agent 沒有左右模式）＋ `Models/TerminalTab.cs` 的 `Cowork` 相關屬性。
   → 搬過來後**立刻改名**：`CoworkGroup`→`AgentGroup`（步驟 2 重寫）、`TerminalTab.Cowork`→`TerminalTab.Agent`（型別 `AgentSlot?`）、`CoworkVisibility`→`AgentGroupVisibility`（＝這個 tab 是組的格 1）、`NotCoworkVisibility`→`NotAgentVisibility`。
6. `Services/AppSettings.cs SavedTab`：不要 Cowork* 四欄，改加 `AgentKey`（string，組 GUID）、`AgentSlot`（int 1–4）、`AgentRole`（string）、`AgentBackend`（string）、`AgentRatio`（double）。`AppSettings` 加 `MultiAgentMaxMessages = 30`、`MultiAgentLastSetup = ""`。
7. Loc 字串：搬 `tip.cowork*`／`menu.cowork*` 時直接改成 `ma.*` 命名（步驟 10 一起整理）。

驗收：`dotnet build` 0 錯誤；用測試模式開一般分頁一切如常（分頁列、分割模式、恢復）。

### 步驟 2　Models
- `Models/AgentGroup.cs`：`Key`(GUID)、`Number`(1–9)、`Dir`、`Slots`(AgentSlot[4])、`Ratio`、`Paused`、`MessageCount`、`DeliverySeq`、`EnabledSlots` 輔助、`static int NextFreeNumber(IEnumerable<AgentGroup> open)`。
- `Models/AgentSlot.cs`：`Index`、`AgentId => $"Agent-{Group.Number}{Index}"`、`Role`、`Backend`、`Enabled`、`Tab`、`Queue`(Queue<AgentMessage>)、`Delivering`、`LastDeliveredUtc`、`RoleInjected`、`Label => $"{AgentId} · {RoleTitle} · {BackendName}"`。
- `Models/AgentMessage.cs`：`FileName`、`FullPath`、`From`、`To`、`Type`、`Task`、`Status`、`Body`、`CreatedUtc`、`static AgentMessage? Parse(string path)`（§3.3 寬鬆規則）、`static string NextFileName(dir, from, to)`（給 AwayTerminal 自己寫信用）。

### 步驟 3　Adapters
`Services/MultiAgent/ICodingAgentAdapter.cs`、`CliAdapterBase.cs`（共用：從 `AppSettings.Current.CustomConns` 找同 icon 的那筆、否則 `CustomConnDialog.ResolveTool`；把 `ResolveTool`／`KnownTools` 改 `internal static`）、`Adapters/ClaudeCodeAdapter.cs`、`CodexAdapter.cs`、`OpenCodeAdapter.cs`、`GeminiCliAdapter.cs`、`AdapterRegistry.cs`（`All`、`ByKey`）。規格見 §3.7。
驗收：暫時在 diag 印四家 `Detect()` 結果（這台電腦：claude、codex 桌面版 CLI 應該有；opencode 有 `@opencode-aidesktop`，gemini 沒有）。

### 步驟 4　RoleLibrary 與角色檔
- `Resources/MultiAgent/common.md`、`roles/{product-manager,software-engineer,software-architect,qa-engineer}.md`：從 `Desktop\tmp.txt` 整理（§3.2），csproj `<EmbeddedResource Include="Resources\MultiAgent\**\*.md" />`。
- `Services/MultiAgent/RoleLibrary.cs`：`EnsureDefaults()`（缺檔才複製）、`RestoreDefaults()`（覆寫）、`ListRoles()`（檔名→標題：`software-engineer`→`Software Engineer`；讀檔第一個 `#` 標題優先）、`Compose(AgentGroup g, AgentSlot s) → 成品路徑`（common＋role＋Runtime Context，§3.2 範本；UTF-8 無 BOM；`sessions\<組號>\` 每次開組先清空）。
驗收：組一份，人工看內容；Codex 用的 `OneLine` 版長度印出來（目前 tmp.txt 全文約 20K 字元 → **Codex 幾乎一定會超過 8,000，走 FirstMessage**，這是預期行為，記在 CLAUDE.md）。

### 步驟 5　MessageBus
`Services/MultiAgent/MessageBus.cs`：`Start(dir)`／`Stop()`；`FileSystemWatcher`（Created／Changed／Renamed，`*.md`）＋ `System.Threading.Timer` 3 s 掃描；候選檔進「待穩定」表，最後修改 ≥ 1.5 s 才 `Parse` → 事件 `MessageArrived(AgentMessage)`（背景執行緒觸發，呼叫端自己 `Dispatcher`）；已在 `.delivered` 或已發出事件的檔名跳過；`MarkDelivered(fileName)` 追加寫 `.delivered`。`EnsureGitIgnore(projectDir)`：`.gitignore` 沒有 `.ai/` 這行就追加（含換行處理、UTF-8）。
驗收：對測試資料夾手動丟檔、改檔，diag 看到 `ma msg 0001-… from=… to=… type=…` 各一次。

### 步驟 6　MultiAgentManager
`Services/MultiAgent/MultiAgentManager.cs`（持有 `MainWindow` 需要的回呼，或做成 `MainWindow.MultiAgent.cs` partial 的一部分——二選一，以 partial 較貼近既有寫法）：
- `OpenGroup(setup, dir)`：組號、`.gitignore`、`RoleLibrary.Compose` 每格、依格 1→4 用 `OpenCustom(conn, dir, restoreTitle, addHistory:false, extraArgs, psPrefix)` 開分頁、`tab.Agent = slot`、`LinkGroup`（→ JS `g`）、`MessageBus.Start`、`AddHistory(Type="multiagent", Dir)`。
- `EnableSlot(group, index)`：就地啟動一格（右鍵設定用）。
- `OnMessageArrived`：找組（依 bus 的 dir）→ 收件人格（`to` 比對 `AgentId`，`all` 展開）→ 入佇列 → `TryDeliver(slot)`。
- `TryDeliver(slot)`：§3.4 條件 → 組字串 → `SendTextThenEnter` → `MarkDelivered` → `MessageCount++`、`DeliverySeq++` → 重送計時器 → `RaiseState()`。每 500 ms 有一個統一計時器對所有佇列非空的格再試（用既有 `UpdateStatuses` 的節奏即可）。
- `Pause/Resume(group)`、`CloseGroup(group)`（逐格 `CloseTab`、`MessageBus.Stop`、`u` 協定）、`OnTabClosed(tab)`（組內某格 session 結束→格保留、tab 標「已結束」；整組最後一格關掉→拆組）。
- 狀態推送：`UpdateStatuses` 迴圈裡對每個有 `Agent` 的 tab 送 `E{id}{US}{state}`（只在變化時送）。
- FirstMessage 注入：slot 第一次閒置時送（§3.7）。
- 恢復：`RestoreTabs` 開完所有 SavedTab 後，依 `AgentKey` 把 tab 歸回組（組號重新取空號；`AgentId` 因此可能變，Runtime Context 重新 Compose——所以恢復時也要重新注入，等於重新啟動 CLI，這在既有恢復流程本來就是重開 session）。
驗收：步驟 11 假 agent。

### 步驟 7　MultiAgentDialog
`Dialogs/MultiAgentDialog.xaml(.cs)`，規格 §3.5。回傳 `MultiAgentSetup`（Dir、4 格的 Enabled／Backend／Role）。中英文都要（`Loc`）。

### 步驟 8　MainWindow 接線
- New 下拉：在「自訂…」之前加「Multi-Agent」（圖示 `multi-agent.png`）；沒有任何 adapter 偵測到時仍列出，點了在對話框提示。
- 分頁列那一列與右鍵（§3.5）；`TerminalTab` 加 `AgentStateText`（`✉3/30`／`⏸3/30`）、`AgentGroupVisibility`、tooltip 段落。
- 關閉整組確認、程式關閉時 `FinishExitAsync` 對組內每格存 `AgentKey/AgentSlot/AgentRole/AgentBackend/AgentRatio`。
- 遠端／巨集排除：所有 `t.Cowork == null` 的判斷改成 `t.Agent == null`。
- `UsesDirTitle`／`DirTabName`：組的標題＝資料夾名。

### 步驟 9　terminal.js 泛化成 N pane
- `g` 協定改 §3.6 格式；DOM：`.agents`（取代 `.cowork`）＝ `flex-direction: column`；子元素 `.agents-top`（flex row，放 top panes）、`.ag-divider`、bottom pane。只有 bottom 時不畫 top 與 divider。
- 每個 pane `el.style.borderColor = color`、`el.classList.add("agent-pane")`；`paneTitle` 用 label；`E` 協定切 pill class（`.st-idle/.st-busy/.st-wait`，文字由 C# 給或 JS 依語言？→ 由 C# 隨 `g` 的 label 之外再送一次 `t`? 簡化：pill 文字固定三個 Loc 字串由 C# 在 `T{json}` 全域設定裡多帶 `agentStates:["閒置","忙碌","等你"]`）。
- `unitEl`、`layout`、`onDrop`、`notifyOrder`、`refit`（隱藏組用 `measureHidden(group.el, ids)`）全部改成以組陣列為準；`ungroup` 把所有 pane 放回 container。
- 分隔線拖曳只有上下；`G` 回報 bottomId。
驗收：測試模式開一組 4 格：版面正確、拖分隔線、切分割模式、拖曳分頁列順序、關閉整組後畫面乾淨。

### 步驟 10　Loc 字串
全部用 `ma.` 前綴（例：`ma.title`＝Multi-Agent、`ma.menuSetup`、`ma.menuPause`／`ma.menuResume`、`ma.menuOpenBus`、`ma.menuCloseGroup`、`ma.closeConfirm`、`ma.noBackend`、`ma.limitReached`、`ma.stateIdle/Busy/Wait`、`ma.tip*`）。中英都要。

### 步驟 11　測試
1. **假 agent**（不打真 API）：舊 session 的工具還在磁碟：
   `C:\Users\AwayWork\AppData\Local\Temp\claude\C--Users-AwayWork-Desktop-WORKSPACE2-AwayTerminal\ec343f65-6643-4562-a649-b1f8b3f60fac\scratchpad\fakeagent\`（`Program.cs`＋csproj，是上一版協作測試用的 console app）。複製到本 session 的 scratchpad 改成：印提示符 `> `、逐行讀 stdin；收到含「請讀 」的行→取路徑→讀檔→等 1 s→依 `--role` 寫回信：`pm`：收到使用者需求（第一行任意輸入）→寫 `TASK` 給 `--to`；收到 `TASK_RESULT` → 寫 `REVIEW_REQUEST` 給 QA（若 `--qa` 有給）否則印「DONE」；`se`：收到 `TASK` → 寫 `TASK_RESULT completed`；`qa`：收到 `REVIEW_REQUEST` → 寫 `REVIEW_RESULT pass`。所有寫檔照 §3.3 格式。也印出收到的整行，方便看 diag 與畫面。
2. 測試資料夾：`scratchpad\testdata\`；先放一份 `settings.json`，`CustomConns` 三筆 icon＝`claude-code`／`codex`／`geminicli`、Path 都指到 fakeagent.exe、Args 各自 `--role pm --to Agent-12 --qa Agent-14`／`--role se`／`--role qa`（Backend 偵測沿用自訂連線那筆，所以 adapter 不會去找真 claude）。
3. 啟動：Bash `(AWAYTERMINAL_DATA_DIR='<testdata 絕對路徑>' ./bin/Debug/net9.0-windows/AwayTerminal.exe &)`；用 UIA／`v.ps1` 類工具或直接請使用者操作：New → Multi-Agent → 選 scratchpad 裡的假專案資料夾 → 格 1 ClaudeCode/PM、格 2 Codex/SE、格 4 GeminiCLI/QA → 確定。
4. 在格 1 打一行需求＋Enter。預期 diag：`ma msg 0001 … type=TASK` → `ma deliver #1 → Agent-12` → `ma msg 0002 … TASK_RESULT` → `ma deliver #2 → Agent-11` → `0003 REVIEW_REQUEST` → `#3 → Agent-14` → `0004 REVIEW_RESULT` → `#4 → Agent-11`；`.ai/bus/` 四封信＋`.delivered` 四行；`.gitignore` 有 `.ai/`；分頁列 `✉4/30`。
5. 上限：暫時把 `MultiAgentMaxMessages` 設 2 → 第 3 封停住、`⏸2/2`、右鍵繼續→補送。
6. 恢復：關程式（勾恢復分頁）→ 重開 → 組回來、`AgentId` 正確、`.delivered` 生效不重送；程式關著時手動丟一封新信 → 重開後投遞。
7. **真 CLI**：只有使用者說可以才做：格 1 ClaudeCode、格 2 Codex，一個很小的任務（例如「在 README 加一行」），確認角色注入生效（PM 不自己改碼、SE 回結構化結果）。
8. 也要確認一般分頁、SSH、協作以外的功能沒被步驟 1 的搬移弄壞（開幾個分頁、分割模式、恢復）。

### 步驟 12　文件、版本、commit
- `AwayTerminal.csproj` `<Version>1.2.0</Version>`、`installer/installer.iss` `AppVersion "1.2.0"`。
- `CLAUDE.md`：版本條目 `1.2.0=…`（照既有寫法：做了什麼、根因／驗證、踩雷）、「主要功能行為」加「Multi-Agent 分頁」一節（§3 的精華）、協定表加 `g/u/E/G`、關鍵檔案表加新檔、建置／測試加假 agent 用法。
- `README.md` 特色加一行。
- commit：`v1.2.0: Multi-Agent 分頁（2～4 個 AI CLI 同頁分工、.ai/bus 信箱互傳、自動投遞）`＋系統 attribution 行。**只 commit，不 push、不發佈、不安裝**——等使用者確認。

---

## 5. 驗收清單（全部打勾才算 Phase 1 完成）

- [ ] `cowork` 分支在 GitHub
- [ ] `dotnet build` 0 錯誤 0 新警告
- [ ] 測試模式 `[TEST]` 視窗與正式版同時開著互不干擾
- [ ] New → Multi-Agent 設定視窗：只列偵測到的 backend、角色下拉含自訂 .md、預設值正確、記住上次
- [ ] 一頁 2／3／4 格版面正確、顏色正確、標題與狀態 pill 正確、分隔線可拖且比例恢復
- [ ] 角色檔三層組合正確；Claude 走旗標、Codex 超長走 FirstMessage、Gemini/OpenCode 走 `-i`／`--prompt`＋打字退路
- [ ] 假 agent 四封信全自動走完；`.delivered`、`.gitignore` 正確
- [ ] 上限→暫停→繼續→補送；右鍵四項都可用；關閉整組乾淨
- [ ] 恢復分頁：組、id、比例、`.delivered` 全部正確
- [ ] 遠端清單看不到組內分頁；右鍵無巨集
- [ ] 一般分頁、分割模式、拖曳排序、恢復不受影響
- [ ] CLAUDE.md／README／版本更新；commit 完成（未 push）

---

## 6. 之後才做（Phase 2～4，**現在不要做**）
- Phase 2（1.2.1）：`TaskBoard`（由訊息推導狀態，寫 `.ai/bus/board.md`）、訊息紀錄視窗、pane 標題顯示目前 task、TASK_RESULT 欄位驗證。
- Phase 3（1.2.2）：工作流範本（TASK_RESULT → 自動 REVIEW_REQUEST、fail 自動退回、round 上限）、worker ↔ worker、MCP `send_message`（Claude 可 `--mcp-config`；Codex／Gemini 要全域設定）、Claude 專屬 `claude/channel` 推播叫醒。
- Phase 4（1.3.0）：git worktree per TASK 或檔案鎖、PASS 自動 merge、CONFLICT 退回。

---

## 7. 參考位置
- 提案頁（理由、開源專案比較、各家 CLI 旗標查證）：https://claude.ai/code/artifact/fd87ab73-80fb-4d62-8970-81dc945af40a
- 上一版協作分頁原始碼：`C:\Users\AwayWork\Desktop\WORKSPACE2\AwayTerminal_ORG`（commit fe06799 → 275006d；`CLAUDE.md` 裡「Claude+Codex 協作分頁」「測試模式」段落）
- 上一版測試工具（可複製改用）：`…\ec343f65-6643-4562-a649-b1f8b3f60fac\scratchpad\{fakeagent, render(coweb.js 對真 index.html 的 puppeteer harness, mksettings.js), uihost, probe}`
- 角色檔原稿：`C:\Users\AwayWork\Desktop\tmp.txt`
- 上一版失敗實錄：`%LOCALAPPDATA%\AwayTerminal\diag.log` 2026-09-13 16:45 起的 `cowork` 行（DONE 後 paused, pending）
- 各家 CLI 文件（2026-09-14 查證）：Claude `--append-system-prompt-file`／`--mcp-config`／`--settings`；Codex `developer_instructions` 只有字串版（issue #12926 not-planned）、`-c` 值先當 TOML 解析；OpenCode `OPENCODE_CONFIG_CONTENT`＋`--agent`、`--prompt`；Gemini `GEMINI_SYSTEM_MD` 整份取代、`-i`。
