# Hybrid Real-World Utility Research (User-First)

Date: 2026-03-14  
Owner: Product + Platform  
Status: Historical research note; portions of this proposal shipped later in PR #110 and PR #111.

Note:
- This document is preserved as product research context.
- Treat completed items below as historical roadmap checkpoints rather than current open work.

## 1) Purpose

Make hybrid retrieval useful for real people doing real daily work, not just technically strong in benchmarks.

This brief translates current hybrid capability into a practical product path:
- clearer daily user outcomes
- fewer cognitive steps for users
- safer memory behavior when wrong or stale
- concrete method/API additions that teams can implement in small increments

## 2) What We Have Today (Evidence-Based)

### Strong foundation

- Hybrid retrieval exists and is tested in core:
  - `search_hybrid` in `crates/core/src/temporal_graph.rs`
  - two-stage reranker in `crates/core/src/hybrid.rs`
- Hybrid behavior is available across agent-facing surfaces behind feature gates:
  - `kronroe-agent-memory` (`hybrid`)
  - `kronroe-mcp` (`hybrid`)
  - `kronroe-py` (`hybrid`)
  - `kronroe-wasm` (`hybrid`)
- Stability is intentionally marked as experimental:
  - `docs/API-STABILITY-MATRIX.md`
  - `README.md` capability matrix

### Product-significant gap

- Ingestion is still minimal:
  - `AgentMemory::remember` stores one unstructured `memory` fact per call.
  - MCP `remember` does one simple heuristic extraction: `"X works at Y"` (`parse_works_at`).
- Context assembly is line-oriented and relevance-first, but not action-first:
  - `assemble_context` builds ranked lines with rough token budgeting.
- Current output is technically transparent (scores/confidence) but still requires developer interpretation rather than helping users decide what to do next.

## 3) Real Daily Jobs (People, Not Pipelines)

These are recurring, high-frequency jobs where memory quality matters more than raw retrieval elegance.

1. Personal assistant / founder ops
- "What changed since yesterday for client X?"
- "What do I need to remember before this call in 5 minutes?"

2. Care coordination / support contexts
- "What is still true now?"
- "What conflicts in this person’s record need resolution?"

3. Sales and customer success
- "What did they care about last time?"
- "What are the top three next actions based on recent signals?"

4. Product and engineering execution
- "What decisions are active, outdated, or contradictory?"
- "What do we know with high confidence versus assumptions?"

Core observation: users need **decision-ready memory**, not only ranked memory.

## 4) Gap Analysis (Why Hybrid Is Not Yet Daily-Useful Enough)

### Gap A: Good retrieval, weak memory shaping

Current state:
- Recall can rank well with hybrid scoring.
- But remember-path mostly stores raw note text and very limited structure.

User impact:
- Memory base becomes noisy and harder to maintain.
- Daily users spend time translating text back into actionable facts.

### Gap B: Time awareness exists, but user intent capture is thin

Current state:
- Intent/operator machinery exists (`timeless`, `current_state`, `historical_point`, etc.).
- Most users/clients do not naturally provide these controls.

User impact:
- Better temporal ranking is available but underutilized.
- People get “technically correct but context-misaligned” recall.

### Gap C: Confidence and contradiction systems are present, but not surfaced as guidance

Current state:
- Confidence filtering, uncertainty, contradiction features are implemented.
- APIs return metadata, but no default workflow for handling stale/low-trust/conflicting memory.

User impact:
- Teams can detect risk, but are not guided to resolve it quickly.

### Gap D: No first-class "change over time" user story

Current state:
- Engine supports temporal validity and historical querying.
- Product surface does not prioritize "what changed since X" as a primary daily flow.

User impact:
- High-value temporal differentiator remains buried.

## 5) User-First Design Principles for Next Iteration

1. Answer the user’s next decision, not just their query.
2. Default to plain language semantics; keep advanced temporal controls optional.
3. Make uncertain/conflicting memories visible and actionable.
4. Prefer small, composable methods over one giant “smart” endpoint.
5. Keep feature-gated experimentation, but measure outcomes in human task success.

## 6) Proposed New Methods (Creative, Practical, Incremental)

These methods are designed to build on current architecture while improving real-world usefulness.

### 6.1 `remember_event(...)`

Goal:
- Move from raw note capture to structured event capture without requiring full NLP extraction.

Proposed shape:
- Inputs: `who`, `what`, `when`, `where`, `source`, `confidence`, `episode_id`, optional `embedding`.
- Output: list of created fact IDs and normalized event ID.

Why this matters:
- Preserves human meaning while reducing memory noise.
- Gives downstream recall clearer temporal anchors.

### 6.2 `recall_for_task(...)`

Goal:
- Return decision-ready context for a user task.

Proposed shape:
- Inputs: `task`, optional `subject`, optional `now`, optional `horizon`, optional `query_embedding`.
- Output:
  - `key_facts` (top relevant)
  - `watchouts` (low confidence/conflicts/stale)
  - `recommended_next_checks` (questions user should ask or confirm)

Why this matters:
- Converts ranking into immediate utility for daily workflows.

### 6.3 `what_changed(...)`

Goal:
- Make temporal differentiation first-class.

Proposed shape:
- Inputs: `entity`, `since`, optional `predicate_filter`.
- Output:
  - `new_facts`
  - `invalidated_facts`
  - `corrected_facts`
  - `confidence_shifts`

Why this matters:
- This is the most human-meaningful temporal query pattern in real usage.

### 6.4 `memory_health(...)`

Goal:
- Give users/operators a fast reliability snapshot.

Proposed shape:
- Inputs: optional `entity`, optional `predicate`.
- Output:
  - `conflicts`
  - `stale_high_impact_facts`
  - `low_confidence_facts`
  - `missing_fields` hints (where memory needs richer structure)

Why this matters:
- Encourages maintenance habits before wrong memory becomes user-facing errors.

### 6.5 `apply_feedback(...)`

Goal:
- Close the loop when users say "that’s wrong", "outdated", or "mostly right."

Proposed shape:
- Inputs: `fact_id`, `feedback_type` (`confirm`, `correct`, `invalidate`, `lower_confidence`, `raise_confidence`), optional payload.
- Output: resulting fact IDs and change summary.

Why this matters:
- Turns correction from ad-hoc behavior into explicit, trackable product flow.

## 7) Prioritized Roadmap (User Value First)

### Phase 1 (1-2 weeks): Highest daily utility, low risk

Status update:
- `what_changed` and `memory_health` shipped later through the Wave 1 implementation in PR #110.
- Wrapper and validation hardening referenced by this track shipped later in PR #111.

1. Add `what_changed` (MCP + AgentMemory wrapper first)  
   Completed later in PR #110.
2. Add `memory_health` summary endpoint  
   Completed later in PR #110.
3. Improve `assemble_context` mode with `action_brief` output format:
   - `what_matters_now`
   - `what_might_be_wrong`
   - `what_to_confirm`

Success criteria:
- Fewer user turns needed to prepare for a meeting/task.
- Fewer “wrong memory used” incidents in manual eval scripts.

### Phase 2 (2-3 weeks): Better ingestion quality without over-engineering

1. Add `remember_event`
2. Keep existing `remember` path for backward compatibility
3. Expand extraction rules gradually (human-readable rule sets before heavy NLP)

Success criteria:
- Higher percentage of stored memories that are directly queryable as actionable facts.
- Lower manual correction rate per 100 memories.

### Phase 3 (3-4 weeks): Feedback loop and adaptive trust

1. Add `apply_feedback`
2. Connect feedback to uncertainty/confidence tuning
3. Add periodic “memory hygiene” recommendations

Success criteria:
- Faster recovery from bad memories.
- Higher user trust in recall responses over time.

## 8) Measurement Framework (Human Outcome Metrics)

Track these before/after each phase:

1. Task completion latency
- Median time from query to usable answer for target workflows.

2. Useful-on-first-response rate
- Percentage of recall responses that users accept without follow-up correction.

3. Wrong-memory incident rate
- Cases where stale/incorrect memory drives an incorrect action or recommendation.

4. Correction closure time
- Time between feedback submission and memory state becoming correct.

5. Daily retention signal
- Whether users keep using memory features after first week.

## 9) Research-to-Code To-Do List

### Immediate (this week)

1. Define exact MCP schemas for:
   - `what_changed`  
     Completed later in PR #110.
   - `memory_health`  
     Completed later in PR #110.
2. Add contract tests for new response shape and edge cases.  
   Completed later in PR #110.
3. Add a small "real-life task" eval set:
   - meeting prep
   - care update handoff
   - customer follow-up

### Next (next 2 weeks)

1. Implement `what_changed` in AgentMemory + MCP.  
   Completed later in PR #110.
2. Implement `memory_health` from existing contradiction/uncertainty paths.  
   Completed later in PR #110.
3. Add `assemble_context` mode: `action_brief`.

### After that

1. Implement `remember_event`.
2. Implement `apply_feedback`.
3. Add user-facing guide: "how to keep memory trustworthy daily."

## 10) Guardrails

1. Keep existing APIs stable; add new methods additively.
2. Keep hybrid feature marked experimental until:
   - two consecutive product-gate passes on real task set
   - no major regressions in confidence/contradiction behavior
3. Avoid introducing heavy NLP complexity before rule-based event structuring proves insufficient.

## 11) Bottom Line

Kronroe’s hybrid engine is technically strong enough to build on now.  
The next leverage is not more retrieval math by default; it is product methods that make memory easier to trust, easier to correct, and easier to act on every day.
