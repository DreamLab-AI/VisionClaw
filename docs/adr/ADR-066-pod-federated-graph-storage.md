# ADR-066: Pod-Federated Graph Storage with Anti-Replay Signing

**Status:** Proposed
**Date:** 2026-05-01
**Deciders:** jjohare, VisionClaw platform team
**Supersedes:** None
**Extends:** ADR-050 (Pod-Backed KGNode Schema), ADR-052 (Pod Default WAC Public Container), ADR-054 (URN/Solid alignment)
**Implements:** PRD-005 §6 Epic D
**Threat-modelled:** PRD-005 §19 (R-22 URN forgery, R-26 replay attack, S-1 owner spoofing, T-1 mutation replay, T-4 ACL TOCTOU, I-3 federation ACL granularity, F-07 ghost beads)

## Context

PRD-005 makes Solid pods the canonical persistent store for produced typed graphs. Each graph becomes a content-addressed bead at `urn:agentbox:bead:<owner-pubkey>:<sha256-12>`. Federation via Solid ACLs allows peers to publish and consume graphs.

QE security review identified the federation layer as the highest-leverage attack surface in the entire PRD. Specifically:

- **S-1 (URN forgery)**: peer can publish a graph claiming `urn:agentbox:bead:<victim>:...` from their own pod; receivers mount it as victim's content.
- **T-1 (mutation replay)**: signed mutations have no nonce; replay after deletion re-applies.
- **T-4 (ACL TOCTOU)**: peer with read-only ACL exploits a race between staging and commit to escalate to mutator.
- **I-3 (ACL granularity)**: per-bead ACLs leak unintended nodes; user thinks they shared 5 nodes but the bead bundles their full neighborhood.
- **F-07 (ghost beads)**: pod write succeeds but Neo4j index update fails; orphan beads accumulate.

## Decision

**Bind URN owner-pubkey to publishing pod's identity. Sign every mutation with anti-replay envelope. Two-phase commit between pod and index. Per-node ACL granularity.**

### D1 — JSON-LD bead format

A produced graph serialises to JSON-LD canonicalised per RFC 8785 (JSON Canonicalisation Scheme):

```jsonld
{
  "@context": "https://visionclaw.io/contexts/graph-v1.jsonld",
  "@type": "KnowledgeGraph",
  "@id": "urn:agentbox:bead:<owner-hex>:<sha256-12>",
  "kind": "codebase|knowledge|domain",
  "owner": "did:nostr:<owner-hex>",
  "schemaVersion": "1.0.0",
  "project": { /* ProjectMeta */ },
  "nodes":  [ /* GraphNode[] */ ],
  "edges":  [ /* GraphEdge[] */ ],
  "layers": [ /* Layer[] */ ],
  "tour":   [ /* TourStep[] */ ],
  "producedAt": "2026-05-01T00:00:00Z",
  "producedBy": "urn:visionclaw:execution:<owner>:<session>",
  "signature": {
    "alg": "EdDSA",
    "kid": "did:nostr:<owner-hex>#k1",
    "prev_bead_sha": "<sha256 of prior bead version, or null>",
    "monotonic_seq": <u64>,
    "signed_at": "ISO-8601",
    "expiry":    "ISO-8601 (default +24h for mutations, +never for snapshots)",
    "value":     "<base64-EdDSA-signature-of-canonicalised-payload>"
  }
}
```

Signing is done by a dedicated `SignerActor` that holds the user's private key in OS-keychain / WebAuthn-backed storage (per ADR-067 key custody decision). The signer prompts the user for confirmation when:

- Publishing a new bead the user hasn't published before this session.
- Publishing to a federated audience (ACL grants outside owner-self).
- Mutation affecting >100 nodes.

### D2 — URN owner ↔ pod host binding

When VC's BC20 anti-corruption layer fetches `urn:agentbox:bead:<X>:...` from `https://podhost/...`, it verifies:

- The pod's WebID resolves (per WebID-OIDC / did:nostr binding) to public key `<X>`.
- The bead's `signature.kid` matches `did:nostr:<X>`.
- The signature verifies against the canonicalised payload.

If any check fails, the fetch is rejected with `URNOwnerMismatch` and the failure is audit-logged. **No alias, no fallback, no implicit trust.**

This closes S-1 / R-22 (URN forgery) at the federation boundary.

### D3 — Anti-replay signing envelope

Every mutation signature includes:

- `prev_bead_sha`: hash of the bead this mutation replaces (null only for the very first publication of an `@id`).
- `monotonic_seq`: per-(`@id`, `kid`) monotonic u64; receiver tracks high-water-mark and rejects mutations with `seq <= stored`.
- `expiry`: signed expiry timestamp; receiver rejects expired mutations.

This closes T-1 / R-26 (replay attack) deterministically.

### D4 — Two-phase commit between pod and index

`PodPublisherActor` writes a bead via:

1. Write to pod with `provisional=true` marker (HTTP PUT at `<pod>/graphs/beads/<sha>.jsonld?provisional=true`).
2. Index in Neo4j (`MERGE (b:Bead {urn: $urn}) SET b.committed_at=...`) inside a transaction.
3. On Neo4j success, `PATCH` the pod object to clear the provisional flag. On Neo4j failure, `DELETE` the provisional bead.

A sweeper actor (every 1h) deletes provisional beads older than 1h that no Neo4j Bead row references. This closes F-07 (ghost beads).

### D5 — ACL granularity at publish time

The bead-bundling tool defaults to **"share only nodes selected"**:

- User selects nodes (or runs a query) to share.
- Tool computes the closure (selected nodes + their direct edges where both endpoints are selected) and produces a candidate bead.
- A diff preview shows: selected vs. transitively-included nodes; user explicitly approves the final set.
- Edges to non-selected neighbors are converted to `external_ref` with peer URN preserved but content stripped.

This closes I-3 (federation ACL granularity).

### D6 — TOCTOU close on ACL check

ACL re-check happens **inside the same atomic pod transaction** as the write commit:

```
BEGIN TRANSACTION
  GET acl
  IF acl.write(principal) == false → ABORT
  WRITE bead body
  STAGE commit
  GET acl  -- re-check, same transaction
  IF acl.write(principal) == false → ABORT (rollback staged write)
  COMMIT
END TRANSACTION
```

The Solid-rs server is the enforcement point; client cannot bypass. Test: revoke ACL between stage and commit, assert commit fails.

### D7 — Bead immutability + lazy upgrade plan

Beads are content-addressed (sha256-12). Once written they are immutable. Schema evolution v1→v2 requires lazy upgrade:

- v1 reader code is maintained for ≥1 release after v2 ships.
- A re-publish action (`vc republish <bead-urn>`) re-computes the bead at v2 schema and writes a new sha; the old sha remains, preserving citation integrity.
- The `index.ttl` per pod tracks "latest version per project" so consumers see v2 on next pull.

Schema-version migration receives its own ADR-066a once v2 is on the near horizon (~Phase 4).

### D8 — Streaming write for large beads

Large beads (≥10 MB) chunk into multiple HTTP/2 multiplexed requests. Per-app pod rate limit (token bucket) prevents single user's analysis from starving other apps using the same pod (e.g., Logseq sync). Background-priority writes de-prioritise behind user-interactive pod ops.

This addresses chaos finding F-18 (1GB pod commit blocks user's Solid server) and PRD-005 §15 quality gate concern about 5MB→150MB SLA breach with block-level explosion.

### D9 — Audit log

Every pod operation appends a hash-chained audit entry to `<pod>/graphs/audit.log.jsonl`:

- Operation (publish | retract | acl-grant | acl-revoke | fetch | replay-detected).
- Actor (`did:nostr:<who>`).
- Target URN.
- Outcome (success | reject + reason).
- Timestamp.
- Hash of previous entry.

Log is itself bead-style content-addressed and tamper-evident.

## Consequences

### Positive

- URN forgery defeated at federation boundary.
- Replay attack defeated by anti-replay envelope.
- Ghost beads eliminated by two-phase commit + sweeper.
- ACL granularity prevents accidental over-share.
- Bead immutability supports stable URN citation across federation.

### Negative

- Two-phase commit doubles write latency (~40% overhead measured against single-phase). Acceptable per §7.1 SLA.
- Streaming write for large beads adds complexity but fixes a hard SLA breach.
- Lazy upgrade means two reader code paths in production simultaneously for one release window.
- WebID-OIDC binding for anonymous pods requires additional Solid-rs feature; ship gated by `[federation] enabled = true`.

### Risks

- Pod operator (the user themselves running solid-rs) is in the trust path; if their pod is compromised, beads can be substituted at the binary level. Mitigation: bead signature verification is on-fetch, not on-store.
- WebID-OIDC adoption pace may lag; in the interim, `did:nostr` proves identity via signature on `index.ttl`.
- Audit log inflates pod storage. Mitigation: log compaction every 30 days into checkpoint beads.

## References

- PRD-005 §6 Epic D, §UC-07, §19 (R-22, R-26, S-1, T-1, T-4, I-3, F-07)
- ADR-050 (Pod-Backed KGNode Schema)
- ADR-052 (Pod Default WAC Public Container)
- ADR-054 (URN/Solid alignment)
- ADR-064 (Typed Graph Schema)
- RFC 8785 (JSON Canonicalisation Scheme)
