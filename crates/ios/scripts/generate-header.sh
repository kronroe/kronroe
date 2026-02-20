#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
CRATE_DIR="${IOS_DIR}"
OUT_HEADER="${IOS_DIR}/include/kronroe.h"

if ! command -v cbindgen >/dev/null 2>&1; then
  echo "cbindgen not found. Install with: cargo install cbindgen"
  exit 1
fi

mkdir -p "$(dirname "${OUT_HEADER}")"
cbindgen "${CRATE_DIR}" --config "${IOS_DIR}/cbindgen.toml" --output "${OUT_HEADER}"
echo "Generated ${OUT_HEADER}"
