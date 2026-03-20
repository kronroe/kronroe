#[cfg(feature = "contradiction")]
use crate::contradiction::Contradiction;
use crate::storage_observability::{
    noop_observer, StorageEvent, StorageObserver, StorageOperation,
};
use crate::{fact_id, Fact, FactId, KronroeError, Result, Value};
use chrono::{DateTime, Utc};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

/// On native targets, returns `Instant::now()` for real elapsed-time tracking.
/// On WASM, `Instant` has no clock source and panics, so we return `()` and
/// the `record` method uses `Duration::ZERO`.
#[cfg(not(target_arch = "wasm32"))]
fn storage_now() -> Instant {
    Instant::now()
}

#[cfg(target_arch = "wasm32")]
fn storage_now() {}

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

/// Current on-disk schema version.
///
/// ## Version history
///
/// | Version | Date | What changed |
/// |---------|------|--------------|
/// | 1 | 2026-02-27 | Initial committed format. Tables: `facts`, `idempotency`, `embeddings` (feature=vector), `embedding_meta` (feature=vector). Key: `"{subject}:{predicate}:{fact_id}"`. Value: JSON `Fact`. |
/// | 2 | 2026-03-19 | Kronroe Fact ID migration. Canonical IDs become `kf_...`; existing fact/idempotency/embedding rows auto-migrated on open. |
pub(crate) const SCHEMA_VERSION: u64 = 2;

pub(crate) const META: TableDefinition<&str, u64> = TableDefinition::new("meta");
pub(crate) const FACTS: TableDefinition<&str, &str> = TableDefinition::new("facts");
pub(crate) const IDEMPOTENCY: TableDefinition<&str, &str> = TableDefinition::new("idempotency");
#[cfg(feature = "contradiction")]
pub(crate) const PREDICATE_REGISTRY: TableDefinition<&str, &str> =
    TableDefinition::new("predicate_registry");
#[cfg(feature = "uncertainty")]
pub(crate) const VOLATILITY_REGISTRY: TableDefinition<&str, &str> =
    TableDefinition::new("volatility_registry");
#[cfg(feature = "uncertainty")]
pub(crate) const SOURCE_WEIGHT_REGISTRY: TableDefinition<&str, &str> =
    TableDefinition::new("source_weight_registry");
pub(crate) const EMBEDDINGS: TableDefinition<&str, &[u8]> = TableDefinition::new("embeddings");
pub(crate) const EMBEDDING_META: TableDefinition<&str, u64> =
    TableDefinition::new("embedding_meta");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredFactRecord {
    pub(crate) id: String,
    pub(crate) subject: String,
    pub(crate) predicate: String,
    pub(crate) object: Value,
    pub(crate) valid_from: DateTime<Utc>,
    pub(crate) valid_to: Option<DateTime<Utc>>,
    pub(crate) recorded_at: DateTime<Utc>,
    pub(crate) expired_at: Option<DateTime<Utc>>,
    pub(crate) confidence: f32,
    pub(crate) source: Option<String>,
}

impl StoredFactRecord {
    pub(crate) fn into_fact_with_id(self, id: FactId) -> Fact {
        Fact {
            id,
            subject: self.subject,
            predicate: self.predicate,
            object: self.object,
            valid_from: self.valid_from,
            valid_to: self.valid_to,
            recorded_at: self.recorded_at,
            expired_at: self.expired_at,
            confidence: self.confidence,
            source: self.source,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StoredFactRow {
    pub(crate) key: String,
    pub(crate) fact: Fact,
}

pub(crate) fn fact_row_key(subject: &str, predicate: &str, fact_id: &FactId) -> String {
    format!("{subject}:{predicate}:{}", fact_id.as_str())
}

/// Kronroe-owned storage facade for the current on-disk backend.
///
/// Phase 1 keeps `redb` under the hood while the rest of the engine moves to a
/// Kronroe-shaped storage contract.
pub(crate) struct KronroeStorage {
    db: Database,
    observer: Arc<dyn StorageObserver>,
}

impl KronroeStorage {
    pub(crate) fn open(path: &str) -> Result<Self> {
        let db = Database::create(path)?;
        Ok(Self {
            db,
            observer: noop_observer(),
        })
    }

    pub(crate) fn open_in_memory() -> Result<Self> {
        let backend = redb::backends::InMemoryBackend::new();
        let db = Database::builder().create_with_backend(backend)?;
        Ok(Self {
            db,
            observer: noop_observer(),
        })
    }

    #[cfg(test)]
    pub(crate) fn open_with_observer(
        path: &str,
        observer: Arc<dyn StorageObserver>,
    ) -> Result<Self> {
        let db = Database::create(path)?;
        Ok(Self { db, observer })
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory_with_observer(observer: Arc<dyn StorageObserver>) -> Result<Self> {
        let backend = redb::backends::InMemoryBackend::new();
        let db = Database::builder().create_with_backend(backend)?;
        Ok(Self { db, observer })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn record(
        &self,
        operation: StorageOperation,
        started_at: Instant,
        rows_scanned: usize,
        success: bool,
    ) {
        self.observer.on_event(StorageEvent {
            operation,
            duration: started_at.elapsed(),
            rows_scanned,
            success,
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn record(
        &self,
        operation: StorageOperation,
        _started_at: (),
        rows_scanned: usize,
        success: bool,
    ) {
        self.observer.on_event(StorageEvent {
            operation,
            duration: std::time::Duration::ZERO,
            rows_scanned,
            success,
        });
    }

    pub(crate) fn initialize_schema(&self) -> Result<u64> {
        let started_at = storage_now();
        let result = (|| -> Result<u64> {
            let write_txn = self.db.begin_write()?;
            write_txn.open_table(FACTS)?;
            write_txn.open_table(IDEMPOTENCY)?;
            write_txn.open_table(EMBEDDINGS)?;
            write_txn.open_table(EMBEDDING_META)?;
            #[cfg(feature = "contradiction")]
            {
                write_txn.open_table(PREDICATE_REGISTRY)?;
            }
            #[cfg(feature = "uncertainty")]
            {
                write_txn.open_table(VOLATILITY_REGISTRY)?;
                write_txn.open_table(SOURCE_WEIGHT_REGISTRY)?;
            }

            let stored_version = {
                let mut meta = write_txn.open_table(META)?;
                let stored: Option<u64> = meta.get("schema_version")?.map(|g| g.value());
                match stored {
                    None => {
                        let facts_exist = write_txn
                            .open_table(FACTS)?
                            .iter()?
                            .next()
                            .transpose()?
                            .is_some();
                        let idempotency_exists = write_txn
                            .open_table(IDEMPOTENCY)?
                            .iter()?
                            .next()
                            .transpose()?
                            .is_some();
                        let embeddings_exist = write_txn
                            .open_table(EMBEDDINGS)?
                            .iter()?
                            .next()
                            .transpose()?
                            .is_some();
                        if facts_exist || idempotency_exists || embeddings_exist {
                            1
                        } else {
                            meta.insert("schema_version", SCHEMA_VERSION)?;
                            SCHEMA_VERSION
                        }
                    }
                    Some(v) => v,
                }
            };

            write_txn.commit()?;
            Ok(stored_version)
        })();
        self.record(
            StorageOperation::InitializeSchema,
            started_at,
            0,
            result.is_ok(),
        );
        result
    }

    pub(crate) fn migrate_v1_to_v2(&self) -> Result<()> {
        let started_at = storage_now();
        let result = (|| -> Result<usize> {
            let write_txn = self.db.begin_write()?;

            let facts_rows: Vec<(String, StoredFactRecord)> = {
                let facts = write_txn.open_table(FACTS)?;
                let mut rows = Vec::new();
                for entry in facts.iter()? {
                    let (key, value) = entry?;
                    let fact: StoredFactRecord =
                        serde_json::from_str(value.value()).map_err(|e| {
                            KronroeError::Storage(format!(
                                "invalid v1 fact row `{}` during Fact ID migration: {e}",
                                key.value()
                            ))
                        })?;
                    rows.push((key.value().to_string(), fact));
                }
                rows
            };

            let idempotency_rows: Vec<(String, String)> = {
                let table = write_txn.open_table(IDEMPOTENCY)?;
                let mut rows = Vec::new();
                for entry in table.iter()? {
                    let (key, value) = entry?;
                    rows.push((key.value().to_string(), value.value().to_string()));
                }
                rows
            };

            let embedding_rows: Vec<(String, Vec<u8>)> = {
                let table = write_txn.open_table(EMBEDDINGS)?;
                let mut rows = Vec::new();
                for entry in table.iter()? {
                    let (key, value) = entry?;
                    rows.push((key.value().to_string(), value.value().to_vec()));
                }
                rows
            };

            let mut ordered_facts: Vec<(usize, &StoredFactRecord)> = facts_rows
                .iter()
                .enumerate()
                .map(|(idx, (_key, fact))| (idx, fact))
                .collect();
            ordered_facts.sort_by(|(left_idx, left), (right_idx, right)| {
                left.recorded_at
                    .cmp(&right.recorded_at)
                    .then_with(|| left.id.cmp(&right.id))
                    .then_with(|| left_idx.cmp(right_idx))
            });

            let mut migration_last_ms = 0u64;
            let mut migration_sequence = 0u16;
            let mut migrated_ids: Vec<FactId> = vec![FactId::default(); facts_rows.len()];

            for (idx, fact) in ordered_facts {
                let mut timestamp_ms = fact.recorded_at.timestamp_millis().max(0) as u64;
                if timestamp_ms < migration_last_ms {
                    timestamp_ms = migration_last_ms;
                }
                if timestamp_ms == migration_last_ms {
                    if migration_sequence == u16::MAX {
                        migration_last_ms += 1;
                        migration_sequence = 0;
                    } else {
                        migration_sequence += 1;
                    }
                } else {
                    migration_last_ms = timestamp_ms;
                    migration_sequence = 0;
                }

                let new_id = FactId::from_parts(
                    migration_last_ms,
                    migration_sequence,
                    fact_id::deterministic_entropy(&fact.id),
                );
                migrated_ids[idx] = new_id.clone();
            }

            {
                let mut facts = write_txn.open_table(FACTS)?;
                for (old_key, _fact) in &facts_rows {
                    facts.remove(old_key.as_str())?;
                }
                for ((_, stored_fact), new_id) in facts_rows.iter().zip(migrated_ids.iter()) {
                    let fact = stored_fact.clone().into_fact_with_id(new_id.clone());
                    let key = fact_row_key(&fact.subject, &fact.predicate, &fact.id);
                    let value = serde_json::to_string(&fact)?;
                    facts.insert(key.as_str(), value.as_str())?;
                }
            }

            {
                let mut idempotency = write_txn.open_table(IDEMPOTENCY)?;
                for (key, legacy_id) in &idempotency_rows {
                    let canonical_id = facts_rows
                        .iter()
                        .zip(migrated_ids.iter())
                        .find_map(|((_, fact), new_id)| (fact.id == *legacy_id).then_some(new_id))
                        .ok_or_else(|| {
                            KronroeError::Storage(format!(
                                "missing migrated idempotency mapping for legacy fact id `{legacy_id}`"
                            ))
                        })?;
                    idempotency.insert(key.as_str(), canonical_id.as_str())?;
                }
            }

            {
                let mut embeddings = write_txn.open_table(EMBEDDINGS)?;
                for (old_id, _bytes) in &embedding_rows {
                    embeddings.remove(old_id.as_str())?;
                }
                for (old_id, bytes) in &embedding_rows {
                    let canonical_id = facts_rows
                        .iter()
                        .zip(migrated_ids.iter())
                        .find_map(|((_, fact), new_id)| (fact.id == *old_id).then_some(new_id))
                        .ok_or_else(|| {
                            KronroeError::Storage(format!(
                                "missing migrated embedding mapping for legacy fact id `{old_id}`"
                            ))
                        })?;
                    embeddings.insert(canonical_id.as_str(), bytes.as_slice())?;
                }
            }

            {
                let mut meta = write_txn.open_table(META)?;
                meta.insert("schema_version", SCHEMA_VERSION)?;
            }

            write_txn.commit()?;
            Ok(facts_rows.len())
        })();
        self.record(
            StorageOperation::MigrateV1ToV2,
            started_at,
            result.as_ref().copied().unwrap_or(0),
            result.is_ok(),
        );
        result.map(|_| ())
    }

    pub(crate) fn scan_facts(&self, prefix: &str) -> Result<Vec<StoredFactRow>> {
        let started_at = storage_now();
        let result = (|| -> Result<(Vec<StoredFactRow>, usize)> {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(FACTS)?;
            let mut rows = Vec::new();
            let mut rows_scanned = 0usize;

            for entry in table.iter()? {
                rows_scanned += 1;
                let (k, v) = entry?;
                if k.value().starts_with(prefix) {
                    let fact: Fact = serde_json::from_str(v.value())?;
                    rows.push(StoredFactRow {
                        key: k.value().to_string(),
                        fact,
                    });
                }
            }

            Ok((rows, rows_scanned))
        })();
        self.record(
            StorageOperation::ScanFacts,
            started_at,
            result
                .as_ref()
                .map(|(_rows, rows_scanned)| *rows_scanned)
                .unwrap_or(0),
            result.is_ok(),
        );
        result.map(|(rows, _rows_scanned)| rows)
    }

    pub(crate) fn write_fact(&self, fact: &Fact) -> Result<()> {
        let started_at = storage_now();
        let result = (|| -> Result<()> {
            let key = fact_row_key(&fact.subject, &fact.predicate, &fact.id);
            let value = serde_json::to_string(fact)?;
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(FACTS)?;
                table.insert(key.as_str(), value.as_str())?;
            }
            write_txn.commit()?;
            Ok(())
        })();
        self.record(StorageOperation::WriteFact, started_at, 0, result.is_ok());
        result
    }

    pub(crate) fn replace_fact_row(&self, key: &str, fact: &Fact) -> Result<()> {
        let started_at = storage_now();
        let result = (|| -> Result<()> {
            let value = serde_json::to_string(fact)?;
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(FACTS)?;
                table.insert(key, value.as_str())?;
            }
            write_txn.commit()?;
            Ok(())
        })();
        self.record(
            StorageOperation::ReplaceFactRow,
            started_at,
            0,
            result.is_ok(),
        );
        result
    }

    pub(crate) fn get_idempotency(&self, idempotency_key: &str) -> Result<Option<FactId>> {
        let started_at = storage_now();
        let result = (|| -> Result<Option<FactId>> {
            let read_txn = self.db.begin_read()?;
            let idem_table = match read_txn.open_table(IDEMPOTENCY) {
                Ok(table) => table,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let existing: Option<String> = idem_table
                .get(idempotency_key)?
                .map(|guard| guard.value().to_string());
            existing
                .map(|existing_id| {
                    FactId::parse(&existing_id).map_err(|e| {
                        KronroeError::Storage(format!(
                            "corrupt idempotency fact id `{existing_id}`: {e}"
                        ))
                    })
                })
                .transpose()
        })();
        self.record(
            StorageOperation::GetIdempotency,
            started_at,
            usize::from(
                result
                    .as_ref()
                    .ok()
                    .and_then(|fact_id| fact_id.as_ref())
                    .is_some(),
            ),
            result.is_ok(),
        );
        result
    }

    pub(crate) fn write_fact_and_idempotency(
        &self,
        idempotency_key: &str,
        fact: &Fact,
    ) -> Result<FactId> {
        let started_at = storage_now();
        let result = (|| -> Result<FactId> {
            let write_txn = self.db.begin_write()?;

            {
                let idem_table = write_txn.open_table(IDEMPOTENCY)?;
                let existing: Option<String> = idem_table
                    .get(idempotency_key)?
                    .map(|guard| guard.value().to_string());
                if let Some(existing_id) = existing {
                    return FactId::parse(&existing_id).map_err(|e| {
                        KronroeError::Storage(format!(
                            "corrupt idempotency fact id `{existing_id}`: {e}"
                        ))
                    });
                }
            }

            let key = fact_row_key(&fact.subject, &fact.predicate, &fact.id);
            let value = serde_json::to_string(fact)?;
            {
                let mut facts = write_txn.open_table(FACTS)?;
                facts.insert(key.as_str(), value.as_str())?;
            }
            {
                let mut idem_table = write_txn.open_table(IDEMPOTENCY)?;
                idem_table.insert(idempotency_key, fact.id.as_str())?;
            }

            write_txn.commit()?;
            Ok(fact.id.clone())
        })();
        self.record(
            StorageOperation::WriteFactAndIdempotency,
            started_at,
            0,
            result.is_ok(),
        );
        result
    }

    #[cfg(feature = "contradiction")]
    pub(crate) fn write_fact_with_contradiction_check<F>(
        &self,
        subject: &str,
        predicate: &str,
        fact: &Fact,
        reject_on_conflict: bool,
        check: F,
    ) -> Result<Vec<Contradiction>>
    where
        F: FnOnce(&[Fact]) -> Result<Vec<Contradiction>>,
    {
        let started_at = storage_now();
        let result = (|| -> Result<(Vec<Contradiction>, usize)> {
            let write_txn = self.db.begin_write()?;
            let prefix = format!("{subject}:{predicate}:");

            let existing: Vec<Fact> = {
                let table = write_txn.open_table(FACTS)?;
                let mut results = Vec::new();
                for entry in table.iter()? {
                    let (k, v) = entry?;
                    if k.value().starts_with(prefix.as_str()) {
                        let stored_fact: Fact = serde_json::from_str(v.value())?;
                        if stored_fact.expired_at.is_none() {
                            results.push(stored_fact);
                        }
                    }
                }
                results
            };

            let contradictions = check(&existing)?;
            if reject_on_conflict && !contradictions.is_empty() {
                drop(write_txn);
                return Err(KronroeError::ContradictionRejected(contradictions));
            }

            let key = fact_row_key(&fact.subject, &fact.predicate, &fact.id);
            let value = serde_json::to_string(fact)?;
            {
                let mut table = write_txn.open_table(FACTS)?;
                table.insert(key.as_str(), value.as_str())?;
            }
            write_txn.commit()?;
            Ok((contradictions, existing.len()))
        })();
        self.record(
            StorageOperation::ContradictionCheckedWrite,
            started_at,
            result.as_ref().map(|(_, rows)| *rows).unwrap_or(0),
            result.is_ok(),
        );
        result.map(|(contradictions, _)| contradictions)
    }

    #[cfg(feature = "vector")]
    pub(crate) fn write_fact_with_embedding(&self, fact: &Fact, embedding: &[f32]) -> Result<()> {
        let started_at = storage_now();
        let result = (|| -> Result<()> {
            if embedding.is_empty() {
                return Err(KronroeError::InvalidEmbedding(
                    "embedding must not be empty".into(),
                ));
            }

            let write_txn = self.db.begin_write()?;

            {
                let mut meta = write_txn.open_table(EMBEDDING_META)?;
                let stored_dim: Option<u64> = meta.get("dim")?.map(|g| g.value());
                match stored_dim {
                    None => {
                        meta.insert("dim", embedding.len() as u64)?;
                    }
                    Some(d) => {
                        let d = d as usize;
                        if embedding.len() != d {
                            return Err(KronroeError::InvalidEmbedding(format!(
                                "embedding dimension mismatch: expected {d}, got {}",
                                embedding.len()
                            )));
                        }
                    }
                }
            }

            let key = fact_row_key(&fact.subject, &fact.predicate, &fact.id);
            let value = serde_json::to_string(fact)?;
            {
                let mut facts = write_txn.open_table(FACTS)?;
                facts.insert(key.as_str(), value.as_str())?;
            }

            let bytes: Vec<u8> = embedding.iter().flat_map(|x| x.to_le_bytes()).collect();
            {
                let mut emb_table = write_txn.open_table(EMBEDDINGS)?;
                emb_table.insert(fact.id.as_str(), bytes.as_slice())?;
            }

            write_txn.commit()?;
            Ok(())
        })();
        self.record(
            StorageOperation::WriteFactWithEmbedding,
            started_at,
            0,
            result.is_ok(),
        );
        result
    }

    #[cfg(feature = "vector")]
    pub(crate) fn embedding_rows(&self) -> Result<Vec<(FactId, Vec<f32>)>> {
        let started_at = storage_now();
        let result = (|| -> Result<Vec<(FactId, Vec<f32>)>> {
            let read_txn = self.db.begin_read()?;
            let emb_table = match read_txn.open_table(EMBEDDINGS) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
                Err(e) => return Err(KronroeError::Storage(e.to_string())),
            };

            let mut rows = Vec::new();
            for entry in emb_table.iter()? {
                let (key, value) = entry?;
                let fact_id = FactId::parse(key.value()).map_err(|e| {
                    KronroeError::Storage(format!(
                        "corrupt embedding fact id `{}` while rebuilding vector index: {e}",
                        key.value()
                    ))
                })?;
                let bytes = value.value();

                if bytes.len() % 4 != 0 {
                    return Err(KronroeError::Storage(format!(
                        "corrupt embedding for fact {fact_id}: byte length {} is not a multiple of 4",
                        bytes.len()
                    )));
                }

                let embedding: Vec<f32> = bytes
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                rows.push((fact_id, embedding));
            }

            Ok(rows)
        })();
        self.record(
            StorageOperation::EmbeddingRows,
            started_at,
            result.as_ref().map(|rows| rows.len()).unwrap_or(0),
            result.is_ok(),
        );
        result
    }

    #[cfg(feature = "contradiction")]
    pub(crate) fn load_predicate_registry_entries(&self) -> Result<Vec<(String, String)>> {
        let started_at = storage_now();
        let result = (|| -> Result<Vec<(String, String)>> {
            let read_txn = self.db.begin_read()?;
            let reg_table = match read_txn.open_table(PREDICATE_REGISTRY) {
                Ok(table) => table,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
                Err(error) => return Err(error.into()),
            };

            let mut rows = Vec::new();
            for entry in reg_table.iter()? {
                let (k, v) = entry?;
                rows.push((k.value().to_string(), v.value().to_string()));
            }
            Ok(rows)
        })();
        self.record(
            StorageOperation::LoadPredicateRegistryEntries,
            started_at,
            result.as_ref().map(|rows| rows.len()).unwrap_or(0),
            result.is_ok(),
        );
        result
    }

    #[cfg(feature = "contradiction")]
    pub(crate) fn write_predicate_registry_entry(
        &self,
        predicate: &str,
        encoded: &str,
    ) -> Result<()> {
        let started_at = storage_now();
        let result = (|| -> Result<()> {
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(PREDICATE_REGISTRY)?;
                table.insert(predicate, encoded)?;
            }
            write_txn.commit()?;
            Ok(())
        })();
        self.record(
            StorageOperation::WritePredicateRegistryEntry,
            started_at,
            0,
            result.is_ok(),
        );
        result
    }

    #[cfg(feature = "uncertainty")]
    pub(crate) fn load_volatility_registry_entries(&self) -> Result<Vec<(String, String)>> {
        let started_at = storage_now();
        let result = (|| -> Result<Vec<(String, String)>> {
            let read_txn = self.db.begin_read()?;
            let table = match read_txn.open_table(VOLATILITY_REGISTRY) {
                Ok(table) => table,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
                Err(error) => return Err(error.into()),
            };
            let mut rows = Vec::new();
            for entry in table.iter()? {
                let (k, v) = entry?;
                rows.push((k.value().to_string(), v.value().to_string()));
            }
            Ok(rows)
        })();
        self.record(
            StorageOperation::LoadVolatilityRegistryEntries,
            started_at,
            result.as_ref().map(|rows| rows.len()).unwrap_or(0),
            result.is_ok(),
        );
        result
    }

    #[cfg(feature = "uncertainty")]
    pub(crate) fn load_source_weight_registry_entries(&self) -> Result<Vec<(String, String)>> {
        let started_at = storage_now();
        let result = (|| -> Result<Vec<(String, String)>> {
            let read_txn = self.db.begin_read()?;
            let table = match read_txn.open_table(SOURCE_WEIGHT_REGISTRY) {
                Ok(table) => table,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
                Err(error) => return Err(error.into()),
            };
            let mut rows = Vec::new();
            for entry in table.iter()? {
                let (k, v) = entry?;
                rows.push((k.value().to_string(), v.value().to_string()));
            }
            Ok(rows)
        })();
        self.record(
            StorageOperation::LoadSourceWeightRegistryEntries,
            started_at,
            result.as_ref().map(|rows| rows.len()).unwrap_or(0),
            result.is_ok(),
        );
        result
    }

    #[cfg(feature = "uncertainty")]
    pub(crate) fn write_volatility_registry_entry(
        &self,
        predicate: &str,
        encoded: &str,
    ) -> Result<()> {
        let started_at = storage_now();
        let result = (|| -> Result<()> {
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(VOLATILITY_REGISTRY)?;
                table.insert(predicate, encoded)?;
            }
            write_txn.commit()?;
            Ok(())
        })();
        self.record(
            StorageOperation::WriteVolatilityRegistryEntry,
            started_at,
            0,
            result.is_ok(),
        );
        result
    }

    #[cfg(feature = "uncertainty")]
    pub(crate) fn write_source_weight_registry_entry(
        &self,
        source: &str,
        encoded: &str,
    ) -> Result<()> {
        let started_at = storage_now();
        let result = (|| -> Result<()> {
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(SOURCE_WEIGHT_REGISTRY)?;
                table.insert(source, encoded)?;
            }
            write_txn.commit()?;
            Ok(())
        })();
        self.record(
            StorageOperation::WriteSourceWeightRegistryEntry,
            started_at,
            0,
            result.is_ok(),
        );
        result
    }

    #[cfg(test)]
    pub(crate) fn seed_schema_v1_file(
        path: &str,
        facts: &[StoredFactRecord],
        idempotency: &[(&str, &str)],
        embeddings: &[(&str, &[f32])],
    ) -> Result<()> {
        let raw = Database::create(path)?;
        let txn = raw.begin_write()?;

        {
            let mut facts_table = txn.open_table(FACTS)?;
            for fact in facts {
                let key = format!("{}:{}:{}", fact.subject, fact.predicate, fact.id);
                let value = serde_json::to_string(fact)?;
                facts_table.insert(key.as_str(), value.as_str())?;
            }
        }

        {
            let mut idempotency_table = txn.open_table(IDEMPOTENCY)?;
            for (key, fact_id) in idempotency {
                idempotency_table.insert(*key, *fact_id)?;
            }
        }

        {
            let mut embeddings_table = txn.open_table(EMBEDDINGS)?;
            let mut embedding_meta = txn.open_table(EMBEDDING_META)?;
            if let Some((_, first)) = embeddings.first() {
                embedding_meta.insert("dim", first.len() as u64)?;
            }
            for (fact_id, embedding) in embeddings {
                let bytes: Vec<u8> = embedding
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect();
                embeddings_table.insert(*fact_id, bytes.as_slice())?;
            }
        }

        {
            let mut meta = txn.open_table(META)?;
            meta.insert("schema_version", 1)?;
        }

        txn.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn write_schema_version_for_test(path: &str, version: u64) -> Result<()> {
        let raw = Database::create(path)?;
        let txn = raw.begin_write()?;
        {
            let mut meta = txn.open_table(META)?;
            meta.insert("schema_version", version)?;
        }
        txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_observability::{StorageEvent, StorageObserver, StorageOperation};
    use std::sync::{Arc, Mutex};

    fn build_fact(subject: &str, predicate: &str, object: impl Into<Value>) -> Fact {
        Fact::new(subject, predicate, object, Utc::now())
    }

    #[derive(Default)]
    struct RecordingObserver {
        events: Mutex<Vec<StorageEvent>>,
    }

    impl StorageObserver for RecordingObserver {
        fn on_event(&self, event: StorageEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn storage_fact_and_idempotency_write_is_atomic() {
        let storage = KronroeStorage::open_in_memory().unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        let fact = build_fact("alice", "works_at", "Acme");
        let fact_id = storage.write_fact_and_idempotency("evt-1", &fact).unwrap();

        let scanned = storage.scan_facts("alice:works_at:").unwrap();
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].fact.id, fact_id);

        let stored = storage.get_idempotency("evt-1").unwrap();
        assert_eq!(stored, Some(fact_id));
    }

    #[test]
    fn storage_embedding_write_preserves_existing_rows_on_dim_error() {
        let storage = KronroeStorage::open_in_memory().unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        let first = build_fact("alice", "interest", "Rust");
        storage
            .write_fact_with_embedding(&first, &[1.0, 0.0, 0.0])
            .unwrap();

        let second = build_fact("alice", "interest", "Python");
        let error = storage
            .write_fact_with_embedding(&second, &[0.0, 1.0])
            .unwrap_err();
        assert!(matches!(error, KronroeError::InvalidEmbedding(_)));

        let scanned = storage.scan_facts("alice:interest:").unwrap();
        assert_eq!(
            scanned.len(),
            1,
            "failed embedding write must not add fact row"
        );

        let embeddings = storage.embedding_rows().unwrap();
        assert_eq!(
            embeddings.len(),
            1,
            "failed embedding write must not add bytes"
        );
        assert_eq!(embeddings[0].0, first.id);
    }

    #[test]
    fn storage_observer_records_scan_and_write_events() {
        let observer = Arc::new(RecordingObserver::default());
        let storage = KronroeStorage::open_in_memory_with_observer(observer.clone()).unwrap();

        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);
        let fact = build_fact("alice", "works_at", "Acme");
        storage.write_fact(&fact).unwrap();
        let rows = storage.scan_facts("alice:works_at:").unwrap();
        assert_eq!(rows.len(), 1);

        let events = observer.events.lock().unwrap();
        assert!(events.iter().any(|event| {
            event.operation == StorageOperation::InitializeSchema && event.success
        }));
        assert!(events
            .iter()
            .any(|event| { event.operation == StorageOperation::WriteFact && event.success }));
        assert!(events.iter().any(|event| {
            event.operation == StorageOperation::ScanFacts
                && event.success
                && event.rows_scanned == 1
        }));
    }
}
