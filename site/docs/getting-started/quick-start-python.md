# Quick Start: Python

## Installation

```python
pip install kronroe
```

## Two APIs

Kronroe exposes two Python classes:

- **`KronroeDb`** -- direct access to the temporal graph engine. You control every detail.
- **`AgentMemory`** -- high-level API designed for AI agent use cases. Adds scored recall, context assembly, change tracking, and health reports on top of the engine.

Most users should start with `AgentMemory`.

## KronroeDb (low-level)

### Open a database

<div class="docs-tabs" data-docs-tabs>
  <div class="docs-tabs-list" role="tablist" aria-label="KronroeDb database modes">
    <button class="docs-tab" role="tab" id="python-kronroedb-file-tab" aria-controls="python-kronroedb-file-panel" aria-selected="true">File-backed</button>
    <button class="docs-tab" role="tab" id="python-kronroedb-memory-tab" aria-controls="python-kronroedb-memory-panel" aria-selected="false" tabindex="-1">In-memory</button>
  </div>
  <div class="docs-tab-panels">
    <div class="docs-tab-panel" role="tabpanel" id="python-kronroedb-file-panel" aria-labelledby="python-kronroedb-file-tab">
      <p class="docs-tab-note">Use this when you want a durable local `.kronroe` file.</p>
      <pre><code class="language-python">from kronroe import KronroeDb

db = KronroeDb.open("./data.kronroe")</code></pre>
    </div>
    <div class="docs-tab-panel" role="tabpanel" id="python-kronroedb-memory-panel" aria-labelledby="python-kronroedb-memory-tab" hidden>
      <p class="docs-tab-note">Use this for tests, notebooks, and ephemeral sessions.</p>
      <pre><code class="language-python">from kronroe import KronroeDb

db = KronroeDb.open_in_memory()</code></pre>
    </div>
  </div>
</div>

### Assert facts

```python
fact_id = db.assert_fact("alice", "works_at", "Acme")
fact_id = db.assert_fact("alice", "age", 34)
fact_id = db.assert_fact("alice", "active", True)
```

The `object` parameter accepts `str`, `int`, `float`, or `bool`. Each call returns a Kronroe Fact ID (`kf_...`).

### Search

```python
results = db.search("alice Acme", limit=10)
for fact in results:
    print(f"{fact['subject']} {fact['predicate']} {fact['object']}")
```

Results are returned as a list of dicts. Each dict contains `id`, `subject`, `predicate`, `object`, `object_type`, `valid_from`, `valid_to`, `recorded_at`, `expired_at`, `confidence`, and `source`.

## AgentMemory (high-level)

### Open a database

<div class="docs-tabs" data-docs-tabs>
  <div class="docs-tabs-list" role="tablist" aria-label="AgentMemory database modes">
    <button class="docs-tab" role="tab" id="python-agentmemory-file-tab" aria-controls="python-agentmemory-file-panel" aria-selected="true">File-backed</button>
    <button class="docs-tab" role="tab" id="python-agentmemory-memory-tab" aria-controls="python-agentmemory-memory-panel" aria-selected="false" tabindex="-1">In-memory</button>
  </div>
  <div class="docs-tab-panels">
    <div class="docs-tab-panel" role="tabpanel" id="python-agentmemory-file-panel" aria-labelledby="python-agentmemory-file-tab">
      <p class="docs-tab-note">Use this for long-lived agents and local applications that keep memory on disk.</p>
      <pre><code class="language-python">from kronroe import AgentMemory

memory = AgentMemory.open("./my-agent.kronroe")</code></pre>
    </div>
    <div class="docs-tab-panel" role="tabpanel" id="python-agentmemory-memory-panel" aria-labelledby="python-agentmemory-memory-tab" hidden>
      <p class="docs-tab-note">Use this for short-lived sessions and tests.</p>
      <pre><code class="language-python">from kronroe import AgentMemory

memory = AgentMemory.open_in_memory()</code></pre>
    </div>
  </div>
</div>

### Assert facts

```python
# Basic assertion
memory.assert_fact("alice", "works_at", "Acme")
memory.assert_fact("alice", "lives_in", "London")

# With confidence score (0.0 to 1.0)
memory.assert_with_confidence("alice", "works_at", "Beta Corp", 0.95)

# With confidence and source provenance
memory.assert_with_confidence("alice", "salary", 120000, 0.9, source="hr:system")
```

### Recall facts

`recall` performs a full-text search and returns matching facts as dicts:

```python
results = memory.recall("where does Alice work?", limit=5)
for fact in results:
    print(f"{fact['subject']} {fact['predicate']} {fact['object']}")
```

### Scored recall

`recall_scored` returns each result with a signal breakdown so you can see why it ranked:

```python
scored = memory.recall_scored("Alice", limit=10)
for row in scored:
    fact = row["fact"]
    score = row["score"]
    print(f"{fact['subject']} {fact['predicate']} {fact['object']}")
    print(f"  type={score['type']} confidence={score['confidence']}")
```

The `score` dict includes `type` (`"text"` or `"hybrid"`), `confidence`, and `effective_confidence`. Text-only scores also include `rank` and `bm25_score`. Hybrid scores include `rrf_score`, `text_contrib`, and `vector_contrib`.

You can filter by minimum confidence:

```python
scored = memory.recall_scored("Alice", limit=10, min_confidence=0.8)
```

### Assemble LLM-ready context

`assemble_context` retrieves relevant facts and formats them as plain text within a token budget:

```python
context = memory.assemble_context("alice", max_tokens=200)
print(context)
```

### Correct a fact

Correcting a fact preserves the old value in history and creates a new fact with the updated value:

```python
fact_id = memory.assert_fact("alice", "works_at", "Acme")

# Later, correct it -- old value is never deleted
new_fact_id = memory.correct_fact(fact_id, "Beta Corp")
```

### Invalidate a fact

Invalidating a fact retires it by setting its `expired_at` timestamp. It no longer appears in current queries but remains in history:

```python
memory.invalidate_fact(fact_id)
```

### Query all facts about an entity

```python
facts = memory.facts_about("alice")
for fact in facts:
    print(f"  {fact['predicate']}: {fact['object']} (confidence={fact['confidence']})")
```

## KronroeDb vs AgentMemory

| | KronroeDb | AgentMemory |
|---|---|---|
| Level | Low-level engine access | High-level agent API |
| Search | `search(query, limit)` | `recall`, `recall_scored`, `assemble_context` |
| Scored results | No | Yes, with signal breakdown |
| Confidence | Not built-in | `assert_with_confidence`, confidence filtering |
| Source provenance | Not built-in | `assert_with_confidence(..., source="...")` |
| Corrections | Not exposed | `correct_fact(fact_id, new_value)` |
| Invalidation | Not exposed | `invalidate_fact(fact_id)` |
| Entity queries | Not exposed | `facts_about` |
| Context assembly | No | `assemble_context(query, max_tokens)` |

Use `KronroeDb` when you need direct engine control. Use `AgentMemory` for everything else.
