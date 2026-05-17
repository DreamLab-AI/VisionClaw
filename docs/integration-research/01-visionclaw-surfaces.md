# VisionClaw Communication Surfaces

> Evidence-based map of how the VisionClaw Rust substrate at
> `/home/devuser/workspace/project/` actually speaks (or fails to speak)
> Nostr / `did:nostr` / Solid as of 2026-05-07.
>
> Every claim is grounded by a `file:line` citation against the working tree.
> "ACTUAL" sections are runtime behaviour confirmed in source. "PLANNED" sections
> are taken from PRD/DDD/ADR text and explicitly do **not** correspond to working
> Rust unless cited.

---

## 1. Identity (did:nostr)

### ACTUAL

There are **three** distinct "server identity" surfaces in the substrate today,
all derived from a single secp256k1 keypair format (BIP-340 x-only, 32-byte
hex), but loaded from environment variables independently and not unified:

1. **`ServerIdentity`** — the canonical operational keypair.
   - `src/services/server_identity.rs:43-51` defines `ServerIdentity { keys: Keys, relay_urls, client: Option<Arc<Client>> }`.
   - Loaded once at boot from `SERVER_NOSTR_PRIVKEY` (nsec1… or 64-char hex), in `from_env()` at `src/services/server_identity.rs:64-128`.
   - Production fail-closed: `APP_ENV=production` with no key aborts startup (`src/services/server_identity.rs:82-93`).
   - Dev fallback `SERVER_NOSTR_AUTO_GENERATE=true` mints an ephemeral key (`src/services/server_identity.rs:88-101`). Pubkey is logged; private key is never logged.
   - Wired in `main.rs` at `src/main.rs:579-595`: `ServerIdentity::from_env()? ; si.connect_relays().await ; Arc::new(si)` then started as `ServerNostrActor`.
   - Public surface: `pubkey_hex()` (`:170-172`), `pubkey_npub()` (`:174-180`), `relay_urls()` (`:182-185`).

2. **`NostrBridge` / `NostrBeadPublisher`** — a *second* keypair, env var
   `VISIONCLAW_NOSTR_PRIVKEY` (hex only — no nsec acceptance).
   - `src/services/nostr_bridge.rs:62-94` and `src/services/nostr_bead_publisher.rs:46-70`.
   - These two services share the same env var and same bot identity, but they
     are **not the same key** as `ServerIdentity`'s `SERVER_NOSTR_PRIVKEY`. Two
     keys, two surfaces, no conscious unification — `src/services/pod_client.rs:10-13` even comments that "the sibling agent's `ServerIdentity` once merged" is a future state.

3. **`PodClient`** — fallback path that re-reads `SERVER_NOSTR_PRIVKEY` per
   request (`src/services/pod_client.rs:34`, env constant
   `SERVER_NOSTR_PRIVKEY_ENV`). It signs NIP-98 events over LDP requests.

`did:nostr:<hex>` minting is centralised:

- `mint_did_nostr()` at `src/uri/mint.rs:45-51` accepts hex / `did:nostr:` /
  `npub1…`, normalises via `normalise_pubkey()` and re-emits the canonical
  `did:nostr:<64-lowercase-hex>` form.
- `normalise_pubkey()` at `src/uri/parse.rs:244-253` is the choke-point.
  Handles bech32 npub via `nostr_sdk::PublicKey::from_bech32` (`:256-261`).
- `validate_hex64` at `src/uri/parse.rs:222-236` enforces 64-char hex.

A DID document is **never minted server-side** — the URI resolver redirects
`did:nostr:<hex>` to `/api/v1/identity/<hex>/did.json`
(`src/handlers/uri_resolver_handler.rs:159-162`), but **no handler implements
that route in this repo**. The Location header points at a 404.

### Verifying NIP-98 signed challenges

`NostrIdentityVerifier` (`src/services/nostr_identity_verifier.rs:34-64`)
verifies XR presence challenge signatures using `secp256k1::schnorr` directly:
hashes `(nonce || timestamp_us)`, decodes the claimed 32-byte x-only pubkey,
verifies via `SECP256K1.verify_schnorr`, and on success returns
`Did::parse(format!("did:nostr:{}", claimed_pubkey_hex))`. This confirms hex
pubkey is the canonical scope form everywhere.

A second verifier `WellFormedOnlyVerifier`
(`src/services/nostr_identity_verifier.rs:69-92`) skips the cryptographic check
and only validates hex shape — a documented test/CI stub.

### `GET /api/server/identity`

Public discovery endpoint at `src/handlers/server_identity_handler.rs:28-38`:

```
{ pubkey_hex, pubkey_npub, supported_kinds: [30023, 30100, 30200, 30300], relay_urls }
```

Mounted in `main.rs` at `src/main.rs:955-958` as `/api/server/identity`.
`SUPPORTED_KINDS` is hardcoded at `src/services/server_identity.rs:33`.
This is the only externally observable endpoint that publishes the server's
Nostr identity. There is no `/.well-known/did.json` on the substrate.

### PLANNED-BUT-NOT-DONE

- A unified server identity that subsumes `SERVER_NOSTR_PRIVKEY` and
  `VISIONCLAW_NOSTR_PRIVKEY` into one key. `src/services/pod_client.rs:10-13`
  explicitly flags the divergence as transitional.
- A DID document handler at `/api/v1/identity/{hex}/did.json` (referenced by
  `src/handlers/uri_resolver_handler.rs:159-162` but never registered).
- ADR-040-style enterprise-identity DID derivation (referenced as
  "Defer to ADR-040 follow-up" in `docs/PRD-006-…:363`).

---

## 2. Outbound nostr (publish)

### ACTUAL

Three independent publication paths exist, each with its own keypair and its
own relay (no shared client pool):

#### 2.1 `ServerIdentity::sign_and_broadcast` (kinds 30023 / 30100 / 30200 / 30300)

- `src/services/server_identity.rs:219-262`. Signs an event with the server
  keypair, then **best-effort** broadcasts to every relay in
  `NOSTR_RELAY_URLS`. Failure is logged, never returned: the signed `Event` is
  always given back (`:261`).
- Timeout per publish is `BROADCAST_TIMEOUT = 8s` (`src/services/server_identity.rs:36`).
- `connect_relays()` (`:138-155`) builds an `nostr_sdk::Client` once at startup;
  `Some(client)` only when `NOSTR_RELAY_URLS` is non-empty (`:139-142`).
  When no client is configured, broadcast is a no-op
  (`src/services/server_identity.rs:228-233`).
- Relay URL scheme is enforced (`ws://` or `wss://` only, `:283-294`).
- The `ServerNostrActor` (`src/actors/server_nostr_actor.rs:40-97`) wraps this
  in four typed `actix` messages — `SignMigrationApproval` (kind 30023),
  `SignBridgePromotion` (30100), `SignBeadStamp` (30200), `SignAuditRecord`
  (30300). Each handler tags `h=visionclaw-server` (`:31`), an addressable
  `d` tag, an `event_type`, and the kind-specific payload. Concrete handlers
  at `src/actors/server_nostr_actor.rs:113-160`, `:173-216`, `:230-272`,
  `:286-338`.
- Prometheus counters `server_nostr_signed_total{kind}` and
  `server_nostr_broadcast_errors_total` are incremented in `observe_sign_outcome`
  (`src/actors/server_nostr_actor.rs:60-76`).

#### 2.2 `NostrBeadPublisher` — kind 30001 to JSS

- `src/services/nostr_bead_publisher.rs:88-136`: signs a parameterized-replaceable
  kind-30001 event with `d=<bead_id>` so re-publishes overwrite, plus tags
  `h=visionclaw-activity`, `bead_id`, `brief_id`, `debrief_path`, optional
  `user_pubkey` (`:96-109`).
- Publishes via raw `tokio_tungstenite::connect_async` to `JSS_RELAY_URL`
  (default `ws://jss:3030/relay`, see `:51-57`). It does **not** use
  `nostr_sdk::Client`; it opens a fresh WebSocket per `send_to_relay`
  (`:170-200`), waits up to 5s for an `OK` response, closes.
- Retry classification: `is_transient` matches `"timeout" | "connect" | "closed without OK" | "send failed"` (`:241-247`); `classify_error` maps everything else to `BeadOutcome::RelayRejected` or `RelayUnreachable` (`:249-259`).
- Optional Neo4j provenance write (`:202-238`) creates a
  `(:NostrEvent)-[:PROVENANCE_OF]->(:Bead)` pair when `with_neo4j(graph)` was
  called.
- Wired into `BeadLifecycleOrchestrator` at `src/main.rs:560-567`. Spawned
  fire-and-forget by HTTP handlers (per `src/services/bead_lifecycle.rs:4-18`).

#### 2.3 `NostrBridge` — JSS kind 30001 → Forum kind 9

- `src/services/nostr_bridge.rs:53-59` and `:106-179`.
- **Subscribes** to `JSS_RELAY_URL` for kind 30001 (`src/services/nostr_bridge.rs:147-152`),
  verifies signatures (`:188-195`), and **republishes** as NIP-29 group kind 9
  (`:219-220`) re-signed with the bridge keypair, to `FORUM_RELAY_URL`.
- Tags carried over: `h=visionclaw-activity`, `bead_id`, `source_event=<original_id>`
  (`src/services/nostr_bridge.rs:204-217`).
- Reconnect loop with exponential backoff 5 s → 300 s × 2.0 (`src/services/nostr_bridge.rs:27-31, :112-136`),
  reset to 5s after `HEALTHY_CONNECTION_SECS = 60s` of healthy streaming.
- Spawned in `main.rs` at `src/main.rs:570-575`:

```rust
if let Some(bridge) = NostrBridge::from_env() {
    tokio::spawn(bridge.run());
    info!("[main] NostrBridge spawned");
} else {
    info!("[main] NostrBridge not started (VISIONCLAW_NOSTR_PRIVKEY or FORUM_RELAY_URL not set)");
}
```

So the bridge is **opt-in**: if either env var is missing, the substrate runs
without the JSS→Forum relay hop.

### Kinds published — summary

| Kind  | Publisher                               | Path                                                       | File:line                                                  |
| ----- | --------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| 9     | `NostrBridge::send_to_forum`            | re-signs JSS 30001 → forum                                 | `src/services/nostr_bridge.rs:219-247`                     |
| 30001 | `NostrBeadPublisher::publish_bead_complete` | direct WS to JSS                                       | `src/services/nostr_bead_publisher.rs:111-122`             |
| 30023 | `ServerNostrActor::SignMigrationApproval`   | `nostr-sdk` to `NOSTR_RELAY_URLS`                      | `src/actors/server_nostr_actor.rs`                 |
| 30100 | `ServerNostrActor::SignBridgePromotion`     | `nostr-sdk` to `NOSTR_RELAY_URLS`                      | `src/actors/server_nostr_actor.rs`                 |
| 30200 | `ServerNostrActor::SignBeadStamp`           | `nostr-sdk` to `NOSTR_RELAY_URLS`                      | `src/actors/server_nostr_actor.rs`                 |
| 30300 | `ServerNostrActor::SignAuditRecord` / `SignBrokerDecision` | `nostr-sdk` to `NOSTR_RELAY_URLS`           | `src/actors/server_nostr_actor.rs`                 |
| 30301 | `ServerNostrActor::SignEnrichmentProposal`  | `nostr-sdk` to `NOSTR_RELAY_URLS`                      | `src/actors/server_nostr_actor.rs`                 |
| 31400 | `ServerNostrActor::PublishGovernancePanel`  | `nostr-sdk` to `NOSTR_RELAY_URLS`                      | `src/actors/server_nostr_actor.rs` (Agent Control Surface) |
| 31402 | `ServerNostrActor::PublishActionRequest`    | `nostr-sdk` to `NOSTR_RELAY_URLS`                      | `src/actors/server_nostr_actor.rs` (Agent Control Surface) |

Note: kinds 30001 and 30023/30100/30200/30300/30301/31400/31402 take **different transports**.
The JSS bead path is hand-rolled tungstenite; the server-identity path uses
`nostr_sdk::Client`. There is no shared relay pool.

### PLANNED-BUT-NOT-DONE

- The bidirectional agent-events channel published as Nostr events — PRD-004
  P1.8 envisions JSONL agent events optionally republished as kind 30078
  parameterised replaceable events under `[sovereign_mesh] publish_agent_events`. No code emits 30078 in this repo.
- NIP-33 `AgentExecutionCompleted` fan-out described in
  `docs/ddd-agentbox-integration-context.md:191-193` — no event_type "agent_execution_completed" is signed anywhere.

---

## 3. Inbound nostr (subscribe)

### ACTUAL

The substrate has **one** persistent inbound Nostr subscription, and it lives
inside the bridge:

- `src/services/nostr_bridge.rs:139-179`. `run_once()` connects to
  `JSS_RELAY_URL` via `tokio_tungstenite::connect_async`, sends a single
  `["REQ", "bridge-sub", {"kinds": [30001]}]` (`:147-152`), and reads
  `["EVENT", <sub_id>, <event>]` frames (`:160-176`). On signature verification
  failure it skips the event (`:192-195`). On WebSocket error or close it
  returns `Err`, which the outer `run()` loop treats as "reconnect with
  exponential backoff".

There is **no other Nostr client subscription** in the source tree. The
`ServerIdentity` `nostr_sdk::Client` (`src/services/server_identity.rs:143-154`)
is configured for **publish only** — it calls `client.add_relay`/`client.connect`
but never `subscribe`, never reads `client.notifications()`. The check is direct:

```bash
grep -rn "client_subscribe\|REQ.*kinds.*\[3\\|subscribe.*nostr\|nostr_sdk::Client" /home/devuser/workspace/project/src/
# → only one hit: src/services/nostr_bridge.rs:149 (the kind-30001 REQ above)
```

The rest of the codebase that names "nostr" inbound is event *processing*
(verifying NIP-98 challenges, validating events from disk) — not a relay
subscription.

### Inbound from clients (not relays)

These exist but are HTTP/WS handlers, not Nostr relay subscriptions:

- `POST /api/auth/nostr` — login via NIP-98-shaped event in JSON body
  (`src/handlers/nostr_handler.rs:55-67, :129-165`). The substrate verifies the
  signature inline via `NostrService::verify_auth_event`, never touches a relay.
- `/ws/presence` — XR presence handshake. The client signs the
  `(nonce || timestamp_us)` challenge; the substrate verifies it locally
  (`src/handlers/presence_handler.rs:141-194` → `IdentityVerifier::verify_signed_challenge`).
  No relay involvement.
- `/wss/agent-events` — bidirectional with the **agentbox container**, not a
  relay (`src/handlers/agent_events_ws_handler.rs:1-179`). Subprotocol
  `vc-agent-events.v1`. Inbound JSON `agent_action` envelopes spawn transient
  `BeamEdge`+`ChargeModulation` (`:69-91`); outbound `user_interaction` events
  are broadcast (`:181-196`).

### NIP-42 AUTH

```bash
grep -rn "NIP-42\|NIP_42\|nip42\|AUTH.*relay\|relay.*AUTH" /home/devuser/workspace/project/src/
# → empty
```

NIP-42 (relay-side AUTH challenge) is **not implemented**. Neither
`NostrBridge` nor `NostrBeadPublisher` nor the `nostr_sdk::Client` opened by
`ServerIdentity` knows how to respond to a `["AUTH", challenge]` frame.
The bridge subscribes anonymously; if a relay started requiring AUTH, the
subscription would silently drop. The bead publisher would simply see its
`OK` response replaced with a relay error and `is_transient` would
classify it as "transient" → infinite retry against a relay that will never
accept it.

### PLANNED-BUT-NOT-DONE

- A relay-side subscription that reads kinds 30023/30100/30200/30300/30301/31400/31402
  (server's own published events). The substrate emits these to relays but never
  reads them back -- there is no "inbox" for ack from another VisionClaw or for
  audit-log replay. Note: kinds 31400/31402 (Agent Control Surface) are designed
  to be consumed by Forum Kit UI clients subscribing to the relay.
- A subscription to `urn:agentbox:bead:*` events from the agentbox sibling
  (federation hop in PRD-006 §5.2). This would be a precondition for the BC20
  ACL (§6).
- NIP-42 AUTH is referenced nowhere in code, design docs, or env templates.

---

## 4. Relay clients & connections (transports, URLs, retry, AUTH)

### ACTUAL

There are **three** WebSocket relay clients in the substrate, each constructed
independently:

#### 4.1 `nostr_sdk::Client` (server-identity publisher)

- Built from `Keys` at `src/services/server_identity.rs:143`, given relays
  via `client.add_relay(url)` (`:144-148`), `client.connect()` (`:149`).
- URL list: `NOSTR_RELAY_URLS` (comma-separated), parsed at `:104-106` and
  validated as `ws://` / `wss://` only (`:283-294`).
- Persistent: yes — the SDK manages the WebSocket internally.
- Reconnect: handled by `nostr_sdk` library defaults (not configured here).
- AUTH: not implemented.
- Failure mode on broadcast: 8 s timeout → log + return Ok event
  (`src/services/server_identity.rs:236-258`). The failure is **swallowed**.

#### 4.2 `NostrBeadPublisher` raw tungstenite

- One ephemeral WebSocket per publish (`src/services/nostr_bead_publisher.rs:171-200`).
  No connection pooling — every bead-complete reconnects.
- URL: `JSS_RELAY_URL`, default `ws://jss:3030/relay` (`:51-52`).
- Retry: configurable `BeadRetryConfig` with `max_attempts`, exponential
  backoff via `delay_for_attempt(n)` (`:138-167`). Defaults: 3 attempts, 1 s
  base, 10 s max, multiplier 2.0 (per the test at `:413-422`).
- AUTH: not implemented; relies on JSS being a private relay with whitelisted
  pubkeys.

#### 4.3 `NostrBridge` raw tungstenite

- **Two** sockets: an inbound subscription to JSS (`src/services/nostr_bridge.rs:140-160`)
  held for the duration of `run_once()`, plus an outbound ephemeral socket
  per forwarded event (`:251-278`).
- URLs: `JSS_RELAY_URL` (default `ws://jss:3030/relay`, `:70-71`) and
  `FORUM_RELAY_URL` (no default; bridge fails to start if missing,
  `:67-69`).
- Retry: exponential backoff 5 s → 300 s × 2.0 (`:27-29, :131-135`), reset
  after 60 s healthy (`:30-31, :130-132`).
- Health surface: `BridgeHealth { connected: AtomicBool, last_event_at: Mutex<Option<Instant>> }`
  exposed via `bridge.health()` (`src/services/nostr_bridge.rs:34-51, :97-102`).
  Note: the health handle is constructed but never wired into a `/health`
  handler that the rest of the substrate exposes.
- AUTH: not implemented.

### URLs at a glance

| Env var              | Default                       | Used by                                                              | File:line                                                |
| -------------------- | ----------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------- |
| `NOSTR_RELAY_URLS`   | empty (publish disabled)      | `ServerIdentity::connect_relays`                                     | `src/services/server_identity.rs:104-106, :138-155`      |
| `JSS_RELAY_URL`      | `ws://jss:3030/relay`         | `NostrBeadPublisher`, `NostrBridge` (inbound)                        | `src/services/nostr_bead_publisher.rs:51-58`, `nostr_bridge.rs:70-71` |
| `FORUM_RELAY_URL`    | none — bridge no-op if unset  | `NostrBridge` (outbound)                                             | `src/services/nostr_bridge.rs:67-69`                     |
| `SERVER_NOSTR_PRIVKEY` | required in production      | `ServerIdentity`, `PodClient` fallback                               | `src/services/server_identity.rs:65-90`, `pod_client.rs:34` |
| `VISIONCLAW_NOSTR_PRIVKEY` | none — bridge/publisher disabled | `NostrBridge`, `NostrBeadPublisher`                            | `src/services/nostr_bridge.rs:64-66`, `nostr_bead_publisher.rs:48-50` |
| `SERVER_NOSTR_AUTO_GENERATE` | `false`                  | `ServerIdentity` dev fallback                                        | `src/services/server_identity.rs:73-76`                  |
| `APP_ENV`            | unset                         | gates fail-closed in `ServerIdentity`                                | `src/services/server_identity.rs:69-72`                  |

### Does the substrate maintain a persistent Nostr WS connection?

**Yes, two — both opt-in:**

1. `ServerIdentity` opens an `nostr_sdk::Client` at boot if `NOSTR_RELAY_URLS`
   is set (publish-only).
2. `NostrBridge::run` runs an infinite reconnect loop subscribing to JSS for
   kind 30001, if both `VISIONCLAW_NOSTR_PRIVKEY` and `FORUM_RELAY_URL` are set.

If neither env var triple is set, the substrate runs without any persistent
Nostr socket. The `NostrBeadPublisher` opens a fresh socket per publish — no
persistent connection.

### PLANNED-BUT-NOT-DONE

- A unified relay pool keyed by `(URL, capability)`. PRD-006 alludes to this
  obliquely in §5.7 (long-lived stdio session, but no concrete relay pool design).
- NIP-42 AUTH on any of the three clients.
- Relay health metrics. The Prometheus surface
  (`src/actors/server_nostr_actor.rs:60-76`) exposes `signed_total` and
  `broadcast_errors_total` but **not** per-relay `up/down` gauges. There is no
  `nostr_bridge_connected{relay=…}` gauge.

---

## 5. Bead/concept/kg URN minting & resolution

### ACTUAL

Six URN kinds are minted by `src/uri/`:

| Kind             | Mint fn                                | URN form                                                 |
| ---------------- | -------------------------------------- | -------------------------------------------------------- |
| `Concept`        | `mint_concept(domain, slug)`           | `urn:visionclaw:concept:<domain>:<slug>`                 |
| `Group`          | `mint_group_members(team)`             | `urn:visionclaw:group:<team>#members`                    |
| `OwnedKg`        | `mint_owned_kg(pubkey_hex, payload)`   | `urn:visionclaw:kg:<hex-pubkey>:sha256-12-<12 hex>`      |
| `Bead`           | `mint_bead(pubkey_hex, payload_json)`  | `urn:visionclaw:bead:<hex-pubkey>:sha256-12-<12 hex>`    |
| `AgentExecution` | `mint_execution(action, slot, pk, ts)` | `urn:visionclaw:execution:sha256-12-<12 hex>`            |
| `Did`            | `mint_did_nostr(pubkey_hex)`           | `did:nostr:<64 hex>`                                     |

All in `src/uri/mint.rs:14-83`. Each of the owner-scoped kinds rejects an
empty pubkey at mint time (`mint.rs:33-39, :55-58, :77-79`).

#### Content addressing

`content_hash_12` at `src/uri/parse.rs:269-281` produces `sha256-12-<12 hex>`:
SHA-256 over the bytes, then the first 6 bytes rendered as 12 lowercase hex
chars. The `module-level docstring src/uri/mod.rs:14-15` confirms this is the
"byte-identical form shared with agentbox (PRD-006 F10)". Cross-substrate
content hashes match without negotiation.

#### Pubkey scope

`normalise_pubkey` at `src/uri/parse.rs:244-253` accepts hex / `did:nostr:<hex>`
/ `npub1...` and always returns 64-char lowercase hex. Bech32 npub decoding
goes through `nostr_sdk::PublicKey::from_bech32` (`:256-261`).

This proves the project-level CLAUDE.md claim that "hex pubkey is the canonical
scope form everywhere; bech32 npub is only used at the Nostr relay wire
boundary and in legacy pod filesystem paths."

#### Anti-drift gate

`src/uri/mint.rs:1-6`:

```rust
//! Every URN that crosses an API boundary in VisionClaw is minted here. A
//! clippy-style grep gate in CI rejects ad-hoc `format!("urn:visionclaw:...")`
//! anywhere outside `src/uri/`. See PRD-006 §6 (Anti-Drift Gate).
```

The CI enforcement script lives outside `src/`; this module only documents the
contract. (The grep rule would catch test code in `tests/uri_grammar.rs:31`
that asserts the literal URN form, but it is exempted by being a test.)

#### Legacy forms (`src/uri/legacy.rs`)

Two pre-existing `canonical_iri` minters, both `#[deprecated]` since 0.2.0:

- `canonical_iri_npub(pubkey_hex, relative_path)` →
  `visionclaw:owner:<npub>/kg/<sha256-64>` (`src/uri/legacy.rs:31-49`).
- `canonical_iri_raw_hex(owner_pubkey_hex, relative_path)` →
  `visionclaw:owner:<raw-hex>/kg/<sha256-64>` (`src/uri/legacy.rs:57-66`).

Both forms remain on the `canonical_iri` Neo4j column because `opaque_id.rs:166`
derives bit29 binary-protocol opaque ids from those values. The library
preserves the legacy data; the resolver looks up by *either* `iri` or
`visionclaw_uri` columns.

### Resolution

`GET /api/v1/uri/{urn}` and `GET /api/v1/uri/by-curie/{curie}` are handled in
`src/handlers/uri_resolver_handler.rs`. Dispatch table at `:148-173`:

```rust
match parse(urn)? {
    ParsedUri::Concept { .. }  => 307 → /api/v1/nodes/by-uri/<urn>/jsonld
    ParsedUri::OwnedKg { .. }  => 307 → /api/v1/nodes/by-uri/<urn>/jsonld
    ParsedUri::Did { hex }     => 307 → /api/v1/identity/<hex>/did.json
    ParsedUri::Group { team }  => 307 → /api/v1/wac/groups/<team>
    ParsedUri::Bead { .. } | ParsedUri::AgentExecution { .. } => federation_hop(urn) // 404 with hint
}
```

The handler **does not consult Neo4j**. `:27-32` says explicitly:

> NOTE: this handler currently only ANSWERS resolves; the actual lookup
> against Neo4j (`MATCH (n) WHERE n.iri = $key OR n.visionclaw_uri = $urn`)
> is wired here as a stub returning 404. Wiring the live query is a P3
> concern paired with the BC20 federation work; for P2 we ship the grammar
> surface, error envelopes, and CURIE↔URN normalisation so agentbox can
> point at us today.

The redirect targets `/api/v1/nodes/by-uri/<urn>/jsonld` and
`/api/v1/identity/<hex>/did.json` — **neither route is registered anywhere in
this repo**. A search across `src/main.rs` confirms no such handler is mounted.
A 307 from `/api/v1/uri/<urn>` lands the client at a 404.

`GET /api/v1/uri` (no urn) returns the grammar self-description
(`src/handlers/uri_resolver_handler.rs:82-130`). Six `KindEntry` rows; `bead`
and `execution` carry `resolvable: false`.

### Federation-hop placeholder

`federation_hop` at `src/handlers/uri_resolver_handler.rs:193-205` returns 404
with `error: "federation_hop_required"` and a hint that the URN belongs to the
agentbox sibling. The comment is explicit:

```rust
// 404 with a `federation-hop` hint. PRD-006 §5.5 — once BC20 ships,
// this becomes a 307 to the agentbox sibling's `/v1/uri/<urn>`.
```

So the resolver knows about agentbox, but does **not** make a federation hop.
There is no `urn:agentbox:*` parser, no client, no proxy.

### PLANNED-BUT-NOT-DONE

- The Neo4j lookup (`/api/v1/nodes/by-uri/{urn}/jsonld`,
  `/api/v1/identity/{hex}/did.json`, `/api/v1/wac/groups/{team}`).
- The `urn:agentbox:*` parser and the federation hop to the sibling.
- The `urn:agentbox:thing:visionclaw:*` reciprocal alias (PRD-006 §5.9).
- Backfill of `visionclaw_uri` from `canonical_iri` (PRD-006 §5.1 migration
  comment, `src/uri/legacy.rs:18-20`).

---

## 6. BC20 anti-corruption layer status (planned vs actual)

### ACTUAL

**The BC20 anti-corruption layer does not exist as code in this repo.**

Direct check:

```bash
ls /home/devuser/workspace/project/src/bc20/
# ls: cannot access 'src/bc20/': No such file or directory

grep -rn "FederationSession\|AgentExecution\|bc20\|BC20" src/
# → only doc-comment mentions in src/uri/mod.rs:1-6, src/services/management_api_client.rs:9 etc.
# → zero struct/impl/mod definitions

find src -name "*bc20*" -o -name "federation_session*" -o -name "agent_execution*"
# → no matches
```

What exists today:

- `src/services/management_api_client.rs` — a plain `reqwest`-based HTTP
  client targeting `agentbox:9190`. Comment at `:1-9` documents the
  one-way wire: "VisionFlow Container → ManagementApiClient (HTTP) →
  agentbox:9190 → Management API". It is *not* an ACL — it sends commands
  shaped exactly the way agentbox expects them. There is no domain
  translation layer.
- `src/handlers/agent_events_ws_handler.rs` — a *WebSocket* with the
  agentbox container (subprotocol `vc-agent-events.v1`). Inbound JSON
  `agent_action` envelopes are mapped to `BeamEdge` + `ChargeModulation`
  (`:69-91`). This is the closest thing to `events_acl`, but it is a
  visualisation projector, not an event-bus translator: each frame becomes a
  ≤1 ms transient edge, not a `:AgentEvent` Neo4j node nor a Contributor
  Stratum bus message. `:117-119` even comments "binary frame (ignored,
  JSON-only in Phase 2)".
- `src/services/mcp_relay_manager.rs` — uses `Command::new("docker")` to
  exec into the agentbox container for MCP-relay lifecycle (`:96, :191,
  :227, :282`). This is operational, not an ACL.
- `src/handlers/uri_resolver_handler.rs:193-205` — the federation-hop hint
  endpoint described above. It returns 404, never makes the hop.

PRD-006 §5.5 lists the BC20 module set:

```
src/bc20/
  mod.rs
  federation_session.rs
  federation_lifecycle.rs
  adapter_registry.rs
  agent_execution.rs
  acl/
    mod.rs
    beads_acl.rs
    pods_acl.rs
    memory_acl.rs
    events_acl.rs
    orchestrator_acl.rs
    uris_acl.rs
```

**Zero of these files exist.** The DDD doc
(`docs/ddd-agentbox-integration-context.md:53-176`) elaborates the
`FederationSession` aggregate, the `AgentExecution` aggregate, the
`AdapterEndpointRegistry` value object, and the five ACL modules in detail —
all paper, no Rust.

The single live integration is the JSON envelope in
`src/agent_events/mod.rs:1-8`:

```
//! `target_urn`, and `pubkey` per agentbox ADR-013 grammar. Backward-
//! See ADR-059 §2 for the schema and ADR-014 for the agentbox side.
```

That envelope is shared on the wire, but neither side translates payload
shapes; the substrate just consumes them as-is.

### PLANNED-BUT-NOT-DONE

Everything in §5.5–§5.7 of PRD-006 (six ACL modules, federation handshake at
boot, stdio bridge reader, agent-event WS projector that writes to Neo4j,
replay endpoint `/api/v1/executions/{id}/events`). All paper.

---

## 7. Solid pod consumer (if any)

### ACTUAL

The substrate is **a Solid Pod server, not a Pod consumer of agentbox-owned
pods**. `src/handlers/solid_pod_handler.rs:1-7` is explicit:

```rust
//! Native Solid Pod handler (`solid-pod-rs` backend).
//!
//! ADR-053/056 — dispatches `/solid/*` requests against the external
//! `solid-pod-rs` crate (pinned in Cargo.toml). As of 2026-04-20 this
//! is the sole Pod implementation; the legacy JSS proxy and shadow
//! comparator were retired in the `chore/solid-pod-rs-externalise-jss-cut`
//! commit.
```

Cargo.toml at `Cargo.toml:24-26`:

```toml
# ADR-053/056: solid-pod-rs — native Solid Pod library (JSS retired 2026-04-20).
solid-pod-rs = { version = "0.4.0-alpha.1", features = ["fs-backend", "memory-backend", "nip98-schnorr", "security-primitives"] }
```

Server-side responsibilities the substrate owns:

- LDP method dispatch (`src/handlers/solid_pod_handler.rs:184-206`):
  GET, HEAD, PUT, POST, DELETE, PATCH (N3 patch with optional SPARQL-Update
  hookpoint at `:297-328`), OPTIONS.
- WAC ACL evaluation (`:148-163`) via `solid_pod_rs::wac::evaluate_access`
  with `(method → mode)` per `method_to_mode`.
- NIP-98 authentication (`:140-146, :386-399`) via
  `solid_pod_rs::auth::nip98::verify`. On success the substrate derives a WebID
  with the JSS-compatible shape: `{base}/{pubkey_hex}/profile/card#me`
  (`:405-411`).
- `WAC-Allow` header attached to every response (`:413-427`).
- 403 envelope carries `WAC-Allow` so callers can discover effective modes
  without a follow-up request (`:431-439`).
- Dotfile guard (`:356-364`): only `.well-known`, `.acl`, `.meta` allowed.
  Anything else with a leading dot in any segment → 403.

Path extraction is brittle: `src/handlers/solid_pod_handler.rs:369-382` uses
`req.path().find("/solid")` to compute the pod-relative path, rather than
relying on actix scope path. Consequence: any URL containing the literal
substring `/solid` anywhere (not just as a scope prefix) collapses to that
substring as the pod-relative root.

Service is wired in `main.rs:704-715`:

```rust
let native_solid_data = match NativeSolidService::from_env().await {
    Ok(svc) => web::Data::new(Arc::new(svc)),
    Err(e) => {
        error!("[solid-pod-rs] NativeSolidService init failed: {e}");
        return Err(...); // fatal — there is no fallback path
    }
};
```

Mounted at `/api/solid/{path:.*}` via `configure_solid_native_routes` in
`main.rs:927`. The scope wraps `web::scope("/solid")`
(`src/handlers/solid_pod_handler.rs:493-498`). Because that scope sits inside
`web::scope("/api")`, the live path is `/api/solid/...`.

### Pod consumer surface

There **is** a Pod consumer surface, but it is the *substrate consuming its own
pods or pods it controls*, not pods owned by agentbox agents:

- `src/services/pod_client.rs` — NIP-98-signed `reqwest` client used by the
  ingest saga (`src/services/ingest_saga.rs`). Signs each PUT/DELETE/MOVE/HEAD
  with a fresh NIP-98 event bound to (url, method, payload-hash).
  `SERVER_NOSTR_PRIVKEY_ENV = "SERVER_NOSTR_PRIVKEY"` (`pod_client.rs:34`)
  is the same env var used by `ServerIdentity` — but the comment at `:10-13`
  flags this as "until the sibling agent's `ServerIdentity` is merged".
- `URL_BASE` for the consumer comes from `POD_BASE_URL`
  (`src/services/ingest_saga.rs:51, :68`). Default
  `https://pods.visionclaw.org` (`solid_pod_handler.rs:96-98`).

Whether the substrate can read a pod owned by an agentbox agent is determined
by the WAC document at the pod root, not by any code path. There is no
"agentbox pod" awareness — the substrate just signs NIP-98 and presents the
WebID `{base}/{hex_pubkey}/profile/card#me`. If the target pod is mounted
inside the substrate's own `FsBackend` (`POD_DATA_ROOT`, default
`/app/data/solid-pod-rs`), it is just another file. If it is on a remote pod
server, `pod_client.rs` would need a configurable base URL per call — the
current shape (`PodClient { http: reqwest::Client, keys: Option<Keys> }`)
takes a full URL per request, so this is in principle possible but no caller
supplies an agentbox-side URL today.

### URN-Solid mapping

`src/services/urn_solid_mapping.rs` implements ADR-054. Loads a markdown table
at `docs/reference/urn-solid-mapping.md` (`:34`). Gated by
`URN_SOLID_ALIGNMENT` env (`:31, :38-42`). Hot-reloadable via `notify` watcher
(`:194-246`). Returns `UrnSolidMapping { our_iri, urn_solid, canonical_vocab,
status }` keyed by IRI (`:75-85`). Status enum `Stable | Proposed | Deferred`
(`:46-72`).

This is consumer-facing in the sense that it lets the substrate emit
`owl:sameAs urn:solid:<Name>` predicates on `:OntologyClass` nodes. The actual
emission happens in `src/services/ontology_enrichment_service.rs` (referenced
by the ADR but not re-checked here).

### PLANNED-BUT-NOT-DONE

- Per-user `corpus.jsonl` write at `./public/kg/corpus.jsonl` (ADR-054 §2).
  Not implemented in this surface — no handler emits a corpus file.
- JSON-LD content negotiation on `/api/solid/...` — ADR-053 phase 2 plan, not
  in `solid_pod_handler.rs`. The handler returns Turtle for containers
  (`:213-218`) and the stored content type for resources (`:227-237`).
- Reading pods owned by agentbox agents — the substrate has the
  `PodClient` machinery to do so, but no calling code targets agentbox pod
  URLs.

---

## 8. Gaps / antipatterns / dead code

### G1 — Two server keypairs, two env vars

`SERVER_NOSTR_PRIVKEY` (used by `ServerIdentity` and `PodClient`) versus
`VISIONCLAW_NOSTR_PRIVKEY` (used by `NostrBridge` and `NostrBeadPublisher`).
Comment at `src/services/pod_client.rs:10-13` explicitly flags this as a
transitional split. Nothing currently enforces that they refer to the same key,
so the substrate can produce events under two different identities without
warning. `tests/server_identity.rs` only exercises the `ServerIdentity` side.

### G2 — Two URL conventions for relays

`NOSTR_RELAY_URLS` is comma-separated (`src/services/server_identity.rs:104-106`),
`JSS_RELAY_URL` and `FORUM_RELAY_URL` are single URLs. No env var is shared.
A relay added to "the substrate's relay set" must be added in two places.

### G3 — `nostr_sdk::Client` is publish-only

`src/services/server_identity.rs:138-155` connects relays but never subscribes.
Server-issued kinds 30023/30100/30200/30300/30301/31400/31402 are written but
never read back -- there is no audit-log replay path that consumes its own
events. Note: kinds 31400/31402 (Agent Control Surface Protocol) are specifically
designed for consumption by Forum Kit UI clients, not by VisionClaw itself.
PRD-006 implicitly expects read-back for cross-substrate verification (§5.6
LocalFallbackProbe); no code reads them.

### G4 — NIP-42 AUTH not implemented anywhere

Already cited in §3. If any of the three external relays (NOSTR_RELAY_URLS,
JSS, Forum) starts requiring AUTH, the substrate publishes silently fail
(server identity), forwarder hangs (bridge), or retries forever (bead
publisher).

### G5 — URI resolver redirects to non-existent endpoints

`src/handlers/uri_resolver_handler.rs:148-173` issues 307s to:

- `/api/v1/nodes/by-uri/{urn}/jsonld` — no handler registered.
- `/api/v1/identity/{hex}/did.json` — no handler registered.
- `/api/v1/wac/groups/{team}` — no handler registered.

Every Concept/OwnedKg/Did/Group resolution is functionally a 404 disguised as
a 307. The handler is a grammar surface only, per its own docstring (`:27-32`).

### G6 — BC20 ACL is paper

§6 above. Six modules and two aggregate types described in
`docs/ddd-agentbox-integration-context.md:54-176` and `docs/PRD-006-…:206-225`
have zero code presence. PRD-006 P3 (planned for 2026-06-13) is the gate.

### G7 — `BridgeHealth` is unused

`NostrBridge::health()` (`src/services/nostr_bridge.rs:97-102`) returns a
cloneable `BridgeHealth`, but `main.rs:570-575` constructs the bridge, calls
`bridge.run()` (which consumes `self`), and never retrieves the health handle
before the move. The handle would have to be requested **before** `tokio::spawn(bridge.run())`,
which is what the docstring at `:96` specifies but `main.rs` does not do.
There is no `/health` route that exposes bridge connectedness.

### G8 — Bead publisher's `is_transient` is permissive

`src/services/nostr_bead_publisher.rs:241-247`. `is_transient` matches the
literal substrings "timeout", "connect", "closed without OK", "send failed".
A relay that responds with `["NOTICE", "auth-required"]` instead of `OK` would
hit `relay closed without OK` (line 196) → classified transient → infinite
retry against an AUTH-gated relay.

### G9 — `solid_pod_handler::extract_solid_path` is fragile

`src/handlers/solid_pod_handler.rs:369-382` uses `req.path().find("/solid")`
instead of consulting actix's match info. Any URL containing `/solid` anywhere
(e.g. `/api/foo/solid-not-really`) maps to whatever follows that substring as
the pod-relative path.

### G10 — `WellFormedOnlyVerifier` lives in production code

`src/services/nostr_identity_verifier.rs:69-92` ships a verifier that only
validates hex shape and never checks the Schnorr signature. It is intended
for CI, but it lives in `src/`, not `tests/`. No `#[cfg(test)]` gate. A
miswiring at `main.rs:730` would silently disable presence-handshake auth.

### G11 — Hand-rolled tungstenite vs `nostr_sdk::Client` divergence

Three publication paths use three different transport layers
(`nostr_sdk::Client` for server identity; raw `tungstenite` for bead publisher
and bridge). The two raw paths reimplement REQ/EVENT/OK frame parsing
inline. There is no unified frame parser; bug-fix in one path doesn't
propagate.

### G12 — Federation-hop hint message is a future-tense lie

`src/handlers/uri_resolver_handler.rs:198-204` returns a JSON body saying
"URN is owned by the agentbox sibling. Try its /v1/uri/ endpoint." but doesn't
include the URL. A client that obeys the hint blindly has to guess
`http://agentbox:9190/v1/uri/<urn>` — a guess that will work in this
deployment but is fragile across configurations.

### G13 — `legacy.rs` produces values still in production data

`src/uri/legacy.rs:1-21` documents that the two `canonical_iri_*` shims are
preserved because `opaque_id.rs:166` derives bit29 binary-protocol opaque ids
from the value. So even with `#[deprecated]`, the legacy minters are
load-bearing for the binary protocol. A hard removal would break opaque-id
stability across restarts. Migration plan exists in the comment, no code
implements it.

---

## 9. Observed coupling to forum or agentbox

### To Forum (DreamLab Forum relay)

- `NostrBridge` re-signs JSS kind 30001 events as kind 9 with `h="visionclaw-activity"`
  and forwards them to `FORUM_RELAY_URL` (`src/services/nostr_bridge.rs:182-247`).
- Server-signed kinds 30023/30100/30200/30300/30301/31400/31402 carry the
  `h="visionclaw-server"` group tag (`src/actors/server_nostr_actor.rs`,
  `SERVER_H_TAG` constant) so a forum relay running NIP-29 group filters can
  accept them on the same whitelist. Kinds 31400 (governance panel) and 31402
  (action request) are Agent Control Surface Protocol events designed for
  consumption by `nostr-bbs-core` governance UI components.

The forum is **a destination, not a peer**. The substrate writes to the forum
relay; it does not read from it.

### To Agentbox

Three live coupling points:

- **HTTP** — `src/services/management_api_client.rs:1-9` documents
  `VisionClaw → http://agentbox:9190 → Management API`. Comment at `:587, :591`
  uses `localhost:9190` as a test default; production uses the docker-compose
  service name `agentbox`.
- **WebSocket (`/wss/agent-events`)** — `src/handlers/agent_events_ws_handler.rs`,
  subprotocol `vc-agent-events.v1`, ADR-059 / ADR-014. Inbound: JSON
  `agent_action` envelopes from agentbox WS subscribers (`:5-8`). Outbound:
  `UserInteractionEvent` broadcast (`:181-196`). This is the nearest thing to
  the BC20 events_acl, but the inbound payload is consumed as-is into a
  visualisation actor (`TransientEdgeActor`) — no ACL translation, no Neo4j
  persistence, no Contributor Stratum bus.
- **`docker exec`** — `src/services/mcp_relay_manager.rs:96, :191, :227, :282`
  shells out to `docker exec agentbox …` for MCP relay lifecycle
  (status / start / stop / detect-process). This is host-level coupling, not
  agentbox-API coupling.

`src/services/agent_visualization_protocol.rs:90` documents that an
optional canonical URN field in agent-vis payloads "typically `did:nostr:<hex>`
or `urn:agentbox:agent:<scope>:<local>`". So agentbox URNs can flow into the
substrate as opaque strings, but no parser handles them — `src/uri/parse.rs:62`
returns `UriError::UnknownKind` for any kind other than `concept | group |
kg | bead | execution`.

### Cross-system tests

`grep` across `tests/` for nostr/federation/agentbox names:

```
tests/bridge_signing_fanout.rs    — server-identity bridge promotion fan-out
tests/server_identity.rs          — ServerIdentity unit-shaped integration tests
tests/auth_sovereign_mesh.rs      — NIP-98 optional auth + visibility filter
tests/uri_grammar.rs              — pure URI mint/parse tests
tests/visibility_transitions.rs   — visibility transition saga
```

None of these exercises a live cross-system flow:

- No test boots a real Nostr relay (`nostr-rs-relay` or similar) and verifies
  publish.
- No test boots agentbox's management-api and verifies a federation hop.
- No test verifies that an agentbox-emitted bead URN can be resolved back to a
  VisionClaw concept URN.
- The `uri_grammar` suite (`tests/uri_grammar.rs:1-60+`) covers mint
  determinism, parse round-trip, normalisation — pure computation, no I/O.

There is **no end-to-end Nostr-relay integration test** in the substrate.
Running the substrate against a real `wss://relay.damus.io` or against a local
ephemeral relay is a manual exercise.

### PLANNED-BUT-NOT-DONE

- Federation handshake `GET http://agentbox:9190/v1/meta` at boot
  (`docs/ddd-agentbox-integration-context.md:84-108`). No code calls it.
- LocalFallbackProbe over Ed25519-signed `/probe/origin`
  (`docs/ddd-agentbox-integration-context.md:24, :104-105`). No code performs
  the probe.
- A long-lived stdio session reader for agentbox's `stdio-bridge.js`
  (PRD-006 §5.7, open-question Q1). The closest thing is the
  `/wss/agent-events` WebSocket, which is inbound TCP on the substrate side,
  not stdio.
- Cross-substrate JSON-LD context federation
  (`docs/PRD-006-…:275-283`). VisionClaw context is not pinned in any
  agentbox-side `linked-data-contexts.nix`.

---

## Citations index

| Subject                                  | File:line                                               |
| ---------------------------------------- | ------------------------------------------------------- |
| Server identity load                     | `src/services/server_identity.rs:64-128`                |
| `connect_relays` (publish-only client)   | `src/services/server_identity.rs:138-155`               |
| `sign_and_broadcast`                     | `src/services/server_identity.rs:219-262`               |
| `SUPPORTED_KINDS = [30023,30100,30200,30300]` | `src/services/server_identity.rs:33`              |
| Identity HTTP route                      | `src/handlers/server_identity_handler.rs:28-38, main.rs:955-958` |
| `ServerNostrActor` four messages         | `src/actors/server_nostr_actor.rs:99-338`               |
| Bridge subscribe + verify + republish    | `src/services/nostr_bridge.rs:139-247`                  |
| Bridge boot                              | `src/main.rs:570-575`                                   |
| Bead publisher write path                | `src/services/nostr_bead_publisher.rs:88-200`           |
| URI mint                                 | `src/uri/mint.rs:14-83`                                 |
| URI parse + normalise pubkey             | `src/uri/parse.rs:31-260`                               |
| Content hash 12                          | `src/uri/parse.rs:269-281`                              |
| Resolver dispatch                        | `src/handlers/uri_resolver_handler.rs:148-173`          |
| Federation hop placeholder               | `src/handlers/uri_resolver_handler.rs:193-205`          |
| Solid Pod handler entry                  | `src/handlers/solid_pod_handler.rs:1-7, 126-178`        |
| NIP-98 verify                            | `src/handlers/solid_pod_handler.rs:386-399`             |
| WebID derivation                         | `src/handlers/solid_pod_handler.rs:401-411`             |
| Solid service wiring + fatal init        | `src/main.rs:704-715, 927`                              |
| Agent events WS handler                  | `src/handlers/agent_events_ws_handler.rs:1-196`         |
| Management API client → agentbox:9190    | `src/services/management_api_client.rs:1-9`             |
| MCP relay docker exec                    | `src/services/mcp_relay_manager.rs:96-296`              |
| Identity verifier (Schnorr)              | `src/services/nostr_identity_verifier.rs:34-64`         |
| Permissive verifier (CI stub)            | `src/services/nostr_identity_verifier.rs:69-92`         |
| URN/Solid mapping                        | `src/services/urn_solid_mapping.rs:1-246`               |
| `solid-pod-rs` dependency pin            | `Cargo.toml:24-26`                                      |
| Legacy URN shims                         | `src/uri/legacy.rs:31-66`                               |
| BC20 directory absence                   | `src/bc20/` (does not exist)                            |
| Tests inventory                          | `tests/server_identity.rs`, `tests/bridge_signing_fanout.rs`, `tests/auth_sovereign_mesh.rs`, `tests/uri_grammar.rs`, `tests/visibility_transitions.rs` |
