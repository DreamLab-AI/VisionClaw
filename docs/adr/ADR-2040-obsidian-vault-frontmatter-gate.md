---
id: ADR-2040
title: "The authored corpus is an Obsidian vault; YAML frontmatter `public`/`owl-class` gate KG inclusion with bounded Logseq tolerance"
date: 2026-09-02
decision_status: proposed
implementation_status: partial
activation_status: staged
supersedes: [ADR-2014]
superseded_by: []
verified_commit:
verified_paths: [crates/visionclaw-domain/src/vault/mod.rs, crates/visionclaw-domain/src/vault/link.rs, src/services/file_service.rs, src/services/github_sync_service.rs, src/services/parsers/knowledge_graph_parser.rs, src/services/github/content_enhanced.rs, src/services/ontology_mutation_service.rs, src/services/decision_elevation.rs, docs/VAULT-corpus-format.md]
owner: jjohare
review_trigger: "the first GitHub sync run after the corpus repo is converted in place, or 2026-12-01, whichever is earlier — at which point the Logseq `key:: value` tolerance is removed"
repo: visionclaw
domain: VAULT-corpus-format
lineage: "ADR-2014 (GitHub `public:: true` gate; `owl:class::` bypass), legacy ADR-050/051 (publish arbiter), PRD-LCR-01 in the corpus repo (JSON-LD canonical blocks)"
---

# ADR-2040 — The authored corpus is an Obsidian vault; frontmatter gates inclusion

## Context

The authored corpus (8,638 pages) is read by GitHub sync through Logseq
conventions: a `public:: true` property line, `owl:class::`, `source-domain::`,
`elevatedFrom:: [[..]]`, namespace files named `a___b.md`. Those conventions
are scattered across five Rust files and three agentbox scripts. The owner is
moving authoring from Logseq to Obsidian, whose native metadata is YAML
frontmatter and whose namespaces are folders. Obsidian renders `key:: value`
as plain text, so continuing to write Logseq properties would make every
new page invisible to the gate in the editor the owner actually uses.

## Decision

1. The authored corpus **is an Obsidian vault** as specified in
   [`docs/VAULT-corpus-format.md`](../VAULT-corpus-format.md) §V1–V5. That
   document is the governing authority for layout, frontmatter keys, body
   dialect, and the inclusion gate.
2. The KG inclusion gate is: frontmatter `public: true` **or** a non-empty
   frontmatter `owl-class`. Absence of both is private. This keeps ADR-2014's
   rule (formal data bypasses the publish gate; the gate anchors on parsed
   metadata, never on the path) and changes only the carrier.
3. **Bounded legacy tolerance.** Readers additionally accept the Logseq lines
   `public:: true`, `owl:class::`, `source-domain::`, `alias::`, `title::`,
   `elevatedFrom::` **only** when they occur in the leading property block
   (contiguous `key:: value` lines before the first blank line or heading).
   `public:: true` found anywhere else (the previous `is_public_file`
   behaviour, `file_service.rs:736`) no longer counts. Tolerance ends at the
   `review_trigger`.
4. A single parsing entry point — `visionclaw_domain::vault::PageMeta`
   (`parse(content) -> PageMeta { public, owl_class, source_domain, aliases,
   title, elevated_from, tags, extra }`) — replaces the six ad-hoc line
   scanners. Every reader listed in the governing doc's "Readers and writers"
   table calls it.
5. Page identity is the vault-relative path under `pages/` without `.md`,
   with `/` as the namespace separator; `___` and `%2F` decode to `/` on read.
6. Listing skips `/.obsidian/` and `/.trash/` in addition to the existing
   `/bak/`, `/logseq/`, `/.recycle/`, `/journals/`.
7. Writers (`OntologyMutationService`, `DecisionElevation`) emit frontmatter
   pages only.

## Consequences

- ADR-2014 is superseded (same rule, new carrier); its "re-checked server-side"
  property is preserved because the gate still runs in the sync path.
- A private page that previously leaked into the KG because `public:: true`
  appeared mid-body (e.g. inside a quoted example) is now correctly excluded.
  This is a deliberate narrowing.
- Node ids of non-namespace pages are unchanged. Namespace pages gain ids
  derived from `Ns/Title`, which matches how the corpus already links to them.
- The corpus repo must be converted with `vault-migrate` (ADR-2042) before the
  tolerance is removed; until then both formats ingest.
- agentbox consumers must move to the `[vault]` path authority (agentbox
  ADR-2028) in the same release.

## Verification

2026-09-02 on the `obsidian` branch: `cargo test -p visionclaw-domain` — 240
lib tests incl. 40 `vault` unit tests (frontmatter true/false/absent,
`public: "true"` string rejected, `owl-class` bypass, legacy leading-block
accepted, `public:: true` after a heading or inside a fence rejected,
`elevatedFrom` alias stripping, `___`/`%2F`/`pages/` identity decoding,
render/split round-trip and idempotence) + 5 fixture tests;
`cargo test --test vault_gate_test` — 9 over `tests/fixtures/vault/`;
`cargo test --lib services::` — 276; `cargo clippy -p visionclaw-domain
--all-targets` — 0 warnings in `vault`. Full suite 1109 passed, 1
pre-existing unrelated failure (`force_compute_actor::dag_rank_tests::
directed_hierarchy_relation_accepts_only_class_subsumption`, byte-identical to
`main`, self-contradictory assertion — tracked separately). Deliberate
findings: `DecisionElevation` drafts previously carried no `public` marker
and would have been dropped by any gate; `OntologyMutationService`'s
amendment path appended `- rel:: [[x]]` lines after the body (a §V5
violation) and now edits frontmatter. Shadow sync 2026-09-02 (EXP-V08): same binary, `main` vs converted branch →
13,351 nodes both, edges 156,153 vs 156,101, three title-driven label diffs.
The sync exposed that page identity had always been the bare basename (288
colliding basenames, 254 of them the intended main↔working twin join); the
identity is now the path relative to the matched base path, `title` echoes
of the identity are ignored, and wikilinks resolve by Obsidian's
shortest-path rule (`crates/visionclaw-domain/src/vault/link.rs`:
`VaultIndex`/`VaultContext`/`LinkResolution`; index built once from the full
listing before the SHA1 filter). Tests: 259 + 5 domain, 18 vault_gate,
276 services. `activation_status: staged` until the first GitHub sync runs
against the converted corpus.

## Closeout extension — 2026-09-04

CP-01/02/04/08. Owner remains jjohare with vault/knowledge/privacy maintainers. Implementation is partial against the complete typed/shared-reader contract. The shared parser and links pass 56 native tests, but owl-class accepts boolean/numeric scalars rendered as strings without IRI validation. Local fallback metadata scanning bypasses the inclusion check. Existing proposed/staged status remains; source implementation does not establish deployment adoption.

**Acceptance condition:** Define and enforce class-marker type/IRI policy and make public-false-plus-class semantics explicit. Exercise each ingest/fallback path with private, malformed, quoted and legacy metadata; distinguish metadata collection, KG inclusion, node visibility and public publication. Account for every reader/writer and record the actual converted generation before retiring legacy support. Reopen on metadata typing, fallback, class exception or migration activation changes. See the [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/authored-vault-transition.md#inclusion-typing-and-local-fallback) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/vault-inclusion-snapshot.json). No actual corpus scan, sync, graph ingest or publication ran.

## Acceptance progress — 2026-09-05

**Implemented.** `crates/visionclaw-domain/src/vault/mod.rs`. The reproduced
defect — `owl-class` accepted boolean and numeric scalars rendered as strings,
with no IRI validation, so `owl-class: true` opened the inclusion gate on a
value that is not a class IRI at all — is closed by a two-part policy.

* *Type.* `yaml_class_marker` requires a genuine YAML **string**. A boolean or
  number is never a class IRI however it renders.
* *Grammar.* `is_class_marker` requires a CURIE (`prefix:local`, prefix starting
  with a letter and continuing with letters, digits, `_`, `-`, `.`) or an
  absolute IRI (`http://`, `https://`, `urn:<nid>:<nss>`), with no whitespace or
  control characters. A bare word with no colon is rejected, which is what turns
  away a *quoted* `"true"` that passes the type check.

A present-but-rejected value is retained in `PageMeta::owl_class_rejected` so an
author can see why the page was excluded, while `owl_class` stays `None` and the
gate stays shut. The legacy leading-property-block carrier applies the same
grammar (it has no YAML type to check). A rejected marker is not re-emitted by
`to_frontmatter_yaml`, so a rewritten page cannot smuggle it back.

The local-fallback bypass and the public-false-plus-class semantics are recorded
under ADR-2014; both live in this module.

**Tests.** `cargo test -p visionclaw-domain --lib vault` — 68 passed, 0 failed
(15 new): boolean and numeric markers rejected; quoted non-IRI markers rejected;
well-formed CURIEs and absolute IRIs accepted; the grammar exercised directly
over 7 good and 14 bad values (with surrounding whitespace trimmed, not
rejected); the legacy carrier applying the same policy; and a rejected marker
not surviving a render round trip.

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2014-2040-vault-inclusion.txt`.

**Remains open.** Every reader/writer is not individually accounted for, and no
corpus scan or graph ingest ran, so deployment adoption is still not established
and legacy support cannot be retired on this evidence.
