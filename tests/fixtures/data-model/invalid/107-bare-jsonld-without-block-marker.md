public:: true
title:: Invalid — bare JSON without code-fence marker
invalid:: true
expected-error:: MissingCodeFenceMarker

# INVALID FIXTURE

The JSON content below is in a generic ``` fence (no `json-ld` language
tag). The parser only scans ```json-ld fences. Anything else is treated
as documentation and skipped, so this file produces ZERO domain events
— which is itself the validator failure (a file declared
`expected-error:: MissingCodeFenceMarker` in frontmatter but yielding
no events tripwires the validator).

```
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:invisiblepage000000000000000000000000000000000000000000000000",
  "@type": "Page",
  "vc:slug": "invisible-page",
  "vc:title": "Invisible Page",
  "vc:public": true,
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T12:35:00Z", "@type": "xsd:dateTime" }
}
```

Indented JSON is similarly invisible:

    {
      "@context": "https://narrativegoldmine.com/context/v1.jsonld",
      "@id": "urn:visionflow:page:also-invisible00000000000000000000000000000000000000000000000",
      "@type": "Page"
    }
