# ADR-039: Settings/Physics Object Consolidation

## Status

Implemented 2026-04-20

## Date

2026-04-14 (proposed) / 2026-04-20 (actor consolidation implemented)

## Implementation Notes (2026-04-20)

The actor-side scope of this ADR has been implemented. `OptimizedSettingsActor`
is now the canonical `SettingsActor`, carrying two partitions:

- **Public partition** (`settings: Arc<RwLock<AppFullSettings>>`) — subject to
  subscribe/read via `GetSettings`, `GetSettingByPath(s)`, `UpdateSettings`,
  `SetSettingsByPaths`, hot-reload, and auto-balance updates.
- **Protected partition** (`protected: Arc<RwLock<ProtectedSettings>>`) —
  admin-only, routed through `GetApiKeys`, `ValidateClientToken`,
  `StoreClientToken`, `UpdateUserApiKeys`, `CleanupExpiredTokens`,
  `MergeSettings`, `SaveSettings`, `GetUser` as a write-through gate.

The former `ProtectedSettingsActor` struct has been removed; the name is
retained as a backward-compatible type alias over `OptimizedSettingsActor`
so that existing imports (`use crate::actors::protected_settings_actor::*;`)
compile unchanged. `AppState` now spawns a single actor and exposes the
same `Addr` under both `settings_addr` and `protected_settings_addr`.

Physics-object consolidation (`SimulationParams`/`PhysicsSettingsDTO`/
`UpdatePhysicsRequest` removal) remains in scope for a follow-up sprint and
is tracked separately from the actor unification.

See `tests/settings_consolidation_test.rs` for partition-level regression
coverage.

## Context

VisionFlow has 7 physics settings representations with inconsistent field names,
types, and value ranges. Two competing HTTP handlers silently shadow each other:

| # | Type | Location | Issue |
|---|------|----------|-------|
| 1 | `PhysicsSettings` | `config/physics.rs` | Canonical config (`repulsion_strength`) |
| 2 | `SimParams` `#[repr(C)]` 172B | `force_compute_actor.rs` | GPU wire format (`repel_k`) |
| 3 | `SimulationParams` | API layer | Duplicate of #1 with different field names |
| 4 | `PhysicsSettingsDTO` | `settings_handler.rs` | Third naming convention |
| 5 | `UpdatePhysicsRequest` | Handler request body | Fourth naming convention |
| 6 | Client `PhysicsSettings` | `settingsApi.ts` (11 fields) | Subset interface |
| 7 | Client `PhysicsSettings` | `settings.ts` (40+ fields) | Superset interface |

Two `PUT /api/settings/physics` handlers are mounted. The first propagates to GPU;
the second updates a local copy without GPU propagation. Which runs depends on route
registration order. Result: settings update returns 200 but GPU may not change, and
subsequent GET may return stale values.

## Decision Drivers

- A settings update must deterministically reach the GPU.
- Field names must be consistent from client to kernel.
- Minimise intermediate translation layers.
- `SimParams` `#[repr(C)]` must match the CUDA kernel struct (cannot rename freely).

## Considered Options

### Option 1: PhysicsSettings -> SimParams -> GPU (chosen)
Eliminate `SimulationParams`, `PhysicsSettingsDTO`, `UpdatePhysicsRequest`. One
`From<&PhysicsSettings> for SimParams` handles name mapping. One handler, one client
interface.
- **Pros**: Two Rust structs total. Single `From` impl documents all mapping. Duplicate
  handler bug structurally eliminated.
- **Cons**: Existing API consumers must migrate field names (aliases ease transition).

### Option 2: Keep DTO layer with serde aliases
- **Pros**: API names can differ from internal. **Cons**: Three structs; alias
  proliferation; does not fix duplicate handler.

### Option 3: Code-generate from shared schema
- **Pros**: Guaranteed cross-language consistency. **Cons**: `#[repr(C)]` layout
  cannot derive from JSON Schema; high cost for one struct.

## Decision

**Option 1: Consolidate to PhysicsSettings -> SimParams -> GPU.**

1. **Two Rust structs**: `PhysicsSettings` (canonical, serde-deserializable with
   `#[serde(alias)]` for old names during migration) and `SimParams` (`#[repr(C)]`,
   derived only via `From<&PhysicsSettings>`). Delete `SimulationParams`,
   `PhysicsSettingsDTO`, `UpdatePhysicsRequest`.
2. **Single handler**: One `PUT /api/settings/physics`. Deserializes
   `PhysicsSettings`, validates, converts to `SimParams`, sends to GPU actor,
   updates `GraphStateActor`, returns applied settings. Remove duplicate handler.
3. **One client interface**: Consolidate `settingsApi.ts` and `settings.ts` into
   `types/physics.ts`. Field names match Rust `PhysicsSettings` serde names.
4. **Migration**: `#[serde(alias = "repel_k")]` etc. on `PhysicsSettings` for
   backward compatibility. Remove aliases after one release cycle.

## Consequences

### Positive
- Settings deterministically reach GPU on every request.
- Two Rust structs instead of five; one TS interface instead of two.
- `From` impl is the single field-name translation point.
- Duplicate handler bug structurally impossible.

### Negative
- Clients using old field names must migrate within one release (aliases bridge gap).
- `SimParams` names (`repel_k`) permanently differ from `PhysicsSettings`
  (`repulsion_strength`) due to GPU ABI. The `From` impl documents this.

### Neutral
- GPU kernel code unchanged.
- WebSocket protocol unchanged.
- `GET /api/settings/physics` returns `PhysicsSettings` directly.

## Related Decisions
- ADR-034: Position Data Flow Consolidation
- ADR-013: Render Performance

## References
- `src/config/physics.rs`
- `src/actors/gpu/force_compute_actor.rs`
- `src/handlers/socket_flow_handler/http_handler.rs`
- `src/actors/messages/physics_messages.rs`
