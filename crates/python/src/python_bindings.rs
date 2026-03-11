use ::chrono::Utc;
use kronroe_agent_memory::{AgentMemory, RecallOptions, RecallScore};
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

fn fact_to_dict<'py>(py: Python<'py>, fact: &Fact) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("id", fact.id.0.clone())?;
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
        Ok(id.0)
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

#[pymethods]
impl PyAgentMemory {
    #[classmethod]
    fn open(_cls: &Bound<'_, PyType>, path: &str) -> PyResult<Self> {
        let path = path.to_owned();
        let inner = Python::with_gil(|py| py.allow_threads(|| AgentMemory::open(&path)))
            .map_err(to_py_err)?;
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
        Ok(id.0)
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
        Ok(id.0)
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
        let embedding: Option<Vec<f32>> =
            query_embedding.map(|values| values.into_iter().map(|v| v as f32).collect());
        let query_embedding = embedding.as_deref();

        if query_embedding.is_some() {
            #[cfg(feature = "hybrid")]
            if query_embedding.is_some_and(|values| values.iter().any(|v| !v.is_finite())) {
                return Err(PyValueError::new_err(
                    "query_embedding must contain finite numbers",
                ));
            }

            #[cfg(not(feature = "hybrid"))]
            {
                return Err(PyRuntimeError::new_err(
                    "query_embedding is unavailable without hybrid feature",
                ));
            }
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
        let query_owned = query.to_owned();
        let query_embedding: Option<Vec<f32>> =
            query_embedding.map(|values| values.into_iter().map(|v| v as f32).collect());

        #[cfg(feature = "hybrid")]
        let has_embedding = query_embedding.is_some();
        #[cfg(feature = "hybrid")]
        if has_embedding {
            let values = query_embedding.as_deref().unwrap_or(&[]);
            if values.iter().any(|v| !v.is_finite()) {
                return Err(PyValueError::new_err(
                    "query_embedding must contain finite numbers",
                ));
            }
        }
        #[cfg(not(feature = "hybrid"))]
        if query_embedding.is_some() {
            return Err(PyRuntimeError::new_err(
                "hybrid/temporal controls are unavailable without hybrid feature",
            ));
        }
        #[cfg(not(feature = "hybrid"))]
        if use_hybrid {
            return Err(PyRuntimeError::new_err(
                "use_hybrid requires the 'hybrid' feature",
            ));
        }

        let mut opts = RecallOptions::new(&query_owned).with_limit(limit);

        if let Some(embedding) = query_embedding.as_deref() {
            opts = opts.with_embedding(embedding);
        }

        #[cfg(feature = "hybrid")]
        {
            if has_embedding {
                if use_hybrid {
                    opts = opts.with_hybrid(true);
                }
            } else if use_hybrid || temporal_intent.is_some() || temporal_operator.is_some() {
                return Err(PyRuntimeError::new_err(
                    "query_embedding is required for hybrid/temporal controls",
                ));
            }
        }

        if let Some(min) = min_confidence {
            if !min.is_finite() {
                return Err(PyValueError::new_err("min_confidence must be finite"));
            }
            match confidence_filter_mode.as_deref().unwrap_or("base") {
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

        if let Some(max_scored_rows) = max_scored_rows {
            opts = opts.with_max_scored_rows(max_scored_rows);
        }

        #[cfg(feature = "hybrid")]
        if let Some(intent) = temporal_intent.as_deref() {
            opts = opts.with_temporal_intent(parse_temporal_intent(Some(intent))?);
        }

        #[cfg(feature = "hybrid")]
        if let Some(operator) = temporal_operator.as_deref() {
            opts = opts.with_temporal_operator(parse_temporal_operator(Some(operator))?);
        }

        #[cfg(not(feature = "hybrid"))]
        if temporal_intent.is_some() || temporal_operator.is_some() || use_hybrid {
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
        let embedding: Option<Vec<f32>> =
            query_embedding.map(|values| values.into_iter().map(|v| v as f32).collect());
        let query_embedding = embedding.as_deref();

        if query_embedding.is_some() {
            #[cfg(feature = "hybrid")]
            if let Some(values) = query_embedding {
                if values.iter().any(|v| !v.is_finite()) {
                    return Err(PyValueError::new_err(
                        "query_embedding must contain finite numbers",
                    ));
                }
            }

            #[cfg(not(feature = "hybrid"))]
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
        let fact_id = kronroe_core::FactId(fact_id.to_string());
        let id = py
            .allow_threads(|| self.inner.correct_fact(&fact_id, new_value))
            .map_err(to_py_err)?;
        Ok(id.0)
    }

    #[pyo3(signature = (fact_id))]
    fn invalidate_fact(&self, py: Python<'_>, fact_id: &str) -> PyResult<()> {
        let fact_id = kronroe_core::FactId(fact_id.to_string());
        py.allow_threads(|| self.inner.invalidate_fact(&fact_id))
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
        let _ = stringify!(super::PyAgentMemory::assemble_context);
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
        use pyo3::prelude::PyAnyMethods;
        use pyo3::types::PyString;
        use pyo3::types::{PyDict, PyDictMethods};
        use pyo3::Python;
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
