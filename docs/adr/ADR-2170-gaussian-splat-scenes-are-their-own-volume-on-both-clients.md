---
id: ADR-2170
title: Gaussian splat scenes are their own volume, rendered by a per-backend library on the desktop and a CPU-sorted quad layer in the headset
date: 2026-11-01
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 858cf136fa89bf9fdc3475ac3b409b3a3783308f
verified_paths: [crates/visionclaw-splat, src/handlers/splat_handler.rs, client/src/features/splats, xr-client/rust/src/frame_budget.rs, xr-client/rust/src/splat_governor.rs, xr-client/rust/src/splat_sort.rs, xr-client/rust/src/splat_loader.rs, xr-client/materials/splat.gdshader, xr-client/scripts/splat_volume.gd]
owner: jjohare
review_trigger: The first splat frame on either client, any three.js bump past 0.183, a Spark release with WebGPURenderer support (sparkjsdev/spark#394), or the first headset session with `XR_SPLATS=1`.
repo: visionclaw
domain: XR-client
evidence_class: source+test
acceptance_boundary: Library source and registry metadata (three 0.187.0 tarball, Spark 2.4.0 and splat-transform 3.10.3 on npm) and the ISS asset's published metadata, read and hashed in the agentbox container on 2026-11-01. No splat was rendered on any GPU; nothing here is a frame-time, fill-rate, stereo or headset measurement. The acceptance numbers below are targets, not results. Implementation (2026-11-04) adds unit, integration and headless-GUT evidence in the agentbox container only (Godot 4.6.1 headless, Node/jsdom); still no GPU render.
lineage: Places splats under ADR-2158 (spatial volumes) and ADR-2166 (entered volume, portals); budgets them beside ADR-2167 (visual extras allowance) and PRD-008 (100k triangles, 50 draw calls); renderer rule unchanged from ADR-2032; content addressing per ADR-2023.
---

# ADR-2170 — Gaussian splat scenes are their own volume, rendered by a per-backend library on the desktop and a CPU-sorted quad layer in the headset

## Context

The owner asked for a splat renderer on both clients; the testbed is the ISS scene at `https://superspl.at/scene/cc026f6a` (Tamás Máté, CC BY 4.0; 9 999 999 Gaussians, SH degree 3, SOG v2, about 137.8 MB; derived from a NASA model and drsys.eu Earth textures, whose own terms were not checked).
The desktop picks `WebGPURenderer` when `navigator.gpu` exists and otherwise a **clean `WebGLRenderer`**; it deliberately discards WebGPURenderer's internal WebGL2 backend (`client/src/rendering/rendererFactory.ts:38-40,141-147,236`), on three 0.183.0 (`client/package.json:74,132`).
Spark 2.4.0 (2026-10-20, peer `three >=0.180`) is WebGL2-only and does not run under `WebGPURenderer`, even with `forceWebGL` (open issue sparkjsdev/spark#394). GaussianSplats3D 0.4.7 is unmaintained and points at Spark.
three.js r186 and later ship a `GaussianSplat` **addon** (`examples/jsm/objects/GaussianSplat.js`, imported from `three/addons`, not core) in 0.187.0. It is TSL and compute-based (a GPU counting sort), runs only under `WebGPURenderer` (its `forceWebGL` backend included, plain `WebGLRenderer` excluded), loads PLY, SPZ, SPLAT, KSPLAT and glTF `KHR_gaussian_splatting`, and has no SOG loader and no LOD.
So no single library covers both desktop paths.
The headset is `gl_compatibility` (`xr-client/project.godot:56`): no compute, no post-process. PRD-008 and ADR-2167 allow 95k triangles and 48 calls after reserve, and the graph already spends about 94.5k of the triangles. At 2 triangles a quad, 50k splats alone exceed the whole limit.

## Decision

1. **A splat scene is its own spatial volume, never attached to a node.** A splat asset renders as a `splat` volume under the ADR-2158 contract (bounds, fit rule 5, emulated everywhere), and in ADR-2166 can be **entered**, so that it becomes the environment while the graph volumes become portals. A graph node may *reference* a splat asset (a link that opens or enters its volume), but splats are never children of a node's transform or of `GraphRoot`: physics would move them, force a re-sort every frame and break the fit. By analogy with XR Invariant 8, splat roots sit under unit-scale roots. On the desktop the same rule holds: a splat scene is a scene-level layer (an environment or a framed volume), not per-node geometry.
2. **No interleaving with graph transparency.** Godot and three.js both sort transparent objects per object, not per fragment, so a splat cloud overlapping edge ribbons or hulls (`blend_mix`, `depth_draw_never`) would composite in the wrong order. Two rules follow, and splats write no depth in either case:
   - **Un-entered splat volume:** its box must not intersect a graph volume's box. In the headset it shares the graph's transparent `render_priority`, so the per-object distance sort orders whole volumes (a fixed lower priority would let a farther graph volume blend over a nearer splat volume).
   - **Entered splat volume (the environment):** it draws at a lower `render_priority` than every graph layer and portal, as a backdrop. Its fit is the ADR-2166 room-scale fit, so the portal ring sits at the cloud's edge. Splat content between the user and a portal is overdrawn by the portal; that artefact is accepted and named.
3. **Desktop: one `SplatLayer` interface, two backends, chosen by `isWebGPURenderer`.**
   - **WebGPU path:** the three `GaussianSplat` addon. This required bumping three from 0.183.0 to 0.187.0 or later, in both `package.json` pins; **done 2026-11-01** (`client/three-0187`, merged to main). The r3f 9.5.0 (`>=0.156`), drei 10.7.7 (`>=0.159`) and postprocessing 3.0.4 (`>=0.156`) peer ranges admit it, but the bump is its own change, tested on the gem materials, `GemPostProcessing` and the WebGL-fallback gate before any splat work lands.
   - **WebGL path:** Spark 2.4.x `SparkRenderer` on the existing clean `WebGLRenderer`. The fallback decision in `rendererFactory.ts` stands: re-enabling WebGPURenderer's WebGL2 backend to run the core addon there is rejected.
   - When Spark ships `WebGPURenderer` support (#394), a single Spark backend replaces both, by amendment.
4. **One served format: SPZ, produced offline.** Both desktop backends read `.spz`; the core addon cannot read SOG. The asset pipeline converts the source (SOG, PLY) with `@playcanvas/splat-transform` (3.10.3, CLI, `.spz` output) into LOD variants. Its `--decimate` / `--decimate-adaptive` steps need a WebGPU adapter (verified: they fail with "Unable to retrieve a WebGPU adapter" in a GPU-less container), so the pipeline runs on a GPU build host, **never the HP while the headset is in use**. The headset variant is SH degree 0 (`-H 0`): view-dependent colour is dropped in the headset. The desktop keeps SH up to 3.
5. **LOD targets for the ISS testbed.**

   | Variant | Splats | SH | Target use |
   |---|---|---|---|
   | `desktop-hi` | 3 000 000 | 3 | WebGPU path |
   | `desktop` | 1 500 000 | 3 | WebGL path (Spark) and WebGPU low tier |
   | `xr-3` | 100 000 | 0 | headset tier 3 |
   | `xr-2` | 50 000 | 0 | headset tier 2 |
   | `xr-1` | 25 000 | 0 | headset tier 1 |

   Spark's own LoD/paged format (RAD) is allowed on the WebGL path as an optimisation. It does not replace the SPZ variants, which both backends must load.
6. **Headset renderer (Compatibility-safe).** The pattern is the one used by `materials/edge_ribbon.gdshader` and the motes:
   - **Geometry and blending:** one MultiMesh of camera-facing quads, `blend_mix, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled`, at the `render_priority` set by Decision 2. The vertex shader projects the 3D covariance to a 2D ellipse, and the fragment shader evaluates the Gaussian falloff with premultiplied alpha.
   - **Data and sorting:** splat data is static in an RGBA32F data texture, uploaded once. A sort uploads only a sorted index texture (4 bytes per splat, 400 KB at 100k), and the vertex shader reads both by `INSTANCE_ID` with `texelFetch`. The 100k-instance transform buffer (about 8 MB) is never rewritten per sort.
   - **Sorting:** the sort is a CPU radix sort on 16-bit view depth in the Rust gdext, on a worker thread.
   - **Overdraw and comfort guards:** each splat's screen-space radius is capped (at 64 px a quad), and splats within 0.3 m of the near plane fade out, so a head inside the cloud gets neither an overdraw spike nor a wall of noise.
   - **Shader rules:** no `SCREEN_TEXTURE`, no depth-texture read and no `SubViewport` (the existing `test_eye_candy.gd` grep rule extends to the splat shader). No compute, no post-process.
7. **Headset budget: a separate splat account, which needs the owner's decision.** Splat quads cannot live inside `TRI_LIMIT`: the only count that fits next to the graph is a few hundred splats. The decision proposed here is that `frame_budget.rs` gains a **splat account** outside `TRI_LIMIT`, so that splat triangles are reported but not charged against the graph. **This amends PRD-008 and ADR-2167, and is the owner's call.** The account still has these limits:
   - **Draw calls:** splat draw calls do count against the 48 (one per splat volume, at most 2 visible).
   - **Tiers:** tiers 0 to 3 are 0, 25k, 50k and 100k splats (`xr-1` to `xr-3`).
   - **Governor:** the tier is stepped by the ADR-2167 governor's thresholds: drop at once above 8 ms GPU p95, climb below 5.5 ms after 8 s, with the doubling hold-off. Because ADR-2167 notes that the GPU timer is unverified on the XR viewport, frame-time p95 is the fallback, with the splat ceiling at tier 2 and a 20 s hold.
   - **Entering a splat volume:** this turns the graph volumes into portals (ADR-2166), which frees graph near detail. The splat tier is then allowed to climb, but the graph allocation is unchanged in any other state.
   - **If the owner refuses the separate account,** headset splats stay off, and the desktop proceeds alone.
8. **Sort cadence in stereo: one sort for both eyes.**
   - **Shared order:** the sort key is depth from the head (mid-eye) pose, and both eyes draw the same order. At the IPD (about 63 mm), the order error is negligible beyond about 0.5 m, which the near fade hides.
   - **Trigger:** a re-sort is triggered when the head moves 5 cm or turns 2° since the last sort, at no more than 30 Hz. Between sorts the previous order is drawn. A sort is never on the render thread, and a late sort is dropped, never queued.
   - **Desktop:** desktop sorting is the library's own (Spark's worker, or the core addon's GPU counting sort).
9. **Comfort and reduced motion (ADR-2107).**
   - **Placement and motion:** a splat scene is world-locked, static, and never animates its own transform: no auto-rotation or drift in either client.
   - **Reduced motion (the default):** entering or leaving a splat volume is a dim-and-snap, and a tier change swaps at a dim. With reduced motion off, these are a 0.5 s ease.
   - **Low cost:** `XR_VISUAL_QUALITY=low` removes splats entirely and withdraws their request in the same frame (the ADR-2167 low-cost rule).
   - **Scale:** the scene's scale follows the volume fit rule (ADR-2158 decision 5) when un-entered, and the ADR-2166 room-scale fit when entered. A 1:1 metric view (the ISS asset spans about 128 units, consistent with metres for a 109 m station) is not part of this record: it would put the portal ring inside the cloud and needs its own amendment.
10. **Serving, caching and the licence rule.**
    - **Not committed:** splat assets are never committed to any repository. A **splat manifest** entry carries the source URL, the SHA-256 of each source file, author, title, licence (an SPDX id and URL), the attribution text, and the derived variants with their own SHA-256 values.
    - **Served:** the backend fetches, verifies and stores sources and variants in a content-addressed cache (`sha256-12`, ADR-2023) and serves them read-only. Clients cache by hash: the desktop in Cache Storage, the headset under `user://splat-cache/<sha256-12>`. A hash mismatch is a refusal, not a retry.
    - **Licence rule:** only assets whose licence permits redistribution **and adaptation** (decimation and SH stripping are adaptations) enter the cache: CC0, CC BY and CC BY-SA, or an explicit grant. `-ND` licences, assets with no stated licence, and `-NC` licences are refused outside a private dev cache. Every view of a cached asset shows its attribution, the licence link and "modified (decimated)": the desktop in the scene info panel, the headset on the HUD credits line. The ISS asset passes this rule under CC BY 4.0. Its upstream inputs (the NASA ISS model and the drsys.eu Earth textures) are recorded in the manifest as unchecked.

## Consequences

- **Cost:** neither surface is a one-library drop-in. The desktop needs:
  - a three bump (four minor versions);
  - two backends behind one interface;
  - an offline conversion step.

  The headset needs a renderer of its own and an owner decision on the budget.
- **Shippable alone:** the WebGL-path Spark backend is the cheapest slice and can ship first. WebGPU users then see no splats until the bump lands, unless the user forces WebGL (`setForceWebGLOverride`).
- **Two code paths:** the desktop carries two splat code paths until Spark #394 closes.
- **Headset look:** splats in the headset are SH0 (no view-dependent colour) and at most 100k splats. The ISS scene at 1 % of its source density will look visibly sparser than on SuperSplat.
- **Follow-on work, in order:**
  1. the asset pipeline and manifest;
  2. the three bump;
  3. the Spark backend;
  4. the core-addon backend;
  5. the owner's budget decision;
  6. the headset layer;
  7. the HP measurements, with the lead's approval and never while the owner is in the headset.

## Acceptance (targets the implementation must meet; none measured yet)

**Desktop, ISS testbed, 1920×1080, the graph's 13 164-node benchmark shown in its own volume beside the splat volume:**
- **Frame time:**
  - WebGPU path, `desktop-hi` (3M): p95 frame time ≤ 16.7 ms.
  - WebGL path, `desktop` (1.5M): p95 frame time ≤ 16.7 ms.
  - Both are measured on a named desktop GPU recorded with the result.
- **Load and memory:**
  - From a warm cache, the first splat frame arrives ≤ 3 s after the volume opens.
  - JS heap plus GPU buffers stay ≤ 1.5 GB for `desktop-hi`.
- **Behaviour:**
  - A forced WebGL session renders the same asset through Spark.
  - No backend throws under the other renderer.
  - Removing the splat volume returns the graph's draw calls and frame time to within 5 % of the no-splat baseline.

**Headset, VIVE Pro on the HP, Compatibility stereo, ISS `xr-3` entered:**
- **Frame rate:** 90 Hz held for 60 s, with no more than 1 dropped frame and frame-time p99 ≤ 11.1 ms.
- **GPU time:** GPU p95 ≤ 8 ms, so the governor settles at tier 3. If the GPU timer reads zero, frame-time p95 ≤ 11.1 ms at tier 2.
- **Sort:** the CPU sort of 100k is ≤ 4 ms p95 on the worker, and is never on the render thread.
- **Budget:** total draw calls ≤ 48, and graph triangles inside `TRI_LIMIT`, unchanged from the no-splat allocation outside an entered splat volume.
- **Stereo and comfort:**
  - Both eyes render the splat (Invariant 1 re-observed).
  - No visible order popping is reported in a 60 s head-turn sweep.
  - `XR_VISUAL_QUALITY=low` removes splats within one frame.
- **Tests:** the headless tests are the `frame_budget` splat-account tests, the shader grep rule, the manifest hash and licence refusal tests, and the sort-cadence tests (sorts per second under scripted head motion).

## Implementation (2026-11-04)

The owner accepted Decision 7 on 2026-11-04 and directed the implementation; the record is accepted. `implementation_status: partial` because the LOD variants are not produced (decimation needs a WebGPU adapter, which the container lacks) and no acceptance target is measured. `activation_status: staged`: the headset layer is off unless `XR_SPLATS=1` or the `xr.splat_scene` pref is set, and the desktop shows nothing until a variant is admitted.

- **Contract (`crates/visionclaw-splat`, MIT).** Manifest (`data/splats/manifest.json`, committed; assets never), the decision-10 licence rule, `sha256-12` addressing, the content-addressed cache that re-hashes every variant at open, and an SPZ v3 codec. `splat-admit` (feature `cli`) runs fetch, build (via `splat-transform`), admit and verify. **Served format is SPZ v3 (gzip), not splat-transform's default v4 (zstd)**: v3 is what the three addon, Spark and the pure-Rust headset decoder all read; v4 is refused at admission.
- **Backend.** `GET /api/splats` (listing) and `GET /api/splats/blob/{sha256-12}` (immutable). `SPLAT_MANIFEST` and `SPLAT_CACHE_DIR` override `data/splats/manifest.json` and `data/splat-cache`.
- **Desktop (`client/src/features/splats`).** One `SplatLayer`, backend chosen at mount from the live renderer and loaded by dynamic `import()`: three `GaussianSplat` + `SPZLoader` on WebGPU, Spark 2.4.0 `SplatMesh` on WebGL. SHA-256 is verified on fetch and on every cache hit; a mismatch is a refusal. The desktop has no volume system, so framed and entered placement live in `fit.ts`. The framed volume sits outside the sphere enclosing both graphs and the memory cloud (`sceneFitBounds` with `cloudFitInput`). That separation is from source; no render has checked it.
- **Headset.** The splat account (`allocate_with_splats`): draw calls taken before extras are priced, triangles reported in `splat_tris` and outside `TRI_LIMIT` and `over_budget`; un-entered volumes are capped at tier 1. The governor floor is tier 1 (tier 0 draws nothing, so nothing could be measured to climb); only low cost reaches 0. Sort is a two-pass radix on a worker. The frame-budget pass subtracts applied splat primitives from `other_tris`, the same way extras are subtracted.

Evidence (container only):
- `cargo test -p visionclaw-splat`: 30 passed. The real-file test decodes splat-transform 3.10.3 v3 output (221 063 and 1 357 052 splats).
- Root `cargo test --lib splat_handler`: 2 passed.
- `xr-client/rust` `cargo test --workspace`: 699 passed, 0 failed, 3 ignored.
- GUT, Godot 4.6.1 headless: 598 tests, 595 passing, 3 pending pixel tests that need GL.
- Desktop vitest: 1 593 passed, 47 of them splat tests; `tsc --noEmit` clean.
- Sort, 100k random splats on container CPU (not HP, not Quest): p95 1.64 ms.

Unverified:
- any GPU frame on either client;
- splat orientation on both desktop backends;
- desktop Fit view, which does not include the framed splat volume, so the volume may open off screen;
- `INSTANCE_ID` plus `texelFetch` under Compatibility multiview;
- the 4096-texel data texture against the GLES 3.0 2048 guarantee (it matters only for a Quest export);
- the extras and splat governors stepping together on one timer;
- every target in Acceptance.

## Amendment (2026-11-04): HUD toggle and visible state

In the first headset session the scene loaded (the cached `xr-1` matched its served SHA-256) but the owner found no option for it: the Session page's volume row lists only the three graph volumes (`hud.gd` `VOLUME_IDS`), the only control was an Enter button hidden on the Layout page until a variant landed, and nothing called `XrPrefs.set_splat_scene`, so `XR_SPLATS` was the only switch.

- **Toggle.** A `Splat` button sits in the Session volume row beside KG, Onto and Code (`splat_toggle`). The first press builds the volume live and stores `xr.splat_scene = true`; later presses show or hide it like a graph volume (hidden withdraws the splat demand; hiding while entered leaves first; showing a failed scene retries). The stored choice outranks `XR_SPLATS`. The splat volume stays out of `VOLUME_ORDER`, focus and the connection gate: it has no server.
- **Visible state.** Its face reads `Splat off`, `Splat …` (loading), `Splat` or `Splat ×` (failed). The Layout page's Splat Scene group shows whenever the scene is on; its button is disabled with "Loading splat scene…" or the failure reason until a variant has loaded, then Enter/Leave as before.
- **Evidence.** Headless GUT, Godot 4.6.1, worktree build of the gdext library: 602 tests, 599 passing, 3 pending pixel tests, 0 failing; four new tests cover the toggle faces, the Layout group states, the Session row's width with the longest faces, and build/hide/reshow/remember through `GraphScene`. Not yet seen in the headset.

## Verification

Evidence for the Context (2026-11-01, before implementation):
- **three:** the `three-0.187.0` npm tarball was read, `GaussianSplat.js:79-86` (the WebGPURenderer-only note) and the loader list. Registry dates: 0.183.0 2026-02-18, 0.186.0 2026-09-08, 0.187.0 2026-10-30.
- **Spark:** `registry.npmjs.org/@sparkjsdev/spark` latest is 2.4.0 (2026-10-20), peer `three >=0.180.0`, and the GitHub API shows #394 open.
- **ISS asset:**
  - `https://d28zzqy0iyovbz.cloudfront.net/cc026f6a/v1/meta.json` (`count` 9999999, `shN.bands` 3, generator splat-transform 1.10.2) and its seven WebP files total 137 823 626 bytes, against the page's 137 824 185. SHA-256 values are in `/home/devuser/workspace/.tmp/splat-iss/SHA256SUMS`.
  - `splat-transform --stats` reads 9 999 999 Gaussians, SH 3, fill ratio 21.9.
  - A CPU opacity filter (not decimation) gave 221 063 splats at SH 0 as 3.69 MB SPZ, and 1 357 052 splats at SH 3 as 28.3 MB SPZ, which is a size guide only.
- **Repository:** the renderer-factory lines were read at `ecd78b6cb`.

Re-verified 2026-11-05 at `858cf136f` for ADR-2178: Governed paths changed only by ADR-2178: the manifest and listing gain an optional `stage`, the loader plan carries it, and `SplatVolume.stage()` exposes it. Decision 2 is amended for the merged space only (a graph box inside the splat box; the backdrop overdraw is named there). Crate 33 pass, gdext splat_loader 26 pass, GUT 609/609 on the HP rig.
