# Bi-Temporal Model

Kronroe implements the TSQL-2 bi-temporal model as a first-class engine primitive. Every fact stored in Kronroe carries two independent time dimensions, giving callers the ability to distinguish between *when something was true in the world* and *when the database learned about it*.

## Two Time Dimensions

### Valid Time

Valid time captures when a fact was true in the real world, independent of when it was recorded. A job that started on 2020-01-15 has `valid_from = 2020-01-15` even if the fact was first stored in 2024.

- `valid_from` -- when the fact became true in the world
- `valid_to` -- when the fact stopped being true (`None` means it is still current)

### Transaction Time

Transaction time captures when the database learned about a fact. This is managed automatically by the engine; callers do not set transaction timestamps directly.

- `recorded_at` -- when this fact was first written to the database
- `expired_at` -- when this fact was superseded or invalidated (`None` means still active)

## The Four Timestamps

| Field | Dimension | Meaning |
|---|---|---|
| `valid_from` | Valid time | When the fact became true in the world |
| `valid_to` | Valid time | When it stopped being true (`None` = still current) |
| `recorded_at` | Transaction time | When we first stored this fact |
| `expired_at` | Transaction time | When we overwrote or invalidated it (`None` = still active) |

A fact is **currently valid** when both `valid_to` and `expired_at` are `None`. This means it is still believed to be true in the world and has not been superseded in the database.

## Valid Time vs Transaction Time: An Example

Consider tracking where Alice works:

```
Step 1: On 2024-06-01 we record that Alice works at Acme (started 2023-01-10).
  Fact A: subject="alice", predicate="works_at", object="Acme"
          valid_from=2023-01-10  valid_to=None
          recorded_at=2024-06-01 expired_at=None

Step 2: On 2024-09-15 we learn Alice actually moved to Globex on 2024-08-01.
  We correct Fact A and assert Fact B:

  Fact A (corrected):
          valid_from=2023-01-10  valid_to=2024-09-15
          recorded_at=2024-06-01 expired_at=2024-09-15

  Fact B (new):
          valid_from=2024-09-15  valid_to=None
          recorded_at=2024-09-15 expired_at=None
```

After step 2, the database can answer two distinct questions:

- **"Where does Alice work right now?"** -- `current_facts("alice", "works_at")` returns Fact B (Globex).
- **"Where did we believe Alice worked on 2024-07-01?"** -- `facts_at("alice", "works_at", 2024-07-01)` returns Fact A (Acme), because at that point in valid time, Acme was still the recorded employer.

## How Corrections Work

When a fact is corrected via `correct_fact(fact_id, new_value, at)`:

1. The old fact's `valid_to` and `expired_at` are set to `at`, closing both its valid-time and transaction-time windows.
2. A new fact is created with the same `subject` and `predicate`, the new object value, and `valid_from = at`.
3. The old fact is not deleted. It remains in the database for historical queries.

This means every correction is non-destructive. The full history of what was believed and when is always preserved.

## How Invalidation Works

`invalidate_fact(fact_id, at)` sets both `valid_to` and `expired_at` to `at` on the target fact. After invalidation:

- The fact no longer appears in `current_facts()`.
- The fact still appears in `facts_at()` for timestamps before `at`.

## Point-in-Time Queries

`facts_at(subject, predicate, at)` queries the valid-time axis. It returns all facts for the given subject and predicate that satisfy:

- `valid_from <= at`
- `valid_to` is either `None` or `> at`
- `expired_at` is either `None` or `> at`

This enables queries like "what did we know about Alice's employer as of March 2024?" without any special application logic.

## Additional Fact Metadata

Beyond the four temporal timestamps, every fact carries two optional metadata fields:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `confidence` | `f32` | `1.0` | Confidence score in the range [0.0, 1.0]. Useful for representing uncertain or inferred knowledge. |
| `source` | `Option<String>` | `None` | Provenance marker identifying where the fact came from (e.g. `"user:alice"`, `"api:linkedin"`, `"episode:conv-42"`). |

These metadata fields integrate with Kronroe's optional uncertainty model (feature: `uncertainty`), which uses confidence and source to compute effective confidence at query time with age decay and source authority weighting.
