use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use kronroe::{FactId, TemporalGraph, Value};
use serde_json::{json, Value as JsonValue};
use std::env;
use std::io::{self, BufRead, BufReader, Write};

struct AppState {
    graph: TemporalGraph,
}

impl AppState {
    fn open() -> Result<Self> {
        let db_path =
            env::var("KRONROE_MCP_DB_PATH").unwrap_or_else(|_| "./kronroe-mcp.kronroe".to_string());
        let graph = TemporalGraph::open(&db_path)?;
        Ok(Self { graph })
    }
}

fn main() -> Result<()> {
    let mut state = AppState::open().context("failed to open kronroe database")?;
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    loop {
        let maybe = read_message(&mut reader)?;
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

    match method {
        "initialize" => id.map(|id_val| {
            json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "kronroe-mcp", "version": env!("CARGO_PKG_VERSION") }
                }
            })
        }),
        "notifications/initialized" => None,
        "tools/list" => id.map(|id_val| {
            json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "result": {
                    "tools": tools_schema()
                }
            })
        }),
        "tools/call" => id.map(|id_val| {
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
        }),
        "ping" => id.map(|id_val| json!({ "jsonrpc": "2.0", "id": id_val, "result": {} })),
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
                    "episode_id": {"type": "string"}
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
                    "limit": {"type": "integer", "minimum": 1}
                },
                "required": ["query"]
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
                    "valid_from": {"type": "string"}
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
        "remember" => {
            let text = args
                .get("text")
                .and_then(JsonValue::as_str)
                .context("text is required")?;
            let episode_id = args
                .get("episode_id")
                .and_then(JsonValue::as_str)
                .unwrap_or("default");

            // Phase 0 extraction heuristic: store raw episode text, and if pattern
            // "<subject> works at <object>" exists, assert a structured fact too.
            let mut ids = vec![
                state
                    .graph
                    .assert_fact(episode_id, "note", text.to_string(), Utc::now())?
                    .0,
            ];
            if let Some((subject, employer)) = parse_works_at(text) {
                ids.push(
                    state
                        .graph
                        .assert_fact(subject, "works_at", employer.to_string(), Utc::now())?
                        .0,
                );
            }

            Ok(json!({
                "content": [{ "type": "text", "text": format!("stored {} fact(s)", ids.len()) }],
                "structuredContent": { "fact_ids": ids }
            }))
        }
        "recall" => {
            let query = args
                .get("query")
                .and_then(JsonValue::as_str)
                .context("query is required")?;
            let limit = args.get("limit").and_then(JsonValue::as_u64).unwrap_or(10) as usize;
            let facts = state.graph.search(query, limit)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("found {} fact(s)", facts.len()) }],
                "structuredContent": { "facts": facts }
            }))
        }
        "facts_about" => {
            let entity = args
                .get("entity")
                .and_then(JsonValue::as_str)
                .context("entity is required")?;
            let facts = state.graph.all_facts_about(entity)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("{} fact(s) about {entity}", facts.len()) }],
                "structuredContent": { "facts": facts }
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
            let object = json_to_value(args.get("object").context("object is required")?);
            let valid_from = parse_valid_from(args.get("valid_from"))?;
            let fact_id = state
                .graph
                .assert_fact(subject, predicate, object, valid_from)?;
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
            let new_value = json_to_value(args.get("new_value").context("new_value is required")?);
            let new_id =
                state
                    .graph
                    .correct_fact(&FactId(fact_id.to_string()), new_value, Utc::now())?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("corrected fact {fact_id} -> {}", new_id.0) }],
                "structuredContent": { "new_fact_id": new_id.0 }
            }))
        }
        _ => anyhow::bail!("unknown tool: {name}"),
    }
}

fn parse_works_at(text: &str) -> Option<(&str, &str)> {
    let lower = text.to_lowercase();
    let idx = lower.find(" works at ")?;
    let subject = text.get(0..idx)?.trim();
    let employer = text.get(idx + " works at ".len()..)?.trim();
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

fn json_to_value(v: &JsonValue) -> Value {
    match v {
        JsonValue::Bool(b) => Value::Boolean(*b),
        JsonValue::Number(n) => n
            .as_f64()
            .map(Value::Number)
            .unwrap_or_else(|| Value::Text(n.to_string())),
        JsonValue::String(s) => Value::Text(s.clone()),
        _ => Value::Text(v.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn temp_state() -> AppState {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_string_lossy().to_string();
        AppState {
            graph: TemporalGraph::open(&path).unwrap(),
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
}
