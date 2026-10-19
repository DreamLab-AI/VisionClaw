---
id: ADR-2140
title: Physics forces have one stored definition, applied as stored — no scale factors, caps or shadow state
date: 2026-10-13
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 2107637d662ac70f38418c08e3892b77d5358c4e
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

## Layout lock
The host writes the device config as flat 32-bit word images. `semantic_forces.cu` carries `static_assert`s for every size and offset the image assumes (140 bytes; per-group offsets; the 4108-byte relationship table), so a layout the compiler does not give fails the build. The group's types reject unknown keys (`deny_unknown_fields`): a misspelt or snake_case key is a 400, never a quiet no-op.

## Not yet done (recorded, not hidden)
- The layout MODE and its bias priming (`fill_mode_defaults`) still live in the compute actor; the store holds a different vocabulary (`qualityGates.layoutMode`). Moving the mode into `physics.*` is a schema change of its own.
- `POST /sssp/toggle` patches the persisted parameters but does not write the toggle back to the store.

## Verification
- `the_stored_settings_document_is_what_the_device_holds` (GPU): JSON as stored → actor parameters → device constant memory, read back through the device's own struct view.
- `every_slider_range_is_exactly_the_validators_range`, `the_committed_json_copies_are_current`, `a_nested_patch_changes_only_the_field_it_names`, `semantic_parameters_are_validated_up_front_by_the_one_central_validator`.
- `semantic_force_magnitudes_against_the_live_force_field` (ignored measurement) produced the calibration table.

## Amendment — 2026-10-19

First verification, at `b4a175412`. The structural decisions hold in code; the "no clamp, cap or multiplier" rule is narrower than its wording, and the governing reference was still describing the removed actor. Governing doc updated in the same change: `docs/reference/physics-parameters.md` (the "Semantic forces" section is rewritten from the stored definition).

**Holds (opened and checked):**
- Decision 1 and 3: one stored group, `PhysicsSettings.semantic` (`crates/visionclaw-domain/src/types/semantic_config.rs`, header `:1-23`); the PUT deep-merges and validates once (`src/settings/api/settings_routes.rs:1280` merge, tests `a_nested_patch_changes_only_the_field_it_names` `:1788` and `semantic_parameters_are_validated_up_front_by_the_one_central_validator` `:1845`).
- Decision 4: every force has its own `enabled` flag (`semantic_config.rs:45`, `:72`, `:96`, `:123`, `:148`, `:172`, `:197`, `:225`) and `refresh_semantic` (`force_compute_actor.rs:4045`) logs an error and leaves semantics off, with no fallback.
- Decision 5: `docs/reference/semantic-forces-fields.json` exists and tests pin it (`the_committed_json_copies_are_current`, `every_slider_range_is_exactly_the_validators_range`, `semantic_config.rs`).
- Decision 6: `dag` (`:55`) and `maturityLayout` (`:206`) default to `enabled: false`.
- Removals: `SemanticForcesActor`, `Configure{DAG,TypeClustering,Collision}`, `UpdateAdvancedParams` and the stub FFI bridge have no remaining reference in `src/`, `crates/` or `client/src` (a commented-out legacy test in `tests/stress_majorization_integration.rs` still names `UpdateAdvancedParams`; it is dead and should be deleted); `src/actors/gpu/mod.rs` no longer registers the actor.
- Layout lock: `static_assert`s at `semantic_forces.cu:139-162` (140-byte `SemanticConfig`, 4108-byte `DynamicRelationshipBuffer`); `#[serde(deny_unknown_fields)]` on every force type and the group (`semantic_config.rs:31`, `:62`, ..., `:242`); GPU read-back test `the_stored_settings_document_is_what_the_device_holds` (`src/utils/unified_gpu_compute/semantic.rs`).
- "Not yet done" is still accurate: `fill_mode_defaults` is still in the compute actor (`force_compute_actor.rs:4261`, called `:2432`, `:2606`), and `toggle_sssp` still patches live parameters via `UpdateSimulationParams` without writing the store (`src/handlers/api_handler/analytics/sssp_handlers.rs:55-90`).

**Drifted: the scope of decision 2.** "There is no scale factor, multiplier, clamp or cap between the store and the GPU" is true of the *semantic group* and of every stored force parameter. It is not true of the compute path as a whole, and was not when written. Three code-held constants remain, none of them a stored parameter:
1. Defensive divergence backstops after read-back: position clamp `MAX_COORD = 100_000`, velocity `MAX_VELOCITY_MAGNITUDE = 1_000`, kinetic-energy trip `MAX_KINETIC_ENERGY = 1e9` (`force_compute_actor.rs:39-50`, clamp at `:2206-2212`). They only act on runaway frames.
2. Per-node derived inertia and repulsion: class mass `min(0.5 + 2w, 5.0)` from normalised degree weight (`force_compute_actor.rs:1231`) and the per-domain repulsion charge 1.2 / 0.6 (`src/actors/gpu/domain_charge.rs:25-27`), both computed on upload.
3. The display Z compression clamp (`display_projection.rs:78`).
The rule therefore reads: *stored force parameters are applied as stored; derived per-node weights and divergence backstops are code constants, documented where they are defined* (`physics-parameters.md`, "Degree-weighted gravity" and "Semantic forces"). These are not a violation of the single-definition rule, but they are a second place a repulsion value lives (`domain_charge.rs`); moving it into `physics.*` is the follow-up if the charge ever needs tuning.
