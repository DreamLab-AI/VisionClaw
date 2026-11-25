---
id: ADR-2220
title: The vault readers accept vocabulary 3 headers beside vocabulary 2, through one shared reading
date: 2026-11-25
decision_status: accepted
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 6a946b8bcd97666cdf72c61c0cc4bc2bcb541a51
verified_paths: [crates/vault-core/src/promotion.rs, crates/vault-core/src/visibility.rs, crates/vault-core/src/roster.rs, crates/vault-core/src/lifecycle.rs, crates/vault-core/src/principal.rs, crates/vault-core/tests/ontomark_v3_parity.rs, crates/visionclaw-domain/tests/ontomark_v3_parity.rs]
owner: jjohare
review_trigger: ontomark releasing vocabulary 3 and the corpus being migrated (then delete the vocabulary 2 reading paths listed below), or the corpus sync being taught to honour authored clocks
repo: visionclaw
domain: VAULT-corpus-format
lineage: visionGraph ADR-VG-010 (time, supersession, typed sources, memory kinds), ADR-VG-011 (stamps name a principal key and the instrument), ADR-VG-012 (identity and visibility), ADR-VG-013 (review lifecycle); VisionClaw ADR-2113 (one parser), ADR-2112 (frontmatter-only), VisionFlow ADR-2022 (corpus history); estate plan of 2026-11-25
---

# ADR-2220 — The vault readers accept vocabulary 3 headers beside vocabulary 2, through one shared reading

## Context

visionGraph's vocabulary 3 is decided and being rolled out (ontomark's reader and the page migration are on an unreleased branch; the corpus is still vocabulary 2). It changes what a page header says: `visibility` replaces `public` on working pages, `generated.by` becomes a `did:nostr:` principal with the instrument in `via`, `sources` gain `rel`, `locator` and pins, and working pages gain `Brief`, `Proposal`, `Belief` and `Procedure` with a per-type `status`. VisionClaw reads headers in three places that did not share code: the domain parser (`PageMeta`, the sync and KG gate), `vault-core` (`Page`, `OkfBlock`, the `vault` CLI and build) and the publication census. By reading the code, and by tests that failed on the old code, a v3 header was mis-read, not rejected: a working page that lost `public` fell out of the graph (ontomark 0.9.0 measured 171 public working pages become 170 on that mistake; the same follows here because the gate read only `public`), a `did:nostr:` stamp was dropped without a word (`Stamp::from_yaml` returned `None`), and a human's `verified` entry lost its weight (`is_human_verified` is a prefix test). The old VisionClaw code was not run against a migrated copy.

## Decision

1. **One reading, in `vault-core`.** `visibility`, the stamp and actor grammar, the principal roster, per-type status and current recall live in `vault_core::{visibility, okf, principal, roster, lifecycle}`. `visionclaw-domain` depends on `vault-core` and calls them; the census in `vault` does too. There is no second parser to drift.
2. **Both header forms give one answer.** `public: true` is `visibility: public`. Where `visibility` is present and readable it decides (as ontomark's publisher does); an unknown value, or a non-boolean `public` with no `visibility`, is not public; two disagreeing keys are reported and the census refuses to build over them. Removing `public` from a page that says `visibility: public` does not unpublish it. A re-rendered page keeps the key it was authored with.
3. **Stamps name a principal through the roster.** `did:nostr:<64 lowercase hex>` is an `Actor` beside `human:<npub>`; `via` is kept; `human:<npub>`, a `did:nostr:` key and an `also` key resolve to one canonical principal. Human certification follows the roster: a `human:` actor is human, a `did:nostr:` actor only if the roster resolves it to `kind: human`, so Fungible Token keeps its human `verified` after its stamp is rewritten. This reaches the promotion state machine too (`PromotionState::of_with`, `promotion::apply_with`): a human's `did:nostr:` approval is accepted, an organisation's is refused; the roster-less `of` and `apply` keep the vocabulary 2 rule. bech32 is the maintained `bech32` crate, pinned `=0.11.1` (the release ontomark pins), checked against the NIP-19 vector and the seven published npub/hex pairs.
4. **Nothing is dropped silently.** An unreadable `generated`, `verified` or `sources` value is listed (`OkfBlock::problems`, `PageMeta::stamp_problems`) and reported by `vault validate` as `INVALID_STAMP` (warning under vocabulary 2, error under 3); a re-render writes it back as authored. Typed sources keep `rel`, `at`, `locator`, pins and unknown fields; an id-only source is legal.
5. **The new kinds are recognised.** The vocabulary loader reads the vocabulary 3 `working_types` map and `working_required`, `valid-from`, `valid-to`, `supersedes`, `superseded-by` and `statements` are known keys from version 3, and `vault validate` checks `status` per type (memory kinds take `draft`, `approved`, `superseded`, `invalidated`, `archived`; a vocabulary may declare a set for a type). `lifecycle::is_recallable` is the one recall rule: an `approved` memory kind inside its valid-time window; never a brief, a proposal, or a superseded, invalidated, archived or draft page.
6. **Drift from ontomark fails the build.** ontomark's vocabulary 3 fixture vault is vendored byte-for-byte (hash manifest) with VisionClaw's own pages, and `golden.json` holds ontomark's answers for every page, generated by calling ontomark's predicates. `vault-core` and the domain parser are asserted equal to it. One difference is deliberate and named in the test: a `human:<name>` resolves through the roster label, so the page names the same principal before and after its migration.

## Consequences

- A page answers the same (visibility, principal, human certification) before and after it is migrated. Recall is the one answer that changes by design: the 188 Episodes become `approved` and 109 aggregates become Briefs, so what is recalled changes at the migration, not because a reader differs.
- **Deleted once the vault is fully on vocabulary 3** (a small follow-up, not done here because both forms must read until the migration lands): the `human:` actor and its prefix rule (`Actor::Human`, `Actor::is_human`, `OkfBlock::is_human_verified`, the `human:<name>` label route in `Roster::resolve`, and the named exception in the parity test); the roster's vocabulary 2 `generators` section and `GeneratorFacts`; the vocabulary 2 list form of `working_types`; the `INVALID_STAMP` warning branch and the `is_v3()` severity and status branches in `vault validate`; the `Status` enum's role for working pages; and, for working pages, the `public:` reading (`Form::LegacyPublic`, `Problem::InvalidPublic`, `Problem::Conflict`). Knowledge pages keep `public: true` under vocabulary 3, so `visibility::read` becomes vault-aware instead of losing the `public` key outright.
- `visionclaw-domain` now depends on `vault-core` (and so on `ontomark-parse`, `rdfs-certificate` and `bech32`): the cost of one reading.
- **Not done here.** The corpus sync does not yet honour authored clocks (a page that says `corrected`, `withdrawn` or `world-changed`); see the scope below. Replacing `vault-core`'s header code with ontomark's parser needs a released, light crate; today nothing of the kind is published (visionGraph's v3 reader is unreleased and lives in `ontomark-compile`, which brings the reasoner and takes ontomark's own `Page` types).
- The root crate type-checks with the feature set the temporal work used (see Verification); its test targets cannot be compiled in this container.

## Scope: the corpus sync honouring authored clocks (planned ADR-2022 P2 amendment; not implemented)

The sync (`corpus_history::plan_history`) closes every open version whose triple has left the public projection, with a valid-time closure at the sync instant. Authored clocks would add, per page: `corrected` or `withdrawn` produces a recorded-time closure through `build_correction`; `world-changed` or a deprecated-but-public page's `valid-to` produces a valid-time closure at that `valid-to`; knowledge `merged` and `renamed` close nothing. It needs the page header to reach the planner (today `public_subgraph` sees only node metadata), a `(page, version)` link, and tests on the vector cases; the design, files and risks are in the delivery report and are to be written into ADR-2022's amendment when the work is scheduled.

## Verification

`cargo test -p vault-core -p visionclaw-domain -p vault` and `cargo clippy` / `cargo fmt --check` on those crates; the parity run over the real corpus (11,649 pages, old reader against new, zero differences) and the before/after migration run are recorded in the delivery report. Source and test only: nothing deployed, no store touched.
