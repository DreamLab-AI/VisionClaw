public:: false
title:: Leon Battista Alberti
note:: This page is NOT public, but its OntologyBlock still surfaces — see ADR-08 D3.

# Leon Battista Alberti

Leon Battista Alberti (1404-1472) was a Genoese-born humanist active in
Florence, Rimini, and Mantua. His treatise *De re aedificatoria* (1452,
printed 1485) is the first modern theoretical work on architecture and
established the discipline as a humanist art rather than a craft.

His built work includes the [[Tempio Malatestiano]] (Rimini, 1450),
[[Palazzo Rucellai]] (Florence, 1455), and the façade of [[Santa Maria Novella]]
(Florence, 1470).

### OntologyBlock

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:humanist-architect",
  "@type": "OntologyClass",
  "ontology": true,
  "rdfs:label": "Humanist Architect",
  "vc:definition": "An architect whose practice is grounded in humanist scholarship — typically literate in Latin, conversant with the surviving texts of Vitruvius, and active across multiple liberal arts. Distinguished from craft-trained architects of the Gothic guilds by theoretical rather than purely empirical formation.",
  "rdfs:subClassOf": { "@id": "urn:visionflow:owl:class:renaissance-architect" },
  "vc:sourceDomain": ["architecture", "intellectual-history"],
  "vc:status": "active",
  "vc:maturity": "stable",
  "vc:qualityScore": { "@value": "0.92", "@type": "xsd:float" },
  "vc:authorityScore": { "@value": "0.88", "@type": "xsd:float" },
  "vc:definedIn": { "@id": "urn:visionflow:page:5566778899aabbccddeeff00112233445566778899aabbccddeeff0011223344" },
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T09:22:00Z", "@type": "xsd:dateTime" }
}
```

The host page is private (`public:: false`) but the OntologyClass above
must still produce a domain event per ADR-08 D3.
