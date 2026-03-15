# Hybrid Stage 3 → Production Readiness Checklist

Date: 2026-03-14  
Scope: `hybrid-experimental` retrieval path across core + agent-memory + MCP + wrappers

## Current Stage

Hybrid is **adopted behind experimental guardrails** and is not yet promoted to stable contract.

## Exit Gates (Must Pass Before Promotion)

### A) Product utility gates

- [ ] Real-task eval pass in two consecutive runs (same criteria + dataset family)
- [ ] “What changed” and “memory health” flows reduce user correction workload in scripted daily tasks
- [ ] Agent orchestration quality pass (recommended actions lead to correct next tool in eval harness)

### B) Retrieval quality + regression gates

- [ ] Hybrid wins or ties strongest baseline on aggregate + hard slices
- [ ] Time-slice regressions bounded (documented thresholds)
- [ ] Variance sanity rerun completed and attached to decision log

### C) Contract + API gates

- [x] Hybrid controls are feature-gated across core / agent-memory / MCP
- [x] MCP returns machine-actionable decision metadata for user-first methods
- [ ] Cross-surface contract parity refreshed (Rust/Python/WASM wrapper checks)
- [ ] Stage-3 reranker metadata contract finalized and documented

### D) Reliability + operations gates

- [x] CI/tests cover hybrid + non-hybrid branches in Rust core surfaces
- [ ] Wrapper-level smoke for hybrid controls (npm + python packages)
- [ ] Runbook for incident rollback (`use_hybrid` off / feature toggle strategy)
- [ ] Production telemetry schema defined (query intent, selected action, correction rate)

### E) Documentation + release gates

- [ ] Hybrid behavior guide with examples (plain-language + API examples)
- [ ] Stability matrix updated with promotion decision (if promoted)
- [ ] Release notes include compatibility stance and migration notes

## Recommended Next 3 Moves

1. Finalize reranker-stage metadata contract and enforce in MCP/SDK snapshots.  
2. Run product-gate + variance rerun and write decision memo (`promote` vs `remain experimental`).  
3. Add wrapper-level hybrid smoke tests so package users match workspace guarantees.

## Immediate Hardening Completed This Session

- Added robust correction-linking tolerance for near-equal timestamps in `what_changed`.
- Added agent-first ranked decision metadata in MCP `agent_brief` outputs.
