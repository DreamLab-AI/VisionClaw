---
title: VisionClaw Current Architecture Baseline
doc_id: VC-BASELINE
version: 0.2.1
status: draft-for-ratification
verified_commit: 
date: 2026-08-31
sources:
  - Cargo.toml
  - docker-compose.unified.yml
  - src/app_state.rs
  - src/main.rs
  - src/actors/graph_service_supervisor.rs
  - src/actors/gpu/mod.rs
  - src/actors/gpu/gpu_manager_actor.rs
  - src/services/github_sync_service.rs
  - src/middleware/rbac_gate.rs
  - src/services/role_store.rs
  - src/middleware/public_demo.rs
  - crates/visionclaw-protocol/src/lib.rs
  - crates/visionclaw-protocol/src/protocols/binary_settings_protocol.rs
  - src/utils/binary_protocol.rs
changelog:
  - "0.2.1 (2026-09-06): Remediation — 2026-09-05 section: Wave 3 ADRs (2094–2101, 2061, 2071, 2085; proposed 2102–2105) and the ledger/diagram re-verification landed in 2cf222406 — re-verified at "
  - "0.2.0 — Data pipeline: the authored corpus is an Obsidian vault (docs/VAULT-corpus-format.md); inclusion gate is frontmatter `public: true` or `owl-class` (ADR-2040), superseding the Logseq `public:: true` property line."
  - "0.1.1 — Corrected wire-protocol citation: WireNodeDataItemV3 struct is defined in src/utils/binary_protocol.rs (webxr crate), not the visionclaw-protocol lib.rs doc-comment."
---

# VisionClaw Current Architecture Baseline

## Purpose

Ground-truth map of what actually runs today: the effective stores, processes,
actor topology, crate layout, data pipeline, GPU/XR/client surfaces, and trust
boundaries. Code wins over legacy ADR prose; every load-bearing claim cites
`file:line` at commit `73540faa0`.

## Current State

### Stores (effective persistence)

Persistence is embedded, not networked. Neo4j is fully removed — no `neo4rs` or
`neo4j` dependency exists in `Cargo.toml` (grep returns nothing; legacy ADR-132).

- **Oxigraph** (RocksDB-backed SPARQL 1.1 quad store) is the canonical knowledge
  graph + ontology store. `oxigraph = { version = "0.4" }`, `Cargo.toml:80`;
  default features include `persistence-oxigraph`, `Cargo.toml:249,268`. Opened
  once at `data/oxigraph` and shared by both repositories:
  `OxigraphOntologyRepository::open(...)` then
  `OxigraphGraphRepository::from_store(store.clone())` — `src/app_state.rs:449-456`.
- **SQLite** carries all non-triple state, one DB file per single-writer domain
  under `data/`:
  - `settings.sqlite3` — settings + RBAC role store, `src/app_state.rs:459-465`.
  - `enrichment.sqlite3` — durable `EnrichmentProposal` lifecycle, isolated writer,
    `src/app_state.rs:472-481`.
  - `liveness.sqlite3` — liveness/canary rows, `src/app_state.rs:487-491`.
  - `kpi.sqlite3` — KPI snapshot + lineage, `src/app_state.rs:517-522`.
- **Storage root**: `DATA_DIR` (default `./data`), mounted from the Docker named
  volume `visionclaw-data:/app/data` (`docker-compose.unified.yml:95,190,303`);
  `DATA_ROOT=/app/data` in production, `docker-compose.unified.yml:172`.

### Processes and ports

Single application container `visionclaw_container`
(`docker-compose.unified.yml:47`) running the Actix backend plus a Vite dev
server behind nginx:

- **actix backend** — `SYSTEM_NETWORK_PORT` default `4000`
  (`docker-compose.unified.yml:6`); health `GET /api/health`
  (`docker-compose.unified.yml:24,137`). Started via `HttpServer::new`,
  `src/main.rs:852`.
- **nginx** — dev `3001` (`DEV_NGINX_PORT`, `docker-compose.unified.yml:126`);
  production readiness `GET /readyz` on `3001` (`:211`).
- **Vite** — dev server `5173` (`VITE_DEV_SERVER_PORT`,
  `docker-compose.unified.yml:63`), proxying the API on `4000` (`:64`).

### Crate layout

The monolith split (legacy ADR-090) is **real but incomplete**. The root binary
is renamed `visionclaw-server` (`Cargo.toml` `[package] name = "visionclaw-server"`)
with `src/lib.rs` + `src/main.rs`. The workspace declares **twelve** members
(`Cargo.toml` `[workspace].members`) — the root `"."` plus eleven `crates/`:

`visionclaw-contracts`, `visionclaw-domain`, `visionclaw-protocol`,
`visionclaw-adapters`, `visionclaw-gpu`, `visionclaw-ontology`,
`visionclaw-actors`, `visionclaw-xr-presence`, `visionclaw-analytics-oracle`,
plus `vault-migrate` and `visionclaw-integration-tests`, added since this doc's
original root-plus-nine census (`ADR-2005` re-verification, 2026-09-05). The empty
orphan dir `graph-cognition-extract`, never a member, has been removed.
`xr-client/rust` (Godot gdext, Quest APK cdylib) and `agentbox/crates/headroom-napi`
are workspace-**excluded** so a server build does not compile them.

**Actor extraction is unfinished**: `src/actors/*.rs` holds **23** actor
source files while `crates/visionclaw-actors/src` holds **11** counted
recursively (4 at the top level — `lib.rs`, `supervisor.rs`, `voice_commands.rs`,
`protected_settings_actor.rs` — plus 7 under `messages/`). The live supervisor
tree still runs from `src/actors/`, not the crate. The `src/` count was 25 until
`ADR-2045` deleted the two dead supervision files; note the crate still carries
its own copy of `supervisor.rs`, which the root crate no longer has.

### Actor system topology

Two independent Actix supervision trees, both spawned from `AppState::new`
(`src/app_state.rs`).

**Graph/service tree** — root `GraphServiceSupervisor`
(`src/actors/graph_service_supervisor.rs:422`, `impl Actor` at `:1281`), started
at `src/app_state.rs:807-809` over the Oxigraph-backed repository. It self-wires
its children in `initialize_actors` and owns the position-broadcast path:
`GraphStateActor` (`graph_state_actor.rs:834`) →
`PhysicsOrchestratorActor` (`physics_orchestrator_actor.rs:1084`) →
`ClientCoordinatorActor` (`client_coordinator_actor.rs:1151`) for WebSocket push
(`graph_service_supervisor.rs:2010,2025`). The live `ClientCoordinatorActor` is
started directly in `AppState` (`src/app_state.rs:709`) and rebound into the
supervisor via `SetClientCoordinatorAddr` (`src/app_state.rs:944` region) because
the supervisor's own child has an empty client registry. Peer actors started in
`AppState`: `MetadataActor` (`:802`), `AgentBeamActor` (`:719`, fans 0x23 agent
frames), plus `OntologyActor`, `SemanticProcessorActor`, `MetadataActor`,
`OptimizedSettingsActor`, `ProtectedSettingsActor`, `WorkspaceActor`,
`PresenceActor`, `TaskOrchestratorActor`, `ElevationActor`,
`DecisionElevationActor`, `VoiceInterfaceActor`,
`AgentMonitorActor` (each `impl Actor` in the corresponding `src/actors/*.rs`).
`SemanticProcessorActor` is started by `GraphServiceSupervisor`
(`src/actors/graph_service_supervisor.rs:719`), not by `AppState`; `MultiMcpVisualizationActor` is declared in `src/actors/mod.rs` but started nowhere in `src/` (2026-09-06 grep) — a removal candidate for the next dead-code pass.

`GraphServiceSupervisor` is now the **only** supervision mechanism. Two others existed and both
were dead; `ADR-2045` removed them. A generic restart supervisor (`SupervisorActor`, formerly
`src/actors/supervisor.rs`) carried backoff, restart-window caps and a
drain/`InitiateGracefulShutdown` path, but `SupervisorActor::new` was called only from its own
`#[cfg(test)]` module, `InitiateGracefulShutdown` was never sent anywhere in `src/`, and its
`Escalate` arm only logged because the type carried no parent field. Its one non-test coupling was
`GraphServiceSupervisor`'s `parent_supervisor` field, settable only by `SetParentSupervisor` —
which nothing ever sent, so the field was permanently `None` and the escalation branch always took
the stop path; field, message, handler and branch were removed with the file, and `Escalate` now
states that it is the top of the tree. A second, fully independent mechanism,
`ActorLifecycleManager` (formerly `src/actors/lifecycle.rs`) with its own
`PhysicsOrchestratorActor`/`SemanticProcessorActor` pair,
`initialize_actor_system`/`shutdown_actor_system` and a health monitor, was never called from
anywhere and was removed entirely.

**GPU tree** — root `GPUManagerActor` (`src/actors/gpu/gpu_manager_actor.rs:57`,
`impl Actor` at `:140`), started at `src/app_state.rs:965` when GPU is enabled.
Refactored from a "God Actor" into a coordinator that spawns four subsystem
supervisors (`gpu_manager_actor.rs:88-121`), documented in `gpu/mod.rs:1-33`:

- `ResourceSupervisor` → `GPUResourceActor` (GPU init, timeouts).
- `PhysicsSupervisor` → `ForceComputeActor`, `StressMajorizationActor`,
  `ConstraintActor` / `OntologyConstraintActor`, `SemanticForcesActor`.
- `AnalyticsSupervisor` → `ClusteringActor`, `AnomalyDetectionActor`,
  `PageRankActor`.
- `GraphAnalyticsSupervisor` → `ShortestPathActor`, `ConnectedComponentsActor`.

Subsystems receive `SharedGPUContext` via a decentralised `GPUContextBus`
broadcast (`gpu/context_bus.rs`), not a central handle.

### Data pipeline (GitHub → Oxigraph → client)

`GitHubSyncService` (`src/services/github_sync_service.rs`) synchronises markdown
from the source repo into Oxigraph. The source is the authored **Obsidian
vault** — plain markdown with YAML frontmatter, specified in
[`VAULT-corpus-format.md`](VAULT-corpus-format.md), synced from the GitHub repo
(still literally named `jjohare/visionGraph`) with base path `pages/`. A page is
ingested as a KG node iff its frontmatter carries `public: true` **or** a
non-empty `owl-class` (formal data bypasses the publish gate); absence of both
is private, fail-closed. Legacy Logseq property lines (`public:: true`,
`owl:class::`) are tolerated only in a page's leading property block for the
bounded window named in ADR-2040. The gate anchors on parsed metadata, never on
the file path. The live ingest path extracts JSON-LD blocks
and writes **quads** through `OxigraphOntologyRepository` — `sync_graphs()`
(`:264`) / `sync_graphs_with()` (`:272`), `insert_quads_to_store()` (`:1085`);
module header `:4-6`. It clears then repopulates the store, resolves bridge edges
in a final pass (`:341-352`), rebuilds the OWL assert graph
`urn:ngm:graph:ontology:assert` (`:917`), and runs reasoning + persists inference
via `store_inference_results()` (`:1147`). The service is wired to
`GPUManagerActor` so `POST /api/admin/sync` can dispatch semantic constraints to
the GPU (`src/main.rs:449-457`; registered `src/app_state.rs:990-996`). Positions
then flow GPU → `ForceComputeActor` broadcast → `PhysicsOrchestratorActor` →
`ClientCoordinatorActor` → WebSocket binary.

### GPU physics pipeline

Dual Rust/CUDA `SimParams` (212 bytes, static assertions; legacy ADR-138/141).
`ForceComputeActor` runs force integration and drives the periodic
full-position broadcast; the SSSP result map (`node_sssp`,
`src/app_state.rs:695-698`) feeds wire slot 28. Ontology constraints
(legacy ADR-098) are wired and live via `ENABLE_CONSTRAINTS`. DAG-rank radial
layout is live (commit `73540faa0`, "hierarchical" edge label). Analytics
kernels are Partial — see divergences.

### Live wire protocol

The canonical node-data encoder is `src/utils/binary_protocol.rs` in the webxr
crate. One **52-byte** `WireNodeDataItemV3` frame, version byte `0x03`,
`centrality@48`, stride 52 (`struct WireNodeDataItemV3` at
`src/utils/binary_protocol.rs:41-51`, `WIRE_V3_ITEM_SIZE == 52` at `:72-82`).
The `visionclaw-protocol` crate's own `binary_protocol` module was removed as a
stale 48-byte copy; its `src/lib.rs:11-24` doc-comment now only points to the
webxr encoder as the single source of truth. An optional **V5 envelope**
`[0x05][u64 broadcast_seq][V3 body]` is emitted/parsed alongside `0x03`
(`crates/visionclaw-protocol/src/protocols/binary_settings_protocol.rs:222,249,350,378`).
Separate `0x23`/`0x43`/`0x44` frame types carry agent/other channels. Legacy
ADR-061 ("28B") and ADR-031 ("48B") are both stale; the V5 envelope has no
owning ADR (the Protocol Registry becomes its owner).

### XR + React clients

- **XR**: Godot 4 gdext client (`xr-client/rust`, separate workspace, Quest APK
  cdylib) forced to `gl_compatibility` — Vulkan multiview black-screens on
  SteamVR/Linux/NVIDIA (legacy ADR-136); glow/bloom off, Linear tonemap. 90fps
  validated only on VIVE + dual RTX 6000; Quest 3 unmeasured, APK unbuilt.
  Presence via `visionclaw-xr-presence` + `PresenceActor`
  (`src/actors/presence_actor.rs:380`).
- **React/web**: Vite client (`client/`) served through nginx, consuming the
  binary WebSocket position stream and the `/api` REST surface.

### Trust boundaries

RBAC is live (legacy ADR-142): Owner > Admin > Editor > Viewer on NIP-98 pubkeys,
enforced by `RbacGate` middleware (`src/middleware/rbac_gate.rs`), role store in
`settings.sqlite3` (`src/services/role_store.rs`). **Shipped posture is open by
default via compose**: the structural code default for `RBAC_PUBLIC_READS` is
`false` (`rbac_gate.rs:122-128`, `unwrap_or(false)`), but `docker-compose.unified.yml:93`
sets `RBAC_PUBLIC_READS=1`, `:94` sets `RBAC_ALLOW_OWNERLESS=1`, and an unassigned
authenticated pubkey resolves to `Editor` — the resolution is
`UserRole::default_authenticated()` (`src/models/rbac.rs:70`), reached from
`parse_default_role` for both unset and empty (`role_store.rs:197-198`).
`PUBKEY_VISIBILITY_FILTER` defaults `1` in compose (`:107`); the code-level demo
read-only guard defaults off (`public_demo.rs:29-33`, `unwrap_or(false)`).

## Known divergences and open items

- **Security fixes landing 2026-08-31** (write-up reflects post-fix state):
  (1) `PUBKEY_VISIBILITY_FILTER` default flipped ON (encoder existed but was
  inert); (2) NIP-98 single-use event-id replay cache added in
  `src/utils/nip98.rs` (was ±60s window, no replay protection); (3) agentbox AoE
  `:9095` gains token auth (was `--auth none`), staged for next image rebuild.
- `?token=` accepted on `/wss` in release, contradicting legacy ADR-011 — medium,
  log-hygiene; header path also exists.
- NIP-26 delegation not wired — `nostr_bridge.rs` re-signs under the bridge key;
  fail-closed NIP-26 deferred (legacy Phase 5). Pod signing can fall back unsigned
  (agentbox ADR-026).
- **Actor extraction incomplete**: 25 `src/actors/*.rs` vs 11 in
  `crates/visionclaw-actors`. Live tree runs from `src/`.
- **BrokerActor never merged**; main uses a stateless ACSP producer + a
  cherry-picked storage-agnostic domain broker kernel (~936 LOC).
- **GPU analytics Partial**: Louvain (`sigma_tot` race → converges ~0), DBSCAN
  (border points → noise), PageRank (dangling-kernel block bound bug); LOF fixed.
  Node embeddings (legacy ADR-072) are effectively random (hash bag-of-characters).
- **Sources-of-truth conflict**: Oxigraph (132) vs Pod write-master (050/052) vs
  GitHub `public::true` (051) vs RuVector agent memory (030). `deleteAgentMemory()`
  has no reverse tombstone to RuVector — deletion does not revoke agent memory.
- **No estate-wide erasure/backup design**: `scripts/backup-sqlite.sh`
  (2026-08-31) covers SQLite only; no point-in-time Oxigraph backup, no
  cross-store consistent restore, no RPO/RTO.
- **Identifier grammars unreconciled** (vc:, urn:visionclaw:, visionclaw:owner:,
  hex/npub), DID doc ADR-074-D2' vs ADR-125 conflict.
- **SOPS** (legacy ADR-109) accepted 2026-05-09, never executed — `.env`
  plaintext today, no SOPS artifacts.

## Invariants (must not silently change)

- Persistence is Oxigraph (`data/oxigraph`) + per-domain SQLite files under
  `DATA_DIR`; a single Oxigraph store is shared by ontology and graph
  repositories. No networked graph DB.
- Live node wire format is 52-byte V3 (`0x03`), optional V5 envelope
  (`0x05` + `u64 seq` + V3 body). Any width change is a protocol break.
- Position broadcast path: GPU → `ForceComputeActor` → `PhysicsOrchestratorActor`
  → `ClientCoordinatorActor` → WebSocket. The live `ClientCoordinatorActor` must
  be the one clients register with (registry not empty).
- Backend on `4000`, nginx `3001`, Vite `5173`, all inside `visionclaw_container`.
- RBAC open-by-default is a compose-env choice, not a structural code default;
  changing the compose defaults changes the security posture and requires a named
  security profile.

## Change process

This is a living document. Update it in the same PR as any change to store
topology, actor tree, wire protocol, port map, or trust boundary. Re-verify
`verified_commit` and cited `file:line` anchors on each edit; bump `version`
(semver: patch for citation refresh, minor for new subsystem, major on invariant
change). Historical rationale stays in the legacy corpus
(`docs/adr`, `docs/prd`, `docs/ddd`) — cite, do not narrate. Ratification promotes
`status` from `draft-for-ratification` to `ratified`.

## Persistence closeout extension — 2026-09-04

ADR-2004 retains the embedded shared-store decision and now names its CP-01/06/08
closeout boundary: shared Oxigraph ownership does not establish a cross-store
transaction, actor reload consistency or restore correctness. See the current
[graph runtime evidence](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/visionclaw-data-runtime.md)
and the data-authority governing document for sync, provenance and backup limits.

## ACSP workflow closeout — 2026-09-04

ADR-2006 distinguishes the forum event surface from stateful consumers and durable case reconciliation. The retained domain kernel's presence does not prove integration into the elevation actor or inbox DTO. Require signed-event/request correlation, case authority and failure/restart receipts through gate and PR outcomes. See [estate ACSP review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/forum-decisions.md#visionclaw-acsp-consumption-and-recovery). Current source review does not certify a complete human-approval journey.

## Crate and supervision closeout — 2026-09-04

ADR-2005 remains partial; the current workspace adds converter and integration-test members to the historical census. ADR-2007 is partial: four supervisors exist, but context delivery uses direct messages plus optional bus publication. Require responsibility/dependency acceptance, acknowledged context generations and failure/restart evidence. See [estate architecture](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/vision-and-architecture.md#server-extraction-and-enforceable-boundaries) and [supervision review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md#gpu-supervision-and-context-delivery).

## Development and corpus acceptance — 2026-09-04

ADR-2008 is partial: normal development startup uses a timestamp-gated supervisor wrapper, with demonstrated misses for crate CUDA and manifest edits. ADR-2001 retains partial/staged corpus status despite complete closeout-extension coverage of the operative series. Four stale baselines still fail the current validator. See [development inputs](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/configuration-projection.md#development-restart-and-build-input-coverage) and [corpus debt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/canon-and-verification.md#visionclaw-operative-pack-coverage-and-baseline-debt).

## Remediation — 2026-09-05

- ADR-2045 — removed both dead supervision mechanisms. `src/actors/lifecycle.rs` (`ActorLifecycleManager`, `initialize_actor_system`, `shutdown_actor_system`) had no coupling and went first. `src/actors/supervisor.rs` (the generic `SupervisorActor`, `ActorFactory`, `SupervisedActorTrait`, `SupervisionStrategy`, `ActorFailed`, `InitiateGracefulShutdown`) followed once its one non-test coupling was resolved: `GraphServiceSupervisor` held a `parent_supervisor: Option<Addr<SupervisorActor>>` field settable only by `SetParentSupervisor`, which nothing ever sent — so the field was permanently `None` and the `Escalate` branch always took the stop path. The field, the message, its handler and the unreachable escalation branch were removed with it, and `Escalate` now states plainly that it is the top of the tree. `GraphServiceSupervisor` is the sole supervision path.
- ADR-2046 — removed the dead `SettingsActor` (`src/settings/settings_actor.rs`, never started outside its own disabled test), the already-disabled `src/handlers/tests/settings_tests.rs`, and six orphaned `src/config/*.rs` copies (`field_mappings.rs`, `physics.rs`, `services.rs`, `system.rs`, `validation.rs`, `xr.rs`) not declared in `src/config/mod.rs`; `OptimizedSettingsActor` and `crates/visionclaw-domain/src/config/*` remain the sole live definitions.
- ADR-2043 — the ADR-2003 full-disclosure flag pair (`RBAC_PUBLIC_READS=1` with `PUBKEY_VISIBILITY_FILTER=0`) is now rejected at boot on its own terms, unconditionally, rather than only as drift from a *declared* profile. The shipped `demo-open` compose posture is unaffected and a test asserts that for all three ratified profiles.
- ADR-2044 — session credentials fail closed and expire identically on every transport. `NostrService::get_session` enforced no expiry at all while `validate_session` did, so a WebSocket token outlived its REST equivalent indefinitely; both now share one `session_is_fresh` rule. `/ws/client-messages` resolves its token through the session realm instead of checking non-emptiness. Partial: the same defect in `mcp_relay_handler` and `multi_mcp_websocket_handler` is routed to the estate lead.
- ADR-2047 — settings-change broadcasts are emitted from one function and `physics` now emits, closing the per-category asymmetry. Recorded, but not yet useful: no client consumes `settingsUpdated` at all (the React validator knows `settings_update`), so the channel is dead end-to-end until vc-clients adds a handler.
- ADR-2049 — the production image's dependency-warming stage can fail again. `cargo build --release || true && cargo build --release --lib || true` could not fail for any reason, so a broken lockfile or an uncompilable dependency produced a green layer; the stage now gates on `cargo fetch --locked` and tolerates only the crate compile, which legitimately fails against the stub `build.rs`.
- ADR-2048 — re-verified every `file:line` in the sections above. Three compose citations pointed at unrelated lines (`RBAC_PUBLIC_READS` was cited at `:78`, a comment; the real lines are `:93`, `:94`, `:107`), the Editor default was cited at a doc-comment rather than `UserRole::default_authenticated()` (`src/models/rbac.rs:70`), the whole NIP-98 section in IDENTITY-authority-chain had drifted ~60 lines, and the crate census was stale (nine members → eleven, 25 actor files → 23). Two of these were caught by other leads reviewing my edits.
- ADR-2071 — inferred-edge materialisation is one implementation again. `GitHubSyncService::run_post_sync_reasoning` no longer hand-rolls edge selection: it calls `inferred_edge_materialiser` for the vacuous-axiom filter, the transitive-to-immediate parent reduction, asserted-pair suppression and the ≤8 parents-per-child cap (`src/services/github_sync_service.rs:1141` and `:1309`), so the live sync path and `OntologyPipelineService` now apply identical rules. The edges it writes also finally carry `metadata["inferred"]="true"`, so `edge_is_inferred` classifies sync-produced edges onto the client's inferred channel — previously they set `edge_type: "inferred"` and no metadata key, and rendered as ordinary asserted edges. Edge counts drop on deep hierarchies as intended: against real Whelk output over a 6-level chain plus a diamond, 67 entailments yielded 23 edges before and 12 after, the retained set a strict subset of the old one.
- ADR-2098 — `SOLID_POD_URL` names an endpoint that exists. Both defaults pointed at the JSS sidecar removed by ADR-032 M3 (`.github/workflows/ontology-publish.yml` had `http://jss:3030`, `env.example` had `http://visionclaw-jss:3030`); neither host resolves, so the ontology-publish deploy step could only fail DNS unless a repository variable overrode it. Both now default to `http://localhost:4000/solid` — the `/solid` scope the embedded `solid-pod-rs` actually serves on the server's own listener. The workflow's POST to `/.notifications` is annotated as a best-effort no-op on the embedded pod, where that path is a GET WebSocket upgrade, not a POST broadcast trigger.
- ADR-2101 — decision-elevation cases have a named durable case-state authority. `DecisionElevationActor` kept every open case in two in-process `HashMap`s with no repository field at all, so a crash between the kind-31404 publish and its bookkeeping silently lost an open governance decision (the ADR-2006 `partial` gap, and the VC-24.6 diagram divergence). Cases and decisions now persist through `DecisionElevationStore` (`src/adapters/decision_elevation_store.rs`) into the same `data/enrichment.sqlite3` the `ElevationActor` uses — case rows tagged `category="decision-elevation"`, decision rows carrying the ADR-2006 signed-event correlation — written durable-first on open and durable-before-in-memory on the PR stamp; at boot a pure `plan_reconciliation` reloads every non-terminal case and re-arms the merge poll, re-opens a PR whose approval crashed before it landed, re-arms a case still awaiting a human, or expires it past a 14-day TTL with a kind-31404 receipt. A case tracking an open PR is never expired. Resuming an approval can open a duplicate PR if the previous process died between creation and the durable stamp — recorded as follow-on 1 in ADR-2101, not closed.
- ADR-2110 — the VisionClaw judgment surfaces are instrumented against the augmentation conditions (PRD-augmentation-conditions, EXP-AC-002/004/005/006). `ElevationActor` gained the recovery posture ADR-2101 gave its sibling: `OPEN_CASE_TTL` of 14 days, boot reconciliation of durable `pending` rows through a pure `plan_elevation_reconciliation` before the first cycle, and an `elevation_expired` kind-31404 receipt with a terminal durable status that does not depend on the receipt publishing (`src/actors/elevation_actor.rs`). Before this a kind-31403 arriving after a restart hit an empty in-memory `pending` map and returned early — the human's signed decision was dropped and the row sat `pending` for ever. The EL++ consistency gate's synthetic rejection carried the *human responder's* pubkey, so a reasoner outcome counted as a human override; it now carries the reserved non-DID actor `system:whelk-gate`, which `kpi_compute` excludes from the Trust Variance human-outcome series and from both terms of HITL Precision. HITL Precision itself is computed rather than stubbed: warranted ÷ decided over human-decided cases with the denominator always reported and no value at a zero denominator. Declared `intent` is persisted verbatim on `kpi_agent_events` (`migrations/sqlite/0006_kpi_agent_event_intent.sql`) and compared with the act at `/api/trace` (`src/services/intent_match.rs`). `AgentContext.confidence` became `Option<f32>` so the elevation paths' hardcoded `0.5` no longer reaches a reviewer as the agent's own confidence. Follow-ons open, not closed: the rationale gate is client-side only, and the approve path still trusts relay-side admission with no in-process admin allowlist (the same deepsec `acl-check` MEDIUM that stands against `DecisionElevationActor`).
