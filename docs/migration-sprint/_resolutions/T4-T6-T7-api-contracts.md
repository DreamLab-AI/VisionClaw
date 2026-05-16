# T4 / T6 / T7 — API Contract Resolutions

Status     : Proposed
Date       : 2026-05-16
Owner      : api-contract-validator (QE agent)
Affects    : ADR-02, ADR-06, ADR-07, ADR-10, ADR-12
Baseline   : `radical-rollback` @ `41979d33e`

| Tension | Summary |
|---|---|
| **T4** | WebSocket URL space is unowned; no section enumerates the full set. |
| **T6** | Section 7 deletes 5 control-plane routes without confirming "no external consumer". |
| **T7** | Outbound agent-click action envelope is referenced by name but never specified. |

---

## T4 — WebSocket endpoint enumeration is unowned

### Current state

References scattered across sections, none enumerate. **Baseline has seven
registered WS endpoints** (`src/main.rs:694-698`, plus
`bots_visualization_handler.rs:507`, `multi_mcp_websocket_handler.rs:925`,
`api_handler/analytics/mod.rs:193`, `api_handler/ontology/mod.rs:1366`).
**Sprint docs introduce three more** (`/ws/xr-presence`, the canonical
agent-telemetry path replacing `/api/visualization/agents/ws`, and the
enterprise-events stream from ADR-10 D5).

Two doc-vs-baseline inconsistencies need correcting:
- ADR-06 D5's CSP rationale cites `wss://<host>/api/ws/...` — the
  canonical positions path is `/wss`, not `/api/ws/...`.
- ADR-12 D9 names `wss://<host>/ws` for graph data — also wrong; baseline
  binary path is `/wss`.

### Recommended resolution

Add a single canonical appendix as **ADR-06 §D11**. Auth owns the URL
surface; the existing D4 HTTP-endpoint audit table establishes the
precedent. Cross-link from ADR-02 / ADR-07 / ADR-10 / ADR-12.

### Canonical WebSocket endpoint enumeration

All endpoints share the auth model from ADR-02 D8: `?token=<jwt>` query
param on upgrade or `Authorization: Bearer <jwt>` header. Anonymous
upgrades rejected except under `--allow-skip-auth` on compile-time-gated
dev builds (ADR-06 D2).

| Path | Direction | Auth | Owning section | Protocol version | Purpose |
|------|-----------|------|----------------|------------------|---------|
| `/wss` | bidir | RequireAuth | Section 2 | V3 (`magic=0xV3F0`) | Binary position broadcast (PRD-04 + PRD-12 consumers). Settlement-gated cadence. |
| `/ws/speech` | bidir | RequireAuth | Section 9 | JSON | Mic-in (Whisper STT) + agent TTS (Kokoro). PTT-gated. |
| `/ws/client-messages` | server→client | RequireAuth | Section 3 | JSON | `filter_update_success`, `initialGraphLoad`, `memory_flash`, settings sync. |
| `/ws/mcp-relay` | bidir | RequireAuth::power_user | Section 7 | JSON-RPC | MCP tool-call relay. **REMOVE Phase 7** — re-homed in agentbox. |
| `/ws/xr-presence` | client→server | RequireAuth | Section 12 | `visionclaw-xr-presence` v1 | XR head/hand/gaze pose 30Hz. v1 sink-only; v2 adds relay. |
| `/ws/agent-telemetry` | server→client | RequireAuth | Section 10 | `AgentTelemetryEnvelope` v1 (ADR-10 D1) | Agent state from agentbox. **NEW** — replaces baseline `/api/visualization/agents/ws`. |
| `/ws/enterprise-events` | server→client | RequireAuth | Section 10 | `EnterpriseEventEnvelope` v1 (ADR-10 D5) | Forum events: membership / role / session_revoked. |
| `/api/multi-mcp/ws` | bidir | RequireAuth::power_user | Section 7 | JSON | Multi-MCP discovery. **REMOVE Phase 7** — agentbox. |
| `/api/analytics/ws` | server→client | RequireAuth | Section 1 | JSON | PageRank progress, clustering ticks. |
| `/api/ontology/ws` | bidir | RequireAuth | Section 8 | JSON | Reasoning progress + validation events. |
| `/api/visualization/agents/ws` | server→client | OptionalAuth | Section 7 | JSON | **DEPRECATED Phase 7a** → `410 Gone` with `Link` to `/ws/agent-telemetry`. **REMOVE Phase 7b.** |

### Proposed wording — new ADR-06 §D11

```markdown
### D11. WebSocket endpoint enumeration

The WebSocket surface is enumerated below. Every endpoint enforces the
auth model from ADR-02 D8 and rejects anonymous upgrades outside
compile-time-gated dev builds. New endpoints add a row here in the same
PR that introduces them.

[Insert the canonical table above]

Cross-section ownership:
- §D11 owns the URL space and the auth posture per endpoint.
- Each owning section defines the wire format for its endpoint.
- Default backpressure is drop-never-queue (ADR-02 D3); deviations
  documented in the owning ADR.
```

### Proposed wording — ADR-06 D5 correction

Replace the CSP rationale note "`wss://<host>/api/ws/...`" with:

> WebSocket endpoints are enumerated in §D11. All endpoints are
> same-origin (`wss://<host>/wss`, `wss://<host>/ws/...`,
> `wss://<host>/api/<section>/ws`) and fall under `'self'`. External
> `wss:` is not used.

### Proposed wording — ADR-12 D9 correction

Change `/ws` → `/wss` in the auth flow step 4 and in the presence
section's "distinct from" clause. Baseline graph-data path is `/wss`.

---

## T6 — Are the 5 deletion-candidate routes safe to delete?

### Current state

Baseline at `src/handlers/api_handler/bots/mod.rs` registers
`/bots/{data, update, initialize-swarm, status, agents,
spawn-agent-hybrid, remove-task/{id}}`. Three of ADR-07 D12's
deletion-candidate names need reconciliation:

- `POST /api/bots/graph` is **not** a literal baseline route; handler
  `update_bots_graph` is exposed at `POST /api/bots/data` and
  `POST /api/bots/update`. Both are deletion targets.
- `POST /api/bots/spawn-agent` (no `-hybrid`) doesn't exist server-side
  but `BotsControlPanel.tsx:101` calls it as a fallback. Treat both as
  deleted.
- `POST /api/bots/create-task` and `POST /api/bots/stop-task` are **not
  registered routes** at baseline — `CreateTask`/`StopTask` are internal
  actor messages only. The two D12 entries remove non-existent routes;
  either drop from the ADR or keep as belt-and-braces.

**Consumer audit** (grep across `client/`, `docs/`, `multi-agent-docker/`,
`scripts/`; `agentbox/` is gitignored on this branch and not on disk):

| Route | Internal callers | External callers | Doc references |
|-------|------------------|------------------|----------------|
| `POST /api/bots/initialize-swarm` | `MultiAgentInitializationPrompt.tsx:162-163` | None on disk | `docs/reference/rest-api.md`, `docs/how-to/agent-orchestration.md` |
| `POST /api/bots/spawn-agent-hybrid` | `BotsControlPanel.tsx:71`, `AgentControlPanel.tsx:110` | None | `docs/how-to/agent-orchestration.md` |
| `POST /api/bots/spawn-agent` (legacy) | `BotsControlPanel.tsx:101` fallback | None | None |
| `POST /api/bots/data`, `/update` | None in client | None | None |
| `POST /api/bots/create-task` | Not a registered route | N/A | None |
| `POST /api/bots/stop-task` | Not a registered route | N/A | None |
| `DELETE /api/bots/remove-task/{id}` | None in client | None | None |

Disk audit cannot prove no deployed agentbox calls these — which is why
ADR-07 R3 already requires a deprecation window. Deletion is **safe with
the deprecation contract**; bare deletion is not.

### Recommended resolution

Two-phase deletion with explicit deprecation contract. The shim is
documented in ADR-07 D12; the timeline is bounded.

- **Phase 7a (this sprint)**: replace handler bodies with `410 Gone`
  + `Link: <successor>; rel="successor-version"`. Route remains
  registered so callers get structured failure, not silent 404.
- **Phase 7b (~30 days later)**: route + handler deleted; doc refs
  removed from `rest-api.md` and `agent-physics-bridge.md`.

### Proposed wording — revised ADR-07 D12

```markdown
### D12. Control-plane routes — deprecation, then removal

These routes were control-plane surfaces now owned by agentbox. They are
deprecated in Phase 7a and removed in Phase 7b.

| Baseline route | Successor (agentbox) | Phase 7a | Phase 7b |
|----------------|----------------------|----------|----------|
| `POST /api/bots/initialize-swarm` | `POST {agentbox}/swarms/initialize` | `410 Gone` + `Link` | Deleted |
| `POST /api/bots/spawn-agent-hybrid` | `POST {agentbox}/agents/spawn` | `410 Gone` + `Link` | Deleted |
| `POST /api/bots/spawn-agent` (legacy alias) | (same) | `410 Gone` + `Link` | Deleted |
| `POST /api/bots/data`, `POST /api/bots/update` | (no successor — was an inverted client→server graph write; see D3) | `410 Gone` + `Link` to D3 | Deleted |
| `DELETE /api/bots/remove-task/{id}` | `DELETE {agentbox}/tasks/{id}` | `410 Gone` + `Link` | Deleted |

Routes **retained** (re-homed at `src/handlers/telemetry_handler.rs`):
- `GET /api/agents/identity/{id}` — read-only Oxigraph identity lookup.
- `GET /api/bots/status`, `GET /api/bots/agents`, `GET /api/bots/data`
  — read-only telemetry snapshot; retained until `/ws/agent-telemetry`
  achieves parity in Phase 7a.

Phase 7a deprecation response body:

    {
      "error":              "Gone",
      "code":               410,
      "message":            "This endpoint has been migrated to agentbox.",
      "successor":          "https://agentbox.example.com/swarms/initialize",
      "deprecated_since":   "2026-05-16",
      "scheduled_removal":  "2026-06-15"
    }

The `Link` header carries the machine-readable form (RFC 5988). Internal
callers (`BotsControlPanel.tsx`, `MultiAgentInitializationPrompt.tsx`,
`AgentControlPanel.tsx`) are removed alongside the handler replacement.
A counter metric `bots_deprecated_route_calls_total{route}` tracks
external stragglers before flipping to Phase 7b.
```

### Proposed addition — ADR-10 D7 CI check

Append to the disallowed-name CI check:

> Additionally, scan for re-introduction of deprecated bots control-plane
> route names (`/initialize-swarm`, `/spawn-agent`,
> `/spawn-agent-hybrid`, `/remove-task`, `/bots/data` POST,
> `/bots/update` POST) under `src/handlers/`. The Phase 7b removal date
> in ADR-07 D12 is the date these become hard CI failures.

---

## T7 — Outbound agent-click action envelope

### Current state

ADR-07 D8 specifies an internal intent `RequestAgentControlSurface
{ agent_id, swarm_id, cursor_world_position }` and defers resolution to
Section 10. ADR-10 D3 gives a transport (BroadcastChannel same-origin,
deep-link otherwise) and a partial JSON shape but **does not** name the
message-type discriminator, centralise the BroadcastChannel constant,
specify origin verification, or provide a TS type the receiver can
import. Both sides will guess.

### Recommended resolution

Add a full envelope specification to ADR-10 D3:
1. `type` discriminator at envelope level (matches the `visionflow:`
   prefix already used in D4).
2. Versioned payload mirroring D1's `schema_version` discipline.
3. Origin-verification rules per transport.
4. Single source-of-truth at
   `crates/visionclaw-contracts/src/agent_action.rs` emitting the
   TypeScript `.d.ts` via `ts-rs`. Forum + agentbox consume the type
   from the `@visionflow/contracts` npm package; VisionFlow consumes the
   Rust definition directly.

### Full envelope schema

```typescript
// Source of truth: crates/visionclaw-contracts/src/agent_action.rs
// Generated:       client/src/types/contracts/agent-action.d.ts
//                  also published as @visionflow/contracts for agentbox/forum

/**
 * AgentActionEnvelope — outbound message from VisionFlow to agentbox/forum
 * dispatched when the user interacts with an agent node in the 3D scene.
 *
 * Transport selection happens once per session at handshake time:
 *   - same-origin: BroadcastChannel(AGENT_ACTION_CHANNEL)
 *   - cross-origin: window.open(deep-link)
 *   - embedded (window.parent !== window): window.parent.postMessage
 *
 * Receivers MUST:
 *   1. Verify `type === "visionflow:agent-action"`
 *   2. Verify `schema_version === 1` (refuse with structured log otherwise)
 *   3. For postMessage delivery: verify event.origin against allowlist
 *   4. Treat any unknown `kind` as no-op (forward-compatible)
 *
 * Receivers MUST NOT:
 *   - Re-broadcast the envelope (one-way contract)
 *   - Trust `issued_by_pubkey` as auth (informational only; auth is
 *     established via the ADR-10 D4 bridge JWT)
 */
export interface AgentActionEnvelope {
  /** Discriminator. Always exactly this literal. */
  readonly type: "visionflow:agent-action";

  /** Schema version. Bumped per ADR-10 D8 when payload semantics change. */
  readonly schema_version: 1;

  /** UUID v4, generated per click. Receivers MAY use this for dedup. */
  readonly message_id: string;

  /** Unix milliseconds at click time. */
  readonly issued_at_ms: number;

  /** Pubkey of the clicking user, npub format. Informational only —
   *  receiver verifies authorisation via its own bridge session. */
  readonly issued_by_pubkey: string;

  /** Action kind. Forward-compatible: receivers ignore unknowns. */
  readonly kind:
    | "open_panel"     // primary — open the agent's control panel
    | "show_logs"      // open the agent's log view
    | "show_swarm"     // open the parent swarm overview
    | "show_lineage";  // open the agent's parent-chain trace

  /** Agent identity. Required for every kind. */
  readonly agent_id: string;

  /** Swarm identity, when known. Not all agents belong to swarms. */
  readonly swarm_id?: string;

  /** Class flag bits from the V3 node id (ADR-07 D3). Lets the receiver
   *  short-circuit if the click target is not actually an agent. */
  readonly node_class: "agent" | "knowledge" | "ontology";

  /** Click modifiers, for receivers that distinguish primary/secondary
   *  actions. All optional; receivers default to "primary" semantics. */
  readonly modifiers?: {
    readonly shift?: boolean;
    readonly ctrl?: boolean;
    readonly alt?: boolean;
    readonly meta?: boolean;
    readonly button?: 0 | 1 | 2; // 0=primary, 1=middle, 2=secondary
  };

  /** Cursor in scene world-space at click time. Used by the receiver to
   *  position popovers when rendering inside the same browser tab. */
  readonly cursor_world_position?: {
    readonly x: number;
    readonly y: number;
    readonly z: number;
  };

  /** Bridge session id from the ADR-10 D4 auth flow. Receivers MAY use
   *  this to correlate the click with the bridge session that issued
   *  the Authorization for the originating VisionFlow tab. */
  readonly bridge_id?: string;
}

/** BroadcastChannel constant. Both sides import this literal. */
export const AGENT_ACTION_CHANNEL = "visionflow:agent-actions" as const;

/** Deep-link template. Receivers parse incoming requests at this path.
 *  Deep-link is structurally untrusted (URL bar); receivers validate
 *  every field as if it arrived via BroadcastChannel. The `bridge_id`
 *  is the only field linking the click to an authenticated session;
 *  if absent or invalid, the receiver SHOULD challenge for re-auth. */
export const AGENT_ACTION_DEEP_LINK_TEMPLATE =
  "/agents/{agent_id}?source=visionflow&kind={kind}" +
  "&issued_at={issued_at_ms}&issued_by={issued_by_pubkey}" +
  "&message_id={message_id}&node_class={node_class}" +
  "&bridge_id={bridge_id?}&swarm_id={swarm_id?}";

/** Allowed postMessage target origins. The bridge handshake (ADR-10 D4)
 *  establishes this list at session start; the receiver enforces it.
 *  Empty list => BroadcastChannel-only mode. */
export type AgentActionTargetOrigin =
  | "https://agentbox.example.com"
  | "https://forum.example.com";
```

### Origin-check requirement (normative)

| Transport | Required verification |
|-----------|----------------------|
| `BroadcastChannel` | `data.type === "visionflow:agent-action"` AND `data.schema_version === 1`. Browser guarantees same-origin. |
| `window.postMessage` | All of the above PLUS `event.origin` matches `AgentActionTargetOrigin`. Allowlist established at bridge handshake. |
| Deep-link (URL) | All envelope checks PLUS treat every field as untrusted user input. `bridge_id` is the only authenticated link; if absent/invalid, challenge for re-auth before honouring `kind`. |

### Proposed wording — revised ADR-10 D3

Replace the JSON snippet and selection prose with: "The envelope shape,
BroadcastChannel constant, deep-link template, and `AgentActionTargetOrigin`
allowlist type are defined in `crates/visionclaw-contracts/src/agent_action.rs`
and generated as `client/src/types/contracts/agent-action.d.ts`. The full
schema is reproduced in `_resolutions/T4-T6-T7-api-contracts.md` §T7.
Receivers MUST verify `type === "visionflow:agent-action"` and
`schema_version === 1`; postMessage receivers additionally enforce
`event.origin` against the allowlist. Unknown `kind` values are no-ops
(forward-compatible). This envelope supersedes ADR-07 D8's
`RequestAgentControlSurface` intent."

### Proposed wording — revised ADR-07 D8

Replace D8 body with: "Clicking an agent capsule constructs an
`AgentActionEnvelope` (ADR-10 D3 + `crates/visionclaw-contracts/src/
agent_action.rs`) and dispatches it via the session's chosen transport.
VisionFlow does not render a control panel in-process and does not embed
an iframe of one; the renderer's responsibility ends at envelope
dispatch."

---

## Cross-cutting follow-ups (subsequent sprints)

1. **`crates/visionclaw-contracts`** becomes the home for every
   cross-boundary envelope: `AgentActionEnvelope`,
   `AgentTelemetryEnvelope`, `EnterpriseEventEnvelope`, binary-protocol
   header constants. `ts-rs` emits the `.d.ts`. ADR-10 D8 versioning
   gates the crate's semver.

2. **Contract test harness** at `tests/contracts/external-integrations/`:
   instantiates each envelope variant, round-trips through the chosen
   transport, asserts the receiver's structured-error response on
   malformed input. Includes the agentbox-emulator from ADR-10 R1.

3. **CI check for endpoint-enumeration drift**: a script parses
   `App::new()` route registrations and asserts every `.route("/ws...")`
   appears in ADR-06 §D11. PR fails if a new WS path is added without
   a corresponding table row.

4. **Deprecation telemetry**: the Phase 7a `410 Gone` shims emit
   `bots_deprecated_route_calls_total{route}`. The team watches this
   counter before flipping to Phase 7b.
