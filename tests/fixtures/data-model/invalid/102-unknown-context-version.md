public:: true
title:: Invalid — Unknown @context version
invalid:: true
expected-error:: ContextVersionUnknown

# INVALID FIXTURE

The context URL references `v99.jsonld`. Only `v1.jsonld` is currently
accepted; a fictional v99 represents a schema the validator has no
mapping for. The registry must reject unknown versions to prevent silent
schema drift.

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v99.jsonld",
  "@id": "urn:visionflow:owl:class:future-shock",
  "@type": "OntologyClass",
  "rdfs:label": "Future Shock",
  "vc:definition": "A class that uses an unreleased context version.",
  "rdfs:subClassOf": { "@id": "urn:visionflow:owl:class:architectural-period" },
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T12:10:00Z", "@type": "xsd:dateTime" }
}
```
