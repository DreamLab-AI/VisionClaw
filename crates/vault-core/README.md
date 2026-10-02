# vault-core

The parser and data model for an Obsidian vault used as a governed ontology
corpus. Each page is a Markdown file with YAML frontmatter. The frontmatter
carries the ontology, and a vocabulary file declares which keys are legal and
what each one means in OWL.

`vault-core` is the library half of the `vault` CLI. VisionClaw's ingest links
the same crate, so the build and the live graph parse a page identically
(VisionClaw ADR-2113).

## What it does

| Module | Responsibility |
|---|---|
| `page` | A page is YAML frontmatter plus a body. Its id is its path under `pages/` without `.md`. Loads a whole vault. |
| `frontmatter` | Typed access to frontmatter values and the `[[wikilinks]]` inside them. |
| `vocabulary` | `ontology/vocabulary.yaml`: the declared keys, their OWL meaning, the namespaces and the prefixes. |
| `okf` | The OKF v0.2 lifecycle and trust block (`status`, `stale_after`, `generated`, `verified`, `sources`). |
| `graph` | The link graph over frontmatter relations, with expansion depth set per edge type. |
| `promotion` | The promotion state machine: only a human promotes, and a machine-found blocker cannot be waved through. |
| `proposal` | The `PatchProposal` payload a governed edit is submitted as. |
| `slug`, `code`, `json`, `secrets`, `domains` | Identifier rules, code-span masking, JSON helpers, the secret scan and the estate domain registry. |

No network access and no subprocesses. Filesystem reads happen only in the
explicit `load` and `walk` functions.

## Example

```rust
use vault_core::page::Page;

let page = Page::parse(
    "/v/pages/Knowledge Graph.md",
    "pages/Knowledge Graph.md",
    "Knowledge Graph",
    "---\ntype: Class\npublic: true\nis-a: [\"[[Content and Assets]]\"]\n---\nbody\n",
).unwrap();

assert_eq!(page.title(), "Knowledge Graph");
assert_eq!(page.frontmatter.wikilinks("is-a")[0].target, "Content and Assets");
```

The corpus format is specified in
[`docs/VAULT-corpus-format.md`](../../docs/VAULT-corpus-format.md).

## Status

The crate is not yet published. Its defaults still carry this estate's
namespaces (`urn:ngm:`, the `narrativegoldmine.com` prefixes) and its domain
registry. Before it is published to crates.io, these move into the vocabulary
file or a VisionClaw adapter.

## Licence

AGPL-3.0-only. See [LICENSE](LICENSE).
