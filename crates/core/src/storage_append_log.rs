use crate::storage::{fact_row_key, StoredFactRow, SCHEMA_VERSION};
use crate::{Fact, FactId, KronroeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const APPEND_LOG_MAGIC: &str = "kronroe-append-log-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
enum AppendLogRecord {
    Header {
        magic: String,
    },
    SchemaVersion {
        version: u64,
    },
    #[cfg(feature = "contradiction")]
    UpsertPredicateRegistryEntry {
        predicate: String,
        encoded: String,
    },
    #[cfg(feature = "uncertainty")]
    UpsertVolatilityRegistryEntry {
        predicate: String,
        encoded: String,
    },
    #[cfg(feature = "uncertainty")]
    UpsertSourceWeightRegistryEntry {
        source: String,
        encoded: String,
    },
    UpsertFact {
        key: String,
        fact: Fact,
    },
    UpsertFactAndIdempotency {
        key: String,
        fact: Fact,
        idempotency_key: String,
    },
    #[cfg(feature = "vector")]
    UpsertFactWithEmbedding {
        key: String,
        fact: Fact,
        embedding: Vec<f32>,
    },
    ReplaceFact {
        key: String,
        fact: Fact,
    },
}

#[derive(Default)]
struct AppendLogState {
    header_present: bool,
    schema_version: Option<u64>,
    #[cfg(feature = "contradiction")]
    predicate_registry: BTreeMap<String, String>,
    #[cfg(feature = "uncertainty")]
    volatility_registry: BTreeMap<String, String>,
    #[cfg(feature = "uncertainty")]
    source_weight_registry: BTreeMap<String, String>,
    #[cfg(feature = "vector")]
    embedding_dim: Option<usize>,
    #[cfg(feature = "vector")]
    embeddings: BTreeMap<String, Vec<f32>>,
    facts: BTreeMap<String, Fact>,
    fact_key_by_id: BTreeMap<String, String>,
    facts_by_subject_predicate: BTreeMap<String, BTreeSet<String>>,
    active_facts_by_subject_predicate: BTreeMap<String, BTreeSet<String>>,
    current_facts_by_subject_predicate: BTreeMap<String, BTreeSet<String>>,
    version_chain_by_subject_predicate: BTreeMap<String, Vec<String>>,
    idempotency: BTreeMap<String, String>,
}

impl AppendLogState {
    fn subject_predicate_prefix(subject: &str, predicate: &str) -> String {
        format!("{subject}:{predicate}:")
    }

    fn insert_fact_index(&mut self, key: &str, fact: &Fact) {
        let prefix = Self::subject_predicate_prefix(&fact.subject, &fact.predicate);
        self.facts_by_subject_predicate
            .entry(prefix.clone())
            .or_default()
            .insert(key.to_string());
        if fact.expired_at.is_none() {
            self.active_facts_by_subject_predicate
                .entry(prefix.clone())
                .or_default()
                .insert(key.to_string());
        }
        if fact.is_currently_valid() {
            self.current_facts_by_subject_predicate
                .entry(prefix.clone())
                .or_default()
                .insert(key.to_string());
        }
        let chain = self
            .version_chain_by_subject_predicate
            .entry(prefix)
            .or_default();
        let insertion_index = chain.partition_point(|existing_key| {
            let existing = self
                .facts
                .get(existing_key)
                .expect("append-log version-chain key should reference a stored fact");
            existing.valid_from <= fact.valid_from
        });
        chain.insert(insertion_index, key.to_string());
    }

    fn remove_fact_index(&mut self, key: &str, fact: &Fact) {
        let prefix = Self::subject_predicate_prefix(&fact.subject, &fact.predicate);
        if let Some(keys) = self.facts_by_subject_predicate.get_mut(&prefix) {
            keys.remove(key);
            if keys.is_empty() {
                self.facts_by_subject_predicate.remove(&prefix);
            }
        }
        if let Some(keys) = self.active_facts_by_subject_predicate.get_mut(&prefix) {
            keys.remove(key);
            if keys.is_empty() {
                self.active_facts_by_subject_predicate.remove(&prefix);
            }
        }
        if let Some(keys) = self.current_facts_by_subject_predicate.get_mut(&prefix) {
            keys.remove(key);
            if keys.is_empty() {
                self.current_facts_by_subject_predicate.remove(&prefix);
            }
        }
        if let Some(chain) = self.version_chain_by_subject_predicate.get_mut(&prefix) {
            if let Some(position) = chain.iter().position(|existing_key| existing_key == key) {
                chain.remove(position);
            }
            if chain.is_empty() {
                self.version_chain_by_subject_predicate.remove(&prefix);
            }
        }
    }

    fn apply_fact_upsert(&mut self, key: String, fact: Fact) {
        if let Some(previous) = self.facts.insert(key.clone(), fact.clone()) {
            self.remove_fact_index(&key, &previous);
        }
        self.fact_key_by_id
            .insert(fact.id.as_str().to_string(), key.clone());
        self.insert_fact_index(&key, &fact);
    }

    #[cfg(feature = "vector")]
    fn apply_embedding_upsert(&mut self, fact_id: &FactId, embedding: Vec<f32>) {
        self.embedding_dim.get_or_insert(embedding.len());
        self.embeddings
            .insert(fact_id.as_str().to_string(), embedding);
    }

    fn apply_record(&mut self, record: AppendLogRecord) {
        match record {
            AppendLogRecord::Header { magic } => {
                self.header_present = magic == APPEND_LOG_MAGIC;
            }
            AppendLogRecord::SchemaVersion { version } => {
                self.schema_version = Some(version);
            }
            #[cfg(feature = "contradiction")]
            AppendLogRecord::UpsertPredicateRegistryEntry { predicate, encoded } => {
                self.predicate_registry.insert(predicate, encoded);
            }
            #[cfg(feature = "uncertainty")]
            AppendLogRecord::UpsertVolatilityRegistryEntry { predicate, encoded } => {
                self.volatility_registry.insert(predicate, encoded);
            }
            #[cfg(feature = "uncertainty")]
            AppendLogRecord::UpsertSourceWeightRegistryEntry { source, encoded } => {
                self.source_weight_registry.insert(source, encoded);
            }
            AppendLogRecord::UpsertFact { key, fact }
            | AppendLogRecord::ReplaceFact { key, fact } => {
                self.apply_fact_upsert(key, fact);
            }
            AppendLogRecord::UpsertFactAndIdempotency {
                key,
                fact,
                idempotency_key,
            } => {
                self.idempotency
                    .insert(idempotency_key, fact.id.as_str().to_string());
                self.apply_fact_upsert(key, fact);
            }
            #[cfg(feature = "vector")]
            AppendLogRecord::UpsertFactWithEmbedding {
                key,
                fact,
                embedding,
            } => {
                self.apply_embedding_upsert(&fact.id, embedding);
                self.apply_fact_upsert(key, fact);
            }
        }
    }
}

#[allow(dead_code)]
enum AppendLogMode {
    InMemory,
    OnDisk(PathBuf),
}

pub(crate) struct AppendLogBackend {
    mode: AppendLogMode,
    state: Mutex<AppendLogState>,
}

#[allow(dead_code)]
impl AppendLogBackend {
    pub(crate) fn open(path: &str) -> Result<Self> {
        let path = PathBuf::from(path);
        let state = if path.exists() {
            Self::load_state_from_path(&path)?
        } else {
            AppendLogState::default()
        };
        Ok(Self {
            mode: AppendLogMode::OnDisk(path),
            state: Mutex::new(state),
        })
    }

    pub(crate) fn open_in_memory() -> Self {
        Self {
            mode: AppendLogMode::InMemory,
            state: Mutex::new(AppendLogState::default()),
        }
    }

    fn load_state_from_path(path: &Path) -> Result<AppendLogState> {
        let file = fs::File::open(path)
            .map_err(|error| KronroeError::Storage(format!("append-log open failed: {error}")))?;
        let reader = BufReader::new(file);
        let mut state = AppendLogState::default();
        let mut first_record = true;
        for (idx, line) in reader.lines().enumerate() {
            let line = line.map_err(|error| {
                if first_record {
                    KronroeError::Storage(format!(
                        "storage backend mismatch: {} is not a Kronroe append-log file ({error})",
                        path.display()
                    ))
                } else {
                    KronroeError::Storage(format!(
                        "append-log read failed at line {} in {}: {error}",
                        idx + 1,
                        path.display()
                    ))
                }
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let record: AppendLogRecord = serde_json::from_str(&line).map_err(|error| {
                if first_record {
                    KronroeError::Storage(format!(
                        "storage backend mismatch: {} is not a Kronroe append-log file (missing header `{APPEND_LOG_MAGIC}`)",
                        path.display()
                    ))
                } else {
                    KronroeError::Storage(format!(
                        "invalid append-log record at line {} in {}: {error}",
                        idx + 1,
                        path.display()
                    ))
                }
            })?;
            if first_record {
                match &record {
                    AppendLogRecord::Header { magic } if magic == APPEND_LOG_MAGIC => {}
                    _ => {
                        return Err(KronroeError::Storage(format!(
                            "storage backend mismatch: {} is not a Kronroe append-log file (missing header `{APPEND_LOG_MAGIC}`)",
                            path.display()
                        )));
                    }
                }
                first_record = false;
            }
            state.apply_record(record);
        }
        Ok(state)
    }

    fn append_record(&self, record: &AppendLogRecord) -> Result<()> {
        if let AppendLogMode::OnDisk(path) = &self.mode {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|error| {
                    KronroeError::Storage(format!("append-log open for append failed: {error}"))
                })?;
            serde_json::to_writer(&mut file, record)?;
            file.write_all(b"\n").map_err(|error| {
                KronroeError::Storage(format!("append-log newline write failed: {error}"))
            })?;
            file.sync_all().map_err(|error| {
                KronroeError::Storage(format!("append-log sync failed: {error}"))
            })?;
        }
        Ok(())
    }

    pub(crate) fn initialize_schema(&self) -> Result<u64> {
        let mut state = self.state.lock().unwrap();
        if !state.header_present {
            let header = AppendLogRecord::Header {
                magic: APPEND_LOG_MAGIC.to_string(),
            };
            self.append_record(&header)?;
            state.apply_record(header);
        }
        match state.schema_version {
            Some(version) => Ok(version),
            None => {
                let record = AppendLogRecord::SchemaVersion {
                    version: SCHEMA_VERSION,
                };
                self.append_record(&record)?;
                state.apply_record(record);
                Ok(SCHEMA_VERSION)
            }
        }
    }

    #[cfg(feature = "contradiction")]
    pub(crate) fn load_predicate_registry_entries(&self) -> Vec<(String, String)> {
        let state = self.state.lock().unwrap();
        state
            .predicate_registry
            .iter()
            .map(|(predicate, encoded)| (predicate.clone(), encoded.clone()))
            .collect()
    }

    #[cfg(feature = "contradiction")]
    pub(crate) fn write_predicate_registry_entry(
        &self,
        predicate: &str,
        encoded: &str,
    ) -> Result<()> {
        let record = AppendLogRecord::UpsertPredicateRegistryEntry {
            predicate: predicate.to_string(),
            encoded: encoded.to_string(),
        };
        let mut state = self.state.lock().unwrap();
        self.append_record(&record)?;
        state.apply_record(record);
        Ok(())
    }

    #[cfg(feature = "uncertainty")]
    pub(crate) fn load_volatility_registry_entries(&self) -> Vec<(String, String)> {
        let state = self.state.lock().unwrap();
        state
            .volatility_registry
            .iter()
            .map(|(predicate, encoded)| (predicate.clone(), encoded.clone()))
            .collect()
    }

    #[cfg(feature = "uncertainty")]
    pub(crate) fn load_source_weight_registry_entries(&self) -> Vec<(String, String)> {
        let state = self.state.lock().unwrap();
        state
            .source_weight_registry
            .iter()
            .map(|(source, encoded)| (source.clone(), encoded.clone()))
            .collect()
    }

    #[cfg(feature = "uncertainty")]
    pub(crate) fn write_volatility_registry_entry(
        &self,
        predicate: &str,
        encoded: &str,
    ) -> Result<()> {
        let record = AppendLogRecord::UpsertVolatilityRegistryEntry {
            predicate: predicate.to_string(),
            encoded: encoded.to_string(),
        };
        let mut state = self.state.lock().unwrap();
        self.append_record(&record)?;
        state.apply_record(record);
        Ok(())
    }

    #[cfg(feature = "uncertainty")]
    pub(crate) fn write_source_weight_registry_entry(
        &self,
        source: &str,
        encoded: &str,
    ) -> Result<()> {
        let record = AppendLogRecord::UpsertSourceWeightRegistryEntry {
            source: source.to_string(),
            encoded: encoded.to_string(),
        };
        let mut state = self.state.lock().unwrap();
        self.append_record(&record)?;
        state.apply_record(record);
        Ok(())
    }

    pub(crate) fn scan_facts(&self, prefix: &str) -> (Vec<StoredFactRow>, usize) {
        let state = self.state.lock().unwrap();
        if let Some(keys) = state.facts_by_subject_predicate.get(prefix) {
            let rows_scanned = keys.len();
            let rows = keys
                .iter()
                .filter_map(|key| {
                    state.facts.get(key).map(|fact| StoredFactRow {
                        key: key.clone(),
                        fact: fact.clone(),
                    })
                })
                .collect();
            return (rows, rows_scanned);
        }

        let rows_scanned = state.facts.len();
        let rows = state
            .facts
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, fact)| StoredFactRow {
                key: key.clone(),
                fact: fact.clone(),
            })
            .collect();
        (rows, rows_scanned)
    }

    pub(crate) fn fact_by_id(&self, fact_id: &FactId) -> (Option<StoredFactRow>, usize) {
        let state = self.state.lock().unwrap();
        let Some(key) = state.fact_key_by_id.get(fact_id.as_str()) else {
            return (None, 0);
        };
        let row = state.facts.get(key).map(|fact| StoredFactRow {
            key: key.clone(),
            fact: fact.clone(),
        });
        (row, 1)
    }

    pub(crate) fn current_facts(
        &self,
        subject: &str,
        predicate: &str,
    ) -> (Vec<StoredFactRow>, usize) {
        let state = self.state.lock().unwrap();
        let prefix = AppendLogState::subject_predicate_prefix(subject, predicate);
        let Some(keys) = state.current_facts_by_subject_predicate.get(&prefix) else {
            return (Vec::new(), 0);
        };
        let rows_scanned = keys.len();
        let rows = keys
            .iter()
            .filter_map(|key| {
                state.facts.get(key).map(|fact| StoredFactRow {
                    key: key.clone(),
                    fact: fact.clone(),
                })
            })
            .collect();
        (rows, rows_scanned)
    }

    pub(crate) fn facts_at(
        &self,
        subject: &str,
        predicate: &str,
        at: chrono::DateTime<chrono::Utc>,
    ) -> (Vec<StoredFactRow>, usize) {
        let state = self.state.lock().unwrap();
        let prefix = AppendLogState::subject_predicate_prefix(subject, predicate);
        let Some(chain) = state.version_chain_by_subject_predicate.get(&prefix) else {
            return (Vec::new(), 0);
        };

        let upper_bound = chain.partition_point(|key| {
            state
                .facts
                .get(key)
                .map(|fact| fact.valid_from <= at)
                .unwrap_or(false)
        });

        let mut rows = Vec::new();
        let mut rows_scanned = 0usize;
        for key in chain[..upper_bound].iter().rev() {
            rows_scanned += 1;
            let Some(fact) = state.facts.get(key) else {
                continue;
            };
            if fact.was_valid_at(at) {
                rows.push(StoredFactRow {
                    key: key.clone(),
                    fact: fact.clone(),
                });
            }
        }

        (rows, rows_scanned)
    }

    pub(crate) fn write_fact(&self, fact: &Fact) -> Result<()> {
        let key = fact_row_key(&fact.subject, &fact.predicate, &fact.id);
        let record = AppendLogRecord::UpsertFact {
            key,
            fact: fact.clone(),
        };
        let mut state = self.state.lock().unwrap();
        self.append_record(&record)?;
        state.apply_record(record);
        Ok(())
    }

    pub(crate) fn replace_fact_row(&self, key: &str, fact: &Fact) -> Result<()> {
        let record = AppendLogRecord::ReplaceFact {
            key: key.to_string(),
            fact: fact.clone(),
        };
        let mut state = self.state.lock().unwrap();
        self.append_record(&record)?;
        state.apply_record(record);
        Ok(())
    }

    pub(crate) fn get_idempotency(&self, idempotency_key: &str) -> Result<Option<FactId>> {
        let state = self.state.lock().unwrap();
        state
            .idempotency
            .get(idempotency_key)
            .map(|fact_id| FactId::parse(fact_id))
            .transpose()
            .map_err(|error| {
                KronroeError::Storage(format!(
                    "corrupt append-log idempotency fact id for key `{idempotency_key}`: {error}"
                ))
            })
    }

    pub(crate) fn write_fact_and_idempotency(
        &self,
        idempotency_key: &str,
        fact: &Fact,
    ) -> Result<FactId> {
        let mut state = self.state.lock().unwrap();
        if let Some(existing) = state.idempotency.get(idempotency_key) {
            return FactId::parse(existing).map_err(|error| {
                KronroeError::Storage(format!(
                    "corrupt append-log idempotency fact id for key `{idempotency_key}`: {error}"
                ))
            });
        }

        let key = fact_row_key(&fact.subject, &fact.predicate, &fact.id);
        let record = AppendLogRecord::UpsertFactAndIdempotency {
            key,
            fact: fact.clone(),
            idempotency_key: idempotency_key.to_string(),
        };
        self.append_record(&record)?;
        state.apply_record(record);
        Ok(fact.id.clone())
    }

    #[cfg(feature = "vector")]
    pub(crate) fn write_fact_with_embedding(&self, fact: &Fact, embedding: &[f32]) -> Result<()> {
        if embedding.is_empty() {
            return Err(KronroeError::InvalidEmbedding(
                "embedding must not be empty".into(),
            ));
        }

        let mut state = self.state.lock().unwrap();
        if let Some(expected_dim) = state.embedding_dim {
            if embedding.len() != expected_dim {
                return Err(KronroeError::InvalidEmbedding(format!(
                    "embedding dimension mismatch: expected {expected_dim}, got {}",
                    embedding.len()
                )));
            }
        }

        let key = fact_row_key(&fact.subject, &fact.predicate, &fact.id);
        let record = AppendLogRecord::UpsertFactWithEmbedding {
            key,
            fact: fact.clone(),
            embedding: embedding.to_vec(),
        };
        self.append_record(&record)?;
        state.apply_record(record);
        Ok(())
    }

    #[cfg(feature = "vector")]
    pub(crate) fn embedding_rows(&self) -> Result<Vec<(FactId, Vec<f32>)>> {
        let state = self.state.lock().unwrap();
        state
            .embeddings
            .iter()
            .map(|(fact_id, embedding)| {
                FactId::parse(fact_id)
                    .map(|id| (id, embedding.clone()))
                    .map_err(|error| {
                        KronroeError::Storage(format!(
                            "corrupt append-log embedding fact id `{fact_id}`: {error}"
                        ))
                    })
            })
            .collect()
    }

    #[cfg(feature = "contradiction")]
    pub(crate) fn write_fact_with_contradiction_check<F>(
        &self,
        subject: &str,
        predicate: &str,
        fact: &Fact,
        reject_on_conflict: bool,
        check: F,
    ) -> Result<(Vec<crate::contradiction::Contradiction>, usize)>
    where
        F: FnOnce(&[Fact]) -> Result<Vec<crate::contradiction::Contradiction>>,
    {
        let prefix = AppendLogState::subject_predicate_prefix(subject, predicate);
        let mut state = self.state.lock().unwrap();
        let (existing, rows_scanned): (Vec<Fact>, usize) =
            if let Some(keys) = state.active_facts_by_subject_predicate.get(&prefix) {
                (
                    keys.iter()
                        .filter_map(|key| state.facts.get(key).cloned())
                        .collect(),
                    keys.len(),
                )
            } else {
                (
                    state
                        .facts
                        .iter()
                        .filter(|(key, _)| key.starts_with(prefix.as_str()))
                        .map(|(_, fact)| fact.clone())
                        .filter(|fact| fact.expired_at.is_none())
                        .collect(),
                    state.facts.len(),
                )
            };

        let contradictions = check(&existing)?;
        if reject_on_conflict && !contradictions.is_empty() {
            return Err(KronroeError::ContradictionRejected(contradictions));
        }

        let key = fact_row_key(&fact.subject, &fact.predicate, &fact.id);
        let record = AppendLogRecord::UpsertFact {
            key,
            fact: fact.clone(),
        };
        self.append_record(&record)?;
        state.apply_record(record);
        Ok((contradictions, rows_scanned))
    }
}
