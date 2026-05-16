public:: true
title:: Dome Construction (multi-domain ontology class)
ontology:: true

# Dome Construction — Multi-Domain Class

A class that spans two source domains: *architecture* (the formal and
aesthetic concerns) and *engineering* (the structural concerns).
Brunelleschi's Florence Cathedral dome is the canonical example of a
work that cannot be understood from either perspective alone.

### OntologyBlock

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:owl:class:dome-construction",
  "@type": ["OntologyClass", "owl:Class"],
  "ontology": true,
  "rdfs:label": "Dome Construction",
  "vc:definition": "The set of techniques and design decisions involved in building a dome — a hemispherical or near-hemispherical structural element capping a circular or polygonal space. Includes the structural analysis of compressive thrust, the geometric setting-out of the form, and the architectural decisions about openings, lantern, and exterior shell.",
  "rdfs:subClassOf": { "@id": "urn:visionflow:owl:class:structural-typology" },
  "vc:sourceDomain": ["architecture", "engineering"],
  "vc:belongsToDomain": "architecture",
  "vc:bridgesToDomain": "engineering",
  "vc:hasPart": [
    { "@id": "urn:visionflow:owl:class:pendentive" },
    { "@id": "urn:visionflow:owl:class:squinch" },
    { "@id": "urn:visionflow:owl:class:oculus" },
    { "@id": "urn:visionflow:owl:class:double-shell-construction" }
  ],
  "vc:requires": [
    { "@id": "urn:visionflow:owl:class:compressive-thrust-resolution" }
  ],
  "vc:enables": [
    { "@id": "urn:visionflow:owl:class:basilical-vaulting" }
  ],
  "vc:status": "active",
  "vc:maturity": "stable",
  "vc:qualityScore": { "@value": "0.94", "@type": "xsd:float" },
  "vc:authorityScore": { "@value": "0.90", "@type": "xsd:float" },
  "vc:owlPhysicality": "physical",
  "vc:owlRole": "process",
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:15:00Z", "@type": "xsd:dateTime" }
}
```
