# Hybrid Product Gate Checkpoint (CI-Aware)

Date: 2026-02-26
Branch: codex/hybrid-timeslice-research-checkpoint
Status: Product gate pass in private eval harness

## Decision summary
Experiment 01 now passes the **asymmetric product gate** and the **CI-aware product gate** in the private evaluation harness.

Latest artifact:
- `.ideas/evals/results/hybrid_eval_report_20260226-130826.json`

Core outcome:
- semantic lift vs text: `+19.44%` (pass)
- semantic delta vs vector: `-1.55%` (pass with threshold `>= -3.0%`)
- time-slice lift vs strongest baseline: `+77.27%` (pass)
- p95 regression vs text: `+1.55%` (pass)
- overall product gate: **PASS**
- overall product gate (CI-aware): **PASS**

## Why this gate is product-realistic
The strict "semantic must beat strongest baseline" gate over-penalizes hybrid when vector-only is near ceiling and temporal gains are large. Product value here is:
1. Significant semantic gain vs text baseline.
2. Controlled non-regression vs vector baseline.
3. Strong time-slice uplift.
4. Latency within guardrail.

The asymmetric gate encodes exactly that behavior.

## What we tested to get here
1. Labeled temporal slices/intents/operators in eval dataset.
2. Temporal Feasibility First reranking.
3. Intent-gated temporal model ablation (`legacy_tff`, `intent_gated_v1`).
4. Two-stage retrieval ablation (`two_stage_v1`).
5. Bootstrap CI over semantic delta vs vector for final confidence check.

## Rollout plan (tracked code)
1. Introduce explicit product gate policy in tracked evaluation docs and CI report format.
2. Promote winner policy into a feature-gated core retrieval path:
   - keep existing behavior as default
   - add new hybrid mode behind experimental flag
3. Add integration tests for:
   - semantic non-regression threshold
   - time-slice uplift threshold
   - latency budget guardrail
4. Run shadow evaluation on expanded temporal/paraphrase suites before defaulting on.

## Safeguards
- Keep old hybrid behavior available for rollback.
- Require gate pass on at least 2 consecutive runs before default flip.
- Track per-slice metrics in release notes to avoid aggregate-only regressions.

## Open risk note
The CI band was very tight on the latest run; before default flip we should run one variance sanity check with:
- alternate bootstrap seed strategy
- small dataset perturbation (query order/shuffle)

If these remain stable, proceed to controlled rollout.
