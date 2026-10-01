# `vault` — the single door onto the sovereign corpus

The CLI validates, queries, edits and builds the Obsidian ontology corpus.
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
**1** a failed check or execution error, **2** a refused operation or invalid argument.

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
`find` and `retrieve` — is every `.md` file under `pages/` and
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
`_misc` page could not be validated, found or edited. It now lives only in the
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
  --set 'generated={by: process:vault/1.0, at: 2026-09-22T11:37:06Z}' \
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

"Code-looking" is a **content** test, never indentation: an indented Markdown
list is still prose. What actually follows a
real stray opener here is OWL functional syntax, Turtle and the occasional
SPARQL query.

Every change is listed in `--report`, and the command is idempotent — a repaired
page has no unmatched marker, so a second run reports `0 repaired` (an ambiguous
page is reported on every run until it is resolved). This check remains necessary when somebody pastes functional syntax into a page.

### `vault repair bodies`

Imported content can contain outliner residue: headings as bullets (`- ### Overview`), paragraphs as indented
bullets, images sized with `{:height 841, :width 800}`, and the blocks inlined
in place of `{{embed}}` with their first child written twice. This rewrites
the body as Obsidian markdown and carries the frontmatter through byte for byte.

```bash
vault repair bodies --vault all --dry-run --report bodies.json
vault repair bodies --vault all
#   10458 page(s) examined, 10446 converted
vault repair bodies --vault all --check   # the residue gate: exit 1 if any page would change
```

| outline block | becomes |
|---|---|
| `- ## Heading` | a flush heading; it closes the list, so its children start a new one |
| `- ---` | a thematic break |
| a fence, table, quote, HTML or image-only bullet | its own markdown block, indented under its list item when it has one |
| a bare `-` | dropped; its children move up |
| `- TODO …` / `- DONE …` | `- [ ] …` / `- [x] …` |
| anything else | `- ` at two spaces per level, the level counting list items since the last heading |

A fence opened in a bullet and never closed anywhere below is closed where its
block ends, which is where Logseq ended it; a closed fence is never touched,
even when its code is less indented than its bullet. Everything else outside
code is carried as written.

It is idempotent — a second run over the converted corpus reports `0 need
converting` — and was verified on the full corpus against the build: every
artefact but the markdown mirror and `prose-index.json` is identical, and the
prose index only gains. `extract_current_landscape` reads a Current Landscape
heading in either form and ends the section at the next heading of its level
or higher; against the pre-conversion build, 903 pages gain a landscape and
none loses one.

## Golden parity

`tests/golden/` holds 50 canonical Obsidian pages and frozen reference output.
`tests/golden_parity.rs` builds a copy and asserts byte-identity on `scaffold-index.json`, identity
on `prose-index.json` but for its documented landscape superset,
triple-identity on `ontology.ttl`, and clean validation with zero residue.

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
