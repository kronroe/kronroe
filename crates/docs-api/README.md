# kronroe-docs-api

HTTP API serving the Kronroe docs as a queryable, bi-temporal memory
system. Designed to deploy to Google Cloud Run.

This crate is **internal infrastructure**, not published to crates.io.
It's the engine behind the eventual `/api/docs/...` endpoints on
kronroe.dev and the backing service for `kronroe-docs-mcp`.

See `.ideas/PLAN_docs_pipeline.md` (gitignored) for the full Phase 3
roadmap. This crate is the Phase 3a runtime spike.

## Status

**Phase 3a — runtime spike.**

What works:

- axum HTTP server with `GET /healthz` and `POST /api/docs/recall`
- In-memory `TemporalGraph` loaded with five hardcoded test sections
  on boot
- fastembed-rs (`AllMiniLML6V2`, 384-dim) used for both corpus and
  query embedding
- Multi-stage Dockerfile that builds + runs

What doesn't yet (Phase 3b/3c):

- Real corpus loaded from `corpus.json`
- The other three endpoints (`/sections`, `/sections/<id>`, `/symbols/<name>`)
- CORS middleware so kronroe.dev can call this from the browser
- Rate-limiting
- Bi-temporal modelling of API symbols + cross-references
- The `kronroe-docs-mcp` server that consumes this API

## Local development

Requires the workspace's Rust toolchain (stable). The first build is
slow because fastembed pulls the ONNX runtime; subsequent builds
benefit from cargo's incremental compilation.

```bash
# From the repo root.
cargo run --release --package kronroe-docs-api

# Boot logs will look like:
#   loading embedding model (fastembed: AllMiniLML6V2, 384 dims)
#   opening in-memory TemporalGraph
#   embedding 5 test sections
#   corpus loaded { dim: 384, sections: 5 }
#   kronroe-docs-api listening { addr: 0.0.0.0:8080 }
```

Once running, exercise the endpoints:

```bash
# Liveness
curl -s http://localhost:8080/healthz

# Semantic recall
curl -s http://localhost:8080/api/docs/recall \
    -H 'content-type: application/json' \
    -d '{"query": "how do I track when a fact changed", "limit": 3}' \
    | jq

# Expected hits: bi-temporal-model + facts-and-entities + vector-search,
# scored 0.4 - 0.7 against the cosine similarity of AllMiniLML6V2.
```

## Container build (matches Cloud Run target)

```bash
docker build -t kronroe-docs-api -f crates/docs-api/Dockerfile .
docker run --rm -p 8080:8080 kronroe-docs-api
```

## What this spike validates

The whole point of Phase 3a is to confirm the Phase 3b/3c plan's stack
choices before committing to ~16 hours of real implementation:

| Question | Verified by this crate |
|---|---|
| Does Kronroe + fastembed-rs + axum compose in one binary? | Yes — see `bootstrap()` and `recall()` in `src/main.rs` |
| Does the embedding-dimension contract hold end-to-end? | Yes — both corpus and query go through the same `TextEmbedding`; mismatched dim returns a clear 500 |
| Does the binary run cleanly in a container? | Yes — multi-stage Dockerfile produces a ~150MB image |
| Are there hidden runtime / Cloud Run incompatibilities? | TBD — Phase 3a deploy step (manual, not yet automated) |

## Phase 3b transition plan

Phase 3b reuses `bootstrap()`'s shape but swaps the hardcoded
`TEST_CORPUS` for a loader that reads `_root/corpus.json` produced by
`site/scripts/build-docs.py`. The endpoint signatures don't change;
only the data source.

The Bi-temporal modelling switches from "every section is a single
fact" to "sections + cross-references + symbols" per Option B in the
plan doc. That's a bigger structural change but lives entirely behind
the API surface.
