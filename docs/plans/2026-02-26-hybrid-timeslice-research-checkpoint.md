# Hybrid Time-Slice Research Checkpoint (Phase 1.1)

Date: 2026-02-26
Status: Checkpoint before time-slice optimization work

## Why this checkpoint exists
Our latest hybrid sweep is strong on aggregate ranking quality, but fails the promotion gate on hard slices:
- semantic lift vs strongest baseline: +2.00% (target >=10%)
- time_slice lift vs strongest baseline: -68.70% (target >=10%)
- p95 regression vs text baseline: +6.45% (guardrail <=20%, pass)

This means we should not promote hybrid yet. We need time-aware retrieval improvements that are mathematically grounded, not only heuristically tuned.

## Research-backed design direction

### 1) Temporal intent should be modeled explicitly
Temporal IR work shows relevance and freshness are distinct objectives and should be balanced based on query temporal profile.
- Action for Kronroe: classify query intent (`timeless`, `current`, `historical_point`, `historical_interval`) and use intent-aware weights.

### 2) Time should be a first-class score, not only decay
Temporal graph/network literature distinguishes path optimality notions (foremost/fastest/shortest in time-varying settings).
- Action for Kronroe: introduce a temporal-consistency score (valid at time t, interval overlap quality, recency policy fit) as a separate channel.

### 3) Temporal embedding ideas should influence paraphrase alignment
Temporal KG and temporal graph embedding papers show that timestamp-aware representations can outperform static embeddings for evolving facts.
- Action for Kronroe: enrich vector-aligned time-slice queries and evaluate temporal feature injection options in the reranker.

## Implementation plan (next)
1. Eval schema updates:
- add explicit `slice` labels in `hybrid_eval_v1.jsonl` for all cases
- add `temporal_intent` labels for time-sensitive cases

2. Runner scoring updates (experimental):
- compute `semantic_score` and `temporal_consistency_score`
- blend with intent-aware weights (guarded behind experiment flag)

3. Targeted sweep:
- optimize primarily for time_slice nDCG@3 while holding latency guardrail
- reapply promotion gate:
  - semantic lift >=10%
  - time_slice lift >=10%
  - p95 regression <=20%

## Source set (math + temporal graph + temporal retrieval)
1. Cooke, K. L., & Halsey, E. (1966). The shortest route through a network with time-dependent internodal transit times.
- [DOI](https://doi.org/10.1016/0022-247X(66)90009-6)

2. Bui-Xuan, B.-M., Ferreira, A., & Jarry, A. (2003). Computing shortest, fastest, and foremost journeys in dynamic networks.
- [PDF](https://www-npa.lip6.fr/~buixuan/files/BFJ03.pdf)

3. Holme, P., & Saramäki, J. (2012). Temporal networks.
- [DOI](https://doi.org/10.1016/j.physrep.2012.03.001)

4. Casteigts, A., Flocchini, P., Quattrociocchi, W., & Santoro, N. (2012). Time-varying graphs and dynamic networks.
- [DOI](https://doi.org/10.1080/17445760.2012.668546)

5. Li, X., & Croft, W. B. (2003). Time-based language models.
- [DOI](https://doi.org/10.1145/956950.956951)

6. Dai, N., Shokouhi, M., & Davison, B. D. (2011). Learning to rank for freshness and relevance.
- [DOI](https://doi.org/10.1145/2009916.2009933)

7. Xu, D., Ruan, C., Korpeoglu, E., Kumar, S., & Achan, K. (2020). Inductive representation learning on temporal graphs (TGAT).
- [ICLR page](https://iclr.cc/virtual/2020/poster/1456)

8. Dasgupta, S. S., Ray, S. N., & Talukdar, P. (2018). HyTE: Hyperplane-based temporally aware knowledge graph embedding.
- [ACL Anthology](https://aclanthology.org/D18-1225/)

9. Trivedi, R., Dai, H., Wang, Y., & Song, L. (2017). Know-Evolve: Deep temporal reasoning for dynamic knowledge graphs.
- [PMLR](https://proceedings.mlr.press/v70/trivedi17a.html)

10. Piryani, B., Abdallah, A., Mozafari, J., Anand, A., & Jatowt, A. (2025). It's High Time: A survey of temporal information retrieval and question answering.
- [arXiv](https://arxiv.org/abs/2505.20243)

## Decision
Proceed with experimental time-intent + dual-score reranking in private eval harness first, then promote to tracked code only after gate pass.
