# Hybrid Stage 3 -> Production Readiness Checklist

Date: 2026-03-14  
Scope: `hybrid-experimental` retrieval path across core + agent-memory + MCP + wrappers

## Current Stage

Hybrid is **adopted behind experimental guardrails** and is not yet promoted to stable contract.
Eval passed 2026-03-22 (product gate PASS, two consecutive runs). Sole remaining gate: A3.

## Exit Gates (Must Pass Before Promotion)

### A) Product utility gates

- [x] Real-task eval pass in two consecutive runs (2026-03-22 — nDCG@3=0.8249, product gate PASS)
- [x] "What changed" and "memory health" flows reduce user correction workload (shipped PR #110)
- [ ] Agent orchestration quality pass (recommended actions lead to correct next tool in eval harness) — **sole remaining gate**

### B) Retrieval quality + regression gates

- [x] Hybrid wins or ties strongest baseline on aggregate + hard slices (+17% semantic, +47% time-slice)
- [x] Time-slice regressions bounded (+47.18% lift, zero regressions)
- [x] Variance sanity rerun completed (two identical runs, `.ideas/evals/results/hybrid_eval_report_20260322-*.md`)

### C) Contract + API gates

- [x] Hybrid controls are feature-gated across core / agent-memory / MCP
- [x] MCP returns machine-actionable decision metadata for user-first methods
- [x] Cross-surface contract parity refreshed (Rust/Python/WASM wrapper checks — PRs #91, #92, #94)
- [x] Stage-3 reranker metadata contract finalized (`docs/HYBRID-RERANKER-CONTRACT.md`)

### D) Reliability + operations gates

- [x] CI/tests cover hybrid + non-hybrid branches in Rust core surfaces
- [x] Wrapper-level smoke for hybrid controls (npm + python packages)
- [x] Runbook for incident rollback (`docs/HYBRID-ROLLBACK-RUNBOOK.md`)
- [x] Production telemetry schema defined (`docs/HYBRID-TELEMETRY-SCHEMA.md`)

### E) Documentation + release gates

- [x] Hybrid behavior guide with examples (`docs/HYBRID-BEHAVIOR-GUIDE.md`)
- [x] Stability matrix updated — remains Experimental pending gate A3 (`docs/API-STABILITY-MATRIX.md`)
- [x] Release notes include compatibility stance (`docs/RELEASE-NOTES-0.4.md`)

## Remaining Work

Gate A3 (agent orchestration quality pass) requires building an automated eval that
tests whether recommended actions lead to the correct next tool. This is new eval
infrastructure, not a documentation task. Hybrid will not be promoted to Preview
until this gate closes.

## Immediate Hardening Completed

- Added robust correction-linking tolerance for near-equal timestamps in `what_changed`.
- Added agent-first ranked decision metadata in MCP `agent_brief` outputs.
- Added wrapper-level hybrid smoke verification for npm and Python package surfaces.
- Added a single PASS/FAIL wrapper gate so CI can catch package-surface regressions quickly.
