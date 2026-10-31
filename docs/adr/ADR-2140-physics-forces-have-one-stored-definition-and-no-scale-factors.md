---
id: ADR-2140
title: Physics forces have one stored definition, applied as stored — no scale factors, caps or shadow state
date: 2026-10-13
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 412849582c5bfa558730884b88232beb7f95bd03
verified_paths: [src/handlers/layout_handler.rs, src/actors/gpu/force_compute_actor.rs, src/actors/gpu/physics_restore.rs, crates/visionclaw-domain/src/types/layout.rs, src/utils/unified_gpu_compute/execution.rs, src/handlers/api_handler/analytics/sssp_handlers.rs, src/settings/api/settings_routes.rs, crates/visionclaw-domain/src/types/physics_config.rs, src/models/simulation_params.rs, crates/visionclaw-domain/src/models/simulation_params.rs, docs/reference/physics-parameters.md]
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
- ~~The layout MODE and its bias priming (`fill_mode_defaults`) still live in the compute actor; the store holds a different vocabulary (`qualityGates.layoutMode`).~~ Closed 2026-10-31 (item 1; see the second Close-out).
- ~~`POST /sssp/toggle` patches the persisted parameters but does not write the toggle back to the store.~~ Closed 2026-10-31 (see Close-out).
- `GET /analytics/sssp/params` (`sssp_handlers.rs`) still hard-codes `useSsspDistances:false, ssspAlpha:0.5`, and `POST /analytics/sssp/params` is a no-op answering `success:true`. Both contradict `/sssp/status` and the store (default `true` / `1.5`). They predate this change and the client calls none of the `/sssp/*` endpoints; the fix is to read the store in the GET and route the POST through `apply_physics_patch`, or delete both.

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

## Close-out 2026-10-31

`implementation_status` stays **partial**: two of three recorded gaps are closed, the layout-mode move is not.

**Gap 3 — dead legacy test: already closed at HEAD.** `tests/stress_majorization_integration.rs` no longer exists; `ec65631d6` ("clear the residues the verification found") deleted it (686 lines). `grep -rn UpdateAdvancedParams` over `*.rs`/`*.ts` returns nothing.

**Gap 2 — `POST /sssp/toggle` writes the store: closed.**
- `PhysicsSettings.sssp_enabled` (`ssspEnabled`, alias `sssp_enabled`, serde default **true**) holds the toggle. Both conversions read it: `From<&PhysicsSettings> for SimulationParams` (`use_sssp_distances`) and `for SimParams` (`ENABLE_SSSP_SPRING_ADJUST` set only when on). They used to hard-code on.
- The body of `PUT /api/settings/physics` is now `apply_physics_patch` (deep merge, both central validators, store, GPU actors, SQLite, broadcast); the PUT and `toggle_sssp` both call it. `sssp_patched` and `live_params` (a SQLite-only second read path) are deleted. `GET /sssp/status` reads the stored value.
- Behaviour changes: alpha is bounded by the central validator (0..=5) instead of the route's own 0..=1; the toggle persists whether or not a GPU actor exists; the response echoes the stored alpha.
- Review follow-ups, same change: `apply_physics_patch` now returns 500 "GPU update failed: …" when the GPU actor answers `Err` (as the pre-refactor toggle did); this also makes `PUT /api/settings/physics` stricter than before (it used to log and answer 200). `/analytics/sssp/toggle` is wrapped in `RateLimit::per_minute(60)`, like `/api/settings/*`, because one toggle is now a full physics write. `GET /analytics/feature-flags` reports `sssp_integration` read-only from the stored `physics.ssspEnabled`; the `FEATURE_FLAGS` write in `toggle_sssp` and the fallback in `/sssp/status` are gone, and a body that sets `sssp_integration` is ignored. `PhysicsSettingsDTO.sssp_enabled` defaults to true.
- Known limitation (closed by Gap 1): in DAG modes the device can run SSSP on while the store, and so `/sssp/status`, says off (see Gap 1, quality-gate propagation).
- Compat: a document without `ssspEnabled` loads as enabled (test `a_document_stored_before_sssp_enabled_existed_loads_enabled`).
- Tests: `sssp_patch_tests` (6, in `sssp_handlers.rs`) cover patch shape, only-SSSP-fields-change, omitted alpha, stored toggle reaching both GPU conversions, out-of-range alpha refused.

**Gap 1 — layout mode into `physics.*`: not done.** Read at this head, it is a design, not a move:
- `fill_mode_defaults` primes `dagBiasK`/`dagLevelDistance`/`layerBiasK`/`layerSpacing` from code constants (`RADIAL_DEFAULT_*`, `HIERARCHICAL_DEFAULT_*`) when the stored value is `<= 0`. Under decisions 1 and 2 each is a stored per-mode value, which needs a ruling on what the stored `0` ("off") means per mode.
- Three vocabularies must be mapped: `qualityGates.layoutMode` (`force-directed`, `dag-topdown`, `dag-radial`, `dag-leftright`, `type-clustering`, `spectral`, `clustered`, hand-listed in `settings_routes.rs`), the `LayoutMode` enum (6 variants, `visionclaw-domain/src/types/layout.rs`) and `POST /api/layout/mode`; `dag-leftright` and `type-clustering` have no enum variant.
- The quality-gate propagation (`settings_routes.rs:1224-1226`) overrides the *device* value in DAG modes (including a `max(0.5)` floor on `ssspAlpha` and `max(0.1)` on centre gravity); it does not write the store. With `ssspEnabled=false` stored, a DAG layout runs SSSP on while `/sssp/status` reports off, until the next physics write drops the overlay. `settings_handler/physics.rs:102` is unreachable (only called with `None`; routes unmounted) and is dead code to delete.
- The CPU one-shot modes (Spectral/Temporal) are placed by the layout handler, not the GPU; the stored field must say what they do.
- The client writes `qualityGates.layoutMode` (`GraphManager.tsx`) and calls `layoutApi.setMode`.
Needed: the per-mode stored-value ruling, a `physics.layoutMode: LayoutMode` field with an explicit migration from the quality-gate strings, `POST /api/layout/mode` through `apply_physics_patch`, the actor's preserve-override and `fill_mode_defaults` removed, the two override blocks removed, and the client control re-pointed, with a GPU receipt.

**Commands (worktree `adr-sprint/w3`, `CARGO_TARGET_DIR=/home/devuser/workspace/project/target`):**
- `cargo test --lib` (full) → 1695 passed, 0 failed, 9 ignored; `cargo test --lib adr_20` → 43 passed.
- `cargo test --lib sssp` → 7 passed, 0 failed. The idempotency gate compares the whole GPU image, so a stored toggle is not swallowed (`a_stored_toggle_changes_the_gpu_image_the_actor_gate_compares`, in `sssp_handlers.rs`).
- `cargo test -p visionclaw-domain` → 301 passed, 0 failed, 1 ignored (lib; plus 5 passed in `vault_fixtures`).
- `cargo test --lib -- settings_routes simulation_params settings_handler physics` → 110 passed, 0 failed.
- `cargo clippy --lib -p visionclaw-server`: no warning in the touched code after the fix (baseline warnings elsewhere are unchanged).
- `cargo run --bin generate_types` → completes; no tracked client file changed. Generated types are gitignored (`client/src/types/generated/settings.ts`, which does carry `ssspEnabled: boolean`) and regenerate at build; the hand-written client types (`client/src/api/settings/types.ts`) and the control-centre manifest do not carry `ssspEnabled`. The client neither sends nor reads it, and the deep merge preserves the stored value on client physics PUTs. No client test consumes the generated physics keys.

Re-stamped to `b06d44613` (2026-10-31) after the review follow-ups. Re-run: `git diff --stat 5a2723831 b06d44613 -- docs/reference/physics-parameters.md src/handlers/api_handler/analytics/sssp_handlers.rs src/settings/api/settings_routes.rs` → `physics-parameters.md | 5 +++--`, `sssp_handlers.rs | 17 ++++++++++-------`, and the `apply_physics_patch` GPU-`Err` hunk in `settings_routes.rs`. Claims re-checked: the toggle still writes through `apply_physics_patch`; `/sssp/status` and `/analytics/feature-flags` both read the store. Verified binaries: `cargo test --lib sssp` → 9 passed; `cargo test --lib adr_20` → 46 passed; `cargo test --lib -- settings_routes simulation_params settings_handler physics feature_flags` → 113 passed; `cargo test -p visionclaw-domain` → 301 passed, 1 ignored.

## Close-out 2026-10-31 — item 1, the layout mode moves into `physics.*`

`implementation_status` stays **partial**, for one reason that is not item 1: `GET/POST /analytics/sssp/params` (third "Not yet done" bullet) still answer from hard-coded values. Item 1 is closed. When that endpoint pair is routed through the store or deleted, this record can close with `evidence_class` and an `acceptance_boundary` that reads "local runs, not deployed". `activation_status` is unchanged (`staged`).

**Owner rulings, 2026-10-31 (binding, implemented as stated):**
1. *A stored 0 means "use the mode default".* `dagBiasK`, `dagLevelDistance`, `layerBiasK` and `layerSpacing` keep that meaning. The resolution left the compute actor and now happens in `SimulationParams::resolve_mode_defaults`, called by `From<&PhysicsSettings> for SimulationParams`, the one place stored physics becomes device parameters (`SimParams::from(&PhysicsSettings)` goes through it too). The store keeps the 0; existing documents behave as before.
2. *`dag-leftright` and `type-clustering` are `LayoutMode` variants of their own* (`DagLeftRight` = GPU 6, `TypeClustering` = 7, appended; the first six are not renumbered). Nothing is dropped or remapped, no kernel was added. Each does what its string did: `dag-leftright` ran the `hierarchical` layer spring plus the DAG defaults (there is no orientation code, and `DagLeftRight` does not invent one); `type-clustering` ran the force-directed baseline with `clusterStrength` raised to 0.01.

**What changed, with the behaviour each preserved:**
- *Field and migration.* `PhysicsSettings.layout_mode` (`layoutMode`, alias `layout_mode`, default `forceDirected`; `physics_config.rs`). All seven strings map to distinct modes (`LayoutMode::from_quality_gate_str`, `layout.rs`); `temporal` has no string. Write is strict: `merge_physics_patch` refuses an unknown or non-string mode with a 400 before the merge, and stores the serde name for either vocabulary. Load is lenient: the field deserialiser logs and defaults, so one bad value does not drop the document. A stored document without the field takes the mode from the stored `quality_gates.layoutMode` at boot (`migrate_layout_mode`, `physics_restore.rs`, used by `app_state.rs` and `load_saved_physics`) and is written back once; an unknown string is a logged default.
- *Endpoint.* `POST /api/layout/mode` applies `{"layoutMode": …}` through `apply_physics_patch` (`layout_handler.rs`). It now needs an authenticated user and carries `RateLimit::per_minute(60)`, like `/sssp/toggle`. A request for the already-stored mode writes nothing (the client repeats the call on every page load), so a repeat does not rewrite the store, broadcast or resume physics. `GET /layout/modes` and `/layout/status` read the stored mode, and `dagLeftRight` and `typeClustering` are advertised. A legacy `PUT /api/settings/quality-gates` that names a mode is forwarded through the same path, and `GET` reports the quality-gate string from the stored mode.
- *Removed from the actor:* the `SetLayoutMode` message and handler (it also cleared the opposing bias term on a switch), the preserve-override in `UpdateSimulationParams`, `fill_mode_defaults` and its three tests, and the duplicate defaults in `SetRadialLayout` (now the domain constants; it still primes the shells on the device only, as an action, whichever mode is stored).
- *Removed override blocks:* the quality-gate propagation overlay (`settings_routes.rs`, formerly `:1224-1226` and its `type-clustering` arm) and `propagate_physics_to_gpu_with_layout` (`settings_handler/physics.rs`, formerly `:102`, dead).
- *Spectral and Temporal* are CPU one-shot placements by the layout handler. The stored field says "place once"; no GPU term reads the mode (the kernel only carries the field in `SimParams`), so the store and the device cannot disagree.

**Booleans and floors, exactly.** The ruling's "0 = default" does not cover a boolean, so booleans are never touched by a mode: `ssspEnabled` is applied as stored in every mode (a document without it loads `true`), and no mode forces a toggle. The three numeric floors the overlay applied are now stored-0 defaults: in `hierarchical`, `radial` and `dagLeftRight`, `centerGravityK <= 0` → 0.1 and `ssspAlpha <= 0` → 0.5; in `typeClustering`, `clusterStrength <= 0` → 0.01. A positive stored value is applied as stored. `/sssp/status` reports `enabled` and `alpha` from the same conversion, so it reports what the device holds.

**Behaviour changes to accept (the rest is unchanged):**
1. A stored `ssspEnabled=false` is obeyed in DAG modes (the overlay used to force SSSP on).
2. A stored positive `centerGravityK` below 0.1, `ssspAlpha` below 0.5 or `clusterStrength` below 0.01 is obeyed in its mode; the overlay used to raise it. Default documents are unaffected (0.2, 1.5, and 0 → 0.01).
3. No opposing term is cleared on a switch: an explicit positive `layerBiasK` stays in `radial`. Default documents have 0 there.
4. The overlay used to apply only after a quality-gate PUT and was dropped by the next physics write; the mode and its defaults now hold on every write and at boot. A document that stored `qualityGates.layoutMode` but no `physics.layoutMode` boots into that mode instead of `forceDirected` until the client reconnects.
5. The direct enum names `hierarchical` and `radial` now carry the DAG defaults (before, only the `dag-*` strings did); the client only ever sent the strings.

**Client.** The control (`motion.ts`), `GraphManager.tsx`, `GraphCanvas.tsx`, `CommandInput.tsx` and the settings helpers read and write `visualisation.graphs.knowledge.physics.layoutMode`; the control's options are the server's names (`forceDirected`, `hierarchical`, `radial`, `dagLeftRight`, `typeClustering`, `spectral`, `clustered`). `layoutApi.ts` posts the name verbatim; the quality-gate-to-engine remap (which folded `dag-leftright` into `hierarchical` and `type-clustering` into `forceDirected`) is deleted with its test. `qualityGates.layoutMode` stays on the wire as a deprecated read-only echo. `cargo run --bin generate_types` ran; the generated files are gitignored (`client/src/types/generated/settings.ts` carries `layoutMode`).

**Tests (worktree `adr-sprint/2140-layout`, `CARGO_TARGET_DIR=/home/devuser/workspace/project/target`; every count read from a copied binary whose `--list` showed the new tests):**
- Fail first: with the new tests added and none of the field, constants or resolver present, `cargo test -p visionclaw-domain --lib` did not compile (`E0560 no field named layout_mode`, `E0425` for the seven default constants). They pass at `412849582`.
- `cargo test --lib` (full) → 1721 passed, 0 failed, 9 ignored (main: 1702 − 3 deleted `mode_default_tests` + 19 new + 3 GPU). `cargo test -p visionclaw-domain` → 317 passed (main: 301), 1 ignored.
- *GPU receipt* (`adr_2140_layout_mode_gpu_tests`, `execution.rs`, run with `--nocapture`, no `SKIPPED` line): a patch is merged with `merge_physics_patch`, validated with `validate_for_gpu`, pushed through `execute_physics_step_with_bypass`, and `c_params` is read back from device memory. For all seven strings the device holds its own discriminant, `ssspEnabled=false` leaves the SSSP bit clear, and `/sssp/status`'s value equals the device bit; stored zeros read back as 1.0/60 (radial), 1.0/60 (layer), 0.1, 0.5, 0.01; a stored 0.05/0.3 reaches the device unchanged; switching back to `forceDirected` leaves nothing of the previous mode. The receipt drives `merge_physics_patch`, not the HTTP handler, because `apply_physics_patch` needs a live `AppState`.
- Client: `vitest run` → 1457 tests across 139 files pass after the registry test was updated (the frozen fixture is untouched; the old path joins `RETIRED_PATHS`, the new one is asserted independently); `tsc --noEmit` exits 0.

Re-stamped to `412849582` (2026-10-31). Re-run: `git diff --stat 75b72416b 412849582 -- src/handlers/layout_handler.rs src/actors/gpu/force_compute_actor.rs src/settings/api/settings_routes.rs crates/visionclaw-domain/src/models/simulation_params.rs crates/visionclaw-domain/src/types/layout.rs docs/reference/physics-parameters.md`.
