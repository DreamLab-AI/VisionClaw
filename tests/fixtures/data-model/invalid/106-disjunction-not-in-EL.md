public:: true
title:: Invalid — owl:unionOf outside OWL 2 EL profile
invalid:: true
expected-error:: OutsideOwl2ElProfile

# INVALID FIXTURE

The OWL 2 EL profile forbids `owl:unionOf` in class expressions. EL
permits only: intersection, `someValuesFrom` over object properties,
the top/bottom classes, and `oneOf` over a single individual. Including
a union forces the reasoner to use EL+ or DL, which is outside the
profile we run in production.

The fixture below asserts `Villa Rotonda subClassOf (Palladian Villa OR
Roman Villa)` — incoherent in EL.

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:axiom:el-violator-001",
  "@type": ["Axiom", "owl:Axiom"],
  "vc:axiomType": "SubClassOf",
  "vc:subject": { "@id": "urn:visionflow:owl:class:villa-rotonda" },
  "vc:object": {
    "@type": "owl:Class",
    "owl:unionOf": {
      "@list": [
        { "@id": "urn:visionflow:owl:class:palladian-villa" },
        { "@id": "urn:visionflow:owl:class:roman-villa" }
      ]
    }
  },
  "vc:namedGraph": { "@id": "urn:visionflow:graph:ontology:assert" },
  "vc:owlProfile": "EL",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T12:30:00Z", "@type": "xsd:dateTime" }
}
```
