# Changelog

All notable changes to VisionClaw will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### 2026-09-14 — augmentation conditions, VisionClaw substrate (ADR-2110)

The judgment surfaces are instrumented against the six augmentation conditions
of arXiv 2609.12482 (PRD-augmentation-conditions, EXP-AC-002/004/005/006). Nothing
here is deployed — `activation_status: inactive`.

- **Intent kept (FR5.1–5.2).** `kpi_agent_events.intent` persists the
  `/wss/agent-events` envelope's declared intent verbatim
  (`migrations/sqlite/0006_kpi_agent_event_intent.sql`); `None` stays `NULL` and
  is never synthesised. `/api/trace` reports `intent` and a three-valued
  `intent_match` from the new pure `services::intent_match` — `null` for "no
  claim was made" is never conflated with `false`.
- **HITL Precision is real (FR5.3).** The `awaiting_data_source` stub is gone.
  Warranted ÷ decided over human-decided cases, one row per case, denominator
  always reported, `value: None` at a zero denominator rather than a number.
- **System actors are not humans (FR5.4).** The EL++ consistency gate's own
  rejections carried the *human's* pubkey; they now carry `system:whelk-gate` and
  are excluded from the Trust Variance human series and from both HITL terms.
- **A restart no longer drops a decision (FR4.5).** `ElevationActor` gains a
  14-day `OPEN_CASE_TTL`, boot reconciliation of durable `pending` rows before
  the first cycle, and an `elevation_expired` kind-31404 receipt. Previously a
  kind-31403 arriving after a restart returned early at an empty in-memory map.
- **The case card can be judged (FR2.3–2.4, FR6.5).** Full proposal payload
  pretty-printed and unclipped; proposal URN, reasoning summary, reasoning hash
  and generation provenance shown only when present; the human's typed rationale
  published byte-for-byte, mandatory (20 chars) on `high`/`critical`; the agent's
  self-assessment moved BELOW the controls; queue sorted oldest first with an age
  badge. The fabricated `operator {outcome} via control centre` rationale is
  deleted, and `AgentContext.confidence` is `Option<f32>` so the hardcoded `0.5`
  no longer reaches a reviewer as the agent's own judgement.


### 2026-09-05 sprint, diagrams-as-code overhaul and Wave 3 remediation

- **Sprint (2026-09-05):** actor set trimmed to graph-service / agent-beam / presence (346fff7af); GPU actor and compute pipeline consolidated onto `visionclaw-gpu` (da2f5cac7); dead settings, config and physics-v1 modules removed (35c2448a8); explicit runtime security profile and RBAC gate (ac3e12dd1); Oxigraph/SQLite repository and vault-migrate (b47db377c); agent-visualisation, provenance and vault services (1b513295a); backup-posture and dev-build-input contract tests (ca80eafa5); client websocket/binary-protocol and settings surface (1d68d8eb1); xr-client render-store, transport and HUD (f21d30922).
- **Diagrams as code:** `docs/diagrams/{visionclaw,agentbox,estate}` — 71 topic files, 841 mermaid diagrams, every fact a `path:line` citation, gated by `scripts/diagram-index-gen.js` (structure and mmdc render). Phase 2 ADRs 2043–2093 remediated the findings the diagrams exposed.
- **Wave 3 (2026-09-05/06, 2cf222406):** fail-closed `MANAGEMENT_API_KEY` in `AgentMonitorActor` and a cached health verdict (ADR-2094); typed `urn:ngm:class` mint (ADR-2095); vault reader through `vault::parse` (ADR-2096); dead `RefreshMetadata` deleted (ADR-2097); `SOLID_POD_URL` names the in-process pod (ADR-2098); dead V4 0x23 client branch removed and one Solid socket client (ADR-2099/2100); durable decision-elevation state with boot reconciliation (ADR-2101); inferred edges through the shared materialiser and now carrying the `inferred` key (ADR-2071); GPU analytics kernels measured against the CPU oracle — PageRank, DBSCAN, Louvain trusted, LOF broken and recorded (ADR-2061); briefing workflow integration tests (ADR-2085); federation kind map from one shared artefact. Proposed: cross-store erasure orchestration (2102), Oxigraph point-in-time backup (2103), SOPS execute-or-withdraw (2104), correlated promotion chain (2105).
- **Build:** three test targets that no longer compiled at HEAD and two stale integration tests fixed test-side; `cargo fmt --all` (57 files); `vault-migrate` and the hermetic integration contracts added to CI; ESLint errors 25 → 0.
- **Ledger:** 22 stale records re-verified with citations re-derived after formatting; ADR-2038 promoted; ADR-2045 complete; the actor-supervision and external-services diagrams redrawn against surviving code; 360 diagram `sources:` entries added.

### XR immersive-interaction + constrained-layout programme (ADR-137–141)

Landed the week of 2026-08-31 across the Godot/OpenXR client, the GPU layout
engine, and the REST/wire surfaces. New endpoint families are catalogued in
`docs/reference/rest-api.md`; RenderStore + force-channel internals in
`docs/reference/render-store-and-force-channels.md`.

#### Added

- **XR render offload + runtime quality dials (ADR-137).** Per-frame position
  hunt and MultiMesh buffer packing moved from GDScript to a pure-Rust
  `RenderStore` (`xr-client/rust/src/render_store.rs`), exposed via
  `BinaryProtocolClient` — full density (13,164 nodes / 145,692 edges) at 90 fps.
  Draw budgets derived from received topology (was hardcoded 640/3000);
  initial-load quality is now the `initialNodeLimit` settings dial on
  `/api/settings/physics`; WebSocket receive cap raised to 256 MiB. Full-3D force
  layout is the default (`axisCompressionZ` removed; dual-disc flatten opt-in via
  `enableDualDiscLayout`, default OFF).
- **V5 wire protocol.** Client decodes the V5 position wrapper
  (`[0x05][u64 broadcast_seq][V3 records]`); see `docs/reference/binary-protocol.md`.
- **GPU force-channel registry + pinned-node mask (ADR-138).** Bounded
  `ForceChannel` enum (`src/models/force_channels.rs`) mapping each named channel
  to existing `SimParams` scalars + feature-flag bits (mapping-layer seam toward a
  later array-backed `SimParams`); adds the pinned-node bitmask and DAG
  radial-bias channel, with zero change to struct layout, CUDA kernels, or the
  settings wire.
- **Graph2VR-class immersive interaction (ADR-139).** Two-hand pinch
  scale/rotate, radial menu, in-graph search, node expansion API — adopted into
  the Godot/OpenXR client as ideas-level / clean-room re-implementations against
  VisionClaw's own RenderStore, CUDA kernels, and Graph V3 wire (no external code
  or assets vendored).
- **Agent-swarm visualisation in XR (ADR-140, P1 shipped).** Consumes the
  `0x23 AGENT_ACTION` frames (`AgentBeamActor`/`BeamCoalescer`): embodied agent
  capsules hover near their target node, directional work beams, 4-channel status
  halo, and a HUD Swarm tab with tap-to-teleport. Server/XR motion-authority
  split means zero server change; all per-frame cost stays in the RenderStore.
- **Constrained-layout engine programme (ADR-141, P1–P4 complete).** 13-pattern
  taxonomy: Sugiyama layers, stratified planes, spherical shells, ego-radial
  `RadialModes`. New endpoints `POST /api/layout/mode`, `POST /api/layout/radial`.
- **Visual query builder** — `POST /api/graph/query/pattern` (graph-pattern query
  over the semantic planes).
- **Fold ladder** — `GET /api/graph/fold` (hierarchical-density fold levels;
  application layer is the ADR-137 RenderStore).

### Judgment Broker: kernel ported, `BrokerActor` transport superseded (gap-close REC-2 — ADR-130 Decision 2)

Branch `gap-close/2026-07`. Closes the REC-2 register gap on the architecture
`main` actually committed to (ADR-110 ACSP), correcting five documents that
described the unmerged `crashbug` `BrokerActor` as live `main` infrastructure.

#### Added

- **Storage-agnostic broker domain kernel** cherry-picked from `crashbug` to
  `src/domain/broker/` (`BrokerCase` aggregate, `DecisionOrchestrator`,
  six-variant `DecisionOutcome`, `PrecedentRegistry`; 21 kernel tests). The
  kernel is the domain model behind the enrichment REST fallback and the ACSP
  producer — the decision invariants (append-only history, no self-review,
  terminal idempotency, forward-only share-state) live here.
- **`broker:new_case` / `broker:case_decided` WebSocket events** emitted from the
  enrichment-decide handler over the multiplexed `/wss` graph socket
  (`services::broker_events`), so a control-centre case queue can subscribe
  without a second transport.
- **ACSP↔kernel vocabulary reconciliation** locked by tests in
  `services::acsp::events` (kernel `CaseCategory`/`SubjectKind` serde forms are
  byte-identical to the ACSP producer's tag values; `ActionResponse` parses into
  a kernel `DecisionOutcome`).
- **`CANARY-VC-REC2-CASE`** now fires from the decide path when a queued case
  reaches a decision over live traffic (observed traffic only, never a probe).

#### Changed

- **`ElevationActor` (the ACSP consumer) defaults ON in dev/staging**, opt-in in
  production (`ELEVATION_ACTOR_ENABLED` still overrides; also requires
  `FORUM_RELAY_URL` + a panel secret to publish).

#### Corrected (documentation)

- `BrokerActor` was described as live `main` infrastructure in ADR-033, ADR-041,
  `docs/explanation/ecosystem-convergence.md` and `docs/reference/rest-api.md`.
  It never merged from `crashbug` and was tied to a Neo4j store this stack does
  not run (Oxigraph + SQLite). Those documents now describe the ADR-110 ACSP
  producer + ported kernel that actually ships; ADR-041 is marked
  superseded-in-part.

## [Unreleased] — dated entries merged from `docs/CHANGELOG.md` (2026-09-06)

The documentation tree carried a second, diverged changelog from 2026-02 to 2026-08-31.
Its dated `[Unreleased]` blocks are preserved here verbatim; `docs/CHANGELOG.md` is now a
redirect to this file. Versioned releases below are the root history and were not merged.

> **Correction (gap-close REC-2, branch `gap-close/2026-07`, 2026-07-08).** Entries
> below attribute governance-panel and case events to a `BrokerActor` in
> `src/actors/broker_actor.rs`. That actor never merged to `main` (it lives only
> on the unmerged `crashbug` branch, tied to a Neo4j store this stack does not
> run). Per ADR-130 Decision 2, `main` ships ADR-110's ACSP producer
> (`ElevationActor` over the ported `src/domain/broker/` kernel), and the
> `broker:new_case` / `broker:case_decided` WebSocket events are emitted from the
> enrichment-decide handler (`services::broker_events`) over the multiplexed graph
> socket. Read "`BrokerActor`" below as "the broker governance publisher".

## [Unreleased] - 2026-08-31

### Added — XR immersive-interaction + layout programme (ADR-137–141)

The week's landing across the Godot/OpenXR client, the GPU layout engine, and
the REST/wire surfaces. New endpoint families are documented in
`docs/reference/rest-api.md`; the RenderStore and force-channel registry have a
dedicated developer reference at
`docs/reference/render-store-and-force-channels.md`.

- **XR render offload + runtime quality dials (ADR-137).** Per-frame position
  hunt and MultiMesh buffer packing moved out of GDScript into a pure-Rust
  `RenderStore` (`xr-client/rust/src/render_store.rs`) exposed through
  `BinaryProtocolClient`; full density (13,164 nodes / 145,692 edges) now renders
  at 90 fps (was ~11 fps in GDScript). Draw budgets are derived from the received
  topology rather than the hardcoded 640/3000 gates; initial-load quality is a
  settings dial (`initialNodeLimit` on `/api/settings/physics`, replacing the
  compiled-in `DEFAULT_INITIAL_NODE_LIMIT`); WebSocket receive cap raised to
  256 MiB. Full-3D force layout is now the default — `axisCompressionZ` removed,
  dual-disc flatten opt-in (`enableDualDiscLayout`, default OFF).
- **V5 wire protocol.** The client decodes the V5 position wrapper
  (`[0x05][u64 broadcast_seq][V3 records]`); documented in
  `docs/reference/binary-protocol.md`. WebSocket upgrade `frame_size` raised so
  large V5 broadcasts are not truncated.
- **GPU force-channel registry (ADR-138).** A bounded `ForceChannel` enum
  (`src/models/force_channels.rs`) mapping each named channel to the existing
  `SimParams` scalar field(s) and feature-flag bit(s) — one enumerable
  "what channels exist / on-off / strength" registry with zero change to the
  struct layout, CUDA kernels, or settings wire (the migration seam toward the
  later array-backed `SimParams` refactor). Includes the pinned-node bitmask and
  DAG radial-bias channel.
- **Graph2VR-class immersive interaction (ADR-139).** Two-hand pinch
  scale/rotate, radial menu, in-graph search, and a node expansion API adopted
  into the Godot/OpenXR client (ideas-level / clean-room mining record — no
  external code or assets vendored; every feature re-implemented against
  VisionClaw's own `RenderStore`, CUDA kernels, and Graph V3 wire).
- **Agent-swarm visualisation in XR (ADR-140, P1 shipped).** Consumes the
  `0x23 AGENT_ACTION` frames (`AgentBeamActor`/`BeamCoalescer`, server side):
  embodied agent capsules hover near their target node, directional work beams
  stream agent→node, 4-channel status halo, and a Swarm tab on the HUD control
  centre with tap-to-teleport. Motion-authority split — server owns *which* node
  + status, XR client owns *where in the room* — so zero server change. All
  per-frame cost stays in the Rust RenderStore.
- **Constrained-layout engine programme (ADR-141, P1–P4 complete; P5/P6
  deferred).** 13-pattern taxonomy audited and mapped to soft force channel / hard
  projection / CPU one-shot placement methods: Sugiyama layers, stratified planes,
  spherical shells, and ego-radial `RadialModes`. New endpoints
  `POST /api/layout/mode` and `POST /api/layout/radial`.
- **Visual query builder.** `POST /api/graph/query/pattern` — build a graph
  pattern query against the semantic planes.
- **Fold ladder.** `GET /api/graph/fold` — hierarchical-density fold levels; the
  fold application layer is the ADR-137 `RenderStore`.

## [Unreleased] - 2026-08-15

### Added — Semantic-trust gap closures (PRD-022 WS-1/WS-2, PRD-010 G4)

Four long-standing scaffolds finished and verified (four-agent mesh, all gates
green; per-workstream detail in the commit messages):

- **SHACL enforcement** — the five `.shacl.ttl` NodeShapes now load at startup
  and drive a shape-derived validator; the ingest gate is enforcing by default
  (`ontology_agent.shacl_mode`, rollback `advisory`), and the trust-status
  endpoint reports the live mode. Live GitHub sync now skips shape-invalid
  blocks instead of silently ingesting them.
- **PROV-O provenance** — unified into the single append-only ledger
  `urn:ngm:graph:provenance`; `propose_create`/`propose_amend`, inference, and
  decision paths all reify (fail-open). New query surface
  `GET /api/ontology/provenance?entity=<urn>[&depth=N]`.
- **Forum NIP-42 AUTH** (`nostr-rust-forum`) — NIP-42 is the universal write
  gate; the pubkey allowlist is authorisation-after-authentication. Escape
  hatch `AUTH_MODE=allowlist`.
- **Mesh federation** (`nostr-rust-forum`) — PRD-010 forum reference
  implementation shipped: IS-Envelope v1 (JCS/RFC 8785), in-crate NIP-26,
  tri-mode `[mesh]` config, CF-compatible MeshTransport, fan-out planner
  gated on live NIP-42 sessions via `SessionAuthBoundary`.

### Fixed

- Three stale test expectations repaired: two V3 wire-format size asserts
  (48 → 52 bytes/node, ADR-031) and an f64-scale epsilon on the f32 GPU
  community-modularity oracle.

## [Unreleased] - 2026-08-10

### Added — Live linkage: reasoned graph → visible graph (`graphUpdated`)

- **Server now signals connected clients when the reasoned graph changes.**
  `GraphServiceSupervisor` broadcasts a debounced `{"type":"graphUpdated",
  "revision":N,"reason":…}` text frame over the existing
  `ClientCoordinatorActor` fan-out on: full database reload (GitHub sync /
  admin reload — fires *after* the reload completes), bulk node ingest,
  runtime edge inserts, full topology replacement, ontology axiom add/remove,
  and stored Whelk inference results. Previously connected clients were never
  told about any structural change and stayed stale until reconnect.
- **Client refetches on signal.** `textMessageHandler.ts` handles
  `graphUpdated` with a 750 ms trailing debounce: refetch REST
  `/api/graph/data` (topology-hash dedup makes no-change refetches free) and
  refresh the inferred-axioms overlay so `InferredEdges` tracks the evolving
  reasoner output live.
- **Self-heal backstop.** `graphDataManager.updateNodePositions` sample-checks
  the id-keyed binary position stream (~every 2 s, 30 s refetch cooldown); if
  frames reference node ids the client never learned, topology is refetched —
  covers missed signals and frame/refetch races.
- Protocol documented in `docs/reference/websocket-protocol.md` (§ Live
  linkage).

### Fixed — Graph interactions restored (drag + double-click)

- **Node drag and double-click→narrativegoldmine both work again.** Five
  compounding faults: StrictMode unmount disposed the GPU-transform picking
  texture while the memoised mesh survived (raycast fed a null buffer);
  `useMemo` ref side-effects leaked from discarded renders so `useFrame` drove
  a phantom mesh while the R3F-committed mesh (with the pointer handlers)
  rendered nothing; the R3F event manager could mount unconnected (async `gl`
  factory); the `onDragStateChange` → OrbitControls-disable wiring was lost
  when `GraphViewport` was retired; and the `WebSocketAdapter` lacked
  `sendMessage`, so hidden `as unknown as` casts threw at runtime — killing the
  `nodeDragStart/Update/End` server pin protocol *and* stranding OrbitControls
  disabled. Pointer capture now uses R3F's capture shim so drags no longer
  stall when the cursor outruns the node. Commit `2644fdeea`.

### Changed — Ontology reasoner is live by default

- **`ontology_validation` feature flag now defaults ON** (`FeatureFlags::default()` in `src/handlers/api_handler/analytics/types.rs`; `FEATURE_FLAGS` initialises from it in `analytics/state.rs`). The Whelk EL reasoner's HTTP read endpoints (`/api/ontology/*`) are therefore exposed by default — the reasoner is no longer opt-in. Commit `ea8d5e360`.
- **Whelk classifier runs at boot** and materialises ~37k inferred axioms into the named graph `urn:ngm:graph:ontology:inferred` (reflexive `X ⊑ X`, `X ⊑ owl:Thing`, `owl:Nothing ⊑ X`, plus the asserted-subclass closure). The "live reasoned graph" is now actually served, not aspirational.

### Fixed

- **`GET /api/ontology/hierarchy` no longer panics.** It was crashing with a `block_on`-in-async panic; it now returns `200` with the full OWL class hierarchy (~12,417 nodes). The class list/hierarchy is fetched async-direct via `state.ontology_repository.list_owl_classes().await` rather than through the sync `QueryHandler`. Commit `2d17365c5`.

## [Unreleased] - 2026-06-21

### Added — Semantic Trust Layer (PRD-022 Phase 1, ADR-127)

- **W3C SHACL shape catalogue** (`crates/visionclaw-ontology/shapes/*.ttl`): 5 shapes for OntologyClass, InferredAxiom, BridgeRecord, KnowledgeNode, AgentNode. Loaded into dedicated `urn:ngm:graph:shapes` named graph via SPARQL migration.
- **Dual-mode SHACL gate** (`shacl_gate.rs`): Enforcing mode (write paths reject violations) and Advisory mode (read paths log and proceed). Gate produces `ShaclGateReport` with severity-tagged violations.
- **W3C PROV-O provenance reification** (`provenance_emitter.rs`): `reify_activity()` inserts PROV-O triples into append-only `urn:ngm:graph:provenance` named graph. `query_agent_activities()` and `count_*()` SPARQL queries for liveness.
- **BC20 provenance crossing** (`receipt-minter.js`): `crossActivityOutbound()` wired into `mintSpendActivity()` — every spend activity auto-crosses through the BC20 bridge to the VisionClaw provenance graph. Fail-open.
- **Trust status endpoint** (`GET /api/ontology-physics/trust-status`): WS-5 liveness canary reporting shapes loaded, provenance triples, gate modes, federation status.
- **SPARQL migrations**: `0002_bootstrap_shapes_graph` and `0003_bootstrap_provenance_graph` added to the migration registry.
- **Docs**: PRD-022, ADR-127 (keystone), DDD Semantic Trust Layer bounded context (BC22).

## [Unreleased] - 2026-05-12

### Added — Agent Control Surface Protocol Integration

> **Superseded design (ADR-130 Decision 2).** The entries in this section
> record the `crashbug`-branch ACSP integration built around a `BrokerActor`
> (`src/actors/broker_actor.rs`) and a `ServerNostrActor`
> (`src/actors/server_nostr_actor.rs`). **Neither file merged to `main`.** What
> actually ships on `main` is the ADR-110 stateless ACSP producer: kinds 31400
> and 31402 are built by `src/services/acsp/events.rs` (`build_panel_definition`
> / `build_action_request`) over the ported `src/domain/broker/` kernel, and the
> `broker:new_case` / `broker:case_decided` WebSocket events are emitted from the
> enrichment-decide handler (`services::broker_events`). Read every `BrokerActor`
> / `ServerNostrActor` reference below as that superseded transport, not shipped
> `main` code.

- **Governance panel publishing** (kind 31400): registers/updates NIP-33
  parameterized replaceable control panels on the Nostr relay. Panel definitions
  include schema type (ActionInbox, Dashboard, ConfigForm, StatusBoard,
  ChatBridge), field definitions, action buttons, layout hints, and
  capabilities. Wire-compatible with `nostr-bbs-core::PanelDefinition`. On `main`
  this ships as `src/services/acsp/events.rs::build_panel_definition` (ADR-110
  ACSP producer); the `crashbug` `src/actors/server_nostr_actor.rs`
  `PublishGovernancePanel` message named in earlier drafts never merged.
- **Action request publishing** (kind 31402): emits a case for human review with
  case ID, title, category, priority, structured fields, and agent reasoning.
  Wire-compatible with `nostr-bbs-core::ActionRequest`. On `main` this ships as
  `src/services/acsp/events.rs::build_action_request` (ADR-110 ACSP producer);
  the `crashbug` `src/actors/server_nostr_actor.rs` `PublishActionRequest`
  message never merged.
- **Broker startup panel** (superseded `crashbug` `src/actors/broker_actor.rs`,
  never merged): the design published a PanelDefinition (kind 31400, d-tag
  `visionclaw-broker`) from `Actor::started()` with 5 fields (case_id, title,
  category, priority, state), 3 actions (approve, reject, escalate), table
  layout, and a 30-second refresh interval, discoverable via relay subscription.
  On `main` the panel is registered by the ADR-110 ACSP producer, not an actor
  `started()` hook.
- **Broker → forum ActionRequest** (superseded `crashbug`
  `src/actors/broker_actor.rs`, never merged): the design published a kind-31402
  ActionRequest to the forum relay on every `SubmitBrokerCase` (all case
  categories, not just KnowledgeEnrichment), with priority mapping u8 90+ →
  Critical, 70+ → High, 40+ → Medium, else → Low. On `main` the equivalent
  kind-31402 case event is produced by the ACSP producer over the
  enrichment-decide path.
- **Broadened Nostr decision events**: `SignBrokerDecision` (kind 30300) is now emitted for all case categories on `DecideBrokerCase`, not just `KnowledgeEnrichment`.
- **NIP-98 enterprise RBAC** (`src/middleware/enterprise_auth.rs`): `nip98-auth` feature gate adds a Nostr NIP-98 authentication path to the `RequireRole` middleware. When enabled, reads `Authorization: Nostr <base64>`, verifies the Schnorr signature, and resolves the signer's pubkey to an `EnterpriseRole` via the `Nip98RoleResolver` trait. `InMemoryRoleMap` provided for dev/test; `Nip98IdentityExt` request extension carries verified pubkey and role. The `X-Enterprise-Role` header path remains as the default when the feature is disabled.
- **Prometheus counters**: `NostrKind::K31400` and `K31402` variants added to `src/services/metrics.rs` for governance event observability.
- **Supported kinds extended**: `SUPPORTED_KINDS` in `src/services/server_identity.rs` now includes 31400 and 31402.
- **Tests**: `handles_publish_governance_panel` and `handles_publish_action_request` in `server_nostr_actor.rs`; 3 NIP-98 tests in `enterprise_auth.rs` (feature-gated).

### Changed

- `RequireRole` middleware now supports dual-path auth: NIP-98 Schnorr verification (when `nip98-auth` feature enabled and resolver attached) or `X-Enterprise-Role` header extraction (default).
- `crashbug` `ServerNostrActor` module doc (superseded design, not on `main`) updated to list 9 message variants across 7 event kinds (was 7 variants across 5 kinds).
- `crashbug` `BrokerActor` imports consolidated (superseded design, not on
  `main`): all governance types (`PublishGovernancePanel`, `PublishActionRequest`,
  `ActionPriority`, `PanelDefinitionPayload`, etc.) imported at module level.

---

## [Unreleased] - 2026-04-23

### Added — Agentbox integration planning

- **PRD-004** (`docs/PRD-004-agentbox-visionclaw-integration.md`): deprecate `multi-agent-docker/` in favour of `agentbox/` as VisionClaw's agent-container subsystem. 5 milestones (M1-M5), 17 port-in rows (P0-P3), 13 agentbox-design-improvement rows (D.1-D.13), 11 explicit rejections, 10 resolved open questions. All milestone exit criteria are now passable predicates.
- **ADR-058** (`docs/adr/ADR-058-mad-to-agentbox-migration.md`): MAD deprecation with four predicate-based gates, mid-cutover rollback protocol (< 5 min target, CI-verified), 30-day post-cutover window with frozen-MAD container.
- **DDD-BC20 AgentboxIntegration** (`docs/ddd-agentbox-integration-context.md`): new bounded context acting as Anti-Corruption Layer between agentbox's adapter protocol and VisionClaw Rust aggregates. `FederationSession` + `AgentExecution` aggregates, five ACL translator modules, composed `SessionHealth` with per-slot degrade policies, `/v1/meta` handshake protocol, signed Ed25519 `LocalFallbackProbe`.
- **Agentbox subdir**: `agentbox/` added as sibling to `multi-agent-docker/` for in-situ radical-upgrade work. Slated for promotion to standalone repo `github.com/DreamLab-AI/agentbox` once stable.
- **QE fleet pre-implementation audit** (RuVector `agentbox-comparison :: qe-fleet-review-2026-04-23`): verdict **Conditional GO** for M1; five P0 doc edits landed before any code work.

### Changed

- `multi-agent-docker/` is on a deprecation track per ADR-058; no new features land there.
- Durable state (beads, pods, memory) is henceforth a VisionClaw-Rust concern, not a container-internal concern. Agentbox is a federation client (see agentbox ADR-005).

## [Unreleased] - 2026-04-18

### Added

#### Insight Migration Loop design corpus
- Phase 1 research: 9 artefacts totalling ~19,700 words defining the dual-tier identity, sigmoid scoring, broker workflow, physics forces, acceptance tests
- ADR-048 Dual-tier identity model (KGNode + OntologyClass with BRIDGE_TO edges)
- ADR-049 Insight-migration broker workflow (MigrationCase subtype, DecisionOrchestrator contract)
- PRD: Insight Migration Loop (3 personas, 10 capabilities, 6 migration KPIs)
- DDD context refinement (BC13 MigrationCandidate aggregate, BC11 MigrationCase)
- 00-master.md: synthesised reconciliation resolving 5 cross-artefact contradictions, 8 blocking questions for owner decision

#### Enterprise & Regression Testing
- Enterprise drawer (`EnterpriseDrawerMount`, `EnterpriseDrawer`) — full-viewport slide-out panel with frosted-glass alpha blend, Ctrl+Shift+E / Cmd+Shift+E toggle, floating FAB button
- `drawer-fx` WASM crate (`client/src/wasm/drawer-fx/`) — Rust flow-field ambient effect for enterprise drawer canvas layer; zero-copy `Float32Array` pattern matching `scene-effects`
- Regression tests: `tests/physics_orchestrator_settle_regression.rs`, `tests/settings_physics_propagation_regression.rs`
- `tests/smoke/nginx-coep-headers.sh` — COEP header smoke test
- Enterprise drawer design document: `docs/design/2026-04-17-enterprise-drawer.md`
- QE audit report: `docs/audits/2026-04-17/` (master, frontend graph loading, backend settings routing, failure patterns, regression risk, regression tests — 6 files)
- `enterprise-standalone.tsx` with `#/drawer-demo` hash route for isolated drawer preview

### Fixed
- **PHYSICS: Dual `ClientCoordinatorActor` instances** — `SocketFlowServer` registered clients in one coordinator instance while `PhysicsOrchestratorActor` broadcast to a second internally-created instance, causing 0 binary frames to reach any connected client. Fixed by injecting the shared `ClientCoordinatorActor` address into `GraphServiceSupervisor::with_client()` and skipping internal creation when an external instance is provided.
- **PHYSICS: `ClientFilter` default filter to zero** — `ClientFilter::default()` had `enabled: true` with empty `filtered_node_ids`, causing `broadcast_with_filter` to produce no payload for fresh clients. Fixed by setting `enabled: false` as the default (opt-in filtering, not opt-out).
- **PHYSICS: `FastSettle` permanent latch** — `FastSettle` mode set `fast_settle_complete = true` and `is_physics_paused = true` on reaching the iteration cap even when energy had not converged, preventing subsequent physics parameter changes from resuming simulation. Fixed by falling back to `Continuous` mode on non-convergent exhaustion rather than halting.
- **PHYSICS: Boundary-pinned node rescue** — Added detection for nodes oscillating at viewport boundary (`|coord| >= viewport_bounds - 1` for 60+ consecutive frames) and teleporting them to randomised interior positions, complementing the existing runaway-node rescue (nodes beyond 10× viewport bounds).
- **SLIDER RANGES: Calibrated physics UI sliders** — Attraction (`attractionK`) capped at 10, Dual Graph Separation (`graphSeparationX`) capped at 500, Flatten to Planes (`zDamping`) capped at 0.1. Previous maximums were orders of magnitude too wide.
- **AUTH: Enterprise endpoints returning 403** — `apiFetch` was not injecting auth headers; added auth header injection mirroring `authRequestInterceptor`. Backend `verify_access` now accepts `Bearer dev-session-token` in non-production environments before NIP-98 path.
- **NGINX: COEP headers lost on Vite proxy routes** — Per-location `add_header` now set for all Vite module proxy paths (`/.vite`, `/node_modules`, `/@vite`, etc.) because `add_header` in a `location` block drops server-level headers.
- **DEBUG: Console spam from RemoteLogger** — Gated `originalConsole.log/debug/info` echo behind `localStorage.debug.consoleLogging === 'true'`; `warn` and `error` continue to echo unconditionally.
- **DEBUG: BotsDataProvider polling churn** — `pollingConfig` literal re-created on every render caused `useAgentPolling` to stop and restart every 2 seconds. Fixed with `useMemo` + `useCallback`.
- **WebSocket: `permessage-deflate` misused as subprotocol** — Removed `.protocols(&["permessage-deflate"])` from WebSocket upgrade handler (it is a WebSocket extension, not a subprotocol; placing it in `.protocols()` produced a malformed negotiation header).
- **WebSocket: Frame size limit** — Added `.frame_size(4 * 1024 * 1024)` to WebSocket upgrade handler; default 64 KiB was silently truncating large V5 broadcasts.
- **First-frame render** — `GraphManager` now polls via `window.setInterval` for non-zero positions from `graphWorkerProxy` and calls R3F `invalidate()` when data arrives, fixing the case where the graph was invisible until window resize triggered a re-render.

---

## [Unreleased] - 2026-04-12

### Added
- Layout mode system with 6 algorithms (ADR-141; corrected 2026-08-31 from the mis-attributed ADR-031 — ADR-031 defines only the `LayoutMode` enum, the constrained-layout engine programme that lands the algorithms is ADR-141)
- ForceAtlas2 LinLog kernel for community-revealing layout
- Spectral, Hierarchical, Radial, Temporal, Clustered layout engines
- PageRank HTTP API endpoints (compute/result/clear)
- DBSCAN standalone clustering API
- GET /api/graph/positions endpoint (GPU position snapshot)
- Layout API endpoints (modes/status/zones/reset)
- Camera auto-fit to graph bounding box
- Degree-weighted node sizing (sqrt scaling, 10.7x ratio)
- Mass-aware physics (hub inertia)
- Dual-graph X-axis offset (graphSeparationX)
- Constraint zone system for node type separation
- 5 ontology constraint specialized GPU kernels
- 7 semantic force GPU FFI bridges
- Stress majorization GPU-only path
- DBSCAN in settings dropdown

### Fixed
- CUDA_ARCH runtime auto-detection (was using stale .env)
- PTX module lookup in community.rs (wrong module for clustering kernels)  
- Two-sheet Z-axis degeneration (polar angle sampling bias)
- Slider ranges capped to sane values (was 50000 max)
- Clustering visualization: analytics panel writes to node_analytics
- Louvain writes to both cluster_id and community_id slots
- Community detection results stored in node_analytics
- Settings auth bypass for dev containers
- Route registration for pagerank/pathfinding (was 404)
- Node ID type flag encoding in binary protocol

---

## [Unreleased] - 2026-02-08

### Client Architecture Overhaul

#### Graph Worker & Physics

- **Position preservation in setGraphData()**: Worker now preserves interpolated positions for existing nodes when setGraphData is called (from initialGraphLoad, filter updates, reconnects). Only genuinely new nodes receive fresh positions, eliminating the visual "explosion" on graph reload.
- **Interpolation fix**: Server physics lerp factor was 1000x too slow due to `deltaTime / 1000` bug (deltaTime is already in seconds from Three.js clock). Fixed to `1 - Math.pow(0.001, deltaTime)`, converging in ~1 second instead of ~16 minutes.
- **Stable ID mapping**: Non-numeric node IDs now use FNV-1a hash (shared `stringToU32` in `client/src/types/idMapping.ts`) instead of unstable `index + 1`. Collision resolution via linear probe. Ensures consistent numeric IDs across setGraphData calls.
- **ForceComputeActor state preservation**: `iteration_count`, `stability_iterations`, and `reheat_factor` are no longer reset when settings updates arrive. Physics simulation maintains continuity across settings changes.

#### WebSocket Architecture

- **WebSocketEventBus** (`client/src/services/WebSocketEventBus.ts`): New typed pub/sub for cross-service WebSocket events. Event categories: `connection:open/close/error`, `message:graph/voice/bots/pod`, `registry:registered/unregistered/closedAll`.
- **WebSocketRegistry** (`client/src/services/WebSocketRegistry.ts`): Central connection lifecycle tracker. All WebSocket services (Voice, Bots, SolidPod, Graph) register/unregister connections through the registry.
- Eliminated `window.webSocketService` global in favour of direct module imports.

#### Settings Pipeline

- **Simplified useSelectiveSettingsStore**: Reduced from 548 to 152 lines. Removed manual caching, TTL, and debouncing; uses Zustand selectors natively.
- **Backend accepts partial JSON**: Physics and quality-gate PUT handlers now merge partial patches into current settings instead of requiring full payloads.
- **Quality gate defaults raised**: `maxNodeCount` increased from 10,000 to 500,000.

#### Visual System

- **MetadataShapes**: Now respects `nodeSize` setting (applies `sizeMultiplier`). Geometry sizes normalized to ~0.5 bounding sphere radius. Settings lookups hoisted out of per-node per-frame loop for performance.
- **KnowledgeRings**: Only renders on nodes positively identified as `knowledge_graph` type. No longer falls back to the `graphMode` default, preventing incorrect ring display on non-knowledge nodes.

#### Code Quality

- Deleted `lucide-react.d.ts` manual type declarations; converted 32 deep-path imports to barrel imports.
- Replaced `window.webSocketService` global with direct imports across all consuming modules.
- Removed V1 binary protocol dead code. Fixed V4 log spam (warn-once pattern).
- Replaced 14 `console.log` calls with proper logger usage.
- Removed dead functions/imports from GraphManager, websocketStore, and graphDataManager.

### Algorithm Pipeline Completion

- Wire SSSP distances into GPU force kernel `d_sssp_dist` buffer (SSSP-aware spring forces now active)
- Implement delta-stepping for GPU SSSP (configurable bucket width)
- Wire GPU APSP kernel (`approximate_apsp_kernel`) into `ShortestPathActor`
- Add multi-source batched SSSP for efficient landmark computation
- Implement LSH (Locality-Sensitive Hashing) replacing O(n^2) pairwise similarity
- Add CPU SIMD vectorization (AVX2/SSE4.1) for physics fallback
- Implement A* search with Euclidean 3D heuristic
- Implement bidirectional Dijkstra for point-to-point queries
- Add semantic pathfinding with trait-based embedding provider

---

## [1.2.0] - 2026-02-11

### Voice-to-Voice System (b92150b)

- **Multi-User Real-Time Voice Routing** with push-to-talk support
- **LiveKit SFU Sidecar** integration for spatial audio
- **Turbo-Whisper STT** for speech recognition
- **Opus Audio Codec** support for high-quality, low-latency audio
- New components: `VoiceRoutingService`, `SpeechService`, `PttCoordinator`

### Ontology-Guided Agent Intelligence (d856dfe + 1bd5dc4)

#### Added

- **OntologyQueryService**: semantic discovery, enriched note reading, Cypher validation with Levenshtein hints
- **OntologyMutationService**: proposal creation, Logseq markdown generation, Whelk consistency checks, quality scoring
- **GitHubPRService**: full GitHub REST API flow for ontology change PRs
- **7 MCP Tool Definitions**:
  - `ontology_discover` - semantic search across OWL classes
  - `ontology_read` - enriched note reading with axioms and relationships
  - `ontology_query` - schema-aware Cypher query validation
  - `ontology_traverse` - BFS graph traversal with configurable depth
  - `ontology_propose` - create/amend notes with Whelk consistency checks
  - `ontology_validate` - automated completeness and quality scoring
  - `ontology_status` - proposal and PR lifecycle tracking
- **7 REST API Endpoints** under `/ontology-agent/*`
- **13 Integration Tests** for the full ontology pipeline
- **Actix-web DI Wiring** for all new services
- **OntologyAgentSettings** configuration struct

### Documentation Overhaul

- Fixed 11 broken links in `docs/README.md` (`explanations/` → `explanation/`)
- Updated project structure documentation
- Corrected SQLite references to Neo4j throughout documentation
- Added missing documentation for voice and ontology systems

---

## [1.1.0] - 2026-01-12

### 🚀 Heroic Refactor Sprint - Quality Gate Achievement

**Sprint Duration:** 2026-01-08 to 2026-01-12 (5 days)
**Protocol:** AISP 5.1 Platinum (AI-to-AI Coordination with ∎ QED Confirmations)
**Quality Gate:** 60 → 75/100 (+15 points) ✅

---

#### Sprint Summary

The Heroic Refactor Sprint deployed 17 specialized QE agents across 3 waves using AISP 5.1 Platinum hive-mind coordination. All 8 critical issues resolved, test coverage significantly expanded, and code quality metrics improved across the board.

#### Wave 1: Foundation (2026-01-08)
| Agent | Task | Result |
|-------|------|--------|
| qe-coverage-analyzer | Test gap analysis | 62% baseline identified |
| qe-security-auditor | Vulnerability scan | 3 CRITICAL found |
| qe-code-reviewer | Quality standards | 439 unwrap() flagged |
| qe-performance-validator | Bottleneck analysis | Binary protocol mismatch |
| qe-architecture-reviewer | System design audit | CQRS validated |

#### Wave 2: Remediation (2026-01-09-11)
| Agent | Task | Result |
|-------|------|--------|
| security-remediator | Rotate secrets | ✅ Fixed 3 CVEs |
| unwrap-auditor | Critical path fixes | 439 → 371 (-16%) |
| coverage-booster | TypeScript tests | +145 tests |
| flaky-test-stabilizer | Test reliability | 0 flaky tests |
| clippy-cleaner | Lint warnings | 2429 → 1051 (-56%) |

#### Wave 3: Polish (2026-01-12)
| Agent | Task | Result |
|-------|------|--------|
| graph-export-handler | unwrap cleanup | 3 fixes applied |
| useTelemetry-tester | Hook test coverage | +45 tests |
| quality-gate-assessor | Final validation | 75/100 PASS |

---

#### Added

- **337 New Tests**
  - GPU memory manager: 48 tests (11 config, 37 GPU-gated)
  - Neo4j adapters: 49 tests (44 pass, 5 ignored)
  - useActionConnections: 50 tests
  - useTelemetry: 45 tests
  - Binary protocol: 20 tests
  - Agent visualization: 80+ tests

- **Test Framework Migration**
  - Migrated Jest → Vitest 2.1.8 for ESM compatibility
  - Fixed chalk TypeError in Node v23
  - Created `client/vitest.config.ts` with jsdom environment
  - Test pass rate: 77/81 (95.1%)

- **Agent Visualization Feature** (AGENT_ACTION 0x23)
  - `ActionConnectionsLayer.tsx` - 3D animated connections
  - `useActionConnections.ts` - Connection lifecycle management
  - `useAgentActionVisualization.ts` - WebSocket integration
  - Protocol: 15-byte header + variable payload
  - Quest 3 VR optimization (25 max connections)

#### Changed

- **Binary Protocol V2 Unification**
  - Position updates: 21 bytes (u32 ID + 3×f32 pos + u32 ts + u8 flags)
  - Agent state: 49 bytes V2 format
  - Version byte prefix mandatory
  - `createVersionedPayload()` test helper

- **Error Handling Improvements**
  - 68 unwrap()/expect() calls replaced with proper error handling
  - RwLock poison-safe helpers in `semantic_type_registry.rs`
  - Actor unwraps converted to `if let` patterns
  - Handler unwraps converted to `unwrap_or_default()`

#### Fixed

- **Security (3 CRITICAL)**
  - Removed hardcoded secret key fallback from `agent_monitor_actor.rs`
  - WebSocket authentication enabled
  - `.env.example` created for secure defaults

- **Code Quality**
  - Clippy warnings: 2429 → 1051 (56% reduction)
  - Removed ~1381 empty doc comments
  - Converted 6 manual Default impls to `#[derive(Default)]`
  - Fixed MutexGuard await issues

- **Test Reliability**
  - Fixed flaky assertions with deterministic timing
  - Hardcoded timeouts replaced with configurable values
  - Test isolation via `resetInstance()` patterns

#### Quality Metrics

| Metric | Before | After | Target | Status |
|--------|--------|-------|--------|--------|
| Clippy warnings | 2429 | 1051 | <2000 | ✅ PASS |
| Production unwrap() | 439 | 368 | <400 | ✅ PASS |
| Test count | ~500 | 837 | +300 | ✅ PASS |
| Test pass rate | 70% | 95.1% | 90% | ✅ PASS |
| Quality gate | 60 | 75 | 75 | ✅ PASS |

---

## [1.0.0] - 2025-10-27

### 🎉 Major Release - Hexagonal Architecture

VisionClaw v1.0.0 represents a complete architectural transformation from monolithic design to clean hexagonal architecture with CQRS pattern, delivering enterprise-grade reliability, maintainability, and scalability.

---

## Added

### Phase 1: Core Ports & Domain Layer (Completed)
- ✅ **Hexagonal Architecture Foundation**
  - Implemented 8 core ports for clean separation of concerns
  - Created domain-driven design layer with business logic isolation
  - Established CQRS pattern with Hexser framework

- ✅ **Repository Ports** (3 core interfaces)
  - `KnowledgeGraphRepository` - Graph data persistence abstraction
  - `OntologyRepository` - Semantic ontology storage interface
  - `SettingsRepository` - Application configuration management

- ✅ **Service Ports** (5 specialized interfaces)
  - `PhysicsSimulator` - GPU-accelerated physics computation
  - `SemanticAnalyzer` - Knowledge graph semantic analysis
  - `OntologyValidator` - OWL/RDF reasoning and validation
  - `NotificationService` - Cross-cutting notification delivery
  - `AuditLogger` - Compliance and audit trail management

- ✅ **CQRS Application Layer**
  - Command handlers for write operations (Directives)
  - Query handlers for read operations (Queries)
  - Event-driven architecture with domain events

### Phase 2: Adapter Implementation (Completed)
- ✅ **SQLite Repository Adapters** (3 databases)
  - `SqliteKnowledgeGraphRepository` - Knowledge graph persistence
  - `SqliteOntologyRepository` - Ontology data storage
  - `SqliteSettingsRepository` - Settings persistence with validation
  - *Note: v1.2.0 migrated the knowledge graph and ontology stores to Neo4j (see [1.2.0] changelog)*

- ✅ **Actor System Wrappers**
  - `ActorGraphRepository` - Actix actor wrapper for graph operations
  - `ActorOntologyRepository` - Actor-based ontology management
  - Thread-safe message passing for concurrent operations

- ✅ **Performance Optimizations**
  - WAL mode for SQLite (30% write speedup)
  - Connection pooling with r2d2 (5x concurrency improvement)
  - Batch operations (10x throughput for bulk inserts)

### Phase 3: Event-Driven Architecture (Completed)
- ✅ **Event Bus System**
  - Asynchronous domain event publishing
  - Type-safe event handlers with middleware support
  - Event persistence for audit trails

- ✅ **Domain Events** (8 event types)
  - `NodeCreated`, `NodeUpdated`, `NodeDeleted`
  - `EdgeCreated`, `EdgeUpdated`, `EdgeDeleted`
  - `OntologyLoaded`, `ValidationCompleted`

- ✅ **Event Handlers** (4 specialized handlers)
  - `GraphEventHandler` - Graph state change reactions
  - `OntologyEventHandler` - Semantic validation triggers
  - `NotificationEventHandler` - Real-time user notifications
  - `AuditEventHandler` - Compliance event logging

- ✅ **CQRS Integration**
  - Event publishing from command handlers
  - Query optimization with event-sourced projections
  - Eventual consistency management

### Phase 4: Advanced Features (Completed)
- ✅ **Multi-Database Architecture**
  - `settings.db` - Application configuration and physics settings
  - `knowledge_graph.db` - Graph nodes, edges, and metadata
  - `ontology.db` - OWL/RDF semantic framework
  - *Note: v1.2.0 migrated knowledge graph and ontology persistence from SQLite to Neo4j*

- ✅ **Type-Safe Code Generation**
  - Specta integration for TypeScript type generation
  - Automatic TypeScript definitions from Rust structs
  - Client-server type safety guarantees

- ✅ **Binary WebSocket Protocol V2**
  - 36-byte compact message format (80% bandwidth reduction)
  - <10ms latency for real-time synchronization
  - Protocol version negotiation

### Phase 5: Testing & Quality (Completed)
- ✅ **Comprehensive Test Suite** (90%+ coverage)
  - 150+ unit tests for ports and adapters
  - 50+ integration tests for CQRS workflows
  - 25+ event bus integration tests
  - Performance benchmarks (100k+ nodes)

- ✅ **Testing Infrastructure**
  - Mock adapters for isolated unit testing
  - Test fixtures for reproducible scenarios
  - Benchmark suite for performance validation
  - CI/CD pipeline integration

- ✅ **Quality Assurance**
  - Cargo clippy linting (zero warnings)
  - Rustfmt code formatting enforcement
  - Static analysis with cargo-audit
  - Memory safety verification

### Phase 6: Documentation & Cleanup (This Release)
- ✅ **Architecture Documentation**
  - Hexagonal architecture guide (3,000+ lines)
  - Ports and adapters pattern reference
  - CQRS implementation details
  - Event-driven architecture guide

- ✅ **API Documentation**
  - Complete OpenAPI/Swagger specification
  - REST endpoint catalog with examples
  - WebSocket protocol documentation
  - Binary protocol specification

- ✅ **Developer Guides**
  - Getting started tutorial
  - Contributing guidelines
  - Testing strategies
  - Code style guide

- ✅ **Migration Guides**
  - v0.x to v1.0 migration path
  - Breaking changes catalog
  - Deprecation timeline
  - Database migration scripts

- ✅ **Performance Documentation**
  - Benchmark results and analysis
  - Optimization techniques
  - Profiling guide
  - Scalability recommendations

- ✅ **Security Documentation**
  - Security architecture overview
  - Authentication flows
  - Authorization model
  - Vulnerability reporting process

---

## Changed

### Architecture Transformation
- **Database-First Design**: All state now persists in three SQLite databases
- **Server-Authoritative State**: Eliminated client-side caching for consistency
- **CQRS Pattern**: Separated read and write operations for clarity
- **Actor Integration**: Seamless integration with Actix actor system

### API Changes
- **Hexser Directives**: Write operations now use type-safe command handlers
- **Hexser Queries**: Read operations use optimized query handlers
- **Event Notifications**: All state changes emit domain events
- **Error Handling**: Consistent error types across all layers

### Performance Improvements
- **100x GPU Speedup**: Physics simulation with 39 CUDA kernels
- **80% Bandwidth Reduction**: Binary WebSocket protocol V2
- **30% Write Speedup**: SQLite WAL mode
- **5x Concurrency**: R2D2 connection pooling
- **10x Bulk Insert**: Batch operations

### Database Schema Updates
- **Settings Database**: Migrated from YAML/TOML to SQLite
- **Knowledge Graph Database**: Optimized indexes for graph queries
- **Ontology Database**: Support for OWL 2 EL profile reasoning

---

## Deprecated

### Legacy Code Marked for Removal
- **Direct SQL Calls**: Use repository ports instead
  ```rust
  #[deprecated(since = "1.0.0", note = "Use KnowledgeGraphRepository port")]
  pub fn execute_direct_sql(...) { ... }
  ```

- **Direct Actor Messages**: Use adapters instead
  ```rust
  #[deprecated(since = "1.0.0", note = "Use ActorGraphRepository adapter")]
  pub fn send_actor_message(...) { ... }
  ```

- **Monolithic Handlers**: Use CQRS command/query handlers
  ```rust
  #[deprecated(since = "1.0.0", note = "Use GraphApplicationService")]
  pub async fn handle_graph_save(...) { ... }
  ```

- **File-Based Configuration**: Migrated to database
  ```rust
  #[deprecated(since = "1.0.0", note = "Use SettingsRepository")]
  pub fn load_config_file(...) { ... }
  ```

### Deprecation Timeline
- **v1.0.0** (This Release): Deprecated code marked with compiler warnings
- **v1.1.0** (Q2 2025): Deprecated code triggers errors in tests
- **v2.0.0** (Q4 2025): Deprecated code completely removed

---

## Removed

### Legacy Systems Removed
- ❌ Client-side caching layer (caused sync issues)
- ❌ Monolithic configuration files (`config.yml`)
- ❌ Direct database access from handlers
- ❌ Untyped actor messages
- ❌ Hard-coded connection strings

### Unused Dependencies Removed
- Removed 15 unused crates (reduced binary size by 12MB)
- Eliminated deprecated actix-web 3.x dependencies
- Removed legacy serde serialization code

---

## Fixed

### Critical Bug Fixes
- **Settings Persistence**: Fixed race condition in concurrent writes
- **Actor Supervision**: Proper error handling and restart strategies
- **WebSocket Reconnection**: Improved connection stability
- **GPU Memory Leaks**: Fixed cuDNN memory management
- **Ontology Validation**: Corrected inference for class hierarchies

### Performance Fixes
- **Query Optimization**: Added indexes for common graph queries (10x speedup)
- **Connection Pooling**: Eliminated connection exhaustion under load
- **Event Processing**: Fixed event ordering for consistency
- **Binary Protocol**: Corrected byte alignment for 32-bit platforms
- **Physics Simulation**: Optimized force calculations (2x faster)

### Documentation Fixes
- Corrected 247 broken internal links
- Updated 85 outdated code examples
- Fixed 12 architecture diagrams
- Standardized 156 API endpoint descriptions

---

## Security

### Security Enhancements
- **SQL Injection Prevention**: Parameterized queries enforced by type system
- **Actor Isolation**: Message validation prevents unauthorized access
- **Audit Logging**: All state changes logged for compliance
- **Input Validation**: Comprehensive validation with `validator` crate
- **Error Sanitization**: Sensitive data stripped from error responses

### Vulnerability Fixes
- Fixed potential race condition in settings service
- Addressed actor message deserialization vulnerability
- Corrected file path traversal in ontology loader
- Hardened WebSocket authentication flow

---

## Performance Metrics

### Rendering Performance
| Metric | v0.x | v1.0.0 | Improvement |
|--------|------|--------|-------------|
| Frame Rate | 45 FPS | 60 FPS | +33% |
| Node Capacity | 50,000 | 100,000+ | +100% |
| Render Latency | 22ms | <16ms | -27% |

### Database Performance
| Operation | v0.x | v1.0.0 | Improvement |
|-----------|------|--------|-------------|
| Node Insert | 15ms | 2ms | -87% |
| Graph Query | 100ms | 8ms | -92% |
| Batch Insert (1000) | 15s | 1.2s | -92% |

### Network Performance
| Metric | v0.x (JSON) | v1.0.0 (Binary) | Improvement |
|--------|-------------|-----------------|-------------|
| Message Size | 180 bytes | 36 bytes | -80% |
| Latency | 25ms | <10ms | -60% |
| Bandwidth | 2.5 MB/s | 0.5 MB/s | -80% |

### GPU Acceleration
| Operation | CPU Time | GPU Time | Speedup |
|-----------|----------|----------|---------|
| Physics | 1,600ms | 16ms | 100x |
| Clustering | 800ms | 12ms | 67x |
| Pathfinding | 500ms | 8ms | 62x |

---

## Migration Guide

### Upgrading from v0.x to v1.0.0

#### 1. Database Migration
```bash
# Backup existing data
cp data/*.db data/backup/

# Run migration script
cargo run --bin migrate_legacy_configs

# Verify migration
cargo test --test migration_tests
```

#### 2. Environment Variables
```bash
# v0.x (deprecated)
DATABASE_URL=data/visionclaw.db
CONFIG_FILE=config.yml

# v1.0.0 (new)
SETTINGS_DB_PATH=data/settings.db
KNOWLEDGE_GRAPH_DB_PATH=data/knowledge_graph.db
ONTOLOGY_DB_PATH=data/ontology.db
```

#### 3. API Changes
```rust
// v0.x (deprecated)
let graph = database.execute_query("SELECT * FROM nodes").await?;

// v1.0.0 (new - use repository port)
let graph = knowledge_graph_repo.get_graph(graph_id).await?;
```

#### 4. Configuration Migration
```bash
# Remove legacy config files
rm config.yml ontology_physics.toml

# Configuration now in settings.db
# Use Hexser directives to update settings
```

See  for complete upgrade instructions.

---

## Breaking Changes

### API Breaking Changes
1. **Database Access**: All direct SQL calls removed
   - **Migration**: Use repository ports (`KnowledgeGraphRepository`, etc.)

2. **Actor Messages**: Untyped messages deprecated
   - **Migration**: Use typed adapters (`ActorGraphRepository`, etc.)

3. **Configuration**: File-based config removed
   - **Migration**: Use `SettingsRepository` for all config

4. **WebSocket Protocol**: Binary protocol V2 required
   - **Migration**: Client must implement binary message parser

### Database Schema Changes
1. **Settings Table**: New schema with validation
2. **Nodes Table**: Added `metadata_json` column
3. **Edges Table**: Added `semantic_weight` column
4. **Ontology Table**: Support for OWL axioms

### Dependency Updates
1. **Rust**: Minimum version 1.75.0 (was 1.70.0)
2. **actix-web**: Upgraded to 4.11.0 (was 4.8.0)
3. **cudarc**: Upgraded to 0.12.1 (was 0.11.7)

---

## Known Issues

### Resolved in v1.0.0
- ✅ Settings persistence race condition (Fixed)
- ✅ Actor supervision restart loops (Fixed)
- ✅ WebSocket reconnection hangs (Fixed)
- ✅ GPU memory leaks on long runs (Fixed)

### Planned for v1.1.0
- ⏳ Redis distributed caching layer
- ⏳ Multi-server deployment support
- ⏳ Advanced RBAC permission system
- ⏳ SPARQL query interface for ontologies

### Workarounds
- **Large Graphs (>100k nodes)**: Enable GPU acceleration for optimal performance
- **Concurrent Writes**: Use batch operations for high-throughput scenarios

---

## Upgrade Path

### Recommended Upgrade Strategy

1. **Development Environment**
   - Test migration on development database
   - Verify all integration tests pass
   - Review deprecated code warnings

2. **Staging Environment**
   - Deploy v1.0.0 to staging
   - Run performance benchmarks
   - Test with production-like data

3. **Production Deployment**
   - Schedule maintenance window
   - Backup all databases
   - Deploy with rollback plan
   - Monitor performance metrics

### Rollback Procedure
```bash
# If issues arise, rollback to v0.x
docker-compose down
docker-compose -f docker-compose.v0.yml up -d

# Restore database backup
cp data/backup/*.db data/
```

---

## Acknowledgments

### Phase 6 Contributors
- **Architecture Team**: Hexagonal architecture design and implementation
- **Documentation Team**: 10,000+ lines of comprehensive documentation
- **Testing Team**: 90%+ test coverage across all layers
- **Performance Team**: Benchmarking and optimization

### Special Thanks
- **Hexser Framework**: CQRS pattern implementation
- **Actix Project**: Actor system and web framework
- **Neo4j Team**: High-performance graph database
- **NVIDIA**: CUDA GPU computing platform

---

## Resources

### Documentation
- **[Architecture Guide](docs/explanation/backend-architecture.md)** - Hexagonal architecture deep dive
- **[API Reference](docs/reference/rest-api.md)** - Complete API documentation
- **** - Upgrade instructions
- **** - Optimization techniques

### Community
- **GitHub Issues**: https://github.com/DreamLab-AI/VisionClaw/issues
- **Discussions**: https://github.com/DreamLab-AI/VisionClaw/discussions
- **Discord**: https://discord.gg/visionclaw

### Support
- **Enterprise Support**: support@visionclaw.io
- **Documentation**: https://docs.visionclaw.io
- **Roadmap**: 

---

## License

This project is licensed under the Mozilla Public License 2.0 (MPL-2.0).
See [LICENSE](LICENSE) for full details.

---

**VisionClaw v1.0.0** - Enterprise-Grade Knowledge Graph Visualization
Built with ❤️ by the VisionClaw Team
