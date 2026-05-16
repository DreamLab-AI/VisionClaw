# Invalid Fixtures — Per-file Expected Validator Error

Every file in this directory MUST be REJECTED by the validator. The
validator is the JSON-LD-block-aware schema/profile checker invoked by
the parser before any domain event is emitted. Each entry below states
the expected error category and a one-line rationale.

The error categories form a closed set:

- `SchemaVersionMissing`         — `@version` not declared (where required)
- `ContextMissing`               — `@context` absent
- `ContextVersionUnknown`        — `@context` URL not in the accepted set
- `RequiredFieldMissing`         — a required schema field is absent
- `MalformedIri`                 — an `@id` value is not a syntactically valid IRI
- `BridgeTargetMustBeConcrete`   — `vc:bridgeTo` cannot target a LinkedPage stub
- `OutsideOwl2ElProfile`         — uses an OWL construct rejected by the EL profile
- `MissingCodeFenceMarker`       — JSON appears as bare prose, not in a `json-ld` fence
- `ClassBitMismatch`             — `@type` declares a type whose class bit conflicts with `@id`
- `ProvAttributionMissing`       — `prov:wasAttributedTo` is absent
- `ProvTimestampMissing`         — `prov:generatedAtTime` is absent

## Per-file expectations

| File | Expected error | Rationale |
|------|----------------|-----------|
| `100-missing-schema-version.md` | `SchemaVersionMissing` | The `@version: 1.1` declaration is required on any block that uses JSON-LD 1.1 features (here: `@included`, typed values). The block uses `@included` without declaring `@version 1.1` — validator must reject. |
| `101-missing-context.md` | `ContextMissing` | Block has no `@context` field at all. The validator cannot resolve any of the `vc:` predicates and rejects on first scan. |
| `102-unknown-context-version.md` | `ContextVersionUnknown` | Block references `https://narrativegoldmine.com/context/v99.jsonld`. Only `v1.jsonld` is currently accepted; the registry rejects unknown versions to prevent silent schema drift. |
| `103-missing-required-field.md` | `RequiredFieldMissing` | An OntologyClass MUST carry a `rdfs:subClassOf` link to a parent class unless it is `urn:visionflow:owl:class:built-environment` (the declared root). The class here is not the root but has no parent. |
| `104-malformed-iri.md` | `MalformedIri` | The `@id` is `urn visionflow page invalid id` (spaces, missing colons). IRIs must conform to RFC 3987; any whitespace is a hard failure. |
| `105-bridgeTo-pointing-at-stub.md` | `BridgeTargetMustBeConcrete` | A `BridgeRecord` whose `vc:bridgeTo` points to a `LinkedPage` placeholder is incoherent — bridges connect concrete entities. This is the most-common author error (per ADR-08 invariant G2 + DDD-08 §L3). |
| `106-disjunction-not-in-EL.md` | `OutsideOwl2ElProfile` | Uses `owl:unionOf` to express "A subclass of (B union C)". OWL 2 EL forbids unions in class expressions (only EL-permitted operators: intersection, someValuesFrom on object properties, the top/bottom classes, and one-of with a single individual). |
| `107-bare-jsonld-without-block-marker.md` | `MissingCodeFenceMarker` | The JSON content appears in a generic ``` fence (no `json-ld` language tag) or as bare indented prose. The parser scans only ```json-ld fences; anything else is treated as documentation and skipped. The file gets ZERO events emitted — validation catches the empty event set as a likely-author-error. |
| `108-mismatched-class-bit.md` | `ClassBitMismatch` | Declares `@type: OntologyClass` but the `@id` is `urn:visionflow:agent:run-x:step-y`. The class bits implied by `@type` (`0x04000000`) conflict with the type implied by the `urn:visionflow:agent:` IRI scheme. The parser cross-checks and rejects. |
