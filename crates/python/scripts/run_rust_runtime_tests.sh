#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

cd "${REPO_ROOT}"

FEATURES="${KRONROE_PY_RUST_TEST_FEATURES:-}"

# Embedded-interpreter tests need the Python shared library discoverable at run time.
PY_FRAMEWORK_PREFIX="$(python3 - <<'PY'
import sysconfig
print(sysconfig.get_config_var("PYTHONFRAMEWORKPREFIX") or "")
PY
)"
PY_LIBDIR="$(python3 - <<'PY'
import sysconfig
print(sysconfig.get_config_var("LIBDIR") or "")
PY
)"
if [[ -n "${PY_FRAMEWORK_PREFIX}" ]]; then
  export DYLD_FRAMEWORK_PATH="${PY_FRAMEWORK_PREFIX}${DYLD_FRAMEWORK_PATH:+:${DYLD_FRAMEWORK_PATH}}"
fi
if [[ -n "${PY_LIBDIR}" ]]; then
  export DYLD_FALLBACK_LIBRARY_PATH="${PY_LIBDIR}${DYLD_FALLBACK_LIBRARY_PATH:+:${DYLD_FALLBACK_LIBRARY_PATH}}"
fi

if [[ -n "${FEATURES}" ]]; then
    echo "Running Rust-side PyO3 runtime tests without extension-module (features: ${FEATURES})"
  cargo test -p kronroe-py --no-default-features --features "python-runtime-tests ${FEATURES}" --tests
else
  echo "Running Rust-side PyO3 runtime tests without extension-module"
  cargo test -p kronroe-py --no-default-features --features python-runtime-tests --tests
fi
