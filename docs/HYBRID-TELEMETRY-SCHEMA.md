# Hybrid Retrieval Telemetry Schema

Last updated: 2026-03-22
Status: Schema definition — Kronroe does not emit telemetry. Callers are responsible for logging.

This document defines what agents and operators should log when using hybrid retrieval.
Field names align with the `RecallScore` shape defined in `docs/HYBRID-RERANKER-CONTRACT.md`.

## Per-Query Event Fields

Log one event per recall/recall_scored/recall_for_task invocation.

| Field | Type | Source | Description |
|-------|------|--------|-------------|
| `query_text` | `string` | Caller input | The search query text. |
| `query_intent` | `string` | `TemporalIntent` variant | One of: `Timeless`, `CurrentState`, `HistoricalPoint`, `HistoricalInterval`. |
| `temporal_operator` | `string` | `TemporalOperator` variant | One of: `Current`, `AsOf`, `Before`, `By`, `During`, `After`, `Unknown`. |
| `use_hybrid` | `bool` | Caller input | Whether hybrid retrieval was requested. |
| `score_type` | `string` | `RecallScore` variant | `"Hybrid"` or `"TextOnly"` — indicates which scoring path was used. |
| `result_count` | `u32` | Response | Number of facts returned. |
| `top_rrf_score` | `f64` | Top result | `rrf_score` of the top result. `null` if text-only. |
| `top_bm25_score` | `f32` | Top result | `bm25_score` of the top result. `null` if hybrid. |
| `top_confidence` | `f32` | Top result | `confidence` of the top result. |
| `top_effective_confidence` | `f32?` | Top result | `effective_confidence` of the top result. `null` without `uncertainty` feature. |
| `embedding_dimension` | `u32?` | Caller input | Dimension of query embedding. `null` if no embedding provided. |
| `correction_rate` | `f32` | Agent-measured | Fraction of results later corrected by the user. Measured over time, not per-query. |
| `fact_count` | `u64` | Database state | Total facts in the database at query time. |

## Per-Result Fields (Optional)

For detailed tracing, log one sub-record per returned fact.

| Field | Type | Source | Description |
|-------|------|--------|-------------|
| `fact_id` | `string` | Result | Kronroe Fact ID (`kf_...`). |
| `rrf_score` | `f64` | Hybrid result | RRF fusion score. `null` if text-only. |
| `bm25_score` | `f32` | TextOnly result | BM25 score. `null` if hybrid. |
| `text_contrib` | `f64` | Hybrid result | Text-channel contribution. |
| `vector_contrib` | `f64` | Hybrid result | Vector-channel contribution. |
| `confidence` | `f32` | Result | Fact-level stored confidence. |
| `effective_confidence` | `f32?` | Result | Uncertainty-aware confidence. |

## Sampling Guidance

- **During eval:** log 100% of queries with per-result fields.
- **In production:** sample at 10–25% of queries. Per-result fields are optional in production.
- **Correction rate** is measured over a sliding window (e.g. 7 days), not per query.

## Cross-References

- Score field definitions: `docs/HYBRID-RERANKER-CONTRACT.md`
- Temporal intent/operator variants: `crates/core/src/hybrid.rs:45–77`
