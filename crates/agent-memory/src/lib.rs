//! High-level agent memory API built on Kronroe.
//!
//! Designed to be a drop-in alternative to Graphiti / mem0 / MemGPT —
//! without the server, without Neo4j, without Python.
//!
//! # Usage
//!
//! ```rust,no_run
//! use kronroe_agent_memory::AgentMemory;
//!
//! let memory = AgentMemory::open("./my-agent.kronroe").unwrap();
//!
//! // Store a structured fact directly
//! memory.assert("alice", "works_at", "Acme").unwrap();
//!
//! // Query everything known about an entity
//! let facts = memory.facts_about("alice").unwrap();
//!
//! // Query what we knew at a point in time
//! let past: chrono::DateTime<chrono::Utc> = "2024-03-01T00:00:00Z".parse().unwrap();
//! let then = memory.facts_about_at("alice", "works_at", past).unwrap();
//! ```
//!
//! # Phase 1 stubs
//!
//! The following methods are planned for Phase 1 (once the NLP extraction
//! pipeline and vector index are implemented):
//!
//! - `remember(text, episode_id)` — ingest unstructured text
//! - `recall(query, limit)` — semantic search over memory
//! - `assemble_context(query, max_tokens)` — build a context window

use chrono::{DateTime, Utc};
use kronroe::{Fact, FactId, TemporalGraph, Value};

pub use kronroe::KronroeError as Error;
pub type Result<T> = std::result::Result<T, Error>;

/// High-level agent memory store built on a Kronroe temporal graph.
///
/// This is the primary entry point for AI agent developers.
/// It wraps [`TemporalGraph`] with an API designed for agent use cases.
pub struct AgentMemory {
    graph: TemporalGraph,
}

impl AgentMemory {
    /// Open or create an agent memory store at the given path.
    ///
    /// ```rust,no_run
    /// use kronroe_agent_memory::AgentMemory;
    /// let memory = AgentMemory::open("./my-agent.kronroe").unwrap();
    /// ```
    pub fn open(path: &str) -> Result<Self> {
        Ok(Self {
            graph: TemporalGraph::open(path)?,
        })
    }

    /// Store a structured fact with the current time as `valid_from`.
    ///
    /// Use this when you already know the structure of the fact.
    /// For unstructured text, use `remember()` (Phase 1).
    pub fn assert(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
    ) -> Result<FactId> {
        self.graph
            .assert_fact(subject, predicate, object, Utc::now())
    }

    /// Get all currently known facts about an entity (across all predicates).
    pub fn facts_about(&self, entity: &str) -> Result<Vec<Fact>> {
        self.graph.all_facts_about(entity)
    }

    /// Get what was known about an entity for a given predicate at a point in time.
    pub fn facts_about_at(
        &self,
        entity: &str,
        predicate: &str,
        at: DateTime<Utc>,
    ) -> Result<Vec<Fact>> {
        self.graph.facts_at(entity, predicate, at)
    }

    /// Full-text search across known facts.
    ///
    /// Delegates to core search functionality on the underlying temporal graph.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Fact>> {
        self.graph.search(query, limit)
    }

    /// Correct an existing fact by id, preserving temporal history.
    pub fn correct_fact(&self, fact_id: &FactId, new_value: impl Into<Value>) -> Result<FactId> {
        self.graph.correct_fact(fact_id, new_value, Utc::now())
    }

    // -----------------------------------------------------------------------
    // Phase 1 stubs — require NLP extraction pipeline + vector index
    // -----------------------------------------------------------------------

    /// Ingest unstructured text, extract entities and facts, and store them.
    ///
    /// **Phase 1 — not yet implemented.**
    ///
    /// When implemented, this will:
    /// 1. Run NLP entity extraction on `text`
    /// 2. Identify subject-predicate-object triples
    /// 3. Store each as a bi-temporal fact linked to `episode_id`
    #[allow(unused_variables)]
    pub fn remember(&self, text: &str, episode_id: &str) -> Result<Vec<FactId>> {
        unimplemented!(
            "remember() requires the NLP extraction pipeline — planned for Phase 1. \
             Use assert() to store structured facts directly."
        )
    }

    /// Semantic search over memory — returns assembled context for a prompt.
    ///
    /// **Phase 1 — not yet implemented.**
    #[allow(unused_variables)]
    pub fn recall(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        unimplemented!(
            "recall() requires the vector index (hnswlib-rs) — planned for Phase 1. \
             Use facts_about() to query by entity name directly."
        )
    }

    /// Assemble a context window for a prompt.
    ///
    /// **Phase 1 — not yet implemented.**
    ///
    /// When implemented, this will combine semantic search, graph traversal,
    /// and recency weighting into a single context string ready for injection
    /// into an LLM prompt.
    #[allow(unused_variables)]
    pub fn assemble_context(&self, query: &str, max_tokens: usize) -> Result<String> {
        unimplemented!(
            "assemble_context() requires both the vector index and NLP pipeline — Phase 1."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn open_temp_memory() -> (AgentMemory, NamedTempFile) {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_str().unwrap().to_string();
        let memory = AgentMemory::open(&path).unwrap();
        (memory, file)
    }

    #[test]
    fn assert_and_retrieve() {
        let (memory, _tmp) = open_temp_memory();
        memory.assert("alice", "works_at", "Acme").unwrap();

        let facts = memory.facts_about("alice").unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].predicate, "works_at");
    }

    #[test]
    fn multiple_facts_about_entity() {
        let (memory, _tmp) = open_temp_memory();

        memory
            .assert("freya", "attends", "Sunrise Primary")
            .unwrap();
        memory.assert("freya", "has_ehcp", true).unwrap();
        memory.assert("freya", "key_worker", "Sarah Jones").unwrap();

        let facts = memory.facts_about("freya").unwrap();
        assert_eq!(facts.len(), 3);
    }
}
