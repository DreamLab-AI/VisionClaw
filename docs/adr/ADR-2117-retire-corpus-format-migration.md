---
id: ADR-2117
title: Retire the completed corpus format migration
date: 2026-10-01
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: working-tree
verified_paths: []
owner: jjohare
review_trigger: installation of the updated vault binary and host deployment
repo: visionclaw
---

# ADR-2117 — Retire the completed corpus format migration

## Context

The corpus already uses Obsidian Markdown and YAML frontmatter. ADR-2113
required deletion of its one-shot converter after conversion, but the command,
fence reader and migration vocabulary types remained available. The owner
explicitly requested their removal to avoid competing authoring systems.

## Decision

Remove `vault migrate`, its implementation, `vault-core`'s migration feature,
JSON-LD fence reader and migration mapping types. Proposal diff rendering moves
to a shared helper. Convert the 50 regression fixtures to canonical Obsidian
input and retain their immutable publication references.

New imports must supply canonical Markdown/frontmatter through the current
creation/proposal workflow. Retain rejection of obsolete authoring syntax and
current body/fence repair; neither is a legacy corpus reader.

## Consequences

There is one supported authored format. Historical ADRs and fixture provenance
remain evidence, not operational instructions. Existing installed binaries and
host images must be updated separately; this source change does not claim deployment.

## Verification

`cargo test -p vault -p vault-core --no-default-features`: 452 passed, four
existing external/real-corpus probes ignored. This includes 15 golden publication
checks, six publication-gate tests, and explicit rejection of the retired CLI
command. `cargo fmt -p vault -p vault-core` and `git diff --check` pass.
Verification applies to the working tree; no commit or deployment is claimed.
