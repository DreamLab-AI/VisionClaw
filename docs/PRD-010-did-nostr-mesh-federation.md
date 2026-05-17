# PRD-010 — DID:Nostr Mesh Federation

| Field | Value |
|-------|-------|
| Status | Phase 0-1 complete; Phase 2 kit extraction done; federation implementation in progress |
| Authors | Multi-agent research swarm + synthesis (J. O'Hare) |
| Predecessors | PRD-004, PRD-006, ADR-053, ADR-054, ADR-058, ADR-061 |
| Companion ADRs | ADR-073, ADR-074, ADR-075, ADR-076 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` |
| Spans repos | `visionclaw` (this repo), `dreamlab-ai-website` (forum + community-forum-rs), `agentbox` (submodule), `solid-pod-rs` (workspace) |
| Source evidence | `docs/integration-research/01..06-*.md` |

---

## 1. Executive Summary

DreamLab operates three substrates — **VisionClaw** (Rust knowledge-graph & XR substrate),
**Agentbox** (Nix container hosting did:nostr-keyed agents), and the **Dreamlab community
forum** (Leptos WASM + 5 CF Workers) — that today share a vocabulary
(`did:nostr:<hex>`, `sha256-12-<12hex>` content addressing, BIP-340 Schnorr secp256k1)
but **cannot exchange cryptographically authenticated messages** with each other.

The shared substrate (`solid-pod-rs` workspace) is mature on Solid LDP / WAC / NIP-98 /
DID-Tier-3, but its `Storage` trait is Tokio-coupled, NIP-98 has no replay-store trait,
and the embedded relay advertises only NIPs 1/11/16 — none of NIP-04/17/26/44/59/65/90.
Forum reimplements ~4,200 LOC of equivalent surface for Cloudflare Workers; agentbox
runs a `nostr-rs-relay` 0.9.0 process bound to `127.0.0.1:7777` that **no other
container can reach**; agentbox's pod-inbox bridge (`mcp/nostr-bridge/relay-consumer.js`,
473 LOC, fully implemented) is **not wired into management-api boot**; VisionClaw's
BC20 anti-corruption layer is paper-only (zero code, six modules promised in PRD-006).

This PRD specifies the minimum coherent set of changes — protocol, deployment, code,
and contract — to convert these three islands into a federated mesh in which:

1. Any actor (forum user, agentbox agent, VisionClaw substrate) holds an independent
   `did:nostr` identity rooted in their own custody regime.
2. Messages signed by any actor are routable through any of the three relay endpoints
   to any other actor on the mesh.
3. Authority can be delegated cross-system via NIP-26 so that a user's forum key can
   authorise an agentbox agent or a VisionClaw bridge to act on their behalf with
   bounded scope.
4. Each actor's discoverable identity record (DID document) advertises both Solid pod
   and Nostr relay service endpoints so a third party who knows only the hex pubkey
   can route messages without out-of-band configuration.
5. Deployment is configurable: each substrate ships a `[mesh]` block in its own manifest
   choosing **standalone** (private relay only, no federation), **federated** (its own
   relay plus N peer relays), or **client** (no relay; speaks to a peer's relay).

The scope is deliberately bounded to **mesh federation messaging**. It does NOT
attempt to unify the URN namespaces (`urn:visionclaw:*` vs `urn:agentbox:*`) — that is
PRD-006's territory — nor does it touch GPU position streaming, the binary protocol,
or the XR-Godot replacement (PRD-007/008).

---

## 2. Goals

### G1 — A single canonical `did:nostr` resolution path
Any of the three systems, given a 64-char lowercase hex pubkey, can fetch a Tier-3 DID
document advertising at minimum: `#solid-pod` (LDP storage), `#nostr-relay` (preferred
inbox/outbox relay), `#webid` (Solid WebID URL).

### G2 — A unified inter-system message envelope
A single JSON-shape carries cross-system messages over Nostr and is mappable
losslessly to LDN, with provable origin signature, target identity, optional NIP-26
delegation, and a stable URN reference to the originating context.

### G3 — Three relay topologies, one wire protocol
The forum CF Workers relay, the agentbox embedded `nostr-rs-relay`, and a (new or
existing) VisionClaw-side relay all conform to the same private-mesh protocol
(NIPs 1/9/11/16/26/40/42/44/59/65/98), publish a common NIP-11 capability document,
and accept events from any pubkey present in their administrative whitelist.

### G4 — NIP-42 AUTH as the universal write gate
All write paths on every relay require NIP-42 AUTH; reads of public kinds (0, 1, 3, 7,
30000-39999 except moderation) remain open; reads of {4, 13, 14, 1059, 30910-30916}
are AUTH-gated and `#p`-rewritten.

### G5 — NIP-26 as the universal trust pivot
A user-key (forum), agent-key (agentbox), or operator-key (VisionClaw) can delegate
event-signing authority to another key with `kind=N` and `created_at>T` constraints;
all three substrates verify delegation tags before accepting cross-attribution.

### G6 — Discovery without prior configuration
A forum user who knows only `did:nostr:<hex>` can DM an agentbox agent; an agentbox
agent who receives a forum kind-1059 can reply via the same relay; a VisionClaw
substrate observing a bead URN can resolve back to the originating user's relay.

### G7 — Deployment options
Each substrate's manifest exposes `mesh.mode = standalone | federated | client`,
`mesh.peer_relays = [...]`, and `mesh.allowed_pubkeys = [...]`. Defaults preserve
current behaviour (standalone). Operators flip the federation switch; no recompile.

### G8 — Cryptographic correctness before scale
Sprint v9 audit findings + the new specialist research (`docs/integration-research/05-crypto-gotchas.md`)
identified three CRITICAL crypto bugs that must be fixed before any cross-system
message flows. They are scoped into this PRD as gating prerequisites (§7).

### G9 — Established Nostr protocol library, not hand-roll
Forum's `community-forum-rs/crates/nostr-core/` contains 7,892 LOC of hand-rolled
Nostr protocol (NIP-01/04/19/26/44/52/56/59/90/98 + custom moderation), pulling
RustCrypto primitives but reimplementing the protocol layer above them. The C1
NIP-44 v2 critical bug — `Hkdf::new(salt,ikm).expand(&[],&mut conv_key)` instead
of HKDF-Extract — is the textbook failure mode an established library prevents.
Per ADR-076, the protocol layer is absorbed into the upstream `nostr` crate
(rust-nostr.org, already declared in workspace Cargo.toml at `nostr = "0.44"` but
unused), reducing `nostr-core` to a ~700-LOC shim covering project-specific kinds
(30033, 30050, 30910-30916), the PRF→Keys derivation, the three-backend signer
trait, and the WASM bridge.

---

## 3. Non-Goals

- **NG1**: Public discovery / search. Mesh is private; pubkey allowlists gate write
  access. There is no global DHT, no public relay directory.
- **NG2**: Anonymous messaging. Every message is identity-attributed via signature;
  metadata privacy is bounded by NIP-59 gift-wrap (sender hidden from relay) but the
  recipient `p` tag remains visible.
- **NG3**: Real-time streaming media (audio/video, GPU positions). The 24-byte/node
  binary protocol (ADR-061) and any XR streams stay out-of-band.
- **NG4**: URN namespace unification. PRD-006 owns the `urn:visionclaw:*` ↔
  `urn:agentbox:*` translation. This PRD references the BC20 ACL but does not redefine
  it.
- **NG5**: A new general-purpose Nostr relay implementation. We use existing relays
  (CF Worker DO, `nostr-rs-relay`, optional VisionClaw side process) and extend their
  protocol coverage.
- **NG6**: Multi-relay client fan-out for the forum web UI. NIP-65 outbox routing is
  a P3 follow-up; P1/P2 single-preferred-relay-per-actor is sufficient for federation.
- **NG7**: Replacing solid-pod-rs's `Storage` trait. The needed trait redesign for
  WASM Workers compatibility is a separate `solid-pod-rs` 0.5 effort tracked in
  ADR-028 amendments.

---

## 4. Current-State Evidence (load-bearing facts)

The following non-obvious facts are taken verbatim from `docs/integration-research/`
and inform every requirement in §5.

### 4.1 VisionClaw (`docs/integration-research/01-visionclaw-surfaces.md`)

- **Two unrelated keypairs** are loaded from `SERVER_NOSTR_PRIVKEY` and
  `VISIONCLAW_NOSTR_PRIVKEY` (`src/services/server_identity.rs:64-128`,
  `src/services/nostr_bridge.rs:62-94`); `pod_client.rs:10-13` flags this as
  transitional. There is no mechanism that asserts they are the same key.
- **No NIP-42 AUTH support** — `grep -rn "NIP-42|nip42|AUTH.*relay" src/` returns
  empty. If any external relay starts requiring AUTH, all three publication paths
  silently fail.
- **No subscription** — the only persistent Nostr subscription is `NostrBridge`
  reading kind 30001 from JSS; `nostr_sdk::Client` in `ServerIdentity` is publish-only.
  The substrate **never reads back** kinds 30023/30100/30200/30300/30301/31400/31402 it itself emits. (Note: kinds 31400/31402 are Agent Control Surface Protocol events intended for Forum Kit UI consumption, not VisionClaw read-back.)
- **URI resolver redirects to non-existent endpoints** —
  `src/handlers/uri_resolver_handler.rs:148-173` issues 307s to
  `/api/v1/nodes/by-uri/{urn}/jsonld`, `/api/v1/identity/{hex}/did.json`,
  `/api/v1/wac/groups/{team}` — **none of those routes are registered** in this repo.
  Every successful resolve lands the client at a 404.
- **BC20 anti-corruption layer is paper** — `src/bc20/` does not exist; six modules
  + two aggregates from PRD-006 §5.5 are zero-LOC.

### 4.2 Forum (`docs/integration-research/02-forum-surfaces.md`)

- **Single relay URL hard-coded** at `forum-client/src/relay.rs:23`:
  `wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev`. NIP-65 outbox routing
  is **not** implemented client-side; the forum is structurally single-relay.
- **AUTH challenge is sent on connect** (`relay-worker/src/relay_do/mod.rs:140`) but
  the **client has no `["AUTH", challenge]` handler** (`relay.rs:439-524`). Kind-1059
  reads are AUTH-gated server-side, so users cannot read DMs through the current
  client. **Highest-impact gap.**
- **NIP-07 users cannot DM** — `dm/mod.rs:294` requires raw 32-byte privkey; extension
  sessions return `None` from `get_privkey_bytes` even though `Nip07Signer::nip44_*`
  is wired.
- **Kind-4 decoder uses NIP-44** — `dm/mod.rs:482` calls `nip44_decrypt` for kind 4.
  Genuine NIP-04 ciphertext from external clients will not decrypt.
- **Walled garden over public Nostr wire** — relay-worker is a public CF Worker URL;
  admission is gated by D1 whitelist + trust level (`nip_handlers.rs:42-258`); auto-
  whitelist on first kind-0/9021/9024 publish (`nip_handlers.rs:65-81`).
- **DID Tier-3 doc does NOT advertise the user's preferred relay** —
  `pod-worker/src/lib.rs:213` passes `relay_url=None` "not included at Tier 3 without
  lookup". So a third party with `did:nostr:<hex>` learns the pod URL but not the
  relay URL.

### 4.3 Agentbox (`docs/integration-research/03-agentbox-surfaces.md`)

- **`nostr-rs-relay` runs on `127.0.0.1:7777`** (`flake.nix:1129-1141`,
  `agentbox.toml:87-104`) but **port 7777 is not in `docker-compose.yml`** (only
  9090, 9700, 9091, 8484, 8888, 5901, 8080) and `flake.nix:1968-1970` only adds 8484
  to `sovereignPorts`. Embedded relay is **invisible from outside the container**.
- **`RelayConsumer` is fully implemented (473 LOC) but not wired** —
  `mcp/nostr-bridge/relay-consumer.js:70-471`; `grep -rn "RelayConsumer"
  management-api/` returns zero hits. The class enforces DDD-003 invariants I01/I07/I08/I10
  (signature-before-write, allowlist, content-addressed dedup, recipient match).
  Without boot wiring, no inbound Nostr event is ever turned into a pod inbox file.
- **Sovereign identity bootstrap encodes the wrong pubkey as npub** —
  `scripts/sovereign-bootstrap.py:90-91, 133-134` bech32-encodes the **64-byte
  uncompressed SEC1 pubkey** instead of the 32-byte BIP-340 x-only form. Any
  standards-compliant NIP-19 decoder rejects this npub.
- **Operator pubkey is not auto-whitelisted on the embedded relay** — `agentbox.toml:64-68`
  *claims* the operator key has relay-allowlist access, but `flake.nix:732-736` emits
  `pubkey_whitelist` only from `relayCfg.allowed_pubkeys` (empty), not from
  `operator.pubkey_hex`. Comment-as-spec drift.
- **Linked-Data S4 encoder DOES advertise relay endpoints** — `s04-did.js:55-64`
  emits `#nostr-relay` `serviceEndpoint = ws://${bind}:${port}` when `relay` is in
  `service_endpoints`, but the URL points at loopback when accessed externally.
- **No multi-agent identity per container** — `sovereign-bootstrap.py:233`
  hardcodes `agent_id = os.getenv("AGENTBOX_AGENT_ID", "agentbox-core")` — one
  keypair per container.

### 4.4 solid-pod-rs (`docs/integration-research/04-solid-pod-rs-surfaces.md`)

- **`Storage` trait is Tokio-coupled** —
  `crates/solid-pod-rs/src/storage/mod.rs:73, 131` requires
  `Send + Sync + 'static` and returns `tokio::sync::mpsc::Receiver` from `watch()`.
  This is the structural reason forum's pod-worker reimplements 2,300+ LOC for
  Cloudflare Workers.
- **NIP-98 has no replay-store trait** — `auth/nip98.rs` only does ±60s timestamp
  tolerance. Forum's `nostr-core::Nip98ReplayStore` (`nip98.rs:45`) is the reference
  contract that solid-pod-rs needs to absorb.
- **Embedded relay advertises only NIPs 1, 11, 16** (`relay.rs:330`). No NIP-04,
  NIP-17, NIP-44, NIP-59, NIP-65, NIP-26, NIP-09, NIP-90.
- **DID resolver cannot resolve Tier-3 from pubkey + relay URL** —
  `resolve_nostr_to_webid` (`resolver.rs:136`) requires an HTTPS *origin URL*,
  not a Nostr relay URL.
- **WS transport is Tokio-only** — only `dispatch_message` (`ws.rs:111`) is the
  pure parser portable to WASM.
- Three consumers are version-skewed: VisionClaw + agentbox at `0.4.0-alpha.1`
  (published), forum at `0.4.0-alpha.2` (published); workspace tree is unreleased
  Sprint 9-12 work.

### 4.5 Cryptographic alignment (`docs/integration-research/05-crypto-gotchas.md`)

Three CRITICAL drifts block any cross-system message flow:

- **C1**: NIP-44 v2 conversation key derivation is incorrect — `nostr-core/src/nip44.rs:122-128`
  uses `Hkdf::new(salt,ikm).expand(&[], &mut conv_key)` which computes
  `HMAC-SHA256(PRK, 0x01)` instead of the PRK itself. Forum DMs and gift-wrap are
  **not interoperable** with reference Nostr clients.
- **C2**: agentbox's `sovereign-bootstrap.py` bech32-encodes the 64-byte SEC1 pubkey
  as npub instead of the 32-byte x-only form — broken NIP-19.
- **C3**: Three different `verificationMethod.type` strings: forum emits
  `SchnorrSecp256k1VerificationKey2019`; solid-pod-rs-nostr emits
  `NostrSchnorrKey2024`; sovereign-bootstrap.py emits
  `SchnorrSecp256k1VerificationKey2022` (a non-existent cryptosuite).

NIP-26 delegation **exists in nostr-core** (`nip26.rs:64-95, 239-244, 282`) but is
**not wired into agentbox event ingest or VisionClaw `nostr_bridge.rs:219-222`**;
the bridge re-signs events under its own key, **losing original-author attribution**.

### 4.6 URI / data-flow alignment (`docs/integration-research/06-uri-dataflow-alignment.md`)

- `did:nostr:<hex>` is byte-identical across all three substrates; `sha256-12-<12hex>`
  content addressing is byte-identical (PRD-006 F10 confirmed).
- `urn:visionclaw:bead:*` and `urn:agentbox:bead:*` collide in name but disagree in
  content addressing (VC content-addressed; agentbox slug-id'd).
- `urn:visionclaw:execution` drops actor scope in URN body; `urn:agentbox:event`
  preserves scope — round-trip is lossy.
- **No URN form represents "agent X owned by user U running in container C on host H"**.
  The mesh needs many DIDs (forum-user, agentbox-agent, VisionClaw-operator, possibly
  more) and no relational URN binds them.
- **One antipattern in the wild**: `src/services/parsers/block_level_parser.rs:209`
  produces a non-canonical 5-segment Concept URN via raw `format!`; the PRD-006 §6
  anti-drift CI gate is documented but **not enforced**.

---

## 5. Architecture: The Mesh

### 5.1 The actor / DID model

Every participant in the mesh holds **exactly one** `did:nostr:<hex>` identity. There
are at least **six independent classes of actor**:

1. **Forum user** (`U`) — passkey-PRF-derived or NIP-07-extension-held secp256k1
   keypair; lives in the user's browser. Identity scope: per-human.
2. **Agentbox sovereign agent** (`A`) — keypair generated by
   `sovereign-bootstrap.py` and persisted to
   `/var/lib/agentbox/identities/<agent_id>.json`. Identity scope: per-agent
   (multiple agents per container is a P2 follow-up).
3. **VisionClaw operator** (`V`) — keypair loaded from `SERVER_NOSTR_PRIVKEY`.
   Identity scope: per-substrate. (After the dedup fix in §7.1, also covers
   `VISIONCLAW_NOSTR_PRIVKEY`.)
4. **Forum admin / moderator** (`M`) — same key shape as `U` but with
   `is_admin=true` in the relay-worker's D1 whitelist. Has cross-cohort moderation
   authority.
5. **Bridge identity** (`B`) — synthetic key used by `NostrBridge` to re-sign
   forwarded events. Today this conflates with `V`; see §7.1.
6. **Bot / DVM** (`D`) — NIP-90 service-provider keys; out-of-scope for inbox/outbox
   federation but reserved for future.

Each actor publishes a **DID document** at a stable URL (DID-Tier-3, `SchnorrSecp256k1VerificationKey2019`)
advertising:

```jsonld
{
  "@context": [
    "https://www.w3.org/ns/did/v1",
    "https://w3id.org/security/suites/secp256k1-2019/v1"
  ],
  "id": "did:nostr:<hex>",
  "alsoKnownAs": ["<webid_url>"],
  "verificationMethod": [{
    "id":   "did:nostr:<hex>#key-0",
    "type": "SchnorrSecp256k1VerificationKey2019",
    "controller": "did:nostr:<hex>",
    "publicKeyHex": "<hex>",
    "publicKeyMultibase": "z<base58btc(0xe7 0x01 || pk)>"
  }],
  "authentication":  ["did:nostr:<hex>#key-0"],
  "assertionMethod": ["did:nostr:<hex>#key-0"],
  "service": [
    { "id": "...#solid-pod",   "type": "SolidStorage", "serviceEndpoint": "<pod_url>"   },
    { "id": "...#nostr-relay", "type": "NostrRelay",   "serviceEndpoint": "<wss_url>"   },
    { "id": "...#webid",       "type": "SolidWebID",   "serviceEndpoint": "<webid_url>" },
    { "id": "...#mesh",        "type": "DIDNostrMesh", "serviceEndpoint": "<peer_relays_csv>" }
  ]
}
```

The `#mesh` service is new (this PRD) and lists peer relay URLs the actor accepts
inbound on. See ADR-074 for the canonicalisation rules.

### 5.2 The relay topology

Three relay instances form the federated mesh:

| Relay | Implementation | Role | Auth | Default scope |
|-------|----------------|------|------|---------------|
| **Forum CF relay** | `relay-worker` Durable Object | `wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev` | NIP-42 + D1 whitelist | All `U`, `M`; opt-in `A`/`V` |
| **Agentbox relay** | `nostr-rs-relay` 0.9.0 in `agentbox` container | `ws://0.0.0.0:7777` (when exposed; `wss://` via TLS bridge) | NIP-42 + `allowed_pubkeys` | One or more `A` per container; opt-in `U`/`V`/`M` |
| **VisionClaw relay** *(new, optional)* | `nostr-rs-relay` colocated with substrate | `wss://relay.<vc_host>:7777` | NIP-42 + env-configured allowlist | All `V`; opt-in others |

Topology: **bidirectional federation** via NIP-65-style outbox advertisement. Each
relay treats the others as peer outbox endpoints; cross-relay event delivery happens
via:

1. **Outbox publish**: actor signs an event, publishes to *their* preferred relay
   (advertised via `#nostr-relay` service in DID Document), tagging recipient `p`.
2. **Mesh fan-out** (P2): the relay, on event accepted, optionally re-publishes
   to peer relays in `mesh.peer_relays` if event.kind in `mesh.federated_kinds`.
   Implemented as a per-relay outbound worker — not a kernel-level mesh routing.
3. **Inbox read**: recipient's bridge (forum read-side / agentbox `RelayConsumer` /
   VisionClaw `nostr_bridge`) connects to **all** relays advertised in their own
   `#mesh` service, AUTHs, subscribes by `#p`, deduplicates by `event.id`.

Why bidirectional federation and not hub-and-spoke?

- Hub-and-spoke (e.g. forum CF relay as hub) creates a single point of failure +
  trust — if Cloudflare goes down or revokes the worker, every cross-system
  message stops. It also means agentbox's per-container relay becomes purely
  decorative.
- Each substrate already has reasons to run its own relay (forum: walled-garden
  membership; agentbox: per-container sovereignty; VisionClaw: optional but useful
  for substrate-emitted beads). Federation just connects them.

### 5.3 The message envelope contract (IS-Envelope v1)

Inter-system messages ride **gift-wrapped Nostr events** (kind 1059) carrying a
**rumor of kind 14** (chat) or new mesh-specific kinds (defined in ADR-075). The
**rumor content** is the IS-Envelope v1 JSON shape:

```jsonc
{
  "v":      1,                                         // envelope version
  "to":     "did:nostr:<hex>",                          // recipient identity
  "from":   "did:nostr:<hex>",                          // origin identity
  "via":    ["did:nostr:<bridge_hex>"],                 // optional re-attribution chain
  "subj":   "urn:visionclaw:bead:<scope>:<sha256-12>",  // optional originating context URN
  "ttl":    1763000000,                                 // unix ts; envelope MUST NOT be processed past this
  "kind":   "chat" | "tool_invoke" | "tool_result" | "knowledge_link" | "moderation" | "mesh_ping",
  "lang":   "text/markdown" | "text/plain" | "application/json+ld",
  "body":   "<string|object>",                          // payload, shape depends on kind
  "hint":   { ... },                                    // optional rendering / routing hints
  "delegation": {                                       // optional NIP-26 delegation token
    "delegator": "did:nostr:<hex>",
    "conditions": "kind=14&created_at>1763000000",
    "sig": "<128 hex>"
  }
}
```

Signature semantics:
- The **outer kind 1059 wrap** is signed by an ephemeral throwaway key (NIP-59).
- The **kind 13 seal** is signed by `from`'s key (or by `via[last]`'s key with
  `delegation` populated and verified).
- The **kind 14 rumor** is *unsigned* per NIP-59; integrity comes from the seal.

This satisfies G2 (uniform envelope), G5 (NIP-26 trust pivot), supports NIP-59
metadata privacy without losing the routing recipient (`p` tag on the wrap), and
maps losslessly to a Solid LDN payload via straightforward field rename.

ADR-075 specifies the envelope formally including all `kind` variants, the LDN
mapping, and the canonical JSON serialisation.

### 5.4 Discovery: how `did:nostr` becomes a routable address

**Step 1 — Pubkey to DID Document**. Three resolvers in priority order:

1. **DID-via-relay** (new): query any relay in `mesh.peer_relays` for
   `Filter { authors: [hex], kinds: [0, 30033], limit: 2 }` (kind 0 metadata + kind
   30033 mesh service-list, see ADR-075). Parse `content.tags` for service URLs.
   Latency: 200-2000 ms. Authority: bound to relay trust.
2. **DID-via-`.well-known`** (existing in solid-pod-rs-nostr): HTTPS GET
   `<origin>/.well-known/did/nostr/<hex>.json`. Latency: 50-300 ms. Authority: bound
   to TLS + WebID sameAs check (`solid-pod-rs-nostr/src/resolver.rs:136-186`).
3. **DID-via-pod** (existing): if pod URL is known, GET `<pod>/.well-known/did.json`.
   Latency: 50-300 ms. Authority: bound to pod ACL.

Mesh implementations MUST try (1) before (2) before (3) and cache the resolved DID
Document with TTL ≤ `min(token_ttl, 600s)`.

**Step 2 — DID Document to relay URL**. Read `service[?type=NostrRelay].serviceEndpoint`.
If absent, fall back to `service[?type=DIDNostrMesh].serviceEndpoint` (CSV) and pick
the first element. If still absent, the recipient is unreachable — log and drop.

**Step 3 — Relay AUTH**. Connect to the relay over `wss://`. Wait for
`["AUTH", challenge]`. Sign a kind-22242 event with `["challenge", token]` and
`["relay", url]` tags, send `["AUTH", event]`. Server validates and binds session.

**Step 4 — Subscribe / publish**. Subscribe with `Filter { #p: [me_hex], kinds: [1059] }`
to receive inbox; publish gift-wrapped envelopes with `["p", recipient_hex]` tag.

### 5.5 Authority delegation: the NIP-26 trust pivot

Three primary delegation patterns:

**Pattern α — User → Substrate Bridge** (replaces today's silent re-signing):
- User `U` issues delegation `δ_U→V` with `kind=14&kind=1059&created_at<T+24h`.
- VisionClaw bridge stores `δ_U→V` and uses it when forwarding U-originated content.
- Outbound seal includes `δ_U→V` in the envelope's `delegation` field.
- Recipient verifies `δ_U→V`, attributes content to `U` (display) but knows wire was
  signed by `V` (provenance).

**Pattern β — User → Agentbox Agent** (enables agent action on behalf of user):
- User `U` issues delegation `δ_U→A` with `kind=4..29999&created_at<T+T_session`.
- Agent `A` includes `δ_U→A` in any event it signs that should be attributed to U.
- WAC ACLs on U-owned pods recognise both `did:nostr:<U>` and signed-with-delegation
  agents; pod-worker's `acl.rs::agent_matches` extends to recognise delegation tags.

**Pattern γ — Server → Server** (mesh trust):
- VisionClaw operator issues delegation `δ_V_a→V_b` for forwarding between substrates.
- Used only for substrate-emitted beads, not user content.

ADR-074 formalises the delegation grammar and verifier wiring.

### 5.6 Data flow exemplars

**Exemplar 1 — Forum user sends DM to agentbox agent**:

```
[U browser] → forum-client.dm.send_message(plaintext, recipient=A_hex)
            → nostr_core.gift_wrap(rumor=kind_14, sender_seal=U_key, recipient=A_hex)
            → forum-client.relay.publish(kind_1059_event)
   relay-worker checks NIP-42 AUTH (U authed) and ingests event
            → broadcasts to all sessions where session.authed_pubkey == event.p[1]
            (no agentbox agent connected to forum relay yet — see fan-out below)

[mesh fan-out, P2 implementation]
   relay-worker outbound worker observes event.kind==1059, event.p[1]==A_hex,
            looks up A_hex in mesh.peer_relays roster (cached from agentbox's DID Doc),
            republishes to ws://agentbox-host:7777 over a server-key-AUTH'd session.

[A in agentbox] RelayConsumer.subscribe at boot pulled kinds=[1059, ...] for
            the agent's npub; receives event, _verifySig OK, _passesIngressPolicy OK,
            recipient_match OK (event.p[1] == A_hex);
            writes wrapped event to pods/<A_npub>/events/inbox/<event_id>.json.
            Agent process polls inbox, sees new file, decrypts via gift_wrap.unwrap_gift,
            extracts IS-Envelope rumor, dispatches to handler.

[A reply path]
   Agent writes pods/<A_npub>/events/outbox/<pending>.json with target=U_hex.
   RelayConsumer outbox pump signs the gift wrap with A's key, publishes to
   ws://agentbox:7777, mesh fan-out republishes to forum CF relay (forum is in
   A's peer_relays), forum-client subscription delivers to U.
```

**Exemplar 2 — Forum thread referenced as VisionClaw bead** (knowledge linking):
- VisionClaw operator's `nostr_bridge` subscribes to forum relay for kinds [1, 9, 30023]
  in a configured channel.
- For each post, computes `urn:visionclaw:bead:<author_hex>:<sha256-12 of content>`.
- Mints `urn:visionclaw:kg:<author_hex>:<sha256-12>` if the post crosses a salience
  threshold; persists to Neo4j.
- Publishes a `knowledge_link` IS-Envelope back to the forum (kind-1059 wrapped,
  recipient = post author) saying "your post was indexed at <urn>".

**Exemplar 3 — Cross-substrate moderation propagation**:
- Forum admin emits `KIND_BAN` (30910) for pubkey `X` on forum CF relay.
- Mesh fan-out republishes to agentbox + VisionClaw relays.
- Each consumer checks signer admin status (per its own admin roster) and either
  honours (drops X's events) or ignores (X is unknown locally).
- This is opt-in per relay via `mesh.honor_remote_moderation = ["did:nostr:<adminA>", ...]`.

---

## 6. Functional Requirements

### F1 — Identity unification within VisionClaw
Substrate MUST resolve `SERVER_NOSTR_PRIVKEY` and `VISIONCLAW_NOSTR_PRIVKEY` to a
single identity at boot. Path:
- Default: both must reference the same key bytes; fail-closed if they diverge with
  `ErrIdentityKeysplit`.
- Migration: read either, log warning, normalise; both env vars writable for one
  release cycle.
- Code change: new `UnifiedServerIdentity::from_env()` superseding both
  `ServerIdentity::from_env()` and the `VISIONCLAW_NOSTR_PRIVKEY` reads in
  `nostr_bridge.rs:62-94` and `nostr_bead_publisher.rs:46-70`.

### F2 — DID Document publication
Each substrate MUST serve `GET /.well-known/did/nostr/<hex>.json` (or a federation
resolver `GET /api/v1/uri/did:nostr:<hex>` that 307s to the canonical location).
- Forum: already lives at pod-worker (`lib.rs:362-380`) — extend service[] to
  include `#mesh` and (when known) `#nostr-relay`.
- VisionClaw: ADD handler at `src/handlers/identity_did_handler.rs`. Mount at
  `/api/v1/identity/{hex}/did.json` (matches the resolver's redirect target).
- Agentbox: ADD Fastify route in `management-api/routes/did-document.js`. Mount at
  `/.well-known/did.json` (consume the operator key). Per-pod docs already live at
  `<pod>:8484/.well-known/did/nostr/<hex>.json`.

### F3 — Tier-3 DID Document service entries
Every published DID Document MUST include service entries for:
- `#solid-pod` (when an LDP pod URL is known for this pubkey)
- `#nostr-relay` (preferred inbox relay)
- `#webid` (when a Solid WebID URL is known)
- `#mesh` (CSV of peer relay URLs the actor accepts on)

### F4 — verificationMethod canonicalisation
Every emitter MUST use `SchnorrSecp256k1VerificationKey2019` and include
`https://w3id.org/security/suites/secp256k1-2019/v1` in `@context`. Patches required:
- `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs:98, 154` — change from
  `NostrSchnorrKey2024`.
- `agentbox/scripts/sovereign-bootstrap.py:192` — change from
  `SchnorrSecp256k1VerificationKey2022`.
- Forum already aligned (`pod-worker/src/did.rs:88, 146`).
- VisionClaw new emitter: align from inception.

### F5 — NIP-19 npub correctness in agentbox
`agentbox/scripts/sovereign-bootstrap.py` MUST compute the BIP-340 x-only pubkey
(force even-y via `lift_x` parity) and bech32-encode the 32-byte form. Existing
container identity files (`/var/lib/agentbox/identities/<id>.json`) MUST be
re-derived in place: read `private_key_hex`, compute correct x-only pubkey,
re-bech32, rewrite both `npub` field and any pod filesystem paths under
`pods/<npub>/...`. Migration script ships with `agentbox` 0.5.x.

### F6 — NIP-44 v2 conversation key correctness
`nostr-core/src/nip44.rs:122-128` MUST replace
`Hkdf::new(salt,ikm).expand(&[],&mut conv_key)?` with direct HMAC-SHA256(salt, ikm).
Reference test vectors (paulmillr/nip44) MUST be added. Until this lands, no
cross-system DM is interoperable — gating prerequisite.

### F7 — Universal NIP-42 AUTH gate
All relays in the mesh MUST require AUTH for any write and for reads of
{4, 13, 14, 1059, 30910-30916}. forum-client MUST gain an AUTH-RESP code path:
- `forum-client/src/relay.rs:439-524 handle_relay_message` add `["AUTH", challenge]`
  case → schedule `respond_to_auth_challenge(challenge)`.
- `respond_to_auth_challenge` builds kind-22242 event via signer trait, sends
  `["AUTH", event]`. After OK, retry any pending kind-1059 subscriptions.
- agentbox `RelayConsumer.subscribe` and VisionClaw `NostrBridge.run_once` add
  symmetric handling.

### F8 — NIP-26 delegation verifier wiring
Every event-ingest path MUST verify any present `["delegation", ...]` tag against
the configured trust roots:
- Forum: already verified by `auth-worker /api/delegation/verify` in API path; ADD
  inline verification in `relay-worker/src/relay_do/nip_handlers.rs::handle_event` so
  delegated events are accepted with delegator attribution.
- Agentbox: ADD `nostr_core::nip26::DelegationToken::verify` import in
  `mcp/nostr-bridge/relay-consumer.js::_processEvent` (Node port of the Rust verifier
  via `nostr-tools` already pulls this; needs explicit invocation).
- VisionClaw: ADD verification in `src/services/nostr_bridge.rs::run_once` event
  loop; current `:188-195` only verifies the direct signature.

### F9 — Bridge re-signing replacement
Today `src/services/nostr_bridge.rs:219-247` re-signs forwarded JSS kind-30001 events
under the bridge's own pubkey, losing original-author attribution. Replacement:
- If original event already has `["delegation", δ_origin→bridge, ...]`, FORWARD the
  *original signed event* unchanged (not a re-sign); recipient verifies signer chain
  via δ.
- If no delegation present, EITHER refuse to forward (fail-closed default) OR mark
  the forwarded event with `["forwarded-from", original_pubkey, original_id]` and
  document that downstream sees bridge-signed-events with origin attribution metadata
  (fail-open; configurable via `NOSTR_BRIDGE_FORWARD_POLICY=delegate-required|fwd-meta`).

### F10 — `urn:visionclaw:context` references in envelopes
The IS-Envelope's `subj` field MUST accept any canonical URN form from
`urn:visionclaw:*` (validated via `src/uri/parse.rs`) or `urn:agentbox:*`
(validated via `agentbox/management-api/lib/uris.js::isCanonical`). The
BC20 anti-corruption layer (PRD-006 §5.5) is responsible for translation; this PRD
just specifies the envelope carries the URN, not how the receiver resolves it.

### F11 — Federated kind allowlist per relay
Each relay's manifest MUST define `mesh.federated_kinds` (default
`[14, 1059, 30033, 30910..30916]`) and `mesh.federated_pubkeys` (subset of
`allowed_pubkeys` for which fan-out runs). Events outside both lists are not
republished. This bounds blast radius and gives operators a kill switch.

### F12 — Mesh deployment switches
Each substrate's manifest MUST expose the same `[mesh]` block. **Per-substrate canonical schema location**:
- VisionClaw: `Settings.toml` (existing actix-web config) — top-level `[mesh]` table
- agentbox: `agentbox.toml` — `[sovereign_mesh.mesh]` sub-table (consistent with `[sovereign_mesh.relay]` precedent at `agentbox.toml:87-104`)
- Forum kit + dreamlab-ai-website consumer: `<deployment>.toml` — top-level `[mesh]` table (forum kit `wrangler.toml` reads `[vars.mesh_*]` env vars derived from this)

```toml
[mesh]
mode               = "standalone" | "federated" | "client"   # default standalone
peer_relays        = []                                       # CSV-or-list of wss URLs
federated_kinds    = [14, 1059, 30033, 30910, 30911, 30912, 30913, 30914, 30915, 30916]
federated_pubkeys  = []                                       # opt-in: which local actors federate; empty = all
honor_remote_moderation = []                                  # list of trust-root DIDs whose ban/mute are honoured
allowed_remote_dids = []                                      # incoming peers whose events are accepted
delegation_required = true                                    # require NIP-26 on cross-system attribution
```

### F13 — Linked-Data S2/S4 surface coherence
- Agentbox's S4 (DID Documents) ALREADY emits the correct shape post-F4 patches.
  Confirm `agentbox.toml [linked_data.did]` includes `service_endpoints = ["pod", "relay", "mesh"]`.
- Agentbox's S2 (events) JSON-LD shape MUST be reused for IS-Envelope serialisation
  when `Content-Type: application/ld+json` is requested at LDN bridge boundaries.

### F14 — DID-via-relay resolution path
solid-pod-rs MUST gain (in 0.5.x) a `resolve_via_relay(pubkey, relay_url) ->
Result<Option<DidDocument>>` method on `NostrWebIdResolver`. Implementation:
- Connect to relay, AUTH, query `Filter { authors: [hex], kinds: [0, 30033], limit: 2 }`.
- Parse `kind 0` `content.alsoKnownAs` if present; parse `kind 30033` (mesh service-list)
  per ADR-075.
- Return assembled DID Document.

### F15 — VisionClaw resolver routes registered
Implement the four 307 targets currently dangling:
- `GET /api/v1/identity/{hex}/did.json` → DID Document for substrate-known pubkeys.
- `GET /api/v1/nodes/by-uri/{urn}/jsonld` → Neo4j lookup for OwnedKg / Concept.
- `GET /api/v1/wac/groups/{team}` → ACL group resolution.
- `GET /api/v1/uri/did:nostr:<hex>` (new): unified DID resolver per F14, falling
  back to local handler if hex matches `SERVER_NOSTR_PRIVKEY`-derived pubkey.

### F16 — RelayConsumer wired into management-api boot
Apply the 10-15 line patch identified in `docs/integration-research/03-agentbox-surfaces.md` §12 Gap B:
```js
// management-api/server.js, after connectOps settles
const RelayConsumer = require('../mcp/nostr-bridge/relay-consumer');
const consumer = new RelayConsumer({
  npubs: [process.env.AGENTBOX_NPUB],
  adapters: resolvedAdapters,
  relayUrl: 'ws://127.0.0.1:7777',
  ingressPolicy: 'allowlist',
  allowedPubkeys: manifest.mesh?.allowed_remote_dids ?? [],
});
await consumer.start();
app.addHook('onClose', async () => { await consumer.stop(); });
```

### F17 — Agentbox external relay reachability
Operators choosing `mesh.mode == "federated"` MUST also flip:
- `[sovereign_mesh.relay] bind = "0.0.0.0", expose = true` in `agentbox.toml`.
- Add `7777:7777` to `docker-compose.yml`.
- Add `7777/tcp = {}` to `commonPorts` or `sovereignPorts` in `flake.nix`.
- Configure the `https-bridge` (priority-32 supervisord block) to TLS-terminate for 7777.

Documentation in `agentbox/docs/user/mesh-deployment.md` (new).

### F18 — Operator pubkey auto-allowlisted on agentbox relay
Fix `flake.nix:732-736` to derive `pubkey_whitelist` from BOTH `relayCfg.allowed_pubkeys`
AND `sovereignCfg.operator.pubkey_hex` (when present). Spec drift between
`agentbox.toml:64-68` (claims operator-allowlisted) and the flake (only
`allowed_pubkeys` populated) is closed.

### F19 — Pod-inbox payloads as Linked Data Notifications
`relay-consumer.js:215-221` writes raw Nostr-event-wrapped JSON. Change to LDN-native:
write `application/ld+json` shape:
```jsonld
{
  "@context": "https://www.w3.org/ns/activitystreams",
  "type": "Announce",
  "actor": "did:nostr:<from_hex>",
  "target": "did:nostr:<to_hex>",
  "object": { "@type": "Note", "content": "<envelope.body>", "id": "urn:nostr:event:<id>" },
  "x:nostrEvent": <original signed event>,
  "x:envelope": <IS-Envelope rumor>
}
```
The `x:nostrEvent` and `x:envelope` extensions preserve full provenance for verifier
re-runs; the AS2 outer shape lets vanilla LDN consumers process the message.

### F20 — Replay store federation across forum workers
Sprint v9 STREAM-B added per-worker `KvReplayStore` with separate KV namespaces. F20:
single shared `NIP98_REPLAY` KV namespace used by auth-worker, pod-worker, relay-worker,
search-worker. Cost: one extra namespace; benefit: closes cross-worker token replay
under URL-rewriting proxies.

### F21 — Cross-system replay via canonical event id
For mesh fan-out, the dedup primitive MUST be the canonical Nostr event id (32-byte
hex). Each relay's outbound worker keeps a local LRU `seen_ids` cache (capacity 4096,
TTL 600s) and refuses to republish ids it has already emitted. This prevents fan-out
storms.

### F22 — Bead-relay coupling via subscription, not bridge
Replace the JSS-relay tungstenite subscription in `src/services/nostr_bridge.rs` with
a generic `MeshBridge` that subscribes to:
- VisionClaw operator's mesh peers (ALL of them, not just JSS).
- Filters: `kinds = mesh.subscribed_kinds`, `since = on_boot()`.
- For each kind 30001 (legacy bead) OR new kind-30050 (IS-Envelope mesh-event), runs
  the appropriate handler.

The legacy `JSS_RELAY_URL` stays supported via `mesh.peer_relays` containing it; no
flag-day required.

### F23 — Anti-drift CI gates
Activate the previously-documented but inactive lints:
- VisionClaw `src/uri/`: clippy lint `urn-visionclaw-format` rejects `format!("urn:visionclaw:..")`
  outside `src/uri/`. Catches `block_level_parser.rs:209` regression today.
- Agentbox `lib/uris.js`: ESLint custom rule `no-ad-hoc-urn` rejects raw template
  literals matching `^urn:agentbox:` outside `lib/uris.js`.
- Cross-repo: GitHub Action that, for each repo, asserts the published DID Document
  uses `SchnorrSecp256k1VerificationKey2019`.

### F24 — Substrate-emitted bead republishing
VisionClaw substrate-emitted kinds (30023/30100/30200/30300/30301/31400/31402) MUST be re-publishable
via the mesh per F11 if `mesh.federated_pubkeys` includes the substrate's hex.
`server_nostr_actor.rs` event signing path adds `["mesh", "v1"]` tag so receivers know
this is a cross-mesh substrate event.

### F25 — `nostr-core` upstream absorption (per ADR-076)
Forum's `nostr-core` migrates from hand-rolled NIP implementations to a thin shim
over the upstream `nostr` crate. Modules deleted (replaced by upstream): `event.rs`,
`keys.rs` (signing primitives only), `nip04.rs`, `nip19.rs`, `nip26.rs`, `nip44.rs`,
`nip90.rs`, `nip98.rs` (verifier internals only), `gift_wrap.rs`, `groups.rs`,
`calendar.rs`, `deletion.rs`. Modules kept (project-specific): `moderation_events.rs`,
`signer.rs` (refactored to delegate), `wasm_bridge.rs`, `derive_from_prf` from
`keys.rs`, the `Nip98ReplayStore` trait + `KvReplayStore` impl. Net reduction: ~85%
of crypto-protocol code (~6,500 LOC deleted, ~700 retained). Fixes C1 by deletion.

### F26 — WASM/CF Workers compatibility validation (gating spike)
Before any module deletion lands, a 3-5 day validation spike at
`crates/nostr-upstream-canary/` proves: all 5 CF Workers compile against the new
dep set; bundle size delta within +200 KiB per worker (CF free tier 1 MiB ceiling);
cold-start latency delta within +50 ms; forum-client WASM bundle delta within
+500 KiB; all paulmillr/nip44 reference vectors pass. If gate fails, PRD-010
falls back to Shape C (patch-in-place) and ADR-076 status moves to Rejected.

### F27 — Reference test vectors as regression guards
Forum CI gains `tests/vectors/{nip04,nip19,nip26,nip44-v2,nip59}.json` sourced from
upstream `nostr` crate's test suite + paulmillr/nip44 + canonical NIP repos. Per-PR
CI runs `cargo test -p nostr-core --test upstream_vectors` and blocks merge on
failure. This is the durable defence against C1-class bugs returning.

### F28 — Per-PR behaviour-preserving migration
Migration is module-by-module (per ADR-076 D6 ordering: `nip04` → `nip19` → `nip44`
→ `gift_wrap` → `event` → `keys` → `nip26`/`nip90`/`calendar`/`deletion`/`groups`
→ `nip98` → `signer` → `types` → cleanup). Each PR must pass: existing unit tests +
proptest (incl. `tests/{nip19,nip04}_proptests.rs`); new upstream-vector tests;
`wasm32-unknown-unknown` build for `forum-client` and all 5 CF Workers; integration
tests on `forum-client` and `relay-worker` show no behaviour delta.

### F29 — Cargo workspace alignment
After forum migration completes, VisionClaw's `Cargo.toml` workspace dependencies
add `nostr = "0.44"` (matching forum's pin) so future code can pass `nostr::Event`
types directly across the BC20 boundary without translation. VisionClaw's existing
`nostr_sdk = "0.44"` dep is unchanged (SDK depends transitively on `nostr`).

### F30 — Public surface stability across migration
`nostr-core`'s public API surface MUST remain backward-compatible across the
migration. Internal modules disappear; public types either re-export upstream
(e.g. `pub use nostr::Event as NostrEvent`) or wrap them with the existing
forum-side type signature. Consumers in `forum-client/`, `auth-worker/`,
`pod-worker/`, `relay-worker/`, `search-worker/`, `admin-cli/` must not require
edits beyond import path adjustments. This bounds the migration's blast radius.

---

## 7. Sequencing & Phases

### Phase 0 — Crypto correctness + nostr-core absorption (gating, ~2 sprints)

P0 is non-negotiable: until these land, nothing else interoperates. Phase 0 grew
from 1 sprint to ~2 because ADR-076 rolls Shape A (full upstream absorption) into
the gating phase, replacing in-place patches for C1/L20/M14 with module deletion.

- **F26 — WASM/CF Workers compat spike (FIRST, gates all subsequent absorption work).**
  3-5 days. Outcome dictates whether ADR-076 proceeds or PRD-010 falls back to
  Shape C (patch-in-place).
- F4 — `verificationMethod.type` standardised on `SchnorrSecp256k1VerificationKey2019`
  (3 patch sites — independent of absorption; ADR-076 doesn't change DID Document
  emitter shape).
- F5 — agentbox NIP-19 npub fix (1 patch site + migration; agentbox-side, unaffected
  by forum-side absorption).
- F25 — Forum `nostr-core` migration per ADR-076 D6 ordering. Per-module PRs:
  `nip04` → `nip19` → `nip44` (closes C1) → `gift_wrap` (closes inherited C1) →
  `event` → `keys` → `nip26` (used by F8) → `nip90` → `calendar` → `deletion` →
  `groups` → `nip98` → `signer` → `types` → cleanup.
- F27 — Reference test vectors landed alongside the modules they validate.
- F28 — Per-PR CI gating in place from PR 1 of the migration.
- F29 — VisionClaw workspace `nostr` pin aligned at end of phase.
- F30 — Public surface stability verified at each PR.
- §15.H11 — verify JS HKDF info string matches Rust shim's `derive_from_prf`.

If the F26 spike fails, Phase 0 reverts to the original Shape C plan: hand-patch
F6 (NIP-44 conv key in `nip44.rs:122-128`) + L20 (paulmillr vectors) + M14 (bridge
re-signing doc). Phase 0 then collapses back to ~1 sprint and ADR-076 moves to
Rejected status.

### Phase 1 — DID Document & resolver convergence (~1 sprint)

- F1 — VisionClaw identity unification (with deprecation window).
- F2 — DID Document handlers wired in all three substrates.
- F3 — Tier-3 service entries including `#mesh`.
- F15 — VisionClaw resolver routes registered (close 307→404 gap).

### Phase 2 — AUTH + delegation wiring (~1 sprint)

- F7 — NIP-42 AUTH-RESP in forum-client; symmetric handling in agentbox + VC.
- F8 — NIP-26 delegation verifier wiring in all three event-ingest paths.
- F9 — Bridge re-signing replaced with delegation-aware forward.
- F18 — Operator pubkey auto-allowlist on agentbox relay.

### Phase 3 — Bridge wiring and mesh fan-out (~1.5 sprints)

- F16 — `RelayConsumer` wired into agentbox boot.
- F17 — Operator-facing mesh deployment options for agentbox.
- F19 — LDN payloads on pod-inbox writes.
- F22 — VisionClaw `MeshBridge` replacing JSS-only subscription.
- F11 + F12 — Manifest mesh blocks + per-relay federation logic.

### Phase 4 — Envelope contract + cross-system flows (~1 sprint)

- F10 — IS-Envelope reference implementation in nostr-core.
- F14 — DID-via-relay resolver.
- F20, F21 — Replay-store federation + canonical event id dedup.
- ADR-075 conformance tests per substrate.

### Phase 5 — Consolidation (~0.5 sprint)

- F13 — Linked-Data S2/S4 surface coherence.
- F23 — Anti-drift CI gates activated.
- F24 — Substrate-emitted bead republishing.
- Operator runbook for cross-system DM smoke tests.

Total: ~6 sprints (~12 weeks) at 1 engineer FTE; ~4 sprints with 2 engineers parallel
(Phase 0 must precede everything; Phases 1-3 can interleave; Phase 4-5 sequential).
The +1 sprint vs original PRD-010 estimate is ADR-076's absorption work — and is
expected to pay back ≥3x in Sprint v12+ avoided NIP hand-port effort.

---

## 8. Cross-Cutting: Cryptographic Hardening

This PRD inherits the severity-ranked list from
`docs/integration-research/05-crypto-gotchas.md`. CRITICAL items become Phase 0
gating; HIGH items become Phase 1-2 work; MEDIUM/LOW are Phase 5 hygiene.

| ID | Severity | Description | Phase |
|----|----------|-------------|-------|
| C1 | CRITICAL | NIP-44 v2 conv-key derivation incorrect | P0 (F6) |
| C2 | CRITICAL | agentbox bech32-encodes 64-byte pubkey | P0 (F5) |
| C3 | CRITICAL | `verificationMethod.type` three-way drift | P0 (F4) |
| H4 | HIGH     | solid-pod-rs-nostr Tier-1 missing secp256k1-2019 @context | P0 (F4) |
| H5 | HIGH     | NIP-26 delegation verifier wiring | P2 (F8) |
| H6 | HIGH     | Federate NIP-98 replay store across workers | P3 (F20) |
| H7 | HIGH     | Lowercase pubkey in `acl:agent` IRI construction | P0 |
| H8 | HIGH     | WAC agent-IRI normalisation (trim/lowercase/trailing-slash) | P1 |
| H9 | HIGH     | `verify_webid_tag` accept federated WebIDs via DID Doc round-trip | P1 |
| H10 | HIGH    | Key-rotation announcement protocol | P5 |
| H11 | HIGH    | CI assertion that JS+Rust HKDF info strings match | P0 |
| M12 | MED     | All-write AUTH gating on federated relay | P2 (F7) |
| M13 | MED     | NIP-26 delegation in NIP-07 flow | P2 |
| M14 | MED     | Bridge re-signing trade-off documented | P2 (F9) |
| M15 | MED     | Constant-time payload-hash comparison in NIP-98 | P5 |
| M16 | MED     | Replay store TTL audit | P3 (F20) |
| M17 | MED     | NIP-04 fall-through fail-explicit instead of route-via-NIP-44 | P1 |
| L18 | LOW     | `save_privkey_session` panic in release builds | P5 |
| L19 | LOW     | Lint-ban direct `SecretKey::sign` outside `event::sign_event` | P5 |
| L20 | LOW     | NIP-44 v2 reference test vector regression guard | P0 (F6) |
| L21 | LOW     | Lint-gate `format!("urn:visionclaw:..")` outside `src/uri/` | P5 (F23) |
| L22 | LOW     | Document NIP-26 5-element-tag extension | P5 |

---

## 9. Risks & Mitigations

### R1 — Migration breaks existing forum users (HIGH)
Forum users today have keys derived via the existing PRF info string. F4-F6 patches
do not change the info string, but F5 changes agentbox npub format. Existing pod
filesystem paths under `pods/<old_broken_npub>/` will not match new x-only-derived
npubs.

*Mitigation*: F5 ships a one-shot migration that, for each agent identity file,
re-derives the correct npub from `private_key_hex` and renames the pod directory.
Validated by smoke test that round-trips a kind-1059 to a pre-existing agent pod.

### R2 — NIP-44 fix breaks existing forum DM history (MEDIUM)
Forum DMs today are encrypted with the wrong conversation key. After F6, the same
keys + same plaintext produce different ciphertext. **Existing kind-1059 events in
the relay's D1 store remain decryptable** by the old code path, but new events
encrypted post-fix are decryptable only by post-fix clients.

*Mitigation*: F6 ships a feature flag `nostr_core::nip44::USE_LEGACY_HKDF` defaulting
to `true` for one release window; clients try new derivation first, fall back to
legacy on MAC failure. Forum admin announces date of legacy removal. Old DMs become
read-only history.

### R3 — Mesh fan-out storms (MEDIUM)
Naïve fan-out of every event to every peer relay scales as O(events × peers²).

*Mitigation*: F11 (`mesh.federated_kinds` allowlist) + F21 (canonical-event-id
dedup) bound this. Default federated_kinds only includes DM + moderation (~few-per-second
in steady state, much less in practice).

### R4 — Public CF Worker URL becomes inadequate firewall (HIGH)
The forum CF relay is a publicly-reachable URL; F12 + F17 expose agentbox + VisionClaw
relays similarly. Combined with NIP-42 AUTH but absent IP allowlists, this is a
deliberate trade-off.

*Mitigation*: NIP-42 `pubkey_whitelist` is the firewall. Per-relay rate limits
(`agentbox.toml:[sovereign_mesh.relay] messages_per_sec = 5`, forum's 10/IP/sec)
bound DoS. Operator runbook documents the trade-off.

### R5 — solid-pod-rs Storage trait redesign delays adoption (MEDIUM)
Forum's pod-worker reimplements 2,300+ LOC because solid-pod-rs's `Storage` is
Tokio-only. This PRD does NOT solve that; it assumes forum continues to maintain
its WASM-Workers pod stack. See ADR-028 amendment for the medium-term fix
(solid-pod-rs 0.5 with `KvBackend` trait + `MaybeSend` futures). PRD-010 succeeds
without it.

### R6 — Cryptographic invariants drift again (MEDIUM)
F23 anti-drift CI gates plus F4-CI-assertion plus crypto reference vectors guard
against regression. Without these, every refactor risks reintroducing C1-C3.

*Mitigation*: Phase 0 includes the regression vectors; Phase 5 wires the lints.
ADR-076 absorption strengthens this by **eliminating** the source of C1-class
bugs — once the protocol layer is upstream, future regressions can only land in
the project-specific shim, which is small enough to audit by hand.

### R7 — Upstream `nostr` crate fails on CF Workers (HIGH if surfaces, low probability)
The F26 validation spike exists to surface this risk before code-deleting commits
land. Failure modes: `getrandom` feature flag mismatch breaks WASM build; bundle
size exceeds CF Worker 1 MiB ceiling; `wasm-bindgen` interop with rust-nostr's
type signatures requires shims; cold-start latency regression beyond +50 ms.

*Mitigation*: spike ships first (F26 gates the rest of Phase 0); Cargo features
selectively enabled to minimise bundle size; spike worker deployed to CF before
any per-module migration PR; explicit bundle-size and latency budgets enforced.
If spike fails: PRD-010 P0 reverts to Shape C in a single doc revision; ADR-076
moves to Rejected; the 3-5 day spike cost is the only sunk loss.

---

## 10. Open Questions

### Q1 — Mesh routing protocol: pull vs. push?
ADR-075's IS-Envelope assumes push fan-out (relay re-publishes to peers when event
arrives). Alternative: each relay exposes an outbox endpoint, peers pull on
schedule. Push is lower-latency; pull is more resilient to peer outages. Decision
deferred to ADR-073.

### Q2 — Per-relay vs. mesh-wide moderation?
F12's `honor_remote_moderation` defaults empty (each relay enforces only its own
admin's mod actions). Should there be a default-trust trust-root list? Probably
no — encourages cargo-cult trust. Operators must explicitly opt in.

### Q3 — How is the `#mesh` service URL list refreshed?
DID Document is fetched once and cached; if an actor changes their mesh peer set,
how do existing mesh participants learn? Options:
- (a) TTL on cached DID Document (default 600s).
- (b) Subscribe to kind 30033 (mesh service-list, replaceable) for known peers.
- (c) Hybrid: kind 30033 push + TTL cache fallback.

Recommendation: (c). Decision deferred to ADR-074.

### Q4 — Multi-agent identity per agentbox container?
Today `sovereign-bootstrap.py` mints exactly one keypair per container. If a forum
DM is addressed to `did:nostr:<U>` where U is one of multiple agents in container C,
how does the bridge route it? `RelayConsumer` already takes `npubs: [...]` (line 156)
suggesting multi-tenancy was anticipated. Spec drift between the array shape and the
single-key-per-container reality.

Recommendation: explicit multi-agent support in agentbox 0.6.x. Out of scope for
PRD-010.

### Q5 — Relay discovery for previously-unknown actors?
A forum user knows `did:nostr:<X>` from a kind-1 event but has never seen X before.
Step 1 of §5.4 says "query any relay in `mesh.peer_relays`" — but which? Try all in
parallel? Trust-weighted order?

Recommendation: parallel race, accept first valid response; cache result. Decision
deferred to ADR-074.

### Q6 — Solid-pod-rs upgrade timing?
Most of this PRD's solid-pod-rs requirements (NIP-98 replay-store trait, DID-via-relay,
NIP-04/17/44/59 modules) need solid-pod-rs 0.5.x. Forum and agentbox can ship without
the upgrade by maintaining their own copies; VisionClaw needs a clear migration path.

Recommendation: solid-pod-rs 0.5.0-alpha.3 publish + workspace bump in Phase 4. Track
in ADR-028 amendment, not in this PRD.

### Q7 — Should the BC20 anti-corruption layer ship in this PRD?
PRD-006 §5.5 specifies six BC20 modules + two aggregates. Today: zero LOC. Mesh
federation needs the BC20 to translate `urn:visionclaw:bead:*` ↔ `urn:agentbox:bead:*`
on substrate-substrate links. Without BC20, IS-Envelope's `subj` field carries opaque
URNs that receivers cannot resolve.

Recommendation: BC20 P3 (PRD-006) and PRD-010 P3 are companion sprints. Do PRD-010
P0-P2 first (identity + AUTH); then BC20 + PRD-010 P3-P5 in parallel.

### Q8 — Should we publish a `.well-known/mesh.json` for human discovery?
A simple JSON document at each substrate's well-known URL listing supported NIPs,
mesh peers, contact info. Improves operability but bleeds private metadata to
unauthenticated callers.

Recommendation: yes for `livez`-style minimal info (NIPs supported, federated_kinds);
no for peer URLs or pubkeys. Field set TBD in ADR-073.

---

## 11. Success Metrics

### M1 — End-to-end DM round-trip (gating)
A forum user can DM an agentbox agent and receive a reply within 5 seconds. Test:
`tests/mesh_e2e.rs` boots a triple-stack scenario, performs the round-trip, asserts
liveness. Phase 4 deliverable.

### M2 — Cross-system identity resolution coverage
**Test population**: ≥30 known pubkeys per substrate (drawn from production rosters: forum admins, forum cohort members, agentbox sovereign agents, VisionClaw substrate operators) PLUS 10 synthetic test pubkeys (deterministic cold-start scenarios — no prior DID Document cache, no relay history). Total ≥120 pubkeys across ecosystem.

For each pubkey, the other two substrates MUST resolve `did:nostr:<hex>` to a Tier-3 DID Document via either `#mesh`-relay query (ADR-074 D5 step 1) or `.well-known` HTTP (step 2) or pod fallback (step 3).

**Success criterion**: ≥95% of resolutions succeed within 2s p95 latency. Phase 1 deliverable; verified by integration test `tests/mesh_e2e/did_resolution_coverage.rs`.

### M3 — NIP-26 delegation verification rate
Of all cross-substrate events emitted with `["delegation", ...]` tags, 100%
verify successfully on the receiver and attribute to the delegator. Phase 2
deliverable.

### M4 — Anti-drift CI gates passing
F23 lints active in all three repos; zero open warnings; CI gating deploys with
`SchnorrSecp256k1VerificationKey2019` assertion. Phase 0+5 deliverable.

### M5 — Federation overhead bounds
Mesh fan-out adds ≤30ms median latency per peer relay; mesh-replicated event volume
≤5% of original event volume (i.e. fan-out is highly selective, federated_kinds
narrow). Phase 4 deliverable.

### M6 — Reduced reimplementation burden
Forum's pod-worker LOC count drops by ≥1,500 LOC after solid-pod-rs 0.5 absorption
(Q6 follow-up). Out-of-scope for PRD-010 success metric but tracked.

---

## 12. Affected Files (top-level only; specifics in ADRs)

### VisionClaw (`/home/devuser/workspace/project/`)

NEW:
- `src/handlers/identity_did_handler.rs` — F2 DID Document publication
- `src/services/mesh_bridge.rs` — F22 generalised relay bridge
- `src/services/unified_server_identity.rs` — F1 identity unification
- `src/bc20/` (six modules from PRD-006 §5.5) — companion sprint
- `tests/mesh_e2e.rs` — F-tests
- `tests/nip26_delegation.rs` — F8 verifier coverage

MODIFIED:
- `src/services/server_identity.rs` — F1 unification, supersession
- `src/services/nostr_bridge.rs` — F9 delegation-aware forwarding, F22 generalisation
- `src/services/nostr_bead_publisher.rs` — F1 unification
- `src/services/pod_client.rs` — F1 unification, comment cleanup
- `src/handlers/uri_resolver_handler.rs` — F15 wire registered handlers
- `src/handlers/nostr_handler.rs` — F8 delegation verification on /api/auth/nostr
- `src/handlers/agent_events_ws_handler.rs` — F22 BC20 wiring (companion)
- `src/main.rs` — boot wiring for new actors, F22 mesh bridge spawn
- `src/services/parsers/block_level_parser.rs:209` — F23 fix antipattern
- `Cargo.toml` — F29 add `nostr = "0.44"` workspace dep (matching forum pin); solid-pod-rs 0.5.x bump (Q6 follow-up)

### Forum (`./dreamlab-ai-website/community-forum-rs/`)

NEW:
- `crates/forum-client/src/relay/auth_responder.rs` — F7 NIP-42 AUTH-RESP
- `crates/nostr-upstream-canary/` (TEMPORARY, F26) — WASM compat spike, deleted post-validation
- `crates/nostr-core/src/kinds.rs` — F25 project-specific kind catalogue
- `crates/nostr-core/src/mesh.rs` — F25 mesh kinds (30033/30050) wrapper
- `crates/nostr-core/tests/upstream_vectors/` — F27 paulmillr + NIP reference vectors

DELETED (per ADR-076 D1, F25):
- `crates/nostr-core/src/{event,nip04,nip19,nip26,nip44,nip90,calendar,deletion,groups,gift_wrap}.rs`
- `crates/nostr-core/src/keys.rs` keypair plumbing (only `derive_from_prf` retained, ~30 LOC)
- `crates/nostr-core/src/nip98.rs` verifier internals (only `Nip98ReplayStore` trait + `KvReplayStore` retained)
- Direct deps `chacha20poly1305`/`hmac`/`aes`/`cbc`/`bech32`/`k256` from `crates/nostr-core/Cargo.toml`

MODIFIED:
- `crates/nostr-core/Cargo.toml` — F25 features `nostr = { version = "0.44", default-features = false, features = ["nip04","nip17","nip19","nip26","nip29","nip44","nip52","nip56","nip59","nip65","nip90","nip98","std"] }`
- `crates/nostr-core/src/lib.rs` — F30 public re-exports of upstream types
- `crates/nostr-core/src/signer.rs` — F25 refactor to delegate `sign_event` to `nostr::Keys::sign_event`
- `crates/nostr-core/src/moderation_events.rs` — F25 builders use `nostr::EventBuilder`
- `crates/nostr-core/src/wasm_bridge.rs` — F25 surface upstream types
- `crates/forum-client/src/relay.rs:439-524` — F7 add AUTH case
- `crates/forum-client/src/dm/mod.rs:294` — F7 + extension support via Signer trait
- `crates/forum-client/src/auth/nip07.rs:188-208` — M17 fail-explicit on NIP-04
- `crates/forum-client/src/auth/session.rs:96-109` — L18 panic in release
- `crates/pod-worker/src/lib.rs:213` — F3 advertise relay URL when known
- `crates/pod-worker/src/lib.rs:447` — H7 lowercase pubkey in agent IRI
- `crates/pod-worker/src/did.rs:55-58` — H9 federated WebID tolerance
- `crates/pod-worker/src/acl.rs:166` — H8 agent-IRI normalisation
- `crates/relay-worker/src/relay_do/nip_handlers.rs::handle_event` — F8 delegation
- `crates/auth-worker/wrangler.toml` — F20 shared `NIP98_REPLAY` namespace
- `crates/{auth,pod,relay,search}-worker/src/auth.rs` — F20 shared store
- `wrangler.toml` (5×) — F11/F12 mesh manifest (`[vars.mesh_*]`)
- `dreamlab-ai-website/CLAUDE.md` Tech Stack table — note nostr-core is shim over upstream
- All `forum-client/`, `*-worker/`, `admin-cli/` consumers — F30 import path adjustments only

Note: F25 absorbs and supersedes the originally-listed in-place patch at
`crates/nostr-core/src/nip44.rs:122-128` (now deleted). The C1 bug is fixed by
the deletion, not by an edit.

### Agentbox (`./agentbox/`)

NEW:
- `mcp/nostr-bridge/relay-consumer-boot.js` — F16 wire-up shim (or inline in server.js)
- `management-api/routes/did-document.js` — F2 served at /.well-known/did.json
- `management-api/adapters/events/local-nostr.js` — F22 events slot Nostr adapter
- `docs/user/mesh-deployment.md` — F17 operator runbook

MODIFIED:
- `scripts/sovereign-bootstrap.py:90-91, 133-134, 192` — F4 + F5
- `flake.nix:732-736` — F18 operator pubkey auto-allowlist
- `flake.nix:1968-1970` (`sovereignPorts`) — F17 add 7777 when expose=true
- `agentbox.toml` — F11/F12 `[mesh]` block; `[sovereign_mesh.relay] expose` doc
- `docker-compose.yml:19-26` — F17 conditional 7777 publication
- `mcp/nostr-bridge/relay-consumer.js:215-221` — F19 LDN payload shape
- `mcp/nostr-bridge/relay-consumer.js::_processEvent` — F8 delegation
- `management-api/server.js:686-862` — F16 RelayConsumer boot wiring
- `management-api/middleware/linked-data/surfaces/s04-did.js` — F4 verificationMethod.type
- `agentbox/lib/solid-pod-rs.nix` — solid-pod-rs 0.5.x bump (Q6)

### solid-pod-rs (`./solid-pod-rs/`)

NEW (0.5.x):
- `crates/solid-pod-rs/src/auth/nip98_replay_store.rs` — replay-store trait
- `crates/solid-pod-rs-nostr/src/messaging/{nip04,nip17,nip44,gift_wrap}.rs` — DM stack
- `crates/solid-pod-rs-nostr/src/resolver_relay.rs` — F14 DID-via-relay
- `crates/solid-pod-rs-nostr/tests/nip44_vectors.rs` — reference test vectors

MODIFIED (0.5.x):
- `crates/solid-pod-rs-nostr/src/did.rs:98, 154` — F4 type
- `crates/solid-pod-rs-nostr/src/did.rs:93` — H4 @context
- `crates/solid-pod-rs-nostr/src/relay.rs:330` — advertise expanded NIP set
- `crates/solid-pod-rs/src/auth/nip98.rs` — wire replay store
- `Cargo.toml` — `s3-backend` impl finally landed (out of scope)

---

## 13. References

- `docs/integration-research/01-visionclaw-surfaces.md` — VisionClaw evidence
- `docs/integration-research/02-forum-surfaces.md` — Forum evidence
- `docs/integration-research/03-agentbox-surfaces.md` — Agentbox evidence
- `docs/integration-research/04-solid-pod-rs-surfaces.md` — solid-pod-rs evidence
- `docs/integration-research/05-crypto-gotchas.md` — Cryptographic alignment audit
- `docs/integration-research/06-uri-dataflow-alignment.md` — URI/data-flow audit
- `docs/PRD-004-agentbox-visionclaw-integration.md` — predecessor
- `docs/PRD-006-visionclaw-agentbox-uri-federation.md` — predecessor (URI ns)
- `docs/ddd-agentbox-integration-context.md` — predecessor (BC20 design)
- `docs/adr/ADR-053-solid-pod-rs-crate-extraction.md` — solid-pod-rs adoption
- `docs/adr/ADR-058-mad-to-agentbox-migration.md` — agentbox transition
- `agentbox/docs/reference/adr/ADR-009-embedded-nostr-relay.md` — relay decision
- `agentbox/docs/reference/adr/ADR-010-rust-solid-pod-adoption.md` — pod adoption
- `agentbox/docs/reference/adr/ADR-013-canonical-uri-grammar.md` — URN grammar
- ADR-073 — Private Nostr Relay Topology & NIP-42 AUTH (companion)
- ADR-074 — Cross-System DID:Nostr Canonicalisation & Trust Pivot (companion)
- ADR-075 — Inter-System Message Envelope (IS-Envelope v1) (companion)
- ADR-076 — Absorb forum `nostr-core` into upstream `nostr` crate (companion)
- `docs/ddd-mesh-federation-context.md` — bounded-context map (companion)

---

## Appendix A — Glossary

- **IS-Envelope** — Inter-System Envelope v1, the cross-system message contract
  defined in ADR-075.
- **Mesh peer** — a relay URL listed in another substrate's `[mesh] peer_relays`.
- **DID-via-relay** — DID Document resolution over a Nostr `kind:0` / `kind:30033`
  query rather than HTTPS `.well-known`.
- **Trust pivot** — the NIP-26 delegation that lets one identity (forum user)
  authorise another (agentbox agent) to act on its behalf.
- **Bridge identity** — synthetic key used to re-sign forwarded events; replaced by
  delegation-aware forwarding in F9.
- **Federation mode** — `mesh.mode` in {standalone, federated, client}; default
  standalone preserves current behaviour.
- **Walled garden over Nostr** — the forum's deployment shape: publicly reachable
  protocol, privately admitted membership.

## Appendix B — Phase 0 work summary (gating crypto + absorption)

**B.1 — In-place crypto patches (independent of absorption)**:

```
sovereign-bootstrap.py:90-91        [F5, C2] x-only pubkey for npub
sovereign-bootstrap.py:133-134      [F5, C2] same on persistence path
sovereign-bootstrap.py:192          [F4, C3] verificationMethod.type → 2019
solid-pod-rs-nostr/src/did.rs:98    [F4, C3] same
solid-pod-rs-nostr/src/did.rs:154   [F4, C3] same
solid-pod-rs-nostr/src/did.rs:93    [F4, H4] add secp256k1-2019 to Tier-1 @context
VisionClaw new identity emitter     [F4, M1.4] DID Document type-string from inception
agentbox CI assertion               [F4, M1.4] DID Document type-string check
forum CI assertion                  [F4, M1.4] same
visionclaw CI assertion             [F4, M1.4] same
HKDF info string match              [H11, M1.0] cross-language test (forum shim ↔ JS)
```

**B.2 — Forum nostr-core absorption (per ADR-076, F25-F30)**:

```
F26 spike (FIRST)            crates/nostr-upstream-canary/ — WASM compat validation
F25 PR 1                     crates/nostr-core/src/nip04.rs        DELETED (delegate)
F25 PR 2                     crates/nostr-core/src/nip19.rs        DELETED (delegate)
F25 PR 3                     crates/nostr-core/src/nip44.rs        DELETED (closes C1!)
F25 PR 4                     crates/nostr-core/src/gift_wrap.rs    DELETED (closes inherited C1)
F25 PR 5                     crates/nostr-core/src/event.rs        DELETED
F25 PR 6                     crates/nostr-core/src/keys.rs         REDUCED (~30 LOC, derive_from_prf only)
F25 PR 7                     crates/nostr-core/src/{nip26,nip90,calendar,deletion,groups}.rs  DELETED
F25 PR 8                     crates/nostr-core/src/nip98.rs        REDUCED (replay-store trait only)
F25 PR 9                     crates/nostr-core/src/signer.rs       REFACTORED (delegate)
F25 PR 10                    crates/nostr-core/src/types.rs        REDUCED (re-exports)
F25 PR 11 (cleanup)          drop chacha20poly1305/hmac/aes/cbc/bech32/k256 direct deps
F27 (per-PR)                 tests/upstream_vectors/{nip04,nip19,nip44,nip26,nip59}.json + runners
F29 (end of phase)           VisionClaw Cargo.toml — add nostr = "0.44" workspace dep
```

11 deletion/refactor PRs + 1 spike + ongoing CI vector tests. Net: ~6,500 LOC
deleted, ~700 retained (project-specific shim). Bug class C1 + L20 closed by
deletion.

**B.3 — Phase 0 totals**: ~12 in-place patch sites + 3 CI assertions + 1
cross-language test + 11 absorption PRs + 1 validation spike = **~2 sprints**
with one engineer; **~1 sprint** if absorption and in-place patches run parallel
across two engineers.

If F26 spike fails, B.2 collapses to a single in-place patch at
`nostr-core/src/nip44.rs:122-128` (Shape C); Phase 0 returns to ~1 sprint.
