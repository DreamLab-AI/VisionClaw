---
id: ADR-2066
title: Remove unreachable handler and service surfaces
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: b00c28a0d766c8cf46cd00b100dab60ef2dd74a4
verified_paths: []
owner: jjohare
review_trigger: any future `.configure()` call, actor `Handler` impl, or `app_data` registration that would make one of the removed surfaces reachable again — re-derive the ADR from scratch rather than un-deleting this code
repo: visionclaw
domain: BASELINE-architecture
---

# ADR-2066 — Remove unreachable handler and service surfaces

## Context
Phase 1 diagram VC-28.8 flagged `QuicTransportServer`
(`src/handlers/quic_transport_handler.rs:240`) as constructed nowhere, routed
nowhere, only re-exported. Phase 1 diagram VC-28.2 flagged
`handle_ragflow_chat` (`src/handlers/ragflow_handler.rs:193`) as registered on
no route. Phase 1 diagram VC-20.7 flagged the entire Phase-7 inference stack
(`src/handlers/inference_handler.rs`, `src/application/inference_service.rs`,
`src/events/inference_triggers.rs`) as extracting an `InferenceService`
app_data that nothing ever constructs, so every `/api/inference/*` route
500'd. Phase 1 diagram VC-27.10 flagged a second, unused
`MultiMcpVisualizationMessage`/`SwarmInfo`/`GlobalTopology` struct family in
`src/services/agent_visualization_protocol.rs` duplicating the name of the
live, actix-`Handler`-backed enum in `multi_mcp_visualization_actor.rs`. A
dead `storage_rx` field on `SolidNotificationWs`
(`src/handlers/solid_proxy_handler.rs`) was never fed by any sender.

## Decision
Dead code is deleted, not ported or stubbed; where a module partially
overlapped with a real caller, only the unused part is deleted:
- `handle_ragflow_chat` deleted; its now-unused `RagflowChatRequest`/
  `Responder` imports removed. `RagflowChatResponse` stays — it is still used
  by the live `EnhancedRagFlowHandler::chat_enhanced`.
- `quic_transport_handler.rs` trimmed to just `PostcardNodeUpdate`/
  `PostcardBatchUpdate` (the two types `fastwebsockets_handler.rs` — a real,
  separate caller — imports directly). `QuicTransportServer`,
  `QuicClientSession`, `QuicServerConfig`, `CongestionController`,
  `ControlMessage`, `TopologyNode`/`TopologyEdge`, `PostcardDeltaUpdate`,
  `encode_postcard_batch`, `decode_postcard_batch`, `calculate_deltas` and the
  `quinn`/`rustls`/`rcgen` Cargo dependencies they alone needed are removed.
  `src/handlers/mod.rs`'s `pub use quic_transport_handler::{...}` block is
  removed (nothing used that re-export path); `pub mod quic_transport_handler`
  stays so `fastwebsockets_handler` can still reach it directly.
- `inference_handler.rs`, `inference_service.rs`, `inference_triggers.rs` and
  their `mod`/`pub use` lines deleted outright — verified first that nothing
  constructs `InferenceService` or calls `register_inference_triggers`
  anywhere. `src/main.rs`'s `.configure(configure_inference_routes)` call
  (main.rs is another lead's file) has since been removed by that lead in the
  same working tree, so no compatibility no-op shim is retained in
  `handlers/mod.rs` — a shim would only be the same dead code in a new shape.
  The live Whelk reasoning path, `GitHubSyncService::run_post_sync_reasoning`,
  had no connection to this stack and is untouched.
- The duplicate `MultiMcpVisualizationMessage` family and the four
  `AgentVisualizationProtocol` methods that built it
  (`create_discovery_message`, `create_agent_update_message`,
  `create_topology_update`, `create_performance_analysis`) plus their
  exclusive private helpers are deleted. Their only caller,
  `tests/examples/multi_mcp_integration_demo.rs`, is not compiled by cargo
  (lives under `tests/examples/`, not `examples/`, with no `[[test]]`/
  `[[example]]` entry) and itself references a nonexistent `visionclaw_ext`
  crate. `GlobalPerformanceMetrics` and `Bottleneck`, which are also defined
  in this file, are kept — both have real, live callers in
  `multi_mcp_visualization_actor.rs` and/or `multi_mcp_agent_discovery.rs`.
- `SolidNotificationWs::storage_rx` and the stale "storage watch is handled at
  the handler level" comment above it are removed; the actor only ever
  tracked subscriptions for filtering.

## Consequences
`/api/inference/*` now 404s via the emptied `configure_inference_routes` call
site removed from main.rs, instead of 500ing on a missing app_data extractor —
an honest failure mode for a route with no backing service.
`fastwebsockets_handler.rs` and `image_gen_handler.rs`'s live callers are
unaffected; `RagflowChatResponse`, `GlobalPerformanceMetrics`, and `Bottleneck`
remain exactly where their real callers expect them. Anyone wanting inference,
QUIC/WebTransport, or a ragflow direct-chat surface again must design and wire
it fresh, including the app_data registration this stack always lacked.

## Verification
Ran on the uncommitted working tree above `verified_commit`; must be
re-verified at the landing commit.

```
$ grep -rn "handle_ragflow_chat" --include="*.rs" .   # before deletion: only its own def + internal log lines, no route registration
$ grep -rn "QuicTransportServer" --include="*.rs" . | grep -v quic_transport_handler.rs | grep -v handlers/mod.rs
(no output)
$ grep -rn "PostcardBatchUpdate\|PostcardNodeUpdate" src/handlers/fastwebsockets_handler.rs
(9 real usages — kept)
$ grep -rn "InferenceService::new\|InferenceService {" --include="*.rs" .
src/application/inference_service.rs   # only its own definition
$ grep -rn "register_inference_triggers" --include="*.rs" .
src/events/inference_triggers.rs:243 (definition) + events/mod.rs re-export   # no call sites
$ grep -n "configure_inference_routes" src/main.rs
(no output — the call was removed from main.rs during this working session)
$ grep -rn "MultiMcpVisualizationMessage" --include="*.rs" . | grep -v services/agent_visualization_protocol.rs
src/actors/multi_mcp_visualization_actor.rs (its own, separate, live enum — no cross-import)
$ grep -n "GlobalPerformanceMetrics" src/actors/multi_mcp_visualization_actor.rs src/services/multi_mcp_agent_discovery.rs
(multiple real usages — kept)
$ grep -n "storage_rx\|StorageEvent" src/handlers/solid_proxy_handler.rs
(no output after removal)

$ cargo check -p visionclaw-server
    error: could not compile `visionclaw-server` (lib) due to 4 previous errors
    # All 4 errors are in src/services/owl_extractor_service.rs (AnnotatedOntology/
    # read_functional horned-owl API drift, tracked separately under ADR-2064 —
    # another lead's in-flight work on this shared working tree, not this ADR's
    # concern). Verified none touch ragflow_handler.rs, quic_transport_handler.rs,
    # handlers/mod.rs, inference_handler.rs, inference_service.rs,
    # inference_triggers.rs, application/mod.rs, events/mod.rs,
    # solid_proxy_handler.rs, or agent_visualization_protocol.rs:
    #   awk '/^error/{getline; print}' <log>
    #   --> src/services/owl_extractor_service.rs:164:67
    #   --> src/services/owl_extractor_service.rs:174:59
    #   --> src/services/owl_extractor_service.rs:180:37
    #   --> src/services/owl_extractor_service.rs:169:9
    # This ADR's removals introduce zero new errors.
```

## Addendum — 2026-09-05: `bots_visualization_handler` advertised opcodes and its fake data source

Phase 1 diagram **VC-27.11** recorded two defects that were first mis-filed against
`src/services/agent_visualization_processor.rs`. Establishing ground truth located both entirely in
`src/handlers/bots_visualization_handler.rs`; the estate lead, who owned that file, was stood down,
so the fix landed here.

**FIX — `pause_updates` / `resume_updates` now do what they advertise.** Both opcodes were bare
`debug!()` calls, so a client that asked for a pause kept receiving the 16 ms position stream. The
actor gains a `paused: bool`, the two opcodes set it, and the `run_interval` closure returns early
while it is set. Same defect class as the `Unsubscribe` no-op the estate lead removed under ADR-2089;
here the opcode pair has real senders, so the resolution is FIX rather than REMOVE.

**REMOVE — the fake roster source.** `get_real_agent_data()` was documented as returning "real agent
data from AppState if available" while unconditionally returning `vec![]`, and had no callers. It is
deleted: a helper that names itself a data source and returns a constant is worse than no helper.

**Deliberately NOT faked — the empty initialisation roster.** `send_init_state` still reports zero
agents, and that is now explicit in the code rather than disguised behind the deleted helper. A real
roster is reachable (`AppState.bots_client`, `app_state.rs:382`, exposes
`get_agents_snapshot() -> Result<Vec<Agent>>`, `bots_client.rs:231`), but wiring it needs two things
this change will not invent: an async fetch spawned into the actor context, and an
`Agent -> AgentStatus` mapping that does not exist. `AgentStatus`
(`crates/visionclaw-domain/src/types/claude_flow.rs:19`) requires `profile: AgentProfile`,
`active/completed/failed_tasks_count`, `success_rate` and a `timestamp` that `Agent`
(`bots_client.rs:16`) does not carry. A faithful conversion needs a decided contract; inventing
defaults would produce a roster that looks authoritative and is not. Recorded as PROPOSED follow-on
work with the exact gap, in preference to a plausible fabrication.

### Verification (addendum)

```
$ cargo check -p visionclaw-server --lib          # exit 0; zero errors or warnings in this file
$ grep -n "paused" src/handlers/bots_visualization_handler.rs
# field, both opcode assignments, and the run_interval early-return
$ grep -rn "get_real_agent_data" src/ --include=*.rs
# (no matches — removed)
```

## Amendment 2026-11-06 — the fastwebsockets server and its wire types

This record kept `quic_transport_handler.rs` only as the home of `PostcardNodeUpdate` /
`PostcardBatchUpdate`, which `fastwebsockets_handler.rs` imported. `FastWebSocketServer`
was itself never constructed or routed: nothing outside its own file referenced it. Both
files are now removed under this record's rule, with `tests/high_perf_networking_tests.rs`
(entirely commented out) and the direct dependencies only they used (`fastwebsockets`,
`postcard`, `hyper`, `hyper-util`, `http-body-util`, `http`). The server was the only reader of
the shared `node_sssp` map, so the map, `SetNodeSSSP` and `ShortestPathActor`'s write into it
are removed too (PROTOCOL-registry 0.1.19). In the same pass `ProtectedSettingsActor` (two
copies, `src/actors/` and `crates/visionclaw-actors/`), the `ProtectedSettings` model it
alone used, and `AppState::get_api_keys` / `get_nostr_user` (no caller since the API-key GET
reads `NostrService`) were removed.

```
$ grep -rn "FastWebSocketServer\|SetNodeSSSP\|ProtectedSettingsActor\|get_api_keys(" src crates --include=*.rs
# only nostr_handler's own get_api_keys route handler remains
```
