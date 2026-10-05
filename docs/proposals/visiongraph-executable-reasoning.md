---
id: PRD-029
title: "visionGraph executable reasoning — make the Whelk gate and the reasoned closure earn their names"
status: proposed / scope (not authority)
date: 2026-10-05
drives: [ADR-2124, ADR-2125, ADR-2126, ADR-2127, ADR-2128, agentbox ADR-2129]
domain: DDD-023 (Ontology Reasoning & Integrity)
depends_on: [ADR-2112, ADR-2113, ADR-2116, ADR-2023, agentbox ADR-2107]
lands_in: docs/VAULT-corpus-format.md (governing) + the ADRs above (decisions)
review_trigger: the first defined class or disjointness key accepted into ontology/vocabulary.yaml, or an agent decision traced to an empty ontology result
repo: visionclaw
---

# PRD-029: visionGraph executable reasoning

**On placement.** VisionClaw's `docs/archive/prd/` is a frozen corpus (cut 2026-08-31,
`docs/MIGRATION-plan.md`) and is not authority. Following the agentbox precedent
(`agentbox/docs/proposals/sovereign-system-one.md`), the PRD series continues here
beside its DDD. This document is scope, not authority: the ADRs it drives are the
decisions and [`docs/VAULT-corpus-format.md`](../VAULT-corpus-format.md) is the compliance surface.

**Provenance.** The findings come from reviewing *Protégé Pizza / EKA* (Xiaoqi Zhao, CC-BY-NC-SA-4.0; ch13–ch26)
against the code at `c4570c081`. Each claim below cites the code it rests on, not the book.
The book is the lens, not the authority.

---

## Problem

visionGraph is described as "Whelk-reasoned" and its write paths as "Whelk-gated".
Both are true of the machinery and close to vacuous on the data:

1. **The reasoner cannot classify.** Every class is primitive; no `owl:equivalentClass`
   is emitted (`crates/vault/src/whelk.rs:75-82`). Whelk returns the transitive
   closure of the authored hierarchy (9,575 subsumptions at the 5,215-class measurement,
   `whelk.rs:66-71`) and nothing a human did not already imply. *(ADR-2124)*
2. **The `vault propose` consistency gate cannot fire, and the two gates disagree.** The
   elevation actor reads a hand-added `disjoint-with` key (`src/actors/elevation_actor.rs:655-658`);
   `vault` neither registers, emits nor extracts it (`crates/vault/src/whelk.rs:28-37`), and
   domain-root disjointness is off (`crates/vault/src/build/turtle.rs:15-18`). So on the
   `vault propose` path nothing can be unsatisfiable and `WHELK_INCONSISTENT`
   (`crates/vault/src/propose.rs:117-122`) is unreachable, with no test proving otherwise. *(ADR-2125)*
3. **Requirements have no recorded home.** Closed-world rules live in `vault validate`
   by practice, not by decision. One "add an `only` axiom" later, a requirement can be
   written that EL cannot evaluate and the open world never enforces. *(ADR-2126)*
4. **Agents cannot tell silence from "no".** Agent-facing answers carry no basis and no
   open-world marker (`crates/visionclaw-ontology/src/types/ontology_tools.rs:117-121`;
   `agentbox/skills/ontology-augment/SKILL.md:58-60`). *(ADR-2127, agentbox ADR-2129)*
5. **The ontology does not name its generation.** `.generation.json` does
   (`crates/vault/src/build/generation.rs`) but its digest covers pages only
   (`build/mod.rs:477-485`); the Turtle names nothing (`turtle.rs:246`); every property's
   domain and range is `owl:Thing` (`turtle.rs:525-534`). *(ADR-2128)*

## Users and outcomes

| User | Outcome wanted |
|---|---|
| Corpus owner (signer of 31403s) | A Whelk blocker that has been seen to fire, and an inferred-closure diff to review when definitions change |
| Agents grounding through the Loom / `vault` | Answers labelled asserted / inferred / silent / contradicted, citing a generation |
| VisionClaw graph consumers | Inferred edges that include real classifications, not only transitive parents |
| Loom operator | One identity (version IRI) shared by the bundle, the store and the answers |

## Scope

**In:** an opt-in EL defined-class key; registering and wiring the elevation actor's existing `disjoint-with` key, sibling-only, on both paths; probe
regression tests on both write paths; a zero-unsatisfiable build gate; a recorded split
between closed-world validation and open-world reasoning; tri-valued, basis-carrying
agent answers; `owl:versionIRI` per generation; per-property scoped domain/range.

**Out:** replacing Whelk with a DL reasoner; domain-root disjointness (turtle.rs:15-18
records why); full SHACL over the Oxigraph store; Neo4j (legacy, BASELINE-architecture);
any change to identity, `resource` or the publish gate.

## Sequencing

1. ADR-2126 first. It records the boundary the rest depend on and is mostly documentation of current practice.
2. ADR-2128. It is independent and cheap, and it is the identity every later answer cites.
3. ADR-2125, then the probes go red, then green. This arms the gate before anything is given the power to contradict.
4. ADR-2124, with `reason()` choosing its restriction mode from the graph.
5. ADR-2127 and agentbox ADR-2129 last, once `entailed_false`/`contradicted` are reachable.

## Acceptance

| ID | Acceptance | Evidence |
|---|---|---|
| A1 | A probe `P ⊑ A, P ⊑ B, A disjoint-with B` yields `WHELK_INCONSISTENT` through `vault propose` on built output and through the elevation actor's `run_consistency_gate`; both tests fail if the disjointness is removed | `cargo test -p vault probe_`, elevation actor test |
| A2 | `vault build` fails on any unsatisfiable class and names root causes | build fixture |
| A3 | One `defines-as` fixture yields a named subsumption absent from the asserted graph; forcing `Restrictions::Skip` loses it | `cargo test -p vault defined_class_` |
| A4 | A vocabulary key mapped to `owl:allValuesFrom` fails vocabulary load with a named error | vocabulary fixture |
| A5 | A page change and a vocabulary-only change each alter `owl:versionIRI`; an identical rebuild does not; Loom `/health` reports it | build fixture + Loom health |
| A6 | An empty agent query returns `closure: "open"` with the generation; `ontology-augment` reports `silent` distinctly from `degraded` | `tests/ontology_agent_integration_test.rs`, skill test |

## Risks

- **Definitions silently re-parent classes.** Mitigation: a reviewed inferred-closure diff per Schema change (ADR-2124).
- **Disjointness reintroduces mass unsatisfiability.** Mitigation: sibling-only rule plus the A2 build gate (ADR-2125).
- **Restriction saturation slows the build.** Full `Include` does not finish on this corpus, so ADR-2124 uses a relevance-filtered `Relevant` mode; refuse definitions over high-fan-out properties.
