import fs from "node:fs";
import path from "node:path";
import { defineConfig } from "vite";

function collectHtmlEntries(dir: string, entries: Record<string, string> = {}) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === ".vitepress" || entry.name === "dist" || entry.name === "node_modules") continue;
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      collectHtmlEntries(fullPath, entries);
    } else if (entry.isFile() && entry.name === "index.html") {
      const relative = path.relative(process.cwd(), fullPath).replace(/\\/g, "/");
      entries[relative] = fullPath;
    }
  }
  return entries;
}

export default defineConfig({
  build: {
    outDir: "dist",
    target: "es2022",
    rollupOptions: {
      input: collectHtmlEntries(process.cwd()),
    },
  },
  optimizeDeps: {
    exclude: ["kronroe-wasm"],
  },
  server: {
    fs: {
      allow: ["."],
    },
  },
});
