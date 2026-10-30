---
id: ADR-2164
title: Code-graph entities link to knowledge-graph and ontology nodes, resolved at runtime
date: 2026-10-30
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 00669b7a2b8bca15702b8173df1dab268144c3ec
verified_paths: []
owner: jjohare
review_trigger: the vault/KG schema gaining a code-reference property; Codescape ingesting Document nodes; Codescape gaining NIP-98 (its P6) or an egress change; a second link source being proposed; the initial-load node cap or its quality ordering changing; ADR-2161 link records going live
repo: visionclaw
domain: XR-client
depends_on: [ADR-2143, ADR-2158, ADR-2159]
amends: []
relates: [ADR-2135, ADR-2161, ADR-2024, ADR-2021]
---

# ADR-2164 — Code-graph entities link to knowledge-graph and ontology nodes, resolved at runtime

## Context
- The operator asked (2026-10-30) for "linked knowledge and code graphs": the Codescape volume (ADR-2158 D4) should show which code implements which concept in the knowledge and ontology volumes.
- **Today's data holds no usable link.** Measured at `00669b7a2` against the running servers (detail in Verification):
  - KG pages cite no estate code: 6 of 9 612 pages in the live corpus contain a source path, and all 6 are illustrative.
  - Wire metadata carries no path.
  - Code mentions ontology IRIs 271 times (93 distinct), but these are doc-comment examples and test fixtures. Only 20 of the IRIs name a live class.
  - ADR `verified_paths` (247) and the docs' `path:line` citations (1 944) have no endpoint in either volume. ADRs are not KG nodes, and Codescape ingests no Document nodes.
  - Exact label matching gives several hundred polysemous hits (`Address`, `Block`, `Base`).
- **Isolation (ADR-2143 D8, I1, I10).** Codescape takes no dependency on a VisionClaw crate or path, mounts no KG data, and has no egress to VisionClaw. VisionClaw must not proxy Codescape (ADR-2159, rejected alternative).
- **Identifiers each side already sends.**
  - Codescape's XR `initialGraphLoad` carries `metadata_id` (the stable interned key) and `metadata.source_file` per node, at the **file tier** by default (`codescape-server/src/xr.rs:21-61`, `routes/ws.rs:259`).
  - VisionClaw's carries the full metadata map, including `class_iri` and `page_iri` (`src/handlers/socket_flow_handler/types.rs:301-317`).
  - Wire ids are ephemeral per server (ADR-2024).

## Decision
1. **Link source: an explicit annotation in code, `@ontology <iri>`.** The annotation is a comment line in any estate source file. After its comment leader (one of `//`, `///`, `//!`, `#`, `--`, `;`, `*`, `/*` or `<!--`), the line holds the token `@ontology` followed by one to eight IRIs. Each IRI matches `urn:ngm:class:[a-z0-9-]{1,128}` or `urn:visionflow:page:[a-z0-9-]{1,128}`.
   - The explicit marker is what separates a claim from the 271 incidental IRI mentions. A bare IRI never links.
   - Codescape checks only the shape. It never resolves, dereferences or validates an IRI against an ontology, because to Codescape an IRI is an opaque string.
2. **Binding and roll-up, in Codescape (`codescape-ingest`, new module `annotations.rs`).**
   - **Binding.** An annotation in a contiguous comment block that sits directly above a symbol binds to that symbol. Otherwise it binds to the innermost node whose span contains the line, which is the file node at worst.
   - **Roll-up.** A file node carries the union of its own and its symbols' annotations, because the XR stream is file-tier. Directory nodes carry none.
   - **Storage.** Annotations are a side table keyed by interned id, rebuilt for `changed_paths` on every delta. `CodeNode`, the persisted snapshot and the V3 wire mapping are unchanged.
3. **Delivery: on the existing stream, always on.**
   - **On the stream.** Codescape's `initialGraphLoad` adds one string key to the existing node `metadata` object: `ontology_refs`, a space-separated list of `<iri>#L<line>` sorted by IRI then line, at most 32 per node, and omitted when empty. A new key in an existing JSON object is not a new wire format (ADR-2143 D4).
   - **Which nodes carry it.** It goes on every annotated node in the projection: the file node (with its roll-up) and any symbol node the stream includes. `/wss` sends the file tier plus focus symbols (`routes/ws.rs:8-9`).
   - **No duplicates.** One annotation can therefore reach the client twice. The join de-duplicates on (iri, path, line): a symbol node that is present wins, and the file node's copy is suppressed.
   - **Read-only route.** `GET /api/graph/ontology-refs?repo=<label>` returns every annotation as `[{key, id, path, line, iri}]` for verification and the desktop client.
   - **Always on.** Both are sent whether or not any client draws links, so the ADR-2158 switch never changes what the server sends.
4. **Resolution happens in the XR client, never server to server.**
   - **Per-connection tables.** Each `BackendConnection` (ADR-2159) builds a table in Rust from its own topology frame, keyed by (connection, stream generation):
     - from Codescape, `key → (wire id, refs)`;
     - from VisionClaw, `iri → [wire ids]` over `metadata.class_iri`, `metadata.page_iri` and the top-level `owl_class_iri`. The value is a list because one IRI can sit on more than one node: 2 of 17 903 distinct IRIs do in the live graph (`urn:visionflow:page:comfy-ui` and `…:bitcoin`). The join emits one record per target id.
   - **Join.** The client joins the two tables on the IRI. A ref whose IRI is absent from the VisionClaw table is **unresolved**: it is counted and drawn as nothing. That covers an IRI that is not live, that is outside the viewer's visibility, or that fell beyond the initial-load node cap.
   - **Isolation.** No crate, path, proxy or new egress joins Codescape and VisionClaw. The headset holds both connections already.
5. **The link record**, used by every cross-volume link kind. It is client-side render state only: in memory, never persisted, never sent:

   ```
   { source:     { volume, conn, key, id },            # Codescape: key = metadata_id, id = wire id
     target:     { volume, conn, key, id },            # VisionClaw: key = the IRI, id = wire id
     kind:       "annotates" | "bridge" | "dataspace",
     provenance: { via: "codescape-annotation", repo, path, line, revision, generations: [src_gen, dst_gen] } }
   ```

   - `key` is the stable identity. `id` is a product of resolution that is valid only for the generation recorded, and a reconnect on either side discards and rebuilds the records.
   - `volume` is whichever ADR-2158 volume holds the node: an ontology-flagged node lands in the ontology volume, and a knowledge-flagged node in the knowledge volume.
   - `bridge` (ADR-2135) and `dataspace` (ADR-2161) use the same shape. For `dataspace`, ADR-2161's `(subject URN, predicate, object URN, asserting node)` maps onto `source.key`, `kind`, `target.key` and provenance.
6. **Offline behaviour.**
   - If Codescape or VisionClaw is down or reconnecting, the records involving that connection are dropped at once, with no stubs, ghosts or retries of their own.
   - When the connection's next topology arrives, its new generation triggers a fresh join.
   - Losing Codescape never disturbs the knowledge or ontology volumes (ADR-2159).
7. **One switch.** Code links are drawn only while `xr.cross_volume_links` is on. There is no separate switch. Per ADR-2158 D6, it is on in debug builds and off in release, toggled at runtime, and stored per device under a key per build type.
8. **Budget.** All cross-volume links, of every kind, are drawn by one link layer:
   - one `MultiMeshInstance3D`, so **one draw call**, with the edge stride of 16 floats (Invariant 3);
   - at most **512** links, ordered by (kind, target key, source key), with a counter for the overflow;
   - a low-poly segment of at most 12 triangles, so at most 6 144 triangles in all;
   - endpoint positions refreshed at 10 Hz, not every frame.

   The scene charges the layer's triangles as outside-layer triangles in the existing allocator (`frame_budget.rs` pass 1). While the switch is off the layer has no instances and costs nothing.
9. **Seed.** Phase 1 adds at least eight annotations in estate code outside the phase-1-owned files. They use IRIs that are live, public and inside the default 3 000-node initial load, which is ordered by quality: `bm25`, `cosine-similarity`, `canonical-json`, `cha-cha20-poly1305`, `retrieval-augmented-generation`, `sparql`, `embedding` and `did-nostr` (ranks 11 to 1 579 of 9 679 at this revision).

Rejected alternatives:
- **`path:line` citations in KG pages.** The live corpus has none for estate code. They would also drift with every edit, where an annotation moves with its symbol.
- **ADR `verified_paths` and the docs' citations.** These are plentiful, but neither endpoint is a KG or ontology node.
- **Literal IRI mentions in code (no marker).** 20 live classes from 27 files, all of them examples or test fixtures, so every link would mislead.
- **Exact or fuzzy label matching.** The hits are polysemous (`Address`, `Block`). It might be revisited later as a curated suggestion list, never as drawn links.
- **A property on the KG page (for example `implemented-by:`).** This would need a vault schema change and an enrichment pass, and it would cite paths that drift. Code-side annotation needs neither.
- **Resolving in VisionClaw by calling Codescape's API.** This couples release cycles and puts VisionClaw on Codescape's availability path. ADR-2159 already rejected proxying.
- **Resolving in Codescape by reading the KG.** This breaks the filesystem and egress isolation of ADR-2143 D8.

## Consequences
- **Easy.** Each link is a one-line, reviewable claim next to the code it describes, and it follows refactors. The join is pure and testable on fixtures. The desktop client can reuse the route later.
- **Hard.** Links exist only where someone writes them, so the volume starts nearly empty: the seed holds eight. Codescape gains an ingest module, a side table and a route, and it is a separate repository: the work lands as a Codescape commit, and the root picks it up through a submodule-pin bump.
- **Foreclosed.** Links whose target sits outside the viewer's initial load. Links from symbols the XR stream does not carry: they appear only through their file's roll-up.
- **Cost.** One more string per annotated file node on Codescape's topology frame, two small per-connection tables on the headset, and at most one draw call and 6 144 triangles.
- **Revisit** in any of these cases:
  - the KG schema gains a code property (a second source joining the same record);
  - Codescape ingests documents;
  - the initial-load cap or its ordering changes;
  - the operator wants links that persist or federate (ADR-2161).
- **Follow-on.** `docs/XR-client.md` gets a being-implemented entry. `docs/PROTOCOL-registry.md` records the `ontology_refs` key under the Codescape producer **in the same change that lands the key** (agent D). ADR-2143 governs that file, so that change also re-verifies ADR-2143 and bumps its `verified_commit`. The contract is in `docs/proposals/spatial-rooms/XR-PHASE1-CONTRACT.md` (agent D).

## Expectations
- **L1.** An `@ontology` line binds to the documented symbol, or else to its innermost container, and rolls up to the file. A malformed IRI, a bare IRI and a ninth IRI on one line are all ignored (`codescape-ingest` tests).
- **L2.** Codescape's `initialGraphLoad` carries `ontology_refs` on annotated file nodes and on any annotated symbol node it includes, and on no unannotated node. `cargo test --workspace` in `codescape/`, including `tests/isolation.rs`, still passes with no new dependency.
- **L3.** With both connections fed fixture frames, the join yields exactly the expected records. An IRI missing on the VisionClaw side increments `unresolved` and yields no record. A ref carried by both a symbol and its file yields one record, from the symbol. An IRI on two VisionClaw nodes yields two records (xr-client Rust tests).
- **L4.** A reconnect on either side bumps its generation and drops every record that names the old one before any new record is drawn. With the Codescape connection closed, the layer draws zero links and logs no error (GUT).
- **L5.** The switch defaults to `OS.is_debug_build()`. Toggling it off leaves 0 instances in the same frame. The preference stored for the running build type overrides the build default, and the other build type's key is ignored. The layer never exceeds one draw call or 512 instances (GUT).
- **L6.** On the live estate, after the seed: `/api/graph/ontology-refs?repo=estate` lists at least 8 refs, and at least 8 resolve against the live VisionClaw stream at the default cap.

## Verification
`implementation_status: none`. Nothing is built. The evidence for the context was gathered on 2026-10-30. The repository is at `00669b7a2b8bca15702b8173df1dab268144c3ec`, the `codescape` submodule at `c742770ce2a454343d1bba7e1e815c40557cafdf`, the live corpus `visionGraph` at `7b0875f2c`, and both servers were running:
- **Live VisionClaw graph** (`GET :4000/api/graph/data`): 9 688 nodes (9 595 `ontology_node`, 84 `page`, 9 `domain_root`).
  - Metadata keys include `class_iri` (9 595) and `page_iri` (8 310), and no key holds a code path.
  - A regex for source paths across all metadata values found one false positive (`Go/Node.js`).
- **KG page bodies** (`visionGraph/knowledge`, 9 612 files): 6 files contain a `src/` or `scripts/` path, all illustrative (`src/train_model.py`), and none cites an estate file.
  - Frontmatter `sources`, `implements` and `implemented-in-layer` hold reference ids and wiki-links, never code.
  - The 9 GitHub links to `DreamLab-AI/VisionFlow` are images or the repository root.
- **IRIs in code** (`git grep` outside `data/`, `docs/archive`, tests and fixtures): 271 occurrences and 93 distinct IRIs. 20 resolve to a live class, in 27 files (the top one is `src/actors/elevation_actor.rs`, with 22, all doc comments or `#[cfg(test)]`).
  - `@ontology` occurs **0** times in the VisionClaw repository.
- **Docs.** 50 ADRs carry 247 `verified_paths`, and 156 docs carry 1 944 `path:line` citations to 660 files.
  - Live KG nodes with `ADR-` in their label number 2 (`ADR-008`, `ADR-012`, ontology stubs).
  - The Codescape estate snapshot has 0 `document` nodes and 0 `.md` files.
- **Label match** (normalised exact) between ontology labels and Codescape estate nodes: 93 `external`, 173 `struct`, 93 `function` and 32 `class` labels. Samples: `Address`, `Base`, `Block`, `Agent`.
- **Codescape** (`GET :4100/api/health`): repository `estate` first (the default for `/wss`), 94 069 nodes with tiers `[10, 4564, 89495]`.
  - Node `key` values look like `client_src_mocks_filemock`. `path` and `span` are present on file and symbol nodes.
  - `/wss` defaults to tier L1 (`routes/ws.rs:259`).
- **Initial-load cap.** `resolve_initial_node_limit` defaults to 3 000 nodes, sorted by `quality_score` (`types.rs:237-296`, `:560`). The quality at rank 3 000 is 0.72. The seed IRIs' ranks, among 9 679 visible nodes: `bm25` 11, `cosine-similarity` 182, `canonical-json` 378, `cha-cha20-poly1305` 587, `retrieval-augmented-generation` 1 119, `sparql` 1 389, `embedding` 1 427 and `did-nostr` 1 579.
- **Client.** `NodeMetaWire` keeps `metadata_id`, but not `class_iri`, `page_iri` or any `ontology_refs` (`xr-client/rust/src/binary_protocol.rs:173-195`). The topology is parsed in Rust (`:933-937`).
