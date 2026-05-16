# Data Sprint — Canonical Source-Data Schema

Author          : anthropic@xrsystems.uk
Branch baseline : `radical-rollback` @ `6910969b5` (Phase 1 persistence ports landed)
Date            : 2026-05-16
Status          : Sprint kickoff. Founding ADR (D01) authored; consumes from
                  migration sprint, feeds Phase 2 of migration sprint.

## Why this sprint exists

The migration sprint (`docs/migration-sprint/`) resolved *where* VisionFlow's
data lives: Oxigraph for quads, SQLite for settings, hexagonal ports between
domain and store. It did not resolve *what shape the source data has on disk*
before it reaches those stores.

That shape is currently five overlapping conventions stacked on the same
markdown files:

1. Logseq `property:: value` blocks (whatever the author types).
2. `### OntologyBlock` fenced sections (regex-parsed).
3. Custom frontmatter (`public::`, tags, dates, free-form).
4. The OwlClass V2 struct's 40+ Rust fields, populated from any of the above
   by hand-written heuristics in `src/services/knowledge_graph_parser.rs`.
5. Wikilinks `[[Term]]` that may or may not resolve to declared classes.

The result is that the parser, the Oxigraph adapter, the GitHub sync service,
the agent telemetry sink, the settings UI, and the XR client all carry their
own private opinion about what `label`, `definition`, `subClassOf`, `bridge_to`,
`renderTier`, `qualityScore`, `authorityScore`, and `maturity` *mean*. The
authoring surface and the storage surface have drifted from each other; every
new feature deepens the drift.

The user's framing: "merging our systems and labels, triplets, owl2, metadata".
This sprint is the schema unification — one canonical source-data schema that
every consumer reads from and writes to. The schema is JSON-LD 1.1 embedded
in Logseq markdown, an OWL 2 EL profile for reasoning, and a single `@context`
that re-exports W3C vocabularies so `rdfs:label`, `skos:prefLabel`, and
`schema:name` collapse to one canonical term throughout the system.

## Relation to the migration sprint

Dependency direction is strict and one-way:

```
data-sprint ──> migration-sprint Phase 2 (Section 8 implementation)
            ──> migration-sprint Phase 5+ (Section 7 telemetry, Section 10 GitHub adapter)
```

The migration sprint **does not depend on this sprint to land its existing
ADRs**. The persistence ports, the Oxigraph adapter scaffolding, the
named-graph layout (ADR-11 §D2), the OntologyClass-as-domain-type decision
(ADR-08 §D1), and the class-bit mask (TENSIONS-RESOLVED §TC-1) are already
finalised. This sprint operates *inside* those decisions, not against them.

What this sprint adds on top:

- **The wire-in format on disk** that the GitHub adapter (Section 10) reads.
- **The vocabulary mapping table** from Logseq `property::` conventions to
  W3C vocabulary IRIs, eliminating the 40+ ad-hoc OwlClass V2 fields.
- **The validation rules** that gate ingest at pre-commit and pre-write.
- **The migration codemod** for the 199 existing `public:: true` Logseq pages.

ADR-08 §D10 ("Anti-corruption layer at the GitHub adapter boundary") is the
hook this sprint plugs into: the `ParsedMarkdown` value object produced by
the adapter is defined by this sprint's schema. ADR-11 §D2's named graphs are
addressed by this sprint's `@graph` keyword routing.

## Deliverables

Three artefacts land in this directory:

| File                                       | Owner             | Purpose                                                                |
|--------------------------------------------|-------------------|------------------------------------------------------------------------|
| `README.md` (this file)                    | sprint lead       | Sprint overview, ground rules, relation to migration sprint            |
| `ADR-D01-canonical-source-schema.md`       | DDD architect     | The founding architectural decision — 15 numbered decisions D1..D15    |
| `context-v1.jsonld`                        | DDD architect     | The canonical JSON-LD 1.1 `@context`, in-repo + reserved public URL    |

A parallel agent producing **model fixtures** (worked examples for each domain
type + round-trip tests against the Oxigraph adapter) lands those at
`docs/data-sprint/fixtures/` and is tracked separately.

## Ground rules

1. **The schema is a contract, not a suggestion.** Every consumer (parser,
   adapter, sync service, telemetry sink, settings UI, XR client) MUST read
   and write data conforming to this schema. Adapters that need extra fields
   add them under `vc:` extension predicates; they do not invent new
   serialisations.

2. **JSON-LD 1.1 is the wire format.** Markdown remains the human-editable
   surface for pages, but every semantically-meaningful assertion lives in a
   fenced ```json-ld code block. Logseq's free-form `property:: value`
   blocks are tolerated for narrative metadata (tags, dates) but carry no
   schema weight after this sprint.

3. **W3C vocabularies are the canonical vocabulary.** `rdfs:`, `owl:`,
   `skos:`, `schema:`, `prov:`, `foaf:`, `dcterms:`, `xsd:`. VisionFlow-specific
   terms live under the `vc:` prefix and are documented in the ADR's
   exhaustive table (D-D01 §"The vc: vocabulary"). New terms get added to
   `context-v1.jsonld` and the table; they do not appear in the wild without
   that declaration.

4. **OWL 2 EL is the reasoner-aligned profile.** The whelk-rs reasoner
   already vendored in tree handles the EL profile. We reject negation,
   disjunction, allValuesFrom, complementOf, and unionOf at validation time.
   Authors who need OWL 2 DL expressivity get a clear error pointing at the
   spec section that explains why.

5. **Identity is content-addressed where possible.** `urn:visionflow:page:<sha256(canonical-path)>`,
   `urn:visionflow:axiom:<sha256(canonical-form)>`. Identifiers that name
   *authors* use `did:nostr:<pubkey>`. Composition rule: an authored thing
   carries both — the `urn:` identifies the subject, the `did:` identifies
   the asserter via PROV-O `wasAttributedTo`.

6. **Provenance is mandatory.** Every JSON-LD block carries
   `prov:wasAttributedTo` and `prov:generatedAtTime`. No exceptions. Blocks
   without provenance are rejected by the pre-ingest validator.

7. **Page-level Nostr signatures land as MVP.** Every published markdown file
   is signed by its author's Nostr key via a NIP-23 (long-form content) event
   that references the file's content hash. The signature is *not* embedded
   in the markdown — it is a sibling Nostr event verifiable against the
   pubkey in `prov:wasAttributedTo`. DID resolution (NIP-05 enrichment,
   public-key-to-identity lookups) is **out of scope** for this sprint and
   tracked separately.

8. **The schema is versioned.** `context-v1.jsonld` is v1. Future breaking
   changes mint a `context-v2.jsonld`. The parser refuses unknown versions
   with a clear error message. There is no implicit version negotiation.

9. **Validation runs twice.** Once as a pre-commit hook in the authoring
   repository (Logseq corpus), again at pre-ingest in `webxr` before any
   triples reach Oxigraph. Both validators run the same code path (the
   `vc-validate` crate) so divergence is impossible.

10. **The migration codemod is one-shot.** The 199 existing `public:: true`
    Logseq pages are transformed once, in a single codemod run, into the
    new schema. The codemod is reviewed by a human, committed, and retired.
    There is no live conversion layer that papers over the old format.

## Reading order

1. **This README** — establishes sprint context and ground rules.
2. **`ADR-D01-canonical-source-schema.md`** — the founding ADR. 15 numbered
   decisions, options considered, risks, cross-references to PRD-08 /
   DDD-08 / ADR-11 for edits required to reconcile those documents with
   this schema.
3. **`context-v1.jsonld`** — the working `@context` file. Reads as a single
   JSON document; every term is documented inline via `_comment` fields.
4. (Parallel) **`fixtures/`** — worked examples for each domain type. Run
   `cargo test -p vc-validate` to round-trip every fixture through the
   validator and the Oxigraph adapter.

## Phasing

| Phase | Owner                         | Deliverable                                                          | Status     |
|-------|-------------------------------|----------------------------------------------------------------------|------------|
| D-1   | DDD architect (this sprint)   | ADR-D01 + context-v1.jsonld + fixtures                               | In progress|
| D-2   | Tooling                       | `vc-validate` crate: JSON-LD framing + SHACL-lite shape checks       | Next       |
| D-3   | Migration codemod             | One-shot transform of 199 `public:: true` Logseq pages               | After D-2  |
| D-4   | Migration sprint Phase 2      | Section 8 implementation consumes this schema                         | Hand-off   |
| D-5   | Separate sprint               | DID resolution / NIP-05 enrichment / signature verification chain    | Deferred   |

## Cross-references

The following migration-sprint documents will receive amendments after this
ADR lands. Edits are scoped and listed in ADR-D01 §"Cross-references":

- `docs/migration-sprint/08-ontology-graph-data/PRD-08.md` — capabilities
  expressed in W3C vocabulary terms instead of OwlClass V2 field names.
- `docs/migration-sprint/08-ontology-graph-data/DDD-08.md` — Page,
  OntologyClass, OntologyProperty, Axiom aggregates' field surfaces aligned
  to canonical terms; OwlClass V2's 40+ fields mapped explicitly.
- `docs/migration-sprint/11-persistence-migration/ADR-11.md` — §D3 IRI
  minting harmonised with the `urn:visionflow:*` scheme; §D5 audit_log
  column names cross-checked for `prov:` compatibility.

## Out of scope

- DID resolution mechanics (separate sprint).
- Public deployment of `https://narrativegoldmine.com/context/v1.jsonld` (URL is
  reserved; serving the file is a Phase 9 ops task).
- SHACL Compact Syntax — we use SHACL-lite shape definitions inlined in the
  validator crate, not a separate `.shacl` file.
- Multi-language label support beyond `@language: "en"` in the context. The
  schema is i18n-ready but no other locale ships in v1.
- Logseq-side authoring tooling (a Logseq plugin that helps authors write
  conformant JSON-LD blocks is a future deliverable, not part of this sprint).

## Glossary

| Term                  | Meaning in this sprint                                                          |
|-----------------------|---------------------------------------------------------------------------------|
| **Canonical schema**  | The JSON-LD 1.1 schema defined by `context-v1.jsonld` and constrained by ADR-D01|
| **Authoring surface** | Logseq markdown files; what a human edits                                       |
| **Wire format**       | JSON-LD blocks embedded in markdown; what the parser reads                      |
| **Canonical RDF**     | The N-Quads emission produced by JSON-LD expansion; what Oxigraph stores        |
| **Provenance**        | `prov:wasAttributedTo` + `prov:generatedAtTime` on every assertion              |
| **vc: vocabulary**    | VisionFlow-specific predicates under `urn:visionflow:owl:property:`             |
| **Page signature**    | NIP-23 Nostr event over the page's content hash, sibling to the file           |
