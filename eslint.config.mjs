// eslint：**只開兩條規則**（TASK-028）。
//
// 為什麼加它：`setdlg.js` 用了 `setToolLabel` 卻沒有 import，Vite 打包**不會擋**——
// bundle 裡就是一個裸的全域呼叫，要等使用者按下去才炸（而且被 try/catch 吃掉）。
// `no-undef` 一秒就抓得到這一類。
//
// 刻意不開整套風格規則：這個 repo 的風格由 prettier 之外的習慣決定，全開只會製造幾百條噪音，
// 沒有人會看 → 等於沒有檢查。要加新規則請一條一條加，而且要能解釋它擋掉哪一種真的 bug。
import globals from 'globals';

export default [
  {
    // 前端（瀏覽器環境）。`terminal.js` 是從舊版原封不動搬過來的，刻意不碰、也不掃。
    files: ['src/**/*.js'],
    ignores: ['src/terminal.js'],
    languageOptions: {
      ecmaVersion: 2024,
      sourceType: 'module',
      globals: { ...globals.browser },
    },
    rules: {
      'no-undef': 'error',
      // 用不到的東西留著不會壞，但通常代表「改到一半」→ 出聲但不擋
      'no-unused-vars': ['warn', { args: 'none' }],
    },
  },
  {
    // 建置／驗證腳本（Node）
    files: ['scripts/**/*.mjs', '*.config.js', '*.config.mjs'],
    languageOptions: {
      ecmaVersion: 2024,
      sourceType: 'module',
      globals: { ...globals.node },
    },
    rules: {
      'no-undef': 'error',
      'no-unused-vars': ['warn', { args: 'none' }],
    },
  },
  {
    ignores: ['dist/**', 'node_modules/**', 'src-tauri/**', 'reference/**', '.ai/**'],
  },
];
