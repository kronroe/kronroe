# Quick Start: Rust

## Add Kronroe to your project

```bash
cargo add kronroe
```

This gives you the core `TemporalGraph` engine with full-text search enabled by default.

## Open a database

```rust
use kronroe::TemporalGraph;

// File-backed — data persists to disk (ACID storage)
let db = TemporalGraph::open("./my-graph.kronroe")?;

// In-memory — fast, ephemeral, no file I/O
let db = TemporalGraph::open_in_memory()?;
```

## Assert and query facts

Every fact is a subject-predicate-object triple with bi-temporal metadata. The engine tracks both when the fact was true in the world (valid time) and when you stored it (transaction time).

```rust
use kronroe::{TemporalGraph, Value};
use kronroe::KronroeTimestamp;

let db = TemporalGraph::open("./my-graph.kronroe")?;

// Assert a fact — returns a FactId (kf_... format)
let fact_id = db.assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())?;

// Values convert automatically from &str, String, f64, and bool
db.assert_fact("alice", "age", Value::Number(32.0), KronroeTimestamp::now_utc())?;

// Entity references create graph edges
db.assert_fact("alice", "reports_to", Value::Entity("bob".into()), KronroeTimestamp::now_utc())?;

// Query current facts for a subject + predicate
let facts = db.current_facts("alice", "works_at")?;
for fact in &facts {
    println!("{} {} {}", fact.subject, fact.predicate, fact.object);
}

// Point-in-time query — what was true on a past date?
let past = "2024-06-15T00:00:00Z".parse().unwrap();
let facts_then = db.facts_at("alice", "works_at", past)?;

// Get all facts about an entity (any predicate)
let everything = db.all_facts_about("alice")?;
```

## Correct and invalidate facts

Kronroe never deletes data. Corrections and invalidations preserve full history.

```rust
use kronroe::{TemporalGraph, Value};
use kronroe::KronroeTimestamp;

let db = TemporalGraph::open("./my-graph.kronroe")?;
let fact_id = db.assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())?;

// Correct a fact — invalidates the old value, asserts a new one, returns the new FactId
let new_id = db.correct_fact(&fact_id, "Initech", KronroeTimestamp::now_utc())?;

// Invalidate a fact — sets valid_to on the old fact, nothing new is asserted
db.invalidate_fact(&new_id, KronroeTimestamp::now_utc())?;
```

## Full-text search

Full-text search is enabled by default via the `fulltext` feature (BM25 + fuzzy matching).

```rust
use kronroe::TemporalGraph;

let db = TemporalGraph::open_in_memory()?;
db.assert_fact("alice", "works_at", "Acme Corp", chrono::KronroeTimestamp::now_utc())?;
db.assert_fact("bob", "works_at", "Globex", chrono::KronroeTimestamp::now_utc())?;

// Search across all current facts — returns up to `limit` results ranked by BM25
let results = db.search("where does Alice work", 10)?;
for fact in &results {
    println!("{}: {} = {}", fact.subject, fact.predicate, fact.object);
}
```

## AgentMemory API

For AI agent use cases, the `kronroe-agent-memory` crate provides a higher-level API with scored recall and LLM context assembly.

```bash
cargo add kronroe-agent-memory
```

```rust
use kronroe_agent_memory::AgentMemory;

let memory = AgentMemory::open("./agent.kronroe")?;

// Assert structured facts
memory.assert("alice", "works_at", "Acme")?;
memory.assert("alice", "job_title", "Engineer")?;

// Recall matching facts (full-text search, returns up to `limit` results)
let facts = memory.recall("where does alice work", None, 5)?;

// Recall with per-result score breakdown
let scored = memory.recall_scored("alice", None, 5)?;

// Assemble LLM-ready context within a token budget
let context = memory.assemble_context("Tell me about Alice", None, 2048)?;
println!("{context}");

// Query all current facts about an entity
let facts = memory.facts_about("alice")?;
```

### Confidence and source tracking

```rust
use kronroe_agent_memory::AgentMemory;

let memory = AgentMemory::open_in_memory()?;

// Assert with explicit confidence (0.0 to 1.0)
memory.assert_with_confidence("alice", "works_at", "Acme", 0.9)?;

// Assert with source provenance
memory.assert_with_source("alice", "works_at", "Acme", 1.0, "linkedin-scrape")?;
```

## Feature flags

The `kronroe` core crate has the following feature gates:

| Feature | Default | Description |
|---------|---------|-------------|
| `fulltext` | Yes | BM25 + fuzzy full-text search via Kronroe lexical engine |
| `vector` | No | Flat cosine similarity vector search with temporal filtering |
| `hybrid-experimental` | No | Two-stage RRF fusion retrieval (requires `vector`) |
| `contradiction` | No | Singleton predicate registry + Allen's interval overlap detection |
| `uncertainty` | No | Age decay + source authority = effective confidence at query time |

Enable features in your `Cargo.toml`:

```toml
[dependencies]
kronroe = { version = "...", features = ["fulltext", "vector"] }
```

The `kronroe-agent-memory` crate has corresponding feature flags (`hybrid`, `contradiction`, `uncertainty`) that enable the higher-level wrappers for those capabilities.

## In-memory vs file-backed

Both `TemporalGraph` and `AgentMemory` support two storage modes:

| Mode | Constructor | Persistence | Use case |
|------|------------|-------------|----------|
| File-backed | `::open("path.kronroe")` | Durable, ACID | Production, long-lived agents |
| In-memory | `::open_in_memory()` | None (lost on drop) | Tests, ephemeral sessions, WASM |

The API is identical in both modes.
