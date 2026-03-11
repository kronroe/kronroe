# API Stability Matrix and Compatibility Policy

Last updated: 2026-03-11
Applies to: `kronroe` workspace `0.1.x`

Kronroe is still early-stage software, but this document defines which surfaces are safe to build against, which are still evolving, and how compatibility changes are handled.

## Stability Levels

| Level | Meaning | Compatibility expectation |
|---|---|---|
| `Stable` | Intended for long-lived integrations. | No intentional breaking changes in patch releases (`0.1.z`). Breaking changes in a new minor (`0.2`) require migration notes. |
| `Preview` | Usable and supported, but still evolving. | Breaking changes may happen in a new minor release with clear release notes and migration guidance. |
| `Experimental` | Explicitly unstable by design. | May change or be removed without deprecation. Not recommended for hard dependencies. |
| `Internal` | Developer/test infrastructure only. | No public compatibility guarantees. |

## Surface Matrix (Current)

### Core (`crates/core`)

| API surface | Gate | Level | Notes |
|---|---|---|---|
| Bi-temporal CRUD/query (`assert_fact`, `facts_at`, `current_facts`, `correct_fact`, `invalidate_fact`) | base | `Stable` | Primary engine contract. |
| Full-text search (`search`) | `fulltext` (default) | `Stable` | BM25 + fuzzy match path. |
| Vector search (`search_by_vector`, embedding writes) | `vector` | `Stable` | Feature-gated but contract intended to be dependable. |
| Hybrid search (`search_hybrid`) | `hybrid-experimental` + `vector` | `Experimental` | Explicitly marked experimental in feature naming and docs. |
| Contradiction detection (`assert_fact_checked`, `detect_contradictions`) | `contradiction` | `Preview` | Functional and tested; policy/shape may still evolve. |
| Uncertainty model (`register_predicate_volatility`, `effective_confidence`) | `uncertainty` | `Preview` | Functional and tested; modeling knobs may evolve. |

### Agent Memory (`crates/agent-memory`)

| API surface | Gate | Level | Notes |
|---|---|---|---|
| `remember`, `recall`, `recall_scored`, `assemble_context` | base | `Stable` | Primary high-level product API. |
| Confidence/source assertions (`assert_with_confidence`, `assert_with_source`) | base | `Stable` | Part of core ingestion contract. |
| `RecallOptions` / `RecallScore` | base | `Stable` | Designed for additive evolution (`#[non_exhaustive]`). |
| Hybrid recall controls (`with_hybrid`, temporal intent/operator passthrough) | `hybrid` | `Experimental` | Inherits hybrid-experimental risk from core. |
| Contradiction helpers (`assert_checked`, `audit`) | `contradiction` | `Preview` | Depends on preview contradiction engine behavior. |
| Uncertainty helpers (`with_min_effective_confidence`, volatility/source registration) | `uncertainty` | `Preview` | Depends on preview uncertainty model behavior. |

### MCP Server (`crates/mcp-server`)

| API surface | Gate | Level | Notes |
|---|---|---|---|
| Tool names and JSON-RPC framing (`remember`, `recall`, `recall_scored`, `assemble_context`, `facts_about`, `assert_fact`, `correct_fact`, `invalidate_fact`) | base | `Stable` | Main integration surface for AI agents. |
| Core tool arguments (`query`, `limit`, `fact_id`, etc.) | base | `Stable` | Additive fields may be introduced without breaking existing calls. |
| Hybrid recall options (`query_embedding`, `use_hybrid`, temporal intent/operator) | `hybrid` | `Experimental` | Feature-gated and may evolve. |
| Effective-confidence filtering (`confidence_filter_mode=effective`) | `uncertainty` | `Preview` | Depends on uncertainty model evolution. |

### Python (`crates/python`)

| API surface | Gate | Level | Notes |
|---|---|---|---|
| `KronroeDb` and `AgentMemory` class method names | base | `Stable` | Intended for prototyping and production integration. |
| Core/agent method keyword signatures (for shipped methods) | base | `Stable` | Changes should be additive where possible. |
| Hybrid controls in `recall_scored` | `hybrid` | `Experimental` | Mirrors experimental hybrid path. |
| Effective-confidence filtering in `recall_scored` | `uncertainty` | `Preview` | Mirrors preview uncertainty path. |
| Test/build mode toggles (`python-runtime-tests`) | build/test | `Internal` | Not part of user-facing API contract. |

### WASM (`crates/wasm`)

| API surface | Gate | Level | Notes |
|---|---|---|---|
| In-memory temporal CRUD/query/invalidate bindings | base (no fulltext) | `Preview` | Usable, but still narrower than native surfaces. |

### iOS / Android (`crates/ios`, `crates/android`)

| API surface | Gate | Level | Notes |
|---|---|---|---|
| Current FFI/JNI bridge functions (`open`, `assert_text`, `facts_about_json`, error accessors) | base | `Preview` | Contract is intentionally narrow and may expand. |

## Feature Flag Stability

| Feature flag | Crate(s) | Level | Guidance |
|---|---|---|---|
| `fulltext` | `kronroe` | `Stable` | Default in core; safe to depend on. |
| `vector` | `kronroe` | `Stable` | Opt-in but contract is stable. |
| `hybrid-experimental` | `kronroe` | `Experimental` | Do not treat as long-term stable contract yet. |
| `contradiction` | `kronroe`, `kronroe-agent-memory` | `Preview` | Supported, still evolving. |
| `uncertainty` | `kronroe`, `kronroe-agent-memory`, `kronroe-mcp`, `kronroe-py` | `Preview` | Supported, still evolving. |
| `hybrid` | `kronroe-agent-memory`, `kronroe-mcp`, `kronroe-py` | `Experimental` | Transitively depends on `hybrid-experimental`. |
| `extension-module` | `kronroe-py` | `Stable` | Standard packaging path for Python extension builds. |
| `python-runtime-tests` | `kronroe-py` | `Internal` | Runtime test harness mode, not public product API. |

## Compatibility Policy (`0.x`)

### 1) Versioning rules

- Kronroe is currently pre-`1.0` (`0.1.x`), so some evolution is expected.
- For APIs marked `Stable`, patch releases (`0.1.z`) should remain backward compatible.
- For APIs marked `Stable`, breaking changes may occur only in a new minor line (`0.2`, `0.3`, etc.) and must include migration guidance.

### 2) Change management rules

- `Stable`:
  - Prefer additive changes.
  - If a breaking change is unavoidable, document migration steps in release notes and update this matrix.
- `Preview`:
  - Breaking changes are allowed in new minor releases.
  - Must include release-note callouts and a migration section when user-facing behavior changes.
- `Experimental`:
  - No compatibility guarantee.
  - May change/remove quickly to support iteration.
- `Internal`:
  - No public guarantee.

### 3) Documentation and release requirements

For any API-level behavior change:

1. Update this matrix if stability classification changes.
2. Update relevant crate README(s) and root `README.md` if user-facing behavior changes.
3. Include migration notes in PR description when behavior is breaking for `Stable` or `Preview` surfaces.

## Adoption Guidance

- If you need long-term contract stability today, build on `Stable` rows only.
- Treat `Preview` features as opt-in with release-note monitoring.
- Avoid hard product dependencies on `Experimental` paths until promoted.
