#!/usr/bin/env bash
set -euo pipefail

# Build .so libraries for all Android architectures.
# Requires: cargo-ndk (cargo install cargo-ndk)
#           Android NDK (ANDROID_NDK_HOME must be set)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
ANDROID_DIR="${ROOT_DIR}/crates/android"
BUILD_DIR="${ANDROID_DIR}/build/jniLibs"

TARGETS=(
  "aarch64-linux-android"    # arm64-v8a
  "armv7-linux-androideabi"  # armeabi-v7a
  "x86_64-linux-android"     # x86_64
  "i686-linux-android"       # x86
)

ABI_NAMES=(
  "arm64-v8a"
  "armeabi-v7a"
  "x86_64"
  "x86"
)

# Size-optimized defaults for mobile shared libs.
export RUSTFLAGS="-C strip=symbols -C panic=abort ${RUSTFLAGS:-}"
export CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-true}"
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-1}"

# Min Android API level (matches most apps' minSdk).
MIN_API="${ANDROID_MIN_API:-24}"

echo "Building Android shared libraries (API ${MIN_API})..."

for i in "${!TARGETS[@]}"; do
  target="${TARGETS[$i]}"
  abi="${ABI_NAMES[$i]}"

  echo "  Building ${target} (${abi})..."
  cargo ndk --target "${target}" --platform "${MIN_API}" \
    build --release -p kronroe-android

  src="${ROOT_DIR}/target/${target}/release/libkronroe_android.so"
  dest="${BUILD_DIR}/${abi}"
  mkdir -p "${dest}"
  cp "${src}" "${dest}/libkronroe_android.so"

  SIZE_BYTES="$(stat -f%z "${dest}/libkronroe_android.so" 2>/dev/null || stat -c%s "${dest}/libkronroe_android.so")"
  SIZE_MB="$(awk "BEGIN {printf \"%.2f\", ${SIZE_BYTES}/1024/1024}")"
  echo "    ${abi}: ${SIZE_MB} MB"

  if [[ "${CHECK_SIZE_BUDGET:-0}" == "1" ]]; then
    MAX_BYTES=$((6 * 1024 * 1024))
    if (( SIZE_BYTES > MAX_BYTES )); then
      echo "ERROR: ${abi} .so exceeds 6 MB budget."
      exit 2
    fi
  fi
done

echo ""
echo "jniLibs ready at ${BUILD_DIR}/"
ls -lR "${BUILD_DIR}/"
