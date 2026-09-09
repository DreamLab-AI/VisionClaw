---
id: ADR-2109
title: Embody registry agents in XR with a single pose owner; demo mode only via the real ingest path
date: 2026-09-09
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 82cbaf88b594f881def9f1f6e84bacf31274efa9
verified_paths: [xr-client/scripts/agent_choreography.gd, xr-client/scripts/agent_demo_director.gd, xr-client/scripts/agent_effects.gd, xr-client/scripts/graph_scene.gd, xr-client/scenes/GraphScene.tscn, xr-client/rust/src/render_store.rs, xr-client/rust/src/binary_protocol.rs]
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
5. **Demo mode is a producer, not a branch.** `agent_demo_director.gd` is the only demo code. It encodes real `0x23 AGENT_ACTION` frames (wire ids `0x80000000|0xD001..0xD006`, reserved range `0xD001–0xD0FF`, payload `{"intent":…,"demo":true}`) into `ingest()`, reports completion through `apply_agent_state(id,"done",…)`, stamps timestamps from `server_clock_ms()`, and on Stop calls `retire_agents(ids)`. No fake position frames. Demo agents are labelled "[demo]" in the roster. The scene contains no demo-specific rendering path.
6. **Layer toggle is visual.** "Agents" hides `AgentsRoot` + `AgentEffectsRoot` and the render-store class; choreography, registry, handles and physics keep running.

## Consequences
- Live agents are embodied for the first time; the demo exercises exactly that path, so a demo regression is a production regression and vice versa.
- Two narrowly named Rust `#[func]`s exist for lifecycle honesty: `set_agent_anchors` and `retire_agents`; `server_clock_ms` exposes the ADR-2034 clock anchor to producers.
- The deleted paths (`_agent_nudge_targets`, idle radial drift, `_random_graph_point`, GDScript-only demo cycle) must not return; any new motion goes through the choreography.
- The consultant-specified body redesign (role frames, pointer, world-size badges: plan §2.2) and edge packets are follow-ups; the pointer today is the existing cone aimed by `set_aim`.
- Reduced motion (default on) replaces travel with fade/relocate/fade and disables hover; judge the demo with the comfort toggle in mind.

## Verification
- `cargo test -p visionclaw-xr-gdext --all-features` — `beam_starts_at_the_embodiment_anchor_when_one_is_published`, `retire_agents_removes_records_and_anchors_outright` (66 render_store tests green).
- GUT (Godot 4.3 in CI): `tests/unit/test_agent_choreography.gd` (in-place materialise, head turns never move a working agent, explicit done → park at 0.3 alpha, re-task without snap, head exclusion, reduced motion), `tests/unit/test_agent_demo_director.gd` (byte-exact `0x23` layout, real-node targets, keep-alive under TTL, 150 s loop completes/rests/re-tasks along real edges, Stop retires exactly the demo ids).
- HP-Desktop VIVE, Godot 4.6.1: headless `--check-only` on all six scripts exit 0; `/tmp/godot-xr-fresh.log` shows `XR_SESSION_STATE_FOCUSED`, 0 script errors, 0 HUD overflow, 89–90 FPS with the demo loop running.
