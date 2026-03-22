# Changelog

## 2026-03-22

- Bumped the workspace and published package metadata to `0.3.0`.
- Locked the stable cross-surface agent-memory contract with shared MCP/Python/WASM fixtures and conformance tests.
- Removed the npm/Node MCP wrapper from the supported product surface and standardized on the native `kronroe-mcp` binary.

## 2026-03-19

- Replaced the old third-party full-text engine with the Kronroe lexical engine in core search and hybrid retrieval.
- Removed the remaining shadow harness and dependency references so the active codebase and docs now describe the Kronroe-owned lexical path consistently.
- Introduced Kronroe Fact IDs (`kf_...`) as the new canonical stable `fact_id` format.
- Added automatic schema v1 -> v2 migration so reopened databases are rewritten to canonical `kf_...` Fact IDs.
- Bumped the workspace and published wrapper package metadata to `0.2.0` to reflect the stable surface change.
