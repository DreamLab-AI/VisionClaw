# codescape-model

The I/O-free domain model of [Codescape](https://github.com/DreamLab-AI/codescape): a near-real-time,
voice-collaborative 3D view of a codebase that humans and coding agents explore together.

Every other Codescape crate compiles against these types. The crate does no filesystem, network
or GPU work.

| Module | What it holds |
|---|---|
| `graph` | `CodeNode`, `CodeEdge`, `GraphSnapshot` with validation; node kinds, languages, relations |
| `ids` | `IdInterner`: graph keys to 26-bit wire ids, handed out in order and never hashed |
| `delta` | `GraphDelta`: diff and apply between revisions |
| `community` | `stable_match`: Jaccard matching so community colours survive a recompute |
| `lod` | three detail tiers (directories, files, symbols), centroid aggregation and per-client projection |
| `wire` | the frozen 52-byte V3 record and V5 frame, re-exported from [`visionclaw-wire`](https://crates.io/crates/visionclaw-wire), plus Codescape's field mapping |
| `events` | the JSON events the server and clients exchange |

```rust
use codescape_model::IdInterner;

let mut ids = IdInterner::new(1);
let a = ids.intern("src/lib.rs::parse").unwrap();
assert_eq!(ids.intern("src/lib.rs::parse").unwrap(), a); // stable
let bytes = ids.to_bytes();
assert_eq!(IdInterner::from_bytes(&bytes).unwrap(), ids);
```

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT licence](LICENSE-MIT)
at your option. The Codescape server, ingest, layout and client crates are AGPL-3.0-only; this crate
depends on none of them.
