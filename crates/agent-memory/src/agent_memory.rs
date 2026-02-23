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
//! # Phase 1 API
//!
//! This crate exposes a practical Phase 1 surface:
//! - `remember(text, episode_id, embedding)` — store episodic memory
//! - `recall(query, query_embedding, limit)` — retrieve matching facts
//! - `assemble_context(query, query_embedding, max_tokens)` — build LLM context

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

#[derive(Debug, Clone)]
pub struct AssertParams {
    pub valid_from: DateTime<Utc>,
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

    /// Store a structured fact with explicit parameters.
    pub fn assert_with_params(
        &self,
        subject: &str,
        predicate: &str,
        object: impl Into<Value>,
        params: AssertParams,
    ) -> Result<FactId> {
        self.graph
            .assert_fact(subject, predicate, object, params.valid_from)
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

    /// Store an unstructured memory episode as one fact.
    ///
    /// Subject is the `episode_id`, predicate is `"memory"`, object is `text`.
    pub fn remember(
        &self,
        text: &str,
        episode_id: &str,
        #[cfg(feature = "hybrid")] embedding: Option<Vec<f32>>,
        #[cfg(not(feature = "hybrid"))] _embedding: Option<Vec<f32>>,
    ) -> Result<FactId> {
        #[cfg(feature = "hybrid")]
        if let Some(emb) = embedding {
            return self.graph.assert_fact_with_embedding(
                episode_id,
                "memory",
                text.to_string(),
                Utc::now(),
                emb,
            );
        }

        self.graph
            .assert_fact(episode_id, "memory", text.to_string(), Utc::now())
    }

    /// Retrieve memory facts by query, using vector search when embedding is provided.
    pub fn recall(
        &self,
        query: &str,
        #[cfg(feature = "hybrid")] query_embedding: Option<&[f32]>,
        #[cfg(not(feature = "hybrid"))] _query_embedding: Option<&[f32]>,
        limit: usize,
    ) -> Result<Vec<Fact>> {
        #[cfg(feature = "hybrid")]
        if let Some(emb) = query_embedding {
            let hits = self.graph.search_by_vector(emb, limit, None)?;
            return Ok(hits.into_iter().map(|(fact, _score)| fact).collect());
        }

        self.graph.search(query, limit)
    }

    /// Build a token-bounded prompt context from recalled facts.
    pub fn assemble_context(
        &self,
        query: &str,
        query_embedding: Option<&[f32]>,
        max_tokens: usize,
    ) -> Result<String> {
        let facts = self.recall(query, query_embedding, 20)?;
        let char_budget = max_tokens.saturating_mul(4); // rough 1 token ≈ 4 chars
        let mut context = String::new();

        for fact in &facts {
            let object = match &fact.object {
                Value::Text(s) | Value::Entity(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Boolean(b) => b.to_string(),
            };
            let line = format!(
                "[{}] {} · {} · {}\n",
                fact.valid_from.format("%Y-%m-%d"),
                fact.subject,
                fact.predicate,
                object
            );
            if context.len() + line.len() > char_budget {
                break;
            }
            context.push_str(&line);
        }

        Ok(context)
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

    #[test]
    fn test_remember_stores_fact() {
        let (mem, _tmp) = open_temp_memory();
        let id = mem.remember("Alice loves Rust", "ep-001", None).unwrap();
        assert_eq!(id.0.len(), 26);

        let facts = mem.facts_about("ep-001").unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].subject, "ep-001");
        assert_eq!(facts[0].predicate, "memory");
        assert!(matches!(&facts[0].object, Value::Text(t) if t == "Alice loves Rust"));
    }

    #[test]
    fn test_recall_returns_matching_facts() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Alice loves Rust programming", "ep-001", None)
            .unwrap();
        mem.remember("Bob prefers Python for data science", "ep-002", None)
            .unwrap();

        let results = mem.recall("Rust", None, 5).unwrap();
        assert!(!results.is_empty(), "should find Rust-related facts");
        let has_rust = results
            .iter()
            .any(|f| matches!(&f.object, Value::Text(t) if t.contains("Rust")));
        assert!(has_rust);
    }

    #[test]
    fn test_assemble_context_returns_string() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Alice is a Rust expert", "ep-001", None)
            .unwrap();
        mem.remember("Bob is a Python expert", "ep-002", None)
            .unwrap();

        let ctx = mem.assemble_context("expert", None, 500).unwrap();
        assert!(!ctx.is_empty(), "context should not be empty");
        assert!(
            ctx.contains("expert"),
            "context should contain relevant facts"
        );
    }

    #[test]
    fn test_assemble_context_respects_token_limit() {
        let (mem, _tmp) = open_temp_memory();
        for i in 0..20 {
            mem.remember(
                &format!("fact number {} is quite long and wordy", i),
                &format!("ep-{}", i),
                None,
            )
            .unwrap();
        }
        let ctx = mem.assemble_context("fact", None, 50).unwrap();
        assert!(ctx.len() <= 220, "context should respect max_tokens");
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn test_remember_with_embedding() {
        let (mem, _tmp) = open_temp_memory();
        let id = mem
            .remember("Bob likes Python", "ep-002", Some(vec![0.1f32, 0.2, 0.3]))
            .unwrap();
        assert_eq!(id.0.len(), 26);
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn test_recall_with_query_embedding() {
        let (mem, _tmp) = open_temp_memory();
        mem.remember("Rust systems", "ep-rust", Some(vec![1.0f32, 0.0]))
            .unwrap();
        mem.remember("Python notebooks", "ep-py", Some(vec![0.0f32, 1.0]))
            .unwrap();

        let hits = mem.recall("language", Some(&[1.0, 0.0]), 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].subject, "ep-rust");
    }
}
