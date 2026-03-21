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
    UpsertFact {
        key: String,
        fact: Fact,
    },
    UpsertFactAndIdempotency {
        key: String,
        fact: Fact,
        idempotency_key: String,
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
    facts: BTreeMap<String, Fact>,
    facts_by_subject_predicate: BTreeMap<String, BTreeSet<String>>,
    idempotency: BTreeMap<String, String>,
}

impl AppendLogState {
    fn subject_predicate_prefix(subject: &str, predicate: &str) -> String {
        format!("{subject}:{predicate}:")
    }

    fn insert_fact_index(&mut self, key: &str, fact: &Fact) {
        self.facts_by_subject_predicate
            .entry(Self::subject_predicate_prefix(
                &fact.subject,
                &fact.predicate,
            ))
            .or_default()
            .insert(key.to_string());
    }

    fn remove_fact_index(&mut self, key: &str, fact: &Fact) {
        let prefix = Self::subject_predicate_prefix(&fact.subject, &fact.predicate);
        if let Some(keys) = self.facts_by_subject_predicate.get_mut(&prefix) {
            keys.remove(key);
            if keys.is_empty() {
                self.facts_by_subject_predicate.remove(&prefix);
            }
        }
    }

    fn apply_fact_upsert(&mut self, key: String, fact: Fact) {
        if let Some(previous) = self.facts.insert(key.clone(), fact.clone()) {
            self.remove_fact_index(&key, &previous);
        }
        self.insert_fact_index(&key, &fact);
    }

    fn apply_record(&mut self, record: AppendLogRecord) {
        match record {
            AppendLogRecord::Header { magic } => {
                self.header_present = magic == APPEND_LOG_MAGIC;
            }
            AppendLogRecord::SchemaVersion { version } => {
                self.schema_version = Some(version);
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
            if let Some(keys) = state.facts_by_subject_predicate.get(&prefix) {
                (
                    keys.iter()
                        .filter_map(|key| state.facts.get(key).cloned())
                        .filter(|fact| fact.expired_at.is_none())
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
