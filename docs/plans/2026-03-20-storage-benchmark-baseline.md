# Storage Benchmark Baseline

Date: 2026-03-20  
Branch: `codex/storage-facade-phase1`  
Status: Historical baseline note

## Summary

This note captured the first benchmark baseline after the storage facade and
observability groundwork landed.

Its purpose was to establish where time was really going before the
Kronroe-native backend work accelerated.

## What It Established

The baseline showed:

- append-style fact writes were already relatively cheap
- idempotent retries were not a problem area
- scan-heavy historical queries were the dominant hotspot
- correction chains were disproportionately expensive because of repeated broad
  scans

That benchmark evidence is what pushed the later work toward:

- subject/predicate derived indexes
- version-chain lookups
- exact fact-id lookup indexes
- storage parity for registries and embeddings

## Historical Use

This file is kept as context for how the storage replacement priorities were
chosen. It should not be read as guidance for preserving the legacy backend.
