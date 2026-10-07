---
id: ADR-2133
title: The memory cloud serves a live stratified snapshot of the RuVector sidecar, and query results come from the sidecar's own index
date: 2026-10-07
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 4f705ccacc07984797b8f6417f90f48b2756299c
verified_paths: [crates/visionclaw-memory-cloud/src, src/services/memory_cloud_service.rs, src/handlers/memory_cloud_handler.rs, src/utils/auth.rs, tests/memory_cloud_live_test.rs, docker-compose.unified.yml, src/middleware/rate_limit.rs, tests/memory_cloud_auth_test.rs]
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
   hits. A global query whose HNSW candidates were mostly excluded (fewer than k rows after the
   filter) reruns as an exact scan. Every response states `sidecar.method` (`hnsw` | `exact`), so the
   client never labels an exact answer as the index's. `score` is cosine similarity. `sampleIndex` places a hit in the cloud, or is null. The UI
   shows the sidecar's list beside any browser trajectory and never presents the sample search as
   the store's answer.
3. **Measured recall.** After each build the server runs 20 sampled rows through the HNSW path and
   through an exact sequential scan (`SET LOCAL enable_indexscan = off; enable_bitmapscan = off`),
   with k = 10. It checks both plans with `EXPLAIN` and caches tie-aware recall@10 (a hit counts
   when its distance ≤ the 10th exact distance + 1e-6) and the mean latencies.
   `GET /api/memory-cloud/health` reports it with per-namespace counts, the extension version and
   embedder reachability, all served from state cached by the refresh cycle: health never runs a
   `GROUP BY` or calls the embedder per request, so it cannot be used to load either.
4. **Privacy.** `MEMORY_CLOUD_EXCLUDE_NAMESPACES` (default `personal-context`; an empty value keeps
   the default) is never sampled, searched, probed or listed in health. Sidecar sessions run with
   `default_transaction_read_only=on` and connect as the SELECT-only `ruvector_reader` role that the
   sidecar's owner (agentbox) provisions. The connection string comes only from
   `RUVECTOR_PG_CONNINFO`. No password default appears anywhere. Without it, the data endpoints
   return 503 and health returns 200 with `error: "not_configured"`.
5. **Access.** Every endpoint, health included, requires a **NIP-98-signed power user** (Admin
   role or a `POWER_USER_PUBKEYS` key), the broker-inbox bar, regardless of `RBAC_PUBLIC_READS`
   **and of the dev shortcuts**. Any fresh keypair already reaches the default `Editor` tier, so
   `ReadOnly` would protect nothing. `VISIONCLAW_DEV_MODE` admits every caller as one sentinel
   identity, and `DEV_AUTH_LOOPBACK` trusts a header-chosen pubkey. Neither proves who the caller
   is, so the handler reuses the gate's identity only for a `Nostr` header and a non-sentinel
   identity, and otherwise verifies the signature itself. Dev mode is unchanged for the rest of
   the API; a dev operator adds their own pubkey to `POWER_USER_PUBKEYS`. `POST /query` carries a
   per-pubkey budget (`MEMORY_CLOUD_QUERY_PER_MINUTE`, default 30) on the existing `RateLimit`
   middleware. Admission runs before the limiter, so refused callers spend nothing and dev mode's
   shared identity cannot pool the budget.
   **Disclosure.** Clients get fixed messages (`memory store unavailable`, …), with driver detail
   logged only. `health.sidecar.error` is one of `not_configured`, `building` or `unreachable`. The
   embedder URL is not on the wire, and the vectors blob is `Cache-Control: no-store`.
6. **Wire types.** The contract is hand-written in
   `client/src/features/visualisation/memoryCloud/types.ts` and mirrored by
   `visionclaw_memory_cloud::wire`, which has a field-name test. `generate_types` emits only a
   fixed settings string and cannot derive handler structs.

## Consequences
- The cloud shows real memory, so the owner sees `project-state`, `patterns`, `dream-cycle` and the
  knowledge corpora, and every query names the store's own neighbours.
- Anonymous and Editor viewers no longer see a cloud. That is the price of private content. A
  public deployment needs a separate, explicitly public namespace list.
- The recall figure is a property of the sidecar's index, not of this code. It read about 0.70
  before ab-ruvector's serial reindex and 0.89–0.94 after it. This feature never writes to the
  index. The remaining zero-recall probes were investigated (`010609be5`):
  - They are graph unreachability. The index returns a full k rows whose best distance is ~0.75,
    while the exact best is 0.000 (the query is a stored row). It is not filter starvation.
  - `ruvector.ef_search` (0.3.0, default 40) is ignored by the HNSW scan. ef 10 and ef 1000
    return identical results at the same latency in fresh sessions, so no per-query `SET LOCAL`
    is used.
  - The unreachable rows are not duplicates, not post-reindex inserts, not in denser clusters,
    and are unit norm. About 17 % of `hooks:pre-task` and `session-states`, about 2 % of
    `project-state` and 0 % of `ruvnet-kb` are unreachable.
  - The fix belongs to the index owner (ruvector's graph construction or search). The probe
    keeps reporting it.
- The pool stays at 4 connections. Eight concurrent live queries complete in 396 ms wall (max
  395 ms each), and a power-user-only, rate-limited surface does not justify more.
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

At `5a32cbac7` (security review M1, M2, L2, L3): `tests/memory_cloud_auth_test.rs` failed before the
change (an anonymous caller under dev mode reached the handler, 503 instead of 401). It now
passes: with `VISIONCLAW_DEV_MODE=1`, `DEV_AUTH_LOOPBACK=1` and `RBAC_PUBLIC_READS=1`, all four
endpoints answer 401 to anonymous and dev-token callers, 403 to an Editor signer, and admit a
signed power user. A power user's third query in a minute gets 429 against a budget of 2, while a
second power user is unaffected. The live test passed over HTTP through the real gate: the
snapshot, a `no-store` vectors blob, 409 on a stale id, and health with no URL and no excluded
namespace. Recall@10 was 0.940 (HNSW 21 ms/query against exact 370 ms/query).

At `010609be5`: with `ruvnet-kb` excluded, a RuVector-themed global query got 2 of 10 rows from
HNSW (red). The exact fallback now returns 10/10 in 284 ms, labelled `exact`. Global queries
otherwise report `hnsw`, and restricted queries report `exact`. The live test passed.

## Re-verification — 2026-10-07 at c16b25774 (NIP-98 single verification per request)

**Governed change:** `verify_access` (`src/utils/auth.rs:191`) no longer verifies a NIP-98 token a second time in one request. When the request carries `Authorization: Nostr …` and an outer layer (`RbacGate`, an enclosing `RequireAuth`) already left an `AuthenticatedUser` in the request extensions, it reuses that identity and checks only the required level via `effective_access_level`; extensions are server-side and cannot be populated from headers. Before this, every `RequireAuth` scope under `/api` answered NIP-98 callers 401 "Token replayed" (proved by `tests/rbac_gate_require_auth_stacking_test.rs`, now green). The memory-cloud handler's `require_private_reader` keeps calling `effective_access_level` directly; its doc no longer claims `verify_access` must be avoided, because it is now safe to call twice. Line citations into `src/utils/auth.rs` after line 265 shift by +25 (the NIP-98 branch of `verify_access` gains the reuse block; e.g. `nip98_request_url(req)` in that branch moves from `:266` to `:291`); earlier lines are unchanged. The decision holds.

## Re-verification — 2026-10-07 (ae8349b85)

`build_pool` now refuses a connection string with no `user` or `dbname`. An unquoted key=value value in `.env` was cut at its first space and reached the container as `host=…` only, which parsed and then surfaced as an opaque `unreachable`. It now reports `not_configured` with a quoting hint. Read-only sessions, no default password and the exclusions are unchanged; the decision holds.

## Amendment — 2026-10-07: dev bypass admits the memory cloud (operator decision)

**Amends Decision 5 (Access).** At the operator's request, `VISIONCLAW_DEV_MODE=1` now admits every caller to all four endpoints as a power user (`require_power_user`, `src/handlers/memory_cloud_handler.rs`), so a local dev box shows the cloud without a signer. The bypass exists only in debug and `dev-auth` builds, and a release build refuses to boot with the variable set (`utils::auth::dev_full_bypass_active`), so production keeps the signed-power-user rule unchanged. `DEV_AUTH_LOOPBACK` and `RBAC_PUBLIC_READS` still unlock nothing. Under dev mode every caller shares the dev identity's query budget.

**Accepted risk.** The dev compose service publishes its ports on every interface, so with dev mode on, anyone who can reach the dev host can read memory keys, snippets and vectors from non-excluded namespaces (security review M1). `personal-context` and the other excluded namespaces stay excluded. Bind the dev ports to loopback or the rail, or unset `VISIONCLAW_DEV_MODE`, on a shared network.

**Evidence.** `tests/memory_cloud_auth_test.rs` now runs in two phases. With dev mode on, an anonymous caller is admitted on all four endpoints (it failed before this change, getting 401). With dev mode off, anonymous and dev-token callers get 401, an Editor signer 403, and a signed power user is admitted, with the per-pubkey budget enforced. Alongside, `verify_access` now answers a request with no credentials at all with 401 rather than 403 (`src/utils/auth.rs`, legacy-header branch), which the same test pins.

## Re-verification — 2026-10-07 (006332e88)

`wire.rs` gains only `client_fixture_round_trips`, a test that round-trips the client wire fixture through every wire struct so that a field added or removed on either side fails. No wire shape changed. The client now shows `sidecar.method`, the health error category and a 429 rate-limit state, and leaves unsampled sidecar hits out of the agreement count; all of these match this record. The decision holds.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `src/middleware/rate_limit.rs`: the inherent `RateLimit::default()` becomes `impl Default` (still 100 req/min). No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record). **Still holds.**

## Re-verification — 2026-10-07 at 4f705ccac (clippy sweep, round 2)

`chore/clippy-sweep` (merged with main at c025c1694) changes this record's governed paths only as follows:

- `crates/visionclaw-memory-cloud/src/wire.rs`: test-only `type RoundTrip` alias (clippy `type_complexity`).
- `src/middleware/rate_limit.rs`: since the recorded commit: `entry().or_default()`, the inherent `default()` becomes `impl Default`, and the uncalled `extract_identifier` is deleted.
- `src/handlers/memory_cloud_handler.rs`: `cargo fmt` only (it arrived from main unformatted).

None of these changes touches the decision this record makes. Every deletion had no caller in any build (debug, release, `--features redis`). `cargo clippy --workspace --all-targets -- -D warnings` is clean in debug and release; `cargo test --workspace --tests` on the merged tree: 3242 passed, 0 failed, 83 ignored. **Still holds.**
