// 發佈助手：檢查版本、整理資產、產生 `latest.json` 與 release notes。
//
// **預設 dry-run**——只印出「會做什麼」，什麼都不動。加 `--publish` 才真的呼叫
// `gh release create`。
//
//   node scripts/release.mjs                 檢查 ＋ 印出計畫（安全，隨時可跑）
//   node scripts/release.mjs --write         另外把 latest.json / notes 寫進 target
//   node scripts/release.mjs --publish       真的建立 GitHub Release（需要 gh 已登入）
//
// 為什麼要有這支：發佈的步驟多（三處版本、兩種安裝檔 ×3 個檔、簽章、latest.json 的
// signature 欄位、CHANGELOG 的段落、資產命名、tag 指向的 commit），漏一步的後果都是安靜的
// ——例如 `latest.json` 的 `version` 帶了 `v` 前綴，updater 就永遠不會認為有新版。
//
// 簽章：`latest.json` 的 `signature` 要放 `.sig` 檔的內容，而 `.sig` 只有在
// `bundle.createUpdaterArtifacts: true` ＋ 公鑰 ＋ build 時給了 `TAURI_SIGNING_PRIVATE_KEY`
// 才會產生。**沒有 .sig 時這一步會被跳過並明說**
// （見 `docs/RELEASE.md` 第 4 節）。

import { existsSync, readFileSync, writeFileSync, statSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';

const args = process.argv.slice(2);
const doWrite = args.includes('--write') || args.includes('--publish');
const doPublish = args.includes('--publish');

const REPO = 'awaysu/AwayTerminal2';
const OUT_DIR = join('src-tauri', 'target', 'release');
const BUNDLE = join(OUT_DIR, 'bundle');

let fail = 0;
const problem = (msg) => {
  console.log(`❌ ${msg}`);
  fail++;
};
const ok = (msg) => console.log(`✓ ${msg}`);
const note = (msg) => console.log(`  ${msg}`);

// ------------------------------------------------------------- 1. 版本
function readVersions() {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8')).version;
  const conf = JSON.parse(readFileSync(join('src-tauri', 'tauri.conf.json'), 'utf8')).version;
  const cargoToml = readFileSync(join('src-tauri', 'Cargo.toml'), 'utf8');
  const m = /^version\s*=\s*"([^"]+)"/m.exec(cargoToml);
  return { pkg, conf, cargo: m ? m[1] : null };
}

const v = readVersions();
console.log(`版本：package.json=${v.pkg}　tauri.conf.json=${v.conf}　Cargo.toml=${v.cargo}`);
if (v.pkg !== v.conf || v.pkg !== v.cargo) {
  problem('三處版本號不一致——先改成一樣（cargo test --lib version_tests 也會抓到）');
}
const version = v.pkg;
// updater 的 `version` **不可以**帶 `v` 前綴（tag 才有）。帶了的話比對永遠不成立。
if (version.startsWith('v')) problem(`版本號不可以帶 v 前綴：${version}`);
const tag = `v${version}`;

// ------------------------------------------------------------- 2. CHANGELOG
function changelogSection(ver) {
  if (!existsSync('CHANGELOG.md')) return null;
  const md = readFileSync('CHANGELOG.md', 'utf8').replace(/\r\n/g, '\n');
  const lines = md.split('\n');
  // 找 `## <版本>` 或 `## <版本>（…）`
  const start = lines.findIndex((l) => new RegExp(`^## ${ver.replace('.', '\\.')}(\\D|$)`).test(l));
  if (start < 0) return null;
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (lines[i].startsWith('## ')) {
      end = i;
      break;
    }
  }
  return lines.slice(start + 1, end).join('\n').trim();
}

const notes = changelogSection(version);
if (notes) ok(`CHANGELOG.md 有 ${version} 的段落（${notes.split('\n').length} 行）`);
else problem(`CHANGELOG.md 裡找不到 \`## ${version}\` 的段落`);

// ------------------------------------------------------------- 3. 資產
/** 這一版該有哪些檔案。`required` 的缺了就是問題。 */
const assets = [
  { path: join(BUNDLE, 'nsis', `AwayTerminal_${version}_x64-setup.exe`), required: true, kind: 'nsis' },
  { path: join(BUNDLE, 'msi', `AwayTerminal_${version}_x64_en-US.msi`), required: true, kind: 'msi' },
  { path: join(BUNDLE, 'msi', `AwayTerminal_${version}_x64_zh-TW.msi`), required: true, kind: 'msi' },
  // 只有給了私鑰才會產生（見檔頭）
  { path: join(BUNDLE, 'nsis', `AwayTerminal_${version}_x64-setup.exe.sig`), required: false, kind: 'sig' },
];

const found = [];
for (const a of assets) {
  if (existsSync(a.path)) {
    const kb = (statSync(a.path).size / 1024).toFixed(0);
    ok(`${a.path}（${kb} KB）`);
    found.push(a);
  } else if (a.required) {
    problem(`少了 ${a.path}——先跑 \`npm run tauri build\``);
  } else {
    note(`（沒有 ${a.path}）`);
  }
}

// ------------------------------------------------------------- 4. latest.json
// `.sig` 要**三個條件同時成立**才會產生（稽核 I1：以前的訊息只講了後兩個）：
//   1. tauri.conf.json 的 `bundle.createUpdaterArtifacts` 是 true（預設 false＝完全不產生）
//   2. `plugins.updater.pubkey` 有公鑰
//   3. build 時給了 TAURI_SIGNING_PRIVATE_KEY
// 1 開了但沒有 3，`tauri build` 會在最後失敗（"A public key has been found, but no private key"）
// → 所以 1 和 2 要一起開，repo 平常兩個都關著。
const conf = JSON.parse(readFileSync(join('src-tauri', 'tauri.conf.json'), 'utf8'));
const updaterArtifacts = conf.bundle?.createUpdaterArtifacts === true;
const pubkey = String(conf.plugins?.updater?.pubkey || '');
if (updaterArtifacts && !pubkey) {
  problem('tauri.conf.json 的 bundle.createUpdaterArtifacts 開了、updater.pubkey 卻是空的——兩個要一起開（docs/RELEASE.md 第 4 節）');
} else if (!updaterArtifacts && pubkey) {
  problem('tauri.conf.json 有 updater.pubkey，但 bundle.createUpdaterArtifacts 不是 true——build 不會產生 .sig，自動更新發不出去');
}
const sig = found.find((a) => a.kind === 'sig');
const nsis = found.find((a) => a.kind === 'nsis');
let latest = null;
if (!sig) {
  console.log('');
  console.log('⏭  跳過 latest.json：**沒有 .sig 檔，也就是這次 build 沒有簽章**。');
  console.log('   自動更新需要簽章——沒有簽章的 latest.json 會被使用者端的 updater 拒絕，');
  console.log('   放上去只會讓「檢查更新」一直失敗。要開啟的話請照 docs/RELEASE.md 第 4 節：');
  console.log('   產生金鑰 → 公鑰貼進 tauri.conf.json 並把 bundle.createUpdaterArtifacts 設成 true');
  console.log('   → build 時給 TAURI_SIGNING_PRIVATE_KEY。');
  console.log(
    `   現在：createUpdaterArtifacts=${updaterArtifacts}　pubkey=${pubkey ? '有' : '空'}　` +
      `TAURI_SIGNING_PRIVATE_KEY=${process.env.TAURI_SIGNING_PRIVATE_KEY ? '有' : '沒有'}（這一個看的是跑 build 時的環境）`
  );
} else if (nsis) {
  latest = {
    version,
    notes: (notes || '').split('\n').slice(0, 20).join('\n'),
    pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
    platforms: {
      'windows-x86_64': {
        signature: readFileSync(sig.path, 'utf8').trim(),
        url: `https://github.com/${REPO}/releases/download/${tag}/${nsis.path.split(/[\\/]/).pop()}`,
      },
    },
  };
  ok('latest.json 組好了（windows-x86_64）');
  note('mac／Linux 做好之後要在 platforms 裡補 darwin-aarch64／darwin-x86_64／linux-x86_64');
}

// ------------------------------------------------------------- 5. tag 要打在哪個 commit
// `gh release create <tag>` 在 tag 還不存在時，會把 tag 打在**遠端預設分支的 HEAD**——
// 不一定是這次 build 的那個 commit（稽核 I7）。所以：工作樹要乾淨（build 的就是 HEAD）、
// HEAD 要已經 push 上去，再用 `--target <sha>` 明確指定。
const git = (...a) => execFileSync('git', a, { encoding: 'utf8' }).trim();
let headSha = '';
try {
  headSha = git('rev-parse', 'HEAD');
  const dirty = git('status', '--porcelain', '--untracked-files=no');
  const pushed = git('branch', '-r', '--contains', headSha);
  const check = doPublish ? problem : (m) => note(`（--publish 時會擋）${m}`);
  if (dirty) check('工作樹有還沒 commit 的修改——安裝檔不是從任何一個 commit 建出來的，先 commit 再重新 build');
  if (!pushed) check(`HEAD ${headSha.slice(0, 8)} 還沒 push 到遠端——tag 會指向一個 GitHub 上不存在的 commit`);
  if (!dirty && pushed) ok(`tag 會打在 ${headSha.slice(0, 8)}（工作樹乾淨、已 push）`);
} catch (e) {
  problem(`查不到 git 狀態：${e.message}`);
}

// ------------------------------------------------------------- 6. 輸出
const notesPath = join(OUT_DIR, `release-notes-${version}.md`);
const latestPath = join(OUT_DIR, 'latest.json');

console.log('');
console.log('─── 會做的事 ───');
console.log(`tag：${tag}（→ ${headSha ? headSha.slice(0, 8) : '?'}）`);
console.log(`資產：${found.filter((a) => a.kind !== 'sig').map((a) => a.path.split(/[\\/]/).pop()).join('、') || '（沒有）'}`);
console.log(`release notes：${notesPath}${notes ? '' : '（沒有內容）'}`);
console.log(`latest.json：${latest ? latestPath : '不產生（沒有簽章）'}`);
console.log(
  `gh release create：${doPublish ? '**會執行**' : '不執行（加 --publish 才會）'}`
);

if (doWrite && notes) {
  writeFileSync(notesPath, `# AwayTerminal ${version}\n\n${notes}\n`, 'utf8');
  ok(`寫出 ${notesPath}`);
}
if (doWrite && latest) {
  writeFileSync(latestPath, JSON.stringify(latest, null, 2) + '\n', 'utf8');
  ok(`寫出 ${latestPath}`);
}

if (fail) {
  console.log('');
  console.log(`RESULT: FAIL（${fail} 個問題）`);
  process.exit(1);
}

if (doPublish) {
  const files = found.filter((a) => a.kind !== 'sig').map((a) => a.path);
  if (latest) files.push(latestPath);
  const gh = ['release', 'create', tag, ...files, '--target', headSha, '--title', `AwayTerminal ${version}`];
  if (notes) gh.push('--notes-file', notesPath);
  console.log('');
  console.log(`執行：gh ${gh.join(' ')}`);
  try {
    execFileSync('gh', gh, { stdio: 'inherit' });
    ok('GitHub Release 建好了');
  } catch (e) {
    problem(`gh release create 失敗：${e.message}`);
    process.exit(1);
  }
} else {
  console.log('');
  console.log('（dry-run：什麼都沒有動。加 --write 產檔案、--publish 真的發佈。）');
}

console.log('RESULT: PASS');
