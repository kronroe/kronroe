use ::chrono::{DateTime, Utc};
use kronroe_agent_memory::{
    AgentMemory, ConfidenceShift, FactCorrection, MemoryHealthReport, RecallForTaskReport,
    RecallOptions, RecallScore, WhatChangedReport,
};
use kronroe_core::{Fact, TemporalGraph, Value};
#[cfg(feature = "hybrid")]
use kronroe_core::{TemporalIntent, TemporalOperator};
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyType};

fn to_py_err<E: std::fmt::Display>(err: E) -> PyErr {
    PyRuntimeError::new_err(err.to_string())
}

fn py_to_value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if let Ok(b) = obj.extract::<bool>() {
        Ok(Value::Boolean(b))
    } else if let Ok(n) = obj.extract::<f64>() {
        Ok(Value::Number(n))
    } else if let Ok(s) = obj.extract::<String>() {
        Ok(Value::Text(s))
    } else {
        Err(PyTypeError::new_err(
            "object must be str, int, float, or bool",
        ))
    }
}

fn parse_query_embedding(query_embedding: Option<Vec<f64>>) -> PyResult<Option<Vec<f32>>> {
    let Some(values) = query_embedding else {
        return Ok(None);
    };
    if values.is_empty() {
        return Err(PyValueError::new_err("query_embedding must not be empty"));
    }

    let mut out = Vec::with_capacity(values.len());
    for value in values {
        if !value.is_finite() {
            return Err(PyValueError::new_err(
                "query_embedding must contain finite numbers",
            ));
        }
        let narrowed = value as f32;
        if !narrowed.is_finite() {
            return Err(PyValueError::new_err(
                "query_embedding values overflow f32 range",
            ));
        }
        out.push(narrowed);
    }
    Ok(Some(out))
}

#[derive(Debug)]
struct RecallScoredArgs {
    limit: usize,
    query_embedding: Option<Vec<f64>>,
    min_confidence: Option<f64>,
    confidence_filter_mode: Option<String>,
    max_scored_rows: Option<usize>,
    use_hybrid: bool,
    temporal_intent: Option<String>,
    temporal_operator: Option<String>,
}

impl Default for RecallScoredArgs {
    fn default() -> Self {
        Self {
            limit: 10,
            query_embedding: None,
            min_confidence: None,
            confidence_filter_mode: None,
            max_scored_rows: None,
            use_hybrid: false,
            temporal_intent: None,
            temporal_operator: None,
        }
    }
}

fn parse_recall_scored_args_from_options(
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<RecallScoredArgs> {
    let mut args = RecallScoredArgs::default();
    let Some(options) = options else {
        return Ok(args);
    };

    if let Some(value) = options.get_item("limit")? {
        if !value.is_none() {
            args.limit = value.extract::<usize>()?;
        }
    }
    if let Some(value) = options.get_item("query_embedding")? {
        if !value.is_none() {
            args.query_embedding = Some(value.extract::<Vec<f64>>()?);
        }
    }
    if let Some(value) = options.get_item("min_confidence")? {
        if !value.is_none() {
            args.min_confidence = Some(value.extract::<f64>()?);
        }
    }
    if let Some(value) = options.get_item("confidence_filter_mode")? {
        if !value.is_none() {
            args.confidence_filter_mode = Some(value.extract::<String>()?);
        }
    }
    if let Some(value) = options.get_item("max_scored_rows")? {
        if !value.is_none() {
            args.max_scored_rows = Some(value.extract::<usize>()?);
        }
    }
    if let Some(value) = options.get_item("use_hybrid")? {
        if !value.is_none() {
            args.use_hybrid = value.extract::<bool>()?;
        }
    }
    if let Some(value) = options.get_item("temporal_intent")? {
        if !value.is_none() {
            args.temporal_intent = Some(value.extract::<String>()?);
        }
    }
    if let Some(value) = options.get_item("temporal_operator")? {
        if !value.is_none() {
            args.temporal_operator = Some(value.extract::<String>()?);
        }
    }

    Ok(args)
}

fn fact_to_dict<'py>(py: Python<'py>, fact: &Fact) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("id", fact.id.as_str())?;
    d.set_item("subject", fact.subject.clone())?;
    d.set_item("predicate", fact.predicate.clone())?;
    match &fact.object {
        Value::Text(v) | Value::Entity(v) => d.set_item("object", v)?,
        Value::Number(v) => d.set_item("object", v)?,
        Value::Boolean(v) => d.set_item("object", v)?,
    };
    d.set_item(
        "object_type",
        match fact.object {
            Value::Text(_) => "text",
            Value::Number(_) => "number",
            Value::Boolean(_) => "boolean",
            Value::Entity(_) => "entity",
        },
    )?;
    d.set_item("valid_from", fact.valid_from.to_rfc3339())?;
    d.set_item("valid_to", fact.valid_to.map(|v| v.to_rfc3339()))?;
    d.set_item("recorded_at", fact.recorded_at.to_rfc3339())?;
    d.set_item("expired_at", fact.expired_at.map(|v| v.to_rfc3339()))?;
    d.set_item("confidence", fact.confidence)?;
    d.set_item("source", fact.source.clone())?;
    Ok(d)
}

fn facts_to_pylist(py: Python<'_>, facts: Vec<Fact>) -> PyResult<Vec<Py<PyDict>>> {
    let mut out = Vec::with_capacity(facts.len());
    for fact in &facts {
        out.push(fact_to_dict(py, fact)?.unbind());
    }
    Ok(out)
}

fn recall_score_to_dict(py: Python<'_>, score: &RecallScore) -> PyResult<Py<PyDict>> {
    let d = PyDict::new(py);
    match score {
        RecallScore::TextOnly {
            rank,
            bm25_score,
            confidence,
            effective_confidence,
            ..
        } => {
            d.set_item("type", "text")?;
            d.set_item("rank", *rank as u64)?;
            d.set_item("bm25_score", *bm25_score)?;
            d.set_item("confidence", *confidence)?;
            d.set_item("effective_confidence", *effective_confidence)?;
        }
        RecallScore::Hybrid {
            rrf_score,
            text_contrib,
            vector_contrib,
            confidence,
            effective_confidence,
            ..
        } => {
            d.set_item("type", "hybrid")?;
            d.set_item("rrf_score", *rrf_score)?;
            d.set_item("text_contrib", *text_contrib)?;
            d.set_item("vector_contrib", *vector_contrib)?;
            d.set_item("confidence", *confidence)?;
            d.set_item("effective_confidence", *effective_confidence)?;
        }
        _ => {
            d.set_item("type", "unsupported")?;
        }
    }
    Ok(d.unbind())
}

fn recall_for_task_report_to_dict<'py>(
    py: Python<'py>,
    report: &RecallForTaskReport,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("task", report.task.clone())?;
    d.set_item("subject", report.subject.clone())?;
    d.set_item("generated_at", report.generated_at.to_rfc3339())?;
    d.set_item("horizon_days", report.horizon_days)?;
    d.set_item("query_used", report.query_used.clone())?;

    let mut key_facts = Vec::with_capacity(report.key_facts.len());
    for fact in &report.key_facts {
        key_facts.push(fact_to_dict(py, fact)?.unbind());
    }
    d.set_item("key_facts", key_facts)?;
    d.set_item("low_confidence_count", report.low_confidence_count)?;
    d.set_item("stale_high_impact_count", report.stale_high_impact_count)?;
    d.set_item("contradiction_count", report.contradiction_count)?;
    d.set_item("watchouts", report.watchouts.clone())?;
    d.set_item(
        "recommended_next_checks",
        report.recommended_next_checks.clone(),
    )?;
    Ok(d)
}

fn fact_correction_to_dict(py: Python<'_>, correction: &FactCorrection) -> PyResult<Py<PyDict>> {
    let d = PyDict::new(py);
    d.set_item("old_fact", fact_to_dict(py, &correction.old_fact)?.unbind())?;
    d.set_item("new_fact", fact_to_dict(py, &correction.new_fact)?.unbind())?;
    Ok(d.unbind())
}

fn confidence_shift_to_dict(py: Python<'_>, shift: &ConfidenceShift) -> PyResult<Py<PyDict>> {
    let d = PyDict::new(py);
    d.set_item("from_fact_id", shift.from_fact_id.as_str())?;
    d.set_item("to_fact_id", shift.to_fact_id.as_str())?;
    d.set_item("from_confidence", shift.from_confidence)?;
    d.set_item("to_confidence", shift.to_confidence)?;
    Ok(d.unbind())
}

fn what_changed_report_to_dict<'py>(
    py: Python<'py>,
    report: &WhatChangedReport,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("entity", report.entity.clone())?;
    d.set_item("since", report.since.to_rfc3339())?;
    d.set_item("predicate_filter", report.predicate_filter.clone())?;

    let mut new_facts = Vec::with_capacity(report.new_facts.len());
    for fact in &report.new_facts {
        new_facts.push(fact_to_dict(py, fact)?.unbind());
    }
    d.set_item("new_facts", new_facts)?;

    let mut invalidated_facts = Vec::with_capacity(report.invalidated_facts.len());
    for fact in &report.invalidated_facts {
        invalidated_facts.push(fact_to_dict(py, fact)?.unbind());
    }
    d.set_item("invalidated_facts", invalidated_facts)?;

    let mut corrections = Vec::with_capacity(report.corrections.len());
    for correction in &report.corrections {
        corrections.push(fact_correction_to_dict(py, correction)?);
    }
    d.set_item("corrections", corrections)?;

    let mut confidence_shifts = Vec::with_capacity(report.confidence_shifts.len());
    for shift in &report.confidence_shifts {
        confidence_shifts.push(confidence_shift_to_dict(py, shift)?);
    }
    d.set_item("confidence_shifts", confidence_shifts)?;

    Ok(d)
}

fn memory_health_report_to_dict<'py>(
    py: Python<'py>,
    report: &MemoryHealthReport,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("entity", report.entity.clone())?;
    d.set_item("generated_at", report.generated_at.to_rfc3339())?;
    d.set_item("predicate_filter", report.predicate_filter.clone())?;
    d.set_item("total_fact_count", report.total_fact_count)?;
    d.set_item("active_fact_count", report.active_fact_count)?;

    let mut low_confidence_facts = Vec::with_capacity(report.low_confidence_facts.len());
    for fact in &report.low_confidence_facts {
        low_confidence_facts.push(fact_to_dict(py, fact)?.unbind());
    }
    d.set_item("low_confidence_facts", low_confidence_facts)?;

    let mut stale_high_impact_facts = Vec::with_capacity(report.stale_high_impact_facts.len());
    for fact in &report.stale_high_impact_facts {
        stale_high_impact_facts.push(fact_to_dict(py, fact)?.unbind());
    }
    d.set_item("stale_high_impact_facts", stale_high_impact_facts)?;

    d.set_item("contradiction_count", report.contradiction_count)?;
    d.set_item("recommended_actions", report.recommended_actions.clone())?;
    Ok(d)
}

#[cfg(feature = "hybrid")]
fn parse_temporal_intent(value: Option<&str>) -> PyResult<TemporalIntent> {
    let raw = value.unwrap_or("timeless");
    let intent = match raw {
        "timeless" => TemporalIntent::Timeless,
        "current_state" => TemporalIntent::CurrentState,
        "historical_point" => TemporalIntent::HistoricalPoint,
        "historical_interval" => TemporalIntent::HistoricalInterval,
        _ => return Err(PyValueError::new_err("invalid temporal_intent")),
    };
    Ok(intent)
}

#[cfg(feature = "hybrid")]
fn parse_temporal_operator(value: Option<&str>) -> PyResult<TemporalOperator> {
    let raw = value.unwrap_or("current");
    let operator = match raw {
        "current" => TemporalOperator::Current,
        "as_of" => TemporalOperator::AsOf,
        "during" => TemporalOperator::During,
        "before" => TemporalOperator::Before,
        "by" => TemporalOperator::By,
        "after" => TemporalOperator::After,
        "unknown" => TemporalOperator::Unknown,
        _ => return Err(PyValueError::new_err("invalid temporal_operator")),
    };
    Ok(operator)
}

#[pyclass(name = "KronroeDb")]
struct PyKronroeDb {
    inner: TemporalGraph,
}

#[pymethods]
impl PyKronroeDb {
    #[classmethod]
    fn open(_cls: &Bound<'_, PyType>, path: &str) -> PyResult<Self> {
        let path = path.to_owned();
        let inner = Python::with_gil(|py| py.allow_threads(|| TemporalGraph::open(&path)))
            .map_err(to_py_err)?;
        Ok(Self { inner })
    }

    #[classmethod]
    fn open_in_memory(_cls: &Bound<'_, PyType>) -> PyResult<Self> {
        let inner = TemporalGraph::open_in_memory().map_err(to_py_err)?;
        Ok(Self { inner })
    }

    fn assert_fact(
        &self,
        py: Python<'_>,
        subject: &str,
        predicate: &str,
        object: &Bound<'_, PyAny>,
    ) -> PyResult<String> {
        let value = py_to_value(object)?;
        let subject = subject.to_owned();
        let predicate = predicate.to_owned();
        let id = py
            .allow_threads(|| {
                self.inner
                    .assert_fact(&subject, &predicate, value, Utc::now())
            })
            .map_err(to_py_err)?;
        Ok(id.to_string())
    }

    fn search(&self, py: Python<'_>, query: &str, limit: usize) -> PyResult<Vec<Py<PyDict>>> {
        let query = query.to_owned();
        let facts = py
            .allow_threads(|| self.inner.search(&query, limit))
            .map_err(to_py_err)?;
        facts_to_pylist(py, facts)
    }
}

#[pyclass(name = "AgentMemory")]
struct PyAgentMemory {
    inner: AgentMemory,
}

impl PyAgentMemory {
    fn recall_scored_impl(
        &self,
        py: Python<'_>,
        query: &str,
        args: RecallScoredArgs,
    ) -> PyResult<Vec<Py<PyDict>>> {
        let query_owned = query.to_owned();
        let query_embedding = parse_query_embedding(args.query_embedding)?;

        #[cfg(feature = "hybrid")]
        let has_embedding = query_embedding.is_some();
        #[cfg(not(feature = "hybrid"))]
        if query_embedding.is_some() {
            return Err(PyRuntimeError::new_err(
                "hybrid/temporal controls are unavailable without hybrid feature",
            ));
        }
        #[cfg(not(feature = "hybrid"))]
        if args.use_hybrid {
            return Err(PyRuntimeError::new_err(
                "use_hybrid requires the 'hybrid' feature",
            ));
        }

        let mut opts = RecallOptions::new(&query_owned).with_limit(args.limit);

        if let Some(embedding) = query_embedding.as_deref() {
            opts = opts.with_embedding(embedding);
        }

        if args.confidence_filter_mode.is_some() && args.min_confidence.is_none() {
            return Err(PyValueError::new_err(
                "confidence_filter_mode requires min_confidence",
            ));
        }

        #[cfg(feature = "hybrid")]
        {
            if has_embedding {
                if args.use_hybrid {
                    opts = opts.with_hybrid(true);
                }
            } else if args.use_hybrid
                || args.temporal_intent.is_some()
                || args.temporal_operator.is_some()
            {
                return Err(PyRuntimeError::new_err(
                    "query_embedding is required for hybrid/temporal controls",
                ));
            }
        }

        if let Some(min) = args.min_confidence {
            if !min.is_finite() {
                return Err(PyValueError::new_err("min_confidence must be finite"));
            }
            match args.confidence_filter_mode.as_deref().unwrap_or("base") {
                "base" => {
                    opts = opts.with_min_confidence(min as f32);
                }
                "effective" => {
                    #[cfg(feature = "uncertainty")]
                    {
                        opts = opts.with_min_effective_confidence(min as f32);
                    }
                    #[cfg(not(feature = "uncertainty"))]
                    {
                        return Err(PyRuntimeError::new_err(
                            "effective confidence mode requires uncertainty feature",
                        ));
                    }
                }
                _ => {
                    return Err(PyValueError::new_err(
                        "confidence_filter_mode must be 'base' or 'effective'",
                    ));
                }
            }
        }

        if let Some(max_scored_rows) = args.max_scored_rows {
            opts = opts.with_max_scored_rows(max_scored_rows);
        }

        #[cfg(feature = "hybrid")]
        if let Some(intent) = args.temporal_intent.as_deref() {
            opts = opts.with_temporal_intent(parse_temporal_intent(Some(intent))?);
        }

        #[cfg(feature = "hybrid")]
        if let Some(operator) = args.temporal_operator.as_deref() {
            opts = opts.with_temporal_operator(parse_temporal_operator(Some(operator))?);
        }

        #[cfg(not(feature = "hybrid"))]
        if args.temporal_intent.is_some() || args.temporal_operator.is_some() || args.use_hybrid {
            return Err(PyRuntimeError::new_err(
                "hybrid/temporal controls are unavailable without hybrid feature",
            ));
        }

        let scored = py
            .allow_threads(|| self.inner.recall_scored_with_options(&opts))
            .map_err(to_py_err)?;

        let mut out = Vec::with_capacity(scored.len());
        for (fact, score) in scored {
            let row = PyDict::new(py);
            row.set_item("fact", fact_to_dict(py, &fact)?)?;
            row.set_item("score", recall_score_to_dict(py, &score)?)?;
            out.push(row.unbind());
        }
        Ok(out)
    }
}

#[pymethods]
impl PyAgentMemory {
    #[classmethod]
    fn open(_cls: &Bound<'_, PyType>, path: &str) -> PyResult<Self> {
        let path = path.to_owned();
        let inner = Python::with_gil(|py| py.allow_threads(|| AgentMemory::open(&path)))
            .map_err(to_py_err)?;
        Ok(Self { inner })
    }

    #[classmethod]
    fn open_in_memory(_cls: &Bound<'_, PyType>) -> PyResult<Self> {
        let inner = AgentMemory::open_in_memory().map_err(to_py_err)?;
        Ok(Self { inner })
    }

    #[pyo3(signature = (subject, predicate, object))]
    fn assert_fact(
        &self,
        py: Python<'_>,
        subject: &str,
        predicate: &str,
        object: &Bound<'_, PyAny>,
    ) -> PyResult<String> {
        let value = py_to_value(object)?;
        let subject = subject.to_owned();
        let predicate = predicate.to_owned();
        let id = py
            .allow_threads(|| self.inner.assert(&subject, &predicate, value))
            .map_err(to_py_err)?;
        Ok(id.to_string())
    }

    #[pyo3(signature = (subject, predicate, object, confidence, source=None))]
    fn assert_with_confidence(
        &self,
        py: Python<'_>,
        subject: &str,
        predicate: &str,
        object: &Bound<'_, PyAny>,
        confidence: f64,
        source: Option<&str>,
    ) -> PyResult<String> {
        if !confidence.is_finite() {
            return Err(PyValueError::new_err("confidence must be finite"));
        }
        let value = py_to_value(object)?;
        let subject = subject.to_owned();
        let predicate = predicate.to_owned();
        let confidence = confidence as f32;

        let id = if let Some(source) = source {
            py.allow_threads(|| {
                self.inner
                    .assert_with_source(&subject, &predicate, value, confidence, source)
            })
            .map_err(to_py_err)?
        } else {
            py.allow_threads(|| {
                self.inner
                    .assert_with_confidence(&subject, &predicate, value, confidence)
            })
            .map_err(to_py_err)?
        };
        Ok(id.to_string())
    }

    fn facts_about(&self, py: Python<'_>, entity: &str) -> PyResult<Vec<Py<PyDict>>> {
        let entity = entity.to_owned();
        let facts = py
            .allow_threads(|| self.inner.facts_about(&entity))
            .map_err(to_py_err)?;
        facts_to_pylist(py, facts)
    }

    fn search(&self, py: Python<'_>, query: &str, limit: usize) -> PyResult<Vec<Py<PyDict>>> {
        let query = query.to_owned();
        let facts = py
            .allow_threads(|| self.inner.search(&query, limit))
            .map_err(to_py_err)?;
        facts_to_pylist(py, facts)
    }

    #[pyo3(signature = (query, query_embedding=None, limit=10))]
    fn recall(
        &self,
        py: Python<'_>,
        query: &str,
        query_embedding: Option<Vec<f64>>,
        limit: usize,
    ) -> PyResult<Vec<Py<PyDict>>> {
        let query = query.to_owned();
        let embedding = parse_query_embedding(query_embedding)?;
        let query_embedding = embedding.as_deref();

        #[cfg(not(feature = "hybrid"))]
        if query_embedding.is_some() {
            return Err(PyRuntimeError::new_err(
                "query_embedding is unavailable without hybrid feature",
            ));
        }

        let facts = py
            .allow_threads(|| self.inner.recall(&query, query_embedding, limit))
            .map_err(to_py_err)?;
        facts_to_pylist(py, facts)
    }

    #[pyo3(signature = (query, limit=10, query_embedding=None, min_confidence=None, confidence_filter_mode=None, max_scored_rows=None, use_hybrid=false, temporal_intent=None, temporal_operator=None))]
    #[allow(clippy::too_many_arguments)]
    fn recall_scored(
        &self,
        py: Python<'_>,
        query: &str,
        limit: usize,
        query_embedding: Option<Vec<f64>>,
        min_confidence: Option<f64>,
        confidence_filter_mode: Option<String>,
        max_scored_rows: Option<usize>,
        use_hybrid: bool,
        temporal_intent: Option<String>,
        temporal_operator: Option<String>,
    ) -> PyResult<Vec<Py<PyDict>>> {
        let args = RecallScoredArgs {
            limit,
            query_embedding,
            min_confidence,
            confidence_filter_mode,
            max_scored_rows,
            use_hybrid,
            temporal_intent,
            temporal_operator,
        };
        self.recall_scored_impl(py, query, args)
    }

    #[pyo3(signature = (query, options=None))]
    fn recall_scored_with_options(
        &self,
        py: Python<'_>,
        query: &str,
        options: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Vec<Py<PyDict>>> {
        let args = parse_recall_scored_args_from_options(options)?;
        self.recall_scored_impl(py, query, args)
    }

    #[pyo3(signature = (query, max_tokens, query_embedding=None))]
    fn assemble_context(
        &self,
        py: Python<'_>,
        query: &str,
        max_tokens: usize,
        query_embedding: Option<Vec<f64>>,
    ) -> PyResult<String> {
        if max_tokens == 0 {
            return Err(PyValueError::new_err("max_tokens must be >= 1"));
        }

        let query_owned = query.to_owned();
        let embedding = parse_query_embedding(query_embedding)?;
        let query_embedding = embedding.as_deref();

        #[cfg(not(feature = "hybrid"))]
        if query_embedding.is_some() {
            return Err(PyRuntimeError::new_err(
                "query_embedding is unavailable without hybrid feature",
            ));
        }

        py.allow_threads(|| {
            self.inner
                .assemble_context(&query_owned, query_embedding, max_tokens)
        })
        .map_err(to_py_err)
    }

    #[pyo3(signature = (entity, since, predicate=None))]
    fn what_changed(
        &self,
        py: Python<'_>,
        entity: &str,
        since: &str,
        predicate: Option<&str>,
    ) -> PyResult<Py<PyDict>> {
        let since = since
            .parse::<DateTime<Utc>>()
            .map_err(|_| PyValueError::new_err("since must be RFC3339"))?;
        let entity = entity.to_owned();
        let predicate = predicate.map(str::to_owned);
        let report = py
            .allow_threads(|| {
                self.inner
                    .what_changed(&entity, since, predicate.as_deref())
            })
            .map_err(to_py_err)?;
        Ok(what_changed_report_to_dict(py, &report)?.unbind())
    }

    #[pyo3(signature = (entity, predicate=None, low_confidence_threshold=0.7, stale_after_days=90))]
    fn memory_health(
        &self,
        py: Python<'_>,
        entity: &str,
        predicate: Option<&str>,
        low_confidence_threshold: f64,
        stale_after_days: i64,
    ) -> PyResult<Py<PyDict>> {
        if !low_confidence_threshold.is_finite() {
            return Err(PyValueError::new_err(
                "low_confidence_threshold must be finite",
            ));
        }
        if !(0.0..=1.0).contains(&low_confidence_threshold) {
            return Err(PyValueError::new_err(
                "low_confidence_threshold must be between 0.0 and 1.0",
            ));
        }
        if stale_after_days < 0 {
            return Err(PyValueError::new_err("stale_after_days must be >= 0"));
        }

        let entity = entity.to_owned();
        let predicate = predicate.map(str::to_owned);
        let threshold = low_confidence_threshold as f32;
        let report = py
            .allow_threads(|| {
                self.inner
                    .memory_health(&entity, predicate.as_deref(), threshold, stale_after_days)
            })
            .map_err(to_py_err)?;
        Ok(memory_health_report_to_dict(py, &report)?.unbind())
    }

    #[pyo3(signature = (task, subject=None, now=None, horizon_days=None, limit=8, query_embedding=None, use_hybrid=false))]
    #[allow(clippy::too_many_arguments)]
    fn recall_for_task(
        &self,
        py: Python<'_>,
        task: &str,
        subject: Option<&str>,
        now: Option<&str>,
        horizon_days: Option<i64>,
        limit: usize,
        query_embedding: Option<Vec<f64>>,
        use_hybrid: bool,
    ) -> PyResult<Py<PyDict>> {
        if limit == 0 {
            return Err(PyValueError::new_err("limit must be >= 1"));
        }
        if horizon_days.is_some_and(|days| days < 1) {
            return Err(PyValueError::new_err("horizon_days must be >= 1"));
        }

        let now = now
            .map(|value| {
                value
                    .parse::<DateTime<Utc>>()
                    .map_err(|_| PyValueError::new_err("now must be RFC3339"))
            })
            .transpose()?;

        let embedding = parse_query_embedding(query_embedding)?;
        #[cfg(not(feature = "hybrid"))]
        if embedding.is_some() || use_hybrid {
            return Err(PyRuntimeError::new_err(
                "query_embedding/use_hybrid require the hybrid feature",
            ));
        }

        #[cfg(feature = "hybrid")]
        let embedding_for_call = if use_hybrid {
            embedding.as_deref()
        } else {
            None
        };
        #[cfg(not(feature = "hybrid"))]
        let embedding_for_call: Option<&[f32]> = None;

        let task = task.to_owned();
        let subject = subject.map(str::to_owned);
        let report = py
            .allow_threads(|| {
                self.inner.recall_for_task(
                    &task,
                    subject.as_deref(),
                    now,
                    horizon_days,
                    limit,
                    embedding_for_call,
                )
            })
            .map_err(to_py_err)?;

        Ok(recall_for_task_report_to_dict(py, &report)?.unbind())
    }

    #[pyo3(signature = (entity, predicate, at))]
    fn facts_about_at(
        &self,
        py: Python<'_>,
        entity: &str,
        predicate: &str,
        at: &str,
    ) -> PyResult<Vec<Py<PyDict>>> {
        let at = at
            .parse()
            .map_err(|_| PyValueError::new_err("invalid RFC3339 datetime"))?;
        let entity = entity.to_owned();
        let predicate = predicate.to_owned();
        let facts = py
            .allow_threads(|| self.inner.facts_about_at(&entity, &predicate, at))
            .map_err(to_py_err)?;
        facts_to_pylist(py, facts)
    }

    #[pyo3(signature = (fact_id, new_value))]
    fn correct_fact(
        &self,
        py: Python<'_>,
        fact_id: &str,
        new_value: &Bound<'_, PyAny>,
    ) -> PyResult<String> {
        let new_value = py_to_value(new_value)?;
        let id = py
            .allow_threads(|| self.inner.correct_fact(fact_id, new_value))
            .map_err(to_py_err)?;
        Ok(id.to_string())
    }

    #[pyo3(signature = (fact_id))]
    fn invalidate_fact(&self, py: Python<'_>, fact_id: &str) -> PyResult<()> {
        py.allow_threads(|| self.inner.invalidate_fact(fact_id))
            .map_err(to_py_err)?;
        Ok(())
    }
}

#[pymodule]
fn kronroe(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyKronroeDb>()?;
    m.add_class::<PyAgentMemory>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn py_agent_memory_surface_compile_check() {
        // Runtime behavior for these flows is validated in agent-memory + MCP
        // integration suites. This crate's unit tests stay compile-only because
        // extension-module builds don't link an embedded Python interpreter.
        let _ = stringify!(super::PyAgentMemory::assert_fact);
        let _ = stringify!(super::PyAgentMemory::assert_with_confidence);
        let _ = stringify!(super::PyAgentMemory::facts_about);
        let _ = stringify!(super::PyAgentMemory::recall);
        let _ = stringify!(super::PyAgentMemory::recall_scored);
        let _ = stringify!(super::PyAgentMemory::recall_scored_with_options);
        let _ = stringify!(super::PyAgentMemory::assemble_context);
        let _ = stringify!(super::PyAgentMemory::what_changed);
        let _ = stringify!(super::PyAgentMemory::memory_health);
        let _ = stringify!(super::PyAgentMemory::recall_for_task);
        let _ = stringify!(super::PyAgentMemory::correct_fact);
        let _ = stringify!(super::PyAgentMemory::invalidate_fact);
    }

    #[test]
    fn py_kronroe_db_surface_compile_check() {
        let _ = stringify!(super::PyKronroeDb::assert_fact);
        let _ = stringify!(super::PyKronroeDb::search);
    }

    #[cfg(not(feature = "extension-module"))]
    mod runtime {
        use super::super::{AgentMemory, PyAgentMemory};
        use kronroe_agent_memory::AssertParams;
        use pyo3::prelude::PyAnyMethods;
        use pyo3::types::PyString;
        use pyo3::types::{PyDict, PyDictMethods};
        use pyo3::Bound;
        use pyo3::Python;
        use serde_json::Value as JsonValue;
        use std::sync::Once;
        use tempfile::tempdir;

        fn ensure_python_ready() {
            static START: Once = Once::new();
            START.call_once(pyo3::prepare_freethreaded_python);
        }

        fn with_memory<F>(f: F)
        where
            F: FnOnce(Python<'_>, &PyAgentMemory),
        {
            ensure_python_ready();
            let dir = tempdir().expect("tempdir");
            let path = dir.path().join("memory.kronroe");
            let path = path.to_string_lossy().to_string();
            let memory = PyAgentMemory {
                inner: AgentMemory::open(&path).expect("open memory"),
            };
            Python::with_gil(|py| f(py, &memory));
        }

        fn stable_contract_fixture() -> JsonValue {
            serde_json::from_str(include_str!("../../../contracts/stable-agent-memory.json"))
                .expect("stable contract fixture should parse")
        }

        fn fixture_strings(value: &JsonValue) -> Vec<String> {
            value
                .as_array()
                .expect("fixture value should be an array")
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .expect("fixture array entry should be a string")
                        .to_string()
                })
                .collect()
        }

        fn assert_dict_has_keys(dict: &Bound<'_, PyDict>, keys: &[String]) {
            for key in keys {
                assert!(dict
                    .get_item(key)
                    .expect("dict access should succeed")
                    .is_some());
            }
        }

        #[test]
        fn python_assert_with_confidence_and_source_round_trip() {
            with_memory(|py, memory| {
                let object = PyString::new(py, "Acme").into_any();
                memory
                    .assert_with_confidence(
                        py,
                        "alice",
                        "works_at",
                        &object,
                        0.9,
                        Some("user:tests"),
                    )
                    .expect("assert_with_confidence");

                let facts = memory.facts_about(py, "alice").expect("facts_about");
                assert_eq!(facts.len(), 1);
                let first = facts[0].bind(py);

                let confidence = first
                    .get_item("confidence")
                    .expect("confidence key")
                    .expect("confidence value")
                    .extract::<f64>()
                    .expect("confidence as f64");
                assert!((confidence - 0.9).abs() < 1e-6);

                let source = first
                    .get_item("source")
                    .expect("source key")
                    .expect("source value")
                    .extract::<String>()
                    .expect("source as string");
                assert_eq!(source, "user:tests");
            });
        }

        #[test]
        fn python_recall_scored_filters_min_confidence() {
            with_memory(|py, memory| {
                let low = PyString::new(py, "rust rust rust rust").into_any();
                memory
                    .assert_with_confidence(py, "low", "memory", &low, 0.2, None)
                    .expect("assert low");
                let high = PyString::new(py, "rust").into_any();
                memory
                    .assert_with_confidence(py, "high", "memory", &high, 0.95, None)
                    .expect("assert high");

                let rows = memory
                    .recall_scored(
                        py,
                        "rust",
                        10,
                        None,
                        Some(0.9),
                        Some("base".to_string()),
                        None,
                        false,
                        None,
                        None,
                    )
                    .expect("recall_scored");
                assert_eq!(rows.len(), 1);

                let row = rows[0].bind(py);
                let fact = row
                    .get_item("fact")
                    .expect("fact key")
                    .expect("fact value")
                    .downcast_into::<PyDict>()
                    .expect("fact dict");
                let subject = fact
                    .get_item("subject")
                    .expect("subject key")
                    .expect("subject value")
                    .extract::<String>()
                    .expect("subject as string");
                assert_eq!(subject, "high");
            });
        }

        #[test]
        fn python_recall_scored_returns_contract_shape() {
            with_memory(|py, memory| {
                let object = PyString::new(py, "rust memory").into_any();
                memory
                    .assert_with_confidence(py, "shape", "memory", &object, 0.9, None)
                    .expect("assert_with_confidence");

                let rows = memory
                    .recall_scored(
                        py,
                        "rust",
                        10,
                        None,
                        Some(0.1),
                        Some("base".to_string()),
                        None,
                        false,
                        None,
                        None,
                    )
                    .expect("recall_scored");
                assert!(!rows.is_empty());

                let row = rows[0].bind(py);
                let score = row
                    .get_item("score")
                    .expect("score key")
                    .expect("score value")
                    .downcast_into::<PyDict>()
                    .expect("score dict");
                let score_type = score
                    .get_item("type")
                    .expect("type key")
                    .expect("type value")
                    .extract::<String>()
                    .expect("type as string");
                assert_eq!(score_type, "text");
                assert!(score
                    .get_item("confidence")
                    .expect("confidence key")
                    .is_some());
                assert!(score
                    .get_item("effective_confidence")
                    .expect("effective_confidence key")
                    .is_some());
            });
        }

        #[test]
        fn python_recall_scored_with_options_matches_main_path() {
            with_memory(|py, memory| {
                let object = PyString::new(py, "rust options path").into_any();
                memory
                    .assert_with_confidence(py, "options-shape", "memory", &object, 0.9, None)
                    .expect("assert_with_confidence");

                let main_rows = memory
                    .recall_scored(
                        py,
                        "rust",
                        10,
                        None,
                        Some(0.1),
                        Some("base".to_string()),
                        Some(128),
                        false,
                        None,
                        None,
                    )
                    .expect("main recall_scored");

                let options = PyDict::new(py);
                options.set_item("limit", 10).expect("set limit");
                options
                    .set_item("min_confidence", 0.1)
                    .expect("set min_confidence");
                options
                    .set_item("confidence_filter_mode", "base")
                    .expect("set confidence_filter_mode");
                options
                    .set_item("max_scored_rows", 128)
                    .expect("set max_scored_rows");

                let options_rows = memory
                    .recall_scored_with_options(py, "rust", Some(&options))
                    .expect("recall_scored_with_options");

                assert_eq!(main_rows.len(), options_rows.len());
                let main_row = main_rows[0].bind(py);
                let options_row = options_rows[0].bind(py);

                let main_fact = main_row
                    .get_item("fact")
                    .expect("main fact key")
                    .expect("main fact value")
                    .downcast_into::<PyDict>()
                    .expect("main fact dict");
                let options_fact = options_row
                    .get_item("fact")
                    .expect("options fact key")
                    .expect("options fact value")
                    .downcast_into::<PyDict>()
                    .expect("options fact dict");

                let main_id = main_fact
                    .get_item("id")
                    .expect("main id key")
                    .expect("main id value")
                    .extract::<String>()
                    .expect("main id string");
                let options_id = options_fact
                    .get_item("id")
                    .expect("options id key")
                    .expect("options id value")
                    .extract::<String>()
                    .expect("options id string");
                assert_eq!(main_id, options_id);
            });
        }

        #[test]
        fn python_invalidate_fact_removes_recall_hit() {
            with_memory(|py, memory| {
                let object = PyString::new(py, "Acme").into_any();
                let fact_id = memory
                    .assert_fact(py, "alice", "works_at", &object)
                    .expect("assert_fact");

                let before = memory.recall(py, "Acme", None, 10).expect("recall before");
                assert_eq!(before.len(), 1);

                memory
                    .invalidate_fact(py, &fact_id)
                    .expect("invalidate_fact");

                let after = memory.recall(py, "Acme", None, 10).expect("recall after");
                assert_eq!(after.len(), 0);
            });
        }

        #[test]
        fn python_recall_scored_rejects_non_finite_min_confidence() {
            with_memory(|py, memory| {
                let err = memory
                    .recall_scored(
                        py,
                        "rust",
                        10,
                        None,
                        Some(f64::NAN),
                        Some("base".to_string()),
                        None,
                        false,
                        None,
                        None,
                    )
                    .expect_err("expected non-finite min_confidence error");

                assert!(err.to_string().contains("min_confidence must be finite"));
            });
        }

        #[test]
        fn python_recall_scored_requires_threshold_for_confidence_mode() {
            with_memory(|py, memory| {
                let err = memory
                    .recall_scored(
                        py,
                        "rust",
                        10,
                        None,
                        None,
                        Some("base".to_string()),
                        None,
                        false,
                        None,
                        None,
                    )
                    .expect_err("expected confidence mode contract error");
                assert!(err
                    .to_string()
                    .contains("confidence_filter_mode requires min_confidence"));
            });
        }

        #[test]
        fn python_recall_rejects_empty_embedding() {
            with_memory(|py, memory| {
                let err = memory
                    .recall(py, "rust", Some(vec![]), 10)
                    .expect_err("expected empty embedding error");
                assert!(err
                    .to_string()
                    .contains("query_embedding must not be empty"));
            });
        }

        #[test]
        fn python_recall_rejects_embedding_overflow() {
            with_memory(|py, memory| {
                let err = memory
                    .recall(py, "rust", Some(vec![1.0e40]), 10)
                    .expect_err("expected f32 overflow error");
                assert!(err.to_string().contains("overflow f32 range"));
            });
        }

        #[test]
        fn python_recall_for_task_returns_subject_scoped_report() {
            with_memory(|py, memory| {
                let stale = (::chrono::Utc::now() - ::chrono::Duration::days(180)).to_rfc3339();
                let object = PyString::new(py, "Acme").into_any();
                memory
                    .assert_with_confidence(py, "alice", "works_at", &object, 0.6, None)
                    .expect("assert_with_confidence");

                let rows = memory.facts_about(py, "alice").expect("facts_about");
                let row = rows[0].bind(py);
                let fact_id = row
                    .get_item("id")
                    .expect("id key")
                    .expect("id value")
                    .extract::<String>()
                    .expect("id string");
                memory
                    .invalidate_fact(py, &fact_id)
                    .expect("invalidate old row");

                let refreshed = PyString::new(py, "Acme").into_any();
                memory
                    .assert_with_confidence(py, "alice", "works_at", &refreshed, 0.6, None)
                    .expect("assert refreshed");

                let report_obj = memory
                    .recall_for_task(
                        py,
                        "prepare renewal call",
                        Some("alice"),
                        Some(&stale),
                        Some(90),
                        10,
                        None,
                        false,
                    )
                    .expect("recall_for_task");
                let report_any = report_obj.bind(py);
                let report = report_any.downcast::<PyDict>().expect("report dict");

                let subject = report
                    .get_item("subject")
                    .expect("subject key")
                    .expect("subject value")
                    .extract::<String>()
                    .expect("subject string");
                assert_eq!(subject, "alice");

                let key_facts = report
                    .get_item("key_facts")
                    .expect("key_facts key")
                    .expect("key_facts value")
                    .extract::<Vec<pyo3::Py<PyDict>>>()
                    .expect("key_facts list");
                assert!(!key_facts.is_empty());
            });
        }

        #[test]
        fn python_what_changed_returns_corrections_and_confidence_shifts() {
            with_memory(|py, memory| {
                let original = PyString::new(py, "Acme").into_any();
                memory
                    .assert_fact(py, "alice", "works_at", &original)
                    .expect("assert_fact");

                let rows = memory.facts_about(py, "alice").expect("facts_about");
                let row = rows[0].bind(py);
                let fact_id = row
                    .get_item("id")
                    .expect("id key")
                    .expect("id value")
                    .extract::<String>()
                    .expect("id string");

                let since = ::chrono::Utc::now().to_rfc3339();
                memory
                    .invalidate_fact(py, &fact_id)
                    .expect("invalidate_fact");

                let replacement = PyString::new(py, "Beta Corp").into_any();
                memory
                    .assert_with_confidence(py, "alice", "works_at", &replacement, 0.6, None)
                    .expect("assert_with_confidence");

                let report_obj = memory
                    .what_changed(py, "alice", &since, Some("works_at"))
                    .expect("what_changed");
                let report_any = report_obj.bind(py);
                let report = report_any.downcast::<PyDict>().expect("report dict");

                let new_facts = report
                    .get_item("new_facts")
                    .expect("new_facts key")
                    .expect("new_facts value")
                    .extract::<Vec<pyo3::Py<PyDict>>>()
                    .expect("new_facts list");
                let invalidated = report
                    .get_item("invalidated_facts")
                    .expect("invalidated key")
                    .expect("invalidated value")
                    .extract::<Vec<pyo3::Py<PyDict>>>()
                    .expect("invalidated list");
                let corrections = report
                    .get_item("corrections")
                    .expect("corrections key")
                    .expect("corrections value")
                    .extract::<Vec<pyo3::Py<PyDict>>>()
                    .expect("corrections list");
                let shifts = report
                    .get_item("confidence_shifts")
                    .expect("confidence_shifts key")
                    .expect("confidence_shifts value")
                    .extract::<Vec<pyo3::Py<PyDict>>>()
                    .expect("confidence_shifts list");

                assert_eq!(new_facts.len(), 1);
                assert_eq!(invalidated.len(), 1);
                assert_eq!(corrections.len(), 1);
                assert_eq!(shifts.len(), 1);
            });
        }

        #[test]
        fn python_what_changed_rejects_invalid_since() {
            with_memory(|py, memory| {
                let err = memory
                    .what_changed(py, "alice", "not-a-date", None)
                    .expect_err("expected invalid since error");
                assert!(err.to_string().contains("since must be RFC3339"));
            });
        }

        #[test]
        fn python_memory_health_reports_low_confidence_and_stale() {
            with_memory(|py, memory| {
                let old = ::chrono::Utc::now() - ::chrono::Duration::days(200);
                memory
                    .inner
                    .assert_with_confidence_with_params(
                        "alice",
                        "nickname",
                        "Bex",
                        AssertParams { valid_from: old },
                        0.4,
                    )
                    .expect("assert nickname");
                memory
                    .inner
                    .assert_with_confidence_with_params(
                        "alice",
                        "email",
                        "alice@example.com",
                        AssertParams { valid_from: old },
                        0.9,
                    )
                    .expect("assert email");

                let report_obj = memory
                    .memory_health(py, "alice", None, 0.7, 90)
                    .expect("memory_health");
                let report_any = report_obj.bind(py);
                let report = report_any.downcast::<PyDict>().expect("report dict");

                assert_eq!(
                    report
                        .get_item("total_fact_count")
                        .expect("total_fact_count key")
                        .expect("total_fact_count value")
                        .extract::<usize>()
                        .expect("total_fact_count int"),
                    2
                );

                let low_confidence = report
                    .get_item("low_confidence_facts")
                    .expect("low_confidence_facts key")
                    .expect("low_confidence_facts value")
                    .extract::<Vec<pyo3::Py<PyDict>>>()
                    .expect("low_confidence_facts list");
                let stale = report
                    .get_item("stale_high_impact_facts")
                    .expect("stale_high_impact_facts key")
                    .expect("stale_high_impact_facts value")
                    .extract::<Vec<pyo3::Py<PyDict>>>()
                    .expect("stale_high_impact_facts list");

                assert_eq!(low_confidence.len(), 1);
                assert_eq!(stale.len(), 1);
            });
        }

        #[test]
        fn python_memory_health_rejects_invalid_threshold() {
            with_memory(|py, memory| {
                let err = memory
                    .memory_health(py, "alice", None, f64::NAN, 90)
                    .expect_err("expected invalid threshold error");
                assert!(err
                    .to_string()
                    .contains("low_confidence_threshold must be finite"));

                let err = memory
                    .memory_health(py, "alice", None, 1.5, 90)
                    .expect_err("expected out-of-range threshold error");
                assert!(err.to_string().contains("between 0.0 and 1.0"));
            });
        }

        #[test]
        fn python_stable_contract_fact_id_fields_match_fixture() {
            let fixture = stable_contract_fixture();
            let fact_prefix = fixture["methods"]["assert_fact"]["fact_id_prefix"]
                .as_str()
                .expect("fact prefix");

            with_memory(|py, memory| {
                let object = PyString::new(py, "Acme").into_any();
                let fact_id = memory
                    .assert_fact(py, "alice", "works_at", &object)
                    .expect("assert_fact");
                assert!(fact_id.starts_with(fact_prefix));

                let new_fact_id = memory
                    .correct_fact(py, &fact_id, &PyString::new(py, "Globex").into_any())
                    .expect("correct_fact");
                assert!(new_fact_id.starts_with(fact_prefix));
                assert_ne!(new_fact_id, fact_id);

                memory
                    .invalidate_fact(py, &new_fact_id)
                    .expect("invalidate_fact");
                let after = memory.recall(py, "Globex", None, 10).expect("recall after");
                assert_eq!(after.len(), 0);
            });
        }

        #[test]
        fn python_stable_contract_recall_scored_matches_fixture() {
            let fixture = stable_contract_fixture();
            let required_row_keys =
                fixture_strings(&fixture["methods"]["recall_scored"]["required_row_keys"]);
            let fact_required_keys =
                fixture_strings(&fixture["methods"]["recall_scored"]["fact_required_keys"]);
            let score_required_keys =
                fixture_strings(&fixture["methods"]["recall_scored"]["score_required_keys"]);
            let allowed_score_types =
                fixture_strings(&fixture["methods"]["recall_scored"]["allowed_score_types"]);

            with_memory(|py, memory| {
                let object = PyString::new(py, "rust memory").into_any();
                memory
                    .assert_with_confidence(py, "shape", "memory", &object, 0.9, None)
                    .expect("assert_with_confidence");

                let rows = memory
                    .recall_scored(
                        py,
                        "rust",
                        10,
                        None,
                        Some(0.1),
                        Some("base".to_string()),
                        None,
                        false,
                        None,
                        None,
                    )
                    .expect("recall_scored");
                assert!(!rows.is_empty());

                let row = rows[0].bind(py).downcast::<PyDict>().expect("row dict");
                assert_dict_has_keys(&row, &required_row_keys);

                let fact = row
                    .get_item("fact")
                    .expect("fact key")
                    .expect("fact value")
                    .downcast_into::<PyDict>()
                    .expect("fact dict");
                let score = row
                    .get_item("score")
                    .expect("score key")
                    .expect("score value")
                    .downcast_into::<PyDict>()
                    .expect("score dict");
                assert_dict_has_keys(&fact, &fact_required_keys);
                assert_dict_has_keys(&score, &score_required_keys);

                let score_type = score
                    .get_item("type")
                    .expect("type key")
                    .expect("type value")
                    .extract::<String>()
                    .expect("type string");
                assert!(allowed_score_types
                    .iter()
                    .any(|allowed| allowed == &score_type));
            });
        }

        #[test]
        fn python_stable_contract_context_and_reports_match_fixture() {
            let fixture = stable_contract_fixture();
            let assemble_needles =
                fixture_strings(&fixture["methods"]["assemble_context"]["required_substrings"]);
            let what_changed_keys =
                fixture_strings(&fixture["methods"]["what_changed"]["required_report_keys"]);
            let what_changed_error = fixture["methods"]["what_changed"]["required_error_substring"]
                .as_str()
                .expect("what_changed error substring");
            let memory_health_keys =
                fixture_strings(&fixture["methods"]["memory_health"]["required_report_keys"]);

            with_memory(|py, memory| {
                let object = PyString::new(py, "Acme").into_any();
                let fact_id = memory
                    .assert_fact(py, "alice", "works_at", &object)
                    .expect("assert_fact");

                let context = memory
                    .assemble_context(py, "Where does alice work?", 64, None)
                    .expect("assemble_context")
                    .to_lowercase();
                for needle in &assemble_needles {
                    assert!(context.contains(&needle.to_lowercase()));
                }

                let since = ::chrono::Utc::now().to_rfc3339();
                memory
                    .invalidate_fact(py, &fact_id)
                    .expect("invalidate_fact");
                let replacement = PyString::new(py, "Beta Corp").into_any();
                memory
                    .assert_with_confidence(py, "alice", "works_at", &replacement, 0.6, None)
                    .expect("replacement assert");

                let changed = memory
                    .what_changed(py, "alice", &since, Some("works_at"))
                    .expect("what_changed");
                let changed = changed.bind(py).downcast::<PyDict>().expect("changed dict");
                assert_dict_has_keys(&changed, &what_changed_keys);

                let err = memory
                    .what_changed(py, "alice", "not-a-date", None)
                    .expect_err("invalid since should fail");
                assert!(err.to_string().contains(what_changed_error));

                let old = ::chrono::Utc::now() - ::chrono::Duration::days(200);
                memory
                    .inner
                    .assert_with_confidence_with_params(
                        "alice",
                        "nickname",
                        "Bex",
                        AssertParams { valid_from: old },
                        0.4,
                    )
                    .expect("assert nickname");
                let health = memory
                    .memory_health(py, "alice", None, 0.7, 90)
                    .expect("memory_health");
                let health = health.bind(py).downcast::<PyDict>().expect("health dict");
                assert_dict_has_keys(&health, &memory_health_keys);
            });
        }

        #[test]
        fn python_recall_for_task_rejects_zero_horizon_days() {
            with_memory(|py, memory| {
                let err = memory
                    .recall_for_task(
                        py,
                        "prepare renewal call",
                        None,
                        None,
                        Some(0),
                        8,
                        None,
                        false,
                    )
                    .expect_err("expected horizon validation error");
                assert!(err.to_string().contains("horizon_days must be >= 1"));
            });
        }

        #[cfg(feature = "hybrid")]
        #[test]
        fn python_recall_scored_requires_embedding_for_hybrid_controls() {
            with_memory(|py, memory| {
                let err = memory
                    .recall_scored(py, "rust", 10, None, None, None, None, true, None, None)
                    .expect_err("expected hybrid control validation error");
                assert!(err
                    .to_string()
                    .contains("query_embedding is required for hybrid/temporal controls"));
            });
        }
    }
}
