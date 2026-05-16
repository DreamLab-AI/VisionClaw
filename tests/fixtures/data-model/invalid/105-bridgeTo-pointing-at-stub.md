public:: true
title:: Invalid — Bridge points at LinkedPage stub
invalid:: true
expected-error:: BridgeTargetMustBeConcrete

# INVALID FIXTURE

A BridgeRecord whose `vc:bridgeTo` targets a `LinkedPage` placeholder
is incoherent. Bridges connect concrete entities; a placeholder is by
definition unresolved. This is the most common author mistake — they
asserted the bridge in advance of the target's declaration. The
validator catches it because the target IRI has the `urn:visionflow:linked:`
scheme which is reserved for placeholders.

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:bridge:bad-bridge-001",
  "@type": "BridgeRecord",
  "vc:bridgeFrom": { "@id": "urn:visionflow:page:7e6f5d4c3b2a1908172635445362718e9f0a1b2c3d4e5f6071829a3b4c5d6e02" },
  "vc:bridgeTo": { "@id": "urn:visionflow:linked:tempietto" },
  "vc:bridgeKind": "PageToOntologyClass",
  "vc:matchedBy": "normalised-label",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1sync000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T12:25:00Z", "@type": "xsd:dateTime" }
}
```
