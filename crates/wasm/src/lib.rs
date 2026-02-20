//! Kronroe WASM — browser-compatible temporal graph database.
//!
//! This crate wraps the core [`kronroe::TemporalGraph`] engine for use in
//! WebAssembly environments. It uses an in-memory storage backend (no file
//! I/O), making it suitable for browser-based demos and playgrounds.
//!
//! # Usage (JavaScript)
//!
//! ```js
//! import init, { WasmGraph } from 'kronroe-wasm';
//!
//! await init();
//! const graph = WasmGraph.open();
//! const factId = graph.assert_fact("alice", "works_at", "Acme");
//! const facts = graph.current_facts("alice", "works_at");
//! console.log(JSON.parse(facts));
//! ```

use chrono::{DateTime, Utc};
use kronroe::{TemporalGraph, Value};
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

/// Convert KronroeError to a JsValue for wasm-bindgen.
fn to_js_err(e: kronroe::KronroeError) -> JsValue {
    JsValue::from_str(&e.to_string())
}

// ---------------------------------------------------------------------------
// WasmGraph — the public API
// ---------------------------------------------------------------------------

/// An in-memory temporal property graph for browser environments.
///
/// All data lives in memory and is lost when the instance is dropped.
/// This is designed for demos, playgrounds, and ephemeral workloads.
#[wasm_bindgen]
pub struct WasmGraph {
    inner: TemporalGraph,
}

#[wasm_bindgen]
impl WasmGraph {
    /// Create a new in-memory temporal graph.
    #[wasm_bindgen(constructor)]
    pub fn open() -> Result<WasmGraph, JsValue> {
        let inner = TemporalGraph::open_in_memory().map_err(to_js_err)?;
        Ok(WasmGraph { inner })
    }

    /// Assert a new fact and return its ID.
    ///
    /// The object is stored as a text value. For typed values (number,
    /// boolean, entity reference), use `assert_typed_fact`.
    #[wasm_bindgen]
    pub fn assert_fact(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
    ) -> Result<String, JsValue> {
        let now = Utc::now();
        let id = self
            .inner
            .assert_fact(subject, predicate, object, now)
            .map_err(to_js_err)?;
        Ok(id.to_string())
    }

    /// Assert a fact with a specific valid_from timestamp (ISO 8601).
    #[wasm_bindgen]
    pub fn assert_fact_at(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        valid_from_iso: &str,
    ) -> Result<String, JsValue> {
        let valid_from: DateTime<Utc> = valid_from_iso
            .parse()
            .map_err(|e: chrono::ParseError| JsValue::from_str(&e.to_string()))?;
        let id = self
            .inner
            .assert_fact(subject, predicate, object, valid_from)
            .map_err(to_js_err)?;
        Ok(id.to_string())
    }

    /// Assert a numeric fact.
    #[wasm_bindgen]
    pub fn assert_number_fact(
        &self,
        subject: &str,
        predicate: &str,
        value: f64,
    ) -> Result<String, JsValue> {
        let now = Utc::now();
        let id = self
            .inner
            .assert_fact(subject, predicate, Value::Number(value), now)
            .map_err(to_js_err)?;
        Ok(id.to_string())
    }

    /// Assert a boolean fact.
    #[wasm_bindgen]
    pub fn assert_boolean_fact(
        &self,
        subject: &str,
        predicate: &str,
        value: bool,
    ) -> Result<String, JsValue> {
        let now = Utc::now();
        let id = self
            .inner
            .assert_fact(subject, predicate, Value::Boolean(value), now)
            .map_err(to_js_err)?;
        Ok(id.to_string())
    }

    /// Assert an entity reference fact (graph edge).
    #[wasm_bindgen]
    pub fn assert_entity_fact(
        &self,
        subject: &str,
        predicate: &str,
        entity: &str,
    ) -> Result<String, JsValue> {
        let now = Utc::now();
        let id = self
            .inner
            .assert_fact(subject, predicate, Value::Entity(entity.to_string()), now)
            .map_err(to_js_err)?;
        Ok(id.to_string())
    }

    /// Get all currently valid facts for (subject, predicate) as JSON.
    #[wasm_bindgen]
    pub fn current_facts(&self, subject: &str, predicate: &str) -> Result<String, JsValue> {
        let facts = self
            .inner
            .current_facts(subject, predicate)
            .map_err(to_js_err)?;
        serde_json::to_string(&facts).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get facts valid at a specific point in time (ISO 8601) as JSON.
    #[wasm_bindgen]
    pub fn facts_at(
        &self,
        subject: &str,
        predicate: &str,
        at_iso: &str,
    ) -> Result<String, JsValue> {
        let at: DateTime<Utc> = at_iso
            .parse()
            .map_err(|e: chrono::ParseError| JsValue::from_str(&e.to_string()))?;
        let facts = self
            .inner
            .facts_at(subject, predicate, at)
            .map_err(to_js_err)?;
        serde_json::to_string(&facts).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get every fact ever recorded about an entity as JSON.
    #[wasm_bindgen]
    pub fn all_facts_about(&self, subject: &str) -> Result<String, JsValue> {
        let facts = self.inner.all_facts_about(subject).map_err(to_js_err)?;
        serde_json::to_string(&facts).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Invalidate a fact by its ID at the current time.
    #[wasm_bindgen]
    pub fn invalidate_fact(&self, fact_id: &str) -> Result<(), JsValue> {
        let id = kronroe::FactId(fact_id.to_string());
        self.inner
            .invalidate_fact(&id, Utc::now())
            .map_err(to_js_err)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_graph_basic_operations() {
        let graph = WasmGraph::open().unwrap();

        // Assert and retrieve
        let id = graph.assert_fact("alice", "works_at", "Acme").unwrap();
        assert!(!id.is_empty());

        let json = graph.current_facts("alice", "works_at").unwrap();
        assert!(json.contains("Acme"));
        assert!(json.contains("alice"));

        // All facts about
        graph.assert_fact("alice", "has_role", "Engineer").unwrap();
        let all = graph.all_facts_about("alice").unwrap();
        assert!(all.contains("works_at"));
        assert!(all.contains("has_role"));
    }

    #[test]
    fn wasm_graph_typed_values() {
        let graph = WasmGraph::open().unwrap();

        graph.assert_number_fact("alice", "score", 0.95).unwrap();
        graph.assert_boolean_fact("alice", "active", true).unwrap();
        graph
            .assert_entity_fact("alice", "employer", "acme_corp")
            .unwrap();

        let all = graph.all_facts_about("alice").unwrap();
        assert!(all.contains("0.95"));
        assert!(all.contains("true"));
        assert!(all.contains("acme_corp"));
    }

    #[test]
    fn wasm_graph_temporal_query() {
        let graph = WasmGraph::open().unwrap();

        graph
            .assert_fact_at("alice", "works_at", "Acme", "2024-01-01T00:00:00Z")
            .unwrap();

        // Valid in March 2024
        let facts = graph
            .facts_at("alice", "works_at", "2024-03-01T00:00:00Z")
            .unwrap();
        assert!(facts.contains("Acme"));

        // Not valid before January 2024
        let empty = graph
            .facts_at("alice", "works_at", "2023-06-01T00:00:00Z")
            .unwrap();
        assert!(!empty.contains("Acme"));
    }

    #[test]
    fn wasm_graph_invalidation() {
        let graph = WasmGraph::open().unwrap();

        let id = graph.assert_fact("alice", "works_at", "Acme").unwrap();
        graph.invalidate_fact(&id).unwrap();

        let current = graph.current_facts("alice", "works_at").unwrap();
        // Should be empty array — fact was invalidated
        assert_eq!(current, "[]");
    }
}
