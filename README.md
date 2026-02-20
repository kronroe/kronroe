# Kronroe

**Embedded temporal property graph database.**  
Bi-temporal facts as a first-class engine primitive — not an application concern.

> ⚠️ Early development. Not yet ready for use.

---

## What

Every existing embedded graph database treats time as your problem. You add `created_at` to your properties. You write `WHERE valid_at BETWEEN ...` queries. The database does not know that "Alice works at Acme" was true in 2023 and false in 2024.

Kronroe treats bi-temporal facts as a **type-level design primitive enforced by the storage engine**:

```rust
// Assert a temporal fact — valid time is part of the engine, not your schema
db.assert_fact("alice", "works_at", "Acme", Utc::now())?;

// Point-in-time query — first-class operation, not a WHERE clause trick
let employer = db.facts_at("alice", "works_at", date!(2024-03-01))?;

// Invalidation — old fact gets valid_to set; history is preserved, never deleted
db.invalidate_fact(&fact_id, Utc::now())?;
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
| Key-value storage | [`redb`](https://github.com/cberner/redb) — pure Rust ACID B-tree | ✅ Phase 0 |
| Full-text search | [`tantivy`](https://github.com/quickwit-oss/tantivy) — pure Rust BM25 | ⬜ Phase 0 |
| Vector search | `hnswlib-rs` — pure Rust HNSW | ⬜ Phase 1 |
| Python bindings | `PyO3` → `pip install kronroe` | ⬜ Phase 0 |
| MCP server | Native MCP interface | ⬜ Phase 0 |
| iOS XCFramework | `cbindgen` + Swift Package | ⬜ Phase 0 |
| Android AAR | `uniffi` Kotlin bindings | ⬜ Phase 0 |
| WASM / npm | `wasm32-unknown-unknown` | 🟡 Phase 0 (bindings scaffolded) |

## Workspace

```
kronroe/
├── crates/
│   ├── core/           # The embedded database engine (crate: kronroe)
│   ├── agent-memory/   # High-level AgentMemory API (crate: kronroe-agent-memory)
│   └── wasm/           # WebAssembly bindings (crate: kronroe-wasm)
├── examples/
│   └── basic/          # Coming soon
└── README.md
```

## Status

- [x] Bi-temporal `Fact` data model (`valid_from`, `valid_to`, `recorded_at`, `expired_at`)
- [x] `assert_fact`, `current_facts`, `facts_at`, `all_facts_about`, `invalidate_fact`
- [x] `AgentMemory` API skeleton with `assert`, `facts_about`, `facts_about_at`
- [x] Tests: assert + retrieve, point-in-time query, fact invalidation
- [ ] Full-text index (tantivy)
- [ ] Vector index (hnswlib-rs)
- [ ] Python bindings (PyO3)
- [ ] MCP server
- [ ] iOS XCFramework
- [ ] Android AAR (UniFFI)
- [x] WASM bindings crate scaffold (`crates/wasm`)
- [ ] WASM / npm package publish + demo page

## Contributing

Contributions are welcome. Before your first pull request is merged, you'll be asked to sign the [Contributor Licence Agreement](./CLA.md) — a bot will prompt you automatically. The CLA lets us maintain the dual-licence model while keeping the project open.

## Licence

Kronroe is dual-licensed:

- **Open source** — [GNU Affero General Public Licence v3.0](./LICENSE) (AGPL-3.0) for open-source projects, personal use, and research
- **Commercial** — [Commercial Licence](./LICENCE-COMMERCIAL.md) for proprietary products and SaaS applications

If embedding Kronroe in a closed-source product, a commercial licence is required. See [LICENCE-COMMERCIAL.md](./LICENCE-COMMERCIAL.md) for details and how to get in touch.

Copyright © 2026 Kindly Roe Ltd
