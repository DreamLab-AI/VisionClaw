# ADR-050: Pod-backed KGNode Schema — Sovereign Private Nodes

## Status

Ratified

## Date

2026-04-19

## Related Documents

- ADR-048 — dual-tier identity model (`:KGNode` data plane vs `:OntologyClass` OWL)
- ADR-028-ext — NIP-98 auth extension (used for authenticated-as-owner Pod access)
- ADR-030-ext — per-user GitHub creds in Pod (companion Wave-3 ADR)
- ADR-051 — visibility transitions (publish/unpublish saga, companion Wave-2 ADR)
- ADR-052 — WAC default-private container policy
- ADR-053 — solid-pod-rs sidecar (Pod storage backend)
- Commit `89c8d800e` — `refactor/kg-node-rename` (Concept → KGNode rename landed)

## Context

ADR-048 established a two-tier identity model where `:KGNode` is the data-plane
label for narrative pages and `:OntologyClass` is the OWL T-Box vocabulary tier.
Wave 2 of the sovereign-mesh work extends that data-plane tier to support
**per-user Pods as the write-master for private content** while preserving the
topology of the public graph.

The sovereign-private-node model imposes three new requirements on `:KGNode`:

1. **Every wikilink target becomes a `:KGNode`**, regardless of whether the
   target is a published page or a private stub. Private stubs must be first-
   class graph citizens so physics weighting and shape rendering continue to
   work on the anonymous/public view.
2. **Private stubs must never leak**. Their labels, body, metadata, and Pod
   location must be invisible to the anonymous API surface and to other users.
   Only the Pod owner can resolve the opaque placeholder back to its real
   content.
3. **The Pod is the write-master** for private content; Neo4j is a derived
   index. The schema must carry enough metadata to locate the content in the
   Pod and enforce the visibility invariants at query time.

The previous draft designs used `sha256(path)[:12]` style opaque identifiers.
Those are dictionary-attackable: an adversary with a suspected path list can
re-hash candidates and confirm membership. We need an HMAC with a rotating
server secret to close that gap without losing referential stability across a
session.

Bit 29 on the wire-level `node_id` was reserved during the prior binary
protocol consolidation work (ADR-037, since superseded by ADR-061) for an
opaque-node flag. ADR-061 replaces the bit-29 wire mechanism with per-client
broadcast-boundary filtering; see the H2 section below for the current
implementation.

## Decision

Extend the `KGNode` struct (post-rename per commit `89c8d800e`) with four new
fields and reserve bit 29 on `node_id` as the on-wire privacy marker.

### Rust schema additions

```rust
pub struct KGNode {
    // Existing (pre-ADR-050):
    pub id: u32,
    pub metadata_id: Option<String>,
    pub label: Option<String>,
    pub owl_class_iri: Option<String>,
    pub node_type: Option<String>,
    // ... existing semantic fields ...

    // New for sovereign model:
    pub visibility: Visibility,          // enum { Public, Private }
    pub owner_pubkey: Option<String>,    // hex pubkey; None only for server-owned canon
    pub opaque_id: Option<String>,       // 24 hex chars; HMAC(session_salt, owner_pubkey, canonical_iri)
    pub pod_url: Option<String>,         // Pod URI where content lives
}

pub enum Visibility {
    Public,
    Private,
}
```

### Canonical IRI

One scheme identifies a node independent of its current path:

```
visionclaw:owner:{npub}/kg/{sha256(relative_path)}
```

- Namespaced by **owner pubkey** (npub form) plus **SHA-256 of the relative
  page path**.
- **Rename-proof as identity-preserving is a non-goal**: if a user moves
  `page.md` to `folder/page.md`, the IRI changes. Rename **is** a new identity.
  This matches Logseq's filename-as-title semantics and avoids a rename log.
- A move between owners is therefore always a new node on both sides.

### Opaque ID construction

The opaque placeholder surfaced to the anonymous API is an HMAC, not a hash:

```rust
opaque_id = hex_truncate_24(
    HMAC_SHA256(
        key:     server_session_salt,         // rotated daily from server env
        message: concat(owner_pubkey_hex, "|", canonical_iri),
    )
)
```

- **Key rotation**: `server_session_salt` rotates daily. The prior day's salt is
  retained for 48h to keep cached opaque IDs resolvable during the rollover.
- **Output**: 24 hex chars (96 bits). Collision probability is negligible in the
  intended corpus sizes and the server can re-roll on collision.
- **Effect**: HMAC closes the dictionary-attack vector that `sha256(path)[:12]`
  schemes suffer; rotation prevents longitudinal correlation of the same
  private node across users or across days.

### Visibility enforcement at the broadcast boundary

> **Updated by [ADR-061](ADR-061-binary-protocol-unification.md) (2026-04-30)**:
> H2 is implemented at the broadcast boundary, not in the wire format.
> `ClientCoordinator::broadcast_with_filter` drops positions for nodes the
> caller cannot see; the wire id is the raw u32. Anonymous viewers therefore
> receive no positions, no labels, and no metadata for private nodes — same
> end-state as the prior bit-29 mechanism, simpler implementation. The
> single-source wire-format spec is [docs/binary-protocol.md](../binary-protocol.md).

#### Historical (pre-ADR-061): Bit 29 reservation on `node_id`

The prior binary protocol's 32-bit node ID field reserved bit 29 for the
private-opaque flag:

```rust
// Removed by ADR-061 — node_id is the raw u32 on the wire.
pub const PRIVATE_OPAQUE_FLAG: u32 = 0x20000000;  // bit 29 (historical)
```

Under ADR-061, the wire id is the raw u32 with no flag bits. The bit-29
opacification mechanism is replaced by per-client position filtering at
the broadcast boundary; visibility is no longer encoded on the wire.

### Neo4j schema indexes

```cypher
CREATE INDEX kg_node_visibility IF NOT EXISTS
  FOR (n:KGNode) ON (n.visibility);

CREATE INDEX kg_node_owner IF NOT EXISTS
  FOR (n:KGNode) ON (n.owner_pubkey);

CREATE INDEX kg_node_opaque IF NOT EXISTS
  FOR (n:KGNode) ON (n.opaque_id);
```

### Backwards compatibility

Legacy rows without `visibility` are treated as `public` via Cypher
`COALESCE(n.visibility, 'public')`. No backfill migration is required for the
pre-sovereign corpus; it becomes the public canon by default.

## Non-Goals (v1)

- **Cross-user private-node delegation**: a private node owned by user A cannot
  be selectively revealed to user B. Deferred to v2.
- **NIP-44 encryption of Pod-stored content**: content-at-rest encryption in
  the Pod is deferred to v1.5 (defence-in-depth; Pod ACL is sufficient for v1).
- **Multi-Pod-server sharding**: a single Pod server per user in v1; federation
  across Pod servers is a v2 concern.
- **GitHub App OAuth**: v1 uses PAT (see ADR-030-ext); App OAuth is v1.5.

## Consequences

### Positive

- **Physics blind to visibility**: forces compute on topology only, not on
  labels or content. No CUDA kernel changes are required; the private stub is
  just another `:KGNode` from the physics engine's perspective.
- **Binary protocol stays string-free**: per ADR-061, the broadcast boundary
  filter drops private-node positions for unauthorised clients. Clients render
  the opacified shape without ever receiving the real label.
- **Canonical IRI survives renames as a new identity**: this is the *correct*
  semantic for Logseq pages — a rename is an authorial act, not a move.
- **HMAC + rotating salt** closes the dictionary-attack vector present in prior
  `sha256(path)[:12]` designs.
- **Pod-as-write-master** gives users hard data sovereignty: the server cannot
  synthesise a private-looking node that the owner did not create.

### Negative

- **Opaque_id regeneration on salt rotation** forces cache invalidation once a
  day. The 48h dual-salt window bounds user-visible disruption.
- **Added fields bloat node records**; mitigated by Neo4j's variable-length
  property storage. Legacy nodes without the new fields pay no cost.
- **Client trusts the broadcast filter** (post-ADR-061): the client never
  receives positions for nodes it cannot see, so there is no per-frame
  opacity signal to interpret. Any client that still falls back to inspecting
  `label` for opacity has a logic bug worth catching in review.

### Neutral

- The `:OntologyClass` tier (ADR-048) is unaffected. Ontology classes remain
  a public vocabulary tier; private ontology classes are out of scope.
- `:KGNode` IRI scheme diverges from the `vc:{domain}/{slug}` form used in
  ADR-048. This is intentional: `visionclaw:owner:{npub}/kg/…` is the sovereign
  form; the `vc:{domain}/{slug}` form continues to address public-canon nodes.
  A node can carry both IRIs when it is published (ADR-051 handles the
  transition).

## Compliance criteria

- [ ] `Visibility` enum defined in `src/models/node.rs`
- [ ] `owner_pubkey`, `opaque_id`, `pod_url` fields added to `KGNode`
- [ ] Visibility filter applied in `ClientCoordinator::broadcast_with_filter`
  (post-ADR-061). The historical `PRIVATE_OPAQUE_FLAG` constant in
  `src/utils/binary_protocol.rs` is removed.
- [ ] Three Neo4j indexes created (`kg_node_visibility`, `kg_node_owner`, `kg_node_opaque`)
- [ ] HMAC opaque_id with rotating session salt (24 hex chars, daily rotation, 48h dual-salt window)
- [ ] Canonical IRI uses `visionclaw:owner:{npub}/kg/{sha256(path)}` form
- [ ] Visibility enforced at the broadcast boundary (post-ADR-061);
  legacy bit-29 on `node_id` is no longer serialised
- [ ] `COALESCE(n.visibility, 'public')` fallback applied in all read queries touching legacy rows

## Rollback

- All new fields are nullable; rolling back code to pre-050 leaves rows intact
  but unused. No destructive migration.
- Indexes can be dropped idempotently:
  `DROP INDEX kg_node_visibility IF EXISTS;` (repeat for `kg_node_owner`,
  `kg_node_opaque`).
- The broadcast-boundary filter (post-ADR-061) is the rollback target;
  removing it restores pre-050 unfiltered behaviour. The historical
  `PRIVATE_OPAQUE_FLAG` constant has been removed entirely under ADR-061.

## References

- `src/models/node.rs` — `KGNode` struct and `Visibility` enum
- `src/utils/binary_protocol.rs` — `encode_position_frame` (post-ADR-061;
  no flag bits, raw u32 ids); see [docs/binary-protocol.md](../binary-protocol.md)
- `docs/reference/neo4j-schema-unified.md` — master schema reference
- Commit `89c8d800e` — `refactor/kg-node-rename`
