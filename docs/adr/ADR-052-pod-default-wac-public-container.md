# ADR-052: Pod Default WAC + Public Container Model

## Status

Ratified 2026-04-19

## Context

VisionClaw provisions a Solid Pod for every user as the write-master for their
data sovereignty. Today the provisioning path in
`src/handlers/solid_proxy_handler.rs:511-530` writes a root ACL that grants
`foaf:Agent` public read across the entire Pod. This is catastrophic for the
sovereign-private-node model: every newly-provisioned Pod leaks all its
contents to the open web by default, and the user must notice and tighten
the ACL themselves to prevent it.

The sovereign-mesh thesis (ADR-048, ADR-040) demands the opposite default:

- **Private by default.** New Pods start fully locked; only the owner can
  read.
- **A canonical public publishing surface.** Users need an explicit,
  predictable place to put content they intend to publish, not a whole-Pod
  open flag that they toggle.
- **Separation of write-master from performance layer.** The Pod is the
  source of truth for content. The backend graph is the performance layer
  that indexes and serves it. Publishing must be a first-class Pod-level
  operation, not a backend-side flag that the Pod mirrors.
- **Federation surface.** The public publishing container must be shaped
  to plug into Solid-native federation (ActivityPub, RSS, FOAF,
  webmentions) without further restructuring.

The current permissive default is also incompatible with corporate
deployments, where a leaking default would fail procurement on first audit.

## Decision Drivers

- Sovereign-private-node is meaningless if the default is public.
- The public surface must be Solid-native, not a VisionClaw-proprietary
  convention, so that users retain publishing capability if VisionClaw
  goes away.
- Canonical URIs must live on the Pod, not the backend — the user's Pod
  is the long-lived identity; the backend is an indexer.
- Write enforcement must be redundant: a single flag or a single path
  check is too easy to bypass with a broken client. Double-gate the
  published corpus.
- Cache coherence between Pod (write-master) and backend (indexer) must
  survive third-party Solid clients writing to the Pod without going
  through the VisionClaw backend.

## Considered Options

### Option 1: 3+1 container layout with double-gated write enforcement (chosen)

Pods provision default-private with the following structure:

```
/
├── .acl                     owner-only (no foaf:Agent grants)
├── private/
│   ├── .acl                 owner-only, acl:default inherited
│   ├── kg/                  every non-public KG page (full content)
│   ├── config/              GitHub token, prefs (github-owner, repo,
│   │                        branch, base_paths)
│   └── bridges/             draft BRIDGE_TO proposals pre-review
├── public/
│   ├── .acl                 foaf:Agent Read + owner Write/Control,
│   │                        acl:default inherited
│   └── kg/                  only pages carrying public:: true
├── shared/
│   └── .acl                 owner-only for now; future home for
│                            named-group read ACLs (judgment brokers)
└── profile/
    ├── .acl                 public-read card, owner-write
    └── card                 WebID doc claiming Nostr pubkey per NIP-39
```

Write enforcement is double-gated at the backend proxy:

1. **Page flag**: the source KG page must carry `public:: true` on a
   line anchored to the page header.
2. **Container path**: the target Pod path must be under `/public/`.

Both conditions true → write allowed. Either false → 403.

Canonical URI for a published page is its Pod URI, e.g.,
`https://alice.pods.visionclaw.org/public/kg/smart-contract`. Backend
HTML responses emit `<link rel="canonical" href="{pod_uri}">` and
backend URL variants 302 to the canonical Pod URI.

Cache coherence is driven by Solid Notifications (WebSocketChannel2023)
as primary. Third-party Solid clients writing to the Pod trigger
CREATE/UPDATE/DELETE notifications that invalidate the backend's cache
for the affected resources. Polling is a fallback for relays or Pod
providers that do not implement the notification protocol.

Profile card under `/profile/card` is seeded at provisioning time with
a NIP-39 Nostr pubkey claim, making the WebID cryptographically linkable
to the user's Nostr identity (per ADR-040).

Feature flag: `POD_DEFAULT_PRIVATE=true|false`, default `false` for
rollout safety. Existing permissive Pods are migrated idempotently on
backend startup when the flag is flipped true.

- **Pros**: Private by default. Solid-native `./public/` enables
  ActivityPub/RSS/FOAF/webmentions as a free upgrade path. Walk-away
  scenario preserved — Pods continue serving `./public/` on user infra
  if VisionClaw dies. Double-gate makes accidental publication of
  corporate-private data implausible. Publish/unpublish is a symmetric
  Pod MOVE between containers. ACL inheritance via `acl:default` keeps
  the ACL surface small and auditable.
- **Cons**: Existing dev Pods need migration, and the migration must be
  idempotent to survive reruns. Cache invalidation is now a distributed
  concern (Pod → backend). Clients writing to the Pod via third-party
  Solid apps can bypass the backend's double-gate, though the Pod-side
  ACL still prevents ACL-breaking writes; the failure mode is
  "inconsistent cache", not "leaked data".

### Option 2: Single `visibility` property on each resource, no container split

Keep the Pod flat. Use a per-resource `visibility: public | private`
RDF property, enforced by a custom Solid server extension.

- **Pros**: No migration. No directory juggling. One mental model for
  users ("flip the flag").
- **Cons**: Requires custom Solid server behaviour — not portable,
  violates the walk-away guarantee. No natural federation surface; RSS
  and ActivityPub expect containers, not tagged resources. Single-flag
  failure mode leaks everything. Visibility can desync from ACL.

### Option 3: Two Pods per user — "pod-private" and "pod-public"

Provision two Solid Pods per user, one locked down and one open.

- **Pros**: Absolute physical separation. No double-gate logic needed.
- **Cons**: Doubles per-user storage overhead. Two WebIDs to manage,
  two profile cards, two sets of preferences. Moving content between
  Pods is cross-Pod and slow. Enterprise procurement will question
  why provisioning is 2× what peers provision.

## Decision

**Option 1: 3+1 container layout with double-gated write enforcement,
gated by `POD_DEFAULT_PRIVATE` feature flag.**

Concrete provisioning output for a new Pod:

- Root `.acl`: zero `foaf:Agent` grants. Owner read/write/control only.
- `./private/.acl`: owner-only, `acl:default` so all children inherit.
- `./private/kg/`, `./private/config/`, `./private/bridges/` created
  empty.
- `./public/.acl`: `foaf:Agent` Read + owner Write/Control, `acl:default`.
- `./public/kg/` created empty.
- `./shared/.acl`: owner-only. Placeholder for Wave 2 named-group ACLs
  (judgment brokers per ADR-041).
- `./profile/.acl`: public-read card.
- `./profile/card`: WebID doc with `nostr:pubkey` claim (NIP-39).

Double-gate enforcement lives in the `solid_proxy_handler` write path.
The page-flag check reads the `public::` header line from the inbound
document. The path check matches the target against the `/public/` prefix.
Both must pass; neither gate is sufficient alone.

Canonical URI resolution: backend HTML responses emit
`<link rel="canonical" href="{pod_uri}">` for published pages, and
requests to the backend-hosted URL 302 to the Pod URI. The Pod is
authoritative.

Cache coherence: the backend subscribes to Solid Notifications on each
user's `./public/kg/` container. CREATE/UPDATE/DELETE events invalidate
the backend's cache for the affected resource. For Pod providers without
notification support, a polling interval of 60 s is used as fallback.

Migration for existing Pods runs idempotently on backend startup when
`POD_DEFAULT_PRIVATE=true`:

1. Enumerate each user's Pod root `.acl`.
2. If `foaf:Agent` Read is granted at root, replace with the locked
   layout above. Move existing content into `./public/` or `./private/`
   per the page-flag heuristic.
3. If the Pod is already in the new layout, skip.
4. Record migration completion in the backend's user record.

Opening a migrated Pod's ACL back up is not automated; it requires an
admin CLI invocation. This friction is deliberate — the whole point of
the migration is to stop Pods being open by accident.

## Consequences

### Positive

- Privacy is the default — sovereign-real, not sovereign-marketing.
- The Solid-native `./public/` pattern is a standard. ActivityPub
  outbox, RSS feed, FOAF `:knows`, and webmention endpoint can all be
  added later by writing into the `./public/` subtree, with no Pod
  restructuring.
- The walk-away scenario is preserved. A user whose VisionClaw instance
  is decommissioned still has a working `./public/` container serving
  their Pod URIs; they keep publishing on their own infrastructure.
- Corporate-private data is double-gated. Leaking a private page now
  requires both (a) a user marking it `public:: true` and (b) a client
  writing to `./public/`. Neither alone is sufficient.
- Publish and unpublish become symmetric Pod-level MOVE operations. No
  secondary "unpublish" endpoint is needed; move out of `./public/`
  and the backend's Solid Notification subscription invalidates the
  cache.
- WebID ↔ Nostr linkage via the NIP-39 claim in `./profile/card` gives
  agents a canonical way to resolve a Solid actor to a Nostr pubkey
  without a lookup service.

### Negative

- Staleness risk. A Pod written by a third-party Solid client that
  bypasses the backend still triggers Solid Notifications (if the
  provider implements them) but will be stale in the backend cache
  between notification and invalidation. Mitigation: 60 s polling
  fallback for providers without notification support; cache TTL
  capped accordingly.
- Cache invalidation is distributed. The backend's cache is now
  downstream of the Pod's notification stream, which is downstream of
  the Pod provider's implementation. Failure modes multiply.
  Mitigation: explicit notification health checks per user, surfaced
  in the admin dashboard.
- Existing dev Pods need the migration job to run. Mitigation:
  idempotent migration on startup; re-runs are no-ops.
- Opening an ACL back up requires admin CLI, which is deliberate
  friction but may confuse users who want to revert. Mitigation:
  documented recovery procedure; CLI command `visionclaw pod open-acl
  --user <id> --confirm`.

### Neutral

- `./shared/` starts empty and owner-only. Wave 2 will populate named
  groups (e.g., judgment brokers per ADR-041) without structural
  changes.
- `./private/config/` holds sensitive items like the user's GitHub
  token and `github-owner`, `repo`, `branch`, and `base_paths` prefs.
  These were previously implicitly private; the new layout makes the
  guarantee structural.
- `./private/bridges/` is where draft `BRIDGE_TO` proposals (ADR-048)
  live pre-review. Moving an approved bridge out of `./private/
  bridges/` is the final step of the migration candidate approval flow
  (ADR-049).

## Compliance Criteria

- [ ] Root `.acl` of a newly-provisioned Pod has zero `foaf:Agent`
      grants.
- [ ] `./private/kg/`, `./private/config/`, `./private/bridges/` exist
      with owner-only ACL and `acl:default` inheritance.
- [ ] `./public/kg/` exists with `foaf:Agent` Read and owner
      Write/Control, plus `acl:default`.
- [ ] `./shared/` exists with owner-only ACL (placeholder).
- [ ] `./profile/card` exists with a NIP-39 Nostr pubkey claim.
- [ ] Double-gated write: writes with `public:: true` and target under
      `/public/` succeed; writes missing either condition return 403.
- [ ] Existing Pods are migrated idempotently on backend startup when
      `POD_DEFAULT_PRIVATE=true`.
- [ ] Anonymous GET to Pod root returns 401.
- [ ] Anonymous GET to `./public/` returns 200.
- [ ] `tests/pod_provisioning_sovereign.rs` passes the full provisioning
      and migration matrix.

## Rollback

- Set `POD_DEFAULT_PRIVATE=false`. Newly-provisioned Pods revert to the
  existing permissive ACL. Effective immediately.
- For Pods already migrated under `POD_DEFAULT_PRIVATE=true`, re-opening
  the ACL is not automated. It requires an explicit admin CLI
  invocation: `visionclaw pod open-acl --user <id> --confirm`. This
  friction is deliberate: accidental ACL-opening is the failure mode
  we are removing, and we do not want a flag flip to undo that
  property across thousands of user Pods.
- A full revert of this ADR requires reverting the provisioning code
  change; the `POD_DEFAULT_PRIVATE` flag alone is sufficient for new
  Pods.

## Related Decisions

- ADR-011: Universal Authentication Enforcement — the on-wire auth
  primitive (NIP-98) this ADR's Pod-level ACLs complement.
- ADR-027: Pod-Backed Graph Views — establishes the Pod as the
  write-master and the backend as the performance layer.
- ADR-028-ext: NIP-98 Optional-Auth Extension — paired Wave 1 decision;
  backend-side read-path enforcement of the visibility tiers that
  this ADR enforces at the Pod ACL layer.
- ADR-040: Enterprise Identity Strategy — the NIP-39 WebID ↔ Nostr
  linkage in `./profile/card` is the bridge between OIDC-derived
  ephemeral keys and Pod-resident identity.
- ADR-048: Dual-Tier Identity Model — the `./private/bridges/` and
  `./public/kg/` structure maps directly onto the KG-vs-ontology tier
  distinction. Draft bridges live private; approved public pages live
  under `./public/kg/`.
- ADR-049: Insight Migration Broker Workflow — the final step of the
  migration candidate approval flow is a MOVE out of
  `./private/bridges/` into the ontology PR stream.

## References

- `src/handlers/solid_proxy_handler.rs:511-530` — current permissive
  provisioning path (to be replaced)
- `tests/pod_provisioning_sovereign.rs` — provisioning and migration
  matrix
- Solid Protocol §4 Access Control (WAC)
- Solid Notifications Protocol (WebSocketChannel2023)
- NIP-39 External Identities in Profiles
