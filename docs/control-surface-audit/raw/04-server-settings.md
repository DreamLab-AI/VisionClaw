# Server Settings & API Routes Audit

**Scope:** SERVER-SIDE settings schema, HTTP/WS routes, feature flags, dev toggles, and internal tunables.
**Branch:** feature/unified-control-surface
**Date:** 2026-04-28

---

## 1. Settings Schema (AppFullSettings + Child Structs)

### 1.1 Root Settings Struct

**File:** `src/config/app_settings.rs`

`AppFullSettings` is the **canonical** server-side settings schema. All fields serializable (serde), all validated via Rust trait validators.

| Rust Path | Type | Default | Validation | Consumed By | Persistence | Serde Rename | Notes |
|-----------|------|---------|-----------|-------------|-------------|--------------|-------|
| `visualisation` | `VisualisationSettings` | Default::default() | #[validate(nested)] | GPU/rendering handlers | Neo4j/in-memory | camelCase | Graph rendering (nodes, edges, physics, glow, bloom, animations) |
| `system` | `SystemSettings` | Default::default() | #[validate(nested)] | Network/security | Neo4j | camelCase | Network, WebSocket, security, debug config |
| `xr` | `XRSettings` | Default::default() | #[validate(nested)] | XR handlers | Neo4j | camelCase | Hand tracking, haptics, passthrough, teleport, plane detection |
| `auth` | `AuthSettings` | Default::default() | #[validate(nested)] | Auth handler | Neo4j | camelCase | OIDC, OAuth providers, API keys |
| `ragflow` | `Option<RagFlowSettings>` | None | skip_if_none | RAG handlers | Neo4j | camelCase | RAG Flow service config |
| `perplexity` | `Option<PerplexitySettings>` | None | skip_if_none | Perplexity handler | Neo4j | camelCase | Perplexity API client settings |
| `openai` | `Option<OpenAISettings>` | None | skip_if_none | OpenAI handler | Neo4j | camelCase | OpenAI API client settings |
| `kokoro` | `Option<KokoroSettings>` | None | skip_if_none | Voice handler | Neo4j | camelCase | Kokoro TTS service |
| `whisper` | `Option<WhisperSettings>` | None | skip_if_none | Speech handler | Neo4j | camelCase | Whisper speech-to-text |
| `voice_routing` | `Option<VoiceRoutingSettings>` | None | skip_if_none | Voice handler | Neo4j | camelCase | Voice provider routing logic |
| `ontology_agent` | `Option<OntologyAgentSettings>` | None | skip_if_none | Ontology handler | Neo4j | camelCase | Ontology agent configuration |
| `version` | `String` | "1.0.0" | default="default_version" | Settings reader | in-memory | camelCase | Settings schema version |
| `user_preferences` | `UserPreferences` | Default::default() | #[validate(nested)] | Client preference | Neo4j | camelCase | User comfort, theme, language |
| `physics` | `PhysicsSettings` | Default::default() | #[validate(nested)] | GPU/simulation | Neo4j | camelCase | Graph layout physics (deprecated at root, prefer nested in visualisation) |
| `feature_flags` | `FeatureFlags` | Default::default() | (no validation) | Feature gates | in-memory | camelCase | GPU clustering, ontology validation, etc. |
| `developer_config` | `DeveloperConfig` | Default::default() | (no validation) | Dev handlers | in-memory | camelCase | Debug, profiling, verbose logging |

### 1.2 Visualisation Subtree

**File:** `src/config/visualisation.rs`

| Rust Path | Type | Default | Validation | Notes |
|-----------|------|---------|-----------|-------|
| `visualisation.graphs` | `GraphsSettings` | ... | nested | logseq + visionflow (agent) graphs |
| `visualisation.graphs.logseq.physics` | `PhysicsSettings` | ... | nested | Knowledge graph physics (canonical) |
| `visualisation.graphs.visionflow.physics` | `PhysicsSettings` | ... | nested | Agent/bot graph physics |
| `visualisation.rendering` | `RenderingSettings` | ... | nested | Ambient/directional light, shadows, AO |
| `visualisation.animation` | `AnimationSettings` | ... | nested | Motion blur, pulse, wave, selection animations |
| `visualisation.glow` | `GlowSettings` | ... | nested + cross-field validation | Color (#00ffff), opacity (0.8), intensity (0-10), radius, threshold |
| `visualisation.bloom` | `BloomSettings` | ... | nested + cross-field validation | Intensity, radius, threshold, strength, knee |
| `visualisation.hologram` | `HologramSettings` | ... | nested | Pulse effect, scan line, data stream config |
| `visualisation.labels` | `LabelSettings` | ... | nested | Font sizes (desktop/mobile), color, opacity |
| `visualisation.nodes` | `NodeSettings` | base_color=#202724 | nested + hex validation | Metalness, opacity, roughness, quality, instancing, metadata flags |
| `visualisation.edges` | `EdgeSettings` | color=#ff0000 | nested + width range validation | Arrow size, base width, opacity, quality |
| `visualisation.camera` | `CameraSettings` | ... | nested | FOV, near/far clipping, position presets |
| `visualisation.spacepilot` | `SpacePilotSettings` | ... | nested | 3D mouse sensitivity, inversion, acceleration |

### 1.3 Physics Subtree

**File:** `src/config/physics.rs`

| Rust Path | Type | Default | Validation | Notes |
|-----------|------|---------|-----------|-------|
| `visualisation.graphs.logseq.physics.enabled` | `bool` | true | range cross-field (gravity != 0 → must be enabled) | Master on/off |
| `visualisation.graphs.logseq.physics.gravity` | `f32` | 0.0 | range(-10, 10) | Downward acceleration |
| `visualisation.graphs.logseq.physics.damping` | `f32` | 0.8 | range(0, 1) | Velocity dissipation per step |
| `visualisation.graphs.logseq.physics.spring_k` | `f32` | 0.05 | finite check | Spring stiffness (edges pull nodes together) |
| `visualisation.graphs.logseq.physics.repel_k` | `f32` | 100.0 | finite check | Repulsion strength (nodes push apart) |
| `visualisation.graphs.logseq.physics.max_velocity` | `f32` | 200.0 (CANONICAL_MAX_VELOCITY) | range(0, 10000) | Per-node velocity cap |
| `visualisation.graphs.logseq.physics.max_force` | `f32` | 50.0 (CANONICAL_MAX_FORCE) | range(0, 10000) | Per-node force cap |
| `visualisation.graphs.logseq.physics.dt` | `f32` | 0.016 | range(0.001, 1.0) | Timestep (typically 16ms = 60fps) |
| `visualisation.graphs.logseq.physics.bounds_size` | `f32` | 1000.0 | finite check | World boundary size |
| `visualisation.graphs.logseq.physics.center_gravity_k` | `f32` | 0.0 | finite check | Center-of-mass attraction |
| `visualisation.graphs.logseq.physics.rest_length` | `f32` | 50.0 | finite check | Ideal edge length |
| `visualisation.graphs.logseq.physics.auto_balance` | `bool` | false | (no validation) | Enable adaptive parameter tuning |
| `visualisation.graphs.logseq.physics.auto_balance_interval_ms` | `u32` | 500 | (no validation) | Check interval for auto-tune |
| `visualisation.graphs.logseq.physics.auto_balance_config` | `AutoBalanceConfig` | ... | nested | 40+ adaptive tuning thresholds (see below) |
| `visualisation.graphs.logseq.physics.auto_pause` | `AutoPauseConfig` | ... | nested | Equilibrium detection, pause/resume thresholds |
| `visualisation.graphs.logseq.physics.clustering_algorithm` | `String` | "modularity" | (no validation) | "modularity", "louvain", "greedy", etc. |
| `visualisation.graphs.logseq.physics.clustering_resolution` | `f32` | 1.0 | finite check | Algorithm resolution parameter |

**AutoBalanceConfig** (40+ fields, all tunable per-graph):
- `stability_variance_threshold` (f32) – threshold for detecting instability
- `stability_frame_count` (u32) – frames to average for stability
- `clustering_distance_threshold`, `oscillation_detection_frames`, `parameter_adjustment_rate`, etc.
- All nested validated via #[validate(nested)]

**AutoPauseConfig**:
- `enabled` (bool) – turn on/off
- `equilibrium_velocity_threshold` (f32, 0-10) – avg node speed threshold
- `equilibrium_check_frames` (u32, 1-300) – sample window
- `equilibrium_energy_threshold` (f32, 0-1) – total kinetic energy threshold
- `pause_on_equilibrium` (bool) – auto-pause when settled
- `resume_on_interaction` (bool) – auto-resume on user input

### 1.4 System Subtree

**File:** `src/config/system.rs`

| Rust Path | Type | Default | Validation | Notes |
|-----------|------|---------|-----------|-------|
| `system.network.bind_address` | `String` | "0.0.0.0" | (no validation) | Server bind address |
| `system.network.port` | `u16` | 8080 | (no validation) | Server port |
| `system.network.enable_http2` | `bool` | false | (no validation) | HTTP/2 support |
| `system.network.enable_rate_limiting` | `bool` | false | (no validation) | Global rate limit on/off |
| `system.network.max_request_size` | `usize` | 10485760 (10MB) | (no validation) | Max POST payload |
| `system.network.api_client_timeout` | `u64` | 30 | (no validation) | Outbound request timeout (seconds) |
| `system.network.max_concurrent_requests` | `u32` | 1000 | (no validation) | Concurrent request limit |
| `system.websocket.max_connections` | `usize` | 1000 | (no validation) | Max concurrent WS clients |
| `system.websocket.max_message_size` | `usize` | 65536 | (no validation) | Max WS frame size |
| `system.websocket.heartbeat_interval` | `u64` | 30 | (no validation) | Ping interval (seconds) |
| `system.websocket.compression_enabled` | `bool` | false | (no validation) | WS compression on/off |
| `system.security.enable_tls` | `bool` | false | (no validation) | TLS on/off |
| `system.debug.enable_verbose_logging` | `bool` | false | (no validation) | Verbose logs on/off |

### 1.5 XR Subtree

**File:** `src/config/xr.rs`

| Rust Path | Type | Default | Validation | Notes |
|-----------|------|---------|-----------|-------|
| `xr.enabled` | `Option<bool>` | None | (no validation) | XR support on/off (server-side gate) |
| `xr.hand_tracking_enabled` | `bool` | false | (no validation) | Hand mesh + ray rendering |
| `xr.haptic_enabled` | `bool` | false | (no validation) | Controller vibration feedback |
| `xr.gesture_smoothing` | `f32` | 0.7 | (no validation) | Hand gesture EMA filter |
| `xr.teleport_ray_color` | `String` | "" | (no validation) | Teleport ray hex color |
| `xr.controller_ray_color` | `String` | "" | (no validation) | Controller ray hex color |
| `xr.movement_speed` | `f32` | 2.0 | (no validation) | Locomotion speed |
| `xr.dead_zone` | `f32` | 0.1 | (no validation) | Analog stick dead zone |
| `xr.plane_detection_enabled` | `bool` | false | (no validation) | Spatial mapping |
| `xr.passthrough_enabled` | `bool` | false | (no validation) | Passthrough portal overlay |
| `xr.passthrough_opacity` | `f32` | 0.7 | (no validation) | Portal alpha (0-1) |
| (30+ more fields for hand, plane, passthrough, quality, spatial modes) | ... | ... | ... | Fully documented in file |

### 1.6 Feature Flags

**File:** `src/config/app_settings.rs::FeatureFlags`

| Flag | Type | Default | Validation | Consumed By | Notes |
|------|------|---------|-----------|------------|-------|
| `gpu_clustering` | `bool` | false | (no validation) | Clustering handler | GPU-accelerated clustering algorithm toggle |
| `ontology_validation` | `bool` | false | (no validation) | Ontology handler | Enable/disable schema validation on mutations |
| `gpu_anomaly_detection` | `bool` | false | (no validation) | Anomaly handler | GPU-accelerated outlier detection |
| `real_time_insights` | `bool` | false | (no validation) | Analytics handler | Real-time metric aggregation |
| `advanced_visualizations` | `bool` | false | (no validation) | Rendering | High-fidelity rendering mode |
| `performance_monitoring` | `bool` | false | (no validation) | Metrics handler | Detailed performance profiling |
| `stress_majorization` | `bool` | false | (no validation) | Physics handler | Stress-minimization layout algorithm |
| `semantic_constraints` | `bool` | false | (no validation) | Constraint handler | Semantic relationship constraints |
| `sssp_integration` | `bool` | false | (no validation) | Physics handler | Single-source shortest-path distance metrics |

### 1.7 Developer Config

**File:** `src/config/app_settings.rs::DeveloperConfig`

| Flag | Type | Default | Validation | Consumed By | Notes |
|------|------|---------|-----------|------------|-------|
| `debug_mode` | `bool` | false | (no validation) | All handlers | Verbose logging, detailed error messages |
| `show_performance_stats` | `bool` | false | (no validation) | Metrics handler | Show detailed perf metrics in responses |
| `enable_profiling` | `bool` | false | (no validation) | Profiler | CPU/memory profiling on/off |
| `verbose_logging` | `bool` | false | (no validation) | Logger | All logs at DEBUG level |
| `dev_tools_enabled` | `bool` | false | (no validation) | Dev handlers | Enable internal dev API endpoints |

---

## 2. HTTP Settings Routes

All routes under `/api/settings` are configured in `src/settings/api/settings_routes.rs` and registered in `src/main.rs:853`.

**Auth requirement:** Most routes require `AuthenticatedUser` (NIP-98 Nostr auth or Bearer token + X-Nostr-Pubkey header).
**Rate limit:** 60 req/min per IP (applied at scope level in main.rs:854).

### 2.1 Physics Settings Routes

| Method | Path | Handler (file:line) | Auth | Payload | Response | Notes |
|--------|------|-------------------|------|---------|----------|-------|
| GET | `/api/settings/physics` | `settings_routes.rs:224` | OptionalAuth | None | PhysicsSettings JSON | Reads from Neo4j or in-memory actor |
| PUT | `/api/settings/physics` | `settings_routes.rs:257` | AuthenticatedUser | Partial physics JSON | PhysicsSettings JSON (merged) | Validates ranges before apply; broadcasts to GPU/GraphService; persists to Neo4j |
| POST | `/api/settings/physics/compute-mode` | `settings_handler/routes.rs:40` | (inferred) | ComputeMode JSON | 200 OK | Route registered but not fully exposed in main schema |

**Physics validation** (src/settings/api/settings_routes.rs:102-159):
- `gravity`: -10 to 10 (finite check)
- `damping`: 0 to 1 (finite check)
- `spring_k`, `repel_k`, `max_velocity`, `max_force`, `dt`: range + finite checks
- All other f32 fields: NaN/Infinity rejection
- Key normalization: maps snake_case aliases (e.g., `spring_k`, `repulsion_strength`) to canonical camelCase (`springK`)

### 2.2 Constraint Settings Routes

| Method | Path | Handler (file:line) | Auth | Payload | Response | Notes |
|--------|------|-------------------|------|---------|----------|-------|
| GET | `/api/settings/constraints` | `settings_routes.rs:405` | OptionalAuth | None | ConstraintSettings JSON | Reads from Neo4j; returns defaults if missing |
| PUT | `/api/settings/constraints` | `settings_routes.rs:431` | AuthenticatedUser | ConstraintSettings JSON | ConstraintSettings JSON | Validates threshold ordering (near < medium < far); persists to Neo4j; broadcasts to GPU |

**Constraint validation** (src/settings/api/settings_routes.rs:166-179):
- `far_threshold`, `medium_threshold`, `near_threshold`: finite, non-negative
- Order constraint: `near_threshold < medium_threshold < far_threshold`

### 2.3 Rendering Settings Routes

| Method | Path | Handler (file:line) | Auth | Payload | Response | Notes |
|--------|------|-------------------|------|---------|----------|-------|
| GET | `/api/settings/rendering` | `settings_routes.rs:503` | OptionalAuth | None | RenderingSettings JSON | Reads from in-memory actor |
| PUT | `/api/settings/rendering` | `settings_routes.rs:526` | AuthenticatedUser | RenderingSettings JSON | RenderingSettings JSON | Validates light intensity (finite, non-negative); broadcasts "settingsUpdated" to clients |

**Rendering validation** (src/settings/api/settings_routes.rs:187-199):
- `ambient_light_intensity`: finite, >= 0
- `directional_light_intensity`: finite, >= 0
- `environment_intensity`: finite, >= 0

### 2.4 Node Filter Settings Routes

| Method | Path | Handler (file:line) | Auth | Payload | Response | Notes |
|--------|------|-------------------|------|---------|----------|-------|
| GET | `/api/settings/node-filter` | `settings_routes.rs:601` | OptionalAuth | None | NodeFilterSettings JSON | Reads from Neo4j; returns defaults if missing |
| PUT | `/api/settings/node-filter` | `settings_routes.rs:627` | AuthenticatedUser | NodeFilterSettings JSON | NodeFilterSettings JSON | Validates quality/authority thresholds (0-1); validates filter_mode ("and"/"or"); broadcasts to clients |

**Node filter validation** (src/settings/api/settings_routes.rs:206-217):
- `quality_threshold`, `authority_threshold`: 0.0-1.0 inclusive
- `filter_mode`: "and" or "or" only

### 2.5 Quality Gates Settings Routes

| Method | Path | Handler (file:line) | Auth | Payload | Response | Notes |
|--------|------|-------------------|------|---------|----------|-------|
| GET | `/api/settings/quality-gates` | `settings_routes.rs:701` | OptionalAuth | None | QualityGateSettings JSON | Reads from Neo4j; returns defaults if missing |
| PUT | `/api/settings/quality-gates` | `settings_routes.rs:727` | AuthenticatedUser | Partial JSON (deep-merge) | QualityGateSettings JSON | Merges with persisted settings; propagates GPU/semantic forces config; broadcasts via BroadcastMessage |

**Quality gates** (approx 15 fields):
- `gpu_acceleration` (bool) → `compute_mode` (0=CPU, 2=GPU)
- `layout_mode` ("force-directed", "dag-topdown", "dag-radial", "type-clustering") → applies physics overrides
- `semantic_forces` (bool), `ontology_physics` (bool) → ConfigureDAG/ConfigureTypeClustering messages to SemanticForcesActor
- `dag_level_attraction`, `dag_sibling_repulsion`, `type_cluster_attraction`, `type_cluster_radius`, `ontology_strength` (f32)

### 2.6 Visual Settings Routes (Client-Only)

| Method | Path | Handler (file:line) | Auth | Payload | Response | Notes |
|--------|------|-------------------|------|---------|----------|-------|
| GET | `/api/settings/visual` | `settings_routes.rs:929` | OptionalAuth | None | Opaque JSON blob | Reads from Neo4j; returns `{}` if missing |
| PUT | `/api/settings/visual` | `settings_routes.rs:947` | AuthenticatedUser | Partial JSON | Merged JSON | Deep-merges patch into current; persists merged result to Neo4j |

**Visual settings** (client-only; no server-side validation):
- Opaque object structure: `{glow: {...}, bloom: {...}, nodes: {...}, edges: {...}, labels: {...}, ...}`
- Used for client-side visual customization (colors, sizes, animations not affecting physics)

### 2.7 All Settings Route

| Method | Path | Handler (file:line) | Auth | Payload | Response | Notes |
|--------|------|-------------------|------|---------|----------|-------|
| GET | `/api/settings/all` | `settings_routes.rs:993` | OptionalAuth | None | AllSettings JSON | Composites: physics, constraints, rendering, node_filter, quality_gates, visual |

**AllSettings struct** (src/settings/models.rs):
```rust
pub struct AllSettings {
    pub physics: PhysicsSettings,
    pub constraints: ConstraintSettings,
    pub rendering: RenderingSettings,
    pub node_filter: NodeFilterSettings,
    pub quality_gates: QualityGateSettings,
    pub visual: serde_json::Value,
}
```

### 2.8 User Filter Routes

| Method | Path | Handler (file:line) | Auth | Payload | Response | Notes |
|--------|------|-------------------|------|---------|----------|-------|
| GET | `/api/user/filter` | `settings_routes.rs:1093` | AuthenticatedUser | None | UserFilter JSON | Per-user node/edge visibility filter (pubkey-scoped) |
| PUT | `/api/user/filter` | `settings_routes.rs:1118` | AuthenticatedUser | UserFilter JSON | UserFilter JSON | Persists to Neo4j with user pubkey; scoped to authenticated user |

### 2.9 Profile Management Routes

| Method | Path | Handler (file:line) | Auth | Payload | Response | Notes |
|--------|------|-------------------|------|---------|----------|-------|
| POST | `/api/settings/profiles` | `settings_routes.rs:1147` | AuthenticatedUser | SaveProfileRequest | ProfileIdResponse {id: 1} | STUB: Returns dummy ID; actual persistence not yet implemented |
| GET | `/api/settings/profiles` | `settings_routes.rs:1170` | OptionalAuth | None | Vec<SettingsProfile> | STUB: Returns empty vec[] |
| GET | `/api/settings/profiles/{id}` | `settings_routes.rs:1157` | OptionalAuth | None | 404 error or ProfileJSON | STUB: Always returns 404 |
| DELETE | `/api/settings/profiles/{id}` | `settings_routes.rs:1182` | AuthenticatedUser | None | 200 OK | STUB: No-op |

### 2.10 Legacy Unified Routes (via settings_handler)

**File:** `src/handlers/settings_handler/routes.rs`

Routes registered at `/api/settings` scope (in addition to/overlapping with above):

| Method | Path | Handler | Auth | Payload | Response | Notes |
|--------|------|---------|------|---------|----------|-------|
| GET | `/api/settings/path?path=X.Y.Z` | `routes.rs:58` | OptionalAuth | Query param `path` | JSON {success, path, value} | Read arbitrary nested field by dot-path (e.g., `visualisation.physics.damping`) |
| PUT | `/api/settings/path` | `routes.rs:99` | AuthenticatedUser | {path, value} | JSON {success, path, value, previous} | Update arbitrary nested field; triggers physics propagation if path contains `.physics.` |
| GET | `/api/settings/schema?path=X.Y` | `routes.rs` | OptionalAuth | Query param `path` | JSON schema | Introspection endpoint: returns JSON schema for nested struct |
| GET | `/api/settings/current` | `routes.rs` | OptionalAuth | None | Full AppFullSettings JSON | Current in-memory settings |
| GET | `/api/settings` | `routes.rs:26` | OptionalAuth | None | Full AppFullSettings JSON | Alias for `/current` |
| POST | `/api/settings` | `write_handlers.rs:20` | AuthenticatedUser | Partial JSON | Full AppFullSettings JSON | Merge-update via `AppFullSettings::merge_update()` |
| POST | `/api/settings/reset` | `write_handlers.rs:240` | AuthenticatedUser | None | Full AppFullSettings JSON | Reset all settings to schema defaults |
| POST | `/api/settings/save` | `write_handlers.rs:281` | AuthenticatedUser | None | 200 OK | Persist current in-memory settings to Neo4j (explicit save) |

**Route registration:** `src/handlers/settings_handler/routes.rs:16-56` configures above via `web::ServiceConfig`

**Physics propagation on PUT /path:**
- If path contains `.physics.`, `.graphs.logseq.`, or `.graphs.visionflow.`:
  - Extract physics update, call `propagate_physics_to_gpu()`
  - Send `UpdateSimulationParams` to GPUComputeActor
  - Send `ForceResumePhysics` to GraphServiceSupervisor

---

## 3. WebSocket Messages Carrying Settings

**File:** `src/settings/api/settings_routes.rs` (lines where BroadcastMessage is sent)

| Topic | Direction | Handler (file:line) | Payload Schema | Notes |
|-------|-----------|-------------------|---------------|----|
| `settingsUpdated` | Server → All clients | settings_routes.rs:550-562 | `{type: "settingsUpdated", category: "rendering", updatedBy: pubkey, timestamp: millis}` | Broadcast after rendering settings change; clients re-apply light/shadow config |
| `settingsUpdated` | Server → All clients | settings_routes.rs:671-688 | `{type: "settingsUpdated", category: "nodeFilter", settings: {...}, updatedBy: pubkey, timestamp: millis}` | Broadcast after node filter change; clients recompute which nodes pass filter, re-render |
| (Physics broadcasts via GPU actors) | Async | UpdateSimulationParams message | SimulationParams struct | Not a WS-level message; handled via Actix actor messaging to GPU compute layer |

**WS broadcast mechanism:**
- `BroadcastMessage { message: String }` is sent to `state.client_manager_addr` (generic broadcast actor)
- All connected WS clients receive the JSON string
- Clients parse `type` field to route to appropriate handler

---

## 4. Feature Flags & Dev Toggles

### 4.1 Environment-Based Feature Access (feature_access.rs)

**File:** `src/config/feature_access.rs`

**Data structure:** `FeatureAccess` (loaded from env vars at startup)

| Flag | Env Var | Type | Default | Gate Behavior | Notes |
|------|---------|------|---------|---------------|-------|
| `approved_pubkeys` | `APPROVED_PUBKEYS` | Vec<String> | empty | Basic user registration gate | CSV list of Nostr pubkeys allowed access; `register_new_user()` adds new key |
| `perplexity_enabled` | `PERPLEXITY_ENABLED_PUBKEYS` | Vec<String> | empty | Perplexity API access gate | User must be in this list to call Perplexity endpoints |
| `openai_enabled` | `OPENAI_ENABLED_PUBKEYS` | Vec<String> | empty | OpenAI API access gate | User must be in this list to call OpenAI endpoints |
| `ragflow_enabled` | `RAGFLOW_ENABLED_PUBKEYS` | Vec<String> | empty | RAG Flow access gate | User must be in this list to use RAG Flow |
| `power_users` | `POWER_USER_PUBKEYS` | Vec<String> | empty | Power-user privilege gate | Power users can sync settings and access advanced features |
| `settings_sync_enabled` | `SETTINGS_SYNC_ENABLED_PUBKEYS` | Vec<String> | empty | Settings sync gate | User must be in this list OR be power_user to call batch_update_settings |

**Access methods:**
- `has_access(pubkey)` – check if user is approved
- `is_power_user(pubkey)` – check if power user
- `can_sync_settings(pubkey)` – power_user OR in SETTINGS_SYNC_ENABLED_PUBKEYS
- `has_feature_access(pubkey, feature)` – gate specific feature (perplexity, openai, ragflow, settings_sync, power_user)

**Loaded at:** `src/main.rs:` via `FeatureAccess::from_env()`, passed to AppState as `web::Data<FeatureAccess>`

### 4.2 Settings Auth Bypass (HARDCODED SECURITY GATE)

**File:** `src/settings/auth_extractor.rs:44-61` & `src/main.rs`

| Flag | Env Var | Value | Behavior | Production | Notes |
|------|---------|-------|----------|-----------|-------|
| `SETTINGS_AUTH_BYPASS` | `SETTINGS_AUTH_BYPASS` | "true" | Bypass NIP-98 auth, inject dev-user with power_user=true | **FORBIDDEN** (main.rs line ~200 checks and rejects if production detected) | **SECURITY CRITICAL:** Dev-only; causes ERR if APP_ENV/RUST_ENV=production |
| Docker env gate | `DOCKER_ENV=1 && NODE_ENV=development` | boolean | Alt bypass trigger for dev containers | Ignored if APP_ENV/RUST_ENV=production | Fallback to SETTINGS_AUTH_BYPASS check |

**Guard logic** (auth_extractor.rs:44-61):
```rust
let bypass_enabled = std::env::var("SETTINGS_AUTH_BYPASS").unwrap_or_default() == "true"
    || (std::env::var("DOCKER_ENV").is_ok()
        && std::env::var("NODE_ENV").unwrap_or_default() == "development");
if bypass_enabled {
    let is_production = std::env::var("APP_ENV").map(|v| v == "production").unwrap_or(false)
        || std::env::var("RUST_ENV").map(|v| v == "production").unwrap_or(false);
    if is_production {
        warn!("SETTINGS_AUTH_BYPASS is set but ignored in production mode");
        // fall through to normal auth
    } else {
        // Create dev-user with power_user=true
        return Box::pin(async {
            Ok(AuthenticatedUser {
                pubkey: "dev-user".to_string(),
                is_power_user: true,
            })
        });
    }
}
```

### 4.3 Developer Configuration (dev_config.rs)

**File:** `src/config/dev_config.rs`

**Loading:** Loads from `data/dev_config.toml` at startup; falls back to hardcoded defaults.

**Scope:** All fields are BACK-END ONLY; not exposed via HTTP API.

| Section | Field | Type | Default | Tunable | Effect | Should Expose? |
|---------|-------|------|---------|---------|--------|-----------------|
| **physics** | `force_epsilon` | f32 | 1e-8 | Yes | Force vector epsilon for stability | No (internal numerical constant) |
| **physics** | `spring_length_multiplier` | f32 | 5.0 | Yes | Spring rest length = distance * multiplier | No |
| **physics** | `max_force` | f32 | 50.0 (CANONICAL) | Yes | Per-node force cap (synced with config/mod.rs::CANONICAL_MAX_FORCE) | Maybe (mirrors canonical) |
| **physics** | `max_velocity` | f32 | 200.0 (CANONICAL) | Yes | Per-node velocity cap (synced with config/mod.rs::CANONICAL_MAX_VELOCITY) | Maybe (mirrors canonical) |
| **physics** | `repulsion_cutoff` | f32 | 50.0 | Yes | Distance beyond which repulsion is 0 | No |
| **physics** | `center_gravity_k` | f32 | 0.005 | Yes | Center-of-mass gravity strength | No (internal tuning) |
| **physics** | `grid_cell_size` | f32 | 50.0 | Yes | Spatial hash cell size | No (performance tuning) |
| **physics** | `warmup_iterations` | u32 | 100 | Yes | Initial iterations with high damping | No (startup heuristic) |
| **physics** | `anomaly_detection_radius` | f32 | 150.0 | Yes | Neighborhood radius for outlier detection | No |
| **physics** | `cross_graph_repulsion_scale` | f32 | 0.3 | Yes | Inter-graph node repulsion magnitude | No |
| **physics** | `stress_majorization_epsilon` | f32 | 0.001 | Yes | Convergence threshold for stress minimization | No |
| **cuda** | `warmup_iterations_default` | u32 | 200 | Yes | GPU warmup iterations | No (GPU tuning) |
| **cuda** | `max_kernel_time_ms` | u32 | 5000 | Yes | Max GPU kernel runtime before timeout | No (GPU resource limit) |
| **cuda** | `max_nodes` | u32 | 1_000_000 | Yes | GPU memory capacity (nodes) | No (hardware limit) |
| **cuda** | `max_edges` | u32 | 10_000_000 | Yes | GPU memory capacity (edges) | No (hardware limit) |
| **network** | `ws_ping_interval_secs` | u64 | 30 | Yes | WebSocket heartbeat interval | **Maybe** (user-visible latency) |
| **network** | `ws_pong_timeout_secs` | u64 | 10 | Yes | WS timeout before close | **Maybe** (user-visible stability) |
| **network** | `max_retry_attempts` | u32 | 3 | Yes | Outbound API retry count | No |
| **network** | `retry_base_delay_ms` | u64 | 100 | Yes | Exponential backoff base | No |
| **network** | `rate_limit_burst_size` | u32 | 10 | Yes | Token bucket burst capacity | No (internal queueing) |
| **rendering** | `agent_colors` | Map<String, String> | (10 agent types) | Yes | Agent visualization colors (#RRGGBB) | **Maybe** (visual preference) |
| **rendering** | `agent_base_size` | f32 | 1.0 | Yes | Base agent glyph size | No |
| **rendering** | `lod_distance_high` | f32 | 100.0 | Yes | Level-of-detail high distance threshold | No (perf tuning) |
| **performance** | `batch_size_nodes` | usize | 1000 | Yes | Node batch size for batch updates | No (internal batching) |
| **performance** | `cache_ttl_secs` | u64 | 300 | Yes | Cache entry time-to-live | No (internal caching) |
| **performance** | `worker_threads` | usize | 4 | Yes | Tokio worker thread count | No (process startup config) |
| **debug** | `enable_cuda_debug` | bool | false | Yes | GPU kernel debug output | No (dev-only debugging) |
| **debug** | `enable_physics_debug` | bool | false | Yes | Physics engine debug logs | No (dev-only debugging) |
| **debug** | `enable_network_debug` | bool | false | Yes | Network layer debug logs | No (dev-only debugging) |
| **debug** | `log_slow_operations_ms` | u64 | 100 | Yes | Log threshold for slow ops | No (perf monitoring) |

**Accessed via:** `dev_config::physics()`, `dev_config::cuda()`, `dev_config::rendering()`, etc. (convenience functions at end of file)

**Persistence:** Changes made to DevConfig in memory are NOT auto-saved. Must call `DevConfig::save_to_file(path)` explicitly. No HTTP API to modify dev_config at runtime.

---

## 5. Back-End-Only Tunables (Unexposed)

### 5.1 Canonical Constants

**File:** `src/config/mod.rs:17-24`

| Constant | Value | Usage | Should Expose? |
|----------|-------|-------|-----------------|
| `CANONICAL_MAX_VELOCITY` | 200.0 | Default max_velocity across physics; also in dev_config | **Consider:** affects responsiveness; maybe expose via quality gates |
| `CANONICAL_MAX_FORCE` | 50.0 | Default max_force across physics; also in dev_config | **Consider:** affects acceleration feel; maybe expose via quality gates |

### 5.2 Inline Physics Constants (Hardcoded)

**File:** `src/config/physics.rs:7-29` (default function definitions)

| Constant | Value | Validation | Notes |
|----------|-------|-----------|-------|
| `default_auto_balance_interval()` | 500ms | (no validation) | Check auto-balance every 500ms if enabled |
| `default_lin_log_mode()` | true | (no validation) | Use log-space distance calculations |
| `default_scaling_ratio()` | 10.0 | (no validation) | Distance scaling for visualization |
| `default_physicality_strength()` | 0.40 | (no validation) | Weighting for physical constraints |
| `default_role_strength()` | 0.30 | (no validation) | Weighting for role-based constraints |
| `default_maturity_strength()` | 0.15 | (no validation) | Weighting for maturity-based constraints |
| `default_constraint_ramp_frames()` | 60 | (no validation) | Frames to ramp up constraint forces |
| `default_constraint_max_force_per_node()` | 50.0 | (no validation) | Max per-node constraint force |
| `default_bounds_size()` | 1000.0 | (no validation) | World boundary size |

**Access:** Compile-time defaults; no runtime tuning without rebuild.

### 5.3 Validation Regex Patterns

**File:** `src/config/validation.rs:8-20`

| Pattern | Regex | Usage | Notes |
|---------|-------|-------|-------|
| `HEX_COLOR_REGEX` | `^#([A-Fa-f0-9]{6}\|[A-Fa-f0-9]{8})$` | Validates color fields (e.g., glow.base_color) | 6-digit (#RRGGBB) or 8-digit (#RRGGBBAA) hex |
| `URL_REGEX` | `^https?://[^\s/$.?#].[^\s]*$` | Validates URL fields (service endpoints) | HTTP/HTTPS only |
| `DOMAIN_REGEX` | `^[a-zA-Z0-9]([a-zA-Z0-9\-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]...` | Validates domain names | RFC-compliant domains |

**Cross-field validation:** `AppFullSettings::validate_cross_field_constraints()` checks:
- Gravity != 0 → physics must be enabled
- Bloom/glow consistency (intensity, radius, threshold ranges)

---

## 6. Persistence & Sync

### 6.1 Dual Persistence Architecture

| Source | Read Priority | Scope | Persistence Method | Sync |
|--------|---------------|-------|-------------------|------|
| **In-memory Actor** (OptimizedSettingsActor) | 2nd | Global settings | Actix message-passing | Auto: on PUT /settings endpoints |
| **Neo4j Database** | 1st (when available) | Global + per-user | `Neo4jSettingsRepository` | Manual: POST /api/settings/save or auto on certain PUT routes |
| **YAML file** (legacy, now disabled) | N/A (removed) | N/A | ~~Disabled~~ | Settings are now database-first |

**Load order (GET /api/settings/physics):**
1. Try Neo4j: `neo4j_repo.get_setting("physics")`
2. Fall back to actor: `state.settings_addr.send(GetSettings)`
3. Return merged or actor-only result

**Save order (PUT /api/settings/physics):**
1. Update in-memory actor via `UpdateSettings` message
2. Persist to Neo4j via `neo4j_repo.set_setting()`
3. Broadcast to GPU actors (UpdateSimulationParams)
4. Broadcast to clients (BroadcastMessage)

### 6.2 Per-User Settings (Scoped)

**Routes:** `GET/PUT /api/user/filter`, `GET/PUT /api/settings/all` (if authenticated)

| Setting | Scope | Persistence | Notes |
|---------|-------|-------------|-------|
| `user_filter` (NodeFilterSettings) | Per-pubkey | Neo4j (pubkey-keyed) | User-specific node/edge visibility filter |
| Full settings snapshot | Per-pubkey | Neo4j | GET /api/settings/all returns user-specific copy if available, else global |

---

## 7. Disconnects & Issues

### 7.1 Schema Drift

- **Generated client types:** `client/src/types/generated/settings.ts` (auto-generated from Rust schema via Specta)
  - **Status:** Out-of-date or not yet regenerated; verify with `cargo insta review` or `specta` codegen
  - **Risk:** Client sends snake_case; server expects camelCase; field_mappings.rs + serde aliases mitigate

### 7.2 Validation Coverage Gaps

| Field | Validation | Issue | Mitigation |
|-------|-----------|-------|-----------|
| `FeatureFlags` (all 9 fields) | None | No ranges, no mutual constraints | Defaults are safe (all false); can set freely |
| `DeveloperConfig` (all 5 fields) | None | No ranges, no mutual constraints | Defaults are safe (all false); only accessible in dev mode |
| XR settings (30+ fields) | Minimal | Color fields not validated; numeric ranges unchecked | No API exposure (read-only); set via file/default |
| AutoBalanceConfig (40+ fields) | nested | No range validation on individual tuning params | Tuning via dev_config.toml only (no API) |

### 7.3 Missing field_mappings Entries

**File:** `src/config/field_mappings.rs` (FIELD_MAPPINGS HashMap)

- Maps snake_case aliases to camelCase for JSON merge operations
- **Gaps:** Not all fields have entries; some new fields (e.g., quality gates, semantic forces) may lack aliases
- **Impact:** Snake_case input for unmapped fields will not merge correctly; camelCase input works

### 7.4 Commented-Out TODOs

**File:** `src/handlers/settings_handler/write_handlers.rs` and others

- TODO: Full validation on nested physics updates
- TODO: Transaction semantics (atomic update across multiple nested structs)
- TODO: Profile save/load (stubs return dummy data)
- TODO: Rollback on partial apply failures

### 7.5 Route Duplication / Confusion

- **Issue:** `/api/settings` path is mounted at TWO scopes:
  1. `src/main.rs:853` – mounts `webxr::settings::api::configure_routes`
  2. Within that scope: `src/handlers/settings_handler/routes.rs:21` mounts a second `/settings` scope
  
- **Result:** Routes like GET /api/settings/path may conflict or be duplicated
- **Mitigation:** One scope should be the source of truth; audit route conflicts

### 7.6 Auth Extractor Fallthrough

- **Issue:** If NIP-98 auth fails AND Bearer token auth fails, response is 401 Unauthorized
- **Gap:** No fallback to guest auth or rate-limited anonymous access
- **Impact:** All settings mutations require authentication; read-only routes allow OptionalAuth

### 7.7 No Audit Trail

- Logging occurs (info!, warn! macros) but no persistent audit log
- Changes are not attributed to users in the database (except via `updatedBy` in broadcast JSON)
- **Recommendation:** Add audit_log table in Neo4j to track all settings changes

---

## 8. Summary Statistics

| Metric | Count | Notes |
|--------|-------|-------|
| **Root fields in AppFullSettings** | 14 | visualisation, system, xr, auth, ragflow, perplexity, openai, kokoro, whisper, voice_routing, ontology_agent, version, user_preferences, physics, feature_flags, developer_config |
| **Nested config structs** | 40+ | PhysicsSettings, VisualisationSettings, SystemSettings, XRSettings, GraphSettings (x2), NodeSettings, EdgeSettings, RenderingSettings, AnimationSettings, LabelSettings, CameraSettings, SpacePilotSettings, NetworkSettings, WebSocketSettings, SecuritySettings, DebugSettings, AuthSettings, RagFlowSettings, ... |
| **HTTP GET routes** | 10 | /physics, /constraints, /rendering, /node-filter, /quality-gates, /visual, /all, /path?path=X, /schema?path=X, /user/filter, /current, / |
| **HTTP PUT routes** | 10 | /physics, /constraints, /rendering, /node-filter, /quality-gates, /visual, /path (PUT), /user/filter, and legacy POST |
| **HTTP POST routes** | 4 | / (update), /reset, /save, /profiles |
| **WS broadcast topics** | 2 | settingsUpdated (rendering), settingsUpdated (nodeFilter) |
| **Feature flags** | 9 | gpu_clustering, ontology_validation, gpu_anomaly_detection, real_time_insights, advanced_visualizations, performance_monitoring, stress_majorization, semantic_constraints, sssp_integration |
| **Dev toggles (feature_access.rs)** | 6 | APPROVED_PUBKEYS, PERPLEXITY_ENABLED_PUBKEYS, OPENAI_ENABLED_PUBKEYS, RAGFLOW_ENABLED_PUBKEYS, POWER_USER_PUBKEYS, SETTINGS_SYNC_ENABLED_PUBKEYS |
| **Dev config sections** | 6 | physics, cuda, network, rendering, performance, debug |
| **Back-end-only tunables** | 50+ | dev_config.rs fields (not exposed via HTTP) |
| **Validation rules** | 100+ | Ranges, hex colors, width_range order, cross-field constraints, finite checks, threshold ordering, etc. |

---

## 9. Key Security Notes

1. **SETTINGS_AUTH_BYPASS=true** is explicitly rejected at startup if production env vars are detected (src/main.rs ~200).
2. **NIP-98 Schnorr auth** is primary; Bearer token + X-Nostr-Pubkey header is fallback.
3. **Power user privilege** gates `can_sync_settings()` and advanced features.
4. **Rate limit:** 60 req/min on entire /api/settings scope.
5. **Neo4j persistence** is the source of truth; in-memory actor is a cache.
6. **No audit trail** in persistent storage (logging only; consider adding).
7. **Cross-field validation** ensures physics/graph consistency (e.g., gravity != 0 → physics enabled).

---

## 10. References

- **Config source of truth:** `src/config/app_settings.rs` (AppFullSettings)
- **HTTP routes:** `src/settings/api/settings_routes.rs` + `src/handlers/settings_handler/routes.rs`
- **Auth extractor:** `src/settings/auth_extractor.rs` (NIP-98 + Bearer token)
- **Dev config:** `src/config/dev_config.rs` (back-end only, from data/dev_config.toml)
- **Feature gates:** `src/config/feature_access.rs` (env-var based)
- **Validation:** `src/config/validation.rs` + cross-field checks in app_settings.rs
- **Field mappings:** `src/config/field_mappings.rs` (snake_case ↔ camelCase)
- **Path access:** `src/config/path_access.rs` (dot-path navigation)
- **Persistence:** `src/adapters/neo4j_settings_repository.rs` (Neo4j + fallback)
- **Actor:** `src/settings/settings_actor.rs` (OptimizedSettingsActor)
- **Main route registration:** `src/main.rs:850-856` (settings scope)

