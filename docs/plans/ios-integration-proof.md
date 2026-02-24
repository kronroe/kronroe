# iOS Integration Proof (Kindly Roe Simulator Query)

Date (UTC): 2026-02-24T11:27:36Z

## Objective

Provide downstream iOS consuming-app proof (simulator) for Kronroe Swift Package integration, including one real entity query result with reproducible steps.

## Package Reference Used

- Repository: `https://github.com/kronroe/kronroe.git`
- Revision under test: `2c0e73ea103a69d99b7178800b2ac1973032085a`
- Swift Package path in repo: `crates/ios/swift`

Example dependency pin:

```swift
.package(url: "https://github.com/kronroe/kronroe.git", revision: "2c0e73ea103a69d99b7178800b2ac1973032085a")
```

## Simulator Target Used

- Destination ID: `DC57E537-E019-45BD-8AA7-7A0B0AA843AA`
- Device: `iPhone 16 Pro`
- Runtime: `iOS 18.6`

## Reproducible Steps

1. From repo root, run:

```bash
./crates/ios/scripts/build-xcframework.sh
```

2. Run simulator tests (auto-selects available iOS simulator):

```bash
./crates/ios/scripts/run-swift-tests.sh
```

3. Confirm query proof line appears in output:

```bash
PROOF_QUERY_RESULT_JSON=...
```

## Real Query Result (Captured From Simulator Run)

Command executed:

```bash
./crates/ios/scripts/run-swift-tests.sh | tee /tmp/kronroe_ios_proof.log
```

Captured proof line:

```text
PROOF_QUERY_RESULT_JSON=[{"id":"01KJ7PE6YHGXDDC573XMJ6848N","subject":"Freya","predicate":"attends","object":{"type":"Text","value":"Sunrise Primary"},"valid_from":"2026-02-24T11:27:05.169068Z","valid_to":null,"recorded_at":"2026-02-24T11:27:05.169089Z","expired_at":null,"confidence":1.0,"source":null}]
```

## Notes

- This proof executes the same Swift Package integration surface used by a downstream iOS app:
  - `KronroeGraph.open(...)`
  - `KronroeGraph.assert(...)`
  - `KronroeGraph.factsAboutJSON(...)`
- The query payload above is real output from the iOS simulator run (not placeholder text).
