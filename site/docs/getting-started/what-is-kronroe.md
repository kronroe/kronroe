# What is Kronroe?

Kronroe is an embedded bi-temporal property graph database written in pure Rust. It treats temporal facts as a first-class engine primitive, not something you bolt on with `created_at` columns and `WHERE` clause workarounds.

Every fact stored in Kronroe carries four timestamps that track both *when something was true in the world* and *when the database learned about it*. This is the standard bi-temporal model (TSQL-2), built into the storage engine itself.

```rust
use kronroe::{TemporalGraph, Value};
use kronroe::KronroeTimestamp;

let db = TemporalGraph::open("./memory.kronroe")?;

// Assert a temporal fact
let id = db.assert_fact("alice", "works_at", Value::Text("Acme".into()), KronroeTimestamp::now_utc())?;

// Point-in-time query — first-class operation, not a WHERE clause trick
let employer = db.facts_at("alice", "works_at", past_date)?;

// Invalidation preserves history — old fact gets expired_at set, never deleted
db.invalidate_fact(&id, KronroeTimestamp::now_utc())?;

// Full-text search across all current facts
let results = db.search("where does Alice work", 10)?;
```

## The DuckDB Analogy

DuckDB did not "do SQLite better." It redesigned the engine for analytical queries instead of forcing them into a transactional shape.

Kronroe makes the same move for temporal knowledge graphs. Time is not a feature bolted on later. It is the foundation that every retrieval, correction, and confidence score is built on.

## Target Markets

### AI Agent Memory

AI agents need to remember, update, and query facts about the world over time. An agent that learns "Alice works at Acme" in January and "Alice works at Globex" in June needs an engine that understands both facts, their temporal relationship, and which one is current.

Kronroe runs in-process with no server. The `AgentMemory` API provides high-level operations like `remember`, `recall`, and `assemble_context` designed specifically for agent workflows. An MCP server (11 tools, stdio transport) is included for integration with Claude Desktop and other MCP clients.

### Mobile and Edge

iOS and Android apps that need relationship graph capabilities today must either run a server (Neo4j, Memgraph) or accept the limitations of key-value stores. Kronroe compiles to native static libraries for iOS (XCFramework + Swift Package) and Android (JNI + Kotlin wrapper) with zero network latency and no server infrastructure.

The core engine has no C dependencies, making cross-compilation straightforward.

## Understanding Bi-Temporal

Every fact in Kronroe has four timestamps across two independent time dimensions:

| Field | Dimension | Meaning |
|-------|-----------|---------|
| `valid_from` | Valid time | When the fact became true in the real world |
| `valid_to` | Valid time | When it stopped being true (`None` = still current) |
| `recorded_at` | Transaction time | When the database first stored this fact |
| `expired_at` | Transaction time | When it was overwritten or invalidated (`None` = still active) |

**Valid time** answers: "When was this true in the world?"
**Transaction time** answers: "When did the database know about it?"

These two dimensions are independent. You might learn on March 15 that Alice started working at Acme on January 1. The valid time is January 1; the transaction time is March 15.

This separation makes several things possible that single-timestamp systems cannot do:

- **Point-in-time queries**: "What did we know about Alice's employer as of February?" (Answer: nothing yet -- we hadn't recorded it.)
- **Historical reconstruction**: "What was Alice's employer on January 15?" (Answer: Acme, even though we didn't learn that until March.)
- **Audit trail**: Facts are never deleted. Invalidation sets `valid_to` and `expired_at`, preserving the full history of what was known and when.

Facts also carry optional metadata:

| Field | Type | Meaning |
|-------|------|---------|
| `confidence` | `f32` | Confidence score (default `1.0`) |
| `source` | `Option<String>` | Provenance marker for tracking where a fact came from |

## How Kronroe Compares

Kronroe's advantage is not one feature. Time, retrieval, and conflict handling are built into the engine instead of layered on afterward.

| Capability | Kronroe | Graphiti + Neo4j | mcp-memory-service |
|---|---|---|---|
| Deployment | Embedded, in-process | Neo4j-backed | Requires a server |
| Temporal model | Engine-native bi-temporal | Application-level | None at engine level |
| AI extraction | No LLM needed for structural detection | LLM extraction | Varies |
| Mobile support | Native iOS + Android | No | No |
| Browser/WASM | In-memory WASM | No | No |
| Contradictions | Engine-native, feature-gated | Application logic | No |
| Language/runtime | Pure Rust, no C deps | Python + Java | Varies |
| License | AGPL-3.0 + commercial | Mixed | Varies |

## Architecture

### Crate Layering

```
kronroe-agent-memory   <-- Agent ergonomics, high-level memory API
kronroe-py             <-- Python/PyO3 bindings
kronroe-wasm           <-- Browser WASM bindings (in-memory only)
kronroe-mcp            <-- Stdio MCP server (11 tools)
kronroe-ios            <-- C FFI staticlib + Swift Package
kronroe-android        <-- JNI cdylib + Kotlin wrapper
        |
   kronroe (core)      <-- TemporalGraph engine, bi-temporal storage,
                           full-text search (BM25 + fuzzy),
                           flat cosine vector index
```

All platform crates depend on the core `kronroe` crate. The `kronroe-agent-memory` crate provides a higher-level API that wraps `TemporalGraph` with agent-oriented methods like `remember`, `recall`, and `assemble_context`.

### Feature Flags

Several capabilities are gated behind Cargo feature flags:

| Feature | What it enables |
|---------|----------------|
| `fulltext` | BM25 + fuzzy full-text search (default on core) |
| `vector` | Flat cosine similarity vector search with temporal filtering |
| `hybrid-experimental` | Two-stage RRF fusion + temporal reranker (requires `vector`) |
| `contradiction` | Singleton predicate detection, conflict severity, write-time policy |
| `uncertainty` | Age decay, source authority weights, effective confidence at query time |

Features marked experimental have stable test coverage but their APIs may evolve.

### Data Model

Kronroe stores facts as subject-predicate-value triples. Graph edges are expressed using the `Entity` value type:

```rust
use kronroe::Value;

// A property: Alice's job title is "Engineer"
db.assert_fact("alice", "job_title", Value::Text("Engineer".into()), KronroeTimestamp::now_utc())?;

// A relationship: Alice works at Acme (an edge to another entity)
db.assert_fact("alice", "works_at", Value::Entity("acme".into()), KronroeTimestamp::now_utc())?;

// Other value types
db.assert_fact("alice", "age", Value::Number(30.0), KronroeTimestamp::now_utc())?;
db.assert_fact("alice", "active", Value::Boolean(true), KronroeTimestamp::now_utc())?;
```

Every fact gets a `FactId` (`kf_...` prefix) that is lexicographically sortable and monotonically ordered by insertion time.

## Platform Support

| Platform | Crate | Binding | Notes |
|----------|-------|---------|-------|
| Rust | `kronroe`, `kronroe-agent-memory` | Native | Core engine + agent API |
| Python | `kronroe-py` | PyO3 | `KronroeDb` and `AgentMemory` classes |
| iOS | `kronroe-ios` | C FFI + Swift | XCFramework for `aarch64-apple-ios` |
| Android | `kronroe-android` | JNI + Kotlin | 4 target architectures |
| Browser | `kronroe-wasm` | WASM | In-memory backend only |
| MCP | `kronroe-mcp` | Stdio server | 11 tools, npm and pip shims available |

## Licensing

Kronroe is dual-licensed:

- **AGPL-3.0** -- for open-source projects, personal use, and research. If you distribute or provide Kronroe as part of a network service, the AGPL requires you to make your source code available.
- **Commercial license** -- for proprietary products and SaaS applications where AGPL obligations are not acceptable. Contact [hi@kronroe.dev](mailto:hi@kronroe.dev) for details.
