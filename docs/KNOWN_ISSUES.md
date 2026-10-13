---
title: Known Issues
description: Active P1/P2 issues in VisionClaw — read before debugging unexpected behaviour
category: reference
updated-date: 2026-08-31
---

# Known Issues

This file tracks active production issues and design limitations in VisionClaw. Read it before spending time debugging behaviour that is already understood. Each entry includes root cause, impact, and the direction of the intended fix. Where an ADR or explanation doc exists, it is linked.

---

## P1 Issues (Production Impact)

None currently active.

---

## P2 Issues (Degraded Feature)

### AUTH-001: Enterprise SSO — Native RBAC Resolved (OIDC/SAML Still Pending)

> Resolution (2026-08-31, ADR-142): the Nostr-native multi-user RBAC half of
> AUTH-001 is now **implemented on `main`** — not the branch-only
> `enterprise_auth.rs`, but a fresh port bound to the NIP-98-verified pubkey
> (the user's DID) rather than the spoofable `X-Enterprise-Role` header. A
> persisted four-tier lattice (`Owner > Admin > Editor > Viewer`,
> `src/models/rbac.rs`) is stored per-pubkey in SQLite
> (`src/services/role_store.rs`), resolved by the existing `verify_access`, and
> enforced centrally by the `RbacGate` middleware over the whole `/api` scope
> (`src/middleware/rbac_gate.rs`) — closing the unauthenticated-mutation gap.
> Role management lives at `/api/admin/rbac/*`
> (`src/handlers/admin_rbac_handler.rs`). The **remaining** gap is only the
> enterprise-IdP federation (OIDC/SAML/SCIM, ADR-040 Phase 1/2), which stays
> parked. The four-tier `Admin > Broker > Auditor > Contributor` /
> `X-Enterprise-Role` design described below was **never adopted** — it was a
> reference only; the shipped taxonomy is `Owner/Admin/Editor/Viewer`. See
> [ADR-142](archive/adr/ADR-142-multi-user-rbac.md).

> Correction (2026-07-22 doc-drift audit): the entry below asserts that
> `src/middleware/enterprise_auth.rs` (the four-tier Admin > Broker > Auditor >
> Contributor hierarchy, `Nip98RoleResolver`, `X-Enterprise-Role`) is on `main`.
> **It is not.** That middleware lives on the `jss-cut-scaffold` branch. `main`
> carries only the NIP-98 primitives (`src/utils/nip98.rs`) and a **coarser
> `AccessLevel` enum** in `src/middleware/auth.rs` — there is no four-tier
> enterprise role hierarchy compiled into `main` today. The merge decision (adopt
> `enterprise_auth.rs` to `main` vs. leave it on the branch) is **deferred to the
> final-mile sprint tock, pending operator decision**. Read the RBAC-implemented
> claims below as branch-only. Tracked in
> [ADR-131](archive/adr/ADR-131-doc-drift-reconciliation-2026-07.md) §3 (AUTH-001).

**Status**: Native multi-user RBAC resolved-by-implementation (ADR-142, 2026-08-31); OIDC/SAML SSO federation still pending (ADR-040 Phase 1)
**Impact**: VisionClaw's enterprise RBAC middleware (`src/middleware/enterprise_auth.rs`) now supports two authentication paths: (1) NIP-98 Schnorr signature verification with pubkey-to-role resolution via `Nip98RoleResolver` (enabled by the `nip98-auth` compile-time feature), and (2) `X-Enterprise-Role` header extraction for dev/gateway deployments. The four-tier role hierarchy (Admin > Broker > Auditor > Contributor) is enforced on all enterprise-gated routes.

**Remaining gap**: Full OIDC/SAML SSO integration (ADR-040 Phase 1) is not yet implemented. Enterprise users cannot log in via Entra ID, Okta, or Google Workspace. The server-side ephemeral Nostr keypair delegation (OIDC session to secp256k1 key) described in ADR-040 remains unimplemented. SCIM provisioning (ADR-040 Phase 2) is deferred.

**Workaround**: Deploy behind a trusted API gateway that sets the `X-Enterprise-Role` header based on its own SSO verification, or enable the `nip98-auth` feature and populate the `Nip98RoleResolver` with pubkey-to-role mappings for known enterprise users.

**Fix Direction**: Implement ADR-040 Phase 1 (OIDC login flow, ephemeral keypair generation, session management). See [ADR-040](archive/adr/ADR-040-enterprise-identity-strategy.md) and [ADR-088](archive/adr/ADR-088-auth-service-extraction.md) for the auth consolidation plan.

---

### AGENT-001: RVF File Store Not Yet Implemented

**Status**: Draft PRD / Postgres-only in Production
**Impact**: The `rvf-integration-prd.md` describes replacing RuVector's PostgreSQL dependency with portable `.rvf` files for agent memory. This is **not implemented**. The current production agent memory backend is exclusively `ruvector-postgres:5432` (pgvector + HNSW). Any deployment that lacks the `ruvector-postgres` container will fail on the first `memory_store` call with a connection refused error; there is no fallback.

**Workaround**: Ensure the `ruvector-postgres` container is running and reachable at `ruvector-postgres:5432` before starting agents. Connection string is available via `$RUVECTOR_PG_CONNINFO`. Run `docker ps | grep ruvector` to confirm the container is up.

**Fix Direction**: See `docs/adr/rvf-integration-prd.md` for the full specification. Implementation is blocked on `rvf-runtime` WASM target stabilisation.

---

## Design Constraints (DO NOT CHANGE)

### BROADCAST-001: Full Position Snapshots Only — No Delta Encoding

**Status**: Permanent design constraint (PRD-007 §3, ADR-037, ADR-061)
**Rule**: The GPU→client broadcast pipeline MUST always send full absolute position+velocity snapshots for every node. Delta/diff encoding, delta-filtered partial updates, and incremental position messages are permanently prohibited.

**Reasoning**:
1. **Client tweening**: Clients interpolate `currentPositions` toward `targetPositions` at 60fps using an exponential approach with time constant `responseMs` (default 60 ms; XR `CHASE_MS`, pinned equal by a test) decay. They need complete target state for all nodes, not just the ones that moved. A delta-filtered broadcast that omits stationary nodes means the client never receives their final resting positions after physics convergence.
2. **Late-connecting clients**: New WebSocket connections miss all positions accumulated before they joined. Only full snapshots guarantee correct initial state.
3. **Convergence deadlock**: The `BroadcastOptimizer.DeltaCompressor` (delta_threshold filtering) was the root cause of the "incremental updates" bug (May 2026): after physics converges and all velocities reach zero, the delta filter excludes everything, so the client never receives final settled positions. The energy_threshold (0.001) was unreachable for 840-node graphs, causing mandatory fallback to Continuous mode with broken delta-filtered broadcasts.
4. **PRD-007 §3 Non-Goals**: "No introduction of delta/diff encoding. The original 'literal-only, full snapshot' property is preserved for the per-frame stream."

**Implementation**: `force_compute_actor.rs` Continuous mode sends full snapshots of ALL nodes at 10fps (rate-limited by `BroadcastOptimizer` timing, NOT delta filtering). FastSettle mode sends one full broadcast on convergence. Periodic full broadcast every 300 iterations as safety net.

---

## Resolved Issues

Previously known issues that are now fixed. Listed here so that old bug reports, forum threads, or git blame comments referencing these symptoms can be traced to their resolution.

| Issue | Status | Fixed In | Reference |
|-------|--------|----------|-----------|
| ONT-001: Ontology Edge Gap — 62% of ontology nodes isolated due to empty `iri_to_id` map in `neo4j_adapter.rs` | Fixed Apr 2026 | `neo4j_adapter.rs` — added `iri_to_id` population loop after KGNode loading | `docs/explanation/ontology-pipeline.md` |
| WS-001: Delta Encoding — permanently retired; caused position state divergence and latency spikes | Resolved by design | [ADR-037](archive/adr/superseded/ADR-037-binary-protocol-consolidation.md), [ADR-061](archive/adr/ADR-061-binary-protocol-unification.md) | [docs/binary-protocol.md](reference/binary-protocol.md) |
| GPU-002: Analytics actors missing SharedGPUContext — PageRank, SSSP, APSP endpoints returned "GPU context not initialized" | Fixed Apr 2026 | Forward `SetSharedGPUContext` from `PhysicsSupervisor` to `AnalyticsSupervisor` | `src/actors/gpu/` |
| PHYS-001: No graph position reset endpoint — extreme parameters could not be recovered without container restart | Fixed Apr 2026 | `POST /api/graph/reset-positions` randomizes GPU positions and triggers reheat | `src/actors/gpu/force_compute_actor.rs` |
| UI-001: Client slider init race — sliders sent values before fetching server state | Fixed Apr 2026 | Slider ranges capped; `settingsLoaded` gate added | `client/src/features/physics/` |
| UI-002: `SETTINGS_AUTH_BYPASS` not picked up by Docker Compose | Fixed Apr 2026 | Auth bypass also triggers on `DOCKER_ENV=1 + NODE_ENV=development` | `src/auth_extractor.rs` |
| Periodic full broadcast — nodes that converged before client connection never received positions; late-connecting clients saw all nodes at origin | Fixed Mar 2026 | `force_compute_actor.rs` — periodic full broadcast now fires inside the non-empty delta branch every 300 iterations, not only when all nodes are stopped | `docs/explanation/physics-gpu-engine.md` |
| PTX ISA version mismatch — CUDA kernel failed to load on drivers supporting only PTX 9.0 when nvcc 13.2 emitted `.version 9.2` | Fixed Mar 2026 | `build.rs` auto-downgrade: detects driver PTX cap at build time and passes `--gpu-architecture` accordingly | `docs/explanation/physics-gpu-engine.md` |
| Worker position data race — all edges vanished on the first rendered frame because the physics worker initialised all nodes at origin (0, 0, 0) | Fixed | `handleGraphUpdate` now returns `dataWithPositions` (with generated positions); the caller sends this to the worker instead of the original position-free `data` | `docs/explanation/client-architecture.md` |
| Edge hash dedup — edges appeared frozen after physics convergence because `updatePoints` used a 3-value hash (`len`, `pts[0]`, `pts[len-1]`) that matched even when intermediate edge endpoints changed | Fixed | Hash removed; `computeInstanceMatrices` runs unconditionally every frame | `docs/explanation/client-architecture.md` |
| CUDA thrust SM_890 error in GPU initialization — cuBlas context creation failed intermittently on Ada Lovelace GPUs | Fixed Apr 2026 | `force_compute_actor.rs` — added device synchronization before context creation and PTX module cache invalidation on arch mismatch | `src/actors/gpu/force_compute_actor.rs` |
| PTX module lookup in community.rs — clustering kernels referenced incorrect module path causing kernel dispatch failures | Fixed Apr 2026 | `src/utils/ptx.rs` — added module name mapping for `gpu_clustering_kernels`, `ontology_constraints`, `pagerank` | `src/utils/ptx.rs` |
| Slider range degeneration — max values capped at 50000 produced extreme physics parameters on first client connect | Fixed Apr 2026 | `client/src/features/physics/components/SettingsPanel.tsx` — slider ranges capped to sane defaults (repelK: 2000, centerGravityK: 10, damping: 0.98) | `docs/KNOWN_ISSUES.md` |
| Clustering visualization missing analytics — DBSCAN and Louvain results not appearing in client analytics panel | Fixed Apr 2026 (analytics moved off the per-frame wire by ADR-061) | Historical: prior wire format wrote cluster_id to node_analytics. Post-ADR-061: cluster_id rides the `analytics_update` JSON message at recompute cadence; the per-frame binary protocol carries position+velocity only. | `src/actors/gpu/clustering_actor.rs` |
| **Dual ClientCoordinatorActor instances** — `SocketFlowServer` registered clients in one actor while `PhysicsOrchestratorActor` broadcast to a second internally-created instance; result: "2241 positions, 0 clients in manager", zero binary frames delivered to any client | Fixed Apr 2026 (commit fcfc1a166) | `graph_service_supervisor.rs` — new `with_client()` constructor accepts externally-injected `ClientCoordinatorActor`; `app_state.rs` passes shared address; internal creation skipped when external instance provided | `src/actors/graph_service_supervisor.rs` |
| **ClientFilter default filtering to zero** — `ClientFilter::default()` had `enabled: true` with empty `filtered_node_ids`, causing every new client to receive zero nodes from `broadcast_with_filter` | Fixed Apr 2026 (commit fcfc1a166) | `client_filter.rs` — default changed to `enabled: false`; opt-in filtering semantics | `src/actors/client_filter.rs` |
| **FastSettle permanent physics halt** — `FastSettle` mode latched `fast_settle_complete = true` and `is_physics_paused = true` on reaching the iteration cap even without energy convergence; subsequent settings changes could not resume simulation | Fixed Apr 2026 (commit fcfc1a166) | `physics_orchestrator_actor.rs` — non-convergent exhaustion falls back to `Continuous` mode rather than halting | `src/actors/physics_orchestrator_actor.rs` |
| **Delta-filtered broadcasts caused missing positions** — Continuous mode used `BroadcastOptimizer.DeltaCompressor` to filter nodes that hadn't moved >0.01 units; after convergence ALL nodes filtered → client never received final positions; `energy_threshold` 0.001 unreachable for 840-node graphs → FastSettle always fell back to broken Continuous mode | Fixed May 2026 | `force_compute_actor.rs` — Continuous mode now always sends full snapshots at 10fps; energy_threshold raised to 1.0; max_settle_iterations raised to 10000; client lerpBase set to 0.003 for ~200ms smooth tween | See BROADCAST-001 design constraint above |
| **Boundary-pinned nodes** — 468 nodes oscillating at ±2000 (viewport bounds) on Y/Z axes; runaway rescue threshold of 10× bounds did not catch nodes already at the wall | Fixed Apr 2026 (commit fcfc1a166) | `force_compute_actor.rs` — added `boundary_stuck_frames` counter (per node); nodes at boundary for ≥60 frames are teleported to randomised interior position with zeroed velocity | `src/actors/gpu/force_compute_actor.rs` |
