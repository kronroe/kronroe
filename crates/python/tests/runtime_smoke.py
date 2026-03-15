#!/usr/bin/env python3
"""Runtime smoke tests for local kronroe Python bindings.

This script is intentionally lightweight and dependency-free so it can run in
local dev loops and CI after `maturin develop`.
"""

from __future__ import annotations

import tempfile
from pathlib import Path

from kronroe import AgentMemory, KronroeDb


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def run_kronroe_db_checks(db_path: Path) -> None:
    db = KronroeDb.open(str(db_path))
    fact_id = db.assert_fact("alice", "works_at", "Acme")
    require(isinstance(fact_id, str) and fact_id, "assert_fact should return a non-empty id")

    hits = db.search("alice Acme", 10)
    require(len(hits) >= 1, "search should return at least one hit")
    require(hits[0]["subject"] == "alice", "search hit should include alice")


def run_agent_memory_checks(db_path: Path) -> None:
    mem = AgentMemory.open(str(db_path))

    # Base assertion path
    base_id = mem.assert_fact("bob", "works_at", "OldCo")
    require(isinstance(base_id, str) and base_id, "assert_fact should return an id")

    # Confidence/source path
    conf_id = mem.assert_with_confidence(
        "bob", "works_at", "Acme", 0.92, source="user:smoke"
    )
    require(isinstance(conf_id, str) and conf_id, "assert_with_confidence should return an id")

    facts = mem.facts_about("bob")
    require(len(facts) >= 2, "expected at least two facts for bob")
    latest = next(
        (
            row
            for row in facts
            if row["predicate"] == "works_at"
            and row["object"] == "Acme"
            and abs(float(row["confidence"]) - 0.92) < 1e-6
            and row.get("source") == "user:smoke"
        ),
        None,
    )
    require(latest is not None, "expected confidence/source metadata round-trip")

    # Scored recall with confidence filter
    scored = mem.recall_scored(
        "Acme",
        limit=10,
        min_confidence=0.9,
        confidence_filter_mode="base",
    )
    require(len(scored) >= 1, "expected at least one high-confidence scored recall result")
    row = scored[0]
    require("fact" in row and "score" in row, "scored row should include fact + score")
    require(
        float(row["fact"]["confidence"]) >= 0.9,
        "scored fact should satisfy min_confidence",
    )

    # Task-focused decision report
    task_report = mem.recall_for_task(
        "prepare account update",
        subject="bob",
        horizon_days=90,
        limit=8,
    )
    require(isinstance(task_report, dict), "recall_for_task should return a dict")
    require(task_report.get("subject") == "bob", "task report should keep subject context")
    require(
        isinstance(task_report.get("key_facts"), list),
        "task report should include key_facts list",
    )
    require(
        isinstance(task_report.get("recommended_next_checks"), list),
        "task report should include recommended_next_checks list",
    )

    # Context assembly
    context = mem.assemble_context("Acme", 200)
    require(isinstance(context, str) and context, "assemble_context should return non-empty text")
    require("Acme" in context, "assembled context should contain Acme")

    # Correction path
    corrected_id = mem.correct_fact(base_id, "NewCo")
    require(isinstance(corrected_id, str) and corrected_id, "correct_fact should return new id")
    bob_facts = mem.facts_about("bob")
    require(
        any(f["predicate"] == "works_at" and f["object"] == "NewCo" for f in bob_facts),
        "correct_fact should produce replacement value",
    )

    # Invalidation path
    retire_id = mem.assert_fact("charlie", "works_at", "Acme")
    require(isinstance(retire_id, str) and retire_id, "assert for invalidation should return id")
    mem.invalidate_fact(retire_id)
    charlie_hits = mem.recall("charlie Acme", limit=10)
    active_charlie_hits = [
        row
        for row in charlie_hits
        if row.get("subject") == "charlie" and row.get("expired_at") is None
    ]
    require(
        len(active_charlie_hits) == 0,
        "invalidated charlie fact should not appear as active in recall",
    )


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="kronroe-py-smoke-") as tmp:
        db_path = Path(tmp) / "runtime-smoke.kronroe"
        run_kronroe_db_checks(db_path)
        run_agent_memory_checks(db_path)
    print("kronroe Python runtime smoke: PASS")


if __name__ == "__main__":
    main()
