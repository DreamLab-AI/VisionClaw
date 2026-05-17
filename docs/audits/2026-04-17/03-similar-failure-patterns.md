# Audit 03 — Silent-Failure Patterns in the Rust Backend

**Date**: 2026-04-17
**Scope**: `src/actors/**`, with follow-through into `src/services`, `src/events`, `src/utils`.
**Seed bug**: `physics_orchestrator_actor.rs` — `SettleMode::FastSettle` iteration-cap branch flips `fast_settle_complete = true`, and `physics_step()` early-returns forever. No broadcast distinguishes "converged" from "gave up".

---

## 1. Pattern Definition

**"Latched done-flag"**: a boolean field on a long-lived actor whose transition to `true` permanently short-circuits a per-tick processing path, with one or more of these additional liabilities:

1. The limit-hit branch is treated the same as the success branch (same flag, same paused state, same broadcast, same log level).
2. The flag is only cleared by specific, non-obvious paths (param change, explicit resume, or restart), not by the condition that *caused* it going away.
3. The visible signal to the user is indistinguishable from a healthy completion ("paused", "settled", "converged").
4. No watchdog or timer checks that the flag is still *warranted*.

The seed bug exhibits all four.

---

## 2. Instance Table

| # | File:Line | Flag / Mechanism | Trigger | Cleared at runtime by | Severity | Suggested fix |
|---|-----------|------------------|---------|-----------------------|----------|---------------|
| 1 | `src/actors/physics_orchestrator_actor.rs:109, 1794-1825, 266-271, 1748-1750` | `fast_settle_complete: bool` | Energy < threshold **OR** `fast_settle_iteration_count >= max_settle_iterations` — **both branches set the same flag and pause physics** | `UpdateSimulationParams` (line 1401), `GPUInitialized` (1655), `resume_physics` (746). **Not** cleared by new graph data alone or by any watchdog. | **CRITICAL** (seed bug) | Split into two flags (`fast_settle_converged` vs `fast_settle_exhausted`), broadcast distinct state to client, log the cap-hit branch at `warn!` not `info!`, schedule a watchdog retry after N seconds if exhausted but energy later becomes valid. The `exhausted && !energy_valid` branch at 1826 already demonstrates the correct recovery shape — apply it to the exhausted+valid case too. |
| 2 | `src/actors/physics_orchestrator_actor.rs:109, 2023-2024` (tests) | Same flag as #1, but tests **assert** the conflated behaviour | — | — | **HIGH** | Tests lock in the bug. Rewrite once #1 is fixed so cap-hit asserts a different flag / emits a distinct event. |
| 3 | `src/actors/physics_orchestrator_actor.rs:56, 283, 389-437` | `gpu_initialized: bool` + `gpu_init_in_progress: bool` | Set on `GPUInitialized` receipt | Address replacement (1311), 30-s stuck-init timeout (408), `GPUInitFailed` (1688 — correctly keeps `gpu_initialized=false`). | MEDIUM — mostly handled | Already has watchdog + mailbox-closed detection. Keep as the **reference pattern** for fixing the others. |
| 4 | `src/actors/gpu/force_compute_actor.rs:150-154, 244-250, 2154-2173` | `gpu_self_init_attempts >= gpu_self_init_max_retries (3)` | Three failed GPU context inits | **Only actor respawn** — there is no runtime reset. | HIGH | Correctly sends `GPUInitFailed` upstream, but the counter itself is permanent for the life of the actor. Add periodic (e.g. 60 s) retry-budget reset or a handler on `SetSharedGPUContext` that clears the counter when a new context arrives. |
| 5 | `src/actors/workspace_actor.rs:56, 253-259, 597` | `initialized: bool` in `ensure_initialized()` | First successful `load_from_storage` | Never — there is no reload message. | MEDIUM (data staleness) | If `workspaces.json` is edited out-of-band, in-memory state diverges forever. Add `ReloadFromStorage` message or mtime check. |
| 6 | `src/actors/physics_orchestrator_actor.rs:128, "cpu_fallback_warned"` | Log-once guard | First CPU fallback entry | Never | LOW (cosmetic) | Acceptable — logging dedup only. Document as "intentionally latched". |
| 7 | `src/actors/physics_orchestrator_actor.rs:714-728` (`check_equilibrium_and_auto_pause`) | `equilibrium_stability_counter` → `is_physics_paused = true` | Counter reaches `check_frames` while paused-branch skips reset of counter (line 731 `if !is_physics_paused`) | `resume_physics` (741), `UpdateSimulationParams` (1409), `FastSettle` reset (746). | MEDIUM | When paused, counter is *not* zeroed on non-equilibrium frames (guarded by `if !is_physics_paused`), so as soon as physics resumes a single high counter could re-pause immediately. Already partially fixed at 1407-1409 but only on param change; the `resume_physics` path at 741 correctly zeros it. Edge case: resume via a path that does **not** call `resume_physics()` (e.g. direct mutation) would leave it stale. Centralise by making `is_physics_paused` a setter that always zeros the counter. |
| 8 | `src/actors/ontology_actor.rs:119, 287-290, 924` | `graph_cache: HashMap<id, (graph, signature, ts)>` | Populated on first lookup; invalidated only by explicit `.clear()` or signature mismatch | Signature-based — but signature is computed only on demand. | LOW-MEDIUM | External ontology file edits won't bust the cache until a signature recompute is forced. Add TTL or file-watch. |
| 9 | `src/actors/gpu/pagerank_actor.rs`, `stress_majorization_actor.rs`, `ports/*` | `converged: bool` in per-run result structs | Algorithm completion | N/A — these are **per-run result values**, not actor state. | — | No action. Flag is not latched to actor lifecycle. |
| 10 | `src/actors/gpu/physics_supervisor.rs:298-304`, `analytics_supervisor.rs:250`, `graph_analytics_supervisor.rs:197`, `supervisor.rs:149`, `lifecycle.rs:284` | "exceeded max restarts — permanently failed" | Restart count beyond policy | Only `restart_window` elapsed reset (some supervisors do this at 292-295); others do not. | HIGH | Ensure **every** supervisor resets the counter after `restart_window` elapses. `gpu/physics_supervisor.rs` does reset (292), but `analytics_supervisor.rs` and `graph_analytics_supervisor.rs` paths need audit. |
| 11 | `src/services/speech_voice_integration.rs:240-242`, `src/utils/network/retry.rs` (`max_attempts`) | Retry exhaustion | Fixed count | Returns error — caller may or may not retry. | LOW | Correct — each call gets a fresh budget. |
| 12 | `src/events/bus.rs:66-99`, `events/middleware.rs:247-300`, handler `max_retries()` | Per-event retry caps | Per-invocation | Each event is independent. | LOW | Correct. |
| 13 | `src/utils/mcp_client_utils.rs:77-205`, `mcp_tcp_client.rs`, `mcp_connection.rs` | Connection-retry caps | Per-connect | Next connect attempt gets fresh budget. | LOW | Correct. |
| 14 | `src/actors/task_orchestrator_actor.rs:95-127` | Task-creation retry loop | Per-call | Each task gets fresh budget. | LOW | Correct. |
| 15 | `src/utils/unified_gpu_compute/community.rs:189-191` | Async Louvain runs `max_iterations` without convergence check, logged as `warn!` | Every async run | N/A (per-run) | MEDIUM (silent near-failure) | The warning at 191 fires every invocation — consider a flag in the result structure so upstream can tell "converged vs ran-to-cap", matching the fix pattern recommended for #1. |
| 16 | `src/actors/physics_orchestrator_actor.rs:166`, `gpu_init_started_at` watchdog (408-416) | 30-s stuck-init timeout | Auto-clears | Watchdog timer. | — | **Reference pattern** — treat as the template for fixing #1 and #4. |

---

## 3. Silent-Failure Scorecard (ranked by user-visible impact)

| Rank | Instance | User-visible symptom when stuck | Time-to-detect without restart |
|------|----------|--------------------------------|--------------------------------|
| 1 | **#1 — `fast_settle_complete` iteration cap** | Graph freezes mid-layout, broadcast says "physics paused", looks identical to success. | ∞ (never) |
| 2 | #4 — `gpu_self_init` permanent give-up | CPU fallback, quiet performance degradation, no visible error to client. | ∞ |
| 3 | #10 — supervisor `permanently failed` in non-resetting supervisors | Subsystem silently offline until process restart. | ∞ |
| 4 | #7 — equilibrium counter re-pausing on resume | Resume "works" for one tick then re-pauses. | Noticeable within seconds; self-heals on param change. |
| 5 | #5 — `WorkspaceActor.initialized` | Workspace state stale after out-of-band edit. | Until next write. |
| 6 | #15 — Louvain async no-convergence | Lower-quality clustering; warning in logs but no user signal. | Per-call (not latched). |
| 7 | #8 — ontology graph_cache staleness | Stale ontology graph until signature change. | Depends on usage. |

---

## 4. Recommended Architectural Pattern

Introduce a lightweight **`LatchedState<T>`** helper (or apply the rules below manually) for every long-lived flag on an actor:

1. **Two flags, not one.** If "reached-goal" and "gave-up" converge on the same state, split them. Clients and logs should never confuse the two.
2. **Distinct broadcast.** Each terminal transition emits a typed event (`PhysicsSettled { converged: bool, reason: … }`). Never re-use the same WS message for both.
3. **Watchdog or TTL.** Any flag that short-circuits a tick must be paired with either:
   - a wall-clock timer that re-evaluates the precondition (see `gpu_init_started_at` @ 408), or
   - a TTL after which the flag self-clears and the loop retries once.
4. **Reset-on-config-change hook.** All long-lived latched flags must be enumerated in a central `reset_transient_state()` that runs on `UpdateSimulationParams`, `SetGraphData`, `Resume*`, and supervisor respawn. Today this is open-coded in three places (lines 1397-1417, 738-755, 1644-1672); factor it out.
5. **Pair every cap with a recovery path.** Any `>= max_*` branch must either (a) schedule a retry with backoff, (b) surface a typed error upstream, or (c) decay the counter over time. "Permanently failed" is only acceptable for supervisor-level policies that have a restart-window reset (supervisor.rs:292-295 is the correct template).
6. **Centralise `is_physics_paused` mutation.** Today it is written in ≥7 places. Make it a method that always zeros the equilibrium counter and emits the paired broadcast; this eliminates #7 entirely.
7. **Log severity conventions.** `info!` for success, `warn!` for cap-hit / gave-up, `error!` for permanent failure. The seed bug logs cap-hit at `info!` — invisible in normal operation.

---

## 5. Files Worth a Full Follow-Up Audit

Absolute paths for downstream work:

- `/home/devuser/workspace/project/src/actors/physics_orchestrator_actor.rs` (instances #1, #2, #3, #6, #7)
- `/home/devuser/workspace/project/src/actors/gpu/force_compute_actor.rs` (instance #4)
- `/home/devuser/workspace/project/src/actors/workspace_actor.rs` (instance #5)
- `/home/devuser/workspace/project/src/actors/ontology_actor.rs` (instance #8)
- `/home/devuser/workspace/project/src/actors/gpu/analytics_supervisor.rs` and `/home/devuser/workspace/project/src/actors/gpu/graph_analytics_supervisor.rs` (instance #10 — confirm restart-window reset)
- `/home/devuser/workspace/project/src/utils/unified_gpu_compute/community.rs` (instance #15)
- `/home/devuser/workspace/project/src/actors/supervisor.rs` (reference pattern for #10)

No fixes applied.
