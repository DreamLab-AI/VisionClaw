---
id: ADR-2118
title: Append space science and Earth observation domain identities
date: 2026-10-01
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: uncommitted-working-tree
verified_paths: []
owner: visionGraph corpus maintainer
review_trigger: domain identity, vocabulary or graph-tier format change
repo: visionclaw
---

# ADR-2118 — Append space and Earth domain identities

## Context

The corpus owner accepted two linked domains in visionGraph ADR-VG-004 and
authorised the term/header implementation. The exporter and explorer currently
encode six domain identities. New domain membership must survive graph and
Turtle projection without renumbering existing identities.

## Decision

Append `space-science-and-systems` as graph domain 6 and
`earth-observation-and-geospatial-sensing` as graph domain 7. Preserve domain
IDs 0–5 and all existing category IDs. Recognise both roots in the vocabulary,
Turtle taxonomic-root list, graph-tier labels and WebVOWL colours. The explorer
uses the same append-only order, descriptors and theme colours.

Domain membership, navigation grouping and subclassing retain their separate
meanings. No disjointness axiom is added. New headers remain private drafts;
publication and scientific verification are separate lifecycle actions.

## Consequences

Overview array indices place eight domains before existing categories. Consumers
must use the declared domain count for these local indices; they are not
persistent category IDs. Uncategorised members remain visible within the new
domain tiers without inventing scientific superclass axioms for navigation.

## Verification

Graph-tier regression tests exercise legacy IDs, new-domain resolution, overview
indices and deterministic graph artefacts. Explorer tests exercise both URLs,
descriptor IDs and palette entries. The implementation and verification are
local working-tree changes; no deployment or published generation is claimed.

## Owner-authorised release

On 2026-10-01 the owner explicitly authorised merging and deploying the draft vocabulary before funding article research. The shared `vault-core::domains` registry now serves publication, runtime navigation roots and GPU class IDs, with canonical/legacy-name regression coverage. Client palettes recognise both new domains. The site pipeline gates all 922 public draft identities, then dispatches its exact source SHA to the VisionClaw ontology release workflow, whose build checkout is pinned to validation’s source revision. Empty bodies and draft lifecycle remain intentional. Deployment and ingestion receipts are recorded with the corpus programme.
