import { defineConfig } from 'vite';

// Tauri 2 + Vite：固定埠、不要自動開瀏覽器，忽略 src-tauri 變動
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    open: false,
    watch: { ignored: ['**/src-tauri/**', '**/reference/**'] },
  },
  build: {
    target: 'esnext',
    outDir: 'dist',
    emptyOutDir: true,
  },
});
