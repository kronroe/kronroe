# iOS Consumer Integration Findings

**Date:** 2026-02-27
**Context:** Kindly Roe app integrating Kronroe as a local Swift Package dependency
**Xcode version:** Xcode 26.2 (iOS 26.2 SDK)
**Swift version:** Swift 6 (default in Xcode 26)
**Executor:** Claude Code session in `/Users/rebekahcole/kindlyroe/app/`

---

## Summary

Two blockers found when wiring Kronroe into a consuming iOS app as a local
Swift Package. Neither is an error in `KronroeMemoryStore.swift` or the public
API — both are in the package plumbing layer and the FFI bridge layer.

---

## Blocker 1 — xcodeproj gem writes wrong key for local package reference

### What happened

Used the `xcodeproj` Ruby gem (v1.27.0) to add the Kronroe local package
dependency to the Kindly Roe `.xcodeproj`:

```ruby
local_pkg = project.new(Xcodeproj::Project::Object::XCLocalSwiftPackageReference)
local_pkg.path = "/Users/rebekahcole/kronroe/crates/ios/swift"
project.root_object.package_references << local_pkg
```

The gem wrote this into `project.pbxproj`:

```
B8854490ABBE4E74E4BFE756 /* XCLocalSwiftPackageReference "LocalSwiftPackageReference" */ = {
    isa = XCLocalSwiftPackageReference;
    path = /Users/rebekahcole/kronroe/crates/ios/swift;
};
```

Build failed immediately:

```
error: Missing package product 'Kronroe'
(in target 'KindlyRoe' from project 'KindlyRoe')
```

`xcodebuild -resolvePackageDependencies` did **not** list Kronroe at all.

### Root cause

Xcode expects `XCLocalSwiftPackageReference` to use the key `relativePath`
(a path relative to the `.xcodeproj` directory), **not** `path` (absolute).
The xcodeproj gem does expose `relative_path=` but `path=` silently sets the
wrong key.

The gem also assigns the display-name comment as the generic string
`"LocalSwiftPackageReference"` rather than the package directory name.

### Fix

```ruby
local_pkg = project.new(Xcodeproj::Project::Object::XCLocalSwiftPackageReference)
local_pkg.relative_path = Pathname.new(pkg_abs).relative_path_from(Pathname.new(proj_dir)).to_s
project.root_object.package_references << local_pkg
```

The relative path from the project dir
`/Users/rebekahcole/kindlyroe/app/design/mobile/ios/KindlyRoe/`
to the package dir
`/Users/rebekahcole/kronroe/crates/ios/swift`
is:

```
../../../../../../kronroe/crates/ios/swift
```

After this fix, `xcodebuild -resolvePackageDependencies` correctly listed:

```
Kronroe: /Users/rebekahcole/kronroe/crates/ios/swift @ local
```

### Recommendation for Kronroe docs

Add a note to any consumer integration guide:

> When adding Kronroe as a local package via the xcodeproj gem, use
> `local_pkg.relative_path =` not `local_pkg.path =`. Absolute paths are
> silently ignored by Xcode's package resolver.

---

## Blocker 2 — Swift 6 compiler error in `Kronroe.swift` line 86

### What happened

After the local package was correctly resolved, the build failed with:

```
/Users/rebekahcole/kronroe/crates/ios/swift/Sources/Kronroe/Kronroe.swift:86:37:
error: cannot convert value of type 'UnsafePointer<CChar>' (aka 'UnsafePointer<Int8>')
to expected argument type 'UnsafeMutablePointer<CChar>' (aka 'UnsafeMutablePointer<Int8>')
```

### Root cause

Swift 6 (default in Xcode 26) removed the implicit conversion from
`UnsafePointer<T>` to `UnsafeMutablePointer<T>`. This conversion was
allowed as an implicit coercion in Swift 5 but is a hard error in Swift 6
because mutably aliasing an immutable pointer is undefined behaviour.

Line 86 of `Kronroe.swift` passes a value that the compiler deduces as
`UnsafePointer<CChar>` (e.g. a `String`'s internal buffer, or a `[CChar]`
literal) into a C FFI call that expects `UnsafeMutablePointer<CChar>`.

### Fix options (in the Kronroe repo)

**Option A — preferred: add a `withUnsafeMutablePointer` / `strdup` wrapper**

If the C function does not actually mutate the string, the cleanest fix is to
copy the string into a mutable buffer for the call:

```swift
// Before (Swift 5 only)
let result = some_c_func(cString)

// After (Swift 5 + Swift 6)
var mutable = cString  // makes a mutable copy of the [CChar] array, or:
result = mutable.withUnsafeMutableBufferPointer { buf in
    some_c_func(buf.baseAddress!)
}
```

**Option B — `@_silgen_name` / bridging header tweak**

If the underlying Rust/C function signature can be changed to accept
`const char *` instead of `char *`, update the generated C header and
rebuild the XCFramework. This is the correct fix if the function is
genuinely read-only.

**Option C — interim: set Swift language version to 5 for the package**

Add a `swiftSettings` stanza to the `Kronroe` target in `Package.swift`:

```swift
.target(
    name: "Kronroe",
    dependencies: ["KronroeFFI"],
    path: "Sources/Kronroe",
    swiftSettings: [
        .swiftLanguageVersion(.v5)
    ]
)
```

This silences the error for now but defers the Swift 6 migration.

### Which line to look at

`Sources/Kronroe/Kronroe.swift` line 86. The call site is passing a string or
char-array into a function declared in the KronroeFFI XCFramework's C header
with a `char *` (mutable) parameter. Either the C header should declare it
`const char *`, or the Swift wrapper needs an explicit mutable copy.

### Impact

This error blocks compilation for **all** consumers building with Xcode 26 /
Swift 6. Any iOS app targeting iOS 18+ with a fresh Xcode install will hit
this. Apps still on Xcode 15/16 with `SWIFT_VERSION = 5` in their build
settings would not see this error.

---

## What worked correctly

- `KronroeMemoryStore` public API — `recordHighlight`, `recordPin`,
  `recordAnnotation`, `factsAbout` — exactly matched the plan spec. No
  changes needed to `KronroeMemoryStore.swift`.
- The XCFramework binary target structure (`KronroeFFI.xcframework` with
  `ios-arm64` and `ios-arm64-simulator` slices) resolved cleanly once the
  package reference path was fixed.
- `Package.swift` product/target naming (`Kronroe` library wrapping
  `KronroeFFI` binary target) is correct and unambiguous.
- `xcodebuild -resolvePackageDependencies` confirmed resolution at the local
  path after the `relativePath` fix.

---

## Files changed in Kindly Roe repo (tasks 2–3 complete, task 5 blocked)

| File | Change |
|------|--------|
| `KindlyRoe/Core/Memory/KronroeStore.swift` | Created — app singleton |
| `KindlyRoe/Journeys/Adult/Chat/AdultChatView.swift` | 3 call sites added (highlight, pin, annotation) |
| `KindlyRoe.xcodeproj/project.pbxproj` | Local package ref + KronroeStore source ref added via gem |

Task 5 (simulator proof run) is blocked on Blocker 2 above.

---

---

## Blocker 3 — `simctl launch` blocks; log stream captures nothing on launch

### What happened

After the Swift 6 fix landed on Kronroe `main` and the build succeeded,
the simulator task runner (Claude Code's Bash tool) hit a third blocker trying
to complete Task 5 (the proof run).

`xcrun simctl launch <uuid> com.kindlyroe.KindlyRoe` **blocks** — it does not
return until the app process exits. Every invocation hung indefinitely, making
it impossible to layer further commands (log stream, UI automation) after it.

Additionally, `KronroeStore.shared` is a lazy `@MainActor` singleton. It is
only initialised on the **first call site** — i.e. when the user highlights a
message. Simply launching the app does not trigger the `🗄️ [KronroeStore] Opened DB`
NSLog. The proof line only fires after real UI interaction.

### What was tried

| Attempt | Result |
|---------|--------|
| `xcrun simctl launch --console-pty` | NSLog output empty — NSLog routes to unified log, not stdout |
| `xcrun simctl spawn ... log stream --predicate 'process == "KindlyRoe"'` | Stream started, 0 lines — app not yet interacted with |
| `xcrun simctl io screenshot` | `Error creating the image (code=2)` — display not active |
| `idb_companion` (installed at `/opt/homebrew/bin/`) | `idb` CLI not installed; companion alone is not enough |

### Root cause

The proof line requires UI interaction (tap a highlight category on a Roe
message). That needs either:
- `idb` CLI (`brew install fb-idb`) for programmatic touch events, or
- Running the AutoDemo sequence inside the app (sets
  `AutoDemoOrchestrator.shared.isEnabled = true`, which auto-highlights
  messages using mock services), or
- A human tapping the app in Simulator.

### Status

**BUILD SUCCEEDED** — the integration code compiles and links cleanly after
the Swift 6 fix. The proof run itself was not completed by Claude Code.

---

## Next steps for Kronroe

1. ✅ Fix `Kronroe.swift:86` Swift 6 pointer error — confirmed fixed on `main`
2. Run the Kindly Roe simulator yourself:
   - Open Simulator, select iPhone 17 Pro
   - Build and run (`⌘R`) from Xcode
   - Select Adult journey → type any message → wait for Roe's reply
   - Long-press the reply → select a highlight category
   - Copy the `PROOF_MEMORY_STORE_JSON=` line from the Xcode console
3. Paste the proof line into the Evidence section of
   `docs/plans/ios-integration-proof.md` in both repos
4. Optionally: install `idb` CLI (`brew install fb-idb`) to enable headless
   proof runs in future (`idb ui tap <x> <y>` drives the simulator)
