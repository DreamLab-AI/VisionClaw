---
id: ADR-2029
title: Force channels map over the flat struct; execution.rs is the flag authority and ENABLE_CONSTRAINTS is residency-owned
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: e2afd65d656977d082382e724198e86d2b8cabdf
verified_paths: [src/models/force_channels.rs, src/utils/unified_gpu_compute/execution.rs, src/models/simulation_params.rs]
owner: jjohare
review_trigger: array-backed force-term refactor (deferred step 2), or any new host→GPU conversion path
repo: visionclaw
domain: GPU-wire-abi
lineage: legacy ADR-098 (keystone residency wire — flip the already-allocated bit from residency; 18,933 constraints live), ADR-138 (mapping-layer-now / array-backed-later + guarded Constraints round-trip), ADR-141 (phased programme consuming the seam).
---

# ADR-2029 — Force channels map over the flat struct; execution.rs is the flag authority and ENABLE_CONSTRAINTS is residency-owned

## Context
The ten force terms are scattered scalars plus feature-flag bits inside the flat
SimParams (ADR-2028). A future refactor wants them array-backed, but shipping that
now would churn the ABI. Three separate host→GPU conversion paths derive the same
`repulsion/spring/centering` enable bits from the `scalar > 0` rule, risking drift.
Constraints are residency-owned: the bit means "constraints are resident", set from
`num_constraints > 0` (18,933 live per ADR-098), not a user preference. A naive UI
toggle on it would fight the residency system every tick.

## Decision
Force terms are exposed through a bounded `ForceChannel` enum that maps each channel
to its existing scalar + flag — a view/mutator seam, not an array rewrite (the
array-backed step 2 is deliberately not built). `execute_physics_step` is the single
flag authority: it rebuilds `feature_flags` from scratch, calls `to_sim_params()`, then
overwrites the flag word — setting `ENABLE_CONSTRAINTS` iff `num_constraints > 0` and
the runtime SSSP bit. Consequently `Constraints` is a read-only channel: its `apply()`
is a no-op, so any UI constraints toggle is inert by design. This forecloses letting
the registry own constraint enablement and forecloses a second flag-deriving authority.

## Consequences
Flag derivation has one owner, so paths cannot disagree on the constraint or SSSP bit,
and the seam lets the array-backed refactor land later touching only channel bodies.
The costs: `Constraints.apply()` silently doing nothing surprises anyone expecting the
registry to gate it; the three conversion paths still each derive the simple enable bits
(only the authoritative path adds the residency/runtime bits); and the deferred step 2
means the "array-backed" promise is documented but unshipped. A UI author must know the
constraints channel is diagnostic-read-only.

## Verification
Re-checked at e0f8cd896: `src/models/force_channels.rs` — `apply()` returns early when
`is_read_only()` (lines 191–203), `is_read_only` matches only `Constraints` (lines 210–212),
and tests `only_constraints_is_read_only` / `constraints_apply_is_a_noop` (from line ~372).
`src/utils/unified_gpu_compute/execution.rs` — `execute_physics_step` rebuilds
`feature_flags` from 0 (line 884), sets `ENABLE_CONSTRAINTS` when `num_constraints > 0`
(lines 903–904), calls `to_sim_params()` then overwrites the word (lines 912–913).
`From`/`to_sim_params` impls at `simulation_params.rs:222,236,319`. Mapping layer present;
array-backed refactor absent — implementation is partial as recorded.
Re-verified at `542d63d1d` after the ADR-141 formatting sweep (test-only line
wrapping of a `constraint_max_force_per_node` assertion in `force_channels.rs`) —
the flag-derivation semantics are unchanged.

## Closeout extension — 2026-09-04

CP-01/06/08. Owner remains jjohare with simulation/GPU maintainers. Partial/live is retained: the scalar mapping exists and the array-backed step remains deferred. Current source confirms the final physics-step wrapper derives residency/runtime flags and overwrites converter flags before execute; Constraints.apply is a no-op. An actor parameter mirror also derives flags, so the authority claim is scoped to final dispatch rather than every in-memory flag assignment.

**Acceptance condition:** Inventory conversion/update/direct-execute callers. Observe the final device word and force output through zero/nonzero/removed constraint residency, runtime SSSP changes, scalar boundaries and settings updates. Keep constraints read-only in product controls or explicitly revise the ownership decision. Reopen on new conversion/dispatch paths, residency semantics or the deferred array refactor. See [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md#simulation-layout-and-force-authority) and [source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/wire-force-boundaries.json). No constraint upload or GPU tick ran; the historical resident count is not current acceptance evidence.

## Acceptance progress — 2026-09-05

**Caller inventory.** Three paths reach the feature word; only one is
authoritative.

| Path | Role | Authority |
|---|---|---|
| `SimulationParams::to_sim_params()` (converter) | maps settings → `SimParams`, including a feature word | **not** authoritative — its word is overwritten |
| actor parameter mirror | holds a converter-derived word for inspection | informational only |
| `execute_physics_step_with_bypass` → `execute` | builds the final word immediately before dispatch | **authoritative** |

`execute_physics_step` delegates to `execute_physics_step_with_bypass`, so both
direct-execute entry points share one derivation. Two of the derivation's inputs —
constraint residency and the runtime SSSP toggle — are live device state the
converter cannot see, which is *why* the converter's word cannot be trusted and is
assigned over.

**Implemented — the derivation is now a pure, observable function.** The logic was
inline in the physics-step wrapper on `UnifiedGPUCompute`, so the final device word
could not be observed without a GPU. It is extracted to
`models::force_channels::derive_dispatch_feature_flags(ForceDispatchInputs)`, and
the wrapper now calls it. Behaviour is unchanged; what changed is that the exact
word uploaded to the device is computable and assertable on the host.

Rules, now documented in one place: repulsion/springs/centering gate on a
**strictly positive** scalar (so `0.0`, `-0.0`, negative and `NaN` are all off —
a poisoned scalar disables its term rather than enabling one the kernel cannot
evaluate); SSSP spring adjust is the settings flag **or** the runtime toggle;
`ENABLE_CONSTRAINTS` derives from residency (`num_constraints > 0`) and never from
a setting — which is why `ForceChannel::Constraints` is read-only in the registry
and its `apply` is deliberately a no-op.

**Tests run.** `cargo test --lib --no-default-features adr_20` — 34 pass, 8 in
`adr_2029_dispatch_authority`, all observing the final device word:

- residency `0 → 1 → 4096 → 0`, asserting the bit tracks in **both** directions
  (removing every constraint must clear it, or the kernel keeps walking a buffer
  that no longer describes anything);
- residency decides constraints regardless of every scalar and SSSP combination;
- scalar boundaries — `0.0`, `-0.0`, `-1.0`, `NaN`, `-inf` all off;
  `f32::MIN_POSITIVE`, `1e-6`, `1.0`, `inf` all on;
- each scalar gates only its own term;
- runtime SSSP toggle changes the word with no settings change, and the two
  sources are OR-ed;
- the dispatch word overriding the converter's — a settings record with
  `repel_k = 0` plus 3 resident constraints yields a word carrying the
  residency-derived bit the converter cannot know, and differing from the
  converted word;
- input gathering from settings plus runtime state;
- no bit outside the five declared flags is ever produced.

**Governed paths changed.** `src/models/force_channels.rs`,
`src/utils/unified_gpu_compute/execution.rs`.

**Open.** No constraint was uploaded, no GPU tick ran, and no force *output* was
observed — these tests observe the word, not its effect on positions. Settings
updates through the live actor path, and the deferred array-backed channel
representation, are untouched. Partial/live is retained.

## Re-verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. The largest single case,
`src/models/simulation_params.rs` (+303/-70 raw), is the `SIMPARAMS_MANIFEST`
literal reflowed one-field-per-line: its field names and byte offsets hash
identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

**Governed changes since `9423abdb3`:** all three paths —
`src/models/force_channels.rs`, `src/utils/unified_gpu_compute/execution.rs` and
`src/models/simulation_params.rs`.

**The single-authority decision holds, but the authority has moved and the
Verification citations above are now wrong.** That block cites
`execute_physics_step` rebuilding `feature_flags` from `0` at
`execution.rs:884`, setting `ENABLE_CONSTRAINTS` at `:903-904` and overwriting
the converter's word at `:912-913`. At HEAD none of those lines hold that code.
The current shape:

- `execute_physics_step` (`execution.rs:934-939`) is now a thin wrapper
  delegating to `execute_physics_step_with_bypass` (`:941`).
- The flag word is derived by a **pure function** —
  `crate::models::force_channels::derive_dispatch_feature_flags(
  ForceDispatchInputs::new(params, self.num_constraints,
  self.sssp_spring_adjust_enabled))` at `execution.rs:954-960`, with the
  ADR-2029 rationale comment at `:946-953` naming this record.
- The converter's word is still **overwritten, not trusted**:
  `let mut sim_params = params.to_sim_params();` at `:967` immediately followed
  by `sim_params.feature_flags = feature_flags;` at `:968`.

This is the same decision realised better, not a different one: extracting the
derivation to a pure function is precisely what makes the 2026-09-04 acceptance
condition ("observe the final device word … without a GPU") testable, and the
comment at `:947-950` says so. `derive_dispatch_feature_flags` is at
`force_channels.rs:486`, `ForceDispatchInputs` at `:427`, with the residency rule
documented at `:439` and `:478` and the runtime-SSSP-or-settings rule at `:477`.

**Constraints is still a read-only channel.** `apply` at `force_channels.rs:183`
returns early on `is_read_only` (`:210`), which matches only
`ForceChannel::Constraints`; the rationale comment at `:185` and `:207` still
states the flag is rebuilt every physics step from residency. The tests
`only_constraints_is_read_only` (`:373`) and `constraints_apply_is_a_noop`
(`:380`) both survive.

**Status stays `partial`:** the array-backed step 2 is still not built — the
channel enum still maps onto scattered scalars (`:118`, `:132`, `:165`, `:222`) —
and no constraint upload or GPU tick ran in this pass.

**Commands run:** `git diff --stat 9423abdb3..HEAD -- src/models/force_channels.rs
src/utils/unified_gpu_compute/execution.rs src/models/simulation_params.rs`;
`grep -n` over both files for `derive_dispatch_feature_flags|ForceDispatchInputs|
is_read_only|ENABLE_CONSTRAINTS|ENABLE_SSSP|to_sim_params|execute_physics_step`;
`awk` dumps of `execution.rs:930-1000`; `cargo test --lib --no-default-features
force_channels` → **19 passed, 0 failed** (1248 filtered out).


## Source closeout verification — 2026-09-07

Connected extent is a separate reduction consumed only by the unbounded isolated-shell radius. The all-node spatial-index extent and final force-channel flag derivation remain intact. Constraints ownership and the deferred array representation are unchanged; this fix does not certify every runtime force/residency combination.

Verified implementation: `1ad881cab5ed786fc112f6e50db03fd587e23ec0`. Evidence: [VisionClaw execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md). The embedded-pod library suite passed 1,364 tests (six ignored); a subsequent focused three-test handshake suite also passes. Source verification does not assert deployment activation. Earlier dated observations remain historical.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `src/models/force_channels.rs`: `ToSimParams` imported only in the test module that uses it; doc paragraph breaks; `src/models/simulation_params.rs`: calls the renamed `AutoBalanceConfig::recommended()` / `AutoPauseConfig::recommended()` (the inherent constructors formerly spelled `default()`; same values); `src/utils/unified_gpu_compute/execution.rs`: `AABB` renamed `Aabb` (same `repr(C)` layout); `connected_extent` is `pub(crate)`; `div_ceil`. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record). **Still holds.**

## Re-verification — 2026-10-08 at 31bc3d3a1 (ADR-2135 amendment, ADR-2136)

`src/models/simulation_params.rs` drops only the retired `graph_separation_x` from the reverse conversion. Flag derivation is untouched. Decision holds. Citations at 31bc3d3a1: `src/models/simulation_params.rs:182`.

## Re-checked at b4a175412 (2026-10-19)

Governed changes since `31bc3d3a1`: `src/utils/unified_gpu_compute/execution.rs` (grid cell floor and cell-bounds launch fixes, plus a new optional semantic-force pass `enable_semantic` / `set_semantic_config`, applied to the force buffers before integration when `semantic_active`) and `src/models/simulation_params.rs` (see ADR-2028). `force_channels.rs` is unchanged. **Holds.** The flag authority is as decided: `execute_physics_step_with_bypass` (`execution.rs:1056`) calls `force_channels::derive_dispatch_feature_flags` (`:1069`, defined `src/models/force_channels.rs:488-508`; `ENABLE_CONSTRAINTS` iff `num_constraints > 0` `:504`), calls `params.to_sim_params()` (`:1082`) and overwrites the word (`:1083`). `ForceChannel::Constraints` is still the only read-only channel (`is_read_only` `force_channels.rs:212`, `apply` early return `:185-190`). Note, not a conflict: the semantic forces are a separate module outside the ten-channel registry and outside `feature_flags`; they derive no flag bit, so there is still no second flag-deriving authority. They do mean "ten force terms" is now the flat-struct count, not the total number of force contributions per step.

## Close-out 2026-10-31

**Gap closed:** the "GPU tick observing `ENABLE_CONSTRAINTS` effect on positions". Module
`adr_2029_constraint_gpu_tests` in `src/utils/unified_gpu_compute/execution.rs` steps the real kernel with
repulsion, springs, centring, gravity and jitter all zero, so only the constraint loop can move a node. The kernel
loops `for c < num_constraints`, so with nothing resident the bit is moot; the tests therefore keep the **same
resident constraint** and change only `feature_flags`, which `execute()` uploads as given (it does not re-derive):

- `bit_4_alone_decides_whether_a_resident_constraint_moves_positions` — 64 nodes, a DISTANCE constraint between
  nodes 0 and 1 (start gap 400, target 10), `execute` called directly for 30 frames. Bit 4 clear: gap stays 400
  (1e-3). Bit 4 set: the gap closes, positions finite, constrained nodes differ and every other node is
  bit-identical.
- `the_physics_step_authority_sets_the_bit_the_converter_leaves_clear` — the converter's word has bit 4 clear
  (asserted); `execute_physics_step_with_bypass` on the same resident constraint produces positions bit-identical to
  the explicit bit-set run, so the authority's overwrite is what turns the effect on.
- `removing_residency_through_the_authority_restores_the_unconstrained_result` — constraints uploaded then cleared
  give positions identical to a never-constrained engine (residency cleared gives the unconstrained result; with
  zero constraints the loop is empty regardless of bit 4, so the bit's clearing direction is covered by the
  host-side `constraint_enablement_follows_residency_through_zero_nonzero_zero`, not by this GPU test).

An earlier draft of these tests compared "constraints resident" against "none resident"; that shows residency,
not the flag, and a "settings change cannot switch it on" test that could not fail. Both were removed.

**Commands run** (worktree `adr-sprint/w2`, shared target dir). GPU present (`nvidia-smi -L`: RTX A6000 and two
RTX 6000 Ada); no test skipped. `cargo test --lib adr_2029_constraint_gpu -- --nocapture` → 3 passed, 0 failed,
no `SKIPPED` line (about 17 s). `cargo test --lib adr_2029` earlier in the session → 8 passed (the host-side
`adr_2029_dispatch_authority` set, unchanged). `cargo clippy -p visionclaw-server --lib --tests`: the crate has
45 pre-existing warning/error lines; none is in the new module (the one `execution.rs` hit, at line 1986, is in
older test code). `cargo fmt --all --check` reports 280 pre-existing diffs repo-wide, none in the files this change
touches.

**Status stays `partial`.** The array-backed step 2 is deliberately not built (out of scope). The scenario uses a
DISTANCE constraint on the main force pass only; SEPARATION/POSITION/ALIGNMENT kinds, the bad-frame fallback pass
and the production 18,933-constraint residency are not exercised. `verified_commit` is the last code commit; a
docs-only commit follows it.

## Re-verification — 2026-10-31

ADR-2140 (`25cac8851`) makes the settings half of the OR a stored value: `From<&PhysicsSettings>` now sets `use_sssp_distances` from `physics.ssspEnabled` (default on) instead of always on, and `derive_dispatch_feature_flags` is unchanged (`force_channels.rs:499`, `use_sssp_distances || sssp_spring_adjust_enabled`). The runtime toggle is still a separate input that defaults off. `cargo test --lib adr_20` → 43 passed, 0 failed, and `cargo test --lib sssp` passes, including `the_runtime_sssp_toggle_changes_the_word_without_a_settings_change`.

Re-stamped to `b06d44613` after merging W2's close-out (2026-10-31). The `From<&PhysicsSettings> for SimulationParams` conversion this note describes lives in `crates/visionclaw-domain/src/models/simulation_params.rs:467` (one line changed, `use_sssp_distances: physics.sssp_enabled`), outside `verified_paths`. Re-run: `git diff --stat ae0aed893f34e17b89db0562f0770f4ad677bf25 b06d44613 -- src/models/force_channels.rs src/utils/unified_gpu_compute/execution.rs src/models/simulation_params.rs crates/visionclaw-domain/src/models/simulation_params.rs` → `crates/visionclaw-domain/src/models/simulation_params.rs | 2 +-` and `src/models/simulation_params.rs | 8 +++++---` (2 files changed, 6 insertions, 4 deletions); `force_channels.rs` and `execution.rs` are unchanged since W2's close-out, so both sections stand. After the merge: `cargo test --lib adr_20` → 46 passed, 0 failed (43 here plus W2's 3 GPU tests).

## Re-stamp — 2026-10-31 (ADR-2140 item 1)

**Governed change:** `execution.rs` gains only a test module (`adr_2140_layout_mode_gpu_tests`, the GPU receipt) and `simulation_params.rs` routes the direct `PhysicsSettings` conversion through the resolver. `derive_dispatch_feature_flags` and `force_channels.rs` are unchanged, so the final feature word is still derived there from the (now mode-resolved) scalars and the settings flag, and a stored `ssspEnabled=false` clears the bit in every mode (the receipt reads `c_params` back). **Decision holds.** Re-run: `git diff --stat 75b72416b 93302275e -- src/models/simulation_params.rs src/utils/unified_gpu_compute/execution.rs`. Checked by reading that diff against this record's claims; server `cargo test --lib` 1721 passed, 0 failed, 9 ignored, `cargo test -p visionclaw-domain` 317 passed at `93302275e`.

Re-verified at `d184e89fb` after ADR-2173 touched `execution.rs`: `derive_dispatch_feature_flags` is still the single authority (call at `execution.rs:1150`); `cargo test --lib` 1773 passed, including all four `adr_2029_constraint_gpu_tests` on a real device.

## Re-verification — 2026-11-01
Re-verified at `d5c46cdd8`: `execute_physics_step` is still the single flag authority and `Constraints` is still read-only; the only `execution.rs` changes are the stage marks and the live-shape timing test. `cargo test --lib` 1774 passed, 0 failed.

## Re-stamp — 2026-11-01 (ADR-2169 item 9, `phys/hold-springs`)

**Governed change:** `execution.rs` gains the overlay pointer, the held-only gain (applied to `spring_k` and `max_force` after the feature word is derived) and GPU tests. `derive_dispatch_feature_flags` and `force_channels.rs` are unchanged, so the final feature word is as before; the force-flag tests pass. Re-stamped from `37d1226b1` to `bfaf6e77c`. Re-run: `git diff --stat 531df5ef7 bfaf6e77c -- src/utils/unified_gpu_compute/execution.rs` → `1 file changed, 765 insertions(+), 2 deletions(-)`.

## Re-stamp — 2026-11-01 (integration of `phys/tick-rate` and `phys/hold-springs`, `8f796ad86`)

`derive_dispatch_feature_flags` is still the single flag authority (call at `execution.rs:1141`) and `Constraints` is still read-only. The `execution.rs` changes are the stage marks, the overlay-pointer binding (`bind_edge_rest`), the held-only gain applied after the word is derived, and tests. The four existing `adr_2029_constraint_gpu_tests` pass on a real device, now beside the oracle-index equality test (both kernels, nodes in motion), `an_unbound_index_means_no_constraints` and two pointer-state tests. A resident constraint with an unbound index now acts as no constraints, which does not change the flag derivation. Re-run: `git diff --stat 531df5ef7 8f796ad86 -- src/models/force_channels.rs src/utils/unified_gpu_compute/execution.rs src/models/simulation_params.rs` → `1 file changed, 1445 insertions(+), 47 deletions(-)`.

## Re-stamp — 2026-11-01 (comment-only follow-up, `1ec762f39`)

Comment-only change since `8f796ad86` (the commit the tests and measurements in the previous stamp ran at): `git diff 8f796ad86 1ec762f39 -- crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu src/utils/unified_gpu_compute/execution.rs` is 3 comment lines in the `.cu` file and 1 doc-comment line in `execution.rs`, no code token. The `.cu` still compiles with `nvcc -ptx`, and its PTX still has `force_pass_kernel` at 30 parameters, `force_pass_with_stability_kernel` at 29, `c_params[212]` and no `blend_semantic_physics_forces`. Re-run: `git diff --stat 531df5ef7 1ec762f39 -- src/models/force_channels.rs src/utils/unified_gpu_compute/execution.rs src/models/simulation_params.rs` → `1 file changed, 1446 insertions(+), 48 deletions(-)`.

## Re-stamp — 2026-11-01 (constraint scheduling, `e2afd65d6`)

Re-read against `execution.rs` at `e2afd65d6`: the change is test-only (the live-shape timing test steps through the actor's upload decision). `derive_dispatch_feature_flags` is still the single flag authority and `Constraints` is still read-only; the four `adr_2029_constraint_gpu_tests` and the oracle-index equality test pass. Re-run: `git diff --stat 7fbcd997b e2afd65d6 -- src/utils/unified_gpu_compute/execution.rs`.
