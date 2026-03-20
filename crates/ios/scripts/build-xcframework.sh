#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
IOS_DIR="${ROOT_DIR}/crates/ios"
BUILD_DIR="${IOS_DIR}/build"
OUT_DIR="${IOS_DIR}/swift"
OUT_XCFRAMEWORK="${OUT_DIR}/KronroeFFI.xcframework"
HEADER_DIR="${IOS_DIR}/include"

# Size profile (opt-level=z, lto, codegen-units=1) is set in the workspace
# Cargo.toml [profile.release].  Only iOS/Android-specific flags go here.
export RUSTFLAGS="-C panic=abort -C link-arg=-Wl,-dead_strip ${RUSTFLAGS:-}"

echo "Building iOS static libraries..."
cargo build --release --target aarch64-apple-ios -p kronroe-ios
cargo build --release --target aarch64-apple-ios-sim -p kronroe-ios

DEVICE_LIB="${ROOT_DIR}/target/aarch64-apple-ios/release/libkronroe_ios.a"
SIM_LIB="${ROOT_DIR}/target/aarch64-apple-ios-sim/release/libkronroe_ios.a"

# Print raw sizes before stripping.
echo "Pre-strip sizes:"
ls -lh "${DEVICE_LIB}" "${SIM_LIB}"

# Strip debug info and local symbols from staticlibs.
# Cargo's profile.release.strip is a no-op for staticlib crate-types because
# there is no final link step — the .a is just an archive of object files.
# -S  = remove debug sections (STABS, DWARF)
# -x  = remove local (non-global) symbols — linker only needs globals
strip -S -x "${DEVICE_LIB}"
strip -S -x "${SIM_LIB}"

echo "Post-strip sizes:"
ls -lh "${DEVICE_LIB}" "${SIM_LIB}"

mkdir -p "${BUILD_DIR}" "${OUT_DIR}"
rm -rf "${OUT_XCFRAMEWORK}"

echo "Creating XCFramework..."
xcodebuild -create-xcframework \
  -library "${DEVICE_LIB}" -headers "${HEADER_DIR}" \
  -library "${SIM_LIB}" -headers "${HEADER_DIR}" \
  -output "${OUT_XCFRAMEWORK}"

ZIP_PATH="${BUILD_DIR}/KronroeFFI.xcframework.zip"
rm -f "${ZIP_PATH}"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "${OUT_XCFRAMEWORK}" "${ZIP_PATH}"

SIZE_BYTES="$(stat -f%z "${ZIP_PATH}")"
SIZE_MB="$(awk "BEGIN {printf \"%.2f\", ${SIZE_BYTES}/1024/1024}")"
echo "Compressed XCFramework size: ${SIZE_MB} MB (${SIZE_BYTES} bytes)"

if [[ "${CHECK_SIZE_BUDGET:-0}" == "1" ]]; then
  MAX_BYTES=$((6 * 1024 * 1024))
  if (( SIZE_BYTES > MAX_BYTES )); then
    echo "ERROR: compressed XCFramework exceeds 6 MB budget."
    exit 2
  fi
fi

echo "XCFramework ready at ${OUT_XCFRAMEWORK}"
