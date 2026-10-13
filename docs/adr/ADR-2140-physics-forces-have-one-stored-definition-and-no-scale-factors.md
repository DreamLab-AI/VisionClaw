---
id: ADR-2140
title: Physics forces have one stored definition, applied as stored — no scale factors, caps or shadow state
date: 2026-10-13
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit:
verified_paths: []
owner: jjohare
review_trigger: any new force, any new place that holds a physics value, any multiplier or clamp between the settings store and the GPU
repo: visionclaw
---

# ADR-2140 — Physics forces have one stored definition, applied as stored

## Context
- The semantic-force kernels were never launched: their FFI entry points were no-op C stubs, the parameters lived in a separate actor with its own all-enabled defaults, and CPU twins mutated `GraphData` behind a `gpu_available()` check. `gpu_available()` reported the cargo feature, not the link.
- Values could be changed without reaching the GPU: the settings gate compared a hand-kept field list (ten GPU fields were dropped while the PUT answered 200), the route and the actor validated differently, the shallow physics merge replaced a nested patch's whole group, and an "advanced weights" path multiplied stored values in place on every call.
- Measured (9.5k nodes, live field median |f| 36, p95 50): at their old defaults the DAG force had median 91 / p95 351 and collision median 56, enough to flatten the layout.

## Decision
1. **Every physics parameter has one stored definition**: the central physics settings document (`physics.*`; the semantic forces are `physics.semantic`). Everything else derives from it. No actor, endpoint or client holds a second copy, and no value is built from `Default` downstream.
2. **A value is applied exactly as stored.** There is no scale factor, multiplier, clamp or cap between the store and the GPU. What bounds a force is its validated range in the schema.
3. **One write path**: client control → settings store (deep-merged, validated once by the same validator the actor uses, finiteness checked explicitly) → `SimulationParams` → GPU. The idempotency gate compares everything the GPU would receive differently, including the semantic group.
4. **Each force has its own `enabled` flag in the store.** There is no master multiplier. A failure to set a force up (metadata, PTX, upload) is an error in the log and leaves that force off for the graph; there is no CPU fallback.
5. **The group's controls are described once**: the Rust field table emits `semantic-forces-fields.json`; the web registry, the client defaults and the XR HUD load it; tests pin each copy to the Rust definition and every slider range to the validator's accepted range.
6. **Defaults are calibrated by measurement** and recorded where they are defined. Forces that work by pulling nodes onto planes (`dag`, `maturityLayout`) ship off.

## Consequences
- Removed: `SemanticForcesActor` and its supervision, `Configure{DAG,TypeClustering,Collision}`, the stub FFI bridge and semantic stubs, the CPU twins, the quality-gate copies of the DAG/type-cluster parameters, `UpdateAdvancedParams` and its baseline*weight mechanism.
- A control cannot silently do nothing: the web client nests semantic paths into the PUT, and the server deep-merges, so one slider changes one field.
- Every force costs GPU time (each O(n²) kernel ≈ 1–1.7 ms at 9.5k nodes); disabling a force in the store removes it from the step.

## Verification
- `the_stored_settings_document_is_what_the_device_holds` (GPU): JSON as stored → actor parameters → device constant memory, read back through the device's own struct view.
- `every_slider_range_is_exactly_the_validators_range`, `the_committed_json_copies_are_current`, `a_nested_patch_changes_only_the_field_it_names`, `semantic_parameters_are_validated_up_front_by_the_one_central_validator`.
- `semantic_force_magnitudes_against_the_live_force_field` (ignored measurement) produced the calibration table.
