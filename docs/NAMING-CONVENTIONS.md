# Naming Conventions

This document defines repository naming conventions used to keep crate entrypoints and docs unambiguous.

## Rust crate entrypoints

- Policy: do not use legacy default entrypoint filenames. Every crate entrypoint must be explicitly named and declared via `[lib] path = "..."`.
- All crates in this repository follow this convention:
  - `crates/core/Cargo.toml` → `path = "src/temporal_graph.rs"`
  - `crates/agent-memory/Cargo.toml` → `path = "src/agent_memory.rs"`
  - `crates/ios/Cargo.toml` → `path = "src/ffi.rs"`
  - `crates/mcp-server/Cargo.toml` → binary, uses `src/main.rs`
  - `crates/python/Cargo.toml` → `path = "src/python_bindings.rs"`
  - `crates/wasm/Cargo.toml` → `path = "src/wasm_bindings.rs"`

Named entrypoints make the high-level API explicit and avoid confusion between crates.

## Documentation path references

- Prefer referencing crate names first, file paths second.
- When file paths are needed, reference the configured entrypoint path (not assumed defaults).
- For `kronroe-agent-memory`, always reference:
  - `crates/agent-memory/src/agent_memory.rs`

## Feature naming

- Feature names should describe capability, not implementation detail.
- Keep names kebab-case and stable once published.
- Example in this repo:
  - `vector` for vector retrieval support
  - `hybrid-experimental` reserved for explicitly experimental retrieval composition

## Why this exists

This convention reduces onboarding mistakes, prevents stale docs, and keeps commercial-facing engineering communication consistent.
