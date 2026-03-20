# Storage Benchmark Baseline

Date: 2026-03-20  
Branch: `codex/storage-facade-phase1`  
Status: Baseline for current `redb` backend behind `KronroeStorage`

## Summary

This note captures the first storage benchmark baseline after:

- Phase 1 storage facade extraction
- Phase 2 storage observability groundwork

The benchmark harness is internal and currently lives in:

- [`/Users/rebekahcole/kronroe/.codex/worktrees/storage-facade-phase1/crates/core/src/storage_benchmarks.rs`](/Users/rebekahcole/kronroe/.codex/worktrees/storage-facade-phase1/crates/core/src/storage_benchmarks.rs)

It exercises real `TemporalGraph` operations and aggregates internal storage
observer events from `KronroeStorage`.

## How It Was Run

Command:

```bash
KRONROE_STORAGE_BENCHMARK_OUTPUT=/tmp/kronroe-storage-baseline.json \
CARGO_TARGET_DIR=/tmp/kronroe-storage-bench-baseline \
cargo test -p kronroe --all-features print_storage_benchmark_baseline_report -- --ignored --nocapture
```

Scale:

- `baseline`

Important caveat:

- these numbers are useful for shape and relative hotspot analysis
- they are not cross-machine performance claims
- the most important signal is operation mix, scan volume, and where time
  concentrates

## Baseline Results

### Assert-heavy ingestion

- workload wall time: `1132 ms`
- 5,000 assertions across 64 subjects
- dominant storage event: `WriteFact`
- total `WriteFact` duration: `58 ms`

Interpretation:

- straightforward append-style fact writes are relatively cheap
- this path does not currently look like the main storage bottleneck

### Correction-heavy timeline churn

- workload wall time: `18121 ms`
- 1,000 corrections on one `(subject, predicate)`
- dominant storage event: `ScanFacts`
- `ScanFacts` count: `2002`
- total rows scanned: `1,003,002`
- total `ScanFacts` duration: `16507 ms`

Interpretation:

- correction-heavy workloads are dominated by repeated full-table prefix scan
  behavior, not write cost
- this is a strong signal that the current persisted layout is not yet
  Kronroe-shaped for long correction chains

### Current-state scan

- workload wall time: `1522 ms`
- 12 predicates x 128 facts each for one hot subject
- 256 current-state queries
- dominant storage event: `ScanFacts`
- total rows scanned: `393,216`
- total `ScanFacts` duration: `1025 ms`

Interpretation:

- wide subject scans are already scan-cost sensitive
- current-state reads are serviceable, but clearly tied to raw iteration volume

### Historical point-in-time scan

- workload wall time: `22761 ms`
- 1,000 historical versions
- 256 point-in-time queries
- dominant storage event: `ScanFacts`
- `ScanFacts` count: `2254`
- total rows scanned: `1,255,000`
- total `ScanFacts` duration: `21003 ms`

Interpretation:

- historical lookup is the clearest current hotspot
- this strongly supports the research-plan hypothesis that historical workloads
  are where a Kronroe-native backend can materially outperform generic KV
  prefix scans

### Idempotent retries

- workload wall time: `95 ms`
- 256 unique keys
- 12 duplicate rounds per key
- dominant storage event: `GetIdempotency`
- 3,328 idempotent calls total

Interpretation:

- idempotent retry behavior is currently cheap and stable
- this path should be preserved as a non-regression requirement in any future
  backend

### Mixed real-task session

- workload wall time: `1 ms`
- session pattern: assert -> correct -> invalidate -> current -> historical

Interpretation:

- the small mixed-session path is not a useful performance bottleneck indicator
- it remains useful as a correctness-shaped workload and smoke scenario

### Embedding persistence and reopen

- workload wall time after reopen stage: `26 ms`
- embedding writes: `512`
- write stage wall time: `2622 ms`
- dominant storage event: `WriteFactWithEmbedding`
- total `WriteFactWithEmbedding` duration: `2264 ms`
- vector reopen query returned `8` results

Interpretation:

- vector persistence is materially more expensive than plain fact writes, as
  expected
- reopen and vector rebuild look acceptable at this scale
- this is a meaningful baseline to keep as we change storage internals later

## Initial Conclusions

The first baseline supports three practical conclusions:

1. The biggest current storage hotspot is scan-heavy history access, not plain
   writes.
2. Correction chains and historical reads are where the current layout pays the
   heaviest cost.
3. Idempotent retries are already cheap enough that a future Kronroe backend
   should treat them as a protected fast path, not as a target for redesign.

## What This Means For The Replacement Plan

This baseline strengthens the case for a Kronroe-native backend that eventually
provides:

- history-shaped indexing instead of repeated broad scans
- better correction-chain handling
- current-state and historical lookup paths that do not depend on scanning so
  many persisted fact rows

It does **not** currently show a strong need to redesign:

- plain fact insertion
- idempotency handling semantics
- vector reopen behavior at moderate scale

## Recommended Next Step

Move into the next Phase 2 slice:

1. expand the benchmark harness to capture file-size growth and reopen cost more
   explicitly
2. add a small benchmark comparison mode for alternative storage layouts once a
   prototype backend exists
3. start isolating `redb` error conversions behind `KronroeStorage` so backend
   replacement becomes structurally cleaner before Phase 3
