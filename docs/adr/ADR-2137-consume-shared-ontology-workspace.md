---
id: ADR-2137
title: Consume shared ontology semantics through explicit native adapters
date: 2026-10-11
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit:
verified_paths: []
owner: jjohare
review_trigger: Before shared workspace extraction or any consumer dependency or generation migration
repo: visionclaw
---

# ADR-2137 — Consume shared ontology semantics through explicit native adapters

## Context

The corpus parser and builder live here, but their consumers span the estate. Native ingestion calls `vault_core`; mutation still uses the separate `visionclaw-domain::vault` parser and writer. Whelk adapters and the custom constraint reasoner also have different semantic surfaces. Relocation must remove duplication without reducing the estate to the plugin's capabilities.

Estate proposal: [VisionFlow ADR-2014](../../../VisionFlow/docs/adr/ADR-2014-shared-ontology-workspace.md). Evidence and sequencing: [workspace investigation](../../../visionGraph/planning/ontology-workspace/README.md). This proposes an extension to [VAULT-corpus-format](../VAULT-corpus-format.md) and the source-of-record boundary in [DATA-authority-erasure](../DATA-authority-erasure.md).

## Decision

This is a proposed extension. Existing accepted invariants remain in force until a separately reviewed implementation and migration establish the new baseline.

1. Relocate reusable parsing, lossless writing, typed ontology expressions, validation, reasoning and export contracts into the proposed independent Rust workspace. VisionClaw consumes released crates through native adapters. Markdown and its machine-readable header remain the sole authored ontology authority; Oxigraph and other indexes are optional, rebuildable query projections.
2. Share pure semantics with WASM, while exposing richer optional native capabilities. The plugin is one consumer. Keep complete expression and provenance information in the shared model; unsupported backend features return explicit diagnostics rather than disappearing into a pairwise-edge model.
3. Replace the domain parser/writer and corpus-specific reasoner transformations with shared operations. Preserve downstream physics constraints as an explicit projection. Ordered navigation closure and OWL entailment remain distinct outputs. Treat acyclicity as corpus policy, and report reasoning completeness, budgets and unsupported constructs. Re-prove the existing Skip/Relevant optimisation before applying it beyond its current EL profile.
4. Preserve immutable resource IRIs, publication redaction before closure, authored/derived distinctions and fail-closed publication. Model estate layout and strict OKF validation as explicit policy profiles. Any bulk migration produces an identity mapping, visibility diff, asserted/inferred triple diff and rollback artefact; no resource is silently reminted.
5. Version crate APIs, machine headers, export formats and generation manifests independently. Coordinate publication CI, Agentbox pins and Loom readers against a release compatibility matrix. Keep compatibility readers/encoders only for named migration windows. Query storage must rebuild from Markdown plus pinned vocabulary and policy.
6. Keep AI-assisted existing-vault proposals optional and subject to the same validation and governance. OCR and document ingestion are outside this decision and its implementation phases.

## Consequences

One shared implementation reduces drift, but adapters and capability profiles require more explicit contracts. Extracting AGPL code preserves its licence unless rights holders approve another arrangement; do not introduce it into permissive consumers by accident. Legacy byte parity remains a compatibility test, not a requirement to preserve old layout algorithms indefinitely. Reconcile the governing document's stale proposed-status summary for ADR-2124–2128 before changing its accepted baseline.

## Verification

No implementation or activation is claimed. Initial source inspection used `2a2156dbb86e34269f67c870b6a7169c78380c24` with dirty vault build/model/projection files. The documentation worktree starts at `1044b0722`; this is not verification of intervening code. Before implementation, freeze a fresh baseline and run parser/writer round trips, private/public collision and redaction tests, golden exports, defined-class and inconsistent-filler cases, restriction-mode parity with stated limits, native/WASM semantic fixtures and existing mutation/constraint tests. Acceptance also requires the same released fixture generation in native serving, publication and Loom, followed by a separately authorised runtime activation and rollback rehearsal.
