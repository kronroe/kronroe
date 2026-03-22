# MCP Tools Reference

Complete reference for all 11 tools exposed by the Kronroe MCP server. Each tool is callable via the Model Context Protocol stdio transport.

## remember

Ingest free-text and store extracted facts in memory.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `text` | string | Yes | The text to parse and store as facts. Max 32 KiB. |
| `episode_id` | string | No | Group facts under a conversation or episode. Defaults to `"default"`. Max 512 bytes. |
| `idempotency_key` | string | No | Deduplicate repeated ingestion of the same text. Cannot be combined with `query_embedding`. Max 512 bytes. |
| `query_embedding` | array of numbers | No | Pre-computed embedding vector for the text. Requires the `hybrid` feature. Cannot be combined with `idempotency_key`. |

**Example:**

```json
{
  "text": "Alice joined Acme Corp in January 2025 as a senior engineer.",
  "episode_id": "onboarding-chat"
}
```

## recall

Recall facts by natural-language query using full-text search.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | Yes | Natural-language search query. Max 8 KiB. |
| `limit` | integer | No | Maximum number of results to return. Range: 1--200. Default: 10. |
| `include_scores` | boolean | No | When `true`, returns per-channel scoring metadata alongside each fact. Default: `false`. |
| `min_confidence` | number | No | Minimum confidence threshold for returned facts. Range: 0.0--1.0. |
| `confidence_filter_mode` | string | No | Which confidence signal to filter on: `"base"` (raw fact confidence) or `"effective"` (uncertainty-aware; requires `uncertainty` feature). Requires `min_confidence` to be set. |
| `max_scored_rows` | integer | No | Limit the number of rows that receive full scoring computation. Minimum: 1. |
| `query_embedding` | array of numbers | No | Pre-computed embedding vector for hybrid retrieval. Requires the `hybrid` feature. |
| `use_hybrid` | boolean | No | Enable hybrid (text + vector) retrieval. Requires `query_embedding` and the `hybrid` feature. |
| `temporal_intent` | string | No | Temporal intent hint for the reranker: `"timeless"`, `"current_state"`, `"historical_point"`, or `"historical_interval"`. Requires the `hybrid` feature. |
| `temporal_operator` | string | No | Temporal operator hint: `"current"`, `"as_of"`, `"during"`, `"before"`, `"by"`, `"after"`, or `"unknown"`. Requires the `hybrid` feature. |

**Example:**

```json
{
  "query": "where does Alice work",
  "limit": 5,
  "min_confidence": 0.8
}
```

## recall_scored

Recall facts with per-channel scoring metadata included for every result. Identical parameters to `recall`, except there is no `include_scores` parameter (scores are always included).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | Yes | Natural-language search query. Max 8 KiB. |
| `limit` | integer | No | Maximum number of results. Range: 1--200. Default: 10. |
| `min_confidence` | number | No | Minimum confidence threshold. Range: 0.0--1.0. |
| `confidence_filter_mode` | string | No | `"base"` or `"effective"`. Requires `min_confidence`. |
| `max_scored_rows` | integer | No | Limit rows receiving full scoring. Minimum: 1. |
| `query_embedding` | array of numbers | No | Pre-computed embedding vector. Requires the `hybrid` feature. |
| `use_hybrid` | boolean | No | Enable hybrid retrieval. Requires `query_embedding` and the `hybrid` feature. |
| `temporal_intent` | string | No | Temporal intent hint. Requires the `hybrid` feature. |
| `temporal_operator` | string | No | Temporal operator hint. Requires the `hybrid` feature. |

Each result includes a score breakdown with fields like `rrf_score`, `text_contrib`, `vector_contrib`, `confidence`, and `effective_confidence`.

**Example:**

```json
{
  "query": "Alice's job title",
  "limit": 3
}
```

## assemble_context

Build LLM-ready context text from the top-ranked facts matching a query, constrained by a token budget.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | Yes | Natural-language query to find relevant facts. Max 8 KiB. |
| `max_tokens` | integer | Yes | Maximum token budget for the assembled context. Minimum: 1. |
| `query_embedding` | array of numbers | No | Pre-computed embedding vector for hybrid retrieval. |

**Example:**

```json
{
  "query": "everything about Alice's career",
  "max_tokens": 500
}
```

## facts_about

Return all current facts about a specific entity.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `entity` | string | Yes | The entity name to look up (e.g., `"alice"`). |

**Example:**

```json
{
  "entity": "alice"
}
```

## assert_fact

Assert a structured fact with explicit subject, predicate, and object.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `subject` | string | Yes | The entity the fact is about (e.g., `"alice"`). |
| `predicate` | string | Yes | The relationship or property name (e.g., `"works_at"`). |
| `object` | any | Yes | The value: a string, number, or boolean. |
| `valid_from` | string | No | RFC 3339 timestamp for when this fact became true. Defaults to the current time. |
| `confidence` | number | No | Confidence score. Range: 0.0--1.0. Default: 1.0. Cannot be combined with `idempotency_key`. |
| `source` | string | No | Provenance marker (e.g., `"user_statement"`). Cannot be combined with `idempotency_key`. |
| `idempotency_key` | string | No | Deduplicate repeated assertions. Cannot be combined with `confidence` or `source`. |

**Example:**

```json
{
  "subject": "alice",
  "predicate": "works_at",
  "object": "Acme Corp",
  "confidence": 0.95,
  "source": "user_statement",
  "valid_from": "2025-01-15T00:00:00Z"
}
```

## correct_fact

Correct a fact's value by its ID. The old fact is preserved in history with its validity window closed, and a new fact is created with the corrected value.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `fact_id` | string | Yes | The Kronroe Fact ID (`kf_...`) of the fact to correct. |
| `new_value` | any | Yes | The corrected value (string, number, or boolean). |

**Example:**

```json
{
  "fact_id": "kf_01HX...",
  "new_value": "Globex Corp"
}
```

## invalidate_fact

Invalidate a fact by its ID, ending its validity window. The fact is not deleted -- it remains in history with `valid_to` set to the current time.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `fact_id` | string | Yes | The Kronroe Fact ID (`kf_...`) of the fact to invalidate. |

**Example:**

```json
{
  "fact_id": "kf_01HX..."
}
```

## what_changed

Return a change report for an entity since a given timestamp. Shows new facts, invalidated facts, and corrections.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `entity` | string | Yes | The entity to check for changes. |
| `since` | string | Yes | RFC 3339 timestamp. Only changes after this time are included. |
| `predicate` | string | No | Filter changes to a specific predicate (e.g., `"works_at"`). |

**Example:**

```json
{
  "entity": "alice",
  "since": "2025-01-01T00:00:00Z",
  "predicate": "works_at"
}
```

## memory_health

Return an operational health report for an entity's stored facts. Identifies low-confidence facts, stale high-impact facts, and contradictions.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `entity` | string | Yes | The entity to assess. |
| `predicate` | string | No | Scope the report to a specific predicate. |
| `low_confidence_threshold` | number | No | Facts with confidence below this value are flagged. Range: 0.0--1.0. Default: 0.7. |
| `stale_after_days` | integer | No | High-impact facts older than this many days are flagged as stale. Minimum: 0. Default: 90. |

**Example:**

```json
{
  "entity": "alice",
  "low_confidence_threshold": 0.5,
  "stale_after_days": 180
}
```

## recall_for_task

Return decision-ready memory context scoped to a specific task. Provides key facts, watchouts (low-confidence or stale facts relevant to the task), and recommended next checks.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `task` | string | Yes | Description of the task or decision to prepare for. Max 8 KiB. |
| `subject` | string | No | Scope recall to a specific entity. |
| `now` | string | No | RFC 3339 timestamp to use as "now" for staleness calculations. Defaults to the current time. |
| `horizon_days` | integer | No | How far back to look for relevant facts, in days. Minimum: 1. Default: 30. |
| `limit` | integer | No | Maximum number of key facts to return. Range: 1--200. Default: 8. |
| `query_embedding` | array of numbers | No | Pre-computed embedding vector for hybrid retrieval. Requires the `hybrid` feature. |
| `use_hybrid` | boolean | No | Enable hybrid retrieval. Requires `query_embedding` and the `hybrid` feature. |

**Example:**

```json
{
  "task": "Write a performance review for Alice",
  "subject": "alice",
  "horizon_days": 365,
  "limit": 10
}
```
