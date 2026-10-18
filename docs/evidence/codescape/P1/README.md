# Codescape P1 evidence (ADR-2143, Amendment A1)

> **Two kinds of evidence in this file.**
> - **Gate counts: raw output committed.** See [`gate-raw.txt`](gate-raw.txt) (2026-10-17T20:28:18Z) and §P1 gate below.
> - **Performance, conformance and live-run numbers: reported by worker, not re-run by the ADR worker.** The layout and server workers produced them, and the team lead relayed them at **2026-10-17T18:49Z**. No raw output for them is committed yet.

**Host conditions (reported):** loaded host, load average 20-40. The GPU is an idle RTX 6000 Ada. The CPU engine uses 56 threads.

## Layout step time (per step)

| Nodes | CPU (56 threads) | GPU total | GPU kernels only |
|---|---|---|---|
| 10k | 4.6 ms | 2.5 ms | 0.9 ms |
| 50k | 15.5 ms | 5.5 ms | 1.5 ms |
| 100k | 27.7 ms | 7.8 ms | 1.7 ms |

On the GPU, the time not spent in kernels is host-side work per step.

## Settle and live edits at 100k

| Measure | CPU | GPU |
|---|---|---|
| Settle from a cold seed | 56.6 s | 13.7 s (1475 steps) |
| `apply_delta` | ~150 ms | ~200 ms |

## Against the design-review §10 targets

| Target | Result |
|---|---|
| GPU layout step ≤ 5 ms at 100k | **missed** (7.8 ms); open, host-side work identified as the cause |
| Settle ≤ 10 s from cold at 100k | **missed** (13.7 s GPU); open, same cause |

## Layout conformance (GPU vs CPU oracle, 2k-symbol graph)

| Comparison | Symbols k-NN overlap | Files k-NN overlap |
|---|---|---|
| CPU vs CPU with 10% smaller `dt` (self-baseline) | 0.749 | 0.982 |
| GPU vs CPU | 0.752 | 0.975 |

The A1 thresholds are files ≥ 0.9, and symbols ≥ self-baseline − 0.1 (here ≥ 0.649). Both pass.

## Server live run (on the `codescape-model` crate as the repo)

| Measure | Result |
|---|---|
| Graph size | 203 nodes, 443 edges |
| Cold start | 2 s; settled in 1131 steps |
| Warm restart | settled in 5 steps |
| SIGTERM | persists `ids.bin` and `positions.bin` |
| Voice leg | spoken command → caption → answer → 272 KB of s16le speech, round trip |

## Code checked by the ADR worker (existence only, not behaviour)

- **Conformance test:** `crates/codescape/codescape-layout/src/cuda.rs:1077-1078`, `gpu_conforms_to_cpu_oracle`. It is `#[ignore]` and gated on `CODESCAPE_GPU_TESTS=1` (`:985,987`). The CPU self-baseline test is `cpu.rs:700`.
- **ABI guard:**
  - `SIM_PARAMS_BYTES = 212`: `crates/codescape/codescape-layout/src/abi.rs:31`
  - the arity table, now three launched kernels (ADR-2143 A2.4): `abi.rs:33-41`
  - `check_ptx_abi`: `:100`
  - `check_provenance`: `:184-206`; content-tag match: `cuda.rs:459-472`
- **Boot refusal:**
  - `codescape-server/src/config.rs:15,212`
  - `ExitCode::from(2)` at `main.rs:47,52`
  - the test spawns the binary and asserts exit 2: `tests/isolation.rs:156-187`

## P1 gate (raw output committed)

Source: [`gate-raw.txt`](gate-raw.txt). Its header records:
- the date, 2026-10-17T20:28:18Z
- the host, `agentbox`
- `rustc 1.99.0 (b940084d7 2026-09-28)`
- HEAD `458b0ded5da35b968ebd7ad056eba8bc06da38a5`, with the Codescape tree staged and uncommitted
- the graphicle rev `003f93605d895ed69f0cf06690dd296474385b99`
- the `Cargo.lock` sha256 prefix, `34ddf4c9d53d6650`

That prefix matches `sha256sum crates/codescape/Cargo.lock` as re-checked by the ADR worker.

| Command | Result | Exit |
|---|---|---|
| `cargo test --workspace` | 197 passed, 0 failed, 6 ignored (benchmarks, a diagnostic, a host-checkout dogfood test, a live TTS-to-ASR test). Isolation suite: 3 of 3 | `0` |
| `cargo clippy --workspace --all-targets -- -D warnings` | finished | `0` |
| `cargo fmt --all --check` | clean | `0` |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps` | finished | `0` |
| `vitest run --config src/codescape/vitest.config.ts` | 7 files, 60 tests passed (isolation 3) | `0` |

Two limits:
- **The `Permission denied (os error 13)` lines are cargo's global-cache auto-clean, not a test.** The capture filtered the two preceding lines (`warning: failed to auto-clean cache data` and `failed to remove ... global cache`); the agentbox sandbox mounts the cargo registry cache read-only, so the auto-clean cannot delete stale downloads. The warning is emitted after compilation and before `Finished` and has no effect on the test results or exit statuses. Reproduce with `cargo test --workspace 2>&1 | grep -B2 "Permission denied"` in the same container.
- **No `cuda` build.** `codescape-layout` has `default = []` (`Cargo.toml:13`), so this gate never builds or runs `gpu_conforms_to_cpu_oracle`.

## Cross-vendor audit

The audit was run by Codex gpt-6-astra and returned at 2026-10-17T20:30Z, anti-fox verified. It is recorded in ADR-2143 Amendment A2.

| Invariant | Verdict |
|---|---|
| I1-I8 | ACCEPT-WITH-GAP |
| I7a | REJECT: an opt-in ignored test proves nothing unless executed. It is now a required P2 gate step: `CODESCAPE_GPU_TESTS=1 cargo test -p codescape-layout --features cuda -- --ignored gpu_conforms_to_cpu_oracle` |
| I8a | REJECT: not enforced. Compose non-publication is not an egress boundary |
| I9 | Now enforced: `client/src/codescape/src/__tests__/isolation.test.ts:18-21,73-117` |

**Open gaps (verbatim):**
1. I1 scans package names only; a vendored or renamed implementation passes.
2. I3/I4 are helper-level tests, a second encoding path could bypass.
3. I7 accepts a hand-authored manifest saying provenance=compiled.
4. OpenAccess on :4100 is honestly reported as "auth":"open" but publishing 4100 to the LAN exposes all open capabilities until P6 NIP-98. Current mitigation (`docker-compose.unified.yml:477-482`): the port binds to `127.0.0.1` by default, and `CODESCAPE_PUBLISH_ADDR=0.0.0.0` would publish it on the LAN.
5. Bearer scheme match is case-sensitive for "Bearer"/"bearer" only (RFC says case-insensitive) and trims whitespace.
6. RateLimiter is one shared bucket per instance, no constructor validation.

**Fixed in response.** These are working-tree lines. The tree is untracked, so there is no git diff to cite.
- `ids.rs:178-186`: the count is checked against the remaining bytes before `reserve`. Test: `:273-284`.
- `ids.rs:38,47-71`: `#[serde(from = "RawInterner")]` rebuilds the index. Test: `:286-301`.
- `tests/isolation.rs:122-128`: the negative control fails, not skips, when its fixture is missing.
- `tests/isolation.rs:53-59`: the offline-to-online retry prints the offline diagnostic.

## XR wire (raw output committed)

`xr-client/rust/tests/codescape_wire.rs` decodes three frames captured from the running
Codescape server (`/wss` on `visionclaw_container:4100`, CUDA engine, VisionClaw repository,
2026-10-17) with the Godot client's own parsers: the `initialGraphLoad` text frame (1,432 nodes,
3,329 edges, file tier), the join V5 keyframe (every announced node exactly once, no flag bits,
`community_id` equal to `metadata.community` on more than 100 nodes), the first V5 frame after a
hook-driven agent action (exactly one agent-flagged actor record carrying heat) and the `0x23`
agent-action frame (plain target id, a `file` node, source equal to that actor). Result: 3 passed,
`gate-xr-wire-raw.txt`. The fixtures live in `xr-client/rust/tests/fixtures/codescape/`.

## Not exercised in P1, and why

- **Compose deployment: done 2026-10-18 12:19Z.** `CODESCAPE=1 ./scripts/launch.sh up dev` from the
  host shell started the `codescape` service (CUDA engine, VisionClaw 32,173 nodes and agentbox
  19,117 nodes, both from cache) alongside the stack. Two faults surfaced on the first start and
  were fixed: the container could not read its 0600 token (now `cap_add: DAC_READ_SEARCH`), and an
  empty `CODESCAPE_LOOM_URL` from compose reached the egress allowlist as a malformed URL (now
  treated as unset). Through the dev nginx at `http://<host>:3001/codescape/` (ADR-2143 A4): health
  answers, one COOP/COEP/CORP header each, `crossOriginIsolated` is true so the client runs the
  shared-memory decode path, proxy trust resolved to the nginx alias, and the VisionClaw UI still
  serves at `/`. Screenshot `07-shared-port-3001-codescape-cuda-shared-mem.png`. The headset,
  restarted in Codescape mode against `ws://10.10.10.2:3001`, joined the presence shim and rendered
  1,699 file nodes at 90 FPS with no `/ws/presence` retries.
- **Activity-sink hook enablement.** The hook ships on the agentbox branch
  `feat/codescape-activity-sink` and its registration is projected at image boot, so enabling
  `[integrations.activity_sink]` needs a host rebuild from that branch. The hook itself was exercised
  by hand against the agentbox-side Codescape instance (actor and speech bubble visible,
  `04-agentbox-l0-hook-driven-actor.png`).
- **Headset run: done at 21:48Z, see `xr-headset-run.txt`.** The Godot XR client on the HP host
  (Godot 4.6.1, SteamVR, Vive) was restarted against Codescape through a reverse SSH tunnel
  (`127.0.0.1:4100` on HP to `visionclaw_container:4100`), because port 4100 is not yet published
  on the host. It received the Codescape file-tier topology (1,432 nodes, 3,329 edges), applied
  positions for all of them plus the actor record (`positions_known=1433`), the server accepted its
  authenticate envelope, and it rendered at 90 FPS. Gap found: Codescape has no `/ws/presence`
  route, so the client logs a 404 and schedules a reconnect every 2 s (`graph_scene.gd:2958`);
  P4 should add a presence shim or a response the client treats as "presence disabled".
