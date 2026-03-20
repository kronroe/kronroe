# Storage Backend Research Plan

Date: 2026-03-20  
Owner: Core + Product Infrastructure  
Status: Research and design plan

## Summary

Kronroe currently uses `redb` as the embedded storage engine behind
[`TemporalGraph`](/Users/rebekahcole/kronroe/crates/core/src/temporal_graph.rs).
This document captures:

- what `redb` gives Kronroe today
- what Kronroe would need from any replacement
- what "better for Kronroe" should mean in practice
- a benchmark and observability plan before any storage rewrite
- a staged implementation path for a future Kronroe-owned backend

Recommendation:

1. Do **not** replace `redb` immediately.
2. First define a Kronroe storage contract and wrap `redb` behind it.
3. Add observability and benchmark workloads.
4. Only prototype a replacement after we can show a clear product or technical
   constraint that `redb` cannot satisfy.

Strategic end state:

- Kronroe should ultimately ship with a fully Kronroe-owned storage engine and
  file format.
- `redb` is a transition dependency, not the desired long-term ecosystem state.
- The phased plan below is about reaching that end state safely rather than
  treating `redb` as permanent infrastructure.

## Repo-Grounded Current State

Direct `redb` usage is concentrated in:

- [`/Users/rebekahcole/kronroe/crates/core/src/temporal_graph.rs`](/Users/rebekahcole/kronroe/crates/core/src/temporal_graph.rs)
- [`/Users/rebekahcole/kronroe/crates/core/Cargo.toml`](/Users/rebekahcole/kronroe/crates/core/Cargo.toml)
- [`/Users/rebekahcole/kronroe/Cargo.toml`](/Users/rebekahcole/kronroe/Cargo.toml)

Supporting modules depend on persisted state that `TemporalGraph` loads from
storage, but they do not use `redb` directly:

- [`/Users/rebekahcole/kronroe/crates/core/src/vector.rs`](/Users/rebekahcole/kronroe/crates/core/src/vector.rs)
- [`/Users/rebekahcole/kronroe/crates/core/src/contradiction.rs`](/Users/rebekahcole/kronroe/crates/core/src/contradiction.rs)
- [`/Users/rebekahcole/kronroe/crates/core/src/uncertainty.rs`](/Users/rebekahcole/kronroe/crates/core/src/uncertainty.rs)

### Persisted tables and roles

The current storage model uses these tables:

- `META`
  Stores `schema_version`.
- `FACTS`
  Stores JSON `Fact` rows keyed by `"{subject}:{predicate}:{fact_id}"`.
- `IDEMPOTENCY`
  Maps caller-supplied idempotency keys to fact IDs.
- `PREDICATE_REGISTRY`
  Stores contradiction-detection predicate metadata.
- `VOLATILITY_REGISTRY`
  Stores uncertainty volatility configuration.
- `SOURCE_WEIGHT_REGISTRY`
  Stores source authority weights.
- `EMBEDDINGS`
  Stores raw embedding bytes keyed by fact ID.
- `EMBEDDING_META`
  Stores established embedding dimension.

### Current invariants Kronroe depends on

The code relies on these backend properties:

1. Atomic multi-table writes
   Fact rows, idempotency mappings, embedding bytes, and metadata writes must
   commit together or not at all.
2. Single-writer serialization
   The engine assumes write transactions do not interleave, which makes
   contradiction checks and embedding dimension establishment race-free.
3. Snapshot-safe reads
   Rebuilds and scans assume read transactions see a coherent view across tables.
4. Ordered iteration
   The current design depends on composite key iteration plus sortable fact IDs
   to make prefix scans usable.
5. Schema gating and migration
   Open/init depends on version stamping and controlled migration.
6. Durable source of truth
   The in-memory vector index is a derived cache and must be rebuildable from
   persisted data on open.

If a future backend cannot preserve these guarantees, the following paths break:

- `assert_fact_idempotent`
- `assert_fact_with_embedding`
- contradiction-checked writes
- vector index rebuild on open
- `current_facts`, `facts_at`, and any prefix-scan-heavy path
- schema upgrade and compatibility behavior

## What `redb` Officially Gives Us

From primary sources:

- ACID transactions
- crash-safe default behavior
- copy-on-write B-tree storage
- MVCC-style concurrent readers with a single writer
- savepoints and rollbacks
- configurable transaction durability
- a stable file format with stated upgrade intent

References:

- [redb homepage](https://www.redb.org/)
- [redb crate docs](https://docs.rs/redb/latest/redb/)
- [WriteTransaction docs](https://docs.rs/redb/latest/redb/struct.WriteTransaction.html)
- [Durability docs](https://docs.rs/redb/latest/redb/enum.Durability.html)
- [redb GitHub repository](https://github.com/cberner/redb)

What remains unclear from primary sources reviewed so far:

- compaction and long-term file-size behavior under correction-heavy workloads
- explicit upgrade tooling beyond file-format stability intent
- detailed guarantees around in-memory backend semantics

These are not blockers for using `redb` now, but they matter if Kronroe wants
to surpass it with a storage engine that is product-shaped rather than generic.

## What "Better for Kronroe" Means

A replacement should not be justified by "ownership" alone. It should deliver
clear product or operational advantages such as:

- temporal-native storage layout instead of generic KV prefix scans
- better support for append-only history and correction chains
- lower write amplification for correction-heavy workloads
- faster current-state and historical range scans
- more predictable file growth
- better cold-open and rebuild characteristics
- first-class control over file format and migration strategy
- richer storage observability and repair tooling

The bar is high. A replacement must also preserve:

- crash safety
- atomicity
- deterministic ordering
- upgrade safety
- cross-platform compatibility

## Architectural Options

### Option A: Keep `redb`, add a Kronroe storage facade

Description:
Create a storage abstraction inside `crates/core` and implement it with `redb`
first.

Benefits:

- lowest risk
- clarifies Kronroe's storage contract
- makes later replacement possible without changing the public engine API

Risks:

- still constrained by `redb`'s writer model and file format
- may delay learning if we stop at abstraction only

Recommendation:
This should happen first regardless of whether `redb` is eventually replaced.

### Option B: Build a purpose-built transactional KV engine

Description:
Replace `redb` with a Kronroe-owned storage engine that preserves the current
table-style architecture and transaction semantics.

Benefits:

- full control of file format and recovery path
- targeted optimization for current read/write patterns
- smoother migration from the existing model than a more radical redesign

Risks:

- very high correctness burden
- less strategically differentiated than a temporal-native design
- still table-first rather than history-first

### Option C: Append-only temporal log plus derived indexes

Description:
Persist all assertions, corrections, and invalidations as an immutable event log
and derive query indexes from that log.

Benefits:

- best fit with Kronroe's temporal identity
- natural auditability and replay semantics
- indexes become rebuildable derived structures by design

Risks:

- largest implementation effort
- index maintenance and cold-start complexity
- requires a more substantial migration path from current persisted tables

Recommendation:
This is the most promising long-term Kronroe-native direction, but only after
the storage contract and benchmark picture are solid.

## Decision

Current recommendation:

1. Phase in a storage facade over `redb`.
2. Instrument and benchmark the existing workload.
3. Prototype an append-only temporal backend only if benchmarks or product goals
   show that `redb` is a real limiter.

This avoids an early high-risk rewrite while still moving Kronroe toward a fully
Kronroe-owned storage architecture.

## Research Questions

These must be answered before any serious replacement effort:

1. What are Kronroe's heaviest real storage workloads?
2. Which current operations are latency-sensitive versus throughput-sensitive?
3. How large do correction chains and embedding sets get in realistic usage?
4. What file growth patterns appear under correction-heavy and vector-heavy use?
5. Is open/rebuild cost acceptable for current and projected workloads?
6. Are current prefix-scan patterns becoming the bottleneck?
7. Which guarantees are product requirements versus implementation convenience?

## Benchmark Matrix

The benchmark suite should compare current `redb` behavior against any future
prototype backend.

### Workload A: Assert-heavy ingestion

Shape:

- 10k, 100k, and 1M fact assertions
- mixed predicates
- mixed subjects with both hot and cold keys

Measure:

- p50/p95 write latency
- throughput
- file size growth
- write amplification symptoms

Success criteria:

- no regression for small and medium workloads
- measurable improvement if replacement is justified

### Workload B: Correction-heavy timeline churn

Shape:

- repeated corrections and invalidations on the same `(subject, predicate)`
- long chains of superseded facts

Measure:

- p50/p95 correction latency
- file growth per correction
- historical query latency
- current-state query latency after heavy churn

Success criteria:

- better temporal workload behavior than current prefix-scan model

### Workload C: Current-state scan

Shape:

- hot subjects with many facts
- mixed predicate widths

Measure:

- p50/p95 latency for `current_facts`
- CPU cost
- iteration volume

Success criteria:

- stable or improved latency under wide subject records

### Workload D: Historical point-in-time scan

Shape:

- many versions of the same fact family over time
- point-in-time queries over recent and older timestamps

Measure:

- p50/p95 `facts_at` latency
- rows/pages scanned
- sensitivity to correction density

Success criteria:

- improved performance for history-shaped workloads, not just current state

### Workload E: Idempotent retries

Shape:

- repeated duplicate writes with hot idempotency keys
- mixed successful and duplicate requests

Measure:

- duplicate-hit latency
- transaction contention impact
- correctness under concurrency

Success criteria:

- no correctness regressions
- duplicate path remains cheap

### Workload F: Embedding persistence and reopen

Shape:

- fact assertion with embeddings
- large embedding table
- process restart/reopen

Measure:

- embedding write latency
- reopen latency
- vector index rebuild time
- file size overhead

Success criteria:

- no correctness regressions
- measurable improvement if a replacement promises lower reopen cost

### Workload G: Mixed real-task session

Shape:

- realistic sequence: remember -> correct -> invalidate -> recall -> reopen
- include contradiction and uncertainty registries where enabled

Measure:

- end-to-end latency
- storage growth
- rebuild behavior
- correctness after reopen

Success criteria:

- behavior remains fully correct
- results are interpretable in product terms, not only storage metrics

## Observability Plan

Before replacement work, add storage instrumentation for:

- write duration by operation type
- commit duration
- read scan duration by operation type
- rows examined during prefix-scan paths
- database file size over time
- embedding rebuild duration on open
- correction-chain depth
- hot subject/predicate distribution

This should produce evidence for whether `redb` is actually constraining Kronroe.

## Implementation Plan

### Phase 1: Define and isolate the storage contract

Goal:
Move storage operations behind a Kronroe-owned interface while preserving
behavior exactly.

Tasks:

1. Introduce a storage backend abstraction in `crates/core`.
2. Move table open/read/write logic out of the high-level `TemporalGraph`
   methods and behind backend methods.
3. Keep `redb` as the only implementation in this phase.
4. Add backend contract tests for:
   - atomic fact writes
   - atomic fact + idempotency writes
   - atomic fact + embedding writes
   - snapshot-safe rebuilds
   - schema initialization and migration

Exit criteria:

- no public API changes
- no behavioral regressions
- all storage-sensitive tests run against the new abstraction

### Phase 2: Add observability and benchmark harnesses

Goal:
Measure the current system before building a replacement.

Tasks:

1. Add benchmark fixtures for the workload matrix above.
2. Add storage telemetry hooks around critical paths.
3. Document baseline `redb` results in a follow-up benchmark note.

Exit criteria:

- baseline numbers exist for all major workloads
- at least one clear bottleneck or non-bottleneck conclusion is documented

### Phase 3: Prototype a Kronroe-native backend

Goal:
Build an experimental backend without changing the public engine API.

Preferred prototype direction:

- append-only temporal log
- persisted or rebuildable derived indexes for:
  - current state
  - historical lookup
  - idempotency
  - embeddings
  - schema metadata

Tasks:

1. Create an experimental backend crate or internal module.
2. Implement append-only writes and crash-safe commit protocol.
3. Implement index rebuild and open/recovery path.
4. Run the full backend contract suite plus workload benchmarks.

Exit criteria:

- prototype matches correctness expectations
- benchmark results show a real advantage in Kronroe-shaped workloads

### Phase 4: Replacement decision gate

Goal:
Choose whether to keep `redb`, continue prototyping, or begin cutover.

A replacement should only move forward if it proves:

- equal or better correctness
- equal or better recovery safety
- better temporal workload performance or storage efficiency
- acceptable migration cost
- maintainable implementation complexity

If these are not met, Kronroe should keep the facade and continue on `redb`.

If they are met, the target outcome is:

- replace `redb` in the shipping runtime
- ship a Kronroe-owned storage backend as the default engine
- own the file format, migration path, repair tooling, and storage observability

## Risks

Major risks in replacing `redb` too early:

- correctness regressions in persisted history
- crash-recovery bugs
- migration bugs that corrupt `.kronroe` files
- delayed product work due to backend complexity
- building a generic KV engine instead of a truly Kronroe-shaped storage model

Mitigations:

- preserve the public API while swapping internals
- use contract tests aggressively
- benchmark before deciding
- prototype in parallel rather than forcing immediate cutover

## Immediate Next 3 Moves

1. Write the storage contract and backend boundary inside `crates/core`.
2. Add benchmark fixtures and storage telemetry for the current `redb` path.
3. Write a follow-up benchmark report before any prototype replacement work begins.

## Appendix: Why This Is Not Another Tantivy/ULID Situation

`tantivy` and `ulid` were narrower dependencies with relatively contained
replacement surfaces.

`redb` is different:

- it sits underneath nearly every persisted invariant
- correctness is more important than feature ownership
- the wrong replacement path creates long-term maintenance drag

That makes the right strategy:

- own the storage contract first
- collect evidence second
- replace it with a backend that is meaningfully more Kronroe-native and
  ultimately removes `redb` from the shipped ecosystem
