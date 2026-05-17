# URN-Solid Vocabulary Mapping

**Status**: ADR-054 ratified (2026-04-19)
**Purpose**: Maps VisionClaw's per-domain vocabulary IRIs (`bc:`, `mv:`, `rb:`, `dt:`, `ai:`) and common RDF heads to URN-Solid canonical terms (`urn:solid:<Name>`) for ecosystem alignment.
**Consumed by**: `src/services/urn_solid_mapping.rs` (parser + hot-reloader).
**Gated by**: `URN_SOLID_ALIGNMENT=true` — when off, this table is inert data.

## Format

Each row binds one of our IRIs to its canonical URN-Solid term. `Canonical vocab`
is the well-known upstream vocabulary that URN-Solid itself `owl:sameAs`es to
(foaf / schema / dcterms / vcard / activitystreams / prov / ldp / solid). `Status`
is one of:

- `stable` — mapping emitted as `owl:sameAs` on every ingest
- `proposed` — listed but not yet emitted; pending community review
- `deferred` — intentionally not mapped; reason recorded inline

Parser contract: the loader in `urn_solid_mapping.rs` reads every row whose
first non-whitespace character is `|`, skips the header + separator, and ignores
rows that begin with `<!--`.

## Mappings

| Our class (IRI) | `urn:solid:<Name>` | Canonical vocab | Status |
|-----------------|--------------------|:----------------|--------|
| `bc:Person` | `urn:solid:Person` | `foaf:Person` | stable |
| `bc:Document` | `urn:solid:Document` | `schema:CreativeWork` | stable |
| `bc:Event` | `urn:solid:Event` | `schema:Event` | stable |
| `bc:Organization` | `urn:solid:Organization` | `foaf:Organization` | stable |
| `bc:Policy` | `urn:solid:Policy` | `schema:CreativeWork` | stable |
| `bc:BrokerCase` | `urn:solid:Case` | `schema:Thing` | stable |
| `bc:SmartContract` | `urn:solid:SmartContract` | `schema:SoftwareApplication` | proposed |
| `bc:Transaction` | `urn:solid:Transaction` | `schema:MonetaryAmount` | stable |
| `bc:Claim` | `urn:solid:Claim` | `schema:Claim` | stable |
| `bc:Agent` | `urn:solid:Agent` | `foaf:Agent` | stable |
| `mv:Person` | `urn:solid:Person` | `foaf:Person` | stable |
| `mv:Agent` | `urn:solid:Agent` | `foaf:Agent` | stable |
| `mv:Avatar` | `urn:solid:Avatar` | `foaf:Person` | proposed |
| `mv:Brain` | `urn:solid:CognitiveAgent` | `prov:Agent` | proposed |
| `mv:Company` | `urn:solid:Organization` | `foaf:Organization` | stable |
| `mv:Organization` | `urn:solid:Organization` | `foaf:Organization` | stable |
| `mv:Concept` | `urn:solid:Concept` | `skos:Concept` | stable |
| `mv:Entity` | `urn:solid:Thing` | `schema:Thing` | stable |
| `mv:Event` | `urn:solid:Event` | `schema:Event` | stable |
| `mv:Location` | `urn:solid:Place` | `schema:Place` | stable |
| `mv:Project` | `urn:solid:Project` | `schema:Project` | stable |
| `mv:Technology` | `urn:solid:Technology` | `schema:SoftwareApplication` | stable |
| `mv:VirtualEntity` | `urn:solid:VirtualEntity` | `schema:Thing` | proposed |
| `mv:Tool` | `urn:solid:Tool` | `schema:Product` | stable |
| `mv:Idea` | `urn:solid:Idea` | `schema:CreativeWork` | stable |
| `rb:Book` | `urn:solid:Book` | `schema:Book` | stable |
| `rb:Recipe` | `urn:solid:Recipe` | `schema:Recipe` | stable |
| `rb:Ingredient` | `urn:solid:Ingredient` | `schema:DefinedTerm` | stable |
| `rb:Note` | `urn:solid:Note` | `activitystreams:Note` | stable |
| `dt:Dataset` | `urn:solid:Dataset` | `schema:Dataset` | stable |
| `dt:DataPoint` | `urn:solid:DataPoint` | `schema:Observation` | proposed |
| `dt:Measurement` | `urn:solid:Measurement` | `schema:QuantitativeValue` | stable |
| `dt:Schema` | `urn:solid:Schema` | `schema:DataCatalog` | stable |
| `ai:Agent` | `urn:solid:Agent` | `foaf:Agent` | stable |
| `ai:AgentPattern` | `urn:solid:AgentPattern` | `schema:CreativeWork` | proposed |
| `ai:LargeLanguageModel` | `urn:solid:LanguageModel` | `schema:SoftwareApplication` | stable |
| `ai:NeuralNetwork` | `urn:solid:NeuralNetwork` | `schema:SoftwareApplication` | stable |
| `ai:Prompt` | `urn:solid:Prompt` | `schema:CreativeWork` | stable |
| `foaf:Person` | `urn:solid:Person` | `foaf:Person` | stable |
| `foaf:Agent` | `urn:solid:Agent` | `foaf:Agent` | stable |
| `foaf:Organization` | `urn:solid:Organization` | `foaf:Organization` | stable |
| `schema:Person` | `urn:solid:Person` | `schema:Person` | stable |
| `schema:Organization` | `urn:solid:Organization` | `schema:Organization` | stable |
| `schema:Event` | `urn:solid:Event` | `schema:Event` | stable |
| `schema:Place` | `urn:solid:Place` | `schema:Place` | stable |
| `schema:Thing` | `urn:solid:Thing` | `schema:Thing` | stable |
| `schema:CreativeWork` | `urn:solid:CreativeWork` | `schema:CreativeWork` | stable |
| `ldp:Container` | `urn:solid:Container` | `ldp:Container` | stable |
| `solid:Note` | `urn:solid:Note` | `activitystreams:Note` | stable |
| `solid:TypeIndex` | `urn:solid:TypeIndex` | `solid:TypeIndex` | stable |

## Curation notes

- `bc:SomeClass` is a placeholder in test fixtures and intentionally unmapped.
- `mv:Brain` maps to `urn:solid:CognitiveAgent` as `proposed` — URN-Solid does
  not yet have a `Brain` term; the `CognitiveAgent` proposal is under discussion
  upstream. Emit nothing until status flips to `stable`.
- Domain-specific RingBet (`rb:`) and DataTelemetry (`dt:`) heads map to generic
  schema.org equivalents via URN-Solid; richer bindings await those domains
  getting their own `urn:solid:` terms.
- Refresh cadence: manual, against URN-Solid upstream `corpus.jsonl`. No
  auto-sync — registry drift would otherwise break our production emission.
