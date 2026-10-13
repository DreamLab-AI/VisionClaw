# Tracing one agent write across seven layers (TODO-unified X-3, playbook half)

This is the debug playbook for the question "my agent's write did not show up; which layer dropped it?". It follows one agent write through the seven layers named in the X-3 row, with the command at each hop, the code that handles it, and what a failure at that layer looks like.

Read at: VisionClaw `main` HEAD `93f38c6`. All `file:line` references were resolved from `git show HEAD:<path>`. Nothing here was executed against a running stack: commands are given as I would run them and each is marked **verified in code** (the string, route or flag exists at the cited line) or **UNVERIFIED** (I could not confirm it from the code or docs). A log line quoted below is the exact source string; whether your log level prints it is for you to check.

Environment notes: the container is `visionclaw_container` (dev profile, port 4000) or `visionclaw_prod_container` (prod profile, port 3001) per `docker-compose.unified.yml:49,228,215,304`. From inside the agentbox workspace container `docker exec` may be refused (read-only Docker proxy under role isolation); use `docker logs` there and run `docker exec` from the host shell.

## Which write

"An agent write" reaches VisionClaw by three doors. The seven layers are the same; the hops differ.

| Door | Entry | What it carries | Where it ends in VisionClaw |
|------|-------|-----------------|-----------------------------|
| A. Roster poll (live) | VisionClaw polls the MCP hub on `MCP_HOST:9500` every 2 s | Agent list (id, type, status, load) | `GraphStateActor.bots_graph_data` |
| B. REST | `POST /api/bots/update` (also `POST /api/bots/data`) | Agent nodes in JSON | **A process-static `BOTS_GRAPH` only, never the actor tree** (see layer 4) |
| C. Agent-action stream | agentbox pushes `notifications/agent_action` to `/wss/agent-events` | One action per frame | In-process hub, then `AgentBeamActor`, then a `0x23` binary frame to clients |

Layers 5 (CUDA `SimParams`) and 6 (binary position frame) are on the path of any change that reaches the physics graph. I could not verify that a roster write (door A or B) ever reaches the GPU graph; see the note in layer 5.

---

## Layer 1. Nix and compose

Question answered: are the processes that should be running actually up and healthy?

The agentbox side (MCP hub, management API) is built from a Nix flake into the agentbox image and supervised there; VisionClaw is a compose service. Per the workspace notes:

```
supervisorctl status          # agentbox services (run inside the agentbox container)
claude-flow doctor            # agentbox diagnostics
./scripts/launch.sh status    # VisionClaw container status (host shell; launch.sh:1433 -> show_status)
./scripts/launch.sh logs      # follows VisionClaw logs (launch.sh:1399 -> show_logs)
docker ps --format '{{.Names}}\t{{.Status}}' | grep -E 'visionclaw|agent'
curl -fsS http://localhost:4000/api/health       # dev; healthcheck at docker-compose.unified.yml:214-216
curl -fsS http://localhost:3001/readyz            # prod; healthcheck at docker-compose.unified.yml:303-305
```

- `GET /readyz` returns 503 `{"status":"not_ready","reason":...}` when the app reports a degraded subsystem such as Oxigraph (`src/handlers/consolidated_health_handler.rs:463-473`, route at `src/main.rs:1094`). It does not check actors or the MCP hub.
- The MCP endpoint VisionClaw dials comes from `MCP_HOST` (default `agentic-workstation`) and `MCP_TCP_PORT` (default 9500) (`docker-compose.unified.yml:11-12`, `src/app_state.rs:1145-1149`).
- Dev start period is 300 s (`docker-compose.unified.yml:219`); a container marked `starting` for that long is not a failure.

Failure signature: container `unhealthy` or restarting; `/api/health` connection refused; `MCP_HOST` unresolvable from the VisionClaw network (compare with `docker network inspect visionclaw_network`). A wrong `MCP_HOST` shows up at layer 2, not here, so this layer only tells you the processes exist.

UNVERIFIED: the Nix derivation names and the agentbox supervisord program names. I did not read `flake.nix`; treat `supervisorctl status` output as the authority.

## Layer 2. MCP hub

Question answered: did the hub deliver the agent list or action to VisionClaw?

Door A, the poll. `BotsClient` connects over TCP (`src/services/bots_client.rs`), then polls every 2 s (`bots_client.rs:181-232`, interval at `:187`). When the list is non-empty it logs and sends `UpdateBotsGraph` (`bots_client.rs:195` and `:211`).

```
docker logs visionclaw_container 2>&1 | grep -E "MCP server is (not )?reachable|Received [0-9]+ agents from MCP server|BotsClient sending"
```

- Healthy: `Received N agents from MCP server` then `BotsClient sending N agents to graph` every 2 s (`bots_client.rs:195`, `:207`).
- Hub down or wrong host/port: `MCP server is not reachable` (`bots_client.rs:165`, returned as an error at `:167`).
- Empty list: nothing is logged at info; the stored agents are cleared silently ("Clearing stored agents - MCP returned empty list" is `debug!`, `bots_client.rs:218`).
- Query failure: `Failed to query agents from MCP: ...` is `debug!` only (`bots_client.rs:224`). At default log level a failing poll is invisible. Set `RUST_LOG=debug` for `visionclaw_server::services::bots_client` to see it (module path **UNVERIFIED**).

TCP reachability probe from the VisionClaw side (the script's own probe, `scripts/verify-mcp-connection.sh:10`; its hostnames are stale, it still says `multi-agent-container`, so substitute `$MCP_HOST`). Whether `nc` exists in the image is **UNVERIFIED**:

```
docker exec visionclaw_container sh -c 'echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\"}}" | nc "$MCP_HOST" "${MCP_TCP_PORT:-9500}"'
```

There is a second consumer of the roster: `AgentMonitorActor` builds its own agent list and sends `UpdateBotsGraph` (`src/actors/agent_monitor_actor.rs:714`). It suppresses empty rosters to avoid clobbering: `Suppressing empty UpdateBotsGraph (no information; ...)` (`agent_monitor_actor.rs:719`, guard `decide_bots_graph_emit` at `:143`). So two producers write the same `bots_graph_data`; the last one wins.

Door C, the action stream. The upgrade at `/wss/agent-events` (`src/main.rs:1097`) needs a session token, as a Bearer header or `?token=`, checked by `NostrService::get_session` (`src/agent_events/ingest.rs:255-291`); subprotocol `vc-agent-events.v1` (`ingest.rs:54`, `:316`).

- No or bad token: HTTP 401 `Invalid or expired authentication token` (`ingest.rs:284`) or the "no token" 401 at `:291`.
- Frame rejected: `agent-events: malformed frame rejected` (`ingest.rs:230`), `agent-events: non-canonical envelope rejected` (`:226`), `agent-events: binary frames not accepted on ingest socket` (`:237`).
- A working reference producer, with its login and stream steps, is `scripts/dev/agent-beams-driver.cjs` (documented in `docs/how-to/operations/agent-beams-dev-driver.md`). Running it is the quickest way to prove door C end to end.

Note the hub is an in-process `tokio::sync::broadcast` of capacity 256 (`src/agent_events/hub.rs:22-25`). Under load it drops the oldest frames; the consumer logs `AgentBeamActor: hub lagged, N frame(s) skipped — resyncing` (`src/actors/agent_beam_actor.rs:231`, `:247`). Loss under a burst is by design, so a missing single beam is not necessarily a bug.

## Layer 3. NIP-98 and its replay cache

Question answered: did the request authenticate, and was its token spent?

Every mutating `/api` request passes `PublicDemoGuard` then `RbacGate` (`src/main.rs:1110-1121`), which calls `verify_access` (`src/middleware/rbac_gate.rs:270`; `src/utils/auth.rs:191`). Some scopes add `RequireAuth` (`src/middleware/auth.rs:134-170`), which also calls `verify_access`. The NIP-98 path is `verify_access` -> `NostrService::verify_nip98_auth` (`src/services/nostr_service.rs:628`) -> `validate_nip98_token` -> `validate_nip98_token_inner` (`src/utils/nip98.rs:443`).

What the validator checks, in order (`nip98.rs:443-575`): base64, UTF-8, JSON, kind 27235 (`:20`), age (stale: `TokenExpired`; future: `TokenFromFuture`, `:483`), signature, then, last, the single-use claim (`claim_signed_id`, `:573`). Constants: freshness window 60 s (`:169`), replay TTL 120 s (`:178`), 100,000 total entries (`:198`), 1,000 per pubkey (`:215`). The URL the token is checked against is rebuilt from `X-Forwarded-Proto` and `X-Forwarded-Host` first (`src/utils/auth.rs:170-189`), so a token signed for a different host or port will not match.

Show whether it got through:

```
# 401 body names the reason; the dev 'Bearer dev-session-token' path only works in debug builds from loopback with DEV_AUTH_LOOPBACK=1 (src/utils/auth.rs, src/utils/auth.rs `verify_access` and src/settings/auth_extractor.rs:36-46)
curl -i -X POST http://localhost:4000/api/bots/update -H 'Content-Type: application/json' -H "Authorization: Nostr $TOKEN" -d '{"nodes":[]}'
docker logs visionclaw_container 2>&1 | grep -E "NIP-98 (validation failed|auth successful|identity reused)|lacks required|RBAC_GATE"
```

Minting `$TOKEN` needs a NIP-98 signer. No CLI signer is shipped in the repo that I could confirm (the browser uses `computeAuthHeaders`, `client/src/services/api/authInterceptor.ts`, per ADR-2074). **UNVERIFIED**: a standalone command to mint a kind-27235 token. `scripts/dev/agent-beams-driver.cjs` instead logs in to `/api/auth/nostr` for a session token, which is a different realm (ADR-2009).

Failure signatures (the 401 body is `NIP-98 auth failed: <error>`, `src/utils/auth.rs:320-323`):

| Body text (from `Nip98ValidationError`, `nip98.rs:341-379`) | Meaning |
|---|---|
| `Token replayed: this NIP-98 event id has already been used` | The id is in the cache. A client retry that reuses a token fails here, even if the first attempt died further down the stack. Mint a new token per attempt. |
| `Token expired: created Ns ago (max 60s)...` / `Token from the future...` | Clock skew between client and server beyond 60 s. |
| `URL mismatch: expected ..., got ...` | Proxy headers changed the host or port. Compare against `nip98_request_url` (`auth.rs:170`). |
| `Payload hash mismatch` / `...requires the body to be bound` | Body binding. Note every production caller I found passes `None` for the body (`auth.rs`'s `verify_access`, `auth_extractor.rs:154`, `solid_proxy_handler.rs:225`, `memory_cloud_handler.rs:86`, `filter_auth.rs:74`, `http_handler.rs:165`, `speech_socket_handler.rs:222`) and `validate_nip98_token` delegates with `RequiredWhenTagged`, so the payload tag is only compared when both a body and a tag are supplied; I did not trace why. **UNVERIFIED** whether any route enforces `BodyBinding::Required`. |
| `Replay cache at capacity...` / `per-pubkey admission limit exceeded...` | Cache limits above. |
| 403 `Insufficient permissions for this operation` | Authenticated, but the role lacks the level (`auth.rs:283`, `:312`). |

Properties that matter when debugging:

- The replay cache is a process-global in-memory map (`nip98.rs:221`). It is cleared by a restart and does not span replicas (comment at `nip98.rs:205-214`, which points at `docs/SECURITY-profiles.md` invariant 4).
- A spent token is spent even if the request fails afterwards. One request can pass `RbacGate` and then a handler extractor; the second check would see a replay, so the gate leaves the identity in request extensions and later extractors reuse it (`auth.rs:258-290`, `src/settings/auth_extractor.rs:104-118`). If a code path verifies the header a second time without that reuse, you will see `Token replayed` on a request that was never retried.
- `RBAC_GATE_MODE=report` lets a denied request through and logs `RBAC_GATE[report] would DENY ...` at error level (`rbac_gate.rs:284`). If writes succeed that "should" have been refused, check the mode.
- `RBAC_PUBLIC_READS` (default on) lets anonymous reads through (`rbac_gate.rs`, header comment), so a read that works says nothing about auth.
- Door B needs `AuthenticatedUser` from `src/settings/auth_extractor.rs` as well (`bots_handler.rs:191`).

## Layer 4. The Actix actor tree

Question answered: did the message reach `GraphStateActor`, and did it change anything?

Door B stops early. `update_bots_graph` (`src/handlers/bots_handler.rs:190`) logs `Received bots graph update with N nodes` (`:196`), writes into the process-static `BOTS_GRAPH` (`:89`, `:203`) and returns `success: true`. It never sends a message to the actor tree. `get_bots_data` (`:216`) prefers the actor's copy via `GetBotsGraphData` and only falls back to the static when the actor's graph is empty (`:231` logs `Retrieved bots data from graph actor`; `:245` logs `Retrieved bots data from static storage`). So a REST write is invisible whenever the poll (door A) has populated the actor. A 200 from `POST /api/bots/update` does not mean the actor tree saw it.

Door A path:

```
bots_client.rs:211 / agent_monitor_actor.rs:714   do_send(UpdateBotsGraph)       (fire-and-forget)
graph_service_supervisor.rs:1994-2005             forwards to GraphStateActor, else warn
graph_state_actor.rs:1069-1197                    rebuilds bots_graph_data, ids 10000+i
```

```
docker logs visionclaw_container 2>&1 | grep -E "Updated bots graph with|Cannot forward UpdateBotsGraph|Sending graph update with|Suppressing empty UpdateBotsGraph"
curl -s http://localhost:4000/api/bots/data | head -c 400        # nodes served from the actor or the static; the log line above tells you which
curl -s --max-time 5 http://localhost:4000/api/health/physics       # sends GetGraphData to the supervisor with a 3 s timeout (consolidated_health_handler.rs:311-314)
```

- Healthy: `Updated bots graph with N agents and M edges (agent_node_ids=K)` (`graph_state_actor.rs:1191`).
- Supervisor has no child: `Cannot forward UpdateBotsGraph: GraphStateActor not initialized` (`graph_service_supervisor.rs:2002`). Also `GetBotsGraphData: GraphStateActor not initialized` (`:1809`).
- `do_send` is fire-and-forget: a full or closed mailbox drops the message with no error at the sender. UNVERIFIED that any caller logs such a drop.
- `/api/health/physics` returns a `PhysicsSimulationStatus` whose `details` string says whether `GetGraphData` answered in 3 s (`consolidated_health_handler.rs:305-322`); a timeout means the supervisor or `GraphStateActor` mailbox is not draining.

Persistence for graph writes that do reach the actor (`AddNode`, `RemoveNode`, `AddEdge`) is fire-and-forget to Oxigraph: `Failed to persist add_node(..) to Oxigraph store` (`graph_state_actor.rs:362-364`), likewise `remove_node`, `add_edge`. The REST caller got success before the store write finished.

Related: REST handlers that go to the store and skip the actor are inventoried in [actor-bypass-inventory.md](actor-bypass-inventory.md). Writes through those never reach this layer.

## Layer 5. CUDA `SimParams` ABI

Question answered: can the host and device still agree on the parameter struct?

`SimParams` is a 212-byte `repr(C)` struct copied raw to the GPU; the Rust declaration is size-locked at `src/models/simulation_params.rs:227` (`const _: () = assert!(std::mem::size_of::<SimParams>() == 212);`) and the CUDA declaration at `crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu:117` (`static_assert(sizeof(SimParams) == 212, "SimParams size mismatch with Rust");`). Reference: ADR-2028 and `docs/GPU-wire-abi.md`. These guard total size only; a same-size reorder or type change is not caught (`docs/GPU-wire-abi.md`, the note under the two `assert`s).

```
grep -n "size_of::<SimParams>() == 212" src/models/simulation_params.rs
grep -n "static_assert(sizeof(SimParams)" crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu
curl -s http://localhost:4000/api/health/physics
docker logs visionclaw_container 2>&1 | grep -E "ForceComputeActor: (InitializeGPU received|UpdateGPUGraphData received|Skipping compute)"
```

- Failure at build time: the Rust build fails on the `const` assertion (the exact rustc wording is **UNVERIFIED**) or `nvcc` reports `SimParams size mismatch with Rust`. This layer therefore fails the build, not a request.
- Failure at run time, request-visible: no motion and no error. `ForceComputeActor: Skipping compute — no graph data uploaded to GPU yet (waiting for InitializeGPU)` (`src/actors/gpu/force_compute_actor.rs:1798`, logged at `debug!` once per 60 skipped frames). The upload handlers are `InitializeGPU` (`:3195`) and `UpdateGPUGraphData` (`:3279`).
- The feature-flag word is rebuilt from constraint residency on every step (`derive_dispatch_feature_flags` in `src/models/force_channels.rs`, per `docs/GPU-wire-abi.md`), so a flag you set through a settings write can be overwritten before it reaches the device.

Where I could not follow the write: `UpdateBotsGraph` fills `bots_graph_data` (`graph_state_actor.rs:1174-1176`). I found no code that copies `bots_graph_data` into the graph that is uploaded to the GPU (the uploads above take the main graph). **UNVERIFIED** how agent nodes (ids 10000+) get positions from physics, if they do. Treat "the agent shows in `/api/bots/data` but does not move" as a layer 4 to 5 gap until someone traces it.

## Layer 6. Binary wire unpacker

Question answered: does the client decode what the server encoded?

Server encode: `encode_node_data_extended_with_sssp` (`src/utils/binary_protocol.rs:426`), called from the broadcast path in `src/actors/client_coordinator_actor.rs:475-480` and the socket handlers. The record is 52 bytes (`binary_protocol.rs:90-93`, a `const` assert that fails the build); the V5 envelope prefixes an 8-byte sequence (`:98-101`). Wire node ids are 26 bits; an over-range id is masked and logged: `NODE ID OVERFLOW: ... exceeds 26-bit NODE_ID_MASK, remapped to ...` (`:197`, error level, ADR-2024). That log line means two nodes may now alias on screen.

Server decode errors (`binary_protocol.rs:588-640`): `Unknown protocol version: N` (`:620`), `Data size N is not a multiple of V3 wire item size 52` (`:633`).

Client decode, `parseBinaryNodeData` (`client/src/types/binaryProtocol.ts:163`), reached from `handlePositionUpdate` (`client/src/store/websocket/binaryProtocol.ts:307`, parse at `:320`). Failures degrade silently to "no nodes", which is why this layer hides drops:

| Console message | Line | Meaning |
|---|---|---|
| `Declining V2 position frame...` | `types/binaryProtocol.ts:192` | Stale sender. |
| `Declining frame for sibling opcode 0x..` | `:219` | A non-position frame reached the position decoder. |
| `Declining frame with unknown protocol version N` | `:230` | Version byte not 3 or 5 (4 goes to the delta decoder). |
| `Binary data length (N bytes) is not a multiple of node size (52)` | `:243` | Truncated or wrong-stride frame; only whole nodes are used. |
| `Skipping corrupted node at index i` | `:315` | Non-finite or invalid record. |
| `Error parsing binary node data:` | `store/websocket/binaryProtocol.ts:322` | Exception; the frame is dropped. |
| `Unsupported framed-header version` | `services/BinaryWebSocketProtocol.ts:313` | Framed header carries a version outside {3, 5}. |

Commands (browser console on the client page, or via the `browser` skill; storage keys from `client/src/utils/clientDebugState.ts:10-11`; data logging needs both `debug.enabled` and `debug.data` (`isDataDebugEnabled`, `:134`); the accepted stored value format is **UNVERIFIED**):

```
localStorage.setItem('debug.enabled', 'true'); localStorage.setItem('debug.data', 'true')   // reload the page
```

```
cd client && npx vitest run src/features/analytics/__tests__/wireOffsets52.spec.ts     # pins the 52-byte offsets (not run here)
cargo test -p visionclaw-protocol                                                        # Rust fixtures, crates/visionclaw-protocol/src/wire_fixtures.rs (not run here)
```

Door C leaves the position path: `AgentBeamActor` projects actions and sends a coalesced `0x23` AGENT_ACTION frame (`src/actors/agent_beam_actor.rs:13-27`, `encode_agent_actions`). The client decodes `0x23` in `client/src/services/binaryProtocol/agentMessages.ts`. A `0x23` frame given to `parseBinaryNodeData` is declined as a sibling opcode (row `:219` above).

Do not use `tests/test_wire_format.rs` as evidence: it still asserts a 26-byte format.

## Layer 7. WAC inheritance

Question answered: if the write targets a Solid pod, did the access-control list allow it?

This layer applies to writes under `/solid/*` (an agent writing to its pod), not to `/api`. Routes are registered only when the crate is built with the `solid-pod-embed` feature; without it `configure_routes` registers none and `/solid/*` returns 404 (`src/handlers/solid_proxy_handler.rs:1821-1824`; with the feature, the scope is at `:1781`).

Flow in `handle_solid_proxy` (`solid_proxy_handler.rs:311`):

1. `authenticate_request` (`:246`) -> `extract_user_identity` (`:188`) validates a NIP-98 `Authorization: Nostr` header (`validate_nip98_token(..., None)` at `:225`; same replay cache as layer 3). On failure it logs `NIP-98 token validation failed: <error>` (`:238`) and treats the caller as anonymous.
2. Anonymous and `SOLID_ALLOW_ANONYMOUS` unset: 401 `Authentication required` (`:249-255`). With anonymous allowed, a bad token silently becomes an anonymous request, and WAC then decides.
3. `load_acl_for_path` (`:891`) looks for the resource's own `.acl`, then walks up parent containers. An ACL found on an ancestor is marked `inherited` (`inherited_from_ancestor`, `:935`), so only its `acl:default` rules apply, not its `accessTo` (WAC section 4.2). A pod root that grants public read through `accessTo ./` alone therefore covers the root and not its children.
4. `evaluate_access` (`:343`) decides. Anonymous and denied -> 401 `Authentication required` (`:354`). Authenticated and denied -> 403 `Access denied` with `WAC denies <mode> access to <path>` in `details` (`:359-361`).

```
curl -i -X PUT http://localhost:4000/solid/<npub>/path/file.json -H "Authorization: Nostr $TOKEN" --data-binary @file.json    # $TOKEN as in layer 3
curl -i http://localhost:4000/solid/health                                                                                  # route exists (solid_proxy_handler.rs:1783)
docker logs visionclaw_container 2>&1 | grep -E "NIP-98 token validation failed|Solid LDP:|WAC denies"
```

Failure signatures: a 403 whose `details` reads `WAC denies Write access to /<npub>/...` is an ACL decision (check `/.acl` and the nearest ancestor `.acl` for `acl:default`); a 401 on a token that worked once is usually layer 3 (replay) and shows up as the `NIP-98 token validation failed` log line, not as a WAC message. The inheritance rule is pinned by tests in `wac_inheritance_tests` (`solid_proxy_handler.rs:1827-1876`).

---

## What does not exist yet

There is no end-to-end request id. A write cannot be followed from the NIP-98 token to the wire frame and the client console by one identifier. This is the code half of X-3, which is not done and is not attempted here. Today's partial pieces, none joined to the others:

- **HTTP request id.** The browser client generates `X-Request-ID` (`client/src/services/api/authInterceptor.ts:90`). The server reads it in exactly one place that matters here: `verify_access` (`src/utils/auth.rs:196-201`), falling back to a fresh UUID, and uses it only in log lines inside that function. It is not stored in request extensions, not passed to the handler, and not put in any actor message. `src/handlers/settings_handler/enhanced.rs:47,265,391` also read it for their own logs. There is no middleware that assigns one to every request.
- **NIP-98 event id.** Known inside the validator (`nip98.rs:573`) and kept only in the replay cache. It is not returned to the caller and not logged on success (`NIP-98 auth successful for pubkey` logs the pubkey, `nostr_service.rs:657`).
- **Actor correlation id.** `UpdateNodePositions` carries `correlation_id: Option<MessageId>` (`src/actors/messages/graph_messages.rs:58-62`), with a `MessageTracker` (`src/actors/messaging/message_tracker.rs:85`). It is a fresh UUID minted inside the actor system (`message_id.rs`), not derived from the HTTP request. `UpdateBotsGraph`, `AddNode`, `AddEdge` and the other messages on this page carry none.
- **Wire frame.** The 52-byte record and the V5 `broadcast_seq` have no field for a request id, and both sizes are locked by compile-time assertions (ADR-2018, ADR-2024, ADR-2057). A request id on the wire is a protocol change that needs a design first, as the TODO row says.
- **Client console.** Nothing on the client correlates a received frame with the request that caused it.

Until that exists, the method is this page's: go layer by layer, using each layer's own log line as the only evidence. Two gaps in that method are worth fixing first because they hide failures: the `debug!`-level poll failure at layer 2 and the silent `do_send` drops at layer 4.
