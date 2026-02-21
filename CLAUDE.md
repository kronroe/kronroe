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
│   ├── mcp-server/     # `kronroe-mcp` binary — stdio MCP server (remember/recall tools)
│   ├── python/         # `kronroe-python` crate — PyO3 bindings
│   └── wasm/           # `kronroe-wasm` crate — WebAssembly bindings (browser)
├── packages/
│   └── kronroe-mcp/    # npm shim — delegates to `kronroe-mcp` binary on PATH
├── python/
│   └── kronroe-mcp/    # pip shim — `kronroe-mcp` CLI entry point, delegates to binary
├── .github/
│   ├── workflows/
│   │   ├── ci.yml             # cargo test + clippy + fmt on every PR
│   │   ├── cla.yml            # CLA assistant bot (contributors must sign CLA)
│   │   ├── ios.yml            # cross-compile check for aarch64-apple-ios targets
│   │   ├── python-wheels.yml  # build Python wheels (Linux manylinux)
│   │   └── python-publish.yml # publish to PyPI via trusted publisher on version tags
│   └── ISSUE_TEMPLATE/
├── examples/
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
cargo test -p kronroe-python test_name
cargo test -p kronroe-mcp test_name

# Run the MCP server locally (reads/writes ./kronroe-mcp.kronroe by default)
KRONROE_MCP_DB_PATH=./my.kronroe cargo run -p kronroe-mcp

# Build the iOS XCFramework (requires macOS + Xcode CLT)
bash crates/ios/build-xcframework.sh
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

### Key Types (`crates/core`)

| Type | Description |
|------|-------------|
| `TemporalGraph` | Low-level engine: `open`, `open_in_memory`, `assert_fact`, `assert_fact_with_embedding`, `current_facts`, `facts_at`, `all_facts_about`, `invalidate_fact`, `search`, `search_by_vector` |
| `Fact` | The fundamental unit of storage. Fully bi-temporal. |
| `FactId` | ULID — lexicographically sortable, monotonic insertion order |
| `Value` | `Text(String)` \| `Number(f64)` \| `Boolean(bool)` \| `Entity(String)` |
| `KronroeError` | Error type |

`Entity(String)` is a reference to another entity's canonical name — this is how graph edges are expressed.

### Key Types (`crates/agent-memory`)

| Type | Description |
|------|-------------|
| `AgentMemory` | High-level API for AI agent use cases. Wraps `TemporalGraph`. |

Phase 1 methods (`remember`, `recall`, `assemble_context`) are currently `unimplemented!()` stubs.

### Key Types (`crates/python`)

| Type | Description |
|------|-------------|
| `KronroeDb` | Python class wrapping `TemporalGraph` — exposes `assert_fact`, `search`, `facts_about`, `facts_about_at` |
| `AgentMemory` | Python class wrapping `AgentMemory` — high-level agent API |

### Storage

- **Engine:** `redb` 3.1 — pure Rust B-tree CoW ACID key-value store. No C deps. Supports
  file-backed (`Database::create`) and in-memory (`InMemoryBackend`) storage.
- **Key format (Phase 0):** `"subject:predicate:fact_id"` composite string
- **Phase 0 note:** `invalidate_fact` uses a linear scan to find a fact by ID. A dedicated
  ID-keyed index is planned for Phase 1 as a performance improvement.

### Crate Layering

```
kronroe-agent-memory   ← agent ergonomics, Phase 1 NLP/vector stubs
kronroe-python         ← Python/PyO3 bindings
kronroe-wasm           ← browser WASM bindings (in-memory only)
kronroe-mcp            ← stdio MCP server (remember/recall tools)
kronroe-ios            ← C FFI staticlib + cbindgen header + Swift Package
        ↓
   kronroe (core)      ← TemporalGraph, bi-temporal storage, redb 3.1,
                          tantivy full-text (feature: fulltext),
                          flat cosine vector index (feature: vector)
```

Future crates will layer on top: `crates/android/`.

### WASM Notes (`crates/wasm`)

- Compiles to `wasm32-unknown-unknown` via `wasm-pack build --target web`
- Uses `redb::backends::InMemoryBackend` — no file I/O in browser
- `getrandom` with `wasm_js` feature provides `Crypto.getRandomValues` for ULID generation
- tantivy does **not** compile to WASM (rayon dep, `std::time::Instant` panic) — the `wasm`
  crate builds with `--no-default-features` to exclude tantivy; full-text search in core is
  already gated with `#[cfg(feature = "fulltext")]`
- The `vector` feature **does** compile to WASM — flat cosine has no platform restrictions
- Generated `pkg/` directory is gitignored; rebuilt each `wasm-pack build`

### iOS Notes (`crates/ios`)

- `crates/ios` is a thin C FFI crate (`kronroe-ios`) wrapping the core `TemporalGraph` API
- `crate-type = ["staticlib"]` — produces `libkronroe_ios.a` for XCFramework linking
- `cbindgen` generates `KronroeFFI.h` — the C header consumed by the Swift Package
- `build-xcframework.sh` compiles for `aarch64-apple-ios` + `aarch64-apple-ios-sim`, then runs
  `xcodebuild -create-xcframework` to produce `KronroeFFI.xcframework`
- Size budget: ≤ 6 MB for the XCFramework (verified in CI)
- Stable toolchain builds iOS targets cleanly — no nightly workaround needed (verified rustc 1.93.1)
- XCFramework build artifacts (`crates/ios/build/`, `crates/ios/swift/KronroeFFI.xcframework/`)
  are gitignored — run `build-xcframework.sh` locally

### Python Notes (`crates/python`)

- PyO3 bindings exposing `KronroeDb` and `AgentMemory` Python classes
- Built with `maturin` — `maturin develop -m crates/python/Cargo.toml` for local dev
- `python-wheels.yml` builds Linux manylinux wheels on every push to `main`
- `python-publish.yml` publishes to PyPI via trusted publisher on version tags (`v*.*.*`)
- macOS wheel build temporarily disabled in CI — add macOS runner to `python-wheels.yml` when needed
- `fact_to_dict()` serialises all `Fact` fields (including all four timestamps) to Python dicts

### MCP Server Notes (`crates/mcp-server`)

- Stdio transport with LSP-style `Content-Length` framing — works with any MCP client
- Tools: `remember` (stores free-text as facts via tantivy parse), `recall` (full-text search,
  returns structured fact list)
- Database path: `KRONROE_MCP_DB_PATH` env var (default: `./kronroe-mcp.kronroe`)
- Install binary: `cargo install --path crates/mcp-server`
- **npm shim** (`packages/kronroe-mcp`): `npx kronroe-mcp` — delegates to binary on PATH
- **pip shim** (`python/kronroe-mcp`): `pip install .` then `kronroe-mcp`; respects
  `KRONROE_MCP_BIN` env var to point at a custom binary location

### Vector Index Notes (`crates/core`, feature: `vector`)

- Enabled with `--features vector` (not in `default`; callers opt in)
- **Phase 0 implementation:** flat brute-force cosine similarity — O(n·d) search,
  zero new dependencies, works on all targets (native, WASM, iOS, Android)
- `VectorIndex` is an in-memory read cache over the `EMBEDDINGS` redb table — rebuilt from redb
  on every `open()` / `open_in_memory()` call via `rebuild_vector_index_from_db()`
- Kronroe never generates embeddings — the caller (`kronroe-agent-memory`, or the
  application) computes them and passes pre-computed `Vec<f32>` to `assert_fact_with_embedding`
- `search_by_vector(query, k, at)` gates results through a bi-temporal `valid_ids`
  allow-set: invalidated facts are excluded for current queries but remain in the index
  for historical point-in-time searches (`at = Some(t)`)
- **Phase 1 path:** if HNSW is needed, fork `rust-cv/hnsw` (no_std, no rayon, ~350 lines,
  WASM-safe) — **not** `hnsw_rs` (hard rayon+mmap deps = can never work on WASM/iOS)

## Rust / redb Gotchas

- **redb `AccessGuard` borrow:** `table.get("key")?` returns `AccessGuard<V>` that borrows
  `table`. Extract to owned before any mutable borrow:
  `let v: Option<u64> = table.get("key")?.map(|g| g.value());`
- **`unexpected_cfgs` on CI:** CI runs `clippy --all-features`. Any `#[cfg(feature = "foo")]`
  in code requires `foo = []` declared in `Cargo.toml` or clippy fails with `-D unexpected-cfgs`.
- **Targeted `git add` leaves Cargo.toml unstaged:** When committing with specific file paths,
  always run `git status` after to catch modified-but-unstaged files (especially `Cargo.toml`).
- **`Value` does not derive `PartialEq`:** Use `matches!(&val, Value::Text(s) if s == "foo")`
  in tests instead of `assert_eq!`.
- **`.ideas/` has private experiment planning docs** — gitignored, check there for context on
  experimental features before starting new work (e.g. `EXPERIMENT_01_HYBRID_RETRIEVAL_RESEARCH.md`).

## Phase 0 Milestone Status

Snapshot as of 2026-02-21. See GitHub milestones/issues for source of truth.

| # | Milestone | Status | Who |
|---|-----------|--------|-----|
| 0.1 | Scaffold + bi-temporal data model | ✅ Done | — |
| 0.2 | iOS compilation spike | ✅ Done locally (aarch64-apple-ios + aarch64-apple-ios-sim compile) | Rebekah (local) |
| 0.3 | Full-text index (tantivy) | ✅ Done | — |
| 0.4 | Python bindings (PyO3) | ✅ Done | — |
| 0.5 | MCP server | ✅ Done — stdio server, 5 tools (remember/recall/facts_about/assert_fact/correct_fact), pip wrapper | — |
| 0.6 | iOS XCFramework | ✅ Done locally (aarch64-apple-ios + Swift package scaffold, commit cc4287e) | Rebekah (local) |
| 0.7 | Kindly Roe integration | ⬜ Not started | Rebekah (local) |
| 0.8 | Vector index | ✅ Done — flat cosine similarity, zero deps, temporal filtering, PR #18 | — |
| 0.9 | Android AAR (UniFFI) | ⬜ Not started | Claude can help |
| 0.10 | WASM playground | 🟡 Site scaffold + Firebase Hosting config merged — need service account secret + custom domains | Claude can help |
| 0.11 | CI pipeline | ✅ Done — `test` + `clippy` + `fmt` + iOS packaging + Python wheels all green | — |
| 0.12 | Storage format commitment | ⬜ Not started | Rebekah decision |

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
