#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SWIFT_DIR="${ROOT_DIR}/crates/ios/swift"

DEST_ID="$(
  xcrun simctl list devices available iOS \
    | awk -F '[()]' '/iPhone/ { print $2; exit }'
)"

if [[ -z "${DEST_ID}" ]]; then
  echo "ERROR: no available iOS simulator found."
  exit 1
fi

echo "Running Swift tests on simulator id=${DEST_ID}"
cd "${SWIFT_DIR}"
xcodebuild test -scheme Kronroe -destination "id=${DEST_ID}"
