---
id: ADR-2136
title: The memory-cloud query response carries the query's point in the snapshot's cloud
date: 2026-10-08
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 131f066320290b1e6e80c1cafac3d1f3932b00f0
verified_paths: [crates/visionclaw-memory-cloud/src/pca.rs, crates/visionclaw-memory-cloud/src/snapshot.rs, crates/visionclaw-memory-cloud/src/wire.rs, src/services/memory_cloud_service.rs, xr-client/rust/src/memory_query.rs, xr-client/rust/src/memory_route.rs, xr-client/scripts/onscreen_keyboard.gd, xr-client/scripts/memory_search.gd]
owner: jjohare
review_trigger: the snapshot projection changing from PCA; a second consumer drawing from query.position; the headset gaining its own HNSW or the vectors blob
repo: visionclaw
domain: PROTOCOL-registry
---

# ADR-2136 — The memory-cloud query response carries the query's point in the snapshot's cloud

## Context
The headset never fetches the vectors blob or runs HNSW (ADR-2133), and the snapshot's PCA
basis was not on the wire, so it could not place a query in the cloud. Its memory search drew a
rank-order line through the sampled sidecar hits with no query point, and offered only presets
because it had no text entry. The operator's verdict from the headset was "no proper search
yet". The server already holds the basis (`project_to_3d` computed it) but threw it away after
building the snapshot.

## Decision
- **The basis is kept.** `Projection` records the column means and `Projection::projector()`
  returns a `Projector {mean, axes, scale}`. `BuiltSnapshot::projector` holds it (`None` for an
  empty sample). `Projector::project` uses the arithmetic `project_to_3d` used, in the same
  order, so a sampled row's vector maps onto that row's position bit for bit.
- **The wire gains one optional field.** `POST /api/memory-cloud/query` responds with
  `query.position: [x, y, z] | null` in the snapshot's cloud coordinates: the embedded query
  projected with that snapshot's basis and scale. It is `null` for an empty snapshot or a
  dimension mismatch, and never an error. Requests are unchanged. The field is additive: older
  clients ignore it, and the headset reads its absence as "no point".
- **The headset route starts at the point.** With a position, `memory_query::sidecar_route`
  builds query point → sampled hits in rank order (`RouteFrame::origin`). One sampled hit is
  enough. The root ring marks the query point and the answer ring the top hit
  (`RouteSamples::answer`). The caption says "route: query point → sidecar top-k (not a search
  path)". Without a position the old rank-order line, top hit last, is drawn. It is never
  presented as a search traversal.
- **Typed queries.** The Query page's Memory Search mode gains a press-fire on-screen keyboard
  (`onscreen_keyboard.gd`: digits, QWERTY, `' - . ?`, Space, Delete, Search, Cancel; 120
  characters). It opens in place of the lists inside the 532 px host. A typed query is
  searched globally and becomes a recent preset. Presets stay as shortcuts. No speech service
  is added (Invariant 10).
- **Desktop.** The desktop type carries the field; the explorer's own HNSW route is unchanged.

## Consequences
- The PROTOCOL-registry `/api/memory-cloud/query` row and the shared fixtures change with the
  wire type (`wire.json`, `memory_query_response.json`, `wireContract.ts`). A renamed or
  dropped field fails the round-trip tests on both sides.
- The point is a PCA projection, so a query near no sampled row can land in empty space. The
  route still runs to the real hits, which is the honest picture.
- Each query costs one 384 × 3 projection on the server, which is negligible.

## Verification
At the commit stamped in `verified_commit`:
- `cargo test -p visionclaw-memory-cloud`: `projecting_a_sampled_rows_vector_lands_on_its_position`
  (every row of a 120-row, 16-d sample, bit for bit), the `Projector` doc test, and the wire
  key and round-trip tests with `position`. Server lib:
  `query_position_of_a_sampled_rows_vector_is_its_snapshot_position`.
- xr-client: the `memory_query` and `memory_route` origin, answer-knot, gate and caption
  tests, and `tests/memory_query_parity.rs` reading `query.position` from the shared fixture.
- Client vitest: the `wire.test.ts` key lists (112 files, 1,264 tests). GUT on HP (218/218):
  `test_onscreen_keyboard.gd`, plus the keyboard, typed-query and scene-routing cases in
  `test_memory_search.gd`. `test_hud_batching` holds the keyboard to glyphs in the HUD font.
- HP benchmark `route_source=query` with a query point: 34 draw calls, 94,566 triangles, p99
  2.78 ms, every gate passes.
- Citations at 31bc3d3a1: `crates/visionclaw-memory-cloud/src/pca.rs:193` (`Projector`),
  `snapshot.rs:41`, `:286`; `wire.rs:121`; `src/services/memory_cloud_service.rs:637`, `:806`;
  `xr-client/scripts/hud.gd:714`.

## Re-verification — 2026-10-08 at 131f06632 (Scope key, deferred-walk fix)

Amendment from the live check: a typed query searched globally answered "0 of 50 in the sample" (the global top-50 lands in the thinly sampled reference corpus), so it drew no route; the `patterns` preset drew 14 hops from the query point. Typed queries are therefore scoped: the keyboard's Scope key cycles all of memory and the snapshot's eight most-sampled namespaces, starting in `project-state` when present (`xr-client/scripts/memory_search.gd:142` `scopes`, `:31` `DEFAULT_SCOPE`; `hud.gd:783`). Search sends `memory_typed:<scope>|<text>`. Test: `test_memory_search.gd:282`. The wire is unchanged. GUT on HP: 222/222.
