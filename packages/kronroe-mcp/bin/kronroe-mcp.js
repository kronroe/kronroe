#!/usr/bin/env node
const { spawn } = require("node:child_process");
const { existsSync, readFileSync } = require("node:fs");
const { join, resolve } = require("node:path");

/**
 * Resolve the native kronroe-mcp binary on PATH, skipping script wrappers
 * (including this shim) to avoid infinite self-recursion.
 */
function resolveBinary() {
  // Honour explicit override (same convention as the Python wrapper).
  const explicit = process.env.KRONROE_MCP_BIN;
  if (explicit) return explicit;

  const name = "kronroe-mcp";
  const isWin = process.platform === "win32";
  const dirs = (process.env.PATH || "").split(isWin ? ";" : ":");
  const thisScript = resolve(__filename);

  for (const dir of dirs) {
    const candidate = join(dir, name);
    if (!existsSync(candidate)) continue;

    // Skip if this candidate resolves to the current shim.
    if (resolve(candidate) === thisScript) continue;

    // Skip script wrappers (shebang = starts with "#!").
    try {
      const head = readFileSync(candidate, { encoding: "utf8", flag: "r" }).slice(0, 2);
      if (head === "#!") continue;
    } catch {
      // Unreadable file — skip.
      continue;
    }

    return candidate;
  }

  return null;
}

const binary = resolveBinary();

if (!binary) {
  console.error("Could not find native 'kronroe-mcp' binary on PATH.");
  console.error(
    "Install the Rust binary first (for example: `cargo install --path crates/mcp-server`),\n" +
    "or set KRONROE_MCP_BIN to the binary path."
  );
  process.exit(1);
}

const child = spawn(binary, process.argv.slice(2), {
  stdio: "inherit",
});

child.on("error", (err) => {
  console.error("Failed to start kronroe-mcp binary:", err.message);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
