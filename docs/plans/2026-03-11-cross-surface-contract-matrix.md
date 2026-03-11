# Cross-Surface Contract Matrix (MCP/Python/WASM)

Date: 2026-03-11
Owner: Product hardening wave

## Goal

Lock shared behavior for `recall_scored` and embedding validation across the primary
agent-facing surfaces:

- MCP (`crates/mcp-server`)
- Python (`crates/python`)
- WASM (`crates/wasm`)

## Contract Clauses

1. `confidence_filter_mode` requires `min_confidence`.
2. `query_embedding` must be:
   - an array/list
   - non-empty
   - finite
   - representable as `f32` (no overflow to `inf`).
3. `recall_scored` response includes score metadata with confidence fields.
4. `effective` confidence mode errors when uncertainty support is unavailable.

## Coverage Matrix

| Clause | MCP | Python | WASM |
|---|---|---|---|
| 1. `confidence_filter_mode` requires threshold | `recall_scored_rejects_confidence_mode_without_threshold` | `python_recall_scored_requires_threshold_for_confidence_mode` | enforced in `recall_scored` input validation |
| 2a. reject empty embedding | `recall_rejects_empty_embedding_array` | `python_recall_rejects_empty_embedding` | enforced in `parse_embedding` |
| 2b. reject f32 overflow | `recall_rejects_embedding_values_outside_f32_range` | `python_recall_rejects_embedding_overflow` | enforced in `parse_embedding` |
| 3. scored response metadata shape | `recall_scored_returns_metadata` | `python_recall_scored_returns_contract_shape` | `wasm_graph_recall_scored_respects_min_confidence` (hybrid) |
| 4. effective mode feature gate | covered in recall option handling | covered in `recall_scored` handling | covered in `recall_scored` handling |

## Verification Commands

```bash
cargo test -p kronroe-mcp
cargo test -p kronroe-py
cargo check -p kronroe-py --no-default-features --features python-runtime-tests
cargo test -p kronroe-wasm
cargo check -p kronroe-wasm --features kronroe-wasm/hybrid,kronroe-wasm/uncertainty
cargo test -p kronroe-wasm --features kronroe-wasm/hybrid,kronroe-wasm/uncertainty
```

Note: Full `python-runtime-tests` execution can require local Python framework link
configuration; compile checks are used as the baseline gate in environments where
runtime linking is unavailable.
