---
title: Embody a swarm and stream agent-action beams (dev driver)
description: How to register agent nodes in VisionClaw and stream 0x23 AGENT_ACTION beams into every connected client (web and headset) from a script, and which auth realm each step needs.
category: how-to
tags: [agents, beams, agent-events, nip98, xr, dev]
updated-date: 2026-09-02
difficulty-level: intermediate
---

# Embody a swarm and stream agent-action beams

Use this when you want to *see* agent activity in the graph (web client or
headset) without waiting for the agentbox hook pipeline: it registers a swarm
as agent nodes and streams `agent_action` notifications straight into
VisionClaw's `/wss/agent-events` ingest. Every connected client receives the
resulting `0x23 AGENT_ACTION` frames and renders them as beams from the agent
capsule to the node it touched.

Script: [`scripts/dev/agent-beams-driver.cjs`](../../../scripts/dev/agent-beams-driver.cjs),
swarm definition: [`scripts/dev/agent-beams-swarm.json`](../../../scripts/dev/agent-beams-swarm.json).

```bash
# from the agentbox container (nostr-tools + ws come from agentbox's management-api)
VISIONCLAW_URL=http://visionclaw_container:4000 \
node scripts/dev/agent-beams-driver.cjs 15        # minutes to stream
```

## What it does, step by step

| Step | Call | Auth realm |
|---|---|---|
| 1. Login | `POST /api/auth/nostr` with any validly signed Nostr event (kind 22242, content `login`) | none; the response is wrapped: `{success, data:{user, token, expires_at, features}}` |
| 2. Register agents | `POST /api/bots/update` `{nodes:[Agent…], edges:[]}` | session realm: headers `X-Nostr-Pubkey` + `X-Nostr-Token: <token>` |
| 3. Stream actions | WebSocket `/wss/agent-events?token=<token>` sending `notifications/agent_action` JSON-RPC | session token (query or header) |

Posted agents are not graph nodes (ADR-2142 amendment, 2026-11-01). The server's
real-agent producer gives each one a stable wire id in the real-agent band
(`0x8200_0000 | hash`) and embodies it on both clients through `0x23` presence frames.
The id is visible as `nodes[].id` (masked) in `GET /api/bots/data`. The REST roster
owns its agents: a later post that omits one finishes it. The agent goes `done`, runs 30 s to the scene edge, and is then retired. A notification joins a
posted agent only when its `event.metadata.taskId` equals that agent's `id`.
Otherwise its `source_agent_id` (the driver sends `1000 + index`) is a separate
registry agent that hovers beside its target. Each
notification's `event.source_agent_id` must be an agent id and
`event.target_node_id` a raw graph node id; `action_type` 0–5 = query, update,
create, delete, link, transform. `params.message_type` is `35` (`0x23`) and
`params.protocol_version` is `2`.

## Where to watch

- Web client: the **Agents** dock (key `9`) shows the swarm and canaries; the
  KPI tile "Augmentation Ratio" counts agent actions received on
  `/wss/agent-events`; beams render in `TransientBeamsLayer`.
- Headset: the Godot client decodes the same `0x23` frames
  (`render_store.rs`, `agent_beam.gdshader`).
- Server: `agent-events: ingest socket open (session_pubkey=…)` in the backend
  log; rejections would log as warnings from `agent_events::ingest`.

## Why the session realm and not NIP-98

`POST /api/bots/update` validates a NIP-98 header twice on one request
(`utils::auth::verify_access` and then `settings::auth_extractor`), so the
single-use replay cache (ADR-2002) rejects the second check with
`401 Token replayed`. Until that is fixed, sign in once and use the session
token. `Authorization: Bearer <token>` on its own returns
`403 Authentication required`; the session realm is the two `X-Nostr-*`
headers. `Bearer dev-session-token` only works with `DEV_AUTH_LOOPBACK=1`
(ADR-2012), which the dev compose does not set.

## The production path (agentbox hooks)

Real agents do not use this script. Their tool calls are recorded by
`config/hooks/trajectory-recorder.cjs`, posted to agentbox's
`POST /v1/agent-events/emit`, and forwarded by
`management-api/utils/agent-event-ws-subscriber.js` to this same ingest.
Two defects in that path were fixed on agentbox `main` (`d392bd4c2`) and take
effect at the next image rebuild: the emit route rejected every numeric id
(Fastify's type coercion made a `oneOf integer|string` match both branches),
and the forwarder was never started by `server.js`. There is also no
`agent_list` provider behind VisionClaw's bots relay, so agent *nodes* only
exist when something posts them to `/api/bots/update`; the hook path should
do that for each registered session.
