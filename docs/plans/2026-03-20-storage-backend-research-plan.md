# Storage Backend Research Plan

Date: 2026-03-20  
Owner: Core + Product Infrastructure  
Status: Historical design note

## Summary

This note captured the research and design path toward a fully Kronroe-owned
storage engine and file format.

The plan it described has now been completed in the shipped runtime:

1. define a Kronroe storage contract
2. add observability and benchmark workloads
3. prototype an append-log backend
4. measure the real hotspots
5. cut over to the Kronroe-native backend once confidence was high enough

## What The Research Was For

At the time of writing, Kronroe needed to answer four questions:

- what guarantees the storage layer had to preserve
- which workloads actually mattered for product performance
- whether a Kronroe-native backend could outperform the legacy storage path
- how to remove legacy backend assumptions without breaking iOS, Android, WASM,
  Python, or MCP surfaces

## Storage Guarantees Identified

The research established that any shipping Kronroe backend needed:

1. atomic persistence for facts, idempotency, registries, and embeddings
2. deterministic ordering and stable fact identity
3. rebuildable vector state on open
4. fast current-state and historical retrieval
5. clear schema/version handling
6. in-memory operation for tests and browser/WASM use

## Benchmark Focus Areas

The planned measurement areas were:

- assert-heavy ingestion
- correction-heavy timeline churn
- current-state scans
- historical point-in-time scans
- idempotent retries
- mixed real-task sessions
- embedding reopen and vector rebuild cost

## Outcome

The benchmark work that followed showed that the most important hotspot was not
plain writes. It was scan-heavy historical and correction-path access patterns.

That led directly to the chosen implementation direction:

- keep append-style fact persistence
- add derived indexes for subject/predicate lookups
- add version-chain and fact-id lookup indexes
- close parity gaps for registries and embeddings
- cut over the default open path to the Kronroe append-log backend

## Guardrails Learned

Two operating rules came out of this effort and should continue to guide future
dependency replacement work:

1. remove dead legacy paths as part of the change, not later
2. treat cross-surface builds, especially iOS framework generation, as required
   verification instead of optional cleanup

## Current Relevance

This file remains as historical context for why the storage work happened in
stages. It should not be read as an active proposal to preserve legacy backend
support, because the legacy backend removal is now complete.
