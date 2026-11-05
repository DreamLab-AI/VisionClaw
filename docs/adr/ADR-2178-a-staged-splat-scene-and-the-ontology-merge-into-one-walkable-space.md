---
id: ADR-2178
title: A staged splat scene and the ontology volume merge into one walkable space at viewer scale
date: 2026-11-05
decision_status: proposed
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 81e71126779b0b312b93ab459058ee8977233331
verified_paths: [crates/visionclaw-splat/src/manifest.rs, xr-client/rust/src/splat_loader.rs, xr-client/scripts/space_portals.gd, xr-client/scripts/splat_volume.gd, xr-client/scripts/graph_scene.gd, xr-client/scripts/hud.gd, xr-client/scripts/xr_prefs.gd]
owner: jjohare
review_trigger: The first headset session in the merged space, a second staged asset, or any change to ADR-2170's placement or budget rules.
repo: visionclaw
domain: XR-client
evidence_class: source+test
acceptance_boundary: Rust unit tests (crate and gdext loader) in the agentbox container and headless GUT on the HP rig. No frame of the merged space has been judged in the headset; the default scale and the guest placement are measured from the capture, not from a person standing in it.
lineage: Amends ADR-2170 (its decision 2 placement rule and its reserved "1:1 metric view") and extends ADR-2166 (entered volume) with a second entered state; grabs follow ADR-2172.
---

# ADR-2178 — A staged splat scene and the ontology volume merge into one walkable space at viewer scale

## Context

ADR-2170 enters a splat scene as a 6 × 3 × 6 m room (ADR-2166) and reserves a 1:1 metric view for its own amendment. A trained capture of a place (the Sentinel hangar) only reads at the size it was captured: at room scale an 80 m hangar is a dolls' house. The owner wants one mode in which the splat scene is the space you stand and move in and the ontology graph hangs in it, between where you start and what the scene is about. Splat units are arbitrary (COLMAP), so the scale has to be measured per asset and tunable in the headset.

## Decision

1. **A stage per asset.** The manifest and the listing gain an optional `stage {metresPerUnit, focus}`. The asset is baked into its stage frame before decimation: the start is the floor point at the origin, +Y is up (levelled), the start faces −Z, and the focus is the point of interest in scene units. Validation: a positive finite scale, and the focus and the origin inside the bounds. Absent, the asset is only a bounded volume. The field is additive (`serde(default, skip_serializing_if)`); old clients ignore it.
2. **The merged space** is a second entered state of `SpacePortals`, toggled from the Layout page's Splat Scene group (**Merge with Ontology**, shown only for a staged scene):
   - the splat volume is the room, `BOUNDED`, with bounds = content extent × viewer scale, so the fit scale is exactly the viewer scale; it is placed so the stage start is the user's floor point and the stage −Z is the user's heading;
   - the ontology volume is the guest: a cube on the sight line from the eye to the focus, in plan between the user and the focus, centre at most 4 m from the start (the wand ray is 5 m), near face at least 1 m away, at least 0.3 m off the floor and at most 1.5 m above the eye, edge 1.2–3 m by distance; it keeps its own fit, has no membrane, and its nodes grab under ADR-2172;
   - every other volume is hidden and shown again on leave; there are no portals; the ring and bounds come back from the ADR-2166 snapshot;
   - entering, leaving and every scale change land in full dark (the room is tens of metres).
3. **Viewer scale** = `metresPerUnit` × a multiplier stepped by the HUD (0.25, 0.5, 0.71, 1, 1.41, 2), stored in `xr.merged_splat_scale`. A step re-scales about the user's floor point, so the scene point under the user stays under them.
4. **Off by default**, as ADR-2170: the button appears only with the splat scene on (`XR_SPLATS=1` or the HUD toggle) and a staged asset loaded.

## Consequences

- **Amends ADR-2170 decision 2.** A graph box sits inside the splat box while merged. The entered splat stays the backdrop (`PRIORITY_ENTERED`), so opaque graph nodes are depth-correct against it, but the graph's transparent layers (edges, halos) draw over splat content that is nearer than they are. This is a named artefact of the merged space.
- The un-entered splat cap (tier 1) is lifted as for any entered room; the guest keeps a full budget weight.
- Locomotion (trackpad, 1.5 m/s) moves the user through the scene; nothing re-centres on them.
- A new asset must be baked into its stage frame and measured. The Sentinel stage: levelled 10.97° about X (the rig's up axis; both floor fits flat to within 1° afterwards), start at the panorama camera's floor point, focus the solar-array cluster at (0.06, 4.76, −12.02), 0.70 units of eye height → 2.25 m per unit at 1.6 m. Through `splat-transform` 3.10.3 the `-r`/`-t` actions act in the PlayCanvas frame (180° about Z from the PLY): X and Y rotations and X and Y translations take the opposite sign.

## Verification

- `cargo test -p visionclaw-splat --features cli`: 33 pass (stage round trip, listing pass-through, absent key, refusals).
- `cargo test --lib splat_loader` in `xr-client/rust`: 26 pass (a staged plan; a stageless server's listing).
- GUT on the HP rig (Godot 4.8-beta2, snapshot `~/mce-xr/vc-858cf136f`, gdext rebuilt there): 52 scripts, 609 tests, 608 pass on the first run. The one failure was `test_agent_edge_run.gd` reading `client/src/features/graph/utils/agentNudge.ts`, which the snapshot had not copied; with the file added that script passes 20/20. The guard saw no parse errors. New tests: `test_space_portals.gd` (room transform, guest placement, scale steps, enter/leave round trip, refusals, re-scale about the user), `test_splat_volume.gd` (stage from plan), `test_grab_client.gd` (the guest grabs).

## Re-verification — 2026-11-05 (`81e711267`)

Governed-path changes since the last stamp (`6b811f7a1`): `space_portals.gd` (`distance_to_box` returns world metres; the rearm margin is arrangement-scaled) and `graph_scene.gd` (a topology generation change releases active grabs). The splat manifest, loader, `splat_volume.gd`, `hud.gd` and `xr_prefs.gd` are unchanged, and the walk-through entry and splat-room rules this record decides do not depend on the rearm unit. **Decision unaffected.**

Re-run: `git diff 858cf136f..81e711267 -- crates/visionclaw-splat/src/manifest.rs xr-client/rust/src/splat_loader.rs xr-client/scripts/space_portals.gd xr-client/scripts/splat_volume.gd xr-client/scripts/graph_scene.gd xr-client/scripts/hud.gd xr-client/scripts/xr_prefs.gd`. Read from the diff and the repair record; the repair's own test results are in that record and were not repeated here.
