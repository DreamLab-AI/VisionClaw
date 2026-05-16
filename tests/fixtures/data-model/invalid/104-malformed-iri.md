public:: true
title:: Invalid — Malformed IRI
invalid:: true
expected-error:: MalformedIri

# INVALID FIXTURE

The `@id` contains whitespace and uses no IRI scheme separator. IRIs
must conform to RFC 3987; any whitespace is rejected.

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn visionflow page malformed id with spaces",
  "@type": "Page",
  "vc:slug": "malformed-iri",
  "vc:title": "Malformed IRI",
  "vc:public": true,
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T12:20:00Z", "@type": "xsd:dateTime" }
}
```
