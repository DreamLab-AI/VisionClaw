# ADR-073 — Private Nostr Relay Mesh Topology & NIP-42 AUTH

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-010 G3, G4, G7 |
| Supersedes | — |
| Superseded by | — |
| Companion ADRs | ADR-074, ADR-075, ADR-076 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` |

## Context

Three substrates run their own (or no) Nostr relay today:

- **Forum** — `relay-worker` Cloudflare Durable Object at `wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev`. Public CF Worker URL, D1 whitelist + trust-level admission, hard-coded as the only relay in `forum-client/src/relay.rs:23`. NIP-11 advertises NIPs 1/9/11/16/26/29/33/40/42/45/50/59/65/90/98. NIP-42 AUTH challenge sent on connect (`relay_do/mod.rs:140`); only kind-1059 reads enforce the gate today.
- **Agentbox** — `nostr-rs-relay` 0.9.0 in the Nix container at `127.0.0.1:7777` (`flake.nix:1129-1141`, `agentbox.toml:87-104`). NIP-11 advertises NIPs 1/11/42 only. **Port not exposed externally** (not in `docker-compose.yml`, not in `sovereignPorts`). Embedded `RelayConsumer` (`mcp/nostr-bridge/relay-consumer.js`, 473 LOC) implements pod-inbox bridge to filesystem inbox/outbox under `pods/<npub>/events/`, but **is not wired into management-api boot**.
- **VisionClaw** — has no relay process. `nostr_sdk::Client` connects to external `NOSTR_RELAY_URLS` for publishes only; tungstenite WS to `JSS_RELAY_URL` for kind-30001 in/out and `FORUM_RELAY_URL` for re-publishing kind-9. **No NIP-42 AUTH on any of three publication paths.**

PRD-010 mandates federated cross-substrate messaging. The shape of that federation is the load-bearing decision: hub-and-spoke vs. peer mesh; push fan-out vs. pull subscription; what gates writes vs. reads; how does a relay decide whether to republish.

### Forces

- **F-trust**: Each substrate's operator owns their relay's admission policy. A unified hub means one operator gets veto over everyone else.
- **F-availability**: A single relay outage must not partition the mesh. (CF outage, agentbox container restart, VisionClaw substrate redeploy must be survivable.)
- **F-latency**: Forum DM → agentbox agent should round-trip in ≤5s under steady state.
- **F-cost**: Cloudflare Durable Object spin-up + KV writes have non-zero cost; mesh fan-out shouldn't multiply this without bound.
- **F-protocol-purity**: NIP-01/42/65 as written assume relay-client architecture. Relay-relay communication is unspecified by the NIP corpus and we should not invent a new wire protocol.
- **F-deployment-flex**: Operators in three different stacks (Cloudflare + Nix + VC's actor mesh) need to enable federation with manifest-level switches, no recompile.

### Non-forces

- **Public Nostr ecosystem federation** is out of scope. We are not building a Nostr-network-public relay; we are running three private relays that talk to each other via Nostr-native primitives.

## Decision

### D1 — Bidirectional federation, peer mesh

Each substrate runs its own relay; relays federate as **mutual outbox subscribers** to selected peers. There is no hub, no central trust root.

```
       ┌──────────────┐         ┌──────────────┐
       │ Forum CF DO  │ ⇄  …  ⇄ │ Agentbox     │
       │ (relay-      │         │ nostr-rs-    │
       │  worker)     │         │ relay        │
       └──────────────┘         └──────────────┘
              ⇅                        ⇅
              …                        …
              ⇅                        ⇅
       ┌──────────────────────────────────────┐
       │ VisionClaw (optional own relay or    │
       │ MeshBridge subscriber-only mode)     │
       └──────────────────────────────────────┘
```

VisionClaw runs no relay process by default; its `MeshBridge` opens client subscriptions to peer relays and treats them as the substrate's nostr surface. Operators who want a substrate-side relay can run `nostr-rs-relay` colocated; then `mesh.mode = "federated"` and the substrate's relay accepts federation traffic from the forum and agentbox sides.

### D2 — Push fan-out via federation worker

When a relay accepts an event whose kind is in `mesh.federated_kinds` AND whose author is in `mesh.federated_pubkeys` (or empty = all), an **outbound federation worker** publishes the original signed event verbatim to each peer relay in `mesh.peer_relays`. The peer relay receives it via a dedicated AUTHed session signed by the local substrate's operator key, treats it as a normal ingest event, and re-broadcasts to its own subscribers.

Key properties:
- **Verbatim event**: not re-signed, not re-wrapped. The original author's signature is the integrity primitive. Receiver verifies signature against the original `event.pubkey`.
- **Loop avoidance**: each fan-out worker keeps a per-relay LRU `seen_ids` cache (4096 entries, 600s TTL). An event id received via fan-out is NOT republished. Plus tag injection: federation-side ingests carry `["x-mesh-from", peer_relay_id]`; the receiver recognises this and skips its own outbound to that peer.
- **Authority**: federation events are signed by the operator/server key, distinct from user keys. NIP-26 delegation, when present, is forwarded as a `["delegation", ...]` tag verbatim — receiver verifies independently.

### D3 — All-write AUTH, public-read by kind whitelist

Every relay in the mesh enforces NIP-42 AUTH for all writes (`["EVENT", ...]` from non-federation sessions) and for reads of message-private kinds:

- **Reads requiring AUTH**: 4 (legacy DM), 13 (NIP-59 seal — should never appear at relay), 14 (NIP-59 rumor — should never appear), 1059 (gift wrap), 30910–30916 (moderation actions).
- **Reads open**: 0 (metadata), 1 (note), 3 (contacts), 7 (reaction), 1984 (NIP-56 report — public), 30000–39999 (parameterised replaceable, except moderation), 30033 (mesh service-list, see ADR-074).

For AUTH-required reads of kind-1059, the relay rewrites client filters to inject `#p = [authed_pubkey]` so a session can only see its own DMs. Cross-recipient access requires admin AUTH (cohort-bound).

The forum relay-worker already implements this for kind-1059 (`nip_handlers.rs:348-387`). Agentbox + VisionClaw must implement symmetrically.

### D4 — Federation session uses operator key

A relay's outbound federation worker holds a dedicated keypair stored in the substrate's secret store (forum: CF Worker secret; agentbox: `/var/lib/agentbox/identities/federation.json`; VisionClaw: `MESH_FEDERATION_PRIVKEY` env, distinct from `SERVER_NOSTR_PRIVKEY`). On peer connect:

1. Open WSS to peer relay.
2. Receive `["AUTH", challenge]`.
3. Sign kind-22242 with `["challenge", token]`, `["relay", peer_url]` tags using federation key.
4. Send `["AUTH", event]`. Peer relay validates and binds the session as `federation_pubkey = me`.
5. Publish events.

This means peer relays see federation events authored by `federation_pubkey` (not the original event author). Federation key MUST be present in peer's `mesh.allowed_remote_dids` to be accepted.

### D5 — NIP-65 outbox model deferred to client-side discovery

The mesh does NOT require client-side NIP-65 outbox routing. Each actor advertises a single preferred relay via DID Document `#nostr-relay`; clients connect there. Mesh fan-out handles the cross-relay propagation server-side.

Rationale: the forum-client and agentbox `RelayConsumer` are single-relay today; making them multi-relay-aware is significant work. The mesh delivers NIP-65-equivalent semantics via fan-out without requiring client-side multiplex.

NIP-65 client-side discovery becomes a P3 follow-up enhancement: the forum web UI can read `kind:10002` events to learn other users' preferred relays for cross-relay reads (today everyone reads only the forum CF relay).

### D6 — Per-relay manifest controls federation

Every relay's manifest exposes the same `[mesh]` block:

```toml
[mesh]
mode               = "standalone" | "federated" | "client"
peer_relays        = ["wss://...", ...]
federated_kinds    = [14, 1059, 30033, 30910, 30911, 30912, 30913, 30914, 30915, 30916]
federated_pubkeys  = []                                       # empty = all admitted local actors
honor_remote_moderation = []                                  # DIDs whose mod events are honoured
allowed_remote_dids = []                                      # peers' federation keys
delegation_required = true
fanout_lru_size    = 4096
fanout_lru_ttl_seconds = 600
```

- `standalone` (default) — no peer_relays, no fan-out worker, no inbound federation.
- `federated` — runs both inbound (accept federation events from `allowed_remote_dids`) and outbound (publish own events to `peer_relays`) workers.
- `client` — substrate has no relay process; treat `peer_relays` as personal subscriptions; bridge from inbox/outbox via local pump.

Forum's wrangler.toml exposes these as `[vars]` env vars; agentbox's `agentbox.toml` exposes them as a `[mesh]` table; VisionClaw's `Settings.toml`/env exposes a `[mesh]` config section.

### D7 — TLS termination posture

- **Forum CF relay**: already TLS via Cloudflare's edge. No change.
- **Agentbox**: `nostr-rs-relay` listens plain WS on `127.0.0.1:7777`; the `https-bridge` (priority-32 supervisord block at `flake.nix:1086-1098`) is extended to TLS-terminate `:7777` when `mesh.mode != "standalone"`. Operator binds a public hostname + cert; wss URL pattern is `wss://<host>:7777/`.
- **VisionClaw** (when running optional substrate-side relay): runs `nostr-rs-relay` behind the substrate's existing TLS bridge (Caddy or actix-web TLS in `solid_pod_handler` already TLS-aware). wss URL pattern is `wss://<vc_host>:7777/`.

### D8 — Rate limiting and DoS posture

Each relay applies its own per-IP and per-pubkey rate limits. Federation sessions are exempt from per-IP limits (they're trusted server-server) but bound by per-pubkey rate (federation key has higher cap than ordinary users; e.g. 100 events/sec vs. 5).

Per-pubkey overall:
- Forum: 10 events/sec/IP at `relay_do/broadcast.rs:75`.
- Agentbox: 5 messages/sec at `agentbox.toml:101 messages_per_sec`.
- Federation: 100 events/sec/peer (configurable).

### D9 — Loop and fan-out storm prevention

Three layered defences:

1. **Tag-based**: federation worker, before publishing, adds `["x-mesh-from", local_relay_id]`. Receiving relay's federation worker, before publishing onward, checks for `["x-mesh-from", target_relay_id]` and skips. This prevents A→B→A bounces.
2. **LRU dedup**: each federation worker tracks recently-fan-out event ids. Cache is per-peer (otherwise A→B and A→C race). Capacity 4096, TTL 600s.
3. **Federation kind allowlist**: `mesh.federated_kinds` is small and explicit. The default `[14, 1059, 30033, 30910..30916]` covers DMs (kinds 14, 1059), mesh service advertisements (30033), and moderation (30910-30916). Notes (kind 1) are NOT federated by default — operators opt them in for cross-system threading.

### D10 — Federation worker architecture per substrate

**Forum relay-worker (CF Durable Object)**:
- New module `crates/relay-worker/src/federation.rs`.
- Inbound: federation events arrive via the same DO's normal NIP-01 ingress, distinguished by AUTHed session pubkey ∈ `MESH_ALLOWED_REMOTE_DIDS`.
- Outbound: on each accepted event matching the federation predicate, schedule a `worker::Fetch` to each peer URL; reuse a single persistent `worker::WebSocket` per peer (held in `RelayInner.federation_sockets: HashMap<String, WebSocketHandle>`).
- Loop avoidance via tag injection + LRU.

**Agentbox**: relay process is `nostr-rs-relay` upstream; we cannot patch its internals. Instead a sidecar process `mcp/mesh-federation/federation-worker.js`:
- Subscribes to local relay (loopback) for `mesh.federated_kinds` from `mesh.federated_pubkeys`.
- For each event: publishes to each peer relay over a federation-key-AUTHed session.
- Sidecar runs as supervisord program priority 36 (after relay's 35).

**VisionClaw**: implemented inside the new `MeshBridge` service (`src/services/mesh_bridge.rs`). When `mesh.mode = "federated"` (and substrate runs an optional own relay), bidirectional pump. When `mesh.mode = "client"`, only inbound subscriptions, no outbound republishing.

### D11 — Probe and health endpoints

Each substrate exposes `GET /health/mesh` returning:

```json
{
  "mode":          "federated",
  "self_pubkey":   "did:nostr:<hex>",
  "peer_relays":   [
    {"url": "wss://...", "connected": true,  "last_event_at": "...", "events_recv_60s": 14, "events_sent_60s": 7},
    {"url": "wss://...", "connected": false, "last_error": "auth-required-not-allowed", ...}
  ],
  "fanout_lru":    {"size": 1023, "evicted_60s": 0},
  "federated_kinds": [14, 1059, ...],
  "uptime_s":      1284
}
```

Probe sequence: each substrate, at boot and every 30s, opens a fresh WSS to each peer relay, sends `["REQ", "probe", {"limit":0}]`, expects `EOSE` within 5s. Marks peer healthy or unreachable. Failed probes increment `mesh_peer_unreachable_total{peer_url}` Prometheus counter.

## Consequences

### Positive

- **No central trust**: no operator owns the mesh. Each relay's admin sets their own admission. Mesh continues even if one relay is down.
- **Native protocol**: federation uses NIP-01 events + NIP-42 AUTH; no new wire protocol. Compatible with existing `nostr-rs-relay` and the CF Worker DO.
- **Operator switchability**: standalone is the default; federation requires an explicit manifest opt-in. Existing forum / agentbox / VisionClaw deployments are unaffected.
- **Bounded blast radius**: kind allowlist + pubkey allowlist + per-peer LRU bound the fan-out volume. Naïve flooding is impossible.
- **Verbatim signature preservation**: original author's signature flows through the mesh. Recipients verify against `event.pubkey`, not `federation_pubkey`. NIP-26 delegation (ADR-074) handles the user-vs-bridge attribution case.

### Negative

- **State per relay-pair**: O(peers²) connections in the worst case. With 3 substrates this is 6 sockets, manageable. Scales linearly with new peers.
- **Federation key custody**: each substrate's federation-key-loss compromises the mesh. Mitigation: short-lived (<7d) operator-rotated keys; ADR-074's rotation protocol.
- **Cloudflare DO cost**: every fan-out is one outbound WebSocket from the DO; CF charges per-millisecond. With `federated_kinds = [14, 1059, ...]` and steady-state DM volume of <1/sec, cost is bounded but real (~$50/mo at 1k DMs/day at typical CF rates).
- **No public Nostr interop**: public relays (relay.damus.io etc.) won't accept federation-key writes without being added to their (nonexistent) `allowed_remote_dids`. Mesh is private by design.
- **Higher AUTH friction client-side**: forum-client must gain the AUTH-RESP code path (PRD-010 F7). Until landed, kind-1059 reads remain broken (already broken today; this is a fix, not a regression).

### Neutral

- **Operational complexity**: yes, mesh adds health probes, fan-out workers, manifest config. Acceptable for the federation gain.
- **Testing surface**: cross-relay smoke test boots all three substrates + a fake fourth peer relay, verifies fan-out and dedup. Tooling investment ≈ 3 days.

## Alternatives Considered

### Alt-A — Hub-and-spoke with forum CF relay as hub

Every substrate connects to the forum CF relay; CF relay does no fan-out, just acts as the universal store. Agentbox + VC don't run relays.

*Rejected*: single trust root in Cloudflare; forum operator becomes mesh god; CF Worker outage = total mesh failure; agentbox loses sovereignty story (PRD-001's "sovereign mesh" is the project name).

### Alt-B — Full client-side multi-relay (NIP-65 outbox routing)

No server-side fan-out. Every client (forum, agent, substrate-bridge) reads/writes from N relays simultaneously per NIP-65. Mesh is purely client-discoverable.

*Rejected*: forum-client is single-relay today; making it multi-relay needs significant rewrite (relay pool, dedup, per-relay subscription lifecycle, per-relay AUTH state). Out-of-scope for PRD-010 P1-P3. Adopts as P5 enhancement once mesh bootstrap is stable.

### Alt-C — Pull-only (relays expose outbox HTTP endpoints)

Each relay exposes `GET /outbox?since=<ts>` returning new events as JSON. Peers poll on schedule.

*Rejected*: higher latency (poll interval is the floor); HTTP is heavier than persistent WSS; loses NIP-42 AUTH semantic at the wire boundary; adds a new Nostr-shaped HTTP API per relay that has no precedent.

### Alt-D — gRPC / custom binary federation protocol

Relays talk to each other over a non-Nostr protocol (gRPC, MessagePack-over-TCP, etc.).

*Rejected*: custom protocol means custom code on every relay; loses ability to reuse `nostr-rs-relay` upstream; double protocol maintenance burden; no precedent in Nostr ecosystem for relay-relay protocols, so we'd be inventing.

### Alt-E — Bus-style federation via central message broker

Each relay publishes to a central NATS / Kafka / Redis-streams instance; each subscribes. Decouples peer connections.

*Rejected*: another piece of infra to run; central trust again; latency worse than direct WSS; unnecessary architectural weight for 3-peer mesh.

## Implementation notes

### Forum federation worker (CF DO sidecar)

Implementation lives inside the same `NostrRelayDO` since DO instances are 1-per-region. New struct `FederationOutbound` held in `RelayInner`:

```rust
struct FederationOutbound {
  peers: HashMap<String, PeerSession>,   // key = peer URL
  lru:   HashMap<String, BoundedLru>,    // key = peer URL, value = seen_ids
  fed_key: SecretKey,
}

struct PeerSession {
  ws: Option<WebSocket>,
  authed: bool,
  inflight: VecDeque<String>,            // event ids awaiting OK
  last_used: Instant,
}
```

DO `alarm` (current at `relay_do/mod.rs:276`) is extended to fire health probes every 30s. On accept of a federated event, `broadcast_to_peers(event_id, raw_event_json)` is called — adds to inflight and sends `["EVENT", event]`. On `["OK", id, true, ...]` from peer, removes from inflight; on `false`, logs to `mesh_peer_reject_total{peer_url, code}`.

### Agentbox federation worker (sidecar)

`mcp/mesh-federation/federation-worker.js` (~200 LOC):
- Connects to loopback relay as ordinary client (no AUTH needed for own session).
- Subscribes to `mesh.federated_kinds`.
- For each event matching predicate: publishes to each peer.
- Per-peer AUTH state machine identical to forum's.
- Supervisord block:

```ini
[program:mesh-federation]
command=node /opt/agentbox/mcp/mesh-federation/federation-worker.js
directory=/var/lib/agentbox
user=devuser
environment=HOME="/home/devuser",AGENTBOX_FEDERATION_KEY_FILE="/var/lib/agentbox/identities/federation.json"
autostart=true
autorestart=true
priority=36
```

### VisionClaw `MeshBridge`

Replaces `src/services/nostr_bridge.rs` with the more general `mesh_bridge.rs`:

- Configurable subscription set per peer (`mesh.peer_relays`, `mesh.subscribed_kinds`).
- Federation-key-AUTHed sessions (replacing the current anonymous JSS sub).
- Inbound: deserialise IS-Envelope from kind-1059 rumors, dispatch to handlers (`bead_publisher`, `concept_indexer`, `dm_router`).
- Outbound (when `mesh.mode = "federated"` and substrate runs own relay): same as forum's federation worker.

### Forum-client AUTH-RESP

`crates/forum-client/src/relay/auth_responder.rs`:

```rust
pub async fn respond_to_auth_challenge(
  inner: &mut RelayInner,
  signer: &dyn Signer,
  relay_url: &str,
  challenge: &str,
) -> Result<(), RelayError> {
  let event = build_unsigned_event(22242, vec![
    Tag(["challenge", challenge]),
    Tag(["relay", relay_url]),
  ], "");
  let signed = signer.sign_event(event).await?;
  inner.send_raw(&json!(["AUTH", signed]).to_string()).await?;
  // Wait for OK on AUTH; on success replay any pending kind-1059 SUBs.
  ...
}
```

`relay.rs:439` `handle_relay_message` adds:

```rust
"AUTH" => {
  let challenge: String = parse_index(1)?;
  inner.pending_auth_challenge = Some(challenge.clone());
  spawn_local(async move { auth_responder::respond_to_auth_challenge(...).await });
}
```

### Test surface

- Unit: `relay-worker/src/federation.rs::tests` — fan-out predicate, LRU dedup, tag injection.
- Integration: `tests/mesh_e2e/forum_to_agentbox_dm.rs` — bring up DO + nostr-rs-relay in test container, send DM, assert delivery within 5s.
- Failure: `tests/mesh_e2e/peer_unreachable.rs` — simulate one peer offline, verify probe counter increments, others continue, recovery on reconnect.

## References

- `docs/integration-research/02-forum-surfaces.md` — relay-worker behaviour
- `docs/integration-research/03-agentbox-surfaces.md` — nostr-rs-relay deployment
- `docs/integration-research/01-visionclaw-surfaces.md` — VisionClaw nostr surfaces
- `agentbox/docs/reference/adr/ADR-009-embedded-nostr-relay.md` — relay decision
- ADR-074 — DID:Nostr canonicalisation & trust pivot
- ADR-075 — IS-Envelope v1 contract
- ADR-076 — `nostr-core` absorption into upstream `nostr` crate (federation worker uses upstream `nostr::Event` types)
- PRD-010 — DID:Nostr Mesh Federation

## Cross-reference notes (post-ADR-076)

The federation worker (D10) operates on upstream `nostr::Event` instances —
post-absorption, the type flows directly from forum's `nostr-core` (now thin
shim) to VisionClaw's `MeshBridge` to agentbox's federation sidecar without
translation. Agentbox's JS sidecar continues using `nostr-tools` JS, which is
wire-compatible with `nostr` Rust by spec.
