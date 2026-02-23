# Kronroe Technical Phase Review (2026-02-23)

## 1) Executive Snapshot

### Status
- Core platform is stable and shipping-ready for current Phase 0 scope.
- `cargo test --all --all-features` passes.
- WASM playground build passes (`site`).
- iOS packaging pipeline exists and runs in CI.

### Why this matters
- Confirms engineering baseline is healthy before Phase 1 feature expansion.
- Reduces risk of stacking new work on unstable foundations.

### Help notes
- Use this section in standups and investor/lead updates.
- Keep this as the "single source of truth" for immediate technical status.

---

## 2) What Is Built (Technical Inventory)

### Engine and storage
- Bi-temporal fact model in `crates/core` with:
  - `valid_from`, `valid_to`, `recorded_at`, `expired_at`
- Core operations implemented:
  - assert, query-current, query-at-time, query-about-entity, invalidate/correct
- ACID persistence via `redb`.

### Retrieval capabilities
- Full-text retrieval via `tantivy` behind `fulltext` feature.
- Vector retrieval support behind `vector` feature.
- Vector persistence + reopen behavior covered by tests.

### Platform surfaces
- `crates/agent-memory`: high-level wrapper over core.
- `crates/mcp-server`: stdio MCP server + smoke test.
- `crates/python`: PyO3 bindings exposing `KronroeDb` + `AgentMemory`.
- `crates/wasm`: browser bindings used by the playground.
- `crates/ios`: C FFI + Swift wrapper + XCFramework build pipeline.

### Why this matters
- The architecture is already multi-surface and commercially useful for structured memory and temporal querying.

### Help notes
- If someone asks "what can users do today?", point to this section.
- If someone asks "is this still a prototype?", answer: "engine is production-shaped; some higher-level memory UX is Phase 1."

---

## 3) Phase Alignment (Built vs Not Yet)

### Phase 0 (largely complete)
- Engine, temporal model, retrieval primitives, Python/MCP/iOS/WASM interfaces, and CI are all present.

### Still open (per roadmap)
- WASM playground deployment/channel completion.
- Android AAR (UniFFI/Kotlin).
- AgentMemory Phase 1 NLP + semantic recall/context assembly.

### Why this matters
- Clarifies that remaining scope is mostly distribution/productization + advanced memory UX, not core-database viability.

### Help notes
- Keep Phase 0 closed by avoiding large scope creep in core.
- Route new asks into Phase 1/2 buckets explicitly.

---

## 4) Experimental and Scaffold Areas

### `hybrid-experimental` in core
- Feature exists in `crates/core/Cargo.toml`.
- Hybrid types (`HybridParams`, etc.) exist but are scaffold-level/private.
- Not yet integrated as a stable public retrieval API.

### AgentMemory Phase 1 methods are stubs
- `remember()`, `recall()`, `assemble_context()` in `crates/agent-memory` currently `unimplemented!` with explicit Phase 1 messaging.

### Why this matters
- Prevents accidental over-claiming of semantic memory capabilities.
- Keeps demos honest: structured facts are real; unstructured memory ingestion is planned.

### Help notes
- In docs/sales: avoid promising autonomous NLP memory extraction today.
- In engineering: treat these methods as planned contract endpoints.

---

## 5) Test and CI Posture

### Current test signal
- Workspace tests with all features are passing.
- Core has substantial unit test coverage for temporal, search, vector behavior.
- MCP and WASM include tests.
- iOS crate compiles/tests in CI but currently has minimal/no unit test assertions.

### CI posture
- PR CI runs:
  - Rust tests (`--all --all-features`)
  - clippy (`-D warnings`)
  - fmt check
  - site build job
- Separate iOS workflow builds XCFramework on PRs and main.
- iOS size budget enforcement exists in `build-xcframework.sh` via `CHECK_SIZE_BUDGET=1` (< 6 MB compressed).

### Why this matters
- Strong baseline quality gate is already in place.
- Main gap is not "CI missing", but "iOS-specific behavioral tests are thin."

### Help notes
- Keep `--all-features` in CI; do not relax this.
- Add tests when expanding iOS Swift API surface.

---

## 6) iOS Distribution Status (Against Requested Deliverables)

### Present
- `cbindgen` config and generated header are present.
- XCFramework build script exists.
- Swift package wrapper exists.
- CI iOS build on PRs exists.
- Size budget check hook exists in build script and iOS workflow.

### Confirmed API shape
- Swift wrapper supports:
  - `KronroeGraph.open(url:)`
  - `assert(subject:predicate:object:)`
  - query methods (JSON path)
- Additional in-memory open path exists.

### Remaining to lock DoD
- "Kindly Roe app consumes package and executes one real simulator query" should be captured as a concrete integration check artifact (script/log/screenshot note).

### Why this matters
- Most packaging work is done; final DoD is integration proof in consuming app.

### Help notes
- Treat DoD as "proved in downstream app", not only "build artifacts exist."

---

## 7) Risks and Technical Debt (Practical)

### R1: Phase 1 memory methods are visible but non-functional
- Risk: downstream developers may call stubbed methods and hit runtime panic.
- Mitigation:
  - Keep docs explicit.
  - Consider returning typed errors instead of `unimplemented!` in public SDK-facing surfaces.

### R2: iOS functionality not deeply behavior-tested
- Risk: FFI or Swift wrapper regressions may slip despite successful packaging.
- Mitigation:
  - Add 2-3 focused integration tests (open/assert/query/error path).

### R3: Feature scaffolds can drift from implementation reality
- Risk: experimental types may stagnate and confuse prioritization.
- Mitigation:
  - Either promote Phase 1 hybrid plan into active sprint tasks or hide further until scheduled.

### Why this matters
- These are not existential risks; they are execution-quality risks that can be closed quickly.

### Help notes
- Prioritize risk closure by user impact: runtime behavior > packaging cosmetics.

---

## 8) Next Approach (Recommended Plan)

### Approach principle
- Finish "usable memory product path" before adding new platform surfaces.

### Step A: Close iOS integration proof (short, 1-2 days)
- In Kindly Roe app:
  - Add package dependency.
  - Open graph at documents URL.
  - Assert one fact and query one real entity.
- Save evidence:
  - small markdown in repo (`docs/plans/ios-integration-proof.md`) with exact command/run notes.

### Step B: Phase 1 AgentMemory implementation (primary)
- Implement in this order:
  1. Idempotency-key storage and API path.
  2. Hybrid retrieval module wiring (start behind feature gate).
  3. `remember()` structured extraction integration contract.
  4. `recall()` + `assemble_context()` stable return behavior.
- Add tests for each milestone and keep CI green with `--all-features`.

### Step C: Release hardening
- Ensure all public SDK surfaces fail gracefully (no panics for user-invoked paths).
- Update README capability matrix to distinguish:
  - "available now"
  - "experimental behind feature"
  - "planned"

### Step D: Distribution follow-through
- Complete WASM deploy channel and docs.
- Start Android AAR only after Phase 1 memory path is feature-complete in Rust core + API layers.

### Why this approach
- Maximizes product value quickly:
  - proves downstream app adoption
  - unlocks true agent-memory differentiation
  - avoids fragmentation across too many platform fronts

### Help notes
- If capacity is tight, defer Android until AgentMemory Phase 1 is shippable.
- If capacity increases, parallelize Step A (iOS proof) with Step B.1 (idempotency).

---

## 9) Suggested Immediate Backlog (Concrete Tickets)

1. `agent-memory`: replace `unimplemented!` with typed `NotYetAvailable` error variant for Phase 1 methods.
2. `core`: implement idempotency table + tests.
3. `core`: promote hybrid types from crate-private scaffold to usable module API (still gated).
4. `agent-memory`: first implementation of `recall()` using current vector/fulltext primitives.
5. `ios`: add wrapper-level behavior tests (open/assert/query/failure).
6. `docs`: add iOS consuming-app integration proof note.

### Why this matters
- Turns strategy into execution-ready work with low ambiguity.

### Help notes
- Keep each ticket mergeable in < 1 day where possible.

