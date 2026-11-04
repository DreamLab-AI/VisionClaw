---
id: ADR-2138
title: Directed cycles are one server analysis that every client consumes
date: 2026-10-13
decision_status: proposed
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 5c94c3cbdafad99e6985fe509162831bf18c2ec8
verified_paths: [src/layout/scc.rs, src/handlers/api_handler/analytics/cycles_handlers.rs, src/settings/api/settings_routes.rs, client/src/api/layoutApi.ts, client/src/features/graph/hooks/useCyclicNodeIds.ts, xr-client/rust/src/render_store.rs, xr-client/scripts/hud.gd]
owner: jjohare
review_trigger: "Activation (staged → live): the XR Shapes tab first run on a headset against a live graph. Review: before any cycle datum is added to the 52-byte position frame"
repo: visionclaw
---

# ADR-2138 — Directed cycles are one server analysis that every client consumes

## Context

Users had no way to see which nodes sit on a directed cycle (a subclass loop is a modelling error), and the desktop and XR layout-mode vocabularies had drifted: the server whitelist rejected `spectral` and `clustered`, which the engine enum and the XR cycle button already used. A desktop-only answer would run Tarjan in the browser, which cannot serve the XR client or agents. Separately, the CPU `hierarchical_layout` fallback ranked by BFS from in-degree-zero roots, so a rootless cycle fell onto a trailing "unreachable" layer. Production `Hierarchical` does not run that code: it is GPU-resident (ADR-141 P4), ranked by `compute_dag_ranks` (BFS shortest-depth over `Edge::asserts_subsumption` edges, terminating on cycles, pinned by its unit tests).

## Decision

1. `src/layout/scc.rs` is the only directed-cycle implementation: iterative Tarjan, condensation longest-path ranks, `directed_adjacency` and `cycle_report`. The CPU `hierarchical_layout` fallback ranks with it. The GPU `compute_dag_ranks` is **unchanged**: its shortest-depth semantics are deliberate and tested.
2. `GET /api/analytics/cycles[?edgeType=<type>|all]` returns `{nodeCount, edgeCount, edgeFilter, cyclicNodeIds, components}`. It reads the graph through `GetGraphData` and is a public read like `/api/graph/data`. Its default edge set is `Edge::asserts_subsumption`, the predicate the GPU DAG ranker already uses, because the collapsed `hierarchical` label also carries equivalence and `sameAs`, and mutual links and `co_citation` edges are symmetric: each would read as a two-node cycle. REST only: the 52-byte position frame and the analytics buffer are not touched.
3. Clients consume that endpoint and carry no analysis of their own. Desktop: colour scheme `cycles` via `useCyclicNodeIds`. XR: a dedicated `RenderStore` cycle overlay (`set_cycle_overlay` / `clear_cycle_overlay`, separate from the query-variable channel; a query mark outranks it).
4. The server layout-mode whitelist, `layoutApi.ts` and the registry accept the full engine vocabulary, adding `spectral` and `clustered`.
5. XR HUD gains a ninth **Shapes** tab (Planes, Radial Shells, Cycles). Layout keeps Hierarchy & View and Layout Mode at the default separation.

## Consequences

- One cycle analysis for the overlay and agents, over the same edges the GPU layout ranks. The corpus-level `SUBCLASS_CYCLE` check (`vault conflicts`, `ontology_conflict_gate`) stays separate because it reads the disk, not the live graph; the agentbox `ontology-curator` agent documents both.
- Layout and overlay share the edge predicate and the Tarjan code only where they meet (CPU fallback); the GPU ranker does not call `layout::scc`. Moving it onto longest-path ranks would change shell placement and is out of scope.
- The report is recomputed per request (O(V+E)); the desktop refetches when node or edge counts change, XR on toggle or Refresh. No caching until measured.
- Nine tabs in a 1280 px viewport is tighter than eight; unmeasured on a headset.
- Not done, by design: edge bundling, density cloud, colormaps, layout recipe/provenance (needs a wire entry), seeding the GPU warm start (touches the `SimParams` ABI).

## Verification

Not yet pinned to a commit, so the ledger reads `proposed` / `none` until the owner accepts it and the change lands with a `verified_commit`. Evidence at time of writing: `cargo test --lib -- settings layout analytics` 40 pass (includes scc, `hierarchical_layout` on cyclic and rootless-cycle input, and the `cycles_handlers` edge selector); `xr-client/rust` `cargo test --lib cycle_overlay` 1 pass; client `tsc --noEmit` clean and 58 vitest tests pass across `src/api` and `src/features/graph/hooks` (the wider graph, api and control-centre run was 355 before the last additions), including `getCycles` against the real `StandardResponse` envelope; agentbox `tests/config/agent-reconcile.test.sh` 45 pass. Not verified: the GDScript (no Godot binary or gdlint in the container or on HP's PATH), the endpoint against a live graph (the VisionClaw container was not running), the real edge mix (7548 `hierarchical` edges were reported on one deployment; how many carry subClassOf provenance is unmeasured), and the Shapes tab on a headset.

## Close-out 2026-10-30

`proposed / none / inactive → proposed / complete / staged`; acceptance proposed under close-out rule 3a. The change landed on main in
`4306c65e9` ("feat(layout): directed-cycle analysis on the server, SCC-ranked hierarchy, cycles colour scheme").
Verified at `772039bd172027af034d64befacca1ebcf0bbc04`.

**Each decision against the code.**
1. `src/layout/scc.rs` exists. The GPU `compute_dag_ranks` (`src/actors/gpu/force_compute_actor.rs:438`) is unchanged: `git show 4306c65e9 --stat` does not list that file, and the commit names the function only in this record's text.
2. `GET /api/analytics/cycles` is routed at `src/handlers/api_handler/analytics/mod.rs:258` and handled in
   `cycles_handlers.rs`. On the running server (provenance below) it answered
   `{"nodeCount":9688,"edgeCount":10958,"edgeFilter":"subsumption","cyclicNodeIds":[],"components":[]}`.
3. The desktop hook is `client/src/features/graph/hooks/useCyclicNodeIds.ts`, used by `GemNodes.tsx`. The XR overlay
   is `RenderStore::set_cycle_overlay` (`xr-client/rust/src/render_store.rs:1273`).
4. `spectral` and `clustered` are in the settings whitelist (`src/settings/api/settings_routes.rs:413-414`) and in
   `client/src/api/layoutApi.ts:58`, `:76`. The separate advertising list `LIVE_LAYOUT_MODES` in
   `src/handlers/layout_handler.rs:13-28` still withholds `clustered`, by a deliberate comment dating from `ac3e12dd1`
   (`Clustered` is indistinguishable from `ForceDirected` in the actor). That is the advertised menu, not the whitelist
   this decision names.
5. The Shapes tab carries the Planes, Radial Shells and Cycles groups (`xr-client/scripts/hud.gd:582`, `:711`).

Runtime provenance: `visionclaw_container` runs `/app/target/dev-runtime/visionclaw-server` (build stamp `features=gpu,ontology,dev-auth`, i.e. the dev profile with the dev-auth bypass compiled in); its wrapper log (`/app/logs/rust.log`) reads `[RUST-WRAPPER][2026-10-30 19:01:06] Building from published inputs: source_sha=772039bd172027af034d64befacca1ebcf0bbc04 source_dirty=0`, so the responses below come from this commit.

Activation is `staged`, not `live`. The server half answers, but the Shapes tab has not been run on a headset, and
this record's own trigger names that run.

**Lead review (2026-10-30).** `decision_status` is returned to `proposed`. This record reserves acceptance to the
owner, and the close-out does not take owner decisions on the owner's behalf. The implementation and verification
evidence above stands; ratification is a one-line owner action, listed in VisionFlow
`docs/registers/park-2026-10-30.md`.

## Re-verification — 2026-10-31

`src/settings/api/settings_routes.rs` changed for ADR-2140 (`25cac8851`): the body of the physics PUT became `apply_physics_patch`. Nothing in the cycle routes or the layout-mode quality-gate handling this record governs was touched.

Re-stamped to `b06d44613` (2026-10-31, ADR-2140 review follow-ups). Re-run: `git diff --stat 772039bd172027af034d64befacca1ebcf0bbc04 b06d44613 -- src/layout/scc.rs src/handlers/api_handler/analytics/cycles_handlers.rs src/settings/api/settings_routes.rs client/src/api/layoutApi.ts client/src/features/graph/hooks/useCyclicNodeIds.ts xr-client/rust/src/render_store.rs xr-client/scripts/hud.gd` → only `src/settings/api/settings_routes.rs | 123 +++++++++++++++++++++++++++---------` (93 insertions, 30 deletions), in `normalize_physics_keys`, `PhysicsPatchError`, the PUT body and `apply_physics_patch` (GPU `Err` now a 500). The cycle routes and the quality-gate handling are untouched.

Re-stamped to `e49db1836` (2026-10-31, ADR-2166, spatial volumes can be entered). Re-run: `git diff --stat b06d44613dfdaab26faf4877d5599ecae980253d e49db18363bd3822016e9dde1e9eafe73322a5b8 -- src/layout/scc.rs src/handlers/api_handler/analytics/cycles_handlers.rs src/settings/api/settings_routes.rs client/src/api/layoutApi.ts client/src/features/graph/hooks/useCyclicNodeIds.ts xr-client/rust/src/render_store.rs xr-client/scripts/hud.gd` gives only `xr-client/scripts/hud.gd | 15 +++`, the Session-row Enter/Leave button. No Cycles button, request or label changed, and the cycle routes and the Rust renderer are untouched. **Decision unaffected.**

## Re-stamp — 2026-10-31 (ADR-2140 item 1)

**Governed change:** ADR-2140 item 1 changes the layout-mode handling this record's decision 4 describes. The whitelist is now the shared `QUALITY_GATE_LAYOUT_MODES` (seven strings) for the legacy quality-gate field, `physics.layoutMode` accepts either vocabulary and refuses an unknown value, and `layoutApi.ts` posts the engine names verbatim (the quality-gate remap is deleted). Decision 4 still holds: `spectral` and `clustered` are accepted, plus the two new modes `dagLeftRight` and `typeClustering`; `LIVE_LAYOUT_MODES` still withholds `clustered`. The cycle routes and the SCC ranking are untouched. **Decision holds**; the `settings_routes.rs:413-414` and `layoutApi.ts:58/:76` citations have moved. Re-run: `git diff --stat 75b72416b 93302275e -- client/src/api/layoutApi.ts src/settings/api/settings_routes.rs`. Checked by reading that diff against this record's claims; server `cargo test --lib` 1721 passed, 0 failed, 9 ignored, `cargo test -p visionclaw-domain` 317 passed at `93302275e`.

Re-checked at `db3ab82fa`: `settings_routes.rs` also gained `reset_document` (a layout reset keeps the active mode) and `stored_physics`; neither touches the cycle routes or the layout-mode whitelist decision 4 describes. Re-run: `git diff --stat 93302275e db3ab82fa -- src/settings/api/settings_routes.rs`.

Re-stamped to `347f03407` (2026-10-31, ADR-2140 closes: the `/analytics/sssp/params` routes are deleted). The claim that moved is decision 2's route line, now `analytics/mod.rs:258`. The `:255` citation was already stale at `1b8335c4c` (the route was at `:261` there); this change moved it up to `:258` and the citation now matches. Re-run: `git show 1b8335c4c:src/handlers/api_handler/analytics/mod.rs | grep -n get_cycles` → `261`, and `grep -n 'cycles_handlers::get_cycles' src/handlers/api_handler/analytics/mod.rs` → `258`. Nothing else this record cites changed.

Re-stamped to `a71b2608dc905e970c69a5383eeb6880654aa4d6` (2026-11-01, ADR-2140 second review). `settings_routes.rs` gained `layout_mode_to_forward` (a quality-gate PUT forwards a `layoutMode` only when it differs from the stored echo), `reset_document_for`, and a `stored_physics()` that reads the stored `physics` row first. The whitelist decision 4 describes (`validate_quality_gate_settings`, `LIVE_LAYOUT_MODES`) and the cycle routes are untouched. **Decision holds.** Re-run: `git diff --stat 347f03407 a71b2608d -- src/settings/api/settings_routes.rs client/src/api/layoutApi.ts src/layout/scc.rs src/handlers/api_handler/analytics/cycles_handlers.rs` → only `settings_routes.rs | 197`; and `git diff 347f03407 a71b2608d -- src/settings/api/settings_routes.rs | grep -c 'LIVE_LAYOUT_MODES\|validate_quality_gate_settings'` → 1, an unchanged context line (a comment); no added or removed line names either (`... | grep -E '^[+-]' | grep -c 'LIVE_LAYOUT_MODES\|validate_quality_gate_settings'` → 0).

## Re-verification — 2026-10-31 (`ecd78b6cb`)

Re-verified 2026-11-01 at `ecd78b6cb`: the only change to the governed path is `xr-client/scripts/hud.gd`, where `KEY_EDGE_FLOW` (the Key page swatch mirroring the graph's flow-edge colour) moves from cyan to gold and its label gains "breathes slowly". It changes no HUD panel palette, no tab, and nothing this record decides. Evidence: `git diff 8e92c5d5c..ecd78b6cb -- xr-client/scripts/hud.gd` (2 lines).

Re-verified 2026-11-01 at `484d9392f` (xr/grab-client, ADR-2172). Changes since `a71b2608d`: `render_store.rs` gains `hunt_grabs` (ADR-2172) and `hud.gd` has a two-line edge-key edit (colour and text). Neither touches the cycle analysis or its single adjacency. The decision holds.

Re-verified 2026-11-01 at `03ddc61e1` (viz/edge-tracking). `xr-client/rust/src/render_store.rs` gains seven edge/node coincidence tests, appended after the last test so no cited line moves, and `render_store_pack.rs` and `tests/node_lod.rs` re-transform the far-tier ribbons every frame instead of half per frame. Nothing this record decides changed. Evidence: `git diff 531df5ef7..838e06c13 -- xr-client/rust/src/render_store.rs xr-client/rust/src/render_store_pack.rs`; xr-client `cargo test` passes (486 library tests).

## Re-verification — 2026-11-01 (`0df96bc52`)

`render_store.rs` loses the dead `edge_frame` counter (field, `PackSlot` field, swap, increment; never read). The cycle analysis and layout-mode claims are untouched; `cargo test` (xr-client/rust): 610 passed, 0 failed.

## Re-verification — 2026-11-01 (`e99c16d31`)

`render_store.rs` changed in two ways that touch no cycle code: `position_of` goes through the fold-aware resolver (`drawn_position_of`), and the cross-volume link pack (`pack_links`) is re-exported. Cycle overlays read `position_of` only for nodes they draw, which are visible; for them the answer is unchanged. Evidence: `cargo test` 618 passed, 0 failed.

## Re-verification — 2026-11-01 at 15acb92192ebb2cd2539ee227d2c6545b244ff56

`15acb9219` adds agent state handling to `xr-client/rust/src/render_store.rs`. The record keeps a `done` flag, and `retired` removes the agent through the existing retire path. Both are fed by ADR-2142 state frames. Nothing this record decides is touched. The decision holds unchanged.

## Re-verification — 2026-11-01 (`abcf36db1`, branch `viz/edge-tracking`)

Re-verified after the R23 fixes. `render_store.rs` adds `is_drawn` (the edge pack's drawn test, now also asked by the plan, beam and link packs) and removes the beam's node-id fallback. No cycle-overlay code is touched. Every claim in this record holds. Evidence: `cargo test` in `xr-client/rust` (623 passed), server `cargo test --lib` (1781 passed, `CUDA_VISIBLE_DEVICES=1`), client vitest (1540 passed), GUT on a fresh `.so` (553 passed, 0 failed, `gut_guard` ok). Not run: the headset.

Re-verified again at `69b16867d`: A later commit adds `is_drawn_in_slot` (the link pack asks each end's own volume slot) beside `is_drawn`. No cycle-overlay code is touched. GUT on a fresh `.so`: 554 passed, 0 failed; `xr-client/rust` `cargo test`: 624 passed.

## Re-verification — 2026-11-04 (`5c94c3cbd`)

Re-verified after merging feat/splats (ADR-2170, Gaussian splat scenes). The governed-path changes since `69b16867d` are additive: `xr-client/scripts/hud.gd`: a credits line shown only when a splat scene sets one, and an Enter/Leave Splat Scene button hidden until one is loaded. This record's own claims are unchanged. Evidence on the merged tree: `cargo test --lib` 1823 passed, 1 failed (`nip98_proxy_tests::prod_lan_headset_wss_authenticate_validates_through_nginx`, which reads only `nginx.*.conf` and `auth.rs`, neither touched; it fails identically at `d74b25143`); `xr-client/rust` 699 passed; XR GUT 595 of 598 passing (3 pending pixel tests); vitest 1593 passed. Re-run: `git diff 69b16867d..5c94c3cbd -- <governed path>`.
