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

#[cfg(feature = "vector")]
mod vector;

use chrono::{DateTime, Utc};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
#[cfg(feature = "fulltext")]
use std::collections::HashMap;
#[cfg(feature = "fulltext")]
use tantivy::collector::TopDocs;
#[cfg(feature = "fulltext")]
use tantivy::query::{BooleanQuery, FuzzyTermQuery, Occur, QueryParser};
#[cfg(feature = "fulltext")]
use tantivy::schema::{Field, Schema, Value as TantivyValueTrait, STORED, STRING, TEXT};
#[cfg(feature = "fulltext")]
use tantivy::{doc, Index, Term};
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
    #[error("search error: {0}")]
    Search(String),
    #[error("invalid embedding: {0}")]
    InvalidEmbedding(String),
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
#[cfg(feature = "fulltext")]
impl From<tantivy::TantivyError> for KronroeError {
    fn from(e: tantivy::TantivyError) -> Self {
        KronroeError::Search(e.to_string())
    }
}
#[cfg(feature = "fulltext")]
impl From<tantivy::query::QueryParserError> for KronroeError {
    fn from(e: tantivy::query::QueryParserError) -> Self {
        KronroeError::Search(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, KronroeError>;

/// Strategy options for hybrid retrieval experiments.
#[cfg(feature = "hybrid-experimental")]
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) enum HybridFusionStrategy {
    /// Weighted Reciprocal Rank Fusion (RRF).
    Rrf,
}

/// Optional temporal adjustment used by hybrid experimental ranking.
#[cfg(feature = "hybrid-experimental")]
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) enum TemporalAdjustment {
    /// Disable temporal score adjustment.
    None,
    /// Exponential decay using the given half-life in days.
    HalfLifeDays { days: f32 },
}

/// Internal parameters for hybrid experimental retrieval.
#[cfg(feature = "hybrid-experimental")]
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) struct HybridParams {
    /// Number of results requested.
    pub k: usize,
    /// Number of candidates to pull from each channel before fusion.
    pub candidate_window: usize,
    /// Weighted fusion strategy.
    pub fusion: HybridFusionStrategy,
    /// RRF rank constant.
    pub rank_constant: usize,
    /// Relative influence of the lexical channel.
    pub text_weight: f32,
    /// Relative influence of the vector channel.
    pub vector_weight: f32,
    /// Relative influence of the temporal adjustment.
    pub temporal_weight: f32,
    /// Temporal adjustment mode.
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
/// Maps client-supplied idempotency keys to persisted fact IDs.
///
/// Used by [`TemporalGraph::assert_fact_idempotent`] to provide safe retry
/// semantics for ingestion workflows.
const IDEMPOTENCY: TableDefinition<&str, &str> = TableDefinition::new("idempotency");

/// Raw little-endian f32 bytes keyed by fact_id string.
/// Written atomically alongside the fact row in `assert_fact_with_embedding`.
#[cfg(feature = "vector")]
const EMBEDDINGS: TableDefinition<&str, &[u8]> = TableDefinition::new("embeddings");

/// Single-row metadata table for the vector index.
/// Key `"dim"` stores the established embedding dimension (`u64`).
/// Written once (on first insert) inside a serialised write transaction.
#[cfg(feature = "vector")]
const EMBEDDING_META: TableDefinition<&str, u64> = TableDefinition::new("embedding_meta");

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
    /// In-memory vector index cache.  Rebuilt from the `embeddings` redb table
    /// on every [`init`] call, then kept in sync by [`assert_fact_with_embedding`].
    /// The redb tables are the source of truth; this cache is a read-optimised
    /// view of them.
    ///
    /// [`assert_fact_with_embedding`]: TemporalGraph::assert_fact_with_embedding
    #[cfg(feature = "vector")]
    vector_index: std::sync::Mutex<vector::VectorIndex>,
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
            write_txn.open_table(IDEMPOTENCY)?;
            #[cfg(feature = "vector")]
            {
                write_txn.open_table(EMBEDDINGS)?;
                write_txn.open_table(EMBEDDING_META)?;
            }
            write_txn.commit()?;
        }
        #[cfg(feature = "vector")]
        let vector_index = {
            let idx = Self::rebuild_vector_index_from_db(&db)?;
            std::sync::Mutex::new(idx)
        };
        Ok(Self {
            db,
            #[cfg(feature = "vector")]
            vector_index,
        })
    }

    /// Read every persisted embedding from redb and build a fresh in-memory
    /// [`VectorIndex`] cache.
    ///
    /// Called once from [`init`].  If the database was created before the
    /// `embeddings` table existed (old-format file), `TableDoesNotExist` is
    /// handled gracefully — the method returns an empty index and the table
    /// is created by the preceding `open_table` call in `init`.
    #[cfg(feature = "vector")]
    fn rebuild_vector_index_from_db(db: &Database) -> Result<vector::VectorIndex> {
        let mut idx = vector::VectorIndex::new();
        let read_txn = db.begin_read()?;

        let emb_table = match read_txn.open_table(EMBEDDINGS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(idx),
            Err(e) => return Err(KronroeError::Storage(e.to_string())),
        };

        for entry in emb_table.iter()? {
            let (key, value) = entry?;
            let fact_id = FactId(key.value().to_string());
            let bytes = value.value();

            if bytes.len() % 4 != 0 {
                return Err(KronroeError::Storage(format!(
                    "corrupt embedding for fact {fact_id}: \
                     byte length {} is not a multiple of 4",
                    bytes.len()
                )));
            }

            let embedding: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();

            idx.insert(fact_id, embedding);
        }

        Ok(idx)
    }

    /// Write a single fact row inside an already-open [`redb::WriteTransaction`].
    ///
    /// The caller owns the transaction and is responsible for committing (or
    /// letting it drop for an implicit rollback).  This helper is used by both
    /// [`assert_fact`] and [`assert_fact_with_embedding`] so that the embedding
    /// path can include the fact write inside the same atomic transaction.
    ///
    /// [`assert_fact`]: TemporalGraph::assert_fact
    /// [`assert_fact_with_embedding`]: TemporalGraph::assert_fact_with_embedding
    fn write_fact_in_txn(
        write_txn: &redb::WriteTransaction,
        subject: &str,
        predicate: &str,
        object: Value,
        valid_from: DateTime<Utc>,
    ) -> Result<FactId> {
        let fact = Fact::new(subject, predicate, object, valid_from);
        let fact_id = fact.id.clone();
        let key = format!("{}:{}:{}", subject, predicate, fact.id);
        let value = serde_json::to_string(&fact)?;
        {
            let mut table = write_txn.open_table(FACTS)?;
            table.insert(key.as_str(), value.as_str())?;
        }
        Ok(fact_id)
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
        let write_txn = self.db.begin_write()?;
        let fact_id =
            Self::write_fact_in_txn(&write_txn, subject, predicate, object.into(), valid_from)?;
        write_txn.commit()?;
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
        valid_from: DateTime<Utc>,
    ) -> Result<FactId> {
        let write_txn = self.db.begin_write()?;

        {
            let idem_table = write_txn.open_table(IDEMPOTENCY)?;
            let existing: Option<String> = idem_table
                .get(idempotency_key)?
                .map(|guard| guard.value().to_string());
            if let Some(existing_id) = existing {
                return Ok(FactId(existing_id));
            }
        }

        let fact_id =
            Self::write_fact_in_txn(&write_txn, subject, predicate, object.into(), valid_from)?;

        {
            let mut idem_table = write_txn.open_table(IDEMPOTENCY)?;
            idem_table.insert(idempotency_key, fact_id.0.as_str())?;
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
    pub fn facts_at(&self, subject: &str, predicate: &str, at: DateTime<Utc>) -> Result<Vec<Fact>> {
        let prefix = format!("{}:{}:", subject, predicate);
        self.scan_prefix(&prefix, |f| f.was_valid_at(at))
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
        #[cfg(not(feature = "fulltext"))]
        {
            let _ = (query, limit);
            return Err(KronroeError::Search(
                "fulltext feature is disabled for this build".to_string(),
            ));
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
            let (index, id_field, content_field) =
                Self::build_search_index(&facts, &aliases_by_subject)?;
            let reader = index.reader()?;
            let searcher = reader.searcher();

            let parser = QueryParser::for_index(&index, vec![content_field]);
            let parsed = parser.parse_query(query)?;
            let mut top_docs = searcher.search(&parsed, &TopDocs::with_limit(limit))?;

            // Fuzzy fallback for typo-heavy short queries (e.g. "alcie").
            if top_docs.is_empty() {
                let fuzzy = Self::build_fuzzy_query(query, content_field);
                top_docs = searcher.search(&fuzzy, &TopDocs::with_limit(limit))?;
            }

            let facts_by_id: HashMap<String, Fact> =
                facts.into_iter().map(|f| (f.id.0.clone(), f)).collect();
            let mut results = Vec::new();

            for (_score, addr) in top_docs {
                let retrieved = searcher.doc::<tantivy::schema::TantivyDocument>(addr)?;
                if let Some(id_val) = retrieved.get_first(id_field).and_then(|v| v.as_str()) {
                    if let Some(fact) = facts_by_id.get(id_val) {
                        results.push(fact.clone());
                    }
                }
            }

            Ok(results)
        }
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

    /// Retrieve a fact by its id.
    ///
    /// Phase 0 implementation performs a linear scan.
    pub fn fact_by_id(&self, fact_id: &FactId) -> Result<Fact> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(FACTS)?;
        for entry in table.iter()? {
            let (_k, v) = entry?;
            let fact: Fact = serde_json::from_str(v.value())?;
            if fact.id == *fact_id {
                return Ok(fact);
            }
        }
        Err(KronroeError::NotFound(format!("fact id {fact_id}")))
    }

    /// Correct a fact by id while preserving history.
    ///
    /// The old fact is invalidated at `at`, and a replacement fact is asserted
    /// with the same subject/predicate and a new object value.
    pub fn correct_fact(
        &self,
        fact_id: &FactId,
        new_value: impl Into<Value>,
        at: DateTime<Utc>,
    ) -> Result<FactId> {
        let old = self.fact_by_id(fact_id)?;
        self.invalidate_fact(fact_id, at)?;
        self.assert_fact(&old.subject, &old.predicate, new_value, at)
    }

    /// Assert a fact and durably persist its embedding in a single ACID transaction.
    ///
    /// The fact row, the embedding dimension check-and-set, and the raw embedding
    /// bytes are all written to redb inside **one `WriteTransaction`** and committed
    /// atomically.  The in-memory vector index cache is updated *after* the commit,
    /// so the redb tables are always the source of truth.
    ///
    /// Because redb serialises write transactions, the dimension check-and-set is
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
        valid_from: DateTime<Utc>,
        embedding: Vec<f32>,
    ) -> Result<FactId> {
        if embedding.is_empty() {
            return Err(KronroeError::InvalidEmbedding(
                "embedding must not be empty".into(),
            ));
        }

        // One write transaction covers the dim check-and-set, the fact row,
        // and the embedding bytes.  redb serialises writes, so this is atomic
        // and race-free — no two threads can interleave inside here.
        let write_txn = self.db.begin_write()?;

        // --- dim check-and-set ---
        {
            let mut meta = write_txn.open_table(EMBEDDING_META)?;
            // Extract the stored dim as an owned u64 before the match so that the
            // `AccessGuard` borrow on `meta` is released before `meta.insert`.
            let stored_dim: Option<u64> = meta.get("dim")?.map(|g| g.value());
            match stored_dim {
                None => {
                    meta.insert("dim", embedding.len() as u64)?;
                }
                Some(d) => {
                    let d = d as usize;
                    if embedding.len() != d {
                        // Dropping write_txn triggers an implicit redb rollback.
                        return Err(KronroeError::InvalidEmbedding(format!(
                            "embedding dimension mismatch: expected {d}, got {}",
                            embedding.len()
                        )));
                    }
                }
            }
        }

        // --- fact row ---
        let fact_id =
            Self::write_fact_in_txn(&write_txn, subject, predicate, object.into(), valid_from)?;

        // --- embedding bytes (little-endian f32) ---
        {
            let bytes: Vec<u8> = embedding.iter().flat_map(|x| x.to_le_bytes()).collect();
            let mut emb_table = write_txn.open_table(EMBEDDINGS)?;
            emb_table.insert(fact_id.to_string().as_str(), bytes.as_slice())?;
        }

        write_txn.commit()?;

        // Update the in-memory cache after the durable commit.
        // If the process crashes between commit() and here the cache is rebuilt
        // correctly from redb on the next open().
        self.vector_index
            .lock()
            .unwrap()
            .insert(fact_id.clone(), embedding);

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
        at: Option<DateTime<Utc>>,
    ) -> Result<Vec<(Fact, f32)>> {
        use std::collections::{HashMap, HashSet};

        // Validate query dimension against the established index dimension.
        // Return a clear error rather than silently producing zero-scored results
        // (which `cosine_similarity` would return for mismatched lengths).
        {
            let idx = self.vector_index.lock().unwrap();
            if let Some(d) = idx.dim() {
                if query.len() != d {
                    return Err(KronroeError::InvalidEmbedding(format!(
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
            .unwrap()
            .search(query, k, &valid_ids);

        let results = hits
            .into_iter()
            .filter_map(|(id, score)| facts_by_id.get(&id).map(|f| (f.clone(), score)))
            .collect();

        Ok(results)
    }

    // Internal: scan facts table, filter by prefix, apply predicate.
    fn scan_prefix(&self, prefix: &str, predicate: impl Fn(&Fact) -> bool) -> Result<Vec<Fact>> {
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
    fn build_search_index(
        facts: &[Fact],
        aliases_by_subject: &HashMap<String, Vec<String>>,
    ) -> Result<(Index, Field, Field)> {
        let mut schema_builder = Schema::builder();
        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        let mut writer = index.writer(50_000_000)?;

        for fact in facts {
            let mut content_parts = vec![fact.subject.as_str(), &fact.predicate];
            if let Some(aliases) = aliases_by_subject.get(fact.subject.as_str()) {
                for alias in aliases {
                    content_parts.push(alias.as_str());
                }
            }
            if let Value::Text(v) | Value::Entity(v) = &fact.object {
                content_parts.push(v.as_str());
            }

            // Allow "works at" style matching against snake_case predicates.
            let normalized_predicate = fact.predicate.replace('_', " ");
            let content = format!("{} {}", content_parts.join(" "), normalized_predicate);

            writer.add_document(doc!(
                id_field => fact.id.0.clone(),
                content_field => content,
            ))?;
        }

        writer.commit()?;
        Ok((index, id_field, content_field))
    }

    #[cfg(feature = "fulltext")]
    fn build_fuzzy_query(query: &str, content_field: Field) -> BooleanQuery {
        let terms: Vec<(Occur, Box<dyn tantivy::query::Query>)> = query
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| {
                let term = Term::from_field_text(content_field, token);
                (
                    Occur::Should,
                    Box::new(FuzzyTermQuery::new(term, 1, true)) as Box<dyn tantivy::query::Query>,
                )
            })
            .collect();
        BooleanQuery::new(terms)
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
    fn idempotent_assert_same_key_returns_same_fact_id() {
        let (db, _tmp) = open_temp_db();
        let now = Utc::now();

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
        let now = Utc::now();

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
        let now = Utc::now();

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
        let now = Utc::now();

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

    #[test]
    fn correct_fact_preserves_history_and_creates_replacement() {
        let (db, _tmp) = open_temp_db();
        let jan = dt("2024-01-01T00:00:00Z");
        let feb = dt("2024-02-01T00:00:00Z");

        let old_id = db.assert_fact("alice", "works_at", "Acme", jan).unwrap();
        let new_id = db.correct_fact(&old_id, "BetaCorp", feb).unwrap();

        let old = db.fact_by_id(&old_id).unwrap();
        assert_eq!(old.valid_to, Some(feb));

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
        let now = Utc::now();

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
        let jan = "2024-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let jul = "2024-07-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let mar = "2024-03-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();

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
        db.assert_fact("alice", "works_at", "Acme", Utc::now())
            .unwrap();
        let results = db.search_by_vector(&[1.0, 0.0], 5, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    #[cfg(feature = "fulltext")]
    fn search_returns_expected_facts() {
        let (db, _tmp) = open_temp_db();
        let now = Utc::now();

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
        let now = Utc::now();

        db.assert_fact("alice", "works_at", "Acme", now).unwrap();
        db.assert_fact("alice", "has_alias", "ally", now).unwrap();

        let results = db.search("alcie", 10).unwrap();
        assert!(
            results.iter().any(|f| f.subject == "alice"),
            "fuzzy search should match typo query"
        );
    }

    // ------------------------------------------------------------------
    // Vector: error-path tests (P1 / P2 audit findings)
    // ------------------------------------------------------------------

    #[test]
    #[cfg(feature = "vector")]
    fn vector_empty_embedding_returns_error() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let result = db.assert_fact_with_embedding("alice", "interest", "Rust", Utc::now(), vec![]);
        assert!(
            matches!(result, Err(KronroeError::InvalidEmbedding(_))),
            "empty embedding must return InvalidEmbedding, not panic"
        );
    }

    #[test]
    #[cfg(feature = "vector")]
    fn vector_dim_mismatch_returns_error() {
        let db = TemporalGraph::open_in_memory().unwrap();
        let now = Utc::now();

        // Establish dim = 3.
        db.assert_fact_with_embedding("alice", "interest", "Rust", now, vec![1.0, 0.0, 0.0])
            .unwrap();

        // Subsequent insert with dim = 2 must return Err, not panic.
        let result =
            db.assert_fact_with_embedding("alice", "interest", "Python", now, vec![0.0, 1.0]);
        assert!(
            matches!(result, Err(KronroeError::InvalidEmbedding(_))),
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
        let now = Utc::now();

        // Insert a dim-3 embedding to establish the index dimension.
        db.assert_fact_with_embedding("alice", "interest", "Rust", now, vec![1.0, 0.0, 0.0])
            .unwrap();

        // Query with dim=2 must return Err, not silently score 0.0.
        let result = db.search_by_vector(&[1.0, 0.0], 5, None);
        assert!(
            matches!(result, Err(KronroeError::InvalidEmbedding(_))),
            "wrong query dimension must return InvalidEmbedding"
        );
    }

    /// Embeddings are persisted to redb; the vector index must survive a
    /// close-and-reopen without any re-population by the caller.
    #[test]
    #[cfg(feature = "vector")]
    fn vector_index_survives_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("durability.kronroe");
        let path_str = path.to_str().unwrap();
        let now = Utc::now();

        // Write two embeddings, then drop the database.
        {
            let db = TemporalGraph::open(path_str).unwrap();
            db.assert_fact_with_embedding("alice", "interest", "Rust", now, vec![1.0, 0.0, 0.0])
                .unwrap();
            db.assert_fact_with_embedding("alice", "interest", "Python", now, vec![0.0, 1.0, 0.0])
                .unwrap();
        } // db dropped — file closed

        // Reopen: the index must be rebuilt from redb automatically.
        let db = TemporalGraph::open(path_str).unwrap();
        let results = db.search_by_vector(&[1.0, 0.0, 0.0], 2, None).unwrap();
        assert_eq!(results.len(), 2, "both embeddings must survive reopen");
        assert!(
            matches!(&results[0].0.object, Value::Text(s) if s == "Rust"),
            "most similar fact after reopen should be Rust"
        );
    }
}
