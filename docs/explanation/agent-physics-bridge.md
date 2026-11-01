---
title: Agent-to-Physics State Bridge
description: How agents reach the scene. They are not graph nodes and take no part in the physics. Presence and state ride 0x23 frames, and both clients draw the agent from the registry those frames feed.
category: explanation
difficulty-level: advanced
updated-date: 2026-11-01
---

# Agents and the physics: there is no bridge

An agent is **not a graph node**. It has no position on the position stream, no spring force, and no
place in the GPU simulation. The older design in which agents entered the force graph as flagged
nodes (and `BotsNode`, `BotsVisualization` and `BotsDataContext` drew them) is gone. Governing
records: ADR-2142 (the agent pathway), ADR-2135 (the done run) and ADR-2109 (the headset registry).

## What carries an agent

| Concern | Carrier |
|---|---|
| Presence and state (`working`, `done`, `retired`, plus `intent` and `name`) | `0x23 AGENT_ACTION` frames whose JSON payload has a `state` key. Clients draw no beam for them |
| Work (which node it acts on) | an ordinary `0x23` action frame; the beam runs from the agent to the target node |
| Roster fields (type, status, load) | `/bots/data`, the server's `RosterFeed` view (`src/actors/agent_roster_feed.rs`), keyed by the agent's bare wire id |
| Identity | wire id `AGENT_NODE_FLAG | REAL_AGENT_BASE | h` for a real agent (`h` from the content address of its upstream id), `0xD001..0xD0FF` for the demo. Both are disjoint from compact graph node ids |

Two producers write the frames: the roster feed (real agents) and the demo director. Every frame for
one agent passes one strictly-increasing timestamp guard.

## What each client does with them

- **Desktop.** `agentTargetStore` is the one registry. `agentMarkerPass` derives one marker per
  registered agent per frame: beside its target node, or on the centre ring when it has none. The
  marker map is the only agent position path: markers, nameplate, trail, collaboration edges, payment
  labels and the beam source read it. Agents are not in the drawn-positions array.
- **Headset.** `RenderStore`'s agent registry and `agent_choreography.gd` embody the agent. The beam
  starts at the embodiment anchor and nowhere else; an agent without an anchor has no beam.

## The done run

A finished agent eases out to the scene edge in `AGENT_EDGE_SECONDS` = 30 s and holds there until it
leaves the roster, at which point the server sends `retired` and both clients remove it. No client
timer ends the hold. The closed form and the shared constants are in ADR-2135.

## Where to look

- `client/src/features/bots/agentMarkerPass.ts`, `agentTargetStore.ts`; `client/src/features/graph/utils/agentNudge.ts`
- `xr-client/scripts/agent_choreography.gd`; `xr-client/rust/src/render_store.rs` (`build_beam_buffer`)
- `src/actors/agent_roster_feed.rs`, `src/actors/agent_beam_actor.rs`
- Fixture shared by the server, XR and desktop tests: `tests/fixtures/agents/real-agent-frames.v1.json`
