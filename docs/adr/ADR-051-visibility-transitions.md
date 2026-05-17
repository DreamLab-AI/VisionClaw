# ADR-051: Visibility Transitions — Publish / Unpublish Saga

## Status

Ratified

## Date

2026-04-19

## Related Documents

- ADR-050 — Pod-backed `:KGNode` schema (introduces the `visibility` field)
- ADR-048 — dual-tier identity model (KGNode vs OntologyClass)
- ADR-028-ext — NIP-98 auth extension (signs audit and transition events)
- ADR-034 — needle/bead provenance (audit-event patterns reused here)
- ADR-052 — WAC default-private container policy
- ADR-053 — solid-pod-rs sidecar (Pod storage backend)

## Context

ADR-050 added a `visibility` field to `:KGNode` and made the owner's Pod the
write-master for private content. Users will flip `public:: true` on and off
inside Logseq pages as their authoring intent changes. Each flip must drive a
coherent cross-system transition:

1. The Pod resource must **move** between the owner's private and public
   containers so that WAC ACL inheritance (ADR-052) matches the new intent.
2. The Neo4j `:KGNode` row must update `visibility`, clear or re-generate
   `opaque_id`, restore or redact `label`, and swap `pod_url` to the new
   location.
3. The backend cache and third-party caches must be invalidated; public URIs
   that no longer resolve must return `HTTP 410 Gone` (not 404) so downstream
   consumers recognise a deliberate retraction.
4. Clients already streaming the V5 binary protocol must receive an updated
   node record with bit 29 cleared (on publish) or set (on unpublish) so their
   rendering flips in lockstep.
5. The transition must be **auditable** — we emit a server-signed Nostr event
   recording the action, broadcasting to relays when configured.

Atomicity must be user-perceived: a publish either completes fully or leaves
the system in a clean state (the user sees a 500 and the source of truth — the
GitHub `public:: true` flag — stays authoritative for the next sync).

## Decision

Two symmetric sagas implement publish and unpublish. Each is a Pod-first-
Neo4j-second sequence with an audit-event tail.

### Publish saga (private → public)

1. **Validate**: the source page has `public:: true`. The ingest classifier
   re-checks this server-side; client-side flags are never trusted.
2. **Pod MOVE**: `PATCH` the Pod resource from
   `./private/kg/{slug}` to `./public/kg/{slug}`. The container-level ACL on
   `./public/kg/` applies by inheritance (ADR-052); the old owner-only resource
   ACL is discarded.
3. **Neo4j transaction**: atomically update the node:
   ```cypher
   MATCH (n:KGNode {canonical_iri: $iri})
   SET n.visibility = 'public',
       n.opaque_id  = NULL,
       n.label      = $real_label,
       n.pod_url    = $new_public_url
   ```
4. **Backend cache**: publish the new canonical Pod URI; emit
   `<link rel="canonical">` metadata for public reads.
5. **Binary V5 broadcast**: re-emit the node with bit 29 cleared. Clients swap
   their opacified rendering for the real label in the next frame.
6. **Server-Nostr audit event**: sign a kind `30300` event recording the
   publish action (owner pubkey, canonical IRI, old/new pod_url, timestamp).
   Broadcast to configured relays.

### Unpublish saga (public → private)

Mirror of the publish saga, with an additional cache signal on step 4:

- Pod MOVE from `./public/kg/{slug}` back to `./private/kg/{slug}`.
- Neo4j transaction sets `visibility = 'private'`, regenerates `opaque_id` per
  ADR-050, clears `label`, updates `pod_url`.
- **Step 4 addition**: emit `HTTP 410 Gone` for stale public URIs so third-
  party caches (CDN, HTTP mirrors, archival crawlers) recognise the retraction
  rather than treating it as a transient 404.
- Binary V5 broadcast: re-emit with bit 29 **set**.
- Audit event: kind `30300` with action `unpublish`.

### Atomicity model

**Pod-first, Neo4j-second** is chosen deliberately. If the Pod MOVE succeeds
but the Neo4j transaction fails, we record a **pending marker** on the node
and retry the Neo4j write. If the Pod MOVE itself fails, no Neo4j change
occurs; the user sees a 500 and the source of truth (GitHub `public:: true`
flag) remains the arbiter for the next sync attempt.

The Pod is the single source of truth for content. Neo4j is derived state and
can be re-projected from the Pod. This asymmetry justifies the ordering.

### Pending-marker recovery

A node with `visibility_pending: <direction>` on restart is reconciled:

- Re-query the Pod for the resource location.
- Apply the missing Neo4j update.
- Clear the pending marker.

Integration tests kill the process mid-saga and assert recovery completes.

### Cache invalidation

Solid Notifications (CREATE / UPDATE / DELETE) drive the backend cache
refresh. A 30-second default TTL provides a fallback upper bound so a missed
notification does not strand cached opacity indefinitely.

### Feature flag

`VISIBILITY_TRANSITIONS=true|false` gates the entire saga. When `false`, pages
stay in whichever container they were provisioned to; no data loss.

## Consequences

### Positive

- **Symmetric publish/unpublish**: the same code path in reverse. No special
  cases, no one-way operations.
- **Pod is single source of truth for content**; Neo4j is derived state and
  can always be re-projected.
- **Third-party caches receive `410 Gone` on unpublish**: user intent (content
  retraction) is honoured by the broader web, not just our surface.
- **Audit trail**: every transition emits a signed kind-30300 event, so the
  sequence of publish/unpublish decisions is cryptographically reconstructible.

### Negative

- **MOVE atomicity depends on the Pod server**. Both JSS and solid-pod-rs
  (ADR-053) support atomic `PATCH` MOVE; this is verified but binds our
  feature to the intersection of backend capabilities.
- **Brief inconsistency window during saga mid-flight** (< 100ms expected).
  Clients in the window may see a stale opacity state for one frame until the
  next binary protocol broadcast arrives (post-[ADR-061](ADR-061-binary-protocol-unification.md):
  visibility is enforced at the broadcast boundary by `ClientCoordinator::broadcast_with_filter`).

### Neutral

- Physics and layout are unaffected: the node ID does not change, only its
  visibility metadata. Positions and forces stay stable across transitions.
- The canonical IRI is stable across visibility changes; only `opaque_id`,
  `label`, and `pod_url` mutate.

## Compliance criteria

- [ ] Publish saga implemented end-to-end (validate → Pod MOVE → Neo4j flip → binary broadcast → audit event)
- [ ] Unpublish saga mirrors publish and emits `HTTP 410 Gone` for stale public URIs
- [ ] Pending markers persisted for crash recovery
- [ ] Integration test: process is killed mid-saga; recovery completes on restart
- [ ] Solid Notifications drive cache invalidation within the 30s TTL
- [ ] Server-Nostr kind `30300` audit events are signed and broadcast to configured relays
- [ ] `VISIBILITY_TRANSITIONS` feature flag honoured at the saga entry point

## Rollback

- `VISIBILITY_TRANSITIONS=false` disables the saga entry point; pages stay in
  whichever container they were provisioned to; no data loss, no half-moved
  resources.
- Audit events already broadcast cannot be retracted (Nostr events are
  immutable), but a rollback is itself a kind-30300 event with an explicit
  `rollback` tag if a reversal is required.

## References

- `src/sovereign/visibility.rs` — publish/unpublish saga implementation
- `src/pod/move.rs` — Pod PATCH MOVE adapter
- `src/audit/nostr_events.rs` — kind-30300 signer
- `docs/reference/neo4j-schema-unified.md` — Neo4j transaction boundaries
