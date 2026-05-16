public:: false
title:: LinkedPage placeholder — Vitruvian Tradition
note:: This fixture represents a placeholder created by an unresolved wikilink. The placeholder will upgrade to OntologyClass once 010 / 012 are also ingested.

# LinkedPage Placeholder — Vitruvian Tradition

When a wikilink `[[Vitruvian Tradition]]` is encountered before any
declaration for that label, the parser creates a `LinkedPage`
placeholder. The placeholder has a `vc:slug`-derived IRI and an
`upgradedTo: null` field. On observation of a matching `OntologyClass`
declaration (anywhere in the corpus), the placeholder is upgraded
in-place and a `LinkResolved` event is emitted.

### OntologyBlock

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:linked:vitruvian-tradition",
  "@type": "LinkedPage",
  "rdfs:label": "Vitruvian Tradition",
  "vc:slug": "vitruvian-tradition",
  "vc:firstSeenIn": { "@id": "urn:visionflow:page:b1a2c3d4e5f60718293a4b5c6d7e8f9012a3b4c5d6e7f8091a2b3c4d5e6f7081" },
  "vc:upgradedTo": null,
  "vc:status": "dangling",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1sync000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T09:13:00Z", "@type": "xsd:dateTime" }
}
```

### Lifecycle note

If a future ingest run encounters an OntologyClass with `rdfs:label
"Vitruvian Tradition"`, the placeholder upgrades:

- The placeholder's `NodeId` sequence bits are preserved.
- The class bits flip from `0x08000000` (LinkedPage) to `0x04000000`
  (OntologyClass).
- A `LinkResolved` domain event is emitted.
- This placeholder record stays in the audit log but is excluded from
  the `GraphTopology` projection.
