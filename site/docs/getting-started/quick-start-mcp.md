# Quick Start: MCP Server

Kronroe's MCP server gives Claude Desktop, Cursor, and any MCP-compatible AI assistant persistent, bi-temporal memory. It wraps the `AgentMemory` API over a stdio transport with LSP-style `Content-Length` framing -- no separate database server required. All memory is stored in a single local file.

## Installation

Choose one of three installation methods:

<div class="docs-tabs" data-docs-tabs>
  <div class="docs-tabs-list" role="tablist" aria-label="MCP installation methods">
    <button class="docs-tab" role="tab" id="mcp-npm-tab" aria-controls="mcp-npm-panel" aria-selected="true">npm (npx)</button>
    <button class="docs-tab" role="tab" id="mcp-pip-tab" aria-controls="mcp-pip-panel" aria-selected="false" tabindex="-1">pip</button>
    <button class="docs-tab" role="tab" id="mcp-cargo-tab" aria-controls="mcp-cargo-panel" aria-selected="false" tabindex="-1">Cargo</button>
  </div>
  <div class="docs-tab-panels">
    <div class="docs-tab-panel" role="tabpanel" id="mcp-npm-panel" aria-labelledby="mcp-npm-tab">
      <p class="docs-tab-note">Fastest route if you already use Node.js tooling.</p>
      <pre><code class="language-bash">npx kronroe-mcp</code></pre>
      <p class="docs-tab-note">This delegates to the `kronroe-mcp` binary on your PATH.</p>
    </div>
    <div class="docs-tab-panel" role="tabpanel" id="mcp-pip-panel" aria-labelledby="mcp-pip-tab" hidden>
      <p class="docs-tab-note">Use this if your workflow already centers on Python.</p>
      <pre><code class="language-bash">pip install kronroe-mcp
kronroe-mcp</code></pre>
      <p class="docs-tab-note">Set `KRONROE_MCP_BIN` to point at a custom binary location if needed.</p>
    </div>
    <div class="docs-tab-panel" role="tabpanel" id="mcp-cargo-panel" aria-labelledby="mcp-cargo-tab" hidden>
      <p class="docs-tab-note">Use this if you want to build the server from source.</p>
      <pre><code class="language-bash">cargo install --path crates/mcp-server</code></pre>
    </div>
  </div>
</div>

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

The server exposes 11 tools: `remember`, `recall`, `recall_scored`, `assemble_context`, `facts_about`, `assert_fact`, `correct_fact`, `invalidate_fact`, `what_changed`, `memory_health`, and `recall_for_task`. See the [MCP Tools Reference](/docs/api/mcp-tools/) for full parameter documentation.
