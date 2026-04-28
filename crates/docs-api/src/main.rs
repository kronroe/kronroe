//! Kronroe docs API — Phase 3a runtime spike.
//!
//! Single-binary HTTP server that demonstrates the planned Phase 3
//! architecture end-to-end with a tiny test corpus:
//!
//!   browser/agent  ──HTTP──▶  axum  ──▶  fastembed-rs  ──▶  Vec<f32>
//!                                                            │
//!                                            Kronroe (vector) ◀┘
//!                                                            │
//!                                                  search_by_vector
//!                                                            │
//!                                                          JSON ──▶
//!
//! Goals of this spike (Phase 3a):
//!
//!   * Verify Kronroe + fastembed-rs + axum compose cleanly in one
//!     Rust binary suitable for Cloud Run.
//!   * Sanity-check the embedding-dimension contract end-to-end.
//!   * Produce a Dockerfile that builds + runs the binary so we know
//!     the deploy story works before writing 20 hours of real code.
//!
//! NOT goals (those are Phase 3b/3c):
//!   * Real corpus loading from `corpus.json`.
//!   * The four production endpoints (sections / get one / recall /
//!     symbols).
//!   * CORS + rate-limiting middleware.
//!   * MCP server.
//!
//! See `.ideas/PLAN_docs_pipeline.md` for the full roadmap.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use kronroe::{KronroeTimestamp, TemporalGraph, Value};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

// ─── Shared state ─────────────────────────────────────────────────

/// Application state shared across all axum handlers.
///
/// Both the embedding model and the Kronroe graph are wrapped in `Arc`
/// because axum's `State` extractor requires `Clone`. `TextEmbedding`
/// internally holds a thread-safe ONNX session, so concurrent calls
/// from multiple tokio workers are fine.
struct AppState {
    embedder: TextEmbedding,
    graph: TemporalGraph,
    /// Embedding dimension established when the corpus was loaded.
    /// Cached so we can validate query dimensions cheaply.
    dim: usize,
}

// ─── Test corpus ──────────────────────────────────────────────────

/// Five hardcoded sections used to exercise the full pipeline at
/// boot time. Phase 3b replaces this with `corpus.json` produced
/// by `site/scripts/build-docs.py`. The shape (subject, predicate,
/// object, body) is intentionally close to the eventual model so
/// the Phase 3b transition is mostly mechanical.
struct TestSection {
    subject: &'static str,
    predicate: &'static str,
    /// The text that gets embedded — usually the section body.
    body: &'static str,
}

const TEST_CORPUS: &[TestSection] = &[
    TestSection {
        subject: "section:bi-temporal-model",
        predicate: "summarises",
        body: "Kronroe implements the TSQL-2 bi-temporal model. Every fact carries \
               four timestamps: valid_from, valid_to, recorded_at, expired_at. \
               Valid time tracks when a fact is true in the world; transaction \
               time tracks when the database learned about it.",
    },
    TestSection {
        subject: "section:facts-and-entities",
        predicate: "summarises",
        body: "Facts are the storage primitive: subject-predicate-value triples \
               with full bi-temporal metadata. Entities are referenced by \
               canonical name strings; the Value::Entity variant expresses graph \
               edges between entities.",
    },
    TestSection {
        subject: "section:vector-search",
        predicate: "summarises",
        body: "Kronroe ships flat-cosine vector search behind the vector feature \
               flag. Embeddings are stored alongside facts and queried with \
               search_by_vector. Temporal filtering happens via an allow-set: \
               invalidated facts are excluded for current queries.",
    },
    TestSection {
        subject: "section:mcp-server",
        predicate: "summarises",
        body: "The kronroe-mcp binary exposes 11 stdio MCP tools wrapping \
               AgentMemory: remember, recall, recall_scored, assemble_context, \
               facts_about, assert_fact, correct_fact, invalidate_fact, \
               what_changed, memory_health, recall_for_task.",
    },
    TestSection {
        subject: "section:platforms",
        predicate: "summarises",
        body: "Kronroe ships across six target platforms from one codebase: \
               Rust crate, Python via PyO3, iOS XCFramework, Android via JNI \
               + Kotlin, browser WebAssembly, and the stdio MCP server. All \
               share the same TemporalGraph engine.",
    },
];

// ─── Bootstrapping ────────────────────────────────────────────────

/// Initialise the embedding model, create an in-memory Kronroe graph,
/// and load the test corpus. Returns the populated `AppState`.
fn bootstrap() -> anyhow::Result<AppState> {
    tracing::info!("loading embedding model (fastembed: AllMiniLML6V2, 384 dims)");
    let embedder = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;

    tracing::info!("opening in-memory TemporalGraph");
    let graph = TemporalGraph::open_in_memory()?;

    tracing::info!("embedding {} test sections", TEST_CORPUS.len());
    let texts: Vec<&str> = TEST_CORPUS.iter().map(|s| s.body).collect();
    let embeddings: Vec<Vec<f32>> = embedder.embed(texts, None)?;
    let dim = embeddings
        .first()
        .map(|v| v.len())
        .ok_or_else(|| anyhow::anyhow!("embedder returned no vectors"))?;

    let now = KronroeTimestamp::now_utc();
    for (section, vector) in TEST_CORPUS.iter().zip(embeddings.into_iter()) {
        graph.assert_fact_with_embedding(
            section.subject,
            section.predicate,
            Value::Text(section.body.to_string()),
            now,
            vector,
        )?;
    }

    tracing::info!(dim, sections = TEST_CORPUS.len(), "corpus loaded");
    Ok(AppState {
        embedder,
        graph,
        dim,
    })
}

// ─── Handlers ─────────────────────────────────────────────────────

/// Liveness probe for Cloud Run / load balancers. Returns 200 OK as
/// soon as the binary is up and the corpus has loaded.
async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// Request body for `POST /api/docs/recall`.
#[derive(Deserialize)]
struct RecallRequest {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    5
}

/// One match in a recall response.
#[derive(Serialize)]
struct RecallHit {
    subject: String,
    score: f32,
    body: String,
}

/// Wrapper response so future fields (timing, model id, etc.) can be
/// added without breaking compatibility.
#[derive(Serialize)]
struct RecallResponse {
    query: String,
    dim: usize,
    hits: Vec<RecallHit>,
}

/// Embed the query, run vector search against the loaded corpus,
/// return ranked hits.
async fn recall(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecallRequest>,
) -> Result<Json<RecallResponse>, ApiError> {
    if req.query.trim().is_empty() {
        return Err(ApiError::bad_request("query must not be empty"));
    }
    let limit = req.limit.clamp(1, 20);

    // Embed the query. fastembed accepts a batch even when we only
    // want one — vec![&str] is cheap.
    let mut embeddings = state
        .embedder
        .embed(vec![req.query.as_str()], None)
        .map_err(|e| ApiError::internal(format!("embed failed: {e}")))?;
    let query_vec = embeddings
        .pop()
        .ok_or_else(|| ApiError::internal("embedder returned no vectors"))?;

    if query_vec.len() != state.dim {
        return Err(ApiError::internal(format!(
            "query dim {} != index dim {}",
            query_vec.len(),
            state.dim
        )));
    }

    let raw_hits = state
        .graph
        .search_by_vector(&query_vec, limit, None)
        .map_err(|e| ApiError::internal(format!("search failed: {e}")))?;

    let hits: Vec<RecallHit> = raw_hits
        .into_iter()
        .map(|(fact, score)| RecallHit {
            subject: fact.subject,
            score,
            body: match fact.object {
                Value::Text(t) => t,
                other => format!("{other:?}"),
            },
        })
        .collect();

    Ok(Json(RecallResponse {
        query: req.query,
        dim: state.dim,
        hits,
    }))
}

// ─── Error type ──────────────────────────────────────────────────

/// Spike-grade error. Phase 3b will replace with a typed enum
/// implementing IntoResponse so we can return useful JSON shapes.
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }

    fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

// ─── Main ────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let state = Arc::new(bootstrap()?);

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/docs/recall", post(recall))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    // Cloud Run passes the port via the PORT env var. Default 8080
    // is the Cloud Run convention for local dev / direct execution.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();

    tracing::info!(%addr, "kronroe-docs-api listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
