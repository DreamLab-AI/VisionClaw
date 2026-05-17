# ADR-059: Bi-directional URI-keyed agent activity channel (VisionClaw side)

**Status:** Proposed
**Date:** 2026-04-28
**Author:** VisionClaw platform team
**Supersedes:** — (initial bi-directional design; agent_monitor_actor REST polling remains until Phase 2)
**Related:**
- VisionClaw ADR-048 (dual-tier KGNode/OntologyClass + BRIDGE_TO)
- VisionClaw ADR-050 (sovereign-model: visibility + owner_pubkey + opaque_id + bit-29 privacy flag)
- VisionClaw ADR-058 (MAD → agentbox migration, side-by-side ports)
- Agentbox ADR-013 (canonical URI grammar — `did:nostr:<pubkey>`, `urn:agentbox:<kind>:[<scope>:]<local>`)
- Agentbox ADR-005 (pluggable adapter architecture — events slot)
- Agentbox ADR-008 (privacy filter / OPF middleware)
- Agentbox ADR-009 (embedded Nostr relay; future durable channel)
- **Pair:** Agentbox ADR-014 (bi-directional graph-state ingress for agent reaction — the agentbox side of this contract)

## TL;DR

VisionClaw exposes one new WebSocket endpoint, `/wss/agent-events`, and accepts an additive event envelope that carries the existing numeric IDs **plus** optional `source_urn`, `target_urn`, and `pubkey` fields (per agentbox ADR-013 grammar). Inbound `agent_action` events drive a hybrid **beam + gluon** spring effect for `duration_ms` against the existing GPU semantic-forces kernels — no new CUDA. Outbound user-interaction events (`focus`, `select`, `hover`, `drag`) push back to agentbox so agents can react to user attention. Identity is phased: optional pubkey from Phase 1; fail-closed NIP-26 delegation enforcement deferred to a successor ADR. Server-side visibility filtering at the binary encoder is staged separately.

## Context

The current agent → graph path is **REST polling**: `agent_monitor_actor.rs:169-171` calls `Management API:9090` every 3 s, builds `AgentStatus` records, and renders capsule nodes in the GPU spring system. This is one-way and lossy:

1. **Pubkey is dropped.** Inbound events carry `source_agent_id` as a 32-bit hash (`management-api/utils/agent-event-publisher.js:165-189`); the user/operator/agent identity disclosed in agentbox via `did:nostr:<pubkey>` (ADR-013) never reaches VisionClaw.
2. **No URN.** Both `source_agent_id` and `target_node_id` are ints. The viewer (S12) cannot follow links to external surfaces (skills registry, pod credentials, ADRs) because the canonical name is missing.
3. **No reaction path.** Agents in agentbox cannot observe user activity (which node is focused, which is selected). Agent positions are server-driven and ignore user attention.
4. **Bridge mismatch.** Agentbox's `management-api/utils/agent-event-bridge.js` tries to connect TCP `127.0.0.1:9500`, expecting a VisionClaw MCP listener that does not exist; VisionClaw's MCP server actually listens on `:3001` inside `visionflow_container`. The reconnect storm in `tab 4 pane 0` is the visible artefact.

ADR-050 already provides the identity primitives on the storage side (`KGNode.owner_pubkey`, `KGNode.visibility`, `KGNode.opaque_id`, bit-29 opacification). Agentbox ADR-013 provides the URN grammar on the producer side. The wire format is the missing piece.

## Decision

VisionClaw introduces a single bi-directional channel and a single additive event envelope. Six concrete commitments follow.

### 1. Transport

A new WebSocket endpoint `/wss/agent-events` mounts in `src/main.rs` next to `/wss` (binary positions). Subprotocol token: `vc-agent-events.v1`.

- Frames are JSON text by default. A `binary=true` query param negotiates the existing 0x23 binary frame (agent_event_publisher.js:165-189) for parity with agentbox.
- One socket per session. Authentication uses the same `RequireAuth` extractors as `/api/...` (NIP-98 if `NIP98_OPTIONAL_AUTH=true`, else session cookie + CSRF). The authenticated `pubkey` (if any) becomes the **session pubkey**, surfaced through Phase 5 visibility filtering.
- Real MCP TCP on port 9500 is **not** added; the `agent-event-bridge.js` connect target is changed to `ws://visionflow_container/wss/agent-events` instead.

The Nostr-relay durable channel (agentbox `[sovereign_mesh].relay`, ADR-009) is reserved for cross-session state and signed authority grants in Phase 5; the hot path is WebSocket-only.

### 2. Event envelope (additive, backward-compatible)

```jsonc
// inbound: agent → VisionClaw
{
  "version": 3,
  "type": "agent_action",
  "id": 12345,
  "timestamp": 1714312345678,
  "source_agent_id": 7,                                      // legacy int, retained
  "source_urn": "did:nostr:abc...",                          // optional, ADR-013
  "target_node_id": 4242,                                    // legacy int, retained
  "target_urn": "urn:visionclaw:kg:<hex-pubkey>:<sha256-12-hex>",  // optional, ADR-050
  "action_type": 1,                                          // 0..5 per AgentActionType
  "duration_ms": 250,
  "pubkey": "abc...",                                        // optional did:nostr hex
  "metadata": { /* free-form */ }
}
```

The legacy fields (`source_agent_id`, `target_node_id`, `action_type`, `duration_ms`) keep their semantics from `agent-event-publisher.js:11-18` and `agent_visualization_protocol.rs:6-28`. The new fields (`source_urn`, `target_urn`, `pubkey`) are **optional in Phase 1 and Phase 2**; their absence falls back to existing rendering. They become **required in Phase 5** when fail-closed identity attribution is enforced.

### 3. Outbound user-interaction events (first slice of bi-directionality)

Phase 2 ships outbound user-interaction events on the same socket. These are **transient, durational, and tied to the live UI session** — they are NOT persisted to Neo4j and do NOT touch ADR-050 storage.

```jsonc
// outbound: VisionClaw → agent
{
  "version": 1,
  "type": "user_interaction",
  "kind": "focus" | "select" | "hover" | "drag",
  "session_id": "uuid-v4",
  "session_pubkey": "abc...",                                // optional did:nostr hex
  "target_node_id": 4242,
  "target_urn": "urn:visionclaw:kg:...",                     // present if known
  "duration_ms": 1500,                                       // expected lifetime
  "timestamp": 1714312345678
}
```

`focus` fires when a node enters the camera centre band (>1.5 s dwell, hysteresis-debounced). `select` fires on click/tap. `hover` fires on raycast ≥ 250 ms. `drag` fires for the duration of an interactive grab. The agentbox subscriber consumes these per Agentbox ADR-014.

### 4. Spring-system semantic — hybrid beam + gluon

When an `agent_action(action_type, target_node_id, duration_ms)` arrives:

1. **Beam (visual edge)**: a transient edge `(agent_node)-[:ACTION { action_type, started_at }]->(target_node)` is appended to the spring graph for `duration_ms`. The renderer maps `action_type` → colour:

   | action_type | name | colour |
   |---|---|---|
   | 0 | QUERY | blue (#3b82f6) |
   | 1 | UPDATE | yellow (#facc15) |
   | 2 | CREATE | green (#22c55e) |
   | 3 | DELETE | red (#ef4444) |
   | 4 | LINK | purple (#a855f7) |
   | 5 | TRANSFORM | cyan (#06b6d4) |

   These match the colour conventions in `agent-event-publisher.js:11-18` so agentbox's `/v1/agent-events/types` endpoint stays the source of truth.

2. **Gluon (transient force)**: the agent's existing capsule node has its `class_charge` modulated for `duration_ms` so the GPU semantic-forces kernel (`semantic_forces_actor.rs:30-175`) pulls the agent toward `target_node_id`. The kernel and 176-byte struct layout are unchanged; only the per-node `class_charge` buffer is updated for the affected agent.

3. **Despawn**: a single `agent_action_despawn_actor` reaps expired transient edges and zeroes the modulated charge after `duration_ms`. The reap pass is bounded (≤ 1 ms / tick) and runs on the existing actor scheduler.

This deliberately reuses the existing GPU pipeline. No CUDA changes. No new buffers in `unified_gpu_compute`.

### 5. Identity attribution — phased

| Phase | Pubkey behaviour | Server enforcement |
|---|---|---|
| 1 | optional in payload; ignored by renderer | none |
| 2 | optional; renderer tints capsule by `hash(pubkey)` colour when present | none |
| 3 | optional; rendered + recorded in agent_action audit table | none |
| 4 | required when payload claims to mutate ADR-050 owned KGNodes | reject `target_urn` mismatch |
| 5 | required + signed + NIP-26 delegation chain validated | fail-closed; deferred to **ADR-061** |

Phase 5 is out of scope for this ADR. ADR-061 will own the NIP-26 delegation grammar and signature verification (depending on agentbox ADR-013 §R2 scope-bearing rules).

### 6. Server-side visibility filtering at the binary encoder

Today the binary frame ships every node, with bit-29 set on private nodes (ADR-050). This wastes bandwidth and lets a determined client correlate opaque IDs across sessions. Phase 4 introduces:

- The session pubkey (from `RequireAuth`) is attached to the WS connection context.
- `socket_flow_handler` (or successor) filters at frame build time:

  ```
  WHERE n.visibility = 'public'
     OR n.owner_pubkey = $session_pubkey
  ```

- Filtering is **fail-closed**: missing session pubkey ⇒ public-only graph. This matches ADR-050 §Visibility transitions.

This is documented for completeness; the change ships as a separate small ADR (**ADR-060: Owner-pubkey filtered binary encoder**) so it can land independently.

## Phasing

| Phase | Deliverable | Scope of code change | Risk |
|---|---|---|---|
| 1 | Additive payload fields (`source_urn`, `target_urn`, `pubkey`) on existing inbound REST poll path; `agent_visualization_protocol.rs` envelope extension; agentbox `agent-event-publisher.js` populates new fields. | ~30 lines each side. | Low. Backward-compatible. |
| 2 | New `/wss/agent-events` handler in VisionClaw; agentbox switches `agent-event-bridge.js` connect target from `tcp://127.0.0.1:9500` to `ws://...`/wss/agent-events`. Beam + gluon reaper actor. | ~200 lines server, ~100 lines agentbox. | Medium. New socket type. |
| 3 | Outbound `user_interaction` events from client → server → WS broadcast → agentbox subscriber (Agentbox ADR-014). | ~150 lines client + 80 server. | Low–medium. |
| 4 | Server-side visibility filter at binary encoder (**ADR-060**). | ~80 lines, single handler. | Medium. Test matrix grows. |
| 5 | Mandatory + signed identity + NIP-26 delegation (**ADR-061**). | TBD. | High. New crypto path. |

Phase 1 + Phase 2 land within one sprint. Later phases are independently scheduled.

## Consequences

**Positive.**

- Agentbox's existing 6-action-type emission surface (`agent-event-publisher.js:11-18`) requires no rewrite — only an additive payload.
- The hybrid beam + gluon visual maps directly onto the existing semantic-forces GPU kernel; no new CUDA, no new buffers.
- ADR-050 ownership data finally enters the wire format via optional URN, unblocking the URI resolver (`src/handlers/uri_resolver_handler.rs`) to do real Neo4j lookups in Phase 1+ work.
- The agent-event-bridge ECONNREFUSED storm is resolved without standing up a TCP listener that VisionClaw doesn't want to maintain.
- User-interaction outbound events make agents legibly user-aware — capsules visibly drift toward the user's focus for the duration.

**Negative.**

- Two new structures to keep in sync: `AgentActionEvent` (VisionClaw side) and the JSON-RPC notification (agentbox side). Mitigation: agentbox `agent-event-publisher.js` remains the canonical schema source; VisionClaw mirrors it in `src/agent_events/schema.rs` (new module).
- Phased identity means Phase-1 events with no pubkey are indistinguishable from spoofed ones for ~3 phases. Mitigation: visibility filter (Phase 4) is server-side, so unauth'd writes can't clobber ADR-050 owner-private nodes regardless of payload contents.
- Beam edges live alongside ADR-048 `EDGE`/`BRIDGE_TO`/`SUBCLASS_OF`/`RELATES` types but are **transient** and **not persisted to Neo4j**. The renderer must distinguish persistent vs transient edges — handled by a new `transient: bool` flag on the `Edge` struct and skipped by the `load_all_edges` query in `neo4j_graph_repository.rs:431-440`.

**Reversible?** Yes for Phases 1–3 (new fields + new endpoint, additive). Phase 4 (visibility filter) reversibility requires keeping the unfiltered code path behind a feature flag for one release.

## Open questions

1. Should `user_interaction` events also accept an inbound mirror form, so agentbox can echo synthetic interactions for testing? (Recommend yes, behind dev-only header.)
2. Beam colour for `action_type` outside 0–5: should the renderer reject the frame or default to grey? (Recommend default-grey + warn-log.)
3. Phase-2 backpressure: if the WS client falls behind and the server queue exceeds N frames, drop oldest or coalesce by `target_node_id`? (Recommend coalesce-by-target — the visual is duration-based, last-write-wins is correct.)

## References

- Code:
  - VisionClaw: `src/actors/agent_monitor_actor.rs:169-420`, `src/services/agent_visualization_protocol.rs:6-227`, `src/actors/multi_mcp_visualization_actor.rs:33-120`, `src/actors/gpu/semantic_forces_actor.rs:30-175`, `src/handlers/uri_resolver_handler.rs:1-100`, `src/uri/mod.rs:1-42`, `src/middleware/auth.rs:1-150`, `src/handlers/socket_flow_handler.rs` (Phase 4)
  - Agentbox: `management-api/utils/agent-event-publisher.js:11-224`, `management-api/utils/agent-event-bridge.js:1-150`, `management-api/routes/agent-events.js:51-475`, `management-api/lib/uris.js:72-232`, `management-api/middleware/auth.js:33-99`
- Wire format colour palette: `management-api/utils/agent-event-publisher.js:11-18` (canonical)
- Bit-29 privacy flag: ADR-050 §Opaque ID + Visibility, lines 98-130
- Spring kernel struct: `semantic_forces_actor.rs:30-175` (176-byte SemanticConfigGPU)
