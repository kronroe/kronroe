# Hybrid Retrieval Behavior Guide

Last updated: 2026-03-22
Stability: `Experimental` — see `docs/API-STABILITY-MATRIX.md`

## What Hybrid Retrieval Does

Hybrid retrieval combines two search channels:

1. **Lexical (BM25)** — keyword matching via the Kronroe full-text engine
2. **Vector (cosine similarity)** — semantic matching via pre-computed embeddings

These channels are fused using Reciprocal Rank Fusion (RRF), then passed through a
two-stage reranker:

- **Stage 1:** Semantic-dominant candidate pruning (top-20 for timeless queries, top-14 for temporal queries)
- **Stage 2:** Temporal feasibility filtering with intent-weighted reranking

Result: better recall for semantic queries (+17% vs text-only) and dramatically better
temporal queries (+47% time-slice lift). Eval-proven with product gate PASS on
2026-03-22.

## When to Use It

**Use hybrid when:**
- Caller has pre-computed embeddings for the query
- Temporal awareness matters (e.g. "where did Alice work in 2023?")
- Semantic similarity is more important than exact keyword matching

**Do not use when:**
- No embeddings are available — hybrid requires `query_embedding`
- Pure lexical matching is sufficient (exact names, IDs)
- The overhead of embedding computation is not justified

**Default is off** — `use_hybrid: false`. Must be explicitly enabled.

## Eval-Proven Defaults

These are the `HybridSearchParams::default()` values. The eval harness swept multiple
configurations and these won consistently.

| Parameter | Default | Meaning |
|-----------|---------|---------|
| `rank_constant` | `60` | RRF denominator offset. Higher = more conservative fusion. |
| `text_weight` | `0.8` | Relative weight of the lexical channel. |
| `vector_weight` | `0.2` | Relative weight of the vector channel. |
| `candidate_window` | `50` | Candidates pulled from each channel before fusion. |
| `k` | `10` | Number of results to return. |

Override these only if you have domain-specific eval data showing improvement.

Source: `crates/core/src/hybrid.rs:101–113`

## Temporal Intent and Operator

The caller classifies the query's temporal nature. This controls how the reranker
scores temporal feasibility.

### `TemporalIntent` — what kind of time question is this?

| Variant | Example Query | Reranker Behavior |
|---------|---------------|-------------------|
| `Timeless` (default) | "What languages does Alice know?" | No temporal constraint. Adaptive vector-dominance adjusts weights based on signal balance. |
| `CurrentState` | "Where does Alice work now?" | Signal: `validity × (0.5 + 0.5 × recency) × conf` where `validity` is `+1` if currently valid, `-1` if expired, and `recency` is exponential decay with 365-day half-life. Floor of `0.5 × conf` for infinitely old but currently-valid facts. |
| `HistoricalPoint` | "Where did Alice work in 2023?" | Filters to facts valid at the query time. Uses `TemporalOperator` to refine directional semantics. |
| `HistoricalInterval` | "What happened around Q3 2024?" | Checks overlap with a ±90 day window around the query time. `1.0 × conf` on overlap, `-1.0` on no overlap. `TemporalOperator` is silently ignored for this intent. |

Source: `crates/core/src/hybrid.rs:46–56`

### `TemporalOperator` — directional refinement for `HistoricalPoint`

| Variant | Meaning | Signal |
|---------|---------|--------|
| `Current` (default) | Fact must be currently valid | `0.9 × conf` if currently valid, `-0.8` otherwise |
| `AsOf` | Fact must have been valid at query time | `1.0 × conf` if valid at time, `-1.0` otherwise |
| `Before` | Fact must have started before query time | `1.1 × conf` if before and valid; `0.3 × conf` if before but no longer valid; `-1.0` otherwise |
| `By` | Fact must have started by (on or before) query time | `1.05 × conf` if by and valid; `0.2 × conf` if by but no longer valid; `-1.0` otherwise |
| `During` | Fact must overlap with query time | Same as `AsOf` |
| `After` | Fact must have started after query time | `1.0 × conf` if after, `-0.8` otherwise |
| `Unknown` | Falls back to `was_valid_at` check | `0.9 × conf` if valid at query time, `-0.8` otherwise |

Source: `crates/core/src/hybrid.rs:61–76`

## API Examples

### Rust (Core)

```rust
use kronroe::{
    HybridSearchParams, KronroeTimestamp, TemporalGraph,
    TemporalIntent, TemporalOperator,
};

let graph = TemporalGraph::open("memory.kronroe")?;
let embedding: Vec<f32> = compute_embedding("where does alice work?");

let params = HybridSearchParams {
    k: 5,
    intent: TemporalIntent::CurrentState,
    operator: TemporalOperator::Current,
    ..Default::default()
};

let results = graph.search_hybrid("where does alice work?", &embedding, params, None)?;
for (fact, breakdown) in &results {
    println!("{}: {} = {} (rrf: {:.3})",
        fact.subject, fact.predicate, fact.object, breakdown.final_score);
}
```

### Rust (Agent Memory)

```rust
use kronroe::TemporalIntent;
use kronroe_agent_memory::{AgentMemory, RecallOptions};

let memory = AgentMemory::open("memory.kronroe")?;
let embedding: Vec<f32> = compute_embedding("where does alice work?");

let opts = RecallOptions::new("where does alice work?")
    .with_embedding(&embedding)
    .with_hybrid(true)
    .with_temporal_intent(TemporalIntent::CurrentState)
    .with_limit(5);

let results = memory.recall_scored_with_options(&opts)?;
```

### MCP (JSON-RPC)

```json
{
  "method": "tools/call",
  "params": {
    "name": "recall_scored",
    "arguments": {
      "query": "where does alice work?",
      "query_embedding": [0.1, 0.2, 0.3],
      "use_hybrid": true,
      "temporal_intent": "current_state",
      "temporal_operator": "current",
      "limit": 5
    }
  }
}
```

### Python

```python
results = memory.recall_scored(
    "where does alice work?",
    limit=5,
    query_embedding=[0.1, 0.2, 0.3],
    use_hybrid=True,
    temporal_intent="current_state",
    temporal_operator="current",
)
```

## Score Interpretation

Hybrid results return `RecallScore::Hybrid` with channel-level signal breakdown.
Text-only results return `RecallScore::TextOnly` with BM25 scores.

See `docs/HYBRID-RERANKER-CONTRACT.md` for complete field definitions.

Key points:
- `rrf_score` is the **pre-rerank** fusion score — it explains channel contributions but
  does not determine final sort position
- The two-stage reranker may reorder results based on temporal feasibility
- `confidence` and `effective_confidence` are fact-level, not retrieval-level

## Stability and Rollback

Hybrid retrieval is `Experimental`. It may change without deprecation.

- Stability classification: `docs/API-STABILITY-MATRIX.md`
- How to disable: `docs/HYBRID-ROLLBACK-RUNBOOK.md`
- Score field contract: `docs/HYBRID-RERANKER-CONTRACT.md`
