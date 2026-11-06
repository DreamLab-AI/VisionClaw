---
id: ADR-2183
title: Interactive filters and selection grey out through a per-node highlight mask; only access control and population scope remove nodes
date: 2026-11-05
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 4adab22f02a09dfddc29b59b7d1f0213d5b3e948
verified_paths: []
relates: [ADR-2003, ADR-2161, ADR-2179, ADR-2181, ADR-2182, ADR-2184, ADR-2185]
owner: jjohare
review_trigger: "ADR-2185 accepted (its widgets are this mask's main writer); a request for filters that also change the layout; any change to the visibility filter (ADR-2003)."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source
acceptance_boundary: "Filtering read in client/src/features/graph/hooks/useGraphFiltering.ts and the server default in src/settings/models.rs at main 4adab22f0; cosmos.gl greyout read from its config.ts (pointGreyoutColor, linkGreyoutOpacity 0.1) and CosmographCrossfilter docs. No runtime test of filtering was made."
---

# ADR-2183 — Interactive filters and selection grey out through a per-node highlight mask; only access control and population scope remove nodes

## Context
- **Filters remove nodes today.** Client filters (quality, authority, linked-page and degree) drop nodes from `visibleNodes`, the render set (`useGraphFiltering.ts:116`). They are on by default: the server's `NodeFilterSettings::default()` sets `enabled: true` with a quality threshold of 0.9 (`src/settings/models.rs:147-158`). The client's `?? false` at `useGraphFiltering.ts:81` is only a fallback for a missing value.
- **Population scoping is server-side.** It works through `?graph_type=` (`useGraphFiltering.ts:14-18`).
- **Access control removes nodes on the server.** The visibility filter (ADR-2003, fail-closed) drops private nodes from the wire before any client sees them.
- **Removal costs context.** Removing nodes reflows instance buffers and hides the context a user filtered *within*.
- **Cosmograph greys out instead.** Unselected points move toward a grey colour and links drop to 0.1 opacity. All crossfilter widgets write one highlight set, which the renderer reads.

## Decision
1. **The mask.** Each client holds one per-node highlight mask, indexed like the node position buffer, with one value per node:
   - 0 = greyed out;
   - 1 = normal;
   - 2 = emphasised.
   On WebGPU it lives in a GPU buffer or texture beside the positions. Node sprites, overview edges (ADR-2181) and labels (ADR-2186) read it. An edge is greyed out when either endpoint is.
2. **Writers.** Interactive filters, search, selection and neighbourhood focus write the mask through one combinator. The result is the intersection of active filter predicates, with the selection layered as emphasis on top. None of them removes nodes from the render set.
3. **Greyed-out rendering.** A greyed-out node is drawn in a desaturated colour mixed toward the background. A greyed-out edge is drawn at low alpha; the cosmos.gl default of 0.1 is the starting point. Greyed-out nodes do not get labels or glow.
4. **What still removes nodes:**
   - the server visibility filter (ADR-2003);
   - the data-space read gate (ADR-2161, when accepted);
   - the server population scope (`graph_type`);
   - hierarchy collapse, which changes what a node *is* rather than whether it is shown.
   Nothing on the client may reveal a node these remove.
5. **Physics is untouched.** The mask is display-only and never reaches the GPU layout or the wire.

## Consequences
- **Stable buffers.** Filtering no longer reallocates instance buffers, so filter changes are a single small upload.
- **Context is kept.** Users see where a filtered subset sits in the whole graph, which is P3 of ADR-2179.
- **Hard removal becomes an opt-in.** The existing quality and authority filters change meaning from "hide" to "grey". A "hide greyed" toggle can be added as an explicit opt-in that removes nodes, documented as such.

## Verification
None yet. Each decision has its own check:
- **D1/D2:** a unit test shows that the combinator over two predicates and a selection produces the expected mask.
- **D2:** a scene inventory before and after a filter shows unchanged instance counts.
- **D4:** a test shows that a node withheld by the visibility filter has no mask slot and no label.

## Implementation notes
- **Filter classification** (`client/src/features/graph/explore/nodeFilter.ts`, `hooks/useGraphFiltering.ts`):
  - *Grey out* through the mask source `filter:settings`: the quality / authority predicates (AND / OR), `minConnections` and `minMaturity`.
  - *Remove* from the render set: the `linked_page` gate and hierarchy collapse (D4). The `linked_page` gate is population scope, because the server applies the same `include_linked_pages` scope (`graphDataManager.setGraphData` already drops the stubs), so the client gate only mirrors it.
- **The default greys nothing.** The OR arm is ported unchanged. Under the server default (quality on, authority off, OR) a disabled predicate passes vacuously, as it does in `src/actors/client_filter.rs:85-92`, so the 0.9 threshold greys nothing. This is characterised in `hooks/__tests__/useGraphFiltering.test.ts`. Changing the default is a separate product decision.
- **Source names.** `filter:*` sources grey and `emph:*` sources emphasise. Explore widgets use `filter:explore:<widget>`. Every write goes through `explore/filterRegistry.ts`, which skips unchanged id sets.
- **Hide filtered.** `visualisation.graphView.explore.hideFiltered` (default false) is the opt-in that removes nodes greyed by any filter source.
