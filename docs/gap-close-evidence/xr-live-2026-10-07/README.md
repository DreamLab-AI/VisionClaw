# Live SteamVR session on HP, main 1c8caa68d (2026-10-07)

**Result:** the app reaches `XR_SESSION_STATE_FOCUSED` on SteamVR/OpenXR 2.17.10
with the VIVE Pro, but no frame reaches the headset. SteamVR's compositor
(`vrcompositor`) aborts about 6 s after every start because it cannot present
in direct mode. It did so for all three starts today, the first two before
this client was launched. No mirror-window or per-eye capture was possible.

## What ran

- Sources: `git archive 1c8caa68d xr-client crates/visionclaw-xr-presence` into
  `~/mce-xr/live/` on HP. gdext built with the dev profile (opt-level 2).
- GUT smoke: 201/201, `gut_guard` 28/28 scripts, no parse errors.
- Launch: `live.sh` (here). Detached, `--xr-mode on`, GL Compatibility
  (`opengl3`), `XR_BACKEND_WS=ws://10.10.10.2:4000`, `--verbose --print-fps`.
  PID and log path are in `~/mce-xr/live/RUNNING`.

## Session evidence (`xr-session.log`)

- OpenXR 1.1 instance creation is refused, and Godot falls back to 1.0 (normal on SteamVR 2.17).
- `Requested OpenGL version does not meet the minimum version this runtime supports`:
  Godot's warning that SteamVR's GL requirements exceed the 3.3 context
  gl_compatibility creates. It is non-fatal (the session still reaches FOCUSED);
  the renderer is fixed by the XR-client invariant.
- Session states, in order: IDLE → READY → SYNCHRONIZED → VISIBLE → FOCUSED.
- Frame pacing: 4–5 FPS (200–250 ms per frame), 81 samples. That is `xrWaitFrame`
  timing out against a dead compositor, not this client's render cost. The
  worst-frame benchmark on the same tree remains the measure: p99 2.47–2.71 ms.

## Why the compositor dies (`vrcompositor-excerpt.log`)

| | 10 Sep, 4 Oct (worked) | 7 Oct, three starts |
|---|---|---|
| Driver | 580.178.04 (installed 22 Aug) | same |
| Xorg config, X server start | 20 Sep | same (no reboot, 41 days up) |
| Direct display via RandR | not found | not found |
| Direct display via Vulkan WSI | VIVE Pro, DP-6, 2880×1600 | same |
| `zero modifiers for DRM format` error | present | present (so it is not the cause) |
| `WaitForPendingPresent: failed to wait for present` | never | every start |
| Watchdog abort in `WaitForPresent` | never | every start, after 5.8 s |

Display discovery is identical to the working sessions; only presentation to
DP-6 fails, and it started today. The one change known today is the operator
waking the displays (DP-0 and DP-1-2 enabled; two Quadro RTX 6000s, both
display-active). Restoring presentation needs the operator: check the HMD's
link box and DP-6 power state, then restart SteamVR. None of this was touched
from here, per instruction.

## Windowed fallback (main 1c8caa68d, live backend)

`xr-parity-cue.png` and `xr-parity-converged.png`, captured with `--xr-mode off`
while the XR session stayed running:

- 9,473 live graph nodes and a 6,000-row cloud.
- Cloud framed on the live graph at scale 0.905, following the graph centre as it settled.
- Injected route `verdict=apply`.
- HUD line "Route: 3 of 5 sidecar hits are in the sample · 2 of 3 sampled agree with the local top-k".
- Budget: cloud 1 draw call / 6,000 triangles, route 3 / 6,650.
