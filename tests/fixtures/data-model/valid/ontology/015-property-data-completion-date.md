public:: true
title:: completionDate (data property)
ontology:: true

# vc:completionDate — Data Property

A data property carrying the year an architectural work was completed.
Range is `xsd:gYear` (Gregorian year, no month/day precision). The Rust
enum kind is `DataProperty` (note: the trait spelling is *DataProperty*,
NOT *DatatypeProperty*; the OWL2 wire form `owl:DatatypeProperty` is
emitted at the adapter layer).

### OntologyBlock

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:property:completion-date",
  "@type": ["OntologyProperty", "owl:DatatypeProperty"],
  "ontology": true,
  "rdfs:label": "completionDate",
  "vc:definition": "The year in which an architectural work was completed and brought into use. For works completed in stages, the canonical completion year is the year the principal structure was first usable for its intended purpose (e.g. consecration date for a church).",
  "vc:propertyKind": "DataProperty",
  "rdfs:domain": { "@id": "urn:visionflow:owl:class:architectural-work" },
  "rdfs:range": { "@id": "http://www.w3.org/2001/XMLSchema#gYear" },
  "vc:characteristics": {
    "vc:functional": true
  },
  "vc:status": "active",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1carlo00000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:21:00Z", "@type": "xsd:dateTime" }
}
```
