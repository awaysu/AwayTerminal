#!/usr/bin/env node
// AwayTerminal 沙盒模式的指令護欄（Claude Code `PreToolUse` hook）。
//
// 由 `src-tauri/src/sandbox.rs` 在沙盒工作區產生一份（`.claude/awayterm-sandbox-guard.mjs`），
// 並掛進 `.claude/settings.local.json` 的 `PreToolUse`（matcher `Bash`）。
//
// 為什麼用 Node 而不是 PowerShell/sh：
//   1. hook 的輸入是 **stdin 上的一包 JSON**。`sh` 沒有 jq 解不動；PowerShell 7 在
//      mac/Linux 不保證裝了。
//   2. Claude Code 自己就要 Node → 「有 Claude Code 的地方一定有 node」。
//   3. 一支檔案三個平台通用，不必維護兩份會漂掉的實作。
//
// ⚠️ 這是**防呆不是防壞**：agent 只要自己寫一支腳本再執行就繞過去了。
// 真正的隔離是第三層（VM／Windows Sandbox），見 docs/AGENT-SANDBOX.md。
//
// 協定（Claude Code hooks）：stdin 收 JSON，輸出 JSON 到 stdout。
//   允許 → `{}`（或不輸出）
//   拒絕 → `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny",
//            "permissionDecisionReason":"…"}}`
// 這支腳本**任何情況都以 exit 0 結束**：hook 自己壞掉不應該讓 agent 整個動不了。

const RULES = [
  // ---- 按「名稱」砍行程：會把使用者正在用的程式（甚至這個團隊自己）一起砍掉 ----
  {
    id: 'taskkill-by-name',
    re: /\btaskkill\b[^|;&]*\/im\b/i,
    why: '用 /IM 按名稱砍行程會連使用者正在用的同名程式一起砍掉。要砍請用 /PID 指定你自己啟動的行程。',
  },
  {
    id: 'stop-process-by-name',
    re: /\bstop-process\b[^|;&]*(-name\b|-processname\b)/i,
    why: '用 -Name 砍行程會連使用者的同名程式一起砍掉。請用 -Id 指定你自己啟動的行程。',
  },
  {
    id: 'get-process-pipe-stop',
    re: /\bget-process\b[^|]*\|\s*[^|]*\bstop-process\b/i,
    why: 'Get-Process | Stop-Process 會砍掉一整批行程，包含使用者的。',
  },
  {
    id: 'pkill-killall',
    re: /(^|[;&|]\s*)(pkill|killall)\b/i,
    why: 'pkill / killall 是按名稱砍行程。請用 kill <pid> 指定你自己啟動的行程。',
  },
  // ---- 動到整台機器 ----
  {
    id: 'shutdown',
    re: /(^|[;&|]\s*)(shutdown|logoff|restart-computer|stop-computer|reboot)\b/i,
    why: '沙盒模式不允許關機／登出／重新啟動——使用者正在用這台電腦。',
  },
  // ---- 砍沙盒以外的路徑 ----
  {
    id: 'delete-outside-sandbox',
    check: (cmd, ctx) => {
      const del = /(\brm\b[^|;&]*-[a-z]*[rf]|\bremove-item\b[^|;&]*-recurse|\brmdir\b[^|;&]*\/s|\bdel\b[^|;&]*\/s)/i;
      if (!del.test(cmd)) return null;
      const outside = pathsOutsideSandbox(cmd, ctx.root);
      if (outside.length === 0) return null;
      return `遞迴刪除指到沙盒以外的路徑：${outside.join('、')}`;
    },
  },
  // ---- git：別破壞使用者的歷史 ----
  {
    id: 'git-push-force',
    check: (cmd) => {
      // 一條指令列可能串好幾段（; && || |），每段各自判斷
      for (const seg of cmd.split(/[;&|]+/)) {
        const why = checkPushSegment(seg);
        if (why) return why;
      }
      return null;
    },
  },
  {
    id: 'git-worktree-remove',
    re: /\bgit\b[^|;&]*\bworktree\b[^|;&]*\b(remove|prune)\b/i,
    why: 'worktree 是沙盒本身，請用分頁右鍵的「清除沙盒…」。',
  },
  {
    id: 'git-branch-delete',
    re: /\bgit\b[^|;&]*\bbranch\b[^|;&]*(-D\b|--delete\b)/i,
    why: '刪分支可能刪掉還沒合併回去的沙盒成果。',
  },
];

/** 沒用 `=` 帶值時，會把下一個引數當成值吃掉的 git push 選項。 */
const PUSH_OPTS_WITH_VALUE = new Set(['-o', '--push-option', '--receive-pack', '--exec', '--repo']);

/**
 * 判斷一段 `git push …` 是不是「force push 到沙盒分支以外」。回傳 deny 原因或 null。
 *
 * 以前只看整段字串有沒有 `sandbox/`，`git push -f origin sandbox/x:main` 照樣放行（BUG D9）。
 * 現在解析 refspec：**每一個目的端**都是 `sandbox/…` 才放行；沒寫 refspec（推 upstream／
 * push.default）、`--all`／`--mirror` 看不出目的端 → 擋。`+src:dst` 也算 force。
 */
function checkPushSegment(seg) {
  const tokens = seg
    .trim()
    .split(/\s+/)
    .map((t) => t.replace(/^["']|["']$/g, ''));
  const gi = tokens.findIndex((t) => /^git(\.exe)?$/i.test(t));
  if (gi < 0) return null;
  const pi = tokens.findIndex((t, i) => i > gi && t.toLowerCase() === 'push');
  if (pi < 0) return null;
  let force = false;
  let wide = false; // --all／--mirror／--branches：一次推一大批，看不出目的端
  const positional = [];
  for (let i = pi + 1; i < tokens.length; i++) {
    const t = tokens[i];
    if (t === '') continue;
    if (t.startsWith('--')) {
      const name = t.split('=')[0].toLowerCase();
      if (name === '--force' || name === '--force-with-lease' || name === '--force-if-includes') force = true;
      if (name === '--all' || name === '--mirror' || name === '--branches') wide = true;
      if (PUSH_OPTS_WITH_VALUE.has(name) && !t.includes('=')) i++;
      continue;
    }
    if (t.startsWith('-') && t.length > 1) {
      if (PUSH_OPTS_WITH_VALUE.has(t)) {
        i++;
        continue;
      }
      if (t.slice(1).includes('f')) force = true; // -f、-uf、-fu…
      continue;
    }
    positional.push(t);
  }
  const refspecs = positional.slice(1); // 第一個位置引數是 remote
  if (refspecs.some((r) => r.startsWith('+'))) force = true;
  if (!force) return null;
  const dest = (r) => {
    const s = r.replace(/^\+/, '');
    const d = s.includes(':') ? s.slice(s.indexOf(':') + 1) : s;
    return d.replace(/^refs\/heads\//i, '');
  };
  // 推自己的沙盒分支可以（那是這個分頁自己開的）
  if (!wide && refspecs.length > 0 && refspecs.every((r) => /^sandbox\/\S+/i.test(dest(r)))) return null;
  return 'force push 會覆寫遠端歷史。只有目的端是沙盒分支（sandbox/…）、而且明確寫出 refspec 才允許。';
}

/** 從指令裡撈出看起來像「絕對路徑」或「往上跳」的引數，回傳不在沙盒底下的那些。 */
function pathsOutsideSandbox(cmd, root) {
  const out = [];
  // Windows 磁碟機路徑、Unix 絕對路徑、UNC、以及任何含 `..` 的相對路徑
  const re = /(?:"[^"]+"|'[^']+'|\S+)/g;
  const norm = (p) => p.replace(/^["']|["']$/g, '').replace(/\\/g, '/').toLowerCase();
  const rootNorm = root ? norm(root).replace(/\/+$/, '') : '';
  for (const raw of cmd.match(re) || []) {
    const p = norm(raw);
    const absolute = /^[a-z]:\//.test(p) || p.startsWith('/') || p.startsWith('//');
    const climbs = p.includes('../');
    if (!absolute && !climbs) continue;
    if (p.startsWith('-')) continue; // 是旗標不是路徑
    if (absolute && rootNorm && (p === rootNorm || p.startsWith(rootNorm + '/'))) continue;
    out.push(raw);
  }
  return out;
}

/** 對一條 Bash 指令做判斷。回傳 deny 原因，或 null＝放行。 */
export function checkCommand(cmd, root) {
  if (typeof cmd !== 'string' || cmd.trim() === '') return null;
  const ctx = { root: root || process.env.AWAYTERM_SANDBOX_ROOT || '' };
  for (const rule of RULES) {
    if (rule.re && rule.re.test(cmd)) return `${rule.why}`;
    if (rule.check) {
      const why = rule.check(cmd, ctx);
      if (why) return why;
    }
  }
  return null;
}

const SUFFIX =
  '\n（AwayTerminal 沙盒模式擋下。要關掉：分頁右鍵 →「沙盒模式」取消勾選，下次啟動該分頁生效。）';

async function main() {
  let raw = '';
  for await (const chunk of process.stdin) raw += chunk;

  let payload = {};
  try {
    payload = JSON.parse(raw || '{}');
  } catch {
    return; // 讀不懂就放行——hook 壞掉不該讓 agent 動不了
  }

  const input = payload.tool_input || payload.toolInput || {};
  const reason = checkCommand(input.command, process.env.AWAYTERM_SANDBOX_ROOT);
  if (!reason) return;

  process.stdout.write(
    JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'deny',
        permissionDecisionReason: reason + SUFFIX,
      },
    })
  );
}

// 被當模組 import（測試）時不要跑 main
if (import.meta.url === `file://${process.argv[1]}` || process.argv[1]?.endsWith('sandbox-guard.mjs')) {
  main().catch(() => {}); // 任何錯誤都不要讓 exit code 非 0
}
