import { defineConfig } from 'vite';

// Tauri 2 + Vite：固定埠、不要自動開瀏覽器，忽略 src-tauri 變動
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    open: false,
    // `.ai/sandbox/**` 是沙盒模式開的 git worktree——裡面是整個專案的複本，
    // 不排除的話 Vite 會連那份一起 watch，改一個檔案就重載兩三次（實測會看到
    // `page reload .ai/sandbox/…/index.html`）。`.ai/bus` 是代理團隊的信箱，也不用 watch。
    watch: { ignored: ['**/src-tauri/**', '**/reference/**', '**/.ai/**'] },
  },
  build: {
    target: 'esnext',
    outDir: 'dist',
    emptyOutDir: true,
  },
});
