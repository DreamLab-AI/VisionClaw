---
id: ADR-2133
title: The memory cloud serves a live stratified snapshot of the RuVector sidecar, and query results come from the sidecar's own index
date: 2026-10-07
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: c16b257741b980d4599122ab77aa2f5980f94f31
verified_paths: [crates/visionclaw-memory-cloud/src, src/services/memory_cloud_service.rs, src/handlers/memory_cloud_handler.rs, src/utils/auth.rs, tests/memory_cloud_live_test.rs, docker-compose.unified.yml]
owner: jjohare
review_trigger: the client explorer landing (memoryCloud panels); a change of embedding model or dimension; an HNSW rebuild of idx_memory_embedding_hnsw; any request to expose personal-context
repo: visionclaw
domain: PROTOCOL-registry
---

# ADR-2133 — The memory cloud serves a live stratified snapshot of the RuVector sidecar, and query results come from the sidecar's own index

## Context
`EmbeddingCloudLayer` read a static `client/public/embedding-cloud.json` (April 2026) from
`scripts/compute-umap-projection.mjs`: an unordered `LIMIT 50000`, a hard-coded fallback database
password, randomly initialised power iteration and max-radius scaling. The sidecar now holds ~215k
rows, ~162k of them `ruvnet-kb`; the snapshot held only hook/bash noise and none of `project-state`,
`patterns`, `ruvnet-kb`, `knowledge/pages` or `dream-cycle`. A browser-side search over a sample
cannot stand in for the real store: it sees ~3 % of the rows. Measured on 2026-10-07, 11 % of
sampled rows cannot find themselves in the sidecar's HNSW top-10, and ~14 % of rows share an exact
duplicate embedding.

## Decision
1. **Live snapshot.** `GET /api/memory-cloud` serves a sample rebuilt by a background task every
   `MEMORY_CLOUD_REFRESH_SECS` (default 900, minimum 60). The first request waits up to 45 s. The
   sample (`MEMORY_CLOUD_SAMPLE`, default 6000, clamped 500..20000) is stratified per namespace: a floor of
   `min(count, 40)`, then the rest ∝ √count, with noise namespaces (`hooks:*`, `command-*`,
   `legacy/*`, `performance-metrics`, `file-history`) weighted ×0.25. Rows are L2-normalised and
   projected by deterministic PCA. The 99th-percentile |coordinate| is scaled to 100.
   `GET /api/memory-cloud/vectors?snapshot=<id>` serves the same rows as little-endian f32. A stale
   id gets 409.
2. **Search honesty.** The browser may run its own HNSW over the sample to *illustrate* graph
   search. Every query is also answered by `POST /api/memory-cloud/query`, the sidecar's own top-k.
   Global queries use `idx_memory_embedding_hnsw` (cosine `<=>`). Namespace-restricted queries use
   an exact scan over `idx_memory_namespace`, because HNSW post-filtering returned 1 of 5 requested
   hits. `score` is cosine similarity. `sampleIndex` places a hit in the cloud, or is null. The UI
   shows the sidecar's list beside any browser trajectory and never presents the sample search as
   the store's answer.
3. **Measured recall.** After each build the server runs 20 sampled rows through the HNSW path and
   through an exact sequential scan (`SET LOCAL enable_indexscan = off; enable_bitmapscan = off`),
   with k = 10. It checks both plans with `EXPLAIN` and caches tie-aware recall@10 (a hit counts
   when its distance ≤ the 10th exact distance + 1e-6) and the mean latencies.
   `GET /api/memory-cloud/health` reports it with per-namespace counts, the extension version and
   embedder reachability.
4. **Privacy.** `MEMORY_CLOUD_EXCLUDE_NAMESPACES` (default `personal-context`; an empty value keeps
   the default) is never sampled, searched, probed or listed in health. Sidecar sessions run with
   `default_transaction_read_only=on`. The connection string comes only from
   `RUVECTOR_PG_CONNINFO`. No password default appears anywhere. Without it, the data endpoints
   return 503 and health returns 200 with `reachable:false`.
5. **Access.** Snapshot, vectors and query require `PowerUser` (Admin role or power-user pubkey),
   the broker-inbox bar, whatever `RBAC_PUBLIC_READS` says. Any fresh NIP-98 keypair already
   reaches the default `Editor` tier, so `ReadOnly` would protect nothing. When `RbacGate` has
   verified the request, the handler resolves that pubkey's role with `effective_access_level`.
   A second verification would trip the single-use replay cache. Health carries aggregates only
   and follows the central read policy. Under `VISIONCLAW_DEV_MODE=1` the dev bypass admits
   everything, as elsewhere.
6. **Wire types.** The contract is hand-written in
   `client/src/features/visualisation/memoryCloud/types.ts` and mirrored by
   `visionclaw_memory_cloud::wire`, which has a field-name test. `generate_types` emits only a
   fixed settings string and cannot derive handler structs.

## Consequences
- The cloud shows real memory, so the owner sees `project-state`, `patterns`, `dream-cycle` and the
  knowledge corpora, and every query names the store's own neighbours.
- Anonymous and Editor viewers no longer see a cloud. That is the price of private content. A
  public deployment needs a separate, explicitly public namespace list.
- The recall figure is a property of the sidecar's index, not of this code. It currently reads
  about 0.70, because some rows are unreachable in the graph, and it only improves when the agentbox
  owner rebuilds the index serially (the workspace index law). This feature never writes to the
  index.
- Snapshot cost: about 1.5 s per rebuild (counts 83 ms, stratified sample 0.9–1.2 s, PCA and
  encoding 0.4 s with the crate at `opt-level = 3`). The payload is 1.4 MiB of JSON plus a 9 MiB
  blob, both encoded once per snapshot.
- Follow-on: the client must switch from `/embedding-cloud.json` to the live endpoints, sending
  NIP-98 headers through `computeAuthHeaders` (raw `fetch` for the binary blob).

## Verification
At `9d1e0d625`: `cargo test -p visionclaw-memory-cloud` passed 43 unit tests and 18 doctests.
`cargo test --lib memory_cloud` passed 5 tests. `cargo test --test memory_cloud_live_test --
--ignored` passed against the live sidecar: 6000 rows over 465 namespaces, 0 skipped, with
`project-state` 183/2197, `patterns` 102/417 and `dream-cycle` 68/84 sampled. A query took 138 ms in
the sidecar and returned 10 hits. A `project-state`-restricted query returned 5/5 in 31 ms. The
probe measured recall@10 = 0.700 over 20 queries, HNSW 96 ms/query against exact 390 ms/query,
and both `EXPLAIN` checks held. An independent psql self-recall check found 89/100 rows. Clippy
and rustfmt are clean on the new files. The client side is not yet implemented, hence `partial`.

## Re-verification — 2026-10-07 at c16b25774 (NIP-98 single verification per request)

**Governed change:** `verify_access` (`src/utils/auth.rs:191`) no longer verifies a NIP-98 token a second time in one request. When the request carries `Authorization: Nostr …` and an outer layer (`RbacGate`, an enclosing `RequireAuth`) already left an `AuthenticatedUser` in the request extensions, it reuses that identity and checks only the required level via `effective_access_level`; extensions are server-side and cannot be populated from headers. Before this, every `RequireAuth` scope under `/api` answered NIP-98 callers 401 "Token replayed" (proved by `tests/rbac_gate_require_auth_stacking_test.rs`, now green). The memory-cloud handler's `require_private_reader` keeps calling `effective_access_level` directly; its doc no longer claims `verify_access` must be avoided, because it is now safe to call twice. Line citations into `src/utils/auth.rs` after line 265 shift by +25 (the NIP-98 branch of `verify_access` gains the reuse block; e.g. `nip98_request_url(req)` in that branch moves from `:266` to `:291`); earlier lines are unchanged. The decision holds.
