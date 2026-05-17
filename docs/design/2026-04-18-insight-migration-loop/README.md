# Insight Migration Loop — Design Corpus (2026-04-18)

## Summary

The Insight Migration Loop is the operational path by which tacit knowledge captured in Logseq notes becomes formal OWL ontology classes inside VisionClaw's reasoning core. This folder contains the complete design corpus — nine artefacts produced by a 9-agent Phase 1 research mesh on 2026-04-18 — that specifies the mechanisms, acceptance criteria, and decision rationale for physics-visible, broker-approved, Nostr-signed migration. The loop bridges human narrative (Logseq) into machine-executable semantics (OWL) whilst maintaining cryptographic auditability and owner consent at every step.

## How to Read This Folder

**Recommended reading order:**

1. **00-master.md** — Start here. Synthesised reconciliation across all nine research threads; encodes the five design pillars and owner-facing decision summary.
2. **../../prd-insight-migration-loop.md** — Product requirements and success criteria (external reference).
3. **../../adr/ADR-048-dual-tier-identity-model.md** and **ADR-049-insight-migration-broker-workflow.md** — Architecture decision records; the "why" behind the design.
4. **01-prior-art.md** — Competitive landscape and theoretical anchoring.
5. **02-bridge-theory.md** — Formal foundations: ontology semantics and migration theory.
6. **03-physics-mapping.md** — CUDA + Rust implementation specification; the "how" at metal level.
7. **05-candidate-scoring.md** — Sigmoid scoring mathematics for note promotion heuristics.
8. **04-acceptance-tests.feature** — Gherkin acceptance suite; falsifiable behaviour definitions.

## File Index

| File | Purpose | Words |
|------|---------|-------|
| 00-master.md | Synthesised reconciliation; five pillars; owner decisions | 1,797 |
| 01-prior-art.md | Comparative positioning across LLM-bridge, semantic web, and ETL literature | 2,328 |
| 02-bridge-theory.md | Formal ontology semantics, migration calculus, type-theoretic foundations | 1,602 |
| 03-physics-mapping.md | CUDA tensor operations + Rust trait architecture; embedding + relevance scoring | 4,981 |
| 04-acceptance-tests.feature | Gherkin: 6 features, 24 scenarios (end-to-end, broker, signature, rollback) | — |
| 05-candidate-scoring.md | Sigmoid heuristic; promotion confidence thresholds; edge cases | 1,872 |

## Related Documents Outside This Folder

- `../../prd-insight-migration-loop.md` — Product requirements
- `../../adr/ADR-048-dual-tier-identity-model.md` — Identity model architecture decision
- `../../adr/ADR-049-insight-migration-broker-workflow.md` — Broker workflow architecture decision
- `../../explanation/ddd-insight-migration-context.md` — DDD bounded context definition
- `../2026-04-18-unified-knowledge-pipeline.md` — Sibling design: end-to-end pipeline
- `../../audits/2026-04-18-logseq-ontology-audit/00-master.md` — Triggering audit; current ontology state

## Phase 2 Status

Phase 2 (explanation mesh) is underway. Audience-level explanation and tutorial documents will be published at `docs/explanation/insight-migration-loop.md` and `docs/tutorials/promoting-a-note-to-ontology.md`. Phase 3 (implementation sprint) commences after owner approval of the five pillars defined in 00-master.md §10.
