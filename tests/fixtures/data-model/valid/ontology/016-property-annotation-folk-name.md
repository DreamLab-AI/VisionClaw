public:: true
title:: folkName (annotation property)
ontology:: true

# vc:folkName — Annotation Property

An annotation property attaching colloquial / vernacular names to
architectural works (e.g. "il Cupolone" for the Florence Cathedral
dome). Annotation properties carry no logical force — they cannot be
referenced in axioms — but they are essential for natural-language
search and UI labelling.

### OntologyBlock

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:property:folk-name",
  "@type": ["OntologyProperty", "owl:AnnotationProperty"],
  "ontology": true,
  "rdfs:label": "folkName",
  "vc:definition": "A vernacular or affectionate name for an architectural work as used by inhabitants of its host city. Examples: 'il Cupolone' for Santa Maria del Fiore's dome; 'il Bernino' for Bernini's colonnade. Distinct from rdfs:label, which carries the formal name.",
  "vc:propertyKind": "AnnotationProperty",
  "rdfs:domain": { "@id": "urn:visionflow:owl:class:architectural-work" },
  "rdfs:range": { "@id": "http://www.w3.org/2000/01/rdf-schema#Literal" },
  "vc:status": "active",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1carlo00000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:22:00Z", "@type": "xsd:dateTime" }
}
```
