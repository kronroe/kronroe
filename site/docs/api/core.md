# TemporalGraph API Reference

`TemporalGraph` is the core engine type in the `kronroe` crate. It provides an embedded, serverless temporal property graph database where bi-temporal facts are the fundamental primitive.

All writes are ACID-backed. The database file uses the `.kronroe` extension by convention.

```rust
use kronroe::TemporalGraph;
use kronroe::KronroeTimestamp;

let db = TemporalGraph::open("my-graph.kronroe")?;
db.assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())?;
```

## Error Handling

All fallible methods return `kronroe::Result<T>`, which is `std::result::Result<T, KronroeError>`.

```rust
pub enum KronroeError {
    Storage(String),
    Serialization(serde_json::Error),
    NotFound(String),
    Search(String),
    InvalidFactId(String),
    InvalidEmbedding(String),
    Internal(String),
    ContradictionRejected(Vec<Contradiction>),  // feature: contradiction
    SchemaMismatch { found: u64, expected: u64 },
}
```

## Lifecycle

| Method | Signature | Description |
|---|---|---|
| `open` | `fn open(path: &str) -> Result<Self>` | Open or create a database at the given path. Creates the file if it does not exist. |
| `open_in_memory` | `fn open_in_memory() -> Result<Self>` | Create an in-memory database with no file I/O. Data is lost on drop. Useful for WASM, testing, and ephemeral workloads. |

## CRUD Operations

### Writing Facts

| Method | Signature | Feature | Description |
|---|---|---|---|
| `assert_fact` | `fn assert_fact(&self, subject: &str, predicate: &str, object: impl Into<Value>, valid_from: KronroeTimestamp) -> Result<FactId>` | base | Assert a new fact. Confidence defaults to `1.0`. |
| `assert_fact_with_confidence` | `fn assert_fact_with_confidence(&self, subject: &str, predicate: &str, object: impl Into<Value>, valid_from: KronroeTimestamp, confidence: f32) -> Result<FactId>` | base | Assert a fact with explicit confidence (clamped to [0.0, 1.0]). |
| `assert_fact_with_source` | `fn assert_fact_with_source(&self, subject: &str, predicate: &str, object: impl Into<Value>, valid_from: KronroeTimestamp, confidence: f32, source: &str) -> Result<FactId>` | base | Assert a fact with confidence and source provenance. |
| `assert_fact_idempotent` | `fn assert_fact_idempotent(&self, idempotency_key: &str, subject: &str, predicate: &str, object: impl Into<Value>, valid_from: KronroeTimestamp) -> Result<FactId>` | base | Assert with deduplication. If the key was already used, returns the original `FactId` without creating a new fact. |
| `assert_fact_with_embedding` | `fn assert_fact_with_embedding(&self, subject: &str, predicate: &str, object: impl Into<Value>, valid_from: KronroeTimestamp, embedding: Vec<f32>) -> Result<FactId>` | `vector` | Assert a fact and persist its embedding atomically. Kronroe does not generate embeddings -- the caller provides a pre-computed `Vec<f32>`. |

### Reading Facts

| Method | Signature | Feature | Description |
|---|---|---|---|
| `current_facts` | `fn current_facts(&self, subject: &str, predicate: &str) -> Result<Vec<Fact>>` | base | Get all currently valid facts for a (subject, predicate) pair. A fact is currently valid when both `valid_to` and `expired_at` are `None`. |
| `facts_at` | `fn facts_at(&self, subject: &str, predicate: &str, at: KronroeTimestamp) -> Result<Vec<Fact>>` | base | Point-in-time query on the valid-time axis. Returns facts that were true at time `at`. |
| `all_facts_about` | `fn all_facts_about(&self, subject: &str) -> Result<Vec<Fact>>` | base | Get every fact ever recorded for an entity, across all predicates (including expired facts). |
| `fact_by_id` | `fn fact_by_id(&self, fact_id: impl AsRef<str>) -> Result<Fact>` | base | Retrieve a specific fact by its `FactId`. Returns `NotFound` if the ID does not exist. |

### Modifying Facts

| Method | Signature | Feature | Description |
|---|---|---|---|
| `correct_fact` | `fn correct_fact(&self, fact_id: impl AsRef<str>, new_value: impl Into<Value>, at: KronroeTimestamp) -> Result<FactId>` | base | Correct a fact by invalidating the old one at time `at` and asserting a replacement with the same subject and predicate. Returns the new fact's ID. |
| `invalidate_fact` | `fn invalidate_fact(&self, fact_id: impl AsRef<str>, at: KronroeTimestamp) -> Result<()>` | base | Invalidate a fact by closing its valid-time and transaction-time windows. The fact remains for historical queries. |

## Search

| Method | Signature | Feature | Description |
|---|---|---|---|
| `search` | `fn search(&self, query: &str, limit: usize) -> Result<Vec<Fact>>` | `fulltext` (default) | Full-text search over entity names, predicates, and string values. Returns up to `limit` results ranked by BM25 relevance. |
| `search_scored` | `fn search_scored(&self, query: &str, limit: usize) -> Result<Vec<(Fact, f32)>>` | `fulltext` (default) | Full-text search returning `(Fact, bm25_score)` pairs. Scores are comparable within a single query but not across queries. |
| `search_by_vector` | `fn search_by_vector(&self, query: &[f32], k: usize, at: Option<KronroeTimestamp>) -> Result<Vec<(Fact, f32)>>` | `vector` | Cosine similarity search over facts with embeddings. Pass `at = None` for currently-valid facts only, or `at = Some(t)` for point-in-time filtering. Returns `(Fact, similarity_score)` pairs sorted by descending similarity. |
| `search_hybrid` | `fn search_hybrid(&self, text_query: &str, vector_query: &[f32], params: HybridSearchParams, at: Option<KronroeTimestamp>) -> Result<Vec<(Fact, HybridScoreBreakdown)>>` | `hybrid-experimental` + `vector` | RRF fusion of text and vector channels, followed by a two-stage intent-gated temporal reranker. Callers provide `TemporalIntent` and `TemporalOperator` via `HybridSearchParams`. |

### HybridSearchParams

Used with `search_hybrid`. Has eval-proven defaults.

| Field | Type | Default | Description |
|---|---|---|---|
| `k` | `usize` | -- | Number of results to return |
| `candidate_window` | `usize` | varies | Number of candidates for RRF fusion |
| `rank_constant` | `u32` | `60` | RRF rank constant |
| `text_weight` | `f32` | `0.8` | Text channel weight |
| `vector_weight` | `f32` | `0.2` | Vector channel weight |
| `intent` | `TemporalIntent` | `Timeless` | Caller's temporal intent |
| `operator` | `TemporalOperator` | `Current` | Temporal operator hint |

### TemporalIntent

| Variant | Description |
|---|---|
| `Timeless` | No temporal scoring. Uses adaptive vector-dominance weighting. |
| `CurrentState` | Preference for the most recent facts. |
| `HistoricalPoint` | Query anchored to a specific point in time. |
| `HistoricalInterval` | Query spanning a time range. |

### TemporalOperator

| Variant | Description |
|---|---|
| `Current` | Currently active facts |
| `AsOf` | Facts valid as of a timestamp |
| `Before` | Facts valid before a timestamp |
| `By` | Facts valid by a deadline |
| `During` | Facts valid during an interval |
| `After` | Facts valid after a timestamp |
| `Unknown` | Operator could not be determined |

## Contradiction Detection

All methods in this section require the `contradiction` feature flag.

| Method | Signature | Description |
|---|---|---|
| `register_singleton_predicate` | `fn register_singleton_predicate(&self, predicate: &str, policy: ConflictPolicy) -> Result<()>` | Register a predicate as singleton (at most one active value per subject at any time). Persisted to the database. |
| `is_singleton_predicate` | `fn is_singleton_predicate(&self, predicate: &str) -> Result<bool>` | Check whether a predicate is registered as singleton. |
| `singleton_predicates` | `fn singleton_predicates(&self) -> Result<Vec<String>>` | List all registered singleton predicates. |
| `detect_contradictions` | `fn detect_contradictions(&self, subject: &str, predicate: &str) -> Result<Vec<Contradiction>>` | Detect pairwise contradictions for a (subject, predicate) pair. Only checks registered singletons. |
| `detect_all_contradictions` | `fn detect_all_contradictions(&self) -> Result<Vec<Contradiction>>` | Full scan for contradictions across all registered singleton predicates. |
| `assert_fact_checked` | `fn assert_fact_checked(&self, subject: &str, predicate: &str, object: impl Into<Value>, valid_from: KronroeTimestamp) -> Result<(FactId, Vec<Contradiction>)>` | Assert with contradiction checking. Behavior depends on `ConflictPolicy`: `Allow` stores silently, `Warn` stores and returns contradictions, `Reject` blocks storage if contradictions exist. |

### ConflictPolicy

| Variant | Behavior |
|---|---|
| `Allow` | Store the fact, return no contradictions |
| `Warn` | Store the fact, return detected contradictions |
| `Reject` | Block storage if contradictions exist, return `Err(ContradictionRejected(...))` |

### PredicateCardinality

| Variant | Meaning |
|---|---|
| `Singleton` | At most one active value per subject at any point in valid time |
| `MultiValued` | Multiple concurrent values allowed (default for unregistered predicates) |

## Uncertainty Model

All methods in this section require the `uncertainty` feature flag.

| Method | Signature | Description |
|---|---|---|
| `register_predicate_volatility` | `fn register_predicate_volatility(&self, predicate: &str, volatility: PredicateVolatility) -> Result<()>` | Set a half-life in days for a predicate. After the half-life, age decay drops to 0.5. Use `f64::INFINITY` for stable predicates. Persisted to the database. |
| `predicate_volatility` | `fn predicate_volatility(&self, predicate: &str) -> Result<Option<PredicateVolatility>>` | Query the current volatility registration for a predicate. |
| `register_source_weight` | `fn register_source_weight(&self, source: &str, weight: SourceWeight) -> Result<()>` | Set an authority multiplier for a source. Clamped to [0.0, 2.0]. `1.0` = neutral. Persisted to the database. |
| `source_weight` | `fn source_weight(&self, source: &str) -> Result<Option<SourceWeight>>` | Query the current source weight registration. |
| `effective_confidence` | `fn effective_confidence(&self, fact: &Fact, at: KronroeTimestamp) -> Result<EffectiveConfidence>` | Compute effective confidence at query time. Never stored back. |

### EffectiveConfidence

Returned by `effective_confidence`. Contains the computed value and its component breakdown.

| Field | Type | Description |
|---|---|---|
| `value` | `f32` | Final effective confidence, clamped to [0.0, 1.0] |
| `base_confidence` | `f32` | The fact's stored confidence |
| `age_decay` | `f64` | Decay multiplier based on predicate volatility and fact age |
| `source_weight` | `f64` | Authority multiplier from source registration |

**Formula:** `effective = base_confidence * age_decay * source_weight` (clamped to [0.0, 1.0])

Age is measured from `valid_from` (real-world time), not `recorded_at` (database time). The decay function is exponential half-life: `exp(-ln(2) * age_days / half_life_days)`.
