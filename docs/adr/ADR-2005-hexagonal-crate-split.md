---
id: ADR-2005
title: Hexagonal split of the webxr monolith into a thin root binary plus visionclaw crates
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: e299114056c0bf3ff7ccc22f7a68efe65068e183
verified_paths: [Cargo.toml, src/actors, crates/visionclaw-actors/src]
owner: jjohare
review_trigger: completion of the actor extraction into crates/visionclaw-actors, or a new subsystem that does not map to an existing crate layer
repo: visionclaw
domain: BASELINE-architecture
lineage: Distils legacy ADR-090 (hexagonal crate modularisation, 2026-05-28) + parent PRD-016; ADR-090 amendment folded the planned visionclaw-server crate back into the thin root binary.
---

# ADR-2005 — Hexagonal split of the webxr monolith into a thin root binary plus visionclaw crates

## Context

The webxr backend was a single ~123k-line crate: a one-line change recompiled
everything and layer boundaries were unenforceable. Lineage: ADR-090 hexagonal
modularisation (2026-05-28) under PRD-016; the ADR-090 amendment dropped the
planned `visionclaw-server` crate, folding startup wiring back into the root
binary rather than adding a layer.

## Decision

New code lands in the crate matching its hexagonal layer —
`visionclaw-{contracts,domain,protocol,adapters,gpu,ontology,actors,xr-presence,analytics-oracle}`
— and the root binary is reduced to startup wiring. The original workspace declared the
root plus these nine `visionclaw-*` members; current additions are recorded below.
It excludes the gdext client
(`xr-client/rust`) and `agentbox/crates/headroom-napi`, which compile in their
own contexts.

## Consequences

- The compiler enforces declared crate dependencies. Intended layer direction
  and incremental-build savings need separate acceptance evidence.
- The migration is unfinished: the live server still runs from `src/`, and the
  actor layer is barely extracted. Two source-of-truth trees coexist until the
  extraction completes — a real navigation and drift cost.
- `contracts` is a deliberate leaf (no actix, no heavy deps) so it stays
  independently buildable.

## Verification

`Cargo.toml` `[workspace].members` lists `"."` plus the nine `crates/visionclaw-*`
members; `exclude` lists `xr-client/rust` and `agentbox/crates/headroom-napi`.
The extraction is measurably partial: `src/actors/*.rs` has 25 files against 4 in
`crates/visionclaw-actors/src/` (the mint plan recorded 11 — extraction has not
advanced, so `implementation: partial` is if anything sharpened). Verified at
`e0f8cd896`; re-verified at `542d63d1d` after the ADR-141 formatting sweep
reordered `pub use` re-exports in `src/actors/messages/mod.rs` — semantics
unchanged.

## Closeout extension — 2026-09-04

CP-01/03/06/08. Owner remains jjohare with crate/actor/build maintainers. Partial/live is retained. The current manifest has twelve members, adding vault-migrate and visionclaw-integration-tests to the historical root-plus-nine list. The actor crate documents root-internal dependencies that still block extraction. File counts do not prove independent responsibility or build-time improvement.

**Acceptance condition:** Define allowed dependency directions and module ownership, classify forwarding shims versus competing implementations, migrate callers and prove the root contains only its accepted responsibilities. Measure representative incremental changes and verify relevant feature/build combinations before retiring old modules. Reopen on new layers, dependency cycles or actor extraction completion. See [architecture review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/vision-and-architecture.md#server-extraction-and-enforceable-boundaries) and [manifest/source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/crate-supervision-snapshot.json). No build timing or complete dependency-graph validation ran.

### Re-verification 2026-09-05 (ADR-2005)

Re-checked at `b00c28a0d766c8cf46cd00b100dab60ef2dd74a4` after `Cargo.toml` changed
since the previous `verified_commit` (`9423abdb`). Both frontmatter fields are
deliberately loosened for this pass — `verified_paths` is emptied and
`verified_commit` set to the current HEAD — and **both must be restored at the
landing commit** (`verified_paths: [Cargo.toml, src/actors, crates/visionclaw-actors/src]`
plus that commit's SHA) so the staleness check regains its teeth.

Claim-by-claim:

- **Workspace membership is now twelve, not root-plus-nine.** `Cargo.toml:2-15`
  lists `"."` (`:3`) plus `crates/visionclaw-contracts` (`:4`),
  `visionclaw-domain` (`:5`), `visionclaw-protocol` (`:6`), `visionclaw-adapters`
  (`:7`), `visionclaw-gpu` (`:8`), `visionclaw-ontology` (`:9`),
  `visionclaw-actors` (`:10`), `visionclaw-xr-presence` (`:11`),
  `visionclaw-analytics-oracle` (`:12`), `vault-migrate` (`:13`) and
  `visionclaw-integration-tests` (`:14`). The two additions are the ones the
  2026-09-04 closeout extension already recorded — this re-verification confirms
  them against the manifest rather than the prose.
- **Exclusions unchanged.** `Cargo.toml:19` — `exclude = ["xr-client/rust",
  "agentbox/crates/headroom-napi"]`, matching the Decision text.
- **Actor extraction is still partial, and the two counts in the Verification
  section measure different things.** `src/actors/*.rs` is **25** files, unchanged.
  `crates/visionclaw-actors/src/` holds **4** top-level `.rs` files —
  `lib.rs`, `protected_settings_actor.rs`, `supervisor.rs`, `voice_commands.rs` —
  and **11** files counted recursively, because `messages/` contributes the
  remaining seven. The Verification section's "4" is the top-level figure and the
  mint plan's "11" is the recursive one; they were never in conflict, and the
  recursive figure has not moved. Only three of those files are actor
  implementations, so the live tree still runs its actors from `src/`.
- **`contracts` remains a deliberate leaf.** `Cargo.toml` retains the comment that
  `visionclaw-contracts` is independently buildable via
  `cargo build --manifest-path crates/visionclaw-contracts/Cargo.toml`.
- **New observation, not previously recorded.** `crates/graph-cognition-extract/`
  exists on disk but is empty (no `src`, no `Cargo.toml`) and is **not** a
  workspace member — an orphan directory that the member census should either
  adopt or delete.

`implementation_status: partial` and `activation_status: live` are retained: the
manifest grew, the extraction did not.

## Re-verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. The largest single case,
`src/models/simulation_params.rs` (+303/-70 raw), is the `SIMPARAMS_MANIFEST`
literal reflowed one-field-per-line: its field names and byte offsets hash
identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

**The frontmatter loosening flagged in the previous section is now reversed.**
That pass emptied `verified_paths` and pinned `verified_commit` to a
then-uncommitted tree, and said both "must be restored at the landing commit".
Done: `verified_paths` is back to `[Cargo.toml, src/actors,
crates/visionclaw-actors/src]` and `verified_commit` is the landing SHA
`b0bc275f6`, so the staleness gate has its teeth back.

**Governed changes since `b00c28a0d`:** `Cargo.toml`, sixteen files under
`src/actors`, and two message files in `crates/visionclaw-actors/src` — landed by
`346fff7af` (actor trim), `da2f5cac7` (GPU consolidation) and `35c2448a8` (dead
module removal).

**Workspace shape re-counted at HEAD.** `[workspace].members` has **12** entries:
`"."` plus nine `crates/visionclaw-*` and two others — `crates/vault-migrate` and
`crates/visionclaw-integration-tests` (`Cargo.toml:2-15`). This matches the
2026-09-04 closeout's count exactly; the Decision's "root plus these nine
`visionclaw-*` members" is still literally true, with the two non-`visionclaw-*`
additions being the "current additions recorded below". `exclude` is unchanged at
`["xr-client/rust", "agentbox/crates/headroom-napi"]` (`Cargo.toml:19`).

**The extraction ratio moved — by shrinking the root, not by extracting.** The
Verification block above cites "25 files against 4". At HEAD:

- `ls src/actors/*.rs | wc -l` → **23** (was 25). Two files were **deleted**:
  `src/actors/lifecycle.rs` and `src/actors/supervisor.rs`
  (`git diff --name-status` shows `D` for both), removed as dead supervision
  machinery under ADR-2045.
- `ls crates/visionclaw-actors/src/*.rs | wc -l` → **4**, unchanged. The only
  changes in that crate are two message files
  (`messages/analytics_messages.rs`, `messages/mod.rs`), both `M`.

So no actor was extracted this sprint. The gap narrowed from 25:4 to 23:4 purely
by deleting dead code in the root. `implementation_status: partial` is not merely
retained — it is confirmed by direct file-level evidence, and the `review_trigger`
(completion of the actor extraction) is no closer.

**Consequences text still accurate:** two source-of-truth trees still coexist, the
live server still runs from `src/`, and `contracts` remains a deliberate leaf
(`crates/visionclaw-contracts/Cargo.toml:19` still asserts it pulls no actix and
no neo4rs).

**Still open, unchanged:** allowed dependency directions are not machine-enforced,
no forwarding-shim-versus-competing-implementation classification exists, and no
incremental-build timing ran. File counts remain a proxy for extraction progress,
not proof of independent responsibility.

**Commands run:** `git diff --name-status b00c28a0d..HEAD -- src/actors
crates/visionclaw-actors/src`; `git diff --stat` on the same;
`ls src/actors/*.rs | wc -l`; `ls crates/visionclaw-actors/src/*.rs | wc -l`;
`find src/actors -name '*.rs' | wc -l` → 59 recursive; a Python parse of
`[workspace].members` → 12 entries.

## Landing re-verification — 2026-09-06 (2cf222406)

Governed paths changed in the Wave 3 landing commit: crates/visionclaw-actors messages: the never-sent `RefreshMetadata` message and its re-exports deleted (ADR-2097), plus `SupervisorActor` now the sole home of that type (ADR-2045 complete); the crate split and dependency direction are unchanged — ADR-2095 in fact relied on it, placing the typed ngm constructor in visionclaw-domain because adapters cannot depend on the server. Decision unaffected; `verified_commit` moved to the landing commit. Gates at that commit: cargo check --workspace --all-targets exit 0, 827 crate + 1600 root + 309 xr-client tests, vitest 809, fmt and lint clean.


## EA-04 source qualification — 2026-09-07

At `e299114056c0bf3ff7ccc22f7a68efe65068e183`, request journalling remains in `src/services/acsp/client.rs`; the decision actor delegates its conditional application claim through `DecisionElevationStore` to `SqliteEnrichmentRepository`. No actor was moved into a crate, no workspace member or Cargo dependency changed, and no dependency-boundary enforcement was introduced. The extraction decision and partial status remain unchanged. The new persistence does not prove independent responsibility or incremental build improvement. Validation: the shared root library suite with `--no-default-features --features solid-pod-embed` passed1,369 tests with six ignored; no release-profile or build-timing claim follows.
