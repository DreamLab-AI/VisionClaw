---
title: The Insight Migration Loop
description: Conceptual explanation of how VisionClaw promotes Logseq notes into governed OWL 2 ontology classes through a physics-visible, broker-reviewed migration pipeline
category: explanation
tags: [insight-migration, ontology, knowledge-graph, broker, physics, governance]
updated-date: 2026-04-18
---

# The Insight Migration Loop

## Opening

Think of every note you write as a concept asking to become a class. Your team has been building, over years of note-taking, a shared vocabulary — a set of recurring terms, relationships, and distinctions that sit inside people's heads and inside thousands of Logseq pages. Those concepts are real. They do real work in conversations and documents. They just have not yet been declared. Nobody has said: this term means exactly this, it relates to these other terms in exactly these ways, and from now on Whelk can reason over it.

The Insight Migration Loop is the mechanism by which that declaration happens, continuously, as a by-product of normal work rather than as a formal taxonomy project. When a note accumulates enough evidence — ontology wikilinks, agent proposals, maturity markers — VisionClaw surfaces it in the broker's inbox. The broker takes a look, approves, and the system opens a GitHub pull request. On merge, the note becomes a live OWL 2 Web Ontology Language (OWL 2) class that Whelk, VisionClaw's EL++ reasoner, can reason over. The 3D physics graph redraws around it. A governed ontology grows as a side effect of note-taking.

The people who benefit are the ones who currently answer "do we have a position on X?" from memory. A research lead managing 2,000+ pages. A regulated-industry specialist whose auditors require every claim to trace to a controlled vocabulary. A consultant whose firm IP is scattered across bespoke client deliverables. For all three, the migration loop converts private insight into institutional asset without anyone having to sit down and write an ontology from scratch.

---

## The Two Tiers

VisionClaw maintains two distinct populations of nodes, and understanding the difference is essential for understanding why the loop exists.

A **KGNode** is a narrative node — a Logseq page authored with `public:: true`. It is part of your personal or team knowledge graph: exploratory, fast-changing, opinionated. It can be edited every hour without ceremony. An **OntologyClass** is a vocabulary node — a named class in the shared OWL 2 T-Box (the "terminology box", the schema). It changes slowly, only through a broker-reviewed pull request. It is what Whelk reasons over, what agents cite when they make structural claims, and what auditors trace when they need provenance.

The two tiers share one identity scheme (`vc:{domain}/{slug}`), so any external consumer sees a single namespace. But they live separately in Neo4j: `KGNode` and `OntologyClass` are distinct labels, and a `BRIDGE_TO` edge carries the relationship between a note and its potential or confirmed class. Promotion is never a rewrite — the note keeps existing, the class keeps existing, and the bridge edge between them advances from `candidate` through `promoted`. This means rollback is cheap: change the edge label, not the nodes.

[ADR-048](../adr/ADR-048-dual-tier-identity-model.md) documents the full identity model, the IRI scheme, and the Neo4j schema additions.

---

## The Migration Event

The journey from note to ontology class has a clear sequence of steps.

**Detection.** The discovery engine (BC13) scores every `public:: true` note across eight signals: wikilinks to existing ontology classes, semantic co-occurrence with ontology-domain neighbours, an explicit `owl:class::` declaration, an agent proposal, a maturity marker, PageRank centrality, recency, and author authority. These are combined into a confidence score in [0, 1]. When confidence crosses the 0.60 threshold, the note is surfaced.

**Surfacing.** Surfacing creates a `BRIDGE_TO` edge with `kind: "candidate"` and emits a `MigrationCandidateSurfaced` event. BC11 (the Judgment Broker) picks this up and creates a migration case in the broker's inbox. The broker sees it as a card in a dedicated lane.

**Review.** The broker opens the card. The Decision Canvas splits: the note's Markdown content on the left, the proposed OntologyClass on the right (showing parent class, proposed properties, and an OWL diff strip below). Everything needed to decide is on one screen.

**Approval.** One click. The `DecisionOrchestrator` ensures that approval is non-optional with respect to its downstream effects: it calls `ontology_propose`, opens a GitHub pull request carrying the OWL delta, and records the decision as a Nostr-signed bead.

**PR and merge.** The pull request includes the Whelk consistency hash — Whelk has already checked that the new axioms do not make any existing class unsatisfiable. On merge, BC2 (Ontology Governance) applies a SPARQL PATCH to the OWL store.

**Bridge edge flip.** BC13 receives the `OntologyMutationApplied` confirmation and emits `MigrationPromoted`. BC3 (the knowledge graph) updates the `BRIDGE_TO` edge from `kind: "candidate"` to `kind: "promoted"`. From the user's perspective, a toast appears: "Promoted: vc:bc/smart-contract — centrality +0.04." Wall-clock time: under five minutes.

As a concrete example: a researcher writes `Smart Contract.md` in the blockchain domain with three wikilinks to existing ontology classes and a `maturity:: ready` marker. An agent proposes it with 0.80 confidence. The scoring engine puts the combined confidence at 0.68, crossing the threshold. The broker reviews the two-pane canvas, approves, and five minutes later the note is a live OWL class with `parent: Contract`, properties `executed_on` and `triggers`, and a reasoning-consistent provenance chain.

This is the knowledge-unit analogue of the Insight Ingestion Loop described in the [README Layer 3 section](../../README.md#the-insight-ingestion-loop). Where the Insight Ingestion Loop turns shadow workflows into governed process patterns, the Migration Loop turns tacit vocabulary into governed ontology classes. Both serve the same architectural purpose: converting personal discovery into institutional asset.

---

## Physics Tells the Story

The physics view is not decoration. It is a live dashboard of ontology health, and the five migration forces are what make migration state spatially readable.

**Physicality.** Nodes are organised into spatial bands according to whether they represent virtual entities, physical entities, or conceptual entities. Abstract concepts occupy one neighbourhood, software artefacts another, physical objects a third. Inter-physicality repulsion keeps these bands distinct rather than interleaved. The result is that nodes with similar ontological character cluster together at a glance.

**Role.** Superimposed on physicality, a second clustering force groups nodes by semantic role: processes, agents, resources, and concepts each form loose curved bands. A process node and an agent node will tend to sit in different parts of the horizontal plane even if they are closely linked, making the structural character of the graph legible without needing to read labels.

**Maturity (Phase A).** The z-axis encodes quality. Authoritative, stable nodes settle near z = 0, the dense core. Draft and emerging nodes are pushed outward. Looking at the graph from the side, you see the ontology core sitting lower and heavier, with exploratory work distributed around the periphery. When a note is promoted and its bridge edge flips to `promoted`, it begins settling toward the core plane over the following seconds.

**Bridging.** Every `BRIDGE_TO` edge acts as a tendon pulling the source `KGNode` toward its target `OntologyClass`. The pull scales with both bridge kind and confidence: a note that has just crossed the candidate threshold is drawn gently; a fully promoted note is pulled strongly. You can watch migration happen — the node moves across the screen as the broker approves and the PR merges.

**Orphan (Phase B).** Nodes with no ontology anchor at all drift in a fixed direction into a dedicated region of world space. This region renders in amber at reduced opacity. It is not a rubbish tip. It is a migration queue made spatial: every amber node is a candidate waiting for attention. The orphan cloud tells the broker where the next enrichment effort should go without a single filter or query.

---

## The Broker's Role

The loop is deliberately not automatic. Agents propose; systems surface; humans decide.

The README puts it plainly: "human judgment is the irreplaceable layer." Three capacities cannot be automated. Strategic direction — only humans decide whether a proposed term actually belongs in the organisation's vocabulary. Ethical adjudication — whether a class definition carries the right identity criteria, the right rigidity, the right scope. Relational intelligence — whether this promotion will make sense to the team who will use it.

The broker does not need to know OWL to operate the workbench. The Decision Canvas is designed so that the broker sees a familiar side-by-side: note content on one side, proposed class definition on the other, structured diff below. Whelk has already done the consistency checking. The broker's job is to decide whether the concept deserves to be formalised, not to write the axioms.

All of this happens in the [Judgment Broker Workbench](../explanation/ddd-insight-migration-context.md). The workbench is the single seat from which migration decisions are made, alongside workflow proposals, trust drift alerts, and other cases requiring human judgment.

---

## The Migration KPIs

The loop's health is measured by six KPIs defined in [PRD §8](../prd-insight-migration-loop.md). These are not vanity metrics. They measure whether the loop is doing its job: surfacing candidates that deserve promotion, clearing the broker's queue in reasonable time, keeping the ontology stable after promotion, and ensuring provenance is never silent.

Think of them as the pulse of the feedback cycle. High clearance rate and high surface precision together mean the discovery engine is proposing well and the broker is keeping up. Low rollback rate means promotions are lasting. Low ontology incoherence means Whelk is catching axiom problems before they reach production. These six signals give the platform owner visibility into whether the loop is tightening or fragmenting.

---

## Where This Sits in the Platform

The Insight Migration Loop has three direct neighbours in the platform architecture.

The **Insight Ingestion Loop** (described in the README and delivered in the platform's Phase 2 roadmap) is the workflow analogue. Where the Ingestion Loop turns shadow workflows into approved process patterns, the Migration Loop turns recurring vocabulary into approved ontology classes. They are parallel loops operating on different knowledge units.

The **Judgment Broker Workbench** is the seat from which migration decisions are made. Migration candidates appear as a distinct lane in the broker's inbox, alongside escalations and workflow proposals. The broker never leaves the workbench to approve a migration.

**Ontology Governance** (BC2) is the destination. When the broker approves and the PR merges, BC2's `OntologyMutationService` applies the SPARQL PATCH and notifies Whelk to re-check global consistency. BC2 is where ontology classes live once they have been promoted. It is the governed layer that agents query, auditors trace, and Whelk reasons over.

---

## Further Reading

- [PRD: Insight Migration Loop (MVP)](../prd-insight-migration-loop.md)
- [ADR-048: Dual-Tier Identity Model](../adr/ADR-048-dual-tier-identity-model.md)
- [ADR-049: Insight Migration Broker Workflow](../adr/ADR-049-insight-migration-broker-workflow.md)
- [Master Design: Insight Migration Loop](../design/2026-04-18-insight-migration-loop/00-master.md)
- [DDD: Insight Migration Context](./ddd-insight-migration-context.md)
