// 圖示（`public/icon/*.png`，直接沿用舊版 `reference/AwayTerminal/icon/` 那一組）。
//
// 舊版的三處圖示是**同一組圖**：工具列按鈕（圖上字下）、New／我的最愛下拉（圖左字右）、
// 分頁列的連線種類圖示（染綠／紅）。這裡只管「哪個 key 對到哪個檔名」與「怎麼做出 <img>」，
// 尺寸與版面在 `style.css`。
//
// ⚠️ 這組 PNG 的**透明度是整塊圓角方形，不是圖形本身**（`powershell.png` 之類的底圖是滿版
// 圓角矩形，圖形畫在顏色裡）。所以分頁列的染色**不能用 `-webkit-mask-image`**——那會得到
// 一塊純色方塊。改用 `index.html` 裡的 SVG filter（`#tint-ready` / `#tint-busy`），
// 逐像素做 `tint × 亮度^0.7`、alpha 照抄，和舊版 `Services/IconTint.cs` 同一條公式。

/** 圖示檔放在 `public/icon/`，打包後就在 `/icon/` 底下（Vite 原樣複製）。 */
export const ICON_DIR = 'icon/';

/** 找不到對應圖示時用的通用圖（同舊版 `CustomIconFile` 的 `run.png`）。 */
export const FALLBACK_ICON = 'run';

/**
 * 自訂連線可以選的圖示 key。
 *
 * 逐字照抄舊版 `Dialogs/CustomConnDialog.cs` 的 `IconKeys`（順序也一樣），
 * 這樣舊設定檔匯進來時每個 key 都對得到圖。
 */
export const CUSTOM_ICON_KEYS = [
  'powershell',
  'ssh-telnet',
  'adb',
  'wsl',
  'git',
  'docker',
  'claude-code',
  'codex',
  'opencode',
  'geminicli',
  'qwen',
  // Antigravity CLI（2.0.6 新增；舊版沒有這個 key）
  'antigravity',
  // Grok CLI（2.0.10 新增）
  'grok',
  'python',
  'run',
  'none',
];

/**
 * 分頁／我的最愛的連線種類 → 圖示檔名（不含 `.png`）。
 *
 * 照舊版 `MainWindow.xaml.cs` 的 `HistoryIcon`：`ps`→powershell、`claude`→claude-code、
 * `ssh`／`telnet`→ssh-telnet、`com`→com、`adb`→adb、`multiagent`→multi-agent、
 * `chatroom`→chatroom、其餘→new-connecting。新版的 kind 字串（`shell`／`agent`…）一起列進來。
 */
const KIND_ICON = {
  shell: 'powershell',
  ps: 'powershell',
  powershell: 'powershell',
  claude: 'claude-code',
  ssh: 'ssh-telnet',
  telnet: 'ssh-telnet',
  com: 'com',
  adb: 'adb',
  wsl: 'wsl',
  agent: 'multi-agent',
  multiagent: 'multi-agent',
  chat: 'chatroom',
  chatroom: 'chatroom',
};

/** 圖示 key（或連線種類）→ `/icon/xxx.png`。空的／不認識的回 `run.png`（同舊版 `CustomIconFile`）。 */
export function iconUrl(key) {
  const name = String(key || '').trim() || FALLBACK_ICON;
  return `${ICON_DIR}${name}.png`;
}

/** 連線種類 → 圖示 key。自訂連線請直接傳它自己的 `icon`。 */
export function kindIcon(kind) {
  return KIND_ICON[kind] || 'new-connecting';
}

/**
 * 做一個圖示 `<img>`。
 *
 * `className` 決定尺寸與染色（`tool-ico` 工具列 26px、`menu-ico` 下拉 26px、
 * `tab-ico` 分頁列 20px＋染色）。載入失敗就換成 `run.png`（同舊版 `IconTint.Get` 的退路），
 * 連它都失敗就整個藏起來，不要讓破圖示佔位。
 */
export function iconImg(key, className) {
  const img = document.createElement('img');
  img.className = className;
  img.alt = '';
  img.draggable = false;
  img.src = iconUrl(key);
  img.addEventListener('error', () => {
    if (img.dataset.fallback) {
      img.style.visibility = 'hidden';
      return;
    }
    img.dataset.fallback = '1';
    img.src = iconUrl(FALLBACK_ICON);
  });
  return img;
}

/**
 * 換工具列按鈕的文字，**不動它的圖示**。
 *
 * 工具列按鈕是 `<img class="tool-ico"> + <span class="tool-label">`，
 * 直接 `btn.textContent = …` 會把圖示一起洗掉（TASK-027 的 `--verify` 抓到：
 * 我的最愛／其他設定／關於三顆在切語言前就已經是沒有圖的——它們的文字在自己的
 * 模組裡設）。**加新的工具列按鈕、或在別的模組裡設它的文字時，一律用這個。**
 */
export function setToolLabel(btn, text) {
  if (!btn) return;
  const label = btn.querySelector(':scope > .tool-label');
  if (label) label.textContent = text;
  else btn.textContent = text;
}
