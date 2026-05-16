public:: true
title:: Invalid — Missing schema version
invalid:: true
expected-error:: SchemaVersionMissing

# INVALID FIXTURE

The block below uses JSON-LD 1.1 `@included` but does not declare
`@version: 1.1`. JSON-LD 1.1 features (including `@included`,
`@version`, `@nest`, framing) require explicit version declaration.

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:axiom:0000000000ff",
  "@type": "Axiom",
  "vc:axiomType": "SubClassOf",
  "vc:subject": { "@id": "urn:visionflow:owl:class:foo" },
  "vc:object": { "@id": "urn:visionflow:owl:class:bar" },
  "@included": [
    { "@id": "urn:visionflow:activity:x", "@type": "prov:Activity" }
  ],
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T12:00:00Z", "@type": "xsd:dateTime" }
}
```
