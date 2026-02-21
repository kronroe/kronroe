//! In-memory vector index for semantic similarity search.
//!
//! Phase 0 implementation: flat (brute-force) cosine similarity over pre-computed
//! embeddings. No external dependencies. Works on every target — native, WASM, iOS,
//! Android.
//!
//! Callers supply embeddings; Kronroe never generates them. Embedding generation is
//! the responsibility of `kronroe-agent-memory` or the calling application.
//!
//! # Complexity
//! - `insert`: O(1) amortised
//! - `remove`: O(n) swap-remove — acceptable at Phase 0 scale (hundreds to low
//!   thousands of facts)
//! - `search`: O(n·d) where d is embedding dimension
//!
//! When corpora grow to tens of thousands of entries a proper HNSW index should
//! replace this module. See CLAUDE.md §0.8 for the evaluation notes.

use crate::FactId;
use std::collections::HashSet;

/// An entry in the index: a fact identifier paired with its embedding vector.
#[derive(Debug, Clone)]
struct Entry {
    id: FactId,
    embedding: Vec<f32>,
}

/// Flat vector index keyed by [`FactId`].
///
/// The index is held entirely in memory. It is **not** persisted to redb — embeddings
/// are re-populated on application startup. This is intentional for Phase 0: it keeps
/// the storage format simple and avoids coupling vector serialisation to the redb
/// schema before the API has stabilised.
#[derive(Debug, Default, Clone)]
pub struct VectorIndex {
    entries: Vec<Entry>,
    /// Expected embedding dimension. Set on first insert; subsequent inserts are
    /// validated against it.
    dim: Option<usize>,
}

impl VectorIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace an embedding for `id`.
    ///
    /// # Panics
    /// Panics if `embedding` is empty or if its dimension differs from the first
    /// embedding ever inserted into this index.
    pub fn insert(&mut self, id: FactId, embedding: Vec<f32>) {
        assert!(!embedding.is_empty(), "embedding must not be empty");

        match self.dim {
            None => self.dim = Some(embedding.len()),
            Some(d) => assert_eq!(
                embedding.len(),
                d,
                "embedding dimension mismatch: expected {d}, got {}",
                embedding.len()
            ),
        }

        // Replace an existing entry for the same id (e.g. after `correct_fact`).
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.embedding = embedding;
        } else {
            self.entries.push(Entry { id, embedding });
        }
    }

    /// Remove the entry for `id`. No-op if `id` is not present.
    ///
    /// Uses swap-remove for O(1) memory ops at the cost of non-stable ordering —
    /// acceptable because search results are always re-ranked by score.
    ///
    /// Not called from `invalidate_fact` by design: invalidated facts are
    /// excluded via the `valid_ids` allow-list in `search_by_vector`, so their
    /// embeddings must remain in the index to support historical point-in-time
    /// searches. This method exists for future compaction / explicit eviction
    /// scenarios (e.g. permanent deletion in Phase 1).
    #[allow(dead_code)]
    pub fn remove(&mut self, id: &FactId) {
        if let Some(pos) = self.entries.iter().position(|e| &e.id == id) {
            self.entries.swap_remove(pos);
        }
    }

    /// Return the top-`k` entries by cosine similarity to `query`, restricted to
    /// the `valid_ids` allow-list.
    ///
    /// `valid_ids` is computed by the caller from the bi-temporal index (e.g. all
    /// facts valid at time T), enabling temporal filtering without coupling this
    /// module to redb or chrono.
    ///
    /// Results are returned in descending similarity order. If fewer than `k`
    /// entries pass the filter, all passing entries are returned.
    ///
    /// Returns an empty `Vec` if `valid_ids` is empty or `k` is zero.
    pub fn search(
        &self,
        query: &[f32],
        k: usize,
        valid_ids: &HashSet<FactId>,
    ) -> Vec<(FactId, f32)> {
        if k == 0 || valid_ids.is_empty() || self.entries.is_empty() {
            return Vec::new();
        }

        let query_norm = l2_norm(query);
        if query_norm == 0.0 {
            return Vec::new();
        }

        let mut scored: Vec<(FactId, f32)> = self
            .entries
            .iter()
            .filter(|e| valid_ids.contains(&e.id))
            .map(|e| {
                let score = cosine_similarity(query, &e.embedding, query_norm);
                (e.id.clone(), score)
            })
            .collect();

        // Partial sort: bring the top-k to the front. For small n (Phase 0) a full
        // sort is fine and avoids the unstable behaviour of select_nth_unstable.
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }

    /// Expected embedding dimension (set on first insert, `None` if empty).
    ///
    /// Used by [`TemporalGraph::assert_fact_with_embedding`] to pre-validate
    /// the embedding before writing to redb, keeping the two stores in sync.
    pub(crate) fn dim(&self) -> Option<usize> {
        self.dim
    }

    /// Number of entries currently in the index.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if the index contains no entries.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Math helpers
// ---------------------------------------------------------------------------

/// Euclidean (L2) norm of `v`.
fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

/// Cosine similarity between `a` and `b`.
///
/// `a_norm` is pre-computed by the caller to avoid redundant work when the same
/// query is scored against many entries.
///
/// Returns a value in `[-1.0, 1.0]`. Returns `0.0` if `b` is the zero vector.
fn cosine_similarity(a: &[f32], b: &[f32], a_norm: f32) -> f32 {
    // Runtime guard (not debug-only): `insert` enforces uniform dims, but if a
    // and b somehow differ `zip` would silently truncate and return a wrong
    // score.  Returning 0.0 on mismatch is the safest neutral value.
    if a.len() != b.len() {
        return 0.0;
    }

    let b_norm = l2_norm(b);
    if b_norm == 0.0 {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    dot / (a_norm * b_norm)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn new_id() -> FactId {
        FactId::new()
    }

    fn make_ids(n: usize) -> Vec<FactId> {
        (0..n).map(|_| new_id()).collect()
    }

    fn all_ids(ids: &[FactId]) -> HashSet<FactId> {
        ids.iter().cloned().collect()
    }

    // ------------------------------------------------------------------
    // l2_norm / cosine_similarity
    // ------------------------------------------------------------------

    #[test]
    fn test_l2_norm_unit_vector() {
        let v = vec![1.0f32, 0.0, 0.0];
        assert!((l2_norm(&v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_identical_vectors() {
        let v = vec![1.0f32, 2.0, 3.0];
        let norm = l2_norm(&v);
        assert!((cosine_similarity(&v, &v, norm) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_orthogonal_vectors() {
        let a = vec![1.0f32, 0.0];
        let b = vec![0.0f32, 1.0];
        let norm_a = l2_norm(&a);
        assert!((cosine_similarity(&a, &b, norm_a)).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_opposite_vectors() {
        let a = vec![1.0f32, 0.0];
        let b = vec![-1.0f32, 0.0];
        let norm_a = l2_norm(&a);
        assert!((cosine_similarity(&a, &b, norm_a) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_zero_b_returns_zero() {
        let a = vec![1.0f32, 2.0];
        let b = vec![0.0f32, 0.0];
        let norm_a = l2_norm(&a);
        assert_eq!(cosine_similarity(&a, &b, norm_a), 0.0);
    }

    // ------------------------------------------------------------------
    // VectorIndex::insert
    // ------------------------------------------------------------------

    #[test]
    fn test_insert_single() {
        let mut idx = VectorIndex::new();
        let id = new_id();
        idx.insert(id.clone(), vec![1.0, 0.0, 0.0]);
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn test_insert_replaces_existing_id() {
        let mut idx = VectorIndex::new();
        let id = new_id();
        idx.insert(id.clone(), vec![1.0, 0.0, 0.0]);
        idx.insert(id.clone(), vec![0.0, 1.0, 0.0]);
        // Should replace, not append.
        assert_eq!(idx.len(), 1);
    }

    #[test]
    #[should_panic(expected = "embedding must not be empty")]
    fn test_insert_empty_embedding_panics() {
        let mut idx = VectorIndex::new();
        idx.insert(new_id(), vec![]);
    }

    #[test]
    #[should_panic(expected = "embedding dimension mismatch")]
    fn test_insert_dimension_mismatch_panics() {
        let mut idx = VectorIndex::new();
        idx.insert(new_id(), vec![1.0, 0.0]);
        idx.insert(new_id(), vec![1.0, 0.0, 0.0]); // wrong dim
    }

    // ------------------------------------------------------------------
    // VectorIndex::remove
    // ------------------------------------------------------------------

    #[test]
    fn test_remove_existing() {
        let mut idx = VectorIndex::new();
        let id = new_id();
        idx.insert(id.clone(), vec![1.0, 0.0]);
        idx.remove(&id);
        assert!(idx.is_empty());
    }

    #[test]
    fn test_remove_nonexistent_is_noop() {
        let mut idx = VectorIndex::new();
        idx.insert(new_id(), vec![1.0, 0.0]);
        idx.remove(&new_id()); // random id not in index
        assert_eq!(idx.len(), 1);
    }

    // ------------------------------------------------------------------
    // VectorIndex::search — basic ranking
    // ------------------------------------------------------------------

    #[test]
    fn test_search_returns_empty_for_empty_index() {
        let idx = VectorIndex::new();
        let valid: HashSet<FactId> = HashSet::new();
        let results = idx.search(&[1.0, 0.0], 5, &valid);
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_k_zero_returns_empty() {
        let mut idx = VectorIndex::new();
        let id = new_id();
        idx.insert(id.clone(), vec![1.0, 0.0]);
        let valid = all_ids(&[id]);
        let results = idx.search(&[1.0, 0.0], 0, &valid);
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_returns_top_k_in_order() {
        let mut idx = VectorIndex::new();
        let ids = make_ids(3);

        // Three vectors with clear similarity ranking relative to query [1,0,0]:
        //   ids[0] → [1,0,0]  sim = 1.0  (best)
        //   ids[1] → [0,1,0]  sim = 0.0
        //   ids[2] → [-1,0,0] sim = -1.0 (worst)
        idx.insert(ids[0].clone(), vec![1.0, 0.0, 0.0]);
        idx.insert(ids[1].clone(), vec![0.0, 1.0, 0.0]);
        idx.insert(ids[2].clone(), vec![-1.0, 0.0, 0.0]);

        let valid = all_ids(&ids);
        let results = idx.search(&[1.0, 0.0, 0.0], 3, &valid);

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, ids[0]);
        assert!((results[0].1 - 1.0).abs() < 1e-6);
        assert_eq!(results[1].0, ids[1]);
        assert_eq!(results[2].0, ids[2]);
        assert!((results[2].1 + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_search_truncates_to_k() {
        let mut idx = VectorIndex::new();
        let ids = make_ids(5);
        for id in &ids {
            idx.insert(id.clone(), vec![1.0, 0.0]);
        }
        let valid = all_ids(&ids);
        let results = idx.search(&[1.0, 0.0], 3, &valid);
        assert_eq!(results.len(), 3);
    }

    // ------------------------------------------------------------------
    // VectorIndex::search — temporal filtering
    // ------------------------------------------------------------------

    #[test]
    fn test_search_respects_valid_ids_filter() {
        let mut idx = VectorIndex::new();
        let ids = make_ids(3);

        // All three vectors are identical (sim = 1.0) so ranking won't obscure
        // the filtering behaviour.
        for id in &ids {
            idx.insert(id.clone(), vec![1.0, 0.0]);
        }

        // Only ids[0] and ids[2] are "valid at time T" (caller-supplied filter).
        let valid: HashSet<FactId> = [ids[0].clone(), ids[2].clone()].into_iter().collect();
        let results = idx.search(&[1.0, 0.0], 10, &valid);

        assert_eq!(results.len(), 2);
        let returned_ids: HashSet<FactId> = results.into_iter().map(|(id, _)| id).collect();
        assert!(returned_ids.contains(&ids[0]));
        assert!(!returned_ids.contains(&ids[1])); // excluded by temporal filter
        assert!(returned_ids.contains(&ids[2]));
    }

    #[test]
    fn test_search_empty_valid_ids_returns_empty() {
        let mut idx = VectorIndex::new();
        idx.insert(new_id(), vec![1.0, 0.0]);
        let valid: HashSet<FactId> = HashSet::new();
        let results = idx.search(&[1.0, 0.0], 5, &valid);
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_zero_query_returns_empty() {
        let mut idx = VectorIndex::new();
        let id = new_id();
        idx.insert(id.clone(), vec![1.0, 0.0]);
        let valid = all_ids(&[id]);
        // Zero vector has no direction — undefined cosine similarity.
        let results = idx.search(&[0.0, 0.0], 5, &valid);
        assert!(results.is_empty());
    }
}
