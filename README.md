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

| Layer | Crate | Status |
|---|---|---|
| Key-value storage | [`redb`](https://github.com/cberner/redb) — pure Rust ACID B-tree CoW | ✅ Done |
| Full-text search | [`tantivy`](https://github.com/quickwit-oss/tantivy) — pure Rust BM25 (`feature: fulltext`) | ✅ Done |
| Vector search | Flat cosine similarity, zero deps, temporal filtering (`feature: vector`) | ✅ Done |
| Python bindings | `PyO3` → `pip install kronroe` (Linux wheels built on CI) | ✅ Done |
| MCP server | stdio transport, 5 tools (`remember` / `recall` / `facts_about` / `assert_fact` / `correct_fact`) | ✅ Done |
| iOS XCFramework | `cbindgen` + Swift Package (`crates/ios`) | ✅ Done (locally) |
| WASM / npm | `wasm32-unknown-unknown`, in-memory only (`crates/wasm`) | 🟡 Scaffold merged — deploy pending |
| Android AAR | `uniffi` Kotlin bindings | ⬜ Phase 1 |

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

## Status

- [x] Bi-temporal `Fact` data model (`valid_from`, `valid_to`, `recorded_at`, `expired_at`)
- [x] `assert_fact`, `current_facts`, `facts_at`, `all_facts_about`, `invalidate_fact`
- [x] Full-text search (`tantivy` BM25, fuzzy matching, `feature: fulltext`)
- [x] Flat cosine vector search with bi-temporal filtering (`feature: vector`)
- [x] Single-transaction atomicity — fact + embedding commit atomically in one redb `WriteTransaction`
- [x] `AgentMemory` high-level API (`crates/agent-memory`)
- [x] Python bindings (`KronroeDb`, `AgentMemory`) — Linux manylinux wheels built on CI
- [x] MCP server — stdio transport, 5 tools, pip + npm shim wrappers
- [x] iOS XCFramework (`aarch64-apple-ios` + simulator, Swift Package)
- [x] WASM bindings (`wasm32-unknown-unknown`, `redb` in-memory backend)
- [x] CI — test + clippy + fmt + iOS packaging + Python wheels
- [ ] WASM playground deploy (Firebase Hosting — needs service account secret + custom domain)
- [ ] Android AAR (UniFFI Kotlin bindings)
- [ ] `AgentMemory.remember` / `recall` / `assemble_context` NLP layer (Phase 1)

## Contributing

Contributions are welcome. Before your first pull request is merged, you'll be asked to sign the [Contributor Licence Agreement](./CLA.md) — a bot will prompt you automatically. The CLA lets us maintain the dual-licence model while keeping the project open.

Naming standards for crate entrypoints and path references are documented in [`docs/NAMING-CONVENTIONS.md`](./docs/NAMING-CONVENTIONS.md).

## Licence

Kronroe is dual-licensed:

- **Open source** — [GNU Affero General Public Licence v3.0](./LICENSE) (AGPL-3.0) for open-source projects, personal use, and research
- **Commercial** — [Commercial Licence](./LICENCE-COMMERCIAL.md) for proprietary products and SaaS applications

If embedding Kronroe in a closed-source product, a commercial licence is required. See [LICENCE-COMMERCIAL.md](./LICENCE-COMMERCIAL.md) for details and how to get in touch.

Copyright © 2026 Kindly Roe Ltd
