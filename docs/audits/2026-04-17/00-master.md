# Master Audit — Graph-Loading Failure Session

> **HISTORICAL**: This audit from 2026-04-17 identified issues that have since been resolved. Retained for traceability — do not action these findings without first verifying against current code.

**Date:** 2026-04-17
**Coordinator:** hierarchical-coordinator (queen)
**Inputs:** 5 parallel worker reports (01-frontend, 02-backend-routing, 03-silent-failure, 04-regression-risk, 05-tests)
**Status:** Findings acted upon — see KNOWN_ISSUES.md resolved table.

---

## 1. Executive Summary

The session fixed a real production-blocker: module-worker scripts served by Vite were being stripped of their COEP header by nginx's `add_header` non-inheritance under per-location override. Per-location re-declaration of `Cross-Origin-Opener-Policy`/`-Embedder-Policy`/`-Resource-Policy` in `nginx.dev.conf` restored SAB and unblocked WebSocket. That fix is correct, minimal, and safe to ship in dev.

The **frontend commits bundle more than the COEP fix**: a non-SAB fallback path, a dual-position worker write, a WebGPU `instanceMatrix.version++` hack, five staggered synthetic resize kicks, a silent movement-threshold 10× reduction, and an unrelated `enterprise.html` Vite entry. Each is defensible in isolation; together they are a shotgun. Five are flagged **high**: double Comlink RPC per frame (#FE-5/proxy line 208), unguarded resize-kick timers (#GraphCanvas line 196), transition-timeout missing forceDirected SAB restore (#GraphManager line 585), a mass-factor keying change that may silently zero all node masses (#GraphManager line 604), and zero new tests for any of it.

The **backend "slider doesn't move the graph" bug has a critical reframe**. Report 04 (regression-risk) finds that `src/handlers/api_handler/settings/mod.rs:85-142` — the file identified as the bug site — is **dead code, not mounted in `main.rs`**. The live PUT `/api/settings/physics` route is `src/settings/api/settings_routes.rs:~260-395`, which already dispatches `UpdateSimulationParams` to GPUComputeActor and GraphServiceSupervisor and fires `ForceResumePhysics`. If the slider still does not visibly affect a converged graph, the fix is not "wire the handler" — it is BE-3: reset `fast_settle_complete` + `fast_settle_iteration_count` inside `PhysicsOrchestratorActor::Handler<UpdateSimulationParams>` at line 1340. Lines 744 and 1400 already do this reset on other paths; line 1340 must be audited and (likely) patched to match.

Report 02 independently confirms the **architectural** problem even if the specific handler is dead: `OptimizedSettingsActor::update_settings` has no fan-out responsibility despite holding `graph_service_addr` and `gpu_compute_addr`. Eight handler paths write settings; five remember to call `propagate_physics_to_gpu`, three do not. Eleven additional handlers across graph, layout, ontology, and semantic-forces namespaces show the same never-propagated or inverse-never-persisted pattern. This is a **systemic coupling gap**, not a single typo.

Report 03 names the silent-failure pattern itself: "latched done-flag" with conflated success/exhaustion branches. FastSettle is the reference instance; four other actors show the same shape (gpu_self_init counter in force_compute_actor, workspace initialized flag, supervisor permanently-failed counters in two non-resetting supervisors, Louvain no-convergence warning). The gpu_init watchdog at physics_orchestrator_actor.rs:408-416 is the reference pattern for fixing all of them.

**Bottom line:** dev-side changes are safe to commit behind the listed test additions. The backend "fix" must be gated on a runtime trace confirming which route a slider PUT actually hits — fixing dead code is worse than no fix. Prioritise `Handler<UpdateSimulationParams>` unlatch (BE-3), then centralise settings fan-out in `OptimizedSettingsActor`, then sweep the latched-flag pattern using the gpu_init watchdog template.

---

## 2. Cross-Referenced Findings

### 2.1 Convergent findings (same issue named by multiple workers)

| Finding | Reports | Severity |
|---|---|---|
| FastSettle iteration-cap latches permanently without distinct "exhausted" state | 03 #1, 04 §0.3, 04 BE-2/BE-3 | **CRITICAL** |
| `Handler<UpdateSimulationParams>` must reset fast-settle counters (BE-3) | 04 §0.3, 05 test #3 | **CRITICAL** |
| Settings propagation is duplicated/divergent across handler suites | 02 rows #1/#2/#3/#5-#9, 04 §0.1-0.2 | **HIGH (reframed)** |
| No regression tests for any of the FE changes | 01 #18, 04 §2, 05 (provides them) | **HIGH** |
| CSP meta commented out in `index.html` ships to prod too | 01 #3, 04 FE-3 | **MEDIUM** |
| `credentialless` COEP breaks on Safari, fallback path must work | 01 #2, 04 FE-2, 05 gap #4 | **MEDIUM** |
| Worker non-SAB fallback issues second Comlink RPC per binary frame | 01 #4, 04 FE-5, 05 test #2 | **HIGH** |
| Movement threshold 0.001 → 0.01 is a silent perceptual change | 01 #6, 04 FE-7 | **MEDIUM** |
| Mass-factor keying change may zero all node masses | 01 #10 | **HIGH** |
| Latched done-flag pattern present in ≥5 actors | 03 §2 rows #1/#4/#5/#10/#15 | **HIGH** |

### 2.2 Single-observer findings (worth acting on but only one report flags)

| Finding | Report | Severity |
|---|---|---|
| Five staggered `setTimeout` resize kicks lack unmount guard | 01 #7 | HIGH |
| Transition 2x-duration timeout skips forceDirected SAB restore | 01 #9 | HIGH |
| `instanceMatrix.version++` three.js internal-API cast | 01 #12, 04 FE-9 | MEDIUM |
| CQRS graph-write endpoints never notify GraphServiceActor | 02 #40-#45 | HIGH |
| `layout/zones` has explicit TODO for missing forward | 02 #38 | HIGH |
| Settings reset / save do not call `propagate_physics_to_gpu` | 02 #7, #8 | HIGH |
| Semantic-forces config pushes to GPU but not to `AppFullSettings` | 02 #27-#29, #31, #32 | MEDIUM |
| `POST /physics/settle-mode` is a no-op echo stub | 02 #36 | MEDIUM |
| `analytics_supervisor.rs` / `graph_analytics_supervisor.rs` restart counters may not reset after window | 03 #10 | HIGH |
| `enterprise.html` Vite entry is unrelated to graph-loading fix | 01 #19 | LOW |

### 2.3 Contradictions between reports

- Report 02 treats `api_handler/settings/mod.rs:81` as the live bug site and recommends centralising fan-out in `OptimizedSettingsActor`. Report 04 finds the handler is dead code, never mounted. **Resolution:** Run `rg -n 'configure_routes|mod\s+api_handler' src/main.rs src/app_setup.rs` plus actix instrumentation to trace which handler a PUT `/api/settings/physics` actually hits. The architectural recommendation in report 02 stands regardless of which handler is live; the specific fix target changes.

---

## 3. Priority Ranking & Action Sequence

### 3.1 Commit now (already done in session + low-risk follow-ups)

1. **nginx COEP per-location headers** — shipped, keep. Add inline comment referencing nginx `add_header` inheritance semantics (report 01 #1).
2. **Removed duplicate `onBinaryMessage` / `updateCount`** — ship, positive bugfix (report 01 #13-#14).
3. **Verbose worker `onerror` + `messageerror` listener** — ship (report 01 #15).

### 3.2 Fix before prod cut (this sprint, staged as separate commits)

Priority order — each item has a gating test or verification step:

| # | Action | Gate | Owner hint |
|---|---|---|---|
| 1 | **Runtime-trace slider PUT** to confirm live vs dead handler before any backend change | actix middleware log or `curl -v` against running dev backend | backend eng |
| 2 | **Patch `PhysicsOrchestratorActor::Handler<UpdateSimulationParams>` (line 1340)** to reset `fast_settle_complete`, `fast_settle_iteration_count`, and `equilibrium_stability_counter` | Test #3 from `05-regression-tests.md` must pass | backend eng |
| 3 | **Consolidate worker non-SAB fallback** — return `currentPositions` on `processBinaryData` response instead of second RPC (report 01 #4) | Vitest fallback test (#2 in report 05) + FPS regression check on Firefox | frontend eng |
| 4 | **Verify `connectionCountMap` keying** for mass-factor lookup fix (report 01 #10) — may be a silent regression | Integration test asserting mass factors are nonzero for a representative graph | frontend eng |
| 5 | **Guard `GraphCanvas` resize-kick timers** with `mounted` ref + clearTimeout (report 01 #7) | React unmount test | frontend eng |
| 6 | **Add forceDirected SAB restore inside 2x-duration timeout** in `GraphManager.tsx:585` (report 01 #9) | Transition-timeout test | frontend eng |
| 7 | **Restore CSP meta for prod** — build-time inject or split `index.html` (report 01 #3, 04 FE-3) | Prod build assertion `hasCSPMetaOrHeader` | frontend/infra |
| 8 | **Wire all four regression tests from report 05** into CI | `tests/smoke/nginx-coep-headers.sh` post-deploy; two `#[ignore]`d Rust tests in regression job; Vitest test auto-picked up | QE |
| 9 | **Decide on dead handler** — delete `src/handlers/api_handler/settings/mod.rs` or document why retained (report 02, 04 §0.1) | Code review | backend eng |
| 10 | **Split `enterprise.html` Vite entry** out of this PR (report 01 #19) | Commit hygiene | frontend eng |

### 3.3 Next sprint (architectural / systemic)

| # | Action | Rationale |
|---|---|---|
| 11 | **Centralise settings fan-out in `OptimizedSettingsActor::update_settings`** using diffable dispatch (report 02 §Architectural Recommendation) | Eliminates the divergent-handler class of bug by construction |
| 12 | **Split FastSettle flag into `converged` vs `exhausted`** + typed `PhysicsSettled` broadcast + watchdog on exhausted-branch (report 03 #1) | Fixes seed bug's user-visible silence |
| 13 | **Apply the same split to `gpu_self_init_attempts` counter** in `force_compute_actor.rs:150` — add periodic reset or new-context clear (report 03 #4) | Same pattern class |
| 14 | **Audit `analytics_supervisor.rs` / `graph_analytics_supervisor.rs`** for restart-window reset (report 03 #10) | Same pattern class |
| 15 | **Add persistence for `/api/semantic-forces/*/configure`** — currently survives only until restart (report 02 #27-#29, #31) | Data-loss on restart |
| 16 | **Wire `layout/zones` TODO** (report 02 #38) | Known incomplete feature |
| 17 | **Make `is_physics_paused` a setter** that always zeros `equilibrium_stability_counter` (report 03 §4 pt 6) | Eliminates resume-then-repause edge case |
| 18 | **Add runtime trace or actix middleware** logging which handler serves which route, committed under `src/utils/routing_debug.rs` | Prevents future dead-handler audits |

### 3.4 Accept as risk (document, do not fix this cycle)

- **Safari COEP degradation** — `credentialless` not supported; non-SAB fallback already in place, acceptable for dev. Accept with a banner.
- **CSP-disabled dev build** — acceptable for dev velocity; fixed by item 7 above before prod.
- **Movement threshold 0.01** — only problematic for ±1-unit micro-graphs which don't exist in current workload. Revert only if user-reported.
- **`instanceMatrix.version++` internal three.js API** — fragile but contained; re-audit on next three.js upgrade.
- **Neo4j-first writes to graph nodes/edges via CQRS** (report 02 #40-#45) — large refactor; accept until the settings fan-out is centralised, then extend the pattern.

---

## 4. Scorecard

### 4.1 Files shipped this session (frontend)

| File | Verdict | Severity of residual issues | Gate |
|---|---|---|---|
| `nginx.dev.conf` | SHIP | info | Add inline comment |
| `client/vite.config.ts` | SHIP | low | Document dev/prod asymmetry |
| `client/index.html` | SHIP DEV / HOLD PROD | medium | Build-time CSP assertion before prod |
| `client/src/features/graph/managers/graphWorkerProxy.ts` | SHIP WITH FOLLOW-UP | high (double RPC) | Report 01 #4 + test #2 |
| `client/src/features/graph/workers/graph.worker.ts` | SHIP WITH FOLLOW-UP | medium (syncToSharedBuffer wastefulness) + medium (threshold change) | Parity test + threshold justification |
| `client/src/features/graph/components/GraphManager.tsx` | SHIP WITH FOLLOW-UP | high (timeout-path SAB restore, mass-factor keying) | Items 4, 6 above |
| `client/src/features/graph/components/GraphCanvas.tsx` | SHIP WITH FOLLOW-UP | high (unguarded timers) | Item 5 above |
| `client/src/features/graph/components/GemNodes.tsx` | SHIP WITH FOLLOW-UP | medium (internal three.js API) | WebGL smoke test |
| `client/src/app/AppInitializer.tsx` | SHIP | info | — |
| `client/src/features/graph/managers/graphDataManager.ts` | SHIP | info | — |

### 4.2 Backend endpoints / actors by severity

| Path / symbol | Category | Severity | Owner hint | Action |
|---|---|---|---|---|
| `src/actors/physics_orchestrator_actor.rs:1340` (`Handler<UpdateSimulationParams>`) | latched-flag unlatch | **CRITICAL** | backend eng | BE-3 — reset counters + flag |
| `src/actors/physics_orchestrator_actor.rs:1794-1825` (FastSettle cap branch) | latched-flag | **CRITICAL** | backend eng | Split flag, distinct broadcast, watchdog |
| `src/handlers/api_handler/settings/mod.rs:81-293` | dead code or bug site | **HIGH (reframed)** | backend eng | Trace first, then delete or wire |
| `src/actors/optimized_settings_actor.rs:589-610` (non-notifier) | systemic coupling | **HIGH** | backend eng | Item 11 above |
| `src/handlers/settings_handler/write_handlers.rs:240, :281` (reset, save no-propagate) | settings fan-out gap | **HIGH** | backend eng | Add `propagate_physics_to_gpu` |
| `src/handlers/graph_state_handler.rs:179-420` (CQRS writes no notify) | systemic | **HIGH** | backend eng | Accept risk this sprint, fix in item 11 extension |
| `src/handlers/layout_handler.rs:137` (zones TODO) | known incomplete | **HIGH** | backend eng | Item 16 |
| `src/actors/gpu/force_compute_actor.rs:150-154` (permanent give-up) | latched-flag | **HIGH** | backend eng | Item 13 |
| `src/actors/gpu/analytics_supervisor.rs`, `graph_analytics_supervisor.rs` | latched-flag | **HIGH** | backend eng | Item 14 — audit restart-window reset |
| `src/handlers/api_handler/semantic_forces.rs:57, 147, 218` (no persistence) | inverse propagation | **MEDIUM** | backend eng | Item 15 |
| `src/handlers/physics_handler.rs:385` (settle-mode stub) | placeholder | **MEDIUM** | backend eng | Implement or document |
| `src/handlers/api_handler/ontology/mod.rs:487, 642` (partial propagation) | settings fan-out | **MEDIUM** | backend eng | Fold into item 11 |
| `src/actors/workspace_actor.rs:56` (no reload) | latched-flag | **MEDIUM** | backend eng | ReloadFromStorage or mtime watch |
| `src/utils/unified_gpu_compute/community.rs:189` (Louvain no-convergence) | silent near-failure | **MEDIUM** | backend eng | Add convergence flag to result |
| `src/actors/ontology_actor.rs:119` (graph_cache TTL) | cache staleness | **LOW-MED** | backend eng | TTL or file-watch |

### 4.3 Test coverage scorecard

| Surface | Existing | New (report 05) | Still missing |
|---|---|---|---|
| nginx COEP headers | none | `tests/smoke/nginx-coep-headers.sh` | prod config probe |
| Worker SAB fallback | none | `graphWorkerProxy.fallback.test.ts` | real WSS fixture replay |
| FastSettle unlatch | none | `tests/physics_orchestrator_settle_regression.rs` (ignored) | GPU-enabled variant |
| Settings → actor propagation | partial | `tests/settings_physics_propagation_regression.rs` (ignored) | HTTP round-trip (needs `AppState::test_minimal`) |
| Cross-browser COEP | none | none | Playwright Chrome/Firefox/Safari matrix |
| WebGPU vs WebGL `version++` | none | none | Backend-switched smoke test |
| Mass-factor keying (report 01 #10) | none | none | Integration test |
| Layout-transition timeout path | none | none | React/Vitest test |

---

## 5. Ship Gate Summary

- **Commit now:** all frontend files shipped this session, after adding unmount guards (item 5) and verifying mass-factor keying (item 4). Without those two, hold the `GraphManager.tsx` / `GraphCanvas.tsx` changes.
- **Hold until runtime trace:** any fix targeting `src/handlers/api_handler/settings/mod.rs` — premise uncertain.
- **Ship with test:** BE-3 unlatch patch on `physics_orchestrator_actor.rs:1340`, gated by the ignored Rust regression test in report 05.
- **Defer to next sprint:** centralised `OptimizedSettingsActor` fan-out, FastSettle flag split, supervisor restart-window audit, CQRS→actor notification.
- **Accept as risk:** Safari COEP degradation, CSP dev-disable, movement threshold, three.js internal API cast, Neo4j-first graph writes.

---

## 6. Worker Report Index

| File | Worker | Focus | Severity count |
|---|---|---|---|
| `01-frontend-graph-loading.md` | reviewer | Uncommitted FE changes | 4 high / 5 medium / 2 low / 9 info |
| `02-backend-settings-routing.md` | code-analyzer | State→actor propagation gaps | 2 critical / 8 high / 13 medium / 5 low/info |
| `03-similar-failure-patterns.md` | researcher | Latched-flag anti-pattern sweep | 1 critical / 4 high / 4 medium / 7 low/info |
| `04-regression-risk.md` | qe-regression-risk-analyzer | Change-risk matrix + dead-code reframe | 1 critical / 5 high / 4 medium / 5 low |
| `05-regression-tests.md` | qe-test-generator | Four runnable regression tests | — (deliverable) |

All worker reports are in `/home/devuser/workspace/project/docs/audits/2026-04-17/`.
