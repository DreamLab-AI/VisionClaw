public:: true
title:: Invalid — Missing @context
invalid:: true
expected-error:: ContextMissing

# INVALID FIXTURE

The block has no `@context`. The validator cannot resolve any `vc:`
predicate and rejects the block outright.

```json-ld
{
  "@id": "urn:visionflow:page:00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
  "@type": "Page",
  "vc:slug": "ghost-page",
  "vc:title": "Ghost Page",
  "vc:public": true,
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T12:05:00Z", "@type": "xsd:dateTime" }
}
```
