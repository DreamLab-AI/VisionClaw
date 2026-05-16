# ADR-D01 — Canonical Source-Data Schema

Status      : Proposed (founding ADR for the Data Sprint)
Date        : 2026-05-16
Sprint      : Data Sprint (`docs/data-sprint/`)
Branch      : `radical-rollback` @ `6910969b5`
Supersedes  : The implicit Logseq + OwlClass V2 dual-schema covering 199
              `public:: true` markdown pages and 40+ heuristic Rust fields.
Related     : ADR-08 (Ontology & Graph Data — consumes this schema),
              DDD-08 (Knowledge Graph aggregates — fields realigned),
              PRD-08 (capability spec — restated in vocabulary terms),
              ADR-11 §D2/D3 (Persistence — named graphs + IRI minting),
              TENSIONS-RESOLVED §TC-1 (class-bit mask, unchanged),
              ADR-07 (Bots & Telemetry — agent telemetry graph),
              ADR-10 (External Integrations — GitHub adapter boundary).

## Context

The migration sprint resolved that VisionFlow stores its semantic data in
Oxigraph (named-graph segregated, SPARQL 1.1) and its settings in SQLite.
What it did not resolve — by design, deferred to this sprint — is what
the source data on disk *looks like* before it reaches those stores.

That source data is currently a five-way stack of overlapping conventions:

1. **Logseq `property:: value` blocks** parsed by the Logseq client into
   page metadata. Free-form. No schema.
2. **`### OntologyBlock` markdown sections** regex-parsed by
   `src/services/knowledge_graph_parser.rs` to extract ontology assertions
   (`ontology:: true`, `definition::`, `subclassOf::`, `disjointWith::`,
   etc.).
3. **YAML/Logseq frontmatter** for `public:: true`, tags, dates, and
   per-page custom properties — same syntax as (1) but read at a different
   point in the parser.
4. **OwlClass V2 with 40+ fields** (`src/ports/ontology_repository.rs:52`)
   populated by heuristic mapping from any of (1)–(3). Fields include
   `quality_score`, `authority_score`, `status`, `maturity`, `owl_physicality`,
   `owl_role`, `belongs_to_domain`, `bridges_to_domain`, `has_part`,
   `is_part_of`, `requires`, `enables`, `relates_to`, etc.
5. **Wikilinks `[[Term]]`** referencing pages or classes by label, with no
   guarantee that the target exists.

The user's framing of this sprint is precise: *"super mature… hugely force
multiplying… merging our systems and labels, triplets, owl2, metadata."*
What is being merged:

- Three syntaxes (Logseq properties, frontmatter, OntologyBlock) collapse to
  one (JSON-LD 1.1 embedded in fenced code blocks).
- Three label conventions (`rdfs:label`, `skos:prefLabel`, `schema:name`)
  collapse to one canonical alias (`label`) backed by all three predicates.
- 40+ ad-hoc Rust fields collapse onto a small set of W3C-vocabulary
  predicates plus a documented `vc:` extension vocabulary.
- Five identity schemes (file path, slug, OwlClass IRI, Logseq UUID, raw
  u32) collapse to a content-addressed `urn:visionflow:*` scheme for
  subjects and `did:nostr:*` for asserters.

The upstream-fix principle (khive memory `cd142fc6`) is binding: where the
source data shape is wrong, we fix the source, not the downstream consumer.
This ADR is the source-shape definition.

The persistence Phase 1 (`6910969b5`, khive memory `f7ff46cd`) landed the
Oxigraph and SQLite adapter scaffolding. This ADR defines what the adapters
will receive once a real ingest pipeline runs against the new schema.

## Decision

### D1. JSON-LD 1.1 is the embedded serialisation in Logseq markdown

Every semantically meaningful assertion lives in a fenced ```json-ld code
block inside a Logseq markdown file. JSON-LD 1.1 (W3C Recommendation,
July 2020) is the chosen serialisation. Markdown remains the page
surface — narrative prose, headings, lists, tables — but assertions about
*what the page is*, *what it relates to*, and *what it asserts* live in
JSON-LD blocks.

A single page may contain multiple JSON-LD blocks. Each block is a JSON-LD
document independently expandable to N-Quads. The parser concatenates all
blocks from a page and presents them to the domain layer as a single
`ParsedMarkdown` value object (per ADR-08 §D10).

Rationale:

- JSON-LD is JSON; existing tooling (Logseq itself, every editor, every
  language's JSON parser) reads it without dependencies.
- JSON-LD 1.1 supports framing (`@included`, `@graph`, `@nest`,
  type-coercion via `@type`), enough for our needs without dropping to
  Turtle.
- The `@context` mechanism gives us the vocabulary aliasing we need to
  call `label` once and have it mean `rdfs:label` + `skos:prefLabel` +
  `schema:name` at the wire level.
- Logseq's free-form `property:: value` syntax stays available for
  authors who don't want to write JSON-LD — but it carries no schema
  weight after this sprint. The parser ignores it for ontology purposes.

Rejected: YAML frontmatter as primary serialisation (no JSON-LD
equivalent in YAML 1.2; would require custom expansion logic), Turtle
(non-JSON; needs a separate parser; harder to author by hand), N-Quads
directly (illegible for authors).

### D2. Identity scheme — `did:nostr:` + `urn:visionflow:*`

The identity scheme is two-layered:

- **`did:nostr:<pubkey>`** for *asserters* — people, agents, signed
  authorities. Conforms to W3C DID Core 1.0; the method specification
  follows `did:key`-style direct embedding of the secp256k1 public key
  (no separate DID document required for resolution). The pubkey is the
  hex-encoded 32-byte Nostr public key.

- **`urn:visionflow:<resource-type>:<identifier>`** for *subjects* —
  pages, ontology classes, properties, axioms, agent telemetry events.
  Content-addressed where possible. Specific schemes:

  | Resource type        | URN scheme                                                | Identifier         |
  |----------------------|-----------------------------------------------------------|--------------------|
  | Page                 | `urn:visionflow:page:<sha256(canonical-path)>`           | sha256 of path     |
  | Ontology class       | `urn:visionflow:owl:class:<slug>`                         | lowercase-hyphenated label |
  | Ontology property    | `urn:visionflow:owl:property:<slug>`                      | same                |
  | Axiom                | `urn:visionflow:axiom:<sha256(canonical-N-Quad-form)>`    | sha256 of N-Quad   |
  | Agent telemetry      | `urn:visionflow:agent:<run-id>:<step>`                    | run + step         |
  | Named graph          | `urn:visionflow:graph:<name>`                             | per ADR-11 §D2     |

The page IRI is rename-stable via redirect records: a separate
`vc:redirectsFrom` triple is added when a page is renamed, preserving
inbound references against the new content hash. Old URN keeps
resolving via the redirect for a deprecation window.

The slug rule is deterministic: NFKC normalisation → lowercase →
non-alphanumeric → `-` → collapse consecutive `-` → trim. Identical
labels produce identical slugs; the validator rejects duplicate
declarations at pre-ingest.

The composition rule for authored content: an authored thing carries
*both* identifiers. The `urn:visionflow:*` is the subject IRI (`@id`),
the `did:nostr:*` is the asserter via PROV-O `wasAttributedTo`.

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:cybernetics",
  "@type": "OntologyClass",
  "label": "Cybernetics",
  "wasAttributedTo": "did:nostr:abc123…",
  "generatedAtTime": "2026-05-16T10:00:00Z"
}
```

Rejected: UUIDs (not content-addressed; mint a new identifier every
parse; no dedup). Raw integers (current OwlClass V2 path; collides with
binary-protocol wire IDs and adds an arbitrary remapping layer).

### D3. Canonical `@context` v1 — single file, in-repo + reserved public URL

The canonical `@context` lives at:

- **In repo**: `docs/data-sprint/context-v1.jsonld`
- **Reserved public URL**: `https://narrativegoldmine.com/context/v1.jsonld`

The in-repo file is authoritative. Documents on disk reference the public
URL string for portability; the parser substitutes the in-repo file when
the URL matches. There is no network fetch on the hot path.

The context re-exports W3C vocabularies via standard prefixes (`rdf:`,
`rdfs:`, `owl:`, `skos:`, `schema:`, `prov:`, `foaf:`, `dcterms:`,
`xsd:`, `sh:`) and defines the `vc:` prefix for VisionFlow-specific
predicates at `urn:visionflow:owl:property:<name>`.

The single most important normalisation in the schema: `label`,
`prefLabel`, `title`, and `name` collapse to one wire term that — at
expansion time — emits triples under *all* of `rdfs:label`,
`skos:prefLabel`, and `schema:name` where the term is aliased.

Implementation: the friendly alias `label` is defined as
`{"@id": "rdfs:label", "@language": "en"}`. We DO emit `rdfs:label` as
the canonical predicate. Downstream consumers that prefer
`skos:prefLabel` or `schema:name` resolve via SPARQL property paths
(`?s rdfs:label|skos:prefLabel|schema:name ?o`) — this is the standard
W3C idiom. We do *not* triple-write the same literal under three
predicates; that's lexical bloat for no semantic gain.

See `context-v1.jsonld` for the working file. Every term is documented
inline via `_comment_*` fields.

### D4. OWL 2 EL profile boundary

VisionFlow asserts and reasons over OWL 2 EL only. EL is what whelk-rs
(already vendored at `src/inference/`) reasons; it is also the profile
designed for "applications employing large ontologies" (W3C OWL 2 EL
spec §1, "EL is particularly useful in applications employing
ontologies that contain very large numbers of properties and/or
classes").

**Allowed constructs**:

- Class hierarchies: `rdfs:subClassOf`
- Class equivalence: `owl:equivalentClass` (binary only; no expression
  reuse)
- Property hierarchies: `rdfs:subPropertyOf`
- Property domains and ranges: `rdfs:domain`, `rdfs:range`
- Existential restrictions: `owl:someValuesFrom`, `owl:onProperty`
- Property chain axioms: `owl:propertyChainAxiom`
- Property characteristics: `owl:TransitiveProperty`,
  `owl:ReflexiveProperty`, `owl:FunctionalProperty` (the
  EL-compatible subset)
- Datatype properties with `xsd:string`, `xsd:integer`, `xsd:float`,
  `xsd:boolean`, `xsd:dateTime`

**Rejected constructs** (validator returns an error referencing OWL 2
EL spec §3 "Profile Specification"):

- Negation: `owl:complementOf`
- Disjunction: `owl:unionOf`
- Universal restrictions: `owl:allValuesFrom`
- Disjoint classes: `owl:disjointWith`, `owl:AllDisjointClasses`
- Inverse object properties: `owl:inverseOf` on object properties
  (we retain `owl:inverseOf` declarations as annotations but the
  reasoner ignores them)
- Asymmetric / irreflexive properties: `owl:AsymmetricProperty`,
  `owl:IrreflexiveProperty`
- `owl:hasValue`, `owl:hasSelf`, `owl:minCardinality`, `owl:maxCardinality`,
  `owl:cardinality` (cardinality on existential restrictions only via
  `someValuesFrom`)
- Anonymous individuals (blank nodes as A-Box subjects)

The validator emits a structured error for each rejected construct:

```
ValidationError {
    code: "OWL2EL_OUT_OF_PROFILE",
    construct: "owl:disjointWith",
    spec_reference: "OWL 2 EL §3, Table 1",
    suggestion: "Model as a separate axiom in <urn:visionflow:graph:annotation>
                 if disjointness is intent rather than reasoned-over fact."
}
```

Rationale: authors get a clear signal when they exceed the reasoner's
capacity. The error includes a forward path (annotation graph) for the
case where the assertion is real but not reasoner-load-bearing. We do
NOT silently accept-and-ignore out-of-profile axioms; silence breeds
expectation mismatch.

### D5. Wikilink → @id desugaring rule

A wikilink `[[Term]]` in markdown desugars at parse time to a JSON-LD
`@id` reference. The deterministic rule:

1. Strip surrounding `[[` and `]]`.
2. NFKC normalise.
3. Lowercase.
4. Replace non-alphanumeric with `-`.
5. Collapse consecutive `-`.
6. Trim.
7. Prepend `urn:visionflow:owl:class:` if the label resolves (case-fold)
   to a declared OntologyClass label; otherwise prepend
   `urn:visionflow:page:<sha256(canonical-path-of-target-if-known)>`;
   otherwise mint a `LinkedPage` placeholder IRI under
   `urn:visionflow:linkedpage:<slug>`.

The resolution check happens during the post-parse aggregate-population
pass (DDD-08 §G2), not during JSON-LD expansion. Expansion produces a
provisional IRI; resolution rewrites it to the declared IRI when a
match is found. The `LinkResolved` domain event records the upgrade
(DDD-08 §G3).

Within a JSON-LD block, authors may also write wikilinks directly as
strings, which the parser desugars before expansion:

```json-ld
{
  "@id": "urn:visionflow:owl:class:cybernetics",
  "subClassOf": "[[Systems Theory]]"
}
```

Becomes:

```json-ld
{
  "@id": "urn:visionflow:owl:class:cybernetics",
  "subClassOf": {"@id": "urn:visionflow:owl:class:systems-theory"}
}
```

The desugaring is one-way; the canonical RDF emission contains the
URN only.

### D6. Page-level structure

A canonical VisionFlow markdown page has this shape:

```markdown
---
public:: true
---

# Cybernetics

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:abc123…",
  "@type": "Page",
  "title": "Cybernetics",
  "wasAttributedTo": "did:nostr:def456…",
  "generatedAtTime": "2026-05-16T10:00:00Z",
  "schemaVersion": 1,
  "subject": ["[[Cybernetics]]"]
}
```

The science of [[Communication and control]] in animals and machines.

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:cybernetics",
  "@type": "OntologyClass",
  "label": "Cybernetics",
  "definition": "The science of communication and control in animals and machines.",
  "subClassOf": "[[Systems Theory]]",
  "definedIn": {"@id": "urn:visionflow:page:abc123…"},
  "wasAttributedTo": "did:nostr:def456…",
  "generatedAtTime": "2026-05-16T10:00:00Z",
  "qualityScore": 0.92,
  "authorityScore": 0.85,
  "maturity": "mature"
}
```
```

Each JSON-LD block is a complete document. The first block typically
defines the `Page` itself; subsequent blocks define ontology classes,
properties, axioms, and bridges declared on the page.

Each block MUST carry `@context` (or `context`), `@id`, `@type`,
`wasAttributedTo`, and `generatedAtTime`. The validator rejects any
block missing one of these.

A page WITHOUT JSON-LD blocks is still a valid markdown file; it
simply contributes no semantic assertions. (Logseq journal entries
typically fall in this category.)

### D7. Named-graph routing via `@graph`

JSON-LD 1.1's `@graph` keyword scopes statements to a named graph.
The four named graphs (per ADR-11 §D2):

| Named graph IRI                                  | Default for                              |
|--------------------------------------------------|------------------------------------------|
| `urn:visionflow:graph:knowledge`                 | `Page`, `LinkedPage`, wikilinks          |
| `urn:visionflow:graph:ontology:assert`           | `OntologyClass`, `OntologyProperty`, `Axiom` |
| `urn:visionflow:graph:ontology:inferred`         | Whelk-rs output (never authored)         |
| `urn:visionflow:graph:agent`                     | Agent telemetry (`AgentTelemetry`)       |

The parser assigns named graphs based on `@type` per the table above.
Authors who need to override (rare; e.g. a Page that *contains* an
agent telemetry block describing a tool call) wrap the assertions in
`@graph`:

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@graph": [
    {
      "@id": "urn:visionflow:agent:run-42:step-7",
      "@type": "AgentTelemetry",
      "runId": "run-42",
      "step": 7,
      "agentKind": "coder",
      "tool": "Edit",
      "outcome": "success",
      "latencyMs": 230,
      "wasAttributedTo": "did:nostr:agent-key…",
      "generatedAtTime": "2026-05-16T10:01:00Z"
    }
  ]
}
```

The pre-ingest validator verifies that every emitted quad lands in the
correct named graph per the assertion's `@type`. Mismatches (e.g. an
`OntologyClass` in `urn:visionflow:graph:agent`) are rejected.

ADR-11 §D2 lists this graph as `urn:visionflow:graph:ontology`. Per
TENSIONS-RESOLVED §CC-4, Section 11 wins on naming: the asserted
ontology graph is `urn:visionflow:graph:ontology:assert` going forward,
and ADR-11 receives a corresponding edit (see §"Cross-references"
below). This sprint's documents use the `:assert` suffix from D7
onward.

### D8. Provenance is mandatory on every block

Every JSON-LD block MUST carry:

- `prov:wasAttributedTo` → a `did:nostr:<pubkey>` IRI identifying the
  asserter (the person or agent that produced this assertion).
- `prov:generatedAtTime` → an `xsd:dateTime` literal in UTC.

The validator rejects blocks missing either. The reason is operational:
when an axiom turns out to be wrong, we want to know who asserted it
and when, with no ambiguity. PROV-O is the W3C standard for exactly this
question.

Optional but recommended:

- `prov:wasDerivedFrom` → an `@id` reference to a source document. The
  GitHub adapter populates this with the source file's canonical URN
  during ingest.
- `prov:wasGeneratedBy` → for automated assertions, a reference to the
  process / agent run that generated the block. (e.g. whelk-rs
  inferences carry `wasGeneratedBy: did:nostr:reasoner-key`.)

### D9. Page-level Nostr signature (NIP-23 wrapper)

Every published markdown file is signed by its author's Nostr key. The
signature is *not* embedded in the markdown — it is a sibling Nostr
event (NIP-23 long-form content) referencing the page's content hash.

NIP-23 event shape (relevant fields):

```json
{
  "kind": 30023,
  "pubkey": "<author-pubkey-hex>",
  "created_at": <unix-ts>,
  "tags": [
    ["d", "urn:visionflow:page:<sha256(canonical-path)>"],
    ["title", "Cybernetics"],
    ["vc-content-hash", "<sha256(markdown-bytes)>"],
    ["vc-schema-version", "1"]
  ],
  "content": "<markdown-source>",
  "sig": "<schnorr-signature>"
}
```

The `d` tag is the page's URN, making the event addressable. The
`vc-content-hash` tag is the deterministic hash of the markdown bytes
(after newline normalisation to `\n`).

Verification at ingest:

1. Parser computes the markdown content hash.
2. Validator looks up the NIP-23 event by `d` tag (page URN) on the
   author's preferred relay.
3. Validator verifies the Schnorr signature against the event's
   `pubkey`.
4. Validator confirms `vc-content-hash` matches the computed hash.

DID resolution (mapping `did:nostr:<pubkey>` to author identity beyond
the bare pubkey) is **OUT OF SCOPE** for this sprint. NIP-05 enrichment,
relay discovery, signature timestamping via OpenTimestamps, and the full
trust chain are deferred to a separate sprint. The MVP signature is
enough to: (a) detect unauthorised modifications, (b) attribute
assertions to a stable identifier.

For the migration codemod (D13), pages are signed by the project lead's
key. Pre-existing pages receive a synthetic signature event tagged with
`vc-codemod-batch: 2026-05-16` so that the synthetic provenance is
distinguishable from organically-authored content.

### D10. Content-addressed axioms

Axioms carry an IRI minted from the sha256 of their canonical N-Quad
form:

```
urn:visionflow:axiom:<sha256(N-Quad-form-of-subject-predicate-object-graph)>
```

The canonical N-Quad form is produced by:

1. Express the axiom's subject, predicate, object, and named-graph IRI
   as N-Quad terms.
2. Concatenate as `<subject> <predicate> <object> <graph> .` with single
   spaces.
3. UTF-8 encode.
4. SHA-256 → hex-encode.

Example: the axiom "Cybernetics ⊑ Systems Theory" in the asserted
ontology graph:

```
<urn:visionflow:owl:class:cybernetics> <http://www.w3.org/2000/01/rdf-schema#subClassOf> <urn:visionflow:owl:class:systems-theory> <urn:visionflow:graph:ontology:assert> .
```

→ sha256 → axiom IRI `urn:visionflow:axiom:7f3a2b…`.

Benefits:

- **Idempotent ingest.** Two authors asserting the same axiom independently
  produce the same axiom IRI. The Oxigraph adapter's IRI-uniqueness ASK
  (ADR-11 §D6) deduplicates automatically.
- **Verifiable.** Any consumer can recompute the IRI from the content;
  mismatches indicate corruption.
- **Cross-reference-friendly.** A `prov:wasDerivedFrom` reference to an
  axiom by IRI is stable across re-parses.

Provenance still attaches to the axiom: the IRI is content-addressed,
but `wasAttributedTo` records who first asserted it. Two authors
asserting the same axiom produces one axiom IRI and two provenance
triples — both attribuations, both timestamps, both preserved.

### D11. Schema versioning — `@context` URL carries `v1`

Breaking changes to the schema mint a new context file:

```
docs/data-sprint/context-v1.jsonld   → schemaVersion 1
docs/data-sprint/context-v2.jsonld   → schemaVersion 2 (future)
```

The public URL likewise: `https://narrativegoldmine.com/context/v1.jsonld` vs
`/v2.jsonld`. The parser inspects the `@context` URL string; unknown
versions raise:

```
ValidationError {
    code: "UNKNOWN_SCHEMA_VERSION",
    found: "https://narrativegoldmine.com/context/v9.jsonld",
    supported: ["v1"],
    suggestion: "Upgrade webxr to a version that supports v9, or rewrite
                 the document to v1."
}
```

Each document MAY also assert `vc:schemaVersion` as an integer for
defensive cross-checking. The parser verifies that the URL version and
the asserted version match.

Non-breaking additions to v1 (new predicates, new aliases) edit
`context-v1.jsonld` in place and bump no version. Removing a predicate
or changing its semantics is a breaking change and mints v2.

### D12. Validation — pre-commit + pre-ingest, same code path

Validation runs twice:

1. **Pre-commit hook** in the Logseq authoring repository
   (`.git/hooks/pre-commit` calling `vc-validate` against modified
   `.md` files). Fast failure for authors before the data leaves the
   editor.

2. **Pre-ingest validator** in `webxr` invoked by the GitHub adapter
   before any triples reach Oxigraph. Defends the store against
   misformatted assertions slipping past the pre-commit hook (e.g. a
   file edited outside the canonical authoring repo).

Both validators execute the same `vc-validate` crate. The crate
contract:

```rust
pub fn validate_markdown(source: &str, options: ValidatorOptions)
    -> Result<ValidatedDocument, Vec<ValidationError>>;
```

Checks performed:

- **A. JSON-LD expansion** — every block expands to a valid JSON-LD
  document. Malformed JSON, missing `@context`, undeclared terms fail.
- **B. Schema version** — `@context` URL matches a supported version;
  `vc:schemaVersion` (if present) matches the URL.
- **C. Required terms** — every block has `@id`, `@type`, `prov:wasAttributedTo`,
  `prov:generatedAtTime`.
- **D. Identity scheme** — `@id` matches one of the URN schemes from
  D2; `wasAttributedTo` is a `did:nostr:*` IRI.
- **E. OWL 2 EL profile** — emitted triples use only allowed constructs
  (D4). Out-of-profile axioms are rejected with a spec reference.
- **F. Named-graph routing** — assertions land in the named graph
  matching their `@type` (D7).
- **G. SHACL-lite shape checks** — minimal shape constraints inlined in
  the crate (not a separate `.shacl` file). Examples: `qualityScore` ∈
  [0, 1], `renderTier` ∈ {0, 1, 2, 3}, `maturity` ∈ {stub, draft, mature,
  authoritative}.
- **H. Wikilink resolution** — every `[[Term]]` either resolves to a
  declared term or generates a `LinkedPage` placeholder. The validator
  reports unresolved links as warnings (not errors) in pre-commit, as
  upgradable placeholders in pre-ingest.
- **I. Signature verification** — pre-ingest only: NIP-23 event exists,
  signature valid, content hash matches.

Pre-commit failures block the commit. Pre-ingest failures emit a
structured error event and skip the offending file; the operator
dashboard surfaces the queue of failed files for review.

### D13. Migration tooling — one-shot codemod for 199 pages

A standalone binary lives at `tools/migrate-logseq-to-jsonld/src/main.rs`
(a cargo workspace member). Its inputs are a Logseq corpus directory
and the project lead's Nostr key. Its single mode of operation:

1. Walk the corpus; identify files with `public:: true` in frontmatter.
2. For each file:
   a. Extract Logseq `property:: value` blocks and frontmatter.
   b. Extract `### OntologyBlock` sections.
   c. Map heuristically to canonical predicates via the table in
      §"The vc: vocabulary" below.
   d. Synthesise a `Page` JSON-LD block.
   e. Synthesise `OntologyClass` / `OntologyProperty` / `Axiom` blocks
      from the OntologyBlocks.
   f. Compute content hash; sign with the project lead's key (NIP-23).
   g. Write the transformed markdown back to disk.
3. Emit a parity report: count of pages, ontology classes, properties,
   axioms, unresolved wikilinks before/after.
4. Exit.

The codemod is reviewed by a human, committed to the Logseq corpus
repository, and retired. There is no live conversion layer in `webxr`;
the canonical format on disk is the authoritative format.

### D14. Compatibility with whelk-rs

Every axiom emitted by the parser round-trips through whelk-rs without
loss. The round-trip test:

```rust
let canonical_axiom = parse_jsonld_block(input);
let whelk_axiom = whelk::convert_to_internal(canonical_axiom.clone());
let normalised = whelk::normalize(whelk_axiom);
let reemitted = whelk::convert_from_internal(normalised);
assert_eq!(canonical_axiom.subject, reemitted.subject);
assert_eq!(canonical_axiom.predicate, reemitted.predicate);
assert_eq!(canonical_axiom.object, reemitted.object);
```

Constructs that whelk-rs cannot represent (i.e., the OWL 2 DL constructs
listed in D4 as rejected) fail the round-trip and the validator
rejects them upstream. The validator is therefore the canonical
authority on profile compatibility, not whelk-rs itself.

### D15. Compatibility with Oxigraph

Every `@context` term resolves to a fully-qualified IRI. The JSON-LD
expansion produces N-Quads in which no predicate is a relative IRI; the
Oxigraph adapter writes the quads verbatim into the appropriate named
graph.

SPARQL queries against the store use the W3C vocabulary directly:

```sparql
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
PREFIX vc:   <urn:visionflow:owl:property:>
PREFIX vcclass: <urn:visionflow:owl:class:>

SELECT ?class ?label
FROM <urn:visionflow:graph:ontology:assert>
WHERE {
  ?class a owl:Class ;
         rdfs:label ?label ;
         rdfs:subClassOf vcclass:cybernetics .
}
```

Consumers do not need to know that `label` is the friendly alias in
JSON-LD; at the SPARQL layer everything is the expanded predicate. This
is exactly the "no drift between systems" property the user asked for.

## Concrete worked examples

### Example 1 — A Page

```markdown
---
public:: true
---

# Communication and Control

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "@type": "Page",
  "title": "Communication and Control",
  "subject": ["[[Cybernetics]]", "[[Information Theory]]"],
  "wasAttributedTo": "did:nostr:abc123def456…",
  "generatedAtTime": "2026-05-16T10:00:00Z",
  "schemaVersion": 1,
  "modified": "2026-05-16T10:00:00Z",
  "tags": ["systems", "control-theory"]
}
```

The cornerstone concept of cybernetics is that systems regulate themselves
through feedback. See also [[Negative Feedback]].
```

Canonical RDF emission (N-Quads):

```
<urn:visionflow:page:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://schema.org/WebPage> <urn:visionflow:graph:knowledge> .
<urn:visionflow:page:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855> <https://schema.org/name> "Communication and Control"@en <urn:visionflow:graph:knowledge> .
<urn:visionflow:page:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855> <http://purl.org/dc/terms/subject> <urn:visionflow:owl:class:cybernetics> <urn:visionflow:graph:knowledge> .
<urn:visionflow:page:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855> <http://purl.org/dc/terms/subject> <urn:visionflow:owl:class:information-theory> <urn:visionflow:graph:knowledge> .
<urn:visionflow:page:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855> <http://www.w3.org/ns/prov#wasAttributedTo> <did:nostr:abc123def456…> <urn:visionflow:graph:knowledge> .
<urn:visionflow:page:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855> <http://www.w3.org/ns/prov#generatedAtTime> "2026-05-16T10:00:00Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <urn:visionflow:graph:knowledge> .
```

### Example 2 — An OntologyClass

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:cybernetics",
  "@type": "OntologyClass",
  "label": "Cybernetics",
  "altLabel": ["Cybernetic Theory"],
  "definition": "The science of communication and control in animals and machines.",
  "subClassOf": "[[Systems Theory]]",
  "definedIn": {"@id": "urn:visionflow:page:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"},
  "sourceDomain": "systems-science",
  "qualityScore": 0.92,
  "authorityScore": 0.85,
  "maturity": "mature",
  "renderTier": 2,
  "wasAttributedTo": "did:nostr:abc123def456…",
  "generatedAtTime": "2026-05-16T10:00:00Z"
}
```

### Example 3 — An OntologyProperty

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:property:regulates",
  "@type": "OntologyProperty",
  "label": "regulates",
  "definition": "Indicates that the subject controls or modulates the object via feedback.",
  "domain": "[[Cybernetics]]",
  "range": "[[System]]",
  "subPropertyOf": "vc:relatesTo",
  "wasAttributedTo": "did:nostr:abc123def456…",
  "generatedAtTime": "2026-05-16T10:00:00Z"
}
```

### Example 4 — An explicit Axiom

For axioms that are not implied by a `subClassOf` triple on a class
declaration (e.g. property chains, existential restrictions, or
axioms produced by reasoning), authors emit `Axiom` blocks directly:

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:axiom:7f3a2b1c…",
  "@type": "Axiom",
  "subject": {"@id": "urn:visionflow:owl:class:cybernetics"},
  "type": "owl:Restriction",
  "onProperty": {"@id": "urn:visionflow:owl:property:regulates"},
  "someValuesFrom": {"@id": "urn:visionflow:owl:class:system"},
  "wasAttributedTo": "did:nostr:abc123def456…",
  "generatedAtTime": "2026-05-16T10:00:00Z"
}
```

This asserts "Cybernetics ⊑ ∃regulates.System" — an EL-profile
existential restriction.

### Example 5 — A Bridge

A `bridgeTo` relation explicitly links a knowledge-graph page to its
ontology-class counterpart, per ADR-08 §D2:

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "bridgeTo": {"@id": "urn:visionflow:owl:class:cybernetics"},
  "wasAttributedTo": "did:nostr:abc123def456…",
  "generatedAtTime": "2026-05-16T10:00:00Z"
}
```

The bridge quad crosses named graphs (knowledge → ontology). Per
ADR-11 §D2 the bridge triples live in the default graph; the
validator confirms this on write.

### Example 6 — Agent telemetry

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@graph": [
    {
      "@id": "urn:visionflow:agent:run-2026-05-16-001:step-7",
      "@type": "AgentTelemetry",
      "runId": "run-2026-05-16-001",
      "step": 7,
      "agentKind": "coder",
      "tool": "Edit",
      "outcome": "success",
      "latencyMs": 230,
      "wasAttributedTo": "did:nostr:agent-key-hex…",
      "generatedAtTime": "2026-05-16T10:01:00Z",
      "wasGeneratedBy": {"@id": "urn:visionflow:agent:run-2026-05-16-001"}
    }
  ]
}
```

The `@graph` wrapper forces this block into `urn:visionflow:graph:agent`
regardless of the host page's default graph.

### Example 7 — Multi-graph page

A page can carry assertions for multiple named graphs in a single block
via explicit `@graph` keys:

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@graph": [
    {
      "@id": "urn:visionflow:page:abc…",
      "@type": "Page",
      "title": "Cybernetics — Concept Page",
      "wasAttributedTo": "did:nostr:abc…",
      "generatedAtTime": "2026-05-16T10:00:00Z"
    },
    {
      "@id": "urn:visionflow:owl:class:cybernetics",
      "@type": "OntologyClass",
      "label": "Cybernetics",
      "definition": "…",
      "subClassOf": "[[Systems Theory]]",
      "wasAttributedTo": "did:nostr:abc…",
      "generatedAtTime": "2026-05-16T10:00:00Z"
    }
  ]
}
```

The parser routes each entry by `@type`: the Page goes to
`urn:visionflow:graph:knowledge`, the OntologyClass to
`urn:visionflow:graph:ontology:assert`. Authors no longer need
to write two separate fenced blocks for the common Page-plus-Class
pattern (though they may).

## The `vc:` vocabulary

This table is exhaustive for VisionFlow-specific predicates as of v1.
Every term is documented with its W3C-vocabulary mapping (if any) and
its Logseq-property-name mapping (the syntax authors used pre-sprint).
Adding a term post-v1 requires a Pull Request that updates both this
table AND `context-v1.jsonld`.

| Friendly alias       | Canonical IRI                                          | W3C re-export          | Old Logseq property         | OwlClass V2 field         | Datatype          |
|----------------------|--------------------------------------------------------|------------------------|------------------------------|---------------------------|-------------------|
| `label`              | `rdfs:label`                                           | `rdfs:label` + `skos:prefLabel` + `schema:name` | n/a (multiple variants) | `label`                   | `xsd:string@en`   |
| `prefLabel`          | `skos:prefLabel`                                       | direct                 | `preferred_term::`           | `preferred_term`          | `xsd:string@en`   |
| `altLabel`           | `skos:altLabel`                                        | direct                 | `alias::`                    | (n/a; new in v1)          | `xsd:string@en` set |
| `title`              | `schema:name`                                          | direct                 | n/a (page heading)           | n/a                       | `xsd:string@en`   |
| `definition`         | `skos:definition`                                      | direct                 | `definition::`               | `description`             | `xsd:string@en`   |
| `description`        | `schema:description`                                   | direct                 | `description::`              | `description`             | `xsd:string@en`   |
| `comment`            | `rdfs:comment`                                         | direct                 | n/a                          | n/a                       | `xsd:string@en`   |
| `subClassOf`         | `rdfs:subClassOf`                                      | direct                 | `subclassOf::`               | `parent_classes`          | `@id` set         |
| `subPropertyOf`      | `rdfs:subPropertyOf`                                   | direct                 | `subpropertyOf::`            | n/a                       | `@id` set         |
| `equivalentClass`    | `owl:equivalentClass`                                  | direct                 | `equivalentClass::`          | n/a                       | `@id` set         |
| `someValuesFrom`     | `owl:someValuesFrom`                                   | direct                 | n/a                          | n/a                       | `@id`             |
| `onProperty`         | `owl:onProperty`                                       | direct                 | n/a                          | n/a                       | `@id`             |
| `propertyChainAxiom` | `owl:propertyChainAxiom`                               | direct                 | n/a                          | n/a                       | `@id` list        |
| `domain`             | `rdfs:domain`                                          | direct                 | `domain::`                   | `domain`                  | `@id` set         |
| `range`              | `rdfs:range`                                           | direct                 | `range::`                    | `range`                   | `@id` set         |
| `bridgeTo`           | `vc:bridgeTo`                                          | new                    | `bridgesTo::`                | `bridges_to`              | `@id` set         |
| `bridgeFrom`         | `vc:bridgeFrom`                                        | new                    | `bridgesFrom::`              | `bridges_from`            | `@id` set         |
| `hasPart`            | `vc:hasPart`                                           | new                    | `hasPart::`                  | `has_part`                | `@id` set         |
| `isPartOf`           | `vc:isPartOf`                                          | new                    | `isPartOf::`                 | `is_part_of`              | `@id` set         |
| `requires`           | `vc:requires`                                          | new                    | `requires::`                 | `requires`                | `@id` set         |
| `dependsOn`          | `vc:dependsOn`                                         | new                    | `dependsOn::`                | `depends_on`              | `@id` set         |
| `enables`            | `vc:enables`                                           | new                    | `enables::`                  | `enables`                 | `@id` set         |
| `relatesTo`          | `vc:relatesTo`                                         | new                    | `relatesTo::`                | `relates_to`              | `@id` set         |
| `definedIn`          | `vc:definedIn`                                         | new                    | n/a (host page implicit)     | `source_file`             | `@id`             |
| `wikilinkTo`         | `vc:wikilinkTo`                                        | new                    | `[[Target]]`                 | n/a (extracted at parse)  | `@id` set         |
| `wasAttributedTo`    | `prov:wasAttributedTo`                                 | direct                 | n/a (new in v1)              | n/a                       | `@id`             |
| `wasDerivedFrom`     | `prov:wasDerivedFrom`                                  | direct                 | n/a                          | n/a                       | `@id`             |
| `wasGeneratedBy`     | `prov:wasGeneratedBy`                                  | direct                 | n/a                          | n/a                       | `@id`             |
| `generatedAtTime`    | `prov:generatedAtTime`                                 | direct                 | n/a                          | `last_synced`             | `xsd:dateTime`    |
| `invalidatedAt`      | `prov:invalidatedAtTime`                               | direct                 | n/a                          | n/a                       | `xsd:dateTime`    |
| `sourceDomain`       | `vc:sourceDomain`                                      | new                    | `domain::` (when narrative)  | `source_domain`           | `xsd:string`      |
| `qualityScore`       | `vc:qualityScore`                                      | new                    | `qualityScore::`             | `quality_score`           | `xsd:float`       |
| `authorityScore`     | `vc:authorityScore`                                    | new                    | `authorityScore::`           | `authority_score`         | `xsd:float`       |
| `maturity`           | `vc:maturity`                                          | new                    | `maturity::`                 | `maturity`                | `xsd:string` enum |
| `status`             | `vc:status`                                            | new                    | `status::`                   | `status`                  | `xsd:string` enum |
| `physicality`        | `vc:physicality`                                       | new                    | `owl:physicality::`          | `owl_physicality`         | `xsd:string`      |
| `role`               | `vc:role`                                              | new                    | `owl:role::`                 | `owl_role`                | `xsd:string`      |
| `classType`          | `vc:classType`                                         | new                    | `classType::`                | `class_type`              | `xsd:string`      |
| `renderTier`         | `vc:renderTier`                                        | new (Section 4)        | `renderTier::`               | (added by migration 0043) | `xsd:integer`     |
| `ontologyTier`       | `vc:ontologyTier`                                      | new (Section 4)        | `ontologyTier::`             | (added by migration 0044) | `xsd:integer`     |
| `mass`               | `vc:mass`                                              | new (Section 1)        | n/a                          | n/a                       | `xsd:float`       |
| `schemaVersion`      | `vc:schemaVersion`                                     | new                    | n/a                          | n/a                       | `xsd:integer`     |
| `stub`               | `vc:stub`                                              | new                    | n/a                          | n/a                       | `xsd:boolean`     |
| `public`             | `vc:public`                                            | new                    | `public::`                   | `public_access`           | `xsd:boolean`     |
| `source`             | `dcterms:source`                                       | direct                 | `source::`                   | `source_file`             | `@id`             |
| `sourcePath`         | `vc:sourcePath`                                        | new                    | n/a                          | `source_file`             | `xsd:string`      |
| `contentHash`        | `vc:contentHash`                                       | new                    | n/a                          | `file_sha1`               | `xsd:string`      |
| `lastSynced`         | `vc:lastSynced`                                        | new                    | n/a                          | `last_synced`             | `xsd:dateTime`    |
| `created`            | `dcterms:created`                                      | direct                 | `created::`                  | n/a                       | `xsd:dateTime`    |
| `modified`           | `dcterms:modified`                                     | direct                 | `updated::`                  | n/a                       | `xsd:dateTime`    |
| `signedBy`           | `vc:signedBy`                                          | new                    | n/a                          | n/a                       | `@id`             |
| `signatureEvent`     | `vc:signatureEvent`                                    | new                    | n/a                          | n/a                       | `xsd:string`      |
| `runId`              | `vc:runId`                                             | new (Section 7)        | n/a                          | n/a                       | `xsd:string`      |
| `step`               | `vc:step`                                              | new (Section 7)        | n/a                          | n/a                       | `xsd:integer`     |
| `agentKind`          | `vc:agentKind`                                         | new (Section 7)        | n/a                          | n/a                       | `xsd:string`     |
| `tool`               | `vc:tool`                                              | new (Section 7)        | n/a                          | n/a                       | `xsd:string`     |
| `outcome`            | `vc:outcome`                                           | new (Section 7)        | n/a                          | n/a                       | `xsd:string`     |
| `latencyMs`          | `vc:latencyMs`                                         | new (Section 7)        | n/a                          | n/a                       | `xsd:integer`    |
| `creator`            | `dcterms:creator`                                      | direct                 | `author::`                   | n/a                       | `@id`             |
| `license`            | `dcterms:license`                                      | direct                 | `license::`                  | n/a                       | `@id`             |
| `subject`            | `dcterms:subject`                                      | direct                 | `subject::`                  | n/a                       | `@id` set         |
| `tags`               | `schema:keywords`                                      | direct                 | `tags::`                     | n/a                       | `xsd:string` set  |

The OwlClass V2 fields `term_id`, `version`, `content_status`,
`belongs_to_domain`, `markdown_content`, `additional_metadata`,
`other_relationships`, `properties` are explicitly **dropped** in v1:

- `term_id` → replaced by the URN itself.
- `version` → replaced by `vc:schemaVersion` (document level).
- `content_status` → merged into `vc:status`.
- `belongs_to_domain` → identical to `vc:sourceDomain`.
- `markdown_content` → the markdown source IS the storage; not a triple.
- `additional_metadata` → free-form JSON has no place in RDF. Authors
  who need extension predicates declare them under `vc:` and update
  this table.
- `other_relationships` (HashMap<String, Vec<String>>) → each
  custom relationship becomes an explicit `vc:` predicate.
- `properties` (HashMap<String, String>) → same.

This drop is intentional. The 40+-field OwlClass was a symptom of
schema-by-accretion. The canonical schema is opinionated by design.

## Options considered

### O1. Logseq `property::` convention without JSON-LD

Rejected. Logseq property syntax is free-form, untyped, and parses
identically regardless of meaning. There is no way to assert "this
value is an IRI reference, not a string" in `property:: value`
without convention. We end up rebuilding schema-on-read in the parser
heuristics — which is exactly the situation we are escaping. The
heuristics are the bug.

### O2. schema.org as primary vocabulary instead of OWL

Rejected. schema.org is excellent for general-purpose semantic markup
(pages, articles, products) and we DO re-export it (`Page` → `schema:WebPage`,
`title` → `schema:name`). But schema.org is not an OWL ontology in the
reasoner-friendly sense: it lacks proper class axioms, has loose domain
and range definitions, and is not designed for the kind of restricted
subset (EL) that whelk-rs reasons over. We use schema.org as the
*surface* vocabulary for documents-about-the-world; OWL is the *spine*
for the reasoner. The two coexist through the `@context`.

### O3. OWL 2 DL instead of OWL 2 EL

Rejected. OWL 2 DL is more expressive (negation, disjunction, universal
restrictions, complex cardinality) but does not have a tractable
in-tree reasoner. Adopting DL would either: (a) require a new reasoner
dependency (HermiT, Pellet — both JVM, both heavy), or (b) accept
that we cannot reason over our own assertions. Neither is acceptable.
EL is the largest profile whelk-rs handles; we hold to EL and reject
out-of-profile axioms at validation time with a clear error.

If a future need for DL emerges, the path is documented: add an
out-of-profile annotation graph, run a heavyweight reasoner offline,
materialise its output into a new named graph. This is forward-compatible
with v1; it requires no schema change.

### O4. JSON-LD 1.1 + OWL 2 EL + `vc:` extension vocabulary (this ADR)

Adopted. The full rationale spans D1–D15 above.

## Risks

### R1. Migration burden

Transforming 199 existing `public:: true` Logseq pages into the
canonical schema is non-trivial. The codemod (D13) must handle:

- Inconsistent `property:: value` shapes (some `subClassOf:: [[X]]`,
  others `subClassOf:: X`, others `parent:: X`).
- Free-form text in fields that should be IRIs (`domain:: systems
  science` — must canonicalise to `urn:visionflow:owl:class:systems-science`).
- Missing required fields (no author key in many existing pages).

**Mitigation**: the codemod runs in three passes: (1) extract; (2)
canonicalise with explicit warning log per ambiguous input; (3) emit.
A human reviews the diff before commit. Pages with unresolvable
ambiguity remain in their current form and are flagged for manual
edit. The 199-page corpus is small enough for human review.

### R2. Validator complexity

`vc-validate` runs nine distinct checks (D12 §A–I), some of which
require network access (signature verification) or substantial CPU
(JSON-LD framing of large documents). A slow validator becomes a
bottleneck for both pre-commit and pre-ingest.

**Mitigation**: signature verification is cached by event ID (the
NIP-23 event ID is stable; the validator only re-verifies on miss).
JSON-LD framing uses a streaming expansion pass rather than the
full algorithm. Profile-check is a static lookup table. Target
budget: <50ms per page on pre-commit, <100ms on pre-ingest.

### R3. JSON-LD tooling availability

The Rust JSON-LD ecosystem is less mature than the JavaScript one
(`jsonld.js` is canonical; Rust has `json-ld-rs` and `sophia_jsonld`
but both at single-digit version numbers).

**Mitigation**: `vc-validate` wraps `sophia_jsonld` for expansion and
N-Quad emission. If `sophia_jsonld` proves insufficient, we fall back
to shelling out to a vendored Node binary running `jsonld.js`. The
overhead is acceptable for an offline validator; not acceptable on the
hot ingest path, where we mandate a Rust-native solution.

### R4. DID resolution dependencies

`did:nostr:<pubkey>` is a self-resolving identifier (the pubkey IS the
identity), but downstream consumers may want richer metadata (display
name, NIP-05 verified identity, avatar URL). This sprint explicitly
defers that to a separate sprint.

**Mitigation**: the schema is forward-compatible. When DID resolution
lands, the resolver looks up additional facts about the pubkey and
emits them as triples in a new `urn:visionflow:graph:identity` named
graph, without changing the canonical assertions in the asserted
ontology graph. No schema migration is required.

### R5. OWL 2 EL expressivity ceiling

Authors will eventually want to express something EL does not support
(disjointness, negation, universal restrictions). The clear error
message at validation time directs them to model the assertion as an
annotation rather than a reasoned axiom — but some authors will
experience this as a restriction rather than guidance.

**Mitigation**: the error message includes a forward path
(annotation graph). The migration sprint section §"Cross-references"
documents the annotation-graph pattern. A future sprint may introduce
a heavyweight offline reasoner without breaking v1.

### R6. Nostr signature key management

Authors need a Nostr key. Key loss means inability to sign future
edits (and arguably calls into question past attributions). Key
rotation needs a documented procedure.

**Mitigation**: deferred to the separate DID-resolution sprint. For
v1 MVP, the project lead's key is the trust root, and individual
authors can either sign with their own keys (preferred) or have the
project lead co-sign (acceptable for the 199-page corpus). The
schema itself does not constrain key rotation; signatures are events
with timestamps, and a re-signed page produces a new event without
invalidating prior events.

## Cross-references

The following documents receive amendments after this ADR lands. Each
edit is scoped and minimal; existing decisions are NOT re-litigated,
only reconciled with this schema.

### `docs/adr/ADR-032-embed-solid-pod-rs-library.md` (related, no edits)

This data-sprint ADR composes with ADR-032's decision to embed
`solid-pod-rs` as an in-process Rust library replacing the JSS sidecar.
The two ADRs touch the same identity / signing / storage surface from
opposite ends: ADR-032 provides the in-process mechanism, this ADR
specifies the *schema* the mechanism reads and writes.

Specific touch points (no wording changes required in either ADR; just
shared awareness):

- **§D2 identity scheme** — `did:nostr:<pubkey>` is the canonical
  identifier; ADR-032's `solid-pod-rs-nostr` crate is the in-tree
  resolver, NIP-05 endpoint, and Schnorr verifier. The DID is opaque
  to this schema, but `solid-pod-rs-nostr` is the implementation that
  resolves it when a consumer asks "who is `did:nostr:abc…`?".
- **§D8 provenance** — `prov:wasAttributedTo did:nostr:<pubkey>` is
  signed by the same pod-resident key managed by ADR-032's
  `PodResidentSigner` (the single source of truth replacing the three
  prior keygen paths). The schema does not specify how the key is
  generated or stored; that's ADR-032 M3's job.
- **§D9 Nostr signature wrapper** — NIP-23 verification happens via
  `solid-pod-rs`'s NIP-98 verifier (the same Schnorr primitive).
  Signed pages flow through the pod-storage layer.
- **§D6 page storage** — when a markdown file is ingested, it lands
  in a Solid pod resource per LDP semantics (per ADR-032). The
  canonical IRI minting rule (`urn:visionflow:page:<sha256(canonical-path)>`)
  is independent of pod URLs — pods address by container path; the
  IRI is the stable cross-system identifier.
- **No conflict with agentbox ADR-010** — ADR-032 explicitly contrasts
  with agentbox's binary-aggregation pattern. VisionClaw's AGPL-3.0
  licence allows the in-process embedding that agentbox rejects.

The migration-sprint documents below receive scoped amendments.

### `docs/migration-sprint/08-ontology-graph-data/PRD-08.md`

- Restate capabilities in W3C vocabulary terms. Replace mentions of
  OwlClass V2 fields (`quality_score`, `authority_score`, `maturity`,
  etc.) with their `vc:` predicate equivalents per the table above.
- Update the "node types" enumeration to use `OntologyClass`,
  `OntologyProperty`, `Page`, `LinkedPage`, `Axiom`, `AgentTelemetry`
  consistently — these are now canonical-schema type names, not
  loose label-strings.

### `docs/migration-sprint/08-ontology-graph-data/DDD-08.md`

- The `Page` aggregate's `metadata`, `title`, `body_excerpt` field
  surface aligns to `dcterms:` and `schema:` predicates per the
  vocabulary table.
- The `OntologyClass` aggregate's 40-field surface drops to the
  canonical 15-or-so predicates. Aggregate invariants remain unchanged
  (`P1`, `P2`, `P3`, `P4`); only the field representation changes.
- §"Ubiquitous language" gains entries for `wasAttributedTo`,
  `generatedAtTime`, `definedIn`, `bridgeTo`, `vc:` (extension
  vocabulary).

### `docs/migration-sprint/11-persistence-migration/ADR-11.md`

- §D3 "IRI minting" harmonises with this ADR's §D2 URN schemes. The
  existing `vc:kg/<slug>`, `vc:onto/<slug>` patterns become
  `urn:visionflow:page:<sha256(path)>`,
  `urn:visionflow:owl:class:<slug>` per this ADR. The `vc:` prefix in
  ADR-11 expands to `https://visionflow.dreamlab/ns/`; in this ADR
  `vc:` expands to `urn:visionflow:owl:property:`. **The migration
  sprint adopts this ADR's expansion.** The reserved public hostname
  `visionflow.dreamlab` becomes `narrativegoldmine.com`.
- §D2 named-graph naming: the asserted ontology graph rename from
  `urn:visionflow:graph:ontology` to `urn:visionflow:graph:ontology:assert`
  (per TENSIONS-RESOLVED §CC-4 and this ADR §D7) is finalised. ADR-11
  receives the rename.
- §D5 audit_log column names: `actor_pubkey` aligns with this ADR's
  `did:nostr:<pubkey>` scheme. Per-row provenance recoverable by
  joining `audit_log.actor_pubkey` to the `wasAttributedTo` triples
  on the corresponding ingest event.

### `docs/migration-sprint/07-bots-telemetry/ADR-07.md` and `DDD-07.md`

- Agent telemetry events conform to the `AgentTelemetry` JSON-LD type
  defined in this ADR §"Example 6". The wire envelope (per
  TENSIONS-RESOLVED §T7's `AgentActionEnvelope`) maps to canonical
  predicates `runId`, `step`, `agentKind`, `tool`, `outcome`,
  `latencyMs`. ADR-07 absorbs the mapping table.

### `docs/migration-sprint/10-external-integrations/ADR-10.md`

- The GitHub adapter (§D11 per TENSIONS-RESOLVED §CC-15) produces
  `ParsedMarkdown` value objects per ADR-08 §D10. The schema for
  `ParsedMarkdown` is now defined by this ADR §D1–§D7. ADR-10's
  GitHub-adapter ACL section gains a `vc-validate` invocation as the
  last step of `ParsedMarkdown` construction.

## Phasing

| Phase  | Owner                         | Deliverable                                                                       | Status        |
|--------|-------------------------------|------------------------------------------------------------------------------------|---------------|
| D-1    | This ADR                      | ADR-D01 + `context-v1.jsonld` + fixtures directory layout                          | This document |
| D-2    | Tooling                       | `vc-validate` crate at `crates/vc-validate/` — JSON-LD expansion + SHACL-lite      | Next          |
| D-3    | Migration codemod             | `tools/migrate-logseq-to-jsonld/` — one-shot transform of 199 `public:: true`      | After D-2     |
| D-4    | Migration sprint Phase 2      | Section 8 (Ontology & Graph Data) implementation consumes this schema              | Hand-off      |
| D-5    | Separate sprint               | DID resolution, NIP-05 enrichment, signature timestamping, key-rotation procedure  | Deferred      |

Phases D-1 through D-4 are sequential. D-2 cannot begin without D-1
landed. D-3 cannot begin without D-2 stable enough to validate output.
D-4 is the migration sprint's existing Phase 2; it consumes this schema
as input and changes its implementation language but not its
deliverable.

## Bugs and smells at the reset point (`6910969b5`)

To flag for migration awareness:

- The baseline has the 40+-field OwlClass V2 struct in
  `src/ports/ontology_repository.rs:52`. This ADR replaces it. The
  port surface (the `OntologyRepository` trait at line 258) is
  unchanged in this ADR; only the value type changes. The migration
  sprint's Phase 2 (Section 8) carries out the renaming.
- The baseline reads three different syntaxes for the same data
  (Logseq `property::`, frontmatter, `### OntologyBlock`). The
  knowledge_graph_parser at `src/services/parsers/knowledge_graph_parser.rs`
  encodes this in heuristic code. After this sprint, the parser
  reads ONE syntax (fenced JSON-LD blocks). The Logseq-compatibility
  shim (reading `property::` lines into JSON-LD blocks) lives in the
  codemod (D13), not in `webxr`.
- The baseline does not version its schema. Every parse run is
  against an unstated schema version. After this sprint, every
  document declares `vc:schemaVersion` and a versioned `@context`
  URL; old documents without these are flagged for codemod.
- The baseline has no provenance — every assertion is anonymous and
  undated except via filesystem metadata. After this sprint,
  `prov:wasAttributedTo` and `prov:generatedAtTime` are mandatory.
- The baseline lacks page signatures. After this sprint (D9), every
  published page is signed by its author.

## Open questions tracked for future sprints

These are intentionally NOT decided in v1: NIP-05 enrichment,
relay discovery, signature timestamping (OpenTimestamps), Nostr-key
rotation, SHACL Compact Syntax externalisation, multi-lingual labels
beyond `@language: "en"`, inference-graph subscription, and federated
ontologies (schema.org RDF dump, Wikidata fragments). Each is
forward-compatible with v1 — the schema admits these additions
without breaking changes.

## Acceptance criteria

This ADR is accepted when:

- A1. The three deliverables (this ADR, `context-v1.jsonld`, README)
  are in `docs/data-sprint/`.
- A2. `context-v1.jsonld` is valid JSON and a valid JSON-LD 1.1
  `@context` (every term resolves; no circular references; no
  ambiguous aliases).
- A3. The vocabulary table covers every OwlClass V2 field with either
  a mapping or an explicit drop.
- A4. The OWL 2 EL profile boundary lists every rejected construct
  with a spec reference.
- A5. The seven worked examples expand to valid N-Quads in the
  appropriate named graph.
- A6. The cross-references to PRD-08, DDD-08, ADR-11, ADR-07, ADR-10
  enumerate every required amendment.
- A7. Risks R1–R6 each have a concrete mitigation.
- A8. The phasing table chains correctly: each phase's prerequisite
  is the prior phase's deliverable.

The migration sprint's Phase 2 (`docs/migration-sprint/README.md`
§"Implementation phasing") cannot begin until this ADR is accepted.
