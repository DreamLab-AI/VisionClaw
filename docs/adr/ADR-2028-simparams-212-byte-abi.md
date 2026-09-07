---
id: ADR-2028
title: SimParams is a size-locked 212-byte repr(C) raw-copy ABI, grown tail-append only
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 1ad881cab5ed786fc112f6e50db03fd587e23ec0
verified_paths: [src/models/simulation_params.rs, crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu]
owner: jjohare
review_trigger: any new SimParams field, or a driver/toolkit change altering the 212-byte size
repo: visionclaw
domain: GPU-wire-abi
lineage: legacy ADR-138 (flat repr(C) SimParams + dual static_asserts), ADR-141 (grew 180→212 in lockstep), ADR-098 D2 (constraints ride a separate buffer, not a struct field); ADR-031/ADR-061 wire sizes stale.
---

# ADR-2028 — SimParams is a size-locked 212-byte repr(C) raw-copy ABI, grown tail-append only

## Context
The Rust host and the CUDA kernel share simulation parameters every physics tick.
There is no serialisation layer: the struct is copied device-ward as raw bytes.
That makes the memory layout itself the wire contract, and any drift between the
two declarations is silent memory corruption rather than a caught error.
Four shipping clients already speak the current byte prefix (ADR-138/ADR-141).
Successive features (FA2/LinLog, DAG radial bias, the ADR-141 layout blocks)
needed to grow the struct without breaking those clients. Constraints deliberately
do not live in the struct — they ride a separate device buffer (ADR-098 D2).

## Decision
SimParams is a flat `#[repr(C)]` struct declared once in Rust and once in CUDA,
transferred by raw POD device copy via `Pod`/`Zeroable` + `DeviceRepr`/`DeviceCopy`.
Its size is frozen at 212 bytes and guarded by twin compile-time assertions — a Rust
`const _: () = assert!(size_of == 212)` and a CUDA `static_assert(sizeof == 212)`.
Every new field is appended at the tail; the existing prefix is never reordered or
resized. This forecloses adding a serde/flatbuffer layer, inserting fields mid-struct,
and changing any prefix field's type or order. Growing the struct requires bumping
both assertions in the same change.

## Consequences
The layout uses memcpy without an encoding layer. The size assertions reject
size drift; they do not detect all field-order or same-width type mismatches. The cost is rigidity: fields cannot be logically grouped or
reordered for readability, deprecated fields cannot be reclaimed without a coordinated
ABI bump, and the 212 magic number must be maintained in two languages. A reviewer can
verify compliance by checking that a new field is at the tail and both assertions moved.

## Verification
Re-checked at e0f8cd896: `src/models/simulation_params.rs` — derives `Pod, Zeroable`
(line 27), `unsafe impl DeviceRepr`/`DeviceCopy` (lines 118–119), size assertion
`== 212` (line 228), and tail fields each carry the comment "Added at the end to
preserve the existing repr(C) prefix layout" (lines 78–112).
`crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu:117` —
`static_assert(sizeof(SimParams) == 212, ...)`. Both assertions present and in
lockstep; the raw-copy path and tail-append discipline are intact.

## Closeout extension — 2026-09-04

CP-01/06/08. Owner remains jjohare with GPU/protocol/release maintainers. Complete/live is retained for the flat representation and paired size guards. The [host layout probe](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/simparams-layout-probe.json) matches all 53 field offsets at size 212/alignment 4. A temporary same-size field swap still compiles under the original assertion: size guards do not prove field order or type identity.

**Acceptance condition:** Version field/type/offset and feature-bit manifests; compare actual Rust/CUDA toolchains and loaded device-module identity in CI and release evidence. Include same-size drift negatives, copy-size and old/new consumer cases, and coordinated rollout/rollback before tail growth. Reopen on any field, compiler, precompiled module or consumer change. See [simulation compatibility review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md#simulation-layout-and-force-authority). This pass ran host extracted declarations only, with no CUDA or GPU execution.

## Acceptance progress — 2026-09-05

**Implemented — a versioned field/type/offset manifest.** The closeout showed the
existing guard's blind spot precisely: swapping `dt` and `damping` in a fixture
left `size_of` at 212, so the paired `const _: () = assert!(… == 212)` /
`static_assert` still passed while both offsets had moved. Every field is a 4-byte
scalar, so **same-size drift is the ordinary failure mode**, not an exotic one —
and the size guard is blind to all of it.

Added to `src/models/simulation_params.rs`:

- `SIMPARAMS_MANIFEST` — all 53 fields as `(name, FieldType, offset)`, in
  declaration order.
- `SIMPARAMS_FEATURE_BITS` — the 7 `FeatureFlags` bits. The feature word is as
  much part of the device ABI as the offsets: a bit reassigned on one side only
  silently enables the wrong force term.
- `SIMPARAMS_ABI_VERSION` — the coordination token for rollout and rollback, to be
  bumped on *any* manifest change. A precompiled module or raw-copy consumer can
  be checked against this number instead of inferring compatibility from a size
  that did not happen to change.
- `simparams_actual_layout()` reads the real layout with `offset_of!`;
  `verify_simparams_abi()` compares any candidate against any manifest and returns
  every departure as typed `AbiDrift` (`Name`, `Type`, `Offset`, `FieldCount`,
  `Size`). It is generic over the candidate, which is what makes the negative test
  meaningful rather than a restatement of the manifest.
- `simparams_abi_digest()` — a stable FNV-1a tag over the triples plus the feature
  bits, for binding a shipped artefact to the exact ABI it was compiled against.
  Explicitly non-cryptographic and not a security boundary.

**The same-size drift negative test.** Two tests state the gap and its closure as
a pair: `a_same_size_field_swap_still_passes_the_size_assertion` asserts the
drifted fixture is still 212 bytes (so the existing guard passes), and
`the_manifest_catches_the_same_size_field_swap` asserts the manifest reports a
`Name` drift at `dt`'s slot while reporting **no** `Size` drift. A same-size
retype (`f32` → `u32`, which moves nothing and reinterprets every value) and a
tail append (which preserves every existing offset but is still unsafe for a
shorter allocation or an older device module) are covered separately.

**Tests run.** `cargo test --lib --no-default-features adr_20` — 34 pass, 10 in
`adr_2028_abi_manifest`: real struct matches the manifest; frozen size 212 and
alignment 4; dense ascending offsets; unique field names; the two same-size drift
tests above; same-size retype; tail-append growth; digest stability, equality with
the real layout and sensitivity to a same-size reorder; single-bit uniqueness
across all 7 feature bits; and a cross-check that the fields ADR-2029's dispatch
word reads are all present.

**Governed paths changed.** `src/models/simulation_params.rs`.

**Open — this is host-side only.** No CUDA compilation, driver load, device copy
or shipped-PTX comparison ran, and the manifest is not yet compared against the
CUDA declaration by an automated check (the closeout's Python probe does that
comparison out of tree). Matching the loaded device module's identity to the host
binary, and coordinated rollout/rollback across every raw-copy consumer before
tail growth, remain open. `implementation_status` is unchanged: complete/live
still refers to the scoped flat, size-locked representation.

## Re-verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. For this record's largest such file,
`src/models/simulation_params.rs` (+303/-70 raw), the `SIMPARAMS_MANIFEST` field
names and byte offsets hash identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

**Governed change since `eac011303`:** `src/models/simulation_params.rs` only.
`crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu` is **unchanged**,
so the CUDA half of the twin assertion was not touched.

**The 212-byte lock is intact on both sides.** Rust:
`const _: () = assert!(std::mem::size_of::<SimParams>() == 212);` at
`src/models/simulation_params.rs:228` — citation still exact. CUDA:
`static_assert(sizeof(SimParams) == 212, "SimParams size mismatch with Rust");`
at `crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu:117` — also
still exact. The raw-copy path is unchanged: `#[derive(… Pod, Zeroable)]` at
`:27`, `unsafe impl DeviceRepr` / `DeviceCopy` at `:118-119`. Tail-append
discipline is still visible in the source, with the "Added at the end to preserve
the existing repr(C) prefix layout" comments at `:88`, `:94`, `:99`, `:105` and
`:111`.

**The blind spot named in the Consequences is now closed.** That paragraph says
the size assertions "do not detect all field-order or same-width type
mismatches" — every field is a 4-byte scalar, so same-size drift was the ordinary
failure mode and the guard caught none of it. At HEAD there is a versioned
field/type/offset manifest beside the size guard:
`SIMPARAMS_MANIFEST: [SimParamsField; 53]` at `:327`, `SIMPARAMS_SIZE = 212` at
`:296`, `AbiDrift` at `:268`, and a live manifest built with
`std::mem::offset_of!` at `:618-626` and compared against the declared one at
`:967-975`. The test at `:1046` asserts *"same-size drift must be detected"* — the
exact `dt`/`damping` swap the 2026-09-04 closeout used to defeat the size guard.
Cross-checks also assert `SIMPARAMS_MANIFEST.len() * 4 == SIMPARAMS_SIZE`
(`:987`) and that every field lies inside the struct (`:994`), so the manifest
cannot silently disagree with the size.

**Still open, unchanged:** the manifest pins the *Rust* layout. Nothing here
compares the actual CUDA toolchain's layout or the loaded device module's
identity, so the cross-language half of the 2026-09-04 acceptance condition
remains outstanding. No CUDA or GPU execution ran in this pass either.

**Commands run:** `git diff --stat eac011303..HEAD -- src/models/simulation_params.rs
crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu`; `grep -n` over
`simulation_params.rs` for `== 212|Pod, Zeroable|DeviceRepr|DeviceCopy|Added at
the end|SIMPARAMS_MANIFEST|AbiDrift|offset_of`; `grep -n
'static_assert(sizeof(SimParams)' …visionclaw_unified.cu`; `cargo test --lib
--no-default-features simulation_params` → **11 passed, 0 failed**.


## Source closeout verification — 2026-09-07

The CUDA changes add a hard positional clamp and a separate connected-node extent kernel, with no SimParams field, type or offset changes. The actual nvcc integration fixture compiles the production CUDA declaration and passes on a GPU; default-feature Rust binary checking passes. Existing host manifest/offset tests remain in the full library suite.

Verified implementation: `1ad881cab5ed786fc112f6e50db03fd587e23ec0`. Evidence: [VisionClaw execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md). The embedded-pod library suite passed 1,364 tests (six ignored); a subsequent focused three-test handshake suite also passes. Source verification does not assert deployment activation. Earlier dated observations remain historical.
