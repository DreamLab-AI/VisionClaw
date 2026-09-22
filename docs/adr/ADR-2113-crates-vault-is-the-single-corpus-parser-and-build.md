---
id: ADR-2113
title: crates/vault is the single corpus parser and build
date: 2026-09-22
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 06dfe97a55e6a7a42bfc74a26a513108c60d5735
verified_paths: []
owner: jjohare
review_trigger: the crates are committed — bump verified_commit and arm verified_paths
repo: visionclaw
---

# ADR-2113 — `crates/vault` is the single corpus parser and build

## Context

The estate had four doors onto one corpus and they disagreed about its size:
raw disk 8,433 classes, agentbox's `ontology-bridge` 8,433, Loom's served
generation 8,146, VisionClaw's live ontology 4,167. The ontology itself lived in
two `json-ld` fences per page while the vault spec described frontmatter keys
that occur zero times. Four parsers, three reasoning paths, one Python pipeline
(4,322 lines) that nothing else could call, and a Rust `vault-migrate` that
shared no code with any of them. PRD-sovereign-corpus Q11 resolves this by
naming one implementation; contract C2 fixes its command surface, C3 the
artefacts it must emit, C4 the proposal shape.

## Decision

The corpus is parsed, validated, reasoned over and built by **one crate pair**,
in the VisionClaw workspace.

`vault-core` (library) owns the model: the frontmatter parser, the page identity
rule (vault-relative path without `.md`), the `ontology/vocabulary.yaml` model,
the OKF v0.2 types, the link graph with per-edge-type expansion, the promotion
state machine and the `PatchProposal`. VisionClaw's ingest links it, so
`CorpusSource::LocalDirectory` and `vault build` cannot disagree about a page.

`vault` (binary) owns the projections and the command surface: `validate`,
`find`/`retrieve`/`tree`, `edit --expect`, `propose`, `gate`, `conflicts`,
`build` and the one-shot `migrate`. Every subcommand takes `--json`.

Five rules the implementation enforces rather than documents:

1. **The published artefacts are byte-parity.** Loom loads
   `scaffold-index.json`; the explorer's physics worker memory-maps
   `data/graph/*.bin`. The migration is a format change, not a content change,
   and `tests/golden_parity.rs` proves it on 50 real pre-migration pages
   against the retired Python pipeline's own output: **13 of 14 artefacts
   byte-identical**, including all seven NGG1 binary tiers, `ontology.json`,
   `overview.json`, `stats.json` and `bridges.json` — and it still holds with
   the definition moved to the body's leading paragraph. `ontology.ttl` is compared
   as a triple set, because `rdflib`'s serialiser is not reproducible outside
   `rdflib`.

   Reproducing the binaries needed two non-obvious things: Python's Mersenne
   Twister (`build::ngg1::PyRandom` — the baked overview layout is seeded by
   `random.Random(42)`), and **no fused multiply-add** in the layout, because
   `a + b * c` rounds twice in CPython and once under FMA, which over 200
   iterations moved a coordinate across a `round(x, 3)` boundary.
2. **Two closures, deliberately.** `closure.rs` reproduces `reason.py`'s BFS
   because `sup`/`isup` encode its ordering; `whelk.rs` runs EL++ through the
   same Whelk revision VisionClaw's ontology service pins, and that is what
   `ontology-inferred.ttl` carries (contract C3).
3. **Redaction precedes the closure.** `projection.rs` ports the public build
   boundary verbatim: a non-boolean `public` refuses the build, a public/private
   identity collision refuses the build, and private *names* are redacted out of
   prose, not merely dropped from typed edges.
4. **A blocker is not an opinion.** Whelk inconsistency, `SUBCLASS_CYCLE`,
   `RELATION_CONTRADICTION` and vocabulary violations block `vault propose`
   before any 31402 is signed. No human is asked to wave one through.
5. **No hand-rolled cryptography.** The 31402's event id and BIP-340 signature
   come from `nostr-bbs-core`; `vault` assembles tags and content only.

`vault migrate --fences-to-properties` is lossless by construction: every fence
key and every Logseq property key must be covered by the vocabulary's
`migration:` map, and an uncovered key exits 2 with nothing written.

## Consequences

**Becomes true.** One class count. One parser to fix a bug in. `vault build`
emits every contract-C3 artefact into a staging tree that is promoted
atomically, so a failed build leaves the previous bundle intact and a successful
one leaves no obsolete page export behind. Agents reach the corpus through a
CLI with declared blast radii instead of an MCP server with none.

**Costs.** Three frontmatter keys exist purely to make the migration lossless —
`slug` (343 pages have a slug that is not `slugify(title)`), `resource` (96
classes have an IRI whose tail is not the page slug) and `links` (the curated
outbound set, which differs from a body scan on roughly half the corpus and is
what every backlink list is computed from). They are additive to contract C1 and
recorded there.

**Deliberate divergences from the Python output**, both recorded in the golden
test: `search-index.json`'s `labels` is now a superset (every alias, not just
`preferred-term`), and long-tail references are normalised out of the legacy
`urn:visionflow:linked:` / `urn:visionflow:owl:class:` namespaces onto
`urn:ngm:class:` with the slug preserved verbatim — which is why byte-parity
still holds, since every v1 artefact keys the tail on `ref_slug`.

**Forbidden.** Re-adding a second corpus parser; emitting `scaffold-index.json`
through anything but the CPython-compatible writer in `vault_core::json`;
publishing a proposal with a non-empty `blockers` list.

**Follow-on.** `vault migrate`, the `migrate` feature and `vault_core::fences`
are deleted after WS-D's run. `crates/vault-migrate` is deleted by WS-E.

**Vocabulary shape.** `ontology/vocabulary.yaml` has been authored in two
equivalent layouts — a flat `fence_fields` + `ignore` pair, and four per-fence
blocks. `MigrationMap::normalise` folds both into one internal map on load, so
a regeneration in either spelling cannot break the build, and
`is_populated()` refuses a read that caught the file mid-write.

## Verification

**What `verified_commit` attests here.** `06dfe97` is the commit the verification
ran *against*, not a commit containing this work: `crates/vault-core` and
`crates/vault` were still uncommitted when the evidence below was produced, as
the one-shot leaves every repo uncommitted for owner review (PRD §7).
`verified_paths` is therefore deliberately empty and the staleness gate is
**inert** — arming it against paths that are not yet in any commit would assert
something untrue. When the crates land, bump `verified_commit` to that commit
and set `verified_paths: [crates/vault-core, crates/vault, crates/vault/tests/golden]`
to arm it.

Established on the working tree of 2026-09-22, before commit:

```
cargo test  -p vault -p vault-core --all-features   # 432 total, 9 suites
cargo clippy -p vault -p vault-core --all-targets --all-features   # clean under -W clippy::pedantic
cargo fmt   -p vault -p vault-core --check                         # clean
```

### Second pass, 2026-09-22 (WS-C2): the corpus's own frontmatter, both vaults,
### the journals, credentials and the fence defect

376 → 411 tests, golden parity unchanged (15/15, byte-identical). Eight changes,
each measured against `visionGraph` on a scratch copy under `.tmp` — the live
pages were never written to, and `ontology/vocabulary.yaml` was not edited.

| change | measured effect |
|---|---|
| `migration.frontmatter_keys` — a page's own frontmatter becomes the third input category, converted through the existing `Destination` grammar | `vault validate --vault knowledge` **UNKNOWN_KEY 40 → 0**. `legacy_iri` (11) and `legacy_uri` (11) become `sources[]` provenance, `elevatedFrom` (7) becomes `sources[id=origin]`, `schema_version` (11) is dropped |
| `migrate` is set-if-absent for authored frontmatter | all **107** `working/` pages that declare `type: Episode` keep it; previously every one was rewritten to `Note`. `status`, `public` and `generated` likewise survive; lists union rather than replace |
| resolved type is constrained by the destination vault | `working/` can no longer receive `Class` from a legacy `OntologyClass` fence; the refused candidate is reported as `type_not_permitted`, not written. `knowledge/` accepts `Class`/`Property`/`Individual` only |
| `--vault all` runs **both** vaults, and `journals/` is reachable | **10,458** files examined where the old `--vault all` silently examined 8,457: `knowledge` 8,457 + **125** journals, `working` 771 + **1,105** journals. Per-vault counts are in `--report` and on stdout |
| `UNPUBLISHED_DIRS` moved out of enumeration into publish scope; every skip reported | `_misc/` pages are loaded, validated and migrated, and still never published. The **28** `pages/.deleted/` files are reported as skips with a reason — matching `migration.excluded_paths` exactly |
| a `.deleted/` tombstone is no longer treated as a held page | the build had refused the whole bundle over `public/private identity collision on recurrent-neural-network`: the live public page and its own tombstone slugify alike. Deletion is not a publication hold |
| `SECRET_DETECTED` (`vault_core::secrets`) | one **genuine** live `sk-proj-…` OpenAI key found at `working/pages/2026 links.md:1854`. Reported as `file:line` + credential *type*, never the value; `$ENV_VAR` never matches. `vault build` exits **2** and writes nothing if a page selected for publication matches |
| `vault repair fences` | **462** pages repaired (461 stray markers removed, 1 closer inserted) and **52 reported as ambiguous and left untouched**; a second run reports `0 repaired`. `UNTERMINATED_FENCE` falls to the 52 ambiguous pages, not to 0 — see the correction below |

After `migrate --vault all` then `repair fences --vault all` on the scratch copy,
`vault validate --vault knowledge` reports **0 errors** (from 40 `UNKNOWN_KEY` +
9 `LOGSEQ_PROPERTY`), and `working` reports exactly **1** — the credential above,
which is a corpus fact, not a tool defect.

**One blocker, and it needs the owner.** `--vault all` now reaches the journals,
and the journals carry five `key::` spellings that `migration.logseq_keys` does
not cover, so the migration **refuses with exit 2 and writes nothing** — which is
the losslessness contract working as designed, not a bug. All five are Logseq
editor/asset state and want `drop`:

```yaml
  logseq_keys:
    file-path:     drop      # `../assets/…` link; the body already carries it
    hl-color:      drop      # PDF highlight colour
    hl-page:       drop      # PDF highlight page
    ls-type:       drop      # Logseq annotation type
    time-tracked:  drop      # Logseq time-tracking state
```

`file-path` is already mapped under `migration.frontmatter_keys`; in the journals
it occurs as a `key::` line, which is a different category and needs its own
entry. The figures above were obtained with those five lines added to a **scratch
copy** of the vocabulary; the governed file is unchanged and awaits the owner.

Separately, **125** `knowledge/journals/` pages are reported `type_not_permitted`
(`Journal` is a `working_types` member, not a governed type). Reported, not
refused — it is a content decision about where those pages belong.

### Third pass, same day: three corrections from WS-D and ws-d2 review

Review found one real defect in the above and two places where "report it" was
the wrong severity. All three are fixed, and the figures in the table are
corrected here rather than edited above, so the record shows what changed.

**1. `migrate` was not idempotent, and that made it dangerous.** WS-D's dry-run
on the already-migrated vault showed all 8,457 knowledge pages having `type`
removed and re-added. Two independent causes, both fixed:

* *The body separator.* Composition used `body.trim_end()` while always writing a
  `\n` separator, so a body read back off a migrated page kept the blank line the
  previous run produced and gained another. One blank line per run, forever.
  `trim` instead of `trim_end`; `leading_paragraph` already skips leading blanks,
  so no artefact moves.
* *The key order.* Output order followed whichever input supplied each key — the
  fence on a first pass, the authored block on a second — so identical content
  serialised differently. The key order is now seeded from the page's own
  frontmatter before anything is written.

* *Which input supplied the key.* Seeding the order from the page's own
  frontmatter fixed the common case and left 107 `working/` pages reordering
  `sources:` forever, because `source: sources[]` is a *converted* key and so was
  deliberately not seeded. Order is now **canonical** —
  `Frontmatter::sort_canonical`, `CANONICAL_ORDER`, identity → trust →
  provenance → relations → extensions, unknown keys alphabetically after — which
  makes serialisation independent of the input rather than merely usually stable.
  It also makes the corpus diff-stable in git.

Measured on a copy of the live corpus, running the full sequence (`--only pages`,
`--only journals`, `repair fences`) and then re-running all of it: **0 of 10,458
files differ**, and `repair` reports `0 repaired`. All 8,446 `type: Class`,
295 `Episode`, 487 `Note` and 1,105 `Journal` survive, and both vaults validate
with **0 errors**. The
catastrophe WS-D projected (8,457 Classes → `Note`) was already prevented by the
set-if-absent change, which was in the tree but not in their binary; the
idempotency defect was real and separate.

**2. A type fallback now REFUSES (exit 2, nothing written).** Previously reported
and proceeded, on the argument that nothing is *lost* by writing `Note`. The
argument was wrong about the stakes: `type` is inferred from a fence and `migrate`
removes the fences, so a fallback is the tool failing to establish what a page
*is*, and doing it silently at exit 0 to 8,457 governed pages is not a report, it
is data loss. `--allow-type-fallback` is the documented escape for the one case
that is a content decision — `knowledge/journals/`'s 125 `Journal` pages.

**3. `vault repair fences` wrote an empty code block on 34 pages.** Found by
ws-d2. `classify` tested the *entire remainder* of the file for code-looking
lines and then inserted the closer at the first blank line. On an orphaned
**closer** — a marker whose code sits above it, left behind when `migrate`
consumed the opening `json-ld` fence — the first blank line is the very next
line, so a code-looking line anywhere later in the file produced ``` ``` ``` with
nothing between. It validated clean while adding a defect to the source the
repair exists to remove.

The region a closer would enclose is now computed *before* asking whether it
holds code, which makes "never emit an empty pair" structural. And a third
outcome was added: when the marker has nothing below it, code immediately above,
and the file has other markers, the marker **closes the block above it** and the
missing one is earlier — so it is reported as ambiguous and left alone. ws-d2
predicted 5 such pages from a fence-count histogram; the correct discriminator is
what surrounds the marker, and there are **52**, including a 15-marker page whose
last marker closes a mermaid diagram. Removing it, as a marker-count rule would
have, would have unclosed the diagram.

Verified on a copy of the live corpus: **0 files gained an adjacent empty fence
pair**; the 5 that have one carry it at identical line numbers before and after.

Also added, unblocking ws-d2's steps 5 and 6: `migrate --only pages|journals|all`
(so the journals can take a first pass without touching `pages/`) and
`build --publish-out <dir>` (promote the markdown separately from the C3 bundle;
the credential gate applies either way). The credential check earned itself on
day one — ws-d2 confirms it caught the live `sk-proj-` key that a manual
redaction pass had declared clean.

### Fourth pass: the `migrate` "silent no-op", and what `whelk::reason` actually is

**The reported "`migrate` without `--dry-run` writes nothing" is not a write-path
defect.** Reproduced and traced: the live corpus was *already migrated*, so a
run correctly had nothing to write, and the summary reported `pages_converted:
8,446` — a count of pages **examined and converted in memory**, not pages
written. A correct no-op therefore read as a broken write.

Proven on a live copy, both directions:

* A page with a pending rewrite (`legacy_uri` re-introduced into a real corpus
  page) — dry-run reports `1 changed, 8,445 unchanged`; the real run changes that
  page's sha256 and mtime, converts `legacy_uri` into `sources[id=legacy-uri]`,
  drops `schema_version`, keeps `type: Class`; and **exactly 8,445 of 8,446 files
  keep their previous mtime**.
* A fully-migrated corpus — `0 changed, 8,446 unchanged, 0 written`.

Fixed in the reporting and the write, since the confusion was avoidable:

* `pages_changed` and `pages_unchanged` are now separate counters, and the write
  set is `pages_changed`. A page whose conversion is byte-identical is **not
  rewritten**, so mtime stays a usable signal for `git` and file watchers.
* The CLI prints its verdict **first**. `REFUSED — nothing was written` or
  `NO-OP — … 0 written. The corpus is already migrated.` A refusal used to print
  *below* a summary that opened with "8,446 converted".

Two regression tests cover it: a pending rewrite changes the file and counts as
changed, with the second run a zero-**write**; and an unchanged page keeps its
modification time.

### `whelk::reason`: FOUND AND FIXED — the existential restrictions

**Full-corpus `vault build`: does-not-finish → 11 s. `whelk::reason` on 9,135
classes: 570 ms. `vault propose`: >180 s timeout → 3 s.** The target was two
minutes.

**A correction first.** An earlier pass of this section claimed to have disproved
transitivity, restrictions and object properties by ablation. Those ablations
were **ineffective and the conclusions are retracted**: they edited
`characteristics:`, `restriction:` and `emitted:` in a scratch `vocabulary.yaml`,
and the restriction emitter in `build::turtle` **hard-codes**
`for json_key in ["requires", "hasPart"]` and never consults the vocabulary at
all. Phase tracing proved it — the "ablated" run still reported 6,138
existentials, identical to the unmodified corpus. That hard-coding is itself a
defect (the vocabulary documents `restriction: true` as the control and the
emitter ignores it) and is recorded for WS-B. Only the cycle disproof stood, and
it has since been re-run under control.

**The measurement that found it.** Timing the three phases inside `reason`
(`VAULT_WHELK_TRACE=1`, now permanent) at 4,000 pages / 5,215 classes:

```
whelk: 5215 classes, 6445 subClassOf, 6138 existentials, 15 properties
whelk: ontology built in       14µs
whelk: translate_ontology    27ms  -> 17,804 axioms
whelk: reasoner::assert    5,181ms          <- 96.5% of the run
whelk: named_subsumptions     9ms  -> 31,667 pairs
```

Then the same corpus with the existentials withheld from the reasoner:

| | `reasoner::assert` | inferred pairs |
|---|---|---|
| with 6,138 existentials | 5,181 ms | 9,575 |
| without | **122 ms** | **9,575** |

**42× the runtime for zero additional entailments.** On the full corpus the same
change takes `assert` to 348 ms and the whole build to 11 s.

**Why withholding them is sound, not a shortcut.** `whelk::Restrictions::Skip`
documents the argument: the emitter can only produce an existential in the
**superclass** position (`C ⊑ ∃R.D` — a named subject whose `rdfs:subClassOf`
object is a blank restriction node). In EL++ such an axiom contributes to a
**named** subsumption only if some axiom places a restriction in the *subclass*
position (`∃R.D ⊑ B`) or asserts an equivalence involving one, and this crate
emits neither. So they cannot affect the closure, and Whelk saturates over them
for nothing. The restrictions remain in `data/ontology.ttl` — they are asserted
OWL that contract C3 publishes.

`restrictions_change_no_named_subsumption` reasons a fixture that *does* carry
superclass-position restrictions both ways and asserts identical `inferred`,
`unsatisfiable` and `classified`. If the emitter ever gains a subclass-position
restriction or an equivalence, that test fails and `Restrictions::Include`
becomes correct — which is the point of keeping the switch rather than deleting
the code path.

**Disproved, under control:** the 27 `SUBCLASS_CYCLE`s. A cycle-pruned copy at
5,500 pages (verified 0 cycles by `conflicts`) reasons in 104 s against the
baseline's 112 s. The earlier "test" compared two runs that both hit a timeout
cap and proved nothing. `propose` on a cycle-pruned full corpus also still timed
out at 180 s. **`whelk-rs` is not at fault either** — the same pinned revision
`79a1ee2`, driven identically by `visionclaw-adapters`, classifies a synthetic
6,000-class corpus with 69,900 entailments in 412 ms. The input was wrong, not
the reasoner.

**`vault propose`** shares the fix and now returns a signed 31402 in 3 s. It
already ran the cheap `conflicts::analyse` before the reasoner, so ordering was
never the problem; the signing key is now resolved *first*, before the corpus
load, so a batch with no key fails instantly instead of after full reasoning.
`visionclaw-adapters` caches its subsumptions (`whelk_inference_engine.rs:405`);
at 570 ms per run, `vault` no longer needs to.

Delivered: `crates/vault/tests/whelk_scaling.rs` — a CI guard asserting 2,000
classes reason in under 5 s (151 ms, over a fixture that entails 23,300 pairs, so
it cannot pass vacuously), plus `#[ignore]`d diagnostics for the scaling curve,
the multi-parent density grid and a real-repository phase breakdown.

### Three silent-success defects, and the sweep for more

`--vault` was a free-form `String` on every subcommand, so `--vault bogus` was
accepted and silently ran `knowledge`. It is now a strict clap `value_parser` over
`[knowledge, working, all]` on `validate`, `find`, `retrieve`, `tree`, `build`,
`migrate` and `repair`; an unknown value exits **2**. Two tests cover the
rejection and the acceptance of all three values on every subcommand, and
`--only` is likewise closed over `[pages, journals, all]`.

`vault build` now reports `written: N` and **fails if it wrote nothing** —
"nothing built" is not a success. The count is asserted equal to the number of
files actually on disk. The three silent-success defects found today were all the
same shape: a value or a state the tool did not understand, treated as a default,
reported as success. The others are now loud — `migrate` prints `REFUSED` or
`NO-OP … 0 written` before its summary, and `repair` lists the 52 pages it
declined to touch.

### A7: `valid_domains` comes from the census, not a hard-coded set

`INVALID_DOMAIN` **84 → 0**. `scalars.domain` is deliberately free text — folding
`ai` (513 pages) into `artificial-intelligence` is a governance decision, not a
schema one — with six `roots:` and a 17-value `observed:` census. Validating
against a closed set warned about `economics`, a value the owner has recorded a
count for. `ScalarDef::known_values` is now `roots ∪ observed` for a text key, so
the warning fires only for a value the census never saw.

`observed` is typed as `serde_yaml::Value`, not `value: count`: its shape depends
on the key, and on a number key like `quality` it is a statistics block
(`min: 0.35`). Typing it narrowly broke vocabulary loading outright — 14 tests —
which is why `known_values` reads it only for `ScalarType::Text`.

### SUPERSEDED — the earlier record of the reasoner blow-up

Kept because the numbers are the before-measurement for the fix above, and
because the reasoning in it was wrong in an instructive way.

Clearing the validation errors made `vault build` reach the reasoner on the whole
corpus for the first time, and it did not finish. Timed on subsets of
`knowledge/pages`:

| pages | `vault build` / `gate --tier full` | `gate --tier quick` (no reasoner) |
|---|---|---|
| 2,000 | 2 s | — |
| 4,000 | 9 s | — |
| 5,000 | 50 s | — |
| 6,000 | 285 s | **1 s** |
| 8,446 | > 27 min, stopped | — |

The conclusion drawn at the time — "quadratic or worse in class count", "the
defect is in the reasoner, not in this crate" — was **wrong**. It was neither
quadratic in class count nor a reasoner defect: it was ~6,100 existential
restrictions that the reasoner saturated over and that entailed nothing, and the
same reasoner handles 6,000 classes with 69,900 entailments in 412 ms. The error
was inferring a cause from a *shape* (superlinear curve, small output) instead of
measuring which phase consumed the time. One `Instant::now()` around
`reasoner::assert` would have answered it immediately, and now does
(`VAULT_WHELK_TRACE=1`).

Full-corpus `vault build` is now **11 s** and contract C3's bundle is producible.

The parity claim specifically: `tests/golden/fixture` holds 50 unmodified
pre-migration pages; `tests/golden/python` holds what
`python -m pipeline.build` (rdflib 7.6.0) produced from them.
`scaffold_index_is_byte_identical_to_the_python_pipeline` and
`prose_index_is_byte_identical_to_the_python_pipeline` compare the bytes after
blanking the `generated` timestamp; `the_asserted_turtle_has_the_same_triples`
compares 2,245 ground triples and 48 restriction triples with zero asymmetry.

`implementation_status: partial` because the RVDB step (`--with-rvdb`) has been
exercised against a stub embedder in unit tests but not against the live
Xinference endpoint, and the 31402 publish path has been exercised through
`--dry-run` (signed-event construction and verification) but not against the
local relay — that is WS-F's end-to-end test.
