# Hybrid Retrieval Rollback Runbook

Last updated: 2026-03-22

How to disable hybrid retrieval at runtime or compile-time across all Kronroe surfaces.

## Runtime Disablement (No Rebuild)

Hybrid is **off by default** — callers must explicitly enable it. To disable, stop
passing the enabling parameters.

| Surface | How to Disable |
|---------|----------------|
| MCP | Omit `query_embedding`, or pass `"use_hybrid": false` in `recall` / `recall_scored` / `recall_for_task` arguments. |
| Python | Omit `query_embedding`, or pass `use_hybrid=False` in `recall_scored()`. Default is `False`. |
| WASM | Omit `query_embedding`, or pass `use_hybrid: false` in `recall_scored()` options. |
| Rust (`RecallOptions`) | Omit `.with_hybrid(true)`. Default is `false`. |

After disabling, all recall results return `RecallScore::TextOnly` (BM25 scoring).

## Compile-Time Disablement (Rebuild Required)

Build without the `hybrid` feature flag. The feature chain:

```
kronroe-mcp --features hybrid
  └─ kronroe-agent-memory/hybrid
       └─ kronroe/vector  (flat cosine index)
       └─ kronroe/hybrid-experimental  (two-stage reranker)
```

Build commands without hybrid:

```bash
# MCP server (default features do NOT include hybrid)
cargo build -p kronroe-mcp

# Python bindings
maturin develop -m crates/python/Cargo.toml

# All crates without hybrid
cargo build --workspace
```

Note: CI runs `--all-features` which includes hybrid. A hybrid-disabled build is
not the CI default — test locally if needed.

## Fallback Behavior

When hybrid is absent (feature not compiled in):

- `recall_scored()` returns only `RecallScore::TextOnly` variants (BM25 scoring).
- Text-only codepath: `#[cfg(not(feature = "hybrid"))]` in `crates/agent-memory/src/agent_memory.rs:1146–1174`.
- Temporal intent/operator parsers return `None` silently.
- Embeddings passed to API methods are rejected with explicit errors (see below).

## Error Messages by Surface

Callers will see these errors when hybrid features are requested but unavailable:

### MCP Server

| Trigger | Error Message |
|---------|---------------|
| `use_hybrid: true` without feature | `"hybrid is unavailable in this build"` |
| `temporal_intent` / `temporal_operator` without feature | `"temporal controls are unavailable without hybrid feature"` |
| `query_embedding` without feature | `"query_embedding is unavailable without hybrid feature"` |
| `use_hybrid: true` without `query_embedding` | `"use_hybrid requires query_embedding"` |
| `recall_for_task` with hybrid controls without feature | `"hybrid task recall controls are unavailable without hybrid feature"` |

### Python

| Trigger | Error Message |
|---------|---------------|
| `query_embedding` provided without feature | `"hybrid/temporal controls are unavailable without hybrid feature"` |
| `use_hybrid=True` without feature | `"use_hybrid requires the 'hybrid' feature"` |
| `query_embedding` without feature (in `recall` / `assemble_context`) | `"query_embedding is unavailable without hybrid feature"` |
| `use_hybrid=True` without `query_embedding` | `"query_embedding is required for hybrid/temporal controls"` |

### WASM

| Trigger | Error Message |
|---------|---------------|
| `query_embedding` without feature | `` "query_embedding is unavailable without the `hybrid` feature" `` |
| Hybrid controls without feature | `` "hybrid controls are unavailable without the `hybrid` feature" `` |

## Verifying Rollback

After disabling hybrid, confirm:

1. `recall_scored` results contain only `"type": "text_only"` (MCP JSON) or `RecallScore::TextOnly` (Rust).
2. No `"type": "hybrid"` appears in any response.
3. Passing `use_hybrid: true` returns an explicit error (not silent fallback).

## Cross-References

- Score shapes after rollback: `docs/HYBRID-RERANKER-CONTRACT.md`
- Feature flag stability: `docs/API-STABILITY-MATRIX.md`
