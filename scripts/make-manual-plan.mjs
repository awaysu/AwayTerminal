// 從 `docs/REGRESSION-CHECKLIST.md` 的 👤 條目產生 `docs/MANUAL-TEST-PLAN.md`。
//
// 為什麼要產生、不手寫：清單裡有 280 幾條 👤，手抄一份就一定會和清單對不上
// （清單是行為規格，改了規格卻忘了改測試計畫＝計畫在測錯的東西）。
// 這支腳本只負責**分組與排版**，「怎麼做／預期」一律照清單原文。
//
//   node scripts/make-manual-plan.mjs          產生檔案
//   node scripts/make-manual-plan.mjs --check  只檢查是不是最新（CI／發佈前用）
//
// 加了新章節就到下面的 PLAN 裡排一個位置；沒排到的章節會被放進「未分類」並警告。

import { readFileSync, writeFileSync } from 'node:fs';

const SRC = 'docs/REGRESSION-CHECKLIST.md';
const OUT = 'docs/MANUAL-TEST-PLAN.md';

/**
 * 優先度與分節。`secs` 是章節代號（清單裡 `## X.` 的 X）。
 *
 * P0＝舊版的核心行為，新版退步了使用者馬上會撞到；一定要在每次發佈前跑。
 * P1＝常用但不是每天都碰的功能。
 * P2＝新功能與周邊（舊版沒有的，或需要特別環境／帳號的）。
 */
const PLAN = [
  {
    p: 'P0',
    title: '核心：分頁、檢視、終端機操作',
    mins: 25,
    secs: ['A', 'B', 'D', 'E', 'F'],
    note: '舊版每天都在用的部分。這一節有問題就不要發佈。',
  },
  {
    p: 'P0',
    title: '核心：輸入文字視窗與中文輸入',
    mins: 20,
    secs: ['CP'],
    note:
      '注音組字、多行送出。**這是整個重寫案最高的風險**（`CLAUDE.md` 風險 1）：' +
      '舊版的調校是針對 WebView2 調的。另外請一併跑下面「終端機裡的注音與貼上」那一節。',
  },
  {
    p: 'P0',
    title: '核心：SSH（要真的設備）',
    mins: 30,
    secs: ['K'],
    note:
      'SSH 從呼叫 `ssh.exe` 換成內建 `russh`，是改動最大的一塊。' +
      '請用**平常真的會連的那幾台**測，不要只測一台新的 Linux。',
  },
  {
    p: 'P0',
    title: '核心：恢復分頁與斷線重連',
    mins: 25,
    secs: ['R', 'M'],
    note: '關程式再開、拔網路線。畫面紀錄要能倒回去。',
  },
  {
    p: 'P1',
    title: '設定、語言、關於',
    mins: 30,
    secs: ['C', 'ST', 'LG', 'AB'],
    note: '八語是新的，每一種都要看有沒有爆版（德文／法文最長）。',
  },
  {
    p: 'P1',
    title: 'log、我的最愛、自訂連線',
    mins: 25,
    secs: ['G', 'L', 'P'],
  },
  {
    p: 'P1',
    title: 'Telnet 與連接埠（要真的設備／USB 線）',
    mins: 30,
    secs: ['TN', 'CM'],
    note: '連接埠要一條真的 USB 轉序列埠線。`serialport` 不支援的組合會降級並警告。',
  },
  {
    p: 'P1',
    title: 'TTL 巨集（要舊的 .ttl 檔）',
    mins: 30,
    secs: ['T'],
    note:
      '⚠️ 新版的 `and`／`or`／`xor`／`not` 是**位元運算**（照 TeraTerm 原碼），舊版 C# 版當成' +
      '邏輯運算。手上的 `.ttl` 如果依賴舊行為，結果會不一樣——這一節請拿真的檔案跑。',
  },
  {
    p: 'P2',
    title: '沙盒模式（新功能）',
    mins: 25,
    secs: ['Q'],
    note: '前兩層是防呆不是防壞（`docs/AGENT-SANDBOX.md`）。',
  },
  {
    p: 'P2',
    title: '代理團隊與 AI 聊天室',
    mins: 30,
    secs: ['MA', 'CH'],
    note: '會真的花掉 CLI 的額度。',
  },
  {
    p: 'P2',
    title: 'Telegram 遠端（要 bot 與手機）',
    mins: 25,
    secs: ['N'],
    note: '要自己建一個 bot（`docs/TELEGRAM.md` 的一分鐘版）。',
  },
  {
    p: 'P2',
    title: 'Windows 整合與舊版升級',
    mins: 25,
    secs: ['WS', 'AD', 'EX', 'MG'],
    note: 'ADB 要真的接一台手機；匯入舊設定請在**還沒刪掉舊版**的機器上測。',
  },
];

/** P0 多出來的一節：從 `docs/TERMINAL-JS-DIFF.md` 的踩雷表來，不在 checklist 裡。 */
const IME_SECTION = `### P0-IME　終端機裡的注音與貼上（約 25 分鐘）

這些條目**不在 \`REGRESSION-CHECKLIST.md\` 裡**——它們是舊版 \`CLAUDE.md\` 的踩雷紀錄，
對照表在 \`docs/TERMINAL-JS-DIFF.md\` 第二節。\`terminal.js\` 幾乎原封不動搬過來，
所以這些行為「應該」都還在，但**只有人在鍵盤前面才驗得出來**
（\`--verify\` 不能搶鍵盤焦點，見清單的「隱含契約」）。

先開一個 PowerShell 分頁，再開一個 Claude Code 分頁，兩個都跑一遍。

| # | 怎麼做 | 預期 |
|---|---|---|
| IME1 | 用微軟注音打一句中文，按 Enter | 整句進去、**只送一個 Enter**。不可以多一個換行、也不可以有殘留的組字字元 |
| IME2 | 打字中途按 Backspace 改字，再 Enter | 改掉的字不會出現在送出的內容裡 |
| IME3 | 組字中按 Esc 放棄 | 輸入框清空，**什麼都不送**（不會把半成品送進去） |
| IME4 | 打一個字，看候選字清單選第 3 個 | 選到的才送出，數字鍵不會被當成終端機輸入 |
| IME5 | 注音打到一半看畫面 | 組字視窗不閃英文字母（微軟注音每一鍵先回報原始鍵值再更新成注音） |
| IME6 | 用嘸蝦米／倉頡／拼音打一段 | 英數組字的過程**看得到**（1.0.8 曾經「含字母就永久隱藏」＝整段看不見） |
| IME7 | 中英切換來回幾次後繼續打 | 不會漏字、不會重複送出 |
| IME8 | 複製一段**多行**文字，Ctrl+V 貼到 PowerShell 分頁 | 整段進輸入框成為多行，**不會**前幾行被執行掉只剩最後一行 |
| IME9 | 同樣的多行文字貼到 **Claude Code** 分頁 | 換行變成軟換行（ESC+CR），claude 不會把每一行當成送出 |
| IME10 | 複製**單行**純文字貼到 PowerShell 分頁 | 有反應（1.2.6 修過：IME 水位檢查曾經把它整段丟掉，什麼都沒發生） |
| IME11 | 貼一段很長（>4KB）的文字 | 一次寫出、不卡住、沒有掉字 |
| IME12 | 按 Shift+Insert 貼上 | 和 Ctrl+V 一樣（兩條路都要走 \`doPaste\`） |

`;

// ⚠️ 清單是 CRLF（git 的 autocrlf）。不先正規化的話每一行都會多一個 `\r`，
// 章節標題的正規表示式對不上、表格最後一格也會帶 `\r` → 抓到 0 條（第一版就是這樣）。
const md = readFileSync(SRC, 'utf8').replace(/\r\n/g, '\n');
const lines = md.split('\n');

/** 章節代號 → { title, rows: [{id, what, exp}] } */
const chapters = new Map();
/** 在章節裡、帶 👤、但編號認不出來而沒收進計畫的列（稽核 I4：以前是靜默丟掉）。 */
const skipped = [];
let cur = null;
for (const [i, ln] of lines.entries()) {
  // 章節代號可以帶數字（`## K2.`、`## M2.`）——以前的 `[A-Z]{1,3}` 對不上，整章的 👤 都會漏掉
  const h = /^## ([A-Z]{1,3}\d*)\. (.+)$/.exec(ln);
  if (h) {
    cur = h[1];
    chapters.set(cur, { title: h[2].trim(), rows: [] });
    continue;
  }
  if (ln.startsWith('## ')) {
    cur = null;
    continue;
  }
  if (!cur || !ln.startsWith('|') || !ln.includes('👤')) continue;
  const cells = ln.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map((c) => c.trim());
  const [id, what, exp] = cells;
  // 編號可以帶小寫字母後綴（`TN7d`——同一條拆出來的子項）
  if (cells.length < 3 || !/^[A-Z]{1,3}\d+[a-z]*$/.test(id)) {
    skipped.push(`${SRC}:${i + 1}（${cur}）${id.slice(0, 40)}`);
    continue;
  }
  chapters.get(cur).rows.push({ id, what: what.replace('👤', '').trim(), exp });
}

const planned = new Set(PLAN.flatMap((s) => s.secs));
const unplanned = [...chapters.keys()].filter((k) => !planned.has(k) && chapters.get(k).rows.length);
const total = [...chapters.values()].reduce((n, c) => n + c.rows.length, 0);

const out = [];
out.push('# 手動回歸測試計畫');
out.push('');
out.push(
  '`docs/REGRESSION-CHECKLIST.md` 裡標 👤 的條目 ＝ **一定要人看畫面**才驗得出來的。' +
    `目前共 **${total} 條**。這份檔案把它們排成一次 20～30 分鐘跑得完的小節，讓你分批跑。`
);
out.push('');
out.push('> ⚠️ **這個檔案是產生出來的**，不要直接改：');
out.push('>');
out.push('> ```powershell');
out.push('> node scripts/make-manual-plan.mjs');
out.push('> ```');
out.push('>');
out.push(
  '> 內容（怎麼做／預期）一律照 `docs/REGRESSION-CHECKLIST.md` 的原文，' +
    '要改就去改清單；分組與優先度改 `scripts/make-manual-plan.mjs` 的 `PLAN`。'
);
out.push('');
out.push('## 怎麼用');
out.push('');
out.push('| 優先度 | 什麼 | 什麼時候跑 |');
out.push('|---|---|---|');
out.push('| **P0** | 舊版的核心行為。退步了每天都會撞到 | **每次發佈前一定要跑** |');
out.push('| **P1** | 常用但不是每天碰的功能 | 每次發佈前，或改到相關部分時 |');
out.push('| **P2** | 新功能與周邊（需要特別環境／帳號的） | 改到那一塊時，或大版本發佈前 |');
out.push('');
out.push(
  '每一條的「對應編號」就是 `docs/REGRESSION-CHECKLIST.md` 的編號——' +
    '在那邊填 Win／mac／Linux 三欄的結果，不要填在這裡（清單才是紀錄）。'
);
out.push('');
const p0n = PLAN.filter((s) => s.p === 'P0').reduce(
  (n, s) => n + s.secs.reduce((m, k) => m + (chapters.get(k)?.rows.length || 0), 0),
  0
);
out.push(
  `P0 共 ${p0n + 12} 條（含下面的 IME 一節 12 條）、約 ${
    PLAN.filter((s) => s.p === 'P0').reduce((n, s) => n + s.mins, 0) + 25
  } 分鐘。`
);
out.push('');
out.push('---');
out.push('');

let lastP = '';
for (const sec of PLAN) {
  if (sec.p !== lastP) {
    out.push(`## ${sec.p}`);
    out.push('');
    lastP = sec.p;
    if (sec.p === 'P0') {
      out.push(IME_SECTION.trimEnd());
      out.push('');
    }
  }
  const rows = sec.secs.flatMap((k) => {
    const c = chapters.get(k);
    if (!c) {
      console.log(`[make-manual-plan] 警告：PLAN 提到的章節 ${k} 不在清單裡`);
      return [];
    }
    return c.rows;
  });
  const from = sec.secs
    .map((k) => (chapters.has(k) ? `${k}（${chapters.get(k).title}）` : k))
    .join('、');
  out.push(`### ${sec.p}-${sec.secs[0]}　${sec.title}（約 ${sec.mins} 分鐘，${rows.length} 條）`);
  out.push('');
  out.push(`來源章節：${from}`);
  if (sec.note) {
    out.push('');
    out.push(sec.note);
  }
  out.push('');
  out.push('| 對應編號 | 怎麼做 | 預期 |');
  out.push('|---|---|---|');
  for (const r of rows) out.push(`| ${r.id} | ${r.what} | ${r.exp} |`);
  out.push('');
}

if (unplanned.length) {
  out.push('## 未分類');
  out.push('');
  out.push(
    `這些章節還沒排進 \`PLAN\`：${unplanned.join('、')}。` +
      '請到 `scripts/make-manual-plan.mjs` 補一個位置。'
  );
  out.push('');
  for (const k of unplanned) {
    const c = chapters.get(k);
    out.push(`### ${k}. ${c.title}（${c.rows.length} 條）`);
    out.push('');
    out.push('| 對應編號 | 怎麼做 | 預期 |');
    out.push('|---|---|---|');
    for (const r of c.rows) out.push(`| ${r.id} | ${r.what} | ${r.exp} |`);
    out.push('');
  }
}

const text = out.join('\n');
if (skipped.length) {
  console.log(`[make-manual-plan] ⚠ ${skipped.length} 條帶 👤 的列編號認不出來，沒有收進計畫：`);
  for (const s of skipped) console.log(`[make-manual-plan]     ${s}`);
  console.log('[make-manual-plan]     編號要長得像 `A12`／`TN7d`；確定不是測項就把 👤 拿掉。');
}
if (process.argv.includes('--check')) {
  if (skipped.length) {
    console.log('RESULT: FAIL（有 👤 條目沒收進計畫，見上面）');
    process.exit(1);
  }
  let cur2 = '';
  try {
    cur2 = readFileSync(OUT, 'utf8');
  } catch {
    /* 檔案還不存在 */
  }
  if (cur2.replace(/\r\n/g, '\n') !== text) {
    console.log(`❌ ${OUT} 不是最新的——跑 \`node scripts/make-manual-plan.mjs\` 重新產生。`);
    console.log('RESULT: FAIL');
    process.exit(1);
  }
  console.log(`${OUT} 是最新的（${total} 條 👤）`);
  console.log('RESULT: PASS');
} else {
  writeFileSync(OUT, text, 'utf8');
  console.log(
    `寫出 ${OUT}：${total} 條 👤，${PLAN.length} 節${
      unplanned.length ? `，未分類 ${unplanned.length} 章` : ''
    }`
  );
}
