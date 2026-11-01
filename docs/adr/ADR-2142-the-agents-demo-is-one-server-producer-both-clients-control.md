---
id: ADR-2142
title: The agents demo is one server-side producer that both clients control and both see
date: 2026-10-14
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: abcf36db1460f68015133ba5196d14cdf4112e19
verified_paths: [src/actors/agent_demo_director.rs, src/actors/agent_roster_feed.rs, src/actors/agent_beam_actor.rs, client/src/features/bots/agentMarkerPass.ts, client/src/store/agentTargetStore.ts, client/src/features/bots/virtualAgents.ts, tests/fixtures/agents/real-agent-frames.v1.json]
owner: jjohare
review_trigger: "Activation (staged → live): the demo started from one client and observed running on the other (desktop and headset) against a backend built from main. Review: any change to the demo's events or ids, to who can start it, or to what reads the agent-action rail"
repo: visionclaw
---

# ADR-2142 — The agents demo is one server-side producer that both clients control and both see

## Context
- The demo existed twice: a desktop producer (`agentDemoMode.ts`) and a headset producer (`agent_demo_director.gd`), each feeding only its own screen, each with its own copy of the loop timings. Starting it on one client showed nothing on the other.
- Agent actions already travel one rail: the server encodes `0x23` action frames and the client coordinator sends them to every client. Real agents' events also feed the KPI tap, which persists every hub event to SQLite.

## Decision
1. **One producer, on the server.** `AgentDemoDirector` is the only source of demo events. It reads the live graph, plays six role agents through the existing work loop (stagger in, work a real node 9-14 s, 60 % follow a real edge, rest 18-26 s, retask) and sends the resulting action frames to the client coordinator, which fans them out exactly as it does real ones. Both clients draw the same agents working the same nodes.
2. **It does not enter the agent-event hub.** Hub events are persisted by the KPI tap and read by other consumers; a synthetic event there would land in durable records. The director therefore writes frames to the coordinator only, and touches neither the graph, the physics nor the store.
3. **Control is one endpoint** `POST /api/agents/demo {running}` (authenticated) and `GET /api/agents/demo` (status). Both clients' buttons call it, and each shows the server's state by polling (4 s), never its own click, so a demo started on one client reads as running on the other and either can stop it. The demo runs until stopped; it does not depend on the client that started it.
4. **Reserved ids.** The six agents are `0x80000000 | 0xD001..0xD006`. Stopping stops the frames, and each client retires exactly those ids at once (`retire_agents` on the headset, `clearVirtual` on the desktop); the 30 s evidence expiry is the backstop. Real agents are untouched.
5. **The synthetic agents play as real ones.** Nothing in a frame says demo; the only provenance is the reserved id range, in code.
6. **Public nodes only.** The frames go to every client, so a target is never a node some client may not see: the director picks only publicly visible nodes (the position stream withholds the rest per session).
7. **Agents without nodes.** The six agents are not graph nodes. The headset already draws a registry agent as an avatar at its work node; the desktop does the same with a marker per agent hovering beside its target and the same point as the beam source (`virtualAgents.ts`, `VirtualAgentMarkers`). ~~A real agent always resolves as a node first, so real agents are unchanged.~~ *Amended 2026-11-01:* real swarm agents are not graph nodes either; they take this same path (see the amendment below).
8. **Legacy removed.** `agentDemoMode.ts`, `agent_demo_director.gd` and their tests, and the duplicate timing tables, are deleted.

## Consequences
- No wire change: the frames are the existing `0x23` batch. The demo needs no new registry entry.
- A server with an empty or fully private graph refuses to start the demo (HTTP 409 with the reason), and the client shows the reason.
- The desktop's demo agents are visible only while their target node is on screen.

## Verification
- Server: 11 director tests (reserved ids, stagger schedule, real targets, captions, keep-alive under the TTL, strictly increasing timestamps, rest and edge-following, high-degree bias, determinism, empty and single-node graphs, filtered edges, wire decode).
- Web: `virtualAgents` (ids, hover ring, TTL, resolver), `agentTargetStore` presence and `clearVirtual`, `useAgentDemo` (server state, toggle, refusal, clears on stop, real agents untouched), pill control.
- XR GUT: status parsing, toggle asks for the opposite of the server's state, button follows the server, stop retires exactly the six ids and keeps real agents, role names.
- Live: start from one client, observe the other (see the change record).

## Re-checked at b4a175412 (2026-10-19)

First verification; every decision holds. Anchors at this commit:
- Decision 1: `AgentDemoDirector` is the only producer (`src/actors/agent_demo_director.rs`, six `ROLES` `:47-85`, timings `:88-99`: stagger 0-9.1 s, work 9-14 s, follow chance 0.6, follow 7-11 s, rest 18-26 s, keep-alive 5 s). It sends `BroadcastAgentActionFrame(encode_agent_actions(..))` to the coordinator only (`:433-435`).
- Decision 2: no hub, graph, physics or store access in the director (header `:5-6`; the only addresses it holds are the coordinator and `GraphStateActor` for reads).
- Decision 3: `GET`/`POST /api/agents/demo` (`src/handlers/api_handler/agents_demo.rs:19`, `:29`; registered `api_handler/mod.rs:132`); POST needs `AuthenticatedUser` and answers 409 on a refusal (`:36-41`). Clients poll every 4 s: the desktop hook is `useAgentDemo(pollMs = 4000)` in `client/src/features/bots/agentDemo.ts:38` (the record's "`useAgentDemo`" lives there, not in its own file); the headset polls `DEMO_POLL_SEC = 4.0` (`xr-client/scripts/graph_parity.gd:64`, `_poll_demo` `:399`).
- Decision 4: reserved ids `0x8000_0000 | 0xD001..0xD006` (`agent_demo_director.rs:35`, `is_demo_id` `:207`); desktop `clearVirtual` on stop (`agentDemo.ts:47`), headset `retire_agents` (`xr-client/rust/src/render_store.rs`); 30 s expiry `VIRTUAL_AGENT_TTL_MS` (`client/src/features/bots/virtualAgents.ts:14`).
- Decision 6: targets are only nodes whose metadata says `public == "true"`, non-agent (`demo_graph_from`, `agent_demo_director.rs:492-515`), which is stricter than the visibility filter and so fails closed.
- Decision 7: `VirtualAgentMarkers` (`client/src/features/visualisation/components/VirtualAgentMarkers.tsx`) and `virtualAgents.ts`.
- Decision 8: `agentDemoMode.ts` and `agent_demo_director.gd` no longer exist anywhere in the tree.
- Verification: the director has 11 tests (`agent_demo_director.rs:545-749`); the web tests are `virtualAgents.test.ts`, `agentDemo.test.tsx`, `agentTargetStore.virtual.test.ts`, `agentOpsDemo.test.tsx`.

## Amendment — 2026-11-01: real swarm agents take the demo pathway (owner decision)

The owner asked for real agents to be aligned with this pathway. Before this, a real swarm agent was drawn from a separate server bots graph (`bots_graph_data`, ids `10000+i` reassigned on every roster update) by a desktop-only overlay that read `/bots/data`. The headset had no roster presence for it at all. That was a second agent path on each client, and R22 found the desktop overlay drew nothing (`Number('task-1')` is NaN).

**Decision (amends D1, D4, D5 and D7).**
- **Two server producers, one door, one client path.**
  - The demo director produces the demo agents.
  - `AgentBeamActor` produces every real agent: it relays agentbox's hub actions, and its `RosterFeed` (`src/actors/agent_roster_feed.rs`) turns swarm rosters into frames.
  - Both write `0x23` frames to the client coordinator only.
  - Both clients embody whatever the frames name: the headset's registry and choreography (ADR-2109), and the desktop's marker beside the target, which is also the beam source.
  - Agents are never graph nodes. Neither client resolves an agent beam source node-first.
- **Real ids.** A real agent's wire id is `0x8000_0000 | 0x0200_0000 | h`, where `h` is the low 25 bits of `uri::content_address(<upstream agent id>)`.
  - The band is the upper half of the 26-bit id space. It is disjoint from the demo range `0xD001–0xD0FF` and from graph node ids, which are compact `0..N-1`.
  - The key is the upstream agent id (the Management API task id), not the DID: a DID is optional and can be shared by many tasks.
  - A collision with another live agent probes upward inside the band. An agent keeps its id while it is on the roster. After a restart an id is the same unless a collision was probed, since probe order is arrival order.
- **State frames.** A `0x23` event whose JSON payload has `state` (`working` | `done` | `retired`, plus `intent` and `name`) is presence and state, not work. Clients draw no beam for it, and add no heat and no drift credit.
  - The feed sends one on spawn and on every state change.
  - It sends a keep-alive with the current state every 5 s, under the 30 s TTL, so a late client learns the roster and a finished agent stays.
  - **Leaving the roster is finishing, and a finished agent holds at the edge until it leaves** (owner decision, 2026-11-01). Live agentbox lists only running tasks (`getActiveTasks`, `management-api/utils/process-manager.js:321`), so a finished task simply disappears. An agent its producer stops naming gets `done` at once. A roster status of `Completed`, `Failed` or `done` gets the same, and so does a status that already said so. The agent stays embodied with `done` keep-alives while the clients run it to the scene edge (30 s, ADR-2135) and then hold it there. **The server has no timer for this and no copy of the 30 s** (`DONE_RUN_MS` is deleted). The feed sends one `retired` only on a departure: an agent that had reported itself done and is no longer named, or the oldest held finished agent once more than `MAX_HELD_FINISHED` (64) are held, which bounds memory and is not a lifetime. The clients remove the agent on `retired` (headset `retire_agents`; desktop registry retire) and the id is freed. An agent named again while held is working again.
  - The 30 s TTL stays as the backstop.
- **Producers own their agents.** Each roster names its source (Management API, MCP, REST `POST /api/bots/update`, mock). A roster retires only the agents its own source spawned and no longer names.
- **Join.** A hub action whose `metadata.taskId` (or `task_id`) names a live roster agent is re-sourced to that agent's wire id, and its target becomes the agent's work node.
  - Every frame of one agent passes one strictly increasing timestamp guard on the hub's clock, `epoch_ms % u32::MAX`.
  - `pubkey` is not a join key: it is the authenticated caller, one identity for many tasks (agentbox `routes/agent-events.js:412`).
  - An action without a task id keeps agentbox's id and is a separate registry agent, as before.
- **`/api/bots/data`** is the feed's roster: one record per agent keyed by its wire id (masked to 26 bits), the collaboration and S5 payment edges between them, and the `chain` view. It is data for panels and docks, never a position source.
  - Deleted: `bots_graph_data`, the static `BOTS_GRAPH`, `requestSwarmTelemetry` (which streamed made-up agent positions for ids `1000+i`), the CQRS `get_bots_graph` read and the mock handler's persisted `AddEdge` calls.
  - `POST /api/bots/mock-agents` now requires authentication.
- **Desktop.** One marker pass per frame fills one map of agent positions. The position is the hover point beside the target (or the scene centre with no target), plus the shared drift (`agentDriftFeed`), plus the ADR-2135 done run. With no target, the hover point is an id-hashed slot on a ring around the scene centre rather than the centre itself.
  - The marker, beam source, label, trail, collaboration edges and payment labels all read that map.
  - The `/bots/data` overlay (`BotsVisualization`, `BotsNode`, `BotsEdges`, `agentDrawnPosition`) and the drawn-positions agent pass are deleted.
  - Done is the registry's explicit `done` state only; a 15 s idle no longer means done.

**Consequences.**
- Both clients embody a real agent the moment it is on the roster, with the same id, and drop it when it leaves.
- **Open: the live join.** agentbox's hook emitter puts `taskId` only on `pre-task` and `post-task` actions (`management-api/hooks/agent-action-hooks.js:86-114`). Its `source_agent_id` is a per-name counter (`getAgentId`, `:27`), not derived from the task. A task's child process gets `TASK_ID` in its environment (`process-manager.js:189`), but nothing shows the hook poster sending it back as `taskId`. Until agentbox puts the Management API `taskId` on every action, a real task's other actions stay on a separate registry agent, so one task can show two bodies. The fix belongs in agentbox.
- The roster no longer reaches the GPU, the graph or Oxigraph.

**Verification.** See the change record. Rust `RosterFeed` tests write `tests/fixtures/agents/real-agent-frames.v1.json` from the real encoder (`the_shared_real_agent_frame_fixture_is_current`); the XR `cargo test` (`a_real_roster_agent_is_embodied_on_the_registry_path_and_retired`) and the desktop vitest decode the same file.

## Amendment — 2026-11-01: hold at the edge until the agent leaves the roster

Supersedes the done-run timer in decision 74 above and in the roster-feed description. The server's `DONE_RUN_MS` retired a departed agent 30 s after its `done`, which is the moment both clients arrive at the edge, so a real agent vanished on arrival. The feed no longer times anything: `retired` follows a roster departure (or the `MAX_HELD_FINISHED` memory bound) only. Tests: `agent_roster_feed.rs` (`a_finished_agent_is_held_with_no_timer_and_never_retired_by_time`, `a_done_agent_that_leaves_the_roster_is_retired_once_and_frees_its_id`, `held_finished_agents_are_bounded_and_the_oldest_goes_first`, `the_spawn_frame_carries_the_producers_target`); desktop `realAgentMarkers.test.ts` (ten minutes of frames, retired only by the frame); headset `test_agent_edge_run.gd` and `a_done_real_agent_is_held_until_a_retired_frame`. The shared fixture's `retired` frame is now produced by a roster departure.

## Re-verification — 2026-11-01 (`abcf36db1`, branch `viz/edge-tracking`)

Re-verified after the R23 fixes. The amendment above replaces the done-run timer: the feed has no timer, `apply` carries the targets, the fixture's `retired` frame is a roster departure. `agentMarkerPass.ts` changes only the `stepAgentOffset` call. Every claim in this record holds. Evidence: `cargo test` in `xr-client/rust` (623 passed), server `cargo test --lib` (1781 passed, `CUDA_VISIBLE_DEVICES=1`), client vitest (1540 passed), GUT on a fresh `.so` (553 passed, 0 failed, `gut_guard` ok). Not run: the headset.
