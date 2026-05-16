public:: true
title:: Tempio Malatestiano (NIP-23-wrapped)
signed:: true

# Tempio Malatestiano

The Tempio Malatestiano (Rimini, begun 1450, never finished) is Alberti's
adaptation of a medieval Franciscan church into a personal mausoleum for
Sigismondo Malatesta. The arches of the side elevation reference Roman
triumphal arches; the unfinished façade conceals the older Gothic
structure within.

This page is wrapped as a NIP-23 long-form note — the JSON-LD block
appears inside `vc:nip23Content` and the outer envelope carries the
Nostr signature.

### Signed envelope

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:nostr:event:50a1b2c3d4e5f60718293a4b5c6d7e8f9012a3b4c5d6e7f8091a2b3c4d5e6f70",
  "@type": "NostrSignedPage",
  "vc:nostrEventKind": 30023,
  "vc:nostrPubkey": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "vc:nostrSignature": "3045022100abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890022100fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321",
  "vc:nostrCreatedAt": { "@value": "1747389600", "@type": "xsd:long" },
  "vc:nip23Content": {
    "@context": "https://narrativegoldmine.com/context/v1.jsonld",
    "@id": "urn:visionflow:page:d04e1bf3c7a2589607f12b3c4d5e6f7081a2b3c4d5e6f7081a2b3c4d5e6f7081",
    "@type": "Page",
    "vc:slug": "tempio-malatestiano",
    "vc:title": "Tempio Malatestiano",
    "vc:public": true,
    "vc:contentSha1": "cafebabecafebabecafebabecafebabecafebabe",
    "vc:bodyExcerpt": "The Tempio Malatestiano (Rimini, begun 1450, never finished) is Alberti's adaptation of a medieval Franciscan church into a personal mausoleum for Sigismondo Malatesta.",
    "vc:outboundWikilinks": [
      { "@id": "urn:visionflow:linked:leon-battista-alberti", "vc:label": "Leon Battista Alberti" },
      { "@id": "urn:visionflow:linked:sigismondo-malatesta", "vc:label": "Sigismondo Malatesta" }
    ]
  },
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T11:00:00Z", "@type": "xsd:dateTime" }
}
```
