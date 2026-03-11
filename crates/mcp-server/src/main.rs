use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use kronroe::{Fact, FactId, Value};
#[cfg(feature = "hybrid")]
use kronroe::{TemporalIntent, TemporalOperator};
use kronroe_agent_memory::{AgentMemory, RecallOptions, RecallScore};
use serde_json::{json, Value as JsonValue};
use std::env;
use std::io::{self, BufRead, BufReader, Write};

const MAX_MESSAGE_BYTES: usize = 1_048_576; // 1 MiB
const MAX_TEXT_BYTES: usize = 32 * 1024; // 32 KiB
const MAX_QUERY_BYTES: usize = 8 * 1024; // 8 KiB
const MAX_EPISODE_ID_BYTES: usize = 512;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 512;
const MAX_RECALL_LIMIT: usize = 200;

struct AppState {
    memory: AgentMemory,
}

impl AppState {
    fn open() -> Result<Self> {
        let db_path =
            env::var("KRONROE_MCP_DB_PATH").unwrap_or_else(|_| "./kronroe-mcp.kronroe".to_string());
        let memory = AgentMemory::open(&db_path)?;
        Ok(Self { memory })
    }
}

fn main() -> Result<()> {
    let mut state = AppState::open().context("failed to open kronroe database")?;
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    loop {
        let maybe = match read_message(&mut reader) {
            Ok(m) => m,
            Err(e) => {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                write_message(&mut writer, &err_resp)?;
                continue;
            }
        };
        let Some(request) = maybe else {
            break;
        };

        if let Some(response) = handle_request(&mut state, &request) {
            write_message(&mut writer, &response)?;
        }
    }

    Ok(())
}

fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<JsonValue>> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }

        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .context("invalid Content-Length")?,
                );
            }
        }
    }

    let len = content_length.context("missing Content-Length header")?;
    if len > MAX_MESSAGE_BYTES {
        anyhow::bail!("Content-Length {len} exceeds max allowed {MAX_MESSAGE_BYTES} bytes");
    }

    let mut payload = vec![0_u8; len];
    reader.read_exact(&mut payload)?;
    let value: JsonValue = serde_json::from_slice(&payload).context("invalid JSON payload")?;
    Ok(Some(value))
}

fn write_message<W: Write>(writer: &mut W, value: &JsonValue) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", payload.len())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

fn handle_request(state: &mut AppState, req: &JsonValue) -> Option<JsonValue> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(JsonValue::as_str)?;

    // All MCP notifications are fire-and-forget and should never receive a response.
    if method.starts_with("notifications/") {
        return None;
    }

    let missing_id_error = |method: &str| {
        json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": {
                "code": -32600,
                "message": format!("request '{method}' is missing required 'id' field")
            }
        })
    };

    match method {
        "initialize" | "tools/list" | "tools/call" | "ping" => {
            let id_val = match id {
                Some(v) => v,
                None => return Some(missing_id_error(method)),
            };
            Some(match method {
                "initialize" => json!({
                    "jsonrpc": "2.0",
                    "id": id_val,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "kronroe-mcp", "version": env!("CARGO_PKG_VERSION") }
                    }
                }),
                "tools/list" => json!({
                    "jsonrpc": "2.0",
                    "id": id_val,
                    "result": {
                        "tools": tools_schema()
                    }
                }),
                "tools/call" => {
                    let result = call_tool(state, req.get("params"));
                    match result {
                        Ok(tool_result) => json!({
                            "jsonrpc": "2.0",
                            "id": id_val,
                            "result": tool_result
                        }),
                        Err(err) => json!({
                            "jsonrpc": "2.0",
                            "id": id_val,
                            "result": {
                                "content": [{ "type": "text", "text": format!("tool error: {err}") }],
                                "isError": true
                            }
                        }),
                    }
                }
                "ping" => json!({ "jsonrpc": "2.0", "id": id_val, "result": {} }),
                _ => unreachable!("request-method match should be exhaustive"),
            })
        }
        _ => id.map(|id_val| {
            json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "error": {
                    "code": -32601,
                    "message": format!("method not found: {method}")
                }
            })
        }),
    }
}

fn tools_schema() -> Vec<JsonValue> {
    vec![
        json!({
            "name": "remember",
            "description": "Ingest text and store fact(s) in memory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "episode_id": {"type": "string"},
                    "idempotency_key": {"type": "string"},
                    "query_embedding": {"type": "array", "items": {"type": "number"}}
                },
                "required": ["text"]
            }
        }),
        json!({
            "name": "recall",
            "description": "Recall facts by natural language query.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_RECALL_LIMIT},
                    "min_confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                    "confidence_filter_mode": {"type": "string", "enum": ["base", "effective"]},
                    "include_scores": {"type": "boolean"},
                    "query_embedding": {"type": "array", "items": {"type": "number"}},
                    "use_hybrid": {"type": "boolean"},
                    "temporal_intent": {"type": "string", "enum": ["timeless", "current_state", "historical_point", "historical_interval"]},
                    "temporal_operator": {"type": "string", "enum": ["current", "as_of", "during", "before", "by", "after", "unknown"]},
                    "max_scored_rows": {"type": "integer", "minimum": 1}
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "recall_scored",
            "description": "Recall facts with per-channel scoring metadata.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_RECALL_LIMIT},
                    "min_confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                    "confidence_filter_mode": {"type": "string", "enum": ["base", "effective"]},
                    "query_embedding": {"type": "array", "items": {"type": "number"}},
                    "use_hybrid": {"type": "boolean"},
                    "temporal_intent": {"type": "string", "enum": ["timeless", "current_state", "historical_point", "historical_interval"]},
                    "temporal_operator": {"type": "string", "enum": ["current", "as_of", "during", "before", "by", "after", "unknown"]},
                    "max_scored_rows": {"type": "integer", "minimum": 1}
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "assemble_context",
            "description": "Build LLM-ready context from top ranked facts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "max_tokens": {"type": "integer", "minimum": 1},
                    "query_embedding": {"type": "array", "items": {"type": "number"}}
                },
                "required": ["query", "max_tokens"]
            }
        }),
        json!({
            "name": "facts_about",
            "description": "Return all facts about an entity.",
            "inputSchema": {
                "type": "object",
                "properties": { "entity": {"type": "string"} },
                "required": ["entity"]
            }
        }),
        json!({
            "name": "assert_fact",
            "description": "Assert a direct fact.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "subject": {"type": "string"},
                    "predicate": {"type": "string"},
                    "object": {},
                    "valid_from": {"type": "string"},
                    "confidence": {"type": "number"},
                    "source": {"type": "string"},
                    "idempotency_key": {"type": "string"}
                },
                "required": ["subject", "predicate", "object"]
            }
        }),
        json!({
            "name": "correct_fact",
            "description": "Correct a fact by id, preserving history.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "fact_id": {"type": "string"},
                    "new_value": {}
                },
                "required": ["fact_id", "new_value"]
            }
        }),
        json!({
            "name": "invalidate_fact",
            "description": "Invalidate a fact by id by ending its validity window.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "fact_id": {"type": "string"}
                },
                "required": ["fact_id"]
            }
        }),
    ]
}

fn call_tool(state: &mut AppState, params: Option<&JsonValue>) -> Result<JsonValue> {
    let name = params
        .and_then(|v| v.get("name"))
        .and_then(JsonValue::as_str)
        .context("missing tool name")?;
    let args = params
        .and_then(|v| v.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        "remember" => call_tool_remember(state, &args),
        "recall" => call_tool_recall(state, &args, false),
        "recall_scored" => call_tool_recall(state, &args, true),
        "assemble_context" => call_tool_assemble_context(state, &args),
        "facts_about" => {
            let entity = args
                .get("entity")
                .and_then(JsonValue::as_str)
                .context("entity is required")?;
            let facts = state.memory.facts_about(entity)?;
            let out: Vec<JsonValue> = facts.into_iter().map(|fact| fact_to_json(&fact)).collect();
            Ok(json!({
                "content": [{ "type": "text", "text": format!("{} fact(s) about {entity}", out.len()) }],
                "structuredContent": { "facts": out }
            }))
        }
        "assert_fact" => {
            let subject = args
                .get("subject")
                .and_then(JsonValue::as_str)
                .context("subject is required")?;
            let predicate = args
                .get("predicate")
                .and_then(JsonValue::as_str)
                .context("predicate is required")?;
            let object = json_to_value(args.get("object").context("object is required")?)?;
            let valid_from = parse_valid_from(args.get("valid_from"))?;
            let confidence = parse_confidence(args.get("confidence"))?;
            let source = args.get("source").and_then(JsonValue::as_str);
            let idempotency_key = args.get("idempotency_key").and_then(JsonValue::as_str);

            if idempotency_key.is_some() && (confidence.is_some() || source.is_some()) {
                anyhow::bail!(
                    "idempotency_key cannot be used with confidence or source in this endpoint"
                );
            }

            let fact_id = if let Some(key) = idempotency_key {
                state.memory.assert_idempotent_with_params(
                    key,
                    subject,
                    predicate,
                    object,
                    kronroe_agent_memory::AssertParams { valid_from },
                )?
            } else if let Some(source) = source {
                state.memory.assert_with_source_with_params(
                    subject,
                    predicate,
                    object,
                    kronroe_agent_memory::AssertParams { valid_from },
                    confidence.unwrap_or(1.0),
                    source,
                )?
            } else if let Some(confidence) = confidence {
                state.memory.assert_with_confidence_with_params(
                    subject,
                    predicate,
                    object,
                    kronroe_agent_memory::AssertParams { valid_from },
                    confidence,
                )?
            } else {
                state.memory.assert_with_params(
                    subject,
                    predicate,
                    object,
                    kronroe_agent_memory::AssertParams { valid_from },
                )?
            };

            Ok(json!({
                "content": [{ "type": "text", "text": format!("asserted fact {fact_id}") }],
                "structuredContent": { "fact_id": fact_id.0 }
            }))
        }
        "correct_fact" => {
            let fact_id = args
                .get("fact_id")
                .and_then(JsonValue::as_str)
                .context("fact_id is required")?;
            let new_value = json_to_value(args.get("new_value").context("new_value is required")?)?;
            let new_fact = state
                .memory
                .correct_fact(&FactId(fact_id.to_string()), new_value)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("corrected fact {fact_id} -> {}", new_fact.0) }],
                "structuredContent": { "new_fact_id": new_fact.0 }
            }))
        }
        "invalidate_fact" => {
            let fact_id = args
                .get("fact_id")
                .and_then(JsonValue::as_str)
                .context("fact_id is required")?;
            state.memory.invalidate_fact(&FactId(fact_id.to_string()))?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("invalidated fact {fact_id}") }],
                "structuredContent": { "fact_id": fact_id }
            }))
        }
        _ => anyhow::bail!("unknown tool: {name}"),
    }
}

fn call_tool_remember(state: &mut AppState, args: &JsonValue) -> Result<JsonValue> {
    let text = args
        .get("text")
        .and_then(JsonValue::as_str)
        .context("text is required")?;
    if text.len() > MAX_TEXT_BYTES {
        anyhow::bail!("text exceeds max allowed size ({} bytes)", MAX_TEXT_BYTES);
    }

    let episode_id = args
        .get("episode_id")
        .and_then(JsonValue::as_str)
        .unwrap_or("default");
    if episode_id.len() > MAX_EPISODE_ID_BYTES {
        anyhow::bail!(
            "episode_id exceeds max allowed size ({} bytes)",
            MAX_EPISODE_ID_BYTES
        );
    }

    let idempotency_key = args.get("idempotency_key").and_then(JsonValue::as_str);
    if let Some(key) = idempotency_key {
        if key.len() > MAX_IDEMPOTENCY_KEY_BYTES {
            anyhow::bail!(
                "idempotency_key exceeds max allowed size ({} bytes)",
                MAX_IDEMPOTENCY_KEY_BYTES
            );
        }
    }

    let query_embedding = parse_embedding(args.get("query_embedding"))?;
    if idempotency_key.is_some() && query_embedding.is_some() {
        anyhow::bail!("idempotency_key is not supported with query_embedding in remember");
    }

    let note_id = if let Some(key) = idempotency_key {
        state
            .memory
            .remember_idempotent(key, text, episode_id)
            .context("failed to remember note")?
            .0
    } else if query_embedding.is_some() {
        #[cfg(feature = "hybrid")]
        {
            let embedding = query_embedding
                .as_deref()
                .expect("query_embedding checked as some");
            state
                .memory
                .remember(text, episode_id, Some(embedding.to_vec()))
                .context("failed to remember note")?
                .0
        }
        #[cfg(not(feature = "hybrid"))]
        {
            anyhow::bail!("query_embedding is unavailable without hybrid feature");
        }
    } else {
        state
            .memory
            .remember(text, episode_id, None)
            .context("failed to remember note")?
            .0
    };

    let mut ids = vec![note_id];

    if let Some((subject, employer)) = parse_works_at(text) {
        let relation_id = if let Some(key) = idempotency_key {
            state
                .memory
                .assert_idempotent(
                    &format!("{key}:works_at"),
                    subject,
                    "works_at",
                    employer.to_string(),
                )?
                .0
        } else {
            state
                .memory
                .assert(subject, "works_at", employer.to_string())?
                .0
        };
        ids.push(relation_id);
    }

    Ok(json!({
        "content": [{ "type": "text", "text": format!("stored {} fact(s)", ids.len()) }],
        "structuredContent": { "fact_ids": ids }
    }))
}

fn call_tool_recall(
    state: &mut AppState,
    args: &JsonValue,
    scored_only: bool,
) -> Result<JsonValue> {
    let query = args
        .get("query")
        .and_then(JsonValue::as_str)
        .context("query is required")?;
    if query.len() > MAX_QUERY_BYTES {
        anyhow::bail!("query exceeds max allowed size ({} bytes)", MAX_QUERY_BYTES);
    }
    let (limit, include_scores) = {
        let raw_limit = args.get("limit").and_then(JsonValue::as_u64).unwrap_or(10);
        let limit = usize::try_from(raw_limit).context("limit must be a valid integer")?;
        if limit > MAX_RECALL_LIMIT {
            anyhow::bail!("limit exceeds max allowed value ({MAX_RECALL_LIMIT})");
        }
        let include_scores = if scored_only {
            true
        } else {
            args.get("include_scores")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false)
        };
        (limit, include_scores)
    };

    let mut opts = RecallOptions::new(query).with_limit(limit);
    let query_embedding = parse_embedding(args.get("query_embedding"))?;
    let use_hybrid = args
        .get("use_hybrid")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    if let Some(embedding) = query_embedding.as_deref() {
        opts = opts.with_embedding(embedding);
        #[cfg(feature = "hybrid")]
        {
            if use_hybrid {
                opts = opts.with_hybrid(true);
            }
            if let Some(intent) = parse_temporal_intent(args.get("temporal_intent"))? {
                opts = opts.with_temporal_intent(intent);
            }
            if let Some(operator) = parse_temporal_operator(args.get("temporal_operator"))? {
                opts = opts.with_temporal_operator(operator);
            }
        }
        #[cfg(not(feature = "hybrid"))]
        {
            if use_hybrid {
                anyhow::bail!("hybrid is unavailable in this build");
            }
            if args.get("temporal_intent").is_some() || args.get("temporal_operator").is_some() {
                anyhow::bail!("temporal controls are unavailable without hybrid feature");
            }
            if args.get("query_embedding").is_some() {
                anyhow::bail!("query_embedding is unavailable without hybrid feature");
            }
        }
    } else if use_hybrid {
        anyhow::bail!("use_hybrid requires query_embedding");
    } else {
        #[cfg(feature = "hybrid")]
        if args.get("temporal_intent").is_some() || args.get("temporal_operator").is_some() {
            anyhow::bail!("temporal_intent and temporal_operator require query_embedding");
        }
        // Text-only path preserves historical behavior.
    }

    if let Some(max_scored_rows) = args.get("max_scored_rows").and_then(JsonValue::as_u64) {
        opts = opts.with_max_scored_rows(
            max_scored_rows
                .try_into()
                .context("max_scored_rows too large")?,
        );
    }

    let min_confidence = parse_confidence(args.get("min_confidence"))?;
    if let Some(min) = min_confidence {
        let mode = args
            .get("confidence_filter_mode")
            .and_then(JsonValue::as_str)
            .unwrap_or("base");
        match mode {
            "base" => opts = opts.with_min_confidence(min),
            "effective" => {
                #[cfg(feature = "uncertainty")]
                {
                    opts = opts.with_min_effective_confidence(min);
                }
                #[cfg(not(feature = "uncertainty"))]
                {
                    anyhow::bail!("effective confidence mode requires uncertainty feature")
                }
            }
            _ => anyhow::bail!("invalid confidence_filter_mode: {mode}"),
        }
    }

    let recall = state.memory.recall_scored_with_options(&opts)?;
    if scored_only || include_scores {
        let mut results = Vec::with_capacity(recall.len());
        for (fact, score) in recall {
            results.push(json!({
                "fact": fact_to_json(&fact),
                "score": recall_score_to_json(&score),
            }));
        }
        return Ok(json!({
            "content": [{ "type": "text", "text": format!("found {} scored fact(s)", results.len()) }],
            "structuredContent": { "results": results }
        }));
    }

    let facts: Vec<JsonValue> = recall
        .into_iter()
        .map(|(fact, _)| fact_to_json(&fact))
        .collect();
    Ok(json!({
        "content": [{ "type": "text", "text": format!("found {} fact(s)", facts.len()) }],
        "structuredContent": { "facts": facts }
    }))
}

fn call_tool_assemble_context(state: &mut AppState, args: &JsonValue) -> Result<JsonValue> {
    let query = args
        .get("query")
        .and_then(JsonValue::as_str)
        .context("query is required")?;
    if query.len() > MAX_QUERY_BYTES {
        anyhow::bail!("query exceeds max allowed size ({} bytes)", MAX_QUERY_BYTES);
    }
    let max_tokens_raw = args
        .get("max_tokens")
        .and_then(JsonValue::as_u64)
        .context("max_tokens is required")?;
    let max_tokens = usize::try_from(max_tokens_raw)
        .map_err(|_| anyhow::anyhow!("max_tokens value too large for this platform"))?;
    if max_tokens == 0 {
        anyhow::bail!("max_tokens must be >= 1");
    }

    let query_embedding = parse_embedding(args.get("query_embedding"))?;
    let context = state
        .memory
        .assemble_context(query, query_embedding.as_deref(), max_tokens)?;
    Ok(json!({
        "content": [{ "type": "text", "text": context.clone() }],
        "structuredContent": { "context": context }
    }))
}

fn parse_embedding(v: Option<&JsonValue>) -> Result<Option<Vec<f32>>> {
    let Some(v) = v else {
        return Ok(None);
    };

    let arr = v.as_array().context("query_embedding must be an array")?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let n = item
            .as_f64()
            .context("query_embedding values must be numbers")?;
        if !n.is_finite() {
            anyhow::bail!("query_embedding values must be finite");
        }
        out.push(n as f32);
    }
    Ok(Some(out))
}

fn parse_confidence(v: Option<&JsonValue>) -> Result<Option<f32>> {
    let Some(v) = v else {
        return Ok(None);
    };
    let n = v
        .as_f64()
        .context("min_confidence/confidence must be a number")?;
    if !n.is_finite() {
        anyhow::bail!("min_confidence/confidence must be finite");
    }
    Ok(Some(n as f32))
}

#[cfg(feature = "hybrid")]
fn parse_temporal_intent(v: Option<&JsonValue>) -> Result<Option<TemporalIntent>> {
    let Some(raw) = v.and_then(JsonValue::as_str) else {
        return Ok(None);
    };
    let parsed = match raw {
        "timeless" => TemporalIntent::Timeless,
        "current_state" => TemporalIntent::CurrentState,
        "historical_point" => TemporalIntent::HistoricalPoint,
        "historical_interval" => TemporalIntent::HistoricalInterval,
        other => anyhow::bail!("invalid temporal_intent '{other}'"),
    };
    Ok(Some(parsed))
}

#[cfg(feature = "hybrid")]
fn parse_temporal_operator(v: Option<&JsonValue>) -> Result<Option<TemporalOperator>> {
    let Some(raw) = v.and_then(JsonValue::as_str) else {
        return Ok(None);
    };
    let parsed = match raw {
        "as_of" => TemporalOperator::AsOf,
        "during" => TemporalOperator::During,
        "before" => TemporalOperator::Before,
        "by" => TemporalOperator::By,
        "after" => TemporalOperator::After,
        "current" => TemporalOperator::Current,
        "unknown" => TemporalOperator::Unknown,
        other => anyhow::bail!("invalid temporal_operator '{other}'"),
    };
    Ok(Some(parsed))
}

#[cfg(not(feature = "hybrid"))]
#[allow(clippy::unnecessary_wraps, dead_code)]
fn parse_temporal_intent(_: Option<&JsonValue>) -> Result<Option<()>> {
    Ok(None)
}

#[cfg(not(feature = "hybrid"))]
#[allow(clippy::unnecessary_wraps, dead_code)]
fn parse_temporal_operator(_: Option<&JsonValue>) -> Result<Option<()>> {
    Ok(None)
}

fn parse_works_at(text: &str) -> Option<(&str, &str)> {
    let needle = b" works at ";
    let idx = text
        .as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))?;
    let subject = text.get(..idx)?.trim();
    let employer = text.get(idx + needle.len()..)?.trim();
    if subject.is_empty() || employer.is_empty() {
        return None;
    }
    Some((subject, employer))
}

fn parse_valid_from(v: Option<&JsonValue>) -> Result<DateTime<Utc>> {
    match v.and_then(JsonValue::as_str) {
        Some(s) => Ok(s
            .parse::<DateTime<Utc>>()
            .context("valid_from must be RFC3339")?),
        None => Ok(Utc::now()),
    }
}

fn json_to_value(v: &JsonValue) -> anyhow::Result<Value> {
    match v {
        JsonValue::Bool(v) => Ok(Value::Boolean(*v)),
        JsonValue::Number(v) => Ok(Value::Number(v.as_f64().unwrap_or_default())),
        JsonValue::String(v) => Ok(Value::Text(v.clone())),
        JsonValue::Null => anyhow::bail!("object must not be null"),
        _ => anyhow::bail!(
            "object must be a scalar (string, number, or boolean), not an array or object"
        ),
    }
}

fn fact_to_json(fact: &Fact) -> JsonValue {
    json!({
        "id": fact.id.0,
        "subject": fact.subject,
        "predicate": fact.predicate,
        "object": match &fact.object {
            Value::Text(v) | Value::Entity(v) => json!(v),
            Value::Number(v) => json!(v),
            Value::Boolean(v) => json!(v),
        },
        "object_type": match fact.object {
            Value::Text(_) => "text",
            Value::Entity(_) => "entity",
            Value::Number(_) => "number",
            Value::Boolean(_) => "boolean",
        },
        "valid_from": fact.valid_from.to_rfc3339(),
        "valid_to": fact.valid_to.map(|v| v.to_rfc3339()),
        "recorded_at": fact.recorded_at.to_rfc3339(),
        "expired_at": fact.expired_at.map(|v| v.to_rfc3339()),
        "confidence": fact.confidence,
        "source": fact.source,
    })
}

fn recall_score_to_json(score: &RecallScore) -> JsonValue {
    match score {
        RecallScore::TextOnly {
            rank,
            bm25_score,
            confidence,
            effective_confidence,
            ..
        } => {
            json!({
                "type": "text",
                "rank": rank,
                "bm25_score": bm25_score,
                "confidence": confidence,
                "effective_confidence": effective_confidence,
            })
        }
        RecallScore::Hybrid {
            rrf_score,
            text_contrib,
            vector_contrib,
            confidence,
            effective_confidence,
            ..
        } => {
            json!({
                "type": "hybrid",
                "rrf_score": rrf_score,
                "text_contrib": text_contrib,
                "vector_contrib": vector_contrib,
                "confidence": confidence,
                "effective_confidence": effective_confidence,
            })
        }
        _ => json!({
            "type": "unknown",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::NamedTempFile;

    fn temp_state() -> AppState {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_string_lossy().to_string();
        AppState {
            memory: AgentMemory::open(&path).unwrap(),
        }
    }

    #[test]
    fn remember_then_recall_returns_facts() {
        let mut state = temp_state();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "remember",
                "arguments": { "text": "alice works at Acme" }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall",
                "arguments": { "query": "alice works at", "limit": 10 }
            })),
        )
        .unwrap();

        let facts = out
            .get("structuredContent")
            .and_then(|v| v.get("facts"))
            .and_then(JsonValue::as_array)
            .unwrap();
        assert!(!facts.is_empty());
    }

    #[test]
    fn recall_with_min_confidence_filters() {
        let mut state = temp_state();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "ep-low",
                    "predicate": "memory",
                    "object": "low confidence memory",
                    "confidence": 0.2
                }
            })),
        )
        .unwrap();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "ep-high",
                    "predicate": "memory",
                    "object": "high confidence memory",
                    "confidence": 0.9
                }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall",
                "arguments": {
                    "query": "memory",
                    "limit": 10,
                    "min_confidence": 0.5,
                    "confidence_filter_mode": "base"
                }
            })),
        )
        .unwrap();

        let facts = out
            .get("structuredContent")
            .and_then(|v| v.get("facts"))
            .and_then(JsonValue::as_array)
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert!(
            (facts[0]
                .get("confidence")
                .and_then(JsonValue::as_f64)
                .unwrap()
                - 0.9)
                .abs()
                < 0.001
        );
    }

    #[test]
    fn recall_scored_honors_max_scored_rows() {
        let mut state = temp_state();
        for i in 0..8 {
            let _ = call_tool(
                &mut state,
                Some(&json!({
                    "name": "assert_fact",
                    "arguments": {
                        "subject": format!("fact-{i}"),
                        "predicate": "memory",
                        "object": "rust"
                    }
                })),
            )
            .unwrap();
        }

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_scored",
                "arguments": {
                    "query": "rust",
                    "limit": 10,
                    "min_confidence": 1.0,
                    "max_scored_rows": 3
                }
            })),
        )
        .unwrap();

        let results = out
            .get("structuredContent")
            .and_then(|v| v.get("results"))
            .and_then(JsonValue::as_array)
            .unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn recall_scored_returns_metadata() {
        let mut state = temp_state();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "works_at",
                    "object": "Acme",
                    "confidence": 0.9,
                    "source": "user:tests"
                }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_scored",
                "arguments": {
                    "query": "alice",
                    "limit": 10,
                    "confidence_filter_mode": "base"
                }
            })),
        )
        .unwrap();

        let results = out
            .get("structuredContent")
            .and_then(|v| v.get("results"))
            .and_then(JsonValue::as_array)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].get("score").is_some());
        assert!(results[0].get("fact").is_some());
    }

    #[test]
    fn assert_fact_respects_valid_from_when_storing_confidence_and_source() {
        let mut state = temp_state();
        let valid_from = "2024-01-01T00:00:00+00:00";
        let expected = 0.42_f64;

        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "works_at",
                    "object": "Acme",
                    "confidence": expected,
                    "source": "user:tests",
                    "valid_from": valid_from
                }
            })),
        )
        .unwrap();

        let facts = call_tool(
            &mut state,
            Some(&json!({ "name": "facts_about", "arguments": { "entity": "alice" } })),
        )
        .unwrap();

        let facts = facts
            .get("structuredContent")
            .and_then(|v| v.get("facts"))
            .and_then(JsonValue::as_array)
            .unwrap();
        assert_eq!(facts.len(), 1);
        let fact = &facts[0];
        assert_eq!(
            fact.get("valid_from").and_then(JsonValue::as_str).unwrap(),
            valid_from
        );
        let confidence = fact.get("confidence").and_then(JsonValue::as_f64).unwrap();
        assert!(
            (confidence - expected).abs() < 1e-6,
            "confidence should be preserved: {confidence}"
        );
        assert_eq!(
            fact.get("source").and_then(JsonValue::as_str).unwrap(),
            "user:tests"
        );
    }

    #[cfg(not(feature = "hybrid"))]
    #[test]
    fn recall_scored_with_query_embedding_errors_without_hybrid() {
        let mut state = temp_state();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "remember",
                "arguments": { "text": "rust facts", "query_embedding": [0.1, 0.2] }
            })),
        );

        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_scored",
                "arguments": {
                    "query": "rust",
                    "query_embedding": [0.1, 0.2]
                }
            })),
        )
        .expect_err("embedding should be unavailable without hybrid feature")
        .to_string();
        assert!(
            err.contains("query_embedding is unavailable") || err.contains("hybrid is unavailable")
        );
    }

    #[cfg(feature = "hybrid")]
    #[test]
    fn recall_scored_embedding_respects_use_hybrid_toggle() {
        let mut state = temp_state();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "remember",
                "arguments": {
                    "text": "Alice loves rust",
                    "query_embedding": [1.0, 0.0, 0.0]
                }
            })),
        )
        .unwrap();

        let off = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_scored",
                "arguments": {
                    "query": "rust",
                    "query_embedding": [1.0, 0.0, 0.0],
                    "limit": 1
                }
            })),
        )
        .unwrap();
        let off_results = off
            .get("structuredContent")
            .and_then(|v| v.get("results"))
            .and_then(JsonValue::as_array)
            .map_or(&[][..], |rows| rows.as_slice());
        assert!(
            !off_results.is_empty(),
            "expected at least one scored result"
        );
        let off_type = off_results[0]
            .get("score")
            .and_then(|v| v.get("type"))
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        assert_eq!(off_type, "text");

        let on = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_scored",
                "arguments": {
                    "query": "rust",
                    "query_embedding": [1.0, 0.0, 0.0],
                    "use_hybrid": true,
                    "limit": 1
                }
            })),
        )
        .unwrap();
        let on_results = on
            .get("structuredContent")
            .and_then(|v| v.get("results"))
            .and_then(JsonValue::as_array)
            .map_or(&[][..], |rows| rows.as_slice());
        assert!(
            !on_results.is_empty(),
            "expected at least one scored result"
        );
        let on_type = on_results[0]
            .get("score")
            .and_then(|v| v.get("type"))
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        assert_eq!(on_type, "hybrid");
    }

    #[test]
    fn invalidate_fact_removes_fact_from_recall() {
        let mut state = temp_state();
        let first = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "works_at",
                    "object": "Acme"
                }
            })),
        )
        .unwrap();
        let fact_id = first
            .get("structuredContent")
            .and_then(|v| v.get("fact_id"))
            .and_then(JsonValue::as_str)
            .unwrap();

        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "invalidate_fact",
                "arguments": { "fact_id": fact_id }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall",
                "arguments": { "query": "Acme", "limit": 10 }
            })),
        )
        .unwrap();
        let facts = out
            .get("structuredContent")
            .and_then(|v| v.get("facts"))
            .and_then(JsonValue::as_array)
            .unwrap();
        assert_eq!(facts.len(), 0);
    }

    #[test]
    fn known_request_without_id_returns_invalid_request_error() {
        let mut state = temp_state();
        let req = json!({
            "jsonrpc": "2.0",
            "method": "tools/list"
        });
        let resp = handle_request(&mut state, &req).expect("request should produce an error");
        assert_eq!(resp.get("id"), Some(&JsonValue::Null));
        assert_eq!(
            resp.get("error")
                .and_then(|v| v.get("code"))
                .and_then(JsonValue::as_i64),
            Some(-32600)
        );
    }

    #[test]
    fn notification_without_id_is_ignored() {
        let mut state = temp_state();
        let req = json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress"
        });
        assert!(
            handle_request(&mut state, &req).is_none(),
            "notifications should never generate responses"
        );
    }

    #[test]
    fn unknown_method_without_id_is_ignored_as_notification() {
        let mut state = temp_state();
        let req = json!({
            "jsonrpc": "2.0",
            "method": "custom/noop"
        });
        assert!(
            handle_request(&mut state, &req).is_none(),
            "method without id should be treated as JSON-RPC notification"
        );
    }

    #[test]
    fn unknown_method_with_id_returns_method_not_found() {
        let mut state = temp_state();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "custom/noop"
        });
        let resp = handle_request(&mut state, &req).expect("request should produce an error");
        assert_eq!(resp.get("id").and_then(JsonValue::as_i64), Some(42));
        assert_eq!(
            resp.get("error")
                .and_then(|v| v.get("code"))
                .and_then(JsonValue::as_i64),
            Some(-32601)
        );
    }

    #[test]
    fn assert_fact_rejects_non_scalar_object() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "profile",
                    "object": { "company": "Acme" }
                }
            })),
        )
        .expect_err("non-scalar object should fail")
        .to_string();
        assert!(err.contains("object must be a scalar"));
    }

    #[test]
    fn assemble_context_rejects_zero_max_tokens() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "assemble_context",
                "arguments": {
                    "query": "alice",
                    "max_tokens": 0
                }
            })),
        )
        .expect_err("max_tokens=0 should fail")
        .to_string();
        assert!(err.contains("max_tokens must be >= 1"));
    }

    #[cfg(target_pointer_width = "32")]
    #[test]
    fn assemble_context_rejects_platform_oversized_max_tokens() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "assemble_context",
                "arguments": {
                    "query": "alice",
                    "max_tokens": (u32::MAX as u64) + 1
                }
            })),
        )
        .expect_err("oversized max_tokens should fail on 32-bit targets")
        .to_string();
        assert!(err.contains("max_tokens value too large"));
    }

    #[test]
    fn read_message_rejects_oversized_frame() {
        let raw = format!("Content-Length: {}\r\n\r\n", MAX_MESSAGE_BYTES + 1);
        let mut cursor = Cursor::new(raw.into_bytes());
        let err = read_message(&mut cursor).expect_err("oversized frame must fail");
        assert!(err.to_string().contains("max allowed"));
    }

    #[test]
    fn read_message_rejects_non_numeric_content_length() {
        let raw = b"Content-Length: abc\r\n\r\n";
        let mut cursor = Cursor::new(raw.to_vec());
        let err = read_message(&mut cursor).expect_err("non-numeric length must fail");
        assert!(err.to_string().contains("invalid Content-Length"));
    }

    #[test]
    fn read_message_rejects_missing_content_length() {
        let raw = b"X-Custom: foo\r\n\r\n{}";
        let mut cursor = Cursor::new(raw.to_vec());
        let err = read_message(&mut cursor).expect_err("missing header must fail");
        assert!(err.to_string().contains("missing Content-Length"));
    }

    #[test]
    fn read_message_returns_none_on_eof() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let result = read_message(&mut cursor).unwrap();
        assert!(result.is_none());
    }
}
