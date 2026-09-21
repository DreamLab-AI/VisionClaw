---
id: ADR-2030
title: PTX ISA is text-rewritten to .version 9.0 at build time
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 997440cd0717d4c5f9341369571fc69fcf5a38d6
verified_paths: [crates/visionclaw-gpu/build.rs, crates/visionclaw-gpu/src/ptx_policy.rs]
owner: jjohare
review_trigger: host driver gains support for a newer PTX ISA, or nvcc changes its .version emission
repo: visionclaw
domain: GPU-wire-abi
lineage: legacy ADR-070 (CUDA integration hardening — toolkit/driver ISA skew on CachyOS); distils the project-memory PTX-version-mismatch fingerprint.
---

# ADR-2030 — PTX ISA is text-rewritten to .version 9.0 at build time

## Context
CUDA toolkit 13.x emits PTX with `.version 9.x`, but some host drivers (the CachyOS
build environment among them) only accept ISA up to 9.0 and reject anything newer at
load time. Pinning the whole toolkit to an older version to hold the ISA down was the
alternative, but that forfeits newer nvcc and its codegen. The build also has to
tolerate machines with no nvcc at all, where a bundled pre-compiled PTX is the only
option. An empty or missing PTX must fail loudly rather than ship a silently dead kernel.

## Decision
`build.rs` post-processes every compiled `.ptx`: it finds the leading `.version 9.`
token and, if it is not already `.version 9.0`, splices `.version 9.0` in place before
writing the file back. **Both** nvcc failure modes fall back to bundled pre-compiled PTX
from a fixed set of paths — a launch failure (no toolkit on `PATH`) and a non-zero exit are
distinguished only in the *diagnosis*, not in whether the fallback is consulted. If neither
compilation nor a fallback yields PTX it panics. This records the chosen header rewrite rather
than toolkit pinning. Output is rejected by structural validation — directives plus required
symbols — not merely a non-empty check; even so, validation does not establish a tested kernel.
*(Corrected 2026-09-05: this paragraph previously read "failure to launch nvcc panics before
fallback" and described the gate as a non-empty check. Both were true of the original `build.rs`
and are false at `b0bc275f6` — panicking on a missing toolchain defeated the purpose of shipping
fallback PTX, which is exactly why it was changed. Evidence below.)*

## Consequences
The rewrite aims to support older-driver hosts. Driver acceptance and kernel
behaviour still require runtime evidence; failures can occur after the build. The costs: the rewrite is a brittle
string splice keyed on the exact `.version 9.` prefix (a differently formatted or
non-9.x header would slip through), it hard-codes 9.0 as the floor so a genuinely newer
required ISA would need a code change, and the bundled-PTX fallback can mask a missing
toolchain. Compliance means any new kernel goes through this same `build.rs` path.

## Verification
Re-checked at e0f8cd896: `crates/visionclaw-gpu/build.rs` — locates `.version 9.`
(line 165), compares against `.version 9.0` and splices the replacement when different
(lines 168–171), emits a `cargo:warning` on downgrade (lines 172–176); the bundled-PTX
fallback (lines 137–151) and the twin panics on empty/missing PTX (lines 186–187) are
present. The rewrite, fallback and hard-fail behaviours all match the record.

## Closeout extension — 2026-09-04

CP-01/06/08. Owner remains jjohare with GPU/build/release maintainers. Implementation is partial against the original missing-toolchain and compatibility promises; historical live activation is retained. Six [isolated PTX-phase cases](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/ptx-build-probe.json) show absent nvcc panics before fallback, failed nvcc reaches fallback, invalid nonempty output passes and empty output fails. Existing 9.0 content produces a downgrade warning, and an invented two-digit minor exposes fixed-width splicing. Historical verification did not establish those stronger guarantees.

**Acceptance condition:** Test launch failure separately from compiler failure; validate version tokens, required symbols, fallback provenance, native/stub linking and host/device ABI identity. Record the selected runtime module and original/rewritten hashes, then exercise representative kernels on the intended driver with rollback. Reopen on toolkit/driver changes, fallback selection, native linking or runtime loader changes. See [build/runtime review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md#ptx-build-acceptance-and-loaded-artefact-identity). The fixture omits the native phase and uses a fake compiler; no CUDA or driver execution occurred.

## Acceptance progress — 2026-09-05

**Implemented.** The build-time policy is extracted to
`crates/visionclaw-gpu/src/ptx_policy.rs`, which is compiled **twice from one
source**: `include!`d into `build.rs` and declared as `pub mod ptx_policy` in the
library. A build script cannot depend on the crate it builds, which is why the
closeout had to extract a separate probe to test the phase at all — and nothing
guaranteed that probe still matched the script. Now the tests run against the
exact code the build executes. The module is `std`-only, so the include costs
nothing.

Each closeout finding is addressed:

| Finding | Fix |
|---|---|
| `nvcc` absent panicked *before* the fallback was consulted | `NvccOutcome::LaunchFailed` is distinct from `CompilerFailed`; **both** reach the fallback, with different diagnoses. Panicking on a missing toolkit defeated the entire purpose of the bundled PTX. |
| a successful compiler writing `NOT PTX` passed the non-empty gate | `validate_ptx` checks `.version`, `.target`, `.entry` **and required kernel symbols** — the last catches a compiler that succeeded against stale or partial source |
| the fixed 12-byte splice turned `.version 9.10` into `9.00` | `rewrite_ptx_version` locates the version token by span and rewrites exactly those characters, whatever their width. `9.00` is *lower* than either the original or the target — a silent downgrade past the intended floor |
| a "downgrade" warning was emitted on unchanged content | `VersionRewrite::Unchanged` is reported distinctly from `Rewritten` |
| nothing recorded which module was selected, or its identity | `PtxArtefact` records module, source path, provenance, resulting ISA and content tags **before and after** the rewrite; the build writes a manifest to `OUT_DIR` and exports `VISIONCLAW_PTX_MANIFEST` |

Provenance is three-valued — `Compiled`, `FallbackAfterLaunchFailure`,
`FallbackAfterCompilerFailure` — so a manifest line says not just that a fallback
was used but *why*. The panic when no fallback exists now names the diagnosis and
lists every path searched.

**Tests run.**

- `cargo test -p visionclaw-gpu --lib --no-default-features ptx_policy` — 17 pass:
  launch-vs-compiler classification and their distinct diagnoses; success implying
  neither fallback nor validity; fallback provenance per failure; two-digit-minor
  rewrite explicitly asserting `9.00` never appears; unchanged-not-downgraded for
  `9.0`/`8.7`/`1.0`; higher major and minor both downgrading; span-based token
  parsing with tab and multi-space separation; malformed and missing version as
  defects; `9.0` vs `9.10` display; `NOT PTX` and whitespace-only rejected; each
  structural directive required; required symbols checked by name; artefact tag
  divergence on rewrite and equality without.
- **Real toolkit build** — `cargo check -p visionclaw-gpu` (GPU feature on, nvcc
  12.9 present) completed with all 9 modules compiled, validated (including the
  required-symbol check on `visionclaw_unified`) and recorded. Manifest receipt:
  `docs/estate-closeout/2026-09-05/ptx-build-manifest.txt`. The installed toolkit
  emits `.version 8.8`, below the 9.0 target, so every module is correctly
  reported `Unchanged` with `original == rewritten` — no spurious downgrade
  warning, which is the closeout's fourth finding demonstrated on real output.

**Governed paths changed.** `crates/visionclaw-gpu/build.rs`,
`crates/visionclaw-gpu/src/ptx_policy.rs`, `crates/visionclaw-gpu/src/lib.rs`.

**Open — every runtime-side condition.** No driver module was loaded, no kernel
ran, and no rollback was exercised. `ptx_loader`'s *runtime* selection is
untouched: it still prefers modification-time-sorted build outputs, which is not
proof of source revision or ABI identity, and its own `validate_ptx` remains the
weaker three-substring check. Rewriting a declared ISA still does not prove the
instruction body is supported by the target driver. Implementation remains partial
against the original compatibility promise; the build phase is now tested and
recorded, the runtime phase is not.

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

**Governed change since `eac011303`:** `crates/visionclaw-gpu/build.rs`. The
policy it used to inline now lives in `crates/visionclaw-gpu/src/ptx_policy.rs`,
which is **added to `verified_paths`** — it is where the governed behaviour
actually is, and leaving it out would let the whole policy change without
tripping the staleness gate.

**One source, compiled twice.** `build.rs:18` is `include!("src/ptx_policy.rs")`,
with the rationale at `:16-17`; the same file is a `pub mod` of the library. A
build script cannot depend on the crate it builds, which is why the 2026-09-04
closeout had to test a *separate* probe with no guarantee it still matched the
script. That gap is closed: the tests now exercise the exact code the build runs.

**Three claims in the Decision were re-checked against the code, and two were
wrong — corrected inline above:**

1. `NvccOutcome` (`ptx_policy.rs:59-70`) distinguishes `LaunchFailed` from
   `CompilerFailed`, but `needs_fallback()` at `:85-87` is
   `!matches!(self, NvccOutcome::Succeeded)` — **both** failure modes consult the
   fallback. The doc comment at `:55-57` states the reason explicitly: a missing
   toolchain is the case the fallback PTX was shipped for, so panicking there
   defeated its purpose. The old "panics before fallback" text is false at HEAD.
2. The output gate is `validate_ptx` (`:262-293`), which rejects an empty file
   (`:263`) **and** checks directives and required symbols, returning a typed
   `PtxDefect` (`:159`). The closeout's "a fake compiler writing `NOT PTX` passed
   the non-empty gate" case is closed — noted in the module header at `:22`.
3. The version rewrite itself is unchanged in substance and now parsed rather
   than spliced blind: `find_version_token` (`:179`), `parse_version_token`
   (`:198`) and `rewrite_ptx_version` (`:233`). This also closes the closeout's
   "invented two-digit minor exposes fixed-width splicing" finding, since the
   token is parsed into a `PtxVersion` instead of overwritten at a fixed width.

`PtxProvenance` (`:120-132`) additionally records which artefact was used
(`fallback-launch-failure` vs `fallback-compiler-failure`) and `content_tag`
(`:295`) hashes the bytes — the "record the selected runtime module and
original/rewritten hashes" half of the acceptance condition.

**Status stays `partial`, correctly.** Everything above is host-side build
policy. No CUDA compile, driver load or kernel execution ran, so
"exercise representative kernels on the intended driver" is still outstanding.

**Commands run:** `git diff --stat eac011303..HEAD --
crates/visionclaw-gpu/build.rs`; `grep -n 'version 9\.|ptx_policy|include!'
crates/visionclaw-gpu/build.rs`; `grep -n` over `ptx_policy.rs` for
`NvccOutcome|needs_fallback|validate_ptx|find_version_token|rewrite_ptx_version|
PtxProvenance|content_tag`; `awk` dump of `ptx_policy.rs:55-97`;
`cargo test -p visionclaw-gpu --lib ptx` → **44 passed, 0 failed** (27 filtered
out).

## Re-verification — 2026-09-21 at 997440cd0717d4c5f9341369571fc69fcf5a38d6

**Governed change since `b0bc275f6`:** `crates/visionclaw-gpu/build.rs` +2/-1 —
`src/cuda_sources/dynamic_grid.cu` was removed from the compiled list with a
comment recording why: it contains only `__host__` helpers and no launchable
kernel, so it must not be emitted or validated as a device PTX module.
`crates/visionclaw-gpu/src/ptx_policy.rs` is unchanged.

**Decision unaffected.** The decision is about what happens to each compiled
`.ptx`, not about which sources are compiled. At HEAD the `.version 9.0` splice,
the structural validation, the bundled-fallback set consulted on *both* nvcc
failure modes and the final panic when neither compilation nor fallback yields
PTX are all present and unchanged (`build.rs:161-240`). Dropping a kernel-free
translation unit removes a module that could never have been a tested kernel.
`verified_commit` moved to the CI-repair commit.
