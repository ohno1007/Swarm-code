import { defineConfig } from "vite";

// Tauri serves the frontend; keep the build simple and deterministic.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2021",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
  },
});
