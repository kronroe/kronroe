#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
HOST_PYTHON="${HOST_PYTHON:-$(command -v python3)}"

FEATURES="${KRONROE_PY_FEATURES:-}"
echo "Building local Python extension artifact"
if [[ -n "${FEATURES}" ]]; then
  cargo build -p kronroe-py --features "${FEATURES}"
else
  cargo build -p kronroe-py
fi

MODULE_DYLIB="${REPO_ROOT}/target/debug/deps/libkronroe.dylib"
if [[ ! -f "${MODULE_DYLIB}" ]]; then
  echo "Expected extension artifact not found: ${MODULE_DYLIB}" >&2
  exit 1
fi

MODULE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/kronroe-py-module-XXXXXX")"
cp "${MODULE_DYLIB}" "${MODULE_DIR}/kronroe.so"

echo "Running Python runtime smoke test with PYTHONPATH=${MODULE_DIR}"
PYTHONPATH="${MODULE_DIR}${PYTHONPATH:+:${PYTHONPATH}}" \
  "${HOST_PYTHON}" "${CRATE_DIR}/tests/runtime_smoke.py"
