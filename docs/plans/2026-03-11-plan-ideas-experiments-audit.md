# Plan / Ideas / Experiments Audit

Date: 2026-03-11
Scope reviewed:
- `planning/*.md`
- `docs/plans/*.md`
- `.ideas/*.md`
- `.ideas/evals/*`

## Executive Snapshot

Kronroe has moved from "plan-heavy" into "execution-heavy." The core product and major
surfaces are substantially delivered, but planning docs are now partially stale and some
launch-readiness items remain open.

High-confidence summary:
- Core + AgentMemory + MCP + Python + WASM parity work: mostly complete.
- iOS/Android shipped as narrow preview surfaces.
- Hybrid + uncertainty + contradiction are implemented (with feature/stability caveats).
- Not everything in plans/ideas is complete; several release and distribution items remain.

## Source-by-Source Status

### `planning/04-roadmap.md` (active roadmap)

Current state in file:
- Phase 0: mostly complete
- Phase 1: in progress
- Phase 2: not started

Open items still explicitly listed:
1. `0.12` storage format commitment (`Not Started` in roadmap).
2. Unified release automation/runbook (`0.11` still in progress).
3. Immediate next actions:
   - benchmark plan
   - contradiction/uncertainty docs hardening
   - refresh issues for active Phase 1 scope

Cross-check result:
- These are still valid open items.

### `docs/plans/*`

- `2026-02-22-agent-memory-phase1.md`:
  - Historical implementation plan; most planned Phase 1 methods are now shipped.
- `2026-02-23-technical-phase-review.md`:
  - "next" recommendations are mostly executed.
  - Remaining risks still relevant: retrieval regression control, wrapper-level behavior tests, stabilization labeling.
- `2026-02-26-hybrid-product-gate-checkpoint.md`:
  - Product-gate pass documented.
  - Open risk note still pending: variance sanity rerun before any default flip.
- `2026-02-26-hybrid-timeslice-research-checkpoint.md`:
  - Historical checkpoint; superseded by later eval passes.
- `ios-integration-proof.md`:
  - Marked complete with evidence.
- `2026-03-11-cross-surface-contract-matrix.md`:
  - Complete and now backed by scriptable verification (`scripts/verify_contract_matrix.sh`).

### `.ideas/LAUNCH_ROADMAP_2026-02-26.md` (checkbox roadmap)

Checkbox state in file: `14 open / 0 checked`.

Cross-check against current code/docs:
- Completed in practice (but unchecked in file):
  - capability/stability matrix exists
  - contradiction detection shipped
  - uncertainty/provenance weighting shipped
  - temporal scoring/hybrid path shipped (feature-gated)
  - explainable recall surfaced
  - CI path scoping/hardening significantly improved
- Still open in practice:
  - full public panic-audit sign-off doc
  - contradiction/uncertainty model guide docs
  - TypeScript SDK alpha
  - cross-surface release runbook
  - security release checklist

Conclusion:
- This file is now stale as an execution tracker and should be refreshed or replaced by a
  dated status table.

### `.ideas/EXPLAINABLE_RECALL_NEXT_STAGES.md`

Status in file:
- Stage 1: done
- Stage 2: done
- Stage 3 (reranker-stage metadata): deferred

Cross-check result:
- Accurate.

### `.ideas/EXPERIMENT_01_*` + `.ideas/evals/*`

Status in file:
- Decision doc still says "Running" but records gate pass in later passes.

Cross-check result:
- Experiment has produced enough evidence for "adopted behind experimental guardrails."
- Labeling is partially stale (still reads "running" despite pass checkpoints).
- Remaining prudent action: a variance sanity rerun before any stability promotion.

### `.ideas/FUTURE_PHASES.md`

- Intentionally aspirational backlog.
- Not a completion checklist.
- No action required unless promoting items into roadmap.

## Are We "Done" With All Plans/Ideas/Experiments?

No. We are past the midpoint of this planning stack, but not complete.

Most important open work:
1. Storage format commitment + migration stance (`planning/04-roadmap.md` item `0.12`).
2. Release runbook unification across Rust/Python/npm/mobile artifacts.
3. TypeScript SDK alpha (still the biggest surface gap).
4. Contradiction/uncertainty user docs (currently code-rich, doc-light).
5. Hybrid stabilization checklist + variance rerun to support promotion decisions.
6. Refresh stale plan trackers (especially `.ideas/LAUNCH_ROADMAP_2026-02-26.md` and
   `.ideas/EXPERIMENT_01_DECISION.md`) so plans match reality.

## Recommended Next Docket

1. Ship a "status refresh" pass on planning artifacts (small, high leverage).
2. Start TypeScript SDK alpha on the same AgentMemory contract used by MCP/Python/WASM.
3. Complete release hardening docs (runbook + storage contract + security checklist).
4. Publish one concise hybrid/uncertainty/contradiction behavior guide with examples.
