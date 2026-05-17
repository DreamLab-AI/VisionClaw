# 04 — H1 + H2 Opacity on the Wire (ADR-050 §bit-29 / §three-tier)

> **Historical audit (2026-04-20)**: H2 implementation has since changed under
> [ADR-061](../../adr/ADR-061-binary-protocol-unification.md) (2026-04-30).
> Visibility is now enforced at the broadcast boundary
> (`ClientCoordinator::broadcast_with_filter`); the wire id is the raw u32 with
> no flag bits. This audit reflects the prior bit-29 wire mechanism. Current
> wire-format spec: [docs/binary-protocol.md](../../binary-protocol.md).

**Status: CLOSED (both); H2 mechanism replaced by ADR-061 broadcast filter.**

## H1 — Opacify cross-user private nodes (instead of drop)

### REST handler (`src/handlers/api_handler/graph/mod.rs`)

- **Opacifier** `opacify_for_caller(node, salt)` at :170-207 — returns a
  clone with:
  - `label`, `metadata_id`, `metadata`, `pod_url` cleared.
  - `opaque_id` populated via `crate::utils::opaque_id::opaque_id(salt,
    owner, canonical_iri)` at :183-186. Fails closed: when
    `OPAQUE_ID_SALT_SEED` is unset, `opaque_id` stays `None` and the
    client still sees a structural stub — no leakage either way.
  - `owner_pubkey`, `x/y/z/vx/vy/vz`, `color/size/weight/group/node_type`
    preserved (topology + shape are explicitly in scope per ADR-050
    §three-tier).
- **Handler routing** at :282-334 — after the graph-type filter at
  :305-321, the visibility check at :328 routes non-owner private nodes
  through `opacify_for_caller` at :331 instead of filtering them out.
  The salt is read lazily from `OPAQUE_ID_SALT_SEED` at :293-297 so a
  rotation on the next request takes effect without a restart.
- **Predicate** `visibility_allows(metadata, caller)` at :135-154 —
  unchanged from Sprint 1 semantics (public ⇒ always; private ⇒
  owner-match only); Sprint 2 simply changed the **action** on `false`
  from "drop" to "opacify".

### Tests (`tests/bit29_on_wire.rs:93-187`)

| Test | Asserts |
|------|---------|
| `h1a_anonymous_caller_gets_opacified_stub_not_drop` | Anonymous caller + private node: cleared label/metadata/pod_url, populated opaque_id, preserved owner+topology. JSON round-trip shape pinned. |
| `h1b_owner_caller_gets_full_fidelity_not_opacified` | `visibility_allows(meta, Some(owner)) == true`; node unchanged. |
| `h1c_signed_caller_sees_other_user_private_as_opacified` | Bob ↦ Alice's node: opacified; `opaque_id` deterministic for (salt, owner, canonical_iri). |

## H2 — Bit 29 on the wire (V3 + V4)

### V3 full-state encoder (`src/utils/binary_protocol.rs`)

- Signature: `encode_positions_v3_with_privacy(nodes, agent_ids,
  knowledge_ids, ontology_{class,individual,property}_ids, sssp, analytics,
  private_opaque_ids: Option<&HashSet<u32>>)` at :389-398.
- OR-in at :448-458: `encode_node_id(flagged_id, is_private)` where
  `is_private = private_opaque_ids.contains(node_id)` OR
  `contains(&get_actual_node_id(flagged_id))` — the double lookup
  handles the case where the caller passed either bare or type-flagged
  ids in the set.
- Gate: `sovereign_schema_enabled()` at :425 short-circuits when
  `SOVEREIGN_SCHEMA` is off; pre-ADR-050 wire bytes are preserved
  byte-for-byte in that case (H2b test pins this).

### V4 delta encoder (`src/utils/delta_encoding.rs`)

- Privacy helper `apply_private_opaque_flag(flagged_id, base_id, set)`
  at :34-46.
- Two call sites in the delta inner loop:
  - **Existing node with computed delta** at :214-216 (after position
    or velocity change detected).
  - **New node not in previous frame** at :229-231 (treated as full
    change via `DELTA_ALL_CHANGED`).
- Full-state resync path (frame 0, every 60th frame, and i16 overflow
  fallback at :143-152 and :253-256) delegates to the canonical encoder, so
  bit 29 propagates through both historical wire variants. *(post-ADR-061:
  bit-29 mechanism removed; visibility filtered at broadcast boundary.)*

### Encoder call sites (8 distinct locations, all now pass a real set)

| File | Line | Shape of caller |
|------|------|-----------------|
| `src/actors/client_coordinator_actor.rs` | 408, 427 | Per-client set computed by `compute_private_opaque_ids(node_type_arrays, caller_pubkey)` at :97-120. |
| `src/actors/client_coordinator_actor.rs` | 983 | Anonymous-broadcast path: `compute_private_opaque_ids(nta, None)`. |
| `src/handlers/fastwebsockets_handler.rs` | 473, 482 | Set built from `client_state.private_opaque_ids` at :448. |
| `src/handlers/socket_flow_handler/position_updates.rs` | 240, 428, 749, 1053, 1193 | Set computed per-broadcast; comment at :1182 explicitly names the opacify-for-receivers invariant. |
| `src/utils/binary_protocol.rs` | 538, 579 | Legacy thin-wrapper call sites delegating into the privacy encoder. |

Every call site in the current tree passes a real
`Option<&HashSet<u32>>`. No remaining `None`-passing regression path.

### Byte-level proof

`tests/bit29_on_wire.rs:265-272` pins the wire layout:

```rust
let node2_id_byte3 = payload[2 * 48 + 3];
assert_ne!(node2_id_byte3 & 0x20, 0,
    "byte-3 of node2's wire id must have 0x20 set (PRIVATE_OPAQUE_FLAG byte)");
```

`PRIVATE_OPAQUE_FLAG == 0x2000_0000 == 1 << 29` (pinned by
`private_opaque_flag_is_bit_29_reminder` at :353-358). In little-endian
u32, bit 29 lives in the high nibble of the fourth byte — `0x20` on
`byte[3]` is exactly the bit-29 signature.

### Flag-off contract (symmetric)

`h2b_caller_owns_all_private_no_bit29_set` at :278-310 and the
`SOVEREIGN_SCHEMA off` arm of `h2a` at :252-260 assert that when the
flag is disabled, the encoder emits zero bit-29 hits even if the set
is non-empty. This preserves the pre-ADR-050 wire format for rollback.

## Verdict

Both findings closed. Opacity rides through three layers without dropping
the node: the REST handler returns a structural stub with HMAC-derived
opaque_id; the binary V3/V4 encoders OR bit 29 onto the wire id; and the
client can diff using the opaque_id across frames. The feature flag
(`SOVEREIGN_SCHEMA`) gates the wire-format change only — the REST-layer
opacification runs unconditionally, so anonymous REST callers are already
protected even before the binary-wire rollout. This is the correct
layering: HTTP-level privacy is always-on; binary-wire privacy is
opt-in per deployment.
