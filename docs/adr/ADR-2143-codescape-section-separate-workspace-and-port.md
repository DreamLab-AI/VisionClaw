---
id: ADR-2143
title: Codescape is a separate section with its own Cargo workspace, its own port and a tested isolation boundary
date: 2026-10-17
decision_status: accepted
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 458b0ded5da35b968ebd7ad056eba8bc06da38a5
verified_paths: [crates/codescape, docs/PROTOCOL-registry.md]
owner: jjohare
review_trigger: a graphicle rev bump; any change to the V3 record, the V5 envelope or 0x23; Codescape gaining a compose service, a published port or a new egress target; a proposal to merge the workspace into the root
repo: visionclaw
domain: BASELINE-architecture
depends_on: [ADR-2018, ADR-2019, ADR-2024, ADR-2141, ADR-2142]
amends: []
relates: [ADR-2135]
---

# ADR-2143: Codescape is a separate section with its own Cargo workspace, its own port and a tested isolation boundary

## Context
- Intent: humans and agents work together by voice over a near-real-time 3D view of a codebase, seeing who is touching what as it happens. Codescape is that section.
- The code-graph engine is graphicle (Apache-2.0, `graphicle@003f9360:crates/graphicle-graph/Cargo.toml:11`). It is consumed as a pinned git dependency at rev `003f93605d895ed69f0cf06690dd296474385b99` (`crates/codescape/Cargo.toml:40`) and is not re-ported.
- graphicle turns on serde_json `arbitrary_precision` and `preserve_order` (`graphicle@003f9360:crates/graphicle-graph/Cargo.toml:54`). Cargo unifies features across a workspace, so in the root workspace this would change `serde_json::Number` for every VisionClaw crate. VisionClaw has 13 `serde(untagged|flatten)` sites outside `crates/codescape` (grep, this revision). graphicle also pins `smallvec =1.16.1` and `unicode-normalization =0.1.22` (`crates/codescape/Cargo.toml:62-63`).
- Two clients already decode the frozen 52-byte V3 record and the V5 envelope (ADR-2018). The XR client connects to `/wss` (`xr-client/scripts/graph_scene.gd:72`) and takes its topology from an `initialGraphLoad` text frame (`xr-client/rust/src/binary_protocol.rs:163,239`). Wire ids are 26 bits (ADR-2024, `crates/codescape/codescape-model/src/ids.rs:16`), so hashing 100k+ symbol keys into that space would collide.
- `visionclaw_container` mounts `/vault`, the knowledge corpus (`docker-compose.unified.yml:201`), and only `agentbox/schema` of agentbox (`:182`). Code that runs there can read the corpus.

## Decision
1. **Separate workspace.** Codescape is its own Cargo workspace at `crates/codescape/`, with its own `Cargo.lock` and its own target dir (`crates/codescape/Cargo.toml:1-20`). It is never a member of the root workspace.
2. **Five crates, cut by I/O.**
   - `codescape-model` (pure types, ids, LOD, wire mapping)
   - `codescape-ingest` (the only crate that uses graphicle)
   - `codescape-layout` (CPU Barnes-Hut oracle; `cuda` feature for the GPU)
   - `codescape-collab` (pure actors, heat, floor, grammar)
   - `codescape-server` (actix: every network adapter)
   
   A sixth crate needs a Codescape wire format, which item 4 forbids.
3. **Ports.** `:4100` serves clients. `:4101` is the hook-ingest listener: bearer token, never published, reachable only on `visionclaw_network`.
4. **No new wire format.** Codescape produces the frozen V3 record inside V5 envelopes on `/ws/positions`. From P4 it also emulates `/wss`, so the XR client runs unchanged. It honours `0x34` acks for pacing (ADR-2141) and sends agent activity as `0x23` with the existing `AgentActionType` values (ADR-2019).
   - Code nodes carry no flag bits.
   - Actors carry `0x8000_0000`.
   - Bit 29 stays free.
5. **Interned ids, never hashed.** A persisted interner allocates ids monotonically and never reuses one within an epoch. A full re-extract bumps the epoch and resends the topology.
6. **Field mapping.**

   | V3 field | Code-graph meaning |
   |---|---|
   | `community_id` | Leiden community, stable-matched |
   | `cluster_id` | crate / top-level directory |
   | `centrality` | importance 0..1 |
   | `anomaly` | activity heat 0..1 |
   | `sssp_distance`/`sssp_parent` | BFS hops and parent from the focus |

   The mapping lives in `codescape-model/src/wire.rs:32-50`.
7. **GPU.** The GPU path uses visionclaw-gpu's build-compiled PTX through a thin cust 0.3.2 driver. It asserts `c_params` = 212 bytes and the kernel arities, and **refuses fallback-provenance PTX** (amended 2026-10-17, see Amendment A1) (finding F, `docs/evidence/codescape/P0/finding-F-stale-fallback-ptx.md`). CPU Barnes-Hut is the fallback and the conformance oracle (ADR-2061 pattern), with the threshold set in Amendment A1.
8. **Isolation from the knowledge-work system** is enforced as four tested layers:
   - **build graph:** no ontology, memory-cloud, vault, Oxigraph, Postgres, RuVector, Solid or VisionClaw-server crate
   - **egress:** one allowlist covering nemotron-asr, pocket-tts, the Loom door and, from P6, `/v1/voice-intent`
   - **filesystem:** its own compose service, with no `/vault`, `data/markdown` or `visionclaw-data`
   - **client:** `client/src/codescape/**` imports only three, R3F, react, its own modules and `client/src/types/binaryProtocol.ts`
   
   The process never receives `RUVECTOR_PG_CONNINFO`.
9. **Voice and dispatch.** Voice is push-to-talk with a single floor: nemotron-asr for recognition, pocket-tts for speech, and the Loom door for out-of-grammar parsing. Every action is a typed `Command` before it runs, and LLM output that fails the schema is spoken, never executed. Agent dispatch happens only as a browser-signed kind-31402 relayed to `/v1/voice-intent`. Codescape holds no signing key.

## Invariants

Citations refreshed to the working tree on 2026-10-17 (A2); the audit column is the A2 cross-vendor verdict.

| # | Invariant | Enforced by | Audit (A2) |
|---|---|---|---|
| I1 | No forbidden crate in the resolved Codescape build graph (default features; the `cuda` graph is covered from P2) | `codescape-server/tests/isolation.rs:12-33,99-112`; negative control `:114-154` | ACCEPT-WITH-GAP (A2 gap 1) |
| I2 | Records are byte-identical to the frozen fixtures: 52 B, `cluster@36`, `anomaly@40`, `community@44`, `centrality@48`, V5 = 1+8+52n | `codescape-model/src/wire.rs:141-171`; fixtures via `#[path]` `:18-19` | ACCEPT-WITH-GAP |
| I3 | Actors carry `AGENT_FLAG`; code records carry none | `wire.rs:29,40,53-57`; test `:173-178` | ACCEPT-WITH-GAP (A2 gap 2) |
| I4 | `0x34` ack bytes equal the shared fixture | `wire.rs:180-191` | ACCEPT-WITH-GAP (A2 gap 2) |
| I5 | Ids are monotonic and stable, survive a byte and a serde round trip, exhaustion errors instead of wrapping, and a hostile count is refused before allocation | `codescape-model/src/ids.rs:38,56-71,182-184`; tests `:221,237,249,274,287,304,315` | ACCEPT-WITH-GAP (fixed in A2) |
| I6 | The Codescape lock never merges with the root lock | `crates/codescape/Cargo.toml:12-20` (own `[workspace]`); root `Cargo.toml:2-18` lists members explicitly and excludes it | ACCEPT-WITH-GAP |
| I7 | PTX is refused unless `c_params` is 212 B, the launched kernels' arities match and the build manifest says `provenance=compiled` with a content tag matching the file | `codescape-layout/src/abi.rs:31,37-41,100,184-206`; tag check `cuda.rs:459-472` | ACCEPT-WITH-GAP (A2 gap 3) |
| I7a | GPU layout conforms to the CPU oracle at the A1 thresholds | **Not an invariant (A2.1).** The opt-in test `codescape-layout/src/cuda.rs:1077-1078` `gpu_conforms_to_cpu_oracle` (env gate `:985,987`) is a required P2 gate step | REJECT |
| I8 | `RUVECTOR_PG_CONNINFO` set → the server exits 2 at boot | `codescape-server/src/config.rs:15,212`, `main.rs:47,52`; test spawns the binary `tests/isolation.rs:156-187` | ACCEPT-WITH-GAP |
| I8a | Egress allowlist; no `/vault` mount | Not enforced (A2.2). Compose non-publication is not an egress boundary | REJECT |
| I9 | `client/src/codescape/**` reaches no file outside itself except `client/src/types/binaryProtocol.ts`, and only allowlisted packages | `client/src/codescape/src/__tests__/isolation.test.ts:18-21` (roots, allowlists), `:73-112` (graph walk), `:114-117` (the shared file is used only from tests) | enforced since A2 |

## Consequences
- **Gained.** Both existing clients render Codescape without a protocol change. Codescape can bump graphicle, tree-sitter or tokio without touching the lock VisionClaw ships from. The model tests run with no GPU, no network and no graphicle compile (`cargo tree -p codescape-model` shows no graphicle). Collab and the CPU layout are built to the same rule (`cargo tree -p codescape-collab` also shows none).
- **Costs.** A second lock, a second target dir with its own cold compile, and a compose service to maintain. graphicle's non-optional neo4rs and redis (`graphicle@003f9360:crates/graphicle-graph/Cargo.toml:134-135`) compile in as dead code. This is a supply-chain cost (ADR-2137) rather than a breach, and it is the reason the build-graph ban targets estate crates and not those two.
- **Forbidden.** Codescape must not:
  - serve any knowledge route; `/api/memory-cloud/*`, `/api/analytics/*`, `/api/graph/fold` and `/api/canary/*` return 404
  - claim a tag byte or a flag bit
  - hash ids
  - adopt the ADR-2135 agents-at-centroid layout; agents sit at the node they are touching
- **Revisit if:**
  - a supply-chain audit requires graphicle's dead dependencies gone (that triggers a fork with them made optional)
  - XR measurements miss the headset budget (`xr-client/rust/src/frame_budget.rs:43`, 100k triangles) even at file tier
  - VisionClaw changes the V3 record

## Alternatives rejected
- **Root workspace, permanently inside `visionclaw_container`.** `arbitrary_precision` would unify into every root build. That container mounts `/vault`, so isolation would be a convention rather than a property. Every Codescape bump would also churn the shipped lock.
- **A new wire format.** It would fork three decoders, which ADR-2018 and ADR-2019 exist to prevent, in exchange for desktop-only richness that JSON on `/ws/events` already carries.
- **Force-directed edge bundling.** It costs O(E²·iterations) and is unstable under live edits. Tiered aggregated edges give the decluttering without that cost.
- **Stress majorisation.** It needs APSP. The landmark-APSP host path was removed deliberately and is refused under NFR-7 (`crates/visionclaw-gpu/src/cuda_sources/gpu_landmark_apsp.cu:20-25`); it must not come back. Edge weights, cluster cohesion and a hierarchical seed give the structure instead.

## Phase plan
Evidence lives under `docs/evidence/codescape/<phase>/`.

| Phase | Ships | Evidence that must exist |
|---|---|---|
| P0 Spine | workspace, lock, model types, isolation test, this record | `cargo test -p codescape-model` green; the isolation test passes and catches an injected dependency; no graphicle in the model tree |
| P1 Pixels | ingest, CPU layout, `:4100` REST, `/ws/positions`, `/ws/events`, desktop bundle, 404 list, RuVector boot refusal | snapshot counts; a screenshot; p50/p95 frame time; zero interner collisions; id stability across a restart |
| P2 GPU + live + own service | cust engine, PTX ABI and provenance tests, `codescape` compose service, live patching, partial V5 | the PTX test fails on the stale fallback; GPU-vs-CPU k-NN overlap at the A1 thresholds, shown by the committed output of `CODESCAPE_GPU_TESTS=1 cargo test -p codescape-layout --features cuda -- --ignored gpu_conforms_to_cpu_oracle` (required step, A2.1); an egress test (A2.2); `docker inspect` shows no `/vault`; edit-to-visible ≤ 1 s |
| P3 Presence + scale | agentbox activity hook (its own agentbox ADR), `:4101` ingest, halos, LOD L0-L2, picking, timeline | a live session lights nodes; `:4101` returns 401 without a token and is unreachable from the LAN; 100k-node frame trace |
| P4 XR | `/wss` emulation, `initialGraphLoad` at file tier, `0x23`, ack pacing | xr-client parser tests pass on captured fixtures; a Godot capture; XR survives the 404s |
| P5 Voice | `/ws/voice`, floor, grammar, Loom fallback, TTS, captions | ≥ 60 grammar goldens; WAV → Command end to end; floor contention; an invalid LLM output is never executed |
| P6 Publish + dispatch | TLS and NIP-98 on every route, pubkey allowlist, published `:4100`, 31402 relay | unsigned request → 401; `crossOriginIsolated` over LAN HTTPS; a dispatched aoe session flows back into the graph |

## Amendment A1 (2026-10-17): layout conformance threshold and ABI guard
Amended before first commit, on measured P1/P2 evidence (`docs/evidence/codescape/P1/README.md`, reported by the layout and server workers).
1. **Conformance threshold.**
   - **Files:** k-NN overlap ≥ 0.9.
   - **Symbols:** overlap ≥ (CPU self-baseline) − 0.1, where the self-baseline is the CPU engine run against itself with a 10% smaller `dt`.
   
   The original single threshold of 0.9 at symbol level is not a property any engine can satisfy. On a 2k-symbol graph, CPU against CPU scores only 0.749 on symbols and 0.982 on files, because a different integrator reshuffles symbols inside a file without moving the file. Measured GPU against CPU: 0.752 on symbols and 0.975 on files. Enforced by I7a.
2. **ABI guard.** PTX is refused unless all three hold:
   - `c_params` is 212 bytes
   - the five kernel arities match: `force_pass_kernel` 30, `integrate_pass_kernel` 24, `build_grid_kernel` 9, `compute_cell_bounds_kernel` 5, `cluster_cohesion_kernel` 13
   - the build manifest records `provenance=compiled` with a matching tag
   
   Enforced by I7.
3. **Boot refusal test.** The `RUVECTOR_PG_CONNINFO` check spawns the server binary and expects exit 2. It no longer asserts on the test runner's own environment, which agentbox sets everywhere. Enforced by I8.
4. **Open items against the scale targets** (design review §10), from the reported P1 numbers:
   - **GPU step ≤ 5 ms at 100k: missed.** Measured 7.8 ms, of which the kernels take 1.7 ms.
   - **Cold settle ≤ 10 s at 100k: missed.** Measured 13.7 s on the GPU over 1475 steps; the CPU takes 56.6 s.
   
   The identified cause of both is host-side work per step, not the kernels. They stay open until that is removed and re-measured.

## Amendment A2 (2026-10-17): cross-vendor evidence audit
The audit was run by Codex gpt-6-astra and returned at 2026-10-17T20:30Z, anti-fox verified. The verdicts are recorded per invariant in the table above. The full verdicts and the raw gate output are in `docs/evidence/codescape/P1/README.md` §Cross-vendor audit and `docs/evidence/codescape/P1/gate-raw.txt`.
1. **I7a is not an invariant.** An opt-in `#[ignore]` test proves nothing unless someone runs it, and the default `cargo test --workspace` in `gate-raw.txt` does not build the `cuda` feature. The P2 gate therefore requires this command, with its output committed:
   ```
   CODESCAPE_GPU_TESTS=1 cargo test -p codescape-layout --features cuda -- --ignored gpu_conforms_to_cpu_oracle
   ```
2. **I8a is rejected because nothing enforces it.** Compose non-publication is not an egress boundary. A container that is not published can still open outbound connections. I8a stays open until P2 ships an egress test.
3. **I9 is now enforced.** The vitest suite `client/src/codescape/src/__tests__/isolation.test.ts` walks the import graph from every source file under `client/src/codescape` and fails on two things:
   - any import that resolves outside that directory, except `client/src/types/binaryProtocol.ts` (`:20,96-97`)
   - any package that is not on `ALLOWED_PACKAGES` (`:21,106-108`)

   The allowlist is wider than decision 8 states. It also admits `react-dom/client`, `@react-three/drei`, `three/examples/jsm/*`, `vitest`, `vite`, `@vitejs/plugin-react` and `node:*`. The gate run passed 3 of 3 tests (`gate-raw.txt`, vitest section).
4. **A1.2 narrowed (correction).** The code checks three kernel arities, not five: `force_pass_kernel` 30, `cluster_cohesion_kernel` 13 and `integrate_pass_kernel` 24 (`codescape-layout/src/abi.rs:33-41`). `build_grid_kernel` and `compute_cell_bounds_kernel` are no longer launched because the spatial grid is built on the host (`:35-36`). I7 is reworded to "the launched kernels".
5. **Open gaps, recorded verbatim from the audit:**
   1. I1 scans package names only; a vendored or renamed implementation passes.
   2. I3/I4 are helper-level tests, a second encoding path could bypass.
   3. I7 accepts a hand-authored manifest saying provenance=compiled.
   4. OpenAccess on :4100 is honestly reported as "auth":"open" but publishing 4100 to the LAN exposes all open capabilities until P6 NIP-98.
   5. Bearer scheme match is case-sensitive for "Bearer"/"bearer" only (RFC says case-insensitive) and trims whitespace.
   6. RateLimiter is one shared bucket per instance, no constructor validation.

   Where each gap sits in the code:
   - gap 1: `isolation.rs:79-97`
   - gap 3: the tag at `cuda.rs:462` is a content hash any author can compute
   - gap 4: `auth.rs:120-132`, `routes/mod.rs:67`
   - gap 5: `auth.rs:78-82`
   - gap 6: `auth.rs:147-161`

   **Gap 4 mitigation.** `docker-compose.unified.yml:477-482` binds `:4100` to `127.0.0.1` by default. Setting `CODESCAPE_PUBLISH_ADDR=0.0.0.0` exposes it on the LAN, and that must not be done before P6.
6. **Fixed in response to the audit.** `crates/codescape/` is untracked, so these fixes have no diff to cite. Each is cited at its working-tree lines:
   - `IdInterner::from_bytes` refuses a count larger than the remaining bytes can hold, before it reserves memory (`ids.rs:178-186`). Test: `:273-284`.
   - `Deserialize` rebuilds the index through `#[serde(from = "RawInterner")]` (`ids.rs:38,47-71`). Test: `:286-301`.
   - The isolation negative control now fails when its fixture is missing, where it used to skip (`isolation.rs:122-128`).
   - The offline-to-online `cargo metadata` retry now prints the offline diagnostic (`isolation.rs:53-59`).

## Verification
- **Revision.** Verified at `458b0ded5da35b968ebd7ad056eba8bc06da38a5` against the uncommitted working tree of branch `feat/codescape`. `crates/codescape/` is untracked at that commit.
- **Results.** Commands and outputs are in `docs/evidence/codescape/P0/README.md`:
  - `cargo test -p codescape-model`: 27 passed
  - isolation test: 2 passed, 1 failed; an injected `visionclaw-ontology` is flagged (`forbidden: [horned-owl, oxigraph, visionclaw-ontology, whelk]`). The failure was environmental (the test runner itself inherited `RUVECTOR_PG_CONNINFO`) and is replaced by the binary-spawning test of A1.3.
  - `cargo metadata --locked`: 515 packages, none forbidden
- **P1 gate.** Raw output is committed in `docs/evidence/codescape/P1/gate-raw.txt`, with HEAD, rustc, the graphicle rev and the `Cargo.lock` sha in its header, and an exit status for each command:
  - Rust tests: 197 passed, 0 failed, 6 ignored, `exit=0`
  - clippy `-D warnings`: `exit=0`
  - fmt check: `exit=0`
  - rustdoc `-Dwarnings`: `exit=0`
  - vitest: 60 passed, `exit=0`
- **Implementation status is `partial`.** P0 is done. The P1 performance and conformance numbers are still reported by workers, not re-run (`docs/evidence/codescape/P1/README.md`). P2-P6 are not done.
