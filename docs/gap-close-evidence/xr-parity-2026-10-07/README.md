# xr-parity — desktop memory-explorer polish in the headset (2026-10-07)

Branch `feat/xr-cloud-parity`, commit 48e1bf327 (code) + this evidence commit.

## Live capture (HP, windowed `--xr-mode off`)

SteamVR was **not running** on HP at capture time (VIVE Pro present on USB, Steam
in Big Picture, no `vrserver`), so per the brief the windowed capture was used and
SteamVR was not started. `tests/visual/live_parity_capture.gd` loads the real
`GraphScene.tscn` against the live dev backend (`ws://10.10.10.2:4000`, 9 473
nodes), enables the memory cloud through the real signed GET (6 000 rows), and
injects a `memoryRoute` frame through `apply_route_json` (the relay's path) with
`sidecarTotal 5 / sidecarAgree 2`.

| File | What it shows |
|---|---|
| `xr-parity-cue.png` | 1.6 s after the route: route drawn over the glass nodes (no depth test), answer ring, guide dots (camera fallback, no controller in windowed mode) |
| `xr-parity-converged.png` | 5.6 s: cue faded; cloud still framed after physics moved the graph |
| `live-capture.log` | `XR_PARITY_STATE` per shot |

Placement check from the log: cloud robust radius 112.0 × scale 0.7016 = 78.58 =
graph robust radius 78.58 (cloudScale 5); outer node at the graph centre
(−2.72, −5.19, −27.52). Between shots the graph settled to (−2.97, −6.80, −28.40),
r 71.54, and the cloud glided to scale 0.6387 (71.54 / 112.0). HUD line:
"Route: 3 of 5 sidecar hits are in the sample · 2 of 3 sampled agree with the
local top-k".

## Benchmark (HP, Godot 4.6.1 GL, 13 164 nodes / 20 000 edges / 32 hulls, HUD + controllers + avatars, bursts)

| Cloud | Route nodes / sidecar | Draw calls | Triangles | p99 | Gems / cyl / hulls | Result |
|---|---|---|---|---|---|---|
| — | — | 31 | 94 554 | 2.47 ms | 67 / 6 / 32 | PASS |
| 6 000 | 13 / 5 | 34 | 94 558 | 2.95 ms | 35 / 8 / 32 | PASS |
| 20 000 | 13 / 5 | 34 | 94 542 | 2.78 ms | 28 / 8 / 32 | PASS |
| 20 000 | 64 / 64 | 33 | 94 558 | 2.48 ms | 40 / 0 / 32 | PASS |
| same, HUD re-rendered every frame | | 33 | 94 558 | 2.78 ms | 40 / 0 / 32 | PASS |

Combined run: cloud framed on the bench graph (8 000 sprites drawn), route 4 056
triangles including the 12 cue dots, pack CPU p99 0.57 ms, lod_build CPU p99 1.11 ms.

## Tests

- `cargo test -p visionclaw-xr-gdext`: 384 lib + all integration suites green.
- GUT on HP (`gut.sh` + `tests/gut_guard.sh`): 195/195, 27/27 scripts, no parse errors; Graph page 530 px ≤ 532.
- Desktop vitest `memoryCloud/`: 237/237; `tsc --noEmit` clean.
- Server `session_relay` tests: 8/8 (shared fixture with the headset parser).

## Still needs a human in the headset

- Both eyes in the SteamVR mirror with the route drawn over the nodes (the depth-test-free materials are one multiview draw, but the mirror has not been checked).
- The cue from the **tracked controller**, and whether a route drawn over near objects (hands, controllers) is comfortable, since it ignores depth.
- Readability of the 20 px agreement line on the panel.
