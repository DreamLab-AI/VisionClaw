---
id: ADR-2096
title: Route local file sync's inclusion gate through the vault parsing entry point
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: [ADR-2114]
verified_commit: b0bc275f6501aae7751b85a72ce15fe1e730e7e8
verified_paths: []
owner: jjohare
review_trigger: A new corpus reader, removal of the Logseq legacy tolerance in visionclaw_domain::vault, or any change to the §V4 inclusion gate.
repo: visionclaw
domain: VAULT-corpus-format
lineage: enforces ADR-2040 (single parsing entry point, supersedes ADR-2014); diagram docs/diagrams/visionclaw/21-corpus-ingest-and-vault.md:198
---

# ADR-2096 — Route local file sync's inclusion gate through the vault parsing entry point

## Context
ADR-2040 makes `visionclaw_domain::vault::parse` the single parsing entry point
for the authored corpus, and VAULT-corpus-format §V4 defines the inclusion gate
on parsed metadata. `FileService::page_is_kg_included` and
`github_sync_service::page_is_kg_included` both delegate to it.

`LocalFileSyncService::process_file_content` did not. It scanned the first 20
lines for a line equal to `public:: true` or `public::true` after trimming and
lowercasing. That reader carried its own copy of carrier knowledge, and the copy
was wrong in both directions: it could not see the Obsidian frontmatter carrier
(`public: true`) that the corpus is converting to, so those pages were dropped
from local sync while GitHub sync ingested them; and it matched the Logseq
marker anywhere in those 20 lines — inside a code fence or mid-body — which is
the precise leak ADR-2040's bounded tolerance was written to close. A third
scan, `content.contains("public::")`, drove the `skipped_files` counter and
agreed with neither branch.

## Decision
`LocalFileSyncService` gets a `page_is_kg_included(content)` helper that is
exactly `visionclaw_domain::vault::parse(content).is_kg_included()` — the same
one-line delegation `FileService` and `GitHubSyncService` use. The gate is
evaluated **once** per file into `kg_included`, alongside `has_ontology_block`
for the `### OntologyBlock` marker (now a named constant), and both branches
plus the skipped-files counter read those two values. No reader in this crate
re-implements a line scan for a vault property.

## Consequences
- Local sync and GitHub sync now admit the same set of pages. Frontmatter pages
  that local sync silently dropped are ingested; pages that merely quote
  `public:: true` in prose or a code fence are no longer ingested.
- `owl-class` pages without `public` are now admitted by this reader too, which
  is §V4's formal-data route and matches the sibling readers. `public_pages` is
  a function-local set that nothing reads after the loop, so this widening has
  no visibility consequence.
- `skipped_files` is derived from the two real decisions instead of a third
  disagreeing heuristic, so the statistic now means what its name says.
- The Logseq tolerance can be retired in one place when ADR-2040's
  `review_trigger` fires; this reader needs no further change.

## Verification
`cargo check --workspace --all-targets` exit 0.
`cargo test -p visionclaw-server --lib -- local_file_sync` — 5 new tests pass,
covering the behaviour the old scan pinned (`leading_logseq_public_property_is_included`),
what it could not see (`obsidian_frontmatter_is_included`), the leak it had
(`quoted_or_mid_body_marker_is_not_included`), and the fail-closed default
(`a_page_with_no_metadata_is_private`, `explicit_public_false_is_not_included`).
Verification ran on the uncommitted working tree above commit
`b0bc275f6501aae7751b85a72ce15fe1e730e7e8`.
