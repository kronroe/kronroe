//! Engine-native uncertainty model for temporal confidence decay.
//!
//! Computes **effective confidence** at query time from three signals:
//!
//! 1. **Base confidence** — the stored `f32` on the [`Fact`]
//! 2. **Age decay** — exponential half-life decay based on predicate volatility
//! 3. **Source authority** — multiplier based on fact provenance
//!
//! Formula: `effective = base_confidence × age_decay × source_weight`, clamped
//! to \[0.0, 1.0\].
//!
//! All math is pure Rust, zero external dependencies, works on every target.

use crate::Fact;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// How quickly facts with a given predicate go stale.
///
/// A `works_at` fact might have a 2-year half-life (730 days): after 2 years
/// the age-decay multiplier drops to 0.5. A `born_in` fact is essentially
/// stable (`half_life_days = f64::INFINITY`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PredicateVolatility {
    /// Half-life in days.  After this many days, the age-decay multiplier
    /// equals 0.5. `f64::INFINITY` means no decay (stable predicate).
    #[serde(
        serialize_with = "serialize_half_life",
        deserialize_with = "deserialize_half_life"
    )]
    pub half_life_days: f64,
}

// JSON compatibility for stability:
// serde_json cannot represent infinity as a number, so encode `f64::INFINITY`
// as the string "inf". For backward compatibility, `null` is also treated as
// stable.
fn serialize_half_life<S>(value: &f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if !value.is_finite() {
        serializer.serialize_str("inf")
    } else {
        serializer.serialize_f64(*value)
    }
}

fn deserialize_half_life<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct HalfLifeVisitor;

    impl<'de> Visitor<'de> for HalfLifeVisitor {
        type Value = f64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a finite number of days, or the string \"inf\" for stable")
        }

        fn visit_f64<E>(self, value: f64) -> Result<f64, E>
        where
            E: de::Error,
        {
            Ok(PredicateVolatility::new(value).half_life_days)
        }

        fn visit_u64<E>(self, value: u64) -> Result<f64, E>
        where
            E: de::Error,
        {
            Ok(PredicateVolatility::new(value as f64).half_life_days)
        }

        fn visit_i64<E>(self, value: i64) -> Result<f64, E>
        where
            E: de::Error,
        {
            Ok(PredicateVolatility::new(value as f64).half_life_days)
        }

        fn visit_str<E>(self, value: &str) -> Result<f64, E>
        where
            E: de::Error,
        {
            let lower = value.to_ascii_lowercase();
            if lower == "inf" || lower == "infinity" {
                Ok(f64::INFINITY)
            } else {
                Err(E::custom(format!(
                    "invalid half-life string '{value}'; expected 'inf' for stable"
                )))
            }
        }

        fn visit_unit<E>(self) -> Result<f64, E>
        where
            E: de::Error,
        {
            Ok(f64::INFINITY)
        }
    }

    deserializer.deserialize_any(HalfLifeVisitor)
}

impl PredicateVolatility {
    /// Create a volatility profile with the given half-life in days.
    ///
    /// Non-finite or non-positive values are treated as stable (no decay).
    pub fn new(half_life_days: f64) -> Self {
        Self {
            half_life_days: if half_life_days.is_finite() && half_life_days > 0.0 {
                half_life_days
            } else {
                f64::INFINITY
            },
        }
    }

    /// No decay — facts of this predicate never go stale.
    pub fn stable() -> Self {
        Self {
            half_life_days: f64::INFINITY,
        }
    }
}

/// Authority multiplier for a fact source.
///
/// A trusted source like `"user:owner"` might have weight 1.5 (boosted),
/// while an uncertain source like `"api:guess"` might have weight 0.5
/// (penalised). Default for unknown sources is 1.0 (neutral).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SourceWeight {
    /// Multiplier in \[0.0, 2.0\]. 1.0 = neutral.
    #[serde(deserialize_with = "deserialize_source_weight")]
    pub weight: f32,
}

fn deserialize_source_weight<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct SourceWeightVisitor;

    impl<'de> Visitor<'de> for SourceWeightVisitor {
        type Value = f32;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a finite number")
        }

        fn visit_f64<E>(self, value: f64) -> Result<f32, E>
        where
            E: de::Error,
        {
            Ok(SourceWeight::new(value as f32).weight)
        }

        fn visit_u64<E>(self, value: u64) -> Result<f32, E>
        where
            E: de::Error,
        {
            Ok(SourceWeight::new(value as f32).weight)
        }

        fn visit_i64<E>(self, value: i64) -> Result<f32, E>
        where
            E: de::Error,
        {
            Ok(SourceWeight::new(value as f32).weight)
        }

        fn visit_str<E>(self, value: &str) -> Result<f32, E>
        where
            E: de::Error,
        {
            let parsed = value.parse::<f32>().map_err(|_| {
                E::custom(format!(
                    "invalid source weight string '{value}'; expected a finite number"
                ))
            })?;
            Ok(SourceWeight::new(parsed).weight)
        }
    }

    deserializer.deserialize_any(SourceWeightVisitor)
}

impl SourceWeight {
    /// Create a source weight, clamped to \[0.0, 2.0\].
    pub fn new(weight: f32) -> Self {
        Self {
            weight: if weight.is_finite() {
                weight.clamp(0.0, 2.0)
            } else {
                1.0
            },
        }
    }
}

/// Result of computing effective confidence for a fact at a point in time.
///
/// Contains both the final value and the individual components so callers
/// can inspect or display the breakdown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectiveConfidence {
    /// The final computed confidence \[0.0, 1.0\].
    pub value: f32,
    /// The base (stored) confidence from the fact.
    pub base_confidence: f32,
    /// The age-decay multiplier that was applied (0.0, 1.0\].
    pub age_decay: f32,
    /// The source weight multiplier that was applied.
    pub source_weight: f32,
}

// ---------------------------------------------------------------------------
// Pure functions
// ---------------------------------------------------------------------------

/// Compute exponential age decay.
///
/// Returns a value in (0.0, 1.0\] representing how "fresh" the fact is.
/// At exactly one half-life, the result is 0.5. At two half-lives, 0.25.
///
/// An infinite half-life returns 1.0 (no decay). Negative age is treated
/// as zero (fact from the future — no penalty).
pub fn age_decay(age_days: f64, half_life_days: f64) -> f32 {
    if !half_life_days.is_finite() || half_life_days <= 0.0 {
        return 1.0;
    }
    let age = age_days.max(0.0);
    let decay = (-std::f64::consts::LN_2 * age / half_life_days).exp();
    (decay as f32).clamp(0.0, 1.0)
}

/// Compute the effective confidence of a fact at a given point in time.
///
/// Formula: `base_confidence × age_decay(age, half_life) × source_weight`,
/// clamped to \[0.0, 1.0\].
///
/// - `volatility`: looked up from the predicate registry. `None` = stable.
/// - `source_weight`: looked up from the source registry. `None` = 1.0.
/// - `t`: the point in time to evaluate at. Typically `Utc::now()`.
///
/// Age is measured from `fact.valid_from` (when it became true in the world),
/// not `fact.recorded_at` (when we stored it).
pub fn compute_effective_confidence(
    fact: &Fact,
    t: DateTime<Utc>,
    volatility: Option<&PredicateVolatility>,
    source_weight: Option<&SourceWeight>,
) -> EffectiveConfidence {
    let half_life = volatility
        .map(|v| v.half_life_days)
        .unwrap_or(f64::INFINITY);

    let age_days = (t - fact.valid_from).num_seconds().max(0) as f64 / 86_400.0;
    let decay = age_decay(age_days, half_life);

    let sw = source_weight.map(|s| s.weight).unwrap_or(1.0);

    let effective = (fact.confidence * decay * sw).clamp(0.0, 1.0);

    EffectiveConfidence {
        value: effective,
        base_confidence: fact.confidence,
        age_decay: decay,
        source_weight: sw,
    }
}

// ---------------------------------------------------------------------------
// In-memory registry
// ---------------------------------------------------------------------------

/// In-memory registries for uncertainty computation.
///
/// Loaded from redb on init, kept in sync by the registration methods on
/// [`TemporalGraph`](crate::TemporalGraph). Follows the same pattern as
/// `ContradictionDetector`.
pub(crate) struct UncertaintyEngine {
    volatility: HashMap<String, PredicateVolatility>,
    source_weights: HashMap<String, SourceWeight>,
}

impl UncertaintyEngine {
    pub(crate) fn new() -> Self {
        Self {
            volatility: HashMap::new(),
            source_weights: HashMap::new(),
        }
    }

    pub(crate) fn register_volatility(&mut self, predicate: &str, vol: PredicateVolatility) {
        self.volatility.insert(predicate.to_string(), vol);
    }

    pub(crate) fn register_source_weight(&mut self, source: &str, weight: SourceWeight) {
        self.source_weights.insert(source.to_string(), weight);
    }

    pub(crate) fn volatility_for(&self, predicate: &str) -> Option<&PredicateVolatility> {
        self.volatility.get(predicate)
    }

    pub(crate) fn source_weight_for(&self, source: &str) -> Option<&SourceWeight> {
        self.source_weights.get(source)
    }

    /// Compute effective confidence using the registries.
    pub(crate) fn effective_confidence(
        &self,
        fact: &Fact,
        t: DateTime<Utc>,
    ) -> EffectiveConfidence {
        let vol = self.volatility_for(&fact.predicate);
        let sw = fact
            .source
            .as_deref()
            .and_then(|s| self.source_weight_for(s));
        compute_effective_confidence(fact, t, vol, sw)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FactId, Value};
    use chrono::TimeZone;

    fn make_fact(predicate: &str, valid_from: DateTime<Utc>, confidence: f32) -> Fact {
        Fact {
            id: FactId::new(),
            subject: "alice".to_string(),
            predicate: predicate.to_string(),
            object: Value::Text("Acme".to_string()),
            valid_from,
            valid_to: None,
            recorded_at: Utc::now(),
            expired_at: None,
            confidence,
            source: None,
        }
    }

    #[test]
    fn age_decay_zero_age() {
        let d = age_decay(0.0, 365.0);
        assert!(
            (d - 1.0).abs() < 1e-6,
            "fresh fact should have decay=1.0, got {d}"
        );
    }

    #[test]
    fn age_decay_at_half_life() {
        let d = age_decay(365.0, 365.0);
        assert!(
            (d - 0.5).abs() < 1e-5,
            "at half-life, decay should be 0.5, got {d}"
        );
    }

    #[test]
    fn age_decay_infinite_half_life() {
        let d = age_decay(10_000.0, f64::INFINITY);
        assert!(
            (d - 1.0).abs() < 1e-6,
            "infinite half-life = no decay, got {d}"
        );
    }

    #[test]
    fn age_decay_negative_age() {
        let d = age_decay(-100.0, 365.0);
        assert!(
            (d - 1.0).abs() < 1e-6,
            "negative age treated as zero, got {d}"
        );
    }

    #[test]
    fn effective_confidence_multiplicative() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let valid_from = t - chrono::Duration::days(365);
        let fact = make_fact("works_at", valid_from, 0.8);
        let vol = PredicateVolatility::new(365.0);
        let sw = SourceWeight::new(1.0);
        let eff = compute_effective_confidence(&fact, t, Some(&vol), Some(&sw));
        // base=0.8, decay≈0.5, source=1.0 → effective≈0.4
        assert!(
            (eff.value - 0.4).abs() < 0.01,
            "expected ~0.4, got {}",
            eff.value
        );
        assert!((eff.base_confidence - 0.8).abs() < 1e-6);
        assert!((eff.age_decay - 0.5).abs() < 1e-5);
        assert!((eff.source_weight - 1.0).abs() < 1e-6);
    }

    #[test]
    fn effective_confidence_clamped() {
        let t = Utc::now();
        let fact = make_fact("works_at", t, 0.9); // fresh
        let sw = SourceWeight::new(2.0); // max authority boost
        let eff = compute_effective_confidence(&fact, t, None, Some(&sw));
        // 0.9 * 1.0 * 2.0 = 1.8, clamped to 1.0
        assert!(
            (eff.value - 1.0).abs() < 1e-6,
            "should clamp to 1.0, got {}",
            eff.value
        );
    }

    #[test]
    fn effective_confidence_defaults() {
        let t = Utc::now();
        let fact = make_fact("works_at", t, 0.7);
        let eff = compute_effective_confidence(&fact, t, None, None);
        // No volatility (no decay), no source weight (1.0) → base confidence
        assert!(
            (eff.value - 0.7).abs() < 1e-6,
            "with defaults, effective = base, got {}",
            eff.value
        );
    }

    #[test]
    fn engine_registry_round_trip() {
        let mut engine = UncertaintyEngine::new();
        engine.register_volatility("works_at", PredicateVolatility::new(730.0));
        engine.register_source_weight("user:owner", SourceWeight::new(1.5));

        assert!(engine.volatility_for("works_at").is_some());
        assert!(engine.volatility_for("born_in").is_none());
        assert!(engine.source_weight_for("user:owner").is_some());
        assert!(engine.source_weight_for("unknown").is_none());

        let t = Utc::now();
        let mut fact = make_fact("works_at", t - chrono::Duration::days(730), 1.0);
        fact.source = Some("user:owner".to_string());
        let eff = engine.effective_confidence(&fact, t);
        // base=1.0, decay≈0.5 (at half-life), source=1.5 → 1.0*0.5*1.5=0.75
        assert!(
            (eff.value - 0.75).abs() < 0.01,
            "expected ~0.75, got {}",
            eff.value
        );
    }
}
