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
use kronroe::{HybridScoreBreakdown, HybridSearchParams, TemporalIntent, TemporalOperator};
use std::collections::HashSet;

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
        /// Effective confidence after uncertainty model (age decay × source weight).
        /// `None` when uncertainty modeling is disabled.
        effective_confidence: Option<f32>,
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
        /// Effective confidence after uncertainty model (age decay × source weight).
        /// `None` when uncertainty modeling is disabled.
        effective_confidence: Option<f32>,
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

    /// The effective confidence after uncertainty model processing.
    ///
    /// Returns `None` when uncertainty modeling is disabled. When `Some`, this
    /// reflects: `base_confidence × age_decay × source_weight`.
    pub fn effective_confidence(&self) -> Option<f32> {
        match self {
            RecallScore::Hybrid {
                effective_confidence,
                ..
            }
            | RecallScore::TextOnly {
                effective_confidence,
                ..
            } => *effective_confidence,
        }
    }

    /// Convert a [`HybridScoreBreakdown`] from the core engine into a
    /// [`RecallScore::Hybrid`], incorporating the fact's confidence.
    #[cfg(feature = "hybrid")]
    fn from_breakdown(
        b: &HybridScoreBreakdown,
        confidence: f32,
        effective_confidence: Option<f32>,
    ) -> Self {
        RecallScore::Hybrid {
            rrf_score: b.final_score,
            text_contrib: b.text_rrf_contrib,
            vector_contrib: b.vector_rrf_contrib,
            confidence,
            effective_confidence,
        }
    }
}

/// Strategy for deciding which confidence signal drives filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfidenceFilterMode {
    /// Filter using raw fact confidence.
    Base,
    /// Filter using effective confidence (uncertainty-aware).
    ///
    /// Only available when the `uncertainty` feature is enabled. Attempting to
    /// construct this variant without the feature is a compile-time error.
    #[cfg(feature = "uncertainty")]
    Effective,
}

/// Options for recall queries, controlling retrieval behaviour.
///
/// Use [`RecallOptions::new`] to create with defaults, then chain builder
/// methods to customise. The `#[non_exhaustive]` attribute ensures new
/// fields can be added without breaking existing callers.
///
/// ```rust
/// use kronroe_agent_memory::RecallOptions;
///
/// let opts = RecallOptions::new("what does alice do?")
///     .with_limit(5)
///     .with_min_confidence(0.6)
///     .with_max_scored_rows(2_048);
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RecallOptions<'a> {
    /// The search query text.
    pub query: &'a str,
    /// Optional embedding for hybrid retrieval.
    pub query_embedding: Option<&'a [f32]>,
    /// Maximum number of results to return (default: 10).
    pub limit: usize,
    /// Minimum confidence threshold — facts below this are filtered out.
    pub min_confidence: Option<f32>,
    /// Which confidence signal to use when applying `min_confidence`.
    pub confidence_filter_mode: ConfidenceFilterMode,
    /// Maximum rows fetched per confidence-filtered recall batch (default: 4,096).
    ///
    /// Raising this increases recall depth at the cost of larger per-call work.
    /// Lowering it improves bounded latency but may reduce results if strong hits
    /// appear deeper in the result ranking.
    pub max_scored_rows: usize,
    /// Whether to run hybrid retrieval when an embedding is provided.
    ///
    /// Defaults to `false` in options helpers to preserve the existing
    /// `recall_*` method ergonomics.
    #[cfg(feature = "hybrid")]
    pub use_hybrid: bool,
    /// Temporal intent for hybrid reranking.
    #[cfg(feature = "hybrid")]
    pub temporal_intent: TemporalIntent,
    /// Temporal operator used when intent is [`TemporalIntent::HistoricalPoint`].
    #[cfg(feature = "hybrid")]
    pub temporal_operator: TemporalOperator,
}

const DEFAULT_MAX_SCORED_ROWS: usize = 4_096;

impl<'a> RecallOptions<'a> {
    /// Create options with defaults: limit=10, no embedding, no confidence filter.
    pub fn new(query: &'a str) -> Self {
        Self {
            query,
            query_embedding: None,
            limit: 10,
            min_confidence: None,
            confidence_filter_mode: ConfidenceFilterMode::Base,
            max_scored_rows: DEFAULT_MAX_SCORED_ROWS,
            #[cfg(feature = "hybrid")]
            use_hybrid: false,
            #[cfg(feature = "hybrid")]
            temporal_intent: TemporalIntent::Timeless,
            #[cfg(feature = "hybrid")]
            temporal_operator: TemporalOperator::Current,
        }
    }

    /// Set the query embedding for hybrid retrieval.
    pub fn with_embedding(mut self, embedding: &'a [f32]) -> Self {
        self.query_embedding = Some(embedding);
        self
    }

    /// Set the maximum number of results.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Set a minimum confidence threshold to filter low-confidence facts.
    pub fn with_min_confidence(mut self, min: f32) -> Self {
        self.min_confidence = Some(min);
        self.confidence_filter_mode = ConfidenceFilterMode::Base;
        self
    }

    /// Set a minimum effective-confidence threshold to filter low-confidence facts.
    ///
    /// Effective confidence is calculated as:
    /// `base_confidence × age_decay × source_weight`.
    ///
    /// Only available when the `uncertainty` feature is enabled.
    #[cfg(feature = "uncertainty")]
    pub fn with_min_effective_confidence(mut self, min: f32) -> Self {
        self.min_confidence = Some(min);
        self.confidence_filter_mode = ConfidenceFilterMode::Effective;
        self
    }

    /// Set the maximum rows fetched per batch while applying confidence filters.
    ///
    /// Must be at least 1; `recall_scored_with_options` returns a `Search` error
    /// for non-positive values.
    pub fn with_max_scored_rows(mut self, max_scored_rows: usize) -> Self {
        self.max_scored_rows = max_scored_rows;
        self
    }

    /// Enable or disable hybrid reranking when a query embedding is provided.
    ///
    /// Hybrid is only available when the `hybrid` feature is enabled.
    /// The default behavior in options-based recall is text-only unless this
    /// flag is explicitly enabled.
    #[cfg(feature = "hybrid")]
    pub fn with_hybrid(mut self, enabled: bool) -> Self {
        self.use_hybrid = enabled;
        self
    }

    /// Provide a temporal recall intent for hybrid reranking.
    ///
    /// No effect without the `hybrid` feature.
    #[cfg(feature = "hybrid")]
    pub fn with_temporal_intent(mut self, intent: TemporalIntent) -> Self {
        self.temporal_intent = intent;
        self
    }

    /// Provide a temporal operator for historical intent resolution.
    ///
    /// No effect without the `hybrid` feature.
    #[cfg(feature = "hybrid")]
    pub fn with_temporal_operator(mut self, operator: TemporalOperator) -> Self {
        self.temporal_operator = operator;
        self
    }
}

fn normalize_min_confidence(min_confidence: f32) -> Result<f32> {
    if !min_confidence.is_finite() {
        return Err(Error::Search(format!(
            "minimum confidence must be a finite number in [0.0, 1.0], got {min_confidence}"
        )));
    }

    Ok(min_confidence.clamp(0.0, 1.0))
}

fn normalize_fact_confidence(confidence: f32) -> Result<f32> {
    if !confidence.is_finite() {
        return Err(Error::Search(
            "fact confidence must be finite and in [0.0, 1.0], got non-finite value".to_string(),
        ));
    }
    Ok(confidence.clamp(0.0, 1.0))
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

/// A paired correction event linking the invalidated fact and its replacement.
#[derive(Debug, Clone)]
pub struct FactCorrection {
    pub old_fact: Fact,
    pub new_fact: Fact,
}

/// Confidence movement between two related facts.
#[derive(Debug, Clone)]
pub struct ConfidenceShift {
    pub from_fact_id: FactId,
    pub to_fact_id: FactId,
    pub from_confidence: f32,
    pub to_confidence: f32,
}

/// Summary of changes for one entity since a timestamp.
#[derive(Debug, Clone)]
pub struct WhatChangedReport {
    pub entity: String,
    pub since: DateTime<Utc>,
    pub predicate_filter: Option<String>,
    pub new_facts: Vec<Fact>,
    pub invalidated_facts: Vec<Fact>,
    pub corrections: Vec<FactCorrection>,
    pub confidence_shifts: Vec<ConfidenceShift>,
}

/// Operational memory-quality snapshot for one entity.
#[derive(Debug, Clone)]
pub struct MemoryHealthReport {
    pub entity: String,
    pub generated_at: DateTime<Utc>,
    pub predicate_filter: Option<String>,
    pub total_fact_count: usize,
    pub active_fact_count: usize,
    pub low_confidence_facts: Vec<Fact>,
    pub stale_high_impact_facts: Vec<Fact>,
    pub contradiction_count: usize,
    pub recommended_actions: Vec<String>,
}

/// Decision-ready recall result shaped around a concrete user task.
#[derive(Debug, Clone)]
pub struct RecallForTaskReport {
    pub task: String,
    pub subject: Option<String>,
    pub generated_at: DateTime<Utc>,
    pub horizon_days: i64,
    pub query_used: String,
    pub key_facts: Vec<Fact>,
    pub low_confidence_count: usize,
    pub stale_high_impact_count: usize,
    pub contradiction_count: usize,
    pub watchouts: Vec<String>,
    pub recommended_next_checks: Vec<String>,
}

fn is_high_impact_predicate(predicate: &str) -> bool {
    matches!(
        predicate,
        "works_at" | "lives_in" | "job_title" | "email" | "phone"
    )
}

const CORRECTION_LINK_TOLERANCE_SECONDS: i64 = 2;

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
        #[cfg(feature = "uncertainty")]
        Self::register_default_volatilities(&graph)?;
        Ok(Self { graph })
    }

    /// Create an in-memory agent memory store.
    ///
    /// Useful for tests, WASM/browser bindings, and ephemeral workloads.
    pub fn open_in_memory() -> Result<Self> {
        let graph = TemporalGraph::open_in_memory()?;
        #[cfg(feature = "contradiction")]
        Self::register_default_singletons(&graph)?;
        #[cfg(feature = "uncertainty")]
        Self::register_default_volatilities(&graph)?;
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

    /// Store a structured fact with idempotent retry semantics and explicit timing.
    pub fn assert_idempotent_with_params(
        &self,
        idempotency_key: &str,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        params: AssertParams,
    ) -> Result<FactId> {
        self.graph.assert_fact_idempotent(
            idempotency_key,
            subject,
            predicate,
            object,
            params.valid_from,
        )
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

    /// Get currently valid facts for one `(entity, predicate)` pair.
    pub fn current_facts(&self, entity: &str, predicate: &str) -> Result<Vec<Fact>> {
        self.graph.current_facts(entity, predicate)
    }

    /// Return what changed for an entity since a given timestamp.
    ///
    /// This is intentionally decision-oriented: it groups newly-recorded facts,
    /// recently invalidated facts, inferred correction pairs, and confidence shifts.
    pub fn what_changed(
        &self,
        entity: &str,
        since: DateTime<Utc>,
        predicate_filter: Option<&str>,
    ) -> Result<WhatChangedReport> {
        let mut facts = self.graph.all_facts_about(entity)?;
        if let Some(predicate) = predicate_filter {
            facts.retain(|fact| fact.predicate == predicate);
        }

        let mut new_facts: Vec<Fact> = facts
            .iter()
            .filter(|fact| fact.recorded_at >= since)
            .cloned()
            .collect();
        new_facts.sort_by_key(|fact| fact.recorded_at);

        let mut invalidated_facts: Vec<Fact> = facts
            .iter()
            .filter(|fact| {
                fact.expired_at
                    .map(|expired| expired >= since)
                    .unwrap_or(false)
                    || fact
                        .valid_to
                        .map(|valid_to| valid_to >= since)
                        .unwrap_or(false)
            })
            .cloned()
            .collect();
        invalidated_facts.sort_by_key(|fact| {
            fact.expired_at
                .or(fact.valid_to)
                .unwrap_or(fact.recorded_at)
        });

        let mut corrections = Vec::new();
        let mut confidence_shifts = Vec::new();

        for new_fact in &new_facts {
            let exact_match = facts
                .iter()
                .filter(|old| {
                    old.id != new_fact.id
                        && old.subject == new_fact.subject
                        && old.predicate == new_fact.predicate
                        && old.expired_at == Some(new_fact.valid_from)
                        && old.recorded_at <= new_fact.recorded_at
                })
                .max_by_key(|old| old.recorded_at);

            let old_match = exact_match.or_else(|| {
                facts
                    .iter()
                    .filter(|old| {
                        old.id != new_fact.id
                            && old.subject == new_fact.subject
                            && old.predicate == new_fact.predicate
                            && old.recorded_at <= new_fact.recorded_at
                    })
                    .filter_map(|old| {
                        old.expired_at.map(|expired| {
                            (old, (expired - new_fact.valid_from).num_seconds().abs())
                        })
                    })
                    .filter(|(_, delta_seconds)| {
                        *delta_seconds <= CORRECTION_LINK_TOLERANCE_SECONDS
                    })
                    .min_by(|(left_fact, left_delta), (right_fact, right_delta)| {
                        left_delta
                            .cmp(right_delta)
                            .then_with(|| right_fact.recorded_at.cmp(&left_fact.recorded_at))
                    })
                    .map(|(old, _)| old)
            });

            if let Some(old_fact) = old_match {
                corrections.push(FactCorrection {
                    old_fact: old_fact.clone(),
                    new_fact: new_fact.clone(),
                });
                if (old_fact.confidence - new_fact.confidence).abs() > f32::EPSILON {
                    confidence_shifts.push(ConfidenceShift {
                        from_fact_id: old_fact.id.clone(),
                        to_fact_id: new_fact.id.clone(),
                        from_confidence: old_fact.confidence,
                        to_confidence: new_fact.confidence,
                    });
                }
            }
        }

        corrections.sort_by_key(|pair| pair.new_fact.recorded_at);
        confidence_shifts.sort_by(|left, right| left.to_fact_id.0.cmp(&right.to_fact_id.0));

        Ok(WhatChangedReport {
            entity: entity.to_string(),
            since,
            predicate_filter: predicate_filter.map(str::to_string),
            new_facts,
            invalidated_facts,
            corrections,
            confidence_shifts,
        })
    }

    /// Produce a health report for one entity's memory state.
    ///
    /// The report flags low-confidence active facts, stale high-impact facts,
    /// and contradiction counts (when contradiction support is enabled).
    pub fn memory_health(
        &self,
        entity: &str,
        predicate_filter: Option<&str>,
        low_confidence_threshold: f32,
        stale_after_days: i64,
    ) -> Result<MemoryHealthReport> {
        let threshold = normalize_min_confidence(low_confidence_threshold)?;
        let stale_days = stale_after_days.max(0);

        let mut facts = self.graph.all_facts_about(entity)?;
        if let Some(predicate) = predicate_filter {
            facts.retain(|fact| fact.predicate == predicate);
        }

        let generated_at = Utc::now();
        let stale_cutoff = generated_at - chrono::Duration::days(stale_days);
        let active_facts: Vec<Fact> = facts
            .iter()
            .filter(|fact| fact.is_currently_valid())
            .cloned()
            .collect();

        let mut low_confidence_facts: Vec<Fact> = active_facts
            .iter()
            .filter(|fact| fact.confidence < threshold)
            .cloned()
            .collect();
        low_confidence_facts.sort_by(|left, right| {
            left.confidence
                .partial_cmp(&right.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut stale_high_impact_facts: Vec<Fact> = active_facts
            .iter()
            .filter(|fact| {
                is_high_impact_predicate(&fact.predicate) && fact.valid_from <= stale_cutoff
            })
            .cloned()
            .collect();
        stale_high_impact_facts.sort_by_key(|fact| fact.valid_from);

        #[cfg(feature = "contradiction")]
        let contradiction_count = if let Some(predicate) = predicate_filter {
            self.graph.detect_contradictions(entity, predicate)?.len()
        } else {
            self.audit(entity)?.len()
        };
        #[cfg(not(feature = "contradiction"))]
        let contradiction_count = 0usize;

        let mut recommended_actions = Vec::new();
        if contradiction_count > 0 {
            recommended_actions.push(format!(
                "Resolve {contradiction_count} contradiction(s) before relying on this memory."
            ));
        }
        if !low_confidence_facts.is_empty() {
            recommended_actions.push(format!(
                "Review {} low-confidence active fact(s).",
                low_confidence_facts.len()
            ));
        }
        if !stale_high_impact_facts.is_empty() {
            recommended_actions.push(format!(
                "Refresh {} stale high-impact fact(s).",
                stale_high_impact_facts.len()
            ));
        }
        if recommended_actions.is_empty() {
            recommended_actions.push("No immediate memory health issues detected.".to_string());
        }

        Ok(MemoryHealthReport {
            entity: entity.to_string(),
            generated_at,
            predicate_filter: predicate_filter.map(str::to_string),
            total_fact_count: facts.len(),
            active_fact_count: active_facts.len(),
            low_confidence_facts,
            stale_high_impact_facts,
            contradiction_count,
            recommended_actions,
        })
    }

    /// Build task-focused recall output that is immediately useful for planning
    /// and execution-oriented workflows.
    pub fn recall_for_task(
        &self,
        task: &str,
        subject: Option<&str>,
        now: Option<DateTime<Utc>>,
        horizon_days: Option<i64>,
        limit: usize,
        #[cfg(feature = "hybrid")] query_embedding: Option<&[f32]>,
        #[cfg(not(feature = "hybrid"))] _query_embedding: Option<&[f32]>,
    ) -> Result<RecallForTaskReport> {
        if limit == 0 {
            return Err(Error::Search(
                "recall_for_task limit must be >= 1".to_string(),
            ));
        }

        let generated_at = now.unwrap_or_else(Utc::now);
        let horizon_days = horizon_days.unwrap_or(30).max(1);
        let subject = subject.and_then(|raw| {
            let trimmed = raw.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        });

        let query_used = if let Some(subject) = subject {
            format!("{task} {subject}")
        } else {
            task.to_string()
        };

        let opts = RecallOptions::new(&query_used).with_limit(limit);
        #[cfg(feature = "hybrid")]
        let opts = if let Some(embedding) = query_embedding {
            opts.with_embedding(embedding).with_hybrid(true)
        } else {
            opts
        };
        #[cfg(not(feature = "hybrid"))]
        if _query_embedding.is_some() {
            return Err(Error::Search(
                "query_embedding requires the hybrid feature".to_string(),
            ));
        }

        let mut scored = self.recall_scored_with_options(&opts)?;
        if let Some(subject) = subject {
            scored.retain(|(fact, _)| fact.subject == subject);
        }

        let key_facts: Vec<Fact> = scored.into_iter().map(|(fact, _)| fact).collect();
        let low_confidence_count = key_facts
            .iter()
            .filter(|fact| fact.confidence < 0.7)
            .count();

        let stale_cutoff = generated_at - chrono::Duration::days(horizon_days);
        let stale_high_impact_count = key_facts
            .iter()
            .filter(|fact| {
                is_high_impact_predicate(&fact.predicate) && fact.valid_from <= stale_cutoff
            })
            .count();

        #[cfg(feature = "contradiction")]
        let contradiction_count = if let Some(subject) = subject {
            self.audit(subject)?.len()
        } else {
            0
        };
        #[cfg(not(feature = "contradiction"))]
        let contradiction_count = 0usize;

        let mut watchouts = Vec::new();
        if key_facts.is_empty() {
            watchouts.push("No matching facts were found for this task context.".to_string());
        }
        if low_confidence_count > 0 {
            watchouts.push(format!(
                "{low_confidence_count} key fact(s) are low confidence (< 0.7)."
            ));
        }
        if stale_high_impact_count > 0 {
            watchouts.push(format!(
                "{stale_high_impact_count} high-impact key fact(s) are stale for the selected horizon."
            ));
        }
        if contradiction_count > 0 {
            watchouts.push(format!(
                "{contradiction_count} contradiction(s) exist for the subject."
            ));
        }

        let mut recommended_next_checks = Vec::new();
        if key_facts.is_empty() {
            recommended_next_checks
                .push("Ask a clarifying follow-up question before acting.".to_string());
        }
        if low_confidence_count > 0 {
            recommended_next_checks
                .push("Verify low-confidence facts with the latest source of truth.".to_string());
        }
        if stale_high_impact_count > 0 {
            recommended_next_checks.push(
                "Refresh stale high-impact facts (employment, location, role, contact)."
                    .to_string(),
            );
        }
        if contradiction_count > 0 {
            recommended_next_checks
                .push("Resolve contradictions before generating irreversible actions.".to_string());
        }
        if recommended_next_checks.is_empty() {
            recommended_next_checks
                .push("Proceed with the top facts and monitor for new updates.".to_string());
        }

        Ok(RecallForTaskReport {
            task: task.to_string(),
            subject: subject.map(str::to_string),
            generated_at,
            horizon_days,
            query_used,
            key_facts,
            low_confidence_count,
            stale_high_impact_count,
            contradiction_count,
            watchouts,
            recommended_next_checks,
        })
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

    /// Invalidate an existing fact by id, recording the current time as
    /// the transaction end.
    pub fn invalidate_fact(&self, fact_id: &FactId) -> Result<()> {
        self.graph.invalidate_fact(fact_id, Utc::now())
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

    /// Retrieve memory facts by query with an explicit minimum confidence threshold.
    ///
    /// This shares the same filtering semantics as
    /// [`recall_scored_with_options`](Self::recall_scored_with_options), including
    /// confidence filtering before final result truncation.
    ///
    /// ```rust,no_run
    /// use kronroe_agent_memory::AgentMemory;
    ///
    /// let memory = AgentMemory::open("./agent.kronroe").unwrap();
    /// memory.assert_with_confidence("alice", "works_at", "Acme", 0.95).unwrap();
    /// memory.assert_with_confidence("alice", "worked_at", "Startup", 0.42).unwrap();
    ///
    /// let facts = memory
    ///     .recall_with_min_confidence("alice", None, 10, 0.9)
    ///     .unwrap();
    /// assert!(facts.iter().all(|f| f.confidence >= 0.9));
    /// ```
    pub fn recall_with_min_confidence(
        &self,
        query: &str,
        #[cfg(feature = "hybrid")] query_embedding: Option<&[f32]>,
        #[cfg(not(feature = "hybrid"))] _query_embedding: Option<&[f32]>,
        limit: usize,
        min_confidence: f32,
    ) -> Result<Vec<Fact>> {
        let opts = RecallOptions::new(query)
            .with_limit(limit)
            .with_min_confidence(min_confidence);

        #[cfg(feature = "hybrid")]
        let opts = if let Some(embedding) = query_embedding {
            opts.with_embedding(embedding).with_hybrid(true)
        } else {
            opts
        };

        self.recall_with_options(&opts)
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
        let mut opts = RecallOptions::new(query).with_limit(limit);
        #[cfg(not(feature = "hybrid"))]
        let opts = RecallOptions::new(query).with_limit(limit);
        #[cfg(feature = "hybrid")]
        if let Some(embedding) = query_embedding {
            opts = opts
                .with_embedding(embedding)
                .with_hybrid(true)
                .with_temporal_intent(TemporalIntent::Timeless)
                .with_temporal_operator(TemporalOperator::Current);
        }
        self.recall_scored_with_options(&opts)
    }

    #[cfg(feature = "hybrid")]
    fn recall_scored_internal(
        &self,
        query: &str,
        query_embedding: Option<&[f32]>,
        limit: usize,
        intent: TemporalIntent,
        operator: TemporalOperator,
    ) -> Result<Vec<(Fact, RecallScore)>> {
        if let Some(emb) = query_embedding {
            let params = HybridSearchParams {
                k: limit,
                intent,
                operator,
                ..HybridSearchParams::default()
            };
            let hits = self.graph.search_hybrid(query, emb, params, None)?;
            let mut scored = Vec::with_capacity(hits.len());
            for (fact, breakdown) in hits {
                if !fact.is_currently_valid() {
                    continue;
                }
                let confidence = fact.confidence;
                let eff = self.compute_effective_confidence(&fact)?;
                scored.push((
                    fact,
                    RecallScore::from_breakdown(&breakdown, confidence, eff),
                ));
            }
            return Ok(scored);
        }

        let scored_facts = self.graph.search_scored(query, limit)?;
        let mut scored = Vec::with_capacity(scored_facts.len());
        for (i, (fact, bm25)) in scored_facts.into_iter().enumerate() {
            if !fact.is_currently_valid() {
                continue;
            }
            let confidence = fact.confidence;
            let eff = self.compute_effective_confidence(&fact)?;
            scored.push((
                fact,
                RecallScore::TextOnly {
                    rank: i,
                    bm25_score: bm25,
                    confidence,
                    effective_confidence: eff,
                },
            ));
        }
        Ok(scored)
    }

    #[cfg(not(feature = "hybrid"))]
    fn recall_scored_internal(
        &self,
        query: &str,
        _query_embedding: Option<&[f32]>,
        limit: usize,
        _intent: (),
        _operator: (),
    ) -> Result<Vec<(Fact, RecallScore)>> {
        let scored_facts = self.graph.search_scored(query, limit)?;
        let mut scored = Vec::with_capacity(scored_facts.len());
        for (i, (fact, bm25)) in scored_facts.into_iter().enumerate() {
            if !fact.is_currently_valid() {
                continue;
            }
            let confidence = fact.confidence;
            let eff = self.compute_effective_confidence(&fact)?;
            scored.push((
                fact,
                RecallScore::TextOnly {
                    rank: i,
                    bm25_score: bm25,
                    confidence,
                    effective_confidence: eff,
                },
            ));
        }
        Ok(scored)
    }

    /// Retrieve memory facts using scored recall plus confidence filtering.
    ///
    /// Equivalent to [`recall_scored_with_options`](Self::recall_scored_with_options)
    /// with only `limit` and `min_confidence` set, preserving the ordering and
    /// pagination semantics introduced by the options-based path.
    ///
    /// ```rust,no_run
    /// use kronroe_agent_memory::AgentMemory;
    ///
    /// let memory = AgentMemory::open("./agent.kronroe").unwrap();
    /// memory.assert_with_confidence("alice", "works_at", "Acme", 0.95).unwrap();
    /// memory.assert_with_confidence("alice", "visited", "London", 0.55).unwrap();
    ///
    /// let scored = memory
    ///     .recall_scored_with_min_confidence("alice", None, 1, 0.9)
    ///     .unwrap();
    /// assert_eq!(scored.len(), 1);
    /// assert!(scored[0].1.confidence() >= 0.9);
    /// ```
    pub fn recall_scored_with_min_confidence(
        &self,
        query: &str,
        #[cfg(feature = "hybrid")] query_embedding: Option<&[f32]>,
        #[cfg(not(feature = "hybrid"))] _query_embedding: Option<&[f32]>,
        limit: usize,
        min_confidence: f32,
    ) -> Result<Vec<(Fact, RecallScore)>> {
        let opts = RecallOptions::new(query)
            .with_limit(limit)
            .with_min_confidence(min_confidence);

        #[cfg(feature = "hybrid")]
        let opts = if let Some(embedding) = query_embedding {
            opts.with_embedding(embedding).with_hybrid(true)
        } else {
            opts
        };

        self.recall_scored_with_options(&opts)
    }

    /// Retrieve memory facts by query while filtering by *effective* confidence.
    ///
    /// Equivalent to [`recall_scored_with_options`](Self::recall_scored_with_options)
    /// with only `limit` and `with_min_effective_confidence` set.
    ///
    /// Only available when the `uncertainty` feature is enabled.
    #[cfg(feature = "uncertainty")]
    pub fn recall_scored_with_min_effective_confidence(
        &self,
        query: &str,
        #[cfg(feature = "hybrid")] query_embedding: Option<&[f32]>,
        #[cfg(not(feature = "hybrid"))] _query_embedding: Option<&[f32]>,
        limit: usize,
        min_effective_confidence: f32,
    ) -> Result<Vec<(Fact, RecallScore)>> {
        let opts = RecallOptions::new(query)
            .with_limit(limit)
            .with_min_effective_confidence(min_effective_confidence);

        #[cfg(feature = "hybrid")]
        let opts = if let Some(embedding) = query_embedding {
            opts.with_embedding(embedding).with_hybrid(true)
        } else {
            opts
        };

        self.recall_scored_with_options(&opts)
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

    /// Retrieve memory facts using [`RecallOptions`], with per-channel signal breakdowns.
    ///
    /// Like [`recall_scored`](Self::recall_scored) but accepts a `RecallOptions`
    /// struct for cleaner parameter passing. When `min_confidence` is set, facts
    /// below the threshold are filtered from the results. Filtering occurs before
    /// truncating to the final `limit`, so low-confidence head matches cannot
    /// consume the result budget.
    /// By default, filtering uses base fact confidence; call
    /// [`RecallOptions::with_min_effective_confidence`] to use uncertainty-aware
    /// effective confidence instead.
    pub fn recall_scored_with_options(
        &self,
        opts: &RecallOptions<'_>,
    ) -> Result<Vec<(Fact, RecallScore)>> {
        let score_for_filter = |score: &RecallScore| match opts.confidence_filter_mode {
            ConfidenceFilterMode::Base => score.confidence(),
            #[cfg(feature = "uncertainty")]
            ConfidenceFilterMode::Effective => score
                .effective_confidence()
                .unwrap_or_else(|| score.confidence()),
        };
        #[cfg(feature = "hybrid")]
        let query_embedding_for_path = if opts.use_hybrid {
            opts.query_embedding
        } else {
            None
        };
        #[cfg(not(feature = "hybrid"))]
        let query_embedding_for_path = None;

        match opts.min_confidence {
            Some(min_confidence) => {
                let min_confidence = normalize_min_confidence(min_confidence)?;
                if opts.limit == 0 {
                    return Ok(Vec::new());
                }
                if opts.max_scored_rows == 0 {
                    return Err(Error::Search(
                        "max_scored_rows must be at least 1".to_string(),
                    ));
                }
                let max_scored_rows = opts.max_scored_rows;
                #[cfg(feature = "hybrid")]
                let is_hybrid_request = query_embedding_for_path.is_some();
                #[cfg(not(feature = "hybrid"))]
                let is_hybrid_request = false;

                // Hybrid ranking can be non-monotonic as `k` changes, so apply
                // one-shot fetch at the confidence budget then filter locally.
                if is_hybrid_request {
                    let scored = self.recall_scored_internal(
                        opts.query,
                        query_embedding_for_path,
                        max_scored_rows,
                        #[cfg(feature = "hybrid")]
                        opts.temporal_intent,
                        #[cfg(feature = "hybrid")]
                        opts.temporal_operator,
                        #[cfg(not(feature = "hybrid"))]
                        (),
                        #[cfg(not(feature = "hybrid"))]
                        (),
                    )?;
                    let mut filtered = Vec::new();

                    for (fact, score) in scored {
                        if score_for_filter(&score) >= min_confidence {
                            filtered.push((fact, score));
                            if filtered.len() >= opts.limit {
                                break;
                            }
                        }
                    }

                    return Ok(filtered);
                }

                let mut filtered = Vec::new();
                let mut seen_fact_ids: HashSet<String> = HashSet::new();
                let mut fetch_limit = opts.limit.max(1).min(max_scored_rows);
                let mut consecutive_no_confidence_batches = 0u8;

                loop {
                    let scored = self.recall_scored_internal(
                        opts.query,
                        query_embedding_for_path,
                        fetch_limit,
                        #[cfg(feature = "hybrid")]
                        opts.temporal_intent,
                        #[cfg(feature = "hybrid")]
                        opts.temporal_operator,
                        #[cfg(not(feature = "hybrid"))]
                        (),
                        #[cfg(not(feature = "hybrid"))]
                        (),
                    )?;
                    let mut newly_seen = 0usize;
                    let mut newly_confident = 0usize;

                    if scored.is_empty() {
                        break;
                    }

                    for (fact, score) in scored.iter() {
                        if !seen_fact_ids.insert(fact.id.0.clone()) {
                            continue;
                        }
                        newly_seen += 1;

                        if score_for_filter(score) >= min_confidence {
                            filtered.push((fact.clone(), *score));
                            newly_confident += 1;
                            if filtered.len() >= opts.limit {
                                return Ok(filtered);
                            }
                        }
                    }

                    if newly_seen == 0 || fetch_limit >= max_scored_rows {
                        break;
                    }

                    // If the latest fetch returned fewer rows than requested,
                    // we've reached the end of the result set.
                    if scored.len() < fetch_limit {
                        break;
                    }

                    // If we repeatedly fetch windows with zero confident rows,
                    // jump directly to the hard budget to avoid repeated rescans.
                    if newly_confident == 0 {
                        consecutive_no_confidence_batches =
                            consecutive_no_confidence_batches.saturating_add(1);
                        if consecutive_no_confidence_batches >= 2 {
                            fetch_limit = max_scored_rows;
                            continue;
                        }
                    } else {
                        consecutive_no_confidence_batches = 0;
                    }

                    fetch_limit = (fetch_limit.saturating_mul(2)).min(max_scored_rows);
                }

                Ok(filtered)
            }
            None => self.recall_scored_internal(
                opts.query,
                query_embedding_for_path,
                opts.limit,
                #[cfg(feature = "hybrid")]
                opts.temporal_intent,
                #[cfg(feature = "hybrid")]
                opts.temporal_operator,
                #[cfg(not(feature = "hybrid"))]
                (),
                #[cfg(not(feature = "hybrid"))]
                (),
            ),
        }
    }

    /// Retrieve memory facts using [`RecallOptions`].
    ///
    /// Convenience wrapper over [`recall_scored_with_options`](Self::recall_scored_with_options)
    /// that strips the score breakdowns.
    pub fn recall_with_options(&self, opts: &RecallOptions<'_>) -> Result<Vec<Fact>> {
        self.recall_scored_with_options(opts)
            .map(|scored| scored.into_iter().map(|(fact, _)| fact).collect())
    }

    /// Store a structured fact with explicit confidence.
    ///
    /// Like [`assert`](Self::assert) but allows setting the confidence score
    /// (clamped to \[0.0, 1.0\]).
    pub fn assert_with_confidence(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        confidence: f32,
    ) -> Result<FactId> {
        self.assert_with_confidence_with_params(
            subject,
            predicate,
            object,
            AssertParams {
                valid_from: Utc::now(),
            },
            confidence,
        )
    }

    /// Store a structured fact with explicit confidence and explicit timing.
    pub fn assert_with_confidence_with_params(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        params: AssertParams,
        confidence: f32,
    ) -> Result<FactId> {
        let confidence = normalize_fact_confidence(confidence)?;
        self.graph.assert_fact_with_confidence(
            subject,
            predicate,
            object,
            params.valid_from,
            confidence,
        )
    }

    /// Store a structured fact with explicit source provenance.
    ///
    /// Like [`assert`](Self::assert) but attaches a source marker (e.g.
    /// `"user:owner"`, `"api:linkedin"`) that the uncertainty engine uses
    /// for source-weighted confidence.
    pub fn assert_with_source(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        confidence: f32,
        source: &str,
    ) -> Result<FactId> {
        self.assert_with_source_with_params(
            subject,
            predicate,
            object,
            AssertParams {
                valid_from: Utc::now(),
            },
            confidence,
            source,
        )
    }

    /// Store a structured fact with explicit source provenance and explicit timing.
    pub fn assert_with_source_with_params(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        params: AssertParams,
        confidence: f32,
        source: &str,
    ) -> Result<FactId> {
        let confidence = normalize_fact_confidence(confidence)?;
        self.graph.assert_fact_with_source(
            subject,
            predicate,
            object,
            params.valid_from,
            confidence,
            source,
        )
    }

    // -----------------------------------------------------------------------
    // Uncertainty engine
    // -----------------------------------------------------------------------

    /// Register default predicate volatilities for common agent-memory predicates.
    ///
    /// Called automatically from `open()` when the `uncertainty` feature is enabled.
    #[cfg(feature = "uncertainty")]
    fn register_default_volatilities(graph: &TemporalGraph) -> Result<()> {
        use kronroe::PredicateVolatility;
        // Volatile: job/location change every few years
        let defaults = [
            ("works_at", PredicateVolatility::new(730.0)),
            ("job_title", PredicateVolatility::new(730.0)),
            ("lives_in", PredicateVolatility::new(1095.0)),
            ("email", PredicateVolatility::new(1460.0)),
            ("phone", PredicateVolatility::new(1095.0)),
            ("born_in", PredicateVolatility::stable()),
            ("full_name", PredicateVolatility::stable()),
        ];

        for (predicate, volatility) in defaults {
            if graph.predicate_volatility(predicate)?.is_none() {
                graph.register_predicate_volatility(predicate, volatility)?;
            }
        }
        Ok(())
    }

    /// Register a predicate volatility (half-life in days).
    ///
    /// After `half_life_days`, the age-decay multiplier drops to 0.5.
    /// Use `f64::INFINITY` for stable predicates that never decay.
    #[cfg(feature = "uncertainty")]
    pub fn register_volatility(&self, predicate: &str, half_life_days: f64) -> Result<()> {
        use kronroe::PredicateVolatility;
        self.graph
            .register_predicate_volatility(predicate, PredicateVolatility::new(half_life_days))
    }

    /// Register a source authority weight.
    ///
    /// Weight is clamped to \[0.0, 2.0\]. `1.0` = neutral, `>1.0` = boosted,
    /// `<1.0` = penalised. Unknown sources default to `1.0`.
    #[cfg(feature = "uncertainty")]
    pub fn register_source_weight(&self, source: &str, weight: f32) -> Result<()> {
        use kronroe::SourceWeight;
        self.graph
            .register_source_weight(source, SourceWeight::new(weight))
    }

    /// Compute effective confidence for a fact at a point in time.
    ///
    /// Returns `Ok(Some(value))` when uncertainty support is enabled and
    /// `Ok(None)` when uncertainty support is disabled in this build.
    #[cfg(feature = "uncertainty")]
    pub fn effective_confidence_for_fact(
        &self,
        fact: &Fact,
        at: DateTime<Utc>,
    ) -> Result<Option<f32>> {
        self.graph
            .effective_confidence(fact, at)
            .map(|eff| Some(eff.value))
    }

    /// Compute effective confidence for a fact at a point in time.
    ///
    /// Returns `Ok(Some(value))` when uncertainty support is enabled and
    /// `Ok(None)` when uncertainty support is disabled in this build.
    #[cfg(not(feature = "uncertainty"))]
    pub fn effective_confidence_for_fact(
        &self,
        fact: &Fact,
        at: DateTime<Utc>,
    ) -> Result<Option<f32>> {
        let _ = (fact, at);
        Ok(None)
    }

    /// Compute effective confidence for a fact, or `None` if the uncertainty
    /// feature is not enabled.
    fn compute_effective_confidence(&self, fact: &Fact) -> Result<Option<f32>> {
        self.effective_confidence_for_fact(fact, Utc::now())
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
    fn test_assert_idempotent_with_params_uses_valid_from() {
        let (mem, _tmp) = open_temp_memory();
        let valid_from = Utc::now() - chrono::Duration::days(10);
        let first = mem
            .assert_idempotent_with_params(
                "evt-param-1",
                "alice",
                "works_at",
                "Acme",
                AssertParams { valid_from },
            )
            .unwrap();
        let second = mem
            .assert_idempotent_with_params(
                "evt-param-1",
                "alice",
                "works_at",
                "Acme",
                AssertParams {
                    valid_from: Utc::now(),
                },
            )
            .unwrap();
        assert_eq!(first, second);

        let facts = mem.facts_about("alice").unwrap();
        assert_eq!(facts.len(), 1);
        assert!((facts[0].valid_from - valid_from).num_seconds().abs() < 1);
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
    fn recall_for_task_returns_subject_focused_report() {
        let (mem, _tmp) = open_temp_memory();
        let old = Utc::now() - chrono::Duration::days(120);
        mem.assert_with_confidence_with_params(
            "alice",
            "works_at",
            "Acme",
            AssertParams { valid_from: old },
            0.65,
        )
        .unwrap();
        mem.assert_with_confidence("alice", "project", "Renewal Q2", 0.95)
            .unwrap();
        mem.assert_with_confidence("bob", "project", "Other deal", 0.99)
            .unwrap();

        let report = mem
            .recall_for_task(
                "prepare renewal call",
                Some("alice"),
                None,
                Some(90),
                10,
                None,
            )
            .unwrap();

        assert_eq!(report.subject.as_deref(), Some("alice"));
        assert!(
            !report.key_facts.is_empty(),
            "expected task facts for alice"
        );
        assert!(
            report.key_facts.iter().all(|fact| fact.subject == "alice"),
            "task report should stay focused on the requested subject"
        );
        assert!(report.low_confidence_count >= 1);
        assert!(report.stale_high_impact_count >= 1);
        assert!(!report.recommended_next_checks.is_empty());
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

    #[cfg(feature = "hybrid")]
    #[test]
    fn recall_for_task_accepts_query_embedding_for_hybrid_path() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember(
            "Alice focuses on Rust API reliability",
            "alice",
            Some(vec![1.0, 0.0]),
        )
        .unwrap();
        mem.remember("Bob focuses on ML experiments", "bob", Some(vec![0.0, 1.0]))
            .unwrap();

        let report = mem
            .recall_for_task(
                "prepare reliability review",
                Some("alice"),
                None,
                Some(30),
                5,
                Some(&[1.0, 0.0]),
            )
            .unwrap();
        assert!(!report.key_facts.is_empty());
        assert_eq!(report.subject.as_deref(), Some("alice"));
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn recall_with_embedding_without_hybrid_toggle_is_text_scored() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Rust systems", "ep-rust", Some(vec![1.0f32, 0.0]))
            .unwrap();
        mem.remember("Python notebooks", "ep-py", Some(vec![0.0f32, 1.0]))
            .unwrap();

        let opts = RecallOptions::new("Rust")
            .with_embedding(&[1.0, 0.0])
            .with_limit(2);
        let results = mem.recall_scored_with_options(&opts).unwrap();
        assert!(!results.is_empty());
        assert!(matches!(results[0].1, RecallScore::TextOnly { .. }));
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

    #[cfg(feature = "uncertainty")]
    #[test]
    fn default_volatility_registration_preserves_custom_entry() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_str().unwrap().to_string();

        {
            let graph = kronroe::TemporalGraph::open(&path).unwrap();
            graph
                .register_predicate_volatility("works_at", kronroe::PredicateVolatility::new(1.0))
                .unwrap();
        }

        // reopen through AgentMemory; default bootstrap should not overwrite custom 1.0
        // with default 730.0 days.
        {
            let _mem = AgentMemory::open(&path).unwrap();
        }

        let graph = kronroe::TemporalGraph::open(&path).unwrap();
        let vol = graph
            .predicate_volatility("works_at")
            .unwrap()
            .expect("volatility should be persisted");

        assert!(
            (vol.half_life_days - 1.0).abs() < f64::EPSILON,
            "custom volatility should survive default bootstrap, got {}",
            vol.half_life_days
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
            effective_confidence: None,
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
            effective_confidence: None,
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
            effective_confidence: None,
        };
        assert_eq!(text_score.display_tag(), "#1 bm25:4.21");

        let text_score_5 = RecallScore::TextOnly {
            rank: 4,
            bm25_score: 1.50,
            confidence: 1.0,
            effective_confidence: None,
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
            effective_confidence: None,
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

    // -- RecallOptions + confidence tests -----------------------------------

    #[test]
    fn recall_options_default_limit() {
        let opts = RecallOptions::new("test query");
        assert_eq!(opts.limit, 10);
        assert!(opts.query_embedding.is_none());
        assert!(opts.min_confidence.is_none());
        assert_eq!(opts.max_scored_rows, 4_096);
    }

    #[test]
    fn assert_with_confidence_round_trip() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_confidence("alice", "works_at", "Acme", 0.8)
            .unwrap();

        let facts = mem.facts_about("alice").unwrap();
        assert_eq!(facts.len(), 1);
        assert!(
            (facts[0].confidence - 0.8).abs() < f32::EPSILON,
            "confidence should be 0.8, got {}",
            facts[0].confidence,
        );
    }

    #[test]
    fn assert_with_confidence_rejects_non_finite() {
        let (mem, _tmp) = open_temp_memory();

        for confidence in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let err = mem.assert_with_confidence("alice", "works_at", "Rust", confidence);
            match err {
                Err(Error::Search(msg)) => {
                    assert!(msg.contains("finite"), "unexpected search error: {msg}")
                }
                _ => panic!("expected search error for confidence={confidence:?}"),
            }
        }
    }

    #[test]
    fn recall_with_min_confidence_filters() {
        let (mem, _tmp) = open_temp_memory();
        // Store two facts with different confidence levels.
        mem.assert_with_confidence("ep-low", "memory", "low confidence fact about Rust", 0.3)
            .unwrap();
        mem.assert_with_confidence("ep-high", "memory", "high confidence fact about Rust", 0.9)
            .unwrap();

        // Without filter: both returned.
        let all = mem.recall("Rust", None, 10).unwrap();
        assert_eq!(all.len(), 2, "both facts should be returned without filter");

        // With min_confidence=0.5: only the high-confidence fact.
        let opts = RecallOptions::new("Rust")
            .with_limit(10)
            .with_min_confidence(0.5);
        let filtered = mem.recall_with_options(&opts).unwrap();
        assert_eq!(
            filtered.len(),
            1,
            "only high-confidence fact should pass filter"
        );
        assert!(
            (filtered[0].confidence - 0.9).abs() < f32::EPSILON,
            "surviving fact should have confidence 0.9, got {}",
            filtered[0].confidence,
        );
    }

    #[test]
    fn assemble_context_shows_confidence_tag() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_confidence("ep-test", "memory", "notable fact about testing", 0.7)
            .unwrap();

        let ctx = mem.assemble_context("testing", None, 500).unwrap();
        assert!(
            ctx.contains("conf:0.7"),
            "context should include conf:0.7 tag for non-default confidence, got: {ctx}"
        );
    }

    #[test]
    fn recall_scored_with_options_respects_limit() {
        let (mem, _tmp) = open_temp_memory();
        for i in 0..5 {
            mem.assert_with_confidence(
                &format!("ep-{i}"),
                "memory",
                format!("fact number {i} about coding"),
                1.0,
            )
            .unwrap();
        }

        let opts = RecallOptions::new("coding").with_limit(2);
        let results = mem.recall_scored_with_options(&opts).unwrap();
        assert!(
            results.len() <= 2,
            "should respect limit=2, got {} results",
            results.len(),
        );
    }

    #[test]
    fn recall_scored_with_options_filters_confidence_before_limit() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_confidence("low-1", "memory", "rust rust rust rust rust", 0.2)
            .unwrap();
        mem.assert_with_confidence("low-2", "memory", "rust rust rust rust rust", 0.1)
            .unwrap();
        mem.assert_with_confidence("high", "memory", "rust", 0.9)
            .unwrap();

        let opts = RecallOptions::new("rust")
            .with_limit(1)
            .with_min_confidence(0.9);
        let results = mem.recall_scored_with_options(&opts).unwrap();

        assert_eq!(
            results.len(),
            1,
            "expected one surviving result after filtering"
        );
        assert_eq!(results[0].0.subject, "high");
        assert!(
            (results[0].1.confidence() - 0.9).abs() < f32::EPSILON,
            "surviving result should keep confidence=0.9"
        );
    }

    #[test]
    fn recall_scored_with_options_normalizes_min_confidence_bounds() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_confidence("high", "memory", "rust", 1.0)
            .unwrap();
        mem.assert_with_confidence("low", "memory", "rust", 0.1)
            .unwrap();

        let opts = RecallOptions::new("rust")
            .with_limit(2)
            .with_min_confidence(2.0);
        let results = mem.recall_scored_with_options(&opts).unwrap();
        assert_eq!(
            results.len(),
            1,
            "min confidence above 1.0 should be clamped to 1.0"
        );
        assert!(
            (results[0].1.confidence() - 1.0).abs() < f32::EPSILON,
            "surviving row should use clamped threshold 1.0"
        );

        let opts = RecallOptions::new("rust")
            .with_limit(2)
            .with_min_confidence(-1.0);
        let results = mem.recall_scored_with_options(&opts).unwrap();
        assert_eq!(
            results.len(),
            2,
            "min confidence below 0.0 should be clamped to 0.0"
        );
    }

    #[test]
    fn recall_scored_with_options_rejects_non_finite_min_confidence() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_confidence("ep", "memory", "rust", 1.0)
            .unwrap();

        let opts = RecallOptions::new("rust")
            .with_limit(2)
            .with_min_confidence(f32::NAN);
        let err = mem.recall_scored_with_options(&opts).unwrap_err();
        match err {
            Error::Search(msg) => assert!(
                msg.contains("minimum confidence"),
                "unexpected search error: {msg}"
            ),
            _ => panic!("expected search error for NaN min confidence, got {err:?}"),
        }
    }

    #[test]
    fn recall_scored_with_options_respects_scored_rows_cap() {
        let (mem, _tmp) = open_temp_memory();
        for i in 0..5 {
            mem.assert_with_confidence(&format!("ep-{i}"), "memory", "rust and memory", 1.0)
                .unwrap();
        }

        let opts = RecallOptions::new("rust")
            .with_limit(5)
            .with_min_confidence(0.0)
            .with_max_scored_rows(2);
        let results = mem.recall_scored_with_options(&opts).unwrap();
        assert_eq!(
            results.len(),
            2,
            "max_scored_rows should bound the effective recall window in filtered mode"
        );
    }

    #[cfg(feature = "uncertainty")]
    #[test]
    fn recall_scored_with_options_effective_confidence_respects_scored_rows_cap() {
        let (mem, _tmp) = open_temp_memory();
        for i in 0..5 {
            mem.assert_with_source(
                &format!("ep-{i}"),
                "memory",
                "rust and memory",
                1.0,
                "user:owner",
            )
            .unwrap();
        }

        let opts = RecallOptions::new("rust")
            .with_limit(5)
            .with_min_effective_confidence(0.5)
            .with_max_scored_rows(2);
        let results = mem.recall_scored_with_options(&opts).unwrap();
        assert_eq!(
            results.len(),
            2,
            "effective-confidence path should honor max_scored_rows cap"
        );
    }

    #[cfg(all(feature = "hybrid", feature = "uncertainty"))]
    #[test]
    fn recall_scored_with_options_hybrid_effective_confidence_respects_scored_rows_cap() {
        let (mem, _tmp) = open_temp_memory();
        for i in 0..5 {
            mem.remember(
                "rust memory entry",
                &format!("ep-{i}"),
                Some(vec![1.0f32, 0.0]),
            )
            .unwrap();
        }

        let opts = RecallOptions::new("rust")
            .with_embedding(&[1.0, 0.0])
            .with_limit(5)
            .with_min_effective_confidence(0.0)
            .with_max_scored_rows(2);
        let results = mem.recall_scored_with_options(&opts).unwrap();
        assert_eq!(
            results.len(),
            2,
            "hybrid effective-confidence path should honor max_scored_rows cap"
        );
    }

    #[test]
    fn recall_scored_with_options_rejects_zero_max_scored_rows() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_confidence("ep", "memory", "rust", 1.0)
            .unwrap();

        let opts = RecallOptions::new("rust")
            .with_limit(1)
            .with_min_confidence(0.0)
            .with_max_scored_rows(0);
        let err = mem.recall_scored_with_options(&opts).unwrap_err();
        match err {
            Error::Search(msg) => assert!(
                msg.contains("max_scored_rows"),
                "unexpected search error: {msg}"
            ),
            _ => panic!("expected search error for max_scored_rows=0, got {err:?}"),
        }
    }

    #[test]
    fn recall_with_min_confidence_method_filters_before_limit() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_confidence("low-1", "memory", "rust rust rust rust rust", 0.2)
            .unwrap();
        mem.assert_with_confidence("low-2", "memory", "rust rust rust rust rust", 0.1)
            .unwrap();
        mem.assert_with_confidence("high", "memory", "rust", 0.9)
            .unwrap();

        let results = mem
            .recall_with_min_confidence("Rust", None, 1, 0.9)
            .unwrap();

        assert_eq!(
            results.len(),
            1,
            "expected one surviving result after filtering"
        );
        assert_eq!(results[0].subject, "high");
    }

    #[test]
    fn recall_scored_with_min_confidence_method_respects_limit() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_confidence("low", "memory", "rust rust rust rust", 0.2)
            .unwrap();
        mem.assert_with_confidence("high-2", "memory", "rust", 0.95)
            .unwrap();
        mem.assert_with_confidence("high-1", "memory", "rust", 0.98)
            .unwrap();

        let scored = mem
            .recall_scored_with_min_confidence("Rust", None, 2, 0.9)
            .unwrap();

        assert_eq!(scored.len(), 2, "expected exactly 2 surviving results");
        assert!(scored[0].1.confidence() >= 0.9);
        assert!(scored[1].1.confidence() >= 0.9);
    }

    #[test]
    fn recall_scored_with_min_confidence_matches_options_path() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_confidence("low", "memory", "rust rust rust", 0.2)
            .unwrap();
        mem.assert_with_confidence("high", "memory", "rust", 0.95)
            .unwrap();
        mem.assert_with_confidence("high-2", "memory", "rust for sure", 0.99)
            .unwrap();

        let method_results = mem
            .recall_scored_with_min_confidence("Rust", None, 2, 0.9)
            .unwrap()
            .into_iter()
            .map(|(fact, _)| fact.id.0)
            .collect::<Vec<_>>();

        let opts = RecallOptions::new("Rust")
            .with_limit(2)
            .with_min_confidence(0.9);
        let options_results = mem
            .recall_scored_with_options(&opts)
            .unwrap()
            .into_iter()
            .map(|(fact, _)| fact.id.0)
            .collect::<Vec<_>>();

        assert_eq!(method_results, options_results);
    }

    #[test]
    fn assert_with_source_round_trip() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_source("alice", "works_at", "Acme", 0.9, "user:owner")
            .unwrap();

        let facts = mem.facts_about("alice").unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].source.as_deref(), Some("user:owner"));
        assert!((facts[0].confidence - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn assert_with_confidence_with_params_uses_valid_from() {
        let (mem, _tmp) = open_temp_memory();
        let valid_from = Utc::now() - chrono::Duration::days(90);
        mem.assert_with_confidence_with_params(
            "alice",
            "worked_at",
            "Acme",
            AssertParams { valid_from },
            0.7,
        )
        .unwrap();

        let facts = mem.facts_about("alice").unwrap();
        assert_eq!(facts.len(), 1);
        assert!((facts[0].valid_from - valid_from).num_seconds().abs() < 1);
        assert!((facts[0].confidence - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn assert_with_source_with_params_uses_valid_from() {
        let (mem, _tmp) = open_temp_memory();
        let valid_from = Utc::now() - chrono::Duration::days(45);
        mem.assert_with_source_with_params(
            "alice",
            "works_at",
            "Acme",
            AssertParams { valid_from },
            0.85,
            "agent:planner",
        )
        .unwrap();

        let facts = mem.facts_about("alice").unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].source.as_deref(), Some("agent:planner"));
        assert_eq!(facts[0].predicate, "works_at");
        assert!((facts[0].valid_from - valid_from).num_seconds().abs() < 1);
        assert!((facts[0].confidence - 0.85).abs() < f32::EPSILON);
    }

    #[test]
    fn what_changed_reports_new_invalidated_and_confidence_shift() {
        let (mem, _tmp) = open_temp_memory();
        let original_id = mem
            .assert_with_params(
                "alice",
                "works_at",
                "Acme",
                AssertParams {
                    valid_from: Utc::now() - chrono::Duration::days(365),
                },
            )
            .unwrap();

        let since = Utc::now();
        mem.invalidate_fact(&original_id).unwrap();

        let original_fact = mem
            .facts_about("alice")
            .unwrap()
            .into_iter()
            .find(|fact| fact.id == original_id)
            .expect("original fact should still exist in history");
        let replacement_valid_from = original_fact
            .expired_at
            .expect("invalidated fact should have expired_at");

        let replacement_id = mem
            .assert_with_confidence_with_params(
                "alice",
                "works_at",
                "Beta Corp",
                AssertParams {
                    valid_from: replacement_valid_from,
                },
                0.6,
            )
            .unwrap();

        let report = mem
            .what_changed("alice", since, Some("works_at"))
            .expect("what_changed should succeed");
        assert_eq!(report.new_facts.len(), 1);
        assert_eq!(report.new_facts[0].id, replacement_id);
        assert_eq!(report.invalidated_facts.len(), 1);
        assert_eq!(report.invalidated_facts[0].id, original_id);
        assert_eq!(report.corrections.len(), 1);
        assert_eq!(report.corrections[0].old_fact.id, original_id);
        assert_eq!(report.corrections[0].new_fact.id, replacement_id);
        assert_eq!(report.confidence_shifts.len(), 1);
        assert_eq!(report.confidence_shifts[0].from_fact_id, original_id);
        assert_eq!(report.confidence_shifts[0].to_fact_id, replacement_id);
    }

    #[test]
    fn what_changed_links_correction_with_small_timestamp_jitter() {
        let (mem, _tmp) = open_temp_memory();
        let original_id = mem
            .assert_with_params(
                "alice",
                "works_at",
                "Acme",
                AssertParams {
                    valid_from: Utc::now() - chrono::Duration::days(30),
                },
            )
            .unwrap();

        let since = Utc::now();
        mem.invalidate_fact(&original_id).unwrap();

        let original_fact = mem
            .facts_about("alice")
            .unwrap()
            .into_iter()
            .find(|fact| fact.id == original_id)
            .expect("original fact should exist in history");
        let expired_at = original_fact
            .expired_at
            .expect("expired_at should be present after invalidation");
        let jittered_valid_from = expired_at + chrono::Duration::milliseconds(900);

        mem.assert_with_confidence_with_params(
            "alice",
            "works_at",
            "Beta Corp",
            AssertParams {
                valid_from: jittered_valid_from,
            },
            0.65,
        )
        .unwrap();

        let report = mem
            .what_changed("alice", since, Some("works_at"))
            .expect("what_changed should succeed");
        assert_eq!(
            report.corrections.len(),
            1,
            "sub-second drift should still link as a correction"
        );
    }

    #[test]
    fn what_changed_does_not_link_far_apart_replacements() {
        let (mem, _tmp) = open_temp_memory();
        let original_id = mem
            .assert_with_params(
                "alice",
                "works_at",
                "Acme",
                AssertParams {
                    valid_from: Utc::now() - chrono::Duration::days(30),
                },
            )
            .unwrap();

        let since = Utc::now();
        mem.invalidate_fact(&original_id).unwrap();

        let original_fact = mem
            .facts_about("alice")
            .unwrap()
            .into_iter()
            .find(|fact| fact.id == original_id)
            .expect("original fact should exist in history");
        let expired_at = original_fact
            .expired_at
            .expect("expired_at should be present after invalidation");
        let distant_valid_from = expired_at + chrono::Duration::seconds(10);

        mem.assert_with_confidence_with_params(
            "alice",
            "works_at",
            "Beta Corp",
            AssertParams {
                valid_from: distant_valid_from,
            },
            0.65,
        )
        .unwrap();

        let report = mem
            .what_changed("alice", since, Some("works_at"))
            .expect("what_changed should succeed");
        assert_eq!(
            report.corrections.len(),
            0,
            "larger timing gaps should not be auto-linked as corrections"
        );
    }

    #[test]
    fn memory_health_reports_low_confidence_and_stale_high_impact() {
        let (mem, _tmp) = open_temp_memory();
        let old = Utc::now() - chrono::Duration::days(200);

        mem.assert_with_confidence_with_params(
            "alice",
            "nickname",
            "Bex",
            AssertParams { valid_from: old },
            0.4,
        )
        .unwrap();
        mem.assert_with_confidence_with_params(
            "alice",
            "email",
            "alice@example.com",
            AssertParams { valid_from: old },
            0.9,
        )
        .unwrap();

        let report = mem
            .memory_health("alice", None, 0.7, 90)
            .expect("memory_health should succeed");
        assert_eq!(report.total_fact_count, 2);
        assert_eq!(report.active_fact_count, 2);
        assert_eq!(report.low_confidence_facts.len(), 1);
        assert_eq!(report.low_confidence_facts[0].predicate, "nickname");
        assert_eq!(report.stale_high_impact_facts.len(), 1);
        assert_eq!(report.stale_high_impact_facts[0].predicate, "email");
        assert_eq!(report.contradiction_count, 0);
        assert!(
            report
                .recommended_actions
                .iter()
                .any(|entry| entry.contains("low-confidence")),
            "expected low-confidence action"
        );
        assert!(
            report
                .recommended_actions
                .iter()
                .any(|entry| entry.contains("stale high-impact")),
            "expected stale high-impact action"
        );
    }

    #[cfg(feature = "uncertainty")]
    #[test]
    fn recall_includes_effective_confidence() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert("alice", "works_at", "Acme").unwrap();

        let scored = mem.recall_scored("alice", None, 10).unwrap();
        assert!(!scored.is_empty());
        // With uncertainty feature on, effective_confidence should be Some
        let eff = scored[0].1.effective_confidence();
        assert!(
            eff.is_some(),
            "expected Some effective_confidence, got None"
        );
        assert!(eff.unwrap() > 0.0);
    }

    #[cfg(feature = "uncertainty")]
    #[test]
    fn volatile_predicate_decays() {
        let (mem, _tmp) = open_temp_memory();
        // works_at has default 730-day half-life from register_default_volatilities.
        // Assert a fact that's "old" by using the graph directly with a past valid_from.
        let past = Utc::now() - chrono::Duration::days(730);
        mem.graph
            .assert_fact("alice", "works_at", "OldCo", past)
            .unwrap();
        // Assert a fresh fact
        mem.graph
            .assert_fact("alice", "born_in", "London", Utc::now())
            .unwrap();

        let old_eff = mem
            .graph
            .effective_confidence(
                mem.facts_about("alice")
                    .unwrap()
                    .iter()
                    .find(|f| f.predicate == "works_at")
                    .unwrap(),
                Utc::now(),
            )
            .unwrap();
        let fresh_eff = mem
            .graph
            .effective_confidence(
                mem.facts_about("alice")
                    .unwrap()
                    .iter()
                    .find(|f| f.predicate == "born_in")
                    .unwrap(),
                Utc::now(),
            )
            .unwrap();

        // At 730 days (one half-life), works_at effective should be ~0.5
        assert!(
            old_eff.value < 0.6,
            "730-day old works_at should have decayed, got {}",
            old_eff.value
        );
        // born_in is stable, fresh fact should be ~1.0
        assert!(
            fresh_eff.value > 0.9,
            "fresh born_in should be near 1.0, got {}",
            fresh_eff.value
        );
    }

    #[cfg(feature = "uncertainty")]
    #[test]
    fn source_weight_affects_confidence() {
        let (mem, _tmp) = open_temp_memory();
        mem.register_source_weight("trusted", 1.5).unwrap();
        mem.register_source_weight("untrusted", 0.5).unwrap();

        mem.assert_with_source("alice", "works_at", "TrustCo", 1.0, "trusted")
            .unwrap();
        mem.assert_with_source("bob", "works_at", "SketchCo", 1.0, "untrusted")
            .unwrap();

        let alice_facts = mem.facts_about("alice").unwrap();
        let bob_facts = mem.facts_about("bob").unwrap();

        let alice_eff = mem
            .graph
            .effective_confidence(&alice_facts[0], Utc::now())
            .unwrap();
        let bob_eff = mem
            .graph
            .effective_confidence(&bob_facts[0], Utc::now())
            .unwrap();

        assert!(
            alice_eff.value > bob_eff.value,
            "trusted source should have higher effective confidence: {} vs {}",
            alice_eff.value,
            bob_eff.value
        );
    }

    #[cfg(feature = "uncertainty")]
    #[test]
    fn effective_confidence_for_fact_returns_some() {
        let (mem, _tmp) = open_temp_memory();
        mem.assert_with_source("alice", "works_at", "Acme", 0.9, "user:owner")
            .unwrap();

        let fact = mem.facts_about("alice").unwrap().remove(0);
        let eff = mem
            .effective_confidence_for_fact(&fact, Utc::now())
            .unwrap()
            .expect("uncertainty-enabled builds should return effective confidence");

        assert!(
            eff > 0.0,
            "effective confidence should be positive for a fresh fact, got {eff}"
        );
    }
}
