---
id: ADR-2014
title: "GitHub `public:: true` gates KG inclusion; `owl:class::` bypasses it"
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: [ADR-2040]
verified_commit: e0f8cd896
owner: jjohare
review_trigger: "a source of authoritative data whose pages carry neither `public:: true` nor `owl:class::`, or a move to per-page ACLs at ingest"
repo: visionclaw
domain: DATA-authority-erasure
lineage: "legacy ADR-050 (Pod-backed :KGNode visibility), ADR-051 (publish/unpublish saga; GitHub `public:: true` as arbiter, re-checked server-side)"
---

# ADR-2014 — GitHub `public:: true` gates KG inclusion; `owl:class::` bypasses it

## Context

The working knowledge graph must surface only *published* authored pages, yet
formal ontology data must ingest wherever it lives in the repo. Two failure
modes were possible: leaking private working pages, or dropping authoritative
ontology pages that happen to sit outside an ontology directory. Anchoring the
gate on a source directory would get both wrong. Prior state (ADR-050/051) made
`public:: true` the publish arbiter, re-checked server-side.

## Decision

A plain markdown page (no `owl:class::`) becomes a KG node **only** if it
carries `public:: true`. A page whose first parsed node has an `owl_class_iri`
is treated as authoritative formal data and ingests **unconditionally**,
independent of publish tagging or directory. The gate anchors on the parsed
owl:class, not on the file path. This forecloses directory-based inclusion
rules and any implicit "everything in dir X is public" shortcut.

## Consequences

- Private working pages are excluded by default; absence of the marker means
  private (`logseq_page_is_public` returns false on absence).
- An ontology page misauthored without `owl:class::` will be gated as a plain
  page and silently dropped unless it also carries `public:: true`.
- No per-page ACL granularity: the gate is binary. Finer visibility (owner
  scope, groups) is out of scope and would need a new mechanism.
- `linked_page` wikilink stubs are dropped regardless; only authored nodes
  contribute, so a public page's private wikilink targets never materialise.

## Verification

Re-checked at `e0f8cd896`: `github_sync_service.rs` computes
`is_ontology` from `n.owl_class_iri.is_some()`, then the gate
`if !is_ontology && !logseq_page_is_public(content) { return; }`.
`logseq_page_is_public` case-insensitively matches a `public::` line and
returns false on absence. `file_service.rs` `is_public_file` enforces the same
`public-access:: true` / legacy `public:: true` rule on the file-service path.

## Closeout extension — 2026-09-04

CP-01/02/04/08. Owner remains jjohare with vault/knowledge/privacy maintainers. Historical implementation/activation declarations and the link to ADR-2040 are retained. Current readers use PageMeta, not the old anywhere-in-file public-line scanner. Formal-class inclusion still bypasses explicit public false; local fallback metadata collection is distinct from the GitHub gate. This record must not be read as a current all-path privacy certification.

**Acceptance condition:** Define and enforce class-marker type/IRI policy and make public-false-plus-class semantics explicit. Exercise each ingest/fallback path with private, malformed, quoted and legacy metadata; distinguish metadata collection, KG inclusion, node visibility and public publication. Account for every reader/writer and record the actual converted generation before retiring legacy support. Reopen on metadata typing, fallback, class exception or migration activation changes. See the [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/authored-vault-transition.md#inclusion-typing-and-local-fallback) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/vault-inclusion-snapshot.json). No actual corpus scan, sync, graph ingest or publication ran.

## Acceptance progress — 2026-09-05

**Implemented.** Two of the three reproduced problems are closed; see ADR-2040
for the parser detail, which this record shares.

*Local fallback metadata collection no longer bypasses the inclusion gate.*
`src/services/file_service.rs::scan_local_files_to_metadata` carried the check
commented out with "Include ALL files regardless of public status", so every
`.md` under the markdown directory entered the metadata store — and through it
the knowledge graph — whenever GitHub sync failed. The gate is restored and now
matches the GitHub path exactly; excluded pages are counted and logged so a
surprising fallback result is legible. `scan_local_files_to_metadata_in(dir)` is
split out so the gate can be exercised against a fixture corpus.

*Public-false-plus-class semantics are explicit.* `PageMeta` gains
`public_declared_false` (an explicit `public: false` is a different fact from an
absent key) and `InclusionReason { Public, PublicAndFormalClass, FormalClass,
FormalClassDespitePublicFalse, Excluded }`, plus `is_publishable()` — knowledge-graph
inclusion and public publication are now different questions with different
answers. A formal class admits a page to the graph and never makes it publishable;
an explicit `public: false` is honoured for publication whatever the class says.

*Class-marker typing* — see ADR-2040.

**Tests.** `cargo test -p visionclaw-domain --lib vault` — 68 passed, 0 failed
(15 new). Whole-crate `cargo test --lib --no-default-features` — 1254 passed.

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2014-2040-vault-inclusion.txt`.

**Remains open.** No corpus scan, sync, graph ingest or publication ran. Every
reader/writer is not yet accounted for, and the actual converted generation is
not recorded, so legacy support cannot be retired on this evidence.
