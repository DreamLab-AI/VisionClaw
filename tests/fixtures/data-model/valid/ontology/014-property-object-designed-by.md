public:: true
title:: designedBy (OntologyProperty)
ontology:: true

# vc:designedBy — Object Property

An object property relating an architectural work to the architect
responsible for its design. Functional but not inverse-functional — a
work has one principal designer, but an architect may design many
works.

### OntologyBlock

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:property:designed-by",
  "@type": ["OntologyProperty", "owl:ObjectProperty", "owl:FunctionalProperty"],
  "ontology": true,
  "rdfs:label": "designedBy",
  "vc:definition": "Relates a building, structure, or architectural plan to the architect who provided its principal design. Inverse of vc:designed.",
  "vc:propertyKind": "ObjectProperty",
  "rdfs:domain": { "@id": "urn:visionflow:owl:class:architectural-work" },
  "rdfs:range": { "@id": "urn:visionflow:owl:class:architect" },
  "owl:inverseOf": { "@id": "urn:visionflow:owl:property:designed" },
  "vc:characteristics": {
    "vc:functional": true,
    "vc:inverseFunctional": false,
    "vc:transitive": false,
    "vc:symmetric": false
  },
  "vc:status": "active",
  "vc:maturity": "stable",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:20:00Z", "@type": "xsd:dateTime" }
}
```
