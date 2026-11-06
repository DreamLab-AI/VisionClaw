---
id: ADR-2185
title: Linked timeline, histogram, category and search widgets crossfilter the graph by writing the highlight mask
date: 2026-11-05
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 4adab22f02a09dfddc29b59b7d1f0213d5b3e948
verified_paths: []
relates: [ADR-2179, ADR-2183, ADR-2184]
owner: jjohare
review_trigger: "ADR-2183 or ADR-2184 rejected (this record depends on both); a time column confirmed or ruled out on the graph load; the provenance TimelineScrubber being retired or merged."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source
acceptance_boundary: "Existing exploration UI read at main 4adab22f0: TimelineScrubber (GraphCanvas.tsx:530, provenance.enableTimeline default false at api/settings/defaults.ts:210), search dispatch (CommandInput.tsx:465, useGraphSelection.ts:190); no histogram, crossfilter or legend code found in client/src. Cosmograph widget set from cosmograph.app docs (CosmographCrossfilter). No widget was prototyped."
---

# ADR-2185 — Linked timeline, histogram, category and search widgets crossfilter the graph by writing the highlight mask

## Context
- **What exists today:**
  - a provenance `TimelineScrubber`, which tints nodes by diff status and is off by default (`api/settings/defaults.ts:210`);
  - search with fly-to (`useGraphSelection.ts:190`);
  - neighbour lists in `NodeDetailPanel`.
  There are no histograms, category bars, linked brushing or legend.
- **Cosmograph's crossfilter.** Its widgets (timeline, histogram, bars, search) are Mosaic clients over one table. A brush in any one of them updates a shared selection, the others re-aggregate over it, and the graph greys out the rest.
- **The time column.** The page `Metadata` struct has `last_modified`, `last_commit` and `change_count`. Whether they reach the client is unverified (ADR-2184).

## Decision
1. **The widget set** is an explore panel:
   - a **timeline** (brushable histogram over one date column, default `last_commit` when present);
   - **numeric histograms** (brushable, any numeric column);
   - **category bars** (click to select, any categorical column, for example `domain` or `maturity`);
   - **search** (text match over label and chosen text columns).
   Each widget is a Mosaic client over ADR-2184's table, built with vgplot or a thin custom renderer.
2. **Linking.** All widgets share one crossfilter selection. Each widget aggregates over the selection *excluding its own clause*, which is the standard crossfilter rule, so its own bars still show the full distribution.
3. **The graph is a consumer.** The combined selection resolves to a node-id set and is written to ADR-2183's highlight mask. It never removes nodes. Selecting nodes in the graph (click, or lasso when added) adds a clause that the widgets reflect.
4. **Provenance timeline.** The `TimelineScrubber` stays as a separate provenance mode. If a date column exists, the explore-panel timeline is the default time control. Merging the two is a follow-on decision, not part of this record.
5. **Dependencies.** This record ships only after ADR-2183 and ADR-2184. If ADR-2184's DuckDB/Mosaic choice is rejected, the widgets are built over the Arrow table with typed-array aggregation, and the same linking rule applies.

## Consequences
- **What users can do.** Answer "what changed recently, in which domain, how central is it" by brushing, with the graph showing where the answer sits.
- **Dependency.** Adds Mosaic and vgplot (BSD-3-Clause) to the client, loaded lazily with DuckDB.
- **Panel space.** The panel competes with existing panels for screen space. It is a scene occluder (`data-scene-occluder`), so ADR-2180's fit keeps the graph out from under it.

## Verification
None yet. Each decision has its own check:
- **D2:** a unit test shows that brushing widget A re-aggregates B and C but not A.
- **D3:** brushing a timeline range greys out nodes outside it, with instance counts unchanged.
- **D3:** clicking a node narrows the widgets to its clause.
- **D5:** the typed-array fallback is exercised by the same tests with DuckDB disabled.
