# `vault` — the single door onto the sovereign corpus

One binary replaces the `visionGraph/pipeline` Python (4,322 lines), the
`ontology-bridge` and `ontology-propose` MCP servers, and `vault-migrate`.
Agents reach the corpus through this CLI and nothing else; humans use Obsidian.

See [ADR-2113](../../docs/adr/ADR-2113-crates-vault-is-the-single-corpus-parser-and-build.md)
for the decision and [PRD-sovereign-corpus](../../../VisionFlow/docs/PRD-sovereign-corpus.md)
for why.

## Install

```bash
cargo build --release -p vault     # target/release/vault
```

`vault` finds its repository by walking up from the working directory until it
sees `ontology/vocabulary.yaml`. Pass `--repo <dir>` to be explicit.

## Commands

Every subcommand accepts `--json`. Exit codes are contractual: **0** success,
**1** a failed check, **2** a migration the vocabulary does not cover.

### `vault validate`

OKF v0.2 conformance, `vocabulary.yaml` agreement, link integrity, the `public`
gate, and the three migration-residue checks (no `json-ld` fence, no Logseq
`key:: value` line, no `{{embed}}`).

```bash
vault validate                       # knowledge/
vault validate --vault all --strict  # both vaults; warnings fail too
vault validate --json | jq '.knowledge.by_code'
```

An **error** blocks `vault build`; a **warning** does not. `MULTI_PARENT` is
`info`, not a warning — 957 classes carry more than one parent by design.

`SECRET_DETECTED` is an **error**. The corpus is published to the open web, and
a key that reaches a commit is compromised whether or not the page is served.
The finding names the file, the line and the credential **type** and never the
value — a validation report is itself a published artefact:

```
pages/2026 links.md:1854 carries what looks like a openai_key credential;
the value is deliberately not reported
```

`$ENV_VAR` never matches. `api_key=$OPENAI_API_KEY` is the documented way to
write a credential in a page, and a check that flagged it would be ignored.

Both vaults' `journals/` trees are validated too — `total_journals` in the
report — and every markdown file the walk declined to load is listed in
`skipped` with its reason. A count of zero now means there are none, not that
nobody looked.

### What is enumerated, and what is published

These are two different questions and `vault` now keeps them apart.

**Enumerated** — loaded by the vault, and therefore reachable by `validate`,
`find`, `retrieve` and `migrate` — is every `.md` file under `pages/` and
`journals/` except those in a dot-directory. Each skip is reported with a reason,
so an absent page can never be confused with a page that does not exist:

```bash
vault validate --vault all --json | jq '.knowledge.skipped'
# [{"path": "pages/.deleted/AI-0376-algorithmic-accountability.md",
#   "reason": "hidden_directory"}, …]   # 28 of them
```

**Published** is narrower: it excludes `_misc/` (`UNPUBLISHED_DIRS`) and every
page without `public: true`. `_misc` used to be applied during *enumeration*,
which quietly turned a publication decision into an existence decision — a
`_misc` page could not be validated, found or migrated. It now lives only in the
projection's publish scope.

A `.deleted/` or `.trash/` page is a **tombstone**, not a held page, and its
identity is not reserved. Treating it as held meant the live
`Recurrent Neural Network.md` could not publish because its own tombstone
slugified to the same thing, and the build refused the whole bundle. Deletion is
not a publication hold.

### `vault find` / `retrieve` / `tree`

```bash
vault find --query "knowledge graph" --type Class --limit 5
vault find --query "knowlege grph" --fuzzy

# Per-edge-type depths. An edge type absent from --expand is never traversed,
# so the blast radius is always declared.
vault retrieve "Knowledge Graph" --expand is-a=2,requires=1 --max-documents 40

vault tree "Spatial Computing" --depth 3 --predicate is-a
```

Scoring is deterministic, not a ranking model: exact title `1.0`, exact alias
`0.95`, prefix `0.8`, substring `0.6`, token overlap (with `--fuzzy`) up to
`0.5`.

### `vault edit --expect`

Guarded mutation. **An edit that has not declared its blast radius is refused**,
and the refusal names the missing guard.

```bash
vault edit "Knowledge Graph" \
  --set status=stable \
  --set 'verified+={by: human:npub1abc, at: 2026-09-22T00:00:00Z}' \
  --expect docs=1,blocks=2
```

| form | meaning |
|---|---|
| `--set key=value` | set or replace |
| `--set 'key+=value'` | append to a list, creating it if absent |
| `--unset key` | remove |

A value that parses as YAML is stored as YAML, so `quality=0.35` is a number
and a mapping round-trips as a mapping rather than a string:

```bash
vault edit "Some Page" \
  --set 'generated={by: process:vault-migrate/1.0, at: 2026-09-22T11:37:06Z}' \
  --expect docs=1,blocks=1
```

`--dry-run` shows the result without writing it.

### `vault propose`

Builds a contract-C4 `PatchProposal`, runs Whelk and the conflict detector as
**blockers**, and posts a forum 31402 `ActionRequest`.

```bash
# Propose a content change: --diff takes the proposed page in full.
vault propose "Knowledge Graph" --level content \
  --hypothesis "quality is understated" --diff /tmp/proposed.md --dry-run

# A demotion needs no diff file.
vault propose urn:ngm:class:knowledge-graph --level demotion \
  --hypothesis "superseded by Knowledge Base" \
  --secret "$VAULT_NOSTR_SECRET" --relay ws://localhost:7777
```

**A grouped proposal.** Point `--diff` at a *directory* and every `*.md` in it
is a proposed page, named by page id; the result is one `PatchProposal` whose
`diff` is a multi-file unified diff and whose `pages` names the set:

```bash
# Fold `domain: ai` into `artificial-intelligence` across 513 pages — ONE case.
vault propose "Artificial Intelligence" --level schema \
  --hypothesis "ai and artificial-intelligence are the same domain" \
  --diff proposals/ai-merge/ --dry-run
#   grouped proposal: 513 pages, subject `Artificial Intelligence`
```

Contract C4 gains `pages: [...]`. A single-page proposal holds exactly its
subject and its digest is unchanged from before grouping existed, so no case id
moved; a grouped one folds the page set into the digest, because two different
decisions must not share a case. The 31402 carries a `pages` tag with the count —
a reviewer scanning a relay sees tags before content, and "this changes 513
pages" is the fact that decides whether to open it.

Blockers are evaluated for **every** page in the group, and one blocked page
blocks the whole proposal: it is one decision, and signing 512 good changes to
carry one bad one through is what the gate exists to prevent. A manifest entry
identical to the corpus is dropped as a no-op; one naming a page the vault does
not hold refuses the proposal rather than being skipped, because a typo in one of
513 entries would otherwise be invisible.

**A creation.** An elevation — a `working/` note promoted to a new knowledge
Class — is a page that does not exist yet. When the subject names no page by id
or title and the `--diff` file declares a `title` no page has, the proposal is
`kind: "create"` (every other proposal is `kind: "amend"`): its diff is against
`/dev/null`, its `iri` is the staged page's `resource`, and it is blocked by

* validation errors on the staged page (it is assessed inside the real corpus,
  so a dangling relation target is judged against the pages that exist),
* `IRI_COLLISION` — the `resource` is an existing page's,
* `SLUG_COLLISION` — the publish slug is taken, or adding the page would make
  the build re-key an existing page's slug,
* `FILENAME_COLLISION` / `FILENAME_INVALID` — the title differs only in case
  from an existing page, or cannot be a flat filename,
* any conflict or unsatisfiable class the page introduces (a delta, as ever).

The level is `content` unless the page declares a schema-level key (one the
vocabulary does not declare, or a provisional relation), which makes it
`schema`. An amendment's digest is unchanged by `kind`; a creation's folds
`kind:create` in, so the two never share a case. In a manifest, a file naming
an absent page is a creation only when it declares `title: <its id>`.

```bash
vault propose urn:ngm:class:agentic-workshop --diff staged/agentic-workshop.md \
  --hypothesis "elevated from working/Agentic Workshop" --dry-run
```

`--dry-run` prints the signed event without publishing. A proposal with a
non-empty `blockers` list is emitted (so the refusal is auditable) and exits 1
**without** being posted. A `--level schema` proposal declares
`Stakes::Critical`, which floors the panel's risk tier at High.

### `vault create`

The apply side of a creation, once a human has promoted the case:

```bash
vault --repo . create staged/agentic-workshop.md --expect docs=1 \
  --set status=stable --set 'verified+={by: human:npub1…, at: 2026-09-22T20:00:00Z}' --json
```

Writes `knowledge/pages/<title>.md` — the staged page with the `--set`/`--unset`
keys applied, in one write — **only if it is absent**. Every refusal writes
nothing and exits **2**, with `{created: false, code, message, blockers}` on
stdout under `--json`: `--expect` without `docs=1` (or a wrong `blocks=N`), an
unparseable or untitled staged page, a page that already exists (checked against
the vault *and* by an exclusive create on disk, so a second call never
overwrites the first), and a result that fails validation or the collision
checks `vault propose` applied.

### `vault gate` / `vault conflicts`

```bash
vault gate --tier quick    # validate only, in-process
vault gate --tier full     # + conflicts + Whelk consistency
vault conflicts --severity high --json
```

The gate's verdict always restates the caveat: *a passed gate proves graph
consistency only, not enrichment correctness*. A skipped check is reported as
`skip`, never silently treated as a pass.

Conflict kinds: `DUPLICATE_CONCEPT` and `SUBCLASS_CYCLE` (high),
`RELATION_CONTRADICTION` and `TYPE_CONFLICT` (medium).

### `vault build`

```bash
vault build --out www --stats
vault build --out www --with-rvdb        # also embeds the corpus
```

Emits the contract-C3 bundle:

```
<out>/data/ontology.ttl                      asserted OWL 2 EL
<out>/data/ontology-inferred.ttl             Whelk EL++ closure
<out>/data/scaffold-index.json               v1 — byte-parity with the retired pipeline
<out>/data/prose-index.json                  v1 — byte-parity
<out>/data/ontology.json                     WebVOWL graph — byte-parity
<out>/data/graph/overview.json               NGG1 T0: 6 domains + 34 categories
<out>/data/graph/full.bin                    NGG1 uncapped whole graph (CSR)
<out>/data/graph/domain-<slug>.bin ×6        NGG1 T1, relations capped top-8
<out>/data/graph/stats.json, bridges.json
<out>/data/ontology-corpus.records.jsonl     --with-rvdb only: portable records, vectors inline.
                                             Loom's `promote_vault_build` turns them into its serving
                                             `ontology-corpus.rvdb` (a ruvector-core database) and sidecar.
<out>/api/search-index.json
<out>/api/pages/<slug>.json, _domain-index.json
<out>/api/census.json, validation-report.json
<out>/api/markdown/                          --with-markdown-mirror only
<out>/context/v1.jsonld, <out>/api/schema/context.jsonld  and  <out>/ns/v2.jsonld
                                             (the same document; /ns/v2.jsonld
                                             is the served path, and the property
                                             IRIs inside still cite ns/v1#)
<out>/okf/index.md, concepts/<slug>.md
<out>/publish/pages/<title>.md               public knowledge pages (Quartz slug `pages/<title>`)
<out>/publish/working/<subdir>/<title>.md    public working pages, subdirectories kept
<out>/publish/index.md                       the site's home page: OKF §8 extent
<out>/.generation.json                       visionGraph@<sha> + content digest
```

The `publish/` tree (or the `--publish-out` directory, which replaces it) is laid
out as the **published site's URL contract**, so Quartz builds from it with no
re-staging: `pages/**` keeps every knowledge URL the site has always had,
`working/**` keeps the working vault's subdirectories (the two vaults stay in
separate namespaces — hundreds of filenames collide between them), and
`index.md` at the root is the home page. A page under `_misc/` or `misc/` at
any depth is held back whatever its flag says: that is the owner's scratch-tray
decision, the site's `**/misc/**` ignore pattern enforces it too, and staging a
page Quartz then ignores would fail the site's staged-equals-built completeness
check.

`publish/` is the only artefact that spans **both** vaults. Every other one is a
projection of `knowledge/`'s ontology types: OWL, the scaffold and prose
indexes, the search index and the graph tiers. Publication is a per-page
decision an author makes with the `public` flag, so a curator who marks a
`working/` note public gets it published; OWL membership is not a per-page
decision, so that note never reaches `ontology.ttl`. `--vault all` widens the
staging; `--vault working` builds the working vault's own bundle.

```bash
vault build --out www --vault all --stats
#   visionGraph@<sha> -> www
#     published knowledge: 1496 of 1500 page(s)
#     published working: 55 of 150 page(s)

# Promote the markdown separately from the C3 bundle. The credential gate
# applies either way.
vault build --out www --vault all --publish-out published
```

`journals/` is never published: a daily note is working material and nothing in
the estate consumes it. It is still validated and still scanned for credentials.

**The credential gate is the last thing before bytes leave the machine.** One
match on any page selected for publication and the build exits **2**, writes
nothing and leaves the previous bundle intact. A credential on a *private* page
is reported by `vault validate` and does not refuse the build: refusing would
make the corpus unbuildable over a page nobody can read.

The `data/graph/*.bin` tiers are the **frozen NGG1 wire format** the explorer's
physics worker memory-maps straight off the published site (`FORMAT-NGG1.md`):
24-byte node stride, CSR adjacency, a string table pairing label and IRI per
node. The golden test asserts them byte for byte.

`--with-markdown-mirror` is off by default: the title-form markdown mirror has
no consumer, weighs 124 MB, and the Pages budget is 1 GB (decided 2026-09-22).

The whole tree is staged and promoted atomically: a failed build leaves the
previous bundle intact, a successful one leaves no obsolete page export behind.

The full corpus builds in **11 s** (8,432 classes, 331,045 asserted triples,
83,160 inferred, 25,528 artefacts). `whelk::reason` is 570 ms of that.

It was not: the existential restrictions the graph carries (`∃requires.C`,
`∃hasPart.C`) were being handed to the reasoner, which saturated over them at 42×
the cost and derived **nothing** from them — `reasoner::assert` 5,181 ms against
122 ms for a byte-identical closure at 5,215 classes. They can only appear in the
*superclass* position, and an EL++ axiom of that form cannot produce a named
subsumption unless some axiom puts a restriction in the *subclass* position or
asserts an equivalence; this crate emits neither. So `whelk::reason` withholds
them (`Restrictions::Skip`), and `restrictions_change_no_named_subsumption`
reasons a restriction-carrying fixture both ways to prove the closure is
unchanged. They remain in `data/ontology.ttl`, which is asserted OWL that
contract C3 publishes.

`VAULT_WHELK_TRACE=1` prints the per-phase breakdown. `tests/whelk_scaling.rs`
holds a CI guard (2,000 classes under 5 s; currently 151 ms over a fixture that
entails 23,300 pairs, so it cannot pass vacuously) and three `#[ignore]`d
diagnostics. See ADR-2113 § Verification.
A validation **error** refuses the build and writes nothing.

`--with-rvdb` embeds via Xinference (`bge-small-en-v1.5`, 384 dimensions,
`--embed-endpoint` or `VAULT_EMBED_ENDPOINT`). The embedder is locked: a vector
of the wrong width is an error, not a quietly wrong answer. Loom's
`build-concept-records` / `stage_corpus` remain the canonical Postgres write
path; the `.rvdb` file is the portable form.

### `vault repair fences`

457 `knowledge/` pages and 46 `working/` ones carry a triple-backtick opener
that is never closed, and the journals carry more. `vault_core::code`
deliberately reads the region after such an opener as **prose** — treating it as
code would swallow the 1,713 real `key:: value` lines that fall after one — which
keeps the migration lossless without making the page correct. An unclosed fence
still renders the rest of the page as a code block everywhere downstream.

```bash
vault repair fences --vault all --dry-run --report repair.json
vault repair fences --vault all
#   10458 page(s) examined, 462 repaired: 461 spurious opener(s) removed,
#   1 closing fence(s) inserted
#   52 page(s) NOT repaired — the unmatched marker closes the code above it …
```

`vault_core::code` pairs markers **sequentially**, so the leftover on an
odd-count file is always the *last* one. That is where the imbalance surfaces,
not necessarily where it is: a file whose first marker is the real orphan pairs
2-3 and 4-5 happily and still reports marker 5. So the rule reads both sides.

| what surrounds the marker | disposition |
|---|---|
| code **below** it | a genuine unclosed opener → a **closing fence** is inserted at the next paragraph boundary |
| nothing to enclose below, and unambiguous (the file's only marker, or prose both sides) | a stray marker → **removed** |
| nothing below, code **above**, other markers in the file | **ambiguous: reported, nothing changed** |

The third row is load-bearing. Such a marker *closes* the block above it and the
missing one is earlier in the file, so removing it would unclose a legitimate
block — on one page, a mermaid diagram. 52 pages are in this state; they are
listed in `ambiguous` and still fail `UNTERMINATED_FENCE`, deliberately, because
the defect is real and only a human can place the missing marker.

A closing fence is therefore only ever inserted around at least one code-looking
line, which makes "never emit an empty ``` ``` pair" structural rather than a
check. An earlier rule tested the *whole remainder* for code and then inserted
the closer at the first blank line, which on an orphaned closer is the very next
line: that put an empty code block on 34 knowledge pages — validating clean while
leaving a fresh defect in the source the repair exists to remove.

"Code-looking" is a **content** test, never indentation: the corpus was migrated
from a Logseq outliner export and nearly every prose line is still an indented
bullet, so
"indented therefore code" would fence the whole vault. What actually follows a
real stray opener here is OWL functional syntax, Turtle and the occasional
SPARQL query.

Every change is listed in `--report`, and the command is idempotent — a repaired
page has no unmatched marker, so a second run reports `0 repaired` (an ambiguous
page is reported on every run until it is resolved). This is not part of
`vault migrate`: `migrate` is deleted after its one run, and this defect recurs
every time somebody pastes functional syntax into a page.

### `vault migrate` (one-shot)

```bash
vault migrate --fences-to-properties --dry-run --report migration-report.json
vault migrate --fences-to-properties
```

Folds both `json-ld` fences into typed Obsidian Properties, converts the
surviving Logseq `key:: value` lines, rewrites `{{embed [[X]]}}` to `![[X]]`,
and stamps `type`, `resource`, `status: stable` and `generated`.

Pages with **no** fence are converted too — the same machinery minus the fence
step. `working/`'s 574 pages and the 189 fence-less knowledge pages carry
`key::` lines and embeds that would otherwise survive untouched.

`--vault all` runs **both** vaults and both `journals/` trees — 10,458 files,
where it previously ran `knowledge/pages` alone and said nothing about it. The
order is not an implementation detail: every tree is *indexed* before any tree is
*converted*, because `logseq_keys.id: drop` deletes the very `id::` lines
`migration.block_refs` resolves against, and converting `knowledge/` first would
make all 33 of its block references unresolvable.

```bash
vault migrate --vault all --fences-to-properties --dry-run --report m.json
#   10458 pages examined, 10458 converted, …
#     knowledge: 8457 page(s) + 125 journal(s) examined, 8582 converted
#     working: 771 page(s) + 1105 journal(s) examined, 1876 converted
#     28 file(s) skipped: knowledge/pages/.deleted/… (hidden directory)
```

Three input categories, not two. Beside the fences and the `key::` lines, a
page's **own frontmatter** is converted through `migration.frontmatter_keys`,
using the same `Destination` grammar. That is what turns `legacy_iri` and
`legacy_uri` into `sources[]` provenance and drops `schema_version` — 40
`UNKNOWN_KEY` errors before, none after.

A key the vocabulary does **not** name is *authored*: it survives, and it wins
over anything the fences supply. A human wrote it and a fence did not. This is
why an authored `type: Episode` is still `Episode` after a run — 107 `working/`
pages that an earlier run rewrote to `Note`.

The resolved `type` is also constrained by the destination vault: `knowledge/`
accepts `Class`, `Property` and `Individual`; `working/` accepts its
`working_types`. A candidate the vault refuses is reported as
`type_not_permitted` and **not written**, so a legacy `OntologyClass` fence can
no longer put a `Class` into `working/`.

**A type fallback refuses the run** — exit 2, nothing written. A page whose type
falls back to `Note` has not been converted; the tool has failed to establish
what it *is*. That matters most on a re-run: `type: Class` came from a fence, and
`migrate` removes the fences, so without the authored value winning every one of
the 8,457 governed pages would fall back and the ontology would be erased at exit
0. `--allow-type-fallback` proceeds anyway, for the one case that is a content
decision rather than a tool failure: `knowledge/journals/`'s 125 pages are
`Journal`s, which is a `working_types` member and not a governed type.

**The conversion is idempotent, and the report says so.** `pages_changed` is the
write set and `pages_unchanged` is everything else; a byte-identical page is not
rewritten, so its mtime stays a usable signal. The verdict is printed first —
`REFUSED`, or `NO-OP … 0 written. The corpus is already migrated.` — because a
summary opening with "8,446 converted" over a run that wrote nothing reads as a
broken tool:

```bash
vault migrate --vault knowledge --only pages --fences-to-properties   # 8,446 converted
vault migrate --vault knowledge --only pages --fences-to-properties   # 0 diffs
```

Two things were needed for that. The body is composed with `trim`, not
`trim_end` — the separator between frontmatter and body is unconditional, so
trimming only the tail accumulated one blank line per run. And the key **order**
is seeded from the page's own frontmatter, because otherwise the output order
depended on which input supplied each key: a first pass orders by the fence, a
second (no fences left) by the authored block, and the two differ while the
content is identical.

`--only pages | journals | all` selects the tree within each vault, so the 1,230
journal pages can take their first pass without touching `pages/` — the
difference between a reviewable diff and a 10,458-file one.

`--repo` must point at the **repository**, not a vault: block references
resolve against `migration.block_refs.resolve_from`, which reaches into
`working/`.

**Lossless by construction.** Every fence key and every Logseq key must appear
in the vocabulary's `migration:` map — mapped to a frontmatter key, or listed
in `ignore:`. An uncovered key exits **2** and writes nothing, naming the key
and its occurrence count.

Three conditions are **reported, not refused**, because refusing would stop a
run over something with nothing to fix at source:

| condition | disposition |
|---|---|
| an unresolvable block reference | removed, counted per page |
| an unmatched code-fence opener | region read as prose, page recorded |
| a long-tail IRI outside the namespace | normalised, slug preserved, counted |

An unterminated fence is **not** treated as opening a code block. 511 knowledge
pages carry a stray opener and 1,713 real `key::` lines fall after it; treating
"opener to end of file" as code would lose them silently.

Delete this subcommand, the `migrate` feature and `vault_core::fences` after the
run lands.

## Golden parity

`tests/golden/` holds 50 unmodified pre-migration pages and the output the
retired Python pipeline produced from them. `tests/golden_parity.rs` migrates
and builds a copy and asserts byte-identity on `scaffold-index.json` and
`prose-index.json`, triple-identity on `ontology.ttl`, and clean validation with
zero residue.

```bash
cargo test -p vault --test golden_parity
```

A diff there means the build changed what Loom loads. Never regenerate the
reference to make it pass.

Two divergences are deliberate and asserted rather than hidden:
`search-index.json`'s `labels` is a superset (every alias, not just
`preferred-term`), and `ontology-inferred.ttl` is Whelk's EL++ closure where the
Python emitted a plain transitive BFS — contract C3 asks for the reasoner.

## Layers

| crate | owns |
|---|---|
| [`vault-core`](../vault-core) | frontmatter, page identity, vocabulary, OKF v0.2, the link graph, the promotion state machine, `PatchProposal`, credential detection |
| `vault` | the projections (`model`, `projection`, `closure`, `whelk`) and the command surface |

`vault-core` is linked by VisionClaw's ingest too, so the corpus is parsed by
exactly one implementation.

## Licence

AGPL-3.0-only.
