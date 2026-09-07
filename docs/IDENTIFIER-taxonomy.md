---
title: Identifier Taxonomy
doc_id: VC-IDENTIFIERS
version: 0.1.2
status: draft-for-ratification
verified_commit: 
changelog:
  - "0.1.2 (2026-09-06): Remediation — 2026-09-05 section: Wave 3 ADRs (2094–2101, 2061, 2071, 2085; proposed 2102–2105) and the ledger/diagram re-verification landed in 2cf222406 — re-verified at "
  - "0.1.1 — corrected presence_actor room-URN citation (test, not live emission), Invariant 6 debug_assert/release-truncation caveat, qualityScore CURIE-in-comment framing"
sources:
  - src/uri/mod.rs
  - src/utils/binary_protocol.rs
  - src/types/user_context.rs
  - src/services/nostr_identity_verifier.rs
  - src/handlers/enrichment_proposals_handler.rs
  - src/domain/broker/precedent_registry.rs
  - src/services/ontology_mutation_service.rs
  - docs/adr/ADR-074-D2-supersession-multikey-convergence.md
  - docs/adr/ADR-125-did-nostr-multikey-convergence.md
date: 2026-08-31
---

# Identifier Taxonomy

## Purpose

One ground-truth grammar for every identifier the VisionClaw substrate mints,
persists, resolves and renders — reconciling the four coexisting grammars
(semantic IRI, operational URN, sovereign identity, wire node-ID) against what
the code actually emits.

## Current state

There are **four** identifier planes in live code. They are distinct address
spaces, not competing conventions; each is emitted by different subsystems.

### 1. Operational URN — `urn:visionclaw:*` (the durable persistence identifier)

The converged minter is `src/uri/mod.rs` (`NS = "urn:visionclaw"`,
`src/uri/mod.rs:41`). It is fail-closed: every durable ID **must** go through a
typed constructor; ad-hoc `format!()` is prohibited (`src/uri/mod.rs:33-35`).
There is deliberately **no** `urn:visionclaw:agent` kind — an agent's identity
*is* its DID (`src/uri/mod.rs:26-27,51-52`).

| Kind | Grammar | Scope | Minter |
|------|---------|-------|--------|
| `concept` | `urn:visionclaw:concept:<domain>:<slug>` | domain-scoped shared ontology class | `concept()` `:229` |
| `kg` | `urn:visionclaw:kg:<hex-pubkey>:<sha256-12>` | owner-scoped, content-addressed KG node | `kg()` `:242`, `kg_with_address()` `:255` |
| `bead` | `urn:visionclaw:bead:<hex-pubkey>:<sha256-12>` | owner-scoped, content-addressed | `bead()` `:265`, `bead_with_address()` `:284` |
| `execution` | `urn:visionclaw:execution:<sha256-12>` | **unscoped** — owner travels in `owner_did` | `execution()` `:293` |
| `group` | `urn:visionclaw:group:<team>#members` | team-scoped membership ref | `group_members()` `:298` |
| `room` | `urn:visionclaw:room:<sha256-12>` | unscoped XR presence room | `room()` `:308` |
| `avatar` | `urn:visionclaw:avatar:<hex-pubkey>` | identity-bound 1:1 with a DID | `avatar()` `:313` |

Content addressing is `sha256-12-<12 lowercase hex>` — the first 6 bytes of a
SHA-256 digest, byte-identical to the agentbox `sha12()` helper
(`content_address()` `:179`; verified against the hand-computed vector
`sha256-12-b94d27b9934d` for `"hello world"`, `:582`). Owner scope is always the
**64-char lowercase-hex BIP-340 x-only pubkey, never bech32 npub**
(`is_pubkey_hex()` `:136-140`; `src/uri/mod.rs:31`).

Live emission sites confirm the grammar in production, not just tests:
`execution:<activity_kind>-<proposal_id>` in `ontology_mutation_service.rs:118`;
`urn:visionclaw:execution:*` and `urn:visionclaw:kg:<pk>:*` in
`enrichment_proposals_handler.rs:89-91,697-702`;
`urn:visionclaw:concept:bc:*` in `precedent_registry.rs:47-50`.
The `urn:visionclaw:room:*` grammar is exercised by `RoomId::parse` but only
under test at `presence_actor.rs:637` (inside `#[cfg(test)] mod tests`, opened
at `:582`); it is not a production emission site.

### 2. Semantic IRI — `vc:{domain}/{slug}` (RDF display / JSON-LD)

The `vc:` CURIE prefix is a **presentation form for RDF triples**, not a minted
persistence key. In code it appears only as property/predicate CURIEs
(`vc:referencedBy` in `elevation_actor.rs:510`; the `vc:qualityScore`
provenance is noted in a source comment at `client_filter.rs:61`, which
consumes the expanded `qualityScore` key — the CURIE itself is not emitted as
a literal there), i.e. as the JSON-LD `@context` expansion of the
ontology namespace. The `{domain}/{slug}` *subject* form of legacy ADR-100 is
**not independently minted** — the durable subject is the `urn:visionclaw:concept`
above, and `vc:{domain}/{slug}` is its human/RDF rendering. Legacy `urn:ngm:graph:*`
named graphs remain the persistence named-graph IRIs and are not rewritten
(`src/uri/mod.rs:464-466`).

**Reconciliation with the §1 kind table (2026-09-05, ADR-2105 pass).** The two
sections describe different planes and must not be read as competing kind lists.
**No row of the §1 kind table mints `vc:{domain}/{slug}`, and none ever will**: §1
enumerates the seven typed constructors of the durable persistence plane, and the
only concept-bearing member is `concept` →
`urn:visionclaw:concept:<domain>:<slug>`. §2 is a *rendering* plane, not a minting
one — a `vc:` CURIE is produced by JSON-LD `@context` expansion at presentation
time and is never allocated, stored, compared or joined on. Operationally: a `vc:`
CURIE must never appear in a persisted subject position, a database column, a wire
identifier or a cross-store join key; where one is observed there, the durable
`urn:visionclaw:concept` is the identifier and the CURIE is its rendering. The
`{domain}/{slug}` *subject* grammar of legacy ADR-100 is superseded by this
document, which is why it has no kind-table row rather than a row marked
unimplemented. Raised by diagram note VC-23.9
(`docs/diagrams/visionclaw/23-identifiers-urn-did-sha12.md` in the VisionFlow estate tree (see `docs/diagrams/README.md`)), which read the
absence of a row as an unreconciled gap.

### 3. Sovereign identity — `did:nostr:<hex-pubkey>` + display npub

Identity is a DID, minted by `did_nostr()` (`src/uri/mod.rs:220`), prefix
`did:nostr:` (`:47`). The DID **body is the 64-char x-only hex pubkey**; this is
the canonical, comparison-stable identity. The verifier reconstructs the DID
solely from the verified event pubkey and never trusts a claimed field
(`nostr_identity_verifier.rs:73-105`). `did:nostr` carries no `Kind`
(`src/uri/mod.rs:307`).

Display encoding: the bech32 `npub1…` form is a **UI-only rendering** of the
same key (`user_context.rs:16`). Grammar rule: **hex is canonical everywhere in
storage/URN/DID; npub is display-only**. The two are never mixed in a persisted
identifier.

The DID **document** (not the DID string) is governed by the Multikey form
below — see Known divergences for the ADR-074-D2'/125 reconciliation.

### 4. Wire node-ID — sequential `u32` with type flag bits (live protocol)

Live nodes on Protocol V3 (52B/node) carry a compact `u32` ID, not a URN.
Layout (`src/utils/binary_protocol.rs:14-26`):

| Bits | Mask/flag | Meaning |
|------|-----------|---------|
| 31 | `AGENT_NODE_FLAG = 0x80000000` | agent node |
| 30 | `KNOWLEDGE_NODE_FLAG = 0x40000000` | knowledge node |
| 26–28 | `ONTOLOGY_TYPE_MASK = 0x1C000000` | ontology subtype (only when `GraphType::Ontology`): `0x04000000` Class, `0x08000000` Individual, `0x10000000` Property (`:20-22`) |
| 0–25 | `NODE_ID_MASK = 0x03FFFFFF` | the ID, 0 … 67,108,863 (2²⁶−1) |

IDs are sequential `u32` from a `NEXT_NODE_ID` atomic counter. Every encoder
branch routes through `enforce_wire_id_bounds` (`:167-188`), which masks the ID
into wire range via `remap_wire_id` (`:199-201`) and, on overflow, emits an
`error!` naming the class and both IDs. This holds in **all** builds, the
untyped fallback branch included (`:445`); the retained
`debug_assert!(node_id <= NODE_ID_MASK)` (`:168-175`) is a development aid that
fails fast at the offending call site, not the bound itself — ADR-2024,
ADR-2070. An over-range ID is still masked (and therefore aliases another node),
but it is never silent. Decode strips via `& NODE_ID_MASK` (`:227-229`). This is
an **ephemeral render-plane ID** — it maps to a durable `urn:visionclaw:kg:*` in
the graph store, not the reverse.

### Cross-substrate mapping (agentbox → VisionClaw)

`cross_from_agentbox()` (`src/uri/mod.rs:672-719`) is the federation boundary
translator — the counterpart of agentbox `bc20-provenance-bridge.js::toVisionclaw`.
Closed kind map:

| agentbox source | VisionClaw target | Note |
|-----------------|-------------------|------|
| `did:nostr:<pk>` | `did:nostr:<pk>` | already converged, passes through (`:674-683`) |
| `urn:agentbox:agent:<pk>:*` | `did:nostr:<pk>` | identity is the DID (`:692-695`) |
| `urn:agentbox:activity:*` | `urn:visionclaw:execution:<sha256-12>` | unscoped (`:696`) |
| `urn:agentbox:thing:<pk>:*` | `urn:visionclaw:kg:<pk>:<sha256-12>` | owner-scoped (`:697-700`) |
| `urn:agentbox:bead:<pk>:<sha256-12>` | `urn:visionclaw:bead:<pk>:<sha256-12>` | structural crossing — both grammars are already `<pubkey>:<sha256-12>`, so the existing content address is preserved rather than re-hashed (`:701-712`, `bead_with_address()` `:284`). Added by ADR-2072; agentbox ADR-2061 holds the cross-repo parity test |
| `urn:agentbox:memory:*` | *(none — returns `None`)* | needs `{domain,slug}` elevation absent on hot path (`:713`) |
| any other kind | *(none)* | closed-map discipline (`:713`) |

### Dual-read legacy resolution

`parse()` (`:509`) is converged-only and **rejects** `urn:ngm:*`. `parse_dual()`
(`:619`) additionally resolves persisted legacy `urn:ngm:node|edge|domain|graph:*`
IDs as `ParsedUri::LegacyNgm`, carried opaquely so old IDs keep resolving without
re-minting. Every resolve/lookup surface must call `parse_dual`; every mint/validate
surface must call `parse` (legacy ADR-105).

## Known divergences & open items

- **DID-document form: ADR-074-D2' vs ADR-125.** Resolved, not a conflict in
  code. **Canonical single form = ADR-074-D2' §1** (`ADR-074-D2-...md:33-54`): a
  two-context (`w3id.org/did` + `w3id.org/nostr/context`) document with a top-level
  `DIDNostr` type, a single `Multikey` verificationMethod
  (`publicKeyMultibase = "fe70102" + hex`, id `#key1`), and `service: []`. ADR-125 is
  the per-repo enactment and remains in force; 074-D2' is the lead-architect statement
  that governs on conflict (`ADR-074-D2-...md:9`). The DID **string** body is I1-stable
  (byte-identical hex before/after — `:142-144`). The **2019 shape**
  (`SchnorrSecp256k1VerificationKey2019` + `publicKeyHex`, `secp256k1-2019/v1` context,
  populated default `service`) is now **invalid** (`:64-69,170`). Emitter drift is open:
  agentbox `s04-did.js` and forum `did-v1.jsonld` still emit the 2019 shape and must be
  rewritten (`:197,204,247`). `solid-pod-rs` is at target.
- **`vc:{domain}/{slug}` is not independently minted.** Legacy ADR-100 describes it as
  a subject grammar; live code emits it only as an RDF predicate CURIE. The durable
  subject is `urn:visionclaw:concept`. Flag: reconcile ADR-100 prose to
  "presentation of the concept URN".
- **Owner-scoped sovereign form `visionclaw:owner:{npub}/kg/...` (legacy ADR-050) is
  not emitted.** No occurrence in `src/` or `crates/`. Owner scoping is carried by the
  hex pubkey segment of `urn:visionclaw:kg:*`, not by an npub path. ADR-050 grammar is
  superseded by the URN grammar in code.
- **Minted URNs may return null (legacy ADR-063).** `cross_from_agentbox` returns
  `None` for `memory` and unknown kinds (`:711-713`); callers must record the raw
  string + unmapped marker rather than a synthetic ID. `bead` no longer falls into
  this arm — **Resolved — ADR-2072 (2026-09-05)** — it now crosses structurally
  (see the cross-substrate mapping table above).
- **Invariant 7 (`parse` rejects `urn:ngm:*`, `parse_dual` accepts it) is deliberate, not drift — DELIBERATE (2026-09-05, agentbox ADR-2061).** It is a mint-vs-resolve split: `parse` refuses the retired namespace so no *new* durable ID can be minted under it, while `parse_dual` accepts it so IDs persisted before the ADR-105 cutover keep resolving. The two are meant to disagree, and collapsing them in either direction would either strand legacy IDs or re-open minting. Same review pass: agentbox ADR-2061 made the cross-substrate kind list a shared artefact `cross_from_agentbox` now derives from (`include_str!("../../agentbox/schema/federation-kinds.json")`), which shifted every `src/uri/mod.rs` citation in this document by roughly +125 lines — the mapping table above and the `:711-713` / `:650` references are stale by offset, not by content, pending a re-derivation pass by the owning lead.
- **npub vs hex mixing risk.** `user_context` stores npub as the "primary user
  identifier" (`user_context.rs:16`) while the URN/DID layer is hex-canonical. No
  automatic conversion is enforced at that boundary — audit that npub never leaks into
  a persisted URN.
- Identifier-grammar reconciliation across legacy ADR-100/105/050/053/063 is recorded
  as an open governance item; this document supersedes their prose where code diverges.

## Invariants (must not silently change)

1. **Hex is canonical; npub is display-only.** No persisted URN, DID or ACL entry ever
   contains a bech32 npub.
2. **Durable IDs are minted only through `src/uri/mod.rs` typed constructors.** No
   ad-hoc `format!()` for `urn:visionclaw:*` (`:33-35`).
3. **Content address = `sha256-12-` + 12 lowercase hex** (6 bytes), byte-identical to
   agentbox `sha12()`. Changing the truncation length breaks cross-substrate joins.
4. **`did:nostr` body is byte-stable** across DID-document format changes (I1).
5. **No `urn:visionclaw:agent` kind** — identity is the DID.
6. **Wire node-ID is 26-bit** (`NODE_ID_MASK`); bits 26–31 are reserved for type flags.
   Exceeding 2²⁶−1 is caught only by `debug_assert!` (`:118-123`) and so panics in
   debug builds only; release builds silently truncate via the always-applied
   `& NODE_ID_MASK` (`:125,136`). **Open hardening item:** promote the guard to a
   runtime check (`assert!`/`Result`) so over-range IDs cannot silently corrupt in
   release. Callers must never rely on IDs above `NODE_ID_MASK`.
7. **`parse` rejects `urn:ngm:*`; `parse_dual` accepts it.** Mint paths use `parse`,
   resolve paths use `parse_dual`.

## Change process

This is a living document. Amend it in the same PR that changes any identifier
emitter or the wire node-ID layout, and update `verified_commit`. Triggers requiring
re-verification: a new `Kind` variant in `src/uri/mod.rs`; any change to
`NODE_ID_MASK` / flag-bit assignments in `binary_protocol.rs`; a DID-document emitter
change (per ADR-074-D2' `review_trigger`); or a new subsystem emitting a persisted
identifier. Ratification requires the change to hold all seven invariants or to
explicitly amend one here with rationale.

## Cross-repository acceptance qualification — 2026-09-04

The shared local fixture confirms sha256-12 output for identical tested UTF-8 bytes, not arbitrary-object canonicalisation. Precomputed KG addresses are prefix-checked and accept malformed suffixes; complete grammar validation remains open. Rust and JS crossing maps differ on beads, so closed-map correctness does not imply equal coverage. ADR-2023/2025 retain their scoped decisions with these acceptance requirements. See the [estate identifier review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/federation-identifiers.md).

## Mint and identity closeout qualification — 2026-09-04

ADR-2021 is partial against universal constructor use: direct provenance formatting and legacy graph mint sites remain. ADR-2022's canonical naming must be distinguished from signed proof and role authority. [Source evidence and acceptance](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/federation-identifiers.md#mint-site-coverage-and-proof-of-identity) require mint/lookup/persistence coverage and explicit migration exceptions; no named graph or stored ID is authorised for rewrite by this review.

## Wire capacity closeout — 2026-09-04

ADR-2024's release overflow warning belongs to the typed flag setters. **Resolved since — ADR-2070 (2026-09-05):** the untyped branch no longer debug-asserts and passes the ID through unchanged; it calls the same `enforce_wire_id_bounds` as the five typed branches (`src/utils/binary_protocol.rs:445`), so all six log `error!` and mask in release. **Still open:** shared 26-bit masks do not prove allocator safety or generation consistency, and reconnect mapping evidence per class is still required; see [estate wire review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md#wire-identifier-overflow-coverage).

## Remediation — 2026-09-05

- **ADR-2070** — corrects the wire node-ID text: every encoder branch, the untyped fallback included,
  routes through `enforce_wire_id_bounds` and logs on overflow in all builds; `debug_assert!` is a
  development aid, not the bound. Also re-derives the `cross_from_agentbox` citation (`src/uri/mod.rs:650`).
- **ADR-2072** — `cross_from_agentbox` maps `bead`, closing the cross-substrate asymmetry with the
  agentbox BC20 bridge (agentbox ADR-2061 holds the cross-repo contract and its parity test).

- **ADR-2105** (proposed) — the agentbox authoring correlation id is a durable cross-repo
  identifier: the proposal, approval, merge and served-corpus stages accept, persist and echo it,
  and a request arriving without one is recorded `unlinked` rather than given a VisionClaw-side
  synthetic mint. agentbox ADR-2022's own "Remaining" section is the ORIGIN of this gap and is
  referenced (`see`), not superseded.
- **Doc-correct (2026-09-05)** — §2 now carries a dated reconciliation paragraph stating that no
  row of the §1 kind table mints `vc:{domain}/{slug}` and none will: §2 is a rendering plane, and
  a `vc:` CURIE must never occupy a persisted subject, column, wire or join position. Raised by
  diagram note VC-23.9.
- **ADR-2095 (2026-09-05)** — the legacy `urn:ngm:class:<slug>` scheme is minted through a typed
  constructor like every other durable identifier: `class_iri`/`parse_class_iri`/`CLASS_PREFIX` are
  defined in `crates/visionclaw-domain/src/uri.rs:15-56` (the domain crate, so the adapters crate
  can reach them) and re-exported from `uri::ngm` (`src/uri/mod.rs:351`). The five raw `format!`
  mints are gone — `src/actors/elevation_actor.rs:329,514` and
  `crates/visionclaw-adapters/src/oxigraph_ontology_repository.rs:174,1598,1619`. Emitted strings
  are unchanged. Raised by diagram note VC-23.9.
