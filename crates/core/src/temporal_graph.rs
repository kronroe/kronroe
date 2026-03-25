//! Kronroe — embedded temporal property graph database.
//!
//! The core primitive is a [`Fact`]: a subject-predicate-object triple
//! augmented with bi-temporal metadata (valid time + transaction time).
//!
//! **Valid time** (`valid_from` / `valid_to`) captures when something was
//! true *in the world*. **Transaction time** (`recorded_at` / `expired_at`)
//! captures when we *learned* it was true. The engine enforces both — they
//! are not application-level properties.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use kronroe::{KronroeTimestamp, TemporalGraph};
//!
//! let db = TemporalGraph::open("my-graph.kronroe").unwrap();
//!
//! // Assert a fact
//! let id = db
//!     .assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
//!     .unwrap();
//!
//! // Query current state
//! let facts = db.current_facts("alice", "works_at").unwrap();
//!
//! // Point-in-time query
//! let past = "2024-03-01T00:00:00Z".parse().unwrap();
//! let facts_then = db.facts_at("alice", "works_at", past).unwrap();
//! ```

mod error;
mod fact_id;
pub(crate) mod json_read;
pub(crate) mod json_write;
mod kronroe_time;
#[cfg(feature = "fulltext")]
mod lexical;
mod storage;
mod storage_append_log;
#[cfg(test)]
mod storage_benchmarks;
mod storage_observability;
#[cfg(feature = "vector")]
mod vector;

#[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
mod hybrid;
#[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
pub use hybrid::{HybridScoreBreakdown, HybridSearchParams, TemporalIntent, TemporalOperator};

#[cfg(feature = "contradiction")]
mod contradiction;
#[cfg(feature = "contradiction")]
pub use contradiction::{
    ConflictPolicy, ConflictSeverity, Contradiction, PredicateCardinality, SuggestedResolution,
};

#[cfg(feature = "uncertainty")]
mod uncertainty;
#[cfg(feature = "uncertainty")]
pub use uncertainty::{EffectiveConfidence, PredicateVolatility, SourceWeight};

pub use error::{ErrorCode, ErrorContext, KronroeError, OptionContext};
pub use fact_id::{FactId, FactIdParseError};
pub use kronroe_time::{
    default_clock, FixedClock, KronroeClock, KronroeSpan, KronroeTimestamp, SystemClock,
};
#[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
use std::cmp::Ordering;
#[cfg(any(
    feature = "fulltext",
    all(feature = "hybrid-experimental", feature = "vector")
))]
use std::collections::HashMap;
use storage::{KronroeStorage, SCHEMA_VERSION};

pub type Result<T> = std::result::Result<T, KronroeError>;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// The value stored in a fact's object position.
///
/// A fact's object can be a scalar value or a reference to another entity.
#[derive(Debug, Clone)]
pub enum Value {
    /// A text string.
    Text(String),
    /// A numeric value.
    Number(f64),
    /// A boolean.
    Boolean(bool),
    /// A reference to another entity by name or ID.
    Entity(String),
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Text(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}
impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Boolean(b)
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Text(s) | Value::Entity(s) => write!(f, "{s}"),
            Value::Number(n) => write!(f, "{n}"),
            Value::Boolean(b) => write!(f, "{b}"),
        }
    }
}

// -- Kronroe-native JSON codec for Value --
// Format: {"type":"Text","value":"..."} (serde-compatible internally-tagged)
impl Value {
    /// Write this value as JSON to a writer.
    pub fn write_json(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        use json_write::*;
        w.write_all(b"{")?;
        match self {
            Value::Text(s) => {
                write_kv_string(w, "type", "Text")?;
                w.write_all(b",")?;
                write_kv_string(w, "value", s)?;
            }
            Value::Number(n) => {
                write_kv_string(w, "type", "Number")?;
                w.write_all(b",")?;
                write_string(w, "value")?;
                w.write_all(b":")?;
                write_f64(w, *n)?;
            }
            Value::Boolean(b) => {
                write_kv_string(w, "type", "Boolean")?;
                w.write_all(b",")?;
                write_string(w, "value")?;
                w.write_all(b":")?;
                write_bool(w, *b)?;
            }
            Value::Entity(s) => {
                write_kv_string(w, "type", "Entity")?;
                w.write_all(b",")?;
                write_kv_string(w, "value", s)?;
            }
        }
        w.write_all(b"}")
    }

    pub(crate) fn from_json(val: &json_read::JsonValue) -> Result<Self> {
        let typ = val
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| KronroeError::serialization("Value missing 'type' field"))?;
        match typ {
            "Text" => {
                let s = val
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| KronroeError::serialization("Value::Text missing 'value'"))?;
                Ok(Value::Text(s.to_string()))
            }
            "Number" => {
                let n = val
                    .get("value")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| KronroeError::serialization("Value::Number missing 'value'"))?;
                Ok(Value::Number(n))
            }
            "Boolean" => {
                let b = val
                    .get("value")
                    .and_then(|v| v.as_bool())
                    .ok_or_else(|| KronroeError::serialization("Value::Boolean missing 'value'"))?;
                Ok(Value::Boolean(b))
            }
            "Entity" => {
                let s = val
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| KronroeError::serialization("Value::Entity missing 'value'"))?;
                Ok(Value::Entity(s.to_string()))
            }
            other => Err(KronroeError::serialization(format!(
                "unknown Value type: {other}"
            ))),
        }
    }
}

/// The core primitive: a bi-temporal subject-predicate-object triple.
///
/// # Bi-temporal model
///
/// Each fact carries two independent time axes:
///
/// - **Valid time** (`valid_from` / `valid_to`): when the fact was true *in
///   the world*, regardless of when we learned it. A job that started in 2020
///   has `valid_from = 2020-01-15` even if it was recorded in 2024.
///
/// - **Transaction time** (`recorded_at` / `expired_at`): when this fact
///   was present in the database. A correction sets `expired_at` on the old
///   fact and creates a new one — so you can query "what did we *believe*
///   about Alice's employer on 2024-03-01?" separately from "who was Alice's
///   employer on 2024-03-01?"
#[derive(Debug, Clone)]
pub struct Fact {
    /// Stable time-sortable ID.
    pub id: FactId,
    /// Who or what this fact is about.
    pub subject: String,
    /// The relationship or attribute (e.g. `works_at`, `has_role`).
    pub predicate: String,
    /// What is true.
    pub object: Value,
    /// When this became true in the world (valid time start).
    pub valid_from: KronroeTimestamp,
    /// When this stopped being true in the world. `None` = still true.
    pub valid_to: Option<KronroeTimestamp>,
    /// When this was written to the database (transaction time start).
    pub recorded_at: KronroeTimestamp,
    /// When we learned it was no longer true. `None` = still believed true.
    pub expired_at: Option<KronroeTimestamp>,
    /// Confidence in this fact \[0.0, 1.0\].
    pub confidence: f32,
    /// Where this fact came from (conversation ID, document ID, etc.).
    pub source: Option<String>,
}

impl Fact {
    /// Create a new fact with transaction time set to now.
    pub fn new(
        subject: impl Into<String>,
        predicate: impl Into<String>,
        object: impl Into<Value>,
        valid_from: KronroeTimestamp,
    ) -> Self {
        Self {
            id: FactId::new(),
            subject: subject.into(),
            predicate: predicate.into(),
            object: object.into(),
            valid_from,
            valid_to: None,
            recorded_at: KronroeTimestamp::now_utc(),
            expired_at: None,
            confidence: 1.0,
            source: None,
        }
    }

    /// Return a copy with the given confidence.
    ///
    /// Non-finite values (NaN/inf) are ignored by this builder and leave the
    /// previous confidence unchanged; explicit validation is enforced at write time.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        debug_assert!(
            confidence.is_finite(),
            "fact confidence is expected to be finite",
        );
        if confidence.is_finite() {
            self.confidence = confidence.clamp(0.0, 1.0);
        }
        self
    }

    /// Return a copy with the given source provenance marker.
    ///
    /// Source identifies where the fact came from (e.g. `"user:alice"`,
    /// `"api:openai"`, `"episode:conv-42"`). Used by the uncertainty model
    /// for authority weighting when the `uncertainty` feature is enabled.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Is this fact currently valid (valid time is open, not expired)?
    pub fn is_currently_valid(&self) -> bool {
        self.valid_to.is_none() && self.expired_at.is_none()
    }

    /// Was this fact valid at the given point in time (valid time axis)?
    pub fn was_valid_at(&self, at: KronroeTimestamp) -> bool {
        self.valid_from <= at
            && self.valid_to.is_none_or(|t| t > at)
            && self.expired_at.is_none_or(|t| t > at)
    }

    // -- Kronroe-native JSON codec for Fact --

    /// Serialize this fact as a JSON string.
    pub fn to_json_string(&self) -> String {
        let mut buf = Vec::new();
        self.write_json(&mut buf)
            .expect("Vec<u8> write is infallible");
        // SAFETY: write_json only produces valid UTF-8 (ASCII JSON)
        unsafe { String::from_utf8_unchecked(buf) }
    }

    /// Write this fact as JSON to a writer.
    pub fn write_json(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        use json_write::*;
        w.write_all(b"{")?;
        write_kv_string(w, "id", self.id.as_str())?;
        w.write_all(b",")?;
        write_kv_string(w, "subject", &self.subject)?;
        w.write_all(b",")?;
        write_kv_string(w, "predicate", &self.predicate)?;
        w.write_all(b",")?;
        write_string(w, "object")?;
        w.write_all(b":")?;
        self.object.write_json(w)?;
        w.write_all(b",")?;
        write_kv_string(w, "valid_from", &self.valid_from.to_rfc3339_z())?;
        w.write_all(b",")?;
        write_string(w, "valid_to")?;
        w.write_all(b":")?;
        match &self.valid_to {
            Some(t) => write_string(w, &t.to_rfc3339_z())?,
            None => write_null(w)?,
        }
        w.write_all(b",")?;
        write_kv_string(w, "recorded_at", &self.recorded_at.to_rfc3339_z())?;
        w.write_all(b",")?;
        write_string(w, "expired_at")?;
        w.write_all(b":")?;
        match &self.expired_at {
            Some(t) => write_string(w, &t.to_rfc3339_z())?,
            None => write_null(w)?,
        }
        w.write_all(b",")?;
        write_kv_f32(w, "confidence", self.confidence)?;
        w.write_all(b",")?;
        write_kv_option_string(w, "source", &self.source)?;
        w.write_all(b"}")
    }

    pub(crate) fn from_json(val: &json_read::JsonValue) -> Result<Self> {
        let get_str = |key: &str| -> Result<String> {
            val.get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| KronroeError::serialization(format!("Fact missing '{key}'")))
        };

        let id = FactId::parse(&get_str("id")?)
            .map_err(|e| KronroeError::serialization(format!("Fact id: {e}")))?;
        let subject = get_str("subject")?;
        let predicate = get_str("predicate")?;
        let object = Value::from_json(
            val.get("object")
                .ok_or_else(|| KronroeError::serialization("Fact missing 'object'"))?,
        )?;

        let valid_from = KronroeTimestamp::parse_rfc3339(&get_str("valid_from")?)
            .map_err(|e| KronroeError::serialization(format!("Fact valid_from: {e}")))?;
        let valid_to = match val.get("valid_to") {
            Some(v) if !v.is_null() => {
                let s = v
                    .as_str()
                    .ok_or_else(|| KronroeError::serialization("Fact valid_to: not a string"))?;
                Some(
                    KronroeTimestamp::parse_rfc3339(s)
                        .map_err(|e| KronroeError::serialization(format!("Fact valid_to: {e}")))?,
                )
            }
            _ => None,
        };
        let recorded_at = KronroeTimestamp::parse_rfc3339(&get_str("recorded_at")?)
            .map_err(|e| KronroeError::serialization(format!("Fact recorded_at: {e}")))?;
        let expired_at =
            match val.get("expired_at") {
                Some(v) if !v.is_null() => {
                    let s = v.as_str().ok_or_else(|| {
                        KronroeError::serialization("Fact expired_at: not a string")
                    })?;
                    Some(KronroeTimestamp::parse_rfc3339(s).map_err(|e| {
                        KronroeError::serialization(format!("Fact expired_at: {e}"))
                    })?)
                }
                _ => None,
            };

        let confidence = val
            .get("confidence")
            .and_then(|v| v.as_f32())
            .unwrap_or(1.0);
        let source = val
            .get("source")
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        Ok(Self {
            id,
            subject,
            predicate,
            object,
            valid_from,
            valid_to,
            recorded_at,
            expired_at,
            confidence,
            source,
        })
    }
}

/// Kronroe temporal property graph database.
///
/// An embedded, serverless database where bi-temporal facts are the core
/// primitive. All writes are ACID-backed by Kronroe's current default storage
/// engine. The database file
/// uses the `.kronroe` extension by convention.
///
/// # Example
///
/// ```rust,no_run
/// use kronroe::{KronroeTimestamp, TemporalGraph};
///
/// let db = TemporalGraph::open("my-graph.kronroe").unwrap();
/// db.assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
///     .unwrap();
/// let current = db.current_facts("alice", "works_at").unwrap();
/// assert_eq!(current.len(), 1);
/// ```
pub struct TemporalGraph {
    storage: KronroeStorage,
    /// In-memory vector index cache. Rebuilt from persisted embedding rows on
    /// every [`init`] call, then kept in sync by [`assert_fact_with_embedding`].
    /// Persisted storage is the source of truth; this cache is a read-optimised
    /// view of them.
    ///
    /// [`assert_fact_with_embedding`]: TemporalGraph::assert_fact_with_embedding
    #[cfg(feature = "vector")]
    vector_index: std::sync::Mutex<vector::VectorIndex>,
    #[cfg(feature = "contradiction")]
    contradiction_detector: std::sync::Mutex<contradiction::ContradictionDetector>,
    #[cfg(feature = "uncertainty")]
    uncertainty_engine: std::sync::Mutex<uncertainty::UncertaintyEngine>,
}

impl TemporalGraph {
    /// Open or create a Kronroe database at the given path.
    ///
    /// The file will be created if it does not exist. The `.kronroe`
    /// extension is conventional but not enforced.
    ///
    /// This is the default Kronroe storage engine path and uses the
    /// append-log backend.
    pub fn open(path: &str) -> Result<Self> {
        let storage = KronroeStorage::open(path)?;
        Self::init(storage)
    }

    /// Create an in-memory Kronroe database (no file I/O).
    ///
    /// Useful for WASM targets, testing, and ephemeral workloads where
    /// persistence is not needed. Data is lost when the instance is dropped.
    ///
    /// This is the default in-memory Kronroe storage engine path and uses
    /// the append-log backend.
    pub fn open_in_memory() -> Result<Self> {
        let storage = KronroeStorage::open_in_memory()?;
        Self::init(storage)
    }

    fn init(storage: KronroeStorage) -> Result<Self> {
        let stored_version = storage.initialize_schema()?;
        match stored_version {
            v if v == SCHEMA_VERSION => {}
            found => return Err(KronroeError::schema_mismatch(found, SCHEMA_VERSION)),
        }
        #[cfg(feature = "vector")]
        let vector_index = {
            let idx = Self::rebuild_vector_index_from_storage(&storage)?;
            std::sync::Mutex::new(idx)
        };
        #[cfg(feature = "contradiction")]
        let contradiction_detector = {
            let mut det = contradiction::ContradictionDetector::new();
            for (predicate, encoded) in storage.load_predicate_registry_entries()? {
                let (cardinality, policy) = contradiction::decode_predicate_registry(&encoded)
                    .map_err(|e| {
                        KronroeError::storage(format!(
                            "invalid predicate registry entry for '{}': {e}",
                            predicate
                        ))
                    })?;
                det.register(&predicate, cardinality, policy);
            }
            std::sync::Mutex::new(det)
        };
        #[cfg(feature = "uncertainty")]
        let uncertainty_engine = {
            let mut engine = uncertainty::UncertaintyEngine::new();
            for (predicate, encoded) in storage.load_volatility_registry_entries()? {
                let vol =
                    uncertainty::PredicateVolatility::from_json_str(&encoded).map_err(|e| {
                        KronroeError::storage(format!(
                            "invalid volatility registry entry for predicate '{}': {e}",
                            predicate
                        ))
                    })?;
                engine.register_volatility(&predicate, vol);
            }
            for (source, encoded) in storage.load_source_weight_registry_entries()? {
                let sw = uncertainty::SourceWeight::from_json_str(&encoded).map_err(|e| {
                    KronroeError::storage(format!(
                        "invalid source-weight registry entry for source '{}': {e}",
                        source
                    ))
                })?;
                engine.register_source_weight(&source, sw);
            }
            std::sync::Mutex::new(engine)
        };
        Ok(Self {
            storage,
            #[cfg(feature = "vector")]
            vector_index,
            #[cfg(feature = "contradiction")]
            contradiction_detector,
            #[cfg(feature = "uncertainty")]
            uncertainty_engine,
        })
    }

    /// Read every persisted embedding from storage and build a fresh in-memory
    /// [`VectorIndex`] cache.
    ///
    /// Called once from [`init`].  If the database was created before the
    /// `embeddings` table existed (old-format file), `TableDoesNotExist` is
    /// handled gracefully — the method returns an empty index and the table
    /// is created by the preceding `open_table` call in `init`.
    #[cfg(feature = "vector")]
    fn rebuild_vector_index_from_storage(storage: &KronroeStorage) -> Result<vector::VectorIndex> {
        let mut idx = vector::VectorIndex::new();
        for (fact_id, embedding) in storage.embedding_rows()? {
            idx.insert(fact_id, embedding)?;
        }

        Ok(idx)
    }
    fn build_fact(
        subject: &str,
        predicate: &str,
        object: Value,
        valid_from: KronroeTimestamp,
        confidence: f32,
        source: Option<&str>,
    ) -> Result<Fact> {
        let confidence = if confidence.is_finite() {
            confidence.clamp(0.0, 1.0)
        } else {
            return Err(KronroeError::search(
                "confidence must be finite and in [0.0, 1.0], got non-finite value",
            ));
        };

        let mut fact =
            Fact::new(subject, predicate, object, valid_from).with_confidence(confidence);
        if let Some(src) = source {
            fact = fact.with_source(src);
        }
        Ok(fact)
    }

    fn resolve_fact_id_input(&self, fact_id: &str) -> Result<FactId> {
        FactId::parse(fact_id).map_err(|_| KronroeError::invalid_fact_id(fact_id.to_string()))
    }

    /// Assert a new fact and return its [`FactId`].
    ///
    /// The fact is immediately persisted. If you want to invalidate a
    /// previous value for the same `(subject, predicate)` pair, call
    /// [`invalidate_fact`] first.
    ///
    /// [`invalidate_fact`]: TemporalGraph::invalidate_fact
    pub fn assert_fact(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        valid_from: KronroeTimestamp,
    ) -> Result<FactId> {
        let fact = Self::build_fact(subject, predicate, object.into(), valid_from, 1.0, None)?;
        let fact_id = fact.id.clone();
        self.storage.write_fact(&fact)?;
        Ok(fact_id)
    }

    /// Assert a new fact with explicit confidence and return its [`FactId`].
    ///
    /// Like [`assert_fact`] but allows setting the confidence score.
    /// Confidence is clamped to \[0.0, 1.0\].
    ///
    /// [`assert_fact`]: TemporalGraph::assert_fact
    pub fn assert_fact_with_confidence(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        valid_from: KronroeTimestamp,
        confidence: f32,
    ) -> Result<FactId> {
        let fact = Self::build_fact(
            subject,
            predicate,
            object.into(),
            valid_from,
            confidence,
            None,
        )?;
        let fact_id = fact.id.clone();
        self.storage.write_fact(&fact)?;
        Ok(fact_id)
    }

    /// Assert a new fact with explicit confidence and source provenance.
    ///
    /// Like [`assert_fact_with_confidence`] but also records where the fact
    /// came from. The source string is free-form (e.g. `"user:alice"`,
    /// `"api:weather"`, `"episode:conv-42"`).
    ///
    /// [`assert_fact_with_confidence`]: TemporalGraph::assert_fact_with_confidence
    pub fn assert_fact_with_source(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        valid_from: KronroeTimestamp,
        confidence: f32,
        source: &str,
    ) -> Result<FactId> {
        let fact = Self::build_fact(
            subject,
            predicate,
            object.into(),
            valid_from,
            confidence,
            Some(source),
        )?;
        let fact_id = fact.id.clone();
        self.storage.write_fact(&fact)?;
        Ok(fact_id)
    }

    /// Assert a new fact with idempotency-key deduplication.
    ///
    /// If `idempotency_key` has already been used, returns the original
    /// [`FactId`] without creating a new fact row. Otherwise, creates a new fact
    /// and stores the key -> fact mapping atomically in the same transaction.
    pub fn assert_fact_idempotent(
        &self,
        idempotency_key: &str,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        valid_from: KronroeTimestamp,
    ) -> Result<FactId> {
        if let Some(existing_id) = self.storage.get_idempotency(idempotency_key)? {
            return Ok(existing_id);
        }

        let fact = Self::build_fact(subject, predicate, object.into(), valid_from, 1.0, None)?;
        self.storage
            .write_fact_and_idempotency(idempotency_key, &fact)
    }

    /// Get all currently valid facts for `(subject, predicate)`.
    ///
    /// A fact is currently valid if both `valid_to` and `expired_at` are `None`.
    pub fn current_facts(&self, subject: &str, predicate: &str) -> Result<Vec<Fact>> {
        Ok(self
            .storage
            .current_facts(subject, predicate)?
            .into_iter()
            .map(|row| row.fact)
            .collect())
    }

    /// Get all facts valid at a given point in time for `(subject, predicate)`.
    ///
    /// Uses the **valid time** axis: queries when something was true in the
    /// world, regardless of when it was recorded.
    pub fn facts_at(
        &self,
        subject: &str,
        predicate: &str,
        at: KronroeTimestamp,
    ) -> Result<Vec<Fact>> {
        Ok(self
            .storage
            .facts_at(subject, predicate, at)?
            .into_iter()
            .map(|row| row.fact)
            .collect())
    }

    /// Get every fact ever recorded for an entity, across all predicates.
    pub fn all_facts_about(&self, subject: &str) -> Result<Vec<Fact>> {
        let prefix = format!("{}:", subject);
        self.scan_prefix(&prefix, |_| true)
    }

    /// Full-text search over entity names, aliases, predicates, and string values.
    ///
    /// Phase 0 implementation: builds an in-memory index at query time.
    /// This keeps search self-contained while we validate relevance behavior.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Fact>> {
        self.search_scored(query, limit)
            .map(|scored| scored.into_iter().map(|(fact, _)| fact).collect())
    }

    /// Full-text search returning facts with Kronroe BM25 relevance scores.
    ///
    /// Each result is a `(Fact, f32)` pair where the `f32` is the BM25 score
    /// from the full-text engine. Higher scores indicate stronger lexical
    /// relevance to the query. Scores are comparable within a single query's
    /// result set but not across different queries.
    pub fn search_scored(&self, query: &str, limit: usize) -> Result<Vec<(Fact, f32)>> {
        #[cfg(not(feature = "fulltext"))]
        {
            let _ = (query, limit);
            Err(KronroeError::search(
                "fulltext feature is disabled for this build".to_string(),
            ))
        }

        #[cfg(feature = "fulltext")]
        {
            if query.trim().is_empty() || limit == 0 {
                return Ok(Vec::new());
            }

            let facts = self.scan_prefix("", |_| true)?;
            if facts.is_empty() {
                return Ok(Vec::new());
            }

            let aliases_by_subject = self.alias_map(&facts);
            let docs: Vec<lexical::LexicalDocument> = facts
                .iter()
                .map(|fact| {
                    lexical::LexicalDocument::new(
                        fact.id.clone(),
                        Self::search_document_content(
                            fact,
                            aliases_by_subject.get(fact.subject.as_str()),
                        ),
                    )
                })
                .collect();
            let top_docs = lexical::search_scored(&docs, query, limit);

            let facts_by_id: HashMap<FactId, Fact> =
                facts.into_iter().map(|f| (f.id.clone(), f)).collect();
            let mut results = Vec::new();

            for (fact_id, score) in top_docs {
                if let Some(fact) = facts_by_id.get(&fact_id) {
                    results.push((fact.clone(), score));
                }
            }

            Ok(results)
        }
    }

    /// Invalidate a fact by closing both its valid-time and transaction-time
    /// windows (sets `valid_to` and `expired_at` to `at`).
    ///
    /// The fact is not deleted — its history is preserved. After invalidation,
    /// the fact will no longer appear in `current_facts()` but will still be
    /// returned by `facts_at()` for timestamps before `at`.
    pub fn invalidate_fact(&self, fact_id: impl AsRef<str>, at: KronroeTimestamp) -> Result<()> {
        let fact_id = self.resolve_fact_id_input(fact_id.as_ref())?;
        let found = self.storage.fact_by_id(&fact_id)?;

        match found {
            Some(row) => {
                let mut fact = row.fact;
                fact.valid_to = Some(at);
                fact.expired_at = Some(at);
                self.storage.replace_fact_row(&row.key, &fact)?;
                Ok(())
            }
            _ => Err(KronroeError::not_found(format!(
                "fact id {}",
                fact_id.as_str()
            ))),
        }
    }

    /// Retrieve a fact by its id.
    ///
    /// Phase 0 implementation performs a linear scan.
    pub fn fact_by_id(&self, fact_id: impl AsRef<str>) -> Result<Fact> {
        let fact_id = self.resolve_fact_id_input(fact_id.as_ref())?;
        self.storage
            .fact_by_id(&fact_id)?
            .map(|row| row.fact)
            .ok_or_else(|| KronroeError::not_found(format!("fact id {}", fact_id.as_str())))
    }

    /// Correct a fact by id while preserving history.
    ///
    /// The old fact is invalidated at `at`, and a replacement fact is asserted
    /// with the same subject/predicate and a new object value.
    pub fn correct_fact(
        &self,
        fact_id: impl AsRef<str>,
        new_value: impl Into<Value>,
        at: KronroeTimestamp,
    ) -> Result<FactId> {
        let fact_id = fact_id.as_ref();
        let old = self.fact_by_id(fact_id)?;
        self.invalidate_fact(fact_id, at)?;
        self.assert_fact(&old.subject, &old.predicate, new_value, at)
    }

    // -----------------------------------------------------------------------
    // Contradiction detection
    // -----------------------------------------------------------------------

    /// Register a predicate as a singleton with the given conflict policy.
    ///
    /// Singleton predicates allow at most one active value per subject at any
    /// point in valid time. When a new fact is asserted via
    /// [`assert_fact_checked`], the engine checks for contradictions against
    /// existing facts for the same `(subject, predicate)` pair.
    ///
    /// The registration is persisted to the database and survives reopens.
    ///
    /// [`assert_fact_checked`]: TemporalGraph::assert_fact_checked
    #[cfg(feature = "contradiction")]
    pub fn register_singleton_predicate(
        &self,
        predicate: &str,
        policy: ConflictPolicy,
    ) -> Result<()> {
        let cardinality = PredicateCardinality::Singleton;
        let encoded = contradiction::encode_predicate_registry(cardinality, policy);
        self.storage
            .write_predicate_registry_entry(predicate, encoded.as_str())?;

        let mut det = self
            .contradiction_detector
            .lock()
            .map_err(|e| KronroeError::internal(e.to_string()))?;
        det.register(predicate, cardinality, policy);
        Ok(())
    }

    /// Check whether a predicate is already registered as a singleton.
    #[cfg(feature = "contradiction")]
    pub fn is_singleton_predicate(&self, predicate: &str) -> Result<bool> {
        let det = self
            .contradiction_detector
            .lock()
            .map_err(|e| KronroeError::internal(e.to_string()))?;
        Ok(det.is_singleton(predicate))
    }

    /// List all registered singleton predicates.
    #[cfg(feature = "contradiction")]
    pub fn singleton_predicates(&self) -> Result<Vec<String>> {
        let det = self
            .contradiction_detector
            .lock()
            .map_err(|e| KronroeError::internal(e.to_string()))?;
        Ok(det.singleton_predicates().map(String::from).collect())
    }

    /// Detect contradictions for a specific `(subject, predicate)` pair.
    ///
    /// Scans all non-expired facts (including bounded-interval facts with
    /// `valid_to` set) for the given subject and predicate and returns
    /// pairwise contradictions. Only checks if the predicate is registered
    /// as a singleton.
    #[cfg(feature = "contradiction")]
    pub fn detect_contradictions(
        &self,
        subject: &str,
        predicate: &str,
    ) -> Result<Vec<Contradiction>> {
        // Copy singleton check out of the lock, then drop before I/O.
        let is_singleton = {
            let det = self
                .contradiction_detector
                .lock()
                .map_err(|e| KronroeError::internal(e.to_string()))?;
            det.is_singleton(predicate)
        };
        if !is_singleton {
            return Ok(Vec::new());
        }

        // Include bounded-interval facts (valid_to set), not just open-ended.
        let prefix = format!("{subject}:{predicate}:");
        let facts = self.scan_prefix(&prefix, |f| f.expired_at.is_none())?;
        let mut contradictions = Vec::new();
        for i in 0..facts.len() {
            for j in (i + 1)..facts.len() {
                if let Some(c) = contradiction::detect_pairwise(&facts[i], &facts[j]) {
                    contradictions.push(c);
                }
            }
        }
        Ok(contradictions)
    }

    /// Detect all contradictions across every registered singleton predicate.
    ///
    /// Performs a full scan of the facts table and checks all registered
    /// singleton predicates for pairwise contradictions.
    #[cfg(feature = "contradiction")]
    pub fn detect_all_contradictions(&self) -> Result<Vec<Contradiction>> {
        let det = self
            .contradiction_detector
            .lock()
            .map_err(|e| KronroeError::internal(e.to_string()))?;

        let singleton_preds: Vec<String> = det.singleton_predicates().map(String::from).collect();
        drop(det); // Release lock before scan.

        if singleton_preds.is_empty() {
            return Ok(Vec::new());
        }

        // Collect all active facts grouped by (subject, predicate).
        let all_facts = self.scan_prefix("", |f| f.expired_at.is_none())?;
        let mut groups: std::collections::HashMap<(String, String), Vec<Fact>> =
            std::collections::HashMap::new();
        for fact in all_facts {
            if singleton_preds.contains(&fact.predicate) {
                groups
                    .entry((fact.subject.clone(), fact.predicate.clone()))
                    .or_default()
                    .push(fact);
            }
        }

        let mut contradictions = Vec::new();
        for ((_subj, _pred), facts) in &groups {
            for i in 0..facts.len() {
                for j in (i + 1)..facts.len() {
                    if let Some(c) = contradiction::detect_pairwise(&facts[i], &facts[j]) {
                        contradictions.push(c);
                    }
                }
            }
        }
        Ok(contradictions)
    }

    /// Assert a fact with contradiction checking.
    ///
    /// Behavior depends on the predicate's [`ConflictPolicy`]:
    /// - **Allow**: stores the fact, returns `(fact_id, [])`.
    /// - **Warn**: stores the fact, returns `(fact_id, contradictions)`.
    /// - **Reject**: if contradictions exist, returns
    ///   `Err(ContradictionRejected(...))` and does **not** store the fact.
    ///
    /// For predicates not registered as singletons, this behaves identically
    /// to [`assert_fact`].
    ///
    /// # Atomicity
    ///
    /// The contradiction check and the write happen inside a single storage
    /// backend write boundary. This is race-free: no concurrent writer can insert a conflicting fact
    /// between the check and the insert.
    ///
    /// Note: the predicate's conflict policy is read from the in-memory
    /// detector *before* opening the write transaction. A concurrent
    /// `register_singleton_predicate` call could change the policy between
    /// that read and the write. In practice, predicates are registered once
    /// at startup (e.g. in `AgentMemory::open`), not reconfigured at runtime.
    ///
    /// Also note: [`assert_fact`] bypasses contradiction checking entirely.
    /// Strict singleton enforcement only applies when callers use this method.
    ///
    /// [`assert_fact`]: TemporalGraph::assert_fact
    #[cfg(feature = "contradiction")]
    pub fn assert_fact_checked(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        valid_from: KronroeTimestamp,
    ) -> Result<(FactId, Vec<Contradiction>)> {
        let object = object.into();

        let det = self
            .contradiction_detector
            .lock()
            .map_err(|e| KronroeError::internal(e.to_string()))?;
        let policy = det.policy_for(predicate);
        let is_singleton = det.is_singleton(predicate);
        drop(det); // Release detector lock before I/O.

        if !is_singleton || matches!(policy, ConflictPolicy::Allow) {
            let fact_id = self.assert_fact(subject, predicate, object, valid_from)?;
            return Ok((fact_id, Vec::new()));
        }

        let fact = Self::build_fact(subject, predicate, object, valid_from, 1.0, None)?;
        let fact_id = fact.id.clone();
        let reject_on_conflict = matches!(policy, ConflictPolicy::Reject);
        let contradictions = self.storage.write_fact_with_contradiction_check(
            subject,
            predicate,
            &fact,
            reject_on_conflict,
            |existing| {
                let det = self
                    .contradiction_detector
                    .lock()
                    .map_err(|e| KronroeError::internal(e.to_string()))?;
                Ok(det.check_against(&fact, existing))
            },
        )?;
        Ok((fact_id, contradictions))
    }

    /// Assert a fact and durably persist its embedding in a single ACID transaction.
    ///
    /// The fact row, the embedding dimension check-and-set, and the raw embedding
    /// bytes are all written inside one backend write boundary and committed
    /// atomically. The in-memory vector index cache is updated *after* the commit,
    /// so persisted storage is always the source of truth.
    ///
    /// Because the backend serialises the dimension check-and-set, it is
    /// race-free: no two concurrent callers can simultaneously establish different
    /// dimensions on the first insert.
    ///
    /// **Caller responsibility:** Kronroe does not generate embeddings. The caller
    /// (e.g. `kronroe-agent-memory` or the application) must compute `embedding`
    /// before calling this method.
    ///
    /// # Errors
    ///
    /// Returns [`KronroeError::InvalidEmbedding`] if:
    /// - `embedding` is empty, or
    /// - `embedding.len()` differs from the dimension established by the first
    ///   embedding ever inserted into this database.
    ///
    /// [`assert_fact`]: TemporalGraph::assert_fact
    /// [`search_by_vector`]: TemporalGraph::search_by_vector
    #[cfg(feature = "vector")]
    pub fn assert_fact_with_embedding(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        valid_from: KronroeTimestamp,
        embedding: Vec<f32>,
    ) -> Result<FactId> {
        if embedding.is_empty() {
            return Err(KronroeError::invalid_embedding(
                "embedding must not be empty",
            ));
        }

        let fact = Self::build_fact(subject, predicate, object.into(), valid_from, 1.0, None)?;
        let fact_id = fact.id.clone();
        self.storage.write_fact_with_embedding(&fact, &embedding)?;

        // Update the in-memory cache after the durable commit.
        // If the process crashes between commit() and here the cache is rebuilt
        // correctly from storage on the next open().
        self.vector_index
            .lock()
            .map_err(|_| KronroeError::internal("vector index lock poisoned"))?
            .insert(fact_id.clone(), embedding)?;

        Ok(fact_id)
    }

    /// Search for facts semantically similar to `query`, optionally filtered to
    /// those valid at a given point in time.
    ///
    /// Results are sorted by cosine similarity in descending order (most similar
    /// first). At most `k` results are returned.
    ///
    /// Pass `at = None` to restrict results to currently-valid facts (both
    /// `valid_to` and `expired_at` are `None`). Pass `at = Some(t)` to use the
    /// valid-time axis: facts that were true in the world at time `t`.
    ///
    /// Only facts that were previously inserted with
    /// [`assert_fact_with_embedding`] can be returned — facts asserted via
    /// [`assert_fact`] have no embedding and are invisible to this method.
    ///
    /// [`assert_fact_with_embedding`]: TemporalGraph::assert_fact_with_embedding
    /// [`assert_fact`]: TemporalGraph::assert_fact
    #[cfg(feature = "vector")]
    pub fn search_by_vector(
        &self,
        query: &[f32],
        k: usize,
        at: Option<KronroeTimestamp>,
    ) -> Result<Vec<(Fact, f32)>> {
        use std::collections::{HashMap, HashSet};

        // Validate query dimension against the established index dimension.
        // Return a clear error rather than silently producing zero-scored results
        // (which `cosine_similarity` would return for mismatched lengths).
        {
            let idx = self
                .vector_index
                .lock()
                .map_err(|_| KronroeError::internal("vector index lock poisoned"))?;
            if let Some(d) = idx.dim() {
                if query.len() != d {
                    return Err(KronroeError::invalid_embedding(format!(
                        "query dimension mismatch: index has dim {d}, query has {}",
                        query.len()
                    )));
                }
            }
        }

        // Collect all facts passing the temporal filter, then build an allow-set
        // for the vector index and a lookup map for hydrating results.
        let matching_facts = self.scan_prefix("", |f| match at {
            Some(t) => f.was_valid_at(t),
            None => f.is_currently_valid(),
        })?;

        let valid_ids: HashSet<FactId> = matching_facts.iter().map(|f| f.id.clone()).collect();
        let facts_by_id: HashMap<FactId, Fact> = matching_facts
            .into_iter()
            .map(|f| (f.id.clone(), f))
            .collect();

        let hits = self
            .vector_index
            .lock()
            .map_err(|_| KronroeError::internal("vector index lock poisoned"))?
            .search(query, k, &valid_ids);

        let results = hits
            .into_iter()
            .filter_map(|(id, score)| facts_by_id.get(&id).map(|f| (f.clone(), score)))
            .collect();

        Ok(results)
    }

    #[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
    fn search_ranked(&self, query: &str, limit: usize) -> Result<Vec<(FactId, usize)>> {
        #[cfg(not(feature = "fulltext"))]
        {
            let _ = (query, limit);
            return Ok(Vec::new());
        }

        #[cfg(feature = "fulltext")]
        {
            if query.trim().is_empty() || limit == 0 {
                return Ok(Vec::new());
            }

            let facts = self.search(query, limit)?;

            Ok(facts
                .into_iter()
                .enumerate()
                .map(|(rank, fact)| (fact.id, rank))
                .collect())
        }
    }

    #[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
    fn search_by_vector_ranked(
        &self,
        query: &[f32],
        limit: usize,
        at: Option<KronroeTimestamp>,
    ) -> Result<Vec<(FactId, usize)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let hits = self.search_by_vector(query, limit, at)?;
        Ok(hits
            .into_iter()
            .enumerate()
            .map(|(rank, (fact, _score))| (fact.id, rank))
            .collect())
    }

    /// Hybrid retrieval: RRF fusion + two-stage intent-gated reranking.
    ///
    /// Combines full-text and vector search channels via Reciprocal Rank Fusion,
    /// then applies a two-stage reranker (semantic pruning → temporal feasibility).
    ///
    /// Callers provide [`TemporalIntent`] and [`TemporalOperator`] to express what
    /// kind of time query they're making; the reranker adapts its scoring strategy
    /// accordingly.
    ///
    /// For timeless queries, an adaptive vector-dominance path adjusts weights
    /// based on the signal balance in the top candidates. For temporal queries,
    /// the reranker applies feasibility filtering and intent-weighted scoring.
    #[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
    pub fn search_hybrid(
        &self,
        text_query: &str,
        vector_query: &[f32],
        params: HybridSearchParams,
        at: Option<KronroeTimestamp>,
    ) -> Result<Vec<(Fact, HybridScoreBreakdown)>> {
        // ── Validation ──────────────────────────────────────────────────
        if params.k == 0 {
            return Err(KronroeError::search(
                "search_hybrid: `k` must be >= 1".to_string(),
            ));
        }
        if params.candidate_window == 0 {
            return Err(KronroeError::search(
                "search_hybrid: `candidate_window` must be >= 1".to_string(),
            ));
        }
        if params.rank_constant < 1 {
            return Err(KronroeError::search(
                "search_hybrid: `rank_constant` must be >= 1".to_string(),
            ));
        }
        if params.text_weight < 0.0 || params.vector_weight < 0.0 {
            return Err(KronroeError::search(
                "search_hybrid: weights must be non-negative".to_string(),
            ));
        }
        if params.text_weight == 0.0 && params.vector_weight == 0.0 {
            return Err(KronroeError::search(
                "search_hybrid: at least one of `text_weight` or `vector_weight` must be > 0"
                    .to_string(),
            ));
        }

        // ── Stage 0: Reciprocal Rank Fusion ─────────────────────────────
        let window = params.candidate_window;
        let text_ranked = self.search_ranked(text_query, window)?;
        let vec_ranked = self.search_by_vector_ranked(vector_query, window, at)?;

        let rank_constant = params.rank_constant as f64;
        let mut by_id: HashMap<FactId, HybridScoreBreakdown> = HashMap::new();

        for (fact_id, rank) in text_ranked {
            let contrib = params.text_weight as f64 / (rank_constant + (rank + 1) as f64);
            let entry = by_id.entry(fact_id).or_insert(HybridScoreBreakdown {
                final_score: 0.0,
                text_rrf_contrib: 0.0,
                vector_rrf_contrib: 0.0,
                temporal_adjustment: 0.0,
            });
            entry.text_rrf_contrib += contrib;
            entry.final_score += contrib;
        }

        for (fact_id, rank) in vec_ranked {
            let contrib = params.vector_weight as f64 / (rank_constant + (rank + 1) as f64);
            let entry = by_id.entry(fact_id).or_insert(HybridScoreBreakdown {
                final_score: 0.0,
                text_rrf_contrib: 0.0,
                vector_rrf_contrib: 0.0,
                temporal_adjustment: 0.0,
            });
            entry.vector_rrf_contrib += contrib;
            entry.final_score += contrib;
        }

        let mut fused: Vec<(FactId, HybridScoreBreakdown)> = by_id.into_iter().collect();

        // Sort by RRF score descending, FactId ascending for deterministic ties.
        fused.sort_by(|(a_id, a), (b_id, b)| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a_id.cmp(b_id))
        });
        fused.truncate(window);

        // Resolve FactIds to full Facts for the reranker.
        let mut resolved = Vec::with_capacity(fused.len());
        for (fact_id, breakdown) in fused {
            let fact = self.fact_by_id(&fact_id)?;
            resolved.push((fact, breakdown));
        }

        // ── Stages 1+2: Two-stage reranker ──────────────────────────────
        #[cfg(feature = "uncertainty")]
        let reranked = {
            let engine = self
                .uncertainty_engine
                .lock()
                .map_err(|_| KronroeError::internal("uncertainty engine lock poisoned"))?;
            hybrid::rerank_two_stage_with_uncertainty(
                resolved,
                params.k,
                params.intent,
                params.operator,
                at,
                &engine,
            )
        };
        #[cfg(not(feature = "uncertainty"))]
        let reranked =
            hybrid::rerank_two_stage(resolved, params.k, params.intent, params.operator, at);

        Ok(reranked)
    }

    // Internal: scan facts table, filter by prefix, apply predicate.
    fn scan_prefix(&self, prefix: &str, predicate: impl Fn(&Fact) -> bool) -> Result<Vec<Fact>> {
        let mut results = Vec::new();
        for row in self.storage.scan_facts(prefix)? {
            if predicate(&row.fact) {
                results.push(row.fact);
            }
        }

        Ok(results)
    }

    #[cfg(feature = "fulltext")]
    fn alias_map(&self, facts: &[Fact]) -> HashMap<String, Vec<String>> {
        let mut aliases_by_subject: HashMap<String, Vec<String>> = HashMap::new();
        for fact in facts {
            let is_alias_predicate = fact.predicate == "alias"
                || fact.predicate == "has_alias"
                || fact.predicate == "aka";
            if is_alias_predicate {
                if let Value::Text(alias) | Value::Entity(alias) = &fact.object {
                    aliases_by_subject
                        .entry(fact.subject.clone())
                        .or_default()
                        .push(alias.clone());
                }
            }
        }
        aliases_by_subject
    }

    #[cfg(feature = "fulltext")]
    fn search_document_content(fact: &Fact, aliases: Option<&Vec<String>>) -> String {
        let mut content_parts = vec![fact.subject.as_str(), fact.predicate.as_str()];
        if let Some(aliases) = aliases {
            for alias in aliases {
                content_parts.push(alias.as_str());
            }
        }
        if let Value::Text(v) | Value::Entity(v) = &fact.object {
            content_parts.push(v.as_str());
        }

        let normalized_predicate = fact.predicate.replace('_', " ");
        format!("{} {}", content_parts.join(" "), normalized_predicate)
    }

    // -----------------------------------------------------------------------
    // Uncertainty model
    // -----------------------------------------------------------------------

    /// Register a volatility profile for a predicate.
    ///
    /// Facts with this predicate will decay in effective confidence over time
    /// according to the half-life. For example, `works_at` with a 730-day
    /// half-life means after 2 years the age-decay multiplier is 0.5.
    ///
    /// The registration is persisted to the database and survives restarts.
    #[cfg(feature = "uncertainty")]
    pub fn register_predicate_volatility(
        &self,
        predicate: &str,
        volatility: uncertainty::PredicateVolatility,
    ) -> Result<()> {
        let encoded = volatility.to_json_string();
        self.storage
            .write_volatility_registry_entry(predicate, encoded.as_str())?;
        let mut engine = self
            .uncertainty_engine
            .lock()
            .map_err(|_| KronroeError::internal("uncertainty engine lock poisoned"))?;
        engine.register_volatility(predicate, volatility);
        Ok(())
    }

    /// Return the configured volatility for a predicate, if any.
    #[cfg(feature = "uncertainty")]
    pub fn predicate_volatility(
        &self,
        predicate: &str,
    ) -> Result<Option<uncertainty::PredicateVolatility>> {
        let engine = self
            .uncertainty_engine
            .lock()
            .map_err(|_| KronroeError::internal("uncertainty engine lock poisoned"))?;
        Ok(engine.volatility_for(predicate).cloned())
    }

    /// Register an authority weight for a source identifier.
    ///
    /// Facts with this source will have their effective confidence multiplied
    /// by the weight. Values > 1.0 boost (trusted source), < 1.0 penalise.
    ///
    /// The registration is persisted to the database and survives restarts.
    #[cfg(feature = "uncertainty")]
    pub fn register_source_weight(
        &self,
        source: &str,
        weight: uncertainty::SourceWeight,
    ) -> Result<()> {
        let encoded = weight.to_json_string();
        self.storage
            .write_source_weight_registry_entry(source, encoded.as_str())?;
        let mut engine = self
            .uncertainty_engine
            .lock()
            .map_err(|_| KronroeError::internal("uncertainty engine lock poisoned"))?;
        engine.register_source_weight(source, weight);
        Ok(())
    }

    /// Return the configured source weight, if any.
    #[cfg(feature = "uncertainty")]
    pub fn source_weight(&self, source: &str) -> Result<Option<uncertainty::SourceWeight>> {
        let engine = self
            .uncertainty_engine
            .lock()
            .map_err(|_| KronroeError::internal("uncertainty engine lock poisoned"))?;
        Ok(engine.source_weight_for(source).cloned())
    }

    /// Compute the effective confidence of a fact at a given point in time.
    ///
    /// Effective confidence = base confidence × age decay × source weight,
    /// clamped to \[0.0, 1.0\]. Age is measured from `valid_from`.
    ///
    /// Returns an [`EffectiveConfidence`] with the final value and breakdown.
    #[cfg(feature = "uncertainty")]
    pub fn effective_confidence(
        &self,
        fact: &Fact,
        at: KronroeTimestamp,
    ) -> Result<uncertainty::EffectiveConfidence> {
        let engine = self
            .uncertainty_engine
            .lock()
            .map_err(|_| KronroeError::internal("uncertainty engine lock poisoned"))?;
        Ok(engine.effective_confidence(fact, at))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::NamedTempFile;

    fn open_temp_db() -> (TemporalGraph, NamedTempFile) {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_str().unwrap().to_string();
        let db = TemporalGraph::open(&path).unwrap();
        (db, file)
    }

    fn dt(s: &str) -> KronroeTimestamp {
        s.parse().unwrap()
    }

    fn append_bytes(path: &str, bytes: &[u8]) {
        let mut data = fs::read(path).unwrap();
        data.extend_from_slice(bytes);
        fs::write(path, data).unwrap();
    }

    fn insert_bytes_after_nth_newline(path: &str, newline_index: usize, bytes: &[u8]) {
        let data = fs::read(path).unwrap();
        let mut seen = 0usize;
        let mut insert_at = None;
        for (index, byte) in data.iter().enumerate() {
            if *byte == b'\n' {
                if seen == newline_index {
                    insert_at = Some(index + 1);
                    break;
                }
                seen += 1;
            }
        }
        let insert_at = insert_at.expect("newline should exist in append-log fixture");
        let mut mutated = Vec::with_capacity(data.len() + bytes.len());
        mutated.extend_from_slice(&data[..insert_at]);
        mutated.extend_from_slice(bytes);
        mutated.extend_from_slice(&data[insert_at..]);
        fs::write(path, mutated).unwrap();
    }

    #[test]
    fn assert_and_retrieve_current_fact() {
        let (db, _tmp) = open_temp_db();
        db.assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
            .unwrap();

        let facts = db.current_facts("alice", "works_at").unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].subject, "alice");
        assert_eq!(facts[0].predicate, "works_at");
        match &facts[0].object {
            Value::Text(s) => assert_eq!(s, "Acme"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn idempotent_assert_same_key_returns_same_fact_id() {
        let (db, _tmp) = open_temp_db();
        let now = KronroeTimestamp::now_utc();

        let first = db
            .assert_fact_idempotent("evt-123", "alice", "works_at", "Acme", now)
            .unwrap();
        let second = db
            .assert_fact_idempotent("evt-123", "alice", "works_at", "Acme", now)
            .unwrap();

        assert_eq!(first, second, "same idempotency key must dedupe");
        let all = db.all_facts_about("alice").unwrap();
        assert_eq!(all.len(), 1, "same key must not create extra fact rows");
    }

    #[test]
    fn idempotent_assert_different_keys_create_different_facts() {
        let (db, _tmp) = open_temp_db();
        let now = KronroeTimestamp::now_utc();

        let first = db
            .assert_fact_idempotent("evt-aaa", "alice", "works_at", "Acme", now)
            .unwrap();
        let second = db
            .assert_fact_idempotent("evt-bbb", "alice", "works_at", "Acme", now)
            .unwrap();

        assert_ne!(
            first, second,
            "different keys must produce different fact ids"
        );
        let all = db.all_facts_about("alice").unwrap();
        assert_eq!(all.len(), 2, "different keys must create independent facts");
    }

    #[test]
    fn idempotent_assert_survives_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("idempotency-reopen.kronroe");
        let path_str = path.to_str().unwrap();
        let now = KronroeTimestamp::now_utc();

        let first_id = {
            let db = TemporalGraph::open(path_str).unwrap();
            db.assert_fact_idempotent("evt-reopen", "alice", "works_at", "Acme", now)
                .unwrap()
        };

        let second_id = {
            let db = TemporalGraph::open(path_str).unwrap();
            db.assert_fact_idempotent("evt-reopen", "alice", "works_at", "Acme", now)
                .unwrap()
        };

        assert_eq!(
            first_id, second_id,
            "idempotency mapping must persist across reopen"
        );

        let db = TemporalGraph::open(path_str).unwrap();
        let facts = db.all_facts_about("alice").unwrap();
        assert_eq!(facts.len(), 1, "reopen + retry must not duplicate facts");
    }

    #[test]
    fn point_in_time_query() {
        let (db, _tmp) = open_temp_db();
        let jan = dt("2024-01-01T00:00:00Z");
        let mar = dt("2024-03-01T00:00:00Z");
        let dec_prev = dt("2023-12-01T00:00:00Z");

        db.assert_fact("alice", "works_at", "Acme", jan).unwrap();

        // Was valid in March (after valid_from)
        let in_march = db.facts_at("alice", "works_at", mar).unwrap();
        assert_eq!(in_march.len(), 1, "should find 1 fact valid in March");

        // Not yet valid before January
        let before_start = db.facts_at("alice", "works_at", dec_prev).unwrap();
        assert_eq!(
            before_start.len(),
            0,
            "should find no facts before valid_from"
        );
    }

    #[test]
    fn fact_invalidation_preserves_history() {
        let (db, _tmp) = open_temp_db();
        let jan = dt("2024-01-01T00:00:00Z");
        let jun = dt("2024-06-01T00:00:00Z");
        let mar = dt("2024-03-01T00:00:00Z");

        let id = db.assert_fact("alice", "works_at", "Acme", jan).unwrap();
        db.invalidate_fact(&id, jun).unwrap();

        // No longer current
        let current = db.current_facts("alice", "works_at").unwrap();
        assert_eq!(
            current.len(),
            0,
            "fact should no longer be current after invalidation"
        );

        // But history is preserved: still valid in March
        let in_march = db.facts_at("alice", "works_at", mar).unwrap();
        assert_eq!(
            in_march.len(),
            1,
            "historical fact should still be retrievable"
        );

        // Not valid after June (when it was invalidated)
        let after_invalidation = db
            .facts_at("alice", "works_at", dt("2024-09-01T00:00:00Z"))
            .unwrap();
        assert_eq!(
            after_invalidation.len(),
            0,
            "fact should not appear after valid_to"
        );
    }

    #[test]
    fn all_facts_about_entity() {
        let (db, _tmp) = open_temp_db();
        let now = KronroeTimestamp::now_utc();

        db.assert_fact("alice", "works_at", "Acme", now).unwrap();
        db.assert_fact("alice", "has_role", "Engineer", now)
            .unwrap();
        db.assert_fact("alice", "has_skill", "Rust", now).unwrap();
        db.assert_fact("bob", "works_at", "Acme", now).unwrap(); // different subject

        let alice_facts = db.all_facts_about("alice").unwrap();
        assert_eq!(
            alice_facts.len(),
            3,
            "should return all 3 facts about alice"
        );

        let subjects: Vec<&str> = alice_facts.iter().map(|f| f.subject.as_str()).collect();
        assert!(subjects.iter().all(|&s| s == "alice"));
    }

    #[test]
    fn value_types() {
        let (db, _tmp) = open_temp_db();
        let now = KronroeTimestamp::now_utc();

        db.assert_fact("alice", "confidence_score", 0.95_f64, now)
            .unwrap();
        db.assert_fact("alice", "is_active", true, now).unwrap();

        let score_facts = db.current_facts("alice", "confidence_score").unwrap();
        assert_eq!(score_facts.len(), 1);
        match score_facts[0].object {
            Value::Number(n) => assert!((n - 0.95).abs() < 1e-9),
            ref other => panic!("expected Number, got {other:?}"),
        }

        let bool_facts = db.current_facts("alice", "is_active").unwrap();
        assert_eq!(bool_facts.len(), 1);
        assert!(matches!(bool_facts[0].object, Value::Boolean(true)));
    }

    #[test]
    fn correct_fact_preserves_history_and_creates_replacement() {
        let (db, _tmp) = open_temp_db();
        let jan = dt("2024-01-01T00:00:00Z");
        let feb = dt("2024-02-01T00:00:00Z");

        let old_id = db.assert_fact("alice", "works_at", "Acme", jan).unwrap();
        let new_id = db.correct_fact(&old_id, "BetaCorp", feb).unwrap();

        let old = db.fact_by_id(&old_id).unwrap();
        assert_eq!(old.valid_to, Some(feb));
        assert_eq!(
            old.expired_at,
            Some(feb),
            "corrected fact should have expired_at set"
        );

        let new_fact = db.fact_by_id(&new_id).unwrap();
        assert_eq!(new_fact.subject, "alice");
        assert_eq!(new_fact.predicate, "works_at");
        match new_fact.object {
            Value::Text(ref s) => assert_eq!(s, "BetaCorp"),
            ref other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "vector")]
    fn vector_search_returns_most_similar_current_facts() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let now = KronroeTimestamp::now_utc();

        // Three facts with distinct embedding directions.
        // Query [1,0,0] should rank id0 first.
        let id0 = db
            .assert_fact_with_embedding("alice", "interest", "Rust", now, vec![1.0, 0.0, 0.0])
            .unwrap();
        let _id1 = db
            .assert_fact_with_embedding("alice", "interest", "Python", now, vec![0.0, 1.0, 0.0])
            .unwrap();
        let _id2 = db
            .assert_fact_with_embedding("alice", "interest", "Go", now, vec![0.0, 0.0, 1.0])
            .unwrap();

        let results = db.search_by_vector(&[1.0, 0.0, 0.0], 1, None).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.id, id0);
        assert!((results[0].1 - 1.0).abs() < 1e-6, "score should be ~1.0");
    }

    #[test]
    #[cfg(feature = "vector")]
    fn vector_search_respects_temporal_filter() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let jan = "2024-01-01T00:00:00Z".parse::<KronroeTimestamp>().unwrap();
        let jul = "2024-07-01T00:00:00Z".parse::<KronroeTimestamp>().unwrap();
        let mar = "2024-03-01T00:00:00Z".parse::<KronroeTimestamp>().unwrap();

        // Fact valid from Jan, invalidated at Jul.
        let id_old = db
            .assert_fact_with_embedding("alice", "interest", "Rust", jan, vec![1.0, 0.0])
            .unwrap();
        db.invalidate_fact(&id_old, jul).unwrap();

        // Fact valid from Jul onward.
        let _id_new = db
            .assert_fact_with_embedding("alice", "interest", "Python", jul, vec![0.0, 1.0])
            .unwrap();

        // At March: only old fact is valid.
        let at_mar = db.search_by_vector(&[1.0, 0.0], 10, Some(mar)).unwrap();
        assert_eq!(at_mar.len(), 1);
        assert_eq!(at_mar[0].0.id, id_old);

        // Currently (no at): old is invalidated, only new is current.
        let current = db.search_by_vector(&[0.0, 1.0], 10, None).unwrap();
        assert_eq!(current.len(), 1);
        assert!(matches!(current[0].0.object, Value::Text(ref s) if s == "Python"));
    }

    #[test]
    #[cfg(feature = "vector")]
    fn vector_search_returns_empty_when_no_embeddings() {
        let db = TemporalGraph::open_in_memory().unwrap();
        // Assert a plain fact (no embedding).
        db.assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
            .unwrap();
        let results = db.search_by_vector(&[1.0, 0.0], 5, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    #[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
    fn hybrid_search_breakdown_sums_correctly() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let t = KronroeTimestamp::now_utc();
        db.assert_fact_with_embedding(
            "alice",
            "bio",
            "expert Rust systems programmer",
            t,
            vec![1.0, 0.0, 0.0],
        )
        .unwrap();
        db.assert_fact_with_embedding(
            "bob",
            "bio",
            "Python data scientist",
            t,
            vec![0.0, 1.0, 0.0],
        )
        .unwrap();

        let params = HybridSearchParams {
            k: 5,
            ..HybridSearchParams::default()
        };
        let hits = db
            .search_hybrid("Rust", &[1.0, 0.0, 0.0], params, None)
            .unwrap();
        assert!(!hits.is_empty(), "hybrid search should return results");

        for (_fact, breakdown) in &hits {
            let expected = breakdown.text_rrf_contrib
                + breakdown.vector_rrf_contrib
                + breakdown.temporal_adjustment;
            assert!(
                (breakdown.final_score - expected).abs() < 1e-9,
                "breakdown must sum to final_score"
            );
        }
    }

    #[test]
    #[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
    fn hybrid_search_rejects_zero_rank_constant() {
        let db = TemporalGraph::open_in_memory().unwrap();
        db.assert_fact_with_embedding(
            "alice",
            "bio",
            "Rust",
            KronroeTimestamp::now_utc(),
            vec![1.0, 0.0],
        )
        .unwrap();

        let bad = HybridSearchParams {
            rank_constant: 0,
            ..HybridSearchParams::default()
        };
        let result = db.search_hybrid("Rust", &[1.0, 0.0], bad, None);
        assert!(
            result.as_ref().is_err_and(|e| e.is_search()),
            "rank_constant=0 should return a validation error"
        );
    }

    #[test]
    #[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
    fn search_hybrid_returns_reranked_results() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let t = KronroeTimestamp::now_utc();
        db.assert_fact_with_embedding(
            "alice",
            "bio",
            "expert Rust systems programmer",
            t,
            vec![1.0, 0.0, 0.0],
        )
        .unwrap();
        db.assert_fact_with_embedding(
            "bob",
            "bio",
            "Python data scientist",
            t,
            vec![0.0, 1.0, 0.0],
        )
        .unwrap();
        db.assert_fact_with_embedding(
            "carol",
            "bio",
            "Rust and embedded systems",
            t,
            vec![0.9, 0.1, 0.0],
        )
        .unwrap();

        let params = HybridSearchParams {
            k: 3,
            ..HybridSearchParams::default()
        };
        let hits = db
            .search_hybrid("Rust", &[1.0, 0.0, 0.0], params, None)
            .unwrap();
        assert!(!hits.is_empty(), "search_hybrid should return results");
        assert!(hits.len() <= 3);
        // Both alice and carol match "Rust" in text and have high vector similarity
        // to [1,0,0]. Bob (Python, orthogonal vector) should rank last.
        let subjects: Vec<&str> = hits.iter().map(|(f, _)| f.subject.as_str()).collect();
        assert!(
            subjects[0] == "alice" || subjects[0] == "carol",
            "a Rust-related fact should rank first, got {subjects:?}"
        );
        if subjects.len() == 3 {
            assert_eq!(subjects[2], "bob", "bob should rank last, got {subjects:?}");
        }
    }

    #[test]
    #[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
    fn search_hybrid_temporal_query_filters_infeasible() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let jan2023 = dt("2023-01-01T00:00:00Z");
        let jan2024 = dt("2024-01-01T00:00:00Z");
        let jun2023 = dt("2023-06-01T00:00:00Z");

        // Alice at BetaCorp: 2023-01 to 2024-01
        db.assert_fact_with_embedding("alice", "works_at", "BetaCorp", jan2023, vec![1.0, 0.0])
            .unwrap();
        // Invalidate at jan2024 so it has a valid_to
        let id1 = db
            .current_facts("alice", "works_at")
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .id;
        db.invalidate_fact(&id1, jan2024).unwrap();

        // Alice at Acme: 2024-01 onwards
        db.assert_fact_with_embedding("alice", "works_at", "Acme", jan2024, vec![1.0, 0.0])
            .unwrap();

        // Query: "where did Alice work in mid-2023?" — BetaCorp was valid, Acme was not
        let params = HybridSearchParams {
            k: 5,
            intent: TemporalIntent::HistoricalPoint,
            operator: TemporalOperator::AsOf,
            ..HybridSearchParams::default()
        };
        let hits = db
            .search_hybrid("works_at", &[1.0, 0.0], params, Some(jun2023))
            .unwrap();
        assert!(!hits.is_empty(), "temporal query should return results");
        // BetaCorp (valid at jun2023) should rank first; Acme (not yet valid) is infeasible.
        let first_object = &hits[0].0.object;
        assert!(
            matches!(first_object, Value::Text(s) if s == "BetaCorp"),
            "BetaCorp should rank first for jun-2023 AsOf query, got {first_object:?}"
        );
    }

    #[test]
    #[cfg(all(feature = "hybrid-experimental", feature = "vector"))]
    fn search_hybrid_default_params_match_eval_winner() {
        let params = HybridSearchParams::default();
        assert_eq!(params.rank_constant, 60);
        assert!((params.text_weight - 0.8).abs() < f32::EPSILON);
        assert!((params.vector_weight - 0.2).abs() < f32::EPSILON);
        assert_eq!(params.candidate_window, 50);
        assert_eq!(params.intent, TemporalIntent::Timeless);
        assert_eq!(params.operator, TemporalOperator::Current);
    }

    #[test]
    #[cfg(all(
        feature = "hybrid-experimental",
        feature = "vector",
        feature = "fulltext"
    ))]
    fn search_ranked_matches_search_scored_order() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let now = KronroeTimestamp::now_utc();

        db.assert_fact("alice", "works_at", "Acme Corp", now)
            .unwrap();
        db.assert_fact("bob", "works_at", "Acme Industries", now)
            .unwrap();
        db.assert_fact("carol", "works_at", "BetaCorp", now)
            .unwrap();

        let ranked = db.search_ranked("Acme", 10).unwrap();
        let scored = db.search_scored("Acme", 10).unwrap();

        let ranked_ids: Vec<FactId> = ranked.into_iter().map(|(id, _)| id).collect();
        let scored_ids: Vec<FactId> = scored.into_iter().map(|(fact, _)| fact.id).collect();
        assert_eq!(
            ranked_ids, scored_ids,
            "hybrid lexical ranking input should match search_scored ordering"
        );
    }

    #[test]
    fn half_open_interval_boundary_at_valid_from() {
        // Fact valid at [valid_from, valid_to). Query exactly at valid_from
        // should include the fact.
        let (db, _tmp) = open_temp_db();
        let jan = dt("2024-01-01T00:00:00Z");
        let jun = dt("2024-06-01T00:00:00Z");

        let id = db.assert_fact("alice", "works_at", "Acme", jan).unwrap();
        db.invalidate_fact(&id, jun).unwrap();

        let at_start = db.facts_at("alice", "works_at", jan).unwrap();
        assert_eq!(
            at_start.len(),
            1,
            "fact should be valid at exact valid_from"
        );
    }

    #[test]
    fn half_open_interval_boundary_at_valid_to() {
        // Fact valid at [valid_from, valid_to). Query exactly at valid_to
        // should NOT include the fact (half-open upper bound).
        let (db, _tmp) = open_temp_db();
        let jan = dt("2024-01-01T00:00:00Z");
        let jun = dt("2024-06-01T00:00:00Z");

        let id = db.assert_fact("alice", "works_at", "Acme", jan).unwrap();
        db.invalidate_fact(&id, jun).unwrap();

        let at_end = db.facts_at("alice", "works_at", jun).unwrap();
        assert_eq!(
            at_end.len(),
            0,
            "fact should NOT be valid at exact valid_to (half-open)"
        );
    }

    #[test]
    fn half_open_interval_one_instant_before_valid_to() {
        let (db, _tmp) = open_temp_db();
        let jan = dt("2024-01-01T00:00:00Z");
        let jun = dt("2024-06-01T00:00:00Z");

        let id = db.assert_fact("alice", "works_at", "Acme", jan).unwrap();
        db.invalidate_fact(&id, jun).unwrap();

        // One second before valid_to — should still be valid.
        let just_before = dt("2024-05-31T23:59:59Z");
        let before_end = db.facts_at("alice", "works_at", just_before).unwrap();
        assert_eq!(
            before_end.len(),
            1,
            "fact should be valid just before valid_to"
        );
    }

    #[test]
    #[cfg(feature = "fulltext")]
    fn search_returns_expected_facts() {
        let (db, _tmp) = open_temp_db();
        let now = KronroeTimestamp::now_utc();

        db.assert_fact("alice", "works_at", "Acme", now).unwrap();
        db.assert_fact("alice", "has_alias", "ally", now).unwrap();
        db.assert_fact("bob", "works_at", "BetaCorp", now).unwrap();

        let results = db.search("alice works at", 10).unwrap();
        assert!(
            results
                .iter()
                .any(|f| f.subject == "alice" && f.predicate == "works_at"),
            "search should return alice works_at fact"
        );
    }

    #[test]
    #[cfg(feature = "fulltext")]
    fn search_supports_fuzzy_typo_matching() {
        let (db, _tmp) = open_temp_db();
        let now = KronroeTimestamp::now_utc();

        db.assert_fact("alice", "works_at", "Acme", now).unwrap();
        db.assert_fact("alice", "has_alias", "ally", now).unwrap();

        let results = db.search("alcie", 10).unwrap();
        assert!(
            results.iter().any(|f| f.subject == "alice"),
            "fuzzy search should match typo query"
        );
    }

    #[test]
    #[cfg(feature = "fulltext")]
    fn search_supports_alias_matching() {
        let (db, _tmp) = open_temp_db();
        let now = KronroeTimestamp::now_utc();

        db.assert_fact("alice", "works_at", "Acme", now).unwrap();
        db.assert_fact("alice", "has_alias", "ally", now).unwrap();

        let results = db.search("ally", 10).unwrap();
        assert!(
            results
                .iter()
                .any(|f| f.subject == "alice" && f.predicate == "works_at"),
            "alias search should surface the aliased fact"
        );
    }

    #[test]
    #[cfg(feature = "fulltext")]
    fn search_orders_exact_ties_by_fact_id() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let now = KronroeTimestamp::now_utc();

        let first = db.assert_fact("alice", "tag", "rust", now).unwrap();
        let second = db.assert_fact("bob", "tag", "rust", now).unwrap();

        let scored = db.search_scored("rust", 10).unwrap();
        assert!(
            scored.len() >= 2,
            "expected both rust facts in the result set"
        );

        let first_two_ids = [scored[0].0.id.clone(), scored[1].0.id.clone()];
        let expected = if first <= second {
            [first, second]
        } else {
            [second, first]
        };
        assert_eq!(
            first_two_ids, expected,
            "equal-score ties should be ordered by FactId"
        );
    }

    #[test]
    #[cfg(feature = "fulltext")]
    fn search_and_search_scored_same_ordering() {
        let (db, _tmp) = open_temp_db();
        let now = KronroeTimestamp::now_utc();

        db.assert_fact("alice", "works_at", "Acme Corp", now)
            .unwrap();
        db.assert_fact("alice", "has_alias", "ally", now).unwrap();
        db.assert_fact("bob", "works_at", "Acme Industries", now)
            .unwrap();

        let plain = db.search("Acme", 10).unwrap();
        let scored = db.search_scored("Acme", 10).unwrap();

        assert_eq!(
            plain.len(),
            scored.len(),
            "search and search_scored should return the same number of results"
        );

        // Order must match: search() is defined as search_scored() with scores stripped.
        for (i, (fact, (scored_fact, score))) in plain.iter().zip(scored.iter()).enumerate() {
            assert_eq!(
                fact.id, scored_fact.id,
                "result {i} should have the same fact ID"
            );
            assert!(
                *score > 0.0,
                "result {i} should have a positive BM25 score, got {score}"
            );
        }

        // Scores should be in descending order (Kronroe BM25 ranking).
        for w in scored.windows(2) {
            assert!(
                w[0].1 >= w[1].1,
                "scores should be descending: {} >= {}",
                w[0].1,
                w[1].1
            );
        }
    }

    // ------------------------------------------------------------------
    // Vector: error-path tests (P1 / P2 audit findings)
    // ------------------------------------------------------------------

    #[test]
    #[cfg(feature = "vector")]
    fn vector_empty_embedding_returns_error() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let result = db.assert_fact_with_embedding(
            "alice",
            "interest",
            "Rust",
            KronroeTimestamp::now_utc(),
            vec![],
        );
        assert!(
            result.as_ref().is_err_and(|e| e.is_invalid_embedding()),
            "empty embedding must return InvalidEmbedding, not panic"
        );
    }

    #[test]
    #[cfg(feature = "vector")]
    fn vector_dim_mismatch_returns_error() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let now = KronroeTimestamp::now_utc();

        // Establish dim = 3.
        db.assert_fact_with_embedding("alice", "interest", "Rust", now, vec![1.0, 0.0, 0.0])
            .unwrap();

        // Subsequent insert with dim = 2 must return Err, not panic.
        let result =
            db.assert_fact_with_embedding("alice", "interest", "Python", now, vec![0.0, 1.0]);
        assert!(
            result.as_ref().is_err_and(|e| e.is_invalid_embedding()),
            "dim mismatch must return InvalidEmbedding, not panic"
        );

        // The failed insert must not corrupt the index: the original fact is still
        // retrievable.
        let results = db.search_by_vector(&[1.0, 0.0, 0.0], 5, None).unwrap();
        assert_eq!(results.len(), 1, "failed insert must leave index intact");
    }

    #[test]
    #[cfg(feature = "vector")]
    fn vector_search_wrong_query_dim_returns_error() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let now = KronroeTimestamp::now_utc();

        // Insert a dim-3 embedding to establish the index dimension.
        db.assert_fact_with_embedding("alice", "interest", "Rust", now, vec![1.0, 0.0, 0.0])
            .unwrap();

        // Query with dim=2 must return Err, not silently score 0.0.
        let result = db.search_by_vector(&[1.0, 0.0], 5, None);
        assert!(
            result.as_ref().is_err_and(|e| e.is_invalid_embedding()),
            "wrong query dimension must return InvalidEmbedding"
        );
    }

    /// Embeddings are persisted to the default backend; the vector index must survive a
    /// close-and-reopen without any re-population by the caller.
    #[test]
    #[cfg(feature = "vector")]
    fn vector_index_survives_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("durability.kronroe");
        let path_str = path.to_str().unwrap();
        let now = KronroeTimestamp::now_utc();

        // Write two embeddings, then drop the database.
        {
            let db = TemporalGraph::open(path_str).unwrap();
            db.assert_fact_with_embedding("alice", "interest", "Rust", now, vec![1.0, 0.0, 0.0])
                .unwrap();
            db.assert_fact_with_embedding("alice", "interest", "Python", now, vec![0.0, 1.0, 0.0])
                .unwrap();
        } // db dropped — file closed

        // Reopen: the index must be rebuilt from storage automatically.
        let db = TemporalGraph::open(path_str).unwrap();
        let results = db.search_by_vector(&[1.0, 0.0, 0.0], 2, None).unwrap();
        assert_eq!(results.len(), 2, "both embeddings must survive reopen");
        assert!(
            matches!(&results[0].0.object, Value::Text(s) if s == "Rust"),
            "most similar fact after reopen should be Rust"
        );
    }

    #[test]
    fn invalidate_nonexistent_fact_returns_not_found() {
        let (db, _tmp) = open_temp_db();
        let bogus_id = FactId::new();
        let result = db.invalidate_fact(&bogus_id, KronroeTimestamp::now_utc());
        assert!(
            result.is_err(),
            "invalidating a nonexistent fact should fail"
        );
        let err = result.unwrap_err();
        assert!(err.is_not_found(), "error should be NotFound, got: {err:?}");
    }

    #[test]
    fn invalidate_fact_sets_expired_at() {
        let (db, _tmp) = open_temp_db();
        let jan = dt("2024-01-01T00:00:00Z");
        let jun = dt("2024-06-01T00:00:00Z");

        let id = db.assert_fact("alice", "works_at", "Acme", jan).unwrap();
        db.invalidate_fact(&id, jun).unwrap();

        let fact = db.fact_by_id(&id).unwrap();
        assert_eq!(fact.valid_to, Some(jun), "valid_to should be set");
        assert_eq!(
            fact.expired_at,
            Some(jun),
            "expired_at should be set (TSQL-2 transaction time)"
        );
    }

    // -- Contradiction detection integration tests ----------------------------

    #[cfg(feature = "contradiction")]
    #[test]
    fn register_and_detect_singleton_contradiction() {
        let db = TemporalGraph::open_in_memory().unwrap();
        db.register_singleton_predicate("works_at", ConflictPolicy::Warn)
            .unwrap();

        let t1 = KronroeTimestamp::now_utc() - KronroeSpan::days(365);
        let t2 = KronroeTimestamp::now_utc() - KronroeSpan::days(30);
        db.assert_fact("alice", "works_at", "Acme", t1).unwrap();
        db.assert_fact("alice", "works_at", "Beta Corp", t2)
            .unwrap();

        let contradictions = db.detect_contradictions("alice", "works_at").unwrap();
        assert_eq!(contradictions.len(), 1);
        assert_eq!(contradictions[0].subject, "alice");
        assert_eq!(contradictions[0].predicate, "works_at");
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn no_contradiction_for_unregistered_predicate() {
        let db = TemporalGraph::open_in_memory().unwrap();
        // "speaks_language" not registered → defaults to multi-valued
        let t = KronroeTimestamp::now_utc();
        db.assert_fact("alice", "speaks_language", "English", t)
            .unwrap();
        db.assert_fact("alice", "speaks_language", "French", t)
            .unwrap();

        let contradictions = db
            .detect_contradictions("alice", "speaks_language")
            .unwrap();
        assert!(contradictions.is_empty());
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn assert_fact_checked_warn_returns_contradictions() {
        let db = TemporalGraph::open_in_memory().unwrap();
        db.register_singleton_predicate("works_at", ConflictPolicy::Warn)
            .unwrap();

        let t1 = KronroeTimestamp::now_utc() - KronroeSpan::days(30);
        db.assert_fact("alice", "works_at", "Acme", t1).unwrap();

        let (fact_id, contradictions) = db
            .assert_fact_checked(
                "alice",
                "works_at",
                "Beta Corp",
                KronroeTimestamp::now_utc(),
            )
            .unwrap();
        assert!(!fact_id.as_str().is_empty(), "fact should be stored");
        assert_eq!(contradictions.len(), 1, "should detect one contradiction");

        // Regression: conflicting_fact_id must reference the actually-persisted
        // fact, not the temporary candidate used during detection.
        assert_eq!(
            contradictions[0].conflicting_fact_id,
            fact_id.to_string(),
            "conflicting_fact_id should match the stored fact's ID"
        );

        // Verify the fact was actually stored.
        let facts = db.current_facts("alice", "works_at").unwrap();
        assert_eq!(facts.len(), 2);
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn assert_fact_checked_reject_blocks_storage() {
        let db = TemporalGraph::open_in_memory().unwrap();
        db.register_singleton_predicate("lives_in", ConflictPolicy::Reject)
            .unwrap();

        let t1 = KronroeTimestamp::now_utc() - KronroeSpan::days(30);
        db.assert_fact("alice", "lives_in", "London", t1).unwrap();

        let result =
            db.assert_fact_checked("alice", "lives_in", "Paris", KronroeTimestamp::now_utc());
        assert!(result.is_err(), "should be rejected");
        let err = result.unwrap_err();
        assert!(err.is_contradiction_rejected());
        assert_eq!(err.contradictions().unwrap().len(), 1);

        // Verify the fact was NOT stored.
        let facts = db.current_facts("alice", "lives_in").unwrap();
        assert_eq!(facts.len(), 1);
        assert!(matches!(&facts[0].object, Value::Text(s) if s == "London"));
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn detect_all_contradictions_across_subjects() {
        let db = TemporalGraph::open_in_memory().unwrap();
        db.register_singleton_predicate("works_at", ConflictPolicy::Warn)
            .unwrap();
        db.register_singleton_predicate("lives_in", ConflictPolicy::Warn)
            .unwrap();

        let t1 = KronroeTimestamp::now_utc() - KronroeSpan::days(365);
        let t2 = KronroeTimestamp::now_utc() - KronroeSpan::days(30);

        // Alice has contradictions on works_at.
        db.assert_fact("alice", "works_at", "Acme", t1).unwrap();
        db.assert_fact("alice", "works_at", "Beta", t2).unwrap();

        // Bob has contradictions on lives_in.
        db.assert_fact("bob", "lives_in", "London", t1).unwrap();
        db.assert_fact("bob", "lives_in", "Paris", t2).unwrap();

        // Carol has no contradictions (same value).
        db.assert_fact("carol", "works_at", "Gamma", t1).unwrap();

        let all = db.detect_all_contradictions().unwrap();
        assert_eq!(all.len(), 2, "should find contradictions for alice and bob");

        let subjects: Vec<&str> = all.iter().map(|c| c.subject.as_str()).collect();
        assert!(subjects.contains(&"alice"));
        assert!(subjects.contains(&"bob"));
    }

    // -- Confidence tests ---------------------------------------------------

    #[test]
    fn fact_with_confidence_clamps() {
        let now = KronroeTimestamp::now_utc();
        let too_high = Fact::new("s", "p", "v", now).with_confidence(1.5);
        assert!((too_high.confidence - 1.0).abs() < f32::EPSILON);

        let too_low = Fact::new("s", "p", "v", now).with_confidence(-0.3);
        assert!((too_low.confidence - 0.0).abs() < f32::EPSILON);

        let normal = Fact::new("s", "p", "v", now).with_confidence(0.7);
        assert!((normal.confidence - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn assert_fact_with_confidence_persists() {
        let (db, _tmp) = open_temp_db();
        let now = KronroeTimestamp::now_utc();
        let id = db
            .assert_fact_with_confidence("alice", "works_at", "Acme", now, 0.7)
            .unwrap();
        let fact = db.fact_by_id(&id).unwrap();
        assert!(
            (fact.confidence - 0.7).abs() < f32::EPSILON,
            "confidence should be 0.7, got {}",
            fact.confidence,
        );
    }

    #[test]
    fn assert_fact_with_confidence_rejects_non_finite() {
        let (db, _tmp) = open_temp_db();
        let now = KronroeTimestamp::now_utc();

        for confidence in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let err = db.assert_fact_with_confidence("alice", "works_at", "Acme", now, confidence);
            match err {
                Err(ref e) if e.is_search() => assert!(
                    e.message().contains("non-finite"),
                    "unexpected search message for {confidence:?}: {}",
                    e.message()
                ),
                _ => panic!("expected search error for confidence={confidence:?}"),
            }
        }
    }

    #[test]
    fn assert_fact_default_confidence() {
        let (db, _tmp) = open_temp_db();
        let id = db
            .assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
            .unwrap();
        let fact = db.fact_by_id(&id).unwrap();
        assert!(
            (fact.confidence - 1.0).abs() < f32::EPSILON,
            "default confidence should be 1.0, got {}",
            fact.confidence,
        );
    }

    #[test]
    fn fact_with_source_builder() {
        let fact = Fact::new("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
            .with_source("user:rebekah");
        assert_eq!(fact.source.as_deref(), Some("user:rebekah"));
    }

    #[test]
    fn assert_fact_with_source_round_trip() {
        let (db, _tmp) = open_temp_db();
        let id = db
            .assert_fact_with_source(
                "alice",
                "works_at",
                "Acme",
                KronroeTimestamp::now_utc(),
                0.9,
                "api:openai",
            )
            .unwrap();
        let fact = db.fact_by_id(&id).unwrap();
        assert_eq!(fact.source.as_deref(), Some("api:openai"));
        assert!((fact.confidence - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn assert_fact_default_source_is_none() {
        let (db, _tmp) = open_temp_db();
        let id = db
            .assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
            .unwrap();
        let fact = db.fact_by_id(&id).unwrap();
        assert!(fact.source.is_none(), "default source should be None");
    }

    #[test]
    #[cfg(feature = "uncertainty")]
    fn predicate_volatility_and_source_weight_accessors() {
        use crate::{PredicateVolatility, SourceWeight};

        let (db, _tmp) = open_temp_db();
        db.register_predicate_volatility("works_at", PredicateVolatility::new(730.0))
            .unwrap();
        db.register_source_weight("user:owner", SourceWeight::new(1.2))
            .unwrap();

        let vol = db.predicate_volatility("works_at").unwrap();
        assert_eq!(
            vol.expect("volatility should be registered").half_life_days,
            730.0
        );
        let sw = db.source_weight("user:owner").unwrap();
        assert_eq!(sw.expect("source weight should be registered").weight, 1.2);
    }

    #[test]
    #[cfg(feature = "uncertainty")]
    fn register_volatility_persists() {
        use crate::PredicateVolatility;

        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        {
            let db = TemporalGraph::open(&path).unwrap();
            db.register_predicate_volatility("works_at", PredicateVolatility::new(730.0))
                .unwrap();
        }
        // Reopen — volatility should survive.
        let db = TemporalGraph::open(&path).unwrap();
        let fact = db
            .assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
            .unwrap();
        let f = db.fact_by_id(&fact).unwrap();
        let eff = db
            .effective_confidence(&f, KronroeTimestamp::now_utc())
            .unwrap();
        // Fresh fact + 730d half-life → decay ≈ 1.0
        assert!(
            eff.age_decay > 0.99,
            "fresh fact should have decay near 1.0, got {}",
            eff.age_decay
        );
    }

    #[test]
    #[cfg(feature = "uncertainty")]
    fn register_source_weight_persists() {
        use crate::SourceWeight;

        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        {
            let db = TemporalGraph::open(&path).unwrap();
            db.register_source_weight("user:owner", SourceWeight::new(1.5))
                .unwrap();
        }
        let db = TemporalGraph::open(&path).unwrap();
        let id = db
            .assert_fact_with_source(
                "alice",
                "works_at",
                "Acme",
                KronroeTimestamp::now_utc(),
                0.8,
                "user:owner",
            )
            .unwrap();
        let f = db.fact_by_id(&id).unwrap();
        let eff = db
            .effective_confidence(&f, KronroeTimestamp::now_utc())
            .unwrap();
        // 0.8 * 1.0 (fresh) * 1.5 = 1.2, clamped to 1.0
        assert!(
            (eff.value - 1.0).abs() < 1e-6,
            "high source weight should boost to 1.0, got {}",
            eff.value
        );
        assert!((eff.source_weight - 1.5).abs() < 1e-6);
    }

    #[test]
    #[cfg(feature = "uncertainty")]
    fn effective_confidence_query_time() {
        use crate::PredicateVolatility;

        let (db, _tmp) = open_temp_db();
        db.register_predicate_volatility("works_at", PredicateVolatility::new(365.0))
            .unwrap();
        // Fact from 1 year ago.
        let one_year_ago = KronroeTimestamp::now_utc() - KronroeSpan::days(365);
        let id = db
            .assert_fact("alice", "works_at", "Acme", one_year_ago)
            .unwrap();
        let f = db.fact_by_id(&id).unwrap();
        let eff = db
            .effective_confidence(&f, KronroeTimestamp::now_utc())
            .unwrap();
        // At exactly one half-life: decay ≈ 0.5, base = 1.0 → effective ≈ 0.5
        assert!(
            (eff.value - 0.5).abs() < 0.02,
            "at half-life, expected ~0.5, got {}",
            eff.value
        );
    }

    #[test]
    #[cfg(feature = "uncertainty")]
    fn init_rejects_corrupted_volatility_registry_entry() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        {
            let db = TemporalGraph::open(&path).unwrap();
            db.storage
                .write_volatility_registry_entry("broken", "not-json")
                .unwrap();
        }

        match TemporalGraph::open(&path) {
            Err(ref err) if err.is_storage() => {
                assert!(err.message().contains("invalid volatility registry"));
            }
            Err(err) => {
                panic!("expected Storage error for corrupted volatility registry, got {err:?}")
            }
            Ok(_) => panic!("expected reopen to fail with corrupted volatility registry"),
        }
    }

    #[test]
    #[cfg(feature = "uncertainty")]
    fn init_rejects_corrupted_source_weight_registry_entry() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        {
            let db = TemporalGraph::open(&path).unwrap();
            db.storage
                .write_source_weight_registry_entry("trusted-api", "not-json")
                .unwrap();
        }

        match TemporalGraph::open(&path) {
            Err(ref err) if err.is_storage() => {
                assert!(err.message().contains("invalid source-weight registry"));
            }
            Err(err) => {
                panic!("expected Storage error for corrupted source-weight registry, got {err:?}")
            }
            Ok(_) => panic!("expected reopen to fail with corrupted source-weight registry"),
        }
    }

    #[test]
    #[cfg(feature = "contradiction")]
    fn init_rejects_corrupted_predicate_registry_entry() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        {
            let db = TemporalGraph::open(&path).unwrap();
            db.storage
                .write_predicate_registry_entry("works_at", "not-json")
                .unwrap();
        }

        match TemporalGraph::open(&path) {
            Err(ref err) if err.is_storage() => {
                assert!(err.message().contains("invalid predicate registry"));
            }
            Err(err) => {
                panic!("expected Storage error for corrupted predicate registry, got {err:?}")
            }
            Ok(_) => panic!("expected reopen to fail with corrupted predicate registry"),
        }
    }

    #[test]
    fn default_backend_supports_basic_graph_flow() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let now = KronroeTimestamp::now_utc();

        let id = db.assert_fact("alice", "works_at", "Acme", now).unwrap();
        let current = db.current_facts("alice", "works_at").unwrap();

        assert_eq!(current.len(), 1);
        assert_eq!(current[0].id, id);
    }

    #[test]
    fn default_backend_supports_idempotent_reopen_flow() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let now = KronroeTimestamp::now_utc();

        {
            let db = TemporalGraph::open(&path).unwrap();
            let id = db
                .assert_fact_idempotent("evt-append", "alice", "works_at", "Acme", now)
                .unwrap();
            assert_eq!(db.fact_by_id(&id).unwrap().subject, "alice");
        }

        let reopened = TemporalGraph::open(&path).unwrap();
        let reused = reopened
            .assert_fact_idempotent("evt-append", "alice", "works_at", "Acme", now)
            .unwrap();
        assert_eq!(reopened.fact_by_id(&reused).unwrap().subject, "alice");
        assert_eq!(
            reopened.current_facts("alice", "works_at").unwrap().len(),
            1,
            "replayed append-log idempotency should prevent duplicate facts"
        );
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn predicate_registry_persists_across_reopen() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        {
            let db = TemporalGraph::open(&path).unwrap();
            db.register_singleton_predicate("works_at", ConflictPolicy::Warn)
                .unwrap();
        }

        let reopened = TemporalGraph::open(&path).unwrap();
        assert!(reopened.is_singleton_predicate("works_at").unwrap());
    }

    #[cfg(feature = "uncertainty")]
    #[test]
    fn uncertainty_registries_persist_across_reopen() {
        use crate::{PredicateVolatility, SourceWeight};

        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        {
            let db = TemporalGraph::open(&path).unwrap();
            db.register_predicate_volatility("works_at", PredicateVolatility::new(730.0))
                .unwrap();
            db.register_source_weight("user:owner", SourceWeight::new(1.5))
                .unwrap();
        }

        let reopened = TemporalGraph::open(&path).unwrap();
        let volatility = reopened.predicate_volatility("works_at").unwrap();
        assert_eq!(
            volatility
                .expect("volatility should persist across append-log reopen")
                .half_life_days,
            730.0
        );

        let source_weight = reopened.source_weight("user:owner").unwrap();
        assert_eq!(
            source_weight
                .expect("source weight should persist across append-log reopen")
                .weight,
            1.5
        );
    }

    #[cfg(feature = "vector")]
    #[test]
    fn append_log_vector_index_survives_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("append-log-vectors.kronroe");
        let path_str = path.to_str().unwrap();
        let now = KronroeTimestamp::now_utc();

        {
            let db = TemporalGraph::open(path_str).unwrap();
            db.assert_fact_with_embedding("alice", "interest", "Rust", now, vec![1.0, 0.0, 0.0])
                .unwrap();
            db.assert_fact_with_embedding("alice", "interest", "Python", now, vec![0.0, 1.0, 0.0])
                .unwrap();
        }

        let reopened = TemporalGraph::open(path_str).unwrap();
        let results = reopened
            .search_by_vector(&[1.0, 0.0, 0.0], 2, None)
            .unwrap();
        assert_eq!(
            results.len(),
            2,
            "both append-log embeddings must survive reopen"
        );
        assert!(
            matches!(&results[0].0.object, Value::Text(s) if s == "Rust"),
            "most similar append-log fact after reopen should be Rust"
        );
    }

    #[test]
    fn append_log_reopen_survives_truncated_final_newline() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let now = KronroeTimestamp::now_utc();
        let fact_id = {
            let db = TemporalGraph::open(&path).unwrap();
            db.assert_fact("alice", "works_at", "Acme", now).unwrap()
        };

        let mut bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.last().copied(), Some(b'\n'));
        bytes.pop();
        fs::write(&path, bytes).unwrap();

        let reopened = TemporalGraph::open(&path).unwrap();
        assert_eq!(reopened.fact_by_id(&fact_id).unwrap().id, fact_id);
        assert_eq!(
            reopened.current_facts("alice", "works_at").unwrap().len(),
            1
        );
    }

    #[test]
    #[cfg(all(feature = "vector", feature = "contradiction", feature = "uncertainty"))]
    fn append_log_recovery_ignores_truncated_tail_and_preserves_state() {
        use crate::{ConflictPolicy, PredicateVolatility, SourceWeight};

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("recovery-tail.kronroe");
        let path_str = path.to_str().unwrap();
        let now = KronroeTimestamp::now_utc();

        let fact_id = {
            let db = TemporalGraph::open(path_str).unwrap();
            db.register_singleton_predicate("works_at", ConflictPolicy::Warn)
                .unwrap();
            db.register_predicate_volatility("works_at", PredicateVolatility::new(730.0))
                .unwrap();
            db.register_source_weight("api:trusted", SourceWeight::new(1.2))
                .unwrap();
            db.assert_fact_with_embedding("alice", "works_at", "Acme", now, vec![1.0, 0.0, 0.0])
                .unwrap()
        };

        append_bytes(path_str, br#"{"UpsertFact":{"key":"truncated"#);

        let reopened = TemporalGraph::open(path_str).unwrap();
        assert_eq!(reopened.fact_by_id(&fact_id).unwrap().id, fact_id);
        assert!(reopened.is_singleton_predicate("works_at").unwrap());
        assert!(reopened.predicate_volatility("works_at").unwrap().is_some());
        assert!(reopened.source_weight("api:trusted").unwrap().is_some());
        let vector_results = reopened
            .search_by_vector(&[1.0, 0.0, 0.0], 1, None)
            .unwrap();
        assert_eq!(vector_results.len(), 1);
    }

    #[test]
    fn append_log_reopen_rejects_mid_file_corruption() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        {
            let db = TemporalGraph::open(&path).unwrap();
            db.assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
                .unwrap();
        }

        insert_bytes_after_nth_newline(&path, 0, b"not-json\n");

        match TemporalGraph::open(&path) {
            Err(ref err) if err.is_storage() => {
                assert!(err.message().contains("append-log corruption"));
            }
            Err(error) => panic!("expected storage corruption error, got {error:?}"),
            Ok(_) => panic!("expected mid-file corruption to fail reopen"),
        }
    }

    #[test]
    fn append_log_reopen_rejects_wrong_header() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        fs::write(
            &path,
            b"{\"Header\":{\"magic\":\"wrong-backend\"}}\n{\"SchemaVersion\":{\"version\":2}}\n",
        )
        .unwrap();

        match TemporalGraph::open(&path) {
            Err(ref err) if err.is_storage() => {
                assert!(err.message().contains("storage backend mismatch"));
            }
            Err(error) => panic!("expected backend mismatch, got {error:?}"),
            Ok(_) => panic!("expected wrong header to fail reopen"),
        }
    }

    #[test]
    fn append_log_reopen_rejects_unsupported_schema_version() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        fs::write(
            &path,
            b"{\"Header\":{\"magic\":\"kronroe-append-log-v1\"}}\n{\"SchemaVersion\":{\"version\":999}}\n",
        )
        .unwrap();

        match TemporalGraph::open(&path) {
            Err(ref err) if err.is_schema_mismatch() => {
                let (found, expected) = err.schema_versions().unwrap();
                assert_eq!(found, 999);
                assert_eq!(expected, SCHEMA_VERSION);
            }
            Err(error) => panic!("expected schema mismatch, got {error:?}"),
            Ok(_) => panic!("expected unsupported schema version to fail reopen"),
        }
    }

    #[test]
    fn append_log_reopen_handles_long_replacement_chain() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let jan = dt("2024-01-01T00:00:00Z");

        {
            let db = TemporalGraph::open(&path).unwrap();
            let mut fact_id = db.assert_fact("alice", "works_at", "Acme", jan).unwrap();
            for month in 1..12 {
                let at = jan + KronroeSpan::days(month * 30);
                db.invalidate_fact(&fact_id, at).unwrap();
                fact_id = db
                    .assert_fact("alice", "works_at", format!("Company-{month}"), at)
                    .unwrap();
            }
        }

        let reopened = TemporalGraph::open(&path).unwrap();
        let current = reopened.current_facts("alice", "works_at").unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].object.to_string(), "Company-11");

        let historical = reopened
            .facts_at("alice", "works_at", dt("2024-05-15T00:00:00Z"))
            .unwrap();
        assert!(!historical.is_empty());
    }

    #[test]
    fn append_log_compaction_preserves_temporal_state_and_idempotency() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("compaction.kronroe");
        let path_str = path.to_str().unwrap();
        let jan = dt("2024-01-01T00:00:00Z");
        let jun = dt("2024-06-01T00:00:00Z");

        let fact_id = {
            let db = TemporalGraph::open(path_str).unwrap();
            let fact_id = db
                .assert_fact_idempotent("evt-compaction", "alice", "works_at", "Acme", jan)
                .unwrap();
            db.invalidate_fact(&fact_id, jun).unwrap();
            db.assert_fact("alice", "works_at", "TechCorp", jun)
                .unwrap();

            let pre_all: Vec<_> = db
                .all_facts_about("alice")
                .unwrap()
                .into_iter()
                .map(|fact| {
                    (
                        fact.id.to_string(),
                        fact.object.to_string(),
                        fact.valid_from,
                    )
                })
                .collect();
            let pre_current: Vec<_> = db
                .current_facts("alice", "works_at")
                .unwrap()
                .into_iter()
                .map(|fact| {
                    (
                        fact.id.to_string(),
                        fact.object.to_string(),
                        fact.valid_from,
                    )
                })
                .collect();
            let pre_at: Vec<_> = db
                .facts_at("alice", "works_at", dt("2024-03-01T00:00:00Z"))
                .unwrap()
                .into_iter()
                .map(|fact| {
                    (
                        fact.id.to_string(),
                        fact.object.to_string(),
                        fact.valid_from,
                    )
                })
                .collect();

            db.storage.compact().unwrap();

            let post_all: Vec<_> = db
                .all_facts_about("alice")
                .unwrap()
                .into_iter()
                .map(|fact| {
                    (
                        fact.id.to_string(),
                        fact.object.to_string(),
                        fact.valid_from,
                    )
                })
                .collect();
            let post_current: Vec<_> = db
                .current_facts("alice", "works_at")
                .unwrap()
                .into_iter()
                .map(|fact| {
                    (
                        fact.id.to_string(),
                        fact.object.to_string(),
                        fact.valid_from,
                    )
                })
                .collect();
            let post_at: Vec<_> = db
                .facts_at("alice", "works_at", dt("2024-03-01T00:00:00Z"))
                .unwrap()
                .into_iter()
                .map(|fact| {
                    (
                        fact.id.to_string(),
                        fact.object.to_string(),
                        fact.valid_from,
                    )
                })
                .collect();

            assert_eq!(post_all, pre_all);
            assert_eq!(post_current, pre_current);
            assert_eq!(post_at, pre_at);
            fact_id
        };

        let reopened = TemporalGraph::open(path_str).unwrap();
        let reused = reopened
            .assert_fact_idempotent("evt-compaction", "alice", "works_at", "Acme", jan)
            .unwrap();
        assert_eq!(reused, fact_id);
        assert_eq!(
            reopened.current_facts("alice", "works_at").unwrap().len(),
            1
        );
        assert_eq!(
            reopened.current_facts("alice", "works_at").unwrap()[0]
                .object
                .to_string(),
            "TechCorp"
        );
    }

    #[test]
    #[cfg(feature = "vector")]
    fn append_log_compaction_preserves_vector_search() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("compaction-vectors.kronroe");
        let path_str = path.to_str().unwrap();
        let now = KronroeTimestamp::now_utc();

        {
            let db = TemporalGraph::open(path_str).unwrap();
            db.assert_fact_with_embedding("alice", "interest", "Rust", now, vec![1.0, 0.0, 0.0])
                .unwrap();
            db.assert_fact_with_embedding("alice", "interest", "Python", now, vec![0.0, 1.0, 0.0])
                .unwrap();
            let pre: Vec<_> = db
                .search_by_vector(&[1.0, 0.0, 0.0], 2, None)
                .unwrap()
                .into_iter()
                .map(|(fact, score)| (fact.id.to_string(), score))
                .collect();
            db.storage.compact().unwrap();
            let post: Vec<_> = db
                .search_by_vector(&[1.0, 0.0, 0.0], 2, None)
                .unwrap()
                .into_iter()
                .map(|(fact, score)| (fact.id.to_string(), score))
                .collect();
            assert_eq!(post, pre);
        }

        let reopened = TemporalGraph::open(path_str).unwrap();
        let results = reopened
            .search_by_vector(&[1.0, 0.0, 0.0], 2, None)
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(
            matches!(&results[0].0.object, Value::Text(s) if s == "Rust"),
            "most similar fact after compaction should still be Rust"
        );
    }

    #[test]
    fn append_log_open_rejects_second_writer_in_same_process() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let db = TemporalGraph::open(&path).unwrap();

        match TemporalGraph::open(&path) {
            Err(ref err) if err.is_storage() => {
                assert!(err.message().contains("already open for write"));
            }
            Err(error) => panic!("expected single-writer lock error, got {error:?}"),
            Ok(_) => panic!("expected second writer open to fail"),
        }

        drop(db);
    }

    #[test]
    fn append_log_writer_lock_releases_after_drop() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        {
            let _db = TemporalGraph::open(&path).unwrap();
        }
        let reopened = TemporalGraph::open(&path).unwrap();
        assert_eq!(
            reopened.current_facts("alice", "works_at").unwrap().len(),
            0
        );
    }

    #[test]
    fn append_log_in_memory_instances_are_independent() {
        let db1 = TemporalGraph::open_in_memory().unwrap();
        let db2 = TemporalGraph::open_in_memory().unwrap();

        db1.assert_fact("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
            .unwrap();
        assert_eq!(db1.current_facts("alice", "works_at").unwrap().len(), 1);
        assert_eq!(db2.current_facts("alice", "works_at").unwrap().len(), 0);
    }
}
