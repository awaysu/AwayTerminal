# 沙盒模式

規格來源：`CLAUDE.md`「新增功能（舊版沒有，使用者已定案）→ 沙盒模式」。
**舊版沒有這個功能**，所以這份文件是規格＋實作說明，不是搬移對照。

目的：**AI agent（Claude Code、Codex、Gemini CLI、OpenCode…）測試時不要影響使用者
正在用的這台電腦。**

---

## ⚠️ 先講清楚：前兩層是防呆，不是防壞

第 1、2 層擋的是「agent 照常做事但手滑」——它們**不是安全邊界**：

- agent 與使用者在**同一個 Windows 登入工作階段**，桌面、視窗、焦點、剪貼簿都碰得到。
- 指令護欄是比對 Bash 指令字串。agent 只要自己寫一支 `.ps1` 再執行、或用 Python、
  或換個寫法（`taskkill /F /IM` → `Get-CimInstance … | Invoke-CimMethod`），就繞過去了。
- worktree 只隔離**檔案**。agent 仍然可以寫到 repo 外的任何地方（那需要第 3 層）。

真正的隔離只有**第 3 層**（把 agent 放進 VM）。前兩層的價值是：讓「常見的意外」
（砍到使用者的程式、刪到別的資料夾、force push 蓋掉歷史）變成一個明確的拒絕訊息。

---

## 三層

| 層 | 擋什麼 | 擋不了什麼 | 狀態 |
|---|---|---|---|
| 1 工作區隔離 | agent 改到的檔案都在自己的 worktree 裡；編譯產物與暫存檔不污染主工作區 | 寫到 repo 以外的路徑 | ✅ 已實作 |
| 2 行程與指令護欄 | 分頁關掉就把 agent 開的整棵行程樹收乾淨；常見的危險 Bash 指令被拒絕 | 繞過 hook 的寫法、非 Bash 工具、GUI 操作 | ✅ 已實作 |
| 3 桌面隔離 | 桌面、視窗、焦點、其他行程 | （需要 VM，代價見下） | ⬜ 只有這份文件 |

---

## 第 1 層：工作區隔離

### git worktree

工作目錄在 git repo 裡（子目錄也算，程式問 `git rev-parse --show-toplevel`）時：

```
git worktree add -b sandbox/<連線名>-<yyyymmdd-hhmm> <repo>/.ai/sandbox/<連線名>
```

- 從**目前 HEAD** 開分支，所以 agent 看到的是「最後一次 commit 的狀態」。
- 分頁的工作目錄就是那個 worktree。
- 同名沙盒已經存在（同一條連線重開）→ 直接用它，不會重複 `add`。
- `.ai/sandbox/` 會被加進 **`.git/info/exclude`**（不是 `.gitignore`）：那是本機、不進版本
  控制的忽略清單，所以**不會動到使用者會 commit 的檔案**。

工作目錄**不是** git repo 時：沒有 worktree，只做環境變數與第 2 層，分頁 tooltip 會寫
「沙盒（無 worktree，不是 git repo）」，分頁列的沙盒標記是灰色而不是綠色。

### 環境變數

| 變數 | 導到 | 條件 |
|---|---|---|
| `TEMP` / `TMP` | `<沙盒根>/.tmp` | 一律 |
| `CARGO_TARGET_DIR` | `<沙盒根>/.target` | 工作區有 `Cargo.toml` 才設 |
| `AWAYTERM_SANDBOX_ROOT` | 沙盒工作目錄 | 一律（護欄腳本要用） |

**絕對不動** `HOME`／`APPDATA`／`LOCALAPPDATA`／`USERPROFILE`（`CLAUDE.md` 明寫）：
Claude Code、Codex 的登入狀態存在那裡，改了 agent 會掉登入。

環境變數是**疊在現有環境之上**的覆寫，不是「只給這幾個」——否則子行程會失去 `PATH`、
`SystemRoot` 等一切東西（`conpty.rs` 的 `build_env_block`）。

---

## 第 2 層：行程與指令護欄

### Job Object（Windows）

子行程用 `CREATE_SUSPENDED` 建立 → `AssignProcessToJobObject` → `ResumeThread`。
Job 設了 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，所以**session 被關掉（分頁關閉／程式結束）
的那一刻，job 裡剩下的行程全部被系統終止**。agent 開的 `node`、`git`、`cargo`、
以及它們開的孫行程都在同一個 job 裡。

先 suspend 再掛 job 的原因：不然子行程有機會在「還沒被放進 job」之前就開出孫行程，
那個孫行程會漏掉。

> **為什麼這條特別重要**：這個開發團隊自己就跑在舊版 AwayTerminal 底下
> （`.ai/bus/0014`）。按名稱砍行程（`taskkill /IM AwayTerminal.exe`）會把團隊連同自己
> 一起砍掉。Job Object 是「只收我自己開的那一棵」的正確做法——它按 handle 收，不按名稱。

mac／Linux：`#[cfg]` 已分開，之後用 process group（`setsid` + 關閉時 `killpg`）。
目前非 Windows 平台**沒有**這一層。

#### ⚠️ 已知漏洞：Microsoft Store 的 app execution alias（2026-09-27 實測）

用 `%LOCALAPPDATA%\Microsoft\WindowsApps\` 底下那種**別名**啟動的行程
（很多機器上的 `pwsh` 就是；`winget`、`python` 也常是）**不會進我們的 job**：
真正的行程是由 **AppX 啟動服務**建立的，不是我們的子行程建立的，所以
`AssignProcessToJobObject` 的繼承規則管不到它 → 關 job 收不掉。

- 證據：`cargo run --example job_probe`（用 System32 的 `powershell.exe`）→ `PROBE PASS`，
  孫行程被收掉；`cargo run --example job_probe -- --pwsh`（Store 別名）→ 孫行程活著。
- 影響：agent 或巨集若是用別名啟動的解譯器跑東西，分頁關掉後那一棵可能留著。
- 怎麼避開：自訂連線的「路徑」欄**填真實的 exe**（例如
  `C:\Program Files\PowerShell\7\pwsh.exe`）而不是別名。`sandbox_probe` 找 `pwsh` 時
  已經優先回真實路徑。
- 這又是一個「前兩層是防呆不是防壞」的實例——不要把 Job Object 當成保證。

### 指令護欄（Claude Code hook）

啟動時在沙盒工作區產生兩個檔案（**只寫沙盒目錄，不碰使用者原本的設定**）：

```
<沙盒>/.claude/awayterm-sandbox-guard.mjs   ← 護欄本體
<沙盒>/.claude/settings.local.json          ← PreToolUse hook（matcher: Bash）
```

`settings.local.json` 已經存在時**合併 hooks 而不是蓋掉**（worktree 從 HEAD 開所以通常
沒有，但使用者可能 commit 過一份）；同一支腳本只會掛一次，重開分頁不會愈加愈多。

#### 為什麼用 Node 寫

1. hook 的輸入是 **stdin 上的一包 JSON**。`sh` 沒有 `jq` 解不動；PowerShell 7 在
   mac/Linux 不保證裝了。
2. **Claude Code 自己就需要 Node** → 「有 Claude Code 的地方一定有 node」。
3. 一支檔案三個平台通用，不必維護兩份會漂掉的實作。

腳本**任何情況都以 exit 0 結束**：護欄自己壞掉不應該讓 agent 整個動不了。

#### 拒絕清單

| 規則 | 例子 | 為什麼 |
|---|---|---|
| `taskkill /IM` | `taskkill /F /IM pwsh.exe` | 按名稱砍會連使用者的同名程式一起砍 |
| `Stop-Process -Name` / `-ProcessName` | `Stop-Process -Name node` | 同上 |
| `Get-Process … \| Stop-Process` | | 一次砍一整批 |
| `pkill` / `killall` | | 同上（mac/Linux） |
| `shutdown` / `logoff` / `Restart-Computer` / `reboot` | | 使用者正在用這台電腦 |
| 遞迴刪除指到沙盒外 | `rm -rf C:/Users/…`、`rm -rf ../../x` | 刪到別人的東西 |
| `git push --force` / `--force-with-lease` | | 覆寫遠端歷史（**推 `sandbox/…` 分支放行**） |
| `git worktree remove` / `prune` | | worktree 就是沙盒本身 |
| `git branch -D` / `--delete` | | 可能刪掉還沒合併的沙盒成果 |

**放行**的例子（護欄太嚴會讓 agent 什麼都做不了，所以這些一定要通）：
`taskkill /PID 1234`、`Stop-Process -Id 1234`、`kill 1234`、
`git push origin sandbox/x`、`git push origin main`、`git branch -a`、
`rm -rf ./target`、`rm -rf <沙盒>/…`、`cargo build`、`npm test`。

測試：`node scripts/test-sandbox-guard.mjs`（deny 清單每條至少一例 + 15 個必須放行的例子）。

已知限制：我們**不解析 shell 引號**，所以 `echo "taskkill /IM 很危險"` 也會被擋。
誤擋比漏擋安全，而且 agent 換個寫法就能繼續。

### 工具自己的沙盒參數

| 工具 | 追加的參數 |
|---|---|
| Codex | `--sandbox workspace-write` |
| Gemini CLI | `--sandbox` |
| Claude Code | （走 hook，不加參數） |
| 其他 | （只有第 1 層 + Job Object） |

⚠️ 這兩個參數名是照 `CLAUDE.md` 寫的，**還沒有在本機實際跑過那兩個工具驗證**
（這台機器沒裝）。要請使用者確認。

---

## 第 3 層：桌面隔離（只有文件）

唯一真正保護桌面（焦點、視窗、其他行程）的做法。三個選項：

| 做法 | 怎麼做 | 代價 |
|---|---|---|
| **Windows Sandbox** | 開 Windows 功能「Windows 沙箱」，用 `.wsb` 設定檔把專案目錄唯讀／可寫掛進去，在裡面裝 Node + agent CLI | **每次開都是全新環境**：agent 的登入狀態不會保留（每次要重新登入），裝東西也要重來。適合一次性的危險測試，不適合日常 |
| **Hyper-V VM** | 建一台長期存在的 Windows VM，agent 只在裡面跑 | 保留登入與安裝；代價是記憶體（4~8GB）、要維護一台 VM、檔案要走共用資料夾或 git |
| **WSL2** | agent 跑在 Linux 子系統裡 | 最輕（共用檔案系統、啟動快）。但**不隔離 Windows 桌面**——WSL 裡的程式可以呼叫 `powershell.exe` 回到 Windows；而且 Windows 版的 agent CLI 與 Linux 版設定不通 |

結論（`CLAUDE.md`）：先寫成選項，**階段 4 再評估做成「在 VM 中開團隊」**。
真正要「agent 完全碰不到我的桌面」的時候，Hyper-V VM 是唯一夠格的答案。

---

## 工作流程：agent 只看得到已 commit 的內容

**這是使用者一定要知道的一件事。**

沙盒的 worktree 是從 **目前 HEAD** 開的，所以：

- agent **看不到**你還沒 commit 的修改（工作區的改動、staged 但沒 commit 的東西）。
- agent 的成果留在它自己的分支 `sandbox/<連線名>-<時間>` 上，**不會**自動出現在你的
  主工作目錄。

### 要讓 agent 看得到你的修改

先 commit（可以是 WIP commit，之後 `git commit --amend` 或 rebase 整理），再開沙盒分頁。
已經開著的分頁要吃到新的 HEAD 就得重開（右鍵「沙盒模式」關再開，或關掉分頁重開）。

### 把 agent 的成果拿回來

```bash
# 看 agent 做了什麼
git log --oneline main..sandbox/ClaudeCode-20260926-2200
git diff main...sandbox/ClaudeCode-20260926-2200

# 併回來（三種常見選擇）
git merge sandbox/ClaudeCode-20260926-2200              # 保留分支歷史
git cherry-pick <某幾個 commit>                          # 只要其中幾個
git checkout sandbox/... -- path/to/file                # 只要某幾個檔案
```

⚠️ agent **沒有 commit** 的改動只存在那個 worktree 目錄裡。清除沙盒（`git worktree remove
--force`）會**連那些未 commit 的改動一起刪掉**——所以「清除沙盒…」有確認對話框，
而且**分支一律保留**。

### 清沙盒

- 分頁右鍵 →「清除沙盒…」→ 確認後 `git worktree remove --force`，**分支保留**。
- 分頁關閉時**不會**自動清（裡面可能有未 commit 的成果）。
- 手動全清：`git worktree prune` 之後刪掉 `.ai/sandbox/`；分支用
  `git branch --list 'sandbox/*'` 列出來自己決定要不要刪。

---

## 關掉沙盒

- 分頁右鍵 →「沙盒模式」取消勾選。勾勾顯示的是**連線設定**的值。
- `CLAUDE.md`：**改變在下次啟動該分頁時生效**。所以切換後程式會問「要現在重新啟動這個
  分頁嗎？」——按「重新啟動分頁」就關掉目前連線（走優雅結束鍵）再用新設定開一個。
- 也可以在「新分頁 ▾ → 自訂連線設定…」裡對每條連線改。
- WSL 與 ADB 這兩條自動偵測加入的連線**預設是關的**：它們是「使用者拿來操作機器」的
  工具，開沙盒只會讓人莫名其妙進到一個 worktree 裡。

## 沒有沙盒的連線類型

PowerShell 分頁、SSH 分頁、「自訂指令…」開的臨時分頁都**沒有**沙盒選項——
沙盒掛在「自訂連線」的設定上（`CLAUDE.md` 的定義）。SSH 更是沒有本機子行程可以隔離。
