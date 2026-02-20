import { defineConfig } from "vite";

export default defineConfig({
  build: {
    outDir: "dist",
    target: "es2022",
  },
  optimizeDeps: {
    exclude: ["kronroe-wasm"],
  },
  server: {
    fs: {
      allow: [".."],
    },
  },
});
