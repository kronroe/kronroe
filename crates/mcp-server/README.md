# kronroe-mcp

Native MCP server wrapping Kronroe `AgentMemory`.

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
- `recall(query, limit?)`
- `facts_about(entity)`
- `assert_fact(subject, predicate, object, valid_from?)`
- `correct_fact(fact_id, new_value)`

## Claude Desktop config snippet

```json
{
  "mcpServers": {
    "kronroe": {
      "command": "cargo",
      "args": ["run", "-p", "kronroe-mcp"],
      "cwd": "/Users/rebekahcole/kronroe",
      "env": {
        "KRONROE_MCP_DB_PATH": "/Users/rebekahcole/kronroe/.data/kronroe-mcp.kronroe"
      }
    }
  }
}
```

For production usage, replace `cargo run` with a compiled `kronroe-mcp` binary.
