public:: true
title:: Axiom — Tempietto ≡ San Pietro in Montorio Pavilion
ontology:: true

# Equivalent-Class Axiom

The Tempietto and "San Pietro in Montorio Pavilion" are two labels for
the same architectural work; assert the equivalence so that wikilinks
from either label resolve to the same class.

### OntologyBlock

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:axiom:9b3e7710d2af",
  "@type": ["Axiom", "owl:Axiom"],
  "ontology": true,
  "vc:axiomType": "EquivalentClass",
  "vc:subject": { "@id": "urn:visionflow:owl:class:tempietto" },
  "vc:object": { "@id": "urn:visionflow:owl:class:san-pietro-in-montorio-pavilion" },
  "vc:source": {
    "@type": "Asserted",
    "vc:definingPage": { "@id": "urn:visionflow:page:11223344556677889900aabbccddeeff00112233445566778899aabbccddeeff" }
  },
  "vc:namedGraph": { "@id": "urn:visionflow:graph:ontology:assert" },
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1carlo00000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:31:00Z", "@type": "xsd:dateTime" }
}
```
