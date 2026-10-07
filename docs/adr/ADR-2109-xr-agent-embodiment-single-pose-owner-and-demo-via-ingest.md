---
id: ADR-2109
title: Embody registry agents in XR with a single pose owner; demo mode only via the real ingest path
date: 2026-09-09
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: c5723490b2d86655acaf4b4c88ccbb2b96b785b2
verified_paths: [xr-client/scripts/agent_choreography.gd, xr-client/scripts/agent_demo_director.gd, xr-client/scripts/agent_effects.gd, xr-client/scripts/agent_role.gd, xr-client/scripts/graph_scene.gd, xr-client/scenes/GraphScene.tscn, xr-client/rust/src/render_store.rs, xr-client/rust/src/binary_protocol.rs]
owner: jjohare
review_trigger: a DID↔wire-id bridge lands (ADR-140 §5), or a second embodiment consumer (Quest build) ships
repo: visionclaw
---

# ADR-2109 — Embody registry agents in XR with a single pose owner; demo mode only via the real ingest path

## Context
The XR client had an agent avatar (orb + gaze cone + DID badge, ADR-130 D4) that only the demo ever spawned; live swarm agents rendered as 1–3 cm spheres. The avatar spawner sat under `GraphRoot`, so avatars inherited the ~0.03 fit scale (0.15 m orb → ~5 mm). Three code paths wrote each avatar's position every frame (proxemics arc on head-pose change, a nudge lerp, an idle radial drift); demo targets were random coordinates, beams required the agent id in the node index, and the fade path needed 15 s idle against an 8 s demo idle. In the headset this read as "tiny dots fly in from behind, do nothing, retreat, repeat". ADR-140 D5 keeps a work layer (wire-id keyed) and a conversation layer (did:nostr keyed) deliberately separate.

## Decision
1. **Unit-scale roots.** Embodiments live under `AgentsRoot`, work-cue effects under `AgentEffectsRoot`; neither is a child of `GraphRoot`. Positions are converted with `graph_root.to_global(node_position(id))`; avatar geometry is never scaled by the graph fit.
2. **One pose writer.** `agent_choreography.gd` is the sole owner of a work-layer embodiment's position and alpha (materialise → travel → arrive → work → complete → park → rest). The proxemics arc places conversation-layer avatars only. Server owns *which* node, status and task; the client owns *where in the room* (ADR-140 motion-authority split).
3. **Registry is the source of embodiment.** Every id in the Rust agent registry is embodied at ~4 Hz (`_reconcile_embodiment`), keyed `agent_<wireid>`; ids that leave the registry despawn. Explicit completion (`status == done`) drives the completion beat and the 0.3-alpha park; the 30 s evidence TTL is the only other exit from "working". Parked agents stay selectable.
4. **Beam origin follows the body.** The scene publishes server-space anchors per embodied agent each frame (`set_agent_anchors`); `build_beam_buffer` prefers an anchor over the streamed node position. Anchors are visual-only; node positions and physics are never touched.
5. **Demo mode is a producer, not a branch.** `agent_demo_director.gd` is the only demo code. It encodes real `0x23 AGENT_ACTION` frames (wire ids `0x80000000|0xD001..0xD006`, reserved range `0xD001–0xD0FF`, payload `{"intent":…}`) into `ingest()`, reports completion through `apply_agent_state(id,"done",…)`, stamps timestamps from `server_clock_ms()`, and on Stop calls `retire_agents(ids)`. No fake position frames. The synthetic agents play as real agents — names, frames, roster rows and captions carry no demo marker; the Start/Stop Agent Demo button is the only visible sign of the demo, and provenance is the reserved id range in code. The scene contains no demo-specific rendering path.
6. **Layer toggle is visual.** "Agents" hides `AgentsRoot` + `AgentEffectsRoot` and the render-store class; choreography, registry, handles and physics keep running.

## Consequences
- Live agents are embodied for the first time; the demo exercises exactly that path, so a demo regression is a production regression and vice versa.
- Two narrowly named Rust `#[func]`s exist for lifecycle honesty: `set_agent_anchors` and `retire_agents`; `server_clock_ms` exposes the ADR-2034 clock anchor to producers.
- The deleted paths (`_agent_nudge_targets`, idle radial drift, `_random_graph_point`, GDScript-only demo cycle) must not return; any new motion goes through the choreography.
- Body (P1, shipped with this ADR's second verification): six procedural role frames + accent + two-letter badge (`agent_role.gd`, inferred from name then task), a 0.09 m pointer cone for work-layer avatars (the social gaze cone stays for the conversation layer), world-size badges with a gated task caption, hand-off packet beads along the real edge and a 600 ms arrival flash (both off under reduced motion). Remaining follow-ups: a travel trail ribbon, orbiting capability badges, and desktop parity of the loop grammar (plan §2.7a/2.8).
- Reduced motion (default on) replaces travel with fade/relocate/fade and disables hover; judge the demo with the comfort toggle in mind.

## Verification
- `cargo test -p visionclaw-xr-gdext --all-features` — `beam_starts_at_the_embodiment_anchor_when_one_is_published`, `retire_agents_removes_records_and_anchors_outright` (66 render_store tests green).
- GUT (Godot 4.3 in CI): `tests/unit/test_agent_choreography.gd` (in-place materialise, head turns never move a working agent, explicit done → park at 0.3 alpha, re-task without snap, head exclusion, reduced motion), `tests/unit/test_agent_demo_director.gd` (byte-exact `0x23` layout, real-node targets, keep-alive under TTL, 150 s loop completes/rests/re-tasks along real edges, Stop retires exactly the demo ids).
- HP-Desktop VIVE, Godot 4.6.1: headless `--check-only` on all six scripts exit 0; `/tmp/godot-xr-fresh.log` shows `XR_SESSION_STATE_FOCUSED`, 0 script errors, 0 HUD overflow, 89–90 FPS with the demo loop running.

## Re-verification — 2026-09-21 at 997440cd0717d4c5f9341369571fc69fcf5a38d6

**Governed changes since `3eb2ffae5`:**
`xr-client/scripts/agent_demo_director.gd` (role labels lose their `Demo-`
prefix, the action payload drops `"demo": true`, `scene_id_for` is deleted, the
provenance comment rewritten) and `xr-client/scripts/graph_scene.gd` (the roster
row drops its `demo` field and signature component, `_scene_id_for` no longer
special-cases demo ids).

**Decision unaffected — this *is* the decision.** D5 as written already says the
synthetic agents play as real agents, that names, frames, roster rows and
captions carry no demo marker, that the Start/Stop button is the only visible
sign, and that provenance lives in the reserved id range in code. The changes
remove the last markers that contradicted that text. The load-bearing parts are
intact at HEAD: wire ids are still `0x80000000 | 0xD001..0xD006`, Stop still
calls `retire_agents(ids)` so no demo state outlives the demo, frames still go
through `ingest()` with timestamps from `server_clock_ms()`, and the scene still
has no demo-specific rendering branch. `verified_commit` moved to the CI-repair
commit.

## Re-verification — 2026-10-02 at e7e6b61d8 (headset NIP-98 behind the prod nginx)

**Governed changes:** `xr-client/scripts/graph_scene.gd` changes only `_describe_write_failure`: the 401/403 text and its comment now name an Owner/Admin `XR_NOSTR_SECRET` as the remedy and mark `VISIONCLAW_DEV_MODE` as dev-only (owner decision 2026-10-02, Q1 and Q3). **Decision unaffected.** No pose ownership, agent rendering or ingest path changed. `verified_commit` moved to the landing commit. Source reading, plus the unit tests named in that commit.

## Re-verification — 2026-10-07 at b6fbe772d (XR beat clock, memory bursts, attention heat; ADR-2134)

**Governed change:**
- `render_store.rs` packs the desktop beam encoding into `INSTANCE_CUSTOM.rgb` (action code and taper; `.a` is still status, stride 16), and attention heat brightens the colours of touched nodes.
- `binary_protocol.rs` advances the heat clock.
- `graph_scene.gd` creates `BeatPulse`, whose `MemoryBursts` effects live under the unit-scale `AgentEffectsRoot`.

**Decision unaffected.** The beam origin still prefers the embodiment anchor (D4, test `beam_starts_at_the_embodiment_anchor_when_one_is_published`). Bursts sit under a unit-scale root, never `GraphRoot` (D1, GUT `test_scene_creates_the_beat_node_with_bursts_under_the_unit_scale_root`). Choreography remains the only pose writer. Demo `0x23` frames enter through `ingest()`, as before, and now also colour their beams by action and heat their targets like real ones, with no demo branch (D5). Verified with `cargo test -p visionclaw-xr-gdext` (264 + 83) and GUT on HP (123 pass).

## Re-verification — 2026-10-07 at 39f580e93 (merged XR parity tree)

The merge of `feat/xr-cloud` (1c03ffb2a; it carries `feat/xr-graph`) brings these into one tree with the ADR-2134 work:
- xr-cloud's memory cloud and route layers (WP6/7);
- xr-graph's palette, settings sync, hulls and node LOD (WP1/2/4).

Those branches did not move this record's `verified_commit`, so the combined state is re-verified here. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 105 integration tests, and GUT on HP Godot 4.6.1 (`--xr-mode off`) passes 149 tests.

`render_store.rs` adds xr-graph's palette, filter, hull and LOD state in separate fields and impl blocks. `build_beam_buffer`, `agent_anchors` and `set_agent_anchors` are unchanged by the merge. `graph_scene.gd` parents the memory cloud, a data layer and not an embodiment, under `GraphRoot`. Bursts and avatars stay under the unit-scale roots. **Decision unaffected.**

## Re-verification — 2026-10-07 at 944cba88c (xr-graph halo quads and edge LOD merged)

The merge of `feat/xr-graph` (944cba88c) brings in xr-graph's halo quad layer (`NodesHaloMulti`, `node_halo_quad.gdshader`), its edge LOD (near cylinders plus far camera-facing ribbons sharing `edge_flow_common.gdshaderinc`) and the avatar quaternion slerp. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 111 integration tests; GUT on HP Godot 4.6.1 (`--xr-mode off`) runs 161 tests, 158 passing and 3 GL-only tests pending headless.

`render_store.rs` adds the LOD packers in their own impl block. `build_beam_buffer` and the embodiment anchors are unchanged, and avatars and bursts stay under the unit-scale roots. The avatar rotation fix keeps `agent_choreography.gd` as the only pose writer. **Decision unaffected.**

## Re-verification — 2026-10-07 (feat/xr-graph pack plans)

The merge of `feat/xr-graph` at e6c4b0fb5 (merge 015bd642f) changed governed files without updating this ADR, so its changes were checked against the decision. `render_store.rs` now caches node colour in a pack plan and calls `touch()` from mutating setters (agent expiry included); attention heat moved from `emit_node` to the per-frame `live_tint`. Beam anchors, the single pose writer and the demo-via-ingest path are unchanged; the beam and embodiment tests pass. **Still holds.**

## Re-verification — 2026-10-07 (feat/xr-graph 4978356e5)

The merge of `feat/xr-graph` 4978356e5 (c5723490b) moves one `graph_scene.gd` declaration and adds a heat test to `render_store.rs`; no pose writer, beam anchor or demo-ingest code changed. **Still holds.**
