---
id: ADR-2136
title: The memory-cloud query response carries the query's point in the snapshot's cloud
date: 2026-10-08
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 8e5ab0dc22ed80c3304e527b8713c13b4a8ffe07
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

Amendment from the live check: a typed query searched globally answered "0 of 50 in the sample" (the global top-50 lands in the thinly sampled reference corpus), so it drew no route; the `patterns` preset drew 14 hops from the query point. Typed queries are therefore scoped: the keyboard's Scope key cycles all of memory and the snapshot's eight most-sampled namespaces, starting in `project-state` when present (`xr-client/scripts/memory_search.gd:142` `scopes`, `:31` `DEFAULT_SCOPE`; `hud.gd:783`). Search sends `memory_typed:<scope>|<text>`. Test: `test_memory_search.gd:282`. The wire is unchanged. GUT on HP: 222/222. Follow-up at 0f8ea5af8: the scopes list puts the estate namespaces (`ESTATE_SCOPES`, `memory_search.gd:35`: project-state, patterns, coordination) first when present, because live the most-sampled eight left out `patterns`.

## Amendment — 2026-10-08: every hit is placed (`hit.position`)
A global typed query drew nothing live: its top-50 were all outside the 6,000-row sample, and
only sampled hits had a point. The server already holds the basis, so it now places every hit
the same way it places the query.

- **Wire.** Each `MemoryCloudHit` gains optional `position: [x, y, z] | null`
  (`crates/visionclaw-memory-cloud/src/wire.rs:110`): the hit's own embedding in the snapshot's
  cloud coordinates. It is on its row when sampled, where it would sit when not, and `null` when
  it can't be placed. Additive; older clients ignore it.
- **Server.** Both search queries also select `embedding::text`
  (`src/services/memory_cloud_service.rs:79`, mapped at `:621`). `BuiltSnapshot::project_literal`
  (`snapshot.rs:201`) parses and L2-normalises exactly as the snapshot build does, then projects
  with the snapshot's `Projector`. A sampled row's literal lands on its snapshot position bit for
  bit (`snapshot.rs:301` test, extended to hit literals).
- **Headset.** The route runs query point → every placed hit in rank order
  (`RouteFrame::hit_points`, `memory_route.rs:291`; built in `memory_query.rs:261`), so a query
  with no sampled hit still draws. A hit outside the sample has no sprite of its own. It gets a
  ghost mark instead: a dimmer, smaller, cool hollow ring (`ROUTE_GHOST`, `memory_route.rs:45`).
  On the HUD list it shows ☐; ● means sampled, — means unplaceable. Pressing it sends the guide
  cue to its point. The caption reads "query point → sidecar top-k (k drawn, n in sample; not a
  search path)" and the Memory-row line "Route: query point → sidecar top-k (k drawn, n in
  sample)". Without hit positions the sampled-row route is kept. The Scope key stays: a scoped
  query still has more of its hits in the sample. Marks stay ≤ k ≤ 50 (`MAX_SIDECAR` 64), so
  FrameBudget is unchanged.
- **Desktop** (amended 2026-10-08 at 4faff1eb6). Same geometry and wording as the headset:
  in the space view a thin gold line runs from the query point through every placed hit in rank
  order, a hit outside the sample is a dimmer, smaller, cool hollow ring, and the query point is
  a white dot (`TrajectoryLayer.tsx:717`). The placement and caption mirror
  `memory_query::caption` and `memory_route::agreement_line` (`sidecarRoute.ts:64`). The panel
  labels a ghost "outside the sample · drawn" (`MemoryExplorerPanel.tsx:347`), and clicking one
  flies to its point (`cameraFocus.ts:139`). The explorer's own HNSW tube over the sample is
  unchanged and still the thick route.

Verification at 8e5ab0dc2: `cargo test -p visionclaw-memory-cloud` passes 47 tests and 19 doc tests,
both fixtures round-trip, and the hit-literal case is in `snapshot.rs:301`. Server lib passes
1,568 and clippy `-D warnings` is clean. The xr-client workspace passes 551 (`memory_route.rs:1885`,
`memory_query.rs:719`), and clippy and fmt are clean. Client vitest passes 1,264 and tsc is clean.
On HP, GUT runs 222/222 and the guard reports 31/31; the glyph guard rejected `○`, so the list
uses `☐`. The HP benchmark query route, with half its 49 hits as ghosts, holds 34 draw calls,
94,566 triangles and p99 2.78 ms; every gate passes.
