# ontomark vocabulary 3 fixture, shared with VisionClaw

Upstream: `crates/ontomark-cli/tests/vocab-v3-vault` of DreamLab-AI/ontomark at
tag `v0.10.0` (commit `fa173a183e65a29944becb6eb4d55b52b9ba6b06`). The 39 upstream files are vendored byte-for-byte; `UPSTREAM.sha256`
lists their hashes and `tests/ontomark_v3_parity.rs` fails if one is edited. To
update, replace the whole tree from a newer ontomark commit and regenerate
`golden.json`.

`Vc*.md` pages are VisionClaw's additions: the cases the upstream fixture does
not reach (a working page public under each spelling, the two keys disagreeing,
the recall window, every principal spelling, human certification on a knowledge
page). They are not in `UPSTREAM.sha256`.

`golden.json` is **ontomark's** answer for every page, under vocabulary 2 and 3,
at `now = 1794961600` (2026-11-18T00:26:40Z). It is produced by calling
ontomark's predicates, not by reading the pages:

* `public`: `ontomark_compile::check::v3::state::is_public_under`
* `principal`, `verified_principals`: `Roster::resolve_principal` over
  `Actor::principal_did`
* `human_certified`: `OkfBlock::is_human_verified_under(&vocab, Some(&roster))`
  (under vocabulary 3 only a roster principal's own keys certify; `human:` never does)
* `recallable`: `check::v3::state::is_recallable`
* `generated_readable`, `verified_readable`: how many stamps `OkfBlock` parsed

The harness and the exact commands to regenerate `golden.json` are in `ORACLE.md`.

The four `VcKnowledge*` pages (a knowledge page carrying `visibility`, with and
without a disagreeing `public`) were added with the visibility fix. Since the v0.10.0 re-vendoring every entry was regenerated, so the `ontomark` field names that one commit.

Re-vendored at `79ffafb` (2026-11-25): three pages added (`SwarmVerified`: not certified; `OperatorVerified`: certified; `EmptyStamp`) and `ontology/attribution.yaml` gained a generator filed under the operator. `golden.json` was regenerated with the oracle against that checkout; no earlier entry changed.

Re-vendored at `v0.10.0` (`fa173a1`, 2026-11-25): three pages added (`knowledge/HumanPrefixVerified`, `working/JournalProposal`, `working/journals/2026-11-25`; 39 files, `UPSTREAM.sha256` regenerated, no earlier file changed). `golden.json` was regenerated with the oracle against that checkout. One earlier answer changed: `knowledge/VcVerifiedNpub`, vocabulary 3, `human_certified` true -> false (the `human:` spelling no longer certifies there), which VisionClaw already read; the `NOT_CERTIFIED_HERE` exception it needed is gone. A journal is keyed `working/<date>` in `golden.json`. The only remaining named difference is the fail-closed `working/VcConflictPublic` under vocabulary 3 (in the tests, not an oracle exception).

VisionClaw has no notion of resolving a `sources` target, so ontomark's journal-as-source-target rule has no counterpart: a journal is `journals/` furniture, and `crates/vault/tests/journal_source_target.rs` and `a_journal_is_furniture_and_a_source_citing_it_is_kept_not_resolved` pin what it does instead.
