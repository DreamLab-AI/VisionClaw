---
id: ADR-2159
title: The XR client holds one backend connection per content source
date: 2026-10-30
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 00669b7a2b8bca15702b8173df1dab268144c3ec
verified_paths: []
owner: jjohare
review_trigger: the tokio/gdext two-client check failing or passing; a third content source; Codescape gaining NIP-98 (its P6) and presence; any change to the position acknowledgement (ADR-2141); the frame-budget allocator changing its inputs
repo: visionclaw
domain: XR-client
depends_on: [ADR-2141, ADR-2143]
amends: []
relates: [ADR-2135, ADR-2157, ADR-2158, ADR-2161, ADR-2164]
---

# ADR-2159 — The XR client holds one backend connection per content source

Plan: `docs/proposals/spatial-rooms/PLAN.md` §4 (VisionClaw), §5 (P3b). Trial E3 in ADR-2158 needs this.

## Context
- The client has one graph socket URL and one backend mode (`xr-client/scripts/graph_scene.gd:166`, `:625-629`, `_connect_from_env` at `:1515`). `_switch_backend` is exclusive: it abandons in-flight requests, clears fold, cycle, query and pin state, and reconnects (`:909-926`; `docs/XR-client.md`, "Switch hygiene").
- One `BinaryProtocolClient` and one `PresenceClientNode` are built per scene (`graph_scene.gd:479-480`). The position stream is acknowledged per client (ADR-2141), so the acknowledgement sequence belongs to a connection.
- `BackendCaps.for_mode` gates features globally for the active backend (`xr-client/scripts/backend_caps.gd`). Codescape grants no pins, no physics write, no memory cloud and drops `nodeDrag` (`docs/XR-client.md`, "Codescape as a second server").
- Three volumes (ADR-2158) draw from two sources at once: the VisionClaw stream (knowledge and ontology, split by node kind) and Codescape.
- The headset frame budget is 100 000 triangles and 50 draw calls, held by one allocator with a 5 % reserve (`xr-client/rust/src/frame_budget.rs:1-23`). Its inputs today are layers, not volumes.
- The gdext network layer runs on one process-wide tokio runtime of two worker threads (`xr-client/rust/src/runtime.rs:12-20`). Whether two clients behave on it has not been tested.
- **Accepted by the operator on 2026-10-30** for implementation, together with ADR-2158 ("bring in the new spatial containers and linked knowledge and code graphs").

## Decision
The XR client holds **one `BackendConnection` per content source**, replacing the exclusive `_switch_backend`. A source is identified by (source node, dataspace) for a federated source (ADR-2161), and by the server for the local VisionClaw stream and for Codescape. Two spaces of the same remote node are two connections, because their ACLs differ; a connection's `id↔urn` tables are keyed by (node key, dataspace, stream generation) and a reconnect starts a new generation.

1. **Per connection.** Each `BackendConnection` owns its `BinaryProtocolClient`, its acknowledgement sequence (ADR-2141), its render store, its `BackendCaps` and its colour palette. It is bound to the volumes it feeds. Two connections never share a store or a sequence.
2. **Per volume.** Capability gating moves from global to per volume. A drag works in a volume fed by VisionClaw and is refused locally, with a HUD notice, in a volume fed by Codescape; the refused `nodeDrag` is never sent.
3. **Presence.** One presence socket per *joined room*. In trial E3 that is the trial's existing authenticated VisionClaw presence room (today's `/ws/presence` room, whatever room the client already joins), not a space-bound room: E3 needs nothing from ADR-2157. Bound-room admission applies to that socket only after the later ADR-2157 rollout. Codescape's presence shim is not joined.
4. **Frame budget.** The 100k-triangle and 50-draw-call budget stays one budget. The allocator splits it per visible volume, and a hidden volume stops packing. The split rule is part of the implementation, tested against the benchmark in `xr-client/perf/`.
5. **Precondition, verified before any of this is built.** Two `BinaryProtocolClient` instances share the gdext tokio runtime without starving each other or the main-thread pump, and the gdext statics tolerate two instances. This is a test (two clients, one runtime, both receive and acknowledge), run before P3b starts. It is **unverified at the revision named above**.

Rejected alternatives:
- **Keep `_switch_backend` and toggle.** One graph at a time; the trial needs two sources visible together.
- **Proxy Codescape through the VisionClaw server.** Violates the Codescape isolation boundary (ADR-2143) and couples two release cycles.
- **One shared render store keyed by source.** Node ids are ephemeral 26-bit values per server (ADR-2024); two servers' ids collide in one store.
- **A second process per source.** Doubles the headset's CPU cost and splits the frame budget across processes that cannot see each other.

## Consequences
- **Easy:** each source reconnects, backs off and fails on its own; losing Codescape leaves the knowledge volume running.
- **Hard:** state that today is a scene scalar (`_graph_ws_url`, `_caps`, `_presence_ws_url`) becomes a collection. Every `_gate` call site (fifteen call sites in `graph_scene.gd` at this revision) must say which volume it concerns. The HUD backend toggle no longer means one thing.
- **Cost:** two sockets, two stores and two acknowledgement streams on the headset. Measured against E8 (90 Hz on the VIVE with all three volumes visible) before activation.
- **Foreclosed:** a single global capability table; sharing node ids across sources.
- **Revisit** if the two-client check fails (then the runtime moves to a per-client or larger pool, or the second source is read through a single multiplexed connection), if a third source appears, or if Codescape gains presence worth joining.
- **Counter-example that blocks activation:** a Codescape volume that sends `nodeDrag`.
- **Follow-on:** `docs/XR-client.md` lists this record as being implemented. The Transport section's single-backend description stays current until the code lands. Phase-1 file ownership and the `BackendConnection` API are in `docs/proposals/spatial-rooms/XR-PHASE1-CONTRACT.md`.
- **Link resolution needs both connections.** The code-to-concept links of ADR-2164 are joined in the client across two connections' tables. A link is drawn only while both connections are up and in their current stream generation.

## Activation prerequisites
`depends_on` lists code dependencies only. This ADR does not depend on ADR-2158; ADR-2158 depends on this one (`AUDIT-DECISIONS.md` C9).

The two-client tokio test (decision 5) is the **first** build step, ahead of any backend or volume *integration*. Build order is distinct from activation.

**Refined at acceptance (2026-10-30).** New, self-contained files may be written and tested headless in parallel with the preflight, against fixture content sources: `SpatialVolume`, the native shim and the link layer. Until it passes, nothing is wired into `graph_scene.gd`, and no second `BinaryProtocolClient` is constructed outside the preflight test itself. Activation needs that test to have passed and the E8 frame-budget measurement. It is step 2 of the cross-repo order, ahead of ADR-2161. Read-only enforcement for federated sources is a server duty (ADR-2161 decision 4); `BackendCaps` here is UX only.

## Verification
`implementation_status: none`, `activation_status: inactive`. Accepted on 2026-10-30. Nothing is built, and the decision 5 preflight has **not** been run.

**Re-read at `00669b7a2b8bca15702b8173df1dab268144c3ec`.** The citations are unchanged:
- `xr-client/scripts/graph_scene.gd:82` (global `_caps`), `:91-94` (`_binary_client`, `_backend_mode`, `_presence_client`), `:166-167` (`_graph_ws_url`, `_presence_ws_url`), `:479-480`, `:858-875` (`backend_supports`, `_gate`), `:877-906` (`_abandon_backend_requests`), `:909-926` (`_switch_backend`) and `:1515` (`_connect_from_env`);
- `xr-client/rust/src/frame_budget.rs:1-23`;
- `xr-client/rust/src/runtime.rs:12-20`.

No `BackendConnection` exists in `xr-client/`. The topology frame is parsed in Rust (`binary_protocol.rs:933-937`, `classify_graph_text`), so a per-connection metadata table belongs on the Rust side of each client.

**First read at `e74bcddc772d47a45b1971156a2bab540eaa027c`** (proposal), with the same citations.
