# Kronroe

**Embedded temporal property graph database.**
Bi-temporal facts as a first-class engine primitive — not an application concern.

> ⚠️ Early development. Not yet ready for production use.

---

## What

Every existing embedded graph database treats time as your problem. You add `created_at` to your properties. You write `WHERE valid_at BETWEEN ...` queries. The database does not know that "Alice works at Acme" was true in 2023 and false in 2024.

Kronroe treats bi-temporal facts as a **type-level design primitive enforced by the storage engine**:

```rust
// Assert a temporal fact — valid time is part of the engine, not your schema
db.assert_fact("alice", "works_at", "Acme", Utc::now())?;

// Point-in-time query — first-class operation, not a WHERE clause trick
let employer = db.facts_at("alice", "works_at", past_date)?;

// Invalidation — old fact gets valid_to set; history is preserved, never deleted
db.invalidate_fact(&fact_id, Utc::now())?;

// Full-text search across all current facts
let results = db.search("where does Alice work", 10)?;

// Semantic vector search — pass pre-computed embeddings, temporal filtering included
db.assert_fact_with_embedding("alice", "bio", "Software engineer", Utc::now(), embedding)?;
let nearest = db.search_by_vector(query_vec, 5, None)?;
```

This is the DuckDB move. DuckDB did not "do SQLite better" — it said analytical queries deserve their own engine design. Kronroe says temporal knowledge evolution deserves its own graph engine design.

## Why now

Two use cases are completely unserved:

- **AI agent memory** — agents that need to remember, update, and query facts about the world over time, without running a server
- **Mobile/edge** — iOS and Android apps that need relationship graph capabilities without network latency or server infrastructure

The solutions developers reach for today (Graphiti + Neo4j, mcp-memory-service) require a running server, have no temporal model at the engine level, and do not run on mobile.

## Architecture

Pure Rust. No C dependencies in the core engine.

| Layer | Crate | Notes |
|---|---|---|
| Key-value storage | [`redb`](https://github.com/cberner/redb) | Pure Rust ACID B-tree CoW |
| Full-text search | [`tantivy`](https://github.com/quickwit-oss/tantivy) | BM25 + fuzzy matching (`feature: fulltext`) |
| Vector search | `crates/core/src/vector.rs` | Flat cosine similarity + temporal filtering (`feature: vector`) |
| Python bindings | `crates/python` | `PyO3` bindings for core + agent memory |
| MCP server | `crates/mcp-server` | stdio transport, 5 tools |
| iOS bindings | `crates/ios` | C FFI + XCFramework + Swift Package |
| WASM bindings | `crates/wasm` | In-memory backend only |
| Android bindings | _(planned)_ | UniFFI Kotlin bindings |

## Workspace

```
kronroe/
├── crates/
│   ├── core/           # kronroe — TemporalGraph engine, bi-temporal storage
│   ├── agent-memory/   # kronroe-agent-memory — high-level AgentMemory API
│   ├── mcp-server/     # kronroe-mcp — stdio MCP server binary
│   ├── python/         # kronroe-python — PyO3 bindings
│   ├── wasm/           # kronroe-wasm — WebAssembly bindings (in-memory)
│   └── ios/            # kronroe-ios — C FFI staticlib + Swift Package
├── packages/
│   └── kronroe-mcp/    # npm shim — npx kronroe-mcp
├── python/
│   └── kronroe-mcp/    # pip shim — kronroe-mcp CLI entry point
└── examples/
```

## Quickstarts

### MCP server (Claude Desktop / any MCP client)

```bash
cargo install --path crates/mcp-server
```

Add to your MCP client config:

```json
{
  "mcpServers": {
    "kronroe": {
      "command": "kronroe-mcp",
      "env": { "KRONROE_MCP_DB_PATH": "~/.kronroe/memory.kronroe" }
    }
  }
}
```

The server exposes five tools: `remember`, `recall`, `facts_about`, `assert_fact`, `correct_fact`.

### Python

```python
from kronroe import KronroeDb
from datetime import datetime, timezone

db = KronroeDb.open("./memory.kronroe")

# Assert and query temporal facts
db.assert_fact("alice", "works_at", "Acme", datetime.now(timezone.utc))
facts = db.facts_about("alice")

# Full-text search
results = db.search("where does Alice work", 10)
```

### Rust

```rust
use kronroe::{TemporalGraph, Value};
use chrono::Utc;

let db = TemporalGraph::open("./memory.kronroe")?;

let id = db.assert_fact("alice", "works_at", Value::Text("Acme".into()), Utc::now())?;
let current = db.current_facts("alice", "works_at")?;
let historical = db.facts_at("alice", "works_at", past_date)?;
db.invalidate_fact(&id, Utc::now())?;
```

## Capability Matrix

### Available (shipping in repo)

| Capability | Where | Quick verification |
|---|---|---|
| Bi-temporal fact model + core CRUD (`assert_fact`, `facts_at`, `invalidate_fact`, etc.) | `crates/core/src/temporal_graph.rs` | `cargo test -p kronroe` |
| Full-text search (BM25 + fuzzy) | `crates/core/src/temporal_graph.rs` (`feature: fulltext`, default on core) | `cargo test -p kronroe search_ --all-features` |
| Vector search with temporal filtering | `crates/core/src/temporal_graph.rs`, `crates/core/src/vector.rs` (`feature: vector`) | `cargo test -p kronroe vector_ --all-features` |
| Atomic fact + embedding write transaction | `assert_fact_with_embedding` in core | see vector durability/error tests in core suite |
| Idempotent writes (`assert_fact_idempotent`) | core + agent-memory wrappers | `cargo test -p kronroe idempotent --all-features` |
| `AgentMemory` API surface (`assert`, `remember`, `recall`, `assemble_context`) | `crates/agent-memory/src/agent_memory.rs` | `cargo test -p kronroe-agent-memory --all-features` |
| MCP server (5 tools) + npm/pip shims | `crates/mcp-server`, `packages/kronroe-mcp`, `python/kronroe-mcp` | `cargo test -p kronroe-mcp` |
| Python bindings (`KronroeDb`, `AgentMemory`) | `crates/python/src/python_bindings.rs` | `cargo build -p kronroe-py` |
| iOS package artifacts + behavior tests | `crates/ios` | `cargo test -p kronroe-ios` and `./crates/ios/scripts/run-swift-tests.sh` |
| WASM bindings (in-memory engine, no persistent file backend) | `crates/wasm/src/wasm_bindings.rs` | `cargo build -p kronroe-wasm` |

### Experimental (feature-gated, API may change)

| Capability | Gate | Current status |
|---|---|---|
| Hybrid retrieval API (`search_hybrid_experimental`) with deterministic ranking + score breakdown | `kronroe` features `hybrid-experimental` + `vector` | Implemented and tested in core; intentionally marked experimental |
| Agent-memory hybrid recall path (text + vector fusion) | `kronroe-agent-memory` feature `hybrid` | Implemented via core experimental API; contract may evolve |

### Planned (not shipping yet)

| Capability | Status |
|---|---|
| WASM playground hosting/deploy (Firebase Hosting + domain wiring) | Planned |
| Android AAR / Kotlin bindings (UniFFI path) | Planned |
| Rich NLP extraction/planning layer beyond current `AgentMemory` primitives | Planned |

## Contributing

Contributions are welcome. Before your first pull request is merged, you'll be asked to sign the [Contributor Licence Agreement](./CLA.md) — a bot will prompt you automatically. The CLA lets us maintain the dual-licence model while keeping the project open.

Naming standards for crate entrypoints and path references are documented in [`docs/NAMING-CONVENTIONS.md`](./docs/NAMING-CONVENTIONS.md).

## Licence

Kronroe is dual-licensed:

- **Open source** — [GNU Affero General Public Licence v3.0](./LICENSE) (AGPL-3.0) for open-source projects, personal use, and research
- **Commercial** — [Commercial Licence](./LICENCE-COMMERCIAL.md) for proprietary products and SaaS applications

If embedding Kronroe in a closed-source product, a commercial licence is required. See [LICENCE-COMMERCIAL.md](./LICENCE-COMMERCIAL.md) for details and how to get in touch.

Copyright © 2026 Kindly Roe Ltd
