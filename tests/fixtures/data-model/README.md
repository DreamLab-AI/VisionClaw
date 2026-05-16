# VisionFlow Data Sprint — Model Test Fixtures

Synthetic Logseq markdown files with embedded JSON-LD blocks that conform
to the canonical VisionFlow source-data schema (ADR-08, ADR-11). These
fixtures are the model corpus consumed by the parser, the ontology
adapter, the graph adapter, and the parity test harness.

## Domain

The fixtures use **Italian Renaissance architecture** as their toy
domain: a small, coherent corpus with rich subclass structure
(`Built Environment ⊐ Architecture ⊐ Architectural Period ⊐ Renaissance Architecture
⊐ Italian Renaissance ⊐ Florentine Quattrocento`) and genuine ontology
relationships among real architects, buildings, and concepts. Definitions
are accurate to the level a domain reader would find acceptable.

## Schema

Every JSON-LD block is JSON-LD 1.1 and references the canonical
`@context` URL `https://narrativegoldmine.com/context/v1.jsonld`. The actual
context file is authored separately (parallel ADR sprint) and lives at
`docs/data-sprint/context-v1.jsonld`.

Identity forms used:

| Form                                           | Used for                          |
|------------------------------------------------|-----------------------------------|
| `did:nostr:npub1<pubkey-bech32>`               | People, agents, signing identities|
| `urn:visionflow:page:<sha256>`                 | Page entities                     |
| `urn:visionflow:owl:class:<slug>`              | OntologyClass entities            |
| `urn:visionflow:owl:property:<slug>`           | OntologyProperty entities         |
| `urn:visionflow:owl:axiom:<sha256-12>`         | Axiom entities (content-addressed)|
| `urn:visionflow:linked:<slug>`                 | LinkedPage placeholders           |
| `urn:visionflow:agent:<run-id>:<step>`         | AgentTelemetry events             |
| `urn:visionflow:bridge:<source>:<target>`      | BridgeRecord entries              |

Named graphs:

| IRI                                            | Contents                          |
|------------------------------------------------|-----------------------------------|
| `urn:visionflow:graph:knowledge`               | Pages, wikilinks, bridges (KG)    |
| `urn:visionflow:graph:ontology:assert`         | OntologyClass/Property/Axiom      |
| `urn:visionflow:graph:ontology:inferred`       | Whelk-derived inferred axioms     |
| `urn:visionflow:graph:agent`                   | Agent telemetry                   |
| `(default)`                                    | Cross-graph bridge records        |

Class-bit mapping (per TC-1 resolution in T1-class-bits.md):

| `@type`           | Class flag bit | Routed to graph                          |
|-------------------|----------------|------------------------------------------|
| `Page`            | `0x40000000`   | `urn:visionflow:graph:knowledge`         |
| `OntologyClass`   | `0x04000000`   | `urn:visionflow:graph:ontology:assert`   |
| `LinkedPage`      | `0x08000000`   | `urn:visionflow:graph:knowledge`         |
| `Axiom`           | `0x0C000000`   | `urn:visionflow:graph:ontology:assert`   |
| `OntologyProperty`| `0x10000000`   | `urn:visionflow:graph:ontology:assert`   |
| `AgentTelemetry`  | `0x80000000`   | `urn:visionflow:graph:agent`             |
| `BridgeRecord`    | `0x40000000`   | default graph (cross-graph)              |

## OWL 2 EL profile

Only `subClassOf`, `equivalentClass`, `propertyChainAxiom`, and
`someValuesFrom` are permitted. `owl:unionOf`, `owl:complementOf`,
`owl:allValuesFrom`, and `owl:disjointWith` are NOT in the EL profile
and any fixture using them lives in `invalid/`.

## Provenance

Every JSON-LD block carries:

- `prov:wasAttributedTo` — a `did:nostr:` identity from `pubkeys.json`.
- `prov:generatedAtTime` — an `xsd:dateTime` UTC timestamp.

These are required by the validator. A block without either is invalid.

## Directory layout

```
valid/
├── pages/        Page-vocabulary fixtures (the Logseq surface)
├── ontology/     OntologyClass, OntologyProperty, Axiom fixtures
├── agents/       AgentTelemetry fixtures
├── bridges/      Cross-graph bridge records (default named graph)
├── signed/       Nostr-signed and PROV-O-attributed fixtures
└── metadata/     corpus-manifest.json with per-file expectations

invalid/          Each file must be REJECTED by the validator;
                  README.md inside lists the expected error per file.

seed/             seed-oxigraph.rs (loader script) +
                  expected-triples.nq (N-Quads ground truth)
```

## How to use

### From the parity test harness

The parity harness (`tests/adapter_parity/`) constructs adapters and
exercises them. To use these fixtures end-to-end:

1. Load the N-Quads from `seed/expected-triples.nq` into an Oxigraph
   `Store` (this is the "what the parser+adapter should have produced
   from `valid/`" ground truth).
2. Or: run the parser over the markdown files in `valid/`, write the
   resulting domain events through the `OntologyRepository` /
   `GraphRepository` adapters, then ask the same adapters for their
   triple counts and compare against `corpus-manifest.json`.

### From a fresh dev iteration

```bash
# Compile the seed loader.
cargo build --bin seed-oxigraph --features persistence-oxigraph

# Load fixtures into a scratch Oxigraph store.
./target/debug/seed-oxigraph --store /tmp/fx --nquads tests/fixtures/data-model/seed/expected-triples.nq

# Inspect.
./target/debug/seed-oxigraph --store /tmp/fx --report
```

The seed script reports node/edge/axiom counts and class-bit
distribution; compare against `valid/metadata/corpus-manifest.json`.

## Conformance rules

1. Every file under `valid/` MUST contain at least one fenced ```json-ld
   code block with valid JSON inside.
2. Every JSON-LD block MUST reference the canonical `@context` URL.
3. Every block MUST declare `prov:wasAttributedTo` (a DID from
   `pubkeys.json`) and `prov:generatedAtTime` (xsd:dateTime).
4. Wikilink `[[Term]]` desugars deterministically — see slug rule below.
5. Class hierarchies use `subClassOf` with `@id` references; no inline
   subclass declarations.
6. The four named graphs are each exercised by at least one fixture
   (see `corpus-manifest.json`).
7. Stub upgrade lifecycle (LinkedPage → OntologyClass) is exercised by
   `ontology/020-linked-page-stub.md` (placeholder) + the upgrade event
   in `ontology/010-class-renaissance-architecture.md`.

## Slug rule

`slug(s)`: NFKC-normalise, lowercase, replace each non-alphanumeric run
with a single `-`, trim leading and trailing `-`. Empty string maps to
`unnamed`. This is the same rule used by
`src/adapters/oxigraph_ontology_repository.rs::slug()`.

Examples:
- `"Florence Cathedral"` → `florence-cathedral`
- `"Leon Battista Alberti's Influence"` → `leon-battista-alberti-s-influence`
- `"OWL 2 EL"` → `owl-2-el`

## Trip hazards

- **`OntologyProperty` of `DataProperty` kind, not `DatatypeProperty`.**
  The Rust enum is `PropertyType::DataProperty`. The OWL2 wire
  representation is `owl:DatatypeProperty`. Both are accepted as input
  in the JSON-LD layer.
- **Axiom IRI is content-addressed.** Two fixtures asserting the same
  `(subject, predicate, object)` axiom will share an `@id`. This is
  intentional — replay/idempotency is invariant.
- **`LinkedPage` placeholders use stable IDs across upgrades.** The
  `NodeId` sequence bits stay the same; only the class bits flip on
  upgrade. The fixture corpus does not exercise the sequence-bit
  invariant directly (it's a runtime ID-allocator concern), but it
  exercises the upgrade event.

## Authoring guide for additions

If you add a fixture:

1. Put it in `valid/<subdir>/NNN-name.md` or `invalid/NNN-name.md` with
   a fresh sequence number (don't reuse).
2. Add an entry in `valid/metadata/corpus-manifest.json` with the
   expected named graph, class bit, and triple count.
3. Append the expected N-Quads to `seed/expected-triples.nq` and verify
   the seed loader produces matching counts.
4. If it covers an OWL 2 EL violation, put it in `invalid/` and add the
   expected validator error to `invalid/README.md`.

## What this corpus does NOT exercise

- Concurrent writes (single-writer semantics from ADR-11).
- Settings repository (Section 11 owns this; settings fixtures live
  elsewhere).
- GPU physics positions (these are runtime ephemera, not persisted).
- WebSocket binary protocol (Section 2 owns its own fixture set).
