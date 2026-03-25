use crate::storage::{fact_row_key, StoredFactRow, SCHEMA_VERSION};
use crate::{Fact, FactId, KronroeError, KronroeTimestamp, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(target_arch = "wasm32"))]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::os::fd::AsRawFd;

// Direct flock FFI — replaces the `libc` crate dependency.
// flock() is POSIX and available on all our non-WASM targets
// (macOS, Linux, iOS, Android).
#[cfg(not(target_arch = "wasm32"))]
const LOCK_EX: i32 = 2; // Exclusive lock
#[cfg(not(target_arch = "wasm32"))]
const LOCK_NB: i32 = 4; // Non-blocking
#[cfg(not(target_arch = "wasm32"))]
const LOCK_UN: i32 = 8; // Unlock

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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

/// Authoritative append-log replay state.
///
/// Record order is the source of truth. Every index here is derived by replaying
/// that record stream, and replacement-style state is resolved by "latest record
/// wins" semantics.
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

enum AppendLogMode {
    InMemory,
    #[cfg(not(target_arch = "wasm32"))]
    OnDisk {
        path: PathBuf,
        _guard: AppendLogWriteGuard,
    },
}

#[cfg(not(target_arch = "wasm32"))]
struct AppendLogWriteGuard {
    path: PathBuf,
    _process_guard: ProcessOpenGuard,
    _lock_guard: LockFileGuard,
}

#[cfg(not(target_arch = "wasm32"))]
struct ProcessOpenGuard {
    path: PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
struct LockFileGuard {
    path: PathBuf,
    file: File,
}

#[cfg(not(target_arch = "wasm32"))]
fn append_log_open_paths() -> &'static Mutex<BTreeSet<PathBuf>> {
    static OPEN_PATHS: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
    OPEN_PATHS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

#[cfg(not(target_arch = "wasm32"))]
impl ProcessOpenGuard {
    fn acquire(path: &Path) -> Result<Self> {
        let mut open_paths = append_log_open_paths().lock().unwrap();
        if !open_paths.insert(path.to_path_buf()) {
            return Err(KronroeError::storage(format!(
                "append-log database is already open for write in this process: {}",
                path.display()
            )));
        }
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for ProcessOpenGuard {
    fn drop(&mut self) {
        append_log_open_paths().lock().unwrap().remove(&self.path);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl LockFileGuard {
    fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(|error| {
                KronroeError::storage(format!(
                    "append-log lock open failed for {}: {error}",
                    path.display()
                ))
            })?;
        // Retry on EINTR — flock() can be interrupted by signals on mobile
        // (iOS app backgrounding, Android memory pressure notifications).
        loop {
            let rc = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
            if rc == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(KronroeError::storage(format!(
                "append-log is already open for write by another process: {} ({error})",
                path.display()
            )));
        }
        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for LockFileGuard {
    fn drop(&mut self) {
        let _ = unsafe { flock(self.file.as_raw_fd(), LOCK_UN) };
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl AppendLogWriteGuard {
    fn acquire(path: &Path) -> Result<Self> {
        let normalized_path = normalize_storage_path(path)?;
        let process_guard = ProcessOpenGuard::acquire(&normalized_path)?;
        let lock_path = append_log_lock_path(&normalized_path);
        let lock_guard = match LockFileGuard::acquire(&lock_path) {
            Ok(guard) => guard,
            Err(error) => {
                drop(process_guard);
                return Err(error);
            }
        };
        Ok(Self {
            path: normalized_path,
            _process_guard: process_guard,
            _lock_guard: lock_guard,
        })
    }
}

pub(crate) struct AppendLogBackend {
    mode: AppendLogMode,
    state: Mutex<AppendLogState>,
}

impl AppendLogBackend {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open(path: &str) -> Result<Self> {
        let guard = AppendLogWriteGuard::acquire(Path::new(path))?;
        let path = guard.path.clone();
        let state = if path.exists() {
            Self::load_state_from_path(&path)?
        } else {
            AppendLogState::default()
        };
        Ok(Self {
            mode: AppendLogMode::OnDisk {
                path,
                _guard: guard,
            },
            state: Mutex::new(state),
        })
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn open(path: &str) -> Result<Self> {
        Err(KronroeError::storage(format!(
            "on-disk append-log storage is not supported on wasm32 (`{path}`)"
        )))
    }

    pub(crate) fn open_in_memory() -> Self {
        Self {
            mode: AppendLogMode::InMemory,
            state: Mutex::new(AppendLogState::default()),
        }
    }

    fn append_record(&self, record: &AppendLogRecord) -> Result<()> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let AppendLogMode::OnDisk { path, .. } = &self.mode {
                return Self::append_record_to_path(path, record);
            }
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn append_record_to_path(path: &Path, record: &AppendLogRecord) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| {
                KronroeError::storage(format!(
                    "append-log open for append failed for {}: {error}",
                    path.display()
                ))
            })?;
        Self::write_record_line(&mut file, record)?;
        file.sync_all().map_err(|error| {
            KronroeError::storage(format!(
                "append-log sync failed for {}: {error}",
                path.display()
            ))
        })?;
        Ok(())
    }

    fn write_record_line(writer: &mut impl Write, record: &AppendLogRecord) -> Result<()> {
        serde_json::to_writer(&mut *writer, record)?;
        writer.write_all(b"\n").map_err(|error| {
            KronroeError::storage(format!("append-log newline write failed: {error}"))
        })?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_state_from_path(path: &Path) -> Result<AppendLogState> {
        let bytes = fs::read(path).map_err(|error| {
            KronroeError::storage(format!(
                "append-log open failed for {}: {error}",
                path.display()
            ))
        })?;
        if bytes.is_empty() {
            return Ok(AppendLogState::default());
        }

        let ends_with_newline = bytes.last().copied() == Some(b'\n');
        let segments: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
        let mut state = AppendLogState::default();
        let mut saw_valid_record = false;

        for (index, segment) in segments.iter().enumerate() {
            let line_number = index + 1;
            let trimmed = trim_ascii_whitespace(segment);
            if trimmed.is_empty() {
                continue;
            }

            let is_last_segment = index + 1 == segments.len();
            let parsed = serde_json::from_slice::<AppendLogRecord>(trimmed);
            let record = match parsed {
                Ok(record) => record,
                Err(error) => {
                    if saw_valid_record && is_last_segment && !ends_with_newline {
                        return Ok(state);
                    }
                    if !saw_valid_record {
                        return Err(append_log_backend_mismatch(
                            path,
                            format!("missing header `{APPEND_LOG_MAGIC}` ({error})"),
                        ));
                    }
                    return Err(append_log_corruption(
                        path,
                        line_number,
                        format!("invalid append-log record: {error}"),
                    ));
                }
            };

            if !saw_valid_record {
                match &record {
                    AppendLogRecord::Header { magic } if magic == APPEND_LOG_MAGIC => {}
                    AppendLogRecord::Header { magic } => {
                        return Err(append_log_backend_mismatch(
                            path,
                            format!("wrong header `{magic}`, expected `{APPEND_LOG_MAGIC}`"),
                        ));
                    }
                    _ => {
                        return Err(append_log_backend_mismatch(
                            path,
                            format!("missing header `{APPEND_LOG_MAGIC}`"),
                        ));
                    }
                }
            }

            if let AppendLogRecord::SchemaVersion { version } = &record {
                if *version != SCHEMA_VERSION {
                    return Err(KronroeError::schema_mismatch(*version, SCHEMA_VERSION));
                }
            }

            saw_valid_record = true;
            state.apply_record(record);
        }

        Ok(state)
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn compact(&self) -> Result<()> {
        #[cfg(not(target_arch = "wasm32"))]
        if let AppendLogMode::OnDisk { path, .. } = &self.mode {
            let state = self.state.lock().unwrap();
            let temp_path = append_log_temp_path(path);
            let write_result = Self::write_compacted_state(&temp_path, &state);
            if let Err(error) = write_result {
                let _ = fs::remove_file(&temp_path);
                return Err(error);
            }
            fs::rename(&temp_path, path).map_err(|error| {
                KronroeError::storage(format!(
                    "append-log compaction replace failed for {}: {error}",
                    path.display()
                ))
            })?;
            sync_parent_directory(path)?;
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(not(test), allow(dead_code))]
    fn write_compacted_state(path: &Path, state: &AppendLogState) -> Result<()> {
        let records = compaction_records(state)?;
        let mut file = File::create(path).map_err(|error| {
            KronroeError::storage(format!(
                "append-log compaction create failed for {}: {error}",
                path.display()
            ))
        })?;
        for record in records {
            Self::write_record_line(&mut file, &record)?;
        }
        file.sync_all().map_err(|error| {
            KronroeError::storage(format!(
                "append-log compaction sync failed for {}: {error}",
                path.display()
            ))
        })?;
        Ok(())
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
        at: KronroeTimestamp,
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
                KronroeError::storage(format!(
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
                KronroeError::storage(format!(
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
            return Err(KronroeError::invalid_embedding(
                "embedding must not be empty",
            ));
        }

        let mut state = self.state.lock().unwrap();
        if let Some(expected_dim) = state.embedding_dim {
            if embedding.len() != expected_dim {
                return Err(KronroeError::invalid_embedding(format!(
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
                        KronroeError::storage(format!(
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
            return Err(KronroeError::contradiction_rejected(contradictions));
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

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

fn append_log_corruption(path: &Path, line_number: usize, detail: String) -> KronroeError {
    KronroeError::storage(format!(
        "append-log corruption in {} at line {}: {}",
        path.display(),
        line_number,
        detail
    ))
}

fn append_log_backend_mismatch(path: &Path, detail: String) -> KronroeError {
    KronroeError::storage(format!(
        "storage backend mismatch for {}: {}",
        path.display(),
        detail
    ))
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(not(test), allow(dead_code))]
fn append_log_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "append-log".to_string());
    path.with_file_name(format!("{file_name}.compact.tmp"))
}

#[cfg(not(target_arch = "wasm32"))]
fn append_log_lock_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "append-log".to_string());
    path.with_file_name(format!("{file_name}.lock"))
}

#[cfg(not(target_arch = "wasm32"))]
fn normalize_storage_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                KronroeError::storage(format!("failed to resolve current directory: {error}"))
            })?
            .join(path)
    };

    if absolute.exists() {
        return absolute.canonicalize().map_err(|error| {
            KronroeError::storage(format!(
                "failed to canonicalize append-log path {}: {error}",
                absolute.display()
            ))
        });
    }

    let parent = absolute.parent().unwrap_or_else(|| Path::new("."));
    let canonical_parent = parent.canonicalize().map_err(|error| {
        KronroeError::storage(format!(
            "append-log parent directory does not exist for {}: {error}",
            absolute.display()
        ))
    })?;
    Ok(match absolute.file_name() {
        Some(file_name) => canonical_parent.join(file_name),
        None => canonical_parent,
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(not(test), allow(dead_code))]
fn sync_parent_directory(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let directory = File::open(parent).map_err(|error| {
        KronroeError::storage(format!(
            "append-log directory sync open failed for {}: {error}",
            parent.display()
        ))
    })?;
    directory.sync_all().map_err(|error| {
        KronroeError::storage(format!(
            "append-log directory sync failed for {}: {error}",
            parent.display()
        ))
    })
}

#[cfg_attr(not(test), allow(dead_code))]
fn compaction_records(state: &AppendLogState) -> Result<Vec<AppendLogRecord>> {
    let mut records = Vec::new();
    records.push(AppendLogRecord::Header {
        magic: APPEND_LOG_MAGIC.to_string(),
    });
    records.push(AppendLogRecord::SchemaVersion {
        version: state.schema_version.unwrap_or(SCHEMA_VERSION),
    });

    #[cfg(feature = "contradiction")]
    for (predicate, encoded) in &state.predicate_registry {
        records.push(AppendLogRecord::UpsertPredicateRegistryEntry {
            predicate: predicate.clone(),
            encoded: encoded.clone(),
        });
    }

    #[cfg(feature = "uncertainty")]
    for (predicate, encoded) in &state.volatility_registry {
        records.push(AppendLogRecord::UpsertVolatilityRegistryEntry {
            predicate: predicate.clone(),
            encoded: encoded.clone(),
        });
    }

    #[cfg(feature = "uncertainty")]
    for (source, encoded) in &state.source_weight_registry {
        records.push(AppendLogRecord::UpsertSourceWeightRegistryEntry {
            source: source.clone(),
            encoded: encoded.clone(),
        });
    }

    for (key, fact) in &state.facts {
        #[cfg(feature = "vector")]
        if let Some(embedding) = state.embeddings.get(fact.id.as_str()) {
            records.push(AppendLogRecord::UpsertFactWithEmbedding {
                key: key.clone(),
                fact: fact.clone(),
                embedding: embedding.clone(),
            });
            continue;
        }
        records.push(AppendLogRecord::UpsertFact {
            key: key.clone(),
            fact: fact.clone(),
        });
    }

    for (idempotency_key, fact_id) in &state.idempotency {
        let Some(fact_key) = state.fact_key_by_id.get(fact_id) else {
            return Err(KronroeError::storage(format!(
                "append-log compaction failed: missing fact key for idempotency fact id `{fact_id}`"
            )));
        };
        let Some(fact) = state.facts.get(fact_key) else {
            return Err(KronroeError::storage(format!(
                "append-log compaction failed: missing fact row for idempotency fact id `{fact_id}`"
            )));
        };
        records.push(AppendLogRecord::UpsertFactAndIdempotency {
            key: fact_key.clone(),
            fact: fact.clone(),
            idempotency_key: idempotency_key.clone(),
        });
    }

    Ok(records)
}
