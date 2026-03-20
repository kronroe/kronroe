//! Two-stage hybrid retrieval with intent-gated temporal reranking.
//!
//! Promoted from the private eval harness (Experiment 01, 11 benchmark passes).
//! The winning configuration uses RRF fusion (stage 0, in [`TemporalGraph::search_hybrid`])
//! followed by a two-stage reranker:
//!
//! - **Stage 1:** Semantic-dominant candidate pruning
//! - **Stage 2:** Temporal feasibility filtering + intent-weighted rerank
//!
//! Callers provide [`TemporalIntent`] and [`TemporalOperator`] to express what
//! kind of time query they're making. For timeless/semantic queries, an adaptive
//! vector-dominance path adjusts weights based on the signal balance in the top
//! candidates.

use crate::Fact;
use chrono::{DateTime, Utc};
use std::cmp::Ordering;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Score breakdown for one hybrid retrieval hit.
///
/// The `final_score` reflects the pre-rerank RRF fusion score (text + vector
/// contributions). The two-stage reranker uses a composite of `final_score`,
/// `text_rrf_contrib`, `vector_rrf_contrib`, and temporal feasibility signals
/// to determine final ordering — the returned `final_score` is *not* the
/// reranker's sort key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HybridScoreBreakdown {
    /// RRF fusion score (sum of text + vector contributions).
    pub final_score: f64,
    /// Text-channel contribution from weighted RRF.
    pub text_rrf_contrib: f64,
    /// Vector-channel contribution from weighted RRF.
    pub vector_rrf_contrib: f64,
    /// Temporal contribution (currently always 0.0; reserved for future use).
    pub temporal_adjustment: f64,
}

/// The caller's temporal intent classification.
///
/// Determines how the reranker applies temporal feasibility signals.
/// `Timeless` (the default) disables temporal reranking entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TemporalIntent {
    /// No temporal constraint — pure semantic ranking.
    #[default]
    Timeless,
    /// Query about current state (e.g. "where does Alice work?").
    CurrentState,
    /// Query about a specific past point (e.g. "where did Alice work in 2023?").
    HistoricalPoint,
    /// Query about a time range (e.g. "what happened around Q3 2024?").
    HistoricalInterval,
}

/// Temporal operator hint — refines how the temporal signal is computed
/// for [`TemporalIntent::HistoricalPoint`] queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TemporalOperator {
    /// Default: fact must be currently valid.
    #[default]
    Current,
    /// Fact must have been valid at the query time.
    AsOf,
    /// Fact must have started before the query time.
    Before,
    /// Fact must have started by (on or before) the query time.
    By,
    /// Fact must overlap with the query time range.
    During,
    /// Fact must have started after the query time.
    After,
    /// Unknown operator — falls back to `was_valid_at` check.
    Unknown,
}

/// Parameters for the stable hybrid search API.
///
/// Defaults match the eval-proven winning configuration:
/// `rank_constant=60`, `text_weight=0.8`, `vector_weight=0.2`.
#[derive(Debug, Clone)]
pub struct HybridSearchParams {
    /// Number of results to return.
    pub k: usize,
    /// Candidates to pull from each channel before fusion.
    pub candidate_window: usize,
    /// RRF rank constant (denominator offset).
    pub rank_constant: usize,
    /// Relative weight of the lexical (full-text) channel.
    pub text_weight: f32,
    /// Relative weight of the vector (embedding) channel.
    pub vector_weight: f32,
    /// Caller's temporal intent classification.
    pub intent: TemporalIntent,
    /// Temporal operator hint (used with `HistoricalPoint`).
    pub operator: TemporalOperator,
}

impl Default for HybridSearchParams {
    fn default() -> Self {
        Self {
            k: 10,
            candidate_window: 50,
            rank_constant: 60,
            text_weight: 0.8,
            vector_weight: 0.2,
            intent: TemporalIntent::default(),
            operator: TemporalOperator::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Temporal signal
// ---------------------------------------------------------------------------

/// Compute intent-gated temporal feasibility signal for a single fact.
///
/// Returns a score in roughly [-1.0, +1.1] that indicates how temporally
/// feasible a fact is for the given intent and operator. Positive means the
/// fact fits the temporal constraint; negative means it doesn't.
#[cfg_attr(feature = "uncertainty", allow(dead_code))]
pub(crate) fn intent_gated_temporal_signal(
    fact: &Fact,
    intent: TemporalIntent,
    op: TemporalOperator,
    at: Option<DateTime<Utc>>,
) -> f64 {
    let t = at.unwrap_or_else(Utc::now);
    let conf = fact.confidence.clamp(0.2, 1.0) as f64;

    match intent {
        TemporalIntent::Timeless => 0.0,

        TemporalIntent::CurrentState => {
            let age_days = (t - fact.valid_from).num_seconds().max(0) as f64 / 86_400.0;
            let recency = (-std::f64::consts::LN_2 * age_days / 365.0).exp();
            let validity = if fact.is_currently_valid() { 1.0 } else { -1.0 };
            validity * (0.5 + 0.5 * recency) * conf
        }

        TemporalIntent::HistoricalPoint => match op {
            TemporalOperator::AsOf | TemporalOperator::During => {
                if fact.was_valid_at(t) {
                    1.0 * conf
                } else {
                    -1.0
                }
            }
            TemporalOperator::Before => {
                if fact.valid_from < t && fact.was_valid_at(t) {
                    1.1 * conf
                } else if fact.valid_from < t {
                    0.3 * conf
                } else {
                    -1.0
                }
            }
            TemporalOperator::By => {
                if fact.valid_from <= t && fact.was_valid_at(t) {
                    1.05 * conf
                } else if fact.valid_from <= t {
                    0.2 * conf
                } else {
                    -1.0
                }
            }
            TemporalOperator::After => {
                if fact.valid_from > t {
                    1.0 * conf
                } else {
                    -0.8
                }
            }
            TemporalOperator::Current => {
                if fact.is_currently_valid() {
                    0.9 * conf
                } else {
                    -0.8
                }
            }
            TemporalOperator::Unknown => {
                if fact.was_valid_at(t) {
                    0.9 * conf
                } else {
                    -0.8
                }
            }
        },

        TemporalIntent::HistoricalInterval => {
            // Operator is intentionally ignored for intervals — overlap check
            // against ±90 day window is the only signal. Matches eval runner.
            let _ = op;
            let start = t - chrono::Duration::days(90);
            let end = t + chrono::Duration::days(90);
            let overlap = fact.valid_from < end && fact.valid_to.unwrap_or(end) > start;
            if overlap {
                1.0 * conf
            } else {
                -1.0
            }
        }
    }
}

/// Like [`intent_gated_temporal_signal`] but uses the uncertainty engine for
/// effective confidence and predicate-aware age decay instead of hardcoded values.
#[cfg(feature = "uncertainty")]
pub(crate) fn intent_gated_temporal_signal_with_uncertainty(
    fact: &Fact,
    intent: TemporalIntent,
    op: TemporalOperator,
    at: Option<DateTime<Utc>>,
    engine: &crate::uncertainty::UncertaintyEngine,
) -> f64 {
    if matches!(intent, TemporalIntent::Timeless) {
        return 0.0;
    }

    let t = at.unwrap_or_else(Utc::now);
    let eff = engine.effective_confidence(fact, t);
    let conf = eff.value.clamp(0.2, 1.0) as f64;

    match intent {
        TemporalIntent::Timeless => 0.0,

        TemporalIntent::CurrentState => {
            // Age decay is already captured in `eff.age_decay` via the uncertainty
            // engine's per-predicate half-life (instead of the hardcoded 365 days).
            let recency = eff.age_decay as f64;
            let validity = if fact.is_currently_valid() { 1.0 } else { -1.0 };
            validity * (0.5 + 0.5 * recency) * conf
        }

        TemporalIntent::HistoricalPoint => match op {
            TemporalOperator::AsOf | TemporalOperator::During => {
                if fact.was_valid_at(t) {
                    1.0 * conf
                } else {
                    -1.0
                }
            }
            TemporalOperator::Before => {
                if fact.valid_from < t && fact.was_valid_at(t) {
                    1.1 * conf
                } else if fact.valid_from < t {
                    0.3 * conf
                } else {
                    -1.0
                }
            }
            TemporalOperator::By => {
                if fact.valid_from <= t && fact.was_valid_at(t) {
                    1.05 * conf
                } else if fact.valid_from <= t {
                    0.2 * conf
                } else {
                    -1.0
                }
            }
            TemporalOperator::After => {
                if fact.valid_from > t {
                    1.0 * conf
                } else {
                    -0.8
                }
            }
            TemporalOperator::Current => {
                if fact.is_currently_valid() {
                    0.9 * conf
                } else {
                    -0.8
                }
            }
            TemporalOperator::Unknown => {
                if fact.was_valid_at(t) {
                    0.9 * conf
                } else {
                    -0.8
                }
            }
        },

        TemporalIntent::HistoricalInterval => {
            let _ = op;
            let start = t - chrono::Duration::days(90);
            let end = t + chrono::Duration::days(90);
            let overlap = fact.valid_from < end && fact.valid_to.unwrap_or(end) > start;
            if overlap {
                1.0 * conf
            } else {
                -1.0
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Two-stage reranker
// ---------------------------------------------------------------------------

/// Semantic-dominant score for a single candidate.
fn semantic_core(b: &HybridScoreBreakdown) -> f64 {
    (0.75 * b.vector_rrf_contrib) + (0.25 * b.text_rrf_contrib) + (0.02 * b.final_score)
}

fn rerank_two_stage_internal(
    mut hits: Vec<(Fact, HybridScoreBreakdown)>,
    k: usize,
    intent: TemporalIntent,
    temporal_signal: impl Fn(&Fact) -> f64,
) -> Vec<(Fact, HybridScoreBreakdown)> {
    if hits.is_empty() || k == 0 {
        return Vec::new();
    }

    hits.sort_by(|(fa, a), (fb, b)| {
        let sa = semantic_core(a);
        let sb = semantic_core(b);
        sb.partial_cmp(&sa)
            .unwrap_or(Ordering::Equal)
            .then_with(|| fa.id.cmp(&fb.id))
    });

    let stage1_n = if matches!(intent, TemporalIntent::Timeless) {
        20
    } else {
        14
    };
    hits.truncate(stage1_n.min(hits.len()));

    if matches!(intent, TemporalIntent::Timeless) {
        let sample = hits.iter().take(5);
        let (sum_vec, sum_text) = sample.fold((0.0_f64, 0.0_f64), |(sv, st), (_, b)| {
            (sv + b.vector_rrf_contrib, st + b.text_rrf_contrib)
        });
        let vec_ratio = sum_vec / (sum_vec + sum_text + 1e-9);
        let (wv, wt) = if vec_ratio >= 0.60 {
            (0.90_f64, 0.10_f64)
        } else if vec_ratio >= 0.50 {
            (0.75_f64, 0.25_f64)
        } else {
            (0.60_f64, 0.40_f64)
        };

        hits.sort_by(|(fa, a), (fb, b)| {
            let sa =
                (wv * a.vector_rrf_contrib) + (wt * a.text_rrf_contrib) + (0.02 * a.final_score);
            let sb =
                (wv * b.vector_rrf_contrib) + (wt * b.text_rrf_contrib) + (0.02 * b.final_score);
            sb.partial_cmp(&sa)
                .unwrap_or(Ordering::Equal)
                .then_with(|| fa.id.cmp(&fb.id))
        });
        hits.truncate(k);
        return hits;
    }

    let feasible_only: Vec<(Fact, HybridScoreBreakdown)> = hits
        .iter()
        .filter(|(f, _)| temporal_signal(f) > 0.0)
        .cloned()
        .collect();
    if !feasible_only.is_empty() {
        hits = feasible_only;
    }

    let weight = match intent {
        TemporalIntent::CurrentState => 0.10,
        TemporalIntent::HistoricalPoint => 0.22,
        TemporalIntent::HistoricalInterval => 0.20,
        TemporalIntent::Timeless => 0.0,
    };

    hits.sort_by(|(fa, a), (fb, b)| {
        let sa = semantic_core(a) + (weight * temporal_signal(fa));
        let sb = semantic_core(b) + (weight * temporal_signal(fb));
        sb.partial_cmp(&sa)
            .unwrap_or(Ordering::Equal)
            .then_with(|| fa.id.cmp(&fb.id))
    });

    hits.truncate(k);
    hits
}

/// Two-stage reranker: semantic pruning → temporal feasibility rerank.
///
/// For timeless queries: adaptive vector-dominance reranking based on the
/// signal balance in the top-5 candidates.
///
/// For temporal queries:
/// - Stage 1: sort by semantic-dominant score, prune to top-14 candidates
/// - Stage 2: filter to temporally feasible candidates, rerank by
///   semantic + intent-weighted temporal signal
///
/// `k` controls the final output size.
#[cfg_attr(feature = "uncertainty", allow(dead_code))]
pub(crate) fn rerank_two_stage(
    hits: Vec<(Fact, HybridScoreBreakdown)>,
    k: usize,
    intent: TemporalIntent,
    op: TemporalOperator,
    at: Option<DateTime<Utc>>,
) -> Vec<(Fact, HybridScoreBreakdown)> {
    rerank_two_stage_internal(hits, k, intent, |fact| {
        intent_gated_temporal_signal(fact, intent, op, at)
    })
}

/// Like [`rerank_two_stage`] but uses the uncertainty engine for temporal
/// signal computation — per-predicate age decay and source-weighted confidence.
#[cfg(feature = "uncertainty")]
pub(crate) fn rerank_two_stage_with_uncertainty(
    hits: Vec<(Fact, HybridScoreBreakdown)>,
    k: usize,
    intent: TemporalIntent,
    op: TemporalOperator,
    at: Option<DateTime<Utc>>,
    engine: &crate::uncertainty::UncertaintyEngine,
) -> Vec<(Fact, HybridScoreBreakdown)> {
    rerank_two_stage_internal(hits, k, intent, |fact| {
        intent_gated_temporal_signal_with_uncertainty(fact, intent, op, at, engine)
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FactId, Value};
    use chrono::TimeZone;

    fn make_fact(
        subject: &str,
        valid_from: DateTime<Utc>,
        valid_to: Option<DateTime<Utc>>,
        confidence: f32,
    ) -> Fact {
        Fact {
            id: FactId::new(),
            subject: subject.to_string(),
            predicate: "test".to_string(),
            object: Value::Text("val".to_string()),
            valid_from,
            valid_to,
            recorded_at: Utc::now(),
            expired_at: None,
            confidence,
            source: None,
        }
    }

    fn dt(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap()
    }

    #[test]
    fn temporal_signal_timeless_returns_zero() {
        let fact = make_fact("alice", dt(2024, 1, 1), None, 1.0);
        let signal = intent_gated_temporal_signal(
            &fact,
            TemporalIntent::Timeless,
            TemporalOperator::Current,
            None,
        );
        assert!((signal - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn temporal_signal_current_state_valid_fact() {
        let fact = make_fact("alice", dt(2024, 6, 1), None, 1.0);
        let signal = intent_gated_temporal_signal(
            &fact,
            TemporalIntent::CurrentState,
            TemporalOperator::Current,
            Some(dt(2025, 1, 1)),
        );
        assert!(
            signal > 0.0,
            "currently valid fact should score positive, got {signal}"
        );
    }

    #[test]
    fn temporal_signal_current_state_expired_fact() {
        let fact = make_fact("alice", dt(2023, 1, 1), Some(dt(2023, 12, 1)), 1.0);
        let signal = intent_gated_temporal_signal(
            &fact,
            TemporalIntent::CurrentState,
            TemporalOperator::Current,
            Some(dt(2025, 1, 1)),
        );
        assert!(
            signal < 0.0,
            "expired fact should score negative, got {signal}"
        );
    }

    #[test]
    fn temporal_signal_historical_point_as_of_hit() {
        let fact = make_fact("alice", dt(2023, 1, 1), Some(dt(2024, 1, 1)), 1.0);
        let signal = intent_gated_temporal_signal(
            &fact,
            TemporalIntent::HistoricalPoint,
            TemporalOperator::AsOf,
            Some(dt(2023, 6, 1)),
        );
        assert!(
            (signal - 1.0).abs() < f64::EPSILON,
            "fact valid at query time should score 1.0, got {signal}"
        );
    }

    #[test]
    fn temporal_signal_historical_point_as_of_miss() {
        let fact = make_fact("alice", dt(2023, 1, 1), Some(dt(2023, 6, 1)), 1.0);
        let signal = intent_gated_temporal_signal(
            &fact,
            TemporalIntent::HistoricalPoint,
            TemporalOperator::AsOf,
            Some(dt(2024, 1, 1)),
        );
        assert!(
            (signal - -1.0).abs() < f64::EPSILON,
            "fact not valid at query time should score -1.0, got {signal}"
        );
    }

    #[test]
    fn temporal_signal_historical_interval_overlap() {
        // Fact valid from 2023-06 to 2024-06, query at 2024-01 with ±90 day window
        let fact = make_fact("alice", dt(2023, 6, 1), Some(dt(2024, 6, 1)), 1.0);
        let signal = intent_gated_temporal_signal(
            &fact,
            TemporalIntent::HistoricalInterval,
            TemporalOperator::During,
            Some(dt(2024, 1, 1)),
        );
        assert!(
            (signal - 1.0).abs() < f64::EPSILON,
            "overlapping fact should score 1.0, got {signal}"
        );
    }

    #[test]
    fn temporal_signal_historical_interval_no_overlap() {
        // Fact valid 2020-01 to 2020-06, query at 2024-01 — no overlap
        let fact = make_fact("alice", dt(2020, 1, 1), Some(dt(2020, 6, 1)), 1.0);
        let signal = intent_gated_temporal_signal(
            &fact,
            TemporalIntent::HistoricalInterval,
            TemporalOperator::During,
            Some(dt(2024, 1, 1)),
        );
        assert!(
            (signal - -1.0).abs() < f64::EPSILON,
            "non-overlapping fact should score -1.0, got {signal}"
        );
    }
}
