# Storage Backend Comparison Baseline

Date: 2026-03-21  
Branch: `codex/storage-phase3-baseline`  
Status: Historical comparison note

## Summary

This note recorded the first side-by-side benchmark comparison between the
legacy storage backend and the experimental Kronroe append-log backend during
Phase 3 of the storage replacement.

It existed to answer one question: was the append-log direction materially
better on real Kronroe workloads before the default backend was switched?

## What The Comparison Measured

The benchmark harness compared:

- assert-heavy ingestion
- correction-heavy timeline churn
- current-state scans
- historical point-in-time scans
- idempotent retries
- mixed real-task sessions
- embedding reopen behavior where supported

The key signals recorded were:

- wall-clock duration
- backend mode (`InMemory` vs `OnDisk`)
- per-operation timings
- rows scanned

## Main Result

The comparison showed that the Kronroe append-log backend was already
materially better on the workloads that mattered most, especially:

- correction-heavy histories
- point-in-time lookups
- hot current-state scans

The important nuance was that early wins came even before the final index work
was complete, which gave the team confidence to continue down the
Kronroe-native path instead of preserving the legacy backend as the long-term
default.

## Why This File Still Exists

This is now a historical engineering note. It explains why the project moved
from “prototype both paths” to “make append-log the default,” but it should not
be read as an active dual-backend strategy.
