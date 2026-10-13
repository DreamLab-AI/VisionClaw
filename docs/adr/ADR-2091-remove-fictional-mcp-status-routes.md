---
id: ADR-2091
title: Remove the two multi-MCP REST routes that returned fabricated state
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: b00c28a0d766c8cf46cd00b100dab60ef2dd74a4
verified_paths: []
owner: jjohare
review_trigger: a client needing multi-MCP server status or a manual discovery trigger, which must then be implemented against services/multi_mcp_agent_discovery.rs rather than reinstated from this ADR
repo: visionclaw
domain: BASELINE-architecture
lineage: ADR-2083 (the real discovery cadence these routes purported to expose), ADR-2090 (the credential fix on the sibling WebSocket route in the same file)
---

# ADR-2091 — Remove the two multi-MCP REST routes that returned fabricated state

## Context

`src/handlers/multi_mcp_websocket_handler.rs` served two live REST routes under
the `/multi-mcp` scope that reported state they never gathered:

- `GET /multi-mcp/status` → `get_mcp_server_status` returned a **hardcoded JSON
  literal**: a two-server list naming `claude-flow` at `localhost:9500` with
  `"is_connected": true` and `"agent_count": 4`, and `ruv-swarm` at `:9501` with
  `"is_connected": false`, plus `"total_agents": 4`. Only the timestamp was
  computed. It never consulted the discovery service.
- `POST /multi-mcp/refresh` → `refresh_mcp_discovery` logged *"Manual MCP
  discovery refresh requested"* and returned `{"success": true, "message":
  "Discovery refresh initiated"}` without initiating anything.

Both took `_app_state` — bound, underscore-prefixed, unused — which is the
signature of a handler written against an intent that was never wired.

The `"is_connected": true` is the part that makes this worse than dead code. A
monitoring consumer polling `/multi-mcp/status` is told a claude-flow server is
connected with four agents, regardless of whether any MCP server is running. A
route that returns nothing is a gap; a route that returns confident fiction is a
false negative in an operator's health picture.

Found by vc-core during Phase 2 and routed to this lead as owner of the file.

## Decision

Both routes and both handlers are **removed**, along with their registrations in
`configure_multi_mcp_routes`. The `/multi-mcp` scope keeps only `/ws`.

A REST surface reports state it actually gathered, or it does not exist. A
handler that fabricates a plausible response is not a placeholder for a future
implementation — it is a defect that is harder to find than the missing feature
would have been, because nothing fails.

If a client later needs multi-MCP server status or a manual discovery trigger,
it is implemented against `src/services/multi_mcp_agent_discovery.rs`, which
holds the real `discovered_agents` / `server_statuses` state and, since ADR-2083,
polls on the configured per-server cadence. It is not reinstated from this
record.

## Consequences

- `GET /multi-mcp/status` and `POST /multi-mcp/refresh` now 404. No caller
  exists, so no consumer breaks — see Verification.
- The estate loses a route that could have been mistaken for MCP health
  monitoring during an incident. That is the point of the removal.
- `crate::ok_json` became unused in this file with `refresh_mcp_discovery` gone
  and its import was removed, so the change introduces no new warnings.
- The sibling WebSocket route `/multi-mcp/ws` in the same file is untouched here;
  its credential defect was fixed separately under ADR-2090. Keeping the two
  changes in separate records keeps the security fix reviewable on its own.

## Verification

Ran on the **uncommitted working tree** above SHA
`b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`; `verified_paths` is empty because the
tree is uncommitted, and verification must be re-run at the landing commit.

- **No caller anywhere.**
  `grep -rn 'get_mcp_server_status\|refresh_mcp_discovery' src/ client/ xr-client/`
  before the change returned exactly four hits, all inside this one file: the two
  definitions (`:842`, `:871`) and the two registrations (`:885`, `:886`). No
  client, XR-client, test or other Rust module referenced either.
- **No client route reference.**
  `grep -rn 'mcp-server-status\|mcp/status\|refresh.*discovery\|refresh-discovery' client/src xr-client`
  → no matches.
- **Registration is in this file, not `main.rs`.** The routes were registered by
  `configure_multi_mcp_routes` (this file), which `src/main.rs:1108` merely
  `.configure(...)`s. `src/main.rs` belongs to another lead and was **not**
  touched; the scope now registers `/ws` alone.
- `cargo check -p visionclaw-server --message-format=short` →
  `grep -cE '^src.*error'` → **0**, and no warning attributable to this file
  (the transient `unused import: crate::ok_json` my removal created was fixed in
  the same change).
- `cargo test -p visionclaw-server --lib multi_mcp` → **ok. 5 passed; 0 failed**
  (the ADR-2083 discovery-interval suite, unaffected and still green).

## Addendum 2026-10-12: the owner decision to implement discovery (TODO-unified S-4(1))

The review trigger above (a client needing multi-MCP state, implemented against `services/multi_mcp_agent_discovery.rs`) is met by an explicit owner decision on 2026-10-12: retire was offered and "wire real discovery" was chosen, knowing that no client in this repository consumes `/multi-mcp/ws` today. What was built, and nothing more:

- `AppState.multi_mcp_discovery` holds one `MultiMcpAgentDiscovery`, initialised at boot with its default servers. Only `claude-flow` is enabled by default (the same `CLAUDE_FLOW_HOST`/`MCP_TCP_PORT` that `BotsClient` polls, ADR-2084); `ruv-swarm` and `daa` stay disabled until `RUV_SWARM_*` / `DAA_*` are set, so an unconfigured server is not polled and does not log a failure every few seconds.
- Polling follows a subscriber count: `attach()`/`detach()` change it synchronously on connect and disconnect, and the idempotent `sync_running()` starts or stops the loop to match, so reordered tasks cannot leave polling on with nobody attached. Each start bumps an epoch and wakes the loop, so a release-then-acquire inside one interval (a page reload) ends the old loop rather than running two beside `BotsClient` on the same port. Stopping clears the cached state so the next client is not shown the previous session's agents.
- `send_discovery_data` no longer simulates anything (the `fastrand` failures and the 100 ms sleeps are gone). It sends a `discovery_data` frame (`servers`, `agents`, `topology`, `stats`) filtered by the client's subscription, built by the pure `build_discovery_frame`. Nothing is sent at connect; a per-connection one-second tick pushes a frame whenever a new discovery cycle has completed, and `request_discovery` still answers immediately. A failed first cycle now records a disconnected server entry (previously the map stayed empty), so when no server is connected the client gets an `error` frame judged on the state just read.
- The ADR-2094 health gate now folds the discovery service's own per-server `is_connected` instead of probing `localhost:8080+i` services that were not the ones discovery polls. One health task per connection is kept.
- `Handler<RequestDiscoveryData>` used to log a debug line and send nothing, so no client ever received discovery data; it now triggers the send.

Not done, and still true: no client consumes the frame and its shape is defined only by `build_discovery_frame` and its test, so the first consumer should treat that shape as provisional. `request_agents` still carries its own circuit-breaker simulation and was not changed here. The `ruv-swarm` and `daa` servers have no confirmed endpoint in this estate (agentbox lists `ruflo-daa` as "purpose unknown").
