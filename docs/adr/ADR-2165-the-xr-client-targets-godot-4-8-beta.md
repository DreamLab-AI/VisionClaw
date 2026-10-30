---
id: ADR-2165
title: The XR client targets Godot 4.8 (beta), with 4.6.1-stable as rollback until the headset re-test passes
date: 2026-10-30
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 00669b7a2b8bca15702b8173df1dab268144c3ec
verified_paths: [xr-client/visionclaw_xr_gdext.gdextension, xr-client/rust/Cargo.toml, xr-client/project.godot]
owner: jjohare
review_trigger: the Invariant 1 headset re-test on 4.8 (pass or fail); Godot 4.8-beta3, RC or stable; a godot-rust release with an api-4-8 feature; the Quest/Android XR export being unfrozen
repo: visionclaw
domain: XR-client
depends_on: [ADR-136, ADR-137]
relates: [ADR-2158, ADR-2159]
---

# ADR-2165 — The XR client targets Godot 4.8 (beta), with 4.6.1-stable as rollback until the headset re-test passes

## Context
- The client runs Godot 4.6.1-stable under the Compatibility renderer on SteamVR/Linux/NVIDIA 580 (`docs/XR-client.md` Invariant 1). `project.godot` still carries the 4.3-era `config/features` metadata, which is tied to the WP0 Quest renderer decision.
- ADR-2158's native spatial-volume arm needs Godot ≥ 4.8 (`OpenXRSpatialContainerExtension`, PRs #123124/#123736). The operator accepted a beta engine: "happy with updating to a beta… this is all cutting edge".
- Godot 4.8-beta2 was published 2026-10-29 (godot-builds tag `4.8-beta2`, engine commit `cafc17a192acf67903910a68aeb3c076c0907a4b`; the godot repo has no tag for it).
- 4.8 includes PR #116424 (multi-layer projection camera). It deprecates `XRInterface.get_transform_for_view`/`get_projection_for_view`, which breaks `XRInterfaceExtension` implementations. It also samples the XR camera pose earlier in the frame. That is a change to the multiview render path, so Invariant 1 re-opens.
- gdext: the crate pins `godot = "0.2"` (0.2.4, API 4.3). The latest crates.io release, 0.5.5, offers features only up to `api-4-7`.

## Decision
1. **Target.** The XR client's target engine is **Godot 4.8-beta2** (Linux x86_64 editor build, HP `~/godot48b2/Godot_v4.8-beta2_linux.x86_64`). Later 4.8 betas, RCs and the stable release replace it after the same gates.
2. **Rollback.** 4.6.1-stable stays installed and is the rollback (`~/.local/bin/godot` is not repointed). **Live headset sessions stay on 4.6.1 until the Invariant 1 re-test on 4.8 passes.** Activation is then `live`.
3. **gdext unchanged.** Keep `godot = "0.2"` and `compatibility_minimum = "4.3"`. GDExtension is forward compatible and the library loads under 4.8. Bump only when an API we need is missing or loading fails. No current release binds the 4.8 API, so 4.8-only classes (spatial containers) are reached from GDScript, as ADR-2158 already states.
4. **Engine churn is committed separately** from code fixes. `project.godot` `config/features` is not re-pinned by this ADR. The headless import leaves it byte-identical, and an editor re-save would also flip the renderer tag, which is the WP0 decision.
5. **No OpenXR Vendors plugin** is added for desktop OpenXR. The Quest/Android XR export, still frozen, will need it. Version 5.2 is the line that adds 4.8 spatial-container export.

## Consequences
- **Spatial containers become reachable from the code.** ADR-2158's `NativeMainVolume` can be written against real 4.8 API. Settings: `xr/openxr/extensions/spatial_container/enabled` (bool, default false), `…/bounds_mode` (int enum Bounded=0 / Immersive=1, default `"0"`), `…/bounds` (Vector3 metres, default 0.5³). API: the singleton `OpenXRSpatialContainerExtension` and `OpenXRSpatialContainerState.BoundsMode`.
- **Fallback is silent.** Without both `XR_EXT_spatial_container` and `XR_EXT_spatial_container_self_rendering`, `is_enabled()` is false and the session runs normally on the ordinary play space. A verbose-only log line is the only sign. No runtime on our hardware has the extension: not SteamVR build 25998080, and not Monado `main` at `aadd2a52` (headers only).
- **Beta risk.** Engine regressions may appear between betas. Each beta bump repeats the gates in Verification. Rollback is a `GODOT=` change, not a code revert.
- **Upgrades still owed:** GUT stays on its 4.6 line (9.6.1), which parses and runs cleanly on 4.8-beta2. The Quest renderer decision and `config/features` remain open (WP0).
- **Cost.** A second engine on HP, and a headset session to close the gate.

## Verification
Established at `verified_commit` (main, the tree synced to HP `~/mce-xr/godot48/`), 2026-10-30:
- **Install: PASS.** `Godot_v4.8-beta2_linux.x86_64.zip`'s SHA-512 matches the release `SHA512-SUMS.txt`. `--version` returns `4.8.beta2.official.cafc17a19`.
- **Headless load: PASS.** `--headless --editor --import --quit` exits 0, and `--headless --xr-mode off --quit` exits 0. Its error lines are the same as 4.6.1's (the expected `XRBoot: OpenXR initialise() returned false` under `--xr-mode off`), plus a GameMode-daemon warning. gdext logs `Initialize godot-rust (API v4.3.stable.official, runtime v4.8.beta2.official)`.
- **GUT: PASS.** On both 4.6.1 and 4.8-beta2: 33/33 scripts (`tests/gut_guard.sh` clean), 257/257 tests, 1936 asserts. The warning/error histogram is identical. Reports are in HP `~/mce-xr/godot48/xr-client/tests/report/{base461,b48}/`.
- **Rust: PASS.** `cargo test -p visionclaw-xr-gdext`: 544 passed, 0 failed. This is a control only, because the crate is unchanged.
- **API break check (static).** No `XRInterfaceExtension` subclass exists, and no `*_for_view` calls, in `xr-client/` GDScript or Rust. No code fix was needed.
- **Headset (Invariant 1): NOT RUN.** The VIVE was in use by a live 4.6.1 session (HP pid 7926), which the gate does not stop. This is the sole reason `implementation_status` is `partial` and `activation_status` is `staged`. The 4.6.1 baseline frame time from that session is 90 FPS / 11.11 mspf.
