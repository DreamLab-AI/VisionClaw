# Golden corpus — published output compatibility

`fixture/` contains 50 canonical Obsidian pages with YAML frontmatter and
Markdown bodies, plus a frozen `ontology/vocabulary.yaml`.

`python/` contains immutable historical reference output from 2026-09-22.
It is test data, not an executable pipeline. No legacy reader or converter is
needed to run these tests.

`tests/golden_parity.rs` runs `vault build` over a copy of `fixture/` and asserts:

| artefact | assertion |
|---|---|
| `data/scaffold-index.json` | **byte-identical** (modulo timestamps) |
| `data/prose-index.json` | every field **identical** but `cl`, which is a documented **superset** (markdown-form Current Landscape headings are read too; no page loses one) |
| `data/ontology.json` (WebVOWL) | **byte-identical** |
| `data/graph/full.bin` | **byte-identical** |
| `data/graph/domain-*.bin` ×6 | **byte-identical** |
| `data/graph/stats.json` | **byte-identical** |
| `data/graph/overview.json` | **byte-identical** (C3 asks only for JSON-equal) |
| `data/graph/bridges.json` | **byte-identical** |
| `data/ontology.ttl` | identical ground triple set, less the 28 `vc:P rdfs:domain/rdfs:range owl:Thing` triples ADR-2128 removed (documented divergence 4 in `golden_parity.rs`; header triples, now including `owl:versionIRI`, are not compared) |
| `api/search-index.json` | identical except `labels`, a documented superset |

Thirteen of the fourteen artefacts are byte-identical; `ontology.ttl` is
compared as a triple set because `rdflib`'s Turtle serialiser is not
reproducible outside `rdflib`.

## Two things that make the binary tiers reproducible

* **`overview.json`'s baked positions come out of Python's Mersenne Twister.**
  `build::ngg1::PyRandom` reproduces MT19937 and `random.uniform` so the layout
  is the same picture, not merely a similar one.
* **The layout must not use fused multiply-add.** `a + b * c` rounds twice in
  CPython and once under FMA; over 200 iterations that moved one coordinate
  across a `round(x, 3)` boundary. `ngg1.rs` carries a module-level
  `allow(clippy::suboptimal_flops)` for exactly this reason.

Do not regenerate `python/` to make a failing test pass: a diff here means
the Rust build changed what Loom loads.
