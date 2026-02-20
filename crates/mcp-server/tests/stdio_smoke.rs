use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;

fn write_mcp_message(stdin: &mut impl Write, payload: &Value) {
    let body = serde_json::to_vec(payload).unwrap();
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
    stdin.write_all(&body).unwrap();
    stdin.flush().unwrap();
}

fn read_mcp_message(stdout: &mut impl BufRead) -> Value {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = stdout.read_line(&mut line).unwrap();
        assert!(n > 0, "unexpected EOF");
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                content_length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
    }
    let len = content_length.expect("missing Content-Length");
    let mut buf = vec![0_u8; len];
    stdout.read_exact(&mut buf).unwrap();
    serde_json::from_slice(&buf).unwrap()
}

#[test]
fn stdio_server_remember_and_recall() {
    let db = NamedTempFile::new().unwrap();
    let bin = env!("CARGO_BIN_EXE_kronroe-mcp");
    let mut child = Command::new(bin)
        .env("KRONROE_MCP_DB_PATH", db.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    write_mcp_message(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let init = read_mcp_message(&mut stdout);
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["serverInfo"]["name"], "kronroe-mcp");

    write_mcp_message(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "remember",
                "arguments": { "text": "alice works at Acme" }
            }
        }),
    );
    let remember = read_mcp_message(&mut stdout);
    assert_eq!(remember["id"], 2);
    assert!(remember["result"]["structuredContent"]["fact_ids"].is_array());

    write_mcp_message(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "recall",
                "arguments": { "query": "alice works at", "limit": 10 }
            }
        }),
    );
    let recall = read_mcp_message(&mut stdout);
    assert_eq!(recall["id"], 3);
    let facts = recall["result"]["structuredContent"]["facts"]
        .as_array()
        .unwrap();
    assert!(!facts.is_empty());

    // Stop child cleanly.
    drop(stdin);
    let _ = child.wait();
}
