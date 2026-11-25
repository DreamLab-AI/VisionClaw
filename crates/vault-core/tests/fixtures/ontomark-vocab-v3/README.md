# ontomark vocabulary 3 fixture, shared with VisionClaw

Upstream: `crates/ontomark-cli/tests/vocab-v3-vault` of DreamLab-AI/ontomark at
`1c1ec51e846f6b00d7dcb15f9ae0271c24464f6c` (branch `feat/vocabulary-v3`, not yet
released). The 33 upstream files are vendored byte-for-byte; `UPSTREAM.sha256`
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
* `human_certified`: `OkfBlock::is_human_verified_with(Some(&roster))`
* `recallable`: `check::v3::state::is_recallable`
* `generated_readable`, `verified_readable`: how many stamps `OkfBlock` parsed

The harness and the exact commands to regenerate `golden.json` are in `ORACLE.md`.

The four `VcKnowledge*` pages (a knowledge page carrying `visibility`, with and
without a disagreeing `public`) were added with the visibility fix; their
entries in `golden.json` were produced by the oracle at ontomark `1bbf86b`,
which also reproduced every earlier entry unchanged (the `ontomark` field above
still names the commit the original entries came from).
