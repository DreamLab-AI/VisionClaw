---
id: ADR-2115
title: "GitHub sync is optional and off; the `logseq` settings alias is removed"
date: 2026-09-22
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: [ADR-2041]
superseded_by: []
verified_commit: 06dfe97a55e6a7a42bfc74a26a513108c60d5735
verified_paths: []                # the change is uncommitted in the working tree; the staleness gate stays inert until the owner commits it
owner: jjohare
review_trigger: "any decision to re-enable GitHub ingest as a default, or a report of a client still sending `logseq` graph-type values"
repo: visionclaw
domain: VAULT-corpus-format
lineage: "ADR-2041 (the bounded `logseq` read alias), ADR-2114 (the CorpusSource port)"
---

# ADR-2115 — GitHub sync is optional and off; the `logseq` settings alias is removed

## Context

Two leftovers from the Logseq era outlived their release. `GitHubConfig::from_env()`
required `PRIVATE_REPO_GITHUB_PAT`, `GITHUB_OWNER` and `GITHUB_REPO`, so an estate
that ingests from its own vault still had to carry a token for a repository it does
not read. ADR-2041 introduced a bounded, one-release `logseq` read alias for the
renamed `knowledge` graph-settings key — accepted on the serde key, the graph-type
discriminator, the `graphs` JSON object key, the dotted settings path and the binary
settings-path registry. Its `review_trigger` has passed, and the owner's governing
principle for the sovereign-corpus work is that there are no compatibility shims:
git is the rollback.

## Decision

GitHub ingest is optional and off. With `CORPUS_SOURCE=local` (the default when
`VAULT_ROOT` is set) no GitHub variable is read, and boot proceeds with the
`GitHubConfig::disabled()` placeholder. Selecting `CORPUS_SOURCE=github` makes the
credentials required again, and the failure is at selection time, not at boot.

The `logseq` alias is removed everywhere: the `#[serde(alias = "logseq")]` on
`GraphsSettings::knowledge`, the acceptance in `normalise_graph_type`,
`knowledge_graph_value`, `graphs_map_has_knowledge` and
`path_targets_knowledge_graph`, the settings handlers, the optimised settings
actor, `PathAccessible for GraphsSettings` and the binary settings-path registry's
alias table. `logseq` is now an unknown graph type: a settings document keyed on it
fails to deserialise, and `visualisation.graphs.logseq.*` does not resolve.

## Consequences

An estate can run with no GitHub credentials at all. A persisted settings document
still using the `logseq` key must be renamed once — the rename is mechanical and
the error is explicit ("missing field `knowledge`") rather than a silent default.
Clients that still send `logseq` graph-type values get a rejection instead of a
quiet redirect; none are known in the estate.

## Verification

`cargo test --test settings_deserialization_test adr2115_graph_settings_key`
(5 tests) asserts a retired `logseq` document is rejected, the canonical one round
trips, serialisation never emits `logseq`, the retired path segment no longer
resolves and `normalise_graph_type("logseq")` passes the value through unchanged.
`cargo test --lib github::config` (5 tests) includes
`the_disabled_placeholder_is_valid_without_any_github_env`, and
`cargo test --lib corpus_source` includes
`a_local_source_boots_without_any_github_configuration`, which builds the local
source with every GitHub variable removed from the environment.
