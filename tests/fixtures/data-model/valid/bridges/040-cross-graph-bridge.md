public:: true
title:: Bridge — Page(Filippo Brunelleschi) ↔ OntologyClass(Filippo Brunelleschi)
bridge:: true

# Cross-Graph Bridge Record

A `BridgeRecord` is a default-graph (no named graph) triple that
connects a knowledge-graph node to an ontology-graph node. The
canonical case: a `Page` about an architect, and an `OntologyClass`
asserting the architect as a typed entity in the ontology. The bridge
asserts the two are the same subject.

Bridges live in the **default graph** specifically so they are visible
to any query regardless of which named graph it scopes to — they are
the join points across the knowledge / ontology divide.

### Bridge record

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:bridge:filippo-brunelleschi-page-to-class",
  "@type": "BridgeRecord",
  "vc:bridgeFrom": { "@id": "urn:visionflow:page:b1a2c3d4e5f60718293a4b5c6d7e8f9012a3b4c5d6e7f8091a2b3c4d5e6f7081" },
  "vc:bridgeTo": { "@id": "urn:visionflow:owl:class:filippo-brunelleschi-architect" },
  "vc:bridgeKind": "PageToOntologyClass",
  "vc:matchedBy": "normalised-label",
  "vc:namedGraph": null,
  "vc:rationale": "The Page entitled 'Filippo Brunelleschi' shares its normalised label with the OntologyClass urn:visionflow:owl:class:filippo-brunelleschi-architect. ADR-08 D11 requires a bridge in this case.",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1sync000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:40:00Z", "@type": "xsd:dateTime" }
}
```
