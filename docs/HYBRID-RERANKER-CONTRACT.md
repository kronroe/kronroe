# Hybrid Reranker Metadata Contract

Last updated: 2026-03-22
Stability: `Experimental`
Applies to: `kronroe` workspace `0.4.x`

This document defines the score metadata shapes returned by hybrid retrieval across
all Kronroe surfaces (Rust, MCP, Python, WASM). Callers can use these fields to
inspect per-result signal contributions — but final result ordering is determined
by the two-stage reranker, not by any single score field.

## Stability Classification

Hybrid retrieval is `Experimental`. These score shapes may change or gain new fields
without deprecation. Both `RecallScore` and its variants are `#[non_exhaustive]` —
additive fields may appear in any minor release without a breaking change.

See `docs/API-STABILITY-MATRIX.md` for full classification.

## Core Type: `HybridScoreBreakdown`

Returned by `TemporalGraph::search_hybrid()` in `crates/core`.

| Field | Type | Semantics |
|-------|------|-----------|
| `final_score` | `f64` | Pre-rerank RRF fusion score (sum of text + vector contributions). This is **not** the reranker's sort key. |
| `text_rrf_contrib` | `f64` | Text-channel contribution from weighted RRF. |
| `vector_rrf_contrib` | `f64` | Vector-channel contribution from weighted RRF. |
| `temporal_adjustment` | `f64` | Currently always `0.0`. Reserved for future use. |

Source: `crates/core/src/hybrid.rs:30–39`

## Agent-Layer Type: `RecallScore::Hybrid`

Returned by `AgentMemory::recall_scored()` and related methods in `crates/agent-memory`.

| Field | Type | Maps from | Semantics |
|-------|------|-----------|-----------|
| `rrf_score` | `f64` | `HybridScoreBreakdown::final_score` | Pre-rerank RRF fusion score. |
| `text_contrib` | `f64` | `HybridScoreBreakdown::text_rrf_contrib` | Text-channel contribution. |
| `vector_contrib` | `f64` | `HybridScoreBreakdown::vector_rrf_contrib` | Vector-channel contribution. |
| `confidence` | `f32` | `Fact::confidence` | Fact-level stored confidence \[0.0, 1.0\]. |
| `effective_confidence` | `Option<f32>` | Uncertainty model | `None` without `uncertainty` feature. When `Some`: `base_confidence × age_decay × source_weight`. |

Source: `crates/agent-memory/src/agent_memory.rs:68–88`

## Text-Only Comparison: `RecallScore::TextOnly`

Returned when hybrid is disabled or unavailable.

| Field | Type | Semantics |
|-------|------|-----------|
| `rank` | `usize` | Ordinal rank in the result set (0-indexed). |
| `bm25_score` | `f32` | Kronroe BM25 relevance score. Comparable within a single query, not across queries. |
| `confidence` | `f32` | Fact-level stored confidence \[0.0, 1.0\]. |
| `effective_confidence` | `Option<f32>` | Same semantics as `RecallScore::Hybrid`. |

Source: `crates/agent-memory/src/agent_memory.rs:91–102`

## Result Ordering Guarantee

The result list returned by `recall_scored()` is the authoritative ordering. The score
breakdown explains channel contributions but does **not** determine sort position.

The two-stage reranker applies:
- **Timeless queries:** adaptive vector-dominance — inspects top-5 signal balance and adjusts weights
- **Temporal queries:** semantic pruning to top-14 candidates, then temporal feasibility filtering with intent-weighted reranking

## Helper Methods

| Method | Returns | Semantics |
|--------|---------|-----------|
| `display_tag()` | `String` | `"0.032"` for hybrid, `"#1 bm25:4.21"` for text-only. Note: text-only tag is 1-indexed (`rank + 1`) even though `rank` field is 0-indexed. |
| `confidence()` | `f32` | Fact-level confidence regardless of retrieval path. |
| `effective_confidence()` | `Option<f32>` | Uncertainty-aware confidence. `None` when uncertainty is disabled. |

Source: `crates/agent-memory/src/agent_memory.rs:105–161`

## Wire Format (MCP JSON)

When returned via the MCP `recall_scored` tool:

```json
{
  "score": {
    "type": "hybrid",
    "rrf_score": 0.032,
    "text_contrib": 0.018,
    "vector_contrib": 0.014,
    "confidence": 0.95,
    "effective_confidence": 0.87
  }
}
```

For text-only results (all surfaces emit `"type": "text"`):

```json
{
  "score": {
    "type": "text",
    "rank": 0,
    "bm25_score": 4.21,
    "confidence": 0.95,
    "effective_confidence": null
  }
}
```
