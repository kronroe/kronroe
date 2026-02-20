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
//! use kronroe::TemporalGraph;
//! use chrono::Utc;
//!
//! let db = TemporalGraph::open("my-graph.kronroe").unwrap();
//!
//! // Assert a fact
//! let id = db.assert_fact("alice", "works_at", "Acme", Utc::now()).unwrap();
//!
//! // Query current state
//! let facts = db.current_facts("alice", "works_at").unwrap();
//!
//! // Point-in-time query
//! let past = "2024-03-01T00:00:00Z".parse().unwrap();
//! let facts_then = db.facts_at("alice", "works_at", past).unwrap();
//! ```

use chrono::{DateTime, Utc};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum KronroeError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
}

impl From<redb::DatabaseError> for KronroeError {
    fn from(e: redb::DatabaseError) -> Self {
        KronroeError::Storage(e.to_string())
    }
}
impl From<redb::TransactionError> for KronroeError {
    fn from(e: redb::TransactionError) -> Self {
        KronroeError::Storage(e.to_string())
    }
}
impl From<redb::TableError> for KronroeError {
    fn from(e: redb::TableError) -> Self {
        KronroeError::Storage(e.to_string())
    }
}
impl From<redb::StorageError> for KronroeError {
    fn from(e: redb::StorageError) -> Self {
        KronroeError::Storage(e.to_string())
    }
}
impl From<redb::CommitError> for KronroeError {
    fn from(e: redb::CommitError) -> Self {
        KronroeError::Storage(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, KronroeError>;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A stable, time-sortable identifier for a [`Fact`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FactId(pub String);

impl FactId {
    pub fn new() -> Self {
        Self(Ulid::new().to_string())
    }
}

impl Default for FactId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for FactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The value stored in a fact's object position.
///
/// A fact's object can be a scalar value or a reference to another entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub valid_from: DateTime<Utc>,
    /// When this stopped being true in the world. `None` = still true.
    pub valid_to: Option<DateTime<Utc>>,
    /// When this was written to the database (transaction time start).
    pub recorded_at: DateTime<Utc>,
    /// When we learned it was no longer true. `None` = still believed true.
    pub expired_at: Option<DateTime<Utc>>,
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
        valid_from: DateTime<Utc>,
    ) -> Self {
        Self {
            id: FactId::new(),
            subject: subject.into(),
            predicate: predicate.into(),
            object: object.into(),
            valid_from,
            valid_to: None,
            recorded_at: Utc::now(),
            expired_at: None,
            confidence: 1.0,
            source: None,
        }
    }

    /// Is this fact currently valid (valid time is open, not expired)?
    pub fn is_currently_valid(&self) -> bool {
        self.valid_to.is_none() && self.expired_at.is_none()
    }

    /// Was this fact valid at the given point in time (valid time axis)?
    pub fn was_valid_at(&self, at: DateTime<Utc>) -> bool {
        self.valid_from <= at
            && self.valid_to.is_none_or(|t| t > at)
            && self.expired_at.is_none_or(|t| t > at)
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Composite string key: `"{subject}:{predicate}:{fact_id}"`.
///
/// The ULID-based fact_id is time-sortable, so facts for the same
/// (subject, predicate) pair are stored in insertion order.
///
/// This is the Phase 0 storage strategy — a proper multi-level B-tree
/// index will replace this in Phase 1.
const FACTS: TableDefinition<&str, &str> = TableDefinition::new("facts");

/// Kronroe temporal property graph database.
///
/// An embedded, serverless database where bi-temporal facts are the core
/// primitive. All writes are ACID (backed by `redb`). The database file
/// uses the `.kronroe` extension by convention.
///
/// # Example
///
/// ```rust,no_run
/// use kronroe::TemporalGraph;
/// use chrono::Utc;
///
/// let db = TemporalGraph::open("my-graph.kronroe").unwrap();
/// db.assert_fact("alice", "works_at", "Acme", Utc::now()).unwrap();
/// let current = db.current_facts("alice", "works_at").unwrap();
/// assert_eq!(current.len(), 1);
/// ```
pub struct TemporalGraph {
    db: Database,
}

impl TemporalGraph {
    /// Open or create a Kronroe database at the given path.
    ///
    /// The file will be created if it does not exist. The `.kronroe`
    /// extension is conventional but not enforced.
    pub fn open(path: &str) -> Result<Self> {
        let db = Database::create(path)?;
        Self::init(db)
    }

    /// Create an in-memory Kronroe database (no file I/O).
    ///
    /// Useful for WASM targets, testing, and ephemeral workloads where
    /// persistence is not needed. Data is lost when the instance is dropped.
    pub fn open_in_memory() -> Result<Self> {
        let backend = redb::backends::InMemoryBackend::new();
        let db = Database::builder().create_with_backend(backend)?;
        Self::init(db)
    }

    fn init(db: Database) -> Result<Self> {
        {
            let write_txn = db.begin_write()?;
            write_txn.open_table(FACTS)?;
            write_txn.commit()?;
        }
        Ok(Self { db })
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
        valid_from: DateTime<Utc>,
    ) -> Result<FactId> {
        let fact = Fact::new(subject, predicate, object, valid_from);
        let fact_id = fact.id.clone();
        let key = format!("{}:{}:{}", subject, predicate, fact.id);
        let value = serde_json::to_string(&fact)?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(FACTS)?;
            table.insert(key.as_str(), value.as_str())?;
        }
        write_txn.commit()?;

        Ok(fact_id)
    }

    /// Get all currently valid facts for `(subject, predicate)`.
    ///
    /// A fact is currently valid if both `valid_to` and `expired_at` are `None`.
    pub fn current_facts(&self, subject: &str, predicate: &str) -> Result<Vec<Fact>> {
        let prefix = format!("{}:{}:", subject, predicate);
        self.scan_prefix(&prefix, |f| f.is_currently_valid())
    }

    /// Get all facts valid at a given point in time for `(subject, predicate)`.
    ///
    /// Uses the **valid time** axis: queries when something was true in the
    /// world, regardless of when it was recorded.
    pub fn facts_at(
        &self,
        subject: &str,
        predicate: &str,
        at: DateTime<Utc>,
    ) -> Result<Vec<Fact>> {
        let prefix = format!("{}:{}:", subject, predicate);
        self.scan_prefix(&prefix, |f| f.was_valid_at(at))
    }

    /// Get every fact ever recorded for an entity, across all predicates.
    pub fn all_facts_about(&self, subject: &str) -> Result<Vec<Fact>> {
        let prefix = format!("{}:", subject);
        self.scan_prefix(&prefix, |_| true)
    }

    /// Invalidate a fact by setting its `valid_to` timestamp.
    ///
    /// The fact is not deleted — its history is preserved. After invalidation,
    /// the fact will no longer appear in `current_facts()` but will still be
    /// returned by `facts_at()` for timestamps before `at`.
    pub fn invalidate_fact(&self, fact_id: &FactId, at: DateTime<Utc>) -> Result<()> {
        // Phase 0: linear scan to find the fact. Replace with ID index in Phase 1.
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(FACTS)?;

        let mut found_key: Option<String> = None;
        let mut found_fact: Option<Fact> = None;

        for entry in table.iter()? {
            let (k, v) = entry?;
            let fact: Fact = serde_json::from_str(v.value())?;
            if fact.id == *fact_id {
                found_key = Some(k.value().to_string());
                found_fact = Some(fact);
                break;
            }
        }

        drop(table);
        drop(read_txn);

        if let (Some(key), Some(mut fact)) = (found_key, found_fact) {
            fact.valid_to = Some(at);
            let value = serde_json::to_string(&fact)?;
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(FACTS)?;
                table.insert(key.as_str(), value.as_str())?;
            }
            write_txn.commit()?;
        }

        Ok(())
    }

    // Internal: scan facts table, filter by prefix, apply predicate.
    fn scan_prefix(
        &self,
        prefix: &str,
        predicate: impl Fn(&Fact) -> bool,
    ) -> Result<Vec<Fact>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(FACTS)?;
        let mut results = Vec::new();

        for entry in table.iter()? {
            let (k, v) = entry?;
            if k.value().starts_with(prefix) {
                let fact: Fact = serde_json::from_str(v.value())?;
                if predicate(&fact) {
                    results.push(fact);
                }
            }
        }

        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn open_temp_db() -> (TemporalGraph, NamedTempFile) {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_str().unwrap().to_string();
        let db = TemporalGraph::open(&path).unwrap();
        (db, file)
    }

    fn dt(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn assert_and_retrieve_current_fact() {
        let (db, _tmp) = open_temp_db();
        db.assert_fact("alice", "works_at", "Acme", Utc::now())
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
        assert_eq!(before_start.len(), 0, "should find no facts before valid_from");
    }

    #[test]
    fn fact_invalidation_preserves_history() {
        let (db, _tmp) = open_temp_db();
        let jan = dt("2024-01-01T00:00:00Z");
        let jun = dt("2024-06-01T00:00:00Z");
        let mar = dt("2024-03-01T00:00:00Z");

        let id = db
            .assert_fact("alice", "works_at", "Acme", jan)
            .unwrap();
        db.invalidate_fact(&id, jun).unwrap();

        // No longer current
        let current = db.current_facts("alice", "works_at").unwrap();
        assert_eq!(current.len(), 0, "fact should no longer be current after invalidation");

        // But history is preserved: still valid in March
        let in_march = db.facts_at("alice", "works_at", mar).unwrap();
        assert_eq!(in_march.len(), 1, "historical fact should still be retrievable");

        // Not valid after June (when it was invalidated)
        let after_invalidation = db
            .facts_at("alice", "works_at", dt("2024-09-01T00:00:00Z"))
            .unwrap();
        assert_eq!(after_invalidation.len(), 0, "fact should not appear after valid_to");
    }

    #[test]
    fn all_facts_about_entity() {
        let (db, _tmp) = open_temp_db();
        let now = Utc::now();

        db.assert_fact("alice", "works_at", "Acme", now).unwrap();
        db.assert_fact("alice", "has_role", "Engineer", now).unwrap();
        db.assert_fact("alice", "has_skill", "Rust", now).unwrap();
        db.assert_fact("bob", "works_at", "Acme", now).unwrap(); // different subject

        let alice_facts = db.all_facts_about("alice").unwrap();
        assert_eq!(alice_facts.len(), 3, "should return all 3 facts about alice");

        let subjects: Vec<&str> = alice_facts.iter().map(|f| f.subject.as_str()).collect();
        assert!(subjects.iter().all(|&s| s == "alice"));
    }

    #[test]
    fn value_types() {
        let (db, _tmp) = open_temp_db();
        let now = Utc::now();

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
}
