use anyhow::{Context, Result};
#[cfg(test)]
use kronroe::FactId;
use kronroe::{Fact, KronroeSpan, KronroeTimestamp, Value};
#[cfg(feature = "hybrid")]
use kronroe::{TemporalIntent, TemporalOperator};
use kronroe_agent_memory::{
    is_high_impact_predicate, AgentMemory, ConfidenceShift, FactCorrection, MemoryHealthReport,
    RecallForTaskReport, RecallOptions, RecallScore, WhatChangedReport,
};
use serde_json::{json, Map, Value as JsonValue};
use std::env;
use std::io::{self, BufRead, BufReader, Write};

const MAX_MESSAGE_BYTES: usize = 1_048_576; // 1 MiB
const MAX_TEXT_BYTES: usize = 32 * 1024; // 32 KiB
const MAX_QUERY_BYTES: usize = 8 * 1024; // 8 KiB
const MAX_EPISODE_ID_BYTES: usize = 512;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 512;
const MAX_RECALL_LIMIT: usize = 200;
const AGENT_BRIEF_SCHEMA_VERSION: &str = "1.0";

fn confidence_filter_mode_schema() -> JsonValue {
    #[cfg(feature = "uncertainty")]
    {
        json!({ "type": "string", "enum": ["base", "effective"] })
    }
    #[cfg(not(feature = "uncertainty"))]
    {
        json!({ "type": "string", "enum": ["base"] })
    }
}

fn recall_input_schema(include_scores: bool) -> JsonValue {
    let mut properties = Map::new();
    properties.insert("query".to_string(), json!({ "type": "string" }));
    properties.insert(
        "limit".to_string(),
        json!({ "type": "integer", "minimum": 1, "maximum": MAX_RECALL_LIMIT }),
    );
    properties.insert(
        "min_confidence".to_string(),
        json!({ "type": "number", "minimum": 0.0, "maximum": 1.0 }),
    );
    properties.insert(
        "confidence_filter_mode".to_string(),
        confidence_filter_mode_schema(),
    );
    if include_scores {
        properties.insert("include_scores".to_string(), json!({ "type": "boolean" }));
    }
    properties.insert(
        "max_scored_rows".to_string(),
        json!({ "type": "integer", "minimum": 1 }),
    );
    #[cfg(feature = "hybrid")]
    {
        properties.insert(
            "query_embedding".to_string(),
            json!({ "type": "array", "items": {"type": "number"} }),
        );
        properties.insert("use_hybrid".to_string(), json!({ "type": "boolean" }));
        properties.insert(
            "temporal_intent".to_string(),
            json!({
                "type": "string",
                "enum": ["timeless", "current_state", "historical_point", "historical_interval"]
            }),
        );
        properties.insert(
            "temporal_operator".to_string(),
            json!({
                "type": "string",
                "enum": ["current", "as_of", "during", "before", "by", "after", "unknown"]
            }),
        );
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": ["query"]
    })
}

fn recall_for_task_input_schema() -> JsonValue {
    let mut properties = Map::new();
    properties.insert("task".to_string(), json!({ "type": "string" }));
    properties.insert("subject".to_string(), json!({ "type": "string" }));
    properties.insert("now".to_string(), json!({ "type": "string" }));
    properties.insert(
        "horizon_days".to_string(),
        json!({ "type": "integer", "minimum": 1 }),
    );
    properties.insert(
        "limit".to_string(),
        json!({ "type": "integer", "minimum": 1, "maximum": MAX_RECALL_LIMIT }),
    );
    #[cfg(feature = "hybrid")]
    {
        properties.insert(
            "query_embedding".to_string(),
            json!({ "type": "array", "items": {"type": "number"} }),
        );
        properties.insert("use_hybrid".to_string(), json!({ "type": "boolean" }));
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": ["task"]
    })
}

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
            "inputSchema": recall_input_schema(true)
        }),
        json!({
            "name": "recall_scored",
            "description": "Recall facts with per-channel scoring metadata.",
            "inputSchema": recall_input_schema(false)
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
        json!({
            "name": "what_changed",
            "description": "Return a change report for an entity since a timestamp.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entity": {"type": "string"},
                    "since": {"type": "string"},
                    "predicate": {"type": "string"}
                },
                "required": ["entity", "since"]
            }
        }),
        json!({
            "name": "memory_health",
            "description": "Return a practical memory-health report for one entity.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entity": {"type": "string"},
                    "predicate": {"type": "string"},
                    "low_confidence_threshold": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                    "stale_after_days": {"type": "integer", "minimum": 0}
                },
                "required": ["entity"]
            }
        }),
        json!({
            "name": "recall_for_task",
            "description": "Return decision-ready memory context for a concrete task.",
            "inputSchema": recall_for_task_input_schema()
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
                "structuredContent": { "fact_id": fact_id.as_str() }
            }))
        }
        "correct_fact" => {
            let fact_id = args
                .get("fact_id")
                .and_then(JsonValue::as_str)
                .context("fact_id is required")?;
            let new_value = json_to_value(args.get("new_value").context("new_value is required")?)?;
            let new_fact = state.memory.correct_fact(fact_id, new_value)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("corrected fact {fact_id} -> {}", new_fact.as_str()) }],
                "structuredContent": { "new_fact_id": new_fact.as_str() }
            }))
        }
        "invalidate_fact" => {
            let fact_id = args
                .get("fact_id")
                .and_then(JsonValue::as_str)
                .context("fact_id is required")?;
            state.memory.invalidate_fact(fact_id)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("invalidated fact {fact_id}") }],
                "structuredContent": { "fact_id": fact_id }
            }))
        }
        "what_changed" => call_tool_what_changed(state, &args),
        "memory_health" => call_tool_memory_health(state, &args),
        "recall_for_task" => call_tool_recall_for_task(state, &args),
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
            .to_string()
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
                .to_string()
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
            .to_string()
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
                .to_string()
        } else {
            state
                .memory
                .assert(subject, "works_at", employer.to_string())?
                .to_string()
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
        let limit = match args.get("limit") {
            Some(value) => {
                let raw_limit = value
                    .as_u64()
                    .context("limit must be an integer greater than or equal to 1")?;
                let parsed = usize::try_from(raw_limit).context("limit must be a valid integer")?;
                if parsed == 0 {
                    anyhow::bail!("limit must be greater than or equal to 1");
                }
                parsed
            }
            None => 10,
        };
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
    let use_hybrid = args.get("use_hybrid").and_then(JsonValue::as_bool);
    if let Some(embedding) = query_embedding.as_deref() {
        opts = opts.with_embedding(embedding);
        #[cfg(feature = "hybrid")]
        {
            if use_hybrid != Some(false) {
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
            if use_hybrid == Some(true) {
                anyhow::bail!("hybrid is unavailable in this build");
            }
            if args.get("temporal_intent").is_some() || args.get("temporal_operator").is_some() {
                anyhow::bail!("temporal controls are unavailable without hybrid feature");
            }
            if args.get("query_embedding").is_some() {
                anyhow::bail!("query_embedding is unavailable without hybrid feature");
            }
        }
    } else if use_hybrid == Some(true) {
        anyhow::bail!("use_hybrid requires query_embedding");
    } else {
        #[cfg(feature = "hybrid")]
        if args.get("temporal_intent").is_some() || args.get("temporal_operator").is_some() {
            anyhow::bail!("temporal_intent and temporal_operator require query_embedding");
        }
        // Text-only path preserves historical behavior.
    }

    if let Some(value) = args.get("max_scored_rows") {
        let max_scored_rows = value
            .as_u64()
            .context("max_scored_rows must be an integer greater than or equal to 1")?;
        if max_scored_rows == 0 {
            anyhow::bail!("max_scored_rows must be greater than or equal to 1");
        }
        opts = opts.with_max_scored_rows(
            max_scored_rows
                .try_into()
                .context("max_scored_rows too large")?,
        );
    }

    let min_confidence = parse_confidence(args.get("min_confidence"))?;
    let confidence_filter_mode = args
        .get("confidence_filter_mode")
        .and_then(JsonValue::as_str);
    if confidence_filter_mode.is_some() && min_confidence.is_none() {
        anyhow::bail!("confidence_filter_mode requires min_confidence");
    }
    if let Some(min) = min_confidence {
        let mode = confidence_filter_mode.unwrap_or("base");
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

fn call_tool_what_changed(state: &mut AppState, args: &JsonValue) -> Result<JsonValue> {
    let entity = args
        .get("entity")
        .and_then(JsonValue::as_str)
        .context("entity is required")?;
    let since = args
        .get("since")
        .and_then(JsonValue::as_str)
        .context("since is required")?
        .parse::<KronroeTimestamp>()
        .context("since must be RFC3339")?;
    let predicate = args.get("predicate").and_then(JsonValue::as_str);

    let report = state.memory.what_changed(entity, since, predicate)?;
    let report_json = what_changed_report_to_json(&report);
    let agent_brief = what_changed_agent_brief(&report);
    Ok(json!({
        "content": [{ "type": "text", "text": format!(
            "{} new, {} invalidated, {} correction(s) for {entity}",
            report.new_facts.len(),
            report.invalidated_facts.len(),
            report.corrections.len()
        )}],
        "structuredContent": {
            "report": report_json,
            "agent_brief": agent_brief
        }
    }))
}

fn call_tool_memory_health(state: &mut AppState, args: &JsonValue) -> Result<JsonValue> {
    let entity = args
        .get("entity")
        .and_then(JsonValue::as_str)
        .context("entity is required")?;
    let predicate = args.get("predicate").and_then(JsonValue::as_str);

    let low_confidence_threshold = match args.get("low_confidence_threshold") {
        Some(value) => {
            let threshold = value
                .as_f64()
                .context("low_confidence_threshold must be a number")?;
            if !threshold.is_finite() {
                anyhow::bail!("low_confidence_threshold must be finite");
            }
            if !(0.0..=1.0).contains(&threshold) {
                anyhow::bail!("low_confidence_threshold must be between 0.0 and 1.0");
            }
            threshold as f32
        }
        None => 0.7,
    };

    let stale_after_days = match args.get("stale_after_days") {
        Some(value) => {
            let days = value
                .as_i64()
                .context("stale_after_days must be an integer")?;
            if days < 0 {
                anyhow::bail!("stale_after_days must be >= 0");
            }
            days
        }
        None => 90,
    };

    let report = state.memory.memory_health(
        entity,
        predicate,
        low_confidence_threshold,
        stale_after_days,
    )?;

    let report_json = memory_health_report_to_json(&report);
    let agent_brief = memory_health_agent_brief(&report);
    Ok(json!({
        "content": [{ "type": "text", "text": format!(
            "health: {} active / {} total; {} low-confidence; {} stale high-impact; {} contradiction(s)",
            report.active_fact_count,
            report.total_fact_count,
            report.low_confidence_facts.len(),
            report.stale_high_impact_facts.len(),
            report.contradiction_count
        )}],
        "structuredContent": {
            "report": report_json,
            "agent_brief": agent_brief
        }
    }))
}

fn call_tool_recall_for_task(state: &mut AppState, args: &JsonValue) -> Result<JsonValue> {
    let task = args
        .get("task")
        .and_then(JsonValue::as_str)
        .context("task is required")?;
    if task.len() > MAX_QUERY_BYTES {
        anyhow::bail!("task exceeds max allowed size ({} bytes)", MAX_QUERY_BYTES);
    }

    let subject = args.get("subject").and_then(JsonValue::as_str);
    let now = args
        .get("now")
        .and_then(JsonValue::as_str)
        .map(|raw| {
            raw.parse::<KronroeTimestamp>()
                .context("now must be RFC3339 when provided")
        })
        .transpose()?;
    let horizon_days = match args.get("horizon_days") {
        Some(value) => {
            let days = value.as_i64().context("horizon_days must be an integer")?;
            if days < 1 {
                anyhow::bail!("horizon_days must be >= 1");
            }
            days
        }
        None => 30,
    };
    let limit = {
        let raw_limit = args.get("limit").and_then(JsonValue::as_u64).unwrap_or(8);
        let limit = usize::try_from(raw_limit).context("limit must be a valid integer")?;
        if limit == 0 {
            anyhow::bail!("limit must be greater than or equal to 1");
        }
        if limit > MAX_RECALL_LIMIT {
            anyhow::bail!("limit exceeds max allowed value ({MAX_RECALL_LIMIT})");
        }
        limit
    };

    let query_embedding = parse_embedding(args.get("query_embedding"))?;
    let use_hybrid = args.get("use_hybrid").and_then(JsonValue::as_bool);

    #[cfg(not(feature = "hybrid"))]
    if query_embedding.is_some() || use_hybrid == Some(true) {
        anyhow::bail!("hybrid task recall controls are unavailable without hybrid feature");
    }

    #[cfg(feature = "hybrid")]
    let embedding_for_call = if query_embedding.is_some() && use_hybrid != Some(false) {
        query_embedding.as_deref()
    } else {
        None
    };
    #[cfg(not(feature = "hybrid"))]
    let embedding_for_call: Option<&[f32]> = None;

    let report = state.memory.recall_for_task(
        task,
        subject,
        now,
        Some(horizon_days),
        limit,
        embedding_for_call,
    )?;
    let report_json = recall_for_task_report_to_json(&report);
    let agent_brief = recall_for_task_agent_brief(&report);

    Ok(json!({
        "content": [{ "type": "text", "text": format!(
            "task report: {} key fact(s), {} watchout(s), {} next check(s)",
            report.key_facts.len(),
            report.watchouts.len(),
            report.recommended_next_checks.len()
        )}],
        "structuredContent": {
            "report": report_json,
            "agent_brief": agent_brief
        }
    }))
}

fn parse_embedding(v: Option<&JsonValue>) -> Result<Option<Vec<f32>>> {
    let Some(v) = v else {
        return Ok(None);
    };

    let arr = v.as_array().context("query_embedding must be an array")?;
    if arr.is_empty() {
        anyhow::bail!("query_embedding must not be empty");
    }
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let n = item
            .as_f64()
            .context("query_embedding values must be numbers")?;
        if !n.is_finite() {
            anyhow::bail!("query_embedding values must be finite");
        }
        let narrowed = n as f32;
        if !narrowed.is_finite() {
            anyhow::bail!("query_embedding values overflow f32 range");
        }
        out.push(narrowed);
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

fn parse_valid_from(v: Option<&JsonValue>) -> Result<KronroeTimestamp> {
    match v.and_then(JsonValue::as_str) {
        Some(s) => Ok(s
            .parse::<KronroeTimestamp>()
            .context("valid_from must be RFC3339")?),
        None => Ok(KronroeTimestamp::now_utc()),
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
        "id": fact.id.as_str(),
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
            "type": "unsupported",
        }),
    }
}

fn fact_correction_to_json(correction: &FactCorrection) -> JsonValue {
    json!({
        "old_fact": fact_to_json(&correction.old_fact),
        "new_fact": fact_to_json(&correction.new_fact),
    })
}

fn confidence_shift_to_json(shift: &ConfidenceShift) -> JsonValue {
    json!({
        "from_fact_id": shift.from_fact_id.as_str(),
        "to_fact_id": shift.to_fact_id.as_str(),
        "from_confidence": shift.from_confidence,
        "to_confidence": shift.to_confidence,
    })
}

fn what_changed_report_to_json(report: &WhatChangedReport) -> JsonValue {
    let new_facts: Vec<JsonValue> = report.new_facts.iter().map(fact_to_json).collect();
    let invalidated_facts: Vec<JsonValue> =
        report.invalidated_facts.iter().map(fact_to_json).collect();
    let corrections: Vec<JsonValue> = report
        .corrections
        .iter()
        .map(fact_correction_to_json)
        .collect();
    let confidence_shifts: Vec<JsonValue> = report
        .confidence_shifts
        .iter()
        .map(confidence_shift_to_json)
        .collect();

    json!({
        "entity": report.entity.clone(),
        "since": report.since.to_rfc3339(),
        "predicate_filter": report.predicate_filter.clone(),
        "new_facts": new_facts,
        "invalidated_facts": invalidated_facts,
        "corrections": corrections,
        "confidence_shifts": confidence_shifts,
    })
}

fn memory_health_report_to_json(report: &MemoryHealthReport) -> JsonValue {
    let low_confidence_facts: Vec<JsonValue> = report
        .low_confidence_facts
        .iter()
        .map(fact_to_json)
        .collect();
    let stale_high_impact_facts: Vec<JsonValue> = report
        .stale_high_impact_facts
        .iter()
        .map(fact_to_json)
        .collect();

    json!({
        "entity": report.entity.clone(),
        "generated_at": report.generated_at.to_rfc3339(),
        "predicate_filter": report.predicate_filter.clone(),
        "total_fact_count": report.total_fact_count,
        "active_fact_count": report.active_fact_count,
        "low_confidence_facts": low_confidence_facts,
        "stale_high_impact_facts": stale_high_impact_facts,
        "contradiction_count": report.contradiction_count,
        "recommended_actions": report.recommended_actions.clone(),
    })
}

fn recall_for_task_report_to_json(report: &RecallForTaskReport) -> JsonValue {
    let key_facts: Vec<JsonValue> = report.key_facts.iter().map(fact_to_json).collect();
    json!({
        "task": report.task.clone(),
        "subject": report.subject.clone(),
        "generated_at": report.generated_at.to_rfc3339(),
        "horizon_days": report.horizon_days,
        "query_used": report.query_used.clone(),
        "key_facts": key_facts,
        "low_confidence_count": report.low_confidence_count,
        "stale_high_impact_count": report.stale_high_impact_count,
        "contradiction_count": report.contradiction_count,
        "watchouts": report.watchouts.clone(),
        "recommended_next_checks": report.recommended_next_checks.clone(),
    })
}

fn level_score(level: &str) -> f32 {
    match level {
        "high" => 0.9,
        "medium" => 0.6,
        "low" => 0.3,
        _ => 0.4,
    }
}

fn workflow_impact_score(workflow_impact: &str) -> f32 {
    match workflow_impact {
        "critical_identity" => 0.95,
        "decision_reliability" => 0.85,
        "context_coverage" => 0.7,
        "freshness_hygiene" => 0.55,
        "monitoring" => 0.35,
        _ => 0.5,
    }
}

fn execution_mode(automation_confidence: f32, risk_if_skipped: &str) -> &'static str {
    if automation_confidence >= 0.8 && risk_if_skipped == "high" {
        "auto_with_guardrails"
    } else if automation_confidence >= 0.6 {
        "agent_suggest_then_execute"
    } else {
        "human_confirm"
    }
}

struct AgentActionSpec<'a> {
    id: &'a str,
    priority: &'a str,
    action: String,
    rationale: String,
    suggested_tool: &'a str,
    suggested_arguments: JsonValue,
    workflow_impact: &'a str,
    risk_if_skipped: &'a str,
    automation_confidence: f32,
}

fn make_ranked_agent_action(spec: AgentActionSpec<'_>) -> (f32, JsonValue) {
    let priority_score = level_score(spec.priority);
    let risk_score = level_score(spec.risk_if_skipped);
    let workflow_score = workflow_impact_score(spec.workflow_impact);
    let bounded_confidence = spec.automation_confidence.clamp(0.0, 1.0);
    let action_score = (0.45 * risk_score)
        + (0.30 * workflow_score)
        + (0.15 * priority_score)
        + (0.10 * bounded_confidence);

    let action_json = json!({
        "id": spec.id,
        "priority": spec.priority,
        "priority_score": priority_score,
        "risk_if_skipped": spec.risk_if_skipped,
        "risk_score": risk_score,
        "workflow_impact": spec.workflow_impact,
        "workflow_impact_score": workflow_score,
        "automation_confidence": bounded_confidence,
        "execution_mode": execution_mode(bounded_confidence, spec.risk_if_skipped),
        "action_score": action_score,
        "action": spec.action,
        "rationale": spec.rationale,
        "suggested_tool": spec.suggested_tool,
        "suggested_arguments": spec.suggested_arguments,
    });

    (action_score, action_json)
}

fn recall_for_task_agent_brief(report: &RecallForTaskReport) -> JsonValue {
    let status = if report.key_facts.is_empty() {
        "no_context"
    } else if report.watchouts.is_empty() {
        "ready"
    } else {
        "attention_needed"
    };
    let subject = report.subject.as_deref();

    let mut ranked_actions: Vec<(f32, JsonValue)> = Vec::new();
    if report.key_facts.is_empty() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "ask_clarifying_question",
            priority: "high",
            action: "Ask a clarifying follow-up to gather missing task context.".to_string(),
            rationale: "No key facts are available yet for this task.".to_string(),
            suggested_tool: "recall_for_task",
            suggested_arguments: json!({
                "task": report.task.clone(),
                "subject": report.subject.clone(),
                "horizon_days": report.horizon_days,
                "limit": 12
            }),
            workflow_impact: "context_coverage",
            risk_if_skipped: "high",
            automation_confidence: 0.76,
        }));
    }

    if report.low_confidence_count > 0 && subject.is_some() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "verify_low_confidence_facts",
            priority: "high",
            action: "Validate low-confidence task facts with a fresh source.".to_string(),
            rationale: "Low-confidence facts can derail task execution.".to_string(),
            suggested_tool: "memory_health",
            suggested_arguments: json!({
                "entity": subject,
                "low_confidence_threshold": 0.7,
                "stale_after_days": report.horizon_days
            }),
            workflow_impact: "decision_reliability",
            risk_if_skipped: "high",
            automation_confidence: 0.82,
        }));
    }

    if report.stale_high_impact_count > 0 && subject.is_some() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "refresh_stale_high_impact_facts",
            priority: "medium",
            action: "Refresh stale high-impact facts before finalizing the task plan.".to_string(),
            rationale: "Stale high-impact facts often drive incorrect decisions.".to_string(),
            suggested_tool: "what_changed",
            suggested_arguments: json!({
                "entity": subject,
                "since": (report.generated_at - KronroeSpan::days(report.horizon_days)).to_rfc3339()
            }),
            workflow_impact: "freshness_hygiene",
            risk_if_skipped: "medium",
            automation_confidence: 0.77,
        }));
    }

    if report.contradiction_count > 0 && subject.is_some() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "resolve_contradictions_first",
            priority: "high",
            action: "Resolve contradictions before executing task-critical steps.".to_string(),
            rationale: "Contradictions indicate mutually inconsistent memory state.".to_string(),
            suggested_tool: "memory_health",
            suggested_arguments: json!({
                "entity": subject
            }),
            workflow_impact: "critical_identity",
            risk_if_skipped: "high",
            automation_confidence: 0.80,
        }));
    }

    if !report.key_facts.is_empty() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "prepare_task_brief",
            priority: if report.watchouts.is_empty() {
                "high"
            } else {
                "medium"
            },
            action: "Build a concise execution brief from the key facts.".to_string(),
            rationale: "Task briefs reduce context switching for human and AI collaborators."
                .to_string(),
            suggested_tool: "assemble_context",
            suggested_arguments: json!({
                "query": report.query_used.clone(),
                "max_tokens": 350
            }),
            workflow_impact: "decision_reliability",
            risk_if_skipped: "medium",
            automation_confidence: 0.91,
        }));
    }

    if ranked_actions.is_empty() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "monitor_task_memory",
            priority: "low",
            action: "Task memory looks stable; continue periodic monitoring.".to_string(),
            rationale: "No immediate memory quality intervention is required.".to_string(),
            suggested_tool: "recall_for_task",
            suggested_arguments: json!({
                "task": report.task.clone(),
                "subject": report.subject.clone(),
                "horizon_days": report.horizon_days,
                "limit": report.key_facts.len().max(1)
            }),
            workflow_impact: "monitoring",
            risk_if_skipped: "low",
            automation_confidence: 0.93,
        }));
    }

    ranked_actions.sort_by(|left, right| right.0.total_cmp(&left.0));
    let recommended_action_id = ranked_actions
        .first()
        .and_then(|(_, action)| action.get("id"))
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "monitor_task_memory".to_string());
    let automation_ready = ranked_actions.iter().any(|(_, action)| {
        action
            .get("execution_mode")
            .and_then(JsonValue::as_str)
            .is_some_and(|mode| mode != "human_confirm")
    });
    let next_actions: Vec<JsonValue> = ranked_actions
        .into_iter()
        .map(|(_, action)| action)
        .collect();

    json!({
        "schema_version": AGENT_BRIEF_SCHEMA_VERSION,
        "decision_mode": "agent_first",
        "status": status,
        "summary": format!(
            "{} key facts, {} watchouts, {} recommended checks",
            report.key_facts.len(),
            report.watchouts.len(),
            report.recommended_next_checks.len()
        ),
        "signals": {
            "key_fact_count": report.key_facts.len(),
            "low_confidence_count": report.low_confidence_count,
            "stale_high_impact_count": report.stale_high_impact_count,
            "contradiction_count": report.contradiction_count
        },
        "recommended_action_id": recommended_action_id,
        "automation_ready": automation_ready,
        "next_actions": next_actions
    })
}

fn what_changed_agent_brief(report: &WhatChangedReport) -> JsonValue {
    let high_impact_corrections = report
        .corrections
        .iter()
        .filter(|entry| is_high_impact_predicate(&entry.new_fact.predicate))
        .count();
    let risky_confidence_shifts = report
        .confidence_shifts
        .iter()
        .filter(|entry| {
            entry.to_confidence < 0.7 || (entry.from_confidence - entry.to_confidence) >= 0.2
        })
        .count();

    let urgency = if high_impact_corrections > 0 || risky_confidence_shifts > 0 {
        "high"
    } else if !report.corrections.is_empty() || !report.invalidated_facts.is_empty() {
        "medium"
    } else {
        "low"
    };
    let status = if urgency == "low" {
        "stable"
    } else {
        "attention_needed"
    };

    let mut ranked_actions: Vec<(f32, JsonValue)> = Vec::new();
    if !report.corrections.is_empty() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "verify_corrections",
            priority: if high_impact_corrections > 0 {
                "high"
            } else {
                "medium"
            },
            action: format!(
                "Review {} correction(s) and confirm current truth.",
                report.corrections.len()
            ),
            rationale: "Corrections indicate fact replacement and can affect downstream decisions."
                .to_string(),
            suggested_tool: "facts_about",
            suggested_arguments: json!({ "entity": report.entity.clone() }),
            workflow_impact: if high_impact_corrections > 0 {
                "critical_identity"
            } else {
                "decision_reliability"
            },
            risk_if_skipped: if high_impact_corrections > 0 {
                "high"
            } else {
                "medium"
            },
            automation_confidence: if high_impact_corrections > 0 {
                0.78
            } else {
                0.66
            },
        }));
    }
    if risky_confidence_shifts > 0 {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "revalidate_confidence",
            priority: "high",
            action: format!(
                "Re-validate {} confidence shift(s) with significant drop or low endpoint confidence.",
                risky_confidence_shifts
            ),
            rationale:
                "Lower-confidence replacements increase the chance of inaccurate memory-guided actions."
                    .to_string(),
            suggested_tool: "memory_health",
            suggested_arguments: json!({ "entity": report.entity.clone(), "low_confidence_threshold": 0.7, "stale_after_days": 90 }),
            workflow_impact: "decision_reliability",
            risk_if_skipped: "high",
            automation_confidence: 0.84,
        }));
    }
    if report.invalidated_facts.len() > report.new_facts.len() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "fill_invalidation_gaps",
            priority: "medium",
            action: "Check for invalidated facts that were not replaced yet.".to_string(),
            rationale: "More invalidations than additions can leave missing operational context."
                .to_string(),
            suggested_tool: "facts_about",
            suggested_arguments: json!({ "entity": report.entity.clone() }),
            workflow_impact: "context_coverage",
            risk_if_skipped: "medium",
            automation_confidence: 0.72,
        }));
    }
    if ranked_actions.is_empty() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "monitor",
            priority: "low",
            action: "No immediate intervention required; monitor next delta window.".to_string(),
            rationale: "Current changes do not imply high-risk memory drift.".to_string(),
            suggested_tool: "what_changed",
            suggested_arguments: json!({ "entity": report.entity.clone(), "since": report.since.to_rfc3339(), "predicate": report.predicate_filter.clone() }),
            workflow_impact: "monitoring",
            risk_if_skipped: "low",
            automation_confidence: 0.92,
        }));
    }

    ranked_actions.sort_by(|left, right| right.0.total_cmp(&left.0));
    let recommended_action_id = ranked_actions
        .first()
        .and_then(|(_, action)| action.get("id"))
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "monitor".to_string());
    let automation_ready = ranked_actions.iter().any(|(_, action)| {
        action
            .get("execution_mode")
            .and_then(JsonValue::as_str)
            .is_some_and(|mode| mode != "human_confirm")
    });
    let next_actions: Vec<JsonValue> = ranked_actions
        .into_iter()
        .map(|(_, action)| action)
        .collect();

    json!({
        "schema_version": AGENT_BRIEF_SCHEMA_VERSION,
        "decision_mode": "agent_first",
        "status": status,
        "urgency": urgency,
        "summary": format!(
            "{} new, {} invalidated, {} corrections, {} risky confidence shifts",
            report.new_facts.len(),
            report.invalidated_facts.len(),
            report.corrections.len(),
            risky_confidence_shifts
        ),
        "signals": {
            "new_facts": report.new_facts.len(),
            "invalidated_facts": report.invalidated_facts.len(),
            "corrections": report.corrections.len(),
            "high_impact_corrections": high_impact_corrections,
            "risky_confidence_shifts": risky_confidence_shifts
        },
        "recommended_action_id": recommended_action_id,
        "automation_ready": automation_ready,
        "next_actions": next_actions
    })
}

fn memory_health_agent_brief(report: &MemoryHealthReport) -> JsonValue {
    let low_conf_high_impact = report
        .low_confidence_facts
        .iter()
        .filter(|fact| is_high_impact_predicate(&fact.predicate))
        .count();
    let stale_predicates: Vec<String> = report
        .stale_high_impact_facts
        .iter()
        .map(|fact| fact.predicate.clone())
        .collect();

    let urgency = if report.contradiction_count > 0 || low_conf_high_impact > 0 {
        "high"
    } else if !report.low_confidence_facts.is_empty() || !report.stale_high_impact_facts.is_empty()
    {
        "medium"
    } else {
        "low"
    };
    let status = if urgency == "low" {
        "healthy"
    } else {
        "attention_needed"
    };

    let mut ranked_actions: Vec<(f32, JsonValue)> = Vec::new();
    if report.contradiction_count > 0 {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "resolve_contradictions",
            priority: "high",
            action: format!("Resolve {} contradiction(s).", report.contradiction_count),
            rationale: "Contradictions can cause agents to choose mutually inconsistent plans."
                .to_string(),
            suggested_tool: "facts_about",
            suggested_arguments: json!({
                "entity": report.entity.clone(),
            }),
            workflow_impact: "critical_identity",
            risk_if_skipped: "high",
            automation_confidence: 0.86,
        }));
    }
    if !report.low_confidence_facts.is_empty() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "verify_low_confidence",
            priority: if low_conf_high_impact > 0 {
                "high"
            } else {
                "medium"
            },
            action: format!(
                "Validate {} low-confidence active fact(s).",
                report.low_confidence_facts.len()
            ),
            rationale:
                "Low-confidence facts reduce reliability of agent decisions and tool routing."
                    .to_string(),
            suggested_tool: "facts_about",
            suggested_arguments: json!({ "entity": report.entity.clone() }),
            workflow_impact: if low_conf_high_impact > 0 {
                "critical_identity"
            } else {
                "decision_reliability"
            },
            risk_if_skipped: if low_conf_high_impact > 0 {
                "high"
            } else {
                "medium"
            },
            automation_confidence: if low_conf_high_impact > 0 { 0.80 } else { 0.68 },
        }));
    }
    if !report.stale_high_impact_facts.is_empty() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "refresh_stale_high_impact",
            priority: "medium",
            action: format!(
                "Refresh {} stale high-impact fact(s).",
                report.stale_high_impact_facts.len()
            ),
            rationale: "Stale high-impact facts often map to operationally important assumptions."
                .to_string(),
            suggested_tool: "what_changed",
            suggested_arguments: json!({
                "entity": report.entity.clone(),
                "since": (report.generated_at - KronroeSpan::days(30)).to_rfc3339(),
                "predicate": report.predicate_filter.clone()
            }),
            workflow_impact: "freshness_hygiene",
            risk_if_skipped: "medium",
            automation_confidence: 0.74,
        }));
    }
    if ranked_actions.is_empty() {
        ranked_actions.push(make_ranked_agent_action(AgentActionSpec {
            id: "monitor_health",
            priority: "low",
            action: "Memory health is stable; continue periodic checks.".to_string(),
            rationale: "No major reliability risk detected for this entity.".to_string(),
            suggested_tool: "memory_health",
            suggested_arguments: json!({
                "entity": report.entity.clone(),
                "predicate": report.predicate_filter.clone(),
                "low_confidence_threshold": 0.7,
                "stale_after_days": 90
            }),
            workflow_impact: "monitoring",
            risk_if_skipped: "low",
            automation_confidence: 0.93,
        }));
    }

    ranked_actions.sort_by(|left, right| right.0.total_cmp(&left.0));
    let recommended_action_id = ranked_actions
        .first()
        .and_then(|(_, action)| action.get("id"))
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "monitor_health".to_string());
    let automation_ready = ranked_actions.iter().any(|(_, action)| {
        action
            .get("execution_mode")
            .and_then(JsonValue::as_str)
            .is_some_and(|mode| mode != "human_confirm")
    });
    let next_actions: Vec<JsonValue> = ranked_actions
        .into_iter()
        .map(|(_, action)| action)
        .collect();

    json!({
        "schema_version": AGENT_BRIEF_SCHEMA_VERSION,
        "decision_mode": "agent_first",
        "status": status,
        "urgency": urgency,
        "summary": format!(
            "{} active / {} total; {} low-confidence; {} stale high-impact; {} contradictions",
            report.active_fact_count,
            report.total_fact_count,
            report.low_confidence_facts.len(),
            report.stale_high_impact_facts.len(),
            report.contradiction_count
        ),
        "signals": {
            "active_fact_count": report.active_fact_count,
            "total_fact_count": report.total_fact_count,
            "low_confidence_count": report.low_confidence_facts.len(),
            "low_confidence_high_impact_count": low_conf_high_impact,
            "stale_high_impact_count": report.stale_high_impact_facts.len(),
            "stale_high_impact_predicates": stale_predicates,
            "contradiction_count": report.contradiction_count
        },
        "recommended_action_id": recommended_action_id,
        "automation_ready": automation_ready,
        "next_actions": next_actions
    })
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

    #[allow(dead_code)]
    fn get_tool_schema<'a>(tools: &'a [JsonValue], name: &str) -> &'a JsonValue {
        tools
            .iter()
            .find(|tool| tool.get("name").and_then(JsonValue::as_str) == Some(name))
            .expect("tool should be present")
    }

    #[allow(dead_code)]
    fn get_recall_properties(tool: &JsonValue) -> &serde_json::Map<String, JsonValue> {
        tool.get("inputSchema")
            .and_then(|v| v.get("properties"))
            .and_then(JsonValue::as_object)
            .expect("recall tool schema properties should exist")
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
                    "limit": 10
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
        let score = results[0].get("score").expect("score should be present");
        assert_eq!(score.get("type").and_then(JsonValue::as_str), Some("text"));
        assert!(score
            .get("confidence")
            .and_then(JsonValue::as_f64)
            .is_some());
        assert!(score.get("effective_confidence").is_some());
        assert!(results[0].get("fact").is_some());
    }

    #[test]
    fn recall_scored_rejects_confidence_mode_without_threshold() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_scored",
                "arguments": {
                    "query": "alice",
                    "confidence_filter_mode": "base"
                }
            })),
        )
        .expect_err("expected confidence mode contract error")
        .to_string();
        assert!(err.contains("confidence_filter_mode requires min_confidence"));
    }

    #[test]
    fn recall_scored_accepts_confidence_mode_with_threshold() {
        let mut state = temp_state();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "works_at",
                    "object": "Acme",
                    "confidence": 0.95
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
                    "min_confidence": 0.5,
                    "confidence_filter_mode": "base"
                }
            })),
        )
        .expect("expected confidence filter happy path to succeed");

        let results = out
            .get("structuredContent")
            .and_then(|v| v.get("results"))
            .and_then(JsonValue::as_array)
            .expect("results array");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn recall_rejects_empty_embedding_array() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall",
                "arguments": {
                    "query": "alice",
                    "query_embedding": []
                }
            })),
        )
        .expect_err("expected embedding validation error")
        .to_string();
        assert!(err.contains("query_embedding must not be empty"));
    }

    #[test]
    fn recall_rejects_invalid_limit_type() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall",
                "arguments": {
                    "query": "alice",
                    "limit": "10"
                }
            })),
        )
        .expect_err("expected limit type validation error")
        .to_string();
        assert!(err.contains("limit must be an integer"));
    }

    #[test]
    fn recall_rejects_zero_limit() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall",
                "arguments": {
                    "query": "alice",
                    "limit": 0
                }
            })),
        )
        .expect_err("expected zero limit validation error")
        .to_string();
        assert!(err.contains("limit must be greater than or equal to 1"));
    }

    #[test]
    fn recall_rejects_embedding_values_outside_f32_range() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall",
                "arguments": {
                    "query": "alice",
                    "query_embedding": [1.0e40]
                }
            })),
        )
        .expect_err("expected f32 overflow validation error")
        .to_string();
        assert!(err.contains("overflow f32 range"));
    }

    #[test]
    fn assert_fact_respects_valid_from_when_storing_confidence_and_source() {
        let mut state = temp_state();
        let valid_from = "2024-01-01T00:00:00Z";
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
    fn recall_scored_embedding_defaults_to_hybrid_and_honors_toggle() {
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
        assert_eq!(off_type, "hybrid");

        let on = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_scored",
                "arguments": {
                    "query": "rust",
                    "query_embedding": [1.0, 0.0, 0.0],
                    "use_hybrid": false,
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
        assert_eq!(on_type, "text");
    }

    #[test]
    fn correct_fact_returns_new_fact_id() {
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

        let corrected = call_tool(
            &mut state,
            Some(&json!({
                "name": "correct_fact",
                "arguments": { "fact_id": fact_id, "new_value": "Globex" }
            })),
        )
        .unwrap();
        let new_fact_id = corrected
            .get("structuredContent")
            .and_then(|v| v.get("new_fact_id"))
            .and_then(JsonValue::as_str)
            .unwrap();
        assert!(new_fact_id.starts_with("kf_"));
        assert_ne!(new_fact_id, fact_id);

        // The corrected value should appear in recall
        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall",
                "arguments": { "query": "Globex", "limit": 10 }
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
    fn what_changed_tool_reports_corrections_and_agent_brief() {
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
        let first_id = first
            .get("structuredContent")
            .and_then(|v| v.get("fact_id"))
            .and_then(JsonValue::as_str)
            .expect("first fact_id");

        let since = KronroeTimestamp::now_utc().to_rfc3339();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "invalidate_fact",
                "arguments": { "fact_id": first_id }
            })),
        )
        .unwrap();

        let second = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "works_at",
                    "object": "Beta Corp",
                    "confidence": 0.6
                }
            })),
        )
        .unwrap();
        let second_id = second
            .get("structuredContent")
            .and_then(|v| v.get("fact_id"))
            .and_then(JsonValue::as_str)
            .expect("second fact_id");

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "what_changed",
                "arguments": {
                    "entity": "alice",
                    "since": since,
                    "predicate": "works_at"
                }
            })),
        )
        .unwrap();

        let report = out
            .get("structuredContent")
            .and_then(|v| v.get("report"))
            .expect("report should be present");
        assert_eq!(
            report
                .get("new_facts")
                .and_then(JsonValue::as_array)
                .map_or(0, Vec::len),
            1
        );
        assert_eq!(
            report
                .get("invalidated_facts")
                .and_then(JsonValue::as_array)
                .map_or(0, Vec::len),
            1
        );
        let corrections = report
            .get("corrections")
            .and_then(JsonValue::as_array)
            .expect("corrections array");
        assert_eq!(corrections.len(), 1);
        assert_eq!(
            corrections[0]
                .get("old_fact")
                .and_then(|v| v.get("id"))
                .and_then(JsonValue::as_str),
            Some(first_id)
        );
        assert_eq!(
            corrections[0]
                .get("new_fact")
                .and_then(|v| v.get("id"))
                .and_then(JsonValue::as_str),
            Some(second_id)
        );

        let agent_brief = out
            .get("structuredContent")
            .and_then(|v| v.get("agent_brief"))
            .expect("agent_brief should be present");
        assert_eq!(
            agent_brief
                .get("schema_version")
                .and_then(JsonValue::as_str),
            Some("1.0")
        );
        assert_eq!(
            agent_brief.get("decision_mode").and_then(JsonValue::as_str),
            Some("agent_first")
        );
        assert_eq!(
            agent_brief
                .get("recommended_action_id")
                .and_then(JsonValue::as_str),
            Some("verify_corrections")
        );
        assert_eq!(
            agent_brief
                .get("automation_ready")
                .and_then(JsonValue::as_bool),
            Some(true)
        );
    }

    #[test]
    fn what_changed_tool_rejects_invalid_since() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "what_changed",
                "arguments": {
                    "entity": "alice",
                    "since": "not-a-date"
                }
            })),
        )
        .expect_err("expected invalid since error")
        .to_string();
        assert!(err.contains("since must be RFC3339"));
    }

    #[test]
    fn memory_health_tool_reports_low_confidence_and_stale() {
        let mut state = temp_state();
        let old = (KronroeTimestamp::now_utc() - KronroeSpan::days(200)).to_rfc3339();

        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "nickname",
                    "object": "Bex",
                    "confidence": 0.4,
                    "valid_from": old
                }
            })),
        )
        .unwrap();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "email",
                    "object": "alice@example.com",
                    "confidence": 0.9,
                    "valid_from": old
                }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "memory_health",
                "arguments": {
                    "entity": "alice",
                    "low_confidence_threshold": 0.7,
                    "stale_after_days": 90
                }
            })),
        )
        .unwrap();

        let report = out
            .get("structuredContent")
            .and_then(|v| v.get("report"))
            .expect("report should be present");
        assert_eq!(
            report
                .get("total_fact_count")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0),
            2
        );
        assert_eq!(
            report
                .get("low_confidence_facts")
                .and_then(JsonValue::as_array)
                .map_or(0, Vec::len),
            1
        );
        assert_eq!(
            report
                .get("stale_high_impact_facts")
                .and_then(JsonValue::as_array)
                .map_or(0, Vec::len),
            1
        );

        let agent_brief = out
            .get("structuredContent")
            .and_then(|v| v.get("agent_brief"))
            .expect("agent_brief should be present");
        assert_eq!(
            agent_brief
                .get("schema_version")
                .and_then(JsonValue::as_str),
            Some("1.0")
        );
        assert_eq!(
            agent_brief.get("status").and_then(JsonValue::as_str),
            Some("attention_needed")
        );
        assert_eq!(
            agent_brief
                .get("recommended_action_id")
                .and_then(JsonValue::as_str),
            Some("verify_low_confidence")
        );
    }

    #[test]
    fn recall_for_task_agent_brief_without_subject_avoids_null_entity_actions() {
        let brief = recall_for_task_agent_brief(&RecallForTaskReport {
            task: "prepare renewal call".to_string(),
            subject: None,
            generated_at: KronroeTimestamp::now_utc(),
            horizon_days: 90,
            query_used: "prepare renewal call".to_string(),
            key_facts: vec![Fact::new(
                "alice",
                "project",
                "Renewal Q2",
                KronroeTimestamp::now_utc(),
            )],
            low_confidence_count: 1,
            stale_high_impact_count: 1,
            contradiction_count: 0,
            watchouts: vec!["1 key fact is low confidence".to_string()],
            recommended_next_checks: vec!["Verify confidence".to_string()],
        });

        let actions = brief
            .get("next_actions")
            .and_then(JsonValue::as_array)
            .expect("next_actions array expected");
        assert!(
            actions.iter().all(|action| {
                action
                    .get("suggested_arguments")
                    .and_then(|args| args.get("entity"))
                    .map(|entity| !entity.is_null())
                    .unwrap_or(true)
            }),
            "subjectless task briefs should not emit null entity tool arguments"
        );
    }

    #[test]
    fn memory_health_agent_brief_contradictions_point_to_entity_facts() {
        let brief = memory_health_agent_brief(&MemoryHealthReport {
            entity: "alice".to_string(),
            generated_at: KronroeTimestamp::now_utc(),
            predicate_filter: None,
            total_fact_count: 2,
            active_fact_count: 2,
            low_confidence_facts: Vec::new(),
            stale_high_impact_facts: Vec::new(),
            contradiction_count: 1,
            recommended_actions: vec!["Resolve contradiction".to_string()],
        });

        let actions = brief
            .get("next_actions")
            .and_then(JsonValue::as_array)
            .expect("next_actions array expected");
        let contradiction_action = actions
            .iter()
            .find(|action| {
                action.get("id").and_then(JsonValue::as_str) == Some("resolve_contradictions")
            })
            .expect("resolve_contradictions action");
        assert_eq!(
            contradiction_action
                .get("suggested_tool")
                .and_then(JsonValue::as_str),
            Some("facts_about")
        );
        assert_eq!(
            contradiction_action
                .get("suggested_arguments")
                .and_then(|args| args.get("entity"))
                .and_then(JsonValue::as_str),
            Some("alice")
        );
    }

    #[test]
    fn memory_health_tool_rejects_invalid_threshold() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "memory_health",
                "arguments": {
                    "entity": "alice",
                    "low_confidence_threshold": 1.5
                }
            })),
        )
        .expect_err("expected threshold validation error")
        .to_string();
        assert!(err.contains("between 0.0 and 1.0"));
    }

    #[test]
    fn recall_for_task_tool_returns_decision_ready_report() {
        let mut state = temp_state();
        let old = (KronroeTimestamp::now_utc() - KronroeSpan::days(200)).to_rfc3339();

        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "works_at",
                    "object": "Acme",
                    "confidence": 0.65,
                    "valid_from": old
                }
            })),
        )
        .unwrap();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "project",
                    "object": "Renewal Q2",
                    "confidence": 0.95
                }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_for_task",
                "arguments": {
                    "task": "prepare renewal call",
                    "subject": "alice",
                    "horizon_days": 90,
                    "limit": 10
                }
            })),
        )
        .unwrap();

        let report = out
            .get("structuredContent")
            .and_then(|v| v.get("report"))
            .expect("report should be present");
        let key_facts = report
            .get("key_facts")
            .and_then(JsonValue::as_array)
            .expect("key_facts should be present");
        assert!(!key_facts.is_empty(), "expected key facts for task");
        assert!(
            key_facts.iter().all(|fact| {
                fact.get("subject")
                    .and_then(JsonValue::as_str)
                    .is_some_and(|subject| subject == "alice")
            }),
            "task report should stay focused on requested subject"
        );
        assert!(
            report
                .get("low_confidence_count")
                .and_then(JsonValue::as_u64)
                .is_some_and(|value| value >= 1),
            "expected at least one low-confidence watchout"
        );

        let brief = out
            .get("structuredContent")
            .and_then(|v| v.get("agent_brief"))
            .expect("agent_brief should be present");
        assert_eq!(
            brief.get("decision_mode").and_then(JsonValue::as_str),
            Some("agent_first")
        );
        assert!(
            brief
                .get("recommended_action_id")
                .and_then(JsonValue::as_str)
                .is_some(),
            "agent brief should include recommended action"
        );
    }

    #[test]
    fn recall_for_task_tool_rejects_zero_limit() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_for_task",
                "arguments": {
                    "task": "prepare renewal call",
                    "limit": 0
                }
            })),
        )
        .expect_err("expected limit validation error")
        .to_string();
        assert!(err.contains("greater than or equal to 1"));
    }

    #[cfg(not(feature = "hybrid"))]
    #[test]
    fn recall_for_task_tool_rejects_hybrid_controls_without_feature() {
        let mut state = temp_state();
        let err = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_for_task",
                "arguments": {
                    "task": "prepare renewal call",
                    "query_embedding": [0.1, 0.2],
                    "use_hybrid": true
                }
            })),
        )
        .expect_err("expected hybrid gating error")
        .to_string();
        assert!(err.contains("hybrid task recall controls are unavailable"));
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

    #[cfg(not(feature = "hybrid"))]
    #[test]
    fn tools_schema_omits_hybrid_fields_without_feature() {
        let tools = tools_schema();
        let recall = get_tool_schema(&tools, "recall");
        let recall_props = get_recall_properties(recall);
        assert!(
            !recall_props.contains_key("query_embedding"),
            "default build should not advertise query_embedding"
        );
        assert!(
            !recall_props.contains_key("use_hybrid"),
            "default build should not advertise use_hybrid"
        );
        assert!(
            !recall_props.contains_key("temporal_intent"),
            "default build should not advertise temporal_intent"
        );
        assert!(
            !recall_props.contains_key("temporal_operator"),
            "default build should not advertise temporal_operator"
        );
    }

    #[cfg(not(feature = "uncertainty"))]
    #[test]
    fn tools_schema_omits_effective_confidence_mode_without_feature() {
        let tools = tools_schema();
        let recall_scored = get_tool_schema(&tools, "recall_scored");
        let recall_props = get_recall_properties(recall_scored);
        let modes = recall_props
            .get("confidence_filter_mode")
            .and_then(|v| v.get("enum"))
            .and_then(JsonValue::as_array)
            .expect("confidence_filter_mode enum");
        assert_eq!(modes, &vec![json!("base")]);
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

    // ── Orchestration eval: Gate A3 ─────────────────────────────────────
    //
    // Validates that agent_brief recommended actions suggest the correct
    // next tool for every reachable memory state. 100% top-1 accuracy
    // required — every scenario must match.

    fn extract_agent_brief(out: &JsonValue) -> (&str, &str) {
        let brief = out
            .get("structuredContent")
            .and_then(|v| v.get("agent_brief"))
            .expect("agent_brief should be present");
        let action_id = brief
            .get("recommended_action_id")
            .and_then(JsonValue::as_str)
            .expect("recommended_action_id should be present");
        let actions = brief
            .get("next_actions")
            .and_then(JsonValue::as_array)
            .expect("next_actions should be present");
        let top_tool = actions
            .first()
            .and_then(|a| a.get("suggested_tool"))
            .and_then(JsonValue::as_str)
            .expect("top action should have suggested_tool");
        (action_id, top_tool)
    }

    // ── recall_for_task scenarios ────────────────────────────────────────

    #[test]
    fn orch_recall_for_task_no_context_suggests_recall_for_task() {
        let mut state = temp_state();
        // No facts seeded — empty context
        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_for_task",
                "arguments": {
                    "task": "prepare renewal call",
                    "subject": "unknown_entity",
                    "horizon_days": 90,
                    "limit": 8
                }
            })),
        )
        .unwrap();
        let (action_id, tool) = extract_agent_brief(&out);
        assert_eq!(action_id, "ask_clarifying_question");
        assert_eq!(tool, "recall_for_task");
    }

    #[test]
    fn orch_recall_for_task_low_confidence_suggests_memory_health() {
        let mut state = temp_state();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "works_at",
                    "object": "Acme",
                    "confidence": 0.3
                }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_for_task",
                "arguments": {
                    "task": "prepare renewal call",
                    "subject": "alice",
                    "horizon_days": 90,
                    "limit": 8
                }
            })),
        )
        .unwrap();
        let (action_id, tool) = extract_agent_brief(&out);
        assert_eq!(action_id, "verify_low_confidence_facts");
        assert_eq!(tool, "memory_health");
    }

    #[test]
    fn orch_recall_for_task_stale_high_impact_includes_refresh_action() {
        let mut state = temp_state();
        let old = (KronroeTimestamp::now_utc() - KronroeSpan::days(200)).to_rfc3339();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "works_at",
                    "object": "Acme",
                    "confidence": 0.95,
                    "valid_from": old
                }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_for_task",
                "arguments": {
                    "task": "prepare renewal call",
                    "subject": "alice",
                    "horizon_days": 90,
                    "limit": 8
                }
            })),
        )
        .unwrap();
        // When stale facts exist but key_facts is non-empty, prepare_task_brief
        // wins top-1 (decision_reliability > freshness_hygiene in scoring).
        // The refresh action must still appear in the ranked action list.
        let (action_id, tool) = extract_agent_brief(&out);
        assert_eq!(action_id, "prepare_task_brief");
        assert_eq!(tool, "assemble_context");
        let brief = out
            .get("structuredContent")
            .and_then(|v| v.get("agent_brief"))
            .unwrap();
        let actions = brief["next_actions"].as_array().unwrap();
        let has_refresh = actions.iter().any(|a| {
            a.get("id").and_then(JsonValue::as_str) == Some("refresh_stale_high_impact_facts")
        });
        assert!(
            has_refresh,
            "refresh_stale_high_impact_facts should be in actions"
        );
    }

    #[test]
    fn orch_recall_for_task_good_facts_suggests_assemble_context() {
        let mut state = temp_state();
        // Fresh, high-confidence fact — no watchouts
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "project",
                    "object": "Renewal Q2",
                    "confidence": 0.95
                }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "recall_for_task",
                "arguments": {
                    "task": "prepare renewal call",
                    "subject": "alice",
                    "horizon_days": 90,
                    "limit": 8
                }
            })),
        )
        .unwrap();
        let (action_id, tool) = extract_agent_brief(&out);
        assert_eq!(action_id, "prepare_task_brief");
        assert_eq!(tool, "assemble_context");
    }

    #[test]
    fn orch_recall_for_task_contradictions_suggests_memory_health() {
        // Test the agent_brief routing directly — contradictions present
        // means resolve_contradictions_first should win top-1
        let brief = recall_for_task_agent_brief(&RecallForTaskReport {
            task: "prepare renewal call".to_string(),
            subject: Some("alice".to_string()),
            generated_at: KronroeTimestamp::now_utc(),
            horizon_days: 90,
            query_used: "prepare renewal call".to_string(),
            key_facts: vec![Fact::new(
                "alice",
                "works_at",
                "Acme",
                KronroeTimestamp::now_utc(),
            )],
            low_confidence_count: 0,
            stale_high_impact_count: 0,
            contradiction_count: 2,
            watchouts: vec!["2 contradictions".to_string()],
            recommended_next_checks: vec!["Resolve contradictions".to_string()],
        });
        let action_id = brief["recommended_action_id"].as_str().unwrap();
        let top_tool = brief["next_actions"][0]["suggested_tool"].as_str().unwrap();
        assert_eq!(action_id, "resolve_contradictions_first");
        assert_eq!(top_tool, "memory_health");
    }

    // ── what_changed scenarios ───────────────────────────────────────────

    #[test]
    fn orch_what_changed_corrections_suggests_facts_about() {
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
        let fact_id = first["structuredContent"]["fact_id"]
            .as_str()
            .expect("fact_id");

        // Brief sleep to ensure since timestamp is strictly after the assertion
        std::thread::sleep(std::time::Duration::from_millis(2));
        let since = KronroeTimestamp::now_utc().to_rfc3339();

        // Correct the fact — creates a correction event
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "correct_fact",
                "arguments": { "fact_id": fact_id, "new_value": "Globex" }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "what_changed",
                "arguments": {
                    "entity": "alice",
                    "since": since,
                    "predicate": "works_at"
                }
            })),
        )
        .unwrap();
        let (action_id, tool) = extract_agent_brief(&out);
        assert_eq!(action_id, "verify_corrections");
        assert_eq!(tool, "facts_about");
    }

    #[test]
    fn orch_what_changed_risky_confidence_shift_suggests_memory_health() {
        // Test the agent_brief routing directly — risky confidence shifts
        // (drop ≥ 0.2) without corrections should produce revalidate_confidence
        let brief = what_changed_agent_brief(&WhatChangedReport {
            entity: "alice".to_string(),
            since: KronroeTimestamp::now_utc() - KronroeSpan::days(7),
            predicate_filter: None,
            new_facts: vec![
                Fact::new("alice", "works_at", "Globex", KronroeTimestamp::now_utc())
                    .with_confidence(0.5),
            ],
            invalidated_facts: Vec::new(),
            corrections: Vec::new(),
            confidence_shifts: vec![ConfidenceShift {
                from_fact_id: FactId::new(),
                to_fact_id: FactId::new(),
                from_confidence: 0.95,
                to_confidence: 0.5,
            }],
        });
        let action_id = brief["recommended_action_id"].as_str().unwrap();
        let top_tool = brief["next_actions"][0]["suggested_tool"].as_str().unwrap();
        assert_eq!(action_id, "revalidate_confidence");
        assert_eq!(top_tool, "memory_health");
    }

    #[test]
    fn orch_what_changed_invalidation_gaps_suggests_facts_about() {
        let mut state = temp_state();
        let f1 = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "email",
                    "object": "alice@example.com"
                }
            })),
        )
        .unwrap();
        let f2 = call_tool(
            &mut state,
            Some(&json!({
                "name": "assert_fact",
                "arguments": {
                    "subject": "alice",
                    "predicate": "phone",
                    "object": "+44123456"
                }
            })),
        )
        .unwrap();
        let id1 = f1["structuredContent"]["fact_id"].as_str().expect("id1");
        let id2 = f2["structuredContent"]["fact_id"].as_str().expect("id2");

        let since = KronroeTimestamp::now_utc().to_rfc3339();

        // Invalidate both without replacement → more invalidations than new facts
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "invalidate_fact",
                "arguments": { "fact_id": id1 }
            })),
        )
        .unwrap();
        let _ = call_tool(
            &mut state,
            Some(&json!({
                "name": "invalidate_fact",
                "arguments": { "fact_id": id2 }
            })),
        )
        .unwrap();

        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "what_changed",
                "arguments": { "entity": "alice", "since": since }
            })),
        )
        .unwrap();
        let (action_id, tool) = extract_agent_brief(&out);
        assert_eq!(action_id, "fill_invalidation_gaps");
        assert_eq!(tool, "facts_about");
    }

    #[test]
    fn orch_what_changed_stable_suggests_what_changed() {
        let mut state = temp_state();
        let _ = call_tool(
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

        // since is after assertion — nothing changed
        let since = KronroeTimestamp::now_utc().to_rfc3339();
        let out = call_tool(
            &mut state,
            Some(&json!({
                "name": "what_changed",
                "arguments": { "entity": "alice", "since": since }
            })),
        )
        .unwrap();
        let (action_id, tool) = extract_agent_brief(&out);
        assert_eq!(action_id, "monitor");
        assert_eq!(tool, "what_changed");
    }

    // ── memory_health scenarios ──────────────────────────────────────────

    #[test]
    fn orch_memory_health_contradictions_suggests_facts_about() {
        let brief = memory_health_agent_brief(&MemoryHealthReport {
            entity: "alice".to_string(),
            generated_at: KronroeTimestamp::now_utc(),
            predicate_filter: None,
            total_fact_count: 3,
            active_fact_count: 3,
            low_confidence_facts: Vec::new(),
            stale_high_impact_facts: Vec::new(),
            contradiction_count: 2,
            recommended_actions: vec!["Resolve contradictions".to_string()],
        });
        let action_id = brief["recommended_action_id"].as_str().unwrap();
        let top_tool = brief["next_actions"][0]["suggested_tool"].as_str().unwrap();
        assert_eq!(action_id, "resolve_contradictions");
        assert_eq!(top_tool, "facts_about");
    }

    #[test]
    fn orch_memory_health_low_confidence_high_impact_suggests_facts_about() {
        let low_conf_fact = Fact::new("alice", "works_at", "Acme", KronroeTimestamp::now_utc())
            .with_confidence(0.3);

        let brief = memory_health_agent_brief(&MemoryHealthReport {
            entity: "alice".to_string(),
            generated_at: KronroeTimestamp::now_utc(),
            predicate_filter: None,
            total_fact_count: 1,
            active_fact_count: 1,
            low_confidence_facts: vec![low_conf_fact],
            stale_high_impact_facts: Vec::new(),
            contradiction_count: 0,
            recommended_actions: Vec::new(),
        });
        let action_id = brief["recommended_action_id"].as_str().unwrap();
        let top_tool = brief["next_actions"][0]["suggested_tool"].as_str().unwrap();
        assert_eq!(action_id, "verify_low_confidence");
        assert_eq!(top_tool, "facts_about");
    }

    #[test]
    fn orch_memory_health_low_confidence_non_high_impact_suggests_facts_about() {
        let low_conf_fact =
            Fact::new("alice", "nickname", "Bex", KronroeTimestamp::now_utc()).with_confidence(0.4);

        let brief = memory_health_agent_brief(&MemoryHealthReport {
            entity: "alice".to_string(),
            generated_at: KronroeTimestamp::now_utc(),
            predicate_filter: None,
            total_fact_count: 1,
            active_fact_count: 1,
            low_confidence_facts: vec![low_conf_fact],
            stale_high_impact_facts: Vec::new(),
            contradiction_count: 0,
            recommended_actions: Vec::new(),
        });
        let action_id = brief["recommended_action_id"].as_str().unwrap();
        let top_tool = brief["next_actions"][0]["suggested_tool"].as_str().unwrap();
        assert_eq!(action_id, "verify_low_confidence");
        assert_eq!(top_tool, "facts_about");
    }

    #[test]
    fn orch_memory_health_stale_high_impact_suggests_what_changed() {
        let stale_fact = Fact::new(
            "alice",
            "works_at",
            "Acme",
            KronroeTimestamp::now_utc() - KronroeSpan::days(200),
        );

        let brief = memory_health_agent_brief(&MemoryHealthReport {
            entity: "alice".to_string(),
            generated_at: KronroeTimestamp::now_utc(),
            predicate_filter: None,
            total_fact_count: 1,
            active_fact_count: 1,
            low_confidence_facts: Vec::new(),
            stale_high_impact_facts: vec![stale_fact],
            contradiction_count: 0,
            recommended_actions: Vec::new(),
        });
        let action_id = brief["recommended_action_id"].as_str().unwrap();
        let top_tool = brief["next_actions"][0]["suggested_tool"].as_str().unwrap();
        assert_eq!(action_id, "refresh_stale_high_impact");
        assert_eq!(top_tool, "what_changed");
    }

    #[test]
    fn orch_memory_health_healthy_suggests_memory_health() {
        let brief = memory_health_agent_brief(&MemoryHealthReport {
            entity: "alice".to_string(),
            generated_at: KronroeTimestamp::now_utc(),
            predicate_filter: None,
            total_fact_count: 2,
            active_fact_count: 2,
            low_confidence_facts: Vec::new(),
            stale_high_impact_facts: Vec::new(),
            contradiction_count: 0,
            recommended_actions: Vec::new(),
        });
        let action_id = brief["recommended_action_id"].as_str().unwrap();
        let top_tool = brief["next_actions"][0]["suggested_tool"].as_str().unwrap();
        assert_eq!(action_id, "monitor_health");
        assert_eq!(top_tool, "memory_health");
    }

    #[test]
    fn orch_memory_health_contradictions_win_over_low_confidence() {
        let low_conf_fact =
            Fact::new("alice", "nickname", "Bex", KronroeTimestamp::now_utc()).with_confidence(0.3);

        let brief = memory_health_agent_brief(&MemoryHealthReport {
            entity: "alice".to_string(),
            generated_at: KronroeTimestamp::now_utc(),
            predicate_filter: None,
            total_fact_count: 3,
            active_fact_count: 3,
            low_confidence_facts: vec![low_conf_fact],
            stale_high_impact_facts: Vec::new(),
            contradiction_count: 1,
            recommended_actions: vec!["Resolve contradiction".to_string()],
        });
        let action_id = brief["recommended_action_id"].as_str().unwrap();
        let top_tool = brief["next_actions"][0]["suggested_tool"].as_str().unwrap();
        // Contradictions score higher (critical_identity + high risk) than
        // low confidence non-high-impact (decision_reliability + medium risk)
        assert_eq!(action_id, "resolve_contradictions");
        assert_eq!(top_tool, "facts_about");
    }
}
