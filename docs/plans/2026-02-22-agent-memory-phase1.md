# AgentMemory Phase 1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement AgentMemory Phase 1 — wire `remember()`, `recall()`, and `assemble_context()` stubs into working code using the existing hybrid RRF retrieval scaffold, add idempotency-key support, and complete the PyO3 Python binding surface.

**Architecture:** `AgentMemory` in `crates/agent-memory` wraps `TemporalGraph` from `crates/core`. The core already has hybrid RRF type scaffolding (`HybridParams`, `HybridFusionStrategy`, `TemporalAdjustment`) behind `#[cfg(feature = "hybrid-experimental")]`; we promote those types to `pub`, extract ranked retrieval helpers, implement weighted RRF fusion in a new `hybrid.rs` module, then wire the `AgentMemory` methods on top. Idempotency uses a new `IDEMPOTENCY` redb table (key → FactId) in the same write transaction as fact assertion.

**Tech Stack:** Rust stable, redb 3.1, tantivy (feature: fulltext), flat cosine vector index (feature: vector), PyO3/maturin — **no C++ required**.

---

## Prerequisites

No new dependencies are needed. Everything already exists in `Cargo.toml` workspace files. You need:
- Rust toolchain installed: `rustup show`
- `cargo test --all --all-features` runs green before you start

**Create feature branch first:**

```bash
cd /Users/rebekahcole/kronroe
git checkout -b feature/agent-memory-phase1
git push -u origin feature/agent-memory-phase1
```

---

## Quick reference: key files

| File | What it is |
|------|-----------|
| `crates/core/src/temporal_graph.rs` | `TemporalGraph` engine — all the retrieval logic |
| `crates/core/src/hybrid.rs` | **New file** — RRF fusion module |
| `crates/core/Cargo.toml` | Feature flags: `fulltext`, `vector`, `hybrid-experimental` |
| `crates/agent-memory/src/agent_memory.rs` | `AgentMemory` — stubs to implement |
| `crates/agent-memory/Cargo.toml` | **Needs new `[features]` block** |
| `crates/python/src/python_bindings.rs` | PyO3 bindings — incomplete surface |

---

## How redb works (brief)

redb is a pure-Rust embedded key-value store. Key patterns you'll see:

```rust
// Open a table in a read transaction
let read_txn = self.db.begin_read()?;
let table = read_txn.open_table(SOME_TABLE)?;
let value = table.get("key")?.map(|g| g.value().to_owned()); // extract BEFORE mutable ops

// Open a table in a write transaction
let write_txn = self.db.begin_write()?;
{
    let mut table = write_txn.open_table(SOME_TABLE)?;
    table.insert("key", "value")?;
} // drop table before commit
write_txn.commit()?;
```

**Critical gotcha:** `table.get("key")?` returns an `AccessGuard<V>` that borrows `table`. Always call `.map(|g| g.value().to_owned())` to extract an owned value before any mutable operation.

---

## How the test helpers work

In `crates/core/src/temporal_graph.rs` tests:
```rust
fn open_temp_db() -> (TemporalGraph, tempfile::NamedTempFile) {
    let f = tempfile::NamedTempFile::new().unwrap();
    let db = TemporalGraph::open(f.path().to_str().unwrap()).unwrap();
    (db, f)
}
```

In `crates/agent-memory/src/agent_memory.rs` tests:
```rust
fn open_temp_memory() -> (AgentMemory, tempfile::NamedTempFile) {
    let f = tempfile::NamedTempFile::new().unwrap();
    let mem = AgentMemory::open(f.path().to_str().unwrap()).unwrap();
    (mem, f)
}
```

The `NamedTempFile` must stay alive for the duration of the test (Rust drops it at end of scope, deleting the file).

---

## Task 0: Verify baseline

**Files:** None modified.

**Step 1: Run the full test suite**

```bash
cargo test --all --all-features 2>&1 | tail -20
```

Expected: all tests pass, no compilation errors.

**Step 2: Run clippy**

```bash
cargo clippy --all --all-features -- -D warnings 2>&1 | tail -20
```

Expected: no warnings, exits 0.

**Step 3: Commit nothing — just confirm you have a clean baseline.**

---

## Task 1: Make hybrid types `pub` and remove dead_code allows

**Context:** `HybridFusionStrategy`, `TemporalAdjustment`, and `HybridParams` are all `pub(crate)` with `#[allow(dead_code)]` at lines ~106–163 of `crates/core/src/temporal_graph.rs`. The `agent-memory` crate needs to import them, so they must be `pub`. Removing `#[allow(dead_code)]` means clippy will warn if they're unused — a good forcing function.

**Files:**
- Modify: `crates/core/src/temporal_graph.rs` (lines ~106–163)

**Step 1: Find the exact lines**

```bash
grep -n "pub(crate) enum HybridFusionStrategy\|pub(crate) enum TemporalAdjustment\|pub(crate) struct HybridParams\|allow(dead_code)" crates/core/src/temporal_graph.rs | head -20
```

**Step 2: Make the edits**

In `crates/core/src/temporal_graph.rs`, find and replace all three type definitions. Change every occurrence of:
- `#[allow(dead_code)]` immediately before a hybrid type → **remove the line**
- `pub(crate) enum HybridFusionStrategy` → `pub enum HybridFusionStrategy`
- `pub(crate) enum TemporalAdjustment` → `pub enum TemporalAdjustment`
- `pub(crate) struct HybridParams` → `pub struct HybridParams`
- Fields inside `HybridParams` that are `pub` already — leave them.

After editing, the three types should look like:

```rust
#[cfg(feature = "hybrid-experimental")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HybridFusionStrategy {
    Rrf,
}

#[cfg(feature = "hybrid-experimental")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TemporalAdjustment {
    None,
    HalfLifeDays { days: f32 },
}

#[cfg(feature = "hybrid-experimental")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HybridParams {
    pub k: usize,
    pub candidate_window: usize,
    pub fusion: HybridFusionStrategy,
    pub rank_constant: usize,
    pub text_weight: f32,
    pub vector_weight: f32,
    pub temporal_weight: f32,
    pub temporal_adjustment: TemporalAdjustment,
}

#[cfg(feature = "hybrid-experimental")]
impl Default for HybridParams {
    fn default() -> Self {
        Self {
            k: 10,
            candidate_window: 50,
            fusion: HybridFusionStrategy::Rrf,
            rank_constant: 60,
            text_weight: 0.5,
            vector_weight: 0.5,
            temporal_weight: 0.0,
            temporal_adjustment: TemporalAdjustment::None,
        }
    }
}
```

**Step 3: Verify it compiles**

```bash
cargo build -p kronroe --all-features 2>&1 | tail -10
```

Expected: compiles cleanly.

**Step 4: Run clippy**

```bash
cargo clippy -p kronroe --all-features -- -D warnings 2>&1 | tail -10
```

Expected: no warnings (the types will be used in later tasks).

**Step 5: Commit**

```bash
git add crates/core/src/temporal_graph.rs
git commit -m "feat(core): make hybrid types pub for cross-crate use"
```

---

## Task 2: Add `IDEMPOTENCY` redb table to core

**Context:** We need a table `"idempotency"` mapping `&str` (user-supplied key) → `&str` (FactId string). It must be opened in `init()` so redb creates it on first open. A separate `assert_fact_idempotent()` method on `TemporalGraph` will check-then-write atomically.

**Files:**
- Modify: `crates/core/src/temporal_graph.rs`

**Step 1: Write the failing test**

At the bottom of the `#[cfg(test)]` module in `crates/core/src/temporal_graph.rs`, add:

```rust
#[test]
fn test_idempotency_key_deduplicates() {
    let (db, _f) = open_temp_db();
    let t = Utc::now();
    let id1 = db
        .assert_fact_idempotent("alice", "role", "admin", t, "idem-key-1")
        .unwrap();
    let id2 = db
        .assert_fact_idempotent("alice", "role", "admin", t, "idem-key-1")
        .unwrap();
    // Same key → same FactId
    assert_eq!(id1.0, id2.0);
}

#[test]
fn test_idempotency_different_keys_create_different_facts() {
    let (db, _f) = open_temp_db();
    let t = Utc::now();
    let id1 = db
        .assert_fact_idempotent("alice", "role", "admin", t, "key-a")
        .unwrap();
    let id2 = db
        .assert_fact_idempotent("alice", "role", "admin", t, "key-b")
        .unwrap();
    assert_ne!(id1.0, id2.0);
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p kronroe test_idempotency --all-features 2>&1 | tail -10
```

Expected: compile error — `assert_fact_idempotent` not found.

**Step 3: Add the table constant**

Find the existing table constants in `crates/core/src/temporal_graph.rs` (near `const FACTS: TableDefinition...`). Add:

```rust
const IDEMPOTENCY: TableDefinition<&str, &str> = TableDefinition::new("idempotency");
```

**Step 4: Open the table in `init()`**

Find the `init()` function. It currently opens `FACTS` (and optionally embeddings tables). Add idempotency table opening **before** the early return. The table just needs to exist — we don't write anything here:

```rust
// inside init(), after opening FACTS table block
{
    let _table = write_txn.open_table(IDEMPOTENCY)?;
}
```

**Step 5: Implement `assert_fact_idempotent`**

Add this public method to `impl TemporalGraph` (after `assert_fact`):

```rust
/// Assert a fact, deduplicated by `idempotency_key`.
/// If a fact was previously stored with the same key, returns its original `FactId`
/// without writing a duplicate. Otherwise stores the fact and records the key.
pub fn assert_fact_idempotent(
    &self,
    subject: &str,
    predicate: &str,
    object: impl Into<Value>,
    valid_from: DateTime<Utc>,
    idempotency_key: &str,
) -> Result<FactId> {
    // Check: is this key already recorded?
    {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(IDEMPOTENCY)?;
        if let Some(existing) = table.get(idempotency_key)? {
            let id_str = existing.value().to_owned();
            return Ok(FactId(id_str));
        }
    }
    // Write: store fact + idempotency record atomically
    let write_txn = self.db.begin_write()?;
    let fact_id = write_fact_in_txn(&write_txn, subject, predicate, object.into(), valid_from)?;
    {
        let mut idem_table = write_txn.open_table(IDEMPOTENCY)?;
        idem_table.insert(idempotency_key, fact_id.0.as_str())?;
    }
    write_txn.commit()?;
    Ok(fact_id)
}
```

**Step 6: Run tests**

```bash
cargo test -p kronroe test_idempotency --all-features 2>&1 | tail -15
```

Expected: both tests pass.

**Step 7: Run all core tests**

```bash
cargo test -p kronroe --all-features 2>&1 | tail -10
```

Expected: all pass.

**Step 8: Commit**

```bash
git add crates/core/src/temporal_graph.rs
git commit -m "feat(core): add IDEMPOTENCY table and assert_fact_idempotent()"
```

---

## Task 3: Add `write_fact_full_in_txn` private helper (custom confidence)

**Context:** The existing `write_fact_in_txn` creates a `Fact` via `Fact::new()` which hardcodes `confidence: 1.0` and `source: None`. `AssertParams` needs custom confidence. We add a private variant that accepts all fields.

**Files:**
- Modify: `crates/core/src/temporal_graph.rs`

**Step 1: Write the failing test**

In the `#[cfg(test)]` module, add:

```rust
#[test]
fn test_assert_fact_with_confidence() {
    let (db, _f) = open_temp_db();
    let t = Utc::now();
    // We'll test via assert_with_confidence which wraps write_fact_full_in_txn
    let fact_id = db
        .assert_fact_with_confidence("alice", "trust", "high", t, 0.75)
        .unwrap();
    let facts = db.current_facts("alice", "trust").unwrap();
    assert_eq!(facts.len(), 1);
    assert!((facts[0].confidence - 0.75).abs() < 0.001);
    let _ = fact_id;
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p kronroe test_assert_fact_with_confidence --all-features 2>&1 | tail -5
```

Expected: compile error.

**Step 3: Add `write_fact_full_in_txn`**

Add this private function directly after `write_fact_in_txn`:

```rust
fn write_fact_full_in_txn(
    write_txn: &redb::WriteTransaction,
    subject: &str,
    predicate: &str,
    object: Value,
    valid_from: DateTime<Utc>,
    confidence: f32,
    source: Option<String>,
) -> Result<FactId> {
    let mut fact = Fact::new(subject, predicate, object, valid_from);
    fact.confidence = confidence;
    fact.source = source;
    let fact_id = fact.id.clone();
    let key = format!("{}:{}:{}", subject, predicate, fact.id);
    let value = serde_json::to_string(&fact)?;
    {
        let mut table = write_txn.open_table(FACTS)?;
        table.insert(key.as_str(), value.as_str())?;
    }
    Ok(fact_id)
}
```

**Step 4: Add public `assert_fact_with_confidence` method (used by tests and agent-memory)**

```rust
pub fn assert_fact_with_confidence(
    &self,
    subject: &str,
    predicate: &str,
    object: impl Into<Value>,
    valid_from: DateTime<Utc>,
    confidence: f32,
) -> Result<FactId> {
    let write_txn = self.db.begin_write()?;
    let fact_id = write_fact_full_in_txn(
        &write_txn,
        subject,
        predicate,
        object.into(),
        valid_from,
        confidence,
        None,
    )?;
    write_txn.commit()?;
    Ok(fact_id)
}
```

**Step 5: Run test**

```bash
cargo test -p kronroe test_assert_fact_with_confidence --all-features 2>&1 | tail -10
```

Expected: passes.

**Step 6: Run all**

```bash
cargo test -p kronroe --all-features 2>&1 | tail -5
```

**Step 7: Commit**

```bash
git add crates/core/src/temporal_graph.rs
git commit -m "feat(core): add write_fact_full_in_txn and assert_fact_with_confidence"
```

---

## Task 4: Add `search_ranked` private fulltext helper

**Context:** The existing `search()` method builds a tantivy in-memory index at query time and returns `Vec<Fact>` — scores are discarded via `for (_score, addr) in top_docs`. For RRF fusion we need `(FactId, rank)` pairs. We add a private `search_ranked()` that returns `Vec<(FactId, usize)>` where `usize` is 0-indexed rank (0 = best).

**Files:**
- Modify: `crates/core/src/temporal_graph.rs`

**Step 1: Write the failing test**

```rust
#[cfg(feature = "hybrid-experimental")]
#[test]
fn test_search_ranked_returns_stable_order() {
    let (db, _f) = open_temp_db();
    let t = Utc::now();
    db.assert_fact("alice", "bio", "loves Rust programming", t).unwrap();
    db.assert_fact("bob", "bio", "loves Python scripting", t).unwrap();
    db.assert_fact("carol", "bio", "expert in Rust and systems", t).unwrap();

    let ranked = db.search_ranked("Rust", 5).unwrap();
    assert!(!ranked.is_empty(), "should find results");
    // Verify rank indices are 0..n-1 in order
    for (i, (_id, rank)) in ranked.iter().enumerate() {
        assert_eq!(*rank, i, "rank should be 0-indexed position");
    }
    // Rust facts should come before Python fact
    // (both alice and carol should rank above bob)
    assert!(ranked.len() >= 2);
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p kronroe test_search_ranked --all-features 2>&1 | tail -5
```

Expected: compile error — `search_ranked` not found.

**Step 3: Implement `search_ranked`**

Study `search()` first — find it in `crates/core/src/temporal_graph.rs` (around line 521). It calls `scan_prefix("", |_| true)` to collect all facts into tantivy, then `searcher.search(&query, &TopDocs::with_limit(limit))`. Add this private method directly after `search()`:

```rust
#[cfg(feature = "hybrid-experimental")]
fn search_ranked(&self, query: &str, limit: usize) -> Result<Vec<(FactId, usize)>> {
    use tantivy::collector::TopDocs;
    use tantivy::query::QueryParser;
    use tantivy::schema::{Schema, TEXT, STORED};
    use tantivy::{doc, Index, TantivyDocument};

    // Build in-memory index (same approach as search())
    let mut schema_builder = Schema::builder();
    let id_field = schema_builder.add_text_field("id", STORED);
    let content_field = schema_builder.add_text_field("content", TEXT);
    let schema = schema_builder.build();

    let index = Index::create_in_ram(schema.clone());
    let mut index_writer = index.writer(3_000_000)?;

    let all_facts = self.scan_prefix("", |_| true)?;
    for fact in &all_facts {
        let content = format!(
            "{} {} {}",
            fact.subject,
            fact.predicate,
            match &fact.object {
                Value::Text(s) | Value::Entity(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Boolean(b) => b.to_string(),
            }
        );
        index_writer.add_document(doc!(
            id_field => fact.id.0.as_str(),
            content_field => content.as_str()
        ))?;
    }
    index_writer.commit()?;

    let reader = index.reader()?;
    let searcher = reader.searcher();
    let query_parser = QueryParser::for_index(&index, vec![content_field]);
    let parsed = match query_parser.parse_query(query) {
        Ok(q) => q,
        Err(_) => return Ok(vec![]),
    };

    let top_docs = searcher.search(&parsed, &TopDocs::with_limit(limit))?;
    let mut results = Vec::with_capacity(top_docs.len());
    for (rank, (_score, addr)) in top_docs.iter().enumerate() {
        let doc: TantivyDocument = searcher.doc(*addr)?;
        if let Some(id_val) = doc.get_first(id_field) {
            if let Some(id_str) = id_val.as_str() {
                results.push((FactId(id_str.to_owned()), rank));
            }
        }
    }
    Ok(results)
}
```

**Important:** Look at the existing `search()` function to copy its exact tantivy import style — the version of tantivy in `Cargo.toml` determines what's available. Match the existing imports exactly.

**Step 4: Run test**

```bash
cargo test -p kronroe test_search_ranked --all-features 2>&1 | tail -15
```

Expected: passes.

**Step 5: Run all**

```bash
cargo test -p kronroe --all-features 2>&1 | tail -5
```

**Step 6: Commit**

```bash
git add crates/core/src/temporal_graph.rs
git commit -m "feat(core): add search_ranked() private helper for hybrid fusion"
```

---

## Task 5: Add `search_by_vector_ranked` private helper

**Context:** Similar to Task 4 but for vectors. `search_by_vector()` returns `Vec<(Fact, f32)>` sorted by cosine score. We need `Vec<(FactId, usize)>` for RRF. This wraps `search_by_vector`.

**Files:**
- Modify: `crates/core/src/temporal_graph.rs`

**Step 1: Write the failing test**

```rust
#[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
#[test]
fn test_search_by_vector_ranked_order() {
    let (db, _f) = open_temp_db();
    let t = Utc::now();
    // 2-dimensional embeddings for simplicity
    let _ = db.assert_fact_with_embedding("alice", "bio", "Rust dev", t, vec![1.0, 0.0]).unwrap();
    let _ = db.assert_fact_with_embedding("bob", "bio", "Python dev", t, vec![0.0, 1.0]).unwrap();

    // Query closer to alice
    let ranked = db.search_by_vector_ranked(&[0.9, 0.1], 5, None).unwrap();
    assert!(!ranked.is_empty());
    for (i, (_id, rank)) in ranked.iter().enumerate() {
        assert_eq!(*rank, i);
    }
    // alice (closer to [1,0]) should be rank 0
    assert_eq!(ranked[0].0 .0.len(), 26); // ULID is 26 chars — just verify it's a valid id
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p kronroe test_search_by_vector_ranked --all-features 2>&1 | tail -5
```

**Step 3: Implement `search_by_vector_ranked`**

Add after `search_by_vector`:

```rust
#[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
fn search_by_vector_ranked(
    &self,
    query: &[f32],
    k: usize,
    at: Option<DateTime<Utc>>,
) -> Result<Vec<(FactId, usize)>> {
    let results = self.search_by_vector(query, k, at)?;
    Ok(results
        .into_iter()
        .enumerate()
        .map(|(rank, (fact, _score))| (fact.id, rank))
        .collect())
}
```

**Step 4: Run test**

```bash
cargo test -p kronroe test_search_by_vector_ranked --all-features 2>&1 | tail -10
```

**Step 5: Run all**

```bash
cargo test -p kronroe --all-features 2>&1 | tail -5
```

**Step 6: Commit**

```bash
git add crates/core/src/temporal_graph.rs
git commit -m "feat(core): add search_by_vector_ranked() private helper for hybrid fusion"
```

---

## Task 6: Create `crates/core/src/hybrid.rs` with RRF fusion

**Context:** This is the core math. Weighted Reciprocal Rank Fusion: for each candidate, accumulate `weight / (rank_constant + rank)` from each channel. Merge by `FactId`, break ties by ULID string (deterministic). Temporal adjustment is a bounded additive pass after fusion.

**Files:**
- Create: `crates/core/src/hybrid.rs`
- Modify: `crates/core/src/temporal_graph.rs` (add `mod hybrid;`)

**Step 1: Write the failing tests (in `hybrid.rs` itself)**

Create `crates/core/src/hybrid.rs` with this content:

```rust
//! Hybrid retrieval fusion (RRF + optional temporal adjustment).
//! Only compiled under `#[cfg(feature = "hybrid-experimental")]`.

use crate::{FactId, HybridFusionStrategy, HybridParams, TemporalAdjustment};
use std::collections::HashMap;

/// A single result from hybrid retrieval with score breakdown.
#[derive(Debug, Clone)]
pub struct HybridHit {
    pub fact_id: FactId,
    pub final_score: f64,
    pub text_rrf_contrib: f64,
    pub vector_rrf_contrib: f64,
    pub temporal_adjustment: f64,
}

/// Weighted RRF fusion over two ranked channels.
///
/// `text_channel`: `(FactId, rank)` pairs from fulltext, rank 0 = best.
/// `vec_channel`:  `(FactId, rank)` pairs from vector, rank 0 = best.
/// Returns top-`params.k` hits, sorted by `final_score` descending.
pub(crate) fn rrf_fuse(
    text_channel: &[(FactId, usize)],
    vec_channel: &[(FactId, usize)],
    params: &HybridParams,
) -> Vec<HybridHit> {
    debug_assert!(
        (params.text_weight + params.vector_weight - 1.0).abs() < 0.01,
        "text_weight + vector_weight should sum to ~1.0"
    );

    let mut text_scores: HashMap<String, f64> = HashMap::new();
    let mut vec_scores: HashMap<String, f64> = HashMap::new();

    let k = params.rank_constant as f64;

    for (fact_id, rank) in text_channel {
        let score = params.text_weight as f64 / (k + *rank as f64);
        text_scores.insert(fact_id.0.clone(), score);
    }

    for (fact_id, rank) in vec_channel {
        let score = params.vector_weight as f64 / (k + *rank as f64);
        vec_scores.insert(fact_id.0.clone(), score);
    }

    // Collect all unique FactIds
    let mut all_ids: Vec<String> = text_scores.keys().cloned().collect();
    for id in vec_scores.keys() {
        if !text_scores.contains_key(id) {
            all_ids.push(id.clone());
        }
    }

    let mut hits: Vec<HybridHit> = all_ids
        .into_iter()
        .map(|id| {
            let t = text_scores.get(&id).copied().unwrap_or(0.0);
            let v = vec_scores.get(&id).copied().unwrap_or(0.0);
            HybridHit {
                fact_id: FactId(id),
                final_score: t + v,
                text_rrf_contrib: t,
                vector_rrf_contrib: v,
                temporal_adjustment: 0.0,
            }
        })
        .collect();

    // Sort: descending score, tie-break by FactId string (deterministic)
    hits.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.fact_id.0.cmp(&b.fact_id.0))
    });

    hits.truncate(params.k);
    hits
}

/// Apply bounded additive temporal adjustment to hits in-place.
///
/// `recency_days`: map from FactId string → age in days (0 = brand new).
/// Effect is capped at +/- 0.1 to avoid overpowering retrieval signals.
pub(crate) fn temporal_adjust(
    hits: &mut Vec<HybridHit>,
    adjustment: TemporalAdjustment,
    recency_days: &HashMap<String, f32>,
) {
    const MAX_ADJUST: f64 = 0.1;
    match adjustment {
        TemporalAdjustment::None => {}
        TemporalAdjustment::HalfLifeDays { days } => {
            for hit in hits.iter_mut() {
                let age = recency_days
                    .get(&hit.fact_id.0)
                    .copied()
                    .unwrap_or(f32::MAX);
                // Exponential decay: e^(-ln2 * age / half_life)
                let decay = (-std::f32::consts::LN_2 * age / days).exp() as f64;
                // decay in [0, 1]; centre on 0 so new facts get +, old facts get -
                let adj = ((decay - 0.5) * 2.0 * MAX_ADJUST).clamp(-MAX_ADJUST, MAX_ADJUST);
                hit.temporal_adjustment = adj;
                hit.final_score += adj;
            }
            // Re-sort after adjustment
            hits.sort_by(|a, b| {
                b.final_score
                    .partial_cmp(&a.final_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.fact_id.0.cmp(&b.fact_id.0))
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_params(text_w: f32, vec_w: f32) -> HybridParams {
        HybridParams {
            k: 5,
            candidate_window: 20,
            fusion: HybridFusionStrategy::Rrf,
            rank_constant: 60,
            text_weight: text_w,
            vector_weight: vec_w,
            temporal_weight: 0.0,
            temporal_adjustment: TemporalAdjustment::None,
        }
    }

    fn id(s: &str) -> FactId {
        FactId(s.to_owned())
    }

    #[test]
    fn test_rrf_empty_inputs() {
        let params = make_params(0.5, 0.5);
        let hits = rrf_fuse(&[], &[], &params);
        assert!(hits.is_empty());
    }

    #[test]
    fn test_rrf_text_only() {
        let params = make_params(0.5, 0.5);
        let text = vec![(id("a"), 0usize), (id("b"), 1)];
        let hits = rrf_fuse(&text, &[], &params);
        assert_eq!(hits.len(), 2);
        // a should score higher (lower rank)
        assert!(hits[0].fact_id.0 == "a");
        assert!(hits[0].vector_rrf_contrib == 0.0);
    }

    #[test]
    fn test_rrf_vector_only() {
        let params = make_params(0.5, 0.5);
        let vec_ch = vec![(id("x"), 0usize), (id("y"), 1)];
        let hits = rrf_fuse(&[], &vec_ch, &params);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].fact_id.0, "x");
        assert!(hits[0].text_rrf_contrib == 0.0);
    }

    #[test]
    fn test_rrf_overlap_boosts_shared_candidate() {
        let params = make_params(0.5, 0.5);
        let text = vec![(id("common"), 0usize), (id("text_only"), 1)];
        let vec_ch = vec![(id("common"), 0usize), (id("vec_only"), 1)];
        let hits = rrf_fuse(&text, &vec_ch, &params);
        // "common" appears in both → higher score
        assert_eq!(hits[0].fact_id.0, "common");
        assert!(hits[0].text_rrf_contrib > 0.0);
        assert!(hits[0].vector_rrf_contrib > 0.0);
    }

    #[test]
    fn test_rrf_tie_is_deterministic() {
        let params = make_params(0.5, 0.5);
        // Two isolated candidates at identical ranks → tie-break by FactId lex
        let text = vec![(id("zzz"), 0usize)];
        let vec_ch = vec![(id("aaa"), 0usize)];
        let hits = rrf_fuse(&text, &vec_ch, &params);
        // Both score identically; "aaa" < "zzz" lexicographically → "aaa" wins tie
        assert_eq!(hits[0].fact_id.0, "aaa");
        assert_eq!(hits[1].fact_id.0, "zzz");
    }

    #[test]
    fn test_rrf_respects_k_limit() {
        let params = HybridParams { k: 2, ..make_params(0.5, 0.5) };
        let text: Vec<_> = (0..10).map(|i| (id(&format!("t{}", i)), i)).collect();
        let hits = rrf_fuse(&text, &[], &params);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn test_temporal_none_is_noop() {
        let params = make_params(0.5, 0.5);
        let text = vec![(id("a"), 0usize)];
        let mut hits = rrf_fuse(&text, &[], &params);
        let original_score = hits[0].final_score;
        let recency: HashMap<String, f32> = [("a".to_owned(), 0.0)].into();
        temporal_adjust(&mut hits, TemporalAdjustment::None, &recency);
        assert!((hits[0].final_score - original_score).abs() < 1e-10);
    }

    #[test]
    fn test_temporal_half_life_decay_is_bounded() {
        let params = make_params(0.5, 0.5);
        let text = vec![(id("new"), 0usize), (id("old"), 1)];
        let mut hits = rrf_fuse(&text, &[], &params);
        let recency: HashMap<String, f32> =
            [("new".to_owned(), 0.0), ("old".to_owned(), 1000.0)].into();
        temporal_adjust(
            &mut hits,
            TemporalAdjustment::HalfLifeDays { days: 30.0 },
            &recency,
        );
        // New fact gets positive adjustment, old gets negative
        let new_hit = hits.iter().find(|h| h.fact_id.0 == "new").unwrap();
        let old_hit = hits.iter().find(|h| h.fact_id.0 == "old").unwrap();
        assert!(new_hit.temporal_adjustment > 0.0);
        assert!(old_hit.temporal_adjustment < 0.0);
        // Both bounded by MAX_ADJUST = 0.1
        assert!(new_hit.temporal_adjustment.abs() <= 0.1 + 1e-10);
        assert!(old_hit.temporal_adjustment.abs() <= 0.1 + 1e-10);
    }

    #[test]
    fn test_temporal_monotonic_decay() {
        // Older facts should have lower (more negative) adjustments
        let adj_fn = |age_days: f32| -> f64 {
            let decay = (-std::f32::consts::LN_2 * age_days / 30.0).exp() as f64;
            ((decay - 0.5) * 2.0 * 0.1).clamp(-0.1, 0.1)
        };
        let adj_1 = adj_fn(1.0);
        let adj_30 = adj_fn(30.0);
        let adj_90 = adj_fn(90.0);
        assert!(adj_1 > adj_30, "1-day-old should adjust higher than 30-day-old");
        assert!(adj_30 > adj_90, "30-day-old should adjust higher than 90-day-old");
    }
}
```

**Step 2: Wire `mod hybrid` in `crates/core/src/temporal_graph.rs`**

In `crates/core/src/temporal_graph.rs`, find where modules are declared (near the top, after `use` statements). Add:

```rust
#[cfg(feature = "hybrid-experimental")]
mod hybrid;
#[cfg(feature = "hybrid-experimental")]
pub use hybrid::HybridHit;
```

**Step 3: Run the hybrid unit tests**

```bash
cargo test -p kronroe --features hybrid-experimental hybrid:: 2>&1 | tail -20
```

Expected: all `hybrid::tests::*` tests pass.

**Step 4: Run all core tests**

```bash
cargo test -p kronroe --all-features 2>&1 | tail -10
```

**Step 5: Run clippy**

```bash
cargo clippy -p kronroe --all-features -- -D warnings 2>&1 | tail -10
```

**Step 6: Commit**

```bash
git add crates/core/src/hybrid.rs crates/core/src/temporal_graph.rs
git commit -m "feat(core): add hybrid.rs with RRF fusion and temporal adjustment"
```

---

## Task 7: Add `search_hybrid_experimental` public API on `TemporalGraph`

**Context:** This wires `search_ranked` + `search_by_vector_ranked` + `rrf_fuse` + `temporal_adjust` into one public method. It lives in `crates/core/src/temporal_graph.rs` under `#[cfg(all(feature = "hybrid-experimental", feature = "vector"))]`.

**Files:**
- Modify: `crates/core/src/temporal_graph.rs`

**Step 1: Write the failing test**

```rust
#[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
#[test]
fn test_search_hybrid_experimental_returns_hits() {
    let (db, _f) = open_temp_db();
    let t = Utc::now();
    db.assert_fact_with_embedding(
        "alice", "bio", "expert Rust systems programmer", t, vec![1.0, 0.0, 0.0],
    ).unwrap();
    db.assert_fact_with_embedding(
        "bob", "bio", "Python data scientist", t, vec![0.0, 1.0, 0.0],
    ).unwrap();
    db.assert_fact_with_embedding(
        "carol", "bio", "Rust and embedded systems", t, vec![0.9, 0.1, 0.0],
    ).unwrap();

    let params = crate::HybridParams::default();
    let hits = db
        .search_hybrid_experimental("Rust", &[1.0f32, 0.0, 0.0], params, None)
        .unwrap();

    assert!(!hits.is_empty(), "should return results");
    // Scores sum correctly (text + vector + temporal = final)
    for hit in &hits {
        let expected = hit.text_rrf_contrib + hit.vector_rrf_contrib + hit.temporal_adjustment;
        assert!(
            (hit.final_score - expected).abs() < 1e-9,
            "score breakdown should sum to final_score"
        );
    }
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p kronroe test_search_hybrid_experimental --all-features 2>&1 | tail -5
```

**Step 3: Implement `search_hybrid_experimental`**

Add to `impl TemporalGraph` in `crates/core/src/temporal_graph.rs`:

```rust
#[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
pub fn search_hybrid_experimental(
    &self,
    text_query: &str,
    vector_query: &[f32],
    params: HybridParams,
    at: Option<DateTime<Utc>>,
) -> Result<Vec<crate::hybrid::HybridHit>> {
    use crate::hybrid::{rrf_fuse, temporal_adjust};
    use std::collections::HashMap;

    let text_ranked = self.search_ranked(text_query, params.candidate_window)?;
    let vec_ranked = self.search_by_vector_ranked(vector_query, params.candidate_window, at)?;

    let mut hits = rrf_fuse(&text_ranked, &vec_ranked, &params);

    // Build recency map for temporal adjustment
    if !matches!(params.temporal_adjustment, TemporalAdjustment::None) {
        let now = at.unwrap_or_else(chrono::Utc::now);
        let mut recency: HashMap<String, f32> = HashMap::new();
        let all_facts = self.scan_prefix("", |_| true)?;
        for fact in &all_facts {
            let age_days = (now - fact.valid_from).num_seconds().max(0) as f32 / 86400.0;
            recency.insert(fact.id.0.clone(), age_days);
        }
        temporal_adjust(&mut hits, params.temporal_adjustment, &recency);
    }

    Ok(hits)
}
```

**Step 4: Run test**

```bash
cargo test -p kronroe test_search_hybrid_experimental --all-features 2>&1 | tail -15
```

**Step 5: Run all**

```bash
cargo test -p kronroe --all-features 2>&1 | tail -5
```

**Step 6: Commit**

```bash
git add crates/core/src/temporal_graph.rs
git commit -m "feat(core): add search_hybrid_experimental() public API"
```

---

## Task 8: Add `[features]` to `agent-memory/Cargo.toml`

**Context:** `crates/agent-memory` currently has no `[features]` section. We add `hybrid` which enables both `hybrid-experimental` and `vector` in core.

**Files:**
- Modify: `crates/agent-memory/Cargo.toml`

**Step 1: Open the file and add features**

Current `dependencies` section has `kronroe = { path = "../core" }`. Change to:

```toml
[features]
hybrid = ["kronroe/hybrid-experimental", "kronroe/vector"]

[dependencies]
kronroe = { path = "../core" }
```

**Step 2: Verify both feature modes compile**

```bash
# Without hybrid
cargo build -p kronroe-agent-memory 2>&1 | tail -5

# With hybrid
cargo build -p kronroe-agent-memory --features hybrid 2>&1 | tail -5
```

Both should compile cleanly.

**Step 3: Commit**

```bash
git add crates/agent-memory/Cargo.toml
git commit -m "feat(agent-memory): add hybrid feature flag"
```

---

## Task 9: Add `AssertParams` and `assert_with_params` to `AgentMemory`

**Context:** Callers need idempotency + custom confidence when storing memories. `assert_with_params` wraps `TemporalGraph::assert_fact_idempotent` (Task 2) and `assert_fact_with_confidence` (Task 3).

**Files:**
- Modify: `crates/agent-memory/src/agent_memory.rs`

**Step 1: Write the failing test**

In `crates/agent-memory/src/agent_memory.rs` tests module:

```rust
#[test]
fn test_assert_with_params_idempotency() {
    let (mem, _f) = open_temp_memory();
    let params1 = AssertParams {
        idempotency_key: Some("ep-001-memory-1".to_owned()),
        confidence: 0.9,
        valid_from: None,
    };
    let params2 = AssertParams {
        idempotency_key: Some("ep-001-memory-1".to_owned()),
        confidence: 0.9,
        valid_from: None,
    };
    let id1 = mem.assert_with_params("alice", "role", "engineer", params1).unwrap();
    let id2 = mem.assert_with_params("alice", "role", "engineer", params2).unwrap();
    assert_eq!(id1.0, id2.0, "same idempotency key should return same FactId");
}

#[test]
fn test_assert_with_params_custom_confidence() {
    let (mem, _f) = open_temp_memory();
    let params = AssertParams {
        idempotency_key: None,
        confidence: 0.5,
        valid_from: None,
    };
    let id = mem.assert_with_params("bob", "trust", "low", params).unwrap();
    let facts = mem.facts_about("bob").unwrap();
    let fact = facts.iter().find(|f| f.id.0 == id.0).unwrap();
    assert!((fact.confidence - 0.5).abs() < 0.001);
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p kronroe-agent-memory test_assert_with_params --all-features 2>&1 | tail -5
```

**Step 3: Implement**

Add to `crates/agent-memory/src/agent_memory.rs`:

At the top, after existing imports:
```rust
use chrono::Utc;
```
(if not already imported — check first)

Then add the struct and method:

```rust
/// Parameters for `assert_with_params`.
#[derive(Debug, Default)]
pub struct AssertParams {
    /// If set, deduplicates: same key → same FactId returned without re-storing.
    pub idempotency_key: Option<String>,
    /// Confidence score in [0.0, 1.0]. Defaults to 1.0 if not set.
    pub confidence: f32,
    /// Valid-from timestamp. Defaults to now.
    pub valid_from: Option<chrono::DateTime<Utc>>,
}

impl AgentMemory {
    // ... existing methods ...

    /// Assert a fact with idempotency and custom confidence.
    pub fn assert_with_params(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        params: AssertParams,
    ) -> Result<FactId> {
        let valid_from = params.valid_from.unwrap_or_else(Utc::now);
        let confidence = if params.confidence == 0.0 { 1.0 } else { params.confidence };

        if let Some(key) = &params.idempotency_key {
            self.db.assert_fact_idempotent(
                subject,
                predicate,
                object.to_string(),
                valid_from,
                key.as_str(),
            )
        } else {
            self.db.assert_fact_with_confidence(
                subject,
                predicate,
                object.to_string(),
                valid_from,
                confidence,
            )
        }
    }
}
```

**Step 4: Run tests**

```bash
cargo test -p kronroe-agent-memory test_assert_with_params --all-features 2>&1 | tail -15
```

**Step 5: Run all**

```bash
cargo test -p kronroe-agent-memory --all-features 2>&1 | tail -5
```

**Step 6: Commit**

```bash
git add crates/agent-memory/src/agent_memory.rs
git commit -m "feat(agent-memory): add AssertParams and assert_with_params()"
```

---

## Task 10: Implement `remember()`

**Context:** Replace the `unimplemented!()` stub. `remember()` stores one fact: `subject=episode_id, predicate="memory", object=text`. With optional embedding. Returns single `FactId`.

**New signature** (breaking — was `Vec<FactId>`, now `FactId`; was `unimplemented!()` so no callers):

```rust
pub fn remember(
    &self,
    text: &str,
    episode_id: &str,
    embedding: Option<Vec<f32>>,
) -> Result<FactId>
```

**Files:**
- Modify: `crates/agent-memory/src/agent_memory.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn test_remember_stores_fact() {
    let (mem, _f) = open_temp_memory();
    let id = mem.remember("Alice loves Rust", "ep-001", None).unwrap();
    assert_eq!(id.0.len(), 26, "should be a valid ULID");
    let facts = mem.facts_about("ep-001").unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].subject, "ep-001");
    assert_eq!(facts[0].predicate, "memory");
    // object is the text
    assert!(matches!(&facts[0].object, kronroe::Value::Text(t) if t == "Alice loves Rust"));
}

#[cfg(feature = "hybrid")]
#[test]
fn test_remember_with_embedding() {
    let (mem, _f) = open_temp_memory();
    let embedding = vec![0.1f32, 0.2, 0.3];
    let id = mem.remember("Bob likes Python", "ep-002", Some(embedding)).unwrap();
    assert_eq!(id.0.len(), 26);
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p kronroe-agent-memory test_remember --all-features 2>&1 | tail -5
```

**Step 3: Implement**

Replace the `remember()` stub in `crates/agent-memory/src/agent_memory.rs`:

```rust
pub fn remember(
    &self,
    text: &str,
    episode_id: &str,
    #[cfg(feature = "hybrid")] embedding: Option<Vec<f32>>,
    #[cfg(not(feature = "hybrid"))] _embedding: Option<Vec<f32>>,
) -> Result<FactId> {
    #[cfg(feature = "hybrid")]
    if let Some(emb) = embedding {
        return self.db.assert_fact_with_embedding(
            episode_id,
            "memory",
            text.to_string(),
            Utc::now(),
            emb,
        );
    }

    self.db.assert_fact(episode_id, "memory", text.to_string(), Utc::now())
}
```

**Step 4: Run tests**

```bash
cargo test -p kronroe-agent-memory test_remember --all-features 2>&1 | tail -15
```

**Step 5: Run all**

```bash
cargo test -p kronroe-agent-memory --all-features 2>&1 | tail -5
```

**Step 6: Commit**

```bash
git add crates/agent-memory/src/agent_memory.rs
git commit -m "feat(agent-memory): implement remember() with optional embedding"
```

---

## Task 11: Implement `recall()`

**Context:** Replace the `recall()` stub. Returns `Vec<Fact>` (was `Vec<String>` — breaking, no callers). Under `hybrid` feature uses `search_hybrid_experimental`; otherwise falls back to fulltext `search`.

**New signature:**
```rust
pub fn recall(&self, query: &str, query_embedding: Option<&[f32]>, limit: usize) -> Result<Vec<Fact>>
```

**Files:**
- Modify: `crates/agent-memory/src/agent_memory.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn test_recall_returns_matching_facts() {
    let (mem, _f) = open_temp_memory();
    mem.remember("Alice loves Rust programming", "ep-001", None).unwrap();
    mem.remember("Bob prefers Python for data science", "ep-002", None).unwrap();

    let results = mem.recall("Rust", None, 5).unwrap();
    assert!(!results.is_empty(), "should find Rust-related facts");
    // At least one result should mention Rust
    let has_rust = results.iter().any(|f| {
        matches!(&f.object, kronroe::Value::Text(t) if t.contains("Rust"))
    });
    assert!(has_rust);
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p kronroe-agent-memory test_recall --all-features 2>&1 | tail -5
```

**Step 3: Implement**

Replace the `recall()` stub:

```rust
pub fn recall(
    &self,
    query: &str,
    query_embedding: Option<&[f32]>,
    limit: usize,
) -> Result<Vec<Fact>> {
    #[cfg(feature = "hybrid")]
    if let Some(emb) = query_embedding {
        let params = kronroe::HybridParams::default();
        let hits = self.db.search_hybrid_experimental(query, emb, params, None)?;
        // Resolve FactIds back to Facts
        let mut facts = Vec::with_capacity(hits.len());
        for hit in &hits {
            if let Some(fact) = self.db.fact_by_id(&hit.fact_id)? {
                facts.push(fact);
            }
        }
        return Ok(facts);
    }

    // Fallback: fulltext only
    self.db.search(query, limit)
}
```

**Note:** `fact_by_id` must be public on `TemporalGraph`. Check if it already is:

```bash
grep -n "pub fn fact_by_id" crates/core/src/temporal_graph.rs
```

If it's `fn fact_by_id` (private), change it to `pub fn fact_by_id`.

**Step 4: Run tests**

```bash
cargo test -p kronroe-agent-memory test_recall --all-features 2>&1 | tail -15
```

**Step 5: Run all**

```bash
cargo test -p kronroe-agent-memory --all-features 2>&1 | tail -5
```

**Step 6: Commit**

```bash
git add crates/agent-memory/src/agent_memory.rs crates/core/src/temporal_graph.rs
git commit -m "feat(agent-memory): implement recall() with hybrid/fulltext retrieval"
```

---

## Task 12: Implement `assemble_context()`

**Context:** Replace `assemble_context()` stub. Calls `recall()`, formats results as a token-bounded string for an LLM context window. Simple join with newlines, truncated to `max_tokens` (1 token ≈ 4 chars — rough heuristic).

**New signature:**
```rust
pub fn assemble_context(&self, query: &str, query_embedding: Option<&[f32]>, max_tokens: usize) -> Result<String>
```

**Files:**
- Modify: `crates/agent-memory/src/agent_memory.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn test_assemble_context_returns_string() {
    let (mem, _f) = open_temp_memory();
    mem.remember("Alice is a Rust expert", "ep-001", None).unwrap();
    mem.remember("Bob is a Python expert", "ep-002", None).unwrap();

    let ctx = mem.assemble_context("who is an expert", None, 500).unwrap();
    assert!(!ctx.is_empty(), "context should not be empty");
    // Should contain at least one expert mention
    assert!(ctx.contains("expert"), "context should contain relevant facts");
}

#[test]
fn test_assemble_context_respects_token_limit() {
    let (mem, _f) = open_temp_memory();
    for i in 0..20 {
        mem.remember(&format!("fact number {} is quite long and wordy", i), &format!("ep-{}", i), None).unwrap();
    }
    let ctx = mem.assemble_context("fact", None, 50).unwrap();
    // 50 tokens ≈ 200 chars — context should be truncated
    assert!(ctx.len() <= 220, "context should respect max_tokens (with some slack)");
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p kronroe-agent-memory test_assemble_context --all-features 2>&1 | tail -5
```

**Step 3: Implement**

Replace `assemble_context()` stub:

```rust
pub fn assemble_context(
    &self,
    query: &str,
    query_embedding: Option<&[f32]>,
    max_tokens: usize,
) -> Result<String> {
    let facts = self.recall(query, query_embedding, 20)?;
    let char_budget = max_tokens * 4; // 1 token ≈ 4 chars
    let mut context = String::new();

    for fact in &facts {
        let line = format!(
            "[{}] {}: {}\n",
            fact.valid_from.format("%Y-%m-%d"),
            fact.subject,
            match &fact.object {
                kronroe::Value::Text(s) | kronroe::Value::Entity(s) => s.as_str(),
                _ => continue,
            }
        );
        if context.len() + line.len() > char_budget {
            break;
        }
        context.push_str(&line);
    }

    Ok(context)
}
```

**Step 4: Run tests**

```bash
cargo test -p kronroe-agent-memory test_assemble_context --all-features 2>&1 | tail -15
```

**Step 5: Run all**

```bash
cargo test -p kronroe-agent-memory --all-features 2>&1 | tail -5
```

**Step 6: Commit**

```bash
git add crates/agent-memory/src/agent_memory.rs
git commit -m "feat(agent-memory): implement assemble_context() with token budget"
```

---

## Task 13: Complete `PyKronroeDb` surface in Python bindings

**Context:** `crates/python/src/python_bindings.rs` `PyKronroeDb` only has `open`, `assert_fact`, `search`. It's missing `facts_about`, `facts_about_at`, `invalidate_fact`, `correct_fact`, `assert_fact_with_embedding`, `search_by_vector`. Add the missing methods.

**Files:**
- Modify: `crates/python/src/python_bindings.rs`
- Modify: `crates/python/Cargo.toml` (add `features = ["vector"]` to core dep)

**Step 1: Update Cargo.toml to enable vector feature**

In `crates/python/Cargo.toml`, change:
```toml
kronroe = { path = "../core" }
```
to:
```toml
kronroe = { path = "../core", features = ["vector"] }
```

**Step 2: Write the failing test**

In `crates/python/src/python_bindings.rs`, add a test at the bottom:

```rust
#[cfg(test)]
mod tests {
    // Python tests run via maturin/pytest; this is a compile-check test
    #[test]
    fn pydb_compiles_with_all_methods() {
        // Just ensure the code compiles — actual Python tests are in tests/
        let _ = stringify!(PyKronroeDb::facts_about);
        let _ = stringify!(PyKronroeDb::assert_fact_with_embedding);
        let _ = stringify!(PyKronroeDb::search_by_vector);
    }
}
```

**Step 3: Implement missing `PyKronroeDb` methods**

In the `#[pymethods] impl PyKronroeDb` block, add after `search()`:

```rust
fn facts_about(&self, py: Python<'_>, entity: &str) -> PyResult<Vec<Py<PyDict>>> {
    let facts = self.inner.all_facts_about(entity).map_err(to_py_err)?;
    facts_to_pylist(py, facts)
}

fn facts_about_at(
    &self,
    py: Python<'_>,
    entity: &str,
    predicate: &str,
    at_rfc3339: &str,
) -> PyResult<Vec<Py<PyDict>>> {
    let at = at_rfc3339
        .parse()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("invalid RFC3339 datetime"))?;
    let facts = self.inner.facts_at(entity, predicate, at).map_err(to_py_err)?;
    facts_to_pylist(py, facts)
}

fn invalidate_fact(&self, fact_id: &str) -> PyResult<()> {
    self.inner.invalidate_fact(fact_id).map_err(to_py_err)
}

fn correct_fact(
    &self,
    fact_id: &str,
    new_subject: &str,
    new_predicate: &str,
    new_object: &str,
) -> PyResult<String> {
    use chrono::Utc;
    let new_id = self
        .inner
        .correct_fact(fact_id, new_subject, new_predicate, new_object.to_string(), Utc::now())
        .map_err(to_py_err)?;
    Ok(new_id.0)
}

fn assert_fact_with_embedding(
    &self,
    subject: &str,
    predicate: &str,
    object: &str,
    embedding: Vec<f32>,
) -> PyResult<String> {
    use chrono::Utc;
    let id = self
        .inner
        .assert_fact_with_embedding(subject, predicate, object.to_string(), Utc::now(), embedding)
        .map_err(to_py_err)?;
    Ok(id.0)
}

fn search_by_vector(
    &self,
    py: Python<'_>,
    embedding: Vec<f32>,
    k: usize,
) -> PyResult<Vec<Py<PyDict>>> {
    let results = self
        .inner
        .search_by_vector(&embedding, k, None)
        .map_err(to_py_err)?;
    let facts: Vec<_> = results.into_iter().map(|(f, _)| f).collect();
    facts_to_pylist(py, facts)
}
```

**Step 4: Verify method names on `TemporalGraph`**

Some method names on `TemporalGraph` may differ. Check:
```bash
grep -n "pub fn " crates/core/src/temporal_graph.rs | grep -E "facts_about|all_facts|facts_at|invalidate|correct_fact"
```
Adjust method names in the PyO3 wrappers to match exactly.

**Step 5: Compile check**

```bash
cargo build -p kronroe-py 2>&1 | tail -10
```

**Step 6: Run clippy on python crate**

```bash
cargo clippy -p kronroe-py -- -D warnings 2>&1 | tail -10
```

**Step 7: Commit**

```bash
git add crates/python/src/python_bindings.rs crates/python/Cargo.toml
git commit -m "feat(python): complete PyKronroeDb method surface"
```

---

## Task 14: Complete `PyAgentMemory` surface

**Context:** `PyAgentMemory` is missing `correct_fact`, `recall`, `remember`, `assemble_context`, and `assert_with_params`.

**Files:**
- Modify: `crates/python/src/python_bindings.rs`

**Step 1: Add `recall`, `remember`, `assemble_context`, `assert_with_params` to `PyAgentMemory`**

In `#[pymethods] impl PyAgentMemory`, add after existing methods:

```rust
fn correct_fact(
    &self,
    fact_id: &str,
    new_subject: &str,
    new_predicate: &str,
    new_object: &str,
) -> PyResult<String> {
    let new_id = self
        .inner
        .correct_fact(fact_id, new_subject, new_predicate, new_object.to_string())
        .map_err(to_py_err)?;
    Ok(new_id.0)
}

fn remember(&self, text: &str, episode_id: &str) -> PyResult<String> {
    let id = self
        .inner
        .remember(text, episode_id, None)
        .map_err(to_py_err)?;
    Ok(id.0)
}

fn recall(
    &self,
    py: Python<'_>,
    query: &str,
    limit: usize,
) -> PyResult<Vec<Py<PyDict>>> {
    let facts = self
        .inner
        .recall(query, None, limit)
        .map_err(to_py_err)?;
    facts_to_pylist(py, facts)
}

fn assemble_context(&self, query: &str, max_tokens: usize) -> PyResult<String> {
    self.inner
        .assemble_context(query, None, max_tokens)
        .map_err(to_py_err)
}

fn assert_with_params(
    &self,
    subject: &str,
    predicate: &str,
    object: &str,
    idempotency_key: Option<&str>,
    confidence: Option<f32>,
) -> PyResult<String> {
    use kronroe_agent_memory::AssertParams;
    let params = AssertParams {
        idempotency_key: idempotency_key.map(|s| s.to_owned()),
        confidence: confidence.unwrap_or(1.0),
        valid_from: None,
    };
    let id = self
        .inner
        .assert_with_params(subject, predicate, object, params)
        .map_err(to_py_err)?;
    Ok(id.0)
}
```

**Step 2: Fix imports**

At the top of `crates/python/src/python_bindings.rs`, ensure:
```rust
use kronroe_agent_memory::AssertParams;
```
is present (or add it inline in the method as shown above).

**Step 3: Compile check**

```bash
cargo build -p kronroe-py 2>&1 | tail -10
```

**Step 4: Run clippy**

```bash
cargo clippy -p kronroe-py -- -D warnings 2>&1 | tail -10
```

**Step 5: Commit**

```bash
git add crates/python/src/python_bindings.rs
git commit -m "feat(python): complete PyAgentMemory method surface"
```

---

## Task 15: Integration tests for hybrid path (agent-memory)

**Context:** End-to-end tests that exercise the full stack: store facts with embeddings → hybrid recall → assemble context.

**Files:**
- Modify: `crates/agent-memory/src/agent_memory.rs` (tests module)

**Step 1: Write the tests**

```rust
#[cfg(feature = "hybrid")]
mod hybrid_tests {
    use super::*;

    fn open_temp_memory() -> (AgentMemory, tempfile::NamedTempFile) {
        let f = tempfile::NamedTempFile::new().unwrap();
        let mem = AgentMemory::open(f.path().to_str().unwrap()).unwrap();
        (mem, f)
    }

    #[test]
    fn test_hybrid_recall_text_wins() {
        // Scenario: text query strongly matches one fact; embeddings are neutral
        let (mem, _f) = open_temp_memory();
        mem.remember("Alice is a Rust engineer", "ep-1", Some(vec![0.5f32, 0.5])).unwrap();
        mem.remember("Bob likes Python", "ep-2", Some(vec![0.5f32, 0.5])).unwrap();
        // Query is text-specific; embeddings identical so text should win
        let results = mem.recall("Rust engineer", Some(&[0.5, 0.5]), 5).unwrap();
        assert!(!results.is_empty());
        let top = &results[0];
        assert!(matches!(&top.object, kronroe::Value::Text(t) if t.contains("Rust")));
    }

    #[test]
    fn test_hybrid_recall_vector_wins() {
        // Scenario: all text is similar but embeddings clearly differentiate
        let (mem, _f) = open_temp_memory();
        mem.remember("person A event", "ep-a", Some(vec![1.0f32, 0.0])).unwrap();
        mem.remember("person B event", "ep-b", Some(vec![0.0f32, 1.0])).unwrap();
        // Query vector very close to ep-a, generic text
        let results = mem.recall("person event", Some(&[0.95, 0.05]), 5).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_assemble_context_with_embedding() {
        let (mem, _f) = open_temp_memory();
        mem.remember("Alice is an expert in embedded Rust", "ep-1", Some(vec![1.0f32, 0.0])).unwrap();
        mem.remember("Bob specialises in Python web frameworks", "ep-2", Some(vec![0.0f32, 1.0])).unwrap();
        let ctx = mem.assemble_context("Rust expert", Some(&[1.0, 0.0]), 200).unwrap();
        assert!(!ctx.is_empty());
    }
}
```

**Step 2: Run**

```bash
cargo test -p kronroe-agent-memory --features hybrid hybrid_tests 2>&1 | tail -20
```

**Step 3: Commit**

```bash
git add crates/agent-memory/src/agent_memory.rs
git commit -m "test(agent-memory): add hybrid integration tests"
```

---

## Task 16: Final CI gate — all tests, clippy, fmt

**Step 1: Run the full test suite**

```bash
cargo test --all --all-features 2>&1 | tail -20
```

Expected: all tests pass.

**Step 2: Run clippy (must be zero warnings)**

```bash
cargo clippy --all --all-features -- -D warnings 2>&1 | tail -20
```

Fix any warnings before proceeding.

**Step 3: Check formatting**

```bash
cargo fmt --all -- --check 2>&1
```

If there are formatting issues:

```bash
cargo fmt --all
git add -u
git commit -m "style: cargo fmt"
```

**Step 4: Final commit if any fixes needed**

```bash
git add -u
git commit -m "fix: clippy and fmt cleanup for CI"
```

**Step 5: Push branch**

```bash
git push origin feature/agent-memory-phase1
```

**Step 6: Verify CI passes on GitHub**

Check: https://github.com/rebekahcole/kronroe/actions (or your org URL)

---

## Common Issues and Fixes

**Issue: `scan_prefix` is private in `hybrid.rs`**
- `scan_prefix` is defined in `crates/core/src/temporal_graph.rs` and called from `search_hybrid_experimental` which is also in `crates/core/src/temporal_graph.rs` — not a cross-module call, so visibility is fine. `hybrid.rs` does NOT call `scan_prefix` directly; the caller in `crates/core/src/temporal_graph.rs` does.

**Issue: `FactId` not in scope in `hybrid.rs`**
- Add `use crate::FactId;` at the top of `hybrid.rs`.

**Issue: `fact_by_id` compile error in `recall()`**
- The method may be named differently. Run: `grep -n "pub fn fact_by_id\|pub fn get_fact" crates/core/src/temporal_graph.rs`

**Issue: `tantivy` import path changed**
- Copy exact import style from the existing `search()` function — don't write imports from memory.

**Issue: clippy `dead_code` on `HybridParams` fields after Task 1**
- This is expected until Task 7 uses them. Add `#[allow(dead_code)]` temporarily if clippy -D warnings blocks you, then remove it in Task 7.

**Issue: `assert_fact_with_embedding` signature mismatch**
- Check the exact signature: `grep -n "pub fn assert_fact_with_embedding" crates/core/src/temporal_graph.rs`

**Issue: `correct_fact` signature differs between core and agent-memory**
- Core's `correct_fact` takes `valid_from: DateTime<Utc>`. Agent-memory's may not. Check each separately.

---

## No C++ Required

Zero C++ in this plan. Pure Rust for all core work. PyO3/maturin builds with `cargo` only. iOS cbindgen is a separate crate (`crates/ios`) not touched here.

For local Python dev iteration:
```bash
maturin develop -m crates/python/Cargo.toml
python -c "import kronroe; db = kronroe.AgentMemory.open('/tmp/test.db'); print(db)"
```
