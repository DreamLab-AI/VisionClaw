---
id: ADR-2118
title: Append space science and Earth observation domain identities
date: 2026-10-01
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 772039bd172027af034d64befacca1ebcf0bbc04
verified_paths: [crates/vault-core/src/domains.rs]
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

## Fresh-store ingestion correction

A full real-corpus sync exposed an absent assert graph on first ingest: the rebuild used `CLEAR GRAPH` and never reached its INSERT. Assert/inferred rebuilds now use idempotent `CLEAR SILENT GRAPH`. The integration fixture no longer bootstraps the graph to hide this case. Assert rebuilds also retain source domain and draft maturity, and a fresh-ingest regression verifies both new domains and navigation roots. `sync_corpus` now fails on recorded stage errors and can write an actual-graph verification receipt.

## Close-out 2026-10-30

`staged → live`. `verified_commit` moves from the placeholder `uncommitted-working-tree` to `772039bd172027af034d64befacca1ebcf0bbc04`.

**Code at this commit.** `DOMAIN_ROOTS` (`crates/vault-core/src/domains.rs:4-17`) appends
`space-science-and-systems` (publication ID 6) and `earth-observation-and-geospatial-sensing` (ID 7) after IDs 0–5.
`domain_class_id` adds one for the GPU class, which gives 7 and 8 (test at `:62-65`). A ninth root,
`supply-chains-and-logistics`, has since been appended in the same append-only order; that belongs to its own
record, not to this one.

**Activation.** Runtime provenance: `visionclaw_container` runs `/app/target/dev-runtime/visionclaw-server` (build stamp `features=gpu,ontology,dev-auth`, i.e. the dev profile with the dev-auth bypass compiled in); its wrapper log (`/app/logs/rust.log`) reads `[RUST-WRAPPER][2026-10-30 19:01:06] Building from published inputs: source_sha=772039bd172027af034d64befacca1ebcf0bbc04 source_dirty=0`, so the responses below come from this commit.
- The 19:03 boot logged `Materialised 9 domain root nodes with edges`.
- `GET /api/graph/data` (9,688 nodes) contains `domain-root-space-science-and-systems` ("Space Science and Systems")
  and `domain-root-earth-observation-and-geospatial-sensing` ("Earth Observation and Geospatial Sensing").
- The two slugs occur 1,639 and 1,171 times in that response.
