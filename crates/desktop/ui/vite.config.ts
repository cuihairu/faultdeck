import { defineConfig } from "vite";

// Tauri expects the dev server on localhost:5173 (see tauri.conf.json devUrl).
export default defineConfig({
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "es2021",
    outDir: "dist",
    emptyOutDir: true,
  },
});
