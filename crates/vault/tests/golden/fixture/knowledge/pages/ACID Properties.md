---
type: Class
title: ACID Properties
resource: urn:ngm:class:acid-properties
page_resource: urn:visionflow:page:acid-properties
public: true
status: stable
domain: distributed-systems
maturity: mature
quality: 0.62
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links: []
is-a:
- '[[transaction|Transaction]]'
contrasts-with:
- '[[scalability|Scalability]]'
depends-on:
- '[[write-ahead-logging|Write-Ahead Logging]]'
- '[[data-persistence|Data Persistence]]'
enables:
- '[[relational-database|Relational Database]]'
- '[[data-integrity|Data Integrity]]'
- '[[reliability|Reliability]]'
has-part:
- '[[data-integrity|Data Integrity]]'
- '[[fault-tolerance|Fault Tolerance]]'
implements:
- '[[data-integrity|Data Integrity]]'
- '[[reliability|Reliability]]'
part-of:
- '[[transaction|Transaction]]'
related-to:
- '[[transaction|Transaction]]'
- '[[data-persistence|Data Persistence]]'
- '[[distributed-systems|Distributed Systems]]'
requires:
- '[[write-ahead-logging|Write-Ahead Logging]]'
- '[[two-phase-commit|Two-Phase Commit]]'
supports:
- '[[database-management-system|Database Management System]]'
- '[[database-systems|Database Systems]]'
uses:
- '[[two-phase-commit|Two-Phase Commit]]'
---

ACID Properties are the four guarantees - Atomicity, Consistency, Isolation and Durability - that define a reliable database transaction. Atomicity ensures a transaction is all-or-nothing; Consistency preserves invariants; Isolation hides concurrent intermediate states; and Durability persists committed results across failures. Together they let applications reason about correctness despite concurrency and crashes, distinguishing strongly consistent transactional systems from eventually consistent alternatives.

### Overview

- [[Transaction]] provides the broader context within which ACID Properties sits.
- ACID Properties is treated here as a member of the **distributed-systems** domain at **mature** maturity.
- It is referenced across the knowledge graph by classes that depend on or compose it, making it a central node that warranted an explicit definition.

### Key aspects

- It comprises [[Data Integrity]].
- It comprises [[Fault Tolerance]].
- It requires [[Write-Ahead Logging]].
- It requires [[Two-Phase Commit]].

### Mechanisms

- ACID Properties operates by combining its constituent parts into a coherent capability that other classes can rely upon.
- Its guarantees and behaviours are realised through the dependencies and standards captured in the relations below.

### Applications

- Enables and supports [[Relational Database]].
- Enables and supports [[Data Integrity]].
- Enables and supports [[Reliability]].
- Enables and supports [[Database Management System]].
- Enables and supports [[Database Systems]].

### Provenance
