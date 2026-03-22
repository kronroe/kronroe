# Release Notes: 0.4.0 (Draft)

Date: 2026-03-22
Status: Draft — workspace version not yet bumped from `0.3.0`

## Headline Changes

1. **`chrono` removed** — replaced with `KronroeTimestamp`, a Kronroe-owned UTC-only time system
2. **Hybrid retrieval eval pass** — product gate PASS in two consecutive runs

## chrono to KronroeTimestamp Migration

All timestamp types now use `KronroeTimestamp` — a Kronroe-native type with microsecond
precision, UTC-only semantics, and zero external time dependencies.

### What Changed

| Before (0.3.x) | After (0.4.0) |
|-----------------|---------------|
| `chrono::DateTime<chrono::Utc>` | `kronroe::KronroeTimestamp` |
| `chrono::Duration` | `kronroe::KronroeSpan` |
| `chrono::Utc::now()` | `KronroeTimestamp::now_utc()` |
| `"...".parse::<DateTime<Utc>>()` | `"...".parse::<KronroeTimestamp>()` |
| `.to_rfc3339()` | `.to_rfc3339()` (same method name) |

### Migration

`KronroeTimestamp` implements `FromStr`, so parsing works the same way:

```rust
let ts: KronroeTimestamp = "2024-01-01T00:00:00Z".parse().unwrap();
let span = KronroeSpan::days(30);
let later = ts + span;
```

All RFC3339 timestamps are accepted on input (including non-UTC offsets, which are
normalized to UTC). Output always uses canonical `Z` suffix — never `+00:00`.

### Why

- Zero external dependency for time (was 114 lines in `Cargo.lock`)
- Deterministic behavior on all platforms (native, WASM, iOS, Android)
- Smaller binary size
- Kronroe controls its own temporal semantics

### Breaking Changes

- All public Rust APIs that accepted `DateTime<Utc>` now accept `KronroeTimestamp`
- All public Rust APIs that accepted `chrono::Duration` now accept `KronroeSpan`
- Wire format (MCP JSON, Python, WASM) is unchanged — RFC3339 strings in, RFC3339 strings out
- Output timestamps may normalize more strictly (always `Z`, trimmed fractional zeros)

## Hybrid Retrieval Eval Pass

Two consecutive eval runs on 2026-03-22, both product gate PASS:

| Metric | Result |
|--------|--------|
| nDCG@3 | 0.8249 |
| Recall@5 | 0.9342 |
| MRR@5 | 0.8807 |
| Semantic lift vs text-only | +17.03% |
| Time-slice lift vs strongest baseline | +47.18% |
| p95 latency | 0.03ms (no regression) |
| Win/tie/loss vs text | 30/45/6 |

**Stability: remains `Experimental`.** Gate A3 (agent orchestration quality pass) is
still open. Hybrid will not be promoted to Preview until this gate closes.

### New Hybrid Documentation

- `docs/HYBRID-RERANKER-CONTRACT.md` — score metadata field contract
- `docs/HYBRID-ROLLBACK-RUNBOOK.md` — how to disable hybrid at runtime or compile-time
- `docs/HYBRID-TELEMETRY-SCHEMA.md` — what to log for hybrid queries
- `docs/HYBRID-BEHAVIOR-GUIDE.md` — when/how to use hybrid retrieval

## Other Changes

- Codex worktrees cleaned up (12 stale worktrees + 22 local branches removed)
- Contract fixture test infrastructure removed from MCP, Python, WASM crates
  (`serde_json` dev-dep removed from Python crate)
- `serde_json` still available as a workspace dependency for crates that need it

## Compatibility

- Stable APIs remain backward compatible in semantic behavior
- Wire format (MCP JSON) is unchanged
- Python and WASM method signatures are unchanged
- iOS and Android build within size budgets (XCFramework: 1.18 MB)
