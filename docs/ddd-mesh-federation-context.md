# DDD — Mesh Federation Bounded Context Map

| Field | Value |
|-------|-------|
| Status | Draft (2026-05-08) |
| Drives | PRD-010, PRD-013 |
| Companion ADRs | ADR-073, ADR-074, ADR-075, ADR-076, ADR-086 |
| Sibling DDDs | `docs/ddd-agentbox-integration-context.md`, `agentbox/docs/reference/ddd/DDD-003-sovereign-messaging-domain.md`, `agentbox/docs/reference/ddd/DDD-004-linked-data-interchange-domain.md` |

## Purpose

This document maps the bounded contexts involved in PRD-010's DID:Nostr Mesh Federation and PRD-013's Solid Pod Git Ingest Surface, names their aggregates, fixes their invariants, and specifies the anti-corruption layers (ACLs) at each context boundary. It is the single source of truth for *who owns what* and *what translates between them*.

The mesh's architectural challenge is not the wire protocol (ADR-073) nor the message envelope (ADR-075) but the **relational integrity** at boundaries. The forum's user-pubkey, agentbox's agent-pubkey, and VisionClaw's substrate-pubkey are three different identities that must be reasoned about together; the moment a translation drops one, attribution breaks, ACLs misfire, or duplicate side-effects cascade.

---

## Bounded Contexts

### BC-MESH-FORUM — Dreamlab Forum Context

**Owner**: forum operations team (CF Workers + Leptos client).

**Mission**: provide a public-protocol/private-membership Nostr relay + identity service for human users. Users hold passkey-derived or extension-held keys; community moderation enforces zone access and trust progression.

**Aggregates**:

- **`Member`**: { `pubkey_hex`, `cohort`, `trust_level: TL0..TL3`, `is_admin`, `suspended_until`, `silenced`, `claimed_username` }. Persisted in `dreamlab-relay` D1.
- **`AuthSession`**: { `session_id`, `connection_addr`, `ip`, `authed_pubkey: Option<hex>`, `subscriptions`, `challenge_token` }. Per-WS-connection state in the Durable Object.
- **`ModerationAction`**: { `kind: 30910..30916`, `target_pubkey`, `signed_event`, `expires_at`, `actor_admin_pubkey` }. Mirrored from kind-30910/30911 events into D1 `moderation_actions`.
- **`ProfilesRow`**: { `pubkey`, `name`, `display_name`, `picture`, `last_kind0_at` }. Projection of kind-0 metadata.
- **`Pod`**: { `pubkey_hex`, `pod_uri = pods.dreamlab-ai.com/<hex>/`, `quota`, `webid_uri` }. Forum-controlled Solid pod per user.

**Invariants** (BC-MESH-FORUM-Inv):

- **F-Inv-01**: Every event accepted at the relay has a verified Schnorr signature against `event.pubkey` (`relay_do/nip_handlers.rs:200`).
- **F-Inv-02**: Whitelist gate runs before any business logic for non-bypass kinds; kind-0/9021/9024 self-onboard into `lobby` cohort.
- **F-Inv-03**: Kind-1059 reads filter clients to `#p == authed_pubkey` (`nip_handlers.rs:348-387`); cross-recipient leakage is structurally impossible.
- **F-Inv-04**: NIP-98 replay protection via KV-backed seen-event-id cache, TTL ≥ tolerance window (Sprint v9 STREAM-B).
- **F-Inv-05**: WAC ACL `acl:agent` matching is case-sensitive; pubkeys MUST be lowercased before construction (PRD-010 H7).
- **F-Inv-06**: Moderation events kind-30910/30911 written by signers with `is_admin=true` mirror to D1 `moderation_actions` for downstream consumption (`nip_handlers.rs:249-255`).
- **F-Inv-07** (post-ADR-076): All NIP protocol primitives (event id/sign/verify, NIP-04, NIP-19, NIP-26, NIP-44, NIP-59, NIP-90, NIP-98) come from the upstream `nostr` crate, not from forum's `nostr-core`. The `nostr-core` shim contains ONLY: project-specific kind catalogue (30033, 30050, 30910–30916), `derive_from_prf` (PRF→Keys), three-backend `Signer` trait composition, `Nip98ReplayStore` trait + `KvReplayStore` impl, WASM bridge glue, and IS-Envelope encode/decode. Total shim ~700 LOC.

**Public surface**:

- WSS: `wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev` (NIP-01 / NIP-42).
- HTTPS: `https://api.dreamlab-ai.com` (auth-worker), `https://pods.dreamlab-ai.com` (pod-worker), `https://search.dreamlab-ai.com` (search-worker), `https://dreamlab-link-preview.solitary-paper-764d.workers.dev`.
- DID Documents: `pods.dreamlab-ai.com/.well-known/did/nostr/<hex>.json` (Tier-3).

**Existing ACLs at boundary**:

- **NIP-98 verifier**: incoming HTTP requests sign their event; verifier compares `event.pubkey` to the resource owner per WAC. Lives in `auth-worker/src/lib.rs`, `pod-worker/src/lib.rs:421-440`.
- **Whitelist + Trust gate**: incoming Nostr events filtered at `relay_do/nip_handlers.rs:42-258`.

---

### BC-MESH-AGENTBOX — Agentbox Container Context

**Owner**: agentbox project (`github.com/DreamLab-AI/agentbox`).

**Mission**: provide a sovereign Nix-based container hosting one (or in P5 follow-up, many) `did:nostr`-keyed agents, each with a Solid pod, a private nostr-rs-relay, and an MCP-driven orchestrator.

**Aggregates**:

- **`SovereignAgent`**: { `agent_id`, `private_key_hex`, `x_only_pubkey_hex`, `npub`, `created_at`, `pod_path = /var/lib/solid/pods/<npub>/`, `did_uri` }. Materialised by `sovereign-bootstrap.py:95-137`.
- **`PodInbox`**: { `npub`, files at `pods/<npub>/events/inbox/<event_id>.json` }. Atomic-rename writes by `RelayConsumer.

`. Today raw Nostr-event-wrapped JSON; PRD-010 F19 changes to LDN AS2.
- **`PodOutbox`**: { `npub`, files at `pods/<npub>/events/outbox/<pending_id>.json`, status: pending|published|failed }. Polled at 500ms by `RelayConsumer._flushOutbox`.
- **`IntentMarker`**: { `event_id`, `recipient_npub`, file at `pods/<npub>/events/intent-queue/<id>.json` }. For kinds 38000-38099 agent-intent events.
- **`Adapter`**: per-slot {`name`, `impl: local-* | external | off`, `endpoint`, `health`}. Five slots: beads, pods, memory, events, orchestrator.
- **`FederationManifest`**: { `mode: standalone|federated|client`, `peer_relays`, `federated_kinds`, `federated_pubkeys`, `allowed_remote_dids`, `delegation_required` }. Per-container manifest.

**Invariants** (BC-MESH-AGENTBOX-Inv):

- **A-Inv-01**: Every kind-1059 received by `RelayConsumer` is signature-verified before any side effect (DDD-003 I01).
- **A-Inv-02**: Every event has its recipient pubkey matched against the local agent set (DDD-003 I10) — events not addressed to a known local agent are dropped silently.
- **A-Inv-03**: Inbox writes are atomic-rename (write-tmp + rename) (DDD-003 I08).
- **A-Inv-04**: Sovereign keypair is generated exactly once per `agent_id` per container; persisted to `/var/lib/agentbox/identities/<id>.json`; immutable until rotated (DDD-003 SovereignKeyset).
- **A-Inv-05**: Pod ACLs name agents by `did:nostr:<hex>` form (canonical), not `npub` (DDD-003 §AgentIdentity, sovereign-bootstrap.py:152-156).
- **A-Inv-06** (post-PRD-010 F4-F5): `verificationMethod.type == "SchnorrSecp256k1VerificationKey2019"`; npub is bech32 of 32-byte BIP-340 x-only pubkey.
- **A-Inv-07**: Orchestrator-slot adapter is fatal if disconnected at boot (`server.js:830-833`); other adapter slots degrade to `off`.
- **A-Inv-08**: Every adapter dispatch passes through three middleware layers in order: observability → privacy filter → JSON-LD encoder (DDD-004 §L08).
- **A-Inv-09** (NEW, PRD-010 F16): `RelayConsumer` is wired into management-api boot when `[sovereign_mesh.relay].enabled = true`.

**Public surface**:

- HTTPS: `:9090/v1/*` (management-api), `:9091/metrics` (Prometheus).
- HTTP: `:8484` (solid-pod-rs).
- WSS: `:7777/` (nostr-rs-relay) — ONLY when `[mesh].mode != "standalone"` AND TLS bridge configured.
- DID Documents: `pods/<npub>/did-nostr.json` served by solid-pod-rs at `:8484/did:nostr:<hex>`.

**Existing ACLs at boundary**:

- **NIP-98 verifier**: management-api `middleware/auth.js:33-63` validates inbound `Authorization: Nostr <base64>` headers. Auto-elevation to `strict-nip98` when `[sovereign_mesh].enabled = true`.
- **`RelayConsumer` ingress policy**: open / signed-only / allowlist (`relay-consumer.js:339`).

---

### BC-MESH-VISIONCLAW — VisionClaw Substrate Context

**Owner**: VisionClaw project (this repo).

**Mission**: maintain the canonical knowledge graph (Neo4j-backed), ingest contributors' outputs as beads, expose the graph as a 3D/XR visualisation surface, and federate with forum + agentbox over the mesh.

**Aggregates**:

- **`UnifiedServerIdentity`** (post-PRD-010 F1): { `pubkey_hex`, `keys`, `relay_pool` }. Replaces today's split between `SERVER_NOSTR_PRIVKEY` and `VISIONCLAW_NOSTR_PRIVKEY`.
- **`KGNode`**: { `id (sequential u32)`, `iri`, `visionclaw_uri`, `canonical_iri`, `owner_pubkey`, `position`, `flags`, `node_type` }. Persisted in Neo4j; bit-29 opaque id derived for binary protocol.
- **`Bead`**: { `urn = urn:visionclaw:bead:<owner>:<sha256-12>`, `payload_json`, `brief_id`, `debrief_path`, `provenance` }. Persisted in Neo4j as `(:Bead)`.
- **`AgentExecution`** (planned, BC20): { `urn = urn:visionclaw:execution:<sha256-12>`, `agent_pubkey`, `slot`, `started_at`, `completed_at`, `events: [...]` }.
- **`OntologyClass`**: { `iri`, `urn_solid: Option`, `webid: Option`, `members` }. Persisted in Neo4j.
- **`FederationSession`** (planned, BC20): { `id`, `peer_substrate_pubkey`, `manifest_checksum`, `expires_at`, `attribution_chain` }.
- **`MeshBridgeState`** (NEW, PRD-010 F22): { `peer_relays: [{url, connected, last_event_at, lru, fed_session}]`, `subscriptions: [...]` }.
- **`GitRemote`** (NEW, PRD-013 G2): { `id (uuid)`, `url`, `auth: Pat|DidNostr|None`, `owner_did: Option<did:nostr:<hex>>`, `base_paths: Vec<String>`, `branch`, `sync_interval_secs`, `writeback_enabled: bool`, `last_sync`, `last_commit_sha` }. Persisted in Neo4j. Replaces the implicit single-remote model baked into `GITHUB_*` env vars. Each GitRemote represents a registered knowledge source (GitHub, GitLab, Solid pod, bare git repo). Legacy `GITHUB_TOKEN`/`GITHUB_OWNER`/`GITHUB_REPO` env vars auto-register as a single PAT-authenticated GitRemote with `id = "legacy-github"` at boot.

**Invariants** (BC-MESH-VC-Inv):

- **V-Inv-01** (post-PRD-010 F1): At boot, `SERVER_NOSTR_PRIVKEY` and `VISIONCLAW_NOSTR_PRIVKEY` (during deprecation window) MUST resolve to the same key bytes; otherwise fail-closed.
- **V-Inv-02**: Every URN minted under `urn:visionclaw:*` passes through `src/uri/mint.rs` (PRD-006 §6 anti-drift gate, F23 lint).
- **V-Inv-03**: `did:nostr:<hex>` resolution returns Tier-3 DID Document; missing handlers (PRD-010 F15) become a build error after Phase 1.
- **V-Inv-04** (post-PRD-010 F8): Every event ingested via `MeshBridge` verifies any present `["delegation", ...]` tag before attribution decisions.
- **V-Inv-05**: Bead URNs include the original-author hex pubkey in scope; substrate-emitted beads (e.g. server self-record) use the substrate's own `pubkey_hex`.
- **V-Inv-06** (post-PRD-010 F9): When forwarding events received from peer relays, EITHER the original event is forwarded verbatim with delegation proof OR the substrate refuses to forward (configurable).
- **V-Inv-07**: Solid pod handler at `/api/solid/*` enforces NIP-98 + WAC; WebID derivation uses `{base}/{pubkey_hex}/profile/card#me` shape.
- **V-Inv-08** (NEW, PRD-013 G3): Every machine-generated git commit carries structured provenance trailers (`Urn:`, `Proposed-by:`, `Approved-by:`, `Broker-case:`, `Decision:`, `Reasoning-hash:`, `Signed-off-by:`). Commits without valid trailers are rejected by the `WriteBackSaga` before push.
- **V-Inv-09** (NEW, PRD-013 G4): No write-back push occurs without a `DecisionOutcome::Approve` or `::Promote` from the Judgment Broker (BC11). The proposing agent cannot approve its own enrichment (ADR-041 self-review invariant).
- **V-Inv-10** (NEW, PRD-013 G1): Git ingest clones are sandboxed under `GIT_INGEST_ROOT` (default `/app/data/git-ingest/`). Path traversal outside this root is structurally impossible (`git2` handles path safety; solid-pod-rs-git's `guard::path_safe` enforces server-side).

**Public surface**:

- HTTPS: `/api/v1/*` (substrate API), `/api/solid/*` (solid-pod-rs), `/wss/agent-events` (agentbox WS), `/wss/visionflow_*` (XR + visualisation).
- HTTPS: `/api/ingest/remotes` (NEW, PRD-013 G2 — git remote registry CRUD), `/api/ingest/remotes/:id/sync` (trigger manual sync), `/api/ingest/remotes/:id/status` (sync status + last commit).
- WSS: optional substrate-side relay on `:7777` when `[mesh].mode == "federated"`.
- DID Documents: `/api/v1/identity/{hex}/did.json` (NEW, PRD-010 F2/F15).

**Existing ACLs at boundary**:

- **`management_api_client.rs`** → agentbox: HTTP REST, no payload translation. Treats agentbox API as opaque.
- **`agent_events_ws_handler.rs`**: bidirectional WS; inbound JSON `agent_action` becomes `BeamEdge` + `ChargeModulation` (visualisation actor). NOT a domain event ACL — visualisation projector only.
- **`mcp_relay_manager.rs`**: `docker exec`s into agentbox; operational, not domain.

---

### Inter-substrate library convergence (post-ADR-076)

After Phase 0 of PRD-010 completes, all three substrates consume the upstream
`nostr` crate (rust-nostr.org) as the single source of truth for NIP protocol
primitives:

- BC-MESH-FORUM: `nostr-core` shim (~700 LOC) re-exports `nostr` types; no own
  protocol implementation.
- BC-MESH-AGENTBOX: `nostr-tools` JS package (already-established) inside
  `mcp/nostr-bridge/`. Wire-compatible with `nostr` Rust by spec; cross-language
  vectors validate compatibility.
- BC-MESH-VC: `nostr_sdk` (existing) which depends on `nostr`; PRD-010 F29 adds
  direct `nostr` workspace dep so types flow across BC20 boundary without
  translation.

The convergence eliminates a class of cross-context drift: a NIP fix landed
upstream propagates to all three substrates via cargo update / npm update.
Sprint v9-v11's hand-port effort for NIP-04/NIP-44/NIP-26/NIP-98 evolution
becomes zero days post-absorption.

### BC-MESH-SOLID-POD-RS — Shared Crate Workspace Context

**Owner**: solid-pod-rs project (workspace at `./solid-pod-rs/`).

**Mission**: ship a runtime-flexible Solid LDP / WAC / WebID / NIP-98 / DID-Tier-3 / NIP-01-relay foundation library consumed by all three substrates as both library and standalone server.

**Aggregates** (shared types — not stateful aggregates, but the data primitives others depend on):

- **`Storage` trait**: `Send + Sync + 'static` async LDP-storage interface. `solid-pod-rs/src/storage/mod.rs:73`.
- **`PodError`**: cross-module error currency. `solid-pod-rs/src/error.rs`.
- **`Nip98Verifier`**: HTTP auth verifier. `solid-pod-rs/src/auth/nip98.rs`. (PRD-010 F20: gain `Nip98ReplayStore` trait parameter.)
- **`AclDocument`**, `evaluate_access`: WAC enforcement primitives. `solid-pod-rs/src/wac/`.
- **`NostrPubkey`**, `did_nostr_uri`, `render_did_document_tier1/3`: DID primitives. `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs`.
- **`Relay`, `Filter`, `Event`, `EventStore`, `RelayInfo`**: NIP-01 relay primitives. `solid-pod-rs-nostr/src/relay.rs`.
- **`NostrWebIdResolver`**: bidirectional DID ↔ WebID resolver. `solid-pod-rs-nostr/src/resolver.rs`.

**Invariants** (BC-MESH-SOLID-POD-RS-Inv):

- **S-Inv-01**: All public types are runtime-stable across `0.4.0-alpha.2` → `0.5.0-alpha.x`; semver respected.
- **S-Inv-02**: NIP-98 events have body hash verified after structural checks (`auth/nip98.rs`).
- **S-Inv-03** (post-PRD-010): `verificationMethod.type == "SchnorrSecp256k1VerificationKey2019"` in all DID renderers; both `did/v1` and `secp256k1-2019/v1` in `@context`.
- **S-Inv-04** (P5 follow-up, out of scope for PRD-010): `Storage` trait grows a `KvBackend` companion + `MaybeSend` features for WASM Workers compatibility.

**Public surface**: published as crates on crates.io. Workspace at `./solid-pod-rs/`. AGPL-3.0-only.

**Existing ACLs at boundary**:

- **`security::SsrfPolicy`**: rejects RFC 1918 / loopback / link-local / multicast / cloud-metadata at outbound HTTP boundary.
- **`security::DotfileAllowlist`**: only `.well-known`, `.acl`, `.meta` allowed in pod paths.
- **`PathTraversalGuard`**: rejects `..` after percent-decode.

---

## Inter-Context Anti-Corruption Layers

Each ACL is implemented at the boundary between two contexts, owned by the consumer side. The ACL translates the upstream's domain language into the consumer's, validates invariants on the way in, and never leaks upstream concepts past itself.

### ACL-VC↔FORUM — VisionClaw consumes Forum events

**Location**: `src/services/mesh_bridge.rs::handle_inbound_forum` (NEW, PRD-010 F22; replaces `nostr_bridge.rs:139-247`).

**Translates**:

- Forum kind-1 thread post → `urn:visionclaw:bead:<author_hex>:<sha256-12-of-content>` (when crosses salience threshold).
- Forum kind-1 with `["t", "kg"]` tag → `urn:visionclaw:concept:<domain>:<slug>` candidate.
- Forum kind-30910/30911 (moderation) → action on local Neo4j projection (mark `:Member` `suspended` flag).
- Forum kind-1059 IS-Envelope `subj=urn:visionclaw:bead:*` → bead reference resolution (via BC20 ACL).

**Validates**:

- All inbound events have valid Schnorr signatures (V-Inv-04 + ADR-074 D8 delegation verification).
- IS-Envelope conforms to ADR-075 schema before dispatch.
- `from` field ∈ `mesh.allowed_remote_dids` (else event dropped with mesh_peer_unauthorised++).

**Never leaks**:

- D1 row state from forum (forum's `Member` aggregate is not exposed in VisionClaw's domain).
- Forum admin pubkey beyond the explicitly-trusted `mesh.honor_remote_moderation` list.

### ACL-VC↔AGENTBOX (formerly known as BC20)

**Location**: `src/bc20/` (planned per PRD-006 §5.5, six modules + two aggregates; PRD-010 §10 Q7 frames timing).

**Modules** (per `docs/ddd-agentbox-integration-context.md`):

- `mod.rs` (entry)
- `federation_session.rs` (FederationSession aggregate)
- `federation_lifecycle.rs` (handshake, manifest exchange, expiry)
- `adapter_registry.rs` (cached endpoints from agentbox `GET /v1/meta`)
- `agent_execution.rs` (AgentExecution aggregate, per-execution receipts)
- `acl/{beads_acl,pods_acl,memory_acl,events_acl,orchestrator_acl,uris_acl}.rs` (six per-slot ACLs)
- `pod_bridge.rs` (NEW, PRD-013 G5 — agent git clone/commit/push mediation)

**Translates**:

- `urn:agentbox:bead:<scope>:<local>` → `urn:visionclaw:bead:<scope_hex>:<sha256-12>` (when local maps to known content; otherwise opaque-record-with-redirect).
- `urn:agentbox:agent:<id>` → `did:nostr:<agent_pubkey_hex>` resolved via agentbox's URN resolver + DID Document.
- `urn:agentbox:event:<scope>:<local>` → `urn:visionclaw:execution:<sha256-12>` only when the agent execution is mirrored locally; otherwise opaque-record.
- agentbox `GET /v1/meta` → `AdapterEndpointRegistry` value object cached for federation session lifetime.
- (NEW, PRD-013 G5) Agent git clone request → VisionClaw-credentialed `git clone` against registered `GitRemote`; worktree mounted into agent sandbox. Agent commits → validated provenance trailers → `BrokerCase(CaseCategory::KnowledgeEnrichment)` submission. Broker approval → NIP-98-signed `git push` to source remote.

**Validates**:

- Manifest checksum matches expected (FederationSession invariant).
- ProbeEndpoint Ed25519 signature valid (LocalFallbackProbe).
- All translated URNs pass through VisionClaw's `mint_*` (V-Inv-02).

**Never leaks**:

- Raw agentbox URN forms past the ACL boundary into Neo4j; only translated `urn:visionclaw:*` forms are persisted on `:Bead.canonical_urn`.
- agentbox adapter slot configuration into VisionClaw business logic.

**ACL companion fields** (PRD-010 F10): IS-Envelope `subj` URN is parsed by either `src/uri/parse.rs` (visionclaw) OR `agentbox/management-api/lib/uris.js` (agentbox) on the receiver side; the BC20 ACL handles cross-mapping when the URN is opaque to the receiver's native parser.

### ACL-FORUM↔AGENTBOX — Forum consumes agentbox events (and vice versa)

**Location**: forum side: `relay-worker/src/federation.rs::handle_inbound_peer` (NEW, PRD-010 F11). Agentbox side: `mcp/mesh-federation/federation-worker.js::handleInbound` (NEW, ADR-073 D10).

**Translates**:

- Agentbox kind-1059 IS-Envelope (kind=`tool_invoke`) → forum kind-1 with `["t", "agent-result"]` tag (when result destined for forum chat) OR forum kind-1059 wrap targeting same recipient (when DM destined for forum user).
- Agentbox kind-30910 admin event → forum `mod_cache` invalidation IF agentbox admin pubkey in forum's `mesh.honor_remote_moderation`.
- Forum kind-1059 → agentbox pod-inbox file (LDN AS2 shape per PRD-010 F19).

**Validates**:

- IS-Envelope conformance to ADR-075.
- NIP-26 delegation chain verification (ADR-074 D8) when `delegation` field present.
- Federation session pubkey ∈ `allowed_remote_dids`.
- NIP-42 AUTH-bound session (ADR-073 D3-D4).

**Never leaks**:

- Forum's D1 schema details (cohort logic, trust progression) into agentbox event payloads.
- Agentbox's pod path conventions into forum's relay worker.
- Federation key into ordinary user-key handling code.

### ACL-VC↔SOLID-POD-RS — VisionClaw consumes solid-pod-rs

**Location**: `src/handlers/solid_pod_handler.rs` (existing).

**Translates**:

- HTTP method → `wac::AccessMode` (`solid_pod_handler.rs:58`).
- NIP-98 event → `(pubkey, mode)` ACL agent.
- `solid_pod_rs::PodError` → actix `HttpResponse` with structured envelope (`solid_pod_handler.rs:431-439`).

**Validates**:

- Path traversal (`security::resolve_and_check`).
- Dotfile allowlist (only `.well-known`, `.acl`, `.meta`).
- WebID derivation: `{base}/{pubkey_hex}/profile/card#me` form (`solid_pod_handler.rs:401-411`).

### ACL-FORUM↔SOLID-POD-RS — Forum reimplementation gap

**Location**: forum's `pod-worker/src/{acl,webid,did,provision}.rs` reimplement what `solid-pod-rs` exports — see `docs/integration-research/04-solid-pod-rs-surfaces.md` §13.

**Why**: solid-pod-rs's `Storage` trait is Tokio-coupled (S-Inv-04 P5 follow-up). Cloudflare Workers cannot host Tokio reactors. Forum maintains its own `Storage`-equivalent + WAC/WebID/NIP-98 implementations. The ACL is the existence of these duplicates.

**PRD-010 does NOT close this**: the closure depends on solid-pod-rs 0.5.x adopting `KvBackend` + `MaybeSend`. Tracked separately.

---

## Domain Events (the language)

These are the events that flow across context boundaries. Each is an instance of an IS-Envelope kind (ADR-075 D3) or a Nostr event with a recognised kind.

| Domain event | Kind | From context | To context | ACL | Notes |
|--------------|------|--------------|------------|-----|-------|
| `UserSentDM` | 1059→14 | BC-MESH-FORUM | BC-MESH-AGENTBOX | ACL-FORUM↔AGENTBOX | IS-Envelope kind=`chat`; gift-wrap routes via `p` tag |
| `AgentReplied` | 1059→14 | BC-MESH-AGENTBOX | BC-MESH-FORUM | ACL-AGENTBOX↔FORUM | reply via outbox path |
| `UserInvokedTool` | 1059→14 | BC-MESH-FORUM | BC-MESH-AGENTBOX | ACL-FORUM↔AGENTBOX | IS-Envelope kind=`tool_invoke` |
| `ToolReturnedResult` | 1059→14 | BC-MESH-AGENTBOX | BC-MESH-FORUM | ACL-AGENTBOX↔FORUM | IS-Envelope kind=`tool_result` |
| `BeadIndexed` | 1059→14 | BC-MESH-VC | BC-MESH-FORUM | ACL-VC↔FORUM | IS-Envelope kind=`knowledge_link` |
| `ConceptCreated` | 1059→14 | BC-MESH-VC | BC-MESH-FORUM | ACL-VC↔FORUM | IS-Envelope kind=`knowledge_link` claim=`linked` |
| `AgentSpawned` | 30050 | BC-MESH-AGENTBOX | BC-MESH-VC | ACL-VC↔AGENTBOX (BC20) | mints `urn:visionclaw:execution:*` for projection |
| `AgentTerminated` | 30050 | BC-MESH-AGENTBOX | BC-MESH-VC | ACL-VC↔AGENTBOX | closes execution receipt |
| `MeshServiceListUpdated` | 30033 | any | all | ACL-anyone↔* | replaceable; freshens DID Document service list cache |
| `MeshPeerPing` | 30050 | relay-relay | relay-relay | ACL-relay-relay | health probe, ADR-073 D11 |
| `MemberBanned` | 30910 | BC-MESH-FORUM admin | all | ACL-FORUM↔* (honor_remote_moderation) | mod-cache invalidation cascade |
| `MemberMuted` | 30911 | BC-MESH-FORUM admin | all | same | with TTL |
| `MemberWarned` | 30912 | BC-MESH-FORUM admin | BC-MESH-FORUM | local | not federated by default |
| `MemberReported` | 1984 / 30913 | BC-MESH-FORUM member | BC-MESH-FORUM admin | local | NIP-56 std |
| `EnrichmentProposed` | 30301 (NEW, PRD-013 G7) | BC-MESH-AGENTBOX (agent) | BC-MESH-VC (BrokerActor) | ACL-VC↔AGENTBOX | IS-Envelope kind=`tool_invoke`; agent submits enrichment for broker review |
| `BrokerDecisionRecorded` | 30300 (NEW, PRD-013 G7) | BC-MESH-VC (ServerNostrActor) | BC-MESH-AGENTBOX (agent), external subscribers | ACL-VC↔AGENTBOX | IS-Envelope kind=`tool_result`; broker decision audit event |
| `WriteBackPushed` | internal | BC-MESH-VC (WriteBackSaga) | source GitRemote pod/repo | ACL-VC↔SOLID-POD-RS | NIP-98-signed git push with provenance trailers; triggered only after BC11 approval |
| `KnowledgeEnrichmentCaseCreated` | internal + WS `broker:new_case` | BC-MESH-VC (BrokerActor) | BC-MESH-VC (Decision Canvas), BC-MESH-AGENTBOX (G6 pane) | local + ACL-VC↔AGENTBOX | New `CaseCategory::KnowledgeEnrichment` case for human review |
| `GitRemoteSynced` | internal | BC-MESH-VC (GitIngestService) | BC-MESH-VC (IngestSaga) | local | Incremental fetch completed; changed files forwarded to parser pipeline |

---

## Translation Rules (concrete)

### TR-DID-Resolution

Given a `did:nostr:<hex>` URI in any envelope or HTTP request:

1. Lookup local cache (TTL ≤ 600s).
2. On miss:
   a. Try DID-via-relay: query `mesh.peer_relays` (parallel race) for `Filter { authors: [hex], kinds: [0, 30033], limit: 2 }`. Wait up to 2s.
   b. On no result, try DID-via-`.well-known` for any pod URL associated with this hex (substrate-specific lookup tables).
   c. On no result, return 404 with `error: did-unresolvable, hex: <hex>`.
3. Validate type, `@context`, signature on Schnorr verificationMethod.
4. Cache assembled DID Document with TTL.

Per ADR-074 D5.

### TR-Bead-URN-VC↔Agentbox

Given `urn:agentbox:bead:<scope_hex>:<local>` and the receiver is BC-MESH-VC:

1. Pass URN to BC20's `uris_acl::translate_to_visionclaw`.
2. Algorithm:
   - If `<local>` is a valid `sha256-12-...`: synthesise `urn:visionclaw:bead:<scope_hex>:<local>`. Exact 1:1 mapping.
   - Else (local is a slug/id): mint `urn:visionclaw:bead:<scope_hex>:<sha256-12 of canonicalize(payload_or_id)>`. Lossy; receiver records both forms in `:Bead.canonical_urn` AND `:Bead.agentbox_urn`.
3. Receiver stores `:Bead { canonical_urn, agentbox_urn?, body }` so the round-trip survives.

Reverse direction (`urn:visionclaw:bead:*` → `urn:agentbox:bead:*`):

1. Pass to `uris_acl::translate_to_agentbox`.
2. Use the original `:Bead.agentbox_urn` if present (round-trip preservation).
3. Else mint `urn:agentbox:bead:<scope_hex>:<sha256-12-of-vc-urn>` and accept the asymmetry (agentbox doesn't natively content-address beads, so this is a synthetic-ID).
4. Tag the envelope with `["x-vc-original", original_urn]` so receivers can debug.

### TR-Identity-Lowercase

At every receiver-side ACL:

1. Pubkey hex inputs lowercased before any string compare.
2. WebID URLs trimmed and `?` query stripped before compare (path-only matching for ACLs).
3. `did:nostr:<hex>` constructed only via canonical mint (`mint_did_nostr` Rust / `mint('did:nostr', hex)` JS).

Per ADR-074 D1.

### TR-Delegation-Forward

When a bridge (forum bridge / agentbox `RelayConsumer` / VisionClaw `MeshBridge`) forwards an envelope:

1. If original event has `["delegation", δ_origin→bridge, ...]` tag: forward verbatim (NO re-sign).
2. Else if `mesh.forward_policy == "delegate-required"`: refuse with `OK false "delegation-required"`.
3. Else (`forward_policy == "fwd-meta"`): re-sign with bridge key, ADD `["forwarded-from", original_pubkey, original_id]` tag, document attribution in `via[]`.

Per ADR-074 D10 + PRD-010 F9.

### TR-Moderation-Honour

When a relay receives a federated kind-30910/30911:

1. Verify signer is in `mesh.honor_remote_moderation` (else drop).
2. Apply locally: invalidate own `mod_cache` for target pubkey, persist to local `moderation_actions` D1 mirror.
3. Do NOT forward onward (single-hop honour to prevent moderation cascades).

### TR-IS-Envelope-Validation

At every receiver-side ACL:

1. Required field check (`v`, `to`, `from`, `kind`, `body`).
2. `v == 1` (drop with `OK false "envelope-version-unsupported: <v>"` for unknown versions).
3. `kind ∈ { chat, tool_invoke, tool_result, knowledge_link, moderation, mesh_ping }` (drop unknowns to avoid silent corruption).
4. Per-kind body shape match (D3 of ADR-075).
5. `ttl > now()` if present (drop with `OK false "envelope-expired"`).
6. `len(via) ≤ 4` (drop with `OK false "envelope-via-too-long"`).
7. JCS canonical comparison: re-encode and assert byte-identical (drift detect).

Per ADR-075 D2 + D7 + D9 + D5.

---

## Ubiquitous Language

| Term | Definition |
|------|------------|
| Mesh | The federated set of relays per ADR-073 |
| Substrate | A bounded context running on its own infrastructure (forum / agentbox / VisionClaw) |
| Actor | An entity with a `did:nostr:<hex>` identity (user, agent, operator, bridge) |
| Federation | Bidirectional cross-relay event propagation per ADR-073 D2 |
| IS-Envelope | The cross-system message contract per ADR-075 D1 |
| Trust pivot | NIP-26 delegation grant from one identity to another |
| Bridge | Process that translates between substrate-internal events and Nostr envelopes (forum's relay-worker outbound; agentbox's `RelayConsumer`; VisionClaw's `MeshBridge`) |
| Walled garden | A relay reachable on a public URL but admission-gated by allowlist |
| Sovereign agent | An agentbox container's per-agent did:nostr identity; key persisted to filesystem |
| Operator key | A substrate's federation/server identity, distinct from any user/agent |
| Mesh service-list | Kind-30033 event advertising actor's preferred relays + delegations |
| Federation key | Per-relay key used for relay-relay AUTHed sessions, distinct from any actor key |
| Canonical hex | 64-char lowercase hex pubkey form (ADR-074 D1) |
| Tier-3 DID | Full DID Document with service entries (vs. Tier-1 minimal) |
| Git remote | A registered knowledge source (GitHub, GitLab, Solid pod, bare repo) in VisionClaw's `GitRemote` aggregate; replaces the implicit single-source GitHub model |
| Write-back | The reverse flow from VisionClaw's enrichment pipeline back to the source pod/repo as a git commit, gated through the Judgment Broker |
| Knowledge enrichment | A `BrokerCase` category (PRD-013 G4) representing an agent-proposed mutation to the knowledge base (embedding, ontology promotion, edge proposal, annotation) |
| Provenance trailer | Structured git commit trailer block carrying `Urn:`, `Proposed-by:`, `Approved-by:`, `Reasoning-hash:` fields for non-repudiable audit |
| Pod Bridge | The BC20 module (PRD-013 G5) mediating agent git clone/commit/push operations through VisionClaw's credentials and broker approval |
| Broker Review Surface | The agentbox viewer pane (PRD-013 G6) rendering `KnowledgeEnrichment` cases as visual diffs with inline approval actions |
| Nostr Control Plane | Kind-30300/30301 event layer (PRD-013 G7) carrying broker decisions and enrichment proposals across relay boundaries |

---

## Open Domain Questions

### DQ1 — Where does cross-substrate idempotency belong?

A user X sends DM to agent A. Forum relay accepts; mesh fan-out republishes to agentbox. Agentbox `RelayConsumer` writes to pod inbox. Agent processes inbox, replies via outbox. Outbox publish fans back through agentbox relay, mesh fan-out republishes to forum relay. Forum delivers to user X.

Question: at each hop, who deduplicates? PRD-010 F21 says canonical event id. But the *receiver* sees the same event id from two paths (own relay + mesh fan-out from peer). Idempotency belongs at receiver-side — but we have FOUR receivers (sender's relay, peer relay, peer's `RelayConsumer`, peer's user-side client) and at least two dedup primitives (relay's seen_ids, RelayConsumer's content-addressed-file-existence-check). Need explicit per-receiver responsibility.

*Resolution proposal*: each receiver is responsible for dedup at its tier. Relay tier dedups at storage (D1/sqlite UPSERT-or-IGNORE on `event.id`); RelayConsumer dedups at filesystem (atomic existence check); client UI dedups in-memory by `event.id`. Document in BC-MESH-AGENTBOX-Inv as A-Inv-10 (NEW): "Every receiver tier has an event.id dedup primitive; replays are dropped silently."

### DQ2 — What does VisionClaw do with a forum kind-1 from a non-cohort member?

VisionClaw's mesh subscription includes forum's federated kinds. A non-cohort forum user posts a kind-1; agentbox honours forum's whitelist; but VisionClaw doesn't have access to forum's D1. Should VisionClaw drop, accept, or check?

*Resolution proposal*: VisionClaw accepts based on its own `mesh.allowed_remote_dids` list — which by default mirrors forum's known cohort members (synced at session start). Drift between forum and VisionClaw's known-members lists is an operational concern, not a domain one. Document under DQ-out-of-scope.

### DQ3 — How does delegation expire?

NIP-26 conditions support `created_at<T`. But the delegation itself is a *signed token*; once signed, it's valid forever absent expiry conditions. If user U delegates to agent A with no `created_at<T`, A's events are valid until U revokes. How is revocation expressed?

*Resolution proposal*: A revocation is a kind-30033 mesh-service-list event from U with the delegation REMOVED from `tags[]`. Receivers MUST check current kind-30033 before honouring stale delegations: cache TTL ≤ 24h. Add to ADR-074 D9 in next iteration.

### DQ4 — Multi-agent agentbox per container

Today `AGENTBOX_AGENT_ID = "agentbox-core"` (one per container). Can a container host agents A1, A2, A3 simultaneously, each with own `did:nostr:<hex>`?

*Resolution proposal*: P5 follow-up. `RelayConsumer` already takes `npubs: [...]` (line 156 of relay-consumer.js); `sovereign-bootstrap.py` would extend to mint per-agent identity files; flake.nix supervisord would parameterise. Out of scope for PRD-010. Document as A-Inv-11 (planned).

---

## V2 Extension — Five-Substrate Ecosystem (PRD-011 forum kit extraction)

### Rationale for the extension

PRD-011 extracts the community forum from `dreamlab-ai-website` into a reusable kit hosted at `DreamLab-AI/nostr-rust-forum` (canonical kit; public product `nostr-bbs-rs`; internal brand "VisionFlow forum"). The 4-substrate model in this document treated forum + agentbox + VisionClaw + solid-pod-rs as the mesh substrates. Post-PRD-011 there are **five** substrates because the **kit** and the **DreamLab-specific consumer** are now distinct contexts with different responsibilities:

- **Kit**: generic, branding-free, configurable via TOML, federation-native, consumed by N operators
- **DreamLab consumer**: kit + branding/config, deployed at dreamlab-ai.com, one of N consumers

This extension adds the kit as **BC-MESH-FORUM-KIT** and demotes the existing BC-MESH-FORUM to **BC-MESH-DREAMLAB-CONSUMER** (the deployed instance using the kit). The original BC-MESH-FORUM section above remains as the authoritative reference for the *deployed instance* — this extension layers the *kit-as-substrate* concept on top.

### V5 — BC-MESH-FORUM-KIT — VisionFlow Forum Kit Context

**Owner**: nostr-rust-forum maintainers (DreamLab-AI org).

**Mission**: provide a reusable Rust-crate forum substrate that any operator can deploy via TOML configuration. The kit is the upstream of all forum deployments (DreamLab and otherwise).

**Aggregates** (kit-internal; distinct from any deployment instance):

- **`KitDeployment`**: { `deployment_id`, `manifest_toml_path`, `kit_version`, `topology: standalone|federated|client|multi-tenant|HA|air-gapped` (per ADR-080), `boot_state` }. The runtime aggregate when a kit deployment is alive.
- **`OperatorIdentity`**: { `pubkey_hex`, `custody_tier: tier-1|tier-2|tier-3` (per ADR-081 D1), `rotation_cadence`, `last_rotated_at` }. One per role per deployment per ADR-081.
- **`ForumSetupSession`**: { `run_id`, `provider: claude-code|codex|agentbox-nostr|anthropic|openai`, `conversation_history`, `partial_toml`, `validated_toml` }. The skill-driven authoring session per ADR-079.
- **`Fixture`**: { `spec`, `version`, `source_pin`, `vectors[]`, `coverage_substrates[]` }. ADR-082 fixture aggregates living in the master `docs/specs/fixtures/` repo.
- **`KitContract`**: { `contract_id`, `version`, `consumers[]: substrate_id[]` }. The cross-substrate contract per ADR-077 P2 + ADR-082 D7.

**Invariants** (BC-MESH-FORUM-KIT-Inv):

- **K-Inv-01**: Zero `dreamlab` substring matches in `nostr-rust-forum/` repo (excluding contributor names in commit history). Anti-drift CI lint per PRD-011 F2.6 enforces.
- **K-Inv-02**: All NIP protocol primitives delegate to upstream `nostr` crate (per ADR-076 + ADR-078 B1 + PRD-011 F25). Kit's `nostr-bbs-core` is a thin shim ≤700 LOC.
- **K-Inv-03**: WebAuthn delegates to `webauthn-rs = "0.5"` + `passkey-types = "0.3"` (per ADR-078 B3 + PRD-011 F25).
- **K-Inv-04**: Every TOML configuration validated against `nostr-bbs-config` schema before deployment (per PRD-011 F3.3).
- **K-Inv-05**: `[mesh].mode` flag toggles federation per ADR-073 D6; mode is operator-set, kit-respected. Default standalone.
- **K-Inv-06**: Kit emits kind-30033 mesh service-list at boot when `[mesh].mode != "standalone"` (per ADR-074 D9).
- **K-Inv-07**: Kit's `verificationMethod.type` always `SchnorrSecp256k1VerificationKey2019` (per ADR-074 D2 + Q1 finding C3).
- **K-Inv-08**: Federation key custody honours `[custody]` declarations per ADR-081 D2.
- **K-Inv-09**: Forum-setup skill never exposes operator secret material to LLM context (per ADR-079 D12).
- **K-Inv-10**: All cross-substrate contract tests (ADR-082 L2) pass before kit GA tag.

**Public surface**:

- crates.io: `nostr-bbs-core`, `nostr-bbs-config`, `nostr-bbs-mesh`, `nostr-bbs-relay-worker`, `nostr-bbs-pod-worker`, `nostr-bbs-auth-worker`, `nostr-bbs-search-worker`, `nostr-bbs-preview-worker`, `nostr-bbs-forum-client`, `nostr-bbs-admin-cli`, `nostr-bbs-setup-skill` (all per PRD-011 §5.1).
- GitHub: `https://github.com/DreamLab-AI/nostr-rust-forum`.
- Docker Hub (pending): kit container images for one-shot evaluation.
- ADR/PRD set: `docs/adr/ADR-001..` and `docs/prd/PRD-001..` per PRD-011 F9.3 (kit's own numbering, not visionclaw monorepo's).

**ACLs at boundary**:
- **Upstream (kit consumes)**: `nostr` crate, `webauthn-rs`, `solid-pod-rs` (post 0.5 absorption), RustCrypto primitives.
- **Downstream (consumers of kit)**: dreamlab-ai-website's `forum-config/` package, plus N other operator packages.

### V6 — BC-MESH-DREAMLAB-CONSUMER — Dreamlab-Ai-Website Downstream Context

**Owner**: dreamlab-ai-website maintainers (the website team).

**Mission**: deploy DreamLab's flagship public forum at `dreamlab-ai.com/community/` by consuming the VisionFlow forum kit + supplying DreamLab-specific configuration and branding.

This bounded context **supersedes** the BC-MESH-FORUM context defined in the V1 4-substrate section above. The aggregates (`Member`, `AuthSession`, `ModerationAction`, `ProfilesRow`, `Pod`) MOVE FROM BC-MESH-FORUM into BC-MESH-DREAMLAB-CONSUMER (they are deployment-instance state, not kit-internal state). The invariants F-Inv-01..07 remain valid in this consumer's bounded context.

**Additional consumer-specific aggregates**:

- **`DreamlabBrandingPackage`**: { `theme_colours`, `logos`, `copy_strings`, `cohort_names: ["lobby", "members", "trusted"]`, `welcome_bot_pubkey`, `admin_pubkeys[]` }. The DreamLab-specific identity that lives in `forum-config/dreamlab.toml`.
- **`CutoverState`**: { `routing_mode: old-only|new-canary|new-50|new-95|new-only`, `phase_start`, `parity_metrics`, `rollback_triggers[]` }. Per ADR-083 D2.
- **`DreamlabAdminCohort`**: the existing DreamLab admin pubkey set, preserved through migration per ADR-083 D5.

**Additional consumer invariants**:

- **DC-Inv-01**: `forum-config/dreamlab.toml` exists and validates against kit's `nostr-bbs-config` schema.
- **DC-Inv-02**: WebAuthn `rp_id = "dreamlab-ai.com"` and `expected_origin = "https://dreamlab-ai.com"` preserved across cutover (per ADR-083 D5 session continuity invariant).
- **DC-Inv-03**: PRF info string `"nostr-secp256k1-v1"` is byte-identical to kit default (per K-Inv-04 + DC-Inv-02 jointly enforce that DreamLab passkey-PRF nsecs continue to derive correctly).
- **DC-Inv-04**: Existing D1 schema is preserved unchanged (per ADR-083 D4 schema parity invariant).
- **DC-Inv-05**: `community-forum-rs/` subdirectory MUST be deleted at T₇ per ADR-083 D12 (post-cutover cleanup).

### V7 — Inter-context relationship: Upstream/Downstream

```
BC-MESH-FORUM-KIT (upstream)
       ↓ consumes via Cargo deps
BC-MESH-DREAMLAB-CONSUMER (downstream)
       ↓ deploys at
dreamlab-ai.com/community/
```

The relationship is **upstream/downstream** in DDD terminology (Eric Evans Ch. 14). Kit publishes contracts; consumer adapts to them. No cyclic dependencies.

Other consumers (third-party operators) sit in parallel to BC-MESH-DREAMLAB-CONSUMER, all downstream of BC-MESH-FORUM-KIT. Each maintains its own `<deployment>.toml` + branding package.

### V8 — New ACL: ACL-KIT↔CONSUMER

**Location**: implicit at the Cargo dependency boundary; no Rust code performs translation because the kit's public types are the consumer's input types.

**Translates**:
- TOML configuration → kit runtime state (handled by `nostr-bbs-config` validator).
- Operator branding → kit's branding extension points (CSS variables, slot components, copy keys).

**Validates**:
- TOML schema conformance (DC-Inv-01).
- WebAuthn rp_id continuity (DC-Inv-02).
- PRF info string equality (DC-Inv-03).
- Schema parity with kit's expected D1 layout (DC-Inv-04).

**Never leaks**:
- Operator secrets (admin pubkeys are public; private keys never enter kit's process — see ADR-079 D12).
- DreamLab-specific cohort logic into kit defaults (anti-drift K-Inv-01).

### V9 — New domain events for the 5-substrate model

| Domain event | Kind / shape | From context | To context | ACL |
|--------------|--------------|--------------|------------|-----|
| `KitVersionPublished` | crates.io publish + GitHub release | BC-MESH-FORUM-KIT | BC-MESH-DREAMLAB-CONSUMER (and other downstream) | ACL-KIT↔CONSUMER |
| `OperatorTomlValidated` | local CLI invocation | BC-MESH-DREAMLAB-CONSUMER | BC-MESH-FORUM-KIT (boot acceptance) | ACL-KIT↔CONSUMER |
| `KitFederationKeyRotated` | kind-30033 publish (per ADR-074 D9) | BC-MESH-DREAMLAB-CONSUMER | all mesh peers | ACL-KIT↔FORUM↔AGENTBOX↔VC |
| `CutoverPhaseTransitioned` | ROUTING_MODE secret update | BC-MESH-DREAMLAB-CONSUMER | itself (router-worker) | local |
| `RollbackTriggered` | ROUTING_MODE=old-only secret | BC-MESH-DREAMLAB-CONSUMER | itself + observability | ACL-CUTOVER (per ADR-083 D9) |
| `ForumSetupSessionCompleted` | TOML written to disk | BC-MESH-FORUM-KIT (skill) | new operator's deployment | ACL-KIT↔CONSUMER |
| `FixtureRefreshTriggered` | UPSTREAM_PINS.md PR | BC-MESH-VC (master fixture host) | all 5 substrates | ACL-FIXTURE-SHARING |

### V10 — New translation rule: TR-Kit-Boot-Verification

When a kit deployment boots:

1. Load `<deployment>.toml`.
2. Validate against `nostr-bbs-config` schema → reject with operator-facing error if invalid.
3. Resolve `[custody]` declarations per ADR-081 D2 → fetch keys from filesystem/secret-store/HSM.
4. Verify K-Inv-07 (`verificationMethod.type` constant) against the kit's compiled-in expected value.
5. Verify D8 anti-collision (no role-pubkey reuse per ADR-081 D8).
6. Verify D5 file permissions per ADR-081 D5.
7. Apply branding overrides from `[branding]` section.
8. Initialise mesh worker per ADR-073 if `[mesh].mode != "standalone"`.
9. Boot complete → `/health/qe` reports operational.

Each step is an invariant gate; failure is fail-closed with operator-facing remediation message.

### V11 — Updated ubiquitous language additions

| Term | Definition |
|------|------------|
| Kit | The reusable forum substrate at `nostr-rust-forum`; product name `nostr-bbs-rs`; internal brand "VisionFlow forum" |
| Consumer | A package that depends on the kit + supplies a TOML config (e.g. `dreamlab-ai-website/forum-config`) |
| Branding package | Operator-supplied overrides of the kit's defaults |
| Cutover | The migration from `dreamlab-ai-website/community-forum-rs/` (legacy fork) to the kit + `forum-config/` consumer pattern, per ADR-083 |
| Router-worker | The CF Worker traffic-split component during cutover (ADR-083 D2) |
| Federation key | Per ADR-073 D4 + ADR-081; the relay-relay AUTH key, distinct from operator and bridge keys |
| Custody tier | Filesystem (Tier-1) / cloud secret store (Tier-2) / hardware HSM (Tier-3) per ADR-081 D1 |
| Forum-setup skill | The provider-abstracted AI configurator per ADR-079 |
| Fixture | A canonical reference test vector or contract assertion per ADR-082 |
| UPSTREAM_PINS.md | The lockfile tracking external test vector source commits per ADR-082 D2 |

### V12 — Open questions for the 5-substrate evolution

#### DQ5 — Does the kit ship with default zone names?
PRD-011 §5.2 specifies optional `[[zones]]` blocks; if omitted, kit defaults to 3-zone (public/members/private). DreamLab's existing cohort names (`lobby`, `members`, `trusted`) get supplied via `dreamlab.toml` overrides. **Resolved**: kit defaults yes; consumers override via `[[zones]]`.

#### DQ6 — Trust progression cross-deployment
A user with TL3 status on Forum A — does that status carry to Forum B? Kit's `[trust]` block is per-deployment. **Resolution**: trust is per-deployment by default; cross-deployment trust portability is a P5+ feature that would require shared D1 / cross-deployment kind-30033 trust certificates. Out of scope for v3.0.0.

#### DQ7 — Welcome bot identity custody
Per K-Inv-08, welcome bot key is configurable per `[custody]`. But welcome bot keys often need to be filesystem-resident for low-latency bot operation. **Resolution**: kit defaults welcome_bot to Tier-1 (filesystem); operators upgrading to Tier-2/3 accept slight latency penalty.

#### DQ8 — Cutover rollback after T₇ deletion
Once `community-forum-rs/` is deleted at T₇, can DreamLab still rollback? **Resolution**: yes via git revert + redeploy of the deletion commit; full restoration restores the legacy stack. But the CF Workers KV/D1/R2 schemas have continued evolving, so post-T₇ rollback is much costlier than pre-T₇. ADR-083 D9 explicitly does not include "rollback after T₇ + 7 days" as a primary recovery path.

---

## V13 — BC-MESH-DREAMLAB-CONSUMER aggregates extended (PRD-012 / ADR-084 / ADR-085)

PRD-012, ADR-084, and ADR-085 specify the engineering work that takes BC-MESH-DREAMLAB-CONSUMER from "concept" to "deployed package". The V6 section above defined the bounded context conceptually; this V13 extension adds the concrete aggregates + invariants that the consumer's `forum-config/` Cargo package owns.

**Additional consumer-specific aggregates** (post PRD-012):

- **`ForumConfigPackage`**: { `cargo_workspace_root: forum-config/`, `kit_version_pin: "3.0"`, `wrangler_manifests: [auth, pod, relay, search, preview]`, `branding_module: src/dreamlab_branding.rs`, `dreamlab_toml_path` }. The Cargo package itself; built per ADR-085 D2.
- **`CloudResourceMapping`**: { `d1: { auth: "<id>", relay: "<id>" }`, `kv: { sessions, pod_meta, admin_kv, admin_kv_ro, nip98_replay, search_config, rate_limit }`, `r2: ["dreamlab-pods", "dreamlab-vectors"]`, `do: { relay: NostrRelayDO }`, `routes: [...]` }. Per ADR-084 D1+D2; resource-ID preservation invariant. Frozen during transition; identity guaranteed pre/post cutover.
- **`BrandingExtensionConfig`**: { `theme_colours`, `copy_strings`, `logo_url`, `favicon_url`, `custom_css_url`, `og_image_url`, `zone_display_overrides[]` }. Defined in `src/dreamlab_branding.rs`; consumed by kit's `BrandingConfig` extension API per ADR-085 D4.
- **`KitExtensionContract`**: { `dispatch(req, env, ctx, config, branding) -> Result<Response>` for workers; `mount_with_config(config, branding)` for forum-client; `BrandingConfig` shape; `Config` shape }. The kit's binding API per ADR-085 D4. Stable across kit minor versions.

**Additional consumer invariants** (extending DC-Inv-01..05 above):

- **DC-Inv-06** (PRD-012 F4 + ADR-084 D9): every D1 / KV / R2 / DO / route ID in `forum-config/deploy/*.wrangler.toml` MUST exist in live CF state pre-deploy. Pre-deploy validation gate enforces.
- **DC-Inv-07** (ADR-084 D3): consumer worker `name` field exactly matches legacy worker name, enabling D2 zero-downtime route handoff + D4 secrets preservation.
- **DC-Inv-08** (ADR-085 D9): no hardcoded DreamLab strings outside `src/dreamlab_branding.rs` and `dreamlab.toml`. Anti-drift lint enforces per PR.
- **DC-Inv-09** (PRD-012 F8): Sprint Carry-Over Fixture Suite (PRD-011 G6) MUST pass against `forum-config/` deployment in staging before T₃ cutover begins.
- **DC-Inv-10** (ADR-084 D6): `dreamlab.toml` baked into worker WASM via `include_str!`; not separately deployed to KV. Operator config changes require redeploy (acceptable trade-off; admin changes infrequent).
- **DC-Inv-11** (ADR-085 D2): `forum-config/` is its own independent Cargo workspace; not a sub-member of an outer workspace.
- **DC-Inv-12** (ADR-084 D10 + DC-Inv-04): pre-deploy schema sentinel test confirms no D1 schema divergence between legacy and consumer stacks.
- **DC-Inv-13** (ADR-085 D6 DO class re-export): kit's `nostr-bbs-relay-worker` MUST export `NostrRelayDO` with the EXACT class name; consumer wrangler manifest binds to existing DO IDs.

### Updated domain events for the consumer transition

| Domain event | Kind / shape | From context | To context | ACL |
|--------------|--------------|--------------|------------|-----|
| `ForumConfigVersionTagged` | git tag + Cargo.lock update | BC-MESH-DREAMLAB-CONSUMER | itself (boot) | local |
| `KitDepBumpedInForumConfig` | Cargo.toml edit | BC-MESH-DREAMLAB-CONSUMER | itself (CI gates) | local |
| `DreamlabTomlEdited` | git commit on `dreamlab.toml` | BC-MESH-DREAMLAB-CONSUMER | itself + CF Worker boot | local (post-deploy) |
| `BrandingShimEdited` | git commit on `src/dreamlab_branding.rs` | BC-MESH-DREAMLAB-CONSUMER | itself + WASM rebuild | local |
| `CloudResourceMappingValidated` | pre-deploy CI gate (ADR-084 D9) | BC-MESH-DREAMLAB-CONSUMER | itself | local |
| `SchemaSentinelChecked` | pre-deploy sentinel test (ADR-084 D10) | BC-MESH-DREAMLAB-CONSUMER (staging) | BC-MESH-DREAMLAB-CONSUMER (production) | local |
| `SprintCarryOverFixtureSuitePassed` | nightly CI run | BC-MESH-DREAMLAB-CONSUMER | itself | local |
| `CutoverMileStoneReached` | T₃ / T₄ / T₅ / T₆ events per ADR-083 | BC-MESH-DREAMLAB-CONSUMER | itself + observability | local |

### Updated translation rules

#### TR-Consumer-Boot (consumer-specific)

When a `forum-config/` worker boots:
1. Load `dreamlab.toml` (baked at compile time via `include_str!`).
2. Validate via `nostr-bbs-config::Config::from_toml` → fail-closed on schema mismatch.
3. Load `dreamlab_branding()` from `src/dreamlab_branding.rs`.
4. Resolve `[custody]` declarations → fetch keys from CF Workers Secrets per ADR-081 D2 Tier-2.
5. Verify resource bindings (D1/KV/R2/DO) match the manifest expectations → log warning if drift detected (post-deploy, can't abort here but operator gets signal).
6. Initialise mesh worker if `[mesh].mode != "standalone"` per ADR-073 D6.
7. Enter CF Worker `[event(fetch)]` loop.

#### TR-Resource-ID-Preservation

When PR modifies `forum-config/deploy/*.wrangler.toml`:
1. CI extracts every D1 ID, KV ID, R2 bucket name, DO class name, route pattern.
2. Compares against live CF state (via `wrangler list` commands).
3. Any DRIFT (id present in manifest but not live; or vice versa) → CI fails.
4. Mismatch is opt-in: operator can override via `--allow-resource-drift` flag with explicit reason in commit message (used during initial X1 setup before CF resources exist).

## V14 — Five-substrate ecosystem complete bounded context map

After PRD-012 + ADR-084/085 land, the ecosystem's bounded contexts are:

```
                     ┌────────────────────────────┐
                     │ BC-MESH-FORUM-KIT (V5)     │
                     │ (nostr-rust-forum)         │
                     │ Generic configurable kit   │
                     └────────────┬───────────────┘
                                  │ Cargo dep (per ADR-085 D2)
                                  ↓
                     ┌────────────────────────────┐
                     │ BC-MESH-DREAMLAB-CONSUMER  │
                     │ (V6 + V13)                 │
                     │ DreamLab-specific config   │
                     │ (forum-config/)            │
                     └─────────────┬──────────────┘
                                   │ wrangler deploy
                                   ↓
                     ┌──────────────────────────────┐
                     │ Cloud Resource Aggregate     │
                     │ (CF D1/KV/R2/DO/Routes — IDs │
                     │  preserved per ADR-084 D1)   │
                     └──────────────────────────────┘
                                   │ same DO/D1/KV/R2 used by
                                   ↓
                     ┌──────────────────────────────┐
                     │ BC-MESH-AGENTBOX (V2)        │
                     │ + BC-MESH-VC (V3)            │
                     │ (mesh participation)         │
                     └──────────────────────────────┘
```

ACLs:
- **ACL-KIT↔CONSUMER** (V8 above) — Cargo dep boundary; TOML config + branding shim translate
- **ACL-CONSUMER↔CLOUD** (NEW per ADR-084) — wrangler boundary; resource IDs preserved
- **ACL-VC↔FORUM** (V1 §V7) — now applies to the deployed-instance pair (consumer + cloud), not the kit itself
- **ACL-VC↔AGENTBOX** (V1 §BC20) — unchanged

## V15 — Git Ingest Surface and Agent-Mediated Knowledge Federation (PRD-013)

### Rationale for the extension

PRD-013 replaces VisionClaw's GitHub REST API ingest with a git-over-HTTP ingest surface that treats every knowledge source identically (GitHub, GitLab, Solid pod, bare git repo). It adds a write-back path gated through the Judgment Broker (BC11), and introduces agent-mediated enrichment with full `did:nostr` provenance. This extension touches BC-MESH-VISIONCLAW (BC2 Graph Data), BC11 (Judgment Broker), BC20 (Agentbox Integration), and BC13 (Discovery) without creating new bounded contexts.

**Implementation status (2026-05-08):** The BC2 git-ingest bounded context additions are implemented in `src/services/git_ingest/` and `src/handlers/git_ingest_handler.rs`:
- **GitRemote aggregate**: full CRUD via REST surface at `/api/ingest/remotes` (list, create, get, update, delete, trigger sync, status query). Legacy `GITHUB_*` env vars auto-register at boot.
- **WriteBackSaga**: complete 5-phase saga (fetch, apply, commit, push, audit) with conflict detection, NIP-98-signed transport, and Neo4j audit trail recording.
- **ProvenanceTrailer encoder**: `provenance.rs` encodes all seven trailer fields (`Urn:`, `Proposed-by:`, `Approved-by:`, `Broker-case:`, `Decision:`, `Reasoning-hash:`, `Signed-off-by:`) per V-Inv-08.
- **REST surface**: six endpoints under `/api/ingest/remotes` with auth guard, plus manual sync trigger and status query.

### V15.1 — BC2 (Graph Data) gains `GitRemote` aggregate

The `GitRemote` aggregate (defined in BC-MESH-VISIONCLAW aggregates above) is the persistent representation of a registered knowledge source. It replaces the implicit single-remote model expressed by `GITHUB_TOKEN`/`GITHUB_OWNER`/`GITHUB_REPO` env vars with an explicit, multi-remote registry.

The **Git Ingest Adapter** (PRD-013 G1) lives entirely within BC2. It replaces `GitHubClient` + `EnhancedContentAPI` with a local-clone-based pipeline: `GitRemote` registry lookup -> `git2` clone/fetch with auth injection (PAT or NIP-98) -> local worktree on disk -> existing parser pipeline (`KnowledgeGraphParser`, `OntologyParser`, `block_level_parser`) -> `IngestSaga` (ADR-051).

New code location: `src/services/git_ingest/` (modules: `mod.rs`, `remote_registry.rs`, `git_ingest_service.rs`, `provenance.rs`, `writeback_saga.rs`).

### V15.2 — BC11 (Judgment Broker) gains `CaseCategory::KnowledgeEnrichment`

The existing `CaseCategory` enum (ADR-041) gains a new variant:

```rust
pub enum CaseCategory {
    ContributorMeshShare,
    WorkflowReview,
    PolicyException,
    TrustAlert,
    ManualSubmission,
    KnowledgeEnrichment,  // NEW: PRD-013 — write-back gating
}
```

`KnowledgeEnrichment` cases carry a `SubjectRef` pointing at the enriched `KGNode` or `OntologyClass`, with `from_state` and `to_state` representing the enrichment type (e.g., `None -> Embedding`, `KGNode -> OntologyClass`, `None -> ProposedEdge`). All six decision outcomes apply:

- **Approve** triggers `WriteBackSaga` push to source remote.
- **Reject** blocks the push; enrichment stays in Neo4j only.
- **Amend** returns to the proposing agent for modification.
- **Delegate** routes to a domain expert.
- **Promote** elevates and pushes (ontology-level promotion).
- **Precedent** flags the enrichment type for future auto-approval (PRD-013 Phase 5 deferred).

### V15.3 — BC20 (Agentbox Integration) gains Pod Bridge (G5)

The BC20 anti-corruption layer gains a new module `pod_bridge.rs` (PRD-013 G5) that mediates agent git clone/commit/push operations. The Pod Bridge:

1. **Exposes git clone** to agents via the management API (port 9190). Agents request a clone of a registered `GitRemote`; the bridge performs the clone using VisionClaw's server identity credentials and mounts the worktree into the agent's sandbox.

2. **Collects agent commits** after reasoning completes. The agent commits to a local branch; the bridge reads the commits, validates provenance trailers (V-Inv-08), and submits a `BrokerCase(CaseCategory::KnowledgeEnrichment)` for human review via BC11.

3. **Relays approval events** via the embedded nostr-rs-relay (kind 30300). When the broker approves, the bridge pushes the approved commits to the source remote with NIP-98-signed transport.

4. **Maps agent identity** to commit provenance: agent commits carry `Proposed-by: did:nostr:<agent-hex>` and `Signed-off-by: did:nostr:<server-hex>` trailers. The agent cannot push autonomously (V-Inv-09).

### V15.4 — New context relationship: BC2 <-> BC11 via KnowledgeEnrichment

Enrichments discovered by the BC13 Discovery Engine (PRD-009 pipeline: embeddings, gap detection, related-node proposals, ontology promotions) flow into BC11 as `KnowledgeEnrichment` cases for broker review. On approval, the decision flows back to BC2 where the `WriteBackSaga` commits the enrichment to the source pod/repo.

```
BC13 (Discovery Engine)
    │ discovers enrichment candidates
    ▼
BC2 (Graph Data — GitRemote / GitIngestService)
    │ creates BrokerCase with enrichment payload
    ▼
BC11 (Judgment Broker — DecisionOrchestrator)
    │ human reviews via Decision Canvas / G6 pane
    │ DecisionOutcome::Approve or ::Promote
    ▼
BC2 (Graph Data — WriteBackSaga)
    │ commits to source remote with provenance (G3)
    │ records push result in Neo4j audit trail
    ▼
Source pod/repo (git push with NIP-98 signed transport)
```

This is a **customer-supplier** relationship: BC2 defines the enrichment shape and submits it; BC11 reviews it and returns a decision. BC2 is the customer (it needs a decision); BC11 is the supplier (it provides the decision service). Neither context leaks its internals to the other: BC2 submits a `BrokerCase` with opaque `SubjectRef`; BC11 returns a `DecisionOutcome` without knowledge of git or pod semantics.

### V15.5 — Broker Review Surface (G6) as presentation layer

The Broker Review Surface (PRD-013 G6) is a **presentation-layer concern**, not a bounded context. It is an agentbox viewer pane (`enrichment-review-pane.js`) that composes data from BC11 (broker cases, decision outcomes) and BC2 (enrichment payloads, source content) into a two-pane diff view with inline approval actions.

**Data sources:**
- VisionClaw BrokerActor: WebSocket events `broker:new_case`, `broker:case_decided`, `broker:case_claimed`
- VisionClaw REST: `GET /api/broker/inbox`, `POST /api/broker/cases/:id/decide`
- BC2 enrichment payload: `from_state` (source content) and `to_state` (proposed enrichment)

**Rendering:**
- Markdown rendering for `.md` changes
- Syntax-highlighted Turtle for `.ttl` OWL fragments
- Tabular display for `.embeddings.json` vectors
- Provenance trailer block (G3) shown below the diff

The pane uses the agentbox LOSOS pane contract (existing, surface `S12`). It is pane number 7 alongside the existing 6 panes (VC, provenance, capability, runtime, DCAT, handoff).

### V15.6 — Nostr Control Plane extension (G7)

PRD-013 G7 extends the existing relay topology (ADR-073) with two new event kinds for the git ingest control plane:

| Kind | Name | Purpose | Producer | Consumer |
|------|------|---------|----------|----------|
| 30300 | `AuditEvent` (implemented) | Broker decision recorded (approve/reject/amend/delegate/promote/precedent) | VisionClaw ServerNostrActor | Agentbox agents, external subscribers |
| 30301 | `EnrichmentProposal` (implemented) | Agent submits enrichment for broker review | Agentbox agent (via Pod Bridge) | VisionClaw BrokerActor |
| 31400 | `PanelDefinition` (implemented) | Register/update governance control panel (NIP-33 replaceable, keyed by `d` tag) | VisionClaw ServerNostrActor via BrokerActor | Forum Kit UI, any Nostr client |
| 31402 | `ActionRequest` (implemented) | Case submitted for human review with priority, category, fields, reasoning | VisionClaw ServerNostrActor via BrokerActor | Forum Kit UI, broker clients |

Kinds 31400 and 31402 are Agent Control Surface Protocol events (compatible with `nostr-bbs-core` from the Forum Kit). They extend the control plane with structured governance UI primitives, enabling the Forum Kit relay to render broker inboxes and action panels directly from Nostr events.

IS-Envelope mapping (per ADR-075):
- kind 30301 maps to IS-Envelope kind `tool_invoke` (agent submits enrichment)
- kind 30300 maps to IS-Envelope kind `tool_result` (broker decision result)
- kind 31400 maps to IS-Envelope kind `panel_register` (governance panel definition)
- kind 31402 maps to IS-Envelope kind `action_submit` (action request for human review)

The NIP-42 AUTH gate on the agentbox embedded relay restricts kind-30300/30301 subscriptions to known `did:nostr` pubkeys. All events in the control plane carry `did:nostr` identity; no anonymous events are permitted.

The Nostr Control Plane is **optional**. The broker REST API + WebSocket is the primary path for MVP. Nostr adds: push notifications across trust boundaries, human feedback via any Nostr client, agent-to-agent coordination for multi-agent enrichment workflows, and audit event durability (events persisted in relay, not just Neo4j).

### V15.7 — New translation rules

#### TR-Enrichment-Proposal-Ingest

When VisionClaw's BrokerActor receives a kind-30301 event via relay subscription:

1. Verify Schnorr signature (V-Inv-04).
2. Extract IS-Envelope payload; validate kind = `tool_invoke` (TR-IS-Envelope-Validation).
3. Parse `subj` field as `urn:agentbox:*` or `urn:visionclaw:*` — apply BC20 URI translation if needed.
4. Extract `from_state` and `to_state` from envelope body.
5. Create `BrokerCase(CaseCategory::KnowledgeEnrichment, SubjectRef(translated_urn), from_state, to_state)`.
6. Emit `broker:new_case` WebSocket event to connected reviewers.

#### TR-WriteBack-Push

When `WriteBackSaga::execute` runs after broker approval:

1. Look up `GitRemote` by `remote_id` from the `BrokerCase` metadata.
2. `git fetch` latest from remote (detect conflicts; fail-and-notify if diverged).
3. Apply enrichment to local worktree (file format depends on enrichment type).
4. Commit with provenance trailers (G3 encoder): `Urn:`, `Proposed-by:`, `Approved-by:`, `Broker-case:`, `Decision:`, `Reasoning-hash:`, `Timestamp:`, `Signed-off-by:`.
5. `git push` with NIP-98-signed HTTP headers (V-Inv-09 enforced: push blocked if no approval).
6. Record push result in Neo4j audit trail.
7. Emit kind-30300 audit event via ServerNostrActor if Nostr Control Plane enabled.

### V15.8 — Open questions (PRD-013)

#### DQ9 — Conflict resolution on write-back push

If the source pod has diverged since the last fetch, should the WriteBackSaga rebase, merge, or fail? Proposal: fail-and-notify on conflict; the broker re-reviews after manual resolution. Auto-merge is dangerous for knowledge bases where structural integrity matters.

#### DQ10 — Multi-remote write-back target

If the same KGNode is ingested from two remotes (e.g., GitHub mirror + Solid pod), which remote receives the write-back? Proposal: the remote explicitly marked `writeback_enabled = true`; if multiple, the one with the most recent `last_sync`.

#### DQ11 — Precedent-based auto-approval scope

The `DecisionOutcome::Precedent` path (ADR-041) could enable auto-approval for enrichment types approved N times, reducing broker fatigue for routine embedding updates. Deferred to PRD-013 Phase 5 + Phase 7. Requires careful scoping: which enrichment types qualify? What is the confidence threshold?

---

## References (extension)

- PRD-011 — VisionFlow Forum Kit Extraction (drives BC-MESH-FORUM-KIT context)
- PRD-012 — DreamLab Website Kit Adoption (drives BC-MESH-DREAMLAB-CONSUMER V13 extension)
- ADR-073 — Mesh topology
- ADR-074 — DID:Nostr canonicalisation
- ADR-075 — IS-Envelope contract
- ADR-076 — Forum nostr-core absorption (kit applies from inception)
- ADR-077 — Ecosystem QE policy
- ADR-078 — Cross-substrate library convergence
- ADR-079 — Forum-Setup Skill Provider Abstraction
- ADR-080 — Forum Kit Deployment Topology Patterns
- ADR-081 — Federation key custody & rotation
- ADR-082 — Cross-substrate test fixture sharing
- ADR-083 — `dreamlab-ai-website` Cutover Migration Pattern
- ADR-084 — Cloud Infrastructure Mapping for Kit Consumers (V13 invariants)
- ADR-085 — `forum-config/` Package Architecture (V13 aggregates)
- PRD-013 — Solid Pod Git Ingest Surface (drives V15 extension: GitRemote aggregate, KnowledgeEnrichment case category, Pod Bridge, Nostr Control Plane)
- ADR-086 — Git Ingest Surface Architecture (companion to PRD-013, to be written on acceptance)

GitHub repos in the 5-substrate ecosystem:
- https://github.com/DreamLab-AI/VisionClaw (this monorepo, mesh integration substrate, master fixture host)
- https://github.com/DreamLab-AI/nostr-rust-forum (canonical kit; product `nostr-bbs-rs`; internal brand "VisionFlow forum")
- https://github.com/DreamLab-AI/dreamlab-ai-website (downstream consumer of kit; cutover target per ADR-083)
- https://github.com/DreamLab-AI/agentbox (mesh peer + skill provider for forum-setup)
- https://github.com/DreamLab-AI/solid-pod-rs (foundation library, post 0.5 absorption)

## References (original 4-substrate model)

- PRD-010 — DID:Nostr Mesh Federation
- ADR-073 — Private Nostr Relay Mesh Topology & NIP-42 AUTH
- ADR-074 — Cross-System DID:Nostr Canonicalisation & NIP-26 Trust Pivot
- ADR-075 — Inter-System Message Envelope (IS-Envelope v1)
- ADR-076 — Forum `nostr-core` absorption into upstream `nostr` crate
- `docs/ddd-agentbox-integration-context.md` — predecessor BC20 design (this DDD subsumes its mesh-federation-relevant scope; BC20 internals remain authoritative there)
- `agentbox/docs/reference/ddd/DDD-003-sovereign-messaging-domain.md` — agentbox-side messaging domain
- `agentbox/docs/reference/ddd/DDD-004-linked-data-interchange-domain.md` — agentbox-side LD encoding
- `docs/integration-research/01..06-*.md` — evidence corpus
- Eric Evans — *Domain-Driven Design*, Chapter 14 (Maintaining Model Integrity)
