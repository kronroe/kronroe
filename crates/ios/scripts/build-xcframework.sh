#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
IOS_DIR="${ROOT_DIR}/crates/ios"
BUILD_DIR="${IOS_DIR}/build"
OUT_DIR="${IOS_DIR}/swift"
OUT_XCFRAMEWORK="${OUT_DIR}/KronroeFFI.xcframework"
HEADER_DIR="${IOS_DIR}/include"

# Size-optimized defaults for mobile staticlibs.
export RUSTFLAGS="-C strip=symbols -C panic=abort -C link-arg=-Wl,-dead_strip ${RUSTFLAGS:-}"
export CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-true}"
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-1}"

echo "Building iOS static libraries..."
cargo build --release --target aarch64-apple-ios -p kronroe-ios
cargo build --release --target aarch64-apple-ios-sim -p kronroe-ios

DEVICE_LIB="${ROOT_DIR}/target/aarch64-apple-ios/release/libkronroe_ios.a"
SIM_LIB="${ROOT_DIR}/target/aarch64-apple-ios-sim/release/libkronroe_ios.a"

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
