---
id: ADR-2142
title: The agents demo is one server-side producer that both clients control and both see
date: 2026-10-14
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit:
verified_paths: []
owner: jjohare
review_trigger: any change to the demo's events or ids, to who can start it, or to what reads the agent-action rail
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
7. **Agents without nodes.** The six agents are not graph nodes. The headset already draws a registry agent as an avatar at its work node; the desktop does the same with a marker per agent hovering beside its target and the same point as the beam source (`virtualAgents.ts`, `VirtualAgentMarkers`). A real agent always resolves as a node first, so real agents are unchanged.
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
