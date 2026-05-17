# ADR-074 — Cross-System DID:Nostr Canonicalisation & NIP-26 Trust Pivot

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-010 G1, G5, G6 |
| Supersedes | ADR-027 (DID identity stack — extends) |
| Companion ADRs | ADR-073, ADR-075, ADR-076 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` |

## Context

`did:nostr:<hex>` is byte-identical across forum, agentbox, VisionClaw, and `solid-pod-rs` source code (`docs/integration-research/06-uri-dataflow-alignment.md` §1.4). All four agree on:
- 64 lowercase hex characters
- Identity = BIP-340 x-only Schnorr secp256k1 pubkey
- Bech32 npub is wire-only (NIP-19 boundary), hex is canonical

But each substrate currently emits **a different DID Document `verificationMethod.type`**, and the W3C `@context` is inconsistent (`docs/integration-research/05-crypto-gotchas.md` §2-§3):

| Emitter | type | secp256k1-2019 in @context? |
|---------|------|----------------------------|
| pod-worker | `SchnorrSecp256k1VerificationKey2019` | yes ✓ |
| solid-pod-rs-nostr | `NostrSchnorrKey2024` | only Tier-3, not Tier-1 |
| sovereign-bootstrap.py | `SchnorrSecp256k1VerificationKey2022` (non-existent suite) | yes |
| VisionClaw | (does not emit) | n/a |

Three actor classes hold three different keypairs:

- **Forum user** — passkey-PRF-derived in browser, never persisted as private bytes; pubkey hex-pinned to PRF output.
- **Agentbox sovereign agent** — secp256k1 keypair persisted to `/var/lib/agentbox/identities/<agent_id>.json`; one per container.
- **VisionClaw operator** — env-loaded `SERVER_NOSTR_PRIVKEY` (or `VISIONCLAW_NOSTR_PRIVKEY` — the duplication is a transitional split flagged at `pod_client.rs:10-13`).

These keys cannot and must not be merged: three independent custody regimes. So the mesh has many DIDs concurrently, and the question becomes — when forum-user `U` delegates authority to agentbox-agent `A`, how do peer substrates verify the trust chain?

NIP-26 (delegation) is implemented in `nostr-core/src/nip26.rs:64-95, 239-244, 282` (forum side) but **not wired into agentbox event ingest, not wired into VisionClaw `nostr_bridge.rs:188-195`** (which only verifies the direct signature). The nostr_bridge **re-signs** forwarded events under its own key (`:219-222`), losing original-author attribution.

PRD-010 specifies that:
- Identity is unconditional and shared (G1).
- Authority is delegated via NIP-26 with bounded scope (G5).
- A third party with only `did:nostr:<hex>` can route messages without out-of-band config (G6).

This ADR formalises the canonicalisation rules and the delegation grammar across the mesh.

## Decision

### D1 — Canonical hex pubkey form

Across all surfaces:
- **Wire identity**: 64-char lowercase hex (no `0x`, no checksum). Regex `^[0-9a-f]{64}$`.
- **DID URI**: `did:nostr:<64-hex>`. Regex `^did:nostr:[0-9a-f]{64}$`.
- **NIP-19 npub**: bech32 of the 32-byte BIP-340 x-only public key. ONLY at Nostr-relay-wire boundary (NIP-01 events) and in legacy Solid pod filesystem paths (kept by ADR-054 transition).
- **WAC ACL agent IRI**: `did:nostr:<64-hex>`, lowercase hex enforced before string compare.

Enforcement points:
- Forum: `nostr-core/src/nip98.rs:258` already accepts any-case hex; **must** reject non-lowercase. New regex `^[0-9a-f]{64}$` at line 258.
- Forum: `pod-worker/src/lib.rs:447` constructs `did:nostr:{pk}` with `pk` of unspecified case; **must** lowercase before construction (PRD-010 H7).
- Forum: `pod-worker/src/acl.rs:166` compares agent IRIs as strings; **must** normalise both sides (lowercase, trim, strip trailing slash) before compare (PRD-010 H8).
- Agentbox: `sovereign-bootstrap.py:90-91, 133-134` must produce x-only-derived hex (PRD-010 F5).
- VisionClaw: `src/uri/parse.rs:222-253` already correct; identity emitter (new) must use lowercase from inception.

### D2 — Canonical DID Document shape (Tier-3, all emitters)

```jsonld
{
  "@context": [
    "https://www.w3.org/ns/did/v1",
    "https://w3id.org/security/suites/secp256k1-2019/v1"
  ],
  "id":          "did:nostr:<64-lowercase-hex>",
  "alsoKnownAs": ["<webid_url>"],                 // when known
  "verificationMethod": [{
    "id":         "did:nostr:<hex>#key-0",
    "type":       "SchnorrSecp256k1VerificationKey2019",
    "controller": "did:nostr:<hex>",
    "publicKeyHex":       "<hex>",
    "publicKeyMultibase": "z<base58btc(0xe7 0x01 || pk_32)>"
  }],
  "authentication":  ["did:nostr:<hex>#key-0"],
  "assertionMethod": ["did:nostr:<hex>#key-0"],
  "service": [
    { "id": "...#solid-pod",   "type": "SolidStorage", "serviceEndpoint": "<pod_base>"   },
    { "id": "...#nostr-relay", "type": "NostrRelay",   "serviceEndpoint": "<wss_url>"    },
    { "id": "...#webid",       "type": "SolidWebID",   "serviceEndpoint": "<webid_url>"  },
    { "id": "...#mesh",        "type": "DIDNostrMesh", "serviceEndpoint": "<csv_relays>" }
  ]
}
```

Required fields: `@context`, `id`, `verificationMethod`, `authentication`. `assertionMethod` SHOULD be present (used by NIP-98 verifier path). `service` entries are optional; consumers that need a specific service MUST tolerate its absence and fall back per ADR-073 §D5 / PRD-010 §5.4.

### D3 — Multibase encoding

`publicKeyMultibase` = `z` + base58btc(`0xe7 0x01` || `pk_32_bytes`).
- `0xe7` = multicodec for `secp256k1-pub`.
- `0x01` = varint-1 (the codec byte is itself encoded as varint; for codec values < 0x80, varint is 1 byte).
- Per `pod-worker/src/did.rs:168 format_multibase_schnorr`, already correctly implemented forum-side.

### D4 — Tier-1 vs Tier-3

Tier-1 is the minimal DID Document — `@context`, `id`, `verificationMethod`. Used in low-bandwidth contexts (pure resolver responses with no service entries).

Tier-3 adds `alsoKnownAs`, `service`, `assertionMethod`. Used as the default for all production resolutions.

Both tiers MUST include both `did/v1` and `secp256k1-2019/v1` in `@context`. The current solid-pod-rs-nostr Tier-1 omits `secp256k1-2019/v1` (`docs/integration-research/05-crypto-gotchas.md` §3 row 2) — PRD-010 H4 fixes this.

### D5 — Resolution priority

A consumer wanting to resolve `did:nostr:<hex>` to a Tier-3 DID Document MUST try, in priority order:

1. **DID-via-relay** (cache miss case) — query any `mesh.peer_relays` for `Filter { authors: [hex], kinds: [0, 30033], limit: 2 }`. From kind-0 `content`, parse `alsoKnownAs` field if present. From kind-30033 (mesh service-list, see D9), parse the full service array. Assemble a Tier-3 DID Document.
2. **DID-via-`.well-known`** — HTTPS GET `<origin>/.well-known/did/nostr/<hex>.json`. The `<origin>` is discoverable from kind-0 `content.origin` if previously seen; otherwise the substrate's local pod base for own pubkeys.
3. **DID-via-pod** — if a known pod URL exists (e.g. from a prior session), GET `<pod>/.well-known/did.json` (per agentbox's pattern at `agentbox/management-api/server.js:206-210`).

Consumer caches the resolved document with TTL = `min(cache_max_age, kind30033_d_tag_ttl, 600s)`.

### D6 — Identity unification within a substrate

Each substrate has at most ONE operator/server identity. PRD-010 F1 mandates that VisionClaw's `SERVER_NOSTR_PRIVKEY` and `VISIONCLAW_NOSTR_PRIVKEY` resolve to the same key bytes; otherwise startup fails closed with `ErrIdentityKeysplit`.

Forum's three identity backends (passkey-PRF / NIP-07 / nsec) all yield the same primitive (32-byte secret) — already unified per `auth/mod.rs:511 sign_event_async`. No change needed.

Agentbox's per-container key is similarly unique (one identity file per container per `sovereign-bootstrap.py:233`). Multi-agent-per-container is a P5 follow-up (PRD-010 §10 Q4).

### D7 — Multi-DID per actor (the relational identity question)

The question raised by the URI/data-flow audit: how do you express "agent X owned by user U running in container C"?

**Answer: do not embed in URN**. Instead, express via:
- A **kind-30033 mesh service-list** event (see D9) signed by U, listing X as an agent that U has delegated to. The event's `["delegation", δ_U→X, ...]` tag carries the delegation proof.
- The DID Document for `did:nostr:<X>` carries `alsoKnownAs: [<webid_of_X>]` AND a custom `service` entry `{type: "DelegatorOf", serviceEndpoint: "did:nostr:<U>"}` advertising the delegation chain.

Rationale: URN body is a stable identifier; relationships are mutable. Encoding U into X's URN forces re-mint when delegation rotates.

PRD-006 §5.7 open question (composite agent URN) is hereby resolved as: **don't compose**, use delegation chain in DID Document `service` entries.

### D8 — NIP-26 delegation grammar

A delegation token is a `["delegation", delegator_pk_hex, conditions_str, sig_hex]` tag attached to any event signed by the delegatee. Conditions form per NIP-26:

```
"kind=N&kind=M&created_at>T1&created_at<T2"
```

Allowed predicates:
- `kind=N` — exact kind match (multiple `kind=` clauses are OR'd)
- `created_at>T` — strict greater than (timestamp)
- `created_at<T` — strict less than (timestamp)

Signature: `sig = Schnorr_sign_with(delegator_sk, sha256("nostr:delegation:" || delegatee_pk || ":" || conditions_str))`.

Receiver verification:
1. Find `["delegation", ...]` tag on event.
2. Recompute hash; verify Schnorr signature with delegator's public key.
3. Apply conditions: assert `event.kind ∈ allowed_kinds`, `event.created_at > T1` AND `< T2` if those clauses are present.
4. On success, attribute event authorship to **delegator** (display) but preserve delegatee on the wire.
5. On any failure, reject the event with `OK false "delegation-invalid: <reason>"`.

`nostr-core/src/nip26.rs:64-95` is the reference Rust implementation. PRD-010 F8 mandates wiring it into:
- `relay-worker/src/relay_do/nip_handlers.rs::handle_event` (post-signature, pre-storage gate).
- `agentbox/mcp/nostr-bridge/relay-consumer.js::_processEvent` (Node port via `nostr-tools.verifyDelegation`).
- `src/services/mesh_bridge.rs::handle_inbound` (VisionClaw, replacing `nostr_bridge.rs:188-195`).

### D9 — kind-30033: Mesh Service List

A new parameterised replaceable kind, tag `["d", "mesh-services"]`, carrying:

```json
{
  "kind": 30033,
  "pubkey": "<actor_hex>",
  "created_at": 1763000000,
  "tags": [
    ["d", "mesh-services"],
    ["service", "solid-pod",   "https://pod.example.com/<actor_hex>/"],
    ["service", "nostr-relay", "wss://relay.example.com/"],
    ["service", "mesh",        "wss://relay-a.example.com/,wss://relay-b.example.com/"],
    ["delegation", "<delegator_hex>", "kind=14&created_at<1763500000", "<sig>"]
  ],
  "content": "",
  "id":  "<event_id>",
  "sig": "<schnorr_sig>"
}
```

The tag-based shape avoids JSON content parsing on the relay side. `service` tags follow the DID Document `service` array semantics. `delegation` tag (when present) is the receiver's record of authority granted.

Replaceable: a new event with the same `(pubkey, kind, d-tag)` triple supersedes the previous one. Receivers maintain only the latest version.

Federated by default per ADR-073 D2 (kind 30033 ∈ default `mesh.federated_kinds`). This is how mesh-wide service discovery works without out-of-band config.

### D10 — Delegation patterns

Three primary patterns, per PRD-010 §5.5:

**Pattern α — User → Substrate Bridge** (replaces silent re-signing):
```
δ_U→V: conditions = "kind=14&kind=1059&created_at<T+86400"
```
VisionClaw bridge stores δ_U→V; when forwarding U-originated content, the bridge signs the wrap event but tags the seal with `["delegation", δ_U→V, ...]`. Forum reader sees delegator U, knows the wire delivery passed through V. Replaces `nostr_bridge.rs:219-222 re-signing` (PRD-010 F9).

**Pattern β — User → Agentbox Agent** (agent acts on behalf of user):
```
δ_U→A: conditions = "kind=4&kind=14&kind=1059&kind=30001&created_at<T+T_session"
```
Agent A, when emitting events on behalf of user U, includes `["delegation", δ_U→A, ...]` on the rumor (within seal). WAC ACLs on U-owned pods accept delegated agents per D11.

**Pattern γ — Server → Server** (substrate trust):
```
δ_V_a→V_b: conditions = "kind=30001&kind=30200&created_at<T+604800"
```
Long-lived (week-scale) operator-to-operator delegation. Used only for substrate-emitted beads. Operators rotate delegators on weekly schedule.

### D11 — WAC ACL accepts delegated agents

`pod-worker/src/acl.rs::evaluate_access` is extended to recognise events carrying `["delegation", ...]` tags:

1. Parse delegation tag; verify per D8.
2. Build effective agent set: `{event.pubkey, delegator_pk}`.
3. Match ACL `acl:agent` against either pubkey via lowercased `did:nostr:<hex>` form (D1).
4. If `acl:mode` permits, grant access.

This lets a U-owned pod accept writes from delegated agent A without explicit per-agent ACL entries. Operators control via delegation issuance, not per-key ACL maintenance.

ACL agentClass `foaf:Agent` and `acl:AuthenticatedAgent` are unaffected; no new agentClass introduced.

### D12 — Key rotation announcement (P5 follow-up)

Out of scope for PRD-010 P0-P4 but specified here for forward compatibility:

- A user with old key `K_old` who wants to rotate to new key `K_new`:
  1. Signs delegation `δ_K_old→K_new: conditions = "kind=10000..39999"` valid for 7 days.
  2. Publishes a kind-30033 event from K_new advertising K_new as the new identity, with the δ tag.
  3. Optionally publishes a kind-5 (deletion) on K_old after the transition window.
- DID Documents for both keys remain resolvable; receivers honor δ for cross-attribution during the window.
- After 7 days, K_old deactivated: DID Document service[].serviceEndpoint redirects, or DID Document carries `deactivated: true` (DID Core).

### D13 — Anti-drift CI assertions

Each repo's CI MUST assert:
- DID Document type: `verificationMethod[0].type == "SchnorrSecp256k1VerificationKey2019"`.
- @context order: `did/v1` precedes `secp256k1-2019/v1`.
- Pubkey form: `^[0-9a-f]{64}$`.
- Cross-language HKDF info: forum's `nostr-core/src/keys.rs:9 b"nostr-secp256k1-v1"` matches the JS-side `derive_from_prf` info string byte-for-byte.

These assertions run on every CI build; failures block merge.

## Consequences

### Positive

- **Single canonical identity primitive** across four codebases. A forum-side fix benefits agentbox + VC.
- **Trust without sharing keys**: NIP-26 delegation lets users authorise substrate bridges without giving up custody.
- **Discoverability without config**: kind-30033 mesh service-list events propagate via federation; receivers always learn current addresses for actors they care about.
- **Bridge attribution preserved**: replaces the silent re-signing antipattern (`nostr_bridge.rs:219-222`) with delegation-aware forwarding; original author always attributable.
- **WAC + delegation interop**: delegated agents access U-owned pods without per-agent ACL maintenance.

### Negative

- **CI assertions as a tax**: every emitter must add the type/context/info-string assertion. Not large but cross-repo.
- **NIP-26 verifier in three places**: agentbox JS, forum DO, VC Rust. Three implementations to keep in sync. Mitigation: reference test vectors shared across repos.
- **kind-30033 is a new kind**: not in any standard NIP. We are deliberately allocating in the unreserved range (30000-39999 parameterised replaceable). Document in NIP-11 advertised `supported_nips_extended` so external clients understand.
- **Delegation tag visible at wire**: the seal carries `["delegation", ...]`, so a relay operator can see who delegates to whom. NIP-59 wrap doesn't hide this. Trade-off: trust verifiability vs. metadata privacy. We choose verifiability.
- **Multi-DID-per-actor not via URN**: PRD-006's instinct for composite URNs is rejected. Some operators may dislike this; argued for in D7.

### Neutral

- **No effect on existing public Nostr ecosystem compatibility**: external clients without NIP-26 support see events with `["delegation", ...]` tags as just events with extra tags — they still verify against `event.pubkey`. Just lose the delegator-attribution semantic.
- **Tier-1 docs become 200 bytes larger** (extra @context entry). Acceptable.

## Alternatives Considered

### Alt-A — Single shared identity across substrates

User U holds one key; forum, agentbox, and VC all sign with it.

*Rejected*: violates custody boundaries. A compromised browser leaks U's key, which compromises agentbox + VC at the same time. No defence in depth.

### Alt-B — Per-message delegation (no persistent NIP-26)

Each event includes a fresh delegation token per-message. No long-lived delegation state.

*Rejected*: signature overhead doubles per event. NIP-26 token is ~150 bytes per tag; doing it per-event for high-volume substrates is wasteful. Long-lived delegation with bounded TTL is cheaper.

### Alt-C — `did:web` instead of `did:nostr`

Use HTTP-based DID method; no DID-via-relay needed.

*Rejected*: drops the cryptographic linkage. `did:web` uses HTTPS+TLS as authority; `did:nostr` uses signature primitives directly. The whole point of the mesh is signature-rooted identity that survives DNS churn.

### Alt-D — DID Document multitype `verificationMethod`

Emit multiple `verificationMethod` entries: `SchnorrSecp256k1VerificationKey2019` + `Ed25519VerificationKey2020` + ... so consumers pick what they support.

*Rejected*: NIP-only ecosystem; only Schnorr secp256k1 is signed against. Adding Ed25519 entries that no Nostr verifier uses is dead weight.

### Alt-E — Move npub format to bech32 of full SEC1 pubkey (legitimise sovereign-bootstrap.py)

If we change npub semantics so 64-byte SEC1 is what npub encodes, agentbox is "right".

*Rejected*: violates NIP-19 spec, breaks every other Nostr client in the world, including reference relays. Don't fork the standard to accommodate a bug.

## Implementation notes

### Verification library convergence (post-ADR-076)

Three places must verify NIP-26:
- **Forum** (post-ADR-076 absorption): `nostr-core/src/nip26.rs` is **deleted** per ADR-076 D1; the verifier comes from upstream `nostr::nips::nip26::{validate_delegation_tag, verify_delegation_signature}`. Forum's relay-worker imports the upstream verifier directly.
- **VisionClaw**: uses upstream `nostr::nips::nip26` via the `nostr` workspace dep added by PRD-010 F29. Wired into `mesh_bridge.rs::handle_inbound`.
- **Agentbox**: uses `nostr-tools` JS package (already a soft import per `mcp/nostr-bridge/relay-consumer.js:55-58`). Add explicit `verifyDelegation` import.

All three substrates therefore consume **community-maintained, paulmillr-vector-validated NIP-26 implementations** instead of three independent re-implementations. C1-class bugs cannot recur because there is no longer a per-substrate code path to drift in.

Cross-language test fixtures: a JSON file `tests/fixtures/nip26_vectors.json` shared across all three repos via copy-paste-with-CI-check. Each language's tests load vectors and assert verification matches expected. Fixture sourced from upstream `nostr` crate's own test vectors.

### kind-30033 reference implementation

Forum: emit on:
- User registration (initial mesh-services list with no delegations).
- DID Document resolver invocation (refresh trigger).
- Delegation grant (add delegation tag).

Agentbox: emit on:
- Container boot (operator's mesh-services list including agent's allowed-on services).
- Operator delegation issuance.

VisionClaw: emit on:
- Substrate boot (operator pubkey advertises substrate's relays).
- Delegation issuance.

Replaceable rules: receiver keeps only latest by `(pubkey, kind=30033, d-tag="mesh-services")`. TTL on cached Tier-3 doc = min(`event.created_at + 24h - now`, 600s) — shorter of the natural staleness and the polling cycle.

### sovereign-bootstrap.py rewrite (PRD-010 F5)

Pseudo-code:
```python
from cryptography.hazmat.primitives.asymmetric.ec import (
    EllipticCurvePrivateKey, generate_private_key, SECP256K1
)
from cryptography.hazmat.primitives import serialization

priv = generate_private_key(SECP256K1())
priv_bytes = priv.private_numbers().private_value.to_bytes(32, "big")
pub_point  = priv.public_key().public_numbers()
pub_x = pub_point.x.to_bytes(32, "big")
pub_y = pub_point.y

# BIP-340 lift_x: force even-y representation
if pub_y % 2 == 1:
    # Force even y by negating private key
    priv_bytes = ((SECP256K1_N - int.from_bytes(priv_bytes,"big")) % SECP256K1_N).to_bytes(32,"big")

x_only_pubkey_hex = pub_x.hex()                    # 32-byte BIP-340 x-only
nsec = bech32_encode("nsec", priv_bytes)            # 32-byte priv
npub = bech32_encode("npub", pub_x)                 # 32-byte x-only pubkey
```

`SECP256K1_N` = `0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141`.

Migration: existing identity files have `private_key_hex` in correct form (it's just a 32-byte scalar). Re-derive npub with the corrected algorithm; rename pod filesystem under `pods/<old_npub>/` → `pods/<new_npub>/` atomically.

### Event re-signing replacement (PRD-010 F9)

`src/services/mesh_bridge.rs::forward` (replaces `nostr_bridge.rs:182-247`):

```rust
async fn forward(&self, original: &Event) -> Result<()> {
  // Path 1: original carries delegation to bridge — forward verbatim.
  if let Some(delegation_tag) = original.tags.iter().find(|t| t[0] == "delegation") {
      let δ = DelegationToken::parse(delegation_tag)?;
      δ.verify(&original.pubkey, &self.bridge_keys.public)?;
      // Original is already signed by bridge under delegation; just publish.
      self.publish_to_target_relay(original).await
  }
  // Path 2: no delegation, fail-closed by default.
  else if self.config.forward_policy == ForwardPolicy::DelegateRequired {
      return Err(MeshBridgeError::DelegationRequired);
  }
  // Path 3: fail-open with metadata, configurable.
  else {
      let mut forwarded = original.clone();
      forwarded.tags.push(["forwarded-from".into(), original.pubkey.clone(), original.id.clone()]);
      self.bridge_keys.sign(&mut forwarded)?;
      self.publish_to_target_relay(&forwarded).await
  }
}
```

Operator chooses `forward_policy = "delegate-required"` (default, secure) or `"fwd-meta"` (lossy attribution but still works for opportunistic forwarding).

## References

- PRD-010 G1, G5, G6, F4, F5, F8, F9
- ADR-073 — Mesh topology (relay-relay protocol)
- ADR-075 — IS-Envelope contract (envelope shape)
- ADR-076 — Forum `nostr-core` absorption into upstream `nostr` crate (changes implementation of D8 verifier wiring)
- ADR-027 (existing) — DID identity stack (extended here)
- `docs/integration-research/05-crypto-gotchas.md` §1-§3, §8, §13-§14
- `docs/integration-research/06-uri-dataflow-alignment.md` §2, §10
- NIP-19 — Bech32-encoded entities
- NIP-26 — Delegated event signing
- NIP-42 — Authentication of clients to relays
- W3C DID Core — https://www.w3.org/TR/did-core/
- Multibase / Multicodec specs
- BIP-340 — Schnorr signatures for secp256k1
