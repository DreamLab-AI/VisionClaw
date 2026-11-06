---
id: ADR-2184
title: A client-side columnar node table, built from the existing graph load, drives colour, size and legend by any column
date: 2026-11-05
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: c7c78c88d6fbeaa91d873dc80e20d8ed163faffb
verified_paths: []
relates: [ADR-2003, ADR-2018, ADR-2161, ADR-2164, ADR-2179, ADR-2182, ADR-2183, ADR-2185]
owner: jjohare
review_trigger: "The first sample of the live /api/graph/data payload (column inventory); a measured DuckDB-WASM bundle cost; a proposal to add a dedicated table endpoint."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source
acceptance_boundary: "Node shape read in crates/visionclaw-domain/src/models/node.rs and metadata.rs, server metadata keys by grep over src and crates at main 4adab22f0; the 52-byte record's analytics fields read in crates/visionclaw-wire/src/lib.rs. Licences read from the npm registry on 2026-11-05: @duckdb/duckdb-wasm MIT, apache-arrow Apache-2.0, @uwdata/mosaic-core and @uwdata/vgplot BSD-3-Clause. The live /api/graph/data payload was NOT sampled (the container stopped), so which Metadata fields reach the client is unverified. The REST handler (src/handlers/api_handler/graph/mod.rs:217-312) was read for a visibility gate and none was found; that private nodes actually reach it was not tested."
---

# ADR-2184 — A client-side columnar node table, built from the existing graph load, drives colour, size and legend by any column

## Context
- **Colour has no user control.** Node colour is hard-wired to `getDomainColor` (`GemNodes.tsx:555`, `InstancedLabels.tsx:173`, `ClusterHulls.tsx:69`). There is no colour-by or size-by control for knowledge nodes, and no legend for the graph.
- **Attributes exist on the server, in three places:**
  - `Node` typed fields: `node_type`, `group`, `size`, `weight` and `owl_class_iri` (`crates/visionclaw-domain/src/models/node.rs:35-76`);
  - a string map, `metadata: HashMap<String,String>`, with keys including `domain`, `maturity`, `quality_score`, `source_domain`, `public` and `owl_class_iri`;
  - the page `Metadata` struct: `file_size`, `hyperlink_count`, `last_modified`, `last_commit` and `change_count` (`crates/visionclaw-domain/src/models/metadata.rs:7-30`).
  Whether each of these reaches the client in `/api/graph/data` is unverified.
- **The wire carries analytics.** The 52-byte record already carries cluster, community, centrality, anomaly and SSSP per node (ADR-2018), live from the GPU.
- **Cosmograph's data layer.** It keeps nodes as an Apache Arrow table queried by DuckDB-WASM through Mosaic. Encodings and widgets are queries over that table.
- **Isolation headers.** nginx sets COOP `same-origin` and COEP `require-corp` (`nginx.conf:87-88`), so any WebAssembly or worker asset must be served from our own origin.

## Decision
1. **The table is built on the client from data already delivered.** The client builds one Apache Arrow node table in a worker. Its sources are:
   - the existing `/api/graph/data` / `initialGraphLoad` node objects;
   - the analytics fields of the live position stream, joined by wire id and refreshed as frames arrive.
   **No new endpoint and no new read path.** The table therefore exposes nothing beyond what the client already receives.
   A dedicated `/api/graph/table` Arrow endpoint is rejected for now: it would be a second read path that has to re-apply every gate.
   - **Precondition: the REST load must be gated.** The `/wss` `initialGraphLoad` path applies the `PUBKEY_VISIBILITY_FILTER` drop-set (`src/handlers/socket_flow_handler/position_updates.rs:64-72`). No visibility gate was found in the REST `/api/graph/data` handler (`src/handlers/api_handler/graph/mod.rs:217-312`), which filters by population and linked-page only, and the desktop loads from REST. Before this record is accepted, it must be shown, or made true, that the REST load applies the same visibility filter (ADR-2003). Whether private nodes reach that handler today is unverified.
2. **Column typing.** Typed fields keep their types. `metadata` strings are typed per column by a declared schema (number, date, category, text), not by guessing per value. Unknown keys are text.
3. **Missing columns are added as new keys, not new formats.** If a needed column (for example `last_commit`) is not on the load, the server adds it as a new key in the existing node object, under the ADR-2164 precedent. That change is recorded in `docs/PROTOCOL-registry.md`.
4. **Encoding.**
   - Colour-by takes any categorical or numeric column, with categorical and sequential palettes.
   - Size-by takes any numeric column, with a clamp.
   - A legend is always shown for the active encodings.
   - The defaults reproduce today's look (domain colour, current sizing).
   - Encodings write per-node attribute buffers that ADR-2182's sprites and ADR-2186's labels read.
5. **Query engine.**
   - DuckDB-WASM (MIT) with Mosaic (BSD-3-Clause) is the query engine for ADR-2185's widgets.
   - It is loaded lazily, the first time a widget or SQL view opens. It is self-hosted under the client's own origin (no CDN, because of COEP) and runs in a worker.
   - Encoding alone does not require DuckDB: it reads the Arrow table directly.
   - The shipped bundle's size is measured and recorded before acceptance; the npm package (149 MB unpacked) contains several bundles.

## Consequences
- **What users can do.** Encode the graph by maturity, recency, centrality, community or anything else on the node, with a legend. This is the precondition for ADR-2185.
- **Payload.** The graph load may grow by the added columns. That cost is measured when columns are added.
- **Ownership.** The schema in Decision 2 becomes a maintained artefact. It lives beside `client/src/types/` and is generated from Rust where the type exists (`cargo run --bin generate_types`).

## Verification
None yet. Each decision has its own check:
- **D1 precondition:** an integration test shows that a private node owned by another pubkey is absent from REST `/api/graph/data`, as it is from `/wss` `initialGraphLoad`.
- **D1:** a test shows that a node withheld by the visibility filter is absent from the table.
- **D2:** a fixture covers schema typing.
- **D4:** screenshots show colour-by maturity and size-by centrality with their legends.
- **D5:** a recorded bundle size, and a network trace showing DuckDB assets fetched only on first widget open, from our own origin.
- **First:** sample the live `/api/graph/data` and record which columns are present.
