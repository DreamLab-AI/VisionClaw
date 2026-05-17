# 03 — Architecture Perspective

## Pod-first, Neo4j-second saga ordering

`src/services/ingest_saga.rs::execute_batch` enforces the ordering explicitly in three phases (lines 302-396). Phase 1 is a parallel `join_all` over Pod PUTs; Phase 2 builds the committed node set from **only** the Pod-successful nodes (`pod_successful.get(&p.node.id)` filter at line 348-351); Phase 3 marks pending on Neo4j commit failure. **This correctly prevents the orphan-Neo4j-row failure mode**. If the Pod write fails, the node never reaches `save_graph`; if Neo4j fails afterwards, the pending marker is set (line 389) and the resumption task (`RESUMPTION_INTERVAL = 60s`) retries.

**Finding A1 (MEDIUM)**: `execute_batch` is a **single Neo4j save_graph call across the whole batch** — one failure pends every Pod-successful node in that batch, and `pending_nodes` gauge jumps by N. A partial Neo4j failure inside `save_graph` (e.g. constraint violation on one row) cascades to the whole batch even though most rows were fine. The resumption task will re-try the whole batch on the next tick. No data loss, but amplifies recovery time. Consider a row-level Neo4j write path with per-node pending markers.

## Monotonic confidence on `BRIDGE_TO`

`BridgeEdgeService::promote` (`src/services/bridge_edge.rs:335-405`) uses a single Cypher `MERGE`/`ON MATCH`/`ON CREATE` statement with a `CASE WHEN $confidence > r.confidence THEN $confidence ELSE r.confidence END` update. This is atomic at the single-statement level in Neo4j — the MATCH and the update execute under the same implicit transaction. Two concurrent `promote` calls cannot interleave to lower confidence. The `signals` update is also gated on the same condition so `signals` stays consistent with `confidence`. Invariant is correctly enforced.

**Finding A2 (MINOR)**: `bridge_edge_enabled()` checks env vars on every call to `surface` / `promote` / `auto_expire` — cheap but not free. Consider caching at construction. Not a correctness issue.

## Dual-tier identity

`:KGNode` and `:OntologyClass` are kept distinct throughout. `bridge_edge.rs` Cypher consistently uses `MATCH (k:KGNode)-[...]->(o:OntologyClass)`. No grep hit for code that treats them interchangeably. The `BRIDGE_TO` / `BRIDGE_CANDIDATE` relationship labels are reserved (and `auto_expire` explicitly protects `BRIDGE_TO` with `WHERE ... <> 'promoted'` at `bridge_edge.rs:425`). Tier separation is honoured.

## Service boundaries (DDD)

`ingest_saga.rs` depends on `pod_client::PodClient`, `neo4j_adapter::Neo4jAdapter`, `metrics::MetricsRegistry`, and `models::node::Node`. `bridge_edge.rs` depends on `neo4j_adapter` and `metrics`. `orphan_retraction.rs` same. `server_identity.rs` depends on `nostr_sdk` only. Boundaries are clean — the sovereign-mesh code does not reach sideways into other bounded contexts. The one inversion worth noting: `handlers/api_handler/graph/mod.rs` calls `visibility_allows` locally rather than projecting the filter into Cypher. This is documented as a sprint trade-off (comments at lines 112-114 say the Cypher form will arrive with ADR-050 schema rollout). Acceptable for the sprint.

## `solid-pod-rs` Storage trait

`Storage` (referenced from `crates/solid-pod-rs/src/wac.rs:16` and `storage/` submodule) is a minimal async trait with `.get(key)` returning `(bytes, meta)`. The crate's `StorageAclResolver` is parameterised `<S: Storage>`. Backends (`MemoryBackend`, `FsBackend`) are behind Cargo features per ADR-053. Abstraction is clean — no leakage of Cloudflare worker types (the R2 backend would need a feature gate and adapter, no direct dependency yet). **Finding A3 (INFO)**: `PARITY-CHECKLIST.md` exists per the ADR; confirm it is populated.

## Actor-message serialisability

`ServerNostrActor` exposes `SignMigrationApproval`, `SignBridgePromotion`, `SignBeadStamp`, `SignAuditRecord` messages. Inspection of `src/actors/server_nostr_actor.rs:1-100` shows the messages wrap JSON-friendly payloads (the handlers build `json!(...)` content and `Tag` lists). `Tag` and `Event` from `nostr_sdk` are serde-compatible. The messages themselves are ordinary `actix::Message` impls with `Result<Event>` replies. Serialising the message type requires a wrapper (the `Event` reply carries `Signature`, which is serialisable). **Finding A4 (MINOR)**: there is no `serde::Serialize` derive on the message variants themselves — if persistence of in-flight messages becomes a requirement (crash recovery of a mid-sign actor state), add the derives now while the surface is small.

## Three-tier coordination

Top tier: the saga orchestrator (ingest_saga + bridge_edge + orphan_retraction). Middle tier: per-domain services (pod_client, server_identity, nostr_service). Worker tier: handlers. The BRIDGE_TO pipeline is explicitly coordinator-driven (surface → promote → expiry). The saga resumption task runs every 60 s (`RESUMPTION_INTERVAL`) and is spawned once at startup. The orphan retraction task runs every 15 min (`DEFAULT_PERIOD_SECS`) independently of `BRIDGE_EDGE_ENABLED` — correct: retraction is hygiene, promotion is feature-flagged.

## `OPTIONAL_AUTH` flag behaviour

`AccessLevel::Optional` downgrades to `Authenticated` when `NIP98_OPTIONAL_AUTH` is unset or false (`src/utils/auth.rs:92-95`). This means a scope wrapped with `RequireAuth::optional()` behaves like `RequireAuth::authenticated()` by default — anonymous requests get 401 until the flag is explicitly turned on. Matches ADR-028-ext rollback semantic exactly.

## Verdict

Architecturally the sprint is coherent. Pod-first ordering is enforced; monotonicity is atomic; service boundaries are respected; the feature-flag rollback mechanism is idempotent. Biggest structural observation (A1) is batch-size amplification of Neo4j failures — manageable; a fix can land as an independent PR.
