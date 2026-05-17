# DDD: Agentbox Integration Bounded Context

**Context name:** `AgentboxIntegration` (BC20 — provisional, pending BC catalogue update)
**Date:** 2026-04-23
**Author:** VisionClaw platform team
**Related:** PRD-004 (agentbox ↔ VisionClaw integration), ADR-058 (MAD→agentbox migration), ADR-005 (agentbox pluggable adapters), `docs/ddd-bead-provenance-context.md` (BC — Bead Provenance), ADR-056 (JSS parity migration), ADR-057 (Contributor Enablement Platform)

## 1. Purpose

Define the bounded context that mediates between VisionClaw's Rust substrate and the agentbox execution container. This context is an **Anti-Corruption Layer**: its job is to translate between the external agentbox adapter protocol (stable, generic, owned by the agentbox product) and VisionClaw's internal aggregates (evolving, domain-specific, owned by VisionClaw).

Without this context, every VisionClaw actor that wants to spawn an agent or record a receipt would couple itself to agentbox's wire format. With it, agentbox can evolve independently in its own repo and VisionClaw can evolve its domain model without breaking the agent pipeline.

## 2. Ubiquitous language

| Term | Meaning in this context |
|---|---|
| **AgentExecutionRequest** | A VisionClaw-side command asking agentbox to spawn an agent with a role, prompt, and user context |
| **AgentExecutionReceipt** | The bead an agent records when it performs work; one epic per brief, one child per role response |
| **AdapterEndpoint** | A VisionClaw service that implements one of the five agentbox adapter contracts (beads, pods, memory, events, orchestrator) |
| **FederationSession** | A resolved configuration binding a running agentbox instance to a specific set of VisionClaw AdapterEndpoints for its lifetime |
| **SpawnChannel** | The stdio or HTTP contract over which VisionClaw actors drive agent lifecycle inside agentbox |
| **AgentEventStream** | The JSONL event stream from agentbox (spawn, tool-use, progress, completion) consumed by VisionClaw actors |
| **LocalFallbackProbe** | A diagnostic check confirming that an agentbox in federated mode is *not* silently using its local fallbacks when VisionClaw AdapterEndpoints are expected. Mechanism: each external `AdapterEndpoint` must respond to a `GET /probe/origin` (or MCP/stdio equivalent) with a signed token containing `{endpoint_id, issued_at, session_id_nonce}`, signed with the endpoint's registered Ed25519 key. The local fallbacks have no such key, cannot produce a valid signature, and probe failure is treated as session quarantine (see §4.1 invariant) |

## 3. Strategic placement

```mermaid
graph TD
    subgraph VC["VisionClaw Rust substrate"]
        BP["BeadProvenance (BC)<br/>(ddd-bead-...)"]
        CONTRIB["Contributor (BC18)<br/>(ADR-057)"]
        SKILLS["Skills (BC19)"]

        subgraph BC20["AgentboxIntegration (BC20 - this context)"]
            AERH["AgentExecutionRequestHandler"]
            AER["AdapterEndpointRegistry"]
            FS["FederationSession aggregate"]
            AESP["AgentEventStreamProjector"]
            ACA["AntiCorruptionAdapters<br/>(beads, pods, memory, events, orchestrator)<br/>translate agentbox wire to VC domain"]
        end
    end

    subgraph AB["agentbox (sibling container, ADR-005 adapters)"]
    end

    BP --> BC20
    CONTRIB --> BC20
    SKILLS --> BC20
    BC20 -->|"docker-compose-internal DNS"| AB
```

## 4. Aggregates

### 4.1 `FederationSession`

**Root entity.** Represents one running agentbox instance and its resolved adapter bindings.

```rust
pub struct FederationSession {
    id: FederationSessionId,           // UUID v7 — time-ordered
    agentbox_image_hash: ImageHash,    // sha256 from Nix build — invariant per session
    manifest_checksum: ManifestHash,   // sha256 of agentbox.toml
    adapter_bindings: AdapterBindings, // one AdapterEndpoint per slot
    started_at: DateTime<Utc>,
    health: SessionHealth,             // Healthy | Degraded | Failed
    active_executions: Vec<AgentExecutionId>,
}
```

**Invariants:**
- `adapter_bindings` MUST NOT include any `Local` binding when `mode = "client"`. A LocalFallbackProbe failure triggers session quarantine (the session transitions to `Failed`, outstanding executions are cancelled with reason `FederationIntegrityViolation`, and the `LocalFallbackProbeFailed` event is raised).
- `manifest_checksum` MUST match the agentbox container's reported manifest hash at boot. Mismatch = reject session start.
- Once started, `adapter_bindings` are immutable — changing them requires a new session.
- Version compatibility is verified via the `/v1/meta` handshake (see §4.1a) BEFORE the session transitions to Healthy.

**Domain events emitted:**
- `FederationSessionStarted { session_id, manifest_checksum, adapter_bindings }`
- `FederationSessionHealthDegraded { session_id, reason, affected_slots }`
- `LocalFallbackProbeFailed { session_id, slot, expected_endpoint, actual_signature_result }`
- `AdapterContractVersionMismatch { session_id, slot, agentbox_version, visionclaw_range }`
- `FederationSessionStopped { session_id, reason }`

### 4.1a Session boot handshake — `/v1/meta` protocol

On session start, `FederationSessionLifecycleService` executes this sequence before emitting `FederationSessionStarted`:

1. **Pull agentbox meta.** Call `GET http://<agentbox>:9090/v1/meta`. Response shape:
   ```json
   {
     "image_hash": "sha256:...",
     "manifest_checksum": "sha256:...",
     "federation_mode": "client",
     "adapter_contract_versions": {
       "beads": "1.2.0",
       "pods": "1.0.1",
       "memory": "2.0.0",
       "events": "1.1.0",
       "orchestrator": "1.0.0"
     }
   }
   ```
2. **Verify federation mode.** `federation_mode` MUST equal `"client"`. Otherwise abort with `FederationMisconfigured`.
3. **Compare adapter contract versions** against `AdapterEndpointRegistry.compat_ranges`. For each slot, the agentbox-declared version MUST intersect the VisionClaw-declared SemVer range. Otherwise emit `AdapterContractVersionMismatch` and abort.
4. **Run LocalFallbackProbe for every slot.** For each slot, call the registered endpoint's `GET /probe/origin`, verify the Ed25519 signature against the endpoint's registered public key, and confirm `endpoint_id` matches the registry entry. Any failure emits `LocalFallbackProbeFailed` and aborts.
5. **Emit `FederationSessionStarted`** with the resolved bindings and the full handshake record (for audit replay).

The handshake runs on every session start; no caching. Total p95 budget: 2 seconds across all five slots. Timeouts abort session start rather than fall through to partial startup.

### 4.2 `AgentExecution`

**Root entity.** One unit of agent work requested by a VisionClaw actor.

```rust
pub struct AgentExecution {
    id: AgentExecutionId,
    session: FederationSessionId,
    requester: ActorRef,               // which VC actor asked
    role: RoleSlug,                    // architect, dev, ciso, ...
    prompt: PromptPayload,
    user_context: UserContext,         // pubkey + display name (NIP-98 identity)
    brief_ref: Option<BriefRef>,       // if this is part of a Briefing workflow
    bead_ref: Option<BeadRef>,         // the child bead created for this execution
    status: ExecutionStatus,           // Spawning | Running | Completed | Failed
    events: Vec<AgentEvent>,           // lifecycle projection
}
```

**Invariants:**
- An `AgentExecution` belongs to exactly one `FederationSession`.
- `bead_ref` MUST be populated if the session's `adapters.beads != Off`.
- `events` append-only; projection from agentbox's AgentEventStream; last event MUST be terminal (Completed or Failed) before the execution is archived.

**Domain events emitted:**
- `AgentExecutionRequested { execution_id, session, role, user_context }`
- `AgentExecutionSpawned { execution_id, pid_or_token, started_at }`
- `AgentExecutionToolUsed { execution_id, tool, input_ref, output_ref }`
- `AgentExecutionCompleted { execution_id, outcome, artefact_refs }`
- `AgentExecutionFailed { execution_id, reason }`

### 4.3 `AdapterEndpointRegistry`

**Aggregate of value objects.** Not an entity — a read-model that records which VisionClaw sibling containers implement which agentbox adapter slot.

```rust
pub struct AdapterEndpoint {
    slot: AdapterSlot,                 // Beads | Pods | Memory | Events | Orchestrator
    protocol: AdapterProtocol,         // Http | Stdio | Mcp
    address: EndpointAddress,          // e.g. "http://beads-actor:7001"
    health: EndpointHealth,
    contract_version: SemVer,          // adapter contract version it implements
}
```

**Invariants:**
- For every slot, at least one endpoint is Healthy before `FederationSessionStarted` can emit.
- `contract_version` must be compatible with the agentbox version agentbox reports. Incompatibility = rejected session start.

## 5. Anti-Corruption Layer (ACL)

The ACL is the heart of this context. It owns **five translator modules**, one per adapter slot:

| ACL module | Maps agentbox wire → VC domain |
|---|---|
| `beads_acl` | agentbox `bd`-CLI-shaped JSON ↔ `BeadProvenance` aggregate commands/events |
| `pods_acl` | Solid-protocol LDP containers ↔ VisionClaw pod artefact URIs (LDP-compatible but often with extra VC metadata) |
| `memory_acl` | Generic vector query/store ↔ VisionClaw's memory namespace layout (`personal-context`, `project-state`, `patterns`, etc.) |
| `events_acl` | agentbox JSONL event schema ↔ VisionClaw's Contributor Stratum event bus schema (kinds, partition keys, retention) |
| `orchestrator_acl` | agentbox stdio spawn protocol ↔ VisionClaw's actor spawn command/reply pattern |

**ACL rules:**
1. No VisionClaw domain type ever appears in agentbox's repo. Agentbox's adapter interface is generic by design.
2. No agentbox wire type ever leaks into VisionClaw domain code outside this context. Callers see VC-typed commands only.
3. ACL modules version their translations. When agentbox's adapter contract changes, the ACL translator is bumped; VC domain events do not change.
4. Translation must be **total** — every agentbox payload must either map to a VC domain event or be classified as a known-and-ignorable signal. Unknown payloads raise `UnmappedAgentboxPayload` for triage.

## 6. Integration with other bounded contexts

### 6.1 With `BeadProvenance` (ddd-bead-provenance-context.md)

Upstream. `beads_acl` is the ONLY code path that invokes `BeadProvenance` commands on behalf of an agentbox execution. When `adapters.beads = "external"`, agentbox POSTs to the VisionClaw beads-actor endpoint; the actor accepts a narrow command-shape that `beads_acl` translates into rich `CreateEpic` / `CreateChild` / `ClaimBead` commands honouring the context's invariants.

### 6.2 With `Contributor` (BC18, ADR-057)

Upstream. `ContributorStudioSupervisor` is the most common requester of `AgentExecutionRequested`. It passes a `GuidanceSession` context that this context projects into agentbox's `UserContext` + role-specific prompt.

### 6.3 With `Skills` (BC19)

Upstream. Skills are discovered + installed on the agentbox side (skills are content-addressed Nix inputs per agentbox PRD-001 §5). VisionClaw's `SkillRegistrySupervisor` signals **which skill is currently active** per session; the session's adapter bindings may narrow or extend based on skill requirements (e.g. a skill needing ontology tools requires `skills.ontology = true` in the manifest).

### 6.4 With `BeadProvenance` events published via Nostr

Downstream. When `adapters.events = "external"` AND the VC event bus fans out to Nostr, each `AgentExecutionCompleted` produces a NIP-33 addressable replaceable event that the Contributor Stratum's publicTypeIndex can reference.

## 7a. Composed SessionHealth semantics

A `FederationSession`'s health is NOT a boolean AND of its five adapter endpoints. Per-slot degrade policies determine whether partial failures are survivable.

```rust
pub enum SessionHealth { Healthy, Degraded, Failed }

pub enum SlotDegradePolicy {
    Required,           // any non-Healthy → session Failed
    TolerateDegrade,    // Degraded allowed, Unhealthy → Failed
    TolerateOutage,     // Unhealthy allowed (best-effort slot), session remains Degraded
}

// Default policy per slot:
beads:        Required        // every agent execution must be receipt-tracked
pods:         TolerateDegrade // read can be stale, write failure blocks session
memory:       TolerateDegrade // retrieval can be partial, store failure blocks session
events:       TolerateOutage  // event loss is visible in metrics; does not block execution
orchestrator: Required        // can't spawn agents without it — session fails
```

Rule:
- `SessionHealth = Healthy` iff all slots meet or exceed their policy's threshold AND none are `Failed`.
- `SessionHealth = Degraded` iff at least one slot is Degraded under a `TolerateDegrade` policy OR Unhealthy under `TolerateOutage`, AND no slot is Failed under `Required`.
- `SessionHealth = Failed` iff any slot violates its policy.

`FederationSessionHealthDegraded` events carry `affected_slots` so downstream consumers know which capability is impaired. `AdapterHealthMonitor` re-evaluates the composition every 10 s; transitions trigger event emission (no spamming on steady-state).

## 7. Domain services

| Service | Responsibility |
|---|---|
| `FederationSessionLifecycleService` | Start/stop sessions; probes LocalFallbackProbe on boot; quarantines degraded sessions |
| `AgentExecutionCoordinator` | Accepts `AgentExecutionRequested` commands; resolves session; delegates to `orchestrator_acl` |
| `AgentEventStreamProjector` | Consumes agentbox JSONL stream; translates via `events_acl`; emits VC domain events |
| `AdapterHealthMonitor` | Periodic health checks against all registered `AdapterEndpoint`s; publishes `AdapterEndpointHealthChanged`; computes composed `SessionHealth` per §7a |
| `MADDeprecationMigrator` | One-shot service: reads MAD state (beads via MAD's BeadsService, briefs from `team/` filesystem) and replays into VC's BC aggregates before MAD is stopped |

## 8. Policies

1. **No agentbox-side business logic.** Anything the Briefing role-table (architect/dev/ciso/...) decides is configuration in `agentbox/config/briefing-roles.toml`. Policy about who may trigger which role, rate limits, NIP-26 delegation caps — all live in VC's Policy Engine (ADR-045).
2. **Adapter contract versioning is monotonic.** Agentbox publishes adapter contract versions per slot; VC's `AdapterEndpoint` declares the range it supports. Agentbox upgrades that would break older VC endpoints must ship a compat shim; VC upgrades that require newer contracts block on agentbox PR.
3. **Standalone mode is not a VisionClaw concern.** When agentbox runs in `federation.mode = "standalone"`, this context is inert. There is no code path that inspects standalone-mode behaviour.

## 9. Open design questions

1. **Event bus substrate for VC↔agentbox events.** Options: NATS, Redis streams, direct HTTP long-poll, WebSocket. Preference leans to NATS because it's already used elsewhere in VC actors, but not yet decided. Tracking in a follow-up ADR once the first implementation lands.
2. **MAD state replay atomicity.** The `MADDeprecationMigrator` has to replay ~months of BeadsService state into `BeadProvenance`. If it fails mid-way, is the partial replay kept or rolled back? Current lean: keep, since beads are append-only and idempotent; a retry converges. Verify with BeadProvenance owners before cutover.
3. **Contract test location.** Adapter contract tests live in agentbox's repo. VC-side ACL tests live in VC. A shared contract-test fixture repo could reduce duplication but adds release coordination. Defer until the second VC-shaped consumer of agentbox appears.

## 10. Glossary cross-references

- **Bead / Epic / Child / Claim** — see `docs/ddd-bead-provenance-context.md`
- **ContributorStudioSupervisor, GuidanceSession, ShareIntent** — see ADR-057 and the BC18 aggregate in `src/domain/contributor/`
- **SkillPackage, SkillEvalSuite** — see BC19 in `src/domain/skills/`
- **Policy Engine** — see ADR-045
- **publicTypeIndex / NIP-26 delegation** — see ADR-029
- **adapter contract** — see agentbox ADR-005
