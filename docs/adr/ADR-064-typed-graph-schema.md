# ADR-064: Typed Graph Schema (UA-Aligned, URN-Bound)

**Status:** Implementing
**Date:** 2026-05-01
**Implementation:** `crates/graph-cognition-core/` — NodeKind (21), EdgeKind (35), EdgeCategory (8), TypedGraph, 25 tests passing
**Deciders:** jjohare, VisionClaw platform team
**Supersedes:** None
**Extends:** ADR-050 (URN-Traced Operations), ADR-061 (Binary Protocol Unification)
**Implements:** PRD-005 §6 Epic A
**Threat-modelled:** PRD-005 §19 (R-22 URN forgery, T-3 alias rewrite escalation)

## Context

VisionClaw's current graph schema in Neo4j has ~6 node kinds (`Page`, `LinkedPage`, `OwlClass`, `OwlProperty`, `Agent`, `Bot`) and ~10 relationship types, encoded with flag bits (bits 26-31 of node ID) for fast GPU dispatch. This is sufficient for Logseq vault visualisation but not for code-cognition or ontology-reasoning workloads.

Understand-Anything (UA) ships a richer taxonomy — 21 node kinds (5 code + 8 infra + 3 domain + 5 knowledge) and 35 edge kinds organised into 8 categories (structural, behavioural, data-flow, dependencies, semantic, infrastructure, domain, knowledge). This taxonomy is well-tested across multi-language codebases and Karpathy-pattern wikis.

QE review (qe-quality-analyzer) flagged that PRD-005's bit-allocation footnote was wrong: only **2** free bits remain (`0x40000000`, `0x02000000`) in the existing flag word, not 5 as claimed. Adopting 21 new kinds requires an explicit bit-allocation amendment, not a casual extension. QE-security-auditor (T-3) flagged that auto-aliasing of unknown kinds is an attack surface when input arrives from federated origins.

## Decision

**Adopt UA's typed graph schema, encode it through `kind` enum + dedicated kind-id field, and constrain alias rewriting to authenticated origin paths.**

### D1 — Schema definition

`NodeKind` is a 21-variant Rust enum in `crates/graph-cognition-core::kinds` with `serde` and `FromStr`/`Display`. `EdgeKind` is a 35-variant enum grouped by 8 `EdgeCategory`s. Both compile to stable u8 ids for wire and storage. Adding a new variant is a major-version schema change requiring its own ADR.

### D2 — Identification path

Every typed node mints its URN via `src/uri/mint.rs::mint_typed_concept(owner, kind, local) -> Urn` of the form `urn:visionclaw:concept:<hex-pubkey>:<kind>:<local>`. The CI grep-gate (existing) is extended to assert no `GraphNode` is constructed without `URN::from_minted(...)`.

### D3 — Bit allocation amendment

Reserve a dedicated `kind_id: u8` field in the node side-table (HTTP `/api/graph/data`) for the 5-bit kind value. **Do not** pack the new kind into the existing 32-bit flag word. The flag word retains its current allocation: `0x80000000` agent, `0x40000000` knowledge, `0x1C000000` ontology subtype, plus future visibility/persona bits per ADR-061.

This avoids the bit-exhaustion risk identified by QE and keeps ADR-061's binary protocol untouched.

### D4 — Alias rewrite is provenance-gated

The alias-rewrite map (UA's `NODE_TYPE_ALIASES`) ports as `phf::Map<&'static str, NodeKind>`. **Rewrite is permitted only for input from authenticated local origins** (user's own `vc analyze` / Logseq sync). For input from federated peers (Solid pod pull, BC20 acl translation), unknown kinds are **rejected**, never aliased. Federated peers must mint canonical kinds at source.

Alias rewrites are audit-logged with `from`/`to`/`origin` for every rewrite. Closed-set check: any kind not in the alias map's value range is rejected.

### D5 — Migration path

Cypher migration `migrations/2026-05_typed-schema.cypher` runs idempotently:

1. Add `kind` property to all existing nodes via translation table (`Page → article`, `OwlClass → schema`, `Agent → agent`, etc.).
2. Add new labels alongside existing (`(:Page:Article)`).
3. Add indexes on `kind`, `(kind, owner_pubkey)`.
4. After ≥1 release of dual-label coexistence, drop legacy-only labels in cleanup migration ADR-064a.

Migration is wrapped in `apoc.periodic.iterate` with batch size 10,000 and parallel commit. Pre-migration snapshot is mandatory; post-migration verification is part of the same transaction.

Pre-existing 1.17M `memory_entries` rows preserve their URNs unchanged (verified by AC-A.2).

### D6 — Validation actor

`SchemaValidatorActor` consumes `GraphMutation` events. Rejects:
- Unknown kind from any origin (after alias step for local origins).
- Mutation lacking required fields per kind.
- Edges where source/target kind combination is undefined (`function calls table` is rejected).

Validation taxonomy is shared between Rust (this actor) and the JSON-LD ingest path of Epic D — single source of truth in `crates/graph-cognition-core/schema.rs`.

## Consequences

### Positive

- 35 edge kinds in 8 categories enable richer client-side filtering (Epic E.5) and per-category force tuning (Epic I).
- Federated graphs from peers cannot smuggle unknown kinds via aliasing (security hardening).
- URN-minting is uniform; no `Uuid::new_v4()` in the new code paths.
- Dedicated `kind_id: u8` field avoids ADR-061 binary-protocol disruption.

### Negative

- ~32-byte side-table grows by 1 byte per node (kind_id), ~30 KB additional for a 30k-block vault — bandwidth-trivial.
- Migration is irreversible after dual-label cleanup ADR-064a; rollback before that point is reversible.
- Federated peers running older VC versions (no canonical kind support) cannot share graphs until upgraded.

### Risks

- R-22 URN forgery: peer publishes `urn:visionclaw:concept:<victim-pubkey>:...` from their own pod. Mitigated by ADR-066's URN-owner-to-pod-host binding, not this ADR.
- Alias map drift: if UA upstream adds new aliases, our map lags. Mitigated by versioned alias map + nightly drift CI against UA fixtures.

## References

- PRD-005 §6 Epic A, §13.6 GPU memory layout, §19 (R-22, T-3)
- UA: `understand-anything-plugin/packages/core/src/types.ts` (canonical taxonomy)
- UA: `understand-anything-plugin/skills/understand-knowledge/merge-knowledge-graph.py` (alias maps, ported semantics)
- ADR-050 (URN-Traced Operations), ADR-061 (Binary Protocol)
