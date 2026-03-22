# AgentMemory API Reference

`AgentMemory` is the high-level API in the `kronroe-agent-memory` crate, designed as a drop-in alternative to Graphiti, mem0, or MemGPT -- without a server, without Neo4j, without Python.

It wraps `TemporalGraph` with an API shaped for AI agent use cases: episodic memory, scored recall, context assembly, and operational health reporting.

```rust
use kronroe_agent_memory::AgentMemory;

let memory = AgentMemory::open("./my-agent.kronroe")?;
memory.assert("alice", "works_at", "Acme")?;
let facts = memory.facts_about("alice")?;
```

## Error Handling

All methods return `kronroe_agent_memory::Result<T>`, which re-exports `kronroe::KronroeError` as the error type.

## Lifecycle

| Method | Signature | Description |
|---|---|---|
| `open` | `fn open(path: &str) -> Result<Self>` | Open or create an agent memory store. Auto-registers default singletons (feature: `contradiction`) and default volatilities (feature: `uncertainty`). |
| `open_in_memory` | `fn open_in_memory() -> Result<Self>` | Create an in-memory store. Same auto-registration behavior as `open`. |

When `contradiction` is enabled, `open` auto-registers these predicates as `Singleton` with `Warn` policy: `works_at`, `lives_in`, `job_title`, `email`, `phone`.

When `uncertainty` is enabled, `open` auto-registers default volatilities: `works_at` (730d), `job_title` (730d), `lives_in` (1095d), `email` (1460d), `phone` (1095d), `born_in` (stable), `full_name` (stable).

## Structured Assertion

| Method | Signature | Feature | Description |
|---|---|---|---|
| `assert` | `fn assert(&self, subject: &str, predicate: &str, object: impl Into<Value>) -> Result<FactId>` | base | Store a fact with `valid_from = now()` and default confidence `1.0`. |
| `assert_with_params` | `fn assert_with_params(&self, subject: &str, predicate: &str, object: impl Into<Value>, params: AssertParams) -> Result<FactId>` | base | Store a fact with explicit `valid_from`. |
| `assert_idempotent` | `fn assert_idempotent(&self, idempotency_key: &str, subject: &str, predicate: &str, object: impl Into<Value>) -> Result<FactId>` | base | Deduplicated assertion. Reusing the same key returns the original `FactId`. |
| `assert_idempotent_with_params` | `fn assert_idempotent_with_params(&self, idempotency_key: &str, subject: &str, predicate: &str, object: impl Into<Value>, params: AssertParams) -> Result<FactId>` | base | Idempotent assertion with explicit timing. |
| `assert_with_confidence` | `fn assert_with_confidence(&self, subject: &str, predicate: &str, object: impl Into<Value>, confidence: f32) -> Result<FactId>` | base | Assert with explicit confidence (clamped to [0.0, 1.0]). |
| `assert_with_confidence_with_params` | `fn assert_with_confidence_with_params(&self, subject: &str, predicate: &str, object: impl Into<Value>, params: AssertParams, confidence: f32) -> Result<FactId>` | base | Confidence assertion with explicit timing. |
| `assert_with_source` | `fn assert_with_source(&self, subject: &str, predicate: &str, object: impl Into<Value>, confidence: f32, source: &str) -> Result<FactId>` | base | Assert with confidence and source provenance marker. |
| `assert_with_source_with_params` | `fn assert_with_source_with_params(&self, subject: &str, predicate: &str, object: impl Into<Value>, params: AssertParams, confidence: f32, source: &str) -> Result<FactId>` | base | Source assertion with explicit timing. |

### AssertParams

Controls explicit temporal positioning for `_with_params` method variants.

```rust
pub struct AssertParams {
    pub valid_from: DateTime<Utc>,
}
```

## Episodic Memory

| Method | Signature | Feature | Description |
|---|---|---|---|
| `remember` | `fn remember(&self, text: &str, episode_id: &str, embedding: Option<Vec<f32>>) -> Result<FactId>` | base (embedding requires `hybrid`) | Store unstructured text as a fact. Subject = `episode_id`, predicate = `"memory"`, object = `text`. When `hybrid` is enabled and an embedding is provided, the fact is stored with its embedding for hybrid recall. |
| `remember_idempotent` | `fn remember_idempotent(&self, idempotency_key: &str, text: &str, episode_id: &str) -> Result<FactId>` | base | Idempotent version of `remember`. |

## Querying Facts

| Method | Signature | Feature | Description |
|---|---|---|---|
| `facts_about` | `fn facts_about(&self, entity: &str) -> Result<Vec<Fact>>` | base | Get all currently known facts about an entity across all predicates. |
| `facts_about_at` | `fn facts_about_at(&self, entity: &str, predicate: &str, at: DateTime<Utc>) -> Result<Vec<Fact>>` | base | Point-in-time query for a specific entity and predicate. |
| `current_facts` | `fn current_facts(&self, entity: &str, predicate: &str) -> Result<Vec<Fact>>` | base | Get currently valid facts for one (entity, predicate) pair. |
| `search` | `fn search(&self, query: &str, limit: usize) -> Result<Vec<Fact>>` | base | Full-text search across all facts. |

## Recall

### Basic Recall

| Method | Signature | Feature | Description |
|---|---|---|---|
| `recall` | `fn recall(&self, query: &str, query_embedding: Option<&[f32]>, limit: usize) -> Result<Vec<Fact>>` | base | Retrieve matching facts. Strips score breakdowns. When `hybrid` is enabled and an embedding is provided, uses hybrid retrieval. |
| `recall_scored` | `fn recall_scored(&self, query: &str, query_embedding: Option<&[f32]>, limit: usize) -> Result<Vec<(Fact, RecallScore)>>` | base | Retrieve facts with per-channel signal breakdowns. |

### Confidence-Filtered Recall

| Method | Signature | Feature | Description |
|---|---|---|---|
| `recall_with_min_confidence` | `fn recall_with_min_confidence(&self, query: &str, query_embedding: Option<&[f32]>, limit: usize, min_confidence: f32) -> Result<Vec<Fact>>` | base | Recall with base confidence threshold. |
| `recall_scored_with_min_confidence` | `fn recall_scored_with_min_confidence(&self, query: &str, query_embedding: Option<&[f32]>, limit: usize, min_confidence: f32) -> Result<Vec<(Fact, RecallScore)>>` | base | Scored recall with base confidence threshold. |
| `recall_scored_with_min_effective_confidence` | `fn recall_scored_with_min_effective_confidence(&self, query: &str, query_embedding: Option<&[f32]>, limit: usize, min_effective_confidence: f32) -> Result<Vec<(Fact, RecallScore)>>` | `uncertainty` | Scored recall filtered by effective confidence (age-decay-aware). |

### Options-Based Recall

| Method | Signature | Feature | Description |
|---|---|---|---|
| `recall_with_options` | `fn recall_with_options(&self, opts: &RecallOptions<'_>) -> Result<Vec<Fact>>` | base | Recall using `RecallOptions`. Strips score breakdowns. |
| `recall_scored_with_options` | `fn recall_scored_with_options(&self, opts: &RecallOptions<'_>) -> Result<Vec<(Fact, RecallScore)>>` | base | The primary options-based recall method. Supports confidence filtering, hybrid mode, temporal intent, and batch size control. |

## RecallOptions

`RecallOptions` is a `#[non_exhaustive]` builder struct for controlling recall behavior. Create with `RecallOptions::new(query)` and chain builder methods.

```rust
use kronroe_agent_memory::RecallOptions;

let opts = RecallOptions::new("what does alice do?")
    .with_limit(5)
    .with_min_confidence(0.6)
    .with_max_scored_rows(2_048);
```

| Field | Type | Default | Description |
|---|---|---|---|
| `query` | `&str` | (required) | Search query text |
| `query_embedding` | `Option<&[f32]>` | `None` | Embedding for hybrid retrieval |
| `limit` | `usize` | `10` | Maximum results to return |
| `min_confidence` | `Option<f32>` | `None` | Minimum confidence threshold |
| `confidence_filter_mode` | `ConfidenceFilterMode` | `Base` | Which confidence signal to filter on |
| `max_scored_rows` | `usize` | `4,096` | Maximum rows fetched per confidence-filtered batch |
| `use_hybrid` | `bool` | `false` | Enable hybrid retrieval (feature: `hybrid`) |
| `temporal_intent` | `TemporalIntent` | `Timeless` | Temporal intent for hybrid reranking (feature: `hybrid`) |
| `temporal_operator` | `TemporalOperator` | `Current` | Temporal operator hint (feature: `hybrid`) |

### Builder Methods

| Method | Feature | Description |
|---|---|---|
| `with_embedding(embedding)` | base | Set the query embedding |
| `with_limit(limit)` | base | Set max results |
| `with_min_confidence(min)` | base | Set base confidence threshold |
| `with_min_effective_confidence(min)` | `uncertainty` | Set effective confidence threshold |
| `with_max_scored_rows(max)` | base | Set batch size for confidence filtering |
| `with_hybrid(enabled)` | `hybrid` | Enable/disable hybrid retrieval |
| `with_temporal_intent(intent)` | `hybrid` | Set temporal intent |
| `with_temporal_operator(operator)` | `hybrid` | Set temporal operator |

## RecallScore

`RecallScore` is a `#[non_exhaustive]` enum with two variants, indicating which retrieval path produced each result.

### RecallScore::Hybrid

Returned when hybrid retrieval is used (text + vector channels fused via RRF).

| Field | Type | Description |
|---|---|---|
| `rrf_score` | `f64` | Pre-rerank RRF fusion score |
| `text_contrib` | `f64` | Text-channel RRF contribution |
| `vector_contrib` | `f64` | Vector-channel RRF contribution |
| `confidence` | `f32` | Fact-level confidence [0.0, 1.0] |
| `effective_confidence` | `Option<f32>` | Uncertainty-aware confidence. `None` when `uncertainty` feature is disabled. |

### RecallScore::TextOnly

Returned when fulltext search is used without hybrid mode.

| Field | Type | Description |
|---|---|---|
| `rank` | `usize` | Ordinal rank in the result set (0-indexed) |
| `bm25_score` | `f32` | BM25 relevance score. Higher = stronger lexical match. Comparable within a query, not across queries. |
| `confidence` | `f32` | Fact-level confidence [0.0, 1.0] |
| `effective_confidence` | `Option<f32>` | Uncertainty-aware confidence. `None` when `uncertainty` feature is disabled. |

### Common Methods

| Method | Return | Description |
|---|---|---|
| `confidence()` | `f32` | Fact-level confidence, regardless of variant |
| `effective_confidence()` | `Option<f32>` | Effective confidence, regardless of variant |
| `display_tag()` | `String` | Human-readable score tag (e.g. `"0.032"` for hybrid, `"#1 bm25:4.21"` for text-only) |

## ConfidenceFilterMode

Controls which confidence signal is used when `min_confidence` is set on `RecallOptions`.

| Variant | Feature | Description |
|---|---|---|
| `Base` | base | Filter using the raw fact confidence |
| `Effective` | `uncertainty` | Filter using effective confidence (base * age_decay * source_weight) |

## Context Assembly

| Method | Signature | Feature | Description |
|---|---|---|---|
| `assemble_context` | `fn assemble_context(&self, query: &str, query_embedding: Option<&[f32]>, max_tokens: usize) -> Result<String>` | base | Build a token-bounded LLM prompt context from recalled facts. Uses scored recall internally. Output format: `[date] (score) subject . predicate . value`. |

## Modification

| Method | Signature | Feature | Description |
|---|---|---|---|
| `correct_fact` | `fn correct_fact(&self, fact_id: impl AsRef<str>, new_value: impl Into<Value>) -> Result<FactId>` | base | Correct a fact, preserving temporal history. Uses `now()` as the correction timestamp. |
| `invalidate_fact` | `fn invalidate_fact(&self, fact_id: impl AsRef<str>) -> Result<()>` | base | Invalidate a fact at the current time. |

## Operational Reports

| Method | Signature | Feature | Description |
|---|---|---|---|
| `what_changed` | `fn what_changed(&self, entity: &str, since: DateTime<Utc>, predicate_filter: Option<&str>) -> Result<WhatChangedReport>` | base | Summary of changes since a timestamp: new facts, invalidations, corrections, and confidence shifts. |
| `memory_health` | `fn memory_health(&self, entity: &str, predicate_filter: Option<&str>, low_confidence_threshold: f32, stale_after_days: i64) -> Result<MemoryHealthReport>` | base | Health snapshot: low-confidence facts, stale high-impact facts, contradiction counts, recommended actions. |
| `recall_for_task` | `fn recall_for_task(&self, task: &str, subject: Option<&str>, now: Option<DateTime<Utc>>, horizon_days: Option<i64>, limit: usize, query_embedding: Option<&[f32]>) -> Result<RecallForTaskReport>` | base (embedding requires `hybrid`) | Decision-ready recall context scoped to a task. Includes key facts, watchouts, and recommended next checks. |

## Contradiction Detection

All methods require the `contradiction` feature flag.

| Method | Signature | Description |
|---|---|---|
| `assert_checked` | `fn assert_checked(&self, subject: &str, predicate: &str, object: impl Into<Value>) -> Result<(FactId, Vec<Contradiction>)>` | Assert with contradiction checking. Behavior depends on the predicate's `ConflictPolicy`. |
| `audit` | `fn audit(&self, subject: &str) -> Result<Vec<Contradiction>>` | Scan a subject for contradictions across all registered singletons. Cost scales with the subject's fact count. |

## Uncertainty Model

All methods require the `uncertainty` feature flag.

| Method | Signature | Description |
|---|---|---|
| `register_volatility` | `fn register_volatility(&self, predicate: &str, half_life_days: f64) -> Result<()>` | Register a predicate half-life. After `half_life_days`, age decay = 0.5. Use `f64::INFINITY` for stable predicates. |
| `register_source_weight` | `fn register_source_weight(&self, source: &str, weight: f32) -> Result<()>` | Register a source authority weight. Clamped to [0.0, 2.0]. `1.0` = neutral. |
| `effective_confidence_for_fact` | `fn effective_confidence_for_fact(&self, fact: &Fact, at: DateTime<Utc>) -> Result<Option<f32>>` | Compute effective confidence at a point in time. Returns `None` when the `uncertainty` feature is not enabled. |

## Feature Flag Summary

| Feature | What It Enables |
|---|---|
| (base) | Core CRUD, recall, context assembly, reports |
| `contradiction` | `assert_checked`, `audit`, auto-registered singletons |
| `uncertainty` | Volatility registration, source weights, effective confidence, `ConfidenceFilterMode::Effective` |
| `hybrid` | Hybrid recall (RRF + temporal reranking), `RecallOptions` hybrid controls, embedding pass-through in `remember` |
