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

## Decisions recorded — 2026-10-12

Owner decisions, with evidence in [Ontomark ADR-OM-005](https://github.com/DreamLab-AI/ontomark/blob/ffe06e4/docs/adr/ADR-OM-005-estate-alignment-one-producer-explicit-projections.md). This record stays **proposed**; `verified_commit` is deliberately empty so no staleness is claimed.

- **Consumer, not producer.** §3's "explicit projection" is now the stated shape: VisionClaw's GPU force superset projects the shared bundle; it does not re-parse the corpus to build the ontology. A read-only gap check at `3c3f4120` found the layout reads no bundle artefact today (only the generation id, `crates/visionclaw-ontology/src/open_world.rs:720-797`); every input it needs is in the bundle except VisionClaw-owned registries (`SEMANTIC_TYPE_REGISTRY` weights, class charges in `gpu_resource_actor.rs:214-232` / `force_compute_actor.rs:1034-1050`, population scales, seeds) and derived presentation (`ln(bytes)` size, `wikilink_count`, keyword domain fallback). Its own Whelk run (`github_sync_service.rs:1183-1300`, `inferred_edge_materialiser.rs`) yields a different inferred set from `ontology-inferred.ttl`; §3 routes that through the shared reasoner.
- **VC-60 disposition under review: RETIRE, gated** (was REPIN). The pod-resource pack moves to the Agentbox side, which this server reaches directly. Retirement requires (i) a pod-resource producer outside this repo whose output is byte-equal to `scripts/ontology/pack-pod-resources.py` for a dated window; (ii) `src/services/ontology_pull.rs:42` repointed; (iii) **ADR-2106 re-verified or amended** — its `verified_paths` include `.github/workflows/ontology-publish.yml`, the pack script and the pull module, so any such edit arms its staleness check and fires its review trigger ("the release channel moving"); (iv) the input set: this server ingests `working/pages` (`src/services/corpus_source/local.rs:20`, `docker-compose.unified.yml:102`), the published machine artefacts are `knowledge/`-only, and the owner keeps `working/` in the view, so the consumed bundle is Ontomark's private `--vault all` machine bundle. Until all four pass, `ontology-publish.yml` keeps running unchanged.
- **Licence.** `crates/vault` and `crates/vault-core` are 100% owner-identity code (24 commits, 29,383 lines; only port marker `vault/src/gate.rs` ← own `pipeline/gate.py`), so the Consequences paragraph's "extracting AGPL code preserves its licence unless rights holders approve another arrangement" is resolved by the owner's grant in Ontomark `NOTICE`: extraction of the read/edit/gate/conflict verbs is a port. `nostr-bbs-core` (AGPL-3.0-only, owner-held) and the forum/pod posting stay here as the AGPL adapter (§6, DEPRECATION D4). `horned-owl` (LGPL-3.0, both 0.11.0 via the whelk fork and 1.2.0 direct, `crates/vault/Cargo.toml:38`) does not travel; Ontomark emits via `oxrdf`/`oxttl`.
- **Identity.** `visionGraph@<sha>` remains the generation identity; distribution is separate. Re-minting identities across the synthetic corpus is permitted at this stage provided §4's mapping, visibility diff and rollback artefacts ship with it.
- Not decided: per-edge property IRI in the NGG1 tiers (a wire-contract change; the kernel wants it at `edge.rs:239-247`); dual-licensing the transport.
