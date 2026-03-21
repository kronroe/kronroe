# Storage Backend Comparison Baseline

Date: 2026-03-21  
Branch: `codex/storage-phase3-baseline`  
Status: First side-by-side baseline for `Redb` vs experimental `AppendLog`

## Summary

This note captures the first real comparison run after:

- Phase 1 storage facade extraction
- Phase 2 observability groundwork
- Phase 3 prototype append-log backend landing in `#124`

The benchmark harness lives in:

- [`/Users/rebekahcole/kronroe/.codex/worktrees/storage-phase3-baseline/crates/core/src/storage_benchmarks.rs`](/Users/rebekahcole/kronroe/.codex/worktrees/storage-phase3-baseline/crates/core/src/storage_benchmarks.rs)

It now runs the same supported workloads against both storage engines and records:

- `storage_engine`
- `backend_mode`
- wall-clock duration in milliseconds and microseconds
- aggregated per-operation observer metrics

## How It Was Run

Command:

```bash
CARGO_TARGET_DIR=/tmp/kronroe-storage-phase3-baseline \
cargo test -p kronroe --all-features print_storage_benchmark_baseline_report -- --ignored --nocapture
```

Scale:

- `baseline`

Important caveats:

- the directly comparable workloads in this run are all `InMemory`
- `embedding_reopen` remains `Redb`-only because append-log embedding persistence is still intentionally unsupported
- append-log and `redb` currently scan the same number of logical rows on the history-heavy paths, so this run is telling us about engine overhead and access shape, not yet about a smarter historical index
- append-log is new enough that these numbers should be treated as internal engineering guidance, not product-facing performance claims

## Comparison Snapshot

| Workload | Redb | AppendLog | Direction |
|---------|------|-----------|-----------|
| `assert_heavy_ingestion` | `1213 ms` | `17 ms` | AppendLog much faster |
| `correction_heavy_timeline_churn` | `20169 ms` | `303 ms` | AppendLog much faster |
| `current_state_scan` | `1779 ms` | `40 ms` | AppendLog much faster |
| `historical_point_in_time_scan` | `26327 ms` | `382 ms` | AppendLog much faster |
| `idempotent_retries` | `113 ms` | `12 ms` | AppendLog faster |
| `mixed_real_task_session` | `1 ms` | `0 ms` | too small to matter |
| `embedding_reopen` | `28 ms` reopen / `2735 ms` write | N/A | Redb-only |

## What Matters In The Results

### 1. The biggest hotspot is still scan-heavy history work

The correction and historical workloads still examine the same broad row counts across both engines:

- correction churn: `1,003,002` rows scanned
- historical point-in-time: `1,255,000` rows scanned

That means the append-log prototype is **not** winning because it already has a better history index. It is winning despite scanning the same number of rows.

Interpretation:

- the current `redb` path is paying meaningful overhead for these workloads
- the append-log prototype confirms there is real headroom in Kronroe’s storage design even before we add a proper temporal index
- the next big gains are likely to come from reducing scan volume, not just lowering per-row overhead

### 2. Append-only fact and correction flows look very promising

The clearest signal is the correction-heavy workload:

- `Redb`: `20169 ms`
- `AppendLog`: `303 ms`

Interpretation:

- append-style mutation matches Kronroe’s temporal model well
- correction chains appear to be a strong fit for an append-log shaped backend
- this supports continuing with the append-log prototype rather than pivoting back to a more KV-like internal design

### 3. Current-state reads also improved sharply

Even the current-state hot-subject scan improved substantially:

- `Redb`: `1779 ms`
- `AppendLog`: `40 ms`

Interpretation:

- the prototype is not only helping historical workflows
- even before adding dedicated current-state indexes, the simpler backend path is materially lighter

### 4. Idempotency remains a protected fast path

The idempotent retry workload stayed cheap on both engines:

- `Redb`: `113 ms`
- `AppendLog`: `12 ms`

Interpretation:

- idempotency should remain a first-class fast path in the replacement design
- there is no reason to complicate that part of the system during backend replacement

## What This Means For Phase 3

The comparison strengthens the case for a fully Kronroe-owned backend and gives us a much clearer priority order:

1. Keep pursuing the append-log direction.
2. Do **not** optimize plain writes first; they are already in good shape.
3. Focus the next prototype slice on reducing scan volume for:
   - correction chains
   - historical point-in-time queries
   - wide current-state scans
4. Preserve idempotency as a simple, fast path throughout the redesign.

## First Indexed Rerun

After this first comparison note, the append-log prototype gained a narrow
derived index for exact `subject:predicate:` scans. A rerun of the same
baseline showed:

- `current_state_scan` rows scanned dropped from `393,216` to `32,768`
- `current_state_scan` wall time stayed very low at about `32 ms`
- `correction_heavy_timeline_churn` still scanned `1,003,002` rows
- `historical_point_in_time_scan` still scanned `1,255,000` rows

Interpretation:

- the exact subject/predicate candidate index is already useful
- it materially helps wide current-state access patterns
- it does **not** solve the real remaining hotspot: long version chains inside a
  single `(subject, predicate)` history

That means the next backend slice should not be another generic prefix index.
It should be a history-shaped derived structure for append-log, aimed at
reducing candidate volume inside a single temporal chain.

## Recommended Next Slice

The next prototype slice should add the first real history-aware derived index
on top of the append-log engine, rather than broadening feature coverage in
every direction.

Recommended target:

- a subject/predicate version-chain index for append-log, with enough structure
  to reduce candidate sets for:
  - correction chains
  - point-in-time lookups
  - current-state retrieval inside long histories

Why this next:

- it directly targets the largest remaining cost center: raw scan volume inside
  long temporal histories
- it will tell us whether the next order-of-magnitude gain comes from indexing, not just from switching storage engines
- it is more valuable right now than, for example, adding append-log vector persistence

## Follow-up Guardrails

- Keep benchmark output precision at microseconds as well as milliseconds so fast append-log operations do not collapse to misleading zero-duration summaries.
- Keep bridge verification discipline from earlier phases; avoid leaving dead backend paths around after each storage refactor slice.
- Treat `embedding_reopen` as a separate track until append-log has explicit vector persistence support.
