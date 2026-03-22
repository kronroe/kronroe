# Facts and Entities

Kronroe's storage model is built on two primitives: **facts** and **entities**. Together they form a temporal property graph where every edge and property carries full bi-temporal metadata.

## What Is a Fact?

A `Fact` is a subject-predicate-object triple augmented with bi-temporal timestamps and optional metadata. It is the fundamental unit of storage in Kronroe.

```rust
pub struct Fact {
    pub id: FactId,
    pub subject: String,
    pub predicate: String,
    pub object: Value,
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
    pub recorded_at: DateTime<Utc>,
    pub expired_at: Option<DateTime<Utc>>,
    pub confidence: f32,
    pub source: Option<String>,
}
```

| Field | Description |
|---|---|
| `id` | Unique, lexicographically sortable identifier (`kf_...` format) |
| `subject` | The entity this fact is about (e.g. `"alice"`) |
| `predicate` | The relationship or attribute name (e.g. `"works_at"`, `"has_role"`) |
| `object` | The value -- a `Value` enum (see below) |
| `valid_from` | When this became true in the world |
| `valid_to` | When this stopped being true (`None` = still current) |
| `recorded_at` | When the database recorded this fact |
| `expired_at` | When this fact was superseded or invalidated (`None` = still active) |
| `confidence` | Confidence score in [0.0, 1.0], default `1.0` |
| `source` | Optional provenance marker |

### Builder Methods

Facts support chaining via builder methods:

```rust
let fact = Fact::new("alice", "works_at", "Acme", Utc::now())
    .with_confidence(0.9)
    .with_source("user:owner");
```

- `with_confidence(f32)` -- sets confidence, clamped to [0.0, 1.0]. Non-finite values are ignored.
- `with_source(impl Into<String>)` -- sets the source provenance marker.

### Validity Checks

- `is_currently_valid()` -- returns `true` when both `valid_to` and `expired_at` are `None`.
- `was_valid_at(at)` -- returns `true` when the fact was valid at the given point in time on the valid-time axis.

## The Value Enum

A fact's object position holds a `Value`:

```rust
pub enum Value {
    Text(String),
    Number(f64),
    Boolean(bool),
    Entity(String),
}
```

| Variant | Use Case | Example |
|---|---|---|
| `Text(String)` | Free-form string values | `"Senior Engineer"`, `"London"` |
| `Number(f64)` | Numeric values | `42.0`, `3.14` |
| `Boolean(bool)` | Boolean flags | `true`, `false` |
| `Entity(String)` | Reference to another entity by canonical name | `"acme-corp"` |

`Value` implements `From<&str>`, `From<String>`, `From<f64>`, and `From<bool>`, so callers can pass these types directly where `impl Into<Value>` is accepted.

## How Entity References Create Graph Edges

The `Entity(String)` variant is how Kronroe expresses graph edges. When a fact's object is `Value::Entity("acme-corp")`, it creates a directed edge from the subject to the referenced entity.

```
alice --[works_at]--> acme-corp
```

This is represented as:

```rust
db.assert_fact("alice", "works_at", Value::Entity("acme-corp".into()), Utc::now())?;
```

The predicate names the relationship. The subject and the `Entity` reference are both entity identifiers -- there is no separate entity registration step. Entities come into existence implicitly when they appear as a subject or an `Entity` reference.

Multiple facts with different predicates form the full graph structure:

```
alice --[works_at]----> acme-corp
alice --[lives_in]----> london
alice --[reports_to]--> bob
bob   --[works_at]----> acme-corp
```

## FactId Format

Every fact gets a `FactId` on creation. The format is `kf_` followed by 26 base32 characters:

```
kf_01jq5v8g7k3m2n4p6r8s0t1w2x
```

Properties:

- **Lexicographically sortable** -- IDs sort in monotonic insertion order when compared as strings.
- **Unique** -- generated with sufficient entropy to avoid collisions.
- **Stable** -- once assigned, a fact's ID never changes, even across corrections or invalidations.
- **Parseable** -- `FactId::parse(s)` validates the format. Invalid IDs produce a `FactIdParseError`.

FactIds are the primary handle for operations like `fact_by_id`, `invalidate_fact`, and `correct_fact`.

## Thinking About the Graph Structure

Kronroe is a temporal property graph. The graph has these characteristics:

- **Entities** are nodes, identified by canonical name strings. They exist implicitly.
- **Facts** are typed, directed edges or properties on nodes. A fact with `Value::Entity(...)` as its object is an edge; a fact with `Text`, `Number`, or `Boolean` is a property.
- **Predicates** name the relationship or attribute type (e.g. `works_at`, `job_title`, `has_ehcp`).
- **Every edge and property is bi-temporal.** The graph is not a single snapshot -- it is a full history of what was true and when, queryable at any point in time.

To query the graph:

- `current_facts(subject, predicate)` -- get current edges/properties for a specific relationship.
- `all_facts_about(subject)` -- get every fact ever recorded about an entity, across all predicates.
- `facts_at(subject, predicate, at)` -- get edges/properties that were valid at a specific time.
- `search(query, limit)` -- full-text search across all entities, predicates, and values.
