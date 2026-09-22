---
title: VAULT — authored corpus format (Obsidian vault)
version: 2.0.1
status: living
verified_commit:
owner: jjohare
domain: VAULT-corpus-format
ledger: [ADR-2112, ADR-2113, ADR-2114, ADR-2115, ADR-2116]
agentbox_ledger: [ADR-2106, ADR-2107, ADR-2108]
---

# VAULT — authored corpus format

This is the governing document for the **authored knowledge corpus**: the markdown
files that VisionClaw ingests into the knowledge graph, that `vault build` turns
into the Loom bundle and narrativegoldmine.com, that the governance loop writes
back to, and that agentbox skills read and extend.

**v2.0.0 is a format break, not an amendment.** The ontology no longer lives in
two `json-ld` fences per page; it lives in typed Obsidian Properties. There is no
legacy tolerance: `vault validate` **rejects** a fence, a `key:: value` line, an
`{{embed}}`, a `((block-ref))`, an `a___b.md` filename or a Logseq journal. Git is
the rollback. See ADR-2112 (supersedes ADR-2040).

Related governing documents: [`DATA-authority-erasure.md`](DATA-authority-erasure.md)
(ownership of the "Authored content" class),
[`BASELINE-architecture.md`](BASELINE-architecture.md) (the corpus → Oxigraph →
client pipeline), [`IDENTIFIER-taxonomy.md`](IDENTIFIER-taxonomy.md) (IRIs and node
ids), and in the corpus itself `visionGraph/ontology/vocabulary.yaml` (the
normative vocabulary) and `visionGraph/vault.toml` (the manifest).

## Purpose

1. Define the **one** on-disk format the system reads and writes: an
   [Obsidian](https://obsidian.md) vault of plain markdown whose metadata is
   entirely YAML frontmatter.
2. Define the **two vault roles** — `knowledge/` as a governed
   [OKF v0.2](https://openknowledgeformat.org) bundle, `working/` as the curator's
   OKF-conformant space — and the type set each admits.
3. Point every rule at the **versioned vocabulary** (`ontology/vocabulary.yaml`)
   rather than restating it here, so the format and the ontology cannot drift.
4. Give every consumer (VisionClaw backend and client, Loom, agentbox skills,
   Quartz) a single path authority and a single parser.
5. State what the format **forbids**, so the check is mechanical.

## Current state

### Corpus census (2026-09-22, `/home/devuser/workspace/visionGraph`)

Measured by a pipeline-independent parse; full evidence in
[`visionGraph/docs/fence-census-2026-09-22.md`](https://github.com/jjohare/visionGraph/blob/main/docs/fence-census-2026-09-22.md).

| Statistic | knowledge/ | working/ |
|---|---|---|
| Markdown files | 8,671 (28 under `pages/.deleted/`) | 574 |
| Pages with `public: true` in frontmatter | 8,608 | 193 |
| Pages with a `json-ld` fence | 8,454 | 0 |
| `@type: Class` fences | 8,446 | 0 |
| `@type: Page` fences | 8,454 | 0 |
| `vc:LinkResolutionsAnnotation` fences (undocumented until now; derived data) | 3,712 | 0 |
| Distinct relation predicates inside `relations` | 57 | — |
| Relation edges in the fences | 104,731 | — |
| Relation edges present **only** in `key::` lines | 36,632 (18,006 resolvable) | — |
| `key:: value` lines | 100,675 over 643 keys | 7,531 over 17 keys |
| `{{embed}}` occurrences | 35 across 14 files | 41 across 23 files |
| `a___b.md` namespace files | 0 | 0 |
| Logseq journals | 0 | 0 |
| Classes whose `definition` exists **only** in the fence | 5,053 of 8,446 | — |

Three facts from that census drive the design of v2 and are worth stating here
because they contradict v1.4.2:

- **The body `key::` lines are not decoration.** v1.4.2 §V3 classified them as
  "content, not metadata". They carry 18,006 relation edges that resolve to real
  pages and appear nowhere in the fences. Migration takes the **union**.
- **The `definition` is the content.** For 5,053 classes the fence is the only
  place the prose exists. It becomes the page's leading body paragraph, not a
  frontmatter key.
- **The current build silently loses data.** 955 classes declare
  `maturity: mature`, which the Turtle emitter coerces to `draft`; 744 classes
  carry a `qualityScore` the parser never reads and publish `quality 0.0`; and 75
  relation edges across 34 predicates are dropped because `rel_map` has twelve
  entries. v2 fixes the first two and makes the third visible.

### Readers and writers (post-ADR-2113)

**One parser, one vocabulary, one reasoner.** `vault-core` (VisionClaw
`crates/vault`) holds the frontmatter parser, the vocabulary model, the OKF types
and the promotion state machine. VisionClaw's ingest, the `vault` CLI and CI all
call it; nothing else parses the corpus.

| Component | Role | Contract |
|---|---|---|
| `vault-core::parse` | the one parser: frontmatter → `Page { type, title, resource, public, status, generated, verified, sources, relations, scalars }` | ADR-2113 |
| `vault validate` | OKF conformance + `vocabulary.yaml` + `types.json` agreement + link integrity + public gate | C2 |
| `vault find / retrieve / tree` | graph over frontmatter wikilinks, per-edge-type expansion depth, `max_documents` cap | C2 |
| `vault edit --expect docs=N blocks=M` | guarded mutation; refused without a declared blast radius | C2 |
| `vault propose` | `PatchProposal` (C4) + Whelk and conflict blockers + forum 31402 | C4, C5 |
| `vault gate` / `vault conflicts` | the autonomous quality gate and the conflict detector | C2 |
| `vault build` | the bundle (C3), Quartz `static/`, the Loom bundle | C3 |
| `vault migrate --fences-to-properties` | the one-shot; deleted after its run | C1 `migration:` |
| `CorpusSource::LocalDirectory` | VisionClaw ingest over the mounted named volume, via `vault-core` | ADR-2114 |
| Loom | consumes the `vault build` bundle; generation `visionGraph@<sha>` | Loom ADR-141 |
| Quartz v4 | renders narrativegoldmine.com from `knowledge/`, ExplicitPublish ⇐ `public` | PRD Q13 |
| agentbox skills (`ontology-augment`, `podcast-knowledge-ingest`, `ontology-curator`) | read and write via the `vault` binary; **no MCP server** | ADR-2107 |

Deleted by this change: `pipeline/*.py`, `publishing-tools/`,
`crates/vault-migrate`, `agentbox/mcp/servers/ontology-bridge.js` and
`ontology-propose.js`, `loom-mcp-stdio`, the ADR-2041 `serde(alias = "logseq")`.

## The vault contract

### V1 — Layout

```
visionGraph/
├── vault.toml                  # roles, vocabulary version, build targets, policy, publish
├── ontology/vocabulary.yaml    # the normative vocabulary; versioned; Schema-tier
├── knowledge/                  # the governed OKF bundle
│   ├── .obsidian/              # committed: core plugins, types.json, bases/*.base
│   ├── pages/**.md             # frontmatter-only
│   ├── assets/                 # symlink → ../working/assets  (see V9)
│   └── index.md                # OKF §8 index, generated by `vault build`, committed
├── working/                    # the curator's space
│   ├── .obsidian/              # committed: core plugins + Templater templates + Canvas
│   ├── pages/**.md
│   └── assets/
├── quartz/                     # Quartz v4 config; content dir → ../knowledge
└── .github/workflows/publish.yml
```

- `vault.toml` is the single path authority. No consumer hard-codes a corpus path;
  agentbox's manifest `[vault].root` supplies the root and every sub-path derives
  from `vault.toml`.
- **Page identity** is the vault-relative path without `.md`
  (`knowledge/pages/Knowledge Graph` ⇒ id `Knowledge Graph`), with `/` as the
  namespace separator (contract C2). The same relative path in `knowledge/pages/`
  and `working/pages/` is deliberately **one node** — the knowledge↔working twin
  join, 254 such pairs.
- There are no `journals/`, no `a___b.md` names and no `%2F` encodings. A
  namespace is a real directory.
- `pages/.deleted/` is not a namespace. It is deleted, not converted.

### V2 — Frontmatter is the whole of the metadata

Every page begins with a YAML frontmatter block delimited by `---`. Keys are
lower-kebab-case. The reserved Obsidian keys `aliases`, `tags`, `cssclasses` keep
their Obsidian meaning. **There is no metadata anywhere else in the file.**

The key set is not listed here — it is `ontology/vocabulary.yaml`, and
`vault validate` reads that file, not this document. What this document fixes is
the *shape*:

| Group | Keys | Type | Source of truth |
|---|---|---|---|
| Identity | `type`, `title`, `resource` | text | `vocabulary.types`, `vocabulary.identity` |
| Gate | `public` | boolean | `vocabulary.scalars.public` |
| Classification | `domain`, `maturity`, `quality`, `authority`, `gloss`, `aliases`, `tags`, `legacy-term-id` | per vocabulary | `vocabulary.scalars` |
| Relations | `is-a`, `requires`, `enables`, `uses`, `supports`, `has-part`, `part-of`, `related-to`, `depends-on`, `bridges-to`, `contrasts-with`, `implements`, `standardized-by`, `same-as`, + 35 provisional | list of wikilink strings | `vocabulary.relations` |
| OKF lifecycle | `status`, `stale_after` | text, date | `vocabulary.okf.lifecycle` |
| OKF trust | `generated {by, at, rule}`, `verified [{by, at}]` | mapping, list | `vocabulary.okf.trust` |
| OKF provenance | `sources [{id, resource}]` | list | `vocabulary.okf.sources` |

Rules:

1. **Every relation value is a list of wikilink strings**, even a single target:
   `is-a: ["[[Algorithm]]"]`. Wikilinks in property values are quoted.
2. `public` is a real YAML boolean, never `"true"`.
3. `resource` is minted once from `namespace + slug(title)` and is **immutable
   thereafter**. It is never recomputed: 412 existing IRIs come from a
   case/digit-boundary slugifier (`3D Asset` → `3-d-asset`), they are the subject
   of 104,731 edges and of every published URL, and stability beats tidiness.
   `vault validate` checks presence, uniqueness and immutability.
4. `title` is display only, omitted when it equals the filename stem, and never
   the identity path.
5. `ancestors`, the inferred closure, backlinks and outbound-wikilink lists are
   **build outputs** (`<out>/api/`, `ontology-inferred.ttl`). Storing them in a
   page is a violation.
6. An unknown key in `knowledge/` **fails** `vault validate`. `working/` tolerates
   unknown keys (OKF v0.2 §4.1); the documented episodic extensions are in
   `vocabulary.working_extensions`.
7. A page with no frontmatter is **private and invalid** — fail-closed on the gate,
   and a validation error in `knowledge/`.

### V3 — Body dialect

The body is prose and wikilinks. Nothing in it is metadata.

| Construct | Vault form |
|---|---|
| The concept's definition | the **leading paragraph**, immediately after the frontmatter |
| Wikilinks | `[[Page]]`, `[[Page\|Alias]]`, `[[Ns/Page]]` |
| Page embeds | `![[Page]]` |
| Tasks | `- [ ] text`, `- [x] text` |
| Tags | `#multi-word` |
| Assets | `assets/<file>` — vault-root-relative, never `../assets/` |
| Code fences | any language **except `json-ld`** |

Rejected outright by `vault validate` (ADR-2112; no tolerance window, no flag):

- ` ```json-ld ` fences of any `@type`
- `key:: value` lines, in the leading block or in a bullet
- `{{embed [[Page]]}}` and `{{embed ((uuid))}}`
- `((block-ref))`
- `a___b.md` filenames and `%2F` in a path
- Logseq journals and `YYYY_MM_DD.md` filenames
- `TODO` / `DOING` / `NOW` / `LATER` / `DONE` markers
- `#[[multi word]]` tags
- a `### Relationships` section listing edges (regenerated from frontmatter)

### V4 — Inclusion and publish gates

Two gates, and they are now the same gate:

1. **Ingest.** Every page in `knowledge/pages` with valid frontmatter is a KG
   node. There is no `owl-class` bypass and no OR: v1.4.2's `is_kg_included()`
   disjunction (`public: true` OR a class marker) produced pages that were private
   and published at once. `type` and `resource` make a page a class; `public`
   decides only whether it is *published*.
2. **Publish.** A page publishes to narrativegoldmine.com iff `public: true`
   (Quartz ExplicitPublish). Fail-closed: absent means private.

`working/` is never ingested into the governed graph and never published. It is
read by `vault propose` to build proposals and by the curator in Obsidian.

**The publish gate decides visibility, not safety.** `public: true` answers
"should this be published?" and never "is this safe to publish?". Those are
different questions, and answering the first correctly is precisely what
publishes a secret. A 2026-09-22 sweep found 14 live API credentials across 5
files in `working/` — two OpenAI keys and a bearer token among them — one on a
page marked `public: true` and on the publication list; they surfaced only
because a `knowledge/` copy of a journal had been redacted and its `working/`
twin had not. `vault validate` has no credential check and would have passed
that page. Until it has one (`CREDENTIAL_RESIDUE`, error severity, **both**
vaults — the leak reached `public: true` from the ungated side), the gate is
not a safety control and must not be relied on as one.

### V5 — Writers emit frontmatter only

`vault edit`, `vault build`, agentbox's `podcast-knowledge-ingest` and
`web-summary`, VisionClaw's mutation and elevation paths: all emit frontmatter
pages through `vault-core`'s one emitter, so YAML quoting of `"[[Page]]"` and
`mv:Foo` is solved once. No writer emits a fence or a `key::` line. A writer that
must touch a page it did not create still goes through `vault edit --expect`, which
refuses a mutation without a declared blast radius and names the missing guards.

### V6 — Migration (`vault migrate --fences-to-properties`, ADR-2113)

The one remaining one-shot; the subcommand is deleted after its run.

- Lossless **by construction**: the migration fails on any fence field or `key::`
  key not covered by `vocabulary.migration`, rather than guessing or dropping.
- `--dry-run` first; the diff is committed as evidence before the real run.
- Three rules do the work (full statement in `vocabulary.migration`):
  - a `key::` line in the **leading block** is page-level and takes its mapped
    frontmatter destination;
  - a `key::` line **inside a bullet** is block-level: a relation key is
    union-merged, anything else is rendered into that bullet's prose and the key
    dropped — block properties cannot be lifted to page-level frontmatter without
    collapsing many values into one;
  - relation values are the **union** of the fence targets and the `key::`
    wikilink targets, deduplicated by resolved identity, dangling targets
    reported;
  - a `{{embed ((uuid))}}` block reference is **resolved and inlined**, not
    deleted. The uuids were long assumed dead; they are not — the earlier
    survey searched only `knowledge/`, and 33 of the 34 resolve to an `id::`
    line in `working/`. Each becomes a blockquote carrying an HTML provenance
    comment that names the source file. The two remaining occurrences are
    documentation *about* Logseq syntax inside inline code, and are escaped.
- Idempotent: a second run is a no-op.
- `git` is the rollback. There is no reverse converter.

### V7 — Vocabulary is versioned and Schema-tier

`ontology/vocabulary.yaml` carries `version`. `vault.toml` pins it. `vault build`
stamps it into `.generation.json`. Changing the file — adding a relation, changing
an `owl:` value, flipping `emitted`, widening an enum — is a **Schema-level**
change: tier High by floor, human-signed forum 31403, Whelk-clean (PRD Q6/Q7).

The `owl:` value of every relation with `emitted: true` is byte-identical to the
property the retired Python emitter wrote. **Only the authoring syntax changed.** A
change to an `owl:` value is an ontology break.

`inverse:` in the vocabulary is an authoring and build hint, not an emitted
`owl:inverseOf`: inverse object properties are outside the OWL 2 EL profile Whelk
reasons over, so `vault build` materialises the reverse edge in the graph, page API
and scaffold index and emits no inverse axiom. The corpus uses exactly one inverse
pair in both directions (`has-part` / `part-of`).

### V8 — Obsidian tooling and the two type sets

Core plugins only: Bases, `types.json`, Templater, Canvas, obsidian-git. Both
`.obsidian/` directories are committed and validated by `vault build`; a `types.json`
that disagrees with `vocabulary.yaml` is a validation failure.

| | `knowledge/` | `working/` |
|---|---|---|
| Types | `Class`, `Property`, `Individual` | `Note`, `Episode`, `Transcript`, `Draft Concept`, `Journal`, `Canvas` |
| Unknown keys | fail | allow |
| Required | `type`, `title`, `resource`, `public`, `status`, `generated` | `type`, `title` |
| Reasoned | yes (Whelk EL++) | no |
| Published | `public: true` → Quartz | never |
| Documented extensions | — | `source`, `topic`, `episodes`, `assertions`, `promotion-status` |

`Draft Concept` + `status: draft` is what `vault propose` generates a proposal
from. Bases views over the same frontmatter give the curator the review queue,
the stale list and the by-domain browse with no plugin beyond core.

### V9 — Assets: one store, one symlink

`knowledge/assets` is a **symlink** into `working/assets`. It stays. A page
elevated from the working vault keeps its image and PDF links without a copy, and
the alternative — an independent copy per vault, as the v1 converter did — produced
two divergent multi-hundred-megabyte trees. `vault validate` follows the symlink
for link integrity, `vault build` resolves it, Quartz copies through it, and asset
links are always vault-root-relative `assets/<file>`.

## Worked example

`knowledge/pages/Knowledge Graph.md` after migration — the whole file:

```markdown
---
type: Class
title: Knowledge Graph
resource: urn:ngm:class:knowledge-graph
public: true
aliases: [KnowledgeGraph]
domain: spatial-computing
maturity: established
quality: 0.35
is-a: ["[[Content and Assets]]"]
requires: ["[[Ontology]]", "[[Schema Definition]]", "[[Triple Store]]"]
enables: ["[[Reasoning]]", "[[Knowledge Discovery]]", "[[Recommendation System]]"]
part-of: ["[[Semantic Web Infrastructure]]", "[[Knowledge Management System]]"]
status: stable
generated: { by: process:vault-migrate/1.0, at: 2026-09-22T00:00:00Z }
verified: [{ by: human:<npub>, at: 2026-09-22T00:00:00Z }]
sources: [{ id: origin, resource: "[[working/Knowledge graph notes]]" }]
---

A knowledge graph is a structured representation of entities and the relations
between them, expressed so that both people and machines can traverse it.

## Applications

Knowledge graphs underpin [[Semantic Search]] and [[Recommendation System]]s …
```

Note what is absent: no fence, no `key::` line, no `### Relationships` section, no
`ancestors`, no `slug`, no `schemaVersion`, no `@context`. The definition is the
leading paragraph. `resource` was copied from the fence's `@id`, not recomputed.

## Invariants (must not silently change)

1. **Frontmatter is the only metadata.** A fence, a `key::` line or a
   `### Relationships` section anywhere in `knowledge/` or `working/` is a
   validation failure, not a tolerance.
2. **One parser.** `vault-core::parse` is the only code that reads the corpus.
   A second parser — in VisionClaw, in a skill, in a script — is a violation.
3. **`resource` is immutable.** Minted once, never recomputed, never reused.
   A changed `resource` is the `RESOURCE_MUTATED` blocker.
4. **Identity is the vault-relative path.** Non-namespace page ids are
   byte-identical to their v1 values; namespace ids derive from `Ns/Title`.
5. **Path authority is `vault.toml`.** No consumer hard-codes a corpus path.
6. **The emitted OWL surface is fixed.** Every `emitted: true` relation keeps the
   `vc:` property the Python emitter wrote. Changing one, or flipping a
   provisional relation to `emitted: true`, is a signed Schema change.
7. **Fail-closed publish.** `public` absent or false means the page does not
   reach narrativegoldmine.com.
8. **Derived data is never stored in a page.** Inferred closure, backlinks,
   outbound wikilinks, link resolutions and the OKF index are build outputs.
9. **Blockers are never approvable.** Whelk inconsistency, subclass cycles,
   relation contradictions, vocabulary violations and resource collisions block a
   proposal; no signature overrides them.
10. **Agents never push.** `vault` writes working trees; the owner commits and
    publishes.
11. **No credential reaches either vault.** Keys, tokens and bearer strings are
    never committed to `knowledge/` or `working/`. The `public` gate does not
    catch them and is not a safety control; `working/` is not gated at all, and
    a credential there is one elevation away from the open web.

## Expectations (EDD)

| ID | Priority | Expectation | Evidence |
|---|---|---|---|
| EXP-V01 | critical, regression | `vault validate` exits 0 on both vaults; zero fences, zero `key::` lines, zero `{{embed}}`, and every `knowledge/` page carries `type`, `resource`, `status`. | `vault validate --vault all --strict` |
| EXP-V02 | critical, regression | A page carrying a fence, a `key::` line, a `{{embed}}`, an `a___b.md` name or a journal filename fails `vault validate` with a named rule. One fixture per rejected construct. | `cargo test -p vault validate_rejects` |
| EXP-V03 | critical, regression | An unknown frontmatter key fails in `knowledge/` and passes in `working/`. | same |
| EXP-V04 | critical | `vault migrate --fences-to-properties --dry-run` covers every fence field and every one of the 643 `key::` keys; an uncovered field fails the run rather than dropping. | migration report, committed as evidence |
| EXP-V05 | critical | Migration is lossless on relations: the post-migration edge count equals the union of the 104,731 fence edges and the 18,006 resolvable `key::`-only edges, minus duplicates, and the report accounts for every dangling target. | `vault build --stats` against the census |
| EXP-V06 | high, regression | Running `vault migrate` twice yields byte-identical output the second time. | migration test |
| EXP-V07 | critical | `vault build` and the retired Python build emit byte-identical `scaffold-index.json`, and identical `ontology.ttl` **except** for the five deltas enumerated in `vocabulary.migration.intentional_deltas`. | golden-parity test (ADR-2113) |
| EXP-V08 | critical | Loom `/health`, VisionClaw `/api/ontology/classes` and `vault build --stats` report the same class count from one generation. | PRD acceptance 2 |
| EXP-V09 | high | VisionClaw boots with no `PRIVATE_REPO_GITHUB_PAT`, ingests from the mounted volume, and its node/edge counts sit within the explainable delta of the 13,165 / 153,960 baseline. | PRD acceptance 3 |
| EXP-V10 | high | `vault edit` without `--expect` is refused and names the missing guards; with a wrong `--expect` it is refused and names the actual blast radius. | `cargo test -p vault edit_guard` |
| EXP-V11 | high | A `vault propose` on a test IRI posts a 31402; a human 31403 Approve writes `verified` + `status: stable` and a ledger entry with the same case id; Reject writes nothing; a Schema-level proposal is tiered High. | PRD acceptance 4 |
| EXP-V12 | medium | Quartz builds narrativegoldmine.com from `knowledge/` locally, `static/api/search-index.json` and `static/data/ontology.ttl` are present, and only `public: true` pages are rendered. | PRD acceptance 6 |
| EXP-V13 | medium | `grep -ri logseq` across loom, VisionClaw, agentbox, VisionFlow and visionGraph returns only ADR and history references. | PRD acceptance 7 |

## Change process

This is a living document. Amend it in the same change that alters a reader,
writer, gate rule, migration rule or the path authority — and note that most such
changes belong in `ontology/vocabulary.yaml` instead, which is where the key set,
the OWL mapping and the migration table now live. This document governs the
*shape*; the vocabulary governs the *content*.

Bump `version`: patch for wording, minor for a new rule or section, **major for a
change to identity, the gate, or what the format rejects**. Refresh
`verified_commit`. A change to the vocabulary is separately a signed Schema-level
decision (V7).

## Version history

| Version | Date | Change |
|---|---|---|
| 2.0.1 | 2026-09-22 | Invariant 11 (no credentials in either vault) and the V4 note that the publish gate is a visibility control, not a safety one, after 14 live credentials were found in `working/`, one on a `public: true` page. |
| 2.0.0 | 2026-09-22 | **Format break.** Frontmatter-only; the two json-ld fences fold into typed Obsidian Properties; `ontology/vocabulary.yaml` becomes the normative key set and `vault.toml` the manifest; two vault roles with distinct type sets and unknown-key policies (PRD Q9); the `owl-class` gate bypass removed; all legacy tolerance removed — `key::`, `{{embed}}`, `((block-ref))`, `a___b.md` and journals are now validation failures; `vault` (Rust) replaces `vault-migrate`, the Python pipeline and the ontology MCP servers. ADR-2112, supersedes ADR-2040. |
| 1.4.2 | 2026-09-05 | ADR-2096 remediation: `LocalFileSyncService` gate delegates to `vault::parse`; the last raw carrier scan removed. |
| 1.4.1 | 2026-09-05 | ADR-2064 and ADR-2070 remediation: real-corpus ontology loader; typed `owl-class` grammar. |
| 1.4.0 | 2026-09-04 | Converter, inclusion and settings closeout qualifications recorded. |
| 1.3.0 | 2026-09-02 | Shortest-path wikilink resolution rule (EXP-V08); namespace folders; identity via `page_name_from_path`. |
| 1.2.0 | 2026-09-02 | Corpus survey; `vault-migrate` converter contract (ADR-2042). |
| 1.1.0 | 2026-08-31 | Settings and wire vocabulary rename `logseq` → `knowledge` (ADR-2041). |
| 1.0.0 | 2026-08-31 | Initial: Obsidian vault frontmatter, inclusion gate, bounded legacy tolerance (ADR-2040). |

## Superseded qualifications

The v1.4.x closeout qualifications for ADR-2040 (inclusion typing, local
fallback), ADR-2041 (settings migration) and ADR-2042 (converter collision,
dry-run boundaries) are **closed by supersession**, not by remediation: the gate
they qualified is replaced (V4), the alias they qualified is deleted (ADR-2114),
and the converter they qualified is deleted (`crates/vault-migrate` → `vault
migrate`, ADR-2113). Their evidence is retained in
[`docs/estate-review/authored-vault-transition.md`](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/authored-vault-transition.md)
for rationale and history, never as authority.
