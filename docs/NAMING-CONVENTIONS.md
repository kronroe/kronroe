# Naming Conventions

This document defines repository naming conventions used to keep crate entrypoints and docs unambiguous.

## Rust crate entrypoints

- Policy: do not use legacy default entrypoint filenames. Every crate entrypoint must be explicitly named and declared via `[lib] path = "..."`.
- Exception rule: if a crate intentionally uses a named entrypoint, set it explicitly in `Cargo.toml` under `[lib] path = "..."]`.
- Current explicit exception:
  - `crates/agent-memory/Cargo.toml` uses `path = "src/agent_memory.rs"`.

Reason: `agent_memory.rs` makes the high-level API entrypoint explicit and avoids confusion with other crates that also used legacy defaults.

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
