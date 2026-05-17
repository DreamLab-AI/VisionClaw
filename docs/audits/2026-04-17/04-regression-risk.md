# Regression Risk Assessment — 2026-04-17

**Scope:** session FE changes (shipped/unstaged) + proposed backend fixes
**Reviewer:** qe-regression-risk-analyzer
**Branch:** `main`, 2 commits ahead of `dreamlab-github/main`
**Ship gate:** dev-only changes so far. Prod impact gated by nginx config reuse.

---

## 0. Headline findings

1. **Backend gap #2 as stated is based on a false premise.** `src/handlers/api_handler/settings/mod.rs:85-142` is **dead code** — it is never mounted by `main.rs`. The live PUT `/api/settings/physics` handler lives at `src/settings/api/settings_routes.rs:~260-395`, mounted via `webxr::settings::api::configure_routes` in `src/main.rs:717`. That handler **already** dispatches `UpdateSimulationParams` to GPUComputeActor (direct), GraphServiceSupervisor, and fires `ForceResumePhysics`. Before "fixing" the dead handler, confirm via runtime tracing which route a slider PUT actually hits — if it returns 200 but nothing moves, the bug is elsewhere (e.g. GPU addr cache miss during first 6 s, or convergence auto-pause re-engaging before damping override takes effect). **This single reframe changes the ship/hold verdict for the backend fix.**
2. **Double-dispatch risk already exists and is by design.** Live handler sends `UpdateSimulationParams` to GPUComputeActor *and* GraphServiceSupervisor (which forwards to PhysicsOrchestratorActor → ForceComputeActor). Adding the same dispatch to the dead handler, if that handler is later wired in, would triple-dispatch. The `ForceComputeActor::Handler<UpdateSimulationParams>` already has an idempotency check ("GPU-relevant fields unchanged, skipping reset"), so double-send is cosmetic not functional — but still a log-noise and ordering hazard.
3. **FastSettle hit-cap "locks forever" is partially mitigated in code.** `PhysicsOrchestratorActor` line ~1747–1756 already early-returns when `fast_settle_complete` or `is_physics_paused`. The lock is **intentional** — after hit-cap exhaustion it marks complete+paused. What's *missing* is a way to **unlatch** on parameter change: `Handler<UpdateSimulationParams>` at line 1340 would need to reset `fast_settle_iteration_count = 0; fast_settle_complete = false;` to give the new params a fresh settle budget. Code at line 744–746 does this reset on another path; line 1400 also does. Verify 1340 handler does too.
4. **Movement threshold 0.001 → 0.01 is a 10× loss of sensitivity.** For graphs at typical scale (positions in ±100 units) this is 0.01% of range — still fine. For micro-graphs (positions in ±1), this is 1% — visible stuttering. Unlikely in current workload but not zero.

---

## 1. Risk matrix

Severity: **C**ritical / **H**igh / **M**ed / **L**ow. Probability: 0–5. Detection: 0 (obvious in dev) – 5 (only fires in production under load).

### 1a. Frontend changes (shipped this session)

| # | Change | Affected feature | Regression prob. | Detection difficulty | Severity | Notes |
|---|---|---|---|---|---|---|
| FE-1 | `nginx.dev.conf` per-location COEP/COOP/CORP | Cross-origin isolation, SAB, module workers | 1/5 | 3/5 | H | Headers duplicated across 3 locations (`~ ^/(\.vite…)`, static-asset regex, `location /`). Location-regex priority: exact match > `^~` > regex > prefix. Nginx picks regex before prefix `/`, so `static-asset` regex wins for `.js`, which *is* desired (includes CORP). Risk: any new asset extension not in the regex silently loses CORP and breaks SAB. |
| FE-2 | `nginx.dev.conf` COEP = `credentialless` | Third-party iframes/images/CDNs | 2/5 | 2/5 | M | `credentialless` is Chromium/Firefox (Firefox 110+), **not Safari**. Safari still requires `require-corp`. If Safari users hit the dev server, COEP header is ignored → `crossOriginIsolated=false` → SAB branch skipped → fallback path runs. Fallback is in place (see FE-5) so this is graceful degradation, not a hard break. |
| FE-3 | `client/index.html` CSP `<meta>` commented out (dev only) | Dev XSS/exfiltration posture | 2/5 | 4/5 | M | Acceptable for dev; **must be re-enabled before prod**. Add a build-time assertion or lint guard that fails the prod build if CSP meta is absent. |
| FE-4 | `vite.config.ts` COEP `credentialless` | Dev SAB availability | 1/5 | 2/5 | L | Vite-level is overridden by nginx anyway for proxied requests; redundant but harmless. |
| FE-5 | `graphWorkerProxy.ts` fallback: `getCurrentPositions()` in non-SAB path | All rendering when SAB disabled | 2/5 | 3/5 | H | RPC per binary-message batch = extra round-trip per server tick. On a 60 Hz binary stream this is 60 extra comlink RPCs/s. Not catastrophic, but measurable CPU hit in Firefox/Safari where SAB may be off. Mitigation already present: `tickInFlight` guard. |
| FE-6 | `graph.worker.ts` dual position update + syncToSharedBuffer | SAB view freshness, non-SAB parity | 2/5 | 4/5 | H | Two write paths must stay in sync. If `currentPositions` drifts from `targetPositions`, SAB readers see one state, `lastReceivedPositions` readers see another. Risk highest during reheat + parameter change race. |
| FE-7 | `graph.worker.ts` movement threshold 0.001 → 0.01 | Tiny-scale graphs | 2/5 | 4/5 | M | 10× less sensitive. For ±100-unit graphs: invisible. For ±1-unit graphs: stuttering. Currently no such graph exists but user-zoomed "local neighbourhood" views could expose it. |
| FE-8 | `GraphManager.tsx` layout transition safety timeout + mass-factor fix | Layout switches (force/radial/grid) | 2/5 | 3/5 | M | Safety timeout risk: if genuinely slow layout takes > timeout, ends up in inconsistent state. Mass-factor fix is a bugfix → positive-only. |
| FE-9 | `GemNodes.tsx` `liveSettingsRef` + WebGPU version bump | WebGL fallback rendering | 3/5 | 3/5 | H | **The version-bump is unconditional** (line 512 bumps `version++` every frame). For WebGL backend, `needsUpdate = true` is enough; `version++` on a non-existent or differently-managed buffer in some three.js versions could error or no-op. Currently code checks `inst.instanceMatrix.array` before bump, so null-safe. Low hard-break risk, but any buffer reallocation path that resets version counter would cascade-invalidate WebGPU state. |
| FE-10 | `AppInitializer.tsx` removed duplicate `onBinaryMessage` handler | Binary stream intake | 1/5 | 1/5 | H | Bugfix: removing double-handler removes double-count. If any downstream component implicitly relied on the duplicate (e.g. counter-based batching), it breaks. Quick grep needed for anyone reading `updateCount`. |
| FE-11 | `graphDataManager.ts` removed double updateCount | Counters/metrics | 1/5 | 2/5 | L | Same class as FE-10. Positive change. Verify no telemetry dashboard hard-codes the old doubled value. |

### 1b. Proposed backend fixes (NOT yet made)

| # | Change | Affected feature | Regression prob. | Detection difficulty | Severity | Notes |
|---|---|---|---|---|---|---|
| BE-1 | Wire `UpdateSimulationParams` into `src/handlers/api_handler/settings/mod.rs` PUT handler | Physics slider responsiveness | — | — | — | **Premise appears false.** Handler is dead code (not routed in `main.rs`). Live route already dispatches. Action should be either (a) delete the dead file, or (b) if it *is* routed via a path I missed, then wiring is valid but must check for triple-dispatch. Requires runtime trace confirmation first. |
| BE-2 | Change FastSettle iteration-cap behaviour | GPU load, convergence, auto-pause | 3/5 | 4/5 | C | If cap is removed or raised without exit condition beyond energy threshold, and energy threshold is never reached (noisy GPU, oscillating system), GPU runs at 100 % indefinitely. Current MIN_SETTLE_WARMUP=100 + max-iterations double-guard is correct; don't lift without a wall-clock deadline. |
| BE-3 | Reset fast-settle state on `UpdateSimulationParams` at line 1340 | Slider → visible motion after converged-pause | 2/5 | 3/5 | H | This is the real fix if sliders don't visibly affect a converged graph. Low risk: same reset pattern used at lines 744 and 1400 already. Side-effect: every slider twitch reheats — noisy user experience. Consider debouncing or only resetting when params changed materially (delta > epsilon). |

---

## 2. Testing gap table

| Code path | Existing coverage | Gap severity | Highest-ROI new test |
|---|---|---|---|
| `PhysicsOrchestratorActor::Handler<UpdateSimulationParams>` (line 1340) | `tests/physics_parameter_flow_test.rs`, `tests/gpu_physics_pipeline_test.rs` | M | Unit: send UpdateSimulationParams after fast_settle_complete=true → assert counters reset + simulation resumes |
| Live PUT `/api/settings/physics` route | `tests/settings_integration_test.rs`, `tests/integration_settings_sync.rs`, `tests/settings_sync_test.rs` | L-M | End-to-end: PUT → assert ForceComputeActor received UpdateSimulationParams within 500 ms (actix test) |
| Dead handler `api_handler/settings/mod.rs` | none | N/A | Delete file or document why retained |
| FastSettle hit-cap lock | none directly | H | Unit: drive to hit-cap, send UpdateSimulationParams, assert new settle cycle starts |
| `graphWorkerProxy` non-SAB fallback | none | H | Jest/Vitest: mock SharedArrayBuffer unavailable, assert `getCurrentPositions` called per binary message and `lastReceivedPositions` populated |
| `graph.worker` dual-position sync | none | H | Property test: random position sequences → assert `currentPositions === SAB view` after N ticks |
| COEP `credentialless` compatibility | none | M | Playwright matrix (Chrome/Firefox/Safari/Edge): load page, check `self.crossOriginIsolated` and `typeof SharedArrayBuffer !== 'undefined'` |
| Movement threshold 0.01 | none | L | Visual regression: micro-graph (scale ±1) with known motion, snapshot |
| WebGPU vs WebGL `instanceMatrix.version` | none | M | Rendering smoke test on both backends: force WebGL via env flag, assert 60 fps sustain |

---

## 3. Ship / hold per change

| Change | Verdict | Gate |
|---|---|---|
| FE-1 nginx per-location headers | **SHIP (dev)** | Before prod rebuild: verify cloudflared/prod nginx has same per-location duplication. If prod uses Cloudflare Workers for headers, confirm they apply to module-worker URLs. |
| FE-2 COEP credentialless | **SHIP (dev)** | Add automated Safari detection that emits a clear user-visible banner "SAB disabled — reduced performance mode". |
| FE-3 CSP disabled | **SHIP (dev) / HOLD (prod)** | Add `assert(hasCSPMetaOrHeader)` to prod build step. |
| FE-4 vite COEP | **SHIP** | None. |
| FE-5 non-SAB fallback | **SHIP** | Monitor comlink RPC rate in Firefox. |
| FE-6 dual-position update | **SHIP WITH TEST** | Add the parity property test before next prod cut. |
| FE-7 movement threshold 0.01 | **SHIP** | Revert if any user reports "tiny graphs stutter". |
| FE-8 GraphManager safety timeout | **SHIP** | Log when timeout fires in telemetry. |
| FE-9 WebGPU version bump | **SHIP** | Smoke test on WebGL before next prod cut. |
| FE-10/11 double-handler removal | **SHIP** | Grep for any consumer of `updateCount` that assumed doubling (none found in a quick scan). |
| BE-1 wire UpdateSimulationParams into dead handler | **HOLD — premise uncertain** | Produce runtime trace showing which handler a slider PUT actually reaches. If live handler, delete dead file instead. |
| BE-2 change FastSettle cap | **HOLD** | Specify exact new behaviour; add wall-clock deadline to GPU guard. |
| BE-3 reset fast-settle on UpdateSimulationParams | **SHIP AFTER TEST** | Add unit test from row 1 of testing-gap table first. |

---

## 4. Smoke-test checklist (<10 min, manual)

Run in Chrome and Firefox against `https://<dev host>:3001`. Safari optional — document the graceful-degradation result.

1. **Load app** — DevTools console: confirm `crossOriginIsolated === true` in Chrome/Firefox. In Safari confirm graceful warning fires from `graphWorkerProxy.ts:126`.
2. **Verify SAB path** — In console: `typeof SharedArrayBuffer` → `"function"`. Check logs for `SharedArrayBuffer initialized: <N> bytes`.
3. **Fresh-load graph** — Graph appears, nodes move, forces settle. Timing: visible motion within 3 s of first binary message.
4. **Physics slider test** — Open settings panel, move `damping` slider from default to 0.1. Within 1 s the graph should visibly reheat. If not → BE-1/BE-3 gap confirmed.
5. **Repeat slider test after convergence** — Wait for motion to stop (~10 s). Move `repulsion_strength` slider. Must resume motion. If not → BE-3 is the fix, not BE-1.
6. **Layout transition** — Switch force → radial → grid → force. No stuck-in-transition state. Confirms FE-8.
7. **Binary stream rate** — Network tab, WSS frames: ~60/s steady-state.
8. **Worker error resilience** — In console run `graphWorkerProxy.getConsecutiveErrors()` → 0.
9. **WebGL fallback** — Start Chrome with `--use-gl=swiftshader` or disable WebGPU flag. Reload. Nodes still render and move. Confirms FE-9.
10. **CSP re-enable rehearsal** (prod-path only) — Uncomment CSP meta in `client/index.html`, hot-reload, confirm no CSP violations in console for current feature set. Revert.

Total wall-clock: ~8 min per browser.

---

## 5. Recommendation summary

- All FE changes are dev-scope and safe to ship with the listed follow-up tests. Biggest real risk is FE-6 (dual-position sync invariant) and FE-9 (WebGL regression from WebGPU-targeted code).
- **Re-verify the backend premise before touching backend code.** The dead-code finding is the highest-ROI outcome of this review; fixing a non-bug wastes effort and risks regressing the working path.
- Add four targeted tests (one per H-severity gap: dual-position parity, non-SAB fallback RPC, FastSettle unlatch on param change, cross-browser COEP) before next prod cut.
