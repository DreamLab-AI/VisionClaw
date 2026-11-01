---
id: ADR-2172
title: A node is grabbed with either hand, up to two at once, in any VisionClaw volume
date: 2026-11-01
decision_status: accepted
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
amends: [ADR-2158, ADR-2166]
verified_commit: 0df96bc52459cd19b08683a96912e85c45c08f41
verified_paths: [xr-client/scripts/graph_scene.gd, xr-client/scripts/spatial_environment.gd, xr-client/scripts/backend_connection.gd, xr-client/scripts/connection_volume_source.gd, xr-client/rust/src/render_store.rs, xr-client/rust/src/binary_protocol.rs, xr-client/materials/edge_flow_common.gdshaderinc, xr-client/materials/edge_flow.tres]
owner: jjohare
review_trigger: the first headset session grabbing in the Ontology volume and inside an entered room (reach, depth feel, two-hand comfort); or the server's drag-release unpin (stream GS) merging
repo: visionclaw
domain: XR-client
depends_on: [ADR-2158, ADR-2166]
relates: [ADR-2159]
evidence_class: source+test
acceptance_boundary: the gate, the per-hand state, the routing and transform arithmetic, the portal refusal, the release-without-pin rule and the breathe parameters are covered by headless GUT (Godot 4.6.1 and 4.8-beta2) and a Rust unit test; whether a grab feels right in the headset (ray reach, depth, two wands), whether it holds inside a scaled room at 90 Hz, how the server settles two simultaneously pinned nodes, and whether the 36 s breathe at depth 0.8 reads well on the VIVE are not established
---

# ADR-2172 — A node is grabbed with either hand, up to two at once, in any VisionClaw volume

## Context
- Owner finding, 2026-11-01: in the Ontology room a trigger on a node turns the wands green and shows its label, but nothing is grabbed. Node grab is a key feature of the platform.
- Cause: `claim_blocks_drag` refused a node pick in every volume except `OVERLAY_VOLUME` (`knowledge`), and the ray candidates, the drag frames and the hand-to-server conversion all followed that one volume's connection slot and Content transform.
- Knowledge and ontology are two filtered views of one `visionclaw` connection: one render store, one wire id per node, a node in exactly one volume by class mask (`0b0001` / `0b0010`), one simulation. The server's `nodeDrag*` handlers take an id and a position only, with a per-client cap of five.
- Only one node could be held, because the grab state was a single id, controller, depth and target.
- Release recorded the node for Unpin All, although the server (stream GS) now unpins on release, so a released node should rejoin the simulation.
- The gold-edge breathe (32 s, depth 0.45 on an alpha of 0.16) swung by about 0.07 of alpha and was invisible; 32 s does not divide Godot's 3600 s `TIME` rollover.

## Decision
1. **Any VisionClaw volume drags.** `claim_blocks_drag(kind, volume)` blocks a link, and a node or frame in a volume whose connection lacks `BackendCaps.DRAG` (Codescape, ADR-2158 E9). Knowledge and ontology both serve `DRAG` and drag. The old third argument is kept and unused.
2. **One node per hand.** Grab state is per hand (`_grab_id`, `_grab_vol`, `_grab_dist`, `_grab_target`). Each controller holds at most one node and releases independently; one node cannot be held by both hands. The Rust `hunt_grabs(ease, ids, positions)` pins every held node to its hand each frame; `hunt` stays as a one-node wrapper, and the scene falls back to it on a gdext without `hunt_grabs`.
3. **Routed by volume.** A press that claimed a node or frame grabs in that volume only; an unclaimed press lets the Rust tube catch a node in any draggable volume of the ring, or only the entered volume inside a room. Candidates are read from the volume's own pack slot and mapped to world through its drawn Content transform. `nodeDragStart/Update/End` go to that volume's connection, and the world-to-server conversion inverts the same transform, so a grab works in a ring box and in a room whose content is scaled to room size.
4. **Portals never grab.** `grab_volume_allowed(volume, entered)`: the entered volume, or any volume when none is entered. No grab starts during a room transition, and entering, swapping or leaving a space releases every held node, because the Content transform snaps.
5. **Unchanged.** Double-click opens the node's document; the HUD and radial pointers do not grab; haptics are as before; the two-hand graph gesture keeps its own binding (both grips) and never runs while a node is held, so two trigger grabs never start it.
6. **Release rejoins the physics.** A drag release no longer writes `_pinned_ids`. Explicit Pin and Unpin All are unchanged. `BackendConnection.send_drag_end` records nothing either (review R15 F3): the server releases the node, so `close()` has no pin to undo and no longer sends a `nodeUnpin` (and a reheat) for every node ever dragged. **Consequence for the HUD:** nothing in `xr-client/scripts` writes `_pinned_ids` now and no client-side `nodePin` sender exists, so the Unpin All count stays 0, the Pins page stays empty and the fold request's `?pinned=` stays empty until a client-side Pin exists; a node just dragged may therefore fold away. The Pins page is kept for that future Pin (R15 F5).
7. **Review fixes (R15, F1-F8).**
   - **F1, no runaway.** A held node is pinned to the hand, and the E7 refit read the full extent including it: a hand held beyond the box face made the box refit around the node, the same hand map to a farther server point, and the node move again (measured 35 -> 1146 server units in 1 s, content scale 0.03 -> 0.001). `render_aabb_ids` now takes a `PackedInt32Array` of exclusions, `ConnectionVolumeSource.grabbed_ids` is a set, and `_begin_grab` / `_end_grab` write it from the nodes held in that volume. A stale gdext without the array signature fails the probe test, so the deploy needs the gdext rebuilt.
   - **F2, merge cost.** The unclaimed-press candidate merge ran a method call, a held check and a Dictionary write per node, 8 ms per hand per frame at 13k nodes. `_merge_grab_candidates` hoists the Content transform per volume, transforms the whole run with one native `Transform3D * PackedVector3Array`, drops held ids by `find` / `remove_at`, and resolves a node's volume afterwards from per-volume end offsets (`_grab_volume_of`).
   - **F4, F8, locks.** No grab starts while an arrangement ease is in flight (the first room entry eases with no dim). While any node is held a link press, the HUD Front action and a box grip on the holding volume move no volume.
   - **F6.** The double-click state is per hand, so interleaved presses do not defeat a double-click and a hand-off to the other hand grabs.
   - **F7.** The hand-local catch offset (lateral x / y, depth) is stored at grab and applied by the drive, so a node caught up to 5 cm off the aim axis no longer snaps onto it. The breathe comment states the true trough: albedo and alpha are both scaled, so at depth 0.8 the trough is (1 - 0.8)^2 = 0.04 of peak (the owner approved the depth).
8. **The breathe is visible.** `breathe_period` 36 s (divides 3600 exactly), `breathe_depth` 0.8, and the swell scales albedo as well as alpha. It stays whole-edge with nothing travelling, so it runs under reduced motion (owner's choice). The far ribbons copy both values through `NodeLod.EDGE_FLOW_PARAMS`.

## Consequences
- Both hands can rearrange nodes in the Ontology volume and in a room. Two simultaneous server pins stress the GPU force kernel in a way a single drag did not; this has not been run live.
- The server pins a node at drag-end today. Until stream GS's release-unpin lands, a released node stays pinned server-side and the client no longer lists it for Unpin All. Since R15 F3 the connection's `close()` no longer sends a `nodeUnpin` for dragged nodes either, so before GS lands the client has no way at all to release a dragged node. The two changes must merge together.
- A press that claimed nothing and points into a portal's node from inside a room grabs nothing.
- `spatial_environment.gd`'s focus marker follows the first held node, in that node's volume frame; with two nodes held only one carries the marker.
- The gdext gains `hunt_grabs`; a stale `.so` degrades to a single-node echo.

## Verification
- Evidence class `source+test`, headless only: `tests/unit/test_grab_client.gd` (28 tests) covers the gate, the per-hand state, release of one hand keeping the other, refusal of a node held by the other hand, drag frames per volume with each Content transform (including a scaled room), the Codescape refusal sending nothing, the portal refusal, release on entering a room, the two-hand gesture staying off, no pin recorded on release, the hunt echo for two nodes, no jump on grab with the real client and real packed slots (both volumes, both hands, and a scaled entered room), the labels and focus marker following the node's own volume, and the breathe parameters; and for the R15 fixes a probe-shaped test (a hand held 0.4 m beyond the +X face for 90 frames: the drag target stays bounded and the content scale stable), the exclusion set per volume, the merge's volume resolution and held-id filter, a 13k-candidate timing bench (loose bound 2.5 ms per hand; 8.1 ms before, 0.17 ms after on the HP), the ease lock, the link press with a node held, per-hand double-click, the off-axis catch and the true trough. `render_store::tests::hunt_grabs_*` and `binary_protocol::tests::render_aabb_ids_*` cover the Rust side. Updated existing tests: `test_volume_ux.gd`, `test_backend_switch.gd`, `test_node_lod.gd`, `test_backend_connection.gd` (a drag end records no pin), `test_connection_volume_source.gd`.
- Not verified: the headset (reach, depth, comfort), a live server with two simultaneous pins, the shader on a real renderer, 90 Hz inside an entered room with grabs.
- `implementation_status` stays `partial` and `activation_status` `inactive` until a headset session.

## Re-verification — 2026-10-31 (`f1fa1862c`)

Re-verified 2026-11-01 after merging xr/grab-server (ADR-2169), xr/grab-client (ADR-2172), client/three-0187 and docs/splat-adr, each Opus-reviewed (R15/R16/R17). The governed-path changes are those merges: drag holds and release in the GPU actor and socket handler, the held-only global_speed floor, two-hand grab and the edge breathe in the XR client, and the ADR 2168->2172 comment renames. This record's own claims are unchanged. Evidence on the merged tree: `cargo test --lib` 1768 passed, 0 failed (GPU 1); vitest 142 files / 1468 (installed three 0.183; the 0.187 run is on client/three-0187); XR GUT 522 passing on 4.6.1 and 4.8 at the grab-client head. Re-run: `git diff 2257ee7fa..f1fa1862c -- <governed path>`.

Re-verified 2026-11-01 at `03ddc61e1` (viz/edge-tracking). `xr-client/rust/src/render_store.rs` gains seven edge/node coincidence tests, appended after the last test so no cited line moves, and `render_store_pack.rs` and `tests/node_lod.rs` re-transform the far-tier ribbons every frame instead of half per frame. Nothing this record decides changed. Evidence: `git diff 531df5ef7..838e06c13 -- xr-client/rust/src/render_store.rs xr-client/rust/src/render_store_pack.rs`; xr-client `cargo test` passes (486 library tests).

## Re-verification — 2026-11-01 (`0df96bc52`)

`render_store.rs` loses the dead `edge_frame` counter and `graph_scene.gd` gains a comment edit; the two-hand grab path (`hunt_grabs`, `_grab_id`) is untouched. `cargo test` (xr-client/rust): 610 passed, 0 failed.
