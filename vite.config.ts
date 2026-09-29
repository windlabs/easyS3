import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [vue()],

  // Vite 特性专属环境变量前缀
  envPrefix: ["VITE_", "TAURI_ENV_"],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: false,
    // 仓库位于 WSL 挂载的 Windows 盘（/mnt/*），inotify 不可靠，必须轮询（见 AGENTS.md）
    watch: {
      usePolling: true,
    },
  },
});
