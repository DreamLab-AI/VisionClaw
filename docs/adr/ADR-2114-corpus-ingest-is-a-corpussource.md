---
id: ADR-2114
title: "Corpus ingest is a `CorpusSource`; the local vault directory is the default source"
date: 2026-09-22
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: [ADR-2096]
superseded_by: []
verified_commit: 06dfe97a55e6a7a42bfc74a26a513108c60d5735
verified_paths: []                # the change is uncommitted in the working tree; the staleness gate stays inert until the owner commits it
owner: jjohare
review_trigger: "WS-C's `vault_core::parse_page` landing, at which point `services::page_parser::parse_page` delegates to it and implementation_status moves to complete; or the first VisionClaw boot that ingests the migrated frontmatter-only corpus"
repo: visionclaw
domain: VAULT-corpus-format
lineage: "ADR-2040 (vault layout and the inclusion gate), ADR-2112 (frontmatter-only OKF corpus), PRD-sovereign-corpus Q2"
---

# ADR-2114 — Corpus ingest is a `CorpusSource`; the local vault directory is the default source

## Context

Ingest was spelled "GitHub". `GitHubSyncService` reached into the GitHub content
API for three things — the page listing, a per-page change marker (the blob SHA)
and the page body — and everything downstream of that was already source-agnostic.
The corpus is now a local vault (`visionGraph/`, PRD-sovereign-corpus Q1) that the
estate holds on disk in the agent workspace volume, so pulling it over the network
from a mirror is a round trip that can disagree with the vault the curator edits.
A second ingest path, `LocalFileSyncService`, existed as a local-baseline/GitHub-delta
hybrid; it was wired to one binary, duplicated the parse and never ran in the server.

## Decision

The three needs above are a port, `services::corpus_source::CorpusSource`:
`describe()`, `base_paths()`, `list_pages()`, `list_pages_under(prefix)` and
`fetch_page(page)`, over a `CorpusPage { name, path, change_marker, size, fetch_ref }`.

Two implementations ship. `LocalDirectorySource { root, base_paths }` walks
`VAULT_ROOT` over `VAULT_BASE_PATHS` (default `knowledge/pages,working/pages`),
skipping `.obsidian/`, `.trash/`, `_misc`, `journals/`, `bak/`, dot-files and
non-`.md`, with `mtime:size` as the change marker. `GitHubSource` is the previous
behaviour unchanged. Selection is `CORPUS_SOURCE=local|github`, defaulting to
`local` when `VAULT_ROOT` is set.

`sync_graphs()` is unchanged: parse → dual graph → post-sync Whelk reasoning →
inferred-edge materialisation → `ReloadGraphFromDatabase`. The per-page parse has
one seam, `services::page_parser::parse_page`, which delegates to the current
JSON-LD parser today and to `vault_core::parse_page` after the migration.

The dead Logseq-syntax parsers go with it: `visionclaw-ontology`'s
`ontology::parser` module (`parse_logseq_file`, `LogseqPage`, the `key:: value`
`PROPERTY_RE`, `logseq_properties_to_owl`, `OntologyAssembler`), the never-handled
`ProcessOntologyData` message that was its only reference, the
`ontology_content_analyzer` `key::` regex bank and the `ontology_file_cache` and
`OntologyFileMetadata`/`OntologyPriority` types orphaned by the deletion below.
The `ParsedMarkdown` contract's docs and its ts-rs binding are frontmatter-only.

`LocalFileSyncService` and its `sync_local` binary are deleted; `sync_github` is
renamed `sync_corpus`; `load_ontology` walks the corpus through the same port.
The sync database records the source identity under `corpus_source_identity`, and
a change of source forces a full re-sync rather than an incremental top-up.

## Consequences

The server ingests the vault the curator edits, with no network hop and no
credentials. One parse seam means the migrated corpus is adopted by editing one
function. The GitHub path stays exercisable but unused.

Costs: a store built before this ADR carries no `corpus_source_identity`, so the
first local sync after the upgrade needs an explicit `FORCE_FULL_SYNC=1` (documented
in `docs/how-to/operations/corpus-source-local.md`). The compose mount is an
external named volume — the stack refuses to start if the agent workspace volume is
absent, which is the intended loud failure. `GitHubSyncService` keeps its historical
name; a repo-wide rename of the type is deliberately not part of this change.

The remaining `key:: value` readers are the *live* ADR-2040 D3 tolerance in
`visionclaw_domain::vault::parse` (the publish gate), `KnowledgeGraphParser` and
`services::parsers::ontology_parser`. They stay until WS-D's migration run has
removed the 23,900 `key::` lines from the vault — dropping them earlier would
gate out every page whose only publish marker is `public:: true`. The `json-ld`
fence parser likewise stays until `vault_core::parse_page` replaces
`services::page_parser::parse_page`, at which point both go in that single edit.

## Verification

`cargo test --test corpus_local_sync` (3 tests) runs `sync_graphs()` over a
twenty-page vault fixture on disk: 20 pages listed, 19 nodes ingested (the
`public: false` working page is gated out), 27 edges, an unchanged second sync
skipping every page, a rewritten page re-processing alone, and a full sync
rebuilding the assert graph and persisting a Whelk closure.
`cargo test --lib corpus_source` (14 tests) covers the walk, the skip rules, the
change marker's stability, the descriptor identity and source selection.
`cargo test --lib page_parser` (3 tests) pins the parse seam: a fenced page yields
a canonical entity, and a `key:: value` line is body text that yields none and
never overrides a fence.
