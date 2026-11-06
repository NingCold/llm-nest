import { defineConfig, normalizePath } from "vite";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("../android/crypto", import.meta.url));
const api = normalizePath(fileURLToPath(new URL("../android/crypto/api.ts", import.meta.url)));
export default defineConfig({
  root,
  base: "./",
  plugins: [{ name: "native-crypto-api", enforce: "pre", resolveId(source, importer) {
    if (source === "../api" && importer && /\/web\/src\/cloud\/crypto\//.test(importer.replaceAll("\\", "/"))) return api;
  } }],
  build: {
    target: "es2022", outDir: "../app/build/generated/cryptoAssets/crypto", emptyOutDir: false, assetsInlineLimit: 0,
    rollupOptions: { output: { entryFileNames: "runtime.js", chunkFileNames: "[name].js", assetFileNames: "[name][extname]" } },
  },
});
