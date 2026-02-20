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
│   └── agent-memory/   # `kronroe-agent-memory` crate — AgentMemory API
├── .github/
│   ├── workflows/
│   │   ├── ci.yml      # cargo test + clippy + fmt on every PR
│   │   └── cla.yml     # CLA assistant bot (contributors must sign CLA)
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
# Run all tests
cargo test --all

# Lint (must pass with no warnings)
cargo clippy --all -- -D warnings

# Format check
cargo fmt --all -- --check

# Format (apply)
cargo fmt --all

# Run a specific test
cargo test -p kronroe test_name
cargo test -p kronroe-agent-memory test_name
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
| `TemporalGraph` | Low-level engine: `open`, `assert_fact`, `current_facts`, `facts_at`, `all_facts_about`, `invalidate_fact` |
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

### Storage

- **Engine:** `redb` — pure Rust B-tree CoW ACID key-value store. No C deps.
- **Key format (Phase 0):** `"subject:predicate:fact_id"` composite string
- **Phase 0 note:** `invalidate_fact` uses a linear scan to find a fact by ID. A dedicated
  ID-keyed index is planned for Phase 1 as a performance improvement.

### Crate Layering

```
kronroe-agent-memory   ← agent ergonomics, Phase 1 NLP/vector stubs
        ↓
   kronroe (core)      ← TemporalGraph, bi-temporal storage, redb
```

Future crates will layer on top: `crates/python/`, `crates/ios/`, `crates/mcp-server/`,
`crates/android/`, `crates/wasm/`.

## Phase 0 Milestone Status

**3 of 12 complete.** See GitHub milestones for tracked issues.

| # | Milestone | Status | Who |
|---|-----------|--------|-----|
| 0.1 | Scaffold + bi-temporal data model | ✅ Done | — |
| 0.2 | iOS compilation spike | ⬜ Not started | Rebekah (local) |
| 0.3 | Full-text index (tantivy) | ⬜ Not started | Claude can help |
| 0.4 | Python bindings (PyO3) | ⬜ Not started | Claude can help |
| 0.5 | MCP server | ⬜ Not started | Claude can help |
| 0.6 | iOS XCFramework | ⬜ Not started | Claude can help (Rust side) |
| 0.7 | Kindly Roe integration | ⬜ Not started | Rebekah (local) |
| 0.8 | Vector index (hnswlib-rs) | ⬜ Not started | Claude can help |
| 0.9 | Android AAR (UniFFI) | ⬜ Not started | Claude can help |
| 0.10 | WASM playground | ⬜ Not started | Claude can help |
| 0.11 | CI pipeline | 🟡 In progress | Claude can help |
| 0.12 | Storage format commitment | ⬜ Not started | Rebekah decision |

## What Claude Cannot Do in This Repo

- **Run `cargo test`** — no Rust toolchain in the Claude shell. Rebekah runs tests locally.
- **iOS builds** — `cargo build --target aarch64-apple-ios*` requires macOS + Xcode.
  Rebekah runs the iOS spike locally.
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
The CLA bot handles this automatically on PRs. `rebekahcole` is on the allowlist.

## Owner

Rebekah Cole — rebekah@kindlyroe.com
