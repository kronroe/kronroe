# Kronroe Storage Engine

This note describes the current shipped Kronroe storage engine.

## Identity

Kronroe stores on-disk data in a newline-delimited append-log file.

- File header: `kronroe-append-log-v1`
- Current schema version: `2`
- Default open path: `TemporalGraph::open(...)`
- In-memory path: `TemporalGraph::open_in_memory()`

The append-log file is the source of truth. In-memory indexes are derived by
replaying the persisted record stream.

## Replay Model

The file is an ordered stream of JSON records, one record per line.

- Record order is authoritative.
- The latest record wins for replacement-style state.
- Historical facts remain replayable for temporal queries.
- Derived indexes are rebuilt only from replayed records, not from any separate
  sidecar file.

Persisted state includes:

- facts and replacements
- idempotency mappings
- contradiction and uncertainty registries
- embeddings and embedding-dimension state

## Recovery Rules

On open, Kronroe distinguishes these cases:

- Empty or new file: starts with empty state
- Valid file: replays all records
- Truncated final newline: accepted if the final JSON record is otherwise valid
- Truncated final JSON tail: ignored only when it is the incomplete final record
  and all prior records are valid
- Malformed record before the final tail: deterministic storage corruption error
- Wrong header: deterministic storage backend mismatch error
- Unsupported schema version: schema mismatch error

Recovery policy:

- A partial final record is treated as an interrupted append and ignored
- Corruption before the final record is not auto-repaired
- Reopen after recovery must reproduce the same logical state as the last valid
  record boundary

## Durability Contract

Each logical write appends a complete JSON record plus a trailing newline and
fsyncs the file before success is returned.

This applies to:

- fact writes
- fact plus idempotency writes
- fact plus embedding writes
- registry writes

Compaction writes to a fresh temp file, fsyncs it, atomically replaces the
original file, and then syncs the parent directory.

## Compaction

Compaction is currently an internal storage operation.

Guarantees:

- writes to a temp file and replaces atomically on success
- leaves the original file untouched if compaction fails
- preserves current facts
- preserves historical facts needed for `facts_at`, contradiction checks, and
  confidence decay
- preserves idempotency mappings
- preserves registries
- preserves embeddings and vector-index rebuild inputs

Compaction does not introduce a new binary format in this phase; it rewrites a
valid append-log file.

## Locking

Kronroe currently enforces single-writer semantics for on-disk databases.

- A second write-capable open to the same database path fails fast
- Same-process repeated opens to the same file are rejected
- In-memory databases are unaffected and may be opened independently
- Locks are released on drop

The lock is implemented on a lock file adjacent to the database file so the
main file can still be replaced atomically during compaction.

## Source of Truth vs Derived State

Source of truth:

- append-log record stream

Derived state:

- subject/predicate candidate indexes
- current-fact indexes
- version-chain indexes
- fact-id lookup indexes
- vector index
- registry caches

If derived state is lost, it must be rebuildable by replaying the append-log.
