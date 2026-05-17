# ADR-063: URN-Traced Operations Across All Subsystems

**Status:** Accepted  
**Date:** 2026-05-01  
**Deciders:** jjohare  
**Supersedes:** None (extends ADR-013)

## Context

ADR-013 defines the canonical URI grammar (`urn:agentbox:<kind>:[<scope>:]<local>`)
and `management-api/lib/uris.js` implements it correctly. However, multiple
subsystems were generating opaque UUIDv4 identifiers instead of minted URNs,
breaking traceability across the knowledge graph, observability stack, and
federation boundary.

The VisionClaw Rust substrate has `src/uri/` (mint.rs, parse.rs, kinds.rs)
with `mint_concept`, `mint_execution`, `mint_bead`, `mint_did_nostr`,
`mint_owned_kg`, and `mint_group_members` — but several actors still use
`Uuid::new_v4()` directly.

## Decision

**Every identifier emitted by any subsystem MUST be a minted URN** following
the canonical grammar. No `randomUUID()`, `uuidv4()`, `Uuid::new_v4()`, or
ad-hoc string concatenation for identifiers.

### Standing Directive

This is a non-negotiable standing directive across all sessions, all agents,
all subsystems. The URN namespace is the system's connective tissue — without
it, artifacts are opaque blobs disconnected from the graph.

### Agentbox Violations Fixed (this ADR)

| File | Was | Now |
|------|-----|-----|
| `adapters/orchestrator/local-process-manager.js:41` | `randomUUID()` | `uris.mint({kind:'agent', localId:...})` |
| `adapters/orchestrator/stdio-bridge.js:35,67` | `randomUUID()` | `uris.mint({kind:'agent',...})`, `uris.mint({kind:'event',...})` |
| `adapters/beads/local-sqlite.js:67,102` | `randomUUID()` | `uris.mint({kind:'bead', pubkey:..., payload:...})` |
| `adapters/events/local-jsonl.js:75` | `randomUUID()` | `uris.mint({kind:'event', pubkey:..., payload:...})` |
| `adapters/events/external.js:56` | `randomUUID()` | `uris.mint({kind:'event', pubkey:..., payload:...})` |
| `utils/process-manager.js:30` | `uuidv4()` | `uris.mint({kind:'activity', pubkey:..., payload:...})` |
| `utils/comfyui-manager.js:31` | `uuidv4()` | `uris.mint({kind:'activity', pubkey:..., payload:...})` |
| `observability/metrics.js:74` | Ad-hoc concat + Math.random() | `uris.mint({kind:'event', pubkey:..., payload:...})` |

### VisionClaw Remaining Gaps (future work)

These Rust-side `Uuid::new_v4()` sites should migrate to `src/uri/mint_*`:

| File | Line | Current | Target |
|------|------|---------|--------|
| `telemetry/agent_telemetry.rs` | 24 | `Uuid::new_v4()` | `mint_execution(pubkey, ...)` |
| `actors/server_nostr_actor.rs` | 302 | `Uuid::new_v4()` | `mint_execution(pubkey, ...)` |
| `events/types.rs` | 62 | `Uuid::new_v4()` | `mint_execution(pubkey, ...)` |
| `actors/ontology_guidance_actor.rs` | 103 | `Uuid::new_v4()` | `mint_execution(pubkey, ...)` |
| `actors/gpu/clustering_actor.rs` | 535,647 | `Uuid::new_v4()` | `mint_group_members(...)` or `mint_concept(...)` |
| `events/handlers/notification_handler.rs` | 108 | `Uuid::new_v4()` | `mint_execution(pubkey, ...)` |
| `actors/messaging/message_id.rs` | 20 | `Uuid::new_v4()` | `mint_execution(pubkey, ...)` |
| `actors/skill_evaluation_actor.rs` | varies | `Uuid::new_v4()` | `mint_execution(pubkey, ...)` |

### Memory System (Implemented)

Every memory entry stored or retrieved via MCP tools now carries a canonical URN:

```
urn:agentbox:memory:<namespace>.<key>
```

The URN is minted at three layers:

| Layer | File | Change |
|-------|------|--------|
| MCP server | `mcp/servers/mcp-server.js` | `store`, `retrieve`, `list`, `search` actions all mint/annotate URNs via `uris.mint({kind:'memory', localId:...})` |
| Embedded adapter | `adapters/memory/embedded-ruvector.js` | `store()` and `retrieve()` return `urn` field |
| External PG adapter | `adapters/memory/external-pg.js` | `store()`, `retrieve()`, and `search()` return `urn` field |
| Contract test | `tests/contract/memory.contract.spec.js` | New `[M2]` assertion verifies URN presence and grammar |

The `memory` kind in `uris.js` is `ownerScope: false`, `contentAddressed: false`,
with `resolvableSurface: 'memory'`. The `localId` is `<namespace>.<key>` — deterministic
and stable across store/retrieve cycles. The URN minting degrades gracefully (returns
`null`) if `uris.js` is not loadable, so the memory system never fails on URN errors.

### Plugin System

Plugin bootstrap (entrypoint Phase 7) writes `config.json` with
ruvector-postgres connection. Plugin manifests should carry
`urn:agentbox:mcp:<plugin-name>` identifiers.

## Consequences

- **Positive:** Every artifact is a first-class node in the knowledge graph,
  queryable via `/v1/uri/<urn>`, visible in 3D visualization, traceable
  across federation boundary via BC20 anti-corruption layer.
- **Positive:** Observability logs carry canonical URNs instead of opaque
  UUIDs — grep becomes semantic search.
- **Negative:** URN strings are longer than UUID strings. Beads table `id`
  column stores ~80-char URNs vs 36-char UUIDs. Acceptable trade-off.
- **Negative:** VisionClaw Rust-side migration requires `cargo build` + test
  verification. Tracked as future work above.

## Compliance

- ADR-013: Grammar compliance (all mint calls use `uris.js`)
- ADR-005: Adapter contract (all five slots now emit URN identifiers)
- DDD-004 §URICanonicaliser: Every `@id` follows canonical grammar
- PRD-006: JSON-LD surfaces can now link between adapter outputs
