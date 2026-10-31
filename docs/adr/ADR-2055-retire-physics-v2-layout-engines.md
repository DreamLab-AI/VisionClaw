---
id: ADR-2055
title: Retire the frozen physics-v2 layout engines and stop advertising modes the server cannot honour
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: b00c28a0d766c8cf46cd00b100dab60ef2dd74a4
verified_paths: []
owner: jjohare
review_trigger: Any proposal to reintroduce a pluggable LayoutEngine trait, or to add a LayoutMode variant
repo: visionclaw
---

# ADR-2055 — Retire the frozen physics-v2 layout engines and stop advertising modes the server cannot honour

## Context

`Cargo.toml` carried a `physics-v2` feature gating a five-engine `LayoutEngine` registry
(`src/physics/engines/`, `engine_for()`), introduced by ADR-01 D5. Its own declaration
comment said it was "FROZEN / EXPERIMENTAL (closeout 2026-07-03)", that the engine `step()`
bodies "are stubs", and that it "must not be enabled in a shipped build". It was not in
`default`, so in every production build `engine_for()` and all five engines were compiled
out — roughly 6k lines of unreachable code.

Meanwhile `src/handlers/layout_handler.rs` advertised all six `LayoutMode` variants through
`get_layout_modes` and `get_layout_status`, and `set_layout_mode` **silently coerced** an
unparseable mode to `ForceDirected` instead of rejecting it. A client could therefore ask
for a layout the server had no implementation for, receive `200 OK`, and get force-directed
output with no indication anything had been substituted. Diagrams VC-11.8 and VC-16.5 carry
the DIVERGENCE notes.

## Decision

The `physics-v2` feature and the `src/physics/engines/` registry are removed. The live
layout path is `SetLayoutMode` on `ForceComputeActor`, which is the only implementation that
has ever run in production.

A layout mode the server cannot honour is rejected, not substituted. `set_layout_mode`
returns `400 Bad Request` naming the accepted values when the requested mode does not parse.
Silent coercion is forbidden: it converts a client error into a wrong-looking-right result,
which is the failure mode hardest to diagnose from the outside.

The advertised mode list matches what the live handler actually acts on.
`LayoutMode::Clustered` is excluded from the advertised set because `ForceComputeActor`'s
`SetLayoutMode` handler has no dedicated arm for it; the remaining five are advertised.
Advertising and honouring are kept in step by construction rather than by comment.

## Consequences

About 6k lines of stub code and one build feature disappear, and the "must not be enabled in
a shipped build" hazard disappears with them — there is no longer a flag that turns a
production build into a stubbed one.

A client that previously sent a malformed or unsupported mode string and silently received
force-directed layout now receives a `400`. That is a behavioural change and will surface
latent client bugs; it is the point of the change, and the error names the accepted values so
the fix is obvious from the response.

Reintroducing a pluggable layout-engine abstraction is not forbidden, but it starts from a
clean slate rather than from frozen stubs — that is the review trigger. The `LayoutMode`
enum itself is retained: other code matches on it, and narrowing the enum is a separate,
wider change.

## Consequences for the ADR ledger

ADR-01 D5's engine registry is superseded in practice. This ADR does not carry a
`supersedes:` edge because ADR-01 is a legacy pre-consolidation record cited for rationale
only, not an entry in the `docs/adr/` ledger; per `CLAUDE.md` the legacy corpus is never
authority.

## Verification

`ls src/physics/engines/` — directory absent (removed).
`grep -n "physics-v2" Cargo.toml` — only a retirement note remains at the former feature
site; the feature declaration is gone.
`grep -rn "cfg(feature = \"physics-v2\")" src/ crates/` — no remaining gates.
`grep -n "ErrorBadRequest" src/handlers/layout_handler.rs` — `set_layout_mode` now returns a
`400` on parse failure instead of falling back to `ForceDirected`.
`grep -n "LayoutMode::" src/handlers/layout_handler.rs` — the advertised list is five
variants; `Clustered` is excluded with the reason recorded in a comment at the head of the
file.

`cargo check -p visionclaw-server` — **exit 0, zero errors**, with every Phase 2 change in the
tree. (An earlier run in this phase was blocked by concurrent breakage in files owned by other
leads; those were fixed by their owners and the check was re-run clean.)

Verification ran on the uncommitted working tree above the recorded SHA; `verified_paths` is
empty for that reason.

## Amendment — 2026-10-31 (ADR-2140 item 1)

`SetLayoutMode` and `ForceComputeActor`'s mode handling no longer exist: the layout mode is the stored `physics.layoutMode`, and `set_layout_mode` writes it through `apply_physics_patch` (the review trigger "add a `LayoutMode` variant" fired: `DagLeftRight` and `TypeClustering` were added). What this record decided still holds: an unparseable mode is a `400`, never substituted, and the advertised list matches what the handler honours. The advertised list is now seven (`forceDirected`, `hierarchical`, `radial`, `dagLeftRight`, `typeClustering`, `spectral`, `temporal`); `clustered` is still accepted and stored but not advertised, because it is the force-directed baseline plus the independent `clusterStrength` knob. Re-run: `git diff --stat 75b72416b 412849582 -- src/handlers/layout_handler.rs src/actors/gpu/force_compute_actor.rs crates/visionclaw-domain/src/types/layout.rs`. Details: ADR-2140, second Close-out.

