---
title: Guide: Ontology Parser
description: **Version:** 1.0 **Date:** 2025-10-27
category: how-to
tags:
  - tutorial
  - backend
  - documentation
  - reference
  - visionclaw
updated-date: 2025-12-18
difficulty-level: intermediate
---


# Guide: Ontology Parser

> **Superseded (2026-09-22).** The `OntologyParser` module this guide describes has
> been deleted. Corpus pages are parsed once, by `vault_core::page::parse_page`
> through `src/services/page_parser.rs` (ADR-2113), and the corpus is
> frontmatter-only (ADR-2112): see [`VAULT-corpus-format.md`](../../VAULT-corpus-format.md)
> and [Ontology Pipeline](../../explanation/ontology-pipeline.md). The text below is
> kept as a historical record.

**Version:** 1.0
**Date:** 2025-10-27

---

## 1. Overview

The `OntologyParser` module is a crucial component for semantic understanding within the VisionClaw system. It is designed to parse markdown files that contain ontology definitions written in the vault format ([`VAULT-corpus-format.md`](../../VAULT-corpus-format.md)); the Logseq-style property lines ADR-2040 once tolerated carry no metadata since ADR-2112. The parser extracts OWL (Web Ontology Language) structures, including classes, properties, and axioms, which are then used to build the knowledge graph's semantic layer.

This guide provides developers with the necessary information to use the parser, understand its syntax, and integrate it into their workflows.

## 2. Core Features

-   **Markdown Ontology Block Detection:** Identifies `### OntologyBlock` markers in markdown files to isolate ontology definitions.
-   **OWL Class Extraction:** Parses class definitions including IRI, label, description, and parent classes (`subClassOf`).
-   **OWL Property Extraction:** Supports `objectProperty` (relations between entities) and `dataProperty` (relations to literal values), including their domain and range.
-   **Axiom Extraction:** Captures logical statements like `subClassOf` to define class hierarchies.
-   **Source File Tracking:** Automatically records the source file for each parsed class, aiding in traceability.

## 3. Syntax Reference

The parser uses a simple, indented syntax within a designated `OntologyBlock`.

### 3.1. Ontology Block Marker

All ontology definitions must be placed within a block marked as follows:

```markdown
- ### OntologyBlock
  ... ontology definitions go here ...
```

### 3.2. OWL Class Definition

```markdown
- owl-class:: ClassName
  - label:: Human Readable Name
  - description:: A brief description of the class.
  - subClassOf:: ParentClassName
```

**Multiple Parents:**

```markdown
- owl-class:: ChildClass
  - subClassOf:: Parent1
  - subClassOf:: Parent2
```

### 3.3. Property Definitions

**Object Property:**

```markdown
- objectProperty:: propertyName
  - label:: A human-readable label for the property
  - domain:: SourceClass
  - range:: TargetClass
```

**Data Property:**

```markdown
- dataProperty:: propertyName
  - label:: A human-readable label
  - domain:: SourceClass
  - range:: xsd:datatype  # e.g., xsd:string, xsd:integer
```

**Multiple Domains/Ranges:**

```markdown
- objectProperty:: hasRelative
  - domain:: Person, Animal
  - range:: Person, Animal
```

### 3.4. IRI Formats

The parser supports multiple IRI formats for flexibility:

```markdown
- owl-class:: SimpleName
- owl-class:: prefix:Name
- owl-class:: http://example.org/ontology#Name
```

## 4. Usage Example

The following Rust code demonstrates how to use the `OntologyParser`.

```rust
use visionclaw_server::services::parsers::OntologyParser;
use visionclaw_server::ports::ontology_repository::OntologyRepository;

async fn parse-and-store-ontology(
    repo: &impl OntologyRepository,
    markdown-content: &str,
    source-filename: &str
) -> Result<(), String> {
    // 1. Create a new parser instance
    let parser = OntologyParser::new();

    // 2. Parse the markdown content
    let ontology-data = parser.parse(markdown-content, source-filename)?;

    // 3. (Optional) Print the parsed data
    println!("Parsed {} classes, {} properties, and {} axioms.",
        ontology-data.classes.len(),
        ontology-data.properties.len(),
        ontology-data.axioms.len()
    );

    // 4. Store the parsed data in a repository
    repo.save-ontology(
        &ontology-data.classes,
        &ontology-data.properties,
        &ontology-data.axioms
    ).await.map-err(|e| e.to-string())?;

    Ok(())
}
```

## 5. Data Structures

The parser returns an `OntologyData` struct, which contains vectors of the core OWL structures.

```rust
pub struct OntologyData {
    pub classes: Vec<OwlClass>,
    pub properties: Vec<OwlProperty>,
    pub axioms: Vec<OwlAxiom>,
    pub class-hierarchy: Vec<(String, String)>, // (child-iri, parent-iri)
}

pub struct OwlClass {
    pub iri: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub parent-classes: Vec<String>,
    pub source-file: Option<String>,
    // ... other fields
}

pub struct OwlProperty {
    pub iri: String,
    pub label: Option<String>,
    pub property-type: PropertyType, // ObjectProperty, DataProperty
    pub domain: Vec<String>,
    pub range: Vec<String>,
}

pub struct OwlAxiom {
    pub axiom-type: AxiomType, // e.g., SubClassOf
    pub subject: String,
    pub object: String,
    // ... other fields
}
```

## 6. Error Handling

The `parse` method returns a `Result<OntologyData, String>`. If parsing fails, it will return an `Err` with a descriptive error message, such as "No OntologyBlock found in file".

## 7. Testing

The `OntologyParser` module includes a comprehensive suite of unit tests to ensure its correctness. You can run these tests with:

```bash
cargo test --lib parsers::ontology-parser::tests
```

The test suite covers:
-   Basic class and property parsing
-   Class hierarchy and axiom extraction
-   Handling of multiple IRI formats
-   Error handling for missing `OntologyBlock`

