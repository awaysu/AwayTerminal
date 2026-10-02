// 可編輯下拉（舊版 WPF 的 `ComboBox IsEditable="True"`）。
//
// HTML 沒有這個元件：`<input list>` ＋ `<datalist>` 看起來像，但 Chromium 會拿輸入框裡
// **現有的字**去篩清單——欄位填著 115200 時下拉就只剩 115200 一個選項（連接埠的
// Baud rate、字型下拉都踩過）。所以自己疊：
//
//   <span class="combo">
//     <select tabindex="-1" aria-hidden="true"></select>   ← 墊底，只露出右邊的箭頭，負責列出全部選項
//     <input type="text" />                                 ← 疊在上面，負責顯示與自己打字
//   </span>
//
// 樣式在 style.css 的 `.combo`。

/**
 * 重建 `<select>` 的選項。`items` 可以是字串／數字，或 `{ value, label }`。
 */
export function fillCombo(list, items) {
  list.textContent = '';
  for (const it of items) {
    const opt = document.createElement('option');
    const isObj = it !== null && typeof it === 'object';
    opt.value = String(isObj ? it.value : it);
    opt.textContent = String(isObj ? (it.label ?? it.value) : it);
    list.appendChild(opt);
  }
}

/** 讓底下的 `<select>` 跟著輸入框：字剛好是清單裡的就選起來，自己打的值＝不選任何一項。 */
export function syncCombo(input, list) {
  list.value = input.value.trim();
}

/** 把一組 `<input>` ＋ `<select>` 接成可編輯下拉。`onChange`＝值變了（選的或打的）。 */
export function wireCombo(input, list, onChange) {
  const changed = () => {
    if (onChange) onChange(input.value.trim());
  };
  list.addEventListener('change', () => {
    input.value = list.value;
    input.focus();
    changed();
  });
  input.addEventListener('input', () => {
    syncCombo(input, list);
    changed();
  });
  // 上下鍵在清單裡移動（同舊版的 ComboBox）
  input.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const n = list.options.length;
    if (n === 0) return;
    e.preventDefault();
    const step = e.key === 'ArrowDown' ? 1 : -1;
    const at = list.selectedIndex;
    list.selectedIndex = at < 0 ? (step > 0 ? 0 : n - 1) : Math.min(n - 1, Math.max(0, at + step));
    input.value = list.value;
    changed();
  });
}
