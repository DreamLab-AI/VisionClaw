---
id: ADR-2094
title: Fail closed on the Management API credential and cache the MCP health verdict
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: b0bc275f6501aae7751b85a72ce15fe1e730e7e8
verified_paths: []
owner: jjohare
review_trigger: A second in-process consumer of MANAGEMENT_API_KEY, any change to validate_security_env_vars, or a new gate on MCP service health.
repo: visionclaw
domain: IDENTITY-authority-chain
lineage: extends ADR-06 §D1 (compile-time ALLOW_INSECURE_DEFAULTS gate); SECURITY-profiles Invariant 6 (posture asserted before the listener binds); diagram docs/diagrams/visionclaw/27-agent-integration-mcp-relay.md:435,240
---

# ADR-2094 — Fail closed on the Management API credential and cache the MCP health verdict

## Context
Two defects in the agent-integration plane, both recorded on diagram
`docs/diagrams/visionclaw/27-agent-integration-mcp-relay.md`.

`AgentMonitorActor::new` read `MANAGEMENT_API_KEY` itself and, on absence, fell
back to `String::new()` behind a single `warn!`. `AppState` boot validated the
same variable through `validate_security_env_vars` and returned `Err` on a
missing, insecure-default, or under-16-character key. One secret, two policies,
and the actor held the lax one: a weak key was accepted silently and a missing
one produced a client authenticating with an empty bearer token.

`has_healthy_services` spawned a detached probe task on **every** call and then
returned `true` unconditionally. The answer carried no information, the gates it
fed (`send_discovery_data`, `request_performance`) were inert, and each call
leaked a task for the process lifetime.

## Decision
**One validator for one secret.** `validate_security_env_vars` is `pub(crate)`
and is the only crate-internal reader of `MANAGEMENT_API_KEY`.
`AgentMonitorActor::new` calls it. The outcome is decided by the pure function
`decide_management_api_credential(validation, insecure_defaults_allowed)`:

| validation | dev relaxation armed | outcome |
|---|---|---|
| `Ok(key)` | either | client enabled with that key |
| `Err(_)` | `true` | client **disabled** (`None`), logged at `error!` |
| `Err(e)` | `false` | `panic!` — a boot error inside `AppState::new` |

No path yields an empty-string credential. `management_api_client` is
`Option<ManagementApiClient>`; `poll_agent_statuses` returns early on `None`
rather than polling unauthenticated. The relaxation is the **existing**
mechanism only — `insecure_defaults_allowed()` is compile-gated to
`debug_assertions`/`dev-auth` builds exactly as
`socket_flow_handler::http_handler::is_insecure_defaults_allowed`, so a release
binary contains no path to `true` (SECURITY-profiles §Flag matrix).

**One health task per connection.** `MultiMcpVisualizationWs` carries
`healthy_services: Arc<AtomicBool>`, optimistic until the first probe.
`start_health_monitor`, called once from `started()`, owns a single task that
probes every 30s and publishes the verdict. It holds a `Weak` handle on the
cell, so a dropped actor terminates it. `has_healthy_services` is a pure atomic
load. `perform_health_checks` and its `run_interval` are deleted.

## Consequences
- A deployment whose `MANAGEMENT_API_KEY` is missing or weak now fails to boot
  instead of running with a broken agent-telemetry client. This is a **breaking
  change for misconfigured environments** and is the intent.
- The health gate reports the truth, so `send_discovery_data` can now refuse a
  client when the estate is genuinely down — previously impossible.
- Task count per WebSocket connection is bounded at one and falls to zero on
  disconnect, replacing unbounded growth proportional to request volume.
- `AgentMonitorActor::new` can panic. It is only ever called from
  `AppState::new` during boot, before the listener binds, so the blast radius is
  a refused start.

## Verification
`cargo check --workspace --all-targets` exit 0.
`cargo test -p visionclaw-server --lib` — 9 tests in
`actors::agent_monitor_actor::tests` (4 new: `valid_key_enables_the_client`,
`invalid_key_is_a_boot_error_when_fail_closed`,
`invalid_key_disables_the_client_under_dev_relaxation`,
`no_decision_path_yields_an_empty_credential`) and 6 in
`handlers::multi_mcp_websocket_handler::tests` (all new, including
`all_services_unhealthy_is_not_usable` — which fails against the old
constant-`true` gate — and `monitor_stops_when_the_client_is_dropped`) pass.
Verification ran on the uncommitted working tree above commit
`b0bc275f6501aae7751b85a72ce15fe1e730e7e8`.

## Addendum 2026-10-12

The cached-health half of this record still holds (one monitor task per connection, a cached verdict, no per-call spawn), but its probe set changed: the monitor no longer probes `MONITORED_SERVICES` on `localhost:8080+i`. It folds the discovery service's own per-server `is_connected`. See the ADR-2091 addendum of the same date (TODO-unified S-4(1)).
