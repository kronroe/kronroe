# Quick Start: MCP Server

Kronroe's MCP server gives Claude Desktop, Cursor, and any MCP-compatible AI assistant persistent, bi-temporal memory. It wraps the `AgentMemory` API over a stdio transport with LSP-style `Content-Length` framing -- no separate database server required. All memory is stored in a single local file.

## Installation

Choose one of three installation methods:

### Option 1: npm (npx)

```bash
npx kronroe-mcp
```

This delegates to the `kronroe-mcp` binary on your PATH.

### Option 2: pip

```bash
pip install kronroe-mcp
```

Then run:

```bash
kronroe-mcp
```

Set `KRONROE_MCP_BIN` to point at a custom binary location if needed.

### Option 3: Cargo (build from source)

```bash
cargo install --path crates/mcp-server
```

## Claude Desktop Configuration

Add the following to your Claude Desktop MCP configuration file (`claude_desktop_config.json`):

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

## Cursor Configuration

Add Kronroe as an MCP server in Cursor's settings:

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

## Database Path Configuration

The server stores all memory in a single file. Configure the path with the `KRONROE_MCP_DB_PATH` environment variable:

```bash
export KRONROE_MCP_DB_PATH=/path/to/memory.kronroe
```

If unset, the server defaults to `./kronroe-mcp.kronroe` in the current working directory.

## Basic Usage

Once configured, the MCP server exposes 11 tools to your AI assistant. Here is a typical workflow:

### Storing memory with `remember`

Tell your assistant something and it can store it:

```
User: "Alice works at Acme Corp as a senior engineer."
```

The assistant calls `remember` with:

```json
{
  "text": "Alice works at Acme Corp as a senior engineer."
}
```

The server parses the text, extracts facts, and stores them with full bi-temporal metadata.

### Retrieving memory with `recall`

Later, ask your assistant a question and it searches memory:

```
User: "Where does Alice work?"
```

The assistant calls `recall` with:

```json
{
  "query": "where does Alice work",
  "limit": 5
}
```

The server performs a full-text search across all stored facts and returns matching results ranked by relevance.

### Structured assertions with `assert_fact`

For precise, structured facts:

```json
{
  "subject": "alice",
  "predicate": "works_at",
  "object": "Acme Corp",
  "confidence": 0.95,
  "source": "user_statement"
}
```

### Checking what you know with `facts_about`

Retrieve all current facts about an entity:

```json
{
  "entity": "alice"
}
```

## Available Tools

The server exposes 11 tools: `remember`, `recall`, `recall_scored`, `assemble_context`, `facts_about`, `assert_fact`, `correct_fact`, `invalidate_fact`, `what_changed`, `memory_health`, and `recall_for_task`. See the [MCP Tools Reference](/api/mcp-tools) for full parameter documentation.
