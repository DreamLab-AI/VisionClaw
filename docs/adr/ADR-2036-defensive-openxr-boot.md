---
id: ADR-2036
title: Defensive OpenXR boot — query eye-gaze capability and degrade, never blind-bind
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: e0f8cd896
owner: jjohare
review_trigger: Godot upstream fixing hazard #113717, or eye-gaze becoming a required (not optional) interaction on the target headset
repo: visionclaw
domain: XR-client
lineage: distils legacy ADR-071 (OpenXR runtime choice) and ADR-139 (immersive interaction); guards Godot upstream hazard #113717
---

# ADR-2036 — Defensive OpenXR boot: query eye-gaze capability and degrade, never blind-bind

## Context

Binding the `XR_EXT_eye_gaze_interaction` action-map entry on a device that lacks the
extension (e.g. Quest 3) trips an action-map error at boot. Separately, the OpenXR
vendor addon adds XR nodes to the tree asynchronously; a synchronous scene swap during
that window trips "Parent node is busy adding/removing children" (Godot upstream hazard
#113717). Both make boot brittle across headsets.

## Decision

`xr_boot` only *queries* `is_eye_gaze_interaction_supported()` and degrades to
head-gaze as primary — it never blind-binds the eye-gaze action-map entry. The
graph-scene swap is deferred to idle via `change_scene_to_packed.call_deferred()` so it
cannot race the vendor addon's node insertion. This forecloses unconditional eye-gaze
binding and any synchronous scene swap during OpenXR bring-up.

## Consequences

- Boots cleanly on headsets with and without eye-gaze; head-gaze is the reliable
  baseline, eye-gaze an opt-in capability.
- Eye-gaze is left disabled unless explicitly supported — a capability surrendered by
  default rather than assumed.
- Deferring the swap adds one idle frame of latency at boot — negligible, and the price
  of dodging hazard #113717.
- See `docs/XR-client.md`.

## Verification

Re-checked at `e0f8cd896`: `xr-client/scripts/xr_boot.gd:47–50` guards on
`has_method("is_eye_gaze_interaction_supported")`, queries it, and appends a head-gaze
degrade warning without binding; `:59–61` swaps via
`get_tree().change_scene_to_packed.call_deferred(graph_scene)`.

## Closeout extension — 2026-09-04

Work package: **CP-06/08**. Owner remains `jjohare`, with protocol, identity and XR maintainers responsible for their respective boundaries.

Source retains capability checks and deferred scene transition. Missing or failed OpenXR initialisation stops with an error; no headset was tested in this pass.

**Acceptance condition:** Capture current boot receipts with and without eye-gaze, missing runtime, controller fallback and scene transition on each supported target; keep desktop and Quest evidence separate.

Dependencies: CP-01 release identity and CP-04 authority where authenticated actions cross the wire. Reopen on the existing review trigger, a changed opcode or a failing freshness/visibility probe. Existing verification and activation fields retain their historical scope; this annex records source/local tests at `b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`, not a new live certification.

See [rendered-state review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/xr-render-snapshot.json).

## Acceptance progress — 2026-09-05

**No execution possible; the receipt is now specified.** Every condition here —
boot with and without eye-gaze, a missing runtime, controller fallback, scene
transition, on each supported target — requires a headset and a Godot runtime,
neither of which exists in this environment.

`docs/estate-closeout/2026-09-05/xr-export-runtime-revision-matrix.md` (shared with
ADR-2032) now specifies the receipt, with the five ADR-2036 checks as named rows in
Column C, each paired with the artefact that would evidence it:

| Check | Evidence required |
|---|---|
| eye-gaze present | boot log showing the capability found |
| eye-gaze absent | boot log showing the warning, boot continuing |
| OpenXR runtime missing | error surfaced, no scene transition |
| controller fallback | hand tracking unavailable → controllers drive selection |
| scene transition | deferred transition completes after boot checks |

Two of the matrix's pass criteria bear directly on this ADR. **Both** eye-gaze rows
must be filled, present *and* absent: the defensive branch is the whole decision,
and testing only the happy path leaves precisely the branch ADR-2036 is about
unexercised. And desktop and Quest must be filled independently, because the boot
path differs by OpenXR runtime (SteamVR/Monado versus the Quest runtime) — a pass
on one is not evidence for the other.

**Tests run.** None; none are possible here.

**Governed paths changed.** None in source.
`docs/estate-closeout/2026-09-05/xr-export-runtime-revision-matrix.md` added.

**Open — entirely.** The source branches (capability check, warning on absent
eye-gaze, error-and-stop on missing runtime, deferred scene transition) are
unchanged and remain source observations only. No boot receipt was captured on any
target, and desktop and Quest evidence must stay separate when they are.

## Addendum — 2026-10-18: the static gaze binding is removed (estate register ER-13)

- **What changed.** `xr-client/openxr_action_map.tres` no longer carries the `eye_gaze_interaction` interaction profile, its gaze-pose `OpenXRIPBinding`, or `/user/eyes_ext` in the default pose's top-level paths (VisionClaw `2bf892c3f`, `392628eef`). The file had bound the pose unconditionally, which is exactly the blind bind this record forbids. A GUT test asserts their absence.
- **What it costs.** Eye gaze is now unbound on every headset, including those that do expose `XR_EXT_eye_gaze_interaction`, so the gaze source always falls back to head gaze at runtime. The boot query in `xr_boot.gd` still runs and still reports the capability.
- **Why not bind at runtime.** Godot 4.3 loads the action map when the OpenXR interface initialises, and an interaction profile cannot be added to a live action map after the session starts. A runtime rebind after the extension query is therefore not available.
- **How to restore gaze.** Choose the action map at boot, before the OpenXR interface initialises: ship two `.tres` variants, one with the gaze profile and one without, and select between them from a pre-initialisation extension check. Until that exists, gaze stays unbound. Unverified on device (register row L-5).
