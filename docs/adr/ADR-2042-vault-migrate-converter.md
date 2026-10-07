---
id: ADR-2042
title: "`vault-migrate` is the sole Logseq→Obsidian converter — deterministic, output-dir by default, preserve-and-report"
date: 2026-09-02
decision_status: superseded
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: [ADR-2113]
verified_commit: ed5644d0369f9df5ef40d3259858acf3c52e7277
verified_paths: [crates/vault-migrate, Cargo.toml, docs/VAULT-corpus-format.md]
owner: jjohare
review_trigger: the in-place conversion of the corpus repo is committed, after which the crate is kept only as the round-trip/no-op checker
repo: visionclaw
domain: VAULT-corpus-format
lineage: corpus-repo `obsidian_to_visionflow.py` (Obsidian→VisionFlow JSON-LD injector, the reverse direction), PRD-LCR-01 (swarm refactor of the same corpus)
---

# ADR-2042 — `vault-migrate` is the sole converter

## Context

The corpus is the owner's private graph (never pushed by agents). A
conversion that guesses, deletes, or edits in place by default would be an
unrecoverable action on that graph. The transformation is mostly mechanical
(leading property block → frontmatter, `___` → folders, a handful of body
rewrites) with a small tail of constructs that have no faithful Obsidian
equivalent (13 dangling block refs, 193 pages with body-level properties).

## Decision

1. One Rust binary, `crates/vault-migrate`, in the server workspace. No LLM,
   no network. Rules are exactly the V2/V3 tables in the governing doc.
2. Modes: `vault-migrate <logseq-graph-dir> --out <vault-dir>` (default,
   creates `<vault-dir>` and never touches the source) and
   `vault-migrate <dir> --in-place` (explicit; refuses to run on a dirty git
   working tree unless `--allow-dirty`).
3. Output includes a starter `.obsidian/` (`app.json` with `newLinkFormat:
   "shortest"`, `attachmentFolderPath: "assets"`; `daily-notes.json` with
   `folder: "journals"`, `format: "YYYY-MM-DD"`; `core-plugins.json`) and a
   `.gitignore` excluding `.obsidian/workspace*.json` and `.obsidian/cache`.
4. Unknown or unconvertible constructs are preserved verbatim and counted in
   `vault-migrate-report.json` (per-rule counts, per-file list of leftovers).
   The binary exits non-zero only on I/O failure, never on leftovers.
5. Idempotent: running on its own output is a byte-identical no-op and the
   report shows zero conversions (`--check` mode exits non-zero if it would
   change anything — the CI hook for the converted repo).
6. `--dry-run` prints the report without writing.

## Consequences

- The owner decides when to apply in place; agents run output-dir mode.
- The report is the evidence artefact for EXP-V04/V05.
- The crate is small (frontmatter emitter, property-block parser, path mapper,
  body rewriters) and unit-tested per rule with fixtures under
  `crates/vault-migrate/tests/fixtures/`.

## Verification

2026-09-02 on the `obsidian` branch: `cargo test -p vault-migrate` — 66 unit
+ 16 integration tests green (per-rule fixtures under
`crates/vault-migrate/tests/fixtures/logseq-mini` against a byte-exact
`expected-vault` golden, two-run byte identity, no-op on own output,
`--check` drift detection, source-graph never modified, `--in-place`
namespace rename). Real runs, output-dir mode only, source graphs
fingerprint-identical before and after:
`mainKnowledgeGraph` → `/home/devuser/workspace/vault` in 2.6 s
(8,655 pages, 8,634 converted; public 8,615, namespace 201, journals 308,
tasks 239, assets 168; leftovers: 14 block-ref files, 6,541 body-property
files, 4 SCHEDULED/DEADLINE journals; one unreadable root-owned tool file
reported); `workingGraph` → `/home/devuser/workspace/vault-working` in
0.65 s. `--check` on both outputs exits 0. Three survey rows in the
governing doc were corrected from the report (no page embeds exist; body
properties are the corpus's inline relation fields, 6,541 files not 193;
one task page is fully fenced). Two defects found by the real in-place run
and the shadow sync, both fixed the same day (67 unit + 16 integration
tests): zero-byte pages/journals refused to complete their rename (12
journals), and the first release wrote the identity path into `title:` on
223 namespace pages — `title` is display-only, echoes are now removed on
any rerun (`rules.title_echo_removed`) and `--check` flags them as drift.
Applied in place on the corpus (owner decision: branch-first, then a
history-preserving split into jjohare/visionGraph with `knowledge/` and
`working/` vault roots; the logseq `obsidian` branch is the archived
converted source). `activation_status: live` once visionGraph is the
configured `GITHUB_REPO`.

## Closeout extension — 2026-09-04

CP-01/02/08. Owner remains jjohare with vault/data maintainers. Current suite: 70 unit and 16 integration tests pass. Additional actual-CLI output-directory fixtures accept colliding legacy/folder paths and retain only one output body, while preserving both source files. Implementation is partial against complete content preservation. Historical activation evidence is retained, not repeated on the real corpus.

--dry-run combined with explicit --report writes that report, though it creates no vault output. This is narrower than the unconditional no-write decision. The planner's claimed-path set only protects starter config; it does not reject duplicate page destinations.

**Acceptance condition:** Reject or explicitly resolve every destination collision before writing; account for every source page/asset and validate consumer inclusion/identity after conversion. Cover mixed namespace/folder layouts, journal collisions, case/normalisation, existing destinations, source/output aliases and interrupted writes. Define report side effects in dry-run/check modes and test recovery before in-place promotion. Reopen on path mapping, body/frontmatter conversion, write planning or consumer format changes. See the [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/authored-vault-transition.md#converter-collision-and-dry-run-boundaries), [reproducer](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/vault-converter-probe.py) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/vault-converter-probe.json). No real corpus or in-place operation ran.

## Acceptance progress — 2026-09-05

**Implemented.** `crates/vault-migrate/`. All three acceptance items that do not
need the real corpus are closed.

1. *Destination collisions rejected or explicitly resolved before writing.* The
   reproduced defect — colliding legacy/folder paths were accepted, both source
   files preserved but only one output body retained — is closed.
   `resolve_collisions` groups the whole action plan by destination **before any
   write**. `CollisionPolicy::Fail` (the default) aborts the run naming every
   colliding destination and its sources, because keeping one body and
   discarding the other is data loss only the operator can adjudicate;
   `CollisionPolicy::Suffix` (`--on-collision suffix`) keeps the first source at
   the natural path and gives the rest a deterministic ` (2)`, ` (3)` … suffix.
   Either way the collision is recorded in the report as
   `report::Collision { destination, sources, resolution }`.
2. *Report side effects defined in dry-run/check modes.* Stated in the CLI
   long-help, the library docs and the artefact itself: `--dry-run` and
   `--check` produce **no vault output**; the single permitted side effect is
   the JSON report, and only when `--report <PATH>` asks for it (without it,
   `--dry-run` prints to stdout). The report records this in
   `report_side_effects`.
3. *Interrupted writes.* `write_atomically` / `copy_atomically` stage into a
   sibling temporary file and rename, so a killed run never leaves a truncated
   page that a later `--check` would read as valid; the temporary is removed on
   any failure.

**Tests.** `cargo test -p vault-migrate` — 70 unit + 28 integration passed,
0 failed (12 new integration cases): collision rejected before any write with
the message naming both sources and the resolution flag; suffix policy
preserving every body; suffix determinism across runs; a three-way collision
getting distinct suffixes; mixed namespace/folder layouts that do *not* collide
converting normally; journal collisions (`2026_01_02.md` vs `2026-01-02.md`);
`--check` surfacing collisions; dry-run producing no vault output; a refused run
leaving the source graph byte-identical; a truncated destination repaired by the
next run with no staging file left behind; `--check` seeing a truncated
destination as drift; and case-differing names handled on either filesystem.

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2042-vault-migrate.txt`.

**Remains open.** No real corpus or in-place operation ran. Accounting for every
source page/asset and validating consumer inclusion/identity after conversion is
not done, and recovery before in-place promotion is untested against a real
graph.

## Disposition — 2026-10-02

- **Suitability:** discordant
- **Priority:** withdrawn
- **Why:** The claim "vault-migrate is the sole converter" no longer holds. `crates/vault-migrate` was deleted in `f9720fbcb` and replaced by `vault migrate` in `crates/vault` (ADR-2113 "Follow-on"; ADR-2112 §"Supersession"). That converter was itself retired by ADR-2117 once the corpus was converted. `crates/` at `95e98ab12` holds no `vault-migrate`. TODO-unified row N-1 confirms that no rerun of conversion remains.
- **Next:** None. `decision_status` set to `superseded` with `superseded_by: [ADR-2113]`, the record that took over the converter role, and ADR-2113 now lists `supersedes: [ADR-2042]`. ADR-2117 amends ADR-2113 and is not a direct successor. The validator requires a `verified_commit`, so it is stamped `95e98ab12`. The other status fields are unchanged.

## Re-verification — 2026-10-03 at fdcbc9120fda25fd93fdaee744d68d2c00713d6b

`1d3e14a30` and `fdcbc9120` change `Cargo.toml` (the solid-pod-rs pin, feature `mrc20`, a `[patch.crates-io]` for `nostr-bbs-core`; ADR-2111, S4 amendment). None of it concerns the converter this superseded record describes, and `crates/vault-migrate` and `docs/VAULT-corpus-format.md` are untouched. Nothing to re-decide.

## Re-verification — 2026-10-03 at 780eb3edb788c9cb568d5756689a3c8455db79b7

`1e55daebb` removes the `nostr-bbs-core` `[patch.crates-io]` from `Cargo.toml`, which the 2026-10-03 note above covered. It does not concern this superseded converter, and `docs/VAULT-corpus-format.md` is untouched. Nothing to re-decide.

## Re-verification — 2026-10-07 at ed5644d03 (live memory cloud, ADR-2133)

**Governed change:** `Cargo.toml` adds the workspace member `crates/visionclaw-memory-cloud` (pure logic, no server-layer dependency), root dependencies `tokio-postgres 0.7.18`, `deadpool-postgres 0.14.2` and the new crate, and a `[profile.dev.package.visionclaw-memory-cloud] opt-level = 3` override. Line citations into `Cargo.toml` after line 15 shift by +1, after line 128 by +8 and after line 310 by +14. **Decision unaffected.** The vault and its migrate converter are untouched. `verified_commit` moved to `ed5644d03`. Source reading of the diff (`git diff 20499efc6..ed5644d03` on the governed paths) plus `cargo check --lib --bins` and `cargo test --lib -- auth rbac memory_cloud` (62 + 5 pass) at the landing commit.
