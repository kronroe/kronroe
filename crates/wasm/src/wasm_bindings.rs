//! Kronroe WASM — browser-compatible temporal graph database.
//!
//! This crate wraps [`kronroe_agent_memory::AgentMemory`] for WebAssembly
//! environments. It uses an in-memory storage backend (no file I/O), making it
//! suitable for browser-based demos and agent workflows.
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
use kronroe::{FactId, Value};
use kronroe_agent_memory::{AgentMemory, AssertParams, RecallOptions, RecallScore};
use serde_json::json;
use serde_json::Value as JsonValue;
use wasm_bindgen::prelude::*;

#[cfg(feature = "hybrid")]
use kronroe::{TemporalIntent, TemporalOperator};

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

/// Convert KronroeError to a JsValue for wasm-bindgen.
fn to_js_err(e: kronroe::KronroeError) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn parse_valid_from(iso: &str) -> Result<DateTime<Utc>, JsValue> {
    iso.parse::<DateTime<Utc>>()
        .map_err(|e: chrono::ParseError| JsValue::from_str(&e.to_string()))
}

fn parse_embedding(embedding: Option<Vec<f64>>) -> Result<Option<Vec<f32>>, JsValue> {
    let Some(embedding) = embedding else {
        return Ok(None);
    };
    if embedding.is_empty() {
        return Err(JsValue::from_str("query_embedding must not be empty"));
    }

    let mut out = Vec::with_capacity(embedding.len());
    for value in embedding {
        if !value.is_finite() {
            return Err(JsValue::from_str("query_embedding values must be finite"));
        }
        let narrowed = value as f32;
        if !narrowed.is_finite() {
            return Err(JsValue::from_str(
                "query_embedding value overflows f32 range",
            ));
        }
        out.push(narrowed);
    }
    Ok(Some(out))
}

#[cfg(feature = "hybrid")]
fn parse_temporal_intent(raw: Option<String>) -> Result<Option<TemporalIntent>, JsValue> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let parsed = match raw.as_str() {
        "timeless" => TemporalIntent::Timeless,
        "current_state" => TemporalIntent::CurrentState,
        "historical_point" => TemporalIntent::HistoricalPoint,
        "historical_interval" => TemporalIntent::HistoricalInterval,
        _ => {
            return Err(JsValue::from_str(
                "invalid temporal_intent: expected timeless|current_state|historical_point|historical_interval",
            ));
        }
    };
    Ok(Some(parsed))
}

#[cfg(feature = "hybrid")]
fn parse_temporal_operator(raw: Option<String>) -> Result<Option<TemporalOperator>, JsValue> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let parsed = match raw.as_str() {
        "current" => TemporalOperator::Current,
        "as_of" => TemporalOperator::AsOf,
        "before" => TemporalOperator::Before,
        "by" => TemporalOperator::By,
        "during" => TemporalOperator::During,
        "after" => TemporalOperator::After,
        "unknown" => TemporalOperator::Unknown,
        _ => {
            return Err(JsValue::from_str(
                "invalid temporal_operator: expected current|as_of|before|by|during|after|unknown",
            ));
        }
    };
    Ok(Some(parsed))
}

fn recall_score_payload(score: &RecallScore) -> JsonValue {
    match score {
        RecallScore::Hybrid {
            rrf_score,
            text_contrib,
            vector_contrib,
            confidence,
            effective_confidence,
            ..
        } => json!({
            "type": "hybrid",
            "rrf_score": rrf_score,
            "text_contrib": text_contrib,
            "vector_contrib": vector_contrib,
            "confidence": confidence,
            "effective_confidence": effective_confidence,
        }),
        RecallScore::TextOnly {
            rank,
            bm25_score,
            confidence,
            effective_confidence,
            ..
        } => json!({
            "type": "text",
            "rank": rank,
            "bm25_score": bm25_score,
            "confidence": confidence,
            "effective_confidence": effective_confidence,
        }),
        _ => json!({
            "type": "unsupported",
            "warning": "RecallScore variant not yet supported in wasm bindings",
        }),
    }
}

fn extract_source(source: Option<String>) -> Option<String> {
    source.and_then(|source| {
        if source.is_empty() {
            None
        } else {
            Some(source)
        }
    })
}

// ---------------------------------------------------------------------------
// WasmGraph — the public API
// ---------------------------------------------------------------------------

/// An in-memory AgentMemory store for browser environments.
///
/// All data lives in memory and is lost when the instance is dropped.
/// This is designed for demos, playgrounds, and ephemeral workloads.
#[wasm_bindgen]
pub struct WasmGraph {
    inner: AgentMemory,
}

#[wasm_bindgen]
impl WasmGraph {
    /// Create a new in-memory AgentMemory instance.
    #[wasm_bindgen(constructor)]
    pub fn open() -> Result<WasmGraph, JsValue> {
        let inner = AgentMemory::open_in_memory().map_err(to_js_err)?;
        Ok(WasmGraph { inner })
    }

    /// Assert a new fact and return its ID.
    ///
    /// The object is stored as a text value. For typed values (number,
    /// boolean, entity reference), use typed methods.
    #[wasm_bindgen]
    pub fn assert_fact(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
    ) -> Result<String, JsValue> {
        let id = self
            .inner
            .assert(subject, predicate, object)
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
        let valid_from = parse_valid_from(valid_from_iso)?;
        let id = self
            .inner
            .assert_with_params(subject, predicate, object, AssertParams { valid_from })
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
        let id = self
            .inner
            .assert(subject, predicate, Value::Number(value))
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
        let id = self
            .inner
            .assert(subject, predicate, Value::Boolean(value))
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
        let id = self
            .inner
            .assert(subject, predicate, Value::Entity(entity.to_string()))
            .map_err(to_js_err)?;
        Ok(id.to_string())
    }

    /// Assert a numeric fact with a specific valid_from timestamp (ISO 8601).
    #[wasm_bindgen]
    pub fn assert_number_fact_at(
        &self,
        subject: &str,
        predicate: &str,
        value: f64,
        valid_from_iso: &str,
    ) -> Result<String, JsValue> {
        let valid_from = parse_valid_from(valid_from_iso)?;
        let id = self
            .inner
            .assert_with_params(
                subject,
                predicate,
                Value::Number(value),
                AssertParams { valid_from },
            )
            .map_err(to_js_err)?;
        Ok(id.to_string())
    }

    /// Assert a boolean fact with a specific valid_from timestamp (ISO 8601).
    #[wasm_bindgen]
    pub fn assert_boolean_fact_at(
        &self,
        subject: &str,
        predicate: &str,
        value: bool,
        valid_from_iso: &str,
    ) -> Result<String, JsValue> {
        let valid_from = parse_valid_from(valid_from_iso)?;
        let id = self
            .inner
            .assert_with_params(
                subject,
                predicate,
                Value::Boolean(value),
                AssertParams { valid_from },
            )
            .map_err(to_js_err)?;
        Ok(id.to_string())
    }

    /// Assert an entity reference fact with a specific valid_from timestamp (ISO 8601).
    #[wasm_bindgen]
    pub fn assert_entity_fact_at(
        &self,
        subject: &str,
        predicate: &str,
        entity: &str,
        valid_from_iso: &str,
    ) -> Result<String, JsValue> {
        let valid_from = parse_valid_from(valid_from_iso)?;
        let id = self
            .inner
            .assert_with_params(
                subject,
                predicate,
                Value::Entity(entity.to_string()),
                AssertParams { valid_from },
            )
            .map_err(to_js_err)?;
        Ok(id.to_string())
    }

    /// Assert a fact with confidence, optionally attaching a source marker.
    #[wasm_bindgen]
    pub fn assert_with_confidence(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        confidence: f64,
        source: Option<String>,
    ) -> Result<String, JsValue> {
        if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
            return Err(JsValue::from_str(
                "confidence must be a finite number in [0.0, 1.0]",
            ));
        }
        let confidence = confidence as f32;

        let id = match extract_source(source) {
            Some(source) => self
                .inner
                .assert_with_source(subject, predicate, object, confidence, &source),
            None => self
                .inner
                .assert_with_confidence(subject, predicate, object, confidence),
        }
        .map_err(to_js_err)?;

        Ok(id.to_string())
    }

    /// Correct a fact by ID.
    #[wasm_bindgen]
    pub fn correct_fact(&self, fact_id: &str, new_object: &str) -> Result<String, JsValue> {
        let fact_id = FactId(fact_id.to_string());
        let new_id = self
            .inner
            .correct_fact(&fact_id, new_object.to_string())
            .map_err(to_js_err)?;
        Ok(new_id.to_string())
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
        let at = parse_valid_from(at_iso)?;
        let facts = self
            .inner
            .facts_about_at(subject, predicate, at)
            .map_err(to_js_err)?;
        serde_json::to_string(&facts).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get every fact ever recorded about an entity as JSON.
    #[wasm_bindgen]
    pub fn all_facts_about(&self, subject: &str) -> Result<String, JsValue> {
        let facts = self.inner.facts_about(subject).map_err(to_js_err)?;
        serde_json::to_string(&facts).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Alias for `all_facts_about`.
    #[wasm_bindgen]
    pub fn facts_about(&self, subject: &str) -> Result<String, JsValue> {
        self.all_facts_about(subject)
    }

    /// Recall current facts for a query as JSON.
    #[wasm_bindgen]
    pub fn recall(
        &self,
        query: &str,
        query_embedding: Option<Vec<f64>>,
        limit: usize,
    ) -> Result<String, JsValue> {
        #[cfg(not(feature = "hybrid"))]
        if query_embedding.is_some() {
            return Err(JsValue::from_str(
                "query_embedding is unavailable without the `hybrid` feature",
            ));
        }

        let embedding = parse_embedding(query_embedding)?;
        let facts = self
            .inner
            .recall(query, embedding.as_deref(), limit)
            .map_err(to_js_err)?;
        serde_json::to_string(&facts).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Recall facts with score metadata as JSON.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen]
    pub fn recall_scored(
        &self,
        query: &str,
        limit: usize,
        query_embedding: Option<Vec<f64>>,
        min_confidence: Option<f64>,
        confidence_filter_mode: Option<String>,
        max_scored_rows: Option<usize>,
        use_hybrid: bool,
        temporal_intent: Option<String>,
        temporal_operator: Option<String>,
    ) -> Result<String, JsValue> {
        let embedding = parse_embedding(query_embedding)?;

        #[cfg(not(feature = "hybrid"))]
        if embedding.is_some()
            || use_hybrid
            || temporal_intent.is_some()
            || temporal_operator.is_some()
        {
            return Err(JsValue::from_str(
                "hybrid controls are unavailable without the `hybrid` feature",
            ));
        }

        #[cfg(feature = "hybrid")]
        if embedding.is_none()
            && (use_hybrid || temporal_intent.is_some() || temporal_operator.is_some())
        {
            return Err(JsValue::from_str(
                "query_embedding is required for hybrid/temporal controls",
            ));
        }

        let mut opts = RecallOptions::new(query).with_limit(limit);
        if let Some(embedding) = embedding.as_deref() {
            opts = opts.with_embedding(embedding);
            #[cfg(feature = "hybrid")]
            if use_hybrid {
                opts = opts.with_hybrid(true);
            }
        }

        if confidence_filter_mode.is_some() && min_confidence.is_none() {
            return Err(JsValue::from_str(
                "confidence_filter_mode requires min_confidence",
            ));
        }

        if let Some(min) = min_confidence {
            if !min.is_finite() {
                return Err(JsValue::from_str(
                    "min_confidence/confidence must be finite",
                ));
            }
            let mode = confidence_filter_mode
                .as_deref()
                .unwrap_or("base")
                .to_ascii_lowercase();
            if mode == "base" {
                opts = opts.with_min_confidence(min as f32);
            } else if mode == "effective" {
                #[cfg(feature = "uncertainty")]
                {
                    opts = opts.with_min_effective_confidence(min as f32);
                }
                #[cfg(not(feature = "uncertainty"))]
                {
                    return Err(JsValue::from_str(
                        "effective confidence filter requires the `uncertainty` feature",
                    ));
                }
            } else {
                return Err(JsValue::from_str(
                    "confidence_filter_mode must be 'base' or 'effective'",
                ));
            }
        }

        if let Some(rows) = max_scored_rows {
            opts = opts.with_max_scored_rows(rows);
        }

        #[cfg(feature = "hybrid")]
        if let Some(intent) = parse_temporal_intent(temporal_intent)? {
            opts = opts.with_temporal_intent(intent);
        }
        #[cfg(feature = "hybrid")]
        if let Some(operator) = parse_temporal_operator(temporal_operator)? {
            opts = opts.with_temporal_operator(operator);
        }

        let scored = self
            .inner
            .recall_scored_with_options(&opts)
            .map_err(to_js_err)?;
        let mut rows = Vec::with_capacity(scored.len());
        for (fact, score) in scored {
            rows.push(json!({
                "fact": fact,
                "score": recall_score_payload(&score),
            }));
        }
        serde_json::to_string(&rows).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Build a memory-anchored prompt context from recalled facts.
    #[wasm_bindgen]
    pub fn assemble_context(
        &self,
        query: &str,
        max_tokens: usize,
        query_embedding: Option<Vec<f64>>,
    ) -> Result<String, JsValue> {
        if max_tokens == 0 {
            return Err(JsValue::from_str("max_tokens must be >= 1"));
        }

        #[cfg(not(feature = "hybrid"))]
        if query_embedding.is_some() {
            return Err(JsValue::from_str(
                "query_embedding is unavailable without the `hybrid` feature",
            ));
        }

        let embedding = parse_embedding(query_embedding)?;
        self.inner
            .assemble_context(query, embedding.as_deref(), max_tokens)
            .map_err(to_js_err)
    }

    /// Store an unstructured memory episode.
    ///
    /// Optional `idempotency_key` enables deduplicated retries.
    #[wasm_bindgen]
    pub fn remember(
        &self,
        text: &str,
        episode_id: &str,
        query_embedding: Option<Vec<f64>>,
        idempotency_key: Option<String>,
    ) -> Result<String, JsValue> {
        if idempotency_key.is_some() && query_embedding.is_some() {
            return Err(JsValue::from_str(
                "idempotency_key is not supported with query_embedding in remember",
            ));
        }

        #[cfg(not(feature = "hybrid"))]
        if query_embedding.is_some() {
            return Err(JsValue::from_str(
                "query_embedding is unavailable without the `hybrid` feature",
            ));
        }

        if let Some(key) = idempotency_key {
            let id = self
                .inner
                .remember_idempotent(&key, text, episode_id)
                .map_err(to_js_err)?;
            return Ok(id.to_string());
        }

        let embedding = parse_embedding(query_embedding)?;
        let id = if let Some(_embedding) = embedding {
            #[cfg(feature = "hybrid")]
            {
                self.inner.remember(text, episode_id, Some(_embedding))
            }
            #[cfg(not(feature = "hybrid"))]
            {
                return Err(JsValue::from_str(
                    "query_embedding is unavailable without the `hybrid` feature",
                ));
            }
        } else {
            self.inner.remember(text, episode_id, None)
        }
        .map_err(to_js_err)?;
        Ok(id.to_string())
    }

    /// Invalidate a fact by its ID at the current time.
    #[wasm_bindgen]
    pub fn invalidate_fact(&self, fact_id: &str) -> Result<(), JsValue> {
        let id = FactId(fact_id.to_string());
        self.inner.invalidate_fact(&id).map_err(to_js_err)
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

    #[cfg(feature = "hybrid")]
    #[test]
    fn wasm_graph_recall_scored_respects_min_confidence() {
        let graph = WasmGraph::open().unwrap();

        graph
            .remember(
                "Alice joined Acme engineering.",
                "ep-high",
                Some(vec![1.0, 0.0, 0.0]),
                None,
            )
            .unwrap();
        graph
            .remember(
                "Alice likes hiking on weekends.",
                "ep-low",
                Some(vec![0.0, 1.0, 0.0]),
                None,
            )
            .unwrap();

        let rows_json = graph
            .recall_scored(
                "Acme",
                10,
                Some(vec![1.0, 0.0, 0.0]),
                Some(0.5),
                Some("base".to_string()),
                None,
                true,
                None,
                None,
            )
            .unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_str(&rows_json).unwrap();
        assert!(!rows.is_empty());

        let score = &rows[0]["score"];
        assert_eq!(score["type"], "hybrid");
        assert!(score.get("kind").is_none());
        assert!(score["confidence"].as_f64().unwrap() >= 0.5);
    }

    #[test]
    fn wasm_graph_remember_persists_episode_fact() {
        let graph = WasmGraph::open().unwrap();

        graph
            .remember("Alice joined Acme as an engineer.", "ep-1", None, None)
            .unwrap();

        let all = graph.all_facts_about("ep-1").unwrap();
        assert!(all.contains("memory"));
        assert!(all.contains("Alice joined Acme as an engineer."));
    }
}
