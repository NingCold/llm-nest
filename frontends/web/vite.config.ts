import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));
const targetPlatform = process.env.TAURI_ENV_PLATFORM ?? process.platform;

export default defineConfig({
  // Tauri supplies the target platform; standalone dev uses the local OS.
  // Only this boolean enters the bundle, never the process environment.
  define: {
    __LLMN_CUSTOM_WINDOW_CHROME__: ["windows", "win32"].includes(targetPlatform),
  },
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  resolve: {
    alias: {
      "@": `${__dirname}src`,
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    // 开发时把 /api 转发到真实后端（web-server，默认 127.0.0.1:8787），
    // 浏览器同源直连真实 ChatFeature
    proxy: {
      "/api": {
        target: process.env.LLMN_API ?? "http://127.0.0.1:8787",
        changeOrigin: false,
      },
    },
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
