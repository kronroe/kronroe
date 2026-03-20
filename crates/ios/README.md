# kronroe-ios

iOS FFI bindings and Swift Package wrapper for Kronroe.

The full XCFramework and Swift wrapper verification flow is treated as a local
pre-merge check on macOS rather than a required PR job on GitHub Actions. The
GitHub iOS workflow remains available on `main` pushes and via manual dispatch.

## What is included

- Rust staticlib crate (`kronroe-ios`) exposing a C ABI
- C header: `include/kronroe.h` (generated via `cbindgen`)
- XCFramework build script: `scripts/build-xcframework.sh`
- Swift Package wrapper in `swift/`

## Generate header

```bash
cd crates/ios
./scripts/generate-header.sh
```

## Build XCFramework

```bash
cd crates/ios
./scripts/build-xcframework.sh
```

To enforce compressed size budget (< 6 MB):

```bash
CHECK_SIZE_BUDGET=1 ./scripts/build-xcframework.sh
```

## Behavior tests (FFI)

```bash
cargo test -p kronroe-ios
```

These tests cover:
- open/assert/query roundtrip
- open_in_memory roundtrip
- failure-path error propagation for null handle

## Swift wrapper tests (iOS Simulator)

```bash
./crates/ios/scripts/run-swift-tests.sh
```

## Swift usage

```swift
let graph = try KronroeGraph.open(url: documentsURL.appendingPathComponent("memory.kronroe"))
try graph.assert(subject: "Freya", predicate: "attends", object: "Sunrise Primary")
let json = try graph.factsAboutJSON(entity: "Freya")
```
