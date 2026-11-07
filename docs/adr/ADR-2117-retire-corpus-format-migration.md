---
id: ADR-2117
title: Retire the completed corpus format migration
date: 2026-10-01
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 7bd25427e17c9592ef8cb9c07b632005fb10438c
verified_paths: [crates/vault/src/main.rs, crates/vault-core/Cargo.toml]
owner: jjohare
review_trigger: "any reintroduction of a corpus-format converter, a JSON-LD fence reader or migration mapping types; a second supported authored format"
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

## Close-out 2026-10-30

`staged → live`. `verified_commit` moves from the placeholder `working-tree` to `772039bd172027af034d64befacca1ebcf0bbc04`.

**Code at this commit.**
- `vault --help` lists `validate, propose, panel, gate, conflicts, build, certify, repair` and has no
  `migrate`. `crates/vault/src/main.rs:895` asserts that the retired command is rejected.
- `cargo test -p vault -p vault-core --no-default-features`: 544 passed, 0 failed, 5 ignored, including
  `golden_parity` 15 and `publish_gate` 6.

**Activation.** This record's trigger was the installation of the updated binary. The installed binary
(`/nix/store/83ilg82h5ar8vvnqqzysamvxc08iycxq-vault-0.1.0/bin/vault`) answers
`vault migrate --fences-to-properties` with `error: unrecognized subcommand 'migrate'`.

## Scoped re-verification — 2026-11-01, ADR-2168

The CLI adds shared discovery/layout checks, without restoring the retired migrate command or migration feature. The package suite retains the retired-command refusal test. Ontomark migration commands belong to the separate owner and do not reintroduce this retired VisionClaw converter.

All governed paths were byte-identical between the previous verification pin and
the pre-change baseline `5e1bd90a4`. The reviewed delta is the shared-layout change
through `974daf5d22d2d0ea709140777eae58d60ddb2c36`. Existing activation and acceptance limits
remain in force; this stamp does not repeat historical runtime measurements.

## Re-verification — 2026-11-07 (`7bd25427e`)

Governed-path change since `974daf5d2`: `crates/vault/src/main.rs` (`a5a265dd4`) lets a grouped `vault propose --dry-run` run without a signing key (`:455-461`, `:520-535`). No subcommand was added or removed. The close-out cites the retired-command test at `crates/vault/src/main.rs:895`; it is now `retired_migration_command_is_rejected` at `:941-942`, and `parse(&["migrate", "--fences-to-properties"])` must still fail. **Decision unaffected.** Re-run: `git diff 974daf5d2..7bd25427e -- crates/vault/src/main.rs`. Checked by reading that diff and the test at HEAD; the test was not re-run for this stamp.
