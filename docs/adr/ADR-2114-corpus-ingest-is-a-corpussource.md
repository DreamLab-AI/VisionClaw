---
id: ADR-2114
title: "Corpus ingest is a `CorpusSource`; the local vault directory is the default source"
date: 2026-09-22
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: [ADR-2096]
superseded_by: []
verified_commit: 772039bd172027af034d64befacca1ebcf0bbc04
verified_paths: [src/services/page_parser.rs, src/services/corpus_source/mod.rs, src/services/corpus_source/local.rs, crates/visionclaw-domain/src/vault/mod.rs]
owner: jjohare
review_trigger: "a second CorpusSource becoming the default; any change to services::page_parser::parse_page delegating to vault_core::page::parse_page; a request to read `key:: value` metadata again"
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

## Close-out 2026-10-30

`partial / staged → complete / live`. Verified at `772039bd172027af034d64befacca1ebcf0bbc04`.

**Implementation: both named gaps are closed in code.**
- `services::page_parser::parse_page` now delegates to `vault_core::page::parse_page`
  (`src/services/page_parser.rs:73`, `:87`).
- The remaining `key:: value` readers are gone. `services::parsers::ontology_parser` no longer exists.
- `visionclaw_domain::vault` treats a `key:: value` line as body text, `public:: true` included
  (`crates/visionclaw-domain/src/vault/mod.rs:20-27`; test at `:723-725`).
- A `json-ld` fence is body text too (`page_parser.rs:372-376`).

**Activation.** The record's own trigger was the first boot ingesting the frontmatter-only corpus. Runtime provenance: `visionclaw_container` runs `/app/target/dev-runtime/visionclaw-server` (build stamp `features=gpu,ontology,dev-auth`, i.e. the dev profile with the dev-auth bypass compiled in); its wrapper log (`/app/logs/rust.log`) reads `[RUST-WRAPPER][2026-10-30 19:01:06] Building from published inputs: source_sha=772039bd172027af034d64befacca1ebcf0bbc04 source_dirty=0`, so the responses below come from this commit.
- The container's environment has `CORPUS_SOURCE=local` and `VAULT_ROOT=/vault/visionGraph`.
- The 19:03 boot logged `Corpus source: local corpus at /vault/visionGraph (sources: knowledge, working)`
  and `Processing 0 changed files (10375 unchanged)`.
- That boot loaded 9,244 classes into Whelk. `GET /api/graph/data` serves 9,688 nodes.
- The corpus carries no `key::` lines (ADR-2112 close-out).
