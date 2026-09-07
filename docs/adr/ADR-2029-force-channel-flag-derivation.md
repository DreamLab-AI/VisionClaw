---
id: ADR-2029
title: Force channels map over the flat struct; execution.rs is the flag authority and ENABLE_CONSTRAINTS is residency-owned
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 1ad881cab5ed786fc112f6e50db03fd587e23ec0
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
