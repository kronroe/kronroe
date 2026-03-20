# Kronroe

**Embedded bi-temporal graph database for AI agent memory.**

Kronroe is a Rust-native temporal property graph engine with Python bindings. It treats bi-temporal facts as a first-class engine primitive — not an application concern. No server required. Runs on-device.

> DuckDB didn't "do SQLite better" — it redesigned the engine for analytical workloads.
> Kronroe redesigns the embedded graph engine for temporal knowledge evolution.

## Why Kronroe?

| | Kronroe | Graphiti/Zep | mcp-memory-service |
|---|---|---|---|
| Requires server | No — embedded, single file | Yes — Neo4j or FalkorDB | No |
| Temporal model | Bi-temporal (valid time + transaction time) | Bi-temporal (via Neo4j) | None |
| Mobile/edge support | iOS, Android, WASM | No | No |
| LLM required | No — engine-native operations | Yes — entity extraction | No |
| Contradiction detection | Engine-native (Allen's interval algebra) | LLM-based | No |
| Confidence/uncertainty | Engine-native decay model | No | No |
| Full-text search | BM25 + fuzzy (Kronroe lexical engine) | Via Neo4j | No |
| Vector search | Cosine similarity + temporal filtering | Via Neo4j | No |
| Licence | AGPL-3.0 + Commercial | Apache-2.0 | MIT |

## Quickstart

```python
from kronroe import AgentMemory

# Open a database (creates the file if it doesn't exist)
memory = AgentMemory.open("./my-agent.kronroe")

# Store facts — temporal metadata is handled by the engine
memory.assert_fact("alice", "works_at", "Acme")
memory.assert_fact("alice", "lives_in", "London")

# Search with natural language
results = memory.recall("where does Alice work?", limit=5)
for fact in results:
    print(f"{fact['subject']} {fact['predicate']} {fact['object']}")

# Get scored results with signal breakdown
scored = memory.recall_scored("Alice", limit=10)
for row in scored:
    print(f"{row['fact']['subject']}: {row['score']}")

# Assemble LLM-ready context with a token budget
context = memory.assemble_context("alice", max_tokens=200)
print(context)

# Store facts with confidence and source provenance
fact_id = memory.assert_with_confidence(
    "alice", "works_at", "Beta Corp", 0.95, source="hr:system"
)

# Correct a fact — old value is preserved in history, never deleted
memory.correct_fact(fact_id, "New Corp")

# Query all facts about an entity
facts = memory.facts_about("alice")
```

## How it works

Every fact in Kronroe has four timestamps — the standard bi-temporal model:

- **valid_from** / **valid_to** — when the fact was true in the real world
- **recorded_at** / **expired_at** — when the database stored or retired it

This means you can query "what did we know about Alice on March 1st?" and get a different answer than "what do we know about Alice now?" — without writing any temporal logic yourself. The engine handles it.

## Architecture

Pure Rust core. No C dependencies. Python bindings via PyO3.

The same engine also compiles to iOS (XCFramework), Android (JNI), WASM (browser), and runs as an MCP server with 11 tools for Claude Desktop, Cursor, and other MCP clients.

## Low-level API

For direct engine access without the agent memory layer:

```python
from kronroe import KronroeDb

db = KronroeDb.open("./data.kronroe")
fact_id = db.assert_fact("alice", "works_at", "Acme")
results = db.search("alice Acme", limit=10)
```

## Links

- [GitHub](https://github.com/kronroe/kronroe) — source, issues, contributing
- [Commercial licence](https://github.com/kronroe/kronroe/blob/main/LICENCE-COMMERCIAL.md) — for proprietary/SaaS use
- [MCP server](https://github.com/kronroe/kronroe/tree/main/crates/mcp-server) — 11 tools for AI assistants

## Licence

Dual-licensed: [AGPL-3.0](https://github.com/kronroe/kronroe/blob/main/LICENSE) for open-source use, [Commercial](https://github.com/kronroe/kronroe/blob/main/LICENCE-COMMERCIAL.md) for proprietary products.

Copyright 2026 Kindly Roe Ltd.
