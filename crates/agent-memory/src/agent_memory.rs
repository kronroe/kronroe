//! High-level agent memory API built on Kronroe.
//!
//! Designed to be a drop-in alternative to Graphiti / mem0 / MemGPT —
//! without the server, without Neo4j, without Python.
//!
//! # Usage
//!
//! ```rust,no_run
//! use kronroe_agent_memory::AgentMemory;
//!
//! let memory = AgentMemory::open("./my-agent.kronroe").unwrap();
//!
//! // Store a structured fact directly
//! memory.assert("alice", "works_at", "Acme").unwrap();
//!
//! // Query everything known about an entity
//! let facts = memory.facts_about("alice").unwrap();
//!
//! // Query what we knew at a point in time
//! let past: chrono::DateTime<chrono::Utc> = "2024-03-01T00:00:00Z".parse().unwrap();
//! let then = memory.facts_about_at("alice", "works_at", past).unwrap();
//! ```
//!
//! # Phase 1 API
//!
//! This crate exposes a practical Phase 1 surface:
//! - `remember(text, episode_id, embedding)` — store episodic memory
//! - `recall(query, query_embedding, limit)` — retrieve matching facts
//! - `assemble_context(query, query_embedding, max_tokens)` — build LLM context

use chrono::{DateTime, Utc};
#[cfg(feature = "contradiction")]
use kronroe::{ConflictPolicy, Contradiction};
use kronroe::{Fact, FactId, TemporalGraph, Value};
#[cfg(feature = "hybrid")]
use kronroe::{HybridScoreBreakdown, HybridSearchParams};

pub use kronroe::KronroeError as Error;
pub type Result<T> = std::result::Result<T, Error>;

// ---------------------------------------------------------------------------
// Explainable recall
// ---------------------------------------------------------------------------

/// Per-channel signal breakdown for a recalled fact.
///
/// These are the *input signals* that the retrieval engine used to rank
/// results — they explain what each channel contributed, not the final
/// composite ranking score. The result ordering in `recall_scored()` is
/// the authoritative ranking; inspect these fields to understand *why*
/// a given channel dominated or was weak for a particular fact.
///
/// Every variant includes `confidence` — the fact-level confidence score
/// from the underlying [`Fact`] (default 1.0). This lets callers weight
/// or filter results by trustworthiness alongside retrieval signals.
///
/// The variant indicates which retrieval path produced the result:
/// - [`Hybrid`] — RRF fusion input signals (text + vector channels).
///   The engine's two-stage reranker uses these as inputs alongside
///   temporal feasibility to determine final ordering.
/// - [`TextOnly`] — fulltext search with BM25 relevance score.
///
/// [`Hybrid`]: RecallScore::Hybrid
/// [`TextOnly`]: RecallScore::TextOnly
/// [`Fact`]: kronroe::Fact
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum RecallScore {
    /// Input signals from hybrid retrieval (text + vector channels).
    ///
    /// The `rrf_score` is the pre-rerank RRF fusion score (sum of
    /// text + vector contributions). Final result ordering may differ
    /// because the two-stage reranker applies adaptive weighting and
    /// temporal feasibility filtering on top of these signals.
    #[non_exhaustive]
    Hybrid {
        /// Pre-rerank RRF fusion score (text + vector sum).
        rrf_score: f64,
        /// Text-channel contribution from weighted RRF.
        text_contrib: f64,
        /// Vector-channel contribution from weighted RRF.
        vector_contrib: f64,
        /// Fact-level confidence \[0.0, 1.0\] from the stored fact.
        confidence: f32,
    },
    /// Result from fulltext-only retrieval.
    #[non_exhaustive]
    TextOnly {
        /// Ordinal rank in the result set (0-indexed).
        rank: usize,
        /// Tantivy BM25 relevance score. Higher = stronger lexical match.
        /// Comparable within a single query but not across queries.
        bm25_score: f32,
        /// Fact-level confidence \[0.0, 1.0\] from the stored fact.
        confidence: f32,
    },
}

impl RecallScore {
    /// Human-readable score tag suitable for debug output or LLM context.
    ///
    /// Returns a decimal RRF score `"0.032"` for hybrid results, or
    /// a BM25 score with rank `"#1 bm25:4.21"` for text-only results.
    pub fn display_tag(&self) -> String {
        match self {
            RecallScore::Hybrid { rrf_score, .. } => format!("{:.3}", rrf_score),
            RecallScore::TextOnly {
                rank, bm25_score, ..
            } => format!("#{} bm25:{:.2}", rank + 1, bm25_score),
        }
    }

    /// The fact-level confidence score, regardless of retrieval path.
    pub fn confidence(&self) -> f32 {
        match self {
            RecallScore::Hybrid { confidence, .. } | RecallScore::TextOnly { confidence, .. } => {
                *confidence
            }
        }
    }

    /// Convert a [`HybridScoreBreakdown`] from the core engine into a
    /// [`RecallScore::Hybrid`], incorporating the fact's confidence.
    #[cfg(feature = "hybrid")]
    fn from_breakdown(b: &HybridScoreBreakdown, confidence: f32) -> Self {
        RecallScore::Hybrid {
            rrf_score: b.final_score,
            text_contrib: b.text_rrf_contrib,
            vector_contrib: b.vector_rrf_contrib,
            confidence,
        }
    }
}

/// High-level agent memory store built on a Kronroe temporal graph.
///
/// This is the primary entry point for AI agent developers.
/// It wraps [`TemporalGraph`] with an API designed for agent use cases.
pub struct AgentMemory {
    graph: TemporalGraph,
}

#[derive(Debug, Clone)]
pub struct AssertParams {
    pub valid_from: DateTime<Utc>,
}

impl AgentMemory {
    /// Open or create an agent memory store at the given path.
    ///
    /// ```rust,no_run
    /// use kronroe_agent_memory::AgentMemory;
    /// let memory = AgentMemory::open("./my-agent.kronroe").unwrap();
    /// ```
    pub fn open(path: &str) -> Result<Self> {
        let graph = TemporalGraph::open(path)?;
        #[cfg(feature = "contradiction")]
        Self::register_default_singletons(&graph)?;
        Ok(Self { graph })
    }

    /// Store a structured fact with the current time as `valid_from`.
    ///
    /// Use this when you already know the structure of the fact.
    /// For unstructured text, use `remember()` (Phase 1).
    pub fn assert(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
    ) -> Result<FactId> {
        self.graph
            .assert_fact(subject, predicate, object, Utc::now())
    }

    /// Store a structured fact with idempotent retry semantics.
    ///
    /// Reusing the same `idempotency_key` returns the original fact ID and
    /// avoids duplicate writes.
    pub fn assert_idempotent(
        &self,
        idempotency_key: &str,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
    ) -> Result<FactId> {
        self.graph
            .assert_fact_idempotent(idempotency_key, subject, predicate, object, Utc::now())
    }

    /// Store a structured fact with explicit parameters.
    pub fn assert_with_params(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        params: AssertParams,
    ) -> Result<FactId> {
        self.graph
            .assert_fact(subject, predicate, object, params.valid_from)
    }

    /// Get all currently known facts about an entity (across all predicates).
    pub fn facts_about(&self, entity: &str) -> Result<Vec<Fact>> {
        self.graph.all_facts_about(entity)
    }

    /// Get what was known about an entity for a given predicate at a point in time.
    pub fn facts_about_at(
        &self,
        entity: &str,
        predicate: &str,
        at: DateTime<Utc>,
    ) -> Result<Vec<Fact>> {
        self.graph.facts_at(entity, predicate, at)
    }

    /// Full-text search across known facts.
    ///
    /// Delegates to core search functionality on the underlying temporal graph.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Fact>> {
        self.graph.search(query, limit)
    }

    /// Correct an existing fact by id, preserving temporal history.
    pub fn correct_fact(&self, fact_id: &FactId, new_value: impl Into<Value>) -> Result<FactId> {
        self.graph.correct_fact(fact_id, new_value, Utc::now())
    }

    // -----------------------------------------------------------------------
    // Contradiction detection
    // -----------------------------------------------------------------------

    /// Register common agent-memory singleton predicates.
    ///
    /// Called automatically from `open()` when the `contradiction` feature
    /// is enabled. These predicates typically have at most one active value
    /// per subject at any point in time.
    /// Register common agent-memory singleton predicates, preserving any
    /// existing policy the caller may have set (e.g. `Reject`).
    #[cfg(feature = "contradiction")]
    fn register_default_singletons(graph: &TemporalGraph) -> Result<()> {
        for predicate in &["works_at", "lives_in", "job_title", "email", "phone"] {
            if !graph.is_singleton_predicate(predicate)? {
                graph.register_singleton_predicate(predicate, ConflictPolicy::Warn)?;
            }
        }
        Ok(())
    }

    /// Assert a structured fact with contradiction checking.
    ///
    /// Returns the fact ID and any detected contradictions. The behavior
    /// depends on the predicate's conflict policy (set via
    /// [`register_singleton_predicate`] on the underlying graph).
    #[cfg(feature = "contradiction")]
    pub fn assert_checked(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
    ) -> Result<(FactId, Vec<Contradiction>)> {
        self.graph
            .assert_fact_checked(subject, predicate, object, Utc::now())
    }

    /// Audit a subject for contradictions across all registered singletons.
    ///
    /// Scans only the given subject's facts — cost scales with the
    /// subject's fact count, not the total database size.
    #[cfg(feature = "contradiction")]
    pub fn audit(&self, subject: &str) -> Result<Vec<Contradiction>> {
        let singleton_preds = self.graph.singleton_predicates()?;
        let mut contradictions = Vec::new();
        for predicate in &singleton_preds {
            contradictions.extend(self.graph.detect_contradictions(subject, predicate)?);
        }
        Ok(contradictions)
    }

    /// Store an unstructured memory episode as one fact.
    ///
    /// Subject is the `episode_id`, predicate is `"memory"`, object is `text`.
    pub fn remember(
        &self,
        text: &str,
        episode_id: &str,
        #[cfg(feature = "hybrid")] embedding: Option<Vec<f32>>,
        #[cfg(not(feature = "hybrid"))] _embedding: Option<Vec<f32>>,
    ) -> Result<FactId> {
        #[cfg(feature = "hybrid")]
        if let Some(emb) = embedding {
            return self.graph.assert_fact_with_embedding(
                episode_id,
                "memory",
                text.to_string(),
                Utc::now(),
                emb,
            );
        }

        self.graph
            .assert_fact(episode_id, "memory", text.to_string(), Utc::now())
    }

    /// Store an unstructured memory episode with idempotent retry semantics.
    ///
    /// Reusing `idempotency_key` returns the same fact ID and avoids duplicates.
    pub fn remember_idempotent(
        &self,
        idempotency_key: &str,
        text: &str,
        episode_id: &str,
    ) -> Result<FactId> {
        self.graph.assert_fact_idempotent(
            idempotency_key,
            episode_id,
            "memory",
            text.to_string(),
            Utc::now(),
        )
    }

    /// Retrieve memory facts by query.
    ///
    /// Convenience wrapper over [`recall_scored`](Self::recall_scored) that
    /// strips the score breakdowns. Use `recall_scored` when you need
    /// per-channel signal visibility.
    pub fn recall(
        &self,
        query: &str,
        query_embedding: Option<&[f32]>,
        limit: usize,
    ) -> Result<Vec<Fact>> {
        self.recall_scored(query, query_embedding, limit)
            .map(|scored| scored.into_iter().map(|(fact, _)| fact).collect())
    }

    /// Retrieve memory facts by query with per-channel signal breakdowns.
    ///
    /// Returns a `(Fact, RecallScore)` pair for each result. The result
    /// ordering is authoritative — the [`RecallScore`] explains per-channel
    /// contributions and fact confidence, not the final composite ranking
    /// score (see [`RecallScore`] docs for details).
    ///
    /// - **Hybrid path** (with embedding): returns [`RecallScore::Hybrid`]
    ///   with pre-rerank RRF channel contributions (text, vector) and
    ///   fact confidence.
    /// - **Text-only path** (no embedding): returns [`RecallScore::TextOnly`]
    ///   with ordinal rank, BM25 relevance score, and fact confidence.
    pub fn recall_scored(
        &self,
        query: &str,
        #[cfg(feature = "hybrid")] query_embedding: Option<&[f32]>,
        #[cfg(not(feature = "hybrid"))] _query_embedding: Option<&[f32]>,
        limit: usize,
    ) -> Result<Vec<(Fact, RecallScore)>> {
        #[cfg(feature = "hybrid")]
        if let Some(emb) = query_embedding {
            let params = HybridSearchParams {
                k: limit,
                ..HybridSearchParams::default()
            };
            let hits = self.graph.search_hybrid(query, emb, params, None)?;
            return Ok(hits
                .into_iter()
                .map(|(fact, breakdown)| {
                    let confidence = fact.confidence;
                    (fact, RecallScore::from_breakdown(&breakdown, confidence))
                })
                .collect());
        }

        let scored_facts = self.graph.search_scored(query, limit)?;
        Ok(scored_facts
            .into_iter()
            .enumerate()
            .map(|(i, (fact, bm25))| {
                let confidence = fact.confidence;
                (
                    fact,
                    RecallScore::TextOnly {
                        rank: i,
                        bm25_score: bm25,
                        confidence,
                    },
                )
            })
            .collect())
    }

    /// Build a token-bounded prompt context from recalled facts.
    ///
    /// Internally uses scored recall so results are ordered by relevance.
    /// The output format includes a retrieval score tag for transparency:
    ///
    /// ```text
    /// [2024-06-01] (0.032) alice · works_at · Acme            ← hybrid, full confidence
    /// [2024-06-01] (#1 bm25:4.21 conf:0.7) bob · lives_in · NYC  ← text-only, low confidence
    /// ```
    ///
    /// `query_embedding` is used only when the `hybrid` feature is enabled.
    /// Without it, the embedding is ignored and fulltext search is used.
    pub fn assemble_context(
        &self,
        query: &str,
        query_embedding: Option<&[f32]>,
        max_tokens: usize,
    ) -> Result<String> {
        let scored = self.recall_scored(query, query_embedding, 20)?;
        let char_budget = max_tokens.saturating_mul(4); // rough 1 token ≈ 4 chars
        let mut context = String::new();

        for (fact, score) in &scored {
            let object = match &fact.object {
                Value::Text(s) | Value::Entity(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Boolean(b) => b.to_string(),
            };
            // Only show confidence when it deviates from the default (1.0),
            // keeping the common case clean and highlighting uncertainty.
            let conf_tag = if (score.confidence() - 1.0).abs() > f32::EPSILON {
                format!(" conf:{:.1}", score.confidence())
            } else {
                String::new()
            };
            let line = format!(
                "[{}] ({}{}) {} · {} · {}\n",
                fact.valid_from.format("%Y-%m-%d"),
                score.display_tag(),
                conf_tag,
                fact.subject,
                fact.predicate,
                object
            );
            if context.len() + line.len() > char_budget {
                break;
            }
            context.push_str(&line);
        }

        Ok(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn open_temp_memory() -> (AgentMemory, NamedTempFile) {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_str().unwrap().to_string();
        let memory = AgentMemory::open(&path).unwrap();
        (memory, file)
    }

    #[test]
    fn assert_and_retrieve() {
        let (memory, _tmp) = open_temp_memory();
        memory.assert("alice", "works_at", "Acme").unwrap();

        let facts = memory.facts_about("alice").unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].predicate, "works_at");
    }

    #[test]
    fn multiple_facts_about_entity() {
        let (memory, _tmp) = open_temp_memory();

        memory
            .assert("freya", "attends", "Sunrise Primary")
            .unwrap();
        memory.assert("freya", "has_ehcp", true).unwrap();
        memory.assert("freya", "key_worker", "Sarah Jones").unwrap();

        let facts = memory.facts_about("freya").unwrap();
        assert_eq!(facts.len(), 3);
    }

    #[test]
    fn test_remember_stores_fact() {
        let (mem, _tmp) = open_temp_memory();
        let id = mem.remember("Alice loves Rust", "ep-001", None).unwrap();
        assert_eq!(id.0.len(), 26);

        let facts = mem.facts_about("ep-001").unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].subject, "ep-001");
        assert_eq!(facts[0].predicate, "memory");
        assert!(matches!(&facts[0].object, Value::Text(t) if t == "Alice loves Rust"));
    }

    #[test]
    fn test_assert_idempotent_dedupes_same_key() {
        let (mem, _tmp) = open_temp_memory();
        let first = mem
            .assert_idempotent("evt-1", "alice", "works_at", "Acme")
            .unwrap();
        let second = mem
            .assert_idempotent("evt-1", "alice", "works_at", "Acme")
            .unwrap();
        assert_eq!(first, second);

        let facts = mem.facts_about("alice").unwrap();
        assert_eq!(facts.len(), 1);
    }

    #[test]
    fn test_remember_idempotent_dedupes_same_key() {
        let (mem, _tmp) = open_temp_memory();
        let first = mem
            .remember_idempotent("evt-memory-1", "Alice loves Rust", "ep-001")
            .unwrap();
        let second = mem
            .remember_idempotent("evt-memory-1", "Alice loves Rust", "ep-001")
            .unwrap();
        assert_eq!(first, second);

        let facts = mem.facts_about("ep-001").unwrap();
        assert_eq!(facts.len(), 1);
    }

    #[test]
    fn test_recall_returns_matching_facts() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Alice loves Rust programming", "ep-001", None)
            .unwrap();
        mem.remember("Bob prefers Python for data science", "ep-002", None)
            .unwrap();

        let results = mem.recall("Rust", None, 5).unwrap();
        assert!(!results.is_empty(), "should find Rust-related facts");
        let has_rust = results
            .iter()
            .any(|f| matches!(&f.object, Value::Text(t) if t.contains("Rust")));
        assert!(has_rust);
    }

    #[test]
    fn test_assemble_context_returns_string() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Alice is a Rust expert", "ep-001", None)
            .unwrap();
        mem.remember("Bob is a Python expert", "ep-002", None)
            .unwrap();

        let ctx = mem.assemble_context("expert", None, 500).unwrap();
        assert!(!ctx.is_empty(), "context should not be empty");
        assert!(
            ctx.contains("expert"),
            "context should contain relevant facts"
        );
    }

    #[test]
    fn test_assemble_context_respects_token_limit() {
        let (mem, _tmp) = open_temp_memory();
        for i in 0..20 {
            mem.remember(
                &format!("fact number {} is quite long and wordy", i),
                &format!("ep-{}", i),
                None,
            )
            .unwrap();
        }
        let ctx = mem.assemble_context("fact", None, 50).unwrap();
        assert!(ctx.len() <= 220, "context should respect max_tokens");
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn test_remember_with_embedding() {
        let (mem, _tmp) = open_temp_memory();
        let id = mem
            .remember("Bob likes Python", "ep-002", Some(vec![0.1f32, 0.2, 0.3]))
            .unwrap();
        assert_eq!(id.0.len(), 26);
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn test_recall_with_query_embedding() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Rust systems", "ep-rust", Some(vec![1.0f32, 0.0]))
            .unwrap();
        mem.remember("Python notebooks", "ep-py", Some(vec![0.0f32, 1.0]))
            .unwrap();

        let hits = mem.recall("language", Some(&[1.0, 0.0]), 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].subject, "ep-rust");
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn assert_checked_detects_contradiction() {
        let (mem, _tmp) = open_temp_memory();
        // "works_at" is auto-registered as singleton by open()
        mem.assert("alice", "works_at", "Acme").unwrap();
        let (id, contradictions) = mem
            .assert_checked("alice", "works_at", "Beta Corp")
            .unwrap();
        assert!(!id.0.is_empty());
        assert_eq!(contradictions.len(), 1);
        assert_eq!(contradictions[0].predicate, "works_at");
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn default_singletons_registered() {
        let (mem, _tmp) = open_temp_memory();
        // Verify that auto-registered singletons trigger contradiction detection.
        mem.assert("bob", "lives_in", "London").unwrap();
        let (_, contradictions) = mem.assert_checked("bob", "lives_in", "Paris").unwrap();
        assert_eq!(
            contradictions.len(),
            1,
            "lives_in should be a registered singleton"
        );
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn audit_returns_contradictions_for_subject() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert("alice", "works_at", "Acme").unwrap();
        mem.assert("alice", "works_at", "Beta").unwrap();
        mem.assert("bob", "works_at", "Gamma").unwrap(); // No contradiction for bob.

        let contradictions = mem.audit("alice").unwrap();
        assert_eq!(contradictions.len(), 1);
        assert_eq!(contradictions[0].subject, "alice");

        let bob_contradictions = mem.audit("bob").unwrap();
        assert!(bob_contradictions.is_empty());
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn reject_policy_survives_reopen() {
        // Regression: open() must not overwrite a pre-set Reject policy with Warn.
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_str().unwrap().to_string();

        // First open: set works_at to Reject.
        {
            let graph = kronroe::TemporalGraph::open(&path).unwrap();
            graph
                .register_singleton_predicate("works_at", ConflictPolicy::Reject)
                .unwrap();
            graph
                .assert_fact("alice", "works_at", "Acme", Utc::now())
                .unwrap();
        }

        // Second open via AgentMemory: default registration must not downgrade.
        let mem = AgentMemory::open(&path).unwrap();
        let result = mem.assert_checked("alice", "works_at", "Beta Corp");
        assert!(
            result.is_err(),
            "Reject policy should survive AgentMemory::open() reopen"
        );
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn test_recall_hybrid_uses_text_and_vector_signals() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("rare-rust-token", "ep-rust", Some(vec![1.0f32, 0.0]))
            .unwrap();
        mem.remember("completely different", "ep-py", Some(vec![0.0f32, 1.0]))
            .unwrap();

        // Query text matches only ep-rust, vector matches only ep-py.
        // With hybrid fusion enabled, both signals are used and ep-rust should
        // still surface in a top-1 tie-break deterministic setup.
        let hits = mem.recall("rare-rust-token", Some(&[0.0, 1.0]), 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].subject, "ep-rust");
    }

    // -------------------------------------------------------------------
    // Explainable recall tests
    // -------------------------------------------------------------------

    #[test]
    fn recall_scored_text_only_returns_ranks_and_bm25() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Alice loves Rust programming", "ep-001", None)
            .unwrap();
        mem.remember("Bob also enjoys Rust deeply", "ep-002", None)
            .unwrap();

        let scored = mem.recall_scored("Rust", None, 5).unwrap();
        assert!(!scored.is_empty(), "should find Rust-related facts");

        // Every result should be TextOnly with sequential ranks and positive BM25.
        for (i, (_fact, score)) in scored.iter().enumerate() {
            match score {
                RecallScore::TextOnly {
                    rank,
                    bm25_score,
                    confidence,
                    ..
                } => {
                    assert_eq!(*rank, i);
                    assert!(
                        *bm25_score > 0.0,
                        "BM25 should be positive, got {bm25_score}"
                    );
                    assert!(
                        (*confidence - 1.0).abs() < f32::EPSILON,
                        "default confidence should be 1.0"
                    );
                }
                RecallScore::Hybrid { .. } => {
                    panic!("expected TextOnly variant without embedding")
                }
            }
        }
    }

    #[test]
    fn recall_scored_bm25_higher_for_better_match() {
        let (mem, _tmp) = open_temp_memory();
        // "Rust Rust Rust" should score higher than "Rust" for query "Rust".
        mem.remember("Rust Rust Rust programming language", "ep-strong", None)
            .unwrap();
        mem.remember("I once heard of Rust somewhere", "ep-weak", None)
            .unwrap();

        let scored = mem.recall_scored("Rust", None, 5).unwrap();
        assert!(scored.len() >= 2);

        // First result should have higher or equal BM25 than second.
        let bm25_first = match scored[0].1 {
            RecallScore::TextOnly { bm25_score, .. } => bm25_score,
            _ => panic!("expected TextOnly"),
        };
        let bm25_second = match scored[1].1 {
            RecallScore::TextOnly { bm25_score, .. } => bm25_score,
            _ => panic!("expected TextOnly"),
        };
        assert!(
            bm25_first >= bm25_second,
            "first result should have higher BM25: {bm25_first} vs {bm25_second}"
        );
    }

    #[test]
    fn recall_scored_preserves_fact_content() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Kronroe is a temporal graph database", "ep-001", None)
            .unwrap();

        let scored = mem.recall_scored("temporal", None, 5).unwrap();
        assert_eq!(scored.len(), 1);

        let (fact, _score) = &scored[0];
        assert_eq!(fact.subject, "ep-001");
        assert_eq!(fact.predicate, "memory");
        assert!(matches!(&fact.object, Value::Text(t) if t.contains("temporal")));
    }

    #[test]
    fn recall_score_confidence_accessor() {
        // Test the convenience accessor works for both variants.
        let text = RecallScore::TextOnly {
            rank: 0,
            bm25_score: 1.0,
            confidence: 0.8,
        };
        assert!((text.confidence() - 0.8).abs() < f32::EPSILON);
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn recall_score_confidence_accessor_hybrid() {
        let hybrid = RecallScore::Hybrid {
            rrf_score: 0.1,
            text_contrib: 0.05,
            vector_contrib: 0.05,
            confidence: 0.9,
        };
        assert!((hybrid.confidence() - 0.9).abs() < f32::EPSILON);
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn recall_scored_hybrid_returns_breakdown() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember(
            "Rust systems programming",
            "ep-rust",
            Some(vec![1.0f32, 0.0]),
        )
        .unwrap();
        mem.remember("Python data science", "ep-py", Some(vec![0.0f32, 1.0]))
            .unwrap();

        let scored = mem.recall_scored("Rust", Some(&[1.0, 0.0]), 2).unwrap();
        assert!(!scored.is_empty());

        // All results should be Hybrid variant with non-negative scores and confidence.
        for (_fact, score) in &scored {
            match score {
                RecallScore::Hybrid {
                    rrf_score,
                    text_contrib,
                    vector_contrib,
                    confidence,
                    ..
                } => {
                    assert!(
                        *rrf_score >= 0.0,
                        "RRF score should be non-negative, got {rrf_score}"
                    );
                    assert!(
                        *text_contrib >= 0.0,
                        "text contrib should be non-negative, got {text_contrib}"
                    );
                    assert!(
                        *vector_contrib >= 0.0,
                        "vector contrib should be non-negative, got {vector_contrib}"
                    );
                    assert!(
                        (*confidence - 1.0).abs() < f32::EPSILON,
                        "default confidence should be 1.0"
                    );
                }
                RecallScore::TextOnly { .. } => {
                    panic!("expected Hybrid variant with embedding")
                }
            }
        }
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn recall_scored_hybrid_text_dominant_has_higher_text_contrib() {
        let (mem, _tmp) = open_temp_memory();
        // Store with embedding [1, 0], query with orthogonal vector [0, 1]
        // but matching text — so text_contrib should dominate.
        mem.remember(
            "unique-xyzzy-token for testing",
            "ep-text",
            Some(vec![1.0f32, 0.0]),
        )
        .unwrap();

        let scored = mem
            .recall_scored("unique-xyzzy-token", Some(&[0.0, 1.0]), 1)
            .unwrap();
        assert_eq!(scored.len(), 1);

        match &scored[0].1 {
            RecallScore::Hybrid {
                text_contrib,
                vector_contrib,
                ..
            } => {
                assert!(
                    text_contrib > vector_contrib,
                    "text should dominate when query text matches but vector is orthogonal: \
                     text={text_contrib}, vector={vector_contrib}"
                );
            }
            _ => panic!("expected Hybrid variant"),
        }
    }

    #[test]
    fn recall_score_display_tag() {
        let text_score = RecallScore::TextOnly {
            rank: 0,
            bm25_score: 4.21,
            confidence: 1.0,
        };
        assert_eq!(text_score.display_tag(), "#1 bm25:4.21");

        let text_score_5 = RecallScore::TextOnly {
            rank: 4,
            bm25_score: 1.50,
            confidence: 1.0,
        };
        assert_eq!(text_score_5.display_tag(), "#5 bm25:1.50");
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn recall_score_display_tag_hybrid() {
        let hybrid_score = RecallScore::Hybrid {
            rrf_score: 0.0325,
            text_contrib: 0.02,
            vector_contrib: 0.0125,
            confidence: 1.0,
        };
        assert_eq!(hybrid_score.display_tag(), "0.033");
    }

    #[test]
    fn assemble_context_includes_score_tag() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Alice is a Rust expert", "ep-001", None)
            .unwrap();

        let ctx = mem.assemble_context("Rust", None, 500).unwrap();
        assert!(!ctx.is_empty());
        // Text-only path: score tag should include rank and BM25 like "(#1 bm25:X.XX)".
        assert!(
            ctx.contains("(#1 bm25:"),
            "context should contain text-only rank+bm25 tag, got: {ctx}"
        );
    }

    #[test]
    fn assemble_context_omits_confidence_at_default() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Alice is a Rust expert", "ep-001", None)
            .unwrap();

        let ctx = mem.assemble_context("Rust", None, 500).unwrap();
        // Default confidence (1.0) should NOT show "conf:" — keep output clean.
        assert!(
            !ctx.contains("conf:"),
            "default confidence should not appear in context, got: {ctx}"
        );
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn assemble_context_hybrid_includes_rrf_score() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Rust systems", "ep-rust", Some(vec![1.0f32, 0.0]))
            .unwrap();

        let ctx = mem
            .assemble_context("Rust", Some(&[1.0, 0.0]), 500)
            .unwrap();
        assert!(!ctx.is_empty());
        // Hybrid path: score tag should be a decimal like "(0.032)".
        assert!(
            ctx.contains("(0."),
            "context should contain hybrid RRF score tag, got: {ctx}"
        );
    }
}
