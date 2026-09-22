---
id: ADR-2112
title: "The corpus is frontmatter-only OKF governed by a versioned `ontology/vocabulary.yaml`; the json-ld fences and every Logseq construct are deleted, not tolerated"
date: 2026-09-22
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: [ADR-2040]
superseded_by: []
verified_commit: 06dfe97a55e6a7a42bfc74a26a513108c60d5735
verified_paths: []                # the evidence is the corpus census + vocabulary self-consistency, not a governed code path; the staleness gate stays inert by design
owner: jjohare
review_trigger: "`vault migrate --fences-to-properties` completing its real run on visionGraph, at which point implementation_status moves to complete and the intentional_deltas are measured against the golden build; or the first Schema-level proposal against ontology/vocabulary.yaml, whichever is earlier"
repo: visionclaw
domain: VAULT-corpus-format
lineage: "ADR-2040 (Obsidian vault, `public`/`owl-class` gate, bounded Logseq tolerance), ADR-2041 (settings rename), ADR-2042 (`vault-migrate` converter), ADR-2014 (the original `public:: true` gate)"
---

# ADR-2112 — The corpus is frontmatter-only OKF governed by a versioned `ontology/vocabulary.yaml`

## Context

One corpus, four doors, four class counts (8,433 / 8,433 / 8,146 / 4,167) and no
way to say which is right. The ontology does not live where `VAULT-corpus-format.md`
v1.4.2 says it lives: it is in two `json-ld` fences per page, so the spec's
`owl-class`, `source-domain`, `maturity` and `quality` keys occur **zero** times in
frontmatter, and ADR-2040's bounded Logseq tolerance never expired.

A pipeline-independent census of all 8,671 knowledge pages and 574 working pages
(`visionGraph/docs/fence-census-2026-09-22.md`) found the format is also losing
data: 955 classes declaring `maturity: mature` are coerced to `draft` by the
Turtle emitter's five-value enum; 744 classes carry a `qualityScore` the parser
never reads and publish `quality 0.0`; 75 relation edges across 34 predicates are
dropped because `rel_map` has twelve entries; 3,580 `sameAs` values are never
emitted at all; and a third, undocumented fence type (3,712 `LinkResolutionsAnnotation`
blocks) stores derived data in the pages. The `key:: value` lines that v1.4.2
called "content, not metadata" carry 36,632 relation edges absent from the fences,
18,006 of which resolve to real pages.

The owner's governing principle for this work: *"we have git tracking and should
carefully fully migrate to the cleanest outcome."* No shims, no one-more-release
tolerances.

## Decision

**1. Frontmatter is the entire metadata surface.** The `Page` and `Class` fences
fold into typed Obsidian Properties on all 8,454 pages. The
`LinkResolutionsAnnotation` fences are deleted as derived data. `definition`
becomes the page's leading body paragraph — for 5,053 of 8,446 classes the fence
is the only place that prose exists, and it is too long to be a property.

**2. `visionGraph/ontology/vocabulary.yaml` is the normative key set**, versioned,
and `visionGraph/vault.toml` is the manifest. `VAULT-corpus-format.md` governs the
*shape*; the vocabulary governs the *content*. `vault validate` reads the
vocabulary, not the document, so the two cannot drift.

**3. The emitted OWL surface does not change.** Every relation with
`emitted: true` keeps the exact `vc:` property `pipeline/jsonld_to_turtle.py`
wrote. Only the authoring syntax changes. The corpus's 57 predicate keys resolve
to 15 core relations, 11 alias spellings folded by the migration, and 35
provisional relations that are valid frontmatter but `emitted: false` — visible in
the page API and the graph, absent from `ontology.ttl`, so golden-build parity
against the Python output holds. Promoting one is a signed Schema change.

**4. Five deltas against the current build are deliberate and enumerated** in
`vocabulary.migration.intentional_deltas`: `mature` stops collapsing to `draft`
(one new named individual); 744 classes publish their real quality; relations gain
the union of the `key::` edges; the 35 provisional predicates and the 3,580
`same-as` values become visible in the page API. Everything else must be
byte-identical.

**5. No legacy tolerance.** `vault validate` **rejects** — with no flag and no
window — a `json-ld` fence, a `key:: value` line, a `{{embed}}`, a
`((block-ref))`, an `a___b.md` filename, a Logseq journal, a `TODO`/`DONE` marker
and a `#[[multi word]]` tag. ADR-2040's tolerance is not extended; it is deleted
along with the constructs it tolerated.

**6. Migration is lossless by construction.** `vault migrate
--fences-to-properties` fails on any fence field or `key::` key not covered by
`vocabulary.migration` rather than guessing or dropping. Leading-block `key::`
lines take a mapped frontmatter destination; in-bullet ones are union-merged when
they are relations and rendered into prose otherwise; relation values are the
**union** of fence and `key::` targets. A `--dry-run` diff is committed before the
real run. Git is the rollback; there is no reverse converter.

**7. `resource` is immutable.** It is minted once as `namespace + slug(title)` and
copied verbatim thereafter — never recomputed. 412 existing IRIs come from a
case/digit-boundary slugifier (`3D Asset.md` → `urn:ngm:class:3-d-asset`); they
are the subject of 104,731 edges and of every published URL. `vault validate`
checks presence, uniqueness and immutability, not cosmetic agreement with the
filename.

**8. The `owl-class` gate bypass is removed.** v1.4.2's `is_kg_included()`
disjunction let a page be private and published simultaneously. `type` and
`resource` make a page a class; `public` decides only publication, fail-closed.

**9. Two vault roles with distinct contracts** (PRD Q9): `knowledge/` admits
`Class`/`Property`/`Individual`, requires `type`/`title`/`resource`/`public`/
`status`/`generated`, and **fails** on an unknown key; `working/` admits
`Note`/`Episode`/`Transcript`/`Draft Concept`/`Journal`/`Canvas` and tolerates
unknown keys per OKF v0.2 §4.1, with its episodic keys documented as extensions.

**10. `okf.actors` is extended additively with `did:<method>:<id>`**, because that
is what 6,697 classes actually carry (`did:nostr:ontology-mesh` 3,819,
`did:nostr:jjohare` 1,077, …). A DID is a stronger actor identifier than
`agent:<producer>/<version>` and is the identity the forum signing loop matches.

**11. `knowledge/assets` stays a symlink** into `working/assets`. An elevated page
keeps working asset links without a copy; the per-vault-copy alternative produced
two divergent multi-hundred-megabyte trees.

**12. `inverse:` is a build hint, not an axiom.** Inverse object properties are
outside OWL 2 EL, so `vault build` materialises the reverse edge in the graph,
page API and scaffold index and emits no `owl:inverseOf`, exactly as the Python
emitter chose. The corpus uses one inverse pair in both directions
(`has-part`/`part-of`).

## Consequences

**Becomes true.** One parser (`vault-core`) reads the corpus for VisionClaw, the
CLI and CI, so the four class counts collapse to one. The corpus is legible in
Obsidian and Quartz without a plugin, because the metadata is Properties and the
prose is prose. The vocabulary is a single reviewable file, so "what predicates
exist?" has an answer and unknown keys fail the build. Three silent data losses
stop.

**Becomes easier.** Bases views over frontmatter give the curator a review queue
and a stale list with core plugins only. `vault edit --expect` makes agent
mutation auditable. A Schema change is a diff against one YAML file, which is what
makes it signable.

**Becomes harder — deliberately.** Authoring a relation the vocabulary does not
know now fails the build instead of being silently dropped at Turtle time. Adding
a predicate is a tier-High signed decision. Editing a page by hand means editing
YAML, not a fence.

**Becomes forbidden.** A second corpus parser. Storing derived data (inferred
closure, backlinks, outbound wikilinks, link resolutions) in a page. Recomputing
`resource`. Emitting a `key::` line. Approving past a blocker.

**Costs and follow-on work.** The 8,454-page rewrite is one irreversible commit
whose only rollback is git — hence the committed dry-run diff and the migration's
fail-on-uncovered-field posture. `vault-core`, the CLI and the golden-parity test
(ADR-2113) must land before the migration runs; the VisionClaw ingest rewrite
(ADR-2114/2115) and the governance wiring (ADR-2116) follow. The `ai` versus
`artificial-intelligence` domain split (513 pages) is deliberately **not** fixed
here: it is a content decision and goes through the governance loop, not the
migration. The 18,626 dangling `key::` targets are dropped and reported; deciding
their fate is separate content work.

**Supersession.** ADR-2040's inclusion gate, its bounded tolerance, and the
v1.4.x closeout qualifications against ADR-2040/2041/2042 are closed by
supersession rather than remediation: the gate is replaced (§8), the settings
alias is deleted (ADR-2114), and `crates/vault-migrate` is deleted in favour of
`vault migrate` (ADR-2113).

## Verification

`implementation_status: partial` — the contract and its evidence exist; the code
and the migration run do not yet.

Established at this ADR's date, before any `verified_commit`:

- **The vocabulary is derived from the corpus, not from the old spec.** Census at
  `visionGraph/docs/fence-census-2026-09-22.md`, produced by three throwaway
  parsers sharing no code with `pipeline/`: 8,454 fenced pages, 8,446 `Class`
  fences, 57 relation predicates, 104,731 edges, 100,675 `key::` lines over 643
  keys, 8,446 `domain` / 8,446 `maturity` / 7,651 `quality` values, four
  `provenance` shapes.
- **The OWL surface is unchanged.** `ontology/vocabulary.yaml` declares exactly 14
  `emitted: true` relations — `rdfs:subClassOf` plus the thirteen `vc:` object
  properties `pipeline/jsonld_to_turtle.py` declares (`hasPart`, `isPartOf`,
  `requires`, `enables`, `enabledBy`, `dependsOn`, `implements`, `uses`,
  `supports`, `standardizedBy`, `contrastsWith`, `bridgesTo`, `relatedTo`) with
  the same transitivity, sub-property and existential-restriction choices, plus
  the `vc:utilises` super-property. Checked by loading the YAML and diffing the
  emitted set against the emitter's declarations.
- **The migration table is total — against a named extent.** Every json-ld fence
  key and every `key::` key in the corpus has a destination or an explicit `drop`
  in `vocabulary.migration`, and `vault migrate` exits 2 without writing if one is
  missing. `vault migrate --fences-to-properties --dry-run` over the live corpus
  reports 8,635 pages examined, 8,635 converted, 20,596 fences, 100,203 Logseq
  lines, 33 block references inlined, 4,748 aliases added, and 0 uncovered fence
  keys / 0 uncovered Logseq keys / 0 unconvertible embeds. An independently
  written checker (`.tmp/verify_total.py`), sharing no code with the crate,
  agrees.

  **Totality is a property of a pairing — a map and an extent — never of the map
  alone**, so the extent is stated: 8,635 pages on disk plus the 11 under
  `pages/_misc/` that exist at HEAD but not on disk. That distinction is
  load-bearing and was established the hard way. Two defects were found in
  precisely the gap between `git ls-tree HEAD` and `find`: ten Logseq keys that
  occur only on the 189 pages carrying no fence (16,145 `key::` lines, invisible
  to an enumerator that skipped fence-less pages), and `OntologyClass.vc:bridgesTo`,
  which occurs zero times on disk and ten times in the deleted-but-unstaged files.
  Both maps had been described as total when they were total only against what
  their enumerator could see. The standing guard is that the checker and the
  artefact under test must have independently derived extents and be asserted to
  agree.

  The YAML parses and its internal references resolve: every relation alias
  targets a declared relation, every `inverse:` names a declared relation, and
  every migration destination names a declared relation, scalar or reserved key
  (enforced by `vault-core`, which rejects the vocabulary otherwise).
- **`vault.toml` parses** and its build-artefact list matches contract C3
  one-for-one (13 artefacts).
- **Deltas are enumerated, not discovered.** `vocabulary.migration.intentional_deltas`
  lists the five; EXP-V07 in `docs/VAULT-corpus-format.md` requires the golden
  build to differ by those and nothing else.

Outstanding before `implementation_status: complete`: `vault validate` and
`vault migrate` implemented (ADR-2113), the dry-run diff committed, the real run
green on both vaults, and the golden-parity test passing with the five deltas
accounted for. Outstanding before `activation_status: live`: PRD acceptance 2
(one class count across Loom, VisionClaw and `vault build --stats`).
