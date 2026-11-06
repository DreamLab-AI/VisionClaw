---
id: ADR-2180
title: The default camera fit frames the knowledge body; the memory cloud and the other bodies join the frame only on request
date: 2026-11-05
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 4adab22f02a09dfddc29b59b7d1f0213d5b3e948
verified_paths: []
relates: [ADR-2135, ADR-2179]
owner: jjohare
review_trigger: "Acceptance (which amends ADR-2135's Camera clause in the same change); a fourth body joining the separated layout; a change of the default camera direction."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source+runtime
acceptance_boundary: "Fit composition read from client/src/features/graph/utils/sceneFitBounds.ts and useCameraAutoFit.ts at main 4adab22f0. Runtime: one sidecar session with the memory cloud ON (persisted setting); hiding the cloud mesh did not change the fit, because the fit reads the cloud setting, not mesh visibility. Cloud-off framing (the shipped default) was not measured."
---

# ADR-2180 — The default camera fit frames the knowledge body; the memory cloud and the other bodies join the frame only on request

## Context
- **What the fit frames today.** ADR-2135 (accepted) makes the desktop auto-fit frame "the sphere enclosing all three bodies". `sceneFitBounds` (`client/src/features/graph/utils/sceneFitBounds.ts:57-74`) encloses the knowledge vertex and the ontology vertex. It also encloses the memory cloud whenever the cloud is enabled and has a snapshot (`cloudFitKey`, `:80-82`). The fit reads the cloud setting, not whether its mesh is visible.
- **The cloud is ten graphs wide.** It is placed `MEMORY_BODY_SCALE = 10` graphs wide (`triLayout.ts`). It is off by default (`client/src/api/settings/defaults.ts:214-218`, operator call 2026-07-17: it "obscures the graph itself at rest").
- **What a user who enables it sees.** The knowledge graph shrinks to about 60 px at fit. This was observed live on 2026-11-05, with the cloud on.
- **What the fit uses.** Each graph's extent is a robust 5th–95th percentile box (`client/src/utils/robustBounds.ts:6`). With the cloud off, the fit encloses only the two graph vertices. That framing was not measured.

## Decision
1. **The fit target.** The default fit target is the *focus body*: the knowledge graph, unless the user has chosen another. The fit uses that body's robust bounds placed at its vertex, as in `sceneFitBounds` today but for one sphere.
2. **Three fit scopes.** **Fit view** offers *Focus body* (the default), *Both graphs* and *Whole scene*. *Whole scene* is today's ADR-2135 behaviour, including the cloud when it is on. The chosen scope persists per client.
3. **When the camera refits.** Enabling the memory cloud no longer refits the camera unless the scope is *Whole scene*. The cloud-settle refit (`createSettleTrigger` on `cloudFitKey`) applies only to that scope.
4. **The other bodies stay where they are.** The separated layout, vertex placement, cloud placement and XR are unchanged. Only the desktop camera target changes.
5. **Amending ADR-2135.** On acceptance, ADR-2135's Camera clause is amended in the same change. It then reads: "the *Whole scene* fit scope frames the sphere enclosing all three bodies". This record does not supersede ADR-2135. The rest of ADR-2135, the separation itself, stands.

## Consequences
- **What the user sees first.** The first frame shows the knowledge graph at a size where ADR-2181's edges can be read (P1 of ADR-2179).
- **The cloud is no longer framed automatically.** A user who wants the cloud must pick *Whole scene* or navigate to it. This weakens the "all three bodies at a glance" intent of ADR-2135 in the default view.
- **Tests change.** `sceneFitBounds.test.ts` gains a focus-body case. The existing all-bodies cases move under the *Whole scene* scope.

## Verification
None yet. Each decision has its own check:
- **D1:** with default settings except the cloud enabled, the knowledge body's robust radius fills at least 50% of the shorter screen axis after first data (screenshot plus the computed pose).
- **D2:** a unit test shows that *Whole scene* returns the current `sceneFitBounds` result byte for byte.
- **D3:** toggling the cloud with the focus-body scope leaves the camera pose unchanged.
