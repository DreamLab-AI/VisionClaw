public:: true
title:: Architectural Period Taxonomy
ontology:: true

# Architectural Period — Deep Class Hierarchy

This page declares four ontology classes that establish the chain
`Built Environment → Architecture → Architectural Period → Renaissance Architecture`
(depth-3 subClassOf chain — exercises the parity harness's transitive
subclass query path).

### OntologyBlock — Built Environment (root)

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:built-environment",
  "@type": ["OntologyClass", "owl:Class"],
  "ontology": true,
  "rdfs:label": "Built Environment",
  "vc:definition": "The human-made surroundings that provide the setting for human activity: buildings, infrastructure, and the arrangement of land. Distinct from natural environment.",
  "vc:status": "active",
  "vc:maturity": "stable",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:10:00Z", "@type": "xsd:dateTime" }
}
```

### OntologyBlock — Architecture

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:architecture",
  "@type": ["OntologyClass", "owl:Class"],
  "ontology": true,
  "rdfs:label": "Architecture",
  "vc:definition": "The art and discipline of designing and constructing buildings and other physical structures. A subclass of Built Environment that emphasises intentional design rather than mere occupancy of space.",
  "rdfs:subClassOf": { "@id": "urn:visionflow:owl:class:built-environment" },
  "vc:status": "active",
  "vc:maturity": "stable",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:10:05Z", "@type": "xsd:dateTime" }
}
```

### OntologyBlock — Architectural Period

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:architectural-period",
  "@type": ["OntologyClass", "owl:Class"],
  "ontology": true,
  "rdfs:label": "Architectural Period",
  "vc:definition": "A historically-bounded set of architectural conventions, materials, and goals. Categorises individual buildings by their participation in a recognisable style and time. Examples include Romanesque, Gothic, Renaissance, Baroque.",
  "rdfs:subClassOf": { "@id": "urn:visionflow:owl:class:architecture" },
  "vc:status": "active",
  "vc:maturity": "stable",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:10:10Z", "@type": "xsd:dateTime" }
}
```

### OntologyBlock — Italian Renaissance (leaf, depth 4 from root)

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:italian-renaissance",
  "@type": ["OntologyClass", "owl:Class"],
  "ontology": true,
  "rdfs:label": "Italian Renaissance",
  "vc:definition": "Renaissance architecture as practised in Italy from the early Quattrocento (Brunelleschi) through the Cinquecento (Bramante, Palladio). Distinct in its access to surviving Roman built precedent and to a continuous classical-Latin literary tradition.",
  "rdfs:subClassOf": { "@id": "urn:visionflow:owl:class:renaissance-architecture" },
  "vc:status": "active",
  "vc:maturity": "stable",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:10:15Z", "@type": "xsd:dateTime" }
}
```
