# Stable Agent-Memory Cross-Surface Contract

This note defines the current stable contract for the shared Kronroe agent-memory
surfaces across MCP, Python, and WASM.

## Canonical Wire Shape

- MCP JSON is the canonical wire contract.
- Python and WASM are validated against the same semantic contract after
  normalization into the MCP-style field shape.
- The shared fixtures live in
  [`contracts/stable-agent-memory.json`](../contracts/stable-agent-memory.json).

## Locked Methods

- `recall_scored`
- `assemble_context`
- `assert_fact`
- `correct_fact`
- `invalidate_fact`

Also locked where those surfaces exist today:

- `what_changed`
- `memory_health`

WASM does not currently ship `what_changed` or `memory_health`, so this note
does not treat their absence there as drift. The current WASM conformance lane
also only locks the subset that is reliably testable in the host-side cargo
suite: fact ID operations and `recall_scored`.

## Compatibility Rule

- Required field names and their meanings are locked.
- `fact_id` values must use Kronroe Fact IDs (`kf_...`).
- Additive fields are allowed if they do not rename, remove, or change the
  meaning of existing stable fields.
- Ordering must remain deterministic where current stable behavior already
  depends on it.

## Packaging Rule

- The native Rust `kronroe-mcp` binary is the only supported MCP runtime.
- The Python `kronroe-mcp` wrapper remains supported as a thin launcher to the
  native binary.
- The former npm/Node wrapper is no longer part of the supported product
  surface.
