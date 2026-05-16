public:: true
title:: Florentine Quattrocento (ontology class)
ontology:: true

# Florentine Quattrocento — Ontology Class

The Florentine Quattrocento covers Renaissance architecture in Florence
during the 15th century. This OntologyClass declares its bridges to
related concepts and its part-of relationships.

### OntologyBlock

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:florentine-quattrocento",
  "@type": ["OntologyClass", "owl:Class"],
  "ontology": true,
  "rdfs:label": "Florentine Quattrocento",
  "vc:termId": "FLOR-QUAT",
  "vc:definition": "Renaissance architecture as practised in Florence during the 15th century (the Quattrocento). The defining works are the Florence Cathedral Dome (Brunelleschi, 1436), the Palazzo Medici-Riccardi (Michelozzo, 1444), and the Palazzo Rucellai (Alberti, 1455).",
  "rdfs:subClassOf": { "@id": "urn:visionflow:owl:class:italian-renaissance" },
  "vc:bridgeTo": [
    { "@id": "urn:visionflow:page:b1a2c3d4e5f60718293a4b5c6d7e8f9012a3b4c5d6e7f8091a2b3c4d5e6f7081" }
  ],
  "vc:hasPart": [
    { "@id": "urn:visionflow:owl:class:medici-patronage" },
    { "@id": "urn:visionflow:owl:class:florentine-guild-system" }
  ],
  "vc:requires": [
    { "@id": "urn:visionflow:owl:class:vitruvian-tradition" }
  ],
  "vc:enables": [
    { "@id": "urn:visionflow:owl:class:high-renaissance" }
  ],
  "vc:relatesTo": [
    { "@id": "urn:visionflow:owl:class:roman-architecture" }
  ],
  "vc:status": "active",
  "vc:maturity": "stable",
  "vc:qualityScore": { "@value": "0.91", "@type": "xsd:float" },
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:05:00Z", "@type": "xsd:dateTime" }
}
```
