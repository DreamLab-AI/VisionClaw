# ADR-075 — Inter-System Message Envelope (IS-Envelope v1)

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-010 G2, F10, F19 |
| Companion ADRs | ADR-073, ADR-074, ADR-076 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` |

## Context

Three substrates (forum, agentbox, VisionClaw) need to exchange messages across the mesh defined in ADR-073, signed by the identities canonicalised in ADR-074. The messages may be:

- **DMs** (forum user → forum user, or forum user → agentbox agent)
- **Tool invocations** (forum user / agentbox agent → another agentbox agent: "run skill X with args Y")
- **Tool results** (response to invocation)
- **Knowledge links** (VisionClaw substrate → forum user: "your post indexed at urn:visionclaw:bead:...")
- **Moderation actions** (forum admin → all substrates: "ban pubkey X")
- **Mesh control plane** (relay → relay: ping, peer service-list update)

These have very different shapes if encoded ad-hoc — kind-1 chat is text; kind-30001 bead is a JSON Neo4j-shaped object; kind-30910 ban is structured admin action; tool invocation is `{skill_name, args}`. Without a unifying contract, every cross-system handler must teach itself every shape.

The mesh also needs to bridge into Solid LDN: agentbox's pod-inbox writes raw Nostr-event-wrapped JSON today (`relay-consumer.js:215-221`) which is **not** Linked Data Notifications. PRD-010 F19 mandates LDN-shaped inbox payloads. So the envelope must round-trip losslessly between Nostr-rumor-content and LDN AS2.

The cryptographic carrier is fixed by ADR-073 D2 (federated kinds) and ADR-074 D8 (delegation): kind-1059 gift-wrap on the wire, kind-13 seal authored by sender (or by delegatee with delegation tag), kind-14 rumor (or new mesh kinds 30050 + 30033) carrying the payload.

## Decision

### D1 — IS-Envelope v1 — canonical JSON shape

Every cross-system message carries this JSON object as the rumor's `content`:

```jsonc
{
  "v":      1,                                     // envelope schema version (integer)

  "to":     "did:nostr:<hex>",                     // recipient identity (REQUIRED)
  "from":   "did:nostr:<hex>",                     // origin identity (REQUIRED, even with delegation)
  "via":    [                                      // optional re-attribution chain
    "did:nostr:<bridge_hex>"
  ],

  "subj":   "urn:visionclaw:bead:<scope>:<sha>",   // optional URN of originating context
  "thread": "<event_id_hex>",                      // optional reply-to; standard Nostr "e" tag mirror

  "ttl":    1763000000,                            // unix ts; envelope MUST NOT be processed past this
  "kind":   "chat" | "tool_invoke" | "tool_result" | "knowledge_link" | "moderation" | "mesh_ping",
  "lang":   "text/markdown" | "text/plain" | "application/json+ld",

  "body":   "<string|object>",                     // payload (shape per kind, see D3)
  "hint":   {                                      // optional rendering / routing hints
    "render_with":   "<viewer_id>",
    "render_inline": true,
    "priority":      "low"|"normal"|"high"
  },

  "delegation": {                                  // optional NIP-26 delegation token (mirrored from tag)
    "delegator":  "did:nostr:<hex>",
    "conditions": "kind=14&created_at<1763500000",
    "sig":        "<128 hex>"
  }
}
```

### D2 — Required vs. optional fields

Required:
- `v` (integer, currently always `1`)
- `to`, `from` (canonical `did:nostr:<lowercase-hex>` per ADR-074 D1)
- `kind` (one of the seven enumerated values)
- `body` (shape per `kind`)

Optional:
- `via`, `subj`, `thread`, `ttl`, `lang`, `hint`, `delegation`

Receivers MUST ignore unrecognised fields without rejecting the envelope.
Receivers MUST reject envelopes missing required fields with `OK false "envelope-malformed: <field>"`.

### D3 — Per-kind body shapes

#### `chat`
```jsonc
"body": "Hello! Can you check the latest workshop config?"   // plain string
```
or
```jsonc
"body": {
  "text": "Hello!",
  "attachments": [
    { "url": "https://pod.../path", "mime": "image/png", "alt": "screenshot" }
  ]
}
```
Maps to LDN AS2 `{type: "Note", content: <text>, attachment: [...]}`.

#### `tool_invoke`
```jsonc
"body": {
  "tool":     "urn:agentbox:skill:summarise-thread",
  "args":     { "thread_id": "abc123", "max_tokens": 500 },
  "reply_to": "<event_id_hex>"   // expected response thread anchor
}
```
Maps to AS2 `{type: "Offer", actor: <from>, object: {type: "Tool", id: <tool>}, target: <to>}` with `args` as `instrument`.

#### `tool_result`
```jsonc
"body": {
  "tool":     "urn:agentbox:skill:summarise-thread",
  "status":   "ok" | "error",
  "result":   { "summary": "..." } | null,
  "error":    { "code": "...", "message": "..." } | null,
  "in_reply_to": "<event_id_of_tool_invoke>"
}
```
Maps to AS2 `{type: "Add", object: <result> | <error>, target: <to>}` with `inReplyTo`.

#### `knowledge_link`
```jsonc
"body": {
  "subject_urn": "urn:visionclaw:bead:<hex>:<sha256-12>",
  "claim":       "indexed" | "linked" | "deindexed",
  "context":     {
    "graph_uri": "<neo4j-or-pod-url>",
    "labels":    ["KGNode", "Bead"]
  }
}
```
Maps to AS2 `{type: "Announce", object: <subject_urn>, ...}`.

#### `moderation`
```jsonc
"body": {
  "action":  "ban" | "mute" | "warn" | "unban" | "unmute",
  "target":  "did:nostr:<hex>",
  "reason":  "spam" | "abuse" | "...",
  "expires": 1763500000,                  // unix ts; null = permanent
  "kind_event_ref": "<event_id_of_30910>" // canonical mod event reference
}
```
The actual mod event (30910/30911/etc.) flows via the relay; the IS-Envelope is a sidecar notification. Maps to AS2 `{type: "Block", actor: <from>, object: <target_did>, instrument: <kind_event_ref>}`.

#### `mesh_ping`
```jsonc
"body": {
  "self_pubkey": "did:nostr:<hex>",
  "peer_relays": ["wss://...", ...],
  "uptime_s":    1284
}
```
Used for mesh control plane: peer relays exchange pings every 30s to verify reachability and stay current on each other's mesh service-lists. Maps to AS2 `{type: "View", ...}` (lightweight presence).

### D4 — Wire encoding

The envelope is the `content` field of a kind-14 rumor (NIP-59) inside a kind-13 seal inside a kind-1059 gift wrap. Or, for non-DM mesh control traffic, the envelope is the `content` of a kind-30050 ("mesh event") parameterised replaceable event (new kind allocated by this ADR; replaceable so peer service-list updates supersede).

| Use case | Outer kind | Inner kinds | Notes |
|----------|-----------|-------------|-------|
| User DM | 1059 (wrap) | 13 (seal) → 14 (rumor) | Standard NIP-59 |
| Agent invocation | 1059 | 13 → 14 | Same as DM; differentiated by `kind="tool_invoke"` in envelope |
| Tool result | 1059 | 13 → 14 | Same |
| Knowledge link | 1059 | 13 → 14 | Same |
| Moderation sidecar | 1059 | 13 → 14 | The 30910 event itself rides the relay separately; this envelope is the cross-system notification |
| Mesh ping (relay-relay) | 30050 | n/a | Plain signed event; no wrap (relay-to-relay context) |
| Mesh service-list | 30033 | n/a | Per ADR-074 D9 |

### D5 — Canonical JSON serialisation

Envelopes are serialised with **RFC 8785 JSON Canonicalization Scheme (JCS)** before signing. This guarantees:
- Object keys sorted lexicographically.
- No whitespace between tokens.
- Numbers in shortest IEEE-754 form.
- Strings escaped per RFC 8259.

solid-pod-rs already provides JCS (`management-api/middleware/linked-data/jcs.js` per `docs/integration-research/03-agentbox-surfaces.md` §10). VisionClaw + forum reuse the same library for consistency.

JCS is required so that the rumor's id (SHA-256 of canonical event JSON) is stable across encoders. Without JCS, two encoders producing semantically identical envelopes would emit different ids.

### D6 — Signing semantics

Per NIP-59:
- The wrap (kind-1059) is signed by an ephemeral throwaway key. Single-use; never reused.
- The seal (kind-13) is signed by the **delegatee** (the actor whose pubkey is in `from` — or, when delegation is in play, the actor signing on behalf of the delegator).
- The rumor (kind-14) is **unsigned**; its integrity comes from being inside the seal.

When `delegation` is present in the envelope:
- The seal signer is the delegatee.
- The envelope's `from` is the **delegator**.
- The envelope's `via` lists the delegatee chain (most-recent last).
- The seal's `["delegation", delegator_hex, conditions, sig]` tag carries the NIP-26 token (mirrored in the envelope's `delegation` field for in-band visibility).

Receiver verification:
1. Unwrap kind-1059 (verify wrap sig with ephemeral key, decrypt seal with own NIP-44 key).
2. Verify seal signature with `seal.pubkey`.
3. If seal carries `["delegation", ...]`: verify per ADR-074 D8.
4. Decrypt rumor; parse envelope JSON.
5. Assert `from == delegator` (when delegation present) OR `from == seal.pubkey` (when not).
6. Apply business logic for `kind`.

### D7 — TTL semantics

`ttl` (when present) is a **hard cutoff**. Receivers MUST NOT process envelopes whose `ttl < now()`. Default for envelopes lacking TTL: created_at + 7 days (configurable via `mesh.envelope_default_ttl_s`).

Use case: tool invocations time out after expected response window; moderation actions sometimes have hard expiry; mesh pings shouldn't be processed long after their relevance window.

### D8 — `subj` URN format

The `subj` field carries the originating context — typically a `urn:visionclaw:bead:*`, `urn:visionclaw:concept:*`, `urn:agentbox:bead:*`, or `urn:agentbox:event:*`. Validation:

- Form: must match `^urn:(visionclaw|agentbox|solid):[a-z]+:[^\s]+$` (broad regex; substrate-specific validation per BC20 ACL).
- Resolution: receivers MAY attempt to resolve `subj` via their substrate's URI resolver to render the context. If resolution fails, the envelope is still processed (subj is hint, not gate).
- Cross-system: BC20 ACL (PRD-006 §5.5) is responsible for translating `urn:visionclaw:bead:*` ↔ `urn:agentbox:bead:*` when the recipient's substrate doesn't natively understand the originator's namespace.

### D9 — `via` semantics and re-attribution

When a bridge (forum's `auth-worker`, agentbox's `RelayConsumer`, or VisionClaw's `MeshBridge`) forwards an envelope, it appends its own DID to `via[]`. The original `from` is preserved — `via` is the routing breadcrumb trail.

Receivers display `from` as the apparent author; advanced UIs MAY render `via` as "delivered through X" metadata. Rate limiting and abuse heuristics SHOULD use `via[last]` (the immediate sender) as the responsible-party identity.

Maximum chain length: 4 hops. Envelopes with `len(via) > 4` are dropped with `OK false "envelope-via-too-long"`. (Defensive cap; in practice 2-3 hops is typical.)

### D10 — LDN AS2 mapping

For pod-inbox bridge writes (PRD-010 F19), envelope is wrapped in AS2:

```jsonld
{
  "@context": [
    "https://www.w3.org/ns/activitystreams",
    "https://w3id.org/dreamlab/mesh/v1"
  ],
  "type":   "<AS2 type per kind, see D3>",
  "actor":  "did:nostr:<from_hex>",
  "target": "did:nostr:<to_hex>",
  "object": "<envelope.body or AS2-translated body>",
  "id":     "urn:nostr:event:<event_id_hex>",
  "published": "<iso8601 of event.created_at>",

  "x:envelope": <full envelope JSON>,
  "x:nostrEvent": <full signed Nostr event for verification>,
  "x:via": ["did:nostr:<bridge_hex>", ...]                  // mirrored
}
```

The `x:envelope` and `x:nostrEvent` extensions allow LDN consumers to verify the original signature without trusting the bridge's transformation. The AS2 outer shape lets vanilla LDN consumers process the message as a notification.

`https://w3id.org/dreamlab/mesh/v1` is a custom JSON-LD context defining `x:envelope`, `x:nostrEvent`, `x:via`. solid-pod-rs ships this context bundled (`management-api/middleware/linked-data/contexts/mesh-v1.jsonld`).

### D11 — Forward compatibility

Envelopes are versioned:
- v1: this ADR's shape.
- v2+: future. New required fields MUST be additive (receivers ignore unknown). Removed fields require version bump.

Schema discovery: `GET /.well-known/mesh-envelope-schemas/v1.json` returns a JSON Schema for v1. Substrates ship the schema bundled.

### D12 — Replay and dedup

Envelopes are deduplicated by their **outer kind-1059 event id** (or kind-30050/30033 event id for non-wrapped mesh kinds). This is the canonical Nostr event id, stable across encoders by D5 JCS.

Each substrate's event-ingest path checks against an LRU `seen_event_ids` cache (capacity 4096, TTL 600s). Already-seen ids are dropped silently. Combined with ADR-073 D2 fan-out dedup, end-to-end loop-free.

### D13 — Size limits

- Maximum envelope body (after JCS encoding): 64 KiB.
- Maximum gift-wrap event size: 128 KiB (NIP-59 recommends 64 KiB; we double for tool results that may carry small structured data).
- Larger payloads MUST use a `body.attachments[]` shape (D3 chat) referencing pod-hosted resources. The envelope itself stays small.

### D14 — Encoding examples

**Example 1: Forum user `U` DMs agentbox agent `A`**

Outer wrap (kind 1059) — published to forum CF relay:
```jsonc
{
  "kind": 1059,
  "pubkey": "<ephemeral_hex>",
  "created_at": <jittered ±48h>,
  "tags": [["p", "<A_hex>"]],
  "content": "<base64(NIP-44(seal_event_json, throwaway_to_A_conv_key))>",
  "id": "<wrap_id>",
  "sig": "<ephemeral_sig>"
}
```

Seal (kind 13) — encrypted inside wrap:
```jsonc
{
  "kind": 13,
  "pubkey": "<U_hex>",
  "created_at": <jittered ±48h>,
  "tags": [],
  "content": "<base64(NIP-44(rumor_json, U_to_A_conv_key))>",
  "id": "<seal_id>",
  "sig": "<U_sig>"
}
```

Rumor (kind 14) — IS-Envelope as content:
```jsonc
{
  "kind": 14,
  "pubkey": "<U_hex>",
  "created_at": <true_ts>,
  "tags": [["p", "<A_hex>"]],
  "content": "<JCS-canonical envelope JSON>"
}
```

Envelope content:
```jsonc
{
  "v": 1,
  "to": "did:nostr:<A_hex>",
  "from": "did:nostr:<U_hex>",
  "ttl": 1763500000,
  "kind": "chat",
  "lang": "text/markdown",
  "body": "Hi A — can you summarise thread urn:visionclaw:concept:project:onboarding?",
  "subj": "urn:visionclaw:concept:project:onboarding"
}
```

**Example 2: Agentbox agent `A` invokes a skill on agent `B`** (with delegation from user `U`):

```jsonc
{
  "v": 1,
  "to":   "did:nostr:<B_hex>",
  "from": "did:nostr:<U_hex>",
  "via":  ["did:nostr:<A_hex>"],
  "kind": "tool_invoke",
  "ttl":  1763400000,
  "body": {
    "tool":     "urn:agentbox:skill:translate-summary",
    "args":     { "text": "...", "target_lang": "fr" },
    "reply_to": "<this_event_id>"
  },
  "delegation": {
    "delegator":  "did:nostr:<U_hex>",
    "conditions": "kind=14&created_at<1763500000",
    "sig": "<128-hex-sig>"
  }
}
```

The seal is signed by A; the envelope says authored-by-U-via-A; delegation token verifies U authorised A for kind-14 tool invocations.

**Example 3: VisionClaw substrate emits knowledge_link to forum user**

```jsonc
{
  "v": 1,
  "to":   "did:nostr:<U_hex>",
  "from": "did:nostr:<V_hex>",
  "kind": "knowledge_link",
  "subj": "urn:visionclaw:bead:<U_hex>:abcdef012345",
  "body": {
    "subject_urn": "urn:visionclaw:bead:<U_hex>:abcdef012345",
    "claim":       "indexed",
    "context":     {
      "graph_uri": "neo4j://visionclaw/graph#bead/abcdef012345",
      "labels":    ["KGNode", "Bead"]
    }
  }
}
```

Forum-side rendering: small inline notification "Your post was indexed in the knowledge graph" with a click-through to `/api/v1/uri/urn:visionclaw:bead:...`.

### D15 — Conformance test surface

Every substrate ships:
- `tests/envelope_v1_round_trip.rs` (or .js) — for each of the seven `kind` values, build envelope, encode JCS, sign rumor, wrap seal, sign wrap; reverse the whole stack and assert byte-identical envelope.
- `tests/envelope_v1_ldn_mapping.rs` — for each kind, encode envelope and assert LDN AS2 mapping matches the schema in `mesh-v1.jsonld`.
- `tests/envelope_v1_malformed.rs` — required-field absence, wrong types, oversized payloads — assert receiver rejects with the expected error code.

Cross-substrate fixtures (`tests/fixtures/envelope_v1/*.json`) shared via copy-with-CI-check (no submodule; CI compares hashes).

## Consequences

### Positive

- **Single message contract** across three substrates and seven message kinds. Adding a new substrate or a new message kind is contract work, not bespoke code.
- **LDN ↔ Nostr round-trip preserves provenance**: pod-inbox payloads stay verifiable; LDN consumers can inspect `x:nostrEvent` for original-signature replay.
- **Delegation is wire-visible**: receivers can audit attribution chains without trusting any bridge.
- **Pre-defined business kinds** (chat, tool_invoke, tool_result, knowledge_link, moderation, mesh_ping) cover ~90% of expected traffic. Edge cases (custom skill payloads) ride inside `body.args` as opaque JSON.
- **Versioned**: forward-compatible by design; v2 envelopes coexist with v1.

### Negative

- **JCS canonicalisation overhead**: every envelope encode/decode goes through JCS. Cost: ~1ms for 1KB envelopes. Acceptable.
- **Seven kinds is a fence**: anything outside is "unknown kind" and drops to a generic chat-with-structured-body. Operators may find this constraining for novel message types. Mitigation: kind taxonomy expandable in v2.
- **64 KiB body limit**: real-world tool results may exceed (e.g. multi-image generation results). Workaround: `body.attachments[]` with pod-hosted blobs — but adds a latency hop. Trade-off documented; large media is out-of-band.
- **Two new kind allocations** (30050 mesh-event, 30033 mesh-services) in the unreserved 30000-39999 range. Future NIP could collide. Mitigation: track allocations in a public registry; if a NIP is published with a colliding kind, rev to a new range.
- **AS2 `x:` extensions are non-standard**: not all LDN consumers will recognise `x:envelope`/`x:nostrEvent`. They still see the AS2 outer shape so render the message; just lose the verification metadata.

### Neutral

- **Envelope size**: typical chat ~250 bytes, knowledge_link ~400 bytes, tool_invoke ~500-2000 bytes. Sub-1KiB envelope fits comfortably in single TLS segment + nostr-rs-relay's 131072-byte limit.
- **Signing cost**: same as ordinary NIP-59 gift-wrap; one Schnorr per layer plus NIP-44 ChaCha20-Poly1305. Standard cost.

## Alternatives Considered

### Alt-A — One Nostr kind per business message type

Allocate kinds 30060-30065 for chat/tool_invoke/tool_result/knowledge_link/moderation/mesh_ping. No envelope; the kind IS the type.

*Rejected*: bloats the kind space; harder to evolve schema (each kind needs its own per-version contract); harder for relays to filter (they can't say "all mesh kinds" with a single filter). The envelope-with-typed-`kind`-field is simpler.

### Alt-B — Use an existing standard (CloudEvents, ActivityStreams 2.0 directly)

Don't define IS-Envelope; use AS2 or CloudEvents on the wire.

*Rejected*: AS2 is verbose (every field is a JSON-LD URI); not all fields map cleanly to Nostr (no AS2-equivalent of `delegation`); CloudEvents is HTTP-shaped and assumes broker semantics that Nostr doesn't have. The IS-Envelope is purpose-built to map to AS2 at the LDN boundary while staying compact on the Nostr wire.

### Alt-C — Binary envelope (Protocol Buffers or MessagePack)

Use a binary serialisation for envelope content; smaller and faster.

*Rejected*: NIP-59 rumors are text/JSON; mixing binary inside text-based events requires base64 wrapping which loses the size win. Also Nostr ecosystem tooling (nostr-tools, NDK, relay debugging UIs) all assume JSON. Compatibility with existing Nostr clients is more valuable than a 30% size reduction.

### Alt-D — No `delegation` field; rely solely on the seal's `["delegation", ...]` tag

The tag is sufficient for verification; the envelope `delegation` field duplicates it.

*Rejected*: the envelope `delegation` field is a convenience for application logic that processes envelopes without recomposing the seal. Receivers can choose to use either source — but having both means the verifier can cross-check (tag and field MUST agree, else reject).

### Alt-E — Per-kind body shape as separate sub-envelopes

`body` is itself an AS2 ActivityStreams object per-kind; no IS-Envelope-specific shapes.

*Rejected*: AS2 is too verbose for compact mesh use. The IS-Envelope's `body` shapes are deliberately minimal; AS2 mapping happens only at the LDN boundary (D10).

## Implementation notes

### Reference implementation (post-ADR-076)

`nostr-core/src/envelope.rs` (NEW) sits atop the upstream `nostr` crate types
(post-ADR-076 absorption). The envelope itself is project-specific data; the
underlying NIP-59 wrap, kind-13 seal, kind-14 rumor mechanics come from
`nostr::nips::nip59`. JCS canonicalisation reuses the same library across all
substrates. Provides:

```rust
pub struct Envelope {
    pub v: u8,
    pub to: String,
    pub from: String,
    pub via: Vec<String>,
    pub subj: Option<String>,
    pub thread: Option<String>,
    pub ttl: Option<u64>,
    pub kind: EnvelopeKind,
    pub lang: Option<String>,
    pub body: serde_json::Value,
    pub hint: Option<EnvelopeHint>,
    pub delegation: Option<DelegationToken>,
}

pub enum EnvelopeKind { Chat, ToolInvoke, ToolResult, KnowledgeLink, Moderation, MeshPing }

impl Envelope {
    pub fn to_jcs_string(&self) -> String { /* RFC 8785 */ }
    pub fn from_jcs_str(s: &str) -> Result<Self, EnvelopeError> { /* validate */ }
    pub fn to_ldn_as2(&self, original_event: &Event) -> serde_json::Value { /* D10 */ }
    pub fn validate(&self) -> Result<(), EnvelopeError> { /* required fields, sizes, formats */ }
}
```

Forum's `nostr-core` ships this; agentbox ports it to JS (`mcp/nostr-bridge/envelope.js`); VisionClaw consumes the Rust crate.

### Encoder pipelines (post-ADR-076)

Forum (`forum-client/src/dm/mod.rs`) — post-absorption uses upstream gift-wrap
machinery, project-specific shim only for the envelope JSON shape:
```rust
// Outbound DM
let env = Envelope { v: 1, to: ..., from: ..., kind: Chat, body: text, ... };
env.validate()?;
// Upstream (nostr crate) handles the NIP-59 three-layer wrap end-to-end:
let wrap = nostr::nips::nip59::gift_wrap(
    &my_keys,
    &recipient_pubkey,
    nostr::nips::nip59::UnsignedEvent::new(14, my_pk, vec![tag_p(to)], env.to_jcs_string()),
)?;
relay.publish(wrap).await?;
```

Inbound:
```rust
let unwrapped = nip59_unwrap(wrap_event, my_sk)?;
let env = Envelope::from_jcs_str(&unwrapped.rumor.content)?;
env.validate()?;
match env.kind {
  EnvelopeKind::Chat => render_message(&env),
  EnvelopeKind::ToolInvoke => dispatch_tool(&env),
  ...
}
```

Agentbox `RelayConsumer._processEvent`:
```js
const unwrapped = await unwrapGift(event, agentSecretKey);
const envelope = JSON.parse(unwrapped.rumor.content);
validateEnvelope(envelope);
if (envelope.kind === "tool_invoke") {
    const tool = envelope.body.tool;
    const args = envelope.body.args;
    await orchestrator.spawnAgent({ skill: tool, args, replyTo: envelope.body.reply_to });
}
```

### Schema

`docs/specs/envelope-v1.schema.json` (NEW) — JSON Schema 2020-12 describing the IS-Envelope. Used by:
- Forum CI (`pnpm validate:schemas`).
- Agentbox CI (`npm run lint:schemas`).
- VisionClaw CI (`cargo test --test envelope_schema_validation`).

### Backward compatibility with current bead path

VisionClaw's existing kind-30001 bead emission (`src/services/nostr_bead_publisher.rs`) is **not** an IS-Envelope today. Migration:
- Phase 4: ship dual-publish — kind-30001 (legacy) AND kind-1059 wrap with IS-Envelope (new) for each bead.
- Phase 5: deprecate kind-30001; mesh consumers migrate to IS-Envelope.
- Phase 6: legacy kind-30001 emission removed; only IS-Envelope.

Six-month deprecation window. Operators of legacy consumers (pre-mesh forum versions) get notification well in advance.

## References

- PRD-010 G2, F10, F19, F25 (envelope library on top of upstream `nostr` post-absorption)
- ADR-073 — Mesh topology
- ADR-074 — DID:Nostr canonicalisation & trust pivot
- ADR-076 — Forum `nostr-core` absorption (envelope sits atop upstream NIP-59 implementation)
- `docs/integration-research/05-crypto-gotchas.md` §6, §7
- `docs/integration-research/03-agentbox-surfaces.md` §3, §10
- NIP-01 — Basic protocol flow
- NIP-44 — Encrypted payloads (v2)
- NIP-59 — Gift wrap
- W3C ActivityStreams 2.0 — https://www.w3.org/TR/activitystreams-core/
- W3C LDN — https://www.w3.org/TR/ldn/
- RFC 8785 — JSON Canonicalization Scheme (JCS)
