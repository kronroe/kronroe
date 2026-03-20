use super::*;
use crate::storage::KronroeStorage;
use crate::storage_observability::{StorageEvent, StorageObserver, StorageOperation};
use chrono::Duration as ChronoDuration;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tempfile::tempdir;

#[derive(Clone, Copy)]
enum BenchmarkScale {
    Smoke,
    Baseline,
}

impl BenchmarkScale {
    fn from_env() -> Self {
        match std::env::var("KRONROE_STORAGE_BENCH_SCALE") {
            Ok(value) if value.eq_ignore_ascii_case("smoke") => Self::Smoke,
            _ => Self::Baseline,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Baseline => "baseline",
        }
    }
}

#[derive(Clone, Copy)]
struct BenchmarkConfig {
    ingest_asserts: usize,
    ingest_subjects: usize,
    churn_corrections: usize,
    scan_predicates: usize,
    scan_facts_per_predicate: usize,
    scan_queries: usize,
    historical_versions: usize,
    historical_queries: usize,
    idempotent_unique_keys: usize,
    idempotent_duplicate_rounds: usize,
    #[cfg(feature = "vector")]
    embedding_facts: usize,
}

impl BenchmarkConfig {
    fn for_scale(scale: BenchmarkScale) -> Self {
        match scale {
            BenchmarkScale::Smoke => Self {
                ingest_asserts: 128,
                ingest_subjects: 8,
                churn_corrections: 64,
                scan_predicates: 4,
                scan_facts_per_predicate: 24,
                scan_queries: 32,
                historical_versions: 64,
                historical_queries: 32,
                idempotent_unique_keys: 32,
                idempotent_duplicate_rounds: 4,
                #[cfg(feature = "vector")]
                embedding_facts: 32,
            },
            BenchmarkScale::Baseline => Self {
                ingest_asserts: 5_000,
                ingest_subjects: 64,
                churn_corrections: 1_000,
                scan_predicates: 12,
                scan_facts_per_predicate: 128,
                scan_queries: 256,
                historical_versions: 1_000,
                historical_queries: 256,
                idempotent_unique_keys: 256,
                idempotent_duplicate_rounds: 12,
                #[cfg(feature = "vector")]
                embedding_facts: 512,
            },
        }
    }
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

#[derive(Serialize)]
struct StorageBenchmarkReport {
    generated_at: String,
    scale: &'static str,
    workloads: Vec<WorkloadReport>,
}

#[derive(Serialize)]
struct WorkloadReport {
    workload: String,
    wall_duration_ms: u128,
    parameters: BTreeMap<String, usize>,
    notes: BTreeMap<String, String>,
    operations: Vec<OperationSummary>,
}

#[derive(Serialize)]
struct OperationSummary {
    operation: StorageOperation,
    count: usize,
    success_count: usize,
    failure_count: usize,
    total_duration_ms: u128,
    max_duration_ms: u128,
    total_rows_scanned: usize,
}

fn graph_in_memory(observer: Arc<dyn StorageObserver>) -> TemporalGraph {
    let storage = KronroeStorage::open_in_memory_with_observer(observer).unwrap();
    TemporalGraph::init(storage).unwrap()
}

fn graph_on_disk(path: &str, observer: Arc<dyn StorageObserver>) -> TemporalGraph {
    let storage = KronroeStorage::open_with_observer(path, observer).unwrap();
    TemporalGraph::init(storage).unwrap()
}

fn summarize_operations(events: Vec<StorageEvent>) -> Vec<OperationSummary> {
    let mut summaries: BTreeMap<StorageOperation, OperationSummary> = BTreeMap::new();
    for event in events {
        let summary = summaries
            .entry(event.operation)
            .or_insert_with(|| OperationSummary {
                operation: event.operation,
                count: 0,
                success_count: 0,
                failure_count: 0,
                total_duration_ms: 0,
                max_duration_ms: 0,
                total_rows_scanned: 0,
            });
        summary.count += 1;
        if event.success {
            summary.success_count += 1;
        } else {
            summary.failure_count += 1;
        }
        let duration_ms = event.duration.as_millis();
        summary.total_duration_ms += duration_ms;
        summary.max_duration_ms = summary.max_duration_ms.max(duration_ms);
        summary.total_rows_scanned += event.rows_scanned;
    }
    summaries.into_values().collect()
}

fn build_report(
    workload: &str,
    started_at: Instant,
    observer: Arc<RecordingObserver>,
    parameters: BTreeMap<String, usize>,
    notes: BTreeMap<String, String>,
) -> WorkloadReport {
    let events = observer.events.lock().unwrap().clone();
    WorkloadReport {
        workload: workload.to_string(),
        wall_duration_ms: started_at.elapsed().as_millis(),
        parameters,
        notes,
        operations: summarize_operations(events),
    }
}

fn run_assert_heavy_ingestion(config: BenchmarkConfig) -> WorkloadReport {
    let observer = Arc::new(RecordingObserver::default());
    let db = graph_in_memory(observer.clone());
    let started_at = Instant::now();
    let base = Utc::now();

    for i in 0..config.ingest_asserts {
        let subject = format!("entity-{}", i % config.ingest_subjects);
        let predicate = format!("predicate-{}", i % 7);
        let object = format!("value-{i}");
        db.assert_fact(
            subject.as_str(),
            predicate.as_str(),
            object,
            base + ChronoDuration::milliseconds(i as i64),
        )
        .unwrap();
    }

    let mut parameters = BTreeMap::new();
    parameters.insert("asserts".into(), config.ingest_asserts);
    parameters.insert("subjects".into(), config.ingest_subjects);

    let mut notes = BTreeMap::new();
    notes.insert("result".into(), "assert-only ingestion".into());

    build_report(
        "assert_heavy_ingestion",
        started_at,
        observer,
        parameters,
        notes,
    )
}

fn run_correction_heavy_timeline_churn(config: BenchmarkConfig) -> WorkloadReport {
    let observer = Arc::new(RecordingObserver::default());
    let db = graph_in_memory(observer.clone());
    let started_at = Instant::now();
    let base = Utc::now();

    let mut current_id = db
        .assert_fact("timeline", "works_at", "company-0", base)
        .unwrap();

    for i in 1..=config.churn_corrections {
        current_id = db
            .correct_fact(
                current_id.as_str(),
                format!("company-{i}"),
                base + ChronoDuration::minutes(i as i64),
            )
            .unwrap();
    }

    let current = db.current_facts("timeline", "works_at").unwrap();
    let historical = db
        .facts_at(
            "timeline",
            "works_at",
            base + ChronoDuration::minutes((config.churn_corrections / 2) as i64),
        )
        .unwrap();

    let mut parameters = BTreeMap::new();
    parameters.insert("corrections".into(), config.churn_corrections);

    let mut notes = BTreeMap::new();
    notes.insert("current_fact_count".into(), current.len().to_string());
    notes.insert("midpoint_fact_count".into(), historical.len().to_string());
    notes.insert("last_fact_id".into(), current_id.to_string());

    build_report(
        "correction_heavy_timeline_churn",
        started_at,
        observer,
        parameters,
        notes,
    )
}

fn run_current_state_scan(config: BenchmarkConfig) -> WorkloadReport {
    let observer = Arc::new(RecordingObserver::default());
    let db = graph_in_memory(observer.clone());
    let started_at = Instant::now();
    let base = Utc::now();

    for predicate_idx in 0..config.scan_predicates {
        for fact_idx in 0..config.scan_facts_per_predicate {
            db.assert_fact(
                "hot-subject",
                format!("predicate-{predicate_idx}").as_str(),
                format!("value-{predicate_idx}-{fact_idx}"),
                base + ChronoDuration::seconds(
                    (predicate_idx * config.scan_facts_per_predicate + fact_idx) as i64,
                ),
            )
            .unwrap();
        }
    }

    for query_idx in 0..config.scan_queries {
        let predicate = format!("predicate-{}", query_idx % config.scan_predicates);
        let facts = db.current_facts("hot-subject", predicate.as_str()).unwrap();
        assert_eq!(facts.len(), config.scan_facts_per_predicate);
    }

    let mut parameters = BTreeMap::new();
    parameters.insert("predicates".into(), config.scan_predicates);
    parameters.insert(
        "facts_per_predicate".into(),
        config.scan_facts_per_predicate,
    );
    parameters.insert("queries".into(), config.scan_queries);

    let mut notes = BTreeMap::new();
    notes.insert(
        "hot_subject_width".into(),
        (config.scan_predicates * config.scan_facts_per_predicate).to_string(),
    );

    build_report(
        "current_state_scan",
        started_at,
        observer,
        parameters,
        notes,
    )
}

fn run_historical_point_in_time_scan(config: BenchmarkConfig) -> WorkloadReport {
    let observer = Arc::new(RecordingObserver::default());
    let db = graph_in_memory(observer.clone());
    let started_at = Instant::now();
    let base = Utc::now();

    let mut current_id = db.assert_fact("history", "role", "role-0", base).unwrap();

    for i in 1..config.historical_versions {
        current_id = db
            .correct_fact(
                current_id.as_str(),
                format!("role-{i}"),
                base + ChronoDuration::hours(i as i64),
            )
            .unwrap();
    }

    for i in 0..config.historical_queries {
        let at = base + ChronoDuration::hours((i % config.historical_versions) as i64);
        let facts = db.facts_at("history", "role", at).unwrap();
        assert!(!facts.is_empty());
    }

    let mut parameters = BTreeMap::new();
    parameters.insert("versions".into(), config.historical_versions);
    parameters.insert("queries".into(), config.historical_queries);

    let mut notes = BTreeMap::new();
    notes.insert("last_fact_id".into(), current_id.to_string());

    build_report(
        "historical_point_in_time_scan",
        started_at,
        observer,
        parameters,
        notes,
    )
}

fn run_idempotent_retries(config: BenchmarkConfig) -> WorkloadReport {
    let observer = Arc::new(RecordingObserver::default());
    let db = graph_in_memory(observer.clone());
    let started_at = Instant::now();
    let base = Utc::now();

    for key_idx in 0..config.idempotent_unique_keys {
        let idempotency_key = format!("evt-{key_idx}");
        let first_id = db
            .assert_fact_idempotent(
                idempotency_key.as_str(),
                "session",
                "note",
                format!("payload-{key_idx}"),
                base + ChronoDuration::seconds(key_idx as i64),
            )
            .unwrap();

        for duplicate_idx in 0..config.idempotent_duplicate_rounds {
            let duplicate_id = db
                .assert_fact_idempotent(
                    idempotency_key.as_str(),
                    "session",
                    "note",
                    format!("payload-{key_idx}-{duplicate_idx}"),
                    base + ChronoDuration::seconds((key_idx + duplicate_idx) as i64),
                )
                .unwrap();
            assert_eq!(duplicate_id, first_id);
        }
    }

    let mut parameters = BTreeMap::new();
    parameters.insert("unique_keys".into(), config.idempotent_unique_keys);
    parameters.insert(
        "duplicate_rounds".into(),
        config.idempotent_duplicate_rounds,
    );

    let mut notes = BTreeMap::new();
    notes.insert(
        "total_calls".into(),
        (config.idempotent_unique_keys * (config.idempotent_duplicate_rounds + 1)).to_string(),
    );

    build_report(
        "idempotent_retries",
        started_at,
        observer,
        parameters,
        notes,
    )
}

#[cfg(feature = "vector")]
fn run_embedding_reopen(config: BenchmarkConfig) -> WorkloadReport {
    let temp_dir = tempdir().unwrap();
    let db_path: PathBuf = temp_dir.path().join("storage-bench.kronroe");

    let write_observer = Arc::new(RecordingObserver::default());
    let write_started_at = Instant::now();
    {
        let db = graph_on_disk(db_path.to_str().unwrap(), write_observer.clone());
        let base = Utc::now();
        for i in 0..config.embedding_facts {
            db.assert_fact_with_embedding(
                "vector-subject",
                "interest",
                format!("topic-{i}"),
                base + ChronoDuration::seconds(i as i64),
                vec![1.0, (i % 7) as f32, (i % 11) as f32],
            )
            .unwrap();
        }
    }

    let reopen_observer = Arc::new(RecordingObserver::default());
    let reopen_started_at = Instant::now();
    let reopened = graph_on_disk(db_path.to_str().unwrap(), reopen_observer.clone());
    let query_results = reopened
        .search_by_vector(&[1.0, 0.0, 0.0], 8, None)
        .unwrap();

    let mut parameters = BTreeMap::new();
    parameters.insert("embedding_facts".into(), config.embedding_facts);

    let mut notes = BTreeMap::new();
    notes.insert(
        "write_wall_duration_ms".into(),
        write_started_at.elapsed().as_millis().to_string(),
    );
    notes.insert(
        "reopen_query_result_count".into(),
        query_results.len().to_string(),
    );

    let mut events = write_observer.events.lock().unwrap().clone();
    events.extend(reopen_observer.events.lock().unwrap().clone());

    WorkloadReport {
        workload: "embedding_reopen".into(),
        wall_duration_ms: reopen_started_at.elapsed().as_millis(),
        parameters,
        notes,
        operations: summarize_operations(events),
    }
}

fn run_mixed_session(_config: BenchmarkConfig) -> WorkloadReport {
    let observer = Arc::new(RecordingObserver::default());
    let db = graph_in_memory(observer.clone());
    let started_at = Instant::now();
    let base = Utc::now();

    let first_id = db
        .assert_fact_idempotent("evt-mixed", "alice", "works_at", "Acme", base)
        .unwrap();
    let corrected_id = db
        .correct_fact(
            first_id.as_str(),
            "BetaCorp",
            base + ChronoDuration::hours(1),
        )
        .unwrap();
    db.invalidate_fact(corrected_id.as_str(), base + ChronoDuration::hours(2))
        .unwrap();
    let current = db.current_facts("alice", "works_at").unwrap();
    let historical = db
        .facts_at("alice", "works_at", base + ChronoDuration::minutes(30))
        .unwrap();
    let all = db.all_facts_about("alice").unwrap();

    let mut parameters = BTreeMap::new();
    parameters.insert("session_steps".into(), 5);

    let mut notes = BTreeMap::new();
    notes.insert("current_count".into(), current.len().to_string());
    notes.insert("historical_count".into(), historical.len().to_string());
    notes.insert("all_fact_count".into(), all.len().to_string());

    build_report(
        "mixed_real_task_session",
        started_at,
        observer,
        parameters,
        notes,
    )
}

fn build_storage_benchmark_report(scale: BenchmarkScale) -> StorageBenchmarkReport {
    let config = BenchmarkConfig::for_scale(scale);
    let mut workloads = vec![
        run_assert_heavy_ingestion(config),
        run_correction_heavy_timeline_churn(config),
        run_current_state_scan(config),
        run_historical_point_in_time_scan(config),
        run_idempotent_retries(config),
        run_mixed_session(config),
    ];
    #[cfg(feature = "vector")]
    workloads.push(run_embedding_reopen(config));

    StorageBenchmarkReport {
        generated_at: Utc::now().to_rfc3339(),
        scale: scale.as_str(),
        workloads,
    }
}

fn emit_report(report: &StorageBenchmarkReport) {
    let json = serde_json::to_string_pretty(report).unwrap();
    if let Ok(path) = std::env::var("KRONROE_STORAGE_BENCHMARK_OUTPUT") {
        fs::write(path, &json).unwrap();
    }
    println!("{json}");
}

#[test]
fn storage_benchmark_smoke_produces_workload_report() {
    let report = build_storage_benchmark_report(BenchmarkScale::Smoke);
    assert!(report.workloads.len() >= 6);
    assert!(report
        .workloads
        .iter()
        .all(|workload| !workload.operations.is_empty()));
}

#[test]
#[ignore = "benchmark harness"]
fn print_storage_benchmark_baseline_report() {
    let report = build_storage_benchmark_report(BenchmarkScale::from_env());
    emit_report(&report);
}
