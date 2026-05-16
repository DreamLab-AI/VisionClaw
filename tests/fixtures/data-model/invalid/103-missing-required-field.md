public:: true
title:: Invalid — OntologyClass missing subClassOf parent
invalid:: true
expected-error:: RequiredFieldMissing

# INVALID FIXTURE

Every OntologyClass except the declared root `built-environment` must
carry an `rdfs:subClassOf` link to a parent. This class is not the root
and has no parent — invariant C3 from DDD-08 is violated.

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:orphan-class",
  "@type": ["OntologyClass", "owl:Class"],
  "rdfs:label": "Orphan Class",
  "vc:definition": "A class with no declared superclass. Will dangle in the topology projection unless rejected at validation.",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T12:15:00Z", "@type": "xsd:dateTime" }
}
```
