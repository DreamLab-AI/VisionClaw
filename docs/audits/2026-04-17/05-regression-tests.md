# Regression Tests — 2026-04-17 Session

Guards against the four bugs identified in this audit session. All test files
are self-contained and wired into the repo's standard test locations.

## Test Inventory

| # | Test File | Target Bug | Type | CI Integration |
|---|-----------|------------|------|----------------|
| 1 | `tests/smoke/nginx-coep-headers.sh` | #1 Worker COEP load failure | smoke (shell + curl) | Run in a post-deploy stage against the dev/preview URL. Exits non-zero on header mismatch — plug straight into any shell runner. |
| 2 | `client/src/features/graph/managers/__tests__/graphWorkerProxy.fallback.test.ts` | #2 SharedArrayBuffer fallback | unit (Vitest) | Picked up automatically by the existing Vitest suite (same `__tests__` dir as `graphDataManager.test.ts`). Zero config. |
| 3 | `tests/physics_orchestrator_settle_regression.rs` | #3 FastSettle dead-end | integration (actix actor) | Marked `#[ignore]` to keep `cargo test` fast. CI regression job should run `cargo test --test physics_orchestrator_settle_regression -- --ignored`. |
| 4 | `tests/settings_physics_propagation_regression.rs` | #4 Settings→actor propagation | integration (actix actor) + scaffold | Marked `#[ignore]`. First test runs today once the handler is fixed; second test is a documented scaffold for the full HTTP round-trip pending an `AppState::test_minimal()` helper. |

## How to Run

**Shell smoke test (#1)**

```bash
# Against the local dev stack (nginx listens on :3001)
BASE_URL=http://localhost:3001 tests/smoke/nginx-coep-headers.sh

# Against a preview env
BASE_URL=https://preview.example.net tests/smoke/nginx-coep-headers.sh
```

Exits 0 on success, 1 on any header mismatch. Coloured output summarises each
probed URL (`html-root` and `module-worker`).

**Vitest unit test (#2)**

```bash
cd client
npx vitest run src/features/graph/managers/__tests__/graphWorkerProxy.fallback.test.ts
# or
npx vitest            # runs the whole frontend suite, new test included
```

**Rust regression tests (#3, #4)**

```bash
# Include the ignored regression suite
cargo test --test physics_orchestrator_settle_regression -- --ignored --nocapture
cargo test --test settings_physics_propagation_regression -- --ignored --nocapture

# Or run everything ignored at once
cargo test -- --ignored
```

## Coverage by Bug

| Bug | File of Record | Regression Surface Covered | What's Asserted |
|-----|----------------|----------------------------|-----------------|
| #1 COEP headers | `nginx.dev.conf` (per-location re-add) | HTML root + `/src/features/graph/workers/graph.worker.ts` | `Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Embedder-Policy: credentialless`, `Cross-Origin-Resource-Policy: same-origin` on every probed URL. HTTP status is 2xx or 304. |
| #2 SAB fallback | `client/src/features/graph/managers/graphWorkerProxy.ts:198-216, 358-360` | `processBinaryData` → `getCurrentPositions` → `lastReceivedPositions` → `getPositionsSync` | (a) `getCurrentPositions` is invoked exactly once per binary frame when `SharedArrayBuffer` is undefined; (b) returned positions are cached and surfaced via `getPositionsSync`; (c) empty returns do NOT clobber the cached frame; (d) the fallback path is skipped when a SAB view is present. |
| #3 FastSettle dead-end | `src/actors/physics_orchestrator_actor.rs` (iteration-cap branch around L1795-1835) | Actor responsiveness after a non-converging FastSettle cycle | Actor accepts a post-settle `UpdateSimulationParams` within 1 s (not wedged on `fast_settle_complete`), and `GetPhysicsStatus` still responds. Repeated updates also succeed. |
| #4 Settings propagation | `src/handlers/api_handler/settings/mod.rs:81-143` | Handler → `PhysicsOrchestratorActor` message dispatch | Unit-level proxy test asserts `UpdateSimulationParams` is absorbed end-to-end (new `damping` value visible via `GetPhysicsStatus`). Full HTTP round-trip exists as an `#[ignore]`d scaffold pending `AppState::test_minimal()`. |

## Known Gaps (out of scope today)

1. **Full HTTP-level settings propagation test** — requires an `AppState`
   test harness. Tracked inside
   `tests/settings_physics_propagation_regression.rs::http_put_physics_settings_propagates_to_orchestrator`
   as an ignored scaffold with an implementation outline. Needs a
   `AppState::test_minimal(settings_addr, physics_addr)` constructor.

2. **COEP header coverage in production nginx** — this test only covers
   `nginx.dev.conf`. `nginx.production.conf` has its own location blocks that
   should be probed under the same contract. Trivial extension: parameterise
   `BASE_URL` in the smoke script (already done) and add a CI job pointed at
   the prod preview.

3. **Warning-log assertion for FastSettle cap** — the orchestrator emits a
   `warn!` when FastSettle hits the iteration cap without convergence. A
   direct log-capture assertion requires wiring `env_logger`'s `Builder` into
   the test harness or swapping in `test-log`. Not added here to keep the
   dependency set lean.

4. **Real binary protocol fixtures for #2** — the Vitest test uses synthetic
   `Float32Array`s. A richer fixture would replay a captured WebSocket frame
   through the real `parseBinaryNodeData` pipeline. Belongs in a separate
   integration-style frontend test run inside a browser-like environment
   (Playwright component test), not Vitest jsdom.

5. **Worker CSP/CORP edge cases** — the smoke script asserts presence and
   exact values. It does NOT currently verify that the `Content-Security-Policy`
   header is compatible with module workers, nor that SRI is preserved when
   Vite's HMR injects scripts. These were not implicated in this session's
   bug but are plausible next-nearest regressions.

6. **Stress-testing FastSettle convergence under real GPU load** — the Rust
   tests run with `gpu_initialized = false`. A GPU-enabled test rig would
   close the loop on the full physics pipeline but requires CUDA in CI.

## Conventions Followed

- **Rust**: top-level `tests/*.rs` files (Cargo auto-discovery). No
  `[features]` gating — use `#[ignore]` to keep fast CI green.
- **Frontend**: `__tests__` sibling dir matching `graphDataManager.test.ts`.
  Vitest globals via `vi.mock`, no new deps.
- **Shell**: `tests/smoke/` (new subdir — this is the first shell smoke test
  in the repo). POSIX-ish bash, `set -euo pipefail`, coloured output.
