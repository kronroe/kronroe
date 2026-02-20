#!/usr/bin/env node
const { spawn } = require("node:child_process");

const child = spawn("kronroe-mcp", process.argv.slice(2), {
  stdio: "inherit",
});

child.on("error", (err) => {
  console.error("Failed to start kronroe-mcp binary:", err.message);
  console.error(
    "Install the Rust binary first (for example: `cargo install --path crates/mcp-server`)."
  );
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
