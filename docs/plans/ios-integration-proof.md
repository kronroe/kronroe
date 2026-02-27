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

---

# Phase 2 — Kindly Roe App Integration Plan

**Added:** 2026-02-27
**Status:** Ready to execute in the Kindly Roe repo
**Executor:** Claude Code session opened inside `/Users/rebekahcole/kindlyroe/app/`

---

## Goal

Wire Kronroe into the KindlyRoe Xcode app so that user-curated moments —
highlights, pins, and annotations — persist on-device across sessions.
Currently these are transient (lost when the app closes).

This is milestone 0.7 of the Kronroe Phase 0 roadmap.

---

## What Kronroe already provides (do not redo)

The Kronroe repo is at `/Users/rebekahcole/kronroe/`.

### XCFramework
`crates/ios/swift/KronroeFFI.xcframework/`
Built for `ios-arm64` (device) and `ios-arm64-simulator`.
**Gitignored — rebuild if missing:** `bash crates/ios/scripts/build-xcframework.sh`

### Swift Package
`crates/ios/swift/` — contains:

| File | Purpose |
|------|---------|
| `Package.swift` | Swift Package definition, iOS 15+, binary target |
| `Sources/Kronroe/Kronroe.swift` | `KronroeGraph` — low-level graph wrapper |
| `Sources/Kronroe/KronroeMemoryStore.swift` | `KronroeMemoryStore` — conversation memory API |
| `Tests/KronroeTests/KronroeTests.swift` | Tests including `testMemoryStoreHighlightPinAnnotationRoundTrip` |

### KronroeMemoryStore API

```swift
// Open a file-backed graph (persists across launches)
let dbURL = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
    .appendingPathComponent("kindlyroe-memory.kronroe")
let graph = try KronroeGraph.open(url: dbURL)
let store = KronroeMemoryStore(graph: graph)

// Write
try store.recordHighlight(messageId: uuid, category: "rights")   // HighlightCategory.rawValue
try store.recordPin(messageId: uuid, label: "Equality Act adjustments")
try store.recordAnnotation(messageId: uuid, text: "Ask at next GP appointment")

// Read — returns JSON array of Kronroe Fact objects
let json = try store.factsAbout(messageId: uuid)
```

---

## The Kindly Roe iOS project

Project: `design/mobile/ios/KindlyRoe/KindlyRoe.xcodeproj`

Relevant files:

| File | What it does |
|------|-------------|
| `KindlyRoe/Core/Models/Models.swift` | `ConversationPin`, `MessageHighlight`, `HighlightCategory` |
| `KindlyRoe/Journeys/Adult/Chat/AdultChatViewModel.swift` | Adult chat VM — messages, pins, highlights live here |
| `KindlyRoe/Journeys/Family/Chat/FamilyChatViewModel.swift` | Family chat VM — same shape, check before modifying |
| `KindlyRoe/App/KindlyRoeApp.swift` | App entry point |

---

## Tasks

### Task 1 — Add the Kronroe Swift Package (manual Xcode step)

Claude cannot modify `.xcodeproj` files. Rebekah must do this in Xcode:

1. Open `KindlyRoe.xcodeproj`
2. File → Add Package Dependencies…
3. Click **Add Local…** and navigate to `/Users/rebekahcole/kronroe/crates/ios/swift/`
4. Select the **Kronroe** library product
5. Add to the **KindlyRoe** target (not test targets)

Verify by adding `import Kronroe` to any Swift file — it should compile.

### Task 2 — Create `KronroeStore.swift`

Create: `KindlyRoe/Core/Memory/KronroeStore.swift`

This is the app-level singleton that owns the `KronroeGraph` for the app lifetime.

```swift
import Foundation
import Kronroe

/// App-level singleton owning the on-device Kronroe memory graph.
@MainActor
final class KronroeStore {
    static let shared = KronroeStore()

    private let memoryStore: KronroeMemoryStore

    private init() {
        let dbURL = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("kindlyroe-memory.kronroe")
        do {
            let graph = try KronroeGraph.open(url: dbURL)
            self.memoryStore = KronroeMemoryStore(graph: graph)
            NSLog("🗄️ [KronroeStore] Opened DB at: %@", dbURL.path)
        } catch {
            NSLog("⚠️ [KronroeStore] File DB failed (%@), using in-memory fallback", error.localizedDescription)
            let graph = try! KronroeGraph.openInMemory()
            self.memoryStore = KronroeMemoryStore(graph: graph)
        }
    }

    func recordHighlight(messageId: UUID, category: String) {
        do { try memoryStore.recordHighlight(messageId: messageId, category: category) }
        catch { NSLog("⚠️ [KronroeStore] recordHighlight: %@", error.localizedDescription) }
    }

    func recordPin(messageId: UUID, label: String) {
        do { try memoryStore.recordPin(messageId: messageId, label: label) }
        catch { NSLog("⚠️ [KronroeStore] recordPin: %@", error.localizedDescription) }
    }

    func recordAnnotation(messageId: UUID, text: String) {
        do { try memoryStore.recordAnnotation(messageId: messageId, text: text) }
        catch { NSLog("⚠️ [KronroeStore] recordAnnotation: %@", error.localizedDescription) }
    }

    func factsAbout(messageId: UUID) -> String? {
        do { return try memoryStore.factsAbout(messageId: messageId) }
        catch { NSLog("⚠️ [KronroeStore] factsAbout: %@", error.localizedDescription); return nil }
    }
}
```

### Task 3 — Wire KronroeStore into AdultChatViewModel

Read `AdultChatViewModel.swift` to find exactly where highlights, pins, and
annotations are confirmed (not just previewed). Then add the three call sites:

**Highlight confirmed:**
```swift
KronroeStore.shared.recordHighlight(
    messageId: message.id,
    category: category.rawValue
)
```

**Pin confirmed:**
```swift
KronroeStore.shared.recordPin(
    messageId: pin.messageId,
    label: pin.label
)
```

**Annotation confirmed:**
```swift
KronroeStore.shared.recordAnnotation(
    messageId: messageId,
    text: annotationText
)
```

Check `FamilyChatViewModel.swift` — if it has the same features, add the same
call sites there too.

### Task 4 — Verify with a diagnostic query

After wiring, add a temporary debug call somewhere accessible (e.g. a long-press
on a highlighted message, or a button in a debug view) that prints:

```swift
if let json = KronroeStore.shared.factsAbout(messageId: someMessageId) {
    NSLog("PROOF_MEMORY_STORE_JSON=%@", json)
}
```

This produces the evidence line we need.

### Task 5 — Run on simulator and capture evidence

1. Build and run on iPhone 15 or 16 simulator (iOS 17 or 18)
2. Perform: highlight a message → pin it → add an annotation
3. Find `PROOF_MEMORY_STORE_JSON=...` in the Xcode console
4. Force-quit and relaunch — confirm the NSLog `"Opened DB at:"` appears (file persisted)
5. Capture the JSON output

---

## Evidence (fill in after executing)

**Date executed:** 2026-02-27
**Executor:** Claude Code (Tasks 2–4) / Rebekah (Task 5 simulator run)
**Simulator:** _(fill in after Task 5)_
**iOS version:** _(fill in after Task 5)_
**Build succeeded:** _(fill in after Task 5)_
**Package linked successfully:** Yes (Task 1 pre-completed)

### Task 2 — KronroeStore.swift created

File created at:
`design/mobile/ios/KindlyRoe/KindlyRoe/Core/Memory/KronroeStore.swift`

**⚠️ Action required:** Add this file to the Xcode project (right-click `Core/Memory` group
→ "Add Files to KindlyRoe…" → select `KronroeStore.swift` → Add to KindlyRoe target).

### Task 3 — Call sites wired in AdultChatView.swift

Three call sites added to
`design/mobile/ios/KindlyRoe/KindlyRoe/Journeys/Adult/Chat/AdultChatView.swift`:

| Event | Location in file | Method called |
|-------|-----------------|---------------|
| Highlight confirmed | `messagesView` → `onHighlightChange` closure | `KronroeStore.shared.recordHighlight(messageId:category:)` |
| Pin confirmed | `.sheet(isPresented: $showPinSheet)` → `onSave` closure | `KronroeStore.shared.recordPin(messageId:label:)` |
| Annotation confirmed | `setupAutoDemoCallbacks` → `onShowNoteInput` closure | `KronroeStore.shared.recordAnnotation(messageId:text:)` |

`FamilyChatView/FamilyChatViewModel` — no highlight/pin/annotation surfaces exist yet; no changes needed.

### Task 4 — Diagnostic proof line

`PROOF_MEMORY_STORE_JSON=` NSLog added inside `onHighlightChange` immediately after
`recordHighlight`. Fires on every real user highlight action (not just during demo mode).

### Task 5 — Simulator run

**Status:** BLOCKED — see findings doc

**Blocker:** `Sources/Kronroe/Kronroe.swift:86` fails to compile under Swift 6
(Xcode 26 default). Error:

```
error: cannot convert value of type 'UnsafePointer<CChar>'
to expected argument type 'UnsafeMutablePointer<CChar>'
```

Full details and fix options:
`docs/ios-consumer-integration-findings.md` (Kronroe repo)

**Console on first launch:**
```
(fill in after Blocker 2 is fixed and simulator run completes)
```

**PROOF_MEMORY_STORE_JSON after highlight + pin + annotation:**
```json
(fill in after Blocker 2 is fixed and simulator run completes)
```

**Survived force-quit and relaunch:** (fill in after fix)

**Surprises or issues:**
- xcodeproj gem v1.27.0 uses wrong key (`path` vs `relativePath`) for
  `XCLocalSwiftPackageReference` — required manual correction. See findings doc.
- Swift 6 pointer coercion error blocks compilation. Quick fix: add
  `.swiftLanguageVersion(.v5)` to the Kronroe target in `Package.swift`.

---

## After completing

1. Fill in Evidence section above and commit to Kindly Roe repo
2. Copy the Evidence section into this file in the Kronroe repo and commit
3. Mark `planning/04-roadmap.md` milestone 0.7 → ✅ Done
4. Add entry to `.ideas/RUN_CHANGELOG.md`

---

## Future steps (not v1)

- Restore highlights/pins/annotations on app launch from persisted Kronroe facts
- App group container migration if the widget needs Kronroe facts
- `AgentMemory.recall()` wired to give Roe on-device context from past conversations
