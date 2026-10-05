# DDD-023: Ontology Reasoning & Integrity Domain

**Date**: 2026-10-05
**Status**: Proposed
**Bounded Context**: Ontology Reasoning & Integrity — deciding, for one corpus generation, what is *entailed*, what is *contradicted* and what is merely *not asserted*, and refusing proposals that make the corpus unsatisfiable
**Placement**: `docs/archive/ddd/` is the frozen pre-consolidation corpus and is not authority; the DDD series continues here beside its PRD (agentbox precedent: `agentbox/docs/proposals/sovereign-system-one-domain.md`). The compliance surface is [`docs/VAULT-corpus-format.md`](../VAULT-corpus-format.md); the decisions are ADR-2124–ADR-2128 and agentbox ADR-2129.
**Cross-references**: [PRD-029](./visiongraph-executable-reasoning.md) (the product case), [DDD-020](../../agentbox/docs/archive/ddd/DDD-020-semantic-integrity-provenance-domain.md) (agentbox archive; rationale, not authority) (integrity and provenance; its I07 "consistency ≠ integrity" is consumed here, not redefined), [`ddd-semantic-pipeline.md`](../explanation/ddd-semantic-pipeline.md) (ingest → parse → reason → store; this context refines its *reason* stage), ADR-2116 (proposals are forum ActionRequests), ADR-2113 (one corpus parser and build).

---

## TL;DR for newcomers

The corpus is open-world. A missing fact means *the corpus is silent*, not *false*. Validation
asks whether a page is well-formed (a closed-world question with a yes/no answer). Reasoning
asks what follows from the pages (an open-world question). This context owns the second
question and the gate that rejects a corpus where some class can have no members. It
consumes the validator; it never replaces it.

## Ubiquitous language

| Term | Meaning |
|---|---|
| **Generation** | One identified corpus build (`.generation.json`; version IRI per ADR-2128). Every entailment is relative to one. |
| **Asserted** | A triple an author wrote, emitted by `vault build`. |
| **Entailed** | A triple Whelk derives from asserted axioms at a generation. |
| **Silent / not asserted** | Neither asserted nor entailed. *Not* negative evidence. |
| **Contradicted / entailed false** | Its negation is entailed (e.g. membership in a class disjoint from one already held). |
| **Primitive class** | Defined by necessary conditions only (`SubClassOf`); the reasoner never classifies *into* it. |
| **Defined class** | Has an EL `equivalentClass` definition; the reasoner classifies into it (ADR-2124). |
| **Disjoint partition** | Siblings under one parent declared mutually exclusive (ADR-2125). Never domain roots. |
| **Unsatisfiable** | Entailed `⊑ owl:Nothing`. Root-cause if not derived from another unsatisfiable class. |
| **Armed gate** | A gate with a standing probe that proves it can still reject. |
| **Probe case** | A fixture built to be unsatisfiable; it must be rejected, or CI fails. |
| **Integrity rule** | A closed-world `vault validate` rule (ADR-2126). Not part of this context's model. |

## Aggregates

### ReasoningRun (aggregate root)
- Identity: generation id.
- Holds: restriction mode chosen (`Relevant` | `Skip`, ADR-2124 §4), inferred closure, unsatisfiable set, root causes.
- Invariant R1: mode is `Relevant` whenever the input carries a defined class; full `Include` is never used on the corpus (it does not finish).
- Invariant R2: the run is immutable once its generation is published.

### DefinedClass (entity, owned by the corpus page)
- An EL expression: conjunction of named classes and `∃p.C` over `emitted: true` properties.
- Invariant D1: no `only`, cardinality, `hasValue`, negation, disjunction, inverse or nominal (`NON_EL_DEFINITION`).
- Invariant D2: introduced only with a reviewed diff of the closure it adds.

### DisjointPartition (value object)
- A set of slugs sharing one direct parent.
- Invariant P1: members are siblings (`DISJOINT_NOT_SIBLINGS` otherwise).
- Invariant P2: no member is a domain root or taxonomy category.

### ConsistencyVerdict (value object, per proposal)
- `{ introduced_unsat, touched_unsat, preexisting_unsat, generation }`.
- Invariant V1: blocks iff `introduced_unsat ∪ touched_unsat ≠ ∅` (delta-scoped, like the conflict gate).
- Invariant V2: never approvable by signature (VAULT Invariant 9).

### GroundedAnswer (value object, at the agent edge)
- `{ value, basis: asserted|inferred|provenance, truth: entailed|entailed_false|not_asserted, closure: open, generation }`.
- Invariant G1: an empty result is `not_asserted`, never `false`.

## Domain events

| Event | Raised when | Consumed by |
|---|---|---|
| `GenerationReasoned` | a ReasoningRun completes | inferred-edge materialiser, Loom promotion |
| `UnsatisfiabilityIntroduced` | a proposal's verdict blocks | `vault propose` blocker list, forum 31402 |
| `ProbeStoppedRejecting` | a probe case passes cleanly | CI (fails the build) |
| `DefinitionChangedClosure` | a DefinedClass change alters the closure | Schema-level review |

## Context map

- **Upstream, conformist:** Corpus Authoring (`vault-core::parse`, `vocabulary.yaml`). This context accepts the parser's model and adds no second parser (VAULT Invariant 2).
- **Partnership:** Semantic Integrity (DDD-020). Conflict detection runs first; consistency second; ACSP governance third. None substitutes for another.
- **Downstream, published language:** VisionClaw graph (inferred edges tagged `inferred`), agent ontology surface (GroundedAnswer), the Loom (version IRI), agentbox `ontology-augment` (silent / contradicted / degraded).

## Invariants summary

R1–R2, D1–D2, P1–P2, V1–V2, G1 above, plus:

- **I1** Reasoning never rejects a page; validation never adds a triple (ADR-2126 §4).
- **I2** Every gate in this context has a standing probe (ADR-2125 §2).
- **I3** Every entailment and every answer is relative to exactly one generation (ADR-2128).
