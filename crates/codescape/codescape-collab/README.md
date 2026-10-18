# codescape-collab

Collaboration state for [Codescape](https://github.com/DreamLab-AI/codescape), with no I/O: who is
working on a code graph, what they last touched, who holds the push-to-talk floor, and what a
spoken or typed command means.

| Module | What it does |
|---|---|
| `grammar` | `normalise_spoken` and `parse_command`: a deterministic grammar for code-graph commands ("callers of parse config", "blast radius of graph service depth 2", "rewind 5 minutes", "ask claude to add a test") |
| `llm` | `command_schema` and `command_from_llm`: a JSON Schema for a language-model fallback, with strict validation so model output is never executed unchecked |
| `floor` | `FloorArbiter`: one speaker at a time over a single-stream speech recogniser, FIFO queue, hold cap |
| `actors` | `ActorRegistry`: agents and humans, recent actions, per-node activity heat |
| `heat` | exponentially decaying heat with a 60 s half-life |
| `paths` | `PathMap` prefix rewriting and path-to-node resolution |
| `fuzzy` | identifier-aware trigram ranking of spoken names against symbol labels |
| `timeline` | a bounded log of presence and revision markers for rewind and replay |

```rust
use codescape_collab::{parse_command, Command, Target};

let cmd = parse_command("um, who calls graph underscore service dot rs").unwrap();
assert!(matches!(cmd, Command::Callers { target: Target::Name(_), depth: 1 }));
```

The grammar is pinned by a golden table of 96 utterances and a property test that parsing never
panics on arbitrary input.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT licence](LICENSE-MIT)
at your option. It depends only on `codescape-model` (same licence), `serde`, `serde_json` and
`thiserror`.
