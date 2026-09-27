// 舊版 CLAUDE.md 的「踩雷紀錄」× 我們的回歸清單：覆蓋稽核。
//
//   node scripts/audit-pitfalls.mjs              印出三張表
//   node scripts/audit-pitfalls.mjs --check      條數和登記的不一樣就 FAIL（發佈前用）
//   node scripts/audit-pitfalls.mjs --list       只列出每一條與它的分類
//
// 對應 `CLAUDE.md` 風險 12：「把舊版 CLAUDE.md 的踩雷紀錄整理成可逐項勾選的手動回歸
// 測試清單」。TASK-023 做了一次人工稽核（33 已覆蓋／17 不適用／5 漏掉），結論寫進
// `docs/REGRESSION-CHECKLIST.md` 的「踩雷紀錄覆蓋稽核」那一節。
//
// # 這支腳本做什麼、不做什麼
//
// **做**：①數舊版有幾條（條數變了就是舊版 CLAUDE.md 更新過、要重新稽核）
//        ②用關鍵字粗篩每一條在我們的文件裡有沒有被提到
//        ③把下面 VERDICT 裡**人工**下的判斷印出來對照
//
// **不做**：自動判斷「有沒有覆蓋」。關鍵字命中 ≠ 覆蓋——TASK-023 的 55 條裡有 45 條
// 命中，但真正的分類要讀原文才知道（10 條沒命中的全部讀過）。所以判斷寫死在
// VERDICT 裡，這支腳本是**回歸偵測器**：舊版多了新條目、或我們的文件不再提到某個
// 關鍵字時會叫，提醒人再看一次。
//
// 舊版 repo 沒 clone 的話會直接說「跳過」並回 PASS——CI／別台機器上不該因此失敗。

import { existsSync, readFileSync, readdirSync } from 'node:fs';

const V1 = process.env.AWAYTERM_V1_CLAUDEMD || 'reference/AwayTerminal/CLAUDE.md';
const DOCS = [
  'docs/REGRESSION-CHECKLIST.md',
  'docs/TERMINAL-JS-DIFF.md',
  'docs/DEV-SETUP.md',
  'docs/RELEASE.md',
  'docs/MANUAL-TEST-PLAN.md',
  'docs/TELEGRAM.md',
  'docs/MULTI-AGENT.md',
  'docs/PROTOCOL.md',
];

/** TASK-023 稽核時舊版有幾條。條數不一樣＝舊版更新過，要重新稽核。 */
const EXPECTED_TOTAL = 55;

/**
 * 人工判斷（TASK-023）。`c` = covered、`n` = not applicable、`f` = fixed here（漏掉→補上）。
 * 第二個欄位是「補在哪／覆蓋在哪」，第三個是粗篩用的關鍵字。
 */
const VERDICT = {
  1: ['c', 'K/M/MA/AB/TG 各章＋TG2/TG18/TG24＋新增 C6', ['login as', 'offset=-1', '選單']],
  2: ['c', 'finds_new_output_above_a_fixed_input_box ＋ TG23', ['固定輸入框', '增量']],
  3: ['c', 'MA19', ['Ctrl+U']],
  4: ['c', 'MA19、MA20（tui.whimsy）', ['whimsy', '星星']],
  5: ['c', 'MA 章（角色檔講清楚）', ['子代理', '角色檔']],
  6: ['c', 'MA 章的已知現象（舊版也沒修）', ['少一行']],
  7: ['c', 'A 章的捲動項＋`S` 協定', ['往上捲', '捲動']],
  8: ['f', '補進 docs/DEV-SETUP.md ＋ 修掉 gen-bigfile.ps1 缺 BOM（TASK-024 發現 TASK-023 誤判）', ['UTF-8 **with BOM**']],
  9: ['c', 'TG16（輪詢失敗要退避並恢復）', ['退避', '逾時']],
  10: ['n', 'WPF SynchronizationContext；新版對應的是「等前端回覆的 command 要 async」', ['async']],
  11: ['c', 'MA19、TG8（文字與 Enter 分開送）', ['分開送', 'Enter 隔']],
  12: ['c', 'MA 章的已知現象', ['/tui', 'ALTERNATE_SCREEN']],
  13: ['c', 'pty/conpty_host.rs ＋ backend_name', ['OpenConsole']],
  14: ['c', 'docs/PROTOCOL.md ＋ conpty.rs 註解', ['ESC[2J', '第一幀']],
  15: ['c', 'docs/TERMINAL-JS-DIFF.md 第二節 ＋ P0-IME', ['_inputEvent', 'compositionend']],
  16: ['c', 'R5～R7 ＋ 隱含契約', ['held', 'scrollback']],
  17: ['n', '侵入式 UI 自動化；新版 --verify 不搶前景、不點擊', ['前景']],
  18: ['c', 'with_null_std_handles 的註解 ＋ A 章', ['std handle']],
  19: ['c', 'MA19（逐字節流）', ['逐字節', '輸入塊']],
  20: ['n', '舊版自己的顯示殘影，新版沒有那段程式', ['殘影']],
  21: ['n', 'UI 自動化的 SendKeys；新版不用', ['SendKeys']],
  22: ['c', 'scripts/dev-verify.mjs（依 PID 收行程樹）＋ 隱含契約', ['os error 32', 'conhost']],
  23: ['c', 'docs/TERMINAL-JS-DIFF.md 貼上表 ＋ IME8', ['xterm.paste', '多行貼上']],
  24: ['c', '同上 ＋ IME9', ['ESC+CR', '軟換行']],
  25: ['c', '同上 ＋ IME5、IME6', ['注音', '組字']],
  26: ['n', 'UI 自動化的三個前置；新版不用', ['UI 自動化']],
  27: ['n', 'UI 自動化的座標漂移；新版不用', ['座標']],
  28: ['n', 'WebView2 airspace（WPF 疊在上面）；新版對話框都是頁內 DOM', ['airspace']],
  29: ['f', '新增 CM17（專屬執行緒 blocking read）', ['專屬執行緒', 'blocking read']],
  30: ['n', 'WebView2 自己伺服檔案的快取；新版 dev 走 Vite、release 內嵌', ['WebView2 快取']],
  31: ['c', 'docs/PROTOCOL.md ＋ A 章（alt-screen）', ['alt-screen']],
  32: ['c', 'startup.rs 的 clean_inherited_env', ['NO_COLOR']],
  33: ['c', '隱含契約（ConPTY 折行是硬換行）', ['isWrapped', '硬換行']],
  34: ['n', 'xterm 黑帶是舊版 WPF 版面的組合', ['黑帶']],
  35: ['n', '`windowsPty` 選項；新版沒加也不會加', ['windowsPty']],
  36: ['n', '舊版拿掉的實作，新版從來沒有', ['BEL']],
  37: ['c', 'restore.rs 的 exit_confirm（先 hide 再存）', ['Hide()']],
  38: ['n', 'fit 截行是舊版版面的組合', ['截行']],
  39: ['n', '初始尺寸同上', ['初始尺寸']],
  40: ['c', 'D13（清畫面要分開送 Esc 與 Ctrl+L）', ['控制字元']],
  41: ['n', 'WinForms；新版沒有', ['WinForms']],
  42: ['n', 'WinForms 對話框 owner；但最後那句教訓已一般化進隱含契約', ['owner']],
  43: ['c', 'D13', ['清除畫面', '清畫面']],
  44: ['n', 'WPF 的 UseLayoutRounding；新版是 CSS', ['髮絲']],
  45: ['f', '新增 ST16（深色下拉的灰字，新版是 CSS 但症狀可能一樣）', ['ComboBox', '下拉']],
  46: ['c', 'docs/DEV-SETUP.md（防毒鎖檔）', ['防毒']],
  47: ['f', '同第 8 條（舊版把同一件事記了兩次）', ['沒有 BOM 就用系統 ANSI']],
  48: ['f', '補進 docs/RELEASE.md 第 3 節', ['AuthenticodeSignature', 'UnknownError']],
  49: ['c', 'docs/DEV-SETUP.md（PC-cillin 擋剛建置的 exe）', ['PC-cillin']],
  50: ['n', '「私鑰不進 git」放 docs/RELEASE.md 比清單合適', ['私鑰']],
  51: ['c', 'docs/DEV-SETUP.md ＋ 隱含契約', ['ConvertFrom-Json']],
  52: ['f', '**程式已修**（AppSettings.extra ＋ 4 測試）＋ 新增 C6', ['不認識的欄位', '剝欄位']],
  53: ['c', '隱含契約有一條專門寫它 ＋ TG8、TG10', ['位元組流']],
  54: ['f', '新增 TG42（已知缺口）', ['孤兒行', '歡迎框']],
  55: ['c', 'telegram_probe（假 Bot API）＋ TG1～TG19', ['假 Bot API']],
};

const LABEL = { c: '✅ 已覆蓋', n: '➖ 不適用', f: '➕ 漏掉→補上' };

if (!existsSync(V1)) {
  console.log(`跳過：找不到舊版 CLAUDE.md（${V1}）。`);
  console.log('舊版 repo 沒 clone 的話這支腳本沒有東西可以比對——這不是錯誤。');
  console.log('（要指定別的路徑：環境變數 AWAYTERM_V1_CLAUDEMD）');
  console.log('RESULT: PASS');
  process.exit(0);
}

// ---------------------------------------------------------- 讀舊版的踩雷紀錄
const md = readFileSync(V1, 'utf8').replace(/\r\n/g, '\n');
const lines = md.split('\n');
const start = lines.findIndex((l) => l.startsWith('## 踩雷紀錄'));
if (start < 0) {
  console.log(`❌ ${V1} 裡找不到「## 踩雷紀錄」那一節——舊版的結構變了，請人工確認。`);
  console.log('RESULT: FAIL');
  process.exit(1);
}
let end = lines.length;
for (let i = start + 1; i < lines.length; i++) {
  if (lines[i].startsWith('## ')) {
    end = i;
    break;
  }
}
const section = lines.slice(start, end);
const bullets = section.filter((l) => l.startsWith('- '));
const circled = section.join('\n').match(/[①-⑳]/g)?.length ?? 0;

/** 每一條的標題（開頭的粗體，沒有就取前 60 字）。 */
const titles = bullets.map((l) => {
  const body = l.slice(2);
  const m = /^\*\*(.+?)\*\*/.exec(body);
  return (m ? m[1] : body.slice(0, 60)).replace(/\s+/g, ' ');
});

// ---------------------------------------------------------- 讀我們的文件
let ours = '';
for (const d of DOCS) {
  try {
    ours += readFileSync(d, 'utf8');
  } catch {
    /* 檔案還不存在就跳過 */
  }
}

// ---------------------------------------------------------- 比對
const counts = { c: 0, n: 0, f: 0 };
const noVerdict = [];
const keywordMiss = [];

for (let i = 1; i <= bullets.length; i++) {
  const v = VERDICT[i];
  if (!v) {
    noVerdict.push(i);
    continue;
  }
  counts[v[0]]++;
  // 粗篩：覆蓋類的條目，關鍵字應該在我們的文件裡找得到
  if (v[0] !== 'n') {
    const hit = v[2].some((k) => ours.includes(k));
    if (!hit) keywordMiss.push(i);
  }
}

const listMode = process.argv.includes('--list');
const checkMode = process.argv.includes('--check');

console.log(`舊版 ${V1}`);
console.log(`踩雷紀錄：第 ${start + 1}～${end} 行，**${bullets.length} 條**頂層條目、${circled} 個圈號子項`);
console.log('');
console.log(`| 結果 | 條數 |`);
console.log(`|---|---|`);
console.log(`| ${LABEL.c} | ${counts.c} |`);
console.log(`| ${LABEL.n} | ${counts.n} |`);
console.log(`| ${LABEL.f} | ${counts.f} |`);
console.log('');

if (listMode) {
  for (let i = 1; i <= bullets.length; i++) {
    const v = VERDICT[i];
    const tag = v ? LABEL[v[0]] : '❓ 沒有判斷';
    console.log(`${String(i).padStart(2, '0')} ${tag}  ${titles[i - 1].slice(0, 46)}`);
    if (v) console.log(`   → ${v[1]}`);
  }
  console.log('');
}

let fail = 0;

if (bullets.length !== EXPECTED_TOTAL) {
  console.log(
    `❌ 舊版的條數從 ${EXPECTED_TOTAL} 變成 ${bullets.length}——舊版 CLAUDE.md 更新過了。`
  );
  console.log('   請人工看新增／刪掉的是哪幾條，補進這支腳本的 VERDICT，');
  console.log('   並更新 docs/REGRESSION-CHECKLIST.md 的「踩雷紀錄覆蓋稽核」那一節。');
  fail++;
}

if (noVerdict.length) {
  console.log(`❌ 這幾條沒有人工判斷：${noVerdict.join('、')}`);
  console.log('   每一條都要讀原文再分類（c 已覆蓋／n 不適用／f 漏掉→補上）。');
  fail++;
}

if (keywordMiss.length) {
  console.log(
    `⚠️  這幾條標成「已覆蓋／已補上」，但關鍵字在我們的文件裡找不到：${keywordMiss.join('、')}`
  );
  console.log('   可能是文件被改寫了（關鍵字要跟著改），也可能是覆蓋真的不見了。');
  console.log('   **這是警告不是失敗**——關鍵字命中與否只是提示，判斷要靠讀文件。');
}

// ------------------------------------------------- 第二節：活體檢查
//
// 有些踩雷可以**直接檢查我們自己的樹裡有沒有正在踩**。分類表只說「有沒有寫進文件」，
// 這一節說「有沒有真的違規」——TASK-024 就是這樣發現 `gen-bigfile.ps1` 缺 BOM 的
// （TASK-023 把第 8／47 條誤判成已覆蓋，因為 DEV-SETUP 裡的 `BOM` 講的是 log 格式）。

console.log('');
console.log('活體檢查（舊版踩雷有沒有正在我們的樹裡發生）：');

/** 舊版第 8、47 條：`.ps1` 含非 ASCII 就必須有 UTF-8 BOM（PowerShell 5.1 否則用 ANSI 解碼）。 */
function checkPs1Bom() {
  const bad = [];
  const dirs = ['scripts', 'src-tauri', 'platform'];
  const walk = (dir) => {
    let entries = [];
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      const full = `${dir}/${e.name}`;
      if (e.isDirectory()) {
        if (e.name === 'target' || e.name === 'node_modules') continue;
        walk(full);
      } else if (e.name.endsWith('.ps1')) {
        const buf = readFileSync(full);
        const hasBom = buf[0] === 0xef && buf[1] === 0xbb && buf[2] === 0xbf;
        // 非 ASCII ＝ 有任何 byte >= 0x80
        const nonAscii = buf.some((b) => b >= 0x80);
        if (nonAscii && !hasBom) bad.push(full);
      }
    }
  };
  for (const d of dirs) walk(d);
  return bad;
}

const noBom = checkPs1Bom();
if (noBom.length) {
  console.log(`  ❌ 第 8／47 條：這些 .ps1 含非 ASCII 但**沒有 UTF-8 BOM**：`);
  for (const f of noBom) console.log(`     ${f}`);
  console.log('     PowerShell 5.1 會用系統 ANSI（這台機器是 Big5）解碼 → 中文變亂碼。');
  console.log('     修法：用 UTF-8 with BOM 存檔。說明見 docs/DEV-SETUP.md。');
  fail++;
} else {
  console.log('  ✓ 第 8／47 條：含非 ASCII 的 .ps1 都有 UTF-8 BOM');
}

if (fail) {
  console.log('');
  console.log('RESULT: FAIL');
  process.exit(1);
}
console.log('');
console.log(`對照表在 docs/REGRESSION-CHECKLIST.md 的「踩雷紀錄覆蓋稽核」。`);
console.log('RESULT: PASS');
if (checkMode && keywordMiss.length) {
  // --check 也只把「條數變了／沒有判斷／活體違規」當失敗；關鍵字是提示
  process.exitCode = 0;
}
