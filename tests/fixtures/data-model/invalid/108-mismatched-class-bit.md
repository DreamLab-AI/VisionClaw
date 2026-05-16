public:: true
title:: Invalid — class-bit mismatch between @type and @id scheme
invalid:: true
expected-error:: ClassBitMismatch

# INVALID FIXTURE

The block declares `@type: OntologyClass` (class bit `0x04000000`) but
its `@id` is in the `urn:visionflow:agent:` IRI scheme, which is
reserved for AgentTelemetry events (class bit `0x80000000`). The parser
cross-checks the IRI scheme against the type declaration and rejects
mismatches — otherwise the node would be misrouted at the named-graph
adapter and visibility would silently break.

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:agent:run-impostor:step-0",
  "@type": ["OntologyClass", "owl:Class"],
  "rdfs:label": "Impostor",
  "vc:definition": "A class pretending to be an agent telemetry event by carrying the wrong IRI scheme.",
  "rdfs:subClassOf": { "@id": "urn:visionflow:owl:class:architectural-period" },
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T12:40:00Z", "@type": "xsd:dateTime" }
}
```
