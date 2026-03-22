# CLAUDE.md — Kronroe

Context for Claude Code sessions on this repository.

## Project Summary

Kronroe is an embedded temporal property graph database written in pure Rust.
Bi-temporal facts are a first-class engine primitive — not an application concern.

**The DuckDB analogy:** DuckDB didn't do SQLite better — it redesigned the engine for analytical
workloads. Kronroe redesigns the embedded graph engine for temporal knowledge evolution.

**Two target markets:**
1. AI agent memory — no server required, runs on-device
2. Mobile/edge — iOS/Android apps with full relationship graph capabilities, zero network latency

**Primary competition displaced:** Graphiti + Neo4j (requires server), mcp-memory-service
(no temporal model at engine level, no mobile).

## Repository Layout

```
kronroe/
├── crates/
│   ├── core/           # `kronroe` crate — TemporalGraph engine
│   ├── agent-memory/   # `kronroe-agent-memory` crate — AgentMemory API
│   ├── ios/            # `kronroe-ios` crate — C FFI staticlib + cbindgen header + Swift Package
│   ├── android/        # `kronroe-android` crate — JNI cdylib + Kotlin wrapper
│   ├── mcp-server/     # `kronroe-mcp` binary — stdio MCP server (11 tools)
│   ├── python/         # `kronroe-py` crate — PyO3 bindings
│   └── wasm/           # `kronroe-wasm` crate — WebAssembly bindings (browser)
├── python/
│   └── kronroe-mcp/    # pip shim — `kronroe-mcp` CLI entry point, delegates to binary
├── .github/
│   ├── workflows/
│   │   ├── ci.yml             # path-scoped Rust/WASM/site checks on relevant PRs
│   │   ├── cla.yml            # CLA assistant bot (contributors must sign CLA)
│   │   ├── ios.yml            # cross-compile check for aarch64-apple-ios targets
│   │   ├── android.yml        # host tests + cross-compile for 4 Android targets
│   │   ├── python-wheels.yml  # build Python wheels (Linux manylinux)
│   │   ├── python-publish.yml # publish to PyPI via trusted publisher (release/workflow dispatch)
│   │   └── deploy-site.yml    # Firebase Hosting live deploy + post-deploy smoke test
│   └── ISSUE_TEMPLATE/
├── LICENSE             # AGPL-3.0
├── LICENCE-COMMERCIAL.md
├── CLA.md
├── CONTRIBUTING.md
└── README.md
```

## Running the Project

```bash
# Run all tests (CI runs --all-features, match it locally)
cargo test --all --all-features

# Run with vector feature only
cargo test -p kronroe --features vector

# Lint (must pass with no warnings — CI runs --all-features, match it locally)
cargo clippy --all --all-features -- -D warnings

# Format check
cargo fmt --all -- --check

# Format (apply)
cargo fmt --all

# Run a specific test
cargo test -p kronroe test_name
cargo test -p kronroe-agent-memory test_name
cargo test -p kronroe-py test_name
cargo test -p kronroe-mcp test_name

# Run the MCP server locally (reads/writes ./kronroe-mcp.kronroe by default)
KRONROE_MCP_DB_PATH=./my.kronroe cargo run -p kronroe-mcp

# Build the iOS XCFramework (requires macOS + Xcode CLT)
bash crates/ios/scripts/build-xcframework.sh
```

## Architecture

### Bi-temporal Model

Every `Fact` has four timestamps — the standard TSQL-2 bi-temporal model:

| Field | Dimension | Meaning |
|-------|-----------|---------|
| `valid_from` | Valid time | When the fact became true in the world |
| `valid_to` | Valid time | When it stopped being true (`None` = still current) |
| `recorded_at` | Transaction time | When we first stored this fact |
| `expired_at` | Transaction time | When we overwrote/invalidated it (`None` = still active) |

Additional fact metadata fields:

| Field | Type | Meaning |
|-------|------|---------|
| `confidence` | `f32` | Confidence score for the fact (default `1.0`) |
| `source` | `Option<String>` | Optional provenance/source marker |

### Key Types (`crates/core`)

| Type | Description |
|------|-------------|
| `TemporalGraph` | Low-level engine: `open`, `open_in_memory`, `assert_fact`, `assert_fact_with_confidence`, `assert_fact_with_source`, `assert_fact_idempotent`, `assert_fact_with_embedding`, `assert_fact_checked` (feature: contradiction), `current_facts`, `facts_at`, `all_facts_about`, `fact_by_id`, `correct_fact`, `invalidate_fact`, `search`, `search_by_vector`, `search_hybrid` (feature: hybrid-experimental+vector), `register_singleton_predicate`, `detect_contradictions`, `detect_all_contradictions` (feature: contradiction), `register_predicate_volatility`, `register_source_weight`, `predicate_volatility`, `source_weight`, `effective_confidence` (feature: uncertainty) |
| `HybridSearchParams` | Stable hybrid search parameters — eval-proven defaults (rc=60, tw=0.8, vw=0.2) |
| `TemporalIntent` | Caller's temporal intent: `Timeless`, `CurrentState`, `HistoricalPoint`, `HistoricalInterval` |
| `TemporalOperator` | Temporal operator hint: `Current`, `AsOf`, `Before`, `By`, `During`, `After`, `Unknown` |
| `Contradiction` | Detected conflict: two facts, same subject+predicate, different values, overlapping valid time (feature: contradiction) |
| `PredicateCardinality` | `Singleton` (at most one active value) \| `MultiValued` (feature: contradiction) |
| `ConflictPolicy` | Write-time behavior: `Allow` \| `Warn` \| `Reject` (feature: contradiction) |
| `PredicateVolatility` | Half-life in days for predicate age decay. `f64::INFINITY` = stable (feature: uncertainty) |
| `SourceWeight` | Authority multiplier for fact source. Clamped to \[0.0, 2.0\], default 1.0 (feature: uncertainty) |
| `EffectiveConfidence` | Query-time result: `value`, `base_confidence`, `age_decay`, `source_weight` (feature: uncertainty) |
| `Fact` | The fundamental unit of storage. Fully bi-temporal. `with_confidence(f32)` and `with_source(impl Into<String>)` builders. |
| `FactId` | Kronroe Fact ID (`kf_...`) — lexicographically sortable, monotonic insertion order |
| `Value` | `Text(String)` \| `Number(f64)` \| `Boolean(bool)` \| `Entity(String)` |
| `KronroeError` | Error type |

`Entity(String)` is a reference to another entity's canonical name — this is how graph edges are expressed.

### Key Types (`crates/agent-memory`)

| Type | Description |
|------|-------------|
| `AgentMemory` | High-level API for AI agent use cases. Wraps `TemporalGraph`. |
| `AssertParams` | Optional assertion parameters for explicit valid-time control. |
| `RecallOptions` | Query options struct: `query`, `query_embedding`, `limit` (default 10), `min_confidence` filter, `confidence_filter_mode`. `#[non_exhaustive]` + builder methods (`with_min_confidence`, `with_min_effective_confidence` (feature: uncertainty), `with_max_scored_rows`). |
| `RecallScore` | Per-channel signal breakdown: `Hybrid { rrf_score, text_contrib, vector_contrib, confidence, effective_confidence }` \| `TextOnly { rank, bm25_score, confidence, effective_confidence }` |
| `ConfidenceFilterMode` | `Base` (raw fact confidence) \| `Effective` (uncertainty-aware, feature: uncertainty). Used by `RecallOptions` to select filtering signal. |

Phase 1 methods are implemented (`remember`, `recall`, `recall_scored`, `recall_with_options`, `recall_scored_with_options`, `assert_with_confidence`, `assert_with_source`, `assemble_context`, `invalidate_fact`).
`_with_params` variants (`assert_with_params`, `assert_idempotent_with_params`, `assert_with_confidence_with_params`, `assert_with_source_with_params`) accept `AssertParams { valid_from }` for explicit temporal control.
Uncertainty methods (`register_volatility`, `register_source_weight`, `effective_confidence_for_fact`, `recall_scored_with_min_effective_confidence`) available with `uncertainty` feature.
Hybrid recall: `RecallOptions` has `use_hybrid`, `temporal_intent`, `temporal_operator` fields (feature: hybrid) for two-stage reranker control.
Crate entrypoint is explicitly configured at `crates/agent-memory/src/agent_memory.rs`.

### Key Types (`crates/python`)

| Type | Description |
|------|-------------|
| `KronroeDb` | Python class wrapping `TemporalGraph` — exposes `open`, `open_in_memory`, `assert_fact`, `search` |
| `AgentMemory` | Python class wrapping `AgentMemory` — exposes `open`, `open_in_memory`, `assert_fact`, `assert_with_confidence`, `recall`, `recall_scored`, `assemble_context`, `correct_fact`, `invalidate_fact`, `facts_about` |

### Storage

- **Engine:** Kronroe append-log storage backend. Pure Rust, no C deps. Supports
  file-backed and in-memory storage.
- **Key format (Phase 0):** `"subject:predicate:fact_id"` composite string
- **Phase 0 note:** `invalidate_fact` uses a linear scan to find a fact by ID. A dedicated
  ID-keyed index is planned for Phase 1 as a performance improvement.

### Crate Layering

```
kronroe-agent-memory   ← agent ergonomics, Phase 1 memory API
kronroe-py             ← Python/PyO3 bindings
kronroe-wasm           ← browser WASM bindings (in-memory only)
kronroe-mcp            ← stdio MCP server (11 tools)
kronroe-ios            ← C FFI staticlib + cbindgen header + Swift Package
kronroe-android        ← JNI cdylib + Kotlin wrapper
        ↓
   kronroe (core)      ← TemporalGraph, bi-temporal storage, append-log backend,
                          Kronroe lexical full-text (feature: fulltext),
                          flat cosine vector index (feature: vector)
```

See naming rules in `docs/NAMING-CONVENTIONS.md` before introducing or renaming crate entrypoints.

Future crates will layer on top.

### WASM Notes (`crates/wasm`)

- Compiles to `wasm32-unknown-unknown` via `wasm-pack build --target web`
- Uses the in-memory append-log backend — no file I/O in browser
- `getrandom` with `wasm_js` feature provides `Crypto.getRandomValues` for Kronroe Fact ID generation
- The `wasm` crate builds with `--no-default-features`, so browser builds exclude the optional
  full-text engine while keeping the rest of core available; full-text search in core remains
  gated with `#[cfg(feature = "fulltext")]`
- The `vector` feature **does** compile to WASM — flat cosine has no platform restrictions
- Generated `pkg/` directory is gitignored; rebuilt each `wasm-pack build`

### iOS Notes (`crates/ios`)

- `crates/ios` is a thin C FFI crate (`kronroe-ios`) wrapping the core `TemporalGraph` API
- `crate-type = ["staticlib"]` — produces `libkronroe_ios.a` for XCFramework linking
- `cbindgen` generates `kronroe.h` in `crates/ios/include/` — consumed by the Swift Package module map
- `scripts/build-xcframework.sh` compiles for `aarch64-apple-ios` + `aarch64-apple-ios-sim`, then runs
  `xcodebuild -create-xcframework` to produce `KronroeFFI.xcframework`
- `scripts/generate-header.sh` regenerates `crates/ios/include/kronroe.h`
- Size budget: ≤ 6 MB for the XCFramework (verified in CI)
- Stable toolchain builds iOS targets cleanly — no nightly workaround needed (verified rustc 1.93.1)
- XCFramework build artifacts (`crates/ios/build/`, `crates/ios/swift/KronroeFFI.xcframework/`)
  are gitignored — run `scripts/build-xcframework.sh` locally

### Android Notes (`crates/android`)

- `crates/android` is a hand-written JNI crate (`kronroe-android`) wrapping the core `TemporalGraph` API
- `crate-type = ["cdylib", "lib"]` — `cdylib` produces `.so` for Android, `lib` allows `cargo test` on host
- Two-layer architecture: Layer 1 is a pure Rust `KronroeGraphHandle` (testable without JVM/NDK),
  Layer 2 is thin JNI bridge functions using `extern "system"` calling convention
- Only external dependency: `jni` crate (JNI type definitions — `JNIEnv`, `JString`, `jlong`, etc.)
- `default-features = false` on core dep — excludes the optional full-text engine (same as iOS)
- Handle-as-jlong pattern: `Box::into_raw(Box::new(handle)) as jlong` for Kotlin↔Rust lifecycle
- Thread-local `LAST_ERROR` for error messages (same pattern as iOS)
- Kotlin wrapper at `crates/android/kotlin/com/kronroe/KronroeGraph.kt` — mirrors Swift `KronroeGraph`
- `scripts/build-android-libs.sh` cross-compiles for 4 targets via `cargo-ndk`:
  `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`, `i686-linux-android`
- Size budget: ≤ 6 MB per arch (same as iOS)
- Build artifacts (`crates/android/build/`) are gitignored

### Python Notes (`crates/python`)

- PyO3 bindings exposing `KronroeDb` and `AgentMemory` Python classes
- Built with `maturin` — `maturin develop -m crates/python/Cargo.toml` for local dev
- `python-wheels.yml` builds Linux manylinux wheels on path-matching pushes to `main`
- `python-publish.yml` publishes to PyPI via trusted publisher on release publish/workflow dispatch
- macOS wheel build temporarily disabled in CI — add macOS runner to `python-wheels.yml` when needed
- `fact_to_dict()` serialises all `Fact` fields (including all four timestamps) to Python dicts
- **Dual test architecture:** `extension-module` feature for maturin builds, `python-runtime-tests`
  feature with `pyo3/auto-initialize` for embedded interpreter tests (mutually exclusive)
- **Test scripts:** `scripts/run_runtime_smoke.sh` (builds extension, runs Python smoke test),
  `scripts/run_rust_runtime_tests.sh` (embedded interpreter Rust-side tests with DYLD setup)
- All I/O methods use `py.allow_threads()` to release the GIL during storage operations
- Feature gating: `hybrid` and `uncertainty` features properly gated — returns `PyRuntimeError`
  when unavailable features are requested from Python

### MCP Server Notes (`crates/mcp-server`)

- Stdio transport with LSP-style `Content-Length` framing — works with any MCP client
- **Wraps `AgentMemory`** (not raw `TemporalGraph`) — inherits scored recall, context assembly,
  contradiction/uncertainty auto-registration, and all Phase 1 agent features
- Tools (11):
  - `remember` (stores free-text as facts via Kronroe parsing)
  - `recall` (full-text search with optional confidence filtering, hybrid mode, temporal intent)
  - `recall_scored` (recall with per-result signal breakdown: RRF/BM25 scores, confidence, effective confidence)
  - `assemble_context` (LLM-ready text assembly with token budget)
  - `facts_about` (fact lookup scoped to an entity)
  - `assert_fact` (structured assertion with optional confidence, source, idempotency key, valid_from)
  - `correct_fact` (in-place correction preserving history semantics)
  - `invalidate_fact` (retire a fact by ID)
  - `what_changed` (entity change summary since a timestamp)
  - `memory_health` (operational health snapshot for an entity)
  - `recall_for_task` (decision-ready recall context scoped to a task)
- Database path: `KRONROE_MCP_DB_PATH` env var (default: `./kronroe-mcp.kronroe`)
- Install binary: `cargo install --path crates/mcp-server`
- **pip shim** (`python/kronroe-mcp`): `pip install .` then `kronroe-mcp`; respects
  `KRONROE_MCP_BIN` env var to point at a custom binary location
- Feature flags: `hybrid` (temporal intent + hybrid recall), `uncertainty` (effective confidence in scores)

### Vector Index Notes (`crates/core`, feature: `vector`)

- Enabled with `--features vector` (not in `default`; callers opt in)
- **Phase 0 implementation:** flat brute-force cosine similarity — O(n·d) search,
  zero new dependencies, works on all targets (native, WASM, iOS, Android)
- `VectorIndex` is an in-memory read cache over persisted embedding rows — rebuilt from storage
  on every `open()` / `open_in_memory()` call via `rebuild_vector_index_from_db()`
- Kronroe never generates embeddings — the caller (`kronroe-agent-memory`, or the
  application) computes them and passes pre-computed `Vec<f32>` to `assert_fact_with_embedding`
- `search_by_vector(query, k, at)` gates results through a bi-temporal `valid_ids`
  allow-set: invalidated facts are excluded for current queries but remain in the index
  for historical point-in-time searches (`at = Some(t)`)
- **Phase 1 path:** if HNSW is needed, fork `rust-cv/hnsw` (no_std, no rayon, ~350 lines,
  WASM-safe) — **not** `hnsw_rs` (hard rayon+mmap deps = can never work on WASM/iOS)

### Hybrid Retrieval Notes (`crates/core`, feature: `hybrid-experimental`)

- Enabled with `--features hybrid-experimental` (requires `vector` feature too)
- **Two-stage architecture:** RRF fusion → intent-gated temporal reranker
  (reranker logic in `crates/core/src/hybrid.rs`)
- **API:** `search_hybrid(text_query, vector_query, HybridSearchParams, at)` — RRF fusion +
  two-stage reranker in one call. Eval-proven defaults (rc=60, tw=0.8, vw=0.2)
- **Caller provides intent:** `TemporalIntent` + `TemporalOperator` tell the reranker how to
  score temporal feasibility. `Timeless` (default) disables temporal scoring entirely
- **Timeless queries** use adaptive vector-dominance: the reranker inspects top-5 signal balance
  and adjusts vector/text weights dynamically
- **Temporal queries** use a two-stage pipeline: Stage 1 prunes to top-14 by semantic score,
  Stage 2 filters infeasible facts and reranks by semantic + intent-weighted temporal signal
- **Eval provenance:** promoted from `.ideas/evals/hybrid_eval_runner/` after 11 benchmark
  passes — product gate passed with +17% semantic lift, +47% time-slice lift (Mar 2026 rerun on KronroeTimestamp)
- **`agent-memory` integration:** `recall()` with `hybrid` feature uses `search_hybrid` automatically

### Contradiction Detection Notes (`crates/core`, feature: `contradiction`)

- Enabled with `--features contradiction` — no new external dependencies
- **Engine-native:** pure structural/temporal detection, no LLM required
- **Predicate cardinality registry:** callers register predicates as `Singleton` (at most one
  active value per subject at any time) or `MultiValued` (default for unregistered)
- **Detection model:** Allen's interval algebra overlap check + structural value comparison
- **Conflict severity:** `High` (full temporal containment), `Medium` (>30 day overlap), `Low` (≤30 day overlap)
- **API:**
  - `register_singleton_predicate(predicate, policy)` — persist cardinality to storage
  - `detect_contradictions(subject, predicate)` — lazy pairwise scan
  - `detect_all_contradictions()` — full scan across all registered singletons
  - `assert_fact_checked(subject, predicate, object, valid_from)` — eager detection at write time
- **ConflictPolicy:** `Allow` (store, no report), `Warn` (store + return contradictions),
  `Reject` (block storage if contradictions found)
- **`agent-memory` integration:** `open()` auto-registers common singletons (`works_at`,
  `lives_in`, `job_title`, `email`, `phone`) with `Warn` policy. `assert_checked()` and
  `audit(subject)` expose contradiction detection at the agent layer

### Uncertainty Model Notes (`crates/core`, feature: `uncertainty`)

- Enabled with `--features uncertainty` — no new external dependencies, pure Rust math
- **Engine-native:** computes effective confidence at query time, never stored back
- **Formula:** `effective = base_confidence × age_decay × source_weight`, clamped to \[0.0, 1.0\]
- **Age decay:** exponential half-life: `exp(-ln(2) × age_days / half_life_days)`. Age measured
  from `valid_from` (real-world time), not `recorded_at` (database time)
- **Predicate volatility registry:** per-predicate half-life in days. `f64::INFINITY` = stable
  (no decay). Unregistered predicates default to stable
- **Source authority weights:** per-source multiplier \[0.0, 2.0\]. `1.0` = neutral. Unknown = 1.0
- **API:**
  - `register_predicate_volatility(predicate, volatility)` — persist to storage + update in-memory
  - `register_source_weight(source, weight)` — persist to storage + update in-memory
  - `predicate_volatility(predicate) -> Option<PredicateVolatility>` — query current registration
  - `source_weight(source) -> Option<SourceWeight>` — query current registration
  - `effective_confidence(fact, at) -> EffectiveConfidence` — query-time computation
  - `assert_fact_with_source(subject, predicate, object, valid_from, confidence, source)` — store with provenance
- **Hybrid integration:** when both `uncertainty` and `hybrid-experimental` features are enabled,
  the two-stage reranker uses per-predicate decay instead of the hardcoded 365-day half-life
- **`agent-memory` integration:** `open()` auto-registers default volatilities (`works_at`: 730d,
  `job_title`: 730d, `lives_in`: 1095d, `email`: 1460d, `phone`: 1095d, `born_in`: stable,
  `full_name`: stable). `assert_with_source()`, `register_volatility()`, `register_source_weight()`
  convenience methods. `RecallScore.effective_confidence` populated automatically

## Rust / storage Gotchas

- **Borrowed storage state:** extract owned values before mutating the backend again in the
  same logical flow.
- **`unexpected_cfgs` on CI:** CI runs `clippy --all-features`. Any `#[cfg(feature = "foo")]`
  in code requires `foo = []` declared in `Cargo.toml` or clippy fails with `-D unexpected-cfgs`.
- **Targeted `git add` leaves Cargo.toml unstaged:** When committing with specific file paths,
  always run `git status` after to catch modified-but-unstaged files (especially `Cargo.toml`).
- **`Value` does not derive `PartialEq`:** Use `matches!(&val, Value::Text(s) if s == "foo")`
  in tests instead of `assert_eq!`.
- **`.ideas/` has private experiment planning docs** — gitignored, check there for context on
  experimental features before starting new work (e.g. `EXPERIMENT_01_HYBRID_RETRIEVAL_RESEARCH.md`).

## Phase 0 Milestone Status

Snapshot as of 2026-03-09. See GitHub milestones/issues for source of truth.

| # | Milestone | Status | Who |
|---|-----------|--------|-----|
| 0.1 | Scaffold + bi-temporal data model | ✅ Done | — |
| 0.2 | iOS compilation spike | ✅ Done locally (aarch64-apple-ios + aarch64-apple-ios-sim compile) | Rebekah (local) |
| 0.3 | Full-text index (Kronroe lexical engine) | ✅ Done | — |
| 0.4 | Python bindings (PyO3) | ✅ Done | — |
| 0.5 | MCP server | ✅ Done — stdio server, 11 tools (remember/recall/recall_scored/assemble_context/facts_about/assert_fact/correct_fact/invalidate_fact/what_changed/memory_health/recall_for_task), pip wrapper | — |
| 0.6 | iOS XCFramework | ✅ Done locally (aarch64-apple-ios + Swift package scaffold, commit cc4287e) | Rebekah (local) |
| 0.7 | Kindly Roe integration | ✅ Done (PR #76-78 — KronroeMemoryStore + Swift 6 compat + simulator proof) | Rebekah (local) |
| 0.8 | Vector index | ✅ Done — flat cosine similarity, zero deps, temporal filtering, PR #18 | — |
| 0.9 | Android JNI bindings | ✅ Done — hand-written JNI, Kotlin wrapper, CI workflow, 3 host tests | Claude |
| 0.10 | WASM playground | 🟡 Site scaffold + Firebase Hosting config merged — need service account secret + custom domains | Claude can help |
| 0.11 | CI pipeline | ✅ Done — `test` + `clippy` + `fmt` + iOS packaging + Python wheels all green | — |
| 0.12 | Storage format commitment | ✅ Done (PR #75 — schema version stamp + mismatch detection) | — |

## What Claude Can and Cannot Do in This Repo

**Can do** (Rust toolchain is installed via rustup):
- `cargo test --all`, `cargo clippy --all -- -D warnings`, `cargo fmt --all`
- `wasm-pack build --target web` (wasm32-unknown-unknown target installed)
- `rustup target add <target>` for cross-compilation
- `maturin develop -m crates/python/Cargo.toml` for Python bindings dev

**Cannot do:**
- **Publish to crates.io / PyPI / npm** — requires registry credentials.

## Scope Discipline (Phase 0)

Do **not** add these unless a Phase 2 decision has been made:

- Full Cypher/GQL parser
- Distributed or multi-node operation
- Cloud sync
- Schema migrations
- User-facing ACID transaction API

## Licence

Dual-licensed: **AGPL-3.0** (open source) + **commercial** (see `LICENCE-COMMERCIAL.md`).

Kindly Roe (Rebekah's iOS app) has a perpetual irrevocable commercial licence that survives
any future relicensing.

## CLA

External contributors must sign the [CLA](./CLA.md) before their PR can be merged.
The CLA bot handles this automatically on PRs. `rebekahcole` and `Becky9012` are on the allowlist.

## Owner

Rebekah Cole — rebekah@kindlyroe.com

# Memory

## Me
Rebekah Cole — project owner & sole maintainer of Kronroe. Building Kindly Roe (iOS app) that consumes it.

## People

| Who | Role |
|-----|------|
| **Rebekah** (Becky) | Rebekah Cole — owner, sole maintainer. GitHub: rebekahcole / Becky9012. rebekah@kindlyroe.com |

→ Full profiles: `memory/people/`

## Terms

| Term | Meaning |
|------|---------|
| FFI | Foreign Function Interface — C API layer for iOS/Android |
| CoW | Copy on Write |
| Kronroe Fact ID | Canonical `kf_...` sortable fact identifier used across all surfaces |
| MCP | Model Context Protocol — AI tool integration standard |
| PyO3 | Python ↔ Rust bindings framework (crates/python) |
| WASM | WebAssembly — browser target (wasm32-unknown-unknown) |
| XCFramework | Apple multi-arch binary bundle — iOS distribution format |
| AGPL | Affero General Public License v3 — open source licence |
| CLA | Contributor License Agreement — required for external PRs |
| AAR | Android Archive — Android library format (planned) |
| UniFFI | Mozilla's Rust FFI generator — planned for Android |
| TSQL-2 | Temporal SQL standard — bi-temporal model reference |
| HNSW | Hierarchical Navigable Small World — future vector index |
| P0 | Phase 0 — current development phase |
| bi-temporal | Two time dimensions: valid time + transaction time |
| fact | Fundamental storage unit — subject-predicate-value triple with 4 timestamps |
| entity | Graph node, referenced by canonical name string |
| flat cosine | Phase 0 vector search — brute-force cosine similarity, O(n·d) |
| Kindly Roe | Rebekah's iOS app — perpetual commercial licence for Kronroe |
| the DuckDB analogy | Kronroe is to graph DBs what DuckDB is to analytical DBs |

→ Full glossary: `memory/glossary.md`

## Projects

| Name | What | Status |
|------|------|--------|
| **Kronroe** | Embedded temporal property graph DB (Rust) | Active P0 |
| **Kindly Roe** | Rebekah's iOS app — consumes Kronroe | Active |

→ Details: `memory/projects/`

## Crate Short Names

| Short | Crate | Path |
|-------|-------|------|
| core | kronroe | crates/core/ |
| agent-memory | kronroe-agent-memory | crates/agent-memory/ |
| ios | kronroe-ios | crates/ios/ |
| mcp-server | kronroe-mcp | crates/mcp-server/ |
| python | kronroe-py | crates/python/ |
| android | kronroe-android | crates/android/ |
| wasm | kronroe-wasm | crates/wasm/ |

## Preferences
- CI runs `--all-features` — always match locally
- `#[cfg(feature)]` requires feature declared in Cargo.toml
- Targeted `git add` can leave Cargo.toml unstaged — always check `git status`
- `.ideas/` has private experiment planning docs — check before starting new work
