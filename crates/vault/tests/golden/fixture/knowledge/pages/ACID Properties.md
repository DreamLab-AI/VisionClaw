---
public: true
---

# ACID Properties

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v1",
  "@id": "urn:visionflow:page:acid-properties",
  "@type": "Page",
  "title": "ACID Properties",
  "vc:slug": "acid-properties",
  "vc:public": true,
  "vc:schemaVersion": 2,
  "vc:outboundWikilinks": []
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v2.jsonld",
  "@id": "urn:ngm:class:acid-properties",
  "@type": "Class",
  "label": "ACID Properties",
  "definition": "ACID Properties are the four guarantees - Atomicity, Consistency, Isolation and Durability - that define a reliable database transaction. Atomicity ensures a transaction is all-or-nothing; Consistency preserves invariants; Isolation hides concurrent intermediate states; and Durability persists committed results across failures. Together they let applications reason about correctness despite concurrency and crashes, distinguishing strongly consistent transactional systems from eventually consistent alternatives.",
  "domain": "distributed-systems",
  "maturity": "mature",
  "subClassOf": [
    {
      "@id": "urn:ngm:class:transaction",
      "label": "Transaction"
    }
  ],
  "relations": {
    "partOf": [
      {
        "@id": "urn:ngm:class:transaction",
        "label": "Transaction"
      }
    ],
    "hasPart": [
      {
        "@id": "urn:ngm:class:data-integrity",
        "label": "Data Integrity"
      },
      {
        "@id": "urn:ngm:class:fault-tolerance",
        "label": "Fault Tolerance"
      }
    ],
    "requires": [
      {
        "@id": "urn:ngm:class:write-ahead-logging",
        "label": "Write-Ahead Logging"
      },
      {
        "@id": "urn:ngm:class:two-phase-commit",
        "label": "Two-Phase Commit"
      }
    ],
    "implements": [
      {
        "@id": "urn:ngm:class:data-integrity",
        "label": "Data Integrity"
      },
      {
        "@id": "urn:ngm:class:reliability",
        "label": "Reliability"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:relational-database",
        "label": "Relational Database"
      },
      {
        "@id": "urn:ngm:class:data-integrity",
        "label": "Data Integrity"
      },
      {
        "@id": "urn:ngm:class:reliability",
        "label": "Reliability"
      }
    ],
    "supports": [
      {
        "@id": "urn:ngm:class:database-management-system",
        "label": "Database Management System"
      },
      {
        "@id": "urn:ngm:class:database-systems",
        "label": "Database Systems"
      }
    ],
    "dependsOn": [
      {
        "@id": "urn:ngm:class:write-ahead-logging",
        "label": "Write-Ahead Logging"
      },
      {
        "@id": "urn:ngm:class:data-persistence",
        "label": "Data Persistence"
      }
    ],
    "uses": [
      {
        "@id": "urn:ngm:class:two-phase-commit",
        "label": "Two-Phase Commit"
      }
    ],
    "contrastsWith": [
      {
        "@id": "urn:ngm:class:scalability",
        "label": "Scalability"
      }
    ],
    "relatedTo": [
      {
        "@id": "urn:ngm:class:transaction",
        "label": "Transaction"
      },
      {
        "@id": "urn:ngm:class:data-persistence",
        "label": "Data Persistence"
      },
      {
        "@id": "urn:ngm:class:distributed-systems",
        "label": "Distributed Systems"
      }
    ]
  },
  "sameAs": [
    {
      "@id": "urn:ngm:class:acid",
      "label": "ACID"
    }
  ],
  "quality": 0.62,
  "provenance": {
    "attributedTo": "did:nostr:ontology-mesh",
    "generatedAt": "2026-06-15T00:00:00Z",
    "inferenceRule": "GapMaterialisation"
  }
}
```

- ### Definition
  - ACID Properties are the four guarantees - Atomicity, Consistency, Isolation and Durability - that define a reliable database transaction. Atomicity ensures a transaction is all-or-nothing; Consistency preserves invariants; Isolation hides concurrent intermediate states; and Durability persists committed results across failures. Together they let applications reason about correctness despite concurrency and crashes, distinguishing strongly consistent transactional systems from eventually consistent alternatives.
  - Closely related concepts include [[Transaction]], [[Write-Ahead Logging]], [[Data Integrity]], [[Relational Database]], [[Distributed Systems]].
- ### Overview
  - [[Transaction]] provides the broader context within which ACID Properties sits.
  - ACID Properties is treated here as a member of the **distributed-systems** domain at **mature** maturity.
  - It is referenced across the knowledge graph by classes that depend on or compose it, making it a central node that warranted an explicit definition.
- ### Key aspects
  - It comprises [[Data Integrity]].
  - It comprises [[Fault Tolerance]].
  - It requires [[Write-Ahead Logging]].
  - It requires [[Two-Phase Commit]].
- ### Mechanisms
  - ACID Properties operates by combining its constituent parts into a coherent capability that other classes can rely upon.
  - Its guarantees and behaviours are realised through the dependencies and standards captured in the relations below.
- ### Applications
  - Enables and supports [[Relational Database]].
  - Enables and supports [[Data Integrity]].
  - Enables and supports [[Reliability]].
  - Enables and supports [[Database Management System]].
  - Enables and supports [[Database Systems]].
- ### Relationships
  - partOf:: [[Transaction]]
  - hasPart:: [[Data Integrity]]
  - hasPart:: [[Fault Tolerance]]
  - requires:: [[Write-Ahead Logging]]
  - requires:: [[Two-Phase Commit]]
  - implements:: [[Data Integrity]]
  - implements:: [[Reliability]]
  - enables:: [[Relational Database]]
  - enables:: [[Data Integrity]]
  - enables:: [[Reliability]]
  - supports:: [[Database Management System]]
  - supports:: [[Database Systems]]
  - dependsOn:: [[Write-Ahead Logging]]
  - dependsOn:: [[Data Persistence]]
  - uses:: [[Two-Phase Commit]]
  - contrastsWith:: [[Scalability]]
  - relatedTo:: [[Transaction]]
  - relatedTo:: [[Data Persistence]]
  - relatedTo:: [[Distributed Systems]]
  - subClassOf:: [[Transaction]]
- ### Provenance
  - attributedTo:: did:nostr:ontology-mesh
  - inferenceRule:: GapMaterialisation
  - quality:: 0.62
  - updated:: 2026-06-15
