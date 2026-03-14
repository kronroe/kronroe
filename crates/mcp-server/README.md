# kronroe-mcp

MCP (Model Context Protocol) server for Kronroe temporal graph memory. Gives Claude Desktop, Cursor, and any MCP-compatible AI assistant persistent, bi-temporal memory.

Built on `kronroe-agent-memory`. No separate database server required — memory is stored in a single file.

## Run locally

```bash
cargo run -p kronroe-mcp
```

The server communicates over stdio (MCP framing). Database path defaults to:

`./kronroe-mcp.kronroe`

Override with:

```bash
export KRONROE_MCP_DB_PATH=/path/to/memory.kronroe
```

## Tools

- `remember(text, episode_id?)`
- `recall(query, limit? <= 200)`
- `recall_scored(query, limit? <= 200, min_confidence?)`
- `assemble_context(query, max_tokens?)`
- `facts_about(entity)`
- `assert_fact(subject, predicate, object, valid_from?, confidence?, source?, idempotency_key?)`
- `correct_fact(fact_id, new_value)`
- `invalidate_fact(fact_id)`

## Claude Desktop config snippet

```json
{
  "mcpServers": {
    "kronroe": {
      "command": "kronroe-mcp",
      "env": {
        "KRONROE_MCP_DB_PATH": "~/.kronroe/memory.kronroe"
      }
    }
  }
}
```

## Distribution wrappers

- npm wrapper (`npx kronroe-mcp`): `packages/kronroe-mcp/`
- Python wrapper (`pip install kronroe-mcp`): `python/kronroe-mcp/`
