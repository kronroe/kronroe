#[cfg(feature = "contradiction")]
use crate::contradiction::Contradiction;
use crate::storage_append_log::AppendLogBackend;
use crate::storage_observability::{
    noop_observer, StorageEvent, StorageObserver, StorageOperation,
};
use crate::{Fact, FactId, KronroeTimestamp, Result};
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

/// Current append-log schema version.
pub(crate) const SCHEMA_VERSION: u64 = 2;

#[derive(Debug, Clone)]
pub(crate) struct StoredFactRow {
    pub(crate) key: String,
    pub(crate) fact: Fact,
}

pub(crate) fn fact_row_key(subject: &str, predicate: &str, fact_id: &FactId) -> String {
    format!("{subject}:{predicate}:{}", fact_id.as_str())
}

/// Kronroe-owned storage facade for the current storage backend.
pub(crate) struct KronroeStorage {
    backend: AppendLogBackend,
    observer: Arc<dyn StorageObserver>,
}

impl KronroeStorage {
    pub(crate) fn open(path: &str) -> Result<Self> {
        Ok(Self {
            backend: AppendLogBackend::open(path)?,
            observer: noop_observer(),
        })
    }

    pub(crate) fn open_in_memory() -> Result<Self> {
        Ok(Self {
            backend: AppendLogBackend::open_in_memory(),
            observer: noop_observer(),
        })
    }

    #[cfg(test)]
    pub(crate) fn open_with_observer(
        path: &str,
        observer: Arc<dyn StorageObserver>,
    ) -> Result<Self> {
        Ok(Self {
            backend: AppendLogBackend::open(path)?,
            observer,
        })
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory_with_observer(observer: Arc<dyn StorageObserver>) -> Result<Self> {
        Ok(Self {
            backend: AppendLogBackend::open_in_memory(),
            observer,
        })
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
        let result = self.backend.initialize_schema();
        self.record(
            StorageOperation::InitializeSchema,
            started_at,
            0,
            result.is_ok(),
        );
        result
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn compact(&self) -> Result<()> {
        let started_at = storage_now();
        let result = self.backend.compact();
        self.record(StorageOperation::Compact, started_at, 0, result.is_ok());
        result
    }

    pub(crate) fn scan_facts(&self, prefix: &str) -> Result<Vec<StoredFactRow>> {
        let started_at = storage_now();
        let (rows, rows_scanned) = self.backend.scan_facts(prefix);
        self.record(StorageOperation::ScanFacts, started_at, rows_scanned, true);
        Ok(rows)
    }

    pub(crate) fn fact_by_id(&self, fact_id: &FactId) -> Result<Option<StoredFactRow>> {
        let started_at = storage_now();
        let (row, rows_scanned) = self.backend.fact_by_id(fact_id);
        self.record(StorageOperation::ScanFacts, started_at, rows_scanned, true);
        Ok(row)
    }

    pub(crate) fn current_facts(
        &self,
        subject: &str,
        predicate: &str,
    ) -> Result<Vec<StoredFactRow>> {
        let started_at = storage_now();
        let (rows, rows_scanned) = self.backend.current_facts(subject, predicate);
        self.record(StorageOperation::ScanFacts, started_at, rows_scanned, true);
        Ok(rows)
    }

    pub(crate) fn facts_at(
        &self,
        subject: &str,
        predicate: &str,
        at: KronroeTimestamp,
    ) -> Result<Vec<StoredFactRow>> {
        let started_at = storage_now();
        let (rows, rows_scanned) = self.backend.facts_at(subject, predicate, at);
        self.record(StorageOperation::ScanFacts, started_at, rows_scanned, true);
        Ok(rows)
    }

    pub(crate) fn write_fact(&self, fact: &Fact) -> Result<()> {
        let started_at = storage_now();
        let result = self.backend.write_fact(fact);
        self.record(StorageOperation::WriteFact, started_at, 0, result.is_ok());
        result
    }

    pub(crate) fn replace_fact_row(&self, key: &str, fact: &Fact) -> Result<()> {
        let started_at = storage_now();
        let result = self.backend.replace_fact_row(key, fact);
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
        let result = self.backend.get_idempotency(idempotency_key);
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
        let result = self
            .backend
            .write_fact_and_idempotency(idempotency_key, fact);
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
        let result = self.backend.write_fact_with_contradiction_check(
            subject,
            predicate,
            fact,
            reject_on_conflict,
            check,
        );
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
        let result = self.backend.write_fact_with_embedding(fact, embedding);
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
        let result = self.backend.embedding_rows();
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
        let result = Ok(self.backend.load_predicate_registry_entries());
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
        let result = self
            .backend
            .write_predicate_registry_entry(predicate, encoded);
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
        let result = Ok(self.backend.load_volatility_registry_entries());
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
        let result = Ok(self.backend.load_source_weight_registry_entries());
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
        let result = self
            .backend
            .write_volatility_registry_entry(predicate, encoded);
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
        let result = self
            .backend
            .write_source_weight_registry_entry(source, encoded);
        self.record(
            StorageOperation::WriteSourceWeightRegistryEntry,
            started_at,
            0,
            result.is_ok(),
        );
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_observability::{StorageEvent, StorageObserver, StorageOperation};
    use crate::{KronroeError, KronroeSpan, Value};
    use std::sync::{Arc, Mutex};

    fn build_fact(subject: &str, predicate: &str, object: impl Into<Value>) -> Fact {
        Fact::new(subject, predicate, object, KronroeTimestamp::now_utc())
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

    #[test]
    fn append_log_fact_and_idempotency_write_is_recoverable_on_disk() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("append-log.kronroe");
        let path_str = path.to_str().unwrap();

        let storage = KronroeStorage::open(path_str).unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        let fact = build_fact("alice", "works_at", "Acme");
        let fact_id = storage
            .write_fact_and_idempotency("evt-append", &fact)
            .unwrap();
        drop(storage);

        let reopened = KronroeStorage::open(path_str).unwrap();
        assert_eq!(reopened.initialize_schema().unwrap(), SCHEMA_VERSION);
        let scanned = reopened.scan_facts("alice:works_at:").unwrap();
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].fact.id, fact_id);
        assert_eq!(
            reopened.get_idempotency("evt-append").unwrap(),
            Some(fact_id),
        );
    }

    #[test]
    fn append_log_replace_fact_row_replays_latest_fact() {
        let storage = KronroeStorage::open_in_memory().unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        let fact = build_fact("alice", "works_at", "Acme");
        storage.write_fact(&fact).unwrap();
        let key = fact_row_key(&fact.subject, &fact.predicate, &fact.id);

        let mut corrected = fact.clone();
        corrected.object = Value::Text("TechCorp".into());
        corrected.expired_at = Some(KronroeTimestamp::now_utc());
        storage.replace_fact_row(&key, &corrected).unwrap();

        let scanned = storage.scan_facts("alice:works_at:").unwrap();
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].fact.object.to_string(), "TechCorp");
    }

    #[test]
    fn append_log_open_rejects_non_append_log_file_with_backend_mismatch() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("invalid-storage.kronroe");
        let path_str = path.to_str().unwrap();

        std::fs::write(path_str, b"not-a-kronroe-append-log\n").unwrap();

        let error = match KronroeStorage::open(path_str) {
            Ok(_) => panic!("append-log backend should reject non-append-log files"),
            Err(error) => error,
        };
        assert!(matches!(error, KronroeError::Storage(_)));
        assert!(error.to_string().contains("storage backend mismatch"));
    }

    #[test]
    fn append_log_scan_records_examined_rows_not_only_matches() {
        let observer = Arc::new(RecordingObserver::default());
        let storage = KronroeStorage::open_in_memory_with_observer(observer.clone()).unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        storage
            .write_fact(&build_fact("alice", "works_at", "Acme"))
            .unwrap();
        storage
            .write_fact(&build_fact("bob", "lives_in", "London"))
            .unwrap();

        let rows = storage.scan_facts("alice:works_at:").unwrap();
        assert_eq!(rows.len(), 1);

        let events = observer.events.lock().unwrap();
        let scan_event = events
            .iter()
            .find(|event| event.operation == StorageOperation::ScanFacts)
            .expect("scan event should be recorded");
        assert_eq!(scan_event.rows_scanned, 1);
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn append_log_contradiction_write_records_examined_rows() {
        let observer = Arc::new(RecordingObserver::default());
        let storage = KronroeStorage::open_in_memory_with_observer(observer.clone()).unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        let existing = build_fact("alice", "works_at", "Acme");
        storage.write_fact(&existing).unwrap();

        let incoming = build_fact("alice", "works_at", "TechCorp");
        let contradictions = storage
            .write_fact_with_contradiction_check("alice", "works_at", &incoming, false, |facts| {
                Ok(vec![Contradiction {
                    existing_fact_id: facts[0].id.to_string(),
                    conflicting_fact_id: incoming.id.to_string(),
                    subject: "alice".into(),
                    predicate: "works_at".into(),
                    overlap_start: incoming.valid_from,
                    overlap_end: None,
                    severity: crate::ConflictSeverity::High,
                    confidence_delta: 0.0,
                    suggested_resolution: crate::SuggestedResolution::ManualReview,
                }])
            })
            .unwrap();
        assert_eq!(contradictions.len(), 1);

        let events = observer.events.lock().unwrap();
        let event = events
            .iter()
            .find(|event| event.operation == StorageOperation::ContradictionCheckedWrite)
            .expect("contradiction event should be recorded");
        assert_eq!(event.rows_scanned, 1);
    }

    #[test]
    fn append_log_fact_by_id_uses_exact_lookup_index() {
        let observer = Arc::new(RecordingObserver::default());
        let storage = KronroeStorage::open_in_memory_with_observer(observer.clone()).unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        let alice = build_fact("alice", "works_at", "Acme");
        let alice_id = alice.id.clone();
        storage.write_fact(&alice).unwrap();
        storage
            .write_fact(&build_fact("bob", "works_at", "BetaCorp"))
            .unwrap();

        let row = storage
            .fact_by_id(&alice_id)
            .unwrap()
            .expect("fact should exist");
        assert_eq!(row.fact.subject, "alice");

        let events = observer.events.lock().unwrap();
        let scan_event = events
            .iter()
            .find(|event| event.operation == StorageOperation::ScanFacts)
            .expect("scan event should be recorded");
        assert_eq!(scan_event.rows_scanned, 1);
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn append_log_contradiction_write_scans_only_transaction_active_candidates() {
        let observer = Arc::new(RecordingObserver::default());
        let storage = KronroeStorage::open_in_memory_with_observer(observer.clone()).unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        let base = KronroeTimestamp::now_utc();
        for i in 0..5 {
            let mut fact = build_fact("timeline", "role", format!("role-{i}"));
            fact.valid_from = base + KronroeSpan::hours(i);
            fact.recorded_at = fact.valid_from;
            if i < 4 {
                fact.expired_at = Some(base + KronroeSpan::hours(i + 1));
            }
            storage.write_fact(&fact).unwrap();
        }

        let incoming = build_fact("timeline", "role", "candidate");
        let contradictions = storage
            .write_fact_with_contradiction_check("timeline", "role", &incoming, false, |facts| {
                assert_eq!(
                    facts.len(),
                    1,
                    "only the live transaction row should be checked"
                );
                Ok(vec![Contradiction {
                    existing_fact_id: facts[0].id.to_string(),
                    conflicting_fact_id: incoming.id.to_string(),
                    subject: "timeline".into(),
                    predicate: "role".into(),
                    overlap_start: incoming.valid_from,
                    overlap_end: None,
                    severity: crate::ConflictSeverity::High,
                    confidence_delta: 0.0,
                    suggested_resolution: crate::SuggestedResolution::ManualReview,
                }])
            })
            .unwrap();
        assert_eq!(contradictions.len(), 1);

        let events = observer.events.lock().unwrap();
        let event = events
            .iter()
            .find(|event| event.operation == StorageOperation::ContradictionCheckedWrite)
            .expect("contradiction event should be recorded");
        assert_eq!(event.rows_scanned, 1);
    }

    #[test]
    fn append_log_partial_prefix_still_reports_full_scan_when_not_indexed() {
        let observer = Arc::new(RecordingObserver::default());
        let storage = KronroeStorage::open_in_memory_with_observer(observer.clone()).unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        storage
            .write_fact(&build_fact("alice", "works_at", "Acme"))
            .unwrap();
        storage
            .write_fact(&build_fact("bob", "works_at", "BetaCorp"))
            .unwrap();

        let rows = storage.scan_facts("alice:").unwrap();
        assert_eq!(rows.len(), 1);

        let events = observer.events.lock().unwrap();
        let scan_event = events
            .iter()
            .find(|event| event.operation == StorageOperation::ScanFacts)
            .expect("scan event should be recorded");
        assert_eq!(scan_event.rows_scanned, 2);
    }

    #[test]
    fn append_log_current_facts_reports_only_live_candidate_rows() {
        let observer = Arc::new(RecordingObserver::default());
        let storage = KronroeStorage::open_in_memory_with_observer(observer.clone()).unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        let mut historical = build_fact("alice", "works_at", "Acme");
        historical.valid_to = Some(KronroeTimestamp::now_utc());
        historical.expired_at = Some(KronroeTimestamp::now_utc());
        storage.write_fact(&historical).unwrap();

        let current = build_fact("alice", "works_at", "TechCorp");
        storage.write_fact(&current).unwrap();
        storage
            .write_fact(&build_fact("bob", "works_at", "BetaCorp"))
            .unwrap();

        let rows = storage.current_facts("alice", "works_at").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fact.object.to_string(), "TechCorp");

        let events = observer.events.lock().unwrap();
        let scan_event = events
            .iter()
            .find(|event| event.operation == StorageOperation::ScanFacts)
            .expect("scan event should be recorded");
        assert_eq!(scan_event.rows_scanned, 1);
    }

    #[test]
    fn append_log_facts_at_scans_only_chain_prefix_before_query_time() {
        let observer = Arc::new(RecordingObserver::default());
        let storage = KronroeStorage::open_in_memory_with_observer(observer.clone()).unwrap();
        assert_eq!(storage.initialize_schema().unwrap(), SCHEMA_VERSION);

        let base = KronroeTimestamp::now_utc();
        let mut ids = Vec::new();
        for i in 0..5 {
            let mut fact = build_fact("timeline", "role", format!("role-{i}"));
            fact.valid_from = base + KronroeSpan::hours(i);
            fact.recorded_at = fact.valid_from;
            if i < 4 {
                fact.valid_to = Some(base + KronroeSpan::hours(i + 1));
                fact.expired_at = Some(base + KronroeSpan::hours(i + 1));
            }
            ids.push(fact.id.clone());
            storage.write_fact(&fact).unwrap();
        }

        let rows = storage
            .facts_at("timeline", "role", base + KronroeSpan::hours(2))
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fact.id, ids[2]);

        let events = observer.events.lock().unwrap();
        let scan_event = events
            .iter()
            .find(|event| event.operation == StorageOperation::ScanFacts)
            .expect("scan event should be recorded");
        assert_eq!(scan_event.rows_scanned, 3);
    }

    #[cfg(feature = "vector")]
    #[test]
    fn append_log_embedding_write_preserves_existing_rows_on_dim_error() {
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
            "failed append-log embedding write must not add fact row"
        );

        let embeddings = storage.embedding_rows().unwrap();
        assert_eq!(
            embeddings.len(),
            1,
            "failed append-log embedding write must not add bytes"
        );
        assert_eq!(embeddings[0].0, first.id);
    }
}
