#!/usr/bin/env bash
set -euo pipefail

echo "[contract-matrix] MCP"
cargo test -p kronroe-mcp

echo "[contract-matrix] Python (extension build)"
cargo test -p kronroe-py

echo "[contract-matrix] Python (runtime-tests compile gate)"
cargo check -p kronroe-py --no-default-features --features python-runtime-tests --tests

echo "[contract-matrix] WASM default"
cargo test -p kronroe-wasm

echo "[contract-matrix] WASM hybrid+uncertainty"
cargo check -p kronroe-wasm --features kronroe-wasm/hybrid,kronroe-wasm/uncertainty
cargo test -p kronroe-wasm --features kronroe-wasm/hybrid,kronroe-wasm/uncertainty

echo "[contract-matrix] complete"
