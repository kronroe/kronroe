//! Engine-native contradiction detection for singleton predicates.
//!
//! A contradiction exists when two active facts share the same
//! `(subject, predicate)` pair, have different values, and their
//! valid-time intervals overlap — but the predicate is registered
//! as a singleton (at most one value at any point in time).
//!
//! Detection is purely structural: temporal overlap (Allen's interval
//! algebra) + value comparison. No LLM required.

use crate::{Fact, Value};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Whether a predicate allows multiple concurrent values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PredicateCardinality {
    /// At most one active value at any point in time (e.g. "works_at").
    Singleton,
    /// Multiple concurrent values allowed (e.g. "speaks_language").
    MultiValued,
}

/// How severe a detected contradiction is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConflictSeverity {
    /// Informational — values differ but overlap is partial.
    Low,
    /// Likely conflict — different values, significant overlap.
    Medium,
    /// Hard conflict — different values, full temporal containment.
    High,
}

/// What the engine should do when a contradiction is detected at write time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ConflictPolicy {
    /// Store the fact regardless — caller handles conflicts downstream.
    #[default]
    Allow,
    /// Store the fact but return contradictions in the result.
    Warn,
    /// Reject the fact if it contradicts existing facts.
    Reject,
}

/// Suggested resolution for a contradiction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestedResolution {
    /// Close `valid_to` on the older fact to eliminate overlap.
    CloseOlderFact { fact_id: String },
    /// Invalidate the older fact entirely.
    InvalidateOlderFact { fact_id: String },
    /// The new fact should not be stored (used with Reject policy).
    RejectNewFact,
    /// Caller should review — no automatic suggestion.
    ManualReview,
}

/// A detected contradiction between two facts.
#[derive(Debug, Clone)]
pub struct Contradiction {
    /// The existing fact that conflicts.
    pub existing_fact_id: String,
    /// The conflicting fact (may be a proposed new fact).
    pub conflicting_fact_id: String,
    /// Subject they share.
    pub subject: String,
    /// Predicate they share.
    pub predicate: String,
    /// Start of the temporal overlap window.
    pub overlap_start: DateTime<Utc>,
    /// End of the temporal overlap window (None = ongoing).
    pub overlap_end: Option<DateTime<Utc>>,
    /// How severe the conflict is.
    pub severity: ConflictSeverity,
    /// Confidence delta between the two facts (absolute value).
    pub confidence_delta: f32,
    /// Suggested resolution.
    pub suggested_resolution: SuggestedResolution,
}

// ---------------------------------------------------------------------------
// Pure functions
// ---------------------------------------------------------------------------

use serde::{Deserialize, Serialize};

/// Compute valid-time overlap between two active facts.
///
/// Uses Allen's interval algebra: two intervals [a_start, a_end) and
/// [b_start, b_end) overlap iff `a_start < b_end AND b_start < a_end`.
/// Open-ended intervals (valid_to = None) extend to +∞.
///
/// Returns `None` if either fact has been expired (transaction-time
/// invalidated) or if there is no temporal overlap.
pub(crate) fn valid_time_overlap(
    a: &Fact,
    b: &Fact,
) -> Option<(DateTime<Utc>, Option<DateTime<Utc>>)> {
    // Skip facts that have been expired (transaction-time invalidated).
    if a.expired_at.is_some() || b.expired_at.is_some() {
        return None;
    }

    let a_end = a.valid_to.unwrap_or(DateTime::<Utc>::MAX_UTC);
    let b_end = b.valid_to.unwrap_or(DateTime::<Utc>::MAX_UTC);

    // Allen's overlap: a_start < b_end AND b_start < a_end
    if a.valid_from < b_end && b.valid_from < a_end {
        let overlap_start = a.valid_from.max(b.valid_from);
        let overlap_end = {
            let min_end = a_end.min(b_end);
            if min_end == DateTime::<Utc>::MAX_UTC {
                None
            } else {
                Some(min_end)
            }
        };
        Some((overlap_start, overlap_end))
    } else {
        None
    }
}

/// Do two values structurally conflict?
///
/// Values conflict if they are different. Comparison is structural:
/// same variant + same inner value = no conflict.
pub(crate) fn values_conflict(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Text(x), Value::Text(y)) => x != y,
        (Value::Number(x), Value::Number(y)) => (x - y).abs() > f64::EPSILON,
        (Value::Boolean(x), Value::Boolean(y)) => x != y,
        (Value::Entity(x), Value::Entity(y)) => x != y,
        // Different variant types always conflict.
        _ => true,
    }
}

/// Compute conflict severity from overlap characteristics.
///
/// - **High:** one interval fully contains the other (or both open-ended).
/// - **Medium:** partial overlap longer than 30 days.
/// - **Low:** partial overlap of 30 days or less.
pub(crate) fn compute_severity(
    a: &Fact,
    b: &Fact,
    overlap: (DateTime<Utc>, Option<DateTime<Utc>>),
) -> ConflictSeverity {
    let a_end = a.valid_to.unwrap_or(DateTime::<Utc>::MAX_UTC);
    let b_end = b.valid_to.unwrap_or(DateTime::<Utc>::MAX_UTC);

    // Full containment: one interval contains the other entirely.
    let a_contains_b = a.valid_from <= b.valid_from && a_end >= b_end;
    let b_contains_a = b.valid_from <= a.valid_from && b_end >= a_end;

    if a_contains_b || b_contains_a {
        return ConflictSeverity::High;
    }

    // Partial overlap — severity based on duration.
    let (start, end) = overlap;
    let overlap_days = match end {
        Some(e) => (e - start).num_days(),
        None => 365, // Open-ended overlap treated as long.
    };

    if overlap_days > 30 {
        ConflictSeverity::Medium
    } else {
        ConflictSeverity::Low
    }
}

/// Suggest a resolution based on temporal and confidence heuristics.
///
/// - If one fact was recorded significantly later, suggest closing the older.
/// - If confidence differs by ≥ 0.3, suggest invalidating the lower-confidence fact.
/// - Otherwise, suggest manual review.
pub(crate) fn suggest_resolution(a: &Fact, b: &Fact) -> SuggestedResolution {
    let conf_delta = (a.confidence - b.confidence).abs();

    // If confidence differs significantly, suggest invalidating the weaker fact.
    if conf_delta >= 0.3 {
        let weaker = if a.confidence < b.confidence {
            a.id.as_str()
        } else {
            b.id.as_str()
        };
        return SuggestedResolution::InvalidateOlderFact {
            fact_id: weaker.to_string(),
        };
    }

    // If one was recorded much later, suggest closing the older.
    let time_gap = (a.recorded_at - b.recorded_at).num_seconds().unsigned_abs();
    if time_gap > 3600 {
        // > 1 hour apart
        let older = if a.recorded_at < b.recorded_at {
            a.id.as_str()
        } else {
            b.id.as_str()
        };
        return SuggestedResolution::CloseOlderFact {
            fact_id: older.to_string(),
        };
    }

    SuggestedResolution::ManualReview
}

/// Detect a pairwise contradiction between two facts.
///
/// Returns `Some(Contradiction)` if the facts share the same subject and
/// predicate, have conflicting values, and overlap in valid time.
/// Returns `None` if there is no contradiction.
pub(crate) fn detect_pairwise(a: &Fact, b: &Fact) -> Option<Contradiction> {
    // Must share subject and predicate.
    if a.subject != b.subject || a.predicate != b.predicate {
        return None;
    }

    // Values must actually differ.
    if !values_conflict(&a.object, &b.object) {
        return None;
    }

    // Must overlap in valid time.
    let (overlap_start, overlap_end) = valid_time_overlap(a, b)?;

    let severity = compute_severity(a, b, (overlap_start, overlap_end));
    let confidence_delta = (a.confidence - b.confidence).abs();
    let suggested_resolution = suggest_resolution(a, b);

    Some(Contradiction {
        existing_fact_id: a.id.to_string(),
        conflicting_fact_id: b.id.to_string(),
        subject: a.subject.clone(),
        predicate: a.predicate.clone(),
        overlap_start,
        overlap_end,
        severity,
        confidence_delta,
        suggested_resolution,
    })
}

// ---------------------------------------------------------------------------
// ContradictionDetector
// ---------------------------------------------------------------------------

/// In-memory predicate registry that drives contradiction detection.
///
/// Loaded from redb on init, kept in sync by `register_singleton_predicate`.
pub(crate) struct ContradictionDetector {
    registry: HashMap<String, PredicateCardinality>,
    policies: HashMap<String, ConflictPolicy>,
}

impl ContradictionDetector {
    pub(crate) fn new() -> Self {
        Self {
            registry: HashMap::new(),
            policies: HashMap::new(),
        }
    }

    pub(crate) fn register(
        &mut self,
        predicate: &str,
        cardinality: PredicateCardinality,
        policy: ConflictPolicy,
    ) {
        self.registry.insert(predicate.to_string(), cardinality);
        self.policies.insert(predicate.to_string(), policy);
    }

    pub(crate) fn is_singleton(&self, predicate: &str) -> bool {
        self.registry
            .get(predicate)
            .is_some_and(|c| *c == PredicateCardinality::Singleton)
    }

    pub(crate) fn policy_for(&self, predicate: &str) -> ConflictPolicy {
        self.policies
            .get(predicate)
            .copied()
            .unwrap_or(ConflictPolicy::Allow)
    }

    /// Check a proposed fact against existing facts for contradictions.
    ///
    /// Only checks if the predicate is registered as Singleton. Multi-valued
    /// predicates (the default for unregistered predicates) are skipped.
    pub(crate) fn check_against(&self, new_fact: &Fact, existing: &[Fact]) -> Vec<Contradiction> {
        if !self.is_singleton(&new_fact.predicate) {
            return Vec::new();
        }

        existing
            .iter()
            .filter_map(|existing_fact| detect_pairwise(existing_fact, new_fact))
            .collect()
    }

    /// Iterate over all registered singleton predicates.
    pub(crate) fn singleton_predicates(&self) -> impl Iterator<Item = &str> {
        self.registry.iter().filter_map(|(k, v)| {
            if *v == PredicateCardinality::Singleton {
                Some(k.as_str())
            } else {
                None
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FactId;
    use chrono::TimeZone;

    fn make_fact(
        subject: &str,
        predicate: &str,
        object: &str,
        valid_from: DateTime<Utc>,
        valid_to: Option<DateTime<Utc>>,
        confidence: f32,
    ) -> Fact {
        Fact {
            id: FactId::new(),
            subject: subject.to_string(),
            predicate: predicate.to_string(),
            object: Value::Text(object.to_string()),
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

    // -- valid_time_overlap ---------------------------------------------------

    #[test]
    fn overlap_both_open_ended() {
        let a = make_fact("alice", "works_at", "Acme", dt(2023, 1, 1), None, 1.0);
        let b = make_fact("alice", "works_at", "Beta", dt(2024, 1, 1), None, 1.0);
        let overlap = valid_time_overlap(&a, &b);
        assert!(overlap.is_some());
        let (start, end) = overlap.unwrap();
        assert_eq!(start, dt(2024, 1, 1));
        assert!(end.is_none(), "both open-ended → overlap is open-ended");
    }

    #[test]
    fn overlap_partial() {
        let a = make_fact(
            "alice",
            "works_at",
            "Acme",
            dt(2023, 1, 1),
            Some(dt(2024, 6, 1)),
            1.0,
        );
        let b = make_fact(
            "alice",
            "works_at",
            "Beta",
            dt(2024, 1, 1),
            Some(dt(2025, 1, 1)),
            1.0,
        );
        let overlap = valid_time_overlap(&a, &b);
        assert!(overlap.is_some());
        let (start, end) = overlap.unwrap();
        assert_eq!(start, dt(2024, 1, 1));
        assert_eq!(end, Some(dt(2024, 6, 1)));
    }

    #[test]
    fn no_overlap_sequential() {
        let a = make_fact(
            "alice",
            "works_at",
            "Acme",
            dt(2023, 1, 1),
            Some(dt(2024, 1, 1)),
            1.0,
        );
        let b = make_fact("alice", "works_at", "Beta", dt(2024, 1, 1), None, 1.0);
        let overlap = valid_time_overlap(&a, &b);
        assert!(overlap.is_none(), "adjacent intervals with no overlap");
    }

    #[test]
    fn no_overlap_expired() {
        let mut a = make_fact("alice", "works_at", "Acme", dt(2023, 1, 1), None, 1.0);
        a.expired_at = Some(dt(2024, 1, 1));
        let b = make_fact("alice", "works_at", "Beta", dt(2023, 6, 1), None, 1.0);
        let overlap = valid_time_overlap(&a, &b);
        assert!(overlap.is_none(), "expired fact should be skipped");
    }

    // -- values_conflict ------------------------------------------------------

    #[test]
    fn values_conflict_different_text() {
        assert!(values_conflict(
            &Value::Text("Acme".into()),
            &Value::Text("Beta".into()),
        ));
    }

    #[test]
    fn values_conflict_same_text() {
        assert!(!values_conflict(
            &Value::Text("Acme".into()),
            &Value::Text("Acme".into()),
        ));
    }

    #[test]
    fn values_conflict_different_types() {
        assert!(values_conflict(
            &Value::Text("42".into()),
            &Value::Number(42.0),
        ));
    }

    // -- compute_severity -----------------------------------------------------

    #[test]
    fn severity_full_containment_high() {
        let a = make_fact("alice", "works_at", "Acme", dt(2023, 1, 1), None, 1.0);
        let b = make_fact(
            "alice",
            "works_at",
            "Beta",
            dt(2024, 1, 1),
            Some(dt(2024, 6, 1)),
            1.0,
        );
        // a fully contains b
        let overlap = valid_time_overlap(&a, &b).unwrap();
        assert_eq!(compute_severity(&a, &b, overlap), ConflictSeverity::High);
    }

    #[test]
    fn severity_partial_overlap_medium() {
        let a = make_fact(
            "alice",
            "works_at",
            "Acme",
            dt(2023, 1, 1),
            Some(dt(2024, 3, 1)),
            1.0,
        );
        let b = make_fact(
            "alice",
            "works_at",
            "Beta",
            dt(2024, 1, 1),
            Some(dt(2025, 1, 1)),
            1.0,
        );
        // Overlap: 2024-01-01 to 2024-03-01 = 60 days > 30
        let overlap = valid_time_overlap(&a, &b).unwrap();
        assert_eq!(compute_severity(&a, &b, overlap), ConflictSeverity::Medium);
    }

    // -- detect_pairwise ------------------------------------------------------

    #[test]
    fn detect_pairwise_finds_contradiction() {
        let a = make_fact("alice", "works_at", "Acme", dt(2023, 1, 1), None, 1.0);
        let b = make_fact("alice", "works_at", "Beta", dt(2024, 1, 1), None, 0.8);
        let c = detect_pairwise(&a, &b);
        assert!(c.is_some());
        let c = c.unwrap();
        assert_eq!(c.subject, "alice");
        assert_eq!(c.predicate, "works_at");
        assert_eq!(c.severity, ConflictSeverity::High);
    }

    #[test]
    fn detect_pairwise_no_conflict_same_value() {
        let a = make_fact("alice", "works_at", "Acme", dt(2023, 1, 1), None, 1.0);
        let b = make_fact("alice", "works_at", "Acme", dt(2024, 1, 1), None, 1.0);
        assert!(
            detect_pairwise(&a, &b).is_none(),
            "same value = no conflict"
        );
    }

    #[test]
    fn detect_pairwise_no_conflict_different_predicate() {
        let a = make_fact("alice", "works_at", "Acme", dt(2023, 1, 1), None, 1.0);
        let b = make_fact("alice", "lives_in", "London", dt(2023, 1, 1), None, 1.0);
        assert!(detect_pairwise(&a, &b).is_none());
    }

    // -- suggest_resolution ---------------------------------------------------

    #[test]
    fn suggest_resolution_newer_wins() {
        let mut a = make_fact("alice", "works_at", "Acme", dt(2023, 1, 1), None, 1.0);
        let mut b = make_fact("alice", "works_at", "Beta", dt(2024, 1, 1), None, 1.0);
        // Make b recorded 2 hours after a.
        a.recorded_at = dt(2024, 1, 1);
        b.recorded_at = Utc::now();
        let res = suggest_resolution(&a, &b);
        assert!(
            matches!(res, SuggestedResolution::CloseOlderFact { ref fact_id } if *fact_id == a.id.to_string()),
            "should suggest closing the older fact, got {:?}",
            res,
        );
    }

    // -- ContradictionDetector ------------------------------------------------

    #[test]
    fn detector_check_against_singleton() {
        let mut detector = ContradictionDetector::new();
        detector.register(
            "works_at",
            PredicateCardinality::Singleton,
            ConflictPolicy::Warn,
        );

        let existing = vec![make_fact(
            "alice",
            "works_at",
            "Acme",
            dt(2023, 1, 1),
            None,
            1.0,
        )];
        let new_fact = make_fact("alice", "works_at", "Beta", dt(2024, 1, 1), None, 0.9);
        let contradictions = detector.check_against(&new_fact, &existing);
        assert_eq!(contradictions.len(), 1);
    }

    #[test]
    fn detector_skips_multi_valued() {
        let detector = ContradictionDetector::new();
        // "speaks_language" not registered → defaults to MultiValued
        let existing = vec![make_fact(
            "alice",
            "speaks_language",
            "English",
            dt(2023, 1, 1),
            None,
            1.0,
        )];
        let new_fact = make_fact(
            "alice",
            "speaks_language",
            "French",
            dt(2023, 1, 1),
            None,
            1.0,
        );
        let contradictions = detector.check_against(&new_fact, &existing);
        assert!(
            contradictions.is_empty(),
            "multi-valued predicates should not trigger contradictions"
        );
    }
}
