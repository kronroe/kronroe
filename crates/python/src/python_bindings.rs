#![allow(clippy::useless_conversion)]

use ::chrono::Utc;
use ::kronroe::{Fact, TemporalGraph, Value};
use kronroe_agent_memory::AgentMemory;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyType};

fn to_py_err<E: std::fmt::Display>(err: E) -> PyErr {
    PyRuntimeError::new_err(err.to_string())
}

fn fact_to_dict<'py>(py: Python<'py>, fact: &Fact) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new_bound(py);
    d.set_item("id", fact.id.0.clone())?;
    d.set_item("subject", fact.subject.clone())?;
    d.set_item("predicate", fact.predicate.clone())?;
    d.set_item(
        "object",
        match &fact.object {
            Value::Text(v) | Value::Entity(v) => v.into_py(py),
            Value::Number(v) => v.into_py(py),
            Value::Boolean(v) => v.into_py(py),
        },
    )?;
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

#[pyclass(name = "KronroeDb")]
struct PyKronroeDb {
    inner: TemporalGraph,
}

#[pymethods]
impl PyKronroeDb {
    #[classmethod]
    fn open(_cls: &Bound<'_, PyType>, path: &str) -> PyResult<Self> {
        let inner = TemporalGraph::open(path).map_err(to_py_err)?;
        Ok(Self { inner })
    }

    fn assert_fact(&self, subject: &str, predicate: &str, object: &str) -> PyResult<String> {
        let id = self
            .inner
            .assert_fact(subject, predicate, object, Utc::now())
            .map_err(to_py_err)?;
        Ok(id.0)
    }

    fn search(&self, py: Python<'_>, query: &str, limit: usize) -> PyResult<Vec<Py<PyDict>>> {
        let facts = self.inner.search(query, limit).map_err(to_py_err)?;
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
        let inner = AgentMemory::open(path).map_err(to_py_err)?;
        Ok(Self { inner })
    }

    fn assert_fact(&self, subject: &str, predicate: &str, object: &str) -> PyResult<String> {
        let id = self
            .inner
            .assert(subject, predicate, object.to_string())
            .map_err(to_py_err)?;
        Ok(id.0)
    }

    fn facts_about(&self, py: Python<'_>, entity: &str) -> PyResult<Vec<Py<PyDict>>> {
        let facts = self.inner.facts_about(entity).map_err(to_py_err)?;
        facts_to_pylist(py, facts)
    }

    fn search(&self, py: Python<'_>, query: &str, limit: usize) -> PyResult<Vec<Py<PyDict>>> {
        let facts = self.inner.search(query, limit).map_err(to_py_err)?;
        facts_to_pylist(py, facts)
    }

    fn facts_about_at(
        &self,
        py: Python<'_>,
        entity: &str,
        predicate: &str,
        at_rfc3339: &str,
    ) -> PyResult<Vec<Py<PyDict>>> {
        let at = at_rfc3339
            .parse()
            .map_err(|_| PyValueError::new_err("invalid RFC3339 datetime"))?;
        let facts = self
            .inner
            .facts_about_at(entity, predicate, at)
            .map_err(to_py_err)?;
        facts_to_pylist(py, facts)
    }
}

#[pymodule]
fn kronroe(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyKronroeDb>()?;
    m.add_class::<PyAgentMemory>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
