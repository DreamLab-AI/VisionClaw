public:: true
title:: Renaissance Architecture (ontology class)
ontology:: true

# Renaissance Architecture — Ontology Class

This page declares the canonical ontology entry for *Renaissance
architecture*. The OntologyClass below is the authoritative record;
all `[[Renaissance Architecture]]` wikilinks resolve to it.

### OntologyBlock

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@version": 1.1,
  "@id": "urn:visionflow:owl:class:renaissance-architecture",
  "@type": ["OntologyClass", "owl:Class"],
  "ontology": true,
  "rdfs:label": "Renaissance Architecture",
  "vc:termId": "REN-ARCH",
  "vc:preferredTerm": "Renaissance Architecture",
  "vc:definition": "The architecture of the period in European history known as the Renaissance, conventionally dated from the early 15th to the early 17th century. Characterised by the conscious revival of certain elements of Ancient Greek and Roman thought, in particular the orders of columns, the symmetry of plan, and the proportional relationship of parts to whole, derived from a renewed reading of Vitruvius's De architectura.",
  "rdfs:subClassOf": { "@id": "urn:visionflow:owl:class:architectural-period" },
  "vc:sourceDomain": ["architecture", "art-history"],
  "vc:version": "1.0.0",
  "vc:classType": "ArchitecturalPeriod",
  "vc:status": "active",
  "vc:maturity": "stable",
  "vc:qualityScore": { "@value": "0.95", "@type": "xsd:float" },
  "vc:authorityScore": { "@value": "0.93", "@type": "xsd:float" },
  "vc:publicAccess": true,
  "vc:owlPhysicality": "abstract",
  "vc:owlRole": "category",
  "vc:belongsToDomain": "architecture",
  "vc:hasPart": [
    { "@id": "urn:visionflow:owl:class:italian-renaissance" }
  ],
  "vc:relatesTo": [
    { "@id": "urn:visionflow:owl:class:vitruvian-tradition" },
    { "@id": "urn:visionflow:owl:class:classical-orders" }
  ],
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:00:00Z", "@type": "xsd:dateTime" }
}
```

### Upgrade trace

Prior to the corpus assembling this class, three pages referenced
`[[Renaissance Architecture]]` as a placeholder. On ingest of this
declaration, the `LinkedPage` placeholder at
`urn:visionflow:linked:renaissance-architecture` upgrades to this
`OntologyClass`:

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:event:link-resolved:renaissance-architecture",
  "@type": "LinkResolved",
  "vc:placeholderIri": "urn:visionflow:linked:renaissance-architecture",
  "vc:upgradedTo": { "@id": "urn:visionflow:owl:class:renaissance-architecture" },
  "vc:upgradeKind": "ToOntologyClass",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1sync000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:00:01Z", "@type": "xsd:dateTime" }
}
```
